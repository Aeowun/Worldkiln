use crate::project::engine_assets_dir;
use glam::{Vec2, Vec3};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

pub const ENTITY_PACKAGE_DIRECTORY: &str = "entities";

#[derive(Clone, Debug)]
pub struct EntityPackage {
    pub metadata: EntityPackageMetadata,
    pub appearance: EntityAppearanceDefinition,
    pub physics: EntityPhysicsDefaults,
    pub animation: EntityAnimationDefinition,
    pub character: Option<EntityCharacterCapability>,

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
    pub(crate) triangle_mesh_bounds_cache: OnceLock<Option<(Vec3, Vec3)>>,
}

impl EntityPackage {
    pub fn validate(&self) -> Result<(), String> {
        if self.metadata.name.trim().is_empty() {
            return Err("entity package name cannot be empty".to_string());
        }

        if self.metadata.version == 0 {
            return Err("entity package version must be greater than zero".to_string());
        }

        if !self.physics.mass.is_finite() || self.physics.mass <= 0.0 {
            return Err(
                "entity package physics mass must be finite and greater than zero".to_string(),
            );
        }

        let mut node_names = HashSet::new();

        for node in &self.appearance.nodes {
            if node.name.trim().is_empty() {
                return Err("entity package node name cannot be empty".to_string());
            }

            let node_key = node.name.to_ascii_lowercase();

            if !node_names.insert(node_key) {
                return Err(format!("duplicate entity package node '{}'", node.name));
            }

            if let Some(parent) = &node.parent {
                if parent.eq_ignore_ascii_case(&node.name) {
                    return Err(format!(
                        "entity package node '{}' cannot parent itself",
                        node.name
                    ));
                }
            }
        }

        for node in &self.appearance.nodes {
            if let Some(parent) = &node.parent {
                if !node_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(parent))
                {
                    return Err(format!(
                        "entity package node '{}' references unknown parent '{}'",
                        node.name, parent
                    ));
                }
            }
        }

        for part in &self.appearance.geometry {
            if part.node.trim().is_empty() {
                return Err("entity package geometry node cannot be empty".to_string());
            }

            if !node_names.is_empty()
                && !node_names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&part.node))
            {
                return Err(format!(
                    "entity package geometry references unknown node '{}'",
                    part.node
                ));
            }

            if part.material.trim().is_empty() {
                return Err("entity package geometry material cannot be empty".to_string());
            }

            if !self
                .appearance
                .materials
                .keys()
                .any(|name| name.eq_ignore_ascii_case(&part.material))
            {
                return Err(format!(
                    "entity package geometry references unknown material '{}'",
                    part.material
                ));
            }

            part.primitive.validate()?;
        }

        let mut clip_names = HashSet::new();

        for clip in &self.animation.clips {
            if clip.name.trim().is_empty() {
                return Err("entity package animation name cannot be empty".to_string());
            }

            if !clip_names.insert(clip.name.to_ascii_lowercase()) {
                return Err(format!(
                    "duplicate entity package animation '{}'",
                    clip.name
                ));
            }

            if !clip.duration.is_finite() || clip.duration <= 0.0 {
                return Err(format!(
                    "entity package animation '{}' duration must be finite and greater than zero",
                    clip.name
                ));
            }

            for track in &clip.tracks {
                if track.node.trim().is_empty() {
                    return Err(format!(
                        "entity package animation '{}' contains an empty track node",
                        clip.name
                    ));
                }

                if !node_names.is_empty()
                    && !node_names
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&track.node))
                {
                    return Err(format!(
                        "entity package animation '{}' references unknown node '{}'",
                        clip.name, track.node
                    ));
                }

                for key in &track.translation {
                    key.validate()?;
                }

                for key in &track.rotation {
                    key.validate()?;
                }

                for key in &track.scale {
                    key.validate()?;
                }
            }
        }

        if let Some(default_clip) = &self.animation.default_clip {
            if !self
                .animation
                .clips
                .iter()
                .any(|clip| clip.name.eq_ignore_ascii_case(default_clip))
            {
                return Err(format!(
                    "entity package default animation '{}' was not found",
                    default_clip
                ));
            }
        }

        if let Some(character) = &self.character {
            character.validate()?;

            if let Some(default_animation) = &character.default_animation {
                if !self
                    .animation
                    .clips
                    .iter()
                    .any(|clip| clip.name.eq_ignore_ascii_case(default_animation))
                {
                    return Err(format!(
                        "entity character default animation '{}' was not found",
                        default_animation
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn animation_named(&self, name: &str) -> Option<&EntityAnimationClip> {
        self.animation
            .clips
            .iter()
            .find(|clip| clip.name.eq_ignore_ascii_case(name))
    }

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
    pub fn triangle_mesh_bounds(&self) -> Option<(Vec3, Vec3)> {
        *self.triangle_mesh_bounds_cache.get_or_init(|| {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            let mut found = false;
            for part in &self.appearance.geometry {
                let EntityGeometryPrimitive::TriangleMesh { vertices } = &part.primitive else {
                    continue;
                };
                for vertex in vertices {
                    min = min.min(vertex.position);
                    max = max.max(vertex.position);
                    found = true;
                }
            }
            found.then_some((min, max))
        })
    }
}

#[derive(Clone, Debug)]
pub struct EntityPackageMetadata {
    pub name: String,
    pub version: u32,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct EntityAppearanceDefinition {
    pub materials: HashMap<String, [f32; 4]>,
    pub nodes: Vec<EntityNodeDefinition>,
    pub geometry: Vec<EntityGeometryPart>,
}

#[derive(Clone, Debug)]
pub struct EntityNodeDefinition {
    pub name: String,
    pub parent: Option<String>,
    pub translation: Vec3,
    pub rotation: [f32; 4],
    pub scale: Vec3,
}

#[derive(Clone, Debug)]
pub struct EntityGeometryPart {
    pub node: String,
    pub material: String,
    pub when: Option<String>,
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    // Imported base-color images remain package-local project assets. Native
    // procedural geometry leaves this unset and keeps its existing color path.
    pub base_color_texture: Option<PathBuf>,
    pub primitive: EntityGeometryPrimitive,
}

#[derive(Clone, Debug)]
pub enum EntityGeometryPrimitive {
    TriangleMesh {
        vertices: Vec<EntityMeshVertex>,
    },
    Cube {
        min: Vec3,
        max: Vec3,
    },
    Sphere {
        center: Vec3,
        radius: f32,
        segments: usize,
        rings: usize,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityMeshVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coord: Vec2,
    pub color: [f32; 4],
}

impl EntityGeometryPrimitive {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::TriangleMesh { vertices } => {
                if vertices.is_empty() || !vertices.len().is_multiple_of(3) {
                    return Err(
                        "entity package triangle mesh requires a non-empty triangle vertex stream"
                            .to_string(),
                    );
                }
                if vertices.iter().any(|vertex| {
                    !vertex.position.is_finite()
                        || !vertex.normal.is_finite()
                        || !vertex.tex_coord.is_finite()
                        || !vertex.color.iter().all(|value| value.is_finite())
                }) {
                    return Err("entity package triangle mesh vertices must be finite".to_string());
                }
            }
            Self::Cube { min, max } => {
                if !min.is_finite() || !max.is_finite() {
                    return Err("entity package cube bounds must be finite".to_string());
                }

                if min.x > max.x || min.y > max.y || min.z > max.z {
                    return Err("entity package cube min must not exceed max".to_string());
                }
            }

            Self::Sphere {
                center,
                radius,
                segments,
                rings,
            } => {
                if !center.is_finite() || !radius.is_finite() || *radius <= 0.0 {
                    return Err("entity package sphere dimensions are invalid".to_string());
                }

                if *segments < 3 || *rings < 2 {
                    return Err(
                        "entity package sphere requires at least 3 segments and 2 rings"
                            .to_string(),
                    );
                }
            }

            Self::Cylinder {
                radius_x,
                radius_z,
                min_y,
                max_y,
                segments,
            } => {
                if !radius_x.is_finite()
                    || !radius_z.is_finite()
                    || !min_y.is_finite()
                    || !max_y.is_finite()
                    || *radius_x <= 0.0
                    || *radius_z <= 0.0
                    || min_y >= max_y
                {
                    return Err("entity package cylinder dimensions are invalid".to_string());
                }

                if *segments < 3 {
                    return Err("entity package cylinder requires at least 3 segments".to_string());
                }
            }

            Self::Ellipsoid {
                center,
                radius_x,
                radius_y,
                radius_z,
                segments,
                rings,
            } => {
                if !center.is_finite()
                    || !radius_x.is_finite()
                    || !radius_y.is_finite()
                    || !radius_z.is_finite()
                    || *radius_x <= 0.0
                    || *radius_y <= 0.0
                    || *radius_z <= 0.0
                {
                    return Err("entity package ellipsoid dimensions are invalid".to_string());
                }

                if *segments < 3 || *rings < 2 {
                    return Err(
                        "entity package ellipsoid requires at least 3 segments and 2 rings"
                            .to_string(),
                    );
                }
            }

            Self::TaperedFlame {
                radius_bottom,
                radius_top,
                min_y,
                max_y,
                segments,
            } => {
                if !radius_bottom.is_finite()
                    || !radius_top.is_finite()
                    || !min_y.is_finite()
                    || !max_y.is_finite()
                    || *radius_bottom < 0.0
                    || *radius_top < 0.0
                    || min_y >= max_y
                    || (*radius_bottom == 0.0 && *radius_top == 0.0)
                {
                    return Err("entity package tapered_flame dimensions are invalid".to_string());
                }

                if *segments < 3 {
                    return Err(
                        "entity package tapered_flame requires at least 3 segments".to_string()
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct EntityPhysicsDefaults {
    pub enabled: bool,
    pub solid: bool,
    pub anchored: bool,
    pub collision_enabled: bool,
    pub mass: f32,
}

impl Default for EntityPhysicsDefaults {
    fn default() -> Self {
        Self {
            enabled: true,
            solid: true,
            anchored: true,
            collision_enabled: true,
            mass: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct EntityAnimationDefinition {
    pub default_clip: Option<String>,
    pub clips: Vec<EntityAnimationClip>,
}

#[derive(Clone, Debug)]
pub struct EntityAnimationClip {
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub tracks: Vec<EntityTransformTrack>,
}

#[derive(Clone, Debug, Default)]
pub struct EntityTransformTrack {
    pub node: String,
    pub translation: Vec<EntityVec3Keyframe>,
    pub rotation: Vec<EntityQuaternionKeyframe>,
    pub scale: Vec<EntityVec3Keyframe>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct EntityVec3Keyframe {
    pub time: f32,
    pub value: [f32; 3],
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct EntityQuaternionKeyframe {
    pub time: f32,

    #[serde(default)]
    pub value: Option<[f32; 4]>,

    #[serde(default)]
    pub euler: Option<[f32; 3]>,
}

impl EntityVec3Keyframe {
    fn validate(&self) -> Result<(), String> {
        if !self.time.is_finite() || !self.value.iter().all(|value| value.is_finite()) {
            return Err("entity package keyframe values must be finite".to_string());
        }

        Ok(())
    }
}

impl EntityQuaternionKeyframe {
    fn validate(&self) -> Result<(), String> {
        if !self.time.is_finite() {
            return Err("entity package keyframe time must be finite".to_string());
        }

        if self.value.is_none() && self.euler.is_none() {
            return Err("entity package rotation keyframe requires value or euler".to_string());
        }

        if self.value.is_some() && self.euler.is_some() {
            return Err(
                "entity package rotation keyframe cannot define both value and euler".to_string(),
            );
        }

        if let Some(value) = self.value {
            if !value.iter().all(|value| value.is_finite()) {
                return Err(
                    "entity package rotation keyframe quaternion must be finite".to_string()
                );
            }
        }

        if let Some(euler) = self.euler {
            if !euler.iter().all(|value| value.is_finite()) {
                return Err(
                    "entity package rotation keyframe euler values must be finite".to_string(),
                );
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct EntityCharacterCapability {
    pub radius: f32,
    pub height: f32,
    pub default_animation: Option<String>,
    pub blend_speed: f32,
}

impl EntityCharacterCapability {
    fn validate(&self) -> Result<(), String> {
        if !self.radius.is_finite() || self.radius <= 0.0 {
            return Err("entity character radius must be finite and greater than zero".to_string());
        }

        if !self.height.is_finite() || self.height <= 0.0 {
            return Err("entity character height must be finite and greater than zero".to_string());
        }

        if !self.blend_speed.is_finite() || self.blend_speed <= 0.0 {
            return Err(
                "entity character blend_speed must be finite and greater than zero".to_string(),
            );
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct EntityPackageRegistry {
    packages: HashMap<String, Arc<EntityPackage>>,
}

impl EntityPackageRegistry {
    pub fn load(project_path: Option<&Path>) -> Self {
        let mut packages = HashMap::new();

        let mut roots = vec![engine_assets_dir().join(ENTITY_PACKAGE_DIRECTORY)];

        if let Some(project_path) = project_path {
            roots.push(project_path.join(".assets").join(ENTITY_PACKAGE_DIRECTORY));
        }

        for root in roots {
            load_packages_from_root(&root, &mut packages);
        }

        Self { packages }
    }

    pub fn get(&self, name: &str) -> Option<Arc<EntityPackage>> {
        self.packages.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.packages.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn contains(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    pub fn values(&self) -> impl Iterator<Item = &Arc<EntityPackage>> {
        self.packages.values()
    }

    pub fn first(&self) -> Option<Arc<EntityPackage>> {
        let mut names = self.names();

        if names.is_empty() {
            return None;
        }

        self.get(&names.remove(0))
    }
}

static REGISTRY: OnceLock<RwLock<EntityPackageRegistry>> = OnceLock::new();

fn registry_lock() -> &'static RwLock<EntityPackageRegistry> {
    REGISTRY.get_or_init(|| RwLock::new(EntityPackageRegistry::load(None)))
}

pub fn package_registry() -> EntityPackageRegistry {
    registry_lock()
        .read()
        .expect("entity package registry")
        .clone()
}

pub fn reload_packages(project_path: Option<&Path>) {
    let mut registry = registry_lock().write().expect("entity package registry");

    *registry = EntityPackageRegistry::load(project_path);
}

pub fn get_package(name: &str) -> Option<Arc<EntityPackage>> {
    registry_lock()
        .read()
        .expect("entity package registry")
        .get(name)
}

pub fn available_package_names() -> Vec<String> {
    registry_lock()
        .read()
        .expect("entity package registry")
        .names()
}

pub fn load_package(dir: &Path) -> Result<EntityPackage, String> {
    let path = dir.join("package.json");

    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;

    let file: EntityPackageFile = serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))?;

    let package = file.into_package(dir)?;
    package.validate()?;

    Ok(package)
}

fn load_packages_from_root(root: &Path, packages: &mut HashMap<String, Arc<EntityPackage>>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_dir() || !path.join("package.json").is_file() {
            continue;
        }

        let folder_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if folder_name.is_empty() {
            continue;
        }

        match load_package(&path) {
            Ok(package) => {
                packages.insert(folder_name, Arc::new(package));
            }

            Err(error) => {
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Package",
                    format!(
                        "Failed to load entity package '{}': {}",
                        path.display(),
                        error
                    ),
                );
            }
        }
    }
}

#[derive(Deserialize)]
struct EntityPackageFile {
    #[serde(default)]
    name: String,

    #[serde(default = "default_version")]
    version: u32,

    #[serde(default)]
    description: Option<String>,

    #[serde(default)]
    appearance: EntityAppearanceFile,

    #[serde(default)]
    physics: EntityPhysicsFile,

    #[serde(default)]
    animation: EntityAnimationFile,

    #[serde(default)]
    character: Option<EntityCharacterFile>,
}

impl EntityPackageFile {
    fn into_package(self, dir: &Path) -> Result<EntityPackage, String> {
        let name = if self.name.trim().is_empty() {
            dir.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("entity")
                .to_string()
        } else {
            self.name
        };

        Ok(EntityPackage {
            metadata: EntityPackageMetadata {
                name,
                version: self.version,
                description: self.description,
            },
            appearance: self.appearance.into_definition(dir)?,
            physics: self.physics.into_defaults(),
            animation: self.animation.into_definition()?,
            character: self
                .character
                .map(EntityCharacterFile::into_capability)
                .transpose()?,
            triangle_mesh_bounds_cache: OnceLock::new(),
        })
    }
}

fn default_version() -> u32 {
    1
}

#[derive(Deserialize, Default)]
struct EntityAppearanceFile {
    #[serde(default)]
    materials: HashMap<String, [f32; 4]>,

    #[serde(default)]
    nodes: Vec<EntityNodeFile>,

    #[serde(default)]
    geometry: Vec<EntityGeometryFile>,

    #[serde(default)]
    imported_glb: Option<EntityImportedGlbFile>,
}

impl EntityAppearanceFile {
    fn into_definition(self, dir: &Path) -> Result<EntityAppearanceDefinition, String> {
        let nodes = self
            .nodes
            .into_iter()
            .map(EntityNodeFile::into_definition)
            .collect();

        let mut geometry = self
            .geometry
            .into_iter()
            .map(EntityGeometryFile::into_definition)
            .collect::<Result<Vec<_>, _>>()?;

        let mut materials = self.materials;
        if let Some(imported_glb) = self.imported_glb {
            let imported = imported_glb.load(dir)?;
            for primitive in imported.primitives {
                materials
                    .entry(primitive.material_name.clone())
                    .or_insert([1.0, 1.0, 1.0, 1.0]);
                geometry.push(EntityGeometryPart {
                    node: "Root".to_string(),
                    material: primitive.material_name,
                    when: None,
                    base_color_texture: primitive
                        .base_color_texture_index
                        .map(|index| dir.join("textures").join(format!("image_{index}.png"))),
                    primitive: EntityGeometryPrimitive::TriangleMesh {
                        vertices: primitive
                            .vertices
                            .into_iter()
                            .map(|vertex| EntityMeshVertex {
                                position: vertex.position,
                                normal: vertex.normal,
                                tex_coord: vertex.tex_coord,
                                color: vertex.color,
                            })
                            .collect(),
                    },
                });
            }
        }

        Ok(EntityAppearanceDefinition {
            materials,
            nodes,
            geometry,
        })
    }
}

#[derive(Deserialize)]
struct EntityImportedGlbFile {
    source: String,
    mesh: String,
}

impl EntityImportedGlbFile {
    fn load(self, dir: &Path) -> Result<crate::project::GlbStaticMesh, String> {
        let source = Path::new(self.source.trim());
        let mut components = source.components();
        let is_single_file =
            matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none();
        if !is_single_file
            || !source
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
        {
            return Err(
                "entity package imported_glb source must be a .glb file inside the package"
                    .to_string(),
            );
        }
        if self.mesh.trim().is_empty() {
            return Err("entity package imported_glb mesh cannot be empty".to_string());
        }

        crate::project::load_static_glb_mesh(&dir.join(source), self.mesh.trim())
            .map_err(|error| format!("failed to load imported GLB mesh: {error}"))
    }
}

#[derive(Deserialize)]
struct EntityNodeFile {
    name: String,

    #[serde(default)]
    parent: Option<String>,

    #[serde(default)]
    translation: [f32; 3],

    #[serde(default = "identity_rotation")]
    rotation: [f32; 4],

    #[serde(default = "unit_scale_array")]
    scale: [f32; 3],
}

impl EntityNodeFile {
    fn into_definition(self) -> EntityNodeDefinition {
        EntityNodeDefinition {
            name: self.name,
            parent: self.parent,
            translation: Vec3::from_array(self.translation),
            rotation: self.rotation,
            scale: Vec3::from_array(self.scale),
        }
    }
}

fn identity_rotation() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

fn unit_scale_array() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Deserialize)]
struct EntityGeometryFile {
    #[serde(rename = "type")]
    kind: String,

    node: String,
    material: String,

    #[serde(default)]
    when: Option<String>,

    #[serde(default)]
    min: Option<[f32; 3]>,

    #[serde(default)]
    max: Option<[f32; 3]>,

    #[serde(default)]
    center: Option<[f32; 3]>,

    #[serde(default)]
    radius: Option<f32>,

    #[serde(default)]
    radius_x: Option<f32>,

    #[serde(default)]
    radius_y: Option<f32>,

    #[serde(default)]
    radius_z: Option<f32>,

    #[serde(default)]
    radius_bottom: Option<f32>,

    #[serde(default)]
    radius_top: Option<f32>,

    #[serde(default)]
    min_y: Option<f32>,

    #[serde(default)]
    max_y: Option<f32>,

    #[serde(default = "default_segments")]
    segments: usize,

    #[serde(default = "default_rings")]
    rings: usize,
}

impl EntityGeometryFile {
    fn into_definition(self) -> Result<EntityGeometryPart, String> {
        let primitive = match self.kind.as_str() {
            "cube" => EntityGeometryPrimitive::Cube {
                min: self
                    .min
                    .ok_or_else(|| "entity package cube is missing min".to_string())
                    .map(Vec3::from_array)?,

                max: self
                    .max
                    .ok_or_else(|| "entity package cube is missing max".to_string())
                    .map(Vec3::from_array)?,
            },

            "sphere" => EntityGeometryPrimitive::Sphere {
                center: Vec3::from_array(self.center.unwrap_or([0.0, 0.0, 0.0])),
                radius: self.radius.unwrap_or(0.1),
                segments: self.segments,
                rings: self.rings,
            },

            "cylinder" => {
                let radius = self.radius.unwrap_or(0.0);

                EntityGeometryPrimitive::Cylinder {
                    radius_x: self.radius_x.unwrap_or(radius),
                    radius_z: self.radius_z.unwrap_or(radius),
                    min_y: self
                        .min_y
                        .ok_or_else(|| "entity package cylinder is missing min_y".to_string())?,
                    max_y: self
                        .max_y
                        .ok_or_else(|| "entity package cylinder is missing max_y".to_string())?,
                    segments: self.segments,
                }
            }

            "ellipsoid" => EntityGeometryPrimitive::Ellipsoid {
                center: Vec3::from_array(self.center.unwrap_or([0.0, 0.0, 0.0])),
                radius_x: self.radius_x.or(self.radius).unwrap_or(0.1),
                radius_y: self.radius_y.or(self.radius).unwrap_or(0.1),
                radius_z: self.radius_z.or(self.radius).unwrap_or(0.1),
                segments: self.segments,
                rings: self.rings,
            },

            "tapered_flame" => EntityGeometryPrimitive::TaperedFlame {
                radius_bottom: self.radius_bottom.unwrap_or(0.0),
                radius_top: self.radius_top.unwrap_or(0.1),
                min_y: self
                    .min_y
                    .ok_or_else(|| "entity package tapered_flame is missing min_y".to_string())?,
                max_y: self
                    .max_y
                    .ok_or_else(|| "entity package tapered_flame is missing max_y".to_string())?,
                segments: self.segments,
            },

            other => {
                return Err(format!(
                    "unsupported entity package geometry type '{}'",
                    other
                ));
            }
        };

        Ok(EntityGeometryPart {
            node: self.node,
            material: self.material,
            when: self.when,
            base_color_texture: None,
            primitive,
        })
    }
}

fn default_segments() -> usize {
    8
}

fn default_rings() -> usize {
    6
}

#[derive(Deserialize)]
struct EntityPhysicsFile {
    #[serde(default = "default_true")]
    enabled: bool,

    #[serde(default = "default_true")]
    solid: bool,

    #[serde(default = "default_true")]
    anchored: bool,

    #[serde(default = "default_true")]
    collision_enabled: bool,

    #[serde(default = "default_mass")]
    mass: f32,
}

impl Default for EntityPhysicsFile {
    fn default() -> Self {
        Self {
            enabled: true,
            solid: true,
            anchored: true,
            collision_enabled: true,
            mass: 1.0,
        }
    }
}

impl EntityPhysicsFile {
    fn into_defaults(self) -> EntityPhysicsDefaults {
        EntityPhysicsDefaults {
            enabled: self.enabled,
            solid: self.solid,
            anchored: self.anchored,
            collision_enabled: self.collision_enabled,
            mass: self.mass,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_mass() -> f32 {
    1.0
}

#[derive(Deserialize, Default)]
struct EntityAnimationFile {
    #[serde(default)]
    default_clip: Option<String>,

    #[serde(default)]
    clips: Vec<EntityAnimationClipFile>,
}

impl EntityAnimationFile {
    fn into_definition(self) -> Result<EntityAnimationDefinition, String> {
        let clips = self
            .clips
            .into_iter()
            .map(EntityAnimationClipFile::into_definition)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(EntityAnimationDefinition {
            default_clip: self.default_clip,
            clips,
        })
    }
}

#[derive(Deserialize)]
struct EntityAnimationClipFile {
    name: String,
    duration: f32,

    #[serde(rename = "loop", default = "default_true")]
    looping: bool,

    #[serde(default)]
    tracks: Vec<EntityTransformTrackFile>,
}

impl EntityAnimationClipFile {
    fn into_definition(self) -> Result<EntityAnimationClip, String> {
        let tracks = self
            .tracks
            .into_iter()
            .map(EntityTransformTrackFile::into_definition)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(EntityAnimationClip {
            name: self.name,
            duration: self.duration,
            looping: self.looping,
            tracks,
        })
    }
}

#[derive(Deserialize, Default)]
struct EntityTransformTrackFile {
    node: String,

    #[serde(default)]
    translation: Vec<EntityVec3Keyframe>,

    #[serde(default)]
    rotation: Vec<EntityQuaternionKeyframe>,

    #[serde(default)]
    scale: Vec<EntityVec3Keyframe>,
}

impl EntityTransformTrackFile {
    fn into_definition(self) -> Result<EntityTransformTrack, String> {
        Ok(EntityTransformTrack {
            node: self.node,
            translation: self.translation,
            rotation: self.rotation,
            scale: self.scale,
        })
    }
}

#[derive(Deserialize)]
struct EntityCharacterFile {
    radius: f32,
    height: f32,

    #[serde(default)]
    default_animation: Option<String>,

    #[serde(default = "default_blend_speed")]
    blend_speed: f32,
}

impl EntityCharacterFile {
    fn into_capability(self) -> Result<EntityCharacterCapability, String> {
        Ok(EntityCharacterCapability {
            radius: self.radius,
            height: self.height,
            default_animation: self.default_animation,
            blend_speed: self.blend_speed,
        })
    }
}

fn default_blend_speed() -> f32 {
    4.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_PACKAGE_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn write_package(name: &str, contents: &str) -> std::path::PathBuf {
        let counter = TEMP_PACKAGE_COUNTER.fetch_add(1, Ordering::Relaxed);

        let root = std::env::temp_dir().join(format!(
            "Worldkiln_EntityPackage_{}_{}",
            std::process::id(),
            counter
        ));

        let dir = root.join(name);

        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), contents).unwrap();

        dir
    }

    #[test]
    fn test_entity_package_loads_generic_sections() {
        let dir = write_package(
            "door",
            r#"
            {
              "name": "Dungeon Door",
              "version": 1,

              "appearance": {
                "materials": {
                  "door": [0.4, 0.2, 0.1, 1.0]
                },

                "nodes": [
                  {
                    "name": "root"
                  }
                ],

                "geometry": [
                  {
                    "type": "cube",
                    "node": "root",
                    "material": "door",
                    "min": [-0.5, 0, -0.1],
                    "max": [0.5, 2, 0.1]
                  }
                ]
              },

              "physics": {
                "anchored": true,
                "mass": 10.0
              },

              "animation": {
                "default_clip": "closed",
                "clips": [
                  {
                    "name": "closed",
                    "duration": 1.0,
                    "loop": false,
                    "tracks": []
                  }
                ]
              }
            }
            "#,
        );

        let package = load_package(&dir).unwrap();

        assert_eq!(package.metadata.name, "Dungeon Door");
        assert_eq!(package.metadata.version, 1);
        assert_eq!(package.appearance.nodes.len(), 1);
        assert_eq!(package.appearance.geometry.len(), 1);
        assert_eq!(package.physics.mass, 10.0);
        assert!(package.physics.anchored);
        assert_eq!(package.animation.default_clip.as_deref(), Some("closed"));
        assert!(package.character.is_none());

        fs::remove_dir_all(dir.parent().unwrap()).ok();
    }

    #[test]
    fn test_entity_package_character_capability_is_optional() {
        let dir = write_package(
            "npc",
            r#"
            {
              "character": {
                "radius": 0.4,
                "height": 1.8,
                "default_animation": "idle",
                "blend_speed": 4.0
              },

              "animation": {
                "default_clip": "idle",
                "clips": [
                  {
                    "name": "idle",
                    "duration": 1.0,
                    "loop": true,
                    "tracks": []
                  }
                ]
              }
            }
            "#,
        );

        let package = load_package(&dir).unwrap();
        let character = package.character.expect("character capability");

        assert_eq!(character.radius, 0.4);
        assert_eq!(character.height, 1.8);
        assert_eq!(character.default_animation.as_deref(), Some("idle"));

        fs::remove_dir_all(dir.parent().unwrap()).ok();
    }

    #[test]
    fn test_entity_package_rejects_unknown_material() {
        let dir = write_package(
            "invalid",
            r#"
            {
              "appearance": {
                "nodes": [
                  {
                    "name": "root"
                  }
                ],

                "geometry": [
                  {
                    "type": "cube",
                    "node": "root",
                    "material": "missing",
                    "min": [0, 0, 0],
                    "max": [1, 1, 1]
                  }
                ]
              }
            }
            "#,
        );

        let error = load_package(&dir).expect_err("package should be rejected");

        assert!(error.contains("unknown material"));

        fs::remove_dir_all(dir.parent().unwrap()).ok();
    }
}
