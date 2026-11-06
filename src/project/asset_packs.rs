// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
const ASSET_API: &str = "https://api.github.com/repos/Aeowun/Worldkiln-AssetPacks";
fn api_for_pack(_p: &Pack) -> &'static str {
    ASSET_API
}
const MAX_CATALOG: u64 = 8 * 1024 * 1024;
const MAX_ARCHIVE: u64 = 8 * 1024 * 1024 * 1024;
const MAX_EXPANDED: u64 = 16 * 1024 * 1024 * 1024;
pub const DEPENDENCIES: &str = "asset-packs.json";
pub(crate) fn package_directories(root: &Path) -> Vec<PathBuf> {
    fn visit(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 32 {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !meta.is_dir()
                || meta.file_type().is_symlink()
                || entry.file_name().to_string_lossy().starts_with('.')
            {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if path.join("package.json").is_file() {
                out.push(path);
            } else {
                visit(&path, depth + 1, out);
            }
        }
    }
    let mut directories = Vec::new();
    visit(root, 0, &mut directories);
    directories
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub id: String,
    pub version: String,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub target: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackMaterial {
    pub name: String,
    pub document: super::material::MaterialDocument,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub version: String,
    pub min_schema: u32,
    pub max_schema: u32,
    pub min_engine_schema: u32,
    pub max_engine_schema: u32,
    pub tag: String,
    pub asset_id: u64,
    pub asset_name: String,
    pub bytes: u64,
    pub sha256: String,
    pub license: String,
    pub attribution: String,
    pub entries: Vec<Entry>,
    #[serde(default)]
    pub dependencies: Vec<Reference>,
    #[serde(default)]
    pub materials: Vec<PackMaterial>,
}
impl Pack {
    pub fn reference(&self) -> Reference {
        Reference {
            id: self.id.clone(),
            version: self.version.clone(),
            sha256: self.sha256.clone(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub packs: Vec<Pack>,
}
pub fn safe_component(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 100
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn safe_version(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && v != "."
        && v != ".."
}
pub fn safe_relative(v: &str) -> Result<PathBuf, String> {
    if v.is_empty()
        || v.len() > 240
        || v.contains(['\\', ':'])
        || v.starts_with('/')
        || v.split('/').any(|s| {
            s.is_empty()
                || s == "."
                || s == ".."
                || s.ends_with(['.', ' '])
                || s.chars().any(|c| c.is_control() || "*?\"<>|".contains(c))
        })
    {
        return Err(format!("Unsafe asset path {v}"));
    }
    if !Path::new(v)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(format!("Unsafe asset path {v}"));
    }
    for segment in v.split('/') {
        let stem = segment.split('.').next().unwrap_or("").to_ascii_uppercase();
        if [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&stem.as_str())
        {
            return Err("Reserved Windows asset path".into());
        }
    }
    Ok(PathBuf::from(v))
}
fn hash(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())
}
impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_CATALOG {
            return Err("Asset catalog exceeds limit".into());
        }
        let mut value: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        value.packs.retain(|p| p.tag.starts_with("asset-packs-v"));
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || self.packs.len() > 256 {
            return Err("Unsupported asset catalog schema or size".into());
        }
        let mut ids = BTreeSet::new();
        let mut revisions = BTreeSet::new();
        for p in &self.packs {
            if p.min_engine_schema > 1
                || p.max_engine_schema < 1
                || p.min_engine_schema > p.max_engine_schema
            {
                return Err("Unsupported engine schema for optional pack".into());
            }
            if !safe_component(&p.id)
                || !safe_version(&p.version)
                || !p.tag.starts_with("asset-packs-v")
                || !ids.insert(&p.id)
                || !revisions.insert(p.asset_id)
                || p.asset_id == 0
                || p.name.trim().is_empty()
                || p.description.trim().is_empty()
                || p.category.trim().is_empty()
                || p.license.trim().is_empty()
                || p.attribution.trim().is_empty()
                || !hash(&p.sha256)
                || p.bytes == 0
                || p.bytes > MAX_ARCHIVE
                || p.min_schema > 1
                || p.max_schema < 1
                || p.min_schema > p.max_schema
            {
                return Err(format!("Invalid or duplicate pack {}", p.id));
            }
            safe_relative(&p.asset_name)?;
            safe_relative(&p.tag)?;
            if p.asset_name.contains('/')
                || !p.asset_name.to_ascii_lowercase().ends_with(".zip")
                || p.asset_name.eq_ignore_ascii_case("UserData.zip")
                || p.asset_name.to_ascii_lowercase().contains("backup")
                || p.asset_name.to_ascii_lowercase().contains("before-")
                || p.entries.is_empty()
                || p.entries.len() > 50000
            {
                return Err("Engine packages/backups/empty packs are not selectable assets".into());
            }
            let mut sources = BTreeSet::new();
            let mut targets = BTreeSet::new();
            let mut expanded = 0u64;
            for e in &p.entries {
                let path = safe_relative(&e.path)?;
                let target = safe_relative(&e.target)?;
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if ![
                    "png", "jpg", "jpeg", "wav", "mp3", "mp4", "glb", "json", "aeo", "txt", "md",
                    "efkefc", "efkmat", "efkmodel", "efkcurve", "ogg", "tga", "bmp",
                ]
                .contains(&ext.as_str())
                    || !e.target.starts_with(".assets/")
                    || !sources.insert(e.path.to_ascii_lowercase())
                    || !targets.insert(e.target.to_ascii_lowercase())
                    || !hash(&e.sha256)
                    || e.bytes > 512 * 1024 * 1024
                {
                    return Err("Invalid/duplicate/unsupported pack entry".into());
                }
                let category = target
                    .components()
                    .nth(1)
                    .and_then(|c| c.as_os_str().to_str())
                    .unwrap_or("");
                if ![
                    "textures",
                    "materials",
                    "audio",
                    "ui",
                    "video",
                    "entities",
                    "characters",
                    "controllers",
                    "cameras",
                    "licenses",
                    "effects",
                ]
                .contains(&category)
                {
                    return Err("Pack target must be an asset category".into());
                }
                expanded = expanded
                    .checked_add(e.bytes)
                    .ok_or("Pack expansion overflow")?;
            }
            if expanded > MAX_EXPANDED {
                return Err("Pack expansion limit".into());
            }
            let mut documents = BTreeMap::new();
            for material in &p.materials {
                let name = super::material::identifier(&material.name)?;
                if documents
                    .insert(name.clone(), material.document.clone())
                    .is_some()
                    || targets
                        .contains(&format!(".assets/materials/{name}.json").to_ascii_lowercase())
                {
                    return Err("Duplicate pack Material target".into());
                }
                for identity in material.document.textures.values() {
                    let prefix = format!("pack:{}@{}/", p.id, p.version);
                    let target = identity
                        .strip_prefix(&prefix)
                        .ok_or("Pack Material map must reference its exact pack revision")?;
                    if !p.entries.iter().any(|entry| {
                        entry.target == target && target.starts_with(".assets/textures/")
                    }) {
                        return Err("Pack Material references an unlisted Texture".into());
                    }
                }
            }
            let catalog = super::material::MaterialCatalog::from_documents(documents);
            for material in &p.materials {
                catalog.resolve(&material.document.id)?;
            }
        }
        for p in &self.packs {
            for d in &p.dependencies {
                if d.id == p.id || !self.packs.iter().any(|v| v.reference() == *d) {
                    return Err(format!("Missing or conflicting dependency {}", d.id));
                }
            }
        }
        for p in &self.packs {
            let mut stack = BTreeSet::new();
            self.visit(&p.id, &mut stack, &mut BTreeSet::new())?;
        }
        Ok(())
    }
    fn visit(
        &self,
        id: &str,
        stack: &mut BTreeSet<String>,
        done: &mut BTreeSet<String>,
    ) -> Result<(), String> {
        if done.contains(id) {
            return Ok(());
        }
        if !stack.insert(id.into()) {
            return Err("Asset pack dependency cycle".into());
        }
        let pack = self
            .packs
            .iter()
            .find(|p| p.id == id)
            .ok_or("Unknown selected pack")?;
        for d in &pack.dependencies {
            self.visit(&d.id, stack, done)?;
        }
        stack.remove(id);
        done.insert(id.into());
        Ok(())
    }
    pub fn selected(&self, ids: &BTreeSet<String>) -> Result<Vec<Pack>, String> {
        self.validate()?;
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        fn add(
            c: &Catalog,
            id: &str,
            seen: &mut BTreeSet<String>,
            out: &mut Vec<Pack>,
        ) -> Result<(), String> {
            if seen.contains(id) {
                return Ok(());
            }
            let p = c
                .packs
                .iter()
                .find(|p| p.id == id)
                .ok_or("Unknown selected pack")?;
            for d in &p.dependencies {
                add(c, &d.id, seen, out)?;
            }
            seen.insert(id.into());
            out.push(p.clone());
            Ok(())
        }
        for id in ids {
            add(self, id, &mut seen, &mut result)?;
        }
        Ok(result)
    }
}
pub fn cache_root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Aeowun/AssetPacks")
}
fn checked(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() {
                return Err("Linked asset-pack path".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Reparse asset-pack path".into());
                }
            }
        }
    }
    Ok(())
}
fn location(root: &Path, r: &Reference) -> Result<PathBuf, String> {
    if !safe_component(&r.id) || !safe_version(&r.version) || !hash(&r.sha256) {
        return Err("Invalid pack reference".into());
    }
    let path = root.join(&r.id).join(&r.version);
    checked(&path)?;
    Ok(path)
}
fn json_get(client: &reqwest::blocking::Client, url: &str) -> Result<serde_json::Value, String> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(30))
        .send()
        .map_err(|e| format!("Release metadata request failed: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .take(MAX_CATALOG + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_CATALOG {
        return Err("Release metadata exceeds limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn client() -> Result<reqwest::blocking::Client, String> {
    client_builder().build().map_err(|e| e.to_string())
}
fn client_builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .user_agent("Worldkiln-optional-assets")
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .https_only(true)
}
fn select_catalog_asset(releases: &serde_json::Value) -> Option<&serde_json::Value> {
    releases
        .as_array()?
        .iter()
        .filter(|release| release["draft"] != true)
        .find_map(|release| {
            let assets = release["assets"].as_array()?;
            ["assets-manifest-v2.json", "assets-manifest.json"]
                .iter()
                .find_map(|name| {
                    assets
                        .iter()
                        .find(|asset| asset["name"].as_str() == Some(*name))
                })
        })
}
fn fetch_repo_catalog(c: &reqwest::blocking::Client, api: &str) -> Result<Catalog, String> {
    let releases = json_get(c, &format!("{api}/releases?per_page=100"))?;
    let asset = select_catalog_asset(&releases)
        .ok_or_else(|| format!("No published assets manifest in {api}"))?;
    let id = asset["id"]
        .as_u64()
        .ok_or("Catalog asset identity missing")?;
    let mut bytes = Vec::new();
    c.get(format!("{api}/releases/assets/{id}"))
        .timeout(Duration::from_secs(30))
        .header("Accept", "application/octet-stream")
        .send()
        .map_err(|e| format!("Manifest request failed: {e}"))?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .take(MAX_CATALOG + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if asset["size"].as_u64() != Some(bytes.len() as u64) {
        return Err("Catalog size differs from its published revision".into());
    }
    let (digest, _) =
        crate::engine::game_package::sha256_reader(&mut std::io::Cursor::new(&bytes))?;
    if asset["digest"].as_str() != Some(&format!("sha256:{digest}")) {
        return Err("Catalog digest differs from the official release asset".into());
    }
    let catalog = Catalog::parse(&bytes)?;
    if catalog.packs.iter().any(|p| api_for_pack(p) != api) {
        return Err(
            "Asset catalog contains a release tag assigned to the wrong official repository".into(),
        );
    }
    Ok(catalog)
}
fn merge_catalogs(catalogs: impl IntoIterator<Item = Catalog>) -> Result<Catalog, String> {
    let mut packs = Vec::new();
    for catalog in catalogs {
        catalog.validate()?;
        packs.extend(catalog.packs);
    }
    let merged = Catalog { schema: 1, packs };
    merged.validate()?;
    Ok(merged)
}
pub fn fetch_catalog(root: &Path) -> Result<Catalog, String> {
    let c = client()?;
    let previous = cached_catalog(root).ok();
    let mut catalogues = Vec::new();
    let mut failures = Vec::new();
    for api in [ASSET_API] {
        match fetch_repo_catalog(&c, api) {
            Ok(catalog) => catalogues.push(catalog),
            Err(error) => {
                failures.push(format!("{api}: {error}"));
                if let Some(cached) = &previous {
                    let packs = cached
                        .packs
                        .iter()
                        .filter(|p| api_for_pack(p) == api)
                        .cloned()
                        .collect::<Vec<_>>();
                    if !packs.is_empty() {
                        catalogues.push(Catalog { schema: 1, packs });
                    }
                }
            }
        }
    }
    if catalogues.is_empty() {
        return Err(format!(
            "Official asset catalogues unavailable: {}",
            failures.join("; ")
        ));
    }
    let merged = merge_catalogs(catalogues)?;
    if !failures.is_empty() {
        crate::engine::diagnostics::report(
            "WARNING",
            "Assets",
            format!(
                "Asset catalogue refresh partial; previously verified packs retained if available: {}",
                failures.join("; ")
            ),
        );
    }
    let bytes = serde_json::to_vec(&merged).map_err(|e| e.to_string())?;
    let (digest, _) =
        crate::engine::game_package::sha256_reader(&mut std::io::Cursor::new(&bytes))?;
    cache_verified_catalog(root, &bytes, &digest)?;
    Ok(merged)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifiedCatalog {
    sha256: String,
    manifest: Vec<u8>,
}
fn cache_verified_catalog(root: &Path, bytes: &[u8], digest: &str) -> Result<(), String> {
    Catalog::parse(bytes)?;
    checked(root)?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let path = root.join("verified-catalog.json");
    checked(&path)?;
    let record = VerifiedCatalog {
        sha256: digest.into(),
        manifest: bytes.into(),
    };
    super::atomic_file::write_atomically(
        &path,
        &serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn cached_catalog(root: &Path) -> Result<Catalog, String> {
    let path = root.join("verified-catalog.json");
    checked(&path)?;
    if !path.exists() {
        let legacy = root.join("assets-manifest.json");
        checked(&legacy)?;
        let mut bytes = Vec::new();
        File::open(legacy)
            .map_err(|e| e.to_string())?
            .take(MAX_CATALOG + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        return Catalog::parse(&bytes);
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_CATALOG * 5 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_CATALOG * 5 {
        return Err("Cached catalogue exceeds limit".into());
    }
    let record: VerifiedCatalog = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let (digest, _) =
        crate::engine::game_package::sha256_reader(&mut std::io::Cursor::new(&record.manifest))?;
    if digest != record.sha256 {
        return Err("Cached catalogue checksum mismatch".into());
    }
    Catalog::parse(&record.manifest)
}
pub fn installed(root: &Path, r: &Reference) -> bool {
    let Ok(base) = location(root, r) else {
        return false;
    };
    let Ok(bytes) = fs::read(base.join("installed-pack.json")) else {
        return false;
    };
    let Ok(pack) = serde_json::from_slice::<Pack>(&bytes) else {
        return false;
    };
    pack.reference() == *r
        && pack.entries.iter().all(|entry| {
            safe_relative(&entry.target).is_ok()
                && checked(&base.join(&entry.target)).is_ok()
                && fs::metadata(base.join(&entry.target))
                    .is_ok_and(|m| m.is_file() && m.len() == entry.bytes)
        })
        && pack.materials.iter().all(|m| {
            super::material::file_path(&base, &m.name)
                .is_ok_and(|path| checked(&path).is_ok() && path.is_file())
        })
}
pub fn order_by_download_size(mut packs: Vec<Pack>) -> Vec<Pack> {
    packs.sort_by(|a, b| a.bytes.cmp(&b.bytes).then_with(|| a.id.cmp(&b.id)));
    packs
}

pub fn run_ordered_packs(
    packs: &[Pack],
    cancel: &AtomicBool,
    mut install: impl FnMut(&Pack) -> Result<(), String>,
    mut published: impl FnMut(&Pack),
) -> Result<(), String> {
    for pack in packs {
        if cancel.load(Ordering::Acquire) {
            return Err("Asset installation canceled".into());
        }
        install(pack)?;
        published(pack);
    }
    Ok(())
}

pub fn verify_installed_cache(root: &Path, reference: &Reference) -> Result<(), String> {
    if !installed(root, reference) {
        return Err(format!("Pack {} cache is incomplete", reference.id));
    }
    let base = location(root, reference)?;
    let pack: Pack = serde_json::from_slice(
        &fs::read(base.join("installed-pack.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if pack.reference() != *reference {
        return Err("Shared pack revision changed".into());
    }
    for entry in &pack.entries {
        let path = base.join(safe_relative(&entry.target)?);
        checked(&path)?;
        let (digest, bytes) =
            crate::engine::game_package::sha256_file(&path).map_err(|e| e.to_string())?;
        if digest != entry.sha256 || bytes != entry.bytes {
            return Err(format!(
                "Cached pack {} failed checksum for {}; reinstallation required",
                reference.id, entry.target
            ));
        }
    }
    Ok(())
}

fn cancellable_digest(path: &Path, cancel: &AtomicBool) -> Result<(String, u64), String> {
    struct Reader<'a> {
        file: File,
        cancel: &'a AtomicBool,
    }
    impl Read for Reader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.cancel.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "Asset verification cancelled",
                ));
            }
            self.file.read(buffer)
        }
    }
    crate::engine::game_package::sha256_reader(&mut Reader {
        file: File::open(path).map_err(|e| e.to_string())?,
        cancel,
    })
}
pub fn install_archive(
    root: &Path,
    p: &Pack,
    archive: &Path,
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    let mut standalone = p.clone();
    standalone.dependencies.clear();
    Catalog {
        schema: 1,
        packs: vec![standalone],
    }
    .validate()?;
    let dest = location(root, &p.reference())?;
    if verify_installed_cache(root, &p.reference()).is_ok() {
        return Ok(());
    }
    if dest.exists() && (!dest.is_dir() || dest.is_symlink()) {
        return Err("Asset pack cache path is occupied or linked".into());
    }
    let (digest, size) = cancellable_digest(archive, cancel)?;
    if size != p.bytes || digest != p.sha256 {
        return Err("Asset archive size or SHA-256 mismatch".into());
    }
    checked(root)?;
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let stage = dest
        .parent()
        .unwrap()
        .join(format!(".stage-{:032x}", rand::random::<u128>()));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if zip.len() > p.entries.len() + 4096 {
            return Err("Unexpected archive entry count".into());
        }
        let index: BTreeMap<_, _> = p.entries.iter().map(|e| (e.path.as_str(), e)).collect();
        let mut seen = BTreeSet::new();
        let mut completed = 0;
        let total = p.entries.iter().map(|e| e.bytes).sum();
        for i in 0..zip.len() {
            if cancel.load(Ordering::Acquire) {
                return Err("Asset install cancelled".into());
            }
            let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
            let name = file.name().to_owned();
            safe_relative(name.trim_end_matches('/'))?;
            if file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err("Archive symlink rejected".into());
            }
            if file.is_dir() {
                continue;
            }
            let e = *index
                .get(name.as_str())
                .ok_or_else(|| format!("Unexpected archive file {name}"))?;
            if !seen.insert(name)
                || file.size() != e.bytes
                || file.size() > file.compressed_size().max(1).saturating_mul(500)
            {
                return Err("Archive entry size/duplicate/expansion mismatch".into());
            }
            let path = stage.join(&e.target);
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            let mut count = 0;
            let mut buffer = [0; 128 * 1024];
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("Asset install cancelled".into());
                }
                let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                count += n as u64;
                if count > e.bytes {
                    return Err("Expanded entry exceeded size".into());
                }
                output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
                completed += n as u64;
                progress(completed, total);
            }
            output.sync_all().map_err(|e| e.to_string())?;
            drop(output);
            let (digest, size) = cancellable_digest(&path, cancel)?;
            if size != e.bytes || digest != e.sha256 {
                return Err(format!("Invalid pack content {}", e.path));
            }
        }
        if seen.len() != p.entries.len() {
            return Err("Archive is missing listed pack files".into());
        }
        for material in &p.materials {
            let path = super::material::file_path(&stage, &material.name)?;
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            fs::write(
                &path,
                serde_json::to_vec_pretty(&material.document).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let mut dimensions = None;
            for identity in material.document.textures.values() {
                let target = identity
                    .strip_prefix(&format!("pack:{}@{}/", p.id, p.version))
                    .ok_or("Invalid Material resource")?;
                let image = image::ImageReader::open(stage.join(target))
                    .map_err(|e| e.to_string())?
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?;
                if image.format() != Some(image::ImageFormat::Png) {
                    return Err("Pack Material maps must be PNG".into());
                }
                let size = image.into_dimensions().map_err(|e| e.to_string())?;
                if size.0 == 0
                    || size.1 == 0
                    || size.0 > 16384
                    || size.1 > 16384
                    || dimensions.is_some_and(|d| d != size)
                {
                    return Err("Pack Material dimensions do not match".into());
                }
                dimensions = Some(size);
            }
        }
        fs::write(
            stage.join("installed-pack.json"),
            serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if cancel.load(Ordering::Acquire) {
            return Err("Asset install cancelled".into());
        }
        let prior = dest
            .parent()
            .ok_or("Invalid cache directory")?
            .join(format!(".prior-{:032x}", rand::random::<u128>()));
        let had_old = dest.is_dir();
        if had_old {
            fs::rename(&dest, &prior)
                .map_err(|e| format!("Cannot back up incomplete cache: {e}"))?;
        }
        if let Err(error) = fs::rename(&stage, &dest) {
            if had_old {
                fs::rename(&prior, &dest).map_err(|restore| {
                    format!(
                        "Cache publication failed: {error}; backup restore failed: {restore} at {}",
                        prior.display()
                    )
                })?;
            }
            return Err(format!("Cannot publish verified asset pack: {error}"));
        }
        if had_old {
            let _ = fs::remove_dir_all(&prior);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
pub fn download_install(
    root: &Path,
    p: &Pack,
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    if verify_installed_cache(root, &p.reference()).is_ok() {
        return Ok(());
    }
    if !p.tag.starts_with("asset-packs-v") {
        return Err("Asset pack is not hosted in Worldkiln-AssetPacks".into());
    }
    let c = client()?;
    let api = api_for_pack(p);
    let release = json_get(&c, &format!("{api}/releases/tags/{}", p.tag))?;
    let asset = release["assets"]
        .as_array()
        .ok_or("Release assets missing")?
        .iter()
        .find(|a| {
            a["id"].as_u64() == Some(p.asset_id)
                && a["name"] == p.asset_name
                && a["size"].as_u64() == Some(p.bytes)
        })
        .ok_or("Exact catalog asset revision is unavailable")?;
    if asset["digest"].as_str() != Some(&format!("sha256:{}", p.sha256)) {
        return Err("GitHub asset digest differs from catalog".into());
    }
    checked(root)?;
    let downloads = root.join(".downloads");
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let path = downloads.join(format!("{}.part", p.sha256));
    checked(&path)?;
    let offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if offset > p.bytes {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        return Err("Interrupted pack exceeds expected size; retry".into());
    }
    if offset < p.bytes {
        let response = c
            .get(format!("{api}/releases/assets/{}", p.asset_id))
            .header("Accept", "application/octet-stream")
            .header("Range", format!("bytes={offset}-"))
            .send()
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let resume = offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if resume
            && !response
                .headers()
                .get("Content-Range")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.starts_with(&format!("bytes {offset}-")))
        {
            return Err("Invalid resumed download range".into());
        }
        let mut completed = if resume { offset } else { 0 };
        let mut reader = response;
        let mut output = OpenOptions::new()
            .create(true)
            .write(true)
            .append(resume)
            .truncate(!resume)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let transfer = transfer_bytes(
            &mut reader,
            &mut output,
            &mut completed,
            p.bytes,
            cancel,
            progress,
        );
        if let Err(error) = transfer {
            drop(output);
            if cancel.load(Ordering::Acquire) || completed > p.bytes {
                let _ = fs::remove_file(&path);
            }
            return Err(error);
        }
        output.sync_all().map_err(|e| e.to_string())?;
    }
    let result = install_archive(root, p, &path, cancel, progress);
    let _ = fs::remove_file(&path);
    result
}
fn transfer_bytes(
    reader: &mut impl Read,
    output: &mut impl Write,
    completed: &mut u64,
    total: u64,
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    progress(*completed, total);
    let mut buffer = [0; 128 * 1024];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err("Asset download cancelled".into());
        }
        let n = reader.read(&mut buffer).map_err(|e| {
            format!("Interrupted pack transfer; retry resumes verified revision: {e}")
        })?;
        if n == 0 {
            break;
        }
        *completed = completed
            .checked_add(n as u64)
            .ok_or("Download counter overflow")?;
        if *completed > total {
            return Err("Pack download exceeds declared size".into());
        }
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        progress(*completed, total);
    }
    output.flush().map_err(|e| e.to_string())?;
    if *completed != total {
        return Err("Interrupted pack transfer; retry resumes verified revision".into());
    }
    Ok(())
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectPacks {
    schema: u32,
    packs: Vec<Reference>,
}
pub fn references(project: &Path) -> Result<Vec<Reference>, String> {
    let path = project.join(DEPENDENCIES);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let value: ProjectPacks = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if value.schema != 1 {
        return Err("Unsupported project pack schema".into());
    }
    let mut ids = BTreeSet::new();
    for r in &value.packs {
        location(&cache_root(), r)?;
        if !ids.insert(&r.id) {
            return Err("Duplicate project pack dependency".into());
        }
    }
    Ok(value.packs)
}
pub fn bind(root: &Path, project: &Path, packs: Vec<Reference>) -> Result<(), String> {
    if packs.iter().any(|r| !installed(root, r)) {
        return Err("Selected pack is missing or version conflicts; project retained".into());
    }
    let mut ids = BTreeSet::new();
    if packs.iter().any(|r| !ids.insert(&r.id)) {
        return Err("Duplicate project pack dependency".into());
    }
    let mut fx_entries = Vec::new();
    for reference in &packs {
        let base = location(root, reference)?;
        let installed: Pack = serde_json::from_slice(
            &fs::read(base.join("installed-pack.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if installed.reference() != *reference {
            return Err("Optional FX pack revision does not match installed pack".into());
        }
        for entry in &installed.entries {
            if entry.target.starts_with(".assets/effects/") {
                fx_entries.push((base.join(&entry.target), entry.target.clone()));
            }
        }
    }
    super::fx_assets::materialize_bound_effects(project, &fx_entries)?;
    let path = project.join(DEPENDENCIES);
    checked(&path)?;
    let registry = root.join("consumers.json");
    checked(&registry)?;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut consumers: BTreeMap<String, Vec<Reference>> = if registry.exists() {
        serde_json::from_slice(&fs::read(&registry).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?
    } else {
        BTreeMap::new()
    };
    let canonical = fs::canonicalize(project)
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    consumers.insert(canonical, packs.clone());
    super::atomic_file::write_atomically(
        &registry,
        &serde_json::to_vec_pretty(&consumers).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    super::atomic_file::write_atomically(
        &path,
        &serde_json::to_vec_pretty(&ProjectPacks { schema: 1, packs })
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn project_roots_at(root: &Path, project: &Path) -> Result<Vec<PathBuf>, String> {
    references(project)?
        .iter()
        .map(|r| {
            let path = location(root, r)?;
            if !installed(root, r) {
                Err(format!(
                    "Missing optional pack {} version {}; project retained",
                    r.id, r.version
                ))
            } else {
                Ok(path)
            }
        })
        .collect()
}
pub fn project_roots(project: &Path) -> Vec<PathBuf> {
    let root = cache_root();
    let refs = match references(project) {
        Ok(refs) => refs,
        Err(error) => {
            crate::engine::diagnostics::report("WARNING", "Assets", error);
            return Vec::new();
        }
    };
    refs.into_iter().filter_map(|reference|{
        let path=match location(&root,&reference) {
            Ok(path)=>path,
            Err(error)=>{
                crate::engine::diagnostics::report("WARNING","Assets",error);
                return None;
            }
        };
        if installed(&root,&reference) { Some(path) } else {
            crate::engine::diagnostics::report("WARNING","Assets",
                format!("Missing optional pack {} version {}; other selected packs remain available",
                    reference.id,reference.version));
            None
        }
    }).collect()
}
pub fn consumers(root: &Path, r: &Reference) -> Result<Vec<PathBuf>, String> {
    let path = root.join("consumers.json");
    checked(&path)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let registry: BTreeMap<String, Vec<Reference>> =
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut used = Vec::new();
    for (name, recorded) in registry {
        let project = PathBuf::from(name);
        if !project.exists() {
            continue;
        }
        let actual = references(&project).map_err(|error| {
            format!(
                "Cannot verify pack consumers for {}: {error}",
                project.display()
            )
        })?;
        if actual.contains(r) || (recorded.contains(r) && !project.join(DEPENDENCIES).is_file()) {
            used.push(project);
        }
    }
    Ok(used)
}
pub fn uninstall(root: &Path, r: &Reference) -> Result<(), String> {
    if !consumers(root, r)?.is_empty() {
        return Err("Pack is used by projects; remove those dependencies first".into());
    }
    let path = location(root, r)?;
    if !installed(root, r) {
        return Err("Pack revision missing or unrecognized".into());
    }
    fs::remove_dir_all(path).map_err(|e| e.to_string())
}

pub fn resource_identity(r: &Reference, target: &str) -> String {
    format!("pack:{}@{}/{}", r.id, r.version, target)
}
pub fn resource_path_at(root: &Path, project: &Path, identity: &str) -> Result<PathBuf, String> {
    let tail = identity
        .strip_prefix("pack:")
        .ok_or("Not an optional pack resource")?;
    let (revision, target) = tail.split_once('/').ok_or("Invalid pack resource")?;
    let (id, version) = revision.split_once('@').ok_or("Invalid pack revision")?;
    if !safe_component(id) || !safe_version(version) {
        return Err("Unsafe pack resource identity".into());
    }
    safe_relative(target)?;
    let owned = project
        .join(".assets/pack-resources")
        .join(id)
        .join(version)
        .join(target);
    checked(&owned)?;
    if owned.is_file() {
        return Ok(owned);
    }
    let dependency = references(project)?
        .into_iter()
        .find(|r| r.id == id && r.version == version)
        .ok_or("Resource pack is not a declared project dependency")?;
    let base = location(root, &dependency)?;
    let pack: Pack = serde_json::from_slice(
        &fs::read(base.join("installed-pack.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if pack.reference() != dependency || !pack.entries.iter().any(|e| e.target == target) {
        return Err("Resource not in installed pack allowlist".into());
    }
    let path = base.join(target);
    checked(&path)?;
    if !path.is_file() {
        return Err("Installed pack resource missing; reinstall pack".into());
    }
    Ok(path)
}
pub fn resource_path(project: &Path, identity: &str) -> Result<PathBuf, String> {
    resource_path_at(&cache_root(), project, identity)
}
pub fn project_texture_entries(project: &Path) -> Vec<(String, PathBuf)> {
    let mut entries = Vec::new();
    for base in project_roots(project) {
        let Ok(bytes) = fs::read(base.join("installed-pack.json")) else {
            continue;
        };
        let Ok(pack) = serde_json::from_slice::<Pack>(&bytes) else {
            continue;
        };
        for entry in &pack.entries {
            if entry.target.starts_with(".assets/textures/")
                && entry.target.to_ascii_lowercase().ends_with(".png")
            {
                entries.push((
                    resource_identity(&pack.reference(), &entry.target),
                    base.join(&entry.target),
                ));
            }
        }
    }
    entries
}
pub fn make_resource_owned(project: &Path, identity: &str) -> Result<PathBuf, String> {
    make_resource_owned_at(&cache_root(), project, identity)
}
pub fn make_resource_owned_at(
    root: &Path,
    project: &Path,
    identity: &str,
) -> Result<PathBuf, String> {
    let source = resource_path_at(root, project, identity)?;
    let tail = identity
        .strip_prefix("pack:")
        .ok_or("Invalid pack resource")?;
    let (revision, target) = tail.split_once('/').ok_or("Invalid pack resource")?;
    let (id, version) = revision.split_once('@').ok_or("Invalid pack revision")?;
    let dest = project
        .join(".assets/pack-resources")
        .join(id)
        .join(version)
        .join(target);
    checked(&dest)?;
    if dest == source {
        return Ok(dest);
    }
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&dest)
        .map_err(|e| e.to_string())?;
    let result = std::io::copy(
        &mut File::open(source).map_err(|e| e.to_string())?,
        &mut output,
    )
    .and_then(|_| output.sync_all());
    drop(output);
    if let Err(e) = result {
        let _ = fs::remove_file(&dest);
        return Err(e.to_string());
    }
    Ok(dest)
}

pub struct PortableResource {
    pub relative: String,
    pub source: PathBuf,
}
pub fn portable_resources_at(root: &Path, project: &Path) -> Result<Vec<PortableResource>, String> {
    let mut text = String::new();
    fn authored(dir: &Path, depth: usize, text: &mut String) -> Result<(), String> {
        if depth > 32 {
            return Err("Portable project nesting limit".into());
        }
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            checked(&path)?;
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|name| name != ".assets" && name != ".git")
                {
                    authored(&path, depth + 1, text)?;
                }
            } else if path.file_name().is_some_and(|name| name != DEPENDENCIES)
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ["dat", "json", "aeo"].contains(&ext))
            {
                let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
                if metadata.len() > 64 * 1024 * 1024 {
                    return Err("Authored resource references exceed portable scan limit".into());
                }
                text.push_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?);
                text.push('\n');
            }
        }
        Ok(())
    }
    authored(project, 0, &mut text)?;
    let mut identities = BTreeSet::new();
    fn collect(text: &str, out: &mut BTreeSet<String>) {
        for (offset, _) in text.match_indices("pack:") {
            let token = text[offset..]
                .split(|c: char| {
                    c.is_whitespace() || ['"', '\'', ';', ',', ')', ']', '}'].contains(&c)
                })
                .next()
                .unwrap_or("");
            if !token.is_empty() {
                out.insert(token.to_owned());
            }
        }
    }
    collect(&text, &mut identities);
    let mut resources = BTreeMap::new();
    let mut material_sources = BTreeMap::new();
    fn owned_materials(
        project: &Path,
        dir: &Path,
        depth: usize,
        out: &mut BTreeMap<String, (super::material::MaterialDocument, PathBuf, bool)>,
    ) -> Result<(), String> {
        if !dir.exists() {
            return Ok(());
        }
        if depth > 32 {
            return Err("Material directory limit".into());
        }
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            checked(&path)?;
            if path.is_dir() {
                owned_materials(project, &path, depth + 1, out)?;
            } else if path.extension().is_some_and(|ext| ext == "json") {
                let relative = path
                    .strip_prefix(project.join(".assets/materials"))
                    .map_err(|e| e.to_string())?
                    .with_extension("")
                    .to_string_lossy()
                    .replace('\\', "/");
                let document = super::material::read(project, &relative)?;
                out.insert(document.id.clone(), (document, path, true));
            }
        }
        Ok(())
    }
    for reference in references(project)? {
        let base = location(root, &reference)?;
        let pack: Pack = match fs::read(base.join("installed-pack.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
            Err(_) => continue,
        };
        for entry in &pack.entries {
            if !entry.target.starts_with(".assets/materials/") {
                continue;
            }
            let bytes = fs::read(base.join(&entry.target)).map_err(|e| e.to_string())?;
            let document: super::material::MaterialDocument =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            material_sources.insert(
                document.id.clone(),
                (document, base.join(&entry.target), false),
            );
        }
        for material in &pack.materials {
            material_sources.insert(
                material.document.id.clone(),
                (
                    material.document.clone(),
                    super::material::file_path(&base, &material.name)?,
                    false,
                ),
            );
        }
    }
    owned_materials(
        project,
        &project.join(".assets/materials"),
        0,
        &mut material_sources,
    )?;
    let mut used: BTreeSet<String> = material_sources
        .keys()
        .filter(|id| text.contains(id.as_str()))
        .cloned()
        .collect();
    for _ in 0..32 {
        let mut parents = Vec::new();
        for id in &used {
            if let Some((document, _, _)) = material_sources.get(id) {
                if let super::material::MaterialDefinition::Instance { parent, .. } =
                    &document.definition
                {
                    if !material_sources.contains_key(parent) {
                        return Err(format!("Portable Material parent missing: {parent}"));
                    }
                    parents.push(parent.clone());
                }
            }
        }
        let before = used.len();
        used.extend(parents);
        if used.len() == before {
            break;
        }
    }
    for id in used {
        if let Some((document, path, owned)) = material_sources.get(&id) {
            for identity in document.textures.values() {
                collect(identity, &mut identities);
            }
            if !owned {
                let relative = format!(".assets/materials/pack-{id}.json");
                safe_relative(&relative)?;
                resources.insert(relative, path.clone());
            }
        }
    }
    for identity in identities {
        let source = resource_path_at(root, project, &identity)?;
        let tail = identity
            .strip_prefix("pack:")
            .ok_or("Invalid portable resource")?;
        let (revision, target) = tail.split_once('/').ok_or("Invalid portable resource")?;
        let (id, version) = revision
            .split_once('@')
            .ok_or("Invalid portable revision")?;
        let relative = format!(".assets/pack-resources/{id}/{version}/{target}");
        safe_relative(&relative)?;
        resources.insert(relative, source);
    }
    Ok(resources
        .into_iter()
        .map(|(relative, source)| PortableResource { relative, source })
        .collect())
}
pub fn portable_resources(project: &Path) -> Result<Vec<PortableResource>, String> {
    portable_resources_at(&cache_root(), project)
}
pub fn provision_portable_copy(source: &Path, destination: &Path) -> Result<(), String> {
    for resource in portable_resources(source)? {
        let path = destination.join(&resource.relative);
        checked(&path)?;
        if path.exists() {
            continue;
        }
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        std::io::copy(
            &mut File::open(&resource.source).map_err(|e| e.to_string())?,
            &mut file,
        )
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    }
    super::atomic_file::write_atomically(
        &destination.join(DEPENDENCIES),
        br#"{"schema":1,"packs":[]}"#,
    )
    .map_err(|e| e.to_string())
}

fn stage_catalog_models(
    staging: &Path,
    root: &Path,
    packs: &[Pack],
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<Vec<(PathBuf, String)>, String> {
    let mut created = Vec::new();
    let total = packs
        .iter()
        .flat_map(|pack| pack.entries.iter())
        .filter(|entry| {
            (entry.target.starts_with(".assets/entities/")
                || entry.target.starts_with(".assets/characters/"))
                && entry.target.to_ascii_lowercase().ends_with(".glb")
        })
        .count() as u64;
    let mut done = 0u64;
    for pack in packs {
        if cancel.load(Ordering::Acquire) {
            return Err("Asset installation canceled".into());
        }
        let base = location(root, &pack.reference())?;
        let actor_sources: Vec<_> = pack
            .entries
            .iter()
            .filter(|entry| {
                entry.target.starts_with(".assets/entities/")
                    && entry.target.to_ascii_lowercase().ends_with(".glb")
            })
            .collect();
        for entry in actor_sources {
            if cancel.load(Ordering::Acquire) {
                return Err("Asset installation canceled".into());
            }
            let source = base.join(&entry.target);
            let stem = Path::new(&entry.target)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or("Invalid Actor filename")?;
            let namespace = format!("{}_{}", pack.id, stem);
            let result = super::gltf_import::import_static_glb_named(staging, &source, &namespace)
                .map_err(|error| {
                    format!(
                        "Pack '{}' cannot create native Actor from '{}': {error}",
                        pack.name, entry.target
                    )
                })?;
            if result.packages.is_empty() {
                return Err(format!(
                    "Pack '{}' contains an Actor with no usable meshes",
                    pack.name
                ));
            }
            for actor in result.packages {
                created.push((
                    PathBuf::from(".assets/entities").join(actor.package_name),
                    pack.name.clone(),
                ));
            }
            done += 1;
            progress(done, total);
        }
        for entry in pack.entries.iter().filter(|entry| {
            entry.target.starts_with(".assets/characters/")
                && entry.target.to_ascii_lowercase().ends_with(".glb")
        }) {
            if cancel.load(Ordering::Acquire) {
                return Err("Asset installation canceled".into());
            }
            let source = base.join(&entry.target);
            let summary = super::gltf_import::inspect_glb(&source)
                .map_err(|error| format!("Character in pack '{}': {error}", pack.name))?;
            if summary.kind != super::gltf_import::GlbAssetKind::RiggedCharacter {
                return Err(format!(
                    "Pack '{}' labels a static model as a rigged Actor: {}",
                    pack.name, entry.target
                ));
            }
            let stem = Path::new(&entry.target)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or("Invalid Character filename")?;
            let actor_name = format!("{}_{}", pack.id, stem);
            let result =
                super::gltf_import::import_character_glb_named(staging, &source, Some(&actor_name))
                    .map_err(|error| {
                        format!(
                            "Pack '{}' cannot create a native rigged Actor: {error}",
                            pack.name
                        )
                    })?;
            created.push((
                PathBuf::from(".assets/entities").join(result.package_name),
                pack.name.clone(),
            ));
            done += 1;
            progress(done, total);
        }
    }
    Ok(created)
}

pub fn clone_into_project(
    root: &Path,
    project: &Path,
    refs: &[Reference],
    cancel: &AtomicBool,
    progress: &impl Fn(u64, u64),
) -> Result<(), String> {
    if !project.is_dir() {
        return Err("Project is not available for asset installation".into());
    }
    checked(project)?;
    struct Item {
        source: PathBuf,
        relative: PathBuf,
        document: Option<Vec<u8>>,
        digest: Option<String>,
    }
    let mut items = Vec::new();
    let mut selected_packs = Vec::new();
    let mut names = BTreeSet::new();
    for reference in refs {
        if !installed(root, reference) {
            return Err(format!(
                "Shared pack {} is not installed or verified",
                reference.id
            ));
        }
        let base = location(root, reference)?;
        let pack: Pack = serde_json::from_slice(
            &fs::read(base.join("installed-pack.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if pack.reference() != *reference {
            return Err("Installed pack identity changed".into());
        }
        let local_material = |bytes: &[u8]| -> Result<Vec<u8>, String> {
            let mut document: super::material::MaterialDocument =
                serde_json::from_slice(bytes).map_err(|e| format!("Invalid pack Material: {e}"))?;
            for identity in document.textures.values_mut() {
                let prefix = format!("pack:{}@{}/.assets/textures/", pack.id, pack.version);
                if let Some(texture) = identity.strip_prefix(&prefix) {
                    let target = format!(".assets/textures/{texture}");
                    if !pack.entries.iter().any(|entry| entry.target == target) {
                        return Err("Pack Material references a missing texture".into());
                    }
                    *identity = texture.strip_suffix(".png").unwrap_or(texture).to_owned();
                }
            }
            serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())
        };
        for entry in &pack.entries {
            let relative = safe_relative(&entry.target)?;
            if !names.insert(entry.target.to_ascii_lowercase()) {
                return Err(format!("Selected packs conflict on {}", entry.target));
            }
            let source = base.join(&relative);
            checked(&source)?;
            if (entry.target.starts_with(".assets/entities/")
                || entry.target.starts_with(".assets/characters/"))
                && entry.target.to_ascii_lowercase().ends_with(".glb")
            {
                continue;
            }
            let document = if entry.target.starts_with(".assets/materials/")
                && entry.target.ends_with(".json")
            {
                Some(local_material(
                    &fs::read(&source).map_err(|e| e.to_string())?,
                )?)
            } else {
                None
            };
            items.push(Item {
                source,
                relative,
                document,
                digest: Some(entry.sha256.clone()),
            });
        }
        for material in &pack.materials {
            let relative = safe_relative(&format!(
                ".assets/materials/{}.json",
                super::material::identifier(&material.name)?
            ))?;
            if !names.insert(relative.to_string_lossy().to_ascii_lowercase()) {
                return Err(format!("Selected packs conflict on {}", relative.display()));
            }
            let source = base.join(&relative);
            checked(&source)?;
            items.push(Item {
                source,
                relative,
                document: Some(local_material(
                    &serde_json::to_vec(&material.document).map_err(|e| e.to_string())?,
                )?),
                digest: None,
            });
        }
        selected_packs.push(pack);
    }
    let mut pending = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err("Asset installation canceled".into());
        }
        let destination = project.join(&item.relative);
        checked(&destination)?;
        if destination.exists() {
            if !destination.is_file() {
                return Err(format!(
                    "Asset path is occupied: {}",
                    item.relative.display()
                ));
            }
        } else {
            pending.push(index);
        }
    }
    let contains_models = selected_packs.iter().any(|pack| {
        pack.entries.iter().any(|entry| {
            (entry.target.starts_with(".assets/entities/")
                || entry.target.starts_with(".assets/characters/"))
                && entry.target.to_ascii_lowercase().ends_with(".glb")
        })
    });
    if pending.is_empty() && !contains_models {
        return Ok(());
    }
    let staging = project
        .parent()
        .ok_or("Project has no parent directory")?
        .join(format!(".asset-pack-stage-{:032x}", rand::random::<u128>()));
    checked(&staging)?;
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let mut created = Vec::new();
    let mut created_dirs = Vec::new();
    let mut native_published: Vec<PathBuf> = Vec::new();
    let raw_total = pending
        .iter()
        .map(|&i| fs::metadata(&items[i].source).map(|m| m.len()).unwrap_or(0))
        .sum::<u64>();
    let model_count = selected_packs
        .iter()
        .flat_map(|pack| pack.entries.iter())
        .filter(|entry| {
            (entry.target.starts_with(".assets/entities/")
                || entry.target.starts_with(".assets/characters/"))
                && entry.target.to_ascii_lowercase().ends_with(".glb")
        })
        .count() as u64;
    let total = raw_total
        .saturating_add(model_count.saturating_mul(1_000_000))
        .max(1);
    let operation = (|| -> Result<(), String> {
        let mut completed = 0u64;
        for &index in &pending {
            if cancel.load(Ordering::Acquire) {
                return Err("Asset installation canceled".into());
            }
            let item = &items[index];
            let destination = staging.join(&item.relative);
            fs::create_dir_all(destination.parent().ok_or("Invalid staged asset path")?)
                .map_err(|e| e.to_string())?;
            if let Some(bytes) = &item.document {
                fs::write(&destination, bytes).map_err(|e| e.to_string())?;
            } else {
                fs::copy(&item.source, &destination).map_err(|e| e.to_string())?;
            }
            if let Some(expected) = &item.digest {
                let (digest, _) = crate::engine::game_package::sha256_reader(
                    &mut File::open(&destination).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if item.document.is_none() && digest != *expected {
                    return Err(format!(
                        "Shared asset checksum mismatch: {}",
                        item.relative.display()
                    ));
                }
            }
            completed = completed
                .saturating_add(fs::metadata(&item.source).map_err(|e| e.to_string())?.len());
            progress(completed, total);
        }
        let native = stage_catalog_models(&staging, root, &selected_packs, cancel, &|done, _| {
            progress(
                raw_total.saturating_add(done.saturating_mul(1_000_000)),
                total,
            );
        })?;
        let mut native_pending = Vec::new();
        for (index, (relative, _)) in native.iter().enumerate() {
            if cancel.load(Ordering::Acquire) {
                return Err("Asset installation canceled".into());
            }
            let destination = project.join(relative);
            checked(&destination)?;
            if destination.exists() {
                if !destination.is_dir() {
                    return Err(format!(
                        "Native package path occupied by a file: {}",
                        relative.display()
                    ));
                }
            } else {
                native_pending.push(index);
            }
        }
        for &index in &pending {
            if cancel.load(Ordering::Acquire) {
                return Err("Asset installation canceled".into());
            }
            let item = &items[index];
            let destination = project.join(&item.relative);
            checked(&destination)?;
            if destination.exists() && !destination.is_file() {
                return Err(format!(
                    "Project asset changed type during installation: {}",
                    item.relative.display()
                ));
            }
            let parent = destination.parent().ok_or("Invalid project asset path")?;
            let mut missing = Vec::new();
            let mut dir = parent;
            while dir != project && !dir.exists() {
                missing.push(dir.to_owned());
                dir = dir.parent().ok_or("Invalid project directory")?;
            }
            for dir in missing.into_iter().rev() {
                checked(&dir)?;
                fs::create_dir(&dir).map_err(|e| e.to_string())?;
                created_dirs.push(dir);
            }
            if destination.exists() {
                return Err(format!(
                    "Project asset appeared during installation; preserved: {}",
                    item.relative.display()
                ));
            }
            created.push(destination.clone());
            fs::rename(staging.join(&item.relative), &destination)
                .map_err(|e| format!("Cannot publish asset {}: {e}", item.relative.display()))?;
        }
        for &index in &native_pending {
            let (relative, _) = &native[index];
            if cancel.load(Ordering::Acquire) {
                return Err("Asset installation canceled".into());
            }
            let destination = project.join(relative);
            checked(&destination)?;
            if destination.exists() && !destination.is_dir() {
                return Err(format!(
                    "Native asset path changed type: {}",
                    relative.display()
                ));
            }
            let parent = destination.parent().ok_or("Invalid native package path")?;
            let mut missing = Vec::new();
            let mut dir = parent;
            while dir != project && !dir.exists() {
                missing.push(dir.to_owned());
                dir = dir.parent().ok_or("Invalid parent")?;
            }
            for dir in missing.into_iter().rev() {
                checked(&dir)?;
                fs::create_dir(&dir).map_err(|e| e.to_string())?;
                created_dirs.push(dir);
            }
            if destination.exists() {
                return Err(format!(
                    "Native package appeared during installation; preserved: {}",
                    relative.display()
                ));
            }
            native_published.push(destination.clone());
            fs::rename(staging.join(relative), &destination).map_err(|e| {
                format!("Cannot publish native package {}: {e}", relative.display())
            })?;
        }
        let groups_path = project.join(".assets/actor_import_groups.json");
        let mut groups = if groups_path.is_file() {
            serde_json::from_slice::<BTreeMap<String, String>>(
                &fs::read(&groups_path).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Cannot read existing Actor groups: {e}"))?
        } else {
            BTreeMap::new()
        };
        let mut changed = false;
        for (relative, group) in &native {
            if relative.starts_with(".assets/entities") {
                let name = relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("Invalid Actor package name")?;
                if !groups.contains_key(name) {
                    groups.insert(name.to_owned(), group.clone());
                    changed = true;
                }
            }
        }
        if changed {
            checked(&groups_path)?;
            super::atomic_file::write_atomically(
                &groups_path,
                &serde_json::to_vec_pretty(&groups).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("Cannot save downloaded Actor groups: {e}"))?;
        }
        super::asset_registry::AssetRegistry::reconcile(project)?;
        Ok(())
    })();
    if let Err(error) = operation {
        for path in native_published.into_iter().rev() {
            let _ = fs::remove_dir_all(path);
        }
        for path in created.into_iter().rev() {
            let _ = fs::remove_file(path);
        }
        for dir in created_dirs.into_iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        let _ = super::asset_registry::AssetRegistry::reconcile(project);
        let _ = fs::remove_dir_all(staging);
        return Err(format!(
            "{error}; newly installed project assets rolled back"
        ));
    }
    let _ = fs::remove_dir_all(staging);
    if contains_models {
        crate::engine::entity_package::reload_packages(Some(project));
        crate::character::package::reload_packages(Some(project));
    }
    Ok(())
}

#[cfg(test)]
mod catalog_version_tests {
    use super::*;
    #[test]
    fn legacy_catalogue_survives_optional_fx_manifest_publication() {
        let releases = serde_json::json!([
            {"draft":true,"assets":[{"name":"assets-manifest-v2.json","id":1}]},
            {"draft":false,"assets":[
                {"name":"assets-manifest.json","id":10},
                {"name":"assets-manifest-v2.json","id":20}]},
            {"draft":false,"assets":[{"name":"assets-manifest.json","id":30}]}
        ]);
        assert_eq!(select_catalog_asset(&releases).unwrap()["id"], 20);
        let older = serde_json::json!([
            {"draft":false,"assets":[{"name":"assets-manifest.json","id":10}]}]);
        assert_eq!(select_catalog_asset(&older).unwrap()["id"], 10);
        let draft_only = serde_json::json!([
            {"draft":true,"assets":[{"name":"assets-manifest-v2.json","id":1}]}]);
        assert!(select_catalog_asset(&draft_only).is_none());
    }
}
#[cfg(test)]
#[path = "asset_packs_tests.rs"]
mod tests;
