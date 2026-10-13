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
    pub id: u64,
    pub transform: CharacterTransform,
    pub movement: CharacterMovement,
    pub collision: CharacterCollision,
    pub animation: CharacterAnimation,
    pub facing: Vec2,
    pub health: f32,
    pub max_health: f32,
    pub animation_override: Option<TargetAnimation>,
    pub package_name: String,
    pub mesh_type: String,
    pub has_gun: bool,
    pub animation_controller: CharacterAnimationController,
    pub appearance: AppearanceCustomization,
    pub current_pose: EvaluatedPose,
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
            transform: CharacterTransform::new(position),
            movement: CharacterMovement::new(),
            collision: package.collision.clone(),
            animation: CharacterAnimation::new(),
            facing: Vec2::new(0.0, 1.0),
            health: 100.0,
            max_health: 100.0,
            animation_override: None,
            package_name: package.metadata.name.clone(),
            mesh_type: package.metadata.mesh_type.clone(),
            has_gun: package.metadata.has_gun,
            animation_controller: controller,
            appearance: package.appearance.clone(),
            current_pose: pose,
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
