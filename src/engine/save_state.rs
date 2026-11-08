// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SAVE_FORMAT_VERSION: u32 = 2;
pub const DEFAULT_SAVE_SLOT: &str = "default";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum SaveValue {
    Number(f64),
    Bool(bool),
    String(String),
}

#[derive(Debug, Serialize, Deserialize)]
struct SaveDocument {
    format_version: u32,
    slot: String,
    #[serde(default)]
    values: BTreeMap<String, SaveValue>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    defaults: BTreeMap<String, SaveValue>,
}

#[derive(Debug)]
pub struct SaveState {
    path: PathBuf,
    values: BTreeMap<String, SaveValue>,
    defaults: BTreeMap<String, SaveValue>,
    dirty: bool,
    restart_requested: bool,
    loaded_fingerprint: Option<(usize, u64)>,
}

impl SaveState {
    pub fn load(project_path: &Path) -> io::Result<Self> {
        Self::load_from_root(project_path, &save_data_root())
    }

    pub fn load_from_root(project_path: &Path, save_root: &Path) -> io::Result<Self> {
        Self::load_named_from_root(project_path, save_root, DEFAULT_SAVE_SLOT)
    }

    pub fn load_named(project_path: &Path, slot: &str) -> io::Result<Self> {
        Self::load_named_from_root(project_path, &save_data_root(), slot)
    }

    pub fn load_named_from_root(
        project_path: &Path,
        save_root: &Path,
        slot: &str,
    ) -> io::Result<Self> {
        if slot.is_empty()
            || slot.len() > 80
            || !slot
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid save slot name",
            ));
        }
        let path =
            save_path_for_project(project_path, save_root).with_file_name(format!("{slot}.json"));

        if !path.exists() {
            return Ok(Self {
                path,
                values: BTreeMap::new(),
                defaults: BTreeMap::new(),
                dirty: false,
                restart_requested: false,
                loaded_fingerprint: None,
            });
        }

        let bytes = fs::read(&path)?;
        let loaded_fingerprint = Some(bytes_fingerprint(&bytes));
        let document: SaveDocument = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse save file {}: {}", path.display(), error),
            )
        })?;

        if document.format_version > SAVE_FORMAT_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Save format version {} is newer than supported version {}",
                    document.format_version, SAVE_FORMAT_VERSION
                ),
            ));
        }

        if document.slot != slot {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Unsupported save slot '{}'", document.slot),
            ));
        }

        if document
            .values
            .values()
            .any(|value| matches!(value, SaveValue::Number(number) if !number.is_finite()))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Save file contains a non-finite number",
            ));
        }

        Ok(Self {
            path,
            defaults: validated_defaults(&document.values, document.defaults)?,
            values: document.values,
            dirty: false,
            restart_requested: false,
            loaded_fingerprint,
        })
    }

    pub fn request_restart(&mut self) {
        self.restart_requested = true;
    }
    pub fn take_restart_request(&mut self) -> bool {
        std::mem::take(&mut self.restart_requested)
    }
    pub fn reset_values(&mut self, key: Option<&str>) -> Result<usize, String> {
        if let Some(key) = key {
            validate_key(key)?;
        }
        let mut changed = 0;
        if let Some(key) = key {
            if let (Some(current), Some(default)) =
                (self.values.get_mut(key), self.defaults.get(key))
            {
                if current != default {
                    *current = default.clone();
                    changed += 1;
                }
            }
        } else {
            for (key, current) in &mut self.values {
                if let Some(default) = self.defaults.get(key) {
                    if current != default {
                        *current = default.clone();
                        changed += 1;
                    }
                }
            }
        }
        self.dirty |= changed > 0;
        Ok(changed)
    }

    pub fn wipe(&mut self) -> io::Result<()> {
        self.reset_values(None).map_err(io::Error::other)?;
        if !self.path.exists() {
            return Ok(());
        }
        if self.dirty {
            self.commit()?;
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn values(&self) -> &BTreeMap<String, SaveValue> {
        &self.values
    }

    pub fn defaults(&self) -> &BTreeMap<String, SaveValue> {
        &self.defaults
    }
    pub fn get_default(&self, key: &str) -> Option<&SaveValue> {
        self.defaults.get(key)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn get(&self, key: &str) -> Option<&SaveValue> {
        self.values.get(key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn set(&mut self, key: &str, value: SaveValue) -> Result<(), String> {
        self.set_with_default(key, value, None)
    }

    pub fn set_with_default(
        &mut self,
        key: &str,
        value: SaveValue,
        default: Option<SaveValue>,
    ) -> Result<(), String> {
        validate_key(key)?;
        validate_value(&value)?;
        if let Some(default) = &default {
            validate_value(default)?;
            if !same_type(default, &value) {
                return Err("save.set default type must match current value type".into());
            }
        }
        let next_default = match default {
            Some(provided) => provided,
            None => self
                .defaults
                .get(key)
                .filter(|old| same_type(old, &value))
                .cloned()
                .unwrap_or_else(|| empty_default(&value)),
        };
        if self.values.get(key) != Some(&value) {
            self.values.insert(key.to_owned(), value);
            self.dirty = true;
        }
        if self.defaults.get(key) != Some(&next_default) {
            self.defaults.insert(key.to_owned(), next_default);
            self.dirty = true;
        }
        Ok(())
    }

    pub fn set_default(&mut self, key: &str, default: SaveValue) -> Result<(), String> {
        validate_key(key)?;
        validate_value(&default)?;
        let current = self.values.get(key).ok_or("Save field does not exist")?;
        if !same_type(current, &default) {
            return Err("Save default type must match field type".into());
        }
        if self.defaults.get(key) != Some(&default) {
            self.defaults.insert(key.to_owned(), default);
            self.dirty = true;
        }
        Ok(())
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        validate_key(to)?;
        let Some(value) = self.values.get(from).cloned() else {
            return Err(format!("Save field '{from}' does not exist"));
        };
        if from == to {
            return Ok(());
        }
        if self.values.contains_key(to) {
            return Err(format!("Save field '{to}' already exists"));
        }
        self.values.remove(from);
        self.values.insert(to.to_owned(), value.clone());
        let default = self
            .defaults
            .remove(from)
            .unwrap_or_else(|| empty_default(&value));
        self.defaults.insert(to.to_owned(), default);
        self.dirty = true;
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> Result<bool, String> {
        validate_key(key)?;
        let removed = self.values.remove(key).is_some();
        self.defaults.remove(key);
        self.dirty |= removed;
        Ok(removed)
    }

    pub fn commit(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.verify_unchanged_on_disk()?;

        let document = SaveDocument {
            format_version: SAVE_FORMAT_VERSION,
            slot: DEFAULT_SAVE_SLOT.to_string(),
            values: self.values.clone(),
            defaults: self.defaults.clone(),
        };
        let mut bytes = serde_json::to_vec_pretty(&document).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to serialize save data: {}", error),
            )
        })?;
        bytes.push(b'\n');

        let parent = self.path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Save path has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;

        let temp_path = temporary_path(&self.path);
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            self.verify_unchanged_on_disk()?;
            atomic_replace(&temp_path, &self.path)?;
            sync_parent_directory(parent)?;
            Ok::<(), io::Error>(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        } else {
            self.dirty = false;
            self.loaded_fingerprint = Some(bytes_fingerprint(&bytes));
        }

        write_result
    }

    fn verify_unchanged_on_disk(&self) -> io::Result<()> {
        if disk_fingerprint(&self.path)? != self.loaded_fingerprint {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "Save slot changed outside this session: {}. Reopen the slot before saving.",
                    self.path.display()
                ),
            ));
        }
        Ok(())
    }
}

fn bytes_fingerprint(bytes: &[u8]) -> (usize, u64) {
    (bytes.len(), fnv1a64(bytes))
}

fn disk_fingerprint(path: &Path) -> io::Result<Option<(usize, u64)>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes_fingerprint(&bytes))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn inspect_saved_slot(path: &Path) -> io::Result<BTreeMap<String, SaveValue>> {
    let data = fs::read(path)?;
    let document: SaveDocument = serde_json::from_slice(&data).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to parse Save State {}: {error}", path.display()),
        )
    })?;
    if document.format_version > SAVE_FORMAT_VERSION
        || document.slot != DEFAULT_SAVE_SLOT
        || document
            .values
            .values()
            .any(|value| matches!(value, SaveValue::Number(number) if !number.is_finite()))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported or invalid Save State",
        ));
    }
    Ok(document.values)
}

#[derive(Clone, Debug)]
pub struct PreviousSaveSlot {
    pub path: PathBuf,
    pub values: BTreeMap<String, SaveValue>,
}

pub fn previous_saves_for_project(project_path: &Path) -> io::Result<Vec<PreviousSaveSlot>> {
    previous_saves_in_root(project_path, &save_data_root())
}

fn previous_saves_in_root(project_path: &Path, root: &Path) -> io::Result<Vec<PreviousSaveSlot>> {
    let current = save_path_for_project(project_path, root);
    let name = project_path
        .file_name()
        .and_then(OsStr::to_str)
        .map(sanitize_project_name)
        .unwrap_or_else(|| "project".into());
    let prefix = format!("{name}-").to_ascii_lowercase();
    let mut found = Vec::new();
    if !root.exists() {
        return Ok(found);
    }
    for entry in fs::read_dir(root)?.take(1024) {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Some(suffix) = file_name.strip_prefix(&prefix) else {
            continue;
        };
        if suffix.len() != 16 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let path = entry.path().join(format!("{DEFAULT_SAVE_SLOT}.json"));
        if path == current || !path.is_file() {
            continue;
        }
        if path.metadata()?.len() > 8 * 1024 * 1024 {
            continue;
        }
        let Ok(values) = inspect_saved_slot(&path) else {
            continue;
        };
        found.push(PreviousSaveSlot { path, values });
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

pub fn save_data_root() -> PathBuf {
    if let Some(root) = std::env::var_os("WORLDKILN_RUNTIME_SAVE_ROOT") {
        return PathBuf::from(root);
    }
    #[cfg(target_os = "windows")]
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local_app_data)
            .join("Aeowun")
            .join("SaveData");
    }

    if let Some(xdg_data_home) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg_data_home).join("Aeowun").join("SaveData");
    }

    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("Aeowun")
            .join("SaveData");
    }

    std::env::temp_dir().join("Aeowun").join("SaveData")
}

pub fn save_path_for_project(project_path: &Path, save_root: &Path) -> PathBuf {
    let identity_path = project_path
        .canonicalize()
        .unwrap_or_else(|_| project_path.to_path_buf());
    let mut identity = identity_path.to_string_lossy().replace('\\', "/");
    if cfg!(target_os = "windows") {
        identity.make_ascii_lowercase();
    }

    let hash = fnv1a64(identity.as_bytes());
    let project_name = project_path
        .file_name()
        .and_then(OsStr::to_str)
        .map(sanitize_project_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "project".to_string());

    save_root
        .join(format!("{}-{:016x}", project_name, hash))
        .join(format!("{}.json", DEFAULT_SAVE_SLOT))
}

fn empty_default(value: &SaveValue) -> SaveValue {
    match value {
        SaveValue::Number(_) => SaveValue::Number(0.0),
        SaveValue::Bool(_) => SaveValue::Bool(false),
        SaveValue::String(_) => SaveValue::String(String::new()),
    }
}
fn same_type(a: &SaveValue, b: &SaveValue) -> bool {
    matches!(
        (a, b),
        (SaveValue::Number(_), SaveValue::Number(_))
            | (SaveValue::Bool(_), SaveValue::Bool(_))
            | (SaveValue::String(_), SaveValue::String(_))
    )
}
fn validate_value(value: &SaveValue) -> Result<(), String> {
    if matches!(value, SaveValue::Number(n) if !n.is_finite()) {
        Err("Save numbers and defaults must be finite".into())
    } else {
        Ok(())
    }
}
fn validated_defaults(
    values: &BTreeMap<String, SaveValue>,
    supplied: BTreeMap<String, SaveValue>,
) -> io::Result<BTreeMap<String, SaveValue>> {
    for (key, default) in &supplied {
        let Some(current) = values.get(key) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Save default has no current field: {key}"),
            ));
        };
        if !same_type(default, current) || validate_value(default).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid default type/value for field: {key}"),
            ));
        }
    }
    Ok(values
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                supplied
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| empty_default(value)),
            )
        })
        .collect())
}

fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("save keys cannot be empty".to_string());
    }
    if key.len() > 256 {
        return Err("save keys cannot exceed 256 bytes".to_string());
    }
    Ok(())
}

fn sanitize_project_name(name: &str) -> String {
    name.chars()
        .take(48)
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn temporary_path(destination: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let file_name = destination
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(DEFAULT_SAVE_SLOT);
    destination.with_file_name(format!(
        ".{}.{}.{}.tmp",
        file_name,
        std::process::id(),
        nonce
    ))
}

#[cfg(target_os = "windows")]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };

    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn atomic_replace(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn sync_parent_directory(_parent: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn sync_parent_directory(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_save_state_{}_{}_{}",
            name,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn stale_save_slot_commit_rejects_external_edit_and_retains_disk_bytes() {
        let root = test_root("external_conflict");
        let project = root.join("Project");
        let saves = root.join("saves");
        fs::create_dir_all(&project).unwrap();
        let mut first = SaveState::load_from_root(&project, &saves).unwrap();
        first.set("coins", SaveValue::Number(1.0)).unwrap();
        first.commit().unwrap();

        let mut stale = SaveState::load_from_root(&project, &saves).unwrap();
        let mut fresh = SaveState::load_from_root(&project, &saves).unwrap();
        fresh.set("coins", SaveValue::Number(5.0)).unwrap();
        fresh.commit().unwrap();
        let preserved = fs::read(stale.path()).unwrap();

        stale.set("coins", SaveValue::Number(99.0)).unwrap();
        let error = stale.commit().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(error.to_string().contains("Reopen the slot"));
        assert!(stale.is_dirty());
        assert_eq!(fs::read(stale.path()).unwrap(), preserved);

        let second_project = root.join("Second");
        fs::create_dir_all(&second_project).unwrap();
        let mut pending = SaveState::load_from_root(&second_project, &saves).unwrap();
        let mut external = SaveState::load_from_root(&second_project, &saves).unwrap();
        external.set("key", SaveValue::Bool(true)).unwrap();
        external.commit().unwrap();
        let external_bytes = fs::read(external.path()).unwrap();
        pending.set("key", SaveValue::Bool(false)).unwrap();
        assert_eq!(
            pending.commit().unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(external.path()).unwrap(), external_bytes);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn typed_values_persist_only_after_commit() {
        let root = test_root("round_trip");
        let project = root.join("projects").join("Dungeon");
        let saves = root.join("runtime_saves");
        fs::create_dir_all(&project).unwrap();

        let mut state = SaveState::load_from_root(&project, &saves).unwrap();
        state.set("coins", SaveValue::Number(12.0)).unwrap();
        state.set("door_open", SaveValue::Bool(true)).unwrap();
        state
            .set("player_name", SaveValue::String("Ada".to_string()))
            .unwrap();

        assert!(!state.path().exists());
        state.commit().unwrap();

        let loaded = SaveState::load_from_root(&project, &saves).unwrap();
        assert_eq!(loaded.get("coins"), Some(&SaveValue::Number(12.0)));
        assert_eq!(loaded.get("door_open"), Some(&SaveValue::Bool(true)));
        assert_eq!(
            loaded.get("player_name"),
            Some(&SaveValue::String("Ada".to_string()))
        );
        assert!(!loaded.path().starts_with(&project));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn previous_save_lookup_finds_old_project_path_without_touching_source_or_others() {
        let root = test_root("previous_save");
        let save_root = root.join("saves");
        let old = root.join("old_location").join("Marooned");
        let moved = root.join("new_location").join("Marooned");
        let other = root.join("new_location").join("Marooned Copy");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&moved).unwrap();
        fs::create_dir_all(&other).unwrap();

        let mut former = SaveState::load_from_root(&old, &save_root).unwrap();
        former
            .set("marooned_intro_seen", SaveValue::Bool(true))
            .unwrap();
        former.commit().unwrap();
        let old_bytes = fs::read(former.path()).unwrap();
        let mut other_state = SaveState::load_from_root(&other, &save_root).unwrap();
        other_state.set("private", SaveValue::Number(99.0)).unwrap();
        other_state.commit().unwrap();

        let current = SaveState::load_from_root(&moved, &save_root).unwrap();
        assert!(current.values().is_empty());
        let previous = previous_saves_in_root(&moved, &save_root).unwrap();
        assert_eq!(previous.len(), 1);
        assert_eq!(previous[0].path, former.path());
        assert_eq!(
            previous[0].values.get("marooned_intro_seen"),
            Some(&SaveValue::Bool(true))
        );
        assert_eq!(fs::read(former.path()).unwrap(), old_bytes);
        assert!(
            !current.path().exists(),
            "Discovery cannot create a destination save"
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn projects_use_isolated_default_slots() {
        let root = test_root("isolation");
        let saves = root.join("saves");
        let first_project = root.join("projects").join("First");
        let second_project = root.join("projects").join("Second");
        fs::create_dir_all(&first_project).unwrap();
        fs::create_dir_all(&second_project).unwrap();

        let first = SaveState::load_from_root(&first_project, &saves).unwrap();
        let second = SaveState::load_from_root(&second_project, &saves).unwrap();
        assert_ne!(first.path(), second.path());
        assert_eq!(first.path().file_name().unwrap(), "default.json");
        assert_eq!(second.path().file_name().unwrap(), "default.json");

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn remove_is_staged_until_commit() {
        let root = test_root("remove");
        let project = root.join("Project");
        let saves = root.join("saves");
        fs::create_dir_all(&project).unwrap();

        let mut state = SaveState::load_from_root(&project, &saves).unwrap();
        state.set("exists", SaveValue::Bool(true)).unwrap();
        state.commit().unwrap();
        assert!(state.remove("exists").unwrap());
        assert!(!state.has("exists"));
        assert!(
            SaveState::load_from_root(&project, &saves)
                .unwrap()
                .has("exists")
        );

        state.commit().unwrap();
        assert!(
            !SaveState::load_from_root(&project, &saves)
                .unwrap()
                .has("exists")
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn renaming_save_field_preserves_type_and_never_overwrites() {
        let root = test_root("rename");
        let project = root.join("project");
        let mut state = SaveState::load_from_root(&project, &root.join("saves")).unwrap();
        state.set("actor.health", SaveValue::Number(42.5)).unwrap();
        state
            .set("player.name", SaveValue::String("Player".into()))
            .unwrap();
        assert!(state.rename("actor.health", "player.name").is_err());
        assert!(state.rename("actor.health", "").is_err());
        assert_eq!(state.values().len(), 2);
        state.rename("actor.health", "actor.hp").unwrap();
        assert_eq!(state.get("actor.hp"), Some(&SaveValue::Number(42.5)));
        assert_eq!(state.get("actor.health"), None);
        state.commit().unwrap();
        let reopened = SaveState::load_from_root(&project, &root.join("saves")).unwrap();
        assert_eq!(reopened.get("actor.hp"), Some(&SaveValue::Number(42.5)));
        assert_eq!(
            reopened.get("player.name"),
            Some(&SaveValue::String("Player".into()))
        );
        assert!(!reopened.is_dirty());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reset_restores_explicit_defaults_without_restart_or_disk_write() {
        let root = test_root("defaults_reset");
        let project = root.join("Project");
        let saves = root.join("saves");
        fs::create_dir_all(&project).unwrap();
        let mut state = SaveState::load_from_root(&project, &saves).unwrap();
        let path = state.path().to_owned();
        state
            .set_with_default(
                "hearts",
                SaveValue::Number(3.0),
                Some(SaveValue::Number(5.0)),
            )
            .unwrap();
        state
            .set_with_default(
                "unlocked",
                SaveValue::Bool(true),
                Some(SaveValue::Bool(true)),
            )
            .unwrap();
        state
            .set_with_default(
                "name",
                SaveValue::String("Current".into()),
                Some(SaveValue::String("New player".into())),
            )
            .unwrap();
        state.set("coins", SaveValue::Number(99.0)).unwrap();
        assert_eq!(state.get_default("coins"), Some(&SaveValue::Number(0.0)));
        assert!(!path.exists(), "set and reset must not create a new slot");
        assert_eq!(state.reset_values(Some("absent")).unwrap(), 0);
        assert!(!state.has("absent"));
        assert_eq!(state.reset_values(Some("hearts")).unwrap(), 1);
        assert_eq!(state.get("hearts"), Some(&SaveValue::Number(5.0)));
        assert_eq!(state.get("coins"), Some(&SaveValue::Number(99.0)));
        assert_eq!(state.reset_values(None).unwrap(), 2);
        assert_eq!(state.get("unlocked"), Some(&SaveValue::Bool(true)));
        assert_eq!(
            state.get("name"),
            Some(&SaveValue::String("New player".into()))
        );
        assert_eq!(state.get("coins"), Some(&SaveValue::Number(0.0)));
        assert!(
            !state.take_restart_request(),
            "save.reset must not restart Play"
        );
        assert!(!path.exists(), "only save.commit persists changes");
        state.commit().unwrap();
        let reopened = SaveState::load_from_root(&project, &saves).unwrap();
        assert_eq!(reopened.values(), state.values());
        assert_eq!(reopened.defaults(), state.defaults());
        assert_eq!(
            reopened.get_default("hearts"),
            Some(&SaveValue::Number(5.0))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_version_one_saves_preserve_progress_and_gain_typed_defaults() {
        let root = test_root("legacy_default_migration");
        let project = root.join("Project");
        let saves = root.join("saves");
        fs::create_dir_all(&project).unwrap();
        let path = save_path_for_project(&project, &saves);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{
          "format_version": 1, "slot": "default", "values": {
            "coins": {"type":"number","value":42.0},
            "opened": {"type":"bool","value":true},
            "zone": {"type":"string","value":"Forest"}
          }
        }"#,
        )
        .unwrap();
        let bytes_before = fs::read(&path).unwrap();
        let mut state = SaveState::load_from_root(&project, &saves).unwrap();
        assert_eq!(state.get("coins"), Some(&SaveValue::Number(42.0)));
        assert_eq!(state.get_default("coins"), Some(&SaveValue::Number(0.0)));
        assert_eq!(state.get_default("opened"), Some(&SaveValue::Bool(false)));
        assert_eq!(
            state.get_default("zone"),
            Some(&SaveValue::String(String::new()))
        );
        assert_eq!(fs::read(&path).unwrap(), bytes_before);
        assert!(!state.is_dirty());
        state
            .set_with_default(
                "coins",
                SaveValue::Number(84.0),
                Some(SaveValue::Number(12.0)),
            )
            .unwrap();
        state.set("coins", SaveValue::Number(43.0)).unwrap();
        assert_eq!(state.get_default("coins"), Some(&SaveValue::Number(12.0)));
        assert!(
            state
                .set_with_default("coins", SaveValue::Number(4.0), Some(SaveValue::Bool(true)))
                .is_err()
        );
        assert!(
            state
                .set_with_default(
                    "coins",
                    SaveValue::Number(4.0),
                    Some(SaveValue::Number(f64::INFINITY))
                )
                .is_err()
        );
        assert_eq!(state.get("coins"), Some(&SaveValue::Number(43.0)));
        state.commit().unwrap();
        let version: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(version["format_version"], 2);
        let loaded = SaveState::load_from_root(&project, &saves).unwrap();
        assert_eq!(loaded.get_default("coins"), Some(&SaveValue::Number(12.0)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_op_reset_and_commit_never_create_a_save_slot() {
        let root = test_root("no_slot_creation");
        let project = root.join("Project");
        let saves = root.join("saves");
        fs::create_dir_all(&project).unwrap();
        let mut state = SaveState::load_from_root(&project, &saves).unwrap();
        assert_eq!(state.reset_values(None).unwrap(), 0);
        assert_eq!(state.reset_values(Some("unused")).unwrap(), 0);
        state.commit().unwrap();
        assert!(!state.path().exists());
        assert!(!state.take_restart_request());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn newer_save_format_is_rejected() {
        let root = test_root("future_format");
        let project = root.join("Project");
        let saves = root.join("saves");
        fs::create_dir_all(&project).unwrap();
        let path = save_path_for_project(&project, &saves);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"format_version":999,"slot":"default","values":{}}"#,
        )
        .unwrap();

        let error = SaveState::load_from_root(&project, &saves).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("newer than supported"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn invalid_keys_and_numbers_are_rejected() {
        let root = test_root("validation");
        let project = root.join("Project");
        let mut state = SaveState::load_from_root(&project, &root.join("saves")).unwrap();

        assert!(state.set("", SaveValue::Bool(true)).is_err());
        assert!(state.set("bad", SaveValue::Number(f64::NAN)).is_err());
        assert!(state.remove("").is_err());

        fs::remove_dir_all(root).ok();
    }
}
