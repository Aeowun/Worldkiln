//! Runtime save data kept separately from authored project content.
//!
//! The initial persistence contract has one project-specific `default` slot.
//! Scripts mutate an in-memory snapshot and explicitly persist it with
//! `save.commit()`. Save files live under the application data directory, not
//! inside the authored project, so Play mode never rewrites project assets.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SAVE_FORMAT_VERSION: u32 = 1;
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
}

#[derive(Debug)]
pub struct SaveState {
    path: PathBuf,
    values: BTreeMap<String, SaveValue>,
    dirty: bool,
    // Worldkiln 0.8.5 | WO MAT-038 | 2026-10-10 16:24 -05:00
    // Detect an externally changed slot before replacing its content.
    loaded_fingerprint: Option<(usize, u64)>,
}

impl SaveState {
    pub fn load(project_path: &Path) -> io::Result<Self> {
        Self::load_from_root(project_path, &save_data_root())
    }

    pub fn load_from_root(project_path: &Path, save_root: &Path) -> io::Result<Self> {
        let path = save_path_for_project(project_path, save_root);

        if !path.exists() {
            return Ok(Self {
                path,
                values: BTreeMap::new(),
                dirty: false,
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

        if document.slot != DEFAULT_SAVE_SLOT {
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
            values: document.values,
            dirty: false,
            loaded_fingerprint,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // Worldkiln 0.8.5 | WO 0.8.7-58 | 2026-10-09
    /// Editor inspection shares exactly the script-facing typed save values.
    /// Iterating is read-only and does not touch disk or mark values dirty.
    pub fn values(&self) -> &BTreeMap<String, SaveValue> {
        &self.values
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
        validate_key(key)?;
        if matches!(&value, SaveValue::Number(number) if !number.is_finite()) {
            return Err("save.set does not accept NaN or infinite numbers".to_string());
        }

        if self.values.get(key) != Some(&value) {
            self.values.insert(key.to_string(), value);
            self.dirty = true;
        }
        Ok(())
    }

    // Worldkiln 0.8.5 | WO 0.8.7-58 | 2026-10-09
    /// Rename without silently replacing an existing field.
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
        self.values.insert(to.to_owned(), value);
        self.dirty = true;
        Ok(())
    }

    pub fn remove(&mut self, key: &str) -> Result<bool, String> {
        validate_key(key)?;
        let removed = self.values.remove(key).is_some();
        self.dirty |= removed;
        Ok(removed)
    }

    pub fn commit(&mut self) -> io::Result<()> {
        if !self.dirty && self.path.exists() {
            return Ok(());
        }
        // Worldkiln 0.8.5 | WO MAT-038 | 2026-10-10 16:24 -05:00
        // A separately opened/editor-modified slot must not be silently
        // overwritten by this older in-memory save snapshot.
        self.verify_unchanged_on_disk()?;

        let document = SaveDocument {
            format_version: SAVE_FORMAT_VERSION,
            slot: DEFAULT_SAVE_SLOT.to_string(),
            values: self.values.clone(),
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

// Worldkiln 0.8.5 | WO MAT-038 | 2026-10-10 16:24 -05:00
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

// Worldkiln 0.8.5 | WO 0.8.7-59 | 2026-10-09
/// Inspect existing Save State files without committing or changing project
/// identity. Used by the editor to offer non-destructive recovery of older
/// project-path save slots.
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

/// Discover previous default slots with the same **exact project folder name**.
/// These are not automatically merged: same-named projects may be unrelated.
/// Call only on panel opening/Reload, not per frame.
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
    // Limit exploration to bounded directory metadata, never scan unrelated
    // game saves or recursively search entire AppData.
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
        // One corrupt legacy file must not hide other recovery candidates.
        let Ok(values) = inspect_saved_slot(&path) else {
            continue;
        };
        found.push(PreviousSaveSlot { path, values });
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(found)
}

pub fn save_data_root() -> PathBuf {
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
    // MOVEFILE_WRITE_THROUGH flushes the rename on Windows. Opening directories
    // as ordinary Files is not supported by std on this platform.
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

    // Worldkiln 0.8.5 | WO MAT-038 | 2026-10-10 16:24 -05:00
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

        // The slot can also appear on disk after this session opened it.
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

    // Worldkiln 0.8.5 | WO 0.8.7-59 | 2026-10-09
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
