// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
pub struct MaterialMap {
    pub map: String,
    pub path: String,
    pub resolution: [u32; 2],
    pub size_bytes: u64,
    #[serde(default)]
    pub md5: String,
    #[serde(default)]
    pub source_url: String,
}
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryMaterial {
    pub name: String,
    pub category: String,
    pub asset_id: String,
    pub asset_url: String,
    pub license: String,
    pub files: Vec<MaterialMap>,
}
#[derive(Debug, Clone, Deserialize)]
struct SourceCatalog {
    materials: Vec<LibraryMaterial>,
}
#[derive(Debug, Clone)]
pub struct MaterialLibrary {
    pub root: PathBuf,
    pub materials: BTreeMap<String, LibraryMaterial>,
}
#[derive(Debug)]
pub enum MaterialLibraryError {
    Io(String),
    InvalidManifest(String),
}
impl std::fmt::Display for MaterialLibraryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for MaterialLibraryError {}

fn invalid(message: impl Into<String>) -> MaterialLibraryError {
    MaterialLibraryError::InvalidManifest(message.into())
}
impl MaterialLibrary {
    pub fn default_root() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA").map(|p| {
            PathBuf::from(p)
                .join("Aeowun")
                .join("Worldkiln")
                .join("MaterialLibrary")
                .join("4K_CC0")
        })
    }
    pub fn load(root: impl AsRef<Path>) -> Result<Self, MaterialLibraryError> {
        let root = root.as_ref().to_path_buf();
        let manifest = root.join("Worldkiln/MATERIAL_SOURCES.json");
        let source = fs::read(&manifest).map_err(|e| MaterialLibraryError::Io(e.to_string()))?;
        let catalog: SourceCatalog =
            serde_json::from_slice(&source).map_err(|e| invalid(format!("manifest JSON: {e}")))?;
        let mut materials = BTreeMap::new();
        for m in catalog.materials {
            if m.name.trim().is_empty() || m.files.is_empty() || m.license != "CC0-1.0" {
                return Err(invalid(format!("invalid material {}", m.name)));
            }
            let mut seen = std::collections::BTreeSet::new();
            for map in &m.files {
                let relative = Path::new(&map.path);
                if relative.is_absolute()
                    || relative
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    || !map.path.starts_with("Worldkiln/")
                {
                    return Err(invalid(format!("unsafe map path {}", map.path)));
                }
                if map.resolution != [4096, 4096]
                    || !["BaseColor", "NormalGL", "Roughness", "Height", "AO", "ARM"]
                        .contains(&map.map.as_str())
                    || !seen.insert(map.map.as_str())
                {
                    return Err(invalid(format!("bad PBR texture {}", map.path)));
                }
                let path = root.join(relative);
                let meta = fs::metadata(&path)
                    .map_err(|e| MaterialLibraryError::Io(format!("{}: {e}", path.display())))?;
                if meta.len() != map.size_bytes || meta.len() < 10000 {
                    return Err(invalid(format!("size mismatch {}", map.path)));
                }
                use std::io::Read;
                let mut file =
                    fs::File::open(&path).map_err(|e| MaterialLibraryError::Io(e.to_string()))?;
                let mut header = [0u8; 24];
                file.read_exact(&mut header)
                    .map_err(|e| MaterialLibraryError::Io(e.to_string()))?;
                if &header[..8] != b"\x89PNG\r\n\x1a\n"
                    || u32::from_be_bytes(header[16..20].try_into().unwrap()) != 4096
                    || u32::from_be_bytes(header[20..24].try_into().unwrap()) != 4096
                {
                    return Err(invalid(format!("not a real 4K PNG {}", map.path)));
                }
            }
            if !seen.contains("BaseColor") {
                return Err(invalid(format!("base color missing {}", m.name)));
            }
            if materials.insert(m.name.clone(), m).is_some() {
                return Err(invalid("duplicate material name"));
            }
        }
        Ok(Self { root, materials })
    }
    pub fn map_path(&self, material: &str, map: &str) -> Option<PathBuf> {
        self.materials
            .get(material)?
            .files
            .iter()
            .find(|v| v.map == map)
            .map(|v| self.root.join(&v.path))
    }
}

pub fn landscape_pbr_name(value: &str, role: &str) -> String {
    let key = value.strip_prefix("PBR:").unwrap_or(value).trim();
    let mapped = match key.to_ascii_lowercase().as_str() {
        "grass" | "grass_1" | "surface" => "Grass",
        "dirt" | "soil" | "subsurface" => "Dirt",
        "rock" | "tile_24" | "stone" => "Rock",
        "sand" | "beach" => "Sand",
        "snow" => "Snow",
        "mud" => "Mud",
        other if other == "water" => "Water",
        _ => match role {
            "Surface" => "Grass",
            "Subsurface" => "Dirt",
            "Beach" => "Sand",
            _ => "Rock",
        },
    };
    let final_name = if ["Grass", "Dirt", "Rock", "Sand", "Snow", "Mud"]
        .iter()
        .any(|v| v.eq_ignore_ascii_case(key))
    {
        key.to_ascii_lowercase()
    } else {
        mapped.to_ascii_lowercase()
    };
    format!(
        "PBR:{}",
        match final_name.as_str() {
            "grass" => "Grass",
            "dirt" => "Dirt",
            "rock" => "Rock",
            "sand" => "Sand",
            "snow" => "Snow",
            "mud" => "Mud",
            _ => mapped,
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn installed_4k_cc0_pack_has_all_six_materials_and_maps() {
        let Some(root) = MaterialLibrary::default_root() else {
            return;
        };
        if !root.join("Worldkiln/MATERIAL_SOURCES.json").is_file() {
            eprintln!("Optional 4K material pack not installed on this machine");
            return;
        }
        let pack = MaterialLibrary::load(root).unwrap();
        assert_eq!(pack.materials.len(), 6);
        for name in ["Grass", "Rock", "Snow", "Mud", "Sand", "Dirt"] {
            let material = pack.materials.get(name).unwrap();
            assert_eq!(material.files.len(), 6);
            assert!(pack.map_path(name, "BaseColor").unwrap().exists());
            assert!(pack.map_path(name, "NormalGL").unwrap().exists());
            assert!(pack.map_path(name, "Roughness").unwrap().exists());
        }
    }
    #[test]
    fn nonexistent_library_is_a_reported_error_not_a_crash() {
        assert!(MaterialLibrary::load("worldkiln-missing-asset-folder-404").is_err());
    }
}
