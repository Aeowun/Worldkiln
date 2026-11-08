// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::animation::{AnimationClip, CharacterAnimationController, Keyframe, TransformTrack};
use super::character::AppearanceCustomization;
use super::collision::CharacterCollision;
use super::rig::{Joint, Skeleton, Transform};
use crate::project::engine_assets_dir;
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

pub const DEFAULT_CHARACTER_PACKAGE: &str = "Rogue";

#[derive(Clone, Debug)]
pub struct PackageMetadata {
    pub name: String,
    pub aliases: Vec<String>,
    pub mesh_type: String,
    pub has_gun: bool,
    pub default_animation: Option<String>,
    pub attachment_points: HashMap<String, String>,
    pub attachment_rotations: HashMap<String, Quat>,
}

#[derive(Clone, Debug)]
pub struct BlendSettings {
    pub default_state: String,
    pub blend_speed: f32,
    pub idle_clip: String,
    pub walk_clip: String,
    pub run_clip: Option<String>,
    pub run_threshold: f32,
}

#[derive(Clone, Debug)]
pub struct GeometryPart {
    pub base_color_texture: Option<PathBuf>,
    pub joint_index: usize,
    pub material: String,
    pub when: Option<String>,
    pub primitive: GeometryPrimitive,
}

#[derive(Clone, Debug)]
pub enum GeometryPrimitive {
    SkinnedTriangleMesh {
        vertices: Vec<SkinnedVertex>,
        inverse_bind_matrices: Vec<Mat4>,
    },
    Cube {
        min: Vec3,
        max: Vec3,
    },
    Cylinder {
        radius_x: f32,
        radius_z: f32,
        min_y: f32,
        max_y: f32,
        segments: usize,
    },
    Ellipsoid {
        center: Vec3,
        radius_x: f32,
        radius_y: f32,
        radius_z: f32,
        segments: usize,
        rings: usize,
    },
    TaperedFlame {
        radius_bottom: f32,
        radius_top: f32,
        min_y: f32,
        max_y: f32,
        segments: usize,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct SkinnedVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coord: Vec2,
    pub color: [f32; 4],
    pub joints: [usize; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct CharacterPackage {
    pub metadata: PackageMetadata,
    pub source_actor: Option<String>,
    pub scale: Vec3,
    pub material: Option<String>,
    pub controller: Option<String>,
    pub camera: Option<String>,
    pub appearance: AppearanceCustomization,
    pub collision: CharacterCollision,
    pub skeleton: Skeleton,
    pub clips: Vec<AnimationClip>,
    pub blend: BlendSettings,
    pub geometry: Vec<GeometryPart>,
    rest_pose_bounds_cache: OnceLock<Option<(Vec3, Vec3)>>,
}

impl CharacterPackage {
    pub fn rest_pose_bounds(&self) -> Option<(Vec3, Vec3)> {
        *self.rest_pose_bounds_cache.get_or_init(|| {
            let mut controller = CharacterAnimationController::from_package(self);
            let pose = controller.evaluate_pose();
            let vertices = super::geometry::generate_mesh_from_parts(
                &self.geometry,
                &pose,
                &self.appearance,
                self.metadata.has_gun,
            );
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for vertex in vertices.chunks_exact(10) {
                let position = Vec3::new(vertex[0], vertex[1], vertex[2]);
                min = min.min(position);
                max = max.max(position);
            }
            (min.is_finite() && max.is_finite()).then_some((min, max))
        })
    }

    pub fn clip_named(&self, name: &str) -> Option<&AnimationClip> {
        self.clips
            .iter()
            .find(|clip| clip.name.eq_ignore_ascii_case(name))
    }

    pub fn attachment_joint_index(&self, point_name: &str) -> Option<usize> {
        let joint_name = self
            .metadata
            .attachment_points
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(point_name))
            .map(|(_, joint)| joint.as_str())
            .unwrap_or(point_name);
        self.skeleton.joint_index(joint_name)
    }

    pub fn attachment_rotation(&self, point_name: &str) -> Quat {
        self.metadata
            .attachment_rotations
            .iter()
            .find(|(alias, _)| alias.eq_ignore_ascii_case(point_name))
            .map(|(_, rotation)| *rotation)
            .unwrap_or(Quat::IDENTITY)
    }
}

fn built_in_default_character() -> Arc<CharacterPackage> {
    static BUILT_IN: OnceLock<Arc<CharacterPackage>> = OnceLock::new();
    BUILT_IN
        .get_or_init(|| {
            let joint = |name: &str, parent_index: Option<usize>, translation: Vec3| Joint {
                name: name.to_owned(),
                parent_index,
                local_transform: Transform {
                    translation,
                    ..Transform::default()
                },
            };
            let skeleton = Skeleton::new(vec![
                joint("Root", None, Vec3::ZERO),
                joint("LeftHand", Some(0), Vec3::new(-0.32, 0.48, 0.0)),
                joint("RightHand", Some(0), Vec3::new(0.32, 0.48, 0.0)),
            ]);
            let part = |joint_index: usize, material: &str, min: Vec3, max: Vec3| GeometryPart {
                base_color_texture: None,
                joint_index,
                material: material.to_owned(),
                when: None,
                primitive: GeometryPrimitive::Cube { min, max },
            };
            let geometry = vec![
                part(
                    0,
                    "cloth",
                    Vec3::new(-0.2, 0.3, -0.12),
                    Vec3::new(0.2, 0.73, 0.12),
                ),
                part(
                    0,
                    "skin",
                    Vec3::new(-0.13, 0.75, -0.13),
                    Vec3::new(0.13, 0.98, 0.13),
                ),
                part(
                    0,
                    "cloth",
                    Vec3::new(-0.19, 0.04, -0.09),
                    Vec3::new(-0.04, 0.3, 0.09),
                ),
                part(
                    0,
                    "cloth",
                    Vec3::new(0.04, 0.04, -0.09),
                    Vec3::new(0.19, 0.3, 0.09),
                ),
                part(
                    0,
                    "skin",
                    Vec3::new(-0.39, 0.38, -0.08),
                    Vec3::new(-0.22, 0.7, 0.08),
                ),
                part(
                    0,
                    "skin",
                    Vec3::new(0.22, 0.38, -0.08),
                    Vec3::new(0.39, 0.7, 0.08),
                ),
            ];
            let clips = ["Idle", "Walk", "Run"]
                .into_iter()
                .map(|name| AnimationClip {
                    name: name.to_owned(),
                    duration: 1.0,
                    looping: true,
                    tracks: Vec::new(),
                    events: Vec::new(),
                })
                .collect();
            Arc::new(CharacterPackage {
                metadata: PackageMetadata {
                    name: DEFAULT_CHARACTER_PACKAGE.to_owned(),
                    aliases: Vec::new(),
                    mesh_type: "character".to_owned(),
                    has_gun: false,
                    default_animation: Some("Idle".to_owned()),
                    attachment_points: HashMap::from([
                        ("LeftHand".to_owned(), "LeftHand".to_owned()),
                        ("RightHand".to_owned(), "RightHand".to_owned()),
                    ]),
                    attachment_rotations: HashMap::new(),
                },
                source_actor: None,
                scale: Vec3::ONE,
                material: None,
                controller: None,
                camera: None,
                appearance: AppearanceCustomization::default(),
                collision: CharacterCollision {
                    radius: 0.46,
                    height: 1.0,
                },
                skeleton,
                clips,
                blend: BlendSettings {
                    default_state: "Idle".to_owned(),
                    blend_speed: 4.0,
                    idle_clip: "Idle".to_owned(),
                    walk_clip: "Walk".to_owned(),
                    run_clip: Some("Run".to_owned()),
                    run_threshold: 2.0,
                },
                geometry,
                rest_pose_bounds_cache: OnceLock::new(),
            })
        })
        .clone()
}

#[derive(Clone, Debug, Default)]
pub struct CharacterPackageRegistry {
    packages: HashMap<String, Arc<CharacterPackage>>,
    by_asset_id: HashMap<String, Arc<CharacterPackage>>,
}

impl CharacterPackageRegistry {
    pub fn load(project_path: Option<&Path>) -> Self {
        let mut packages = HashMap::new();

        let roots = if let Some(project) = project_path {
            vec![project.join(".assets/characters")]
        } else {
            vec![engine_assets_dir().join("characters")]
        };

        for root in roots {
            load_packages_from_root(&root, &mut packages);
        }

        if !packages.contains_key(DEFAULT_CHARACTER_PACKAGE) {
            register_package_aliases(
                &mut packages,
                DEFAULT_CHARACTER_PACKAGE,
                built_in_default_character(),
            );
        }

        if let Some(project) = project_path {
            append_project_animation_overrides(project, &mut packages);
        }

        let mut by_asset_id = HashMap::new();
        if let Some(project) = project_path {
            if let Ok(registry) = crate::project::asset_registry::AssetRegistry::load(project) {
                for asset in registry.by_type(crate::project::asset_registry::AssetType::Character)
                {
                    if !project.join(&asset.path).is_dir() {
                        continue;
                    }
                    let name = std::path::Path::new(&asset.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if let Some(pkg) = packages.get(name) {
                        by_asset_id.insert(asset.id.clone(), pkg.clone());
                    }
                }
            }
        }
        Self {
            packages,
            by_asset_id,
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<CharacterPackage>> {
        self.packages
            .get(name)
            .or_else(|| self.by_asset_id.get(name))
            .cloned()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .packages
            .values()
            .map(|package| package.metadata.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn contains(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    pub fn values(&self) -> impl Iterator<Item = &Arc<CharacterPackage>> {
        self.packages.values()
    }

    pub fn first(&self) -> Option<Arc<CharacterPackage>> {
        let mut names = self.names();
        if names.is_empty() {
            return None;
        }
        self.get(&names.remove(0))
    }

    pub fn resolve(&self, name: &str) -> Option<Arc<CharacterPackage>> {
        self.get(name)
    }
}

fn append_project_animation_overrides(
    project: &Path,
    packages: &mut HashMap<String, Arc<CharacterPackage>>,
) {
    let Ok(registry) = crate::project::asset_registry::AssetRegistry::load(project) else {
        return;
    };
    for record in registry.by_type(crate::project::asset_registry::AssetType::Animation) {
        let path = project
            .join(&record.path)
            .join(crate::animation::asset::ANIMATION_FILE);
        let Ok(asset) = crate::animation::AnimationAsset::load(&path) else {
            continue;
        };
        let Some(source) = registry.find(&asset.source.rig_asset_id) else {
            continue;
        };
        let source_name = Path::new(&source.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let matching = packages
            .iter()
            .filter_map(|(key, package)| {
                let applies = match source.kind {
                    crate::project::asset_registry::AssetType::Character => {
                        key.eq_ignore_ascii_case(source_name)
                            || package.metadata.name.eq_ignore_ascii_case(source_name)
                    }
                    crate::project::asset_registry::AssetType::Actor => {
                        package.source_actor.as_deref().is_some_and(|actor| {
                            actor.eq_ignore_ascii_case(&source.id)
                                || actor.eq_ignore_ascii_case(source_name)
                        })
                    }
                    _ => false,
                };
                applies.then_some(key.clone())
            })
            .collect::<Vec<_>>();
        for key in matching {
            let Some(package) = packages.get_mut(&key) else {
                continue;
            };
            let package = Arc::make_mut(package);
            let Some(clip) = legacy_clip_from_project_asset(&asset, &package.skeleton) else {
                continue;
            };
            if let Some(existing) = package
                .clips
                .iter()
                .position(|item| item.name.eq_ignore_ascii_case(&clip.name))
            {
                package.clips[existing] = clip;
            } else {
                package.clips.push(clip);
            }
        }
    }
}

fn legacy_clip_from_project_asset(
    asset: &crate::animation::AnimationAsset,
    skeleton: &Skeleton,
) -> Option<AnimationClip> {
    let mut tracks = Vec::new();
    for source in &asset.tracks {
        let Some(joint_index) = skeleton.joints.iter().position(|joint| {
            joint.name.eq_ignore_ascii_case(&source.joint_id)
                || joint.name.eq_ignore_ascii_case(&source.joint_name)
        }) else {
            continue;
        };
        tracks.push(TransformTrack {
            joint_index,
            translations: source
                .translation
                .iter()
                .map(|key| Keyframe {
                    time: key.time,
                    value: Vec3::from_array(key.value),
                    step: key.interpolation == crate::animation::Interpolation::Step,
                })
                .collect(),
            rotations: source
                .rotation
                .iter()
                .map(|key| Keyframe {
                    time: key.time,
                    value: Quat::from_array(key.value).normalize(),
                    step: key.interpolation == crate::animation::Interpolation::Step,
                })
                .collect(),
            scales: source
                .scale
                .iter()
                .map(|key| Keyframe {
                    time: key.time,
                    value: Vec3::from_array(key.value),
                    step: key.interpolation == crate::animation::Interpolation::Step,
                })
                .collect(),
        });
    }
    Some(AnimationClip {
        name: asset.name.clone(),
        duration: asset.duration,
        looping: asset.looping,
        tracks,
        events: asset.events.clone(),
    })
}

static REGISTRY: OnceLock<RwLock<CharacterPackageRegistry>> = OnceLock::new();

fn registry_lock() -> &'static RwLock<CharacterPackageRegistry> {
    REGISTRY.get_or_init(|| RwLock::new(CharacterPackageRegistry::load(None)))
}

pub fn package_registry() -> CharacterPackageRegistry {
    registry_lock()
        .read()
        .expect("character package registry")
        .clone()
}

pub(crate) fn publish_packages(prepared: CharacterPackageRegistry) {
    *REGISTRY
        .get_or_init(|| RwLock::new(CharacterPackageRegistry::default()))
        .write()
        .expect("package registry") = prepared;
}
pub fn reload_packages(project_path: Option<&Path>) {
    let mut registry = registry_lock().write().expect("character package registry");
    *registry = CharacterPackageRegistry::load(project_path);
}

pub fn get_package(name: &str) -> Option<Arc<CharacterPackage>> {
    registry_lock()
        .read()
        .expect("character package registry")
        .get(name)
}

pub fn resolve_package(requested: &str) -> Option<Arc<CharacterPackage>> {
    registry_lock()
        .read()
        .expect("character package registry")
        .resolve(requested)
}

pub fn available_package_names() -> Vec<String> {
    registry_lock()
        .read()
        .expect("character package registry")
        .names()
}

pub fn load_package(dir: &Path) -> Result<CharacterPackage, String> {
    let authored: PackageMetadataFile = read_json(dir, "package.json")?;
    let source_dir = if let Some(source_actor) = authored.source_actor.as_deref() {
        let project = dir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or_else(|| {
                "Character asset is outside a project .assets/characters folder".to_string()
            })?;
        let source =
            crate::project::asset_registry::AssetRegistry::actor_directory(project, source_actor)?;
        if !crate::project::character_asset::is_explicit_character_dir(dir)
            || !source.join("character-package.json").is_file()
        {
            return Err(format!(
                "Character source Actor '{source_actor}' is unavailable"
            ));
        }
        source
    } else {
        dir.to_owned()
    };
    let metadata: PackageMetadataFile = if authored.source_actor.is_some() {
        read_json(&source_dir, "character-package.json")?
    } else {
        read_json(dir, "package.json")?
    };
    let appearance_file: AppearanceFile = read_json(&source_dir, "appearance.json")?;
    let collision_file: CollisionFile = read_json(&source_dir, "collision.json")?;
    let rig_file: RigFile = read_json(&source_dir, "rig.json")?;
    let animation_file: AnimationFile = read_json(&source_dir, "animation.json")?;
    let blend_file: BlendFile = read_json(&source_dir, "blend.json")?;
    let geometry_file: GeometryFile = read_json(&source_dir, "geometry.json")?;

    let skeleton = skeleton_from_file(&rig_file)?;
    for (alias, joint) in &metadata.attachment_points {
        if alias.trim().is_empty() || skeleton.joint_index(joint).is_none() {
            return Err(format!(
                "attachment point '{}' references unknown joint '{}'",
                alias, joint
            ));
        }
    }
    let attachment_rotations = metadata
        .attachment_rotations
        .into_iter()
        .map(|(alias, rotation)| {
            if !metadata
                .attachment_points
                .keys()
                .any(|point| point.eq_ignore_ascii_case(&alias))
            {
                return Err(format!(
                    "attachment rotation '{}' has no matching attachment point",
                    alias
                ));
            }
            let rotation = Quat::from_array(rotation);
            if !rotation.is_finite() || rotation.length_squared() <= f32::EPSILON {
                return Err(format!("attachment rotation '{}' is invalid", alias));
            }
            Ok((alias, rotation.normalize()))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    let clips = clips_from_file(&animation_file, &skeleton)?;
    let geometry = geometry_from_file(&geometry_file, &skeleton, &source_dir)?;
    let mut appearance = appearance_file.into_appearance();
    if let (Some(material), Some(project)) = (
        authored.material.as_deref(),
        dir.parent().and_then(Path::parent).and_then(Path::parent),
    ) {
        if let Ok(document) = crate::project::material::read(project, material) {
            if let Ok(crate::project::material::MaterialOutput::SurfacePbr { base_color, .. }) =
                crate::project::material::MaterialCatalog::load(project).resolve(&document.id)
            {
                for color in appearance.materials.values_mut() {
                    *color = base_color;
                }
            }
        }
    }

    Ok(CharacterPackage {
        metadata: PackageMetadata {
            name: if authored.name.trim().is_empty() {
                dir.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("character")
                    .to_string()
            } else {
                authored.name
            },
            aliases: metadata.aliases,
            mesh_type: metadata.mesh_type,
            has_gun: metadata.has_gun,
            default_animation: metadata.default_animation,
            attachment_points: metadata.attachment_points,
            attachment_rotations,
        },
        source_actor: authored.source_actor,
        scale: Vec3::from_array(authored.scale),
        material: authored.material,
        controller: authored.controller,
        camera: authored.camera,
        appearance,
        collision: CharacterCollision {
            radius: collision_file.radius,
            height: collision_file.height,
        },
        skeleton,
        clips,
        blend: BlendSettings {
            default_state: blend_file.default_state,
            blend_speed: blend_file.blend_speed,
            idle_clip: blend_file.idle_clip,
            walk_clip: blend_file.walk_clip,
            run_clip: blend_file.run_clip,
            run_threshold: blend_file.run_threshold,
        },
        geometry,
        rest_pose_bounds_cache: OnceLock::new(),
    })
}

fn load_packages_from_root(root: &Path, packages: &mut HashMap<String, Arc<CharacterPackage>>) {
    let paths = crate::project::asset_packs::package_directories(root);

    for path in paths {
        if !path.is_dir() {
            continue;
        }

        if !path.join("package.json").is_file() {
            continue;
        }

        let folder_name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        match load_package(&path) {
            Ok(package) => {
                register_package_aliases(packages, &folder_name, Arc::new(package));
            }
            Err(error) => {
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Package",
                    format!(
                        "Failed to load character package '{}': {}",
                        path.display(),
                        error
                    ),
                );
            }
        }
    }
}

fn register_package_aliases(
    packages: &mut HashMap<String, Arc<CharacterPackage>>,
    folder_name: &str,
    package: Arc<CharacterPackage>,
) {
    let mut identifiers = vec![folder_name.to_string(), package.metadata.name.clone()];
    identifiers.extend(package.metadata.aliases.iter().cloned());
    identifiers.retain(|identifier| !identifier.trim().is_empty());
    identifiers.sort();
    identifiers.dedup();

    for identifier in identifiers {
        if let Some(existing) = packages.get(&identifier) {
            if !Arc::ptr_eq(existing, &package) {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Package",
                    format!(
                        "Character package identifier '{}' is defined more than once; the later package takes precedence",
                        identifier
                    ),
                );
            }
        }
        packages.insert(identifier, package.clone());
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(dir: &Path, file: &str) -> Result<T, String> {
    let path = dir.join(file);
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))
}

#[derive(Deserialize)]
struct PackageMetadataFile {
    name: String,
    #[serde(default)]
    source_actor: Option<String>,
    #[serde(default = "unit_scale")]
    scale: [f32; 3],
    #[serde(default)]
    material: Option<String>,
    #[serde(default)]
    controller: Option<String>,
    #[serde(default)]
    camera: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    mesh_type: String,
    #[serde(default)]
    has_gun: bool,
    #[serde(default)]
    default_animation: Option<String>,
    #[serde(default)]
    attachment_points: HashMap<String, String>,
    #[serde(default)]
    attachment_rotations: HashMap<String, [f32; 4]>,
}

#[derive(Deserialize)]
struct AppearanceFile {
    #[serde(default)]
    materials: HashMap<String, [f32; 4]>,
    #[serde(default)]
    show_accessory: Option<bool>,
    #[serde(default)]
    show_accent: Option<bool>,
}

impl AppearanceFile {
    fn into_appearance(self) -> AppearanceCustomization {
        let mut appearance = AppearanceCustomization::from_materials(self.materials);
        if let Some(show_accessory) = self.show_accessory {
            appearance.show_accessory = show_accessory;
        }
        if let Some(show_accent) = self.show_accent {
            appearance.show_accent = show_accent;
        }
        appearance
    }
}

#[derive(Deserialize)]
struct CollisionFile {
    radius: f32,
    height: f32,
}

#[derive(Deserialize)]
struct RigFile {
    joints: Vec<RigJointFile>,
}

#[derive(Deserialize)]
struct RigJointFile {
    name: String,
    #[serde(default)]
    parent: Option<serde_json::Value>,
    #[serde(default)]
    translation: [f32; 3],
    #[serde(default = "identity_quat")]
    rotation: [f32; 4],
    #[serde(default = "unit_scale")]
    scale: [f32; 3],
}

fn identity_quat() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn unit_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Deserialize)]
struct BlendFile {
    default_state: String,
    #[serde(default = "default_blend_speed")]
    blend_speed: f32,
    #[serde(default = "default_idle_clip")]
    idle_clip: String,
    #[serde(default = "default_walk_clip")]
    walk_clip: String,
    #[serde(default)]
    run_clip: Option<String>,
    #[serde(default = "default_run_threshold")]
    run_threshold: f32,
}

fn default_blend_speed() -> f32 {
    4.0
}

fn default_idle_clip() -> String {
    "Idle".to_string()
}

fn default_walk_clip() -> String {
    "Walk".to_string()
}

fn default_run_threshold() -> f32 {
    f32::INFINITY
}

#[derive(Deserialize)]
struct AnimationFile {
    animations: Vec<AnimationClipFile>,
}

#[derive(Deserialize)]
struct AnimationClipFile {
    name: String,
    duration: f32,
    #[serde(rename = "loop", default = "default_true")]
    looping: bool,
    #[serde(default)]
    tracks: Vec<TrackFile>,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
struct TrackFile {
    joint: String,
    #[serde(default)]
    translation: Vec<KeyframeFile>,
    #[serde(default)]
    rotation: Vec<KeyframeFile>,
    #[serde(default)]
    scale: Vec<KeyframeFile>,
}

#[derive(Deserialize)]
struct KeyframeFile {
    time: f32,
    #[serde(default)]
    step: bool,
    #[serde(default)]
    value: Option<Vec<f32>>,
    #[serde(default)]
    euler: Option<[f32; 3]>,
}

#[derive(Deserialize)]
struct GeometryFile {
    parts: Vec<GeometryPartFile>,
}

#[derive(Deserialize)]
struct GeometryPartFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    joint: String,
    #[serde(default)]
    material: String,
    #[serde(default)]
    when: Option<String>,
    #[serde(default)]
    min: Option<[f32; 3]>,
    #[serde(default)]
    max: Option<[f32; 3]>,
    #[serde(default)]
    radius: Option<f32>,
    #[serde(default)]
    radius_x: Option<f32>,
    #[serde(default)]
    radius_z: Option<f32>,
    #[serde(default)]
    min_y: Option<f32>,
    #[serde(default)]
    max_y: Option<f32>,
    #[serde(default)]
    segments: Option<usize>,
    #[serde(default)]
    center: Option<[f32; 3]>,
    #[serde(default)]
    radius_y: Option<f32>,
    #[serde(default)]
    rings: Option<usize>,
    #[serde(default)]
    radius_bottom: Option<f32>,
    #[serde(default)]
    radius_top: Option<f32>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    mesh: Option<String>,
}

fn skeleton_from_file(file: &RigFile) -> Result<Skeleton, String> {
    let mut joints = Vec::with_capacity(file.joints.len());

    for (index, joint) in file.joints.iter().enumerate() {
        let parent_index = match &joint.parent {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::Number(number)) => {
                let parent = number
                    .as_u64()
                    .ok_or_else(|| format!("joint '{}' parent index is invalid", joint.name))?
                    as usize;
                if parent >= file.joints.len() {
                    return Err(format!(
                        "joint '{}' parent index {} is out of range",
                        joint.name, parent
                    ));
                }
                Some(parent)
            }
            Some(serde_json::Value::String(name)) => {
                let parent = file
                    .joints
                    .iter()
                    .position(|candidate| candidate.name == *name)
                    .ok_or_else(|| {
                        format!("joint '{}' parent '{}' was not found", joint.name, name)
                    })?;
                Some(parent)
            }
            Some(_) => {
                return Err(format!("joint '{}' has an invalid parent", joint.name));
            }
        };

        if parent_index == Some(index) {
            return Err(format!("joint '{}' cannot parent itself", joint.name));
        }
        if parent_index.is_some_and(|parent| parent >= index) {
            return Err(format!(
                "joint '{}' parent must precede the child in rig order",
                joint.name
            ));
        }
        let translation = Vec3::from_array(joint.translation);
        let rotation = quat_from_array(joint.rotation);
        let scale = Vec3::from_array(joint.scale);
        if !translation.is_finite()
            || !rotation.is_finite()
            || !scale.is_finite()
            || scale.min_element() <= 0.0
        {
            return Err(format!(
                "joint '{}' has non-finite or invalid transform",
                joint.name
            ));
        }

        joints.push(Joint {
            name: joint.name.clone(),
            parent_index,
            local_transform: Transform {
                translation,
                rotation,
                scale,
            },
        });
    }

    Ok(Skeleton::new(joints))
}

fn clips_from_file(
    file: &AnimationFile,
    skeleton: &Skeleton,
) -> Result<Vec<AnimationClip>, String> {
    let mut clips = Vec::with_capacity(file.animations.len());

    for clip in &file.animations {
        let mut tracks = Vec::new();

        for track in &clip.tracks {
            let joint_index = skeleton.joint_index(&track.joint).ok_or_else(|| {
                format!(
                    "animation '{}' references unknown joint '{}'",
                    clip.name, track.joint
                )
            })?;

            tracks.push(TransformTrack {
                joint_index,
                translations: vec3_keys(&track.translation)?,
                rotations: quat_keys(&track.rotation)?,
                scales: vec3_keys(&track.scale)?,
            });
        }

        clips.push(AnimationClip {
            name: clip.name.clone(),
            duration: clip.duration.max(0.0001),
            looping: clip.looping,
            tracks,
            events: Vec::new(),
        });
    }

    Ok(clips)
}

fn vec3_keys(keys: &[KeyframeFile]) -> Result<Vec<Keyframe<Vec3>>, String> {
    let mut out = Vec::with_capacity(keys.len());

    for key in keys {
        let value = key
            .value
            .as_ref()
            .ok_or_else(|| "translation/scale keyframe is missing value".to_string())?;
        if value.len() != 3 {
            return Err("translation/scale keyframe value must have 3 numbers".to_string());
        }
        out.push(Keyframe {
            step: key.step,
            time: key.time,
            value: Vec3::new(value[0], value[1], value[2]),
        });
    }

    Ok(out)
}

fn quat_keys(keys: &[KeyframeFile]) -> Result<Vec<Keyframe<Quat>>, String> {
    let mut out = Vec::with_capacity(keys.len());

    for key in keys {
        let value = if let Some(euler) = key.euler {
            Quat::from_euler(EulerRot::XYZ, euler[0], euler[1], euler[2])
        } else if let Some(value) = &key.value {
            if value.len() != 4 {
                return Err("rotation keyframe value must have 4 numbers [x, y, z, w]".to_string());
            }
            quat_from_array([value[0], value[1], value[2], value[3]])
        } else {
            return Err("rotation keyframe is missing value or euler".to_string());
        };

        out.push(Keyframe {
            step: key.step,
            time: key.time,
            value,
        });
    }

    Ok(out)
}

fn geometry_from_file(
    file: &GeometryFile,
    skeleton: &Skeleton,
    package_dir: &Path,
) -> Result<Vec<GeometryPart>, String> {
    let mut parts = Vec::with_capacity(file.parts.len());

    for part in &file.parts {
        if part.kind == "imported_glb" {
            let source = confined_glb_source(
                package_dir,
                part.source
                    .as_deref()
                    .ok_or_else(|| "imported_glb part is missing source".to_string())?,
            )?;
            let mesh_name = part
                .mesh
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| "imported_glb part is missing mesh".to_string())?;
            let imported = crate::project::load_skinned_glb_mesh(&source, mesh_name)?;
            let source_to_skeleton = imported
                .joint_names
                .iter()
                .map(|name| {
                    skeleton.joint_index(name).ok_or_else(|| {
                        format!("imported GLB joint '{name}' was not found in rig.json")
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut inverse_bind_matrices = vec![Mat4::IDENTITY; skeleton.joints.len()];
            for (source_index, matrix) in imported.inverse_bind_matrices.iter().copied().enumerate()
            {
                inverse_bind_matrices[source_to_skeleton[source_index]] = matrix;
            }
            for primitive in imported.primitives {
                let vertices = primitive
                    .vertices
                    .into_iter()
                    .map(|vertex| SkinnedVertex {
                        position: vertex.position,
                        normal: vertex.normal,
                        tex_coord: vertex.tex_coord,
                        color: vertex.source_color,
                        joints: vertex
                            .joints
                            .map(|joint| source_to_skeleton[joint as usize]),
                        weights: vertex.weights,
                    })
                    .collect();
                parts.push(GeometryPart {
                    base_color_texture: primitive.base_color_texture_index.map(|index| {
                        package_dir
                            .join("textures")
                            .join(format!("image_{index}.png"))
                    }),
                    joint_index: 0,
                    material: primitive.material_name,
                    when: part.when.clone(),
                    primitive: GeometryPrimitive::SkinnedTriangleMesh {
                        vertices,
                        inverse_bind_matrices: inverse_bind_matrices.clone(),
                    },
                });
            }
            continue;
        }

        let joint_index = skeleton
            .joint_index(&part.joint)
            .ok_or_else(|| format!("geometry part references unknown joint '{}'", part.joint))?;

        let primitive = match part.kind.as_str() {
            "cube" => {
                let min = part
                    .min
                    .ok_or_else(|| "cube part is missing min".to_string())?;
                let max = part
                    .max
                    .ok_or_else(|| "cube part is missing max".to_string())?;
                GeometryPrimitive::Cube {
                    min: Vec3::from_array(min),
                    max: Vec3::from_array(max),
                }
            }
            "cylinder" => {
                let radius = part.radius.unwrap_or(0.0);
                let radius_x = part.radius_x.unwrap_or(radius);
                let radius_z = part.radius_z.unwrap_or(radius);
                GeometryPrimitive::Cylinder {
                    radius_x,
                    radius_z,
                    min_y: part
                        .min_y
                        .ok_or_else(|| "cylinder part is missing min_y".to_string())?,
                    max_y: part
                        .max_y
                        .ok_or_else(|| "cylinder part is missing max_y".to_string())?,
                    segments: part.segments.unwrap_or(8),
                }
            }
            "ellipsoid" => GeometryPrimitive::Ellipsoid {
                center: Vec3::from_array(part.center.unwrap_or([0.0, 0.0, 0.0])),
                radius_x: part.radius_x.or(part.radius).unwrap_or(0.1),
                radius_y: part.radius_y.or(part.radius).unwrap_or(0.1),
                radius_z: part.radius_z.or(part.radius).unwrap_or(0.1),
                segments: part.segments.unwrap_or(8),
                rings: part.rings.unwrap_or(6),
            },
            "tapered_flame" => GeometryPrimitive::TaperedFlame {
                radius_bottom: part.radius_bottom.unwrap_or(0.0),
                radius_top: part.radius_top.unwrap_or(0.1),
                min_y: part
                    .min_y
                    .ok_or_else(|| "tapered_flame part is missing min_y".to_string())?,
                max_y: part
                    .max_y
                    .ok_or_else(|| "tapered_flame part is missing max_y".to_string())?,
                segments: part.segments.unwrap_or(8),
            },
            other => return Err(format!("unsupported geometry type '{}'", other)),
        };

        parts.push(GeometryPart {
            base_color_texture: None,
            joint_index,
            material: part.material.clone(),
            when: part.when.clone(),
            primitive,
        });
    }

    Ok(parts)
}

fn confined_glb_source(package_dir: &Path, source: &str) -> Result<PathBuf, String> {
    let relative = Path::new(source.trim());
    let mut components = relative.components();
    let single_file =
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
    if !single_file
        || !relative
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
    {
        return Err(
            "character imported_glb source must be one .glb file inside the package".to_string(),
        );
    }
    Ok(package_dir.join(relative))
}

fn quat_from_array(value: [f32; 4]) -> Quat {
    Quat::from_xyzw(value[0], value[1], value[2], value[3]).normalize()
}

pub fn controller_from_package(package: &CharacterPackage) -> CharacterAnimationController {
    CharacterAnimationController::from_package(package)
}

pub fn discover_package_dirs(project_path: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut roots = vec![engine_assets_dir().join("characters")];
    if let Some(project_path) = project_path {
        roots.push(project_path.join(".assets").join("characters"));
    }

    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("package.json").is_file() {
                dirs.push(path);
            }
        }
    }

    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generated_bounds(package: Arc<CharacterPackage>) -> (Vec3, Vec3) {
        let character = crate::character::Character::from_package(1, Vec3::ZERO, package);
        let vertices = crate::character::generate_character_mesh(&character);
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);

        for vertex in vertices.chunks_exact(10) {
            let position = Vec3::new(vertex[0], vertex[1], vertex[2]);
            min = min.min(position);
            max = max.max(position);
        }

        assert!(min.is_finite() && max.is_finite());
        (min, max)
    }



    #[test]
    fn rig_import_rejects_forward_parent_cycles_and_invalid_transforms_without_panicking() {
        let valid = serde_json::json!({
            "joints": [
                {"name":"Root", "parent":null},
                {"name":"Hand", "parent":0, "translation":[0.0,1.0,0.0]}
            ]
        });
        let parsed: RigFile = serde_json::from_value(valid.clone()).unwrap();
        let skeleton = skeleton_from_file(&parsed).unwrap();
        let transforms: Vec<_> = skeleton.joints.iter().map(|j| j.local_transform).collect();
        let pose = skeleton.evaluate_pose(&transforms);
        assert_eq!(pose.matrices.len(), 2);
        assert!(pose.matrices.iter().all(|mat| mat.is_finite()));

        let cases = [
            (
                serde_json::json!({"joints":[
                    {"name":"First","parent":1},
                    {"name":"Second","parent":null}
                ]}),
                "precede",
            ),
            (
                serde_json::json!({"joints":[
                    {"name":"First","parent":1},
                    {"name":"Second","parent":0}
                ]}),
                "precede",
            ),
            (
                serde_json::json!({"joints":[
                    {"name":"Root","parent":null},
                    {"name":"Self","parent":1}
                ]}),
                "itself",
            ),
            (
                serde_json::json!({"joints":[
                    {"name":"Root","parent":null},
                    {"name":"Invalid","parent":10}
                ]}),
                "out of range",
            ),
            (
                serde_json::json!({"joints":[
                    {"name":"Root","scale":[0.0,1.0,1.0]}
                ]}),
                "invalid transform",
            ),
        ];
        for (input, expected_error) in cases {
            let parsed: RigFile = serde_json::from_value(input).unwrap();
            let error = skeleton_from_file(&parsed).unwrap_err();
            assert!(error.contains(expected_error), "wrong rig error: {error}");
        }
    }

    #[test]
    fn package_registry_resolves_folder_manifest_and_declared_aliases_without_fallback() {
        let unique = format!("WorldkilnPackageRegistry_{}", std::process::id());
        let root = std::env::temp_dir().join(unique);
        let package_dir = root.join("test_folder_id");
        std::fs::create_dir_all(&package_dir).unwrap();
        let samples = [
            (
                "package.json",
                r#"{"name":"test_manifest_id","aliases":["legacy_character_id"]}"#,
            ),
            ("rig.json", r#"{"joints":[{"name":"Root","parent":null}]}"#),
            (
                "geometry.json",
                r#"{"parts":[{"type":"cube","joint":"Root","material":"skin","min":[-0.2,0.0,-0.2],"max":[0.2,1.0,0.2]}]}"#,
            ),
            (
                "appearance.json",
                r#"{"materials":{"skin":[0.7,0.7,0.7,1.0]}}"#,
            ),
            ("collision.json", r#"{"radius":0.4,"height":1.0}"#),
            (
                "animation.json",
                r#"{"animations":[{"name":"Idle","duration":1.0,"loop":true}]}"#,
            ),
            ("blend.json", r#"{"default_state":"Idle"}"#),
        ];
        for (name, contents) in samples {
            std::fs::write(package_dir.join(name), contents).unwrap();
        }

        let mut packages = HashMap::new();
        load_packages_from_root(&root, &mut packages);
        let registry = CharacterPackageRegistry {
            packages,
            by_asset_id: Default::default(),
        };

        let by_folder = registry.get("test_folder_id").unwrap();
        let by_manifest = registry.resolve("test_manifest_id").unwrap();
        let by_alias = registry.resolve("legacy_character_id").unwrap();
        assert!(Arc::ptr_eq(&by_folder, &by_manifest));
        assert!(Arc::ptr_eq(&by_manifest, &by_alias));
        assert_eq!(registry.names(), vec!["test_manifest_id"]);
        assert!(registry.resolve("missing_package").is_none());

        std::fs::remove_dir_all(root).unwrap();
    }

}
