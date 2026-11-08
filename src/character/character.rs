// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::animation::CharacterAnimation;
use super::animation::{CharacterAnimationController, TargetAnimation};
use super::collision::CharacterCollision;
use super::geometry::generate_mesh_from_parts;
use super::movement::CharacterMovement;
use super::package::{self, CharacterPackage, DEFAULT_CHARACTER_PACKAGE};
use super::rig::EvaluatedPose;
use super::transform::CharacterTransform;
use glam::{Vec2, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaterialSlot {
    Skin,
    Armor,
    Cloth,
    Detail,
    Accessory,
}

impl MaterialSlot {
    fn key(self) -> &'static str {
        match self {
            Self::Skin => "skin",
            Self::Armor => "armor",
            Self::Cloth => "cloth",
            Self::Detail => "detail",
            Self::Accessory => "accessory",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceCustomization {
    pub materials: HashMap<String, [f32; 4]>,
    pub skin_color: [f32; 4],
    pub armor_color: [f32; 4],
    pub cloth_color: [f32; 4],
    pub detail_color: [f32; 4],
    pub accessory_color: [f32; 4],
    pub show_accessory: bool,
    pub show_accent: bool,
}

impl Default for AppearanceCustomization {
    fn default() -> Self {
        let mut appearance = Self {
            materials: HashMap::new(),
            skin_color: [0.70, 0.71, 0.70, 1.0],
            armor_color: [0.46, 0.47, 0.45, 1.0],
            cloth_color: [0.10, 0.10, 0.10, 1.0],
            detail_color: [0.12, 0.13, 0.15, 1.0],
            accessory_color: [0.08, 0.30, 0.34, 1.0],
            show_accessory: true,
            show_accent: true,
        };
        appearance.sync_named_slots_into_materials();
        appearance
    }
}

impl AppearanceCustomization {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_materials(materials: HashMap<String, [f32; 4]>) -> Self {
        let mut appearance = Self {
            materials,
            ..Self::default()
        };
        appearance.pull_named_slots_from_materials();
        appearance.sync_named_slots_into_materials();
        appearance
    }

    pub fn get_color(&self, slot: MaterialSlot) -> [f32; 4] {
        self.material(slot.key())
    }

    pub fn material(&self, name: &str) -> [f32; 4] {
        if let Some(color) = self.materials.get(name) {
            return *color;
        }

        match name {
            "skin" => self.skin_color,
            "armor" => self.armor_color,
            "cloth" => self.cloth_color,
            "detail" => self.detail_color,
            "accessory" => self.accessory_color,
            _ => [1.0, 0.0, 1.0, 1.0],
        }
    }

    fn pull_named_slots_from_materials(&mut self) {
        if let Some(color) = lookup_material(&self.materials, "skin") {
            self.skin_color = color;
        }
        if let Some(color) = lookup_material(&self.materials, "armor") {
            self.armor_color = color;
        }
        if let Some(color) = lookup_material(&self.materials, "cloth") {
            self.cloth_color = color;
        }
        if let Some(color) = lookup_material(&self.materials, "detail") {
            self.detail_color = color;
        }
        if let Some(color) = lookup_material(&self.materials, "accessory") {
            self.accessory_color = color;
        }
    }

    fn sync_named_slots_into_materials(&mut self) {
        self.materials
            .entry("skin".to_string())
            .or_insert(self.skin_color);
        self.materials
            .entry("armor".to_string())
            .or_insert(self.armor_color);
        self.materials
            .entry("cloth".to_string())
            .or_insert(self.cloth_color);
        self.materials
            .entry("detail".to_string())
            .or_insert(self.detail_color);
        self.materials
            .entry("accessory".to_string())
            .or_insert(self.accessory_color);
    }
}

fn lookup_material(materials: &HashMap<String, [f32; 4]>, name: &str) -> Option<[f32; 4]> {
    materials
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, color)| *color)
}

pub struct Character {
    pub model_material_choice: Option<String>,
    pub id: u64,
    pub transform: CharacterTransform,
    pub movement: CharacterMovement,
    pub collision: CharacterCollision,
    pub collision_geometry: Option<crate::engine::collision_geometry::ColliderInstance>,
    pub collision_enabled: bool,
    pub collision_solid: bool,
    pub uses_authored_collider: bool,
    pub animation: CharacterAnimation,
    pub facing: Vec2,
    pub animation_override: Option<TargetAnimation>,
    pub package_name: String,
    pub mesh_type: String,
    pub has_gun: bool,
    pub animation_controller: CharacterAnimationController,
    pub appearance: AppearanceCustomization,
    pub current_pose: EvaluatedPose,
    pub pose_estimate_ns: u64,
    pub pose_last_frame: u64,
    pub package: Arc<CharacterPackage>,
}

impl Character {
    pub fn new(id: u64, position: Vec3) -> Self {
        Self::new_from_package(id, position, DEFAULT_CHARACTER_PACKAGE)
    }

    pub fn new_from_package(id: u64, position: Vec3, requested_package: &str) -> Self {
        let package = package::resolve_package(requested_package)
            .unwrap_or_else(|| panic!("character package '{}' was not found", requested_package));

        Self::from_package(id, position, package)
    }

    pub fn from_package(id: u64, position: Vec3, package: Arc<CharacterPackage>) -> Self {
        let mut controller = CharacterAnimationController::from_package(&package);
        let pose = controller.evaluate_pose();

        Self {
            id,
            model_material_choice: None,
            transform: CharacterTransform::new(position),
            movement: CharacterMovement::new(),
            collision: package.collision.clone(),
            collision_geometry: None,
            collision_enabled: true,
            collision_solid: true,
            uses_authored_collider: false,
            animation: CharacterAnimation::new(),
            facing: Vec2::new(0.0, 1.0),
            animation_override: None,
            package_name: package.metadata.name.clone(),
            mesh_type: package.metadata.mesh_type.clone(),
            has_gun: package.metadata.has_gun,
            animation_controller: controller,
            appearance: package.appearance.clone(),
            current_pose: pose,
            pose_estimate_ns: 100_000,
            pose_last_frame: 0,
            package,
        }
    }
}

pub fn generate_character_mesh(character: &Character) -> Vec<f32> {
    generate_mesh_from_parts(
        &character.package.geometry,
        &character.current_pose,
        &character.appearance,
        character.has_gun,
    )
}

pub fn set_model_material(
    character: &mut Character,
    attributes: &std::collections::BTreeMap<String, crate::world::AttributeValue>,
) {
    character.model_material_choice = attributes
        .get(crate::engine::entity_visual::MODEL_MATERIAL_CHOICE)
        .and_then(|value| match value {
            crate::world::AttributeValue::String(name) => Some(name),
            _ => None,
        })
        .filter(|name| {
            character
                .package
                .geometry
                .iter()
                .any(|part| &part.material == *name)
        })
        .cloned();
}

pub fn generate_character_mesh_batches(
    character: &Character,
) -> Vec<crate::engine::entity_visual::StaticEntityMeshBatch> {
    let _skin_span = crate::engine::development_capture::operation(
        "animation",
        "skeletal_mesh_generation",
        true,
    )
    .map(|span| {
        span.diagnostic(serde_json::json!({"character_id": character.id,
            "package": character.package_name}))
    });
    use super::package::GeometryPrimitive;
    let choice = character.model_material_choice.as_deref().filter(|name| {
        character
            .package
            .geometry
            .iter()
            .any(|part| part.material == *name)
    });
    let selected_part = choice.and_then(|name| {
        character
            .package
            .geometry
            .iter()
            .find(|part| part.material == name)
    });
    character
        .package
        .geometry
        .iter()
        .filter_map(|part| {
            let mut appearance = character.appearance.clone();
            if let Some(name) = choice {
                appearance
                    .materials
                    .insert(part.material.clone(), character.appearance.material(name));
            }
            let mesh = generate_mesh_from_parts(
                std::slice::from_ref(part),
                &character.current_pose,
                &appearance,
                character.has_gun,
            );
            if mesh.is_empty() {
                return None;
            }
            let mut vertices = Vec::with_capacity(mesh.len() / 10 * 12);
            for (index, vertex) in mesh.chunks_exact(10).enumerate() {
                vertices.extend_from_slice(vertex);
                let uv = match &part.primitive {
                    GeometryPrimitive::SkinnedTriangleMesh { vertices, .. } => {
                        vertices[index].tex_coord
                    }
                    _ => glam::Vec2::ZERO,
                };
                vertices.extend_from_slice(&[uv.x, uv.y]);
            }
            Some(crate::engine::entity_visual::StaticEntityMeshBatch {
                texture: selected_part.unwrap_or(part).base_color_texture.clone(),
                vertices,
            })
        })
        .collect()
}
