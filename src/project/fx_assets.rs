// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::path::{Component, Path, PathBuf};

pub fn valid_asset_id(id: &str) -> bool {
    let path = Path::new(id);
    !id.is_empty()
        && !id.contains('\\')
        && !id.contains(':')
        && !id.chars().any(char::is_control)
        && path.components().count() >= 2
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("efkefc"))
}

fn safe_effect_location(project: &Path, effect: &Path) -> Option<(PathBuf, PathBuf)> {
    let base = project.join(".assets").join("effects");
    let real_project = project.canonicalize().ok()?;
    let real_base = base.canonicalize().ok()?;
    if !real_base.starts_with(real_project) {
        return None;
    }
    let mut item = Some(effect);
    while let Some(path) = item {
        let meta = std::fs::symlink_metadata(path).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return None;
            }
        }
        if path == base {
            break;
        }
        item = path.parent();
    }
    let real_effect = effect.canonicalize().ok()?;
    if !real_effect.starts_with(&real_base) || !real_effect.is_file() {
        return None;
    }
    Some((real_base, real_effect))
}

pub fn id_from_path(project: &Path, effect: &Path) -> Option<String> {
    let (root, effect) = safe_effect_location(project, effect)?;
    let relative = effect.strip_prefix(root).ok()?;
    let id = relative.to_str()?.replace('\\', "/");
    valid_asset_id(&id).then_some(id)
}

pub fn resolve(project: &Path, id: &str) -> Option<PathBuf> {
    if !valid_asset_id(id) {
        return None;
    }
    let source = project.join(".assets").join("effects").join(id);
    safe_effect_location(project, &source).map(|(_, effect)| effect)
}

pub fn materialize_bound_effects(
    project: &Path,
    entries: &[(PathBuf, String)],
) -> Result<(), String> {
    use std::{
        collections::BTreeMap,
        fs,
        io::{Read, Write},
    };
    if entries.is_empty() {
        return Ok(());
    }
    let mut packages: BTreeMap<String, Vec<(PathBuf, PathBuf)>> = BTreeMap::new();
    for (source, target) in entries {
        crate::project::asset_packs::safe_relative(target)?;
        let Some(tail) = target.strip_prefix(".assets/effects/") else {
            return Err("Optional FX target is outside the Effects category".into());
        };
        let Some((folder, relative)) = tail.split_once('/') else {
            return Err("Optional FX assets must be grouped into packages".into());
        };
        if !crate::project::asset_packs::safe_component(folder) {
            return Err("Invalid optional FX package folder".into());
        }
        if relative.is_empty() {
            return Err("Missing FX package resource".into());
        }
        let source_meta = fs::symlink_metadata(source).map_err(|e| e.to_string())?;
        if !source_meta.is_file() || source_meta.file_type().is_symlink() {
            return Err("Optional FX resource is not a regular file".into());
        }
        packages
            .entry(folder.to_owned())
            .or_default()
            .push((source.clone(), PathBuf::from(relative)));
    }
    let base = project.join(".assets/effects");
    for ancestor in base.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() {
                return Err("Linked project FX destination refused".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Reparse project FX destination refused".into());
                }
            }
        }
    }
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;

    fn equal_files(a: &Path, b: &Path) -> Result<bool, String> {
        let left = fs::metadata(a).map_err(|e| e.to_string())?;
        let right = fs::metadata(b).map_err(|e| e.to_string())?;
        if left.len() != right.len() {
            return Ok(false);
        }
        let mut a = fs::File::open(a).map_err(|e| e.to_string())?;
        let mut b = fs::File::open(b).map_err(|e| e.to_string())?;
        let (mut ab, mut bb) = ([0u8; 65536], [0u8; 65536]);
        loop {
            let an = a.read(&mut ab).map_err(|e| e.to_string())?;
            let bn = b.read(&mut bb).map_err(|e| e.to_string())?;
            if an != bn || ab[..an] != bb[..bn] {
                return Ok(false);
            }
            if an == 0 {
                return Ok(true);
            }
        }
    }
    for (folder, assets) in packages {
        if !assets.iter().any(|(_, rel)| {
            rel.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("efkefc"))
        }) {
            return Err(format!("Optional FX pack {folder} has no playable effect"));
        }
        let destination = base.join(&folder);
        if destination.exists() {
            let meta = fs::symlink_metadata(&destination).map_err(|e| e.to_string())?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("Optional FX conflicts with a project asset".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Reparse FX package destination refused".into());
                }
            }
            for (source, rel) in assets {
                let existing = destination.join(rel);
                let existing_meta = fs::symlink_metadata(&existing).map_err(|e| e.to_string())?;
                if !existing_meta.is_file() || existing_meta.file_type().is_symlink() {
                    return Err("Linked FX resource refused".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if existing_meta.file_attributes() & 0x400 != 0 {
                        return Err("Reparse FX resource refused".into());
                    }
                }
                if !equal_files(&existing, &source)? {
                    return Err(format!(
                        "Optional FX conflicts with creator-owned assets: {folder}"
                    ));
                }
            }
            continue;
        }
        let stage = base.join(format!(".fx-install-{:032x}", rand::random::<u128>()));
        fs::create_dir(&stage).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            for (source, rel) in &assets {
                let target = stage.join(rel);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut input = fs::File::open(source).map_err(|e| e.to_string())?;
                let mut output = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)
                    .map_err(|e| e.to_string())?;
                std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
                output.flush().map_err(|e| e.to_string())?;
            }
            if destination.exists() {
                return Err("Optional FX package appeared during install".into());
            }
            fs::rename(&stage, &destination).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_pack_fx_files_materialize_without_overwriting_creator_assets() {
        use std::fs;
        let temp =
            std::env::temp_dir().join(format!("wk-fx003-bind-{:016x}", rand::random::<u64>()));
        let project = temp.join("CreatorProject");
        let source = temp.join("Cache");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(source.join("Texture")).unwrap();
        fs::write(source.join("Fire.efkefc"), b"native FX").unwrap();
        fs::write(source.join("Texture").join("fire.png"), b"texture").unwrap();
        let entries = vec![
            (
                source.join("Fire.efkefc"),
                ".assets/effects/Campfire/Fire.efkefc".into(),
            ),
            (
                source.join("Texture/fire.png"),
                ".assets/effects/Campfire/Texture/fire.png".into(),
            ),
        ];
        materialize_bound_effects(&project, &entries).unwrap();
        materialize_bound_effects(&project, &entries).unwrap();
        let effect = project.join(".assets/effects/Campfire/Fire.efkefc");
        assert_eq!(fs::read(&effect).unwrap(), b"native FX");
        assert_eq!(
            id_from_path(&project, &effect).as_deref(),
            Some("Campfire/Fire.efkefc")
        );
        fs::write(&effect, b"creator modification").unwrap();
        assert!(materialize_bound_effects(&project, &entries).is_err());
        assert_eq!(fs::read(&effect).unwrap(), b"creator modification");
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn selected_catalog_pack_binds_effects_into_project_library() {
        use super::super::asset_packs::{self, Entry, Pack};
        use std::fs;
        let temp =
            std::env::temp_dir().join(format!("wk-fx003-pack-bind-{:016x}", rand::random::<u64>()));
        let cache = temp.join("PackCache");
        let project = temp.join("GameProject");
        fs::create_dir_all(&project).unwrap();
        let pack = Pack {
            id: "fx-starter-fire".into(),
            name: "Starter Fire FX".into(),
            description: "Imported native Fire".into(),
            category: "Effects".into(),
            version: "1.0.0".into(),
            min_schema: 1,
            max_schema: 1,
            min_engine_schema: 1,
            max_engine_schema: 1,
            tag: "asset-packs-v1.4.0".into(),
            asset_id: 1234,
            asset_name: "FireFX.zip".into(),
            bytes: 123,
            sha256: "a".repeat(64),
            license: "CC0-1.0".into(),
            attribution: "Effekseer".into(),
            entries: vec![Entry {
                path: "Effects/StarterFire/Fire.efkefc".into(),
                target: ".assets/effects/StarterFire/Fire.efkefc".into(),
                bytes: 9,
                sha256: "b".repeat(64),
            }],
            materials: Vec::new(),
            dependencies: Vec::new(),
        };
        let install = cache.join(&pack.id).join(&pack.version);
        fs::create_dir_all(install.join(".assets/effects/StarterFire")).unwrap();
        fs::write(
            install.join(".assets/effects/StarterFire/Fire.efkefc"),
            b"test fire",
        )
        .unwrap();
        fs::write(
            install.join("installed-pack.json"),
            serde_json::to_vec_pretty(&pack).unwrap(),
        )
        .unwrap();
        asset_packs::bind(&cache, &project, vec![pack.reference()]).unwrap();
        assert!(resolve(&project, "StarterFire/Fire.efkefc").is_some());
        assert!(project.join("asset-packs.json").is_file());
        asset_packs::bind(&cache, &project, vec![pack.reference()]).unwrap();
        fs::remove_dir_all(temp).unwrap();
    }

    #[test]
    fn optional_catalog_accepts_effects_but_rejects_executables() {
        use super::super::asset_packs::{Catalog, Entry, Pack};
        let pack = Pack {
            id: "campfire-effects".into(),
            name: "Campfire FX".into(),
            description: "A reusable effect".into(),
            category: "Effects".into(),
            version: "1.0.0".into(),
            min_schema: 1,
            max_schema: 1,
            min_engine_schema: 1,
            max_engine_schema: 1,
            tag: "asset-packs-v1".into(),
            asset_id: 123,
            asset_name: "CampfireFX.zip".into(),
            bytes: 2,
            sha256: "a".repeat(64),
            license: "CC0-1.0".into(),
            attribution: "Sample CC0".into(),
            entries: vec![Entry {
                path: "FX/Fire.efkefc".into(),
                target: ".assets/effects/Campfire/Fire.efkefc".into(),
                bytes: 2,
                sha256: "b".repeat(64),
            }],
            materials: Vec::new(),
            dependencies: Vec::new(),
        };
        assert!(
            Catalog {
                schema: 1,
                packs: vec![pack.clone()]
            }
            .validate()
            .is_ok()
        );
        let mut invalid = pack;
        invalid.entries[0].path = "FX/virus.exe".into();
        assert!(
            Catalog {
                schema: 1,
                packs: vec![invalid]
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn rejects_unsafe_effect_asset_references() {
        assert!(valid_asset_id("Campfire/Fire.efkefc"));
        assert!(valid_asset_id("Magic/Glow/Blue.efkefc"));
        for id in [
            "",
            "../fire.efkefc",
            "C:/evil.efkefc",
            "Fire.efkefc",
            "pack/../../bad.efkefc",
            r"pack\effect.efkefc",
            "pack/sound.wav",
        ] {
            assert!(!valid_asset_id(id), "must reject {id:?}");
        }
    }
}
