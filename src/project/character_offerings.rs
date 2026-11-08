// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

const API: &str = "https://api.github.com/repos/Aeowun/Worldkiln-AssetPacks";
const INDEX: &str = "worldkiln-characters-v1.json";
const MAX_INDEX: u64 = 2 * 1024 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_CHARACTER: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Offering {
    pub id: String,
    pub name: String,
    pub license: String,
    pub attribution: String,
    pub version: String,
    pub tag: String,
    pub model_asset_id: u64,
    pub model_file: String,
    pub model_bytes: u64,
    pub model_sha256: String,
    pub thumbnail_asset_id: u64,
    pub thumbnail_file: String,
    pub thumbnail_bytes: u64,
    pub thumbnail_sha256: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub characters: Vec<Offering>,
}

fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 32
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn safe_version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 80
        && s != "."
        && s != ".."
        && !s.starts_with('.')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn filename(s: &str, extension: &str) -> bool {
    s.len() <= 160
        && !s.contains(['/', '\\', ':'])
        && s.to_ascii_lowercase().ends_with(extension)
        && !s.starts_with('.')
        && !s.ends_with(' ')
        && !s
            .chars()
            .any(|ch| ch.is_control() || "*?\"<>|".contains(ch))
}
impl Catalog {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() as u64 > MAX_INDEX {
            return Err("Character catalogue exceeds size limit".into());
        }
        let value: Self = serde_json::from_slice(data).map_err(|e| e.to_string())?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || self.characters.len() > 2048 {
            return Err("Unsupported Character catalogue".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.characters {
            if !safe_id(&c.id)
                || !safe_version(&c.version)
                || !safe_version(&c.tag)
                || c.name.trim().is_empty()
                || c.name.len() > 128
                || c.license.trim().is_empty()
                || c.attribution.trim().is_empty()
                || !ids.insert(c.id.to_ascii_lowercase())
                || !filename(&c.model_file, ".glb")
                || !filename(&c.thumbnail_file, ".png")
                || !digest(&c.model_sha256)
                || !digest(&c.thumbnail_sha256)
                || c.model_asset_id == 0
                || c.thumbnail_asset_id == 0
                || c.model_asset_id == c.thumbnail_asset_id
                || c.model_bytes == 0
                || c.model_bytes > MAX_CHARACTER
                || c.thumbnail_bytes == 0
                || c.thumbnail_bytes > MAX_IMAGE
            {
                return Err(format!("Invalid individual Character: {}", c.id));
            }
        }
        Ok(())
    }
}

pub fn cache_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Aeowun")
        .join("Characters")
}
fn check_no_links(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() {
                return Err("Character cache contains a linked path".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Character cache contains a reparse point".into());
                }
            }
        }
    }
    Ok(())
}
pub fn cached_dir(root: &Path, c: &Offering) -> PathBuf {
    root.join(&c.id).join(&c.version)
}
fn file_hash(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    crate::engine::game_package::sha256_reader(&mut f)
        .map(|(hash, _)| hash)
        .map_err(|e| e.to_string())
}
fn check_image(bytes: &[u8]) -> Result<(), String> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| format!("Character thumbnail is not a valid PNG: {e}"))?;
    if img.width() == 0 || img.height() == 0 || img.width() > 4096 || img.height() > 4096 {
        return Err("Character thumbnail dimensions are invalid".into());
    }
    Ok(())
}
pub fn installed(root: &Path, c: &Offering) -> bool {
    let dir = cached_dir(root, c);
    if check_no_links(&dir).is_err()
        || check_no_links(&dir.join("character.glb")).is_err()
        || check_no_links(&dir.join("thumbnail.png")).is_err()
    {
        return false;
    }
    dir.join("offering.json").is_file()
        && fs::read(dir.join("offering.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Offering>(&b).ok())
            .as_ref()
            == Some(c)
        && file_hash(&dir.join("character.glb")).ok().as_deref() == Some(c.model_sha256.as_str())
        && file_hash(&dir.join("thumbnail.png")).ok().as_deref()
            == Some(c.thumbnail_sha256.as_str())
        && fs::read(dir.join("thumbnail.png"))
            .ok()
            .is_some_and(|b| check_image(&b).is_ok())
}
pub fn local(root: &Path) -> Vec<Offering> {
    let mut result = Vec::new();
    if check_no_links(root).is_err() {
        return result;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return result;
    };
    for entry in entries.flatten() {
        if entry.path().is_symlink() || !entry.path().is_dir() {
            continue;
        }
        let Ok(versions) = fs::read_dir(entry.path()) else {
            continue;
        };
        for version in versions.flatten() {
            if version.path().is_symlink() || !version.path().is_dir() {
                continue;
            }
            let Ok(bytes) = fs::read(version.path().join("offering.json")) else {
                continue;
            };
            let Ok(c) = serde_json::from_slice::<Offering>(&bytes) else {
                continue;
            };
            if (Catalog {
                schema: 1,
                characters: vec![c.clone()],
            })
            .validate()
            .is_ok()
                && installed(root, &c)
            {
                result.push(c);
            }
        }
    }
    result.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    result.dedup_by(|a, b| a.id == b.id);
    result
}
fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent("Worldkiln-Characters/1")
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(|e| e.to_string())
}
fn release(
    client: &reqwest::blocking::Client,
    tag: Option<&str>,
) -> Result<serde_json::Value, String> {
    let url = match tag {
        Some(tag) => format!("{API}/releases/tags/{tag}"),
        None => format!("{API}/releases/latest"),
    };
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .take(MAX_INDEX + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_INDEX {
        return Err("Release metadata exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn asset<'a>(
    release: &'a serde_json::Value,
    id: u64,
    name: &str,
    size: u64,
    sha: &str,
) -> Result<&'a serde_json::Value, String> {
    release["assets"]
        .as_array()
        .ok_or("Missing official release assets")?
        .iter()
        .find(|a| {
            a["id"].as_u64() == Some(id)
                && a["name"].as_str() == Some(name)
                && a["size"].as_u64() == Some(size)
                && a["digest"].as_str() == Some(&format!("sha256:{sha}"))
        })
        .ok_or_else(|| format!("Exact Character release asset '{name}' is unavailable"))
}
fn fetch_bytes(client: &reqwest::blocking::Client, id: u64, limit: u64) -> Result<Vec<u8>, String> {
    let mut response = client
        .get(format!("{API}/releases/assets/{id}"))
        .header("Accept", "application/octet-stream")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Character download exceeds declared limit".into());
    }
    Ok(bytes)
}
fn hash_bytes(bytes: &[u8]) -> String {
    crate::engine::game_package::sha256_reader(&mut std::io::Cursor::new(bytes))
        .map(|(hash, _)| hash)
        .unwrap_or_default()
}
pub fn fetch_catalog() -> Result<Catalog, String> {
    let c = client()?;
    let latest = release(&c, None)?;
    let r = if latest["assets"]
        .as_array()
        .is_some_and(|entries| entries.iter().any(|a| a["name"].as_str() == Some(INDEX)))
    {
        latest
    } else {
        release(&c, Some("asset-packs-v1.7.0"))?
    };
    let index = r["assets"]
        .as_array()
        .and_then(|entries| entries.iter().find(|a| a["name"].as_str() == Some(INDEX)))
        .ok_or("No individually published Character catalogue is available yet")?;
    let id = index["id"]
        .as_u64()
        .ok_or("Character index has no asset identity")?;
    let len = index["size"]
        .as_u64()
        .ok_or("Character index has no size")?;
    let sha = index["digest"]
        .as_str()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("Character index has no digest")?;
    if len > MAX_INDEX || !digest(sha) {
        return Err("Character index is too large or unverified".into());
    }
    let bytes = fetch_bytes(&c, id, MAX_INDEX)?;
    if bytes.len() as u64 != len || hash_bytes(&bytes) != sha {
        return Err("Character catalogue integrity failure".into());
    }
    Catalog::parse(&bytes)
}
pub fn preview_bytes(c: &Offering) -> Result<Vec<u8>, String> {
    let path = cached_dir(&cache_root(), c).join("thumbnail.png");
    check_no_links(&path)?;
    if path.is_file() && fs::metadata(&path).is_ok_and(|meta| meta.len() <= MAX_IMAGE) {
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        if hash_bytes(&bytes) == c.thumbnail_sha256 && check_image(&bytes).is_ok() {
            return Ok(bytes);
        }
    }
    let client = client()?;
    let r = release(&client, Some(&c.tag))?;
    asset(
        &r,
        c.thumbnail_asset_id,
        &c.thumbnail_file,
        c.thumbnail_bytes,
        &c.thumbnail_sha256,
    )?;
    let bytes = fetch_bytes(&client, c.thumbnail_asset_id, c.thumbnail_bytes)?;
    if bytes.len() as u64 != c.thumbnail_bytes || hash_bytes(&bytes) != c.thumbnail_sha256 {
        return Err("Character preview digest mismatch".into());
    }
    check_image(&bytes)?;
    Ok(bytes)
}
pub fn download(
    root: &Path,
    c: &Offering,
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    Catalog {
        schema: 1,
        characters: vec![c.clone()],
    }
    .validate()?;
    if installed(root, c) {
        return Ok(());
    }
    let client = client()?;
    let release = release(&client, Some(&c.tag))?;
    asset(
        &release,
        c.model_asset_id,
        &c.model_file,
        c.model_bytes,
        &c.model_sha256,
    )?;
    asset(
        &release,
        c.thumbnail_asset_id,
        &c.thumbnail_file,
        c.thumbnail_bytes,
        &c.thumbnail_sha256,
    )?;
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = {
        let _wait =
            crate::engine::development_capture::operation("wait", "character_cache_lock", true);
        LOCK.lock()
            .map_err(|_| "Character cache installer unavailable")?
    };
    if installed(root, c) {
        return Ok(());
    }
    let dir = cached_dir(root, c);
    check_no_links(&dir)?;
    if dir.exists() {
        return Err("Character cache revision exists but is invalid; refusing to overwrite".into());
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let staging = root.join(format!(".character-{:032x}", rand::random::<u128>()));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        if cancel.load(Ordering::Acquire) {
            return Err("Character download cancelled".into());
        }
        let mut response = client
            .get(format!("{API}/releases/assets/{}", c.model_asset_id))
            .header("Accept", "application/octet-stream")
            .send()
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let mut output =
            fs::File::create(staging.join("character.glb")).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 128 * 1024];
        let mut done = 0u64;
        loop {
            if cancel.load(Ordering::Acquire) {
                return Err("Character download cancelled".into());
            }
            let count = response.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            done = done
                .checked_add(count as u64)
                .ok_or("Character download size overflow")?;
            if done > c.model_bytes {
                return Err("Character download exceeds declared size".into());
            }
            use std::io::Write;
            output
                .write_all(&buffer[..count])
                .map_err(|e| e.to_string())?;
            progress(done, c.model_bytes + c.thumbnail_bytes);
        }
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        if done != c.model_bytes || file_hash(&staging.join("character.glb"))? != c.model_sha256 {
            return Err("Character model checksum mismatch".into());
        }
        if cancel.load(Ordering::Acquire) {
            return Err("Character download cancelled".into());
        }
        let png = fetch_bytes(&client, c.thumbnail_asset_id, c.thumbnail_bytes)?;
        if png.len() as u64 != c.thumbnail_bytes || hash_bytes(&png) != c.thumbnail_sha256 {
            return Err("Character preview checksum mismatch".into());
        }
        check_image(&png)?;
        if cancel.load(Ordering::Acquire) {
            return Err("Character download cancelled".into());
        }
        fs::write(staging.join("thumbnail.png"), &png).map_err(|e| e.to_string())?;
        fs::write(
            staging.join("offering.json"),
            serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        fs::create_dir_all(dir.parent().ok_or("Invalid Character cache path")?)
            .map_err(|e| e.to_string())?;
        fs::rename(&staging, &dir).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    if result.is_ok() {
        progress(
            c.model_bytes + c.thumbnail_bytes,
            c.model_bytes + c.thumbnail_bytes,
        );
    }
    result
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Setup {
    Blank,
    ThirdPerson,
    TopDown,
    Custom,
}

pub fn validate_selection(setup: Setup, character: Option<&Offering>) -> Result<(), String> {
    if setup != Setup::Blank && character.is_none() {
        return Err("Choose a starting Character".into());
    }
    Ok(())
}
fn logic_script(category: &str, setup: Setup) -> Option<(&'static str, &'static str)> {
    match (category, setup) {
        ("cameras", Setup::ThirdPerson) => Some((
            "ThirdPersonCamera",
            include_str!("../../.assets/cameras/thirdPerson/thirdPersonCamera.aeo"),
        )),
        ("cameras", Setup::TopDown) => Some((
            "TopDownCamera",
            include_str!("../../.assets/cameras/topDown/topDownCamera.aeo"),
        )),
        ("controllers", Setup::ThirdPerson) => Some((
            "ThirdPersonController",
            include_str!(
                "../../.assets/controllers/thirdPerson_Controller/thirdPersonController.aeo"
            ),
        )),
        ("controllers", Setup::TopDown) => Some((
            "ThirdPersonController",
            include_str!(
                "../../.assets/controllers/thirdPerson_Controller/thirdPersonController.aeo"
            ),
        )),
        _ => None,
    }
}
pub fn setup_script(category: &str, setup: Setup, name: &str) -> String {
    match logic_script(category, setup) {
        Some((original, source)) => {
            let script = source.replace("\r\n", "\n").replacen(
                &format!("entity {original}"),
                &format!("entity {name}"),
                1,
            );
            script
        }
        None if category == "cameras" => format!(
            "entity {name} {{\n    player = nil\n    fn constructor(player_object) {{ player = player_object }}\n    fn update(dt) {{ }}\n}}\n"
        ),
        None => format!(
            "entity {name} {{\n    player = nil\n    camera = nil\n    input = nil\n    fn constructor(player_object, camera_object, input_system) {{\n        player = player_object\n        camera = camera_object\n        input = input_system\n    }}\n    fn update(dt) {{ }}\n}}\n"
        ),
    }
}
pub fn install_character(
    project: &Path,
    root: &Path,
    offer: &Offering,
    setup: Setup,
) -> Result<String, String> {
    if !installed(root, offer) {
        return Err("Selected Character has not finished downloading".into());
    }
    if setup == Setup::Blank {
        return Err("Blank projects do not automatically create Characters".into());
    }
    let source = cached_dir(root, offer).join("character.glb");
    let summary = crate::project::gltf_import::inspect_glb(&source)?;
    if summary.kind != crate::project::gltf_import::GlbAssetKind::RiggedCharacter {
        return Err("Character offering must contain supported Character animation (skeletal or node-based)".into());
    }
    let actor_name = format!("character_{}", offer.id);
    if project.join(".assets/entities").join(&actor_name).exists()
        || project
            .join(".assets/characters")
            .join(&actor_name)
            .exists()
    {
        return Err("A Character with this identity already exists; refusing to replace it".into());
    }
    let camera_name = format!("WKCamera_{}", offer.id.replace('-', "_"));
    let controller_name = format!("WKController_{}", offer.id.replace('-', "_"));
    for (kind, logic_name) in [("cameras", &camera_name), ("controllers", &controller_name)] {
        if !crate::project::native_logic::valid_package_name(logic_name)
            || project.join(".assets").join(kind).join(logic_name).exists()
        {
            return Err("Character script identity conflicts with an existing asset".into());
        }
    }
    let imported = crate::project::gltf_import::import_character_glb_named(
        project,
        &source,
        Some(&actor_name),
    )?;
    let name = imported.package_name;
    for (kind, name) in [("cameras", &camera_name), ("controllers", &controller_name)] {
        if !crate::project::native_logic::valid_package_name(name) {
            return Err("Character identity cannot produce a valid script name".into());
        }
        let directory = project.join(".assets").join(kind).join(name);
        if directory.exists() {
            return Err(format!("Existing script package conflicts: {name}"));
        }
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        fs::write(
            directory.join(format!("{name}.aeo")),
            setup_script(kind, setup, name),
        )
        .map_err(|e| e.to_string())?;
    }
    let character = crate::project::character_asset::CharacterAsset {
        version: 1,
        name: name.clone(),
        source_actor: name.clone(),
        scale: [1.0; 3],
        material: None,
        controller: Some(controller_name),
        camera: Some(camera_name),
    };
    crate::project::character_asset::save(project, None, &character)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_packs_or_multi_character_catalog_entries() {
        let mut catalog = Catalog {
            schema: 1,
            characters: vec![Offering {
                id: "rogue".into(),
                name: "Rogue".into(),
                license: "CC0".into(),
                attribution: "Test fixture".into(),
                version: "1".into(),
                tag: "characters-v1".into(),
                model_asset_id: 1,
                model_file: "rogue.glb".into(),
                model_bytes: 20,
                model_sha256: "a".repeat(64),
                thumbnail_asset_id: 2,
                thumbnail_file: "rogue.png".into(),
                thumbnail_bytes: 10,
                thumbnail_sha256: "b".repeat(64),
            }],
        };
        assert!(catalog.validate().is_ok());
        catalog.characters.push(catalog.characters[0].clone());
        assert!(catalog.validate().is_err());
        assert!(validate_selection(Setup::TopDown, None).is_err());
        assert!(validate_selection(Setup::Blank, None).is_ok());
    }
    #[test]
    fn custom_has_blank_editable_camera_controller() {
        let camera = setup_script("cameras", Setup::Custom, "WKCamera_Rogue");
        let controller = setup_script("controllers", Setup::Custom, "WKController_Rogue");
        assert!(camera.contains("fn constructor(player_object)"));
        assert!(controller.contains("fn constructor(player_object, camera_object, input_system)"));
        assert!(!camera.contains("camera.set_position"));
        assert!(!controller.contains("player.set_horizontal_velocity"));
        for (category, script) in [("cameras", camera), ("controllers", controller)] {
            let tokens = crate::scripting::lexer::Lexer::new(&script)
                .tokenize()
                .unwrap();
            let program = crate::scripting::parser::Parser::new(tokens)
                .parse()
                .unwrap();
            assert!(
                !program.declarations.is_empty(),
                "{category} script must parse"
            );
        }
    }
    #[test]
    fn third_person_and_top_down_cameras_remain_script_editable_and_collide() {
        for mode in [Setup::ThirdPerson, Setup::TopDown] {
            let camera = setup_script("cameras", mode, "WKCamera_Hero");
            let controller = setup_script("controllers", mode, "WKController_Hero");
            assert!(
                camera.contains("camera.spring_arm({"),
                "Camera must invoke the native boom"
            );
            assert!(
                camera.contains("follow_speed: 8.0"),
                "Camera lag remains editable"
            );
            assert!(
                camera.contains("dead_zone: [0.08, 0.08]"),
                "Screen framing remains script-configured"
            );
            assert!(
                camera.contains("collision: true"),
                "Collision must be enabled by default"
            );
            assert!(
                !camera.contains("physics.resolve_camera_collision"),
                "No duplicate script collision solver"
            );
            for script in [camera, controller] {
                let tokens = crate::scripting::lexer::Lexer::new(&script)
                    .tokenize()
                    .unwrap();
                crate::scripting::parser::Parser::new(tokens)
                    .parse()
                    .unwrap();
            }
        }
    }
}
