// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const CHARACTER_ASSET_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterAsset {
    pub version: u32,
    pub name: String,
    pub source_actor: String,
    #[serde(default = "unit_scale")]
    pub scale: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<String>,
}

fn unit_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn safe_component(value: &str, label: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 {
        return Err(format!("{label} must contain 1 to 128 characters."));
    }
    let path = Path::new(value);
    let mut components = path.components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || value.ends_with('.')
        || value.chars().any(|c| "\\/:*?\"<>|".contains(c))
    {
        return Err(format!("{label} must be one valid project asset name."));
    }
    Ok(value.to_owned())
}

impl CharacterAsset {
    pub fn validate(&self, project: &Path) -> Result<(), String> {
        if self.version != CHARACTER_ASSET_VERSION {
            return Err("Unsupported Character asset version.".into());
        }
        safe_component(&self.name, "Character name")?;
        let actor = safe_component(&self.source_actor, "Source Actor")?;
        let actor_dir =
            crate::project::asset_registry::AssetRegistry::actor_directory(project, &actor)?;
        if !actor_dir.join("source.glb").is_file()
            || !actor_dir.join("character-package.json").is_file()
        {
            return Err("Source Actor is not a rigged Character-capable Actor.".into());
        }
        if self
            .scale
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0 || *v > 100.0)
        {
            return Err("Character scale must be between 0 and 100 on every axis.".into());
        }
        if let Some(material) = self.material.as_deref().filter(|v| !v.trim().is_empty()) {
            crate::project::material::read(project, material)
                .map_err(|error| format!("Character Material is invalid: {error}"))?;
        }
        validate_logic(
            project,
            self.controller.as_deref(),
            "controllers",
            "Controller",
        )?;
        validate_logic(project, self.camera.as_deref(), "cameras", "Camera")?;
        Ok(())
    }
}

fn validate_logic(
    project: &Path,
    value: Option<&str>,
    folder: &str,
    label: &str,
) -> Result<(), String> {
    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return Ok(());
    };
    let name = safe_component(value.trim_end_matches(".aeo"), label)?;
    if let Ok(registry) = crate::project::asset_registry::AssetRegistry::load(project) {
        if let Some(asset) = registry.find(&name) {
            let kind = if folder == "cameras" {
                crate::project::asset_registry::AssetType::Camera
            } else {
                crate::project::asset_registry::AssetType::Controller
            };
            if asset.kind != kind {
                return Err(format!("{label} has incorrect asset type"));
            }
            if std::fs::read_dir(project.join(&asset.path))
                .ok()
                .is_some_and(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.path().extension().is_some_and(|x| x == "aeo"))
                })
            {
                return Ok(());
            }
            return Err(format!("{label} asset has no script"));
        }
    }
    let roots = vec![project.join(".assets").join(folder)];
    let exists = roots.iter().any(|root| {
        root.join(format!("{name}.aeo")).is_file()
            || std::fs::read_dir(root.join(&name))
                .ok()
                .is_some_and(|entries| {
                    entries.filter_map(Result::ok).any(|entry| {
                        entry.file_type().is_ok_and(|kind| kind.is_file())
                            && entry.path().extension().is_some_and(|ext| ext == "aeo")
                    })
                })
    });
    if !exists {
        return Err(format!("{label} asset '{name}' was not found."));
    }
    Ok(())
}

pub fn directory(project: &Path, name: &str) -> Result<PathBuf, String> {
    Ok(project
        .join(".assets/characters")
        .join(safe_component(name, "Character name")?))
}

pub fn load(project: &Path, name: &str) -> Result<CharacterAsset, String> {
    let path = directory(project, name)?.join("package.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("Could not read Character '{}': {error}", path.display()))?;
    let asset: CharacterAsset = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Could not parse Character '{}': {error}", path.display()))?;
    asset.validate(project)?;
    Ok(asset)
}

pub fn directory_for_reference(project: &Path, reference: &str) -> Result<PathBuf, String> {
    if let Ok(registry) = crate::project::asset_registry::AssetRegistry::load(project) {
        if let Some(record) = registry.find(reference) {
            if record.kind != crate::project::asset_registry::AssetType::Character {
                return Err("AssetID is not a Character".into());
            }
            return Ok(project.join(&record.path));
        }
    }
    directory(project, reference)
}
pub fn load_reference(project: &Path, reference: &str) -> Result<CharacterAsset, String> {
    let path = directory_for_reference(project, reference)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid Character package path")?;
    load(project, name)
}

pub fn resolve_player_logic(
    project: &Path,
    selected_character: &str,
    world_controller: &str,
    world_camera: &str,
) -> (String, String) {
    if selected_character.trim().is_empty() || selected_character.eq_ignore_ascii_case("None") {
        return (String::new(), String::new());
    }
    let character = load_reference(project, selected_character).ok();
    let resolve = |kind, selected: Option<&str>, fallback: &str| {
        let reference = selected
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback);
        if reference.trim().is_empty() || reference.eq_ignore_ascii_case("None") {
            String::new()
        } else {
            crate::project::asset_registry::AssetRegistry::native_logic_reference(
                project, kind, reference,
            )
        }
    };
    (
        resolve(
            crate::project::asset_registry::AssetType::Controller,
            character
                .as_ref()
                .and_then(|asset| asset.controller.as_deref()),
            world_controller,
        ),
        resolve(
            crate::project::asset_registry::AssetType::Camera,
            character.as_ref().and_then(|asset| asset.camera.as_deref()),
            world_camera,
        ),
    )
}

pub fn assign_to_world(
    project: &Path,
    name: &str,
    world: &mut crate::world::World,
) -> Result<(), String> {
    let asset = load_reference(project, name)?;
    let id = crate::project::asset_registry::AssetRegistry::load(project)
        .ok()
        .and_then(|registry| {
            registry
                .assets
                .iter()
                .find(|record| {
                    record.kind == crate::project::asset_registry::AssetType::Character
                        && std::path::Path::new(&record.path)
                            .file_name()
                            .is_some_and(|component| component == std::ffi::OsStr::new(&asset.name))
                })
                .map(|record| record.id.clone())
        });
    world.selected_character = id.unwrap_or_else(|| asset.name.clone());
    world.player_character_scale = glam::Vec3::from_array(asset.scale);
    world.selected_controller = asset
        .controller
        .map(|name| {
            crate::project::asset_registry::AssetRegistry::native_logic_asset_id(
                project,
                crate::project::asset_registry::AssetType::Controller,
                &name,
            )
        })
        .unwrap_or_default();
    world.selected_camera = asset
        .camera
        .map(|name| {
            crate::project::asset_registry::AssetRegistry::native_logic_asset_id(
                project,
                crate::project::asset_registry::AssetType::Camera,
                &name,
            )
        })
        .unwrap_or_default();
    Ok(())
}

pub fn save(
    project: &Path,
    original_name: Option<&str>,
    asset: &CharacterAsset,
) -> Result<PathBuf, String> {
    asset.validate(project)?;
    let target = directory(project, &asset.name)?;
    if let Some(original) = original_name {
        let original_dir = directory(project, original)?;
        if original_dir != target && target.exists() {
            return Err(format!(
                "A Character named '{}' already exists.",
                asset.name
            ));
        }
        if original_dir != target && original_dir.exists() {
            std::fs::rename(&original_dir, &target)
                .map_err(|error| format!("Could not rename Character: {error}"))?;
        }
    } else if target.exists() {
        return Err(format!(
            "A Character named '{}' already exists.",
            asset.name
        ));
    }
    std::fs::create_dir_all(&target)
        .map_err(|error| format!("Could not create Character directory: {error}"))?;
    let bytes = serde_json::to_vec_pretty(asset)
        .map_err(|error| format!("Could not serialize Character: {error}"))?;
    let package_file = target.join("package.json");
    if package_file.exists() {
        crate::project::atomic_file::write_atomically(&package_file, &bytes)
            .map_err(|error| format!("Could not save Character: {error}"))?;
    } else {
        std::fs::write(&package_file, &bytes)
            .map_err(|error| format!("Could not save Character: {error}"))?;
    }
    if let Some(previous) = original_name {
        let old_path = directory(project, previous)?;
        crate::project::asset_registry::AssetRegistry::moved(project, &old_path, &target)?;
    }
    crate::character::package::reload_packages(Some(project));
    crate::project::asset_registry::AssetRegistry::reconcile(project)?;
    Ok(target)
}

pub fn is_explicit_character_dir(path: &Path) -> bool {
    std::fs::read(path.join("package.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CharacterAsset>(&bytes).ok())
        .is_some_and(|asset| {
            asset.version == CHARACTER_ASSET_VERSION && !asset.source_actor.is_empty()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_uses_project_scoped_camera_and_controller_package_scripts() {
        let root = std::env::temp_dir().join(format!(
            "wk_character_native_pack_{:032x}",
            rand::random::<u128>()
        ));
        let other = root.join("Other");
        let project = root.join("Selected");
        for dir in [&project, &other] {
            let actor = dir.join(".assets/entities/RiggedHero");
            std::fs::create_dir_all(&actor).unwrap();
            std::fs::write(actor.join("source.glb"), b"model").unwrap();
            std::fs::write(actor.join("character-package.json"), b"{}").unwrap();
        }
        let camera = project.join(".assets/cameras/topDown");
        let controller = project.join(".assets/controllers/thirdPerson_Controller");
        std::fs::create_dir_all(&camera).unwrap();
        std::fs::create_dir_all(&controller).unwrap();
        std::fs::write(camera.join("topDownCamera.aeo"), "entity TopDownCamera {}").unwrap();
        std::fs::write(
            controller.join("thirdPersonController.aeo"),
            "entity ThirdPersonController {}",
        )
        .unwrap();
        let character = CharacterAsset {
            version: 1,
            name: "Hero".into(),
            source_actor: "RiggedHero".into(),
            scale: [1.0; 3],
            material: None,
            controller: Some("thirdPerson_Controller".into()),
            camera: Some("topDown".into()),
        };
        assert!(character.validate(&project).is_ok());
        assert!(
            character
                .validate(&other)
                .unwrap_err()
                .contains("Controller asset")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_character_save_increases_only_character_assets_without_copying_glb() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_character_asset_{:032x}",
            rand::random::<u128>()
        ));
        let actor = root.join(".assets/entities/HeroActor");
        std::fs::create_dir_all(&actor).unwrap();
        std::fs::write(actor.join("source.glb"), b"source").unwrap();
        std::fs::write(
            actor.join("character-package.json"),
            br#"{"name":"HeroActor"}"#,
        )
        .unwrap();
        let asset = CharacterAsset {
            version: 1,
            name: "Hero".into(),
            source_actor: "HeroActor".into(),
            scale: [1.0; 3],
            material: None,
            controller: None,
            camera: None,
        };
        let saved = save(&root, None, &asset).unwrap();
        assert!(saved.join("package.json").is_file());
        assert!(!saved.join("source.glb").exists());
        assert_eq!(load(&root, "Hero").unwrap(), asset);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn character_assignment_and_attached_logic_survive_world_save_reopen() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_character_assignment_{:032x}",
            rand::random::<u128>()
        ));
        let actor = root.join(".assets/entities/HeroActor");
        std::fs::create_dir_all(&actor).unwrap();
        std::fs::write(actor.join("source.glb"), b"source").unwrap();
        std::fs::write(
            actor.join("character-package.json"),
            br#"{"name":"HeroActor"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join(".assets/controllers")).unwrap();
        std::fs::create_dir_all(root.join(".assets/cameras")).unwrap();
        std::fs::write(
            root.join(".assets/controllers/HeroController.aeo"),
            "entity HeroController {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".assets/cameras/HeroCamera.aeo"),
            "entity HeroCamera {}\n",
        )
        .unwrap();
        let asset = CharacterAsset {
            version: 1,
            name: "Hero".into(),
            source_actor: "HeroActor".into(),
            scale: [0.8, 0.9, 1.0],
            material: None,
            controller: Some("HeroController".into()),
            camera: Some("HeroCamera".into()),
        };
        save(&root, None, &asset).unwrap();
        let mut world = crate::world::World::new();
        assign_to_world(&root, "Hero", &mut world).unwrap();
        let world_file = root.join("world.dat");
        crate::world::persistence::save_world(&world, &world_file).unwrap();
        let mut reopened = crate::world::World::new();
        crate::world::persistence::load_world(&mut reopened, &world_file).unwrap();
        assert_eq!(reopened.selected_character, world.selected_character);
        assert_eq!(reopened.selected_controller, world.selected_controller);
        assert_eq!(reopened.selected_camera, world.selected_camera);
        assert_eq!(
            reopened.player_character_scale,
            glam::Vec3::new(0.8, 0.9, 1.0)
        );
        std::fs::remove_dir_all(root).ok();
    }
}
