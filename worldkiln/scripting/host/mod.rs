pub mod host_audio;
pub mod host_camera;
pub mod host_cells;
pub mod host_character;
pub mod host_entities;
pub mod host_input;
pub mod host_ui;

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use glam::Vec3;
use winit::keyboard::KeyCode;

use crate::engine::entity::{EntityId, EntityManager};
use crate::engine::mouse::MouseController;
use crate::scripting::api::EngineHost;
use crate::scripting::value::{DynamicProperty, HandleKind, MapKey, Value};
use crate::world::World;
use crate::world::cell::AttributeValue;

pub use host_input::key_code_name;

fn norm_path(path: &str) -> String {
    let clean = path.replace('\\', "/");

    if clean.starts_with("scripts/")
        || clean.starts_with("controllers/")
        || clean.starts_with("cameras/")
    {
        clean
    } else {
        format!("scripts/{}", clean)
    }
}

/// Runtime bridge between AeoScript and the live engine.
pub struct ScriptHostBridge<'a> {
    pub entity_manager: &'a mut EntityManager,
    pub attachment_system: Option<&'a mut crate::engine::attachment::AttachmentSystem>,
    pub world: &'a mut World,
    pub mouse: &'a mut MouseController,

    pub dynamic_properties: &'a mut std::collections::HashMap<
        (HandleKind, u64),
        std::collections::BTreeMap<String, DynamicProperty>,
    >,

    pub pending_events: &'a mut Vec<(String, Vec<Value>)>,

    pub test_results: &'a mut std::collections::BTreeMap<String, bool>,

    pub pending_enable_scripts: &'a mut Vec<String>,
    pub pending_disable_scripts: &'a mut Vec<String>,

    pub runtime_ui: &'a mut crate::engine::ui::RuntimeUi,

    pub viewport_size: [f32; 2],

    pub keys_down: Option<&'a HashSet<KeyCode>>,

    pub move_input: glam::Vec2,

    pub jump_requested: bool,

    pub orbit_delta: [f32; 2],

    pub character_system: Option<&'a mut crate::character::CharacterSystem>,

    pub actor_system: Option<&'a mut crate::actor::ActorSystem>,

    pub gameplay_camera: Option<&'a mut crate::renderer::camera::GameplayCamera>,

    pub valid_entity_declarations: Option<Arc<std::collections::HashSet<String>>>,

    pub pending_spawns: &'a mut Vec<(String, u64)>,

    pub pending_destroys: &'a mut Vec<u64>,

    pub project_path: Option<&'a std::path::Path>,

    /// Runtime-owned state for the current project's default save slot. This
    /// is intentionally separate from `project_path` and authored files.
    pub save_state: Option<&'a mut crate::engine::save_state::SaveState>,
}

impl<'a> ScriptHostBridge<'a> {
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:43 -05:00
    // An active Character is backed by a runtime Entity that may not have a
    // generic EntityPackage. Consult its associated CharacterPackage before
    // reporting a missing package to Actor/Entity scripts.
    fn script_entity_package_name(&self, entity_id: EntityId) -> Option<&str> {
        self.entity_manager
            .get_entity(entity_id)
            .and_then(|entity| entity.package_name.as_deref())
            .or_else(|| {
                self.character_system
                    .as_deref()
                    .and_then(|system| system.get_character_for_entity(entity_id))
                    .map(|character| character.package_name.as_str())
            })
    }

    pub fn get_property(
        &self,
        kind: HandleKind,
        id: u64,
        name: &str,
    ) -> Result<Option<Value>, String> {
        match kind {
            HandleKind::Mouse => {
                if id != 0 {
                    return Err("Invalid Mouse handle".to_string());
                }

                match name {
                    "setCursorVisible" => {
                        return Ok(Some(Value::Bool(self.mouse.cursor_visible)));
                    }

                    "setScreenLocked" => {
                        return Ok(Some(Value::Bool(self.mouse.screen_locked)));
                    }

                    _ => {}
                }
            }

            HandleKind::Actor => {
                let Some(entity_id) = self.resolve_actor_entity(id) else {
                    return Ok(None);
                };

                match name {
                    // Public Actor identity is the persistent authored Actor ID.
                    "id" | "actor_id" => {
                        return Ok(Some(Value::Number(id as f64)));
                    }

                    // Temporary compatibility while the old Entity-facing API
                    // still exists elsewhere in the runtime.
                    "is_actor" => {
                        return Ok(Some(Value::Bool(true)));
                    }

                    "name" => {
                        if let Some(actor) = self.world.actors.get(&id) {
                            return Ok(Some(Value::String(actor.identity.clone())));
                        }

                        if let Some(name) = self.entity_manager.get_name(entity_id) {
                            return Ok(Some(Value::String(name.to_string())));
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "position" => {
                        if let Some(position) = self.get_position(entity_id.0) {
                            return Ok(Some(Value::array(vec![
                                Value::Number(position.x as f64),
                                Value::Number(position.y as f64),
                                Value::Number(position.z as f64),
                            ])));
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "facing" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(facing) = character_system.get_entity_facing(entity_id) {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(facing.x as f64),
                                    Value::Number(facing.y as f64),
                                ])));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "rotation" => {
                        if let Some(rotation) = self.entity_manager.get_rotation(entity_id) {
                            return Ok(Some(Value::array(vec![
                                Value::Number(rotation.x as f64),
                                Value::Number(rotation.y as f64),
                                Value::Number(rotation.z as f64),
                            ])));
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "scale" => {
                        if let Some(scale) = self.entity_manager.get_scale(entity_id) {
                            return Ok(Some(Value::array(vec![
                                Value::Number(scale.x as f64),
                                Value::Number(scale.y as f64),
                                Value::Number(scale.z as f64),
                            ])));
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "visible" => {
                        return Ok(Some(
                            self.entity_manager
                                .is_visible(entity_id)
                                .map(Value::Bool)
                                .unwrap_or(Value::Nil),
                        ));
                    }

                    "enabled" => {
                        return Ok(Some(
                            self.entity_manager
                                .is_enabled(entity_id)
                                .map(Value::Bool)
                                .unwrap_or(Value::Nil),
                        ));
                    }

                    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:43 -05:00
                    "package" => {
                        return Ok(Some(
                            self.script_entity_package_name(entity_id)
                                .map(|name| Value::String(name.to_owned()))
                                .unwrap_or(Value::Nil),
                        ));
                    }

                    // Actor-backed Characters share the existing EntityAnimation
                    // capability; keep the runtime Entity ID on that handle.
                    "animation" => {
                        return Ok(Some(Value::Handle {
                            kind: HandleKind::EntityAnimation,
                            id: entity_id.0,
                        }));
                    }

                    "attributes" => {
                        let mut map = BTreeMap::new();

                        if let Some(attributes) = self.entity_manager.get_attributes(entity_id) {
                            for (key, attribute) in attributes {
                                let value = match attribute {
                                    AttributeValue::Number(number) => Value::Number(*number),
                                    AttributeValue::Bool(boolean) => Value::Bool(*boolean),
                                    AttributeValue::String(string) => Value::String(string.clone()),
                                };

                                map.insert(MapKey::String(key.clone()), value);
                            }
                        }

                        return Ok(Some(Value::map(map)));
                    }

                    _ => {
                        // Runtime Actor attributes remain accessible as normal
                        // object properties during the migration.
                        if let Some(attributes) = self.entity_manager.get_attributes(entity_id) {
                            if let Some(attribute) = attributes.get(name) {
                                let value = match attribute {
                                    AttributeValue::Number(number) => Value::Number(*number),
                                    AttributeValue::Bool(boolean) => Value::Bool(*boolean),
                                    AttributeValue::String(string) => Value::String(string.clone()),
                                };

                                return Ok(Some(value));
                            }
                        }
                    }
                }
            }

            HandleKind::Cell | HandleKind::Light => {
                if let Some(cell) = self.world.get_effective_cell_by_id(id) {
                    match name {
                        "sound" => {
                            return Ok(Some(Value::Handle {
                                kind: HandleKind::Sound,
                                id,
                            }));
                        }

                        "id" => {
                            return Ok(Some(Value::Number(cell.id as f64)));
                        }

                        "name" => {
                            return Ok(Some(Value::String(
                                cell.entity_identity
                                    .clone()
                                    .unwrap_or_else(|| "Cell".to_string()),
                            )));
                        }

                        "cellType" => {
                            return Ok(Some(Value::String(format!("{:?}", cell.cell_type))));
                        }

                        "position" => {
                            if let Some(position) = self.world.source_position(id) {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(position.x as f64),
                                    Value::Number(position.y as f64),
                                    Value::Number(position.z as f64),
                                ])));
                            }
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(coord.x as f64),
                                    Value::Number(coord.y as f64),
                                    Value::Number(coord.z as f64),
                                ])));
                            }

                            return Ok(Some(Value::Nil));
                        }

                        "visible" => {
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                return Ok(Some(Value::Bool(self.world.is_cell_visible(coord))));
                            }

                            if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                if let Some(value) = runtime_state.visible {
                                    return Ok(Some(Value::Bool(value)));
                                }
                            }

                            return Ok(Some(Value::Bool(cell.visible)));
                        }

                        "enabled" => {
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                return Ok(Some(Value::Bool(self.world.is_light_enabled(coord))));
                            }

                            if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                if let Some(value) = runtime_state.light_enabled {
                                    return Ok(Some(Value::Bool(value)));
                                }
                            }

                            return Ok(Some(Value::Bool(cell.light_enabled)));
                        }

                        "solid" => {
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                return Ok(Some(Value::Bool(self.world.is_cell_solid(coord))));
                            }

                            if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                if let Some(value) = runtime_state.solid {
                                    return Ok(Some(Value::Bool(value)));
                                }
                            }

                            return Ok(Some(Value::Bool(cell.solid)));
                        }

                        "anchored" => {
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                return Ok(Some(Value::Bool(self.world.is_cell_anchored(coord))));
                            }

                            if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                if let Some(value) = runtime_state.anchored {
                                    return Ok(Some(Value::Bool(value)));
                                }
                            }

                            return Ok(Some(Value::Bool(cell.anchored)));
                        }

                        "color" => {
                            let color = if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.get_effective_color(coord)
                            } else if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                runtime_state.color_rgb.unwrap_or(cell.color_rgb)
                            } else {
                                cell.color_rgb
                            };

                            return Ok(Some(Value::array(vec![
                                Value::Number(color.x as f64),
                                Value::Number(color.y as f64),
                                Value::Number(color.z as f64),
                            ])));
                        }

                        "offset" => {
                            let offset = if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.get_visual_offset(coord)
                            } else if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                runtime_state.visual_offset.unwrap_or(Vec3::ZERO)
                            } else {
                                Vec3::ZERO
                            };

                            return Ok(Some(Value::array(vec![
                                Value::Number(offset.x as f64),
                                Value::Number(offset.y as f64),
                                Value::Number(offset.z as f64),
                            ])));
                        }

                        "attributes" => {
                            let mut map = BTreeMap::new();

                            for (key, attribute) in &cell.attributes {
                                let value = match attribute {
                                    AttributeValue::Number(n) => Value::Number(*n),
                                    AttributeValue::Bool(b) => Value::Bool(*b),
                                    AttributeValue::String(s) => Value::String(s.clone()),
                                };

                                map.insert(MapKey::String(key.clone()), value);
                            }

                            if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                                for (key, attribute) in &runtime_state.attribute_overrides {
                                    let value = match attribute {
                                        AttributeValue::Number(n) => Value::Number(*n),
                                        AttributeValue::Bool(b) => Value::Bool(*b),
                                        AttributeValue::String(s) => Value::String(s.clone()),
                                    };

                                    map.insert(MapKey::String(key.clone()), value);
                                }
                            }

                            return Ok(Some(Value::map(map)));
                        }

                        _ => {}
                    }
                }
            }

            HandleKind::Entity => {
                let entity_id = EntityId(id);

                if !self.entity_manager.validate_handle(id) {
                    return Ok(None);
                }

                match name {
                    "sound" => {
                        return Ok(Some(Value::Handle {
                            kind: HandleKind::Sound,
                            id,
                        }));
                    }

                    "id" => {
                        return Ok(Some(Value::Number(id as f64)));
                    }

                    "name" => {
                        if let Some(entity_name) = self.entity_manager.get_name(entity_id) {
                            return Ok(Some(Value::String(entity_name.to_string())));
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "position" => {
                        if let Some(position) = self.get_position(id) {
                            return Ok(Some(Value::array(vec![
                                Value::Number(position.x as f64),
                                Value::Number(position.y as f64),
                                Value::Number(position.z as f64),
                            ])));
                        }

                        return Ok(Some(Value::Nil));
                    }

                    // Visibility and enabled state are generic Entity
                    // capabilities. `get_entity()` may return the runtime
                    // Entity backing an authored Actor, so keep these members
                    // in parity with Actor handles instead of forcing scripts
                    // to rediscover the same object through another API.
                    "visible" => {
                        return Ok(Some(
                            self.entity_manager
                                .is_visible(entity_id)
                                .map(Value::Bool)
                                .unwrap_or(Value::Nil),
                        ));
                    }

                    "enabled" => {
                        return Ok(Some(
                            self.entity_manager
                                .is_enabled(entity_id)
                                .map(Value::Bool)
                                .unwrap_or(Value::Nil),
                        ));
                    }

                    "physics" => {
                        return Ok(Some(Value::Handle {
                            kind: HandleKind::EntityPhysics,
                            id,
                        }));
                    }

                    "appearance" => {
                        return Ok(Some(Value::Handle {
                            kind: HandleKind::EntityAppearance,
                            id,
                        }));
                    }

                    "animation" => {
                        return Ok(Some(Value::Handle {
                            kind: HandleKind::EntityAnimation,
                            id,
                        }));
                    }

                    "velocity" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(velocity) = character_system.get_entity_velocity(entity_id)
                            {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(velocity.x as f64),
                                    Value::Number(velocity.y as f64),
                                    Value::Number(velocity.z as f64),
                                ])));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "facing" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(facing) = character_system.get_entity_facing(entity_id) {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(facing.x as f64),
                                    Value::Number(facing.y as f64),
                                ])));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "grounded" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(grounded) = character_system.is_entity_grounded(entity_id) {
                                return Ok(Some(Value::Bool(grounded)));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "animation_name" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(animation) =
                                character_system.get_entity_animation(entity_id)
                            {
                                return Ok(Some(Value::String(animation.to_string())));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "health" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(health) = character_system.get_entity_health(entity_id) {
                                return Ok(Some(Value::Number(health as f64)));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "max_health" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(max_health) =
                                character_system.get_entity_max_health(entity_id)
                            {
                                return Ok(Some(Value::Number(max_health as f64)));
                            }
                        }

                        return Ok(Some(Value::Nil));
                    }

                    "alive" => {
                        if let Some(character_system) = self.character_system.as_deref() {
                            if let Some(alive) = character_system.is_entity_alive(entity_id) {
                                return Ok(Some(Value::Bool(alive)));
                            }
                        }

                        return Ok(Some(Value::Bool(true)));
                    }

                    "is_actor" => {
                        let is_act = self
                            .actor_system
                            .as_ref()
                            .map_or(false, |sys| sys.is_actor(entity_id));
                        return Ok(Some(Value::Bool(is_act)));
                    }

                    "actor_id" => {
                        if let Some(actor_id) = self
                            .actor_system
                            .as_ref()
                            .and_then(|sys| sys.authored_actor_id_for_entity(entity_id))
                        {
                            return Ok(Some(Value::Number(actor_id as f64)));
                        }
                        return Ok(Some(Value::Nil));
                    }

                    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:43 -05:00
                    "package" => {
                        return Ok(Some(
                            self.script_entity_package_name(entity_id)
                                .map(|name| Value::String(name.to_owned()))
                                .unwrap_or(Value::Nil),
                        ));
                    }

                    "attributes" => {
                        if let Some(attributes) = self.entity_manager.get_attributes(entity_id) {
                            let mut map = std::collections::BTreeMap::new();
                            for (key, attribute) in attributes {
                                let value = match attribute {
                                    crate::world::cell::AttributeValue::Number(n) => {
                                        Value::Number(*n)
                                    }
                                    crate::world::cell::AttributeValue::Bool(b) => Value::Bool(*b),
                                    crate::world::cell::AttributeValue::String(s) => {
                                        Value::String(s.clone())
                                    }
                                };
                                map.insert(
                                    crate::scripting::value::MapKey::String(key.clone()),
                                    value,
                                );
                            }
                            return Ok(Some(Value::map(map)));
                        }
                        return Ok(Some(Value::map(std::collections::BTreeMap::new())));
                    }

                    _ => {
                        if let Some(attributes) = self.entity_manager.get_attributes(entity_id) {
                            if let Some(attr) = attributes.get(name) {
                                let val = match attr {
                                    crate::world::cell::AttributeValue::Number(n) => {
                                        Value::Number(*n)
                                    }
                                    crate::world::cell::AttributeValue::Bool(b) => Value::Bool(*b),
                                    crate::world::cell::AttributeValue::String(s) => {
                                        Value::String(s.clone())
                                    }
                                };
                                return Ok(Some(val));
                            }
                        }
                    }
                }
            }

            HandleKind::EntityPhysics
            | HandleKind::EntityAppearance
            | HandleKind::EntityAnimation => {
                if !self.entity_manager.validate_handle(id) {
                    return Ok(None);
                }

                match kind {
                    HandleKind::EntityPhysics => {
                        let physics = self
                            .entity_manager
                            .get_physics(EntityId(id))
                            .ok_or_else(|| "Invalid Entity physics handle".to_string())?;

                        match name {
                            "entity" => {
                                return Ok(Some(Value::Handle {
                                    kind: HandleKind::Entity,
                                    id,
                                }));
                            }

                            "kind" => {
                                return Ok(Some(Value::String(kind.name().to_string())));
                            }

                            "enabled" => {
                                return Ok(Some(Value::Bool(physics.enabled)));
                            }

                            "solid" => {
                                return Ok(Some(Value::Bool(physics.solid)));
                            }

                            "anchored" => {
                                return Ok(Some(Value::Bool(physics.anchored)));
                            }

                            "collision_enabled" => {
                                return Ok(Some(Value::Bool(physics.collision_enabled)));
                            }

                            "mass" => {
                                return Ok(Some(Value::Number(physics.mass as f64)));
                            }

                            "velocity" => {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(physics.velocity.x as f64),
                                    Value::Number(physics.velocity.y as f64),
                                    Value::Number(physics.velocity.z as f64),
                                ])));
                            }

                            "gravity_scale" => {
                                return Ok(Some(Value::Number(physics.gravity_scale as f64)));
                            }

                            "size" => {
                                return Ok(Some(Value::array(vec![
                                    Value::Number(physics.size.x as f64),
                                    Value::Number(physics.size.y as f64),
                                    Value::Number(physics.size.z as f64),
                                ])));
                            }

                            "grounded" => {
                                return Ok(Some(Value::Bool(physics.grounded)));
                            }

                            _ => {}
                        }
                    }

                    HandleKind::EntityAppearance | HandleKind::EntityAnimation => match name {
                        "entity" => {
                            return Ok(Some(Value::Handle {
                                kind: HandleKind::Entity,
                                id,
                            }));
                        }

                        "kind" => {
                            return Ok(Some(Value::String(kind.name().to_string())));
                        }

                        _ => {}
                    },

                    _ => {}
                }
            }

            HandleKind::Sound => match name {
                "playing" => {
                    return Ok(Some(Value::Bool(
                        self.world.is_audio_playing(id) && !self.world.is_audio_paused(id),
                    )));
                }

                "looped" => {
                    return Ok(Some(Value::Bool(self.world.is_audio_looped(id))));
                }

                "volume" => {
                    return Ok(Some(Value::Number(self.world.get_audio_volume(id) as f64)));
                }

                _ => {}
            },

            HandleKind::Ui => {}
        }

        Ok(None)
    }

    pub fn set_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: &str,
        value: Value,
    ) -> Result<bool, String> {
        match kind {
            HandleKind::Mouse => {
                if id != 0 {
                    return Err("Invalid Mouse handle".to_string());
                }

                match name {
                    "setCursorVisible" => {
                        self.mouse.cursor_visible = value.as_bool()?;
                        return Ok(true);
                    }

                    "setScreenLocked" => {
                        self.mouse.screen_locked = value.as_bool()?;
                        return Ok(true);
                    }

                    _ => {}
                }
            }

            HandleKind::Actor => {
                let entity_id = self
                    .resolve_actor_entity(id)
                    .ok_or_else(|| "Invalid Actor handle".to_string())?;

                match name {
                    "position" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 3 {
                            return Err("Actor.position must be [x, y, z]".to_string());
                        }

                        let position = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );

                        self.set_position(entity_id.0, position);

                        return Ok(true);
                    }

                    "rotation" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 3 {
                            return Err("Actor.rotation must be [x, y, z]".to_string());
                        }

                        let rotation = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );

                        self.entity_manager.set_rotation(entity_id, rotation);

                        return Ok(true);
                    }

                    "scale" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 3 {
                            return Err("Actor.scale must be [x, y, z]".to_string());
                        }

                        let scale = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );

                        self.entity_manager.set_scale(entity_id, scale);

                        return Ok(true);
                    }

                    "visible" => {
                        self.entity_manager.set_visible(entity_id, value.as_bool()?);

                        return Ok(true);
                    }

                    "enabled" => {
                        self.entity_manager.set_enabled(entity_id, value.as_bool()?);

                        return Ok(true);
                    }

                    "id" | "actor_id" | "name" | "package" | "is_actor" => {
                        return Err(format!("Actor.{} is read-only", name));
                    }

                    "attributes" => {
                        return Err("Actor.attributes cannot be replaced directly".to_string());
                    }

                    _ => {}
                }
            }

            HandleKind::Cell | HandleKind::Light => {
                if self.world.get_effective_cell_by_id(id).is_some() {
                    match name {
                        "position" => {
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err(
                                    "position must be a basket of 3 numbers [x, y, z]".to_string()
                                );
                            }

                            let x = borrowed.elements[0].as_number()? as f32;
                            let y = borrowed.elements[1].as_number()? as f32;
                            let z = borrowed.elements[2].as_number()? as f32;

                            if self.world.source(id).is_some() {
                                self.world
                                    .runtime_state
                                    .entry(id)
                                    .or_default()
                                    .source_position = Some(Vec3::new(x, y, z));
                                return Ok(true);
                            }

                            self.move_runtime_cell(id, x as i32, y as i32, z as i32)?;

                            return Ok(true);
                        }

                        "name" => {
                            let name = value.as_string()?;

                            if let Some(cell) = self.world.runtime_cells.get_mut(&id) {
                                cell.entity_identity = Some(name.to_string());
                            } else {
                                return Err(
                                    "Cannot change name of an authored cell at runtime".to_string()
                                );
                            }

                            return Ok(true);
                        }

                        "visible" => {
                            let visible = value.as_bool()?;

                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_visible_runtime(coord, visible);
                            } else {
                                self.world.runtime_state.entry(id).or_default().visible =
                                    Some(visible);
                            }

                            return Ok(true);
                        }

                        "enabled" => {
                            let enabled = value.as_bool()?;

                            // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
                            self.world.set_light_enabled_by_id_runtime(id, enabled);

                            return Ok(true);
                        }

                        "solid" => {
                            let solid = value.as_bool()?;

                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_solid_runtime(coord, solid);
                            } else {
                                self.world.runtime_state.entry(id).or_default().solid = Some(solid);
                            }

                            return Ok(true);
                        }

                        "anchored" => {
                            let anchored = value.as_bool()?;

                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_anchored_runtime(coord, anchored);
                            } else {
                                self.world.runtime_state.entry(id).or_default().anchored =
                                    Some(anchored);
                            }

                            return Ok(true);
                        }

                        "color" => {
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err(
                                    "color must be a basket of 3 numbers [r, g, b]".to_string()
                                );
                            }

                            let color = Vec3::new(
                                borrowed.elements[0].as_number()? as f32,
                                borrowed.elements[1].as_number()? as f32,
                                borrowed.elements[2].as_number()? as f32,
                            );

                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_color_runtime(coord, color);
                            } else {
                                self.world.runtime_state.entry(id).or_default().color_rgb =
                                    Some(color);
                            }

                            return Ok(true);
                        }

                        "offset" => {
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err(
                                    "offset must be a basket of 3 numbers [x, y, z]".to_string()
                                );
                            }

                            let offset = Vec3::new(
                                borrowed.elements[0].as_number()? as f32,
                                borrowed.elements[1].as_number()? as f32,
                                borrowed.elements[2].as_number()? as f32,
                            );

                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_visual_offset_runtime(coord, offset);
                            } else {
                                self.world
                                    .runtime_state
                                    .entry(id)
                                    .or_default()
                                    .visual_offset = Some(offset);
                            }

                            return Ok(true);
                        }

                        _ => {}
                    }
                }
            }

            HandleKind::Entity => {
                let entity_id = EntityId(id);

                if !self.entity_manager.validate_handle(id) {
                    return Err("Invalid Entity handle".to_string());
                }

                match name {
                    "position" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 3 {
                            return Err("Entity.position must be [x, y, z]".to_string());
                        }

                        let position = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );

                        self.set_position(id, position);

                        return Ok(true);
                    }

                    "visible" => {
                        self.entity_manager.set_visible(entity_id, value.as_bool()?);
                        return Ok(true);
                    }

                    "enabled" => {
                        self.entity_manager.set_enabled(entity_id, value.as_bool()?);
                        return Ok(true);
                    }

                    "velocity" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 3 {
                            return Err("Entity.velocity must be [x, y, z]".to_string());
                        }

                        let velocity = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_velocity(entity_id, velocity) {
                            return Err("Entity does not reference a runtime character".to_string());
                        }

                        return Ok(true);
                    }

                    "facing" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 2 {
                            return Err("Entity.facing must be [x, z]".to_string());
                        }

                        let x = borrowed.elements[0].as_number()? as f32;
                        let z = borrowed.elements[1].as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_facing_direction(entity_id, x, z) {
                            return Err("Entity does not reference a runtime character".to_string());
                        }

                        return Ok(true);
                    }

                    "animation_name" => {
                        let animation = value.as_string()?;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        character_system.set_entity_animation(entity_id, animation)?;

                        return Ok(true);
                    }

                    "physics" | "appearance" | "animation" => {
                        return Err(format!(
                            "Entity.{} is a capability handle and cannot be assigned directly",
                            name
                        ));
                    }

                    "health" => {
                        let health = value.as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_health(entity_id, health) {
                            return Err("Entity does not reference a runtime character".to_string());
                        }

                        return Ok(true);
                    }

                    "max_health" => {
                        let max_health = value.as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_max_health(entity_id, max_health) {
                            return Err("Entity does not reference a runtime character".to_string());
                        }

                        return Ok(true);
                    }

                    "id" | "name" | "grounded" | "alive" => {
                        return Err(format!("Entity.{} is read-only", name));
                    }

                    _ => {}
                }
            }

            HandleKind::EntityPhysics
            | HandleKind::EntityAppearance
            | HandleKind::EntityAnimation => {
                if !self.entity_manager.validate_handle(id) {
                    return Err("Invalid Entity capability handle".to_string());
                }

                match kind {
                    HandleKind::EntityPhysics => match name {
                        "enabled" => {
                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .enabled = value.as_bool()?;

                            return Ok(true);
                        }

                        "solid" => {
                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .solid = value.as_bool()?;

                            return Ok(true);
                        }

                        "anchored" => {
                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .anchored = value.as_bool()?;

                            return Ok(true);
                        }

                        "collision_enabled" => {
                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .collision_enabled = value.as_bool()?;

                            return Ok(true);
                        }

                        "mass" => {
                            let mass = value.as_number()? as f32;

                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .set_mass(mass)?;

                            return Ok(true);
                        }

                        "velocity" => {
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err("EntityPhysics.velocity must be [x, y, z]".to_string());
                            }

                            let velocity = Vec3::new(
                                borrowed.elements[0].as_number()? as f32,
                                borrowed.elements[1].as_number()? as f32,
                                borrowed.elements[2].as_number()? as f32,
                            );

                            if !velocity.is_finite() {
                                return Err(
                                    "EntityPhysics.velocity received non-finite values".to_string()
                                );
                            }

                            self.entity_manager
                                .set_physics_velocity(EntityId(id), velocity);

                            return Ok(true);
                        }

                        "gravity_scale" => {
                            let gravity_scale = value.as_number()? as f32;

                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .set_gravity_scale(gravity_scale)?;

                            return Ok(true);
                        }

                        "size" => {
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err("EntityPhysics.size must be [x, y, z]".to_string());
                            }

                            let size = Vec3::new(
                                borrowed.elements[0].as_number()? as f32,
                                borrowed.elements[1].as_number()? as f32,
                                borrowed.elements[2].as_number()? as f32,
                            );

                            self.entity_manager
                                .get_physics_mut(EntityId(id))
                                .ok_or_else(|| "Invalid Entity physics handle".to_string())?
                                .set_size(size)?;

                            return Ok(true);
                        }

                        "grounded" => {
                            return Err("EntityPhysics.grounded is read-only".to_string());
                        }

                        "entity" | "kind" => {
                            return Err(format!("{}.{} is read-only", kind.name(), name));
                        }

                        _ => {}
                    },

                    HandleKind::EntityAppearance | HandleKind::EntityAnimation => {
                        if name == "entity" || name == "kind" {
                            return Err(format!("{}.{} is read-only", kind.name(), name));
                        }

                        return Err(format!("{}.{} is not implemented yet", kind.name(), name));
                    }

                    _ => {}
                }
            }

            HandleKind::Sound => match name {
                "playing" => {
                    let playing = value.as_bool()?;
                    self.world.set_audio_playing_runtime(id, playing);
                    return Ok(true);
                }

                "looped" => {
                    let looped = value.as_bool()?;
                    self.world.set_audio_looped_runtime(id, looped);
                    return Ok(true);
                }

                "volume" => {
                    let volume = value.as_number()? as f32;
                    self.world.set_audio_volume_runtime(id, volume);
                    return Ok(true);
                }

                _ => {}
            },

            HandleKind::Ui => {}
        }

        Ok(false)
    }

    pub fn call_method(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: &str,
        arguments: &[Value],
    ) -> Result<Option<Value>, String> {
        if kind == HandleKind::Sound {
            match name {
                "play" => {
                    self.world.audio_play_runtime(id);
                    return Ok(Some(Value::Nil));
                }

                "stop" => {
                    self.world.audio_stop_runtime(id);
                    return Ok(Some(Value::Nil));
                }

                "pause" => {
                    self.world.audio_pause_runtime(id);
                    return Ok(Some(Value::Nil));
                }

                _ => {}
            }
        }

        match kind {
            HandleKind::Cell | HandleKind::Light => match name {
                "getObject" => {
                    if !arguments.is_empty() {
                        return Err(format!("{}.getObject() expects 0 arguments", kind.name()));
                    }

                    if let Some((kind, id)) = self.get_cell_object(id) {
                        Ok(Some(Value::Handle { kind, id }))
                    } else {
                        Ok(Some(Value::Nil))
                    }
                }

                "is_enabled" => {
                    if !arguments.is_empty() {
                        return Err(format!("{}.is_enabled() expects 0 arguments", kind.name()));
                    }

                    Ok(self
                        .is_light_enabled(id)
                        .map(Value::Bool)
                        .or(Some(Value::Nil)))
                }

                "set_enabled" => {
                    if arguments.len() != 1 {
                        return Err(format!(
                            "{}.set_enabled() expects 1 argument (bool)",
                            kind.name()
                        ));
                    }

                    let enabled = arguments[0].as_bool()?;
                    self.set_light_enabled(id, enabled);

                    Ok(Some(Value::Nil))
                }

                _ => Ok(None),
            },

            HandleKind::Actor => {
                if name == "is_valid" {
                    if !arguments.is_empty() {
                        return Err("Actor.is_valid() expects 0 arguments".to_string());
                    }

                    return Ok(Some(Value::Bool(self.actor_exists(id))));
                }

                let entity_id = self
                    .resolve_actor_entity(id)
                    .ok_or_else(|| "Actor method called on an invalid handle".to_string())?;

                match name {
                    "name" => {
                        if !arguments.is_empty() {
                            return Err("Actor.name() expects 0 arguments".to_string());
                        }

                        if let Some(actor) = self.world.actors.get(&id) {
                            return Ok(Some(Value::String(actor.identity.clone())));
                        }

                        if let Some(name) = self.entity_manager.get_name(entity_id) {
                            return Ok(Some(Value::String(name.to_string())));
                        }

                        Ok(Some(Value::Nil))
                    }

                    "set_position" => {
                        if arguments.len() != 3 {
                            return Err(
                                "Actor.set_position() expects 3 arguments (x, y, z)".to_string()
                            );
                        }

                        let position = Vec3::new(
                            arguments[0].as_number()? as f32,
                            arguments[1].as_number()? as f32,
                            arguments[2].as_number()? as f32,
                        );

                        self.set_position(entity_id.0, position);

                        Ok(Some(Value::Nil))
                    }

                    "translate" => {
                        if arguments.len() != 3 {
                            return Err(
                                "Actor.translate() expects 3 arguments (x, y, z)".to_string()
                            );
                        }

                        let offset = Vec3::new(
                            arguments[0].as_number()? as f32,
                            arguments[1].as_number()? as f32,
                            arguments[2].as_number()? as f32,
                        );

                        if let Some(position) = self.get_position(entity_id.0) {
                            self.set_position(entity_id.0, position + offset);
                        }

                        Ok(Some(Value::Nil))
                    }

                    "set_horizontal_velocity" => {
                        if arguments.len() != 2 {
                            return Err(
                                "Actor.set_horizontal_velocity() expects 2 arguments (vx, vz)"
                                    .to_string(),
                            );
                        }

                        let vx = arguments[0].as_number()? as f32;
                        let vz = arguments[1].as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_horizontal_velocity(entity_id, vx, vz) {
                            return Err("Actor is not a Character".to_string());
                        }

                        Ok(Some(Value::Nil))
                    }

                    "navigate_to" => {
                        if !(1..=3).contains(&arguments.len()) {
                            return Err(
                                "Actor.navigate_to() expects target [, speed [, stop_distance]]"
                                    .to_string(),
                            );
                        }
                        let target = arguments[0].as_basket()?;
                        let target = target.borrow();
                        if target.elements.len() != 3 {
                            return Err("Actor.navigate_to() target must be [x, y, z]".to_string());
                        }
                        let target = Vec3::new(
                            target.elements[0].as_number()? as f32,
                            target.elements[1].as_number()? as f32,
                            target.elements[2].as_number()? as f32,
                        );
                        let speed = arguments
                            .get(1)
                            .map(Value::as_number)
                            .transpose()?
                            .unwrap_or(4.0) as f32;
                        let stop_distance = arguments
                            .get(2)
                            .map(Value::as_number)
                            .transpose()?
                            .unwrap_or(0.1) as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };
                        match character_system.begin_entity_navigation(
                            entity_id,
                            self.world,
                            target,
                            speed,
                            stop_distance,
                        ) {
                            Ok(()) => Ok(Some(Value::Bool(true))),
                            Err(crate::character::system::navigation_control::NavigationFollowError::NoPath(reason)) => {
                                self.pending_events.push((
                                    "navigation_failed".to_string(),
                                    vec![
                                        Value::Handle {
                                            kind: HandleKind::Actor,
                                            id,
                                        },
                                        Value::String(reason.to_string()),
                                    ],
                                ));
                                Ok(Some(Value::Bool(false)))
                            }
                            Err(error) => Err(error.to_string()),
                        }
                    }

                    "cancel_navigation" => {
                        if !arguments.is_empty() {
                            return Err("Actor.cancel_navigation() expects 0 arguments".to_string());
                        }
                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };
                        Ok(Some(Value::Bool(
                            character_system.cancel_entity_navigation(entity_id),
                        )))
                    }

                    "is_navigating" => {
                        if !arguments.is_empty() {
                            return Err("Actor.is_navigating() expects 0 arguments".to_string());
                        }
                        let Some(character_system) = self.character_system.as_deref() else {
                            return Err("CharacterSystem not available".to_string());
                        };
                        Ok(Some(Value::Bool(
                            character_system.is_entity_navigating(entity_id),
                        )))
                    }

                    "set_facing_direction" => {
                        if arguments.len() != 2 {
                            return Err(
                                "Actor.set_facing_direction() expects 2 arguments (dx, dz)"
                                    .to_string(),
                            );
                        }

                        let dx = arguments[0].as_number()? as f32;
                        let dz = arguments[1].as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_facing_direction(entity_id, dx, dz) {
                            return Err("Actor is not a Character".to_string());
                        }

                        Ok(Some(Value::Nil))
                    }

                    "select_animation" => {
                        if arguments.len() != 1 {
                            return Err(
                                "Actor.select_animation() expects 1 argument (name)".to_string()
                            );
                        }

                        let animation = arguments[0].as_string()?;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.set_entity_animation(entity_id, animation)? {
                            return Err("Actor is not a Character".to_string());
                        }

                        Ok(Some(Value::Nil))
                    }

                    "is_grounded" => {
                        if !arguments.is_empty() {
                            return Err("Actor.is_grounded() expects 0 arguments".to_string());
                        }

                        let Some(character_system) = self.character_system.as_deref() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        let grounded = character_system
                            .is_entity_grounded(entity_id)
                            .ok_or_else(|| "Actor is not a Character".to_string())?;

                        Ok(Some(Value::Bool(grounded)))
                    }

                    "apply_vertical_impulse" => {
                        if arguments.len() != 1 {
                            return Err(
                                "Actor.apply_vertical_impulse() expects 1 argument (impulse)"
                                    .to_string(),
                            );
                        }

                        let impulse = arguments[0].as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.apply_entity_vertical_impulse(entity_id, impulse) {
                            return Err("Actor is not a Character".to_string());
                        }

                        Ok(Some(Value::Nil))
                    }

                    "destroy" => {
                        if !arguments.is_empty() {
                            return Err("Actor.destroy() expects 0 arguments".to_string());
                        }

                        self.destroy_entity_runtime(entity_id.0);

                        Ok(Some(Value::Bool(true)))
                    }

                    _ => Ok(None),
                }
            }

            HandleKind::EntityPhysics
            | HandleKind::EntityAppearance
            | HandleKind::EntityAnimation => {
                if name == "is_valid" {
                    if !arguments.is_empty() {
                        return Err(format!("{}.is_valid() expects 0 arguments", kind.name()));
                    }

                    return Ok(Some(Value::Bool(self.entity_manager.validate_handle(id))));
                }

                if !self.entity_manager.validate_handle(id) {
                    return Err(format!(
                        "{} method called on an invalid capability handle",
                        kind.name()
                    ));
                }

                match kind {
                    HandleKind::EntityPhysics => match name {
                        "apply_force" => {
                            if arguments.len() != 1 {
                                return Err(
                                    "EntityPhysics.apply_force() expects 1 argument ([x, y, z])"
                                        .to_string(),
                                );
                            }

                            let basket = arguments[0].as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err(
                                    "EntityPhysics.apply_force() expects [x, y, z]".to_string()
                                );
                            }

                            let force = Vec3::new(
                                borrowed.elements[0].as_number()? as f32,
                                borrowed.elements[1].as_number()? as f32,
                                borrowed.elements[2].as_number()? as f32,
                            );

                            if !force.is_finite() {
                                return Err(
                                    "EntityPhysics.apply_force() received non-finite values"
                                        .to_string(),
                                );
                            }

                            self.entity_manager.apply_physics_force(EntityId(id), force);

                            Ok(Some(Value::Nil))
                        }

                        "apply_impulse" => {
                            if arguments.len() != 1 {
                                return Err(
                                    "EntityPhysics.apply_impulse() expects 1 argument ([x, y, z])"
                                        .to_string(),
                                );
                            }

                            let basket = arguments[0].as_basket()?;
                            let borrowed = basket.borrow();

                            if borrowed.elements.len() != 3 {
                                return Err(
                                    "EntityPhysics.apply_impulse() expects [x, y, z]".to_string()
                                );
                            }

                            let impulse = Vec3::new(
                                borrowed.elements[0].as_number()? as f32,
                                borrowed.elements[1].as_number()? as f32,
                                borrowed.elements[2].as_number()? as f32,
                            );

                            if !impulse.is_finite() {
                                return Err(
                                    "EntityPhysics.apply_impulse() received non-finite values"
                                        .to_string(),
                                );
                            }

                            self.entity_manager
                                .apply_physics_impulse(EntityId(id), impulse);

                            Ok(Some(Value::Nil))
                        }

                        "stop" => {
                            if !arguments.is_empty() {
                                return Err("EntityPhysics.stop() expects 0 arguments".to_string());
                            }

                            self.entity_manager.stop_physics(EntityId(id));

                            Ok(Some(Value::Nil))
                        }

                        "wake" => {
                            if !arguments.is_empty() {
                                return Err("EntityPhysics.wake() expects 0 arguments".to_string());
                            }

                            self.entity_manager.wake_physics(EntityId(id));

                            Ok(Some(Value::Nil))
                        }

                        _ => Ok(None),
                    },

                    HandleKind::EntityAnimation if name == "play_layer" => {
                        if arguments.len() != 3 {
                            return Err("EntityAnimation.play_layer() expects 3 arguments (slot, clip, joint_root)".to_string());
                        }
                        let slot = arguments[0].as_string()?;
                        let clip = arguments[1].as_string()?;
                        let joint_root = arguments[2].as_string()?;
                        let character_system = self
                            .character_system
                            .as_deref_mut()
                            .ok_or_else(|| "CharacterSystem not available".to_string())?;
                        let character = character_system
                            .get_character_for_entity_mut(EntityId(id))
                            .ok_or_else(|| {
                                "Entity does not reference a runtime character".to_string()
                            })?;
                        character
                            .animation_controller
                            .play_layer(slot, clip, joint_root)?;
                        Ok(Some(Value::Bool(true)))
                    }

                    HandleKind::EntityAppearance | HandleKind::EntityAnimation => Ok(None),

                    _ => Ok(None),
                }
            }

            HandleKind::Entity => {
                let entity_id = EntityId(id);

                if name == "is_valid" {
                    if !arguments.is_empty() {
                        return Err("entity.is_valid() expects 0 arguments".to_string());
                    }

                    return Ok(Some(Value::Bool(self.entity_manager.validate_handle(id))));
                }

                if !self.entity_manager.validate_handle(id) {
                    return Err("entity method called on an invalid handle".to_string());
                }

                match name {
                    "name" => {
                        if !arguments.is_empty() {
                            return Err("entity.name() expects 0 arguments".to_string());
                        }

                        if let Some(entity_name) = self.entity_manager.get_name(entity_id) {
                            Ok(Some(Value::String(entity_name.to_string())))
                        } else {
                            Ok(Some(Value::Nil))
                        }
                    }

                    "set_position" => {
                        if arguments.len() != 3 {
                            return Err(
                                "entity.set_position() expects 3 arguments (x, y, z)".to_string()
                            );
                        }

                        let x = arguments[0].as_number()? as f32;
                        let y = arguments[1].as_number()? as f32;
                        let z = arguments[2].as_number()? as f32;

                        self.set_position(id, Vec3::new(x, y, z));

                        Ok(Some(Value::Nil))
                    }

                    "translate" => {
                        if arguments.len() != 3 {
                            return Err(
                                "entity.translate() expects 3 arguments (x, y, z)".to_string()
                            );
                        }

                        let dx = arguments[0].as_number()? as f32;
                        let dy = arguments[1].as_number()? as f32;
                        let dz = arguments[2].as_number()? as f32;

                        if let Some(position) = self.get_position(id) {
                            self.set_position(id, position + Vec3::new(dx, dy, dz));
                        }

                        Ok(Some(Value::Nil))
                    }

                    "jump" => {
                        if arguments.len() > 1 {
                            return Err(
                                "entity.jump() expects 0 or 1 argument (impulse)".to_string()
                            );
                        }

                        let impulse = if arguments.is_empty() {
                            crate::character::movement::JUMP_IMPULSE
                        } else {
                            arguments[0].as_number()? as f32
                        };

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        if !character_system.jump_entity(entity_id, impulse) {
                            return Err("Entity does not reference a runtime character".to_string());
                        }

                        Ok(Some(Value::Nil))
                    }

                    "damage" => {
                        if arguments.len() != 1 {
                            return Err("entity.damage() expects 1 argument (amount)".to_string());
                        }

                        let amount = arguments[0].as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        let Some(health) = character_system.damage_entity(entity_id, amount) else {
                            return Err("Entity does not reference a runtime character".to_string());
                        };

                        Ok(Some(Value::Number(health as f64)))
                    }

                    "heal" => {
                        if arguments.len() != 1 {
                            return Err("entity.heal() expects 1 argument (amount)".to_string());
                        }

                        let amount = arguments[0].as_number()? as f32;

                        let Some(character_system) = self.character_system.as_deref_mut() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        let Some(health) = character_system.heal_entity(entity_id, amount) else {
                            return Err("Entity does not reference a runtime character".to_string());
                        };

                        Ok(Some(Value::Number(health as f64)))
                    }

                    "destroy" => {
                        if !arguments.is_empty() {
                            return Err("entity.destroy() expects 0 arguments".to_string());
                        }

                        if !self.pending_destroys.contains(&id) {
                            self.pending_destroys.push(id);
                        }

                        self.destroy_entity_runtime(id);

                        Ok(Some(Value::Bool(true)))
                    }

                    "find_path" => {
                        if arguments.len() != 1 {
                            return Err(
                                "entity.find_path() expects 1 argument ([x, y, z])".to_string()
                            );
                        }

                        let basket = arguments[0].as_basket()?;
                        let borrowed = basket.borrow();

                        if borrowed.elements.len() != 3 {
                            return Err("entity.find_path() target must be [x, y, z]".to_string());
                        }

                        let target = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );

                        let Some(character_system) = self.character_system.as_deref() else {
                            return Err("CharacterSystem not available".to_string());
                        };

                        let Some(path) =
                            character_system.find_path_for_entity(entity_id, self.world, target)
                        else {
                            return Ok(Some(Value::Nil));
                        };

                        let points = path
                            .into_iter()
                            .map(|point| {
                                Value::array(vec![
                                    Value::Number(point.x as f64),
                                    Value::Number(point.y as f64),
                                    Value::Number(point.z as f64),
                                ])
                            })
                            .collect();

                        Ok(Some(Value::array(points)))
                    }

                    _ => Ok(None),
                }
            }

            HandleKind::EntityPhysics
            | HandleKind::EntityAppearance
            | HandleKind::EntityAnimation
            | HandleKind::Ui
            | HandleKind::Sound
            | HandleKind::Mouse
            | HandleKind::Actor => Ok(None),
        }
    }

    pub fn get_script_property(
        &self,
        kind: HandleKind,
        id: u64,
        name: &str,
    ) -> Option<DynamicProperty> {
        self.dynamic_properties
            .get(&(kind, id))
            .and_then(|properties| properties.get(name))
            .cloned()
    }

    pub fn set_script_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: String,
        value: Value,
        owner_script: Option<String>,
    ) {
        self.dynamic_properties
            .entry((kind, id))
            .or_default()
            .insert(
                name,
                DynamicProperty {
                    value,
                    owner_script,
                },
            );
    }

    pub fn remove_dynamic_properties_by_owner(&mut self, owner_script: &str) {
        let norm = norm_path(owner_script);

        for properties in self.dynamic_properties.values_mut() {
            properties.retain(|_, prop| prop.owner_script.as_deref() != Some(&norm));
        }
    }
}

impl<'a> EngineHost for ScriptHostBridge<'a> {
    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    fn terrain_generate(&mut self, position: [i32; 3], size: [i32; 3]) -> Result<(), String> {
        self.world
            .generate_runtime_terrain(crate::world::terrain::TerrainGenerationRequest {
                position,
                size,
            })
            .map_err(|error| format!("terrain.generate failed: {error}"))
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:05 -05:00
    fn terrain_set_voxels(
        &mut self,
        origin: [i32; 3],
        size: [u32; 3],
        spacing_studs: f32,
        materials: Vec<String>,
        voxels: Vec<(u16, f32)>,
    ) -> Result<(), String> {
        for (index, (material, occupancy)) in voxels.iter().copied().enumerate() {
            if material as usize >= materials.len() {
                return Err(format!(
                    "terrain.set_voxels voxel {index} material index {material} is outside the palette"
                ));
            }
            if !occupancy.is_finite() || !(0.0..=1.0).contains(&occupancy) {
                return Err(format!(
                    "terrain.set_voxels voxel {index} occupancy must be finite 0..1"
                ));
            }
        }
        if !spacing_studs.is_finite() || spacing_studs <= 0.0 {
            return Err("terrain.set_voxels spacing must be finite and greater than zero".into());
        }
        let voxels = voxels
            .into_iter()
            .map(|(material, occupancy)| {
                crate::world::SmoothVoxel::from_unit_occupancy(material, occupancy)
            })
            .collect();
        self.world
            .set_runtime_voxel_terrain_with_spacing(origin, size, spacing_studs, materials, voxels)
            .map_err(|error| format!("terrain.set_voxels failed: {error}"))
    }

    fn particle_burst(
        &mut self,
        preset: &str,
        position: Vec3,
        count: usize,
    ) -> Result<usize, String> {
        self.world.particles.burst(
            position,
            count,
            crate::engine::particles::BurstConfig::preset(preset)?,
        )
    }

    fn particle_set_enabled(&mut self, id: u64, enabled: bool) -> Result<(), String> {
        if self
            .world
            .get_effective_cell_by_id(id)
            .is_none_or(|cell| cell.cell_type != crate::world::CellType::ParticleEmitter)
        {
            return Err("particles.set_enabled: expected a live ParticleEmitter Cell".into());
        }
        self.world.particles.set_emitter_enabled(id, enabled);
        Ok(())
    }

    fn particle_clear(&mut self) -> Result<(), String> {
        self.world.particles.clear_particles();
        Ok(())
    }

    fn particle_count(&self) -> usize {
        self.world.particles.particles().len()
    }

    fn save_get(&self, key: &str) -> Result<Option<Value>, String> {
        let state = self
            .save_state
            .as_deref()
            .ok_or_else(|| "save state is unavailable because no project is active".to_string())?;
        Ok(state.get(key).map(|value| match value {
            crate::engine::save_state::SaveValue::Number(value) => Value::Number(*value),
            crate::engine::save_state::SaveValue::Bool(value) => Value::Bool(*value),
            crate::engine::save_state::SaveValue::String(value) => Value::String(value.clone()),
        }))
    }

    fn save_set(&mut self, key: &str, value: Value) -> Result<(), String> {
        let value = match value {
            Value::Number(value) => crate::engine::save_state::SaveValue::Number(value),
            Value::Bool(value) => crate::engine::save_state::SaveValue::Bool(value),
            Value::String(value) => crate::engine::save_state::SaveValue::String(value),
            other => {
                return Err(format!(
                    "save.set supports only number, bool, and string values; got {}",
                    other.type_name()
                ));
            }
        };
        self.save_state
            .as_deref_mut()
            .ok_or_else(|| "save state is unavailable because no project is active".to_string())?
            .set(key, value)
    }

    fn save_has(&self, key: &str) -> Result<bool, String> {
        let state = self
            .save_state
            .as_deref()
            .ok_or_else(|| "save state is unavailable because no project is active".to_string())?;
        Ok(state.has(key))
    }

    fn save_remove(&mut self, key: &str) -> Result<bool, String> {
        self.save_state
            .as_deref_mut()
            .ok_or_else(|| "save state is unavailable because no project is active".to_string())?
            .remove(key)
    }

    fn save_commit(&mut self) -> Result<(), String> {
        self.save_state
            .as_deref_mut()
            .ok_or_else(|| "save state is unavailable because no project is active".to_string())?
            .commit()
            .map_err(|error| format!("save.commit failed: {}", error))
    }

    fn enable_script(&mut self, path: &str) {
        let norm = norm_path(path);

        self.world.disabled_scripts.retain(|s| s != &norm);

        if !self.pending_enable_scripts.contains(&norm) {
            self.pending_enable_scripts.push(norm);
        }
    }

    fn get_viewport_size(&self) -> [f32; 2] {
        self.viewport_size
    }

    fn set_viewport_size(&mut self, width: f32, height: f32) {
        if (self.viewport_size[0] - width).abs() > 0.5
            || (self.viewport_size[1] - height).abs() > 0.5
        {
            self.viewport_size = [width, height];
            if width > 0.0 && height > 0.0 {
                self.pending_events.push((
                    "viewport_resized".to_string(),
                    vec![
                        crate::scripting::value::Value::Number(width as f64),
                        crate::scripting::value::Value::Number(height as f64),
                    ],
                ));
            }
        }
    }

    fn disable_script(&mut self, path: &str) {
        let norm = norm_path(path);

        if !self.world.disabled_scripts.contains(&norm) {
            self.world.disabled_scripts.push(norm.clone());
        }

        if !self.pending_disable_scripts.contains(&norm) {
            self.pending_disable_scripts.push(norm);
        }
    }

    fn is_script_enabled(&self, path: &str) -> bool {
        let norm = norm_path(path);
        !self.world.disabled_scripts.contains(&norm)
    }

    fn drain_enabled_scripts(&mut self) -> Vec<String> {
        std::mem::take(self.pending_enable_scripts)
    }

    fn drain_disabled_scripts(&mut self) -> Vec<String> {
        std::mem::take(self.pending_disable_scripts)
    }

    fn fire_event(&mut self, event_name: &str, args: Vec<Value>) {
        self.pending_events.push((event_name.to_string(), args));
    }

    fn drain_pending_events(&mut self) -> Vec<(String, Vec<Value>)> {
        std::mem::take(self.pending_events)
    }

    fn complete_test(&mut self, test_name: &str, passed: bool) {
        self.test_results.insert(test_name.to_string(), passed);
    }

    fn is_test_completed(&self, test_name: &str) -> Option<bool> {
        self.test_results.get(test_name).copied()
    }

    fn get_test_results(&self) -> (usize, usize, usize) {
        let passed = self.test_results.values().filter(|&&v| v).count();
        let total = self.test_results.len();
        let failed = total - passed;

        (passed, failed, total)
    }

    fn entity_manager(&self) -> &EntityManager {
        self.entity_manager
    }

    fn get_position(&self, id: u64) -> Option<Vec3> {
        ScriptHostBridge::get_position(self, id)
    }

    fn set_position(&mut self, id: u64, position: Vec3) {
        ScriptHostBridge::set_position(self, id, position);
    }

    fn is_entity_declaration_valid(&self, entity_name: &str) -> bool {
        ScriptHostBridge::is_entity_declaration_valid(self, entity_name)
    }

    fn spawn_character(
        &mut self,
        entity_name: &str,
        position: Vec3,
        character_package: Option<&str>,
    ) -> Result<u64, String> {
        ScriptHostBridge::spawn_character(self, entity_name, position, character_package)
    }

    fn spawn_entity(
        &mut self,
        entity_name: &str,
        position: Vec3,
        package_name: Option<&str>,
    ) -> Result<u64, String> {
        ScriptHostBridge::spawn_entity(self, entity_name, position, package_name)
    }

    fn attach_entity(
        &mut self,
        child_kind: HandleKind,
        child_id: u64,
        child_point: &str,
        parent_kind: HandleKind,
        parent_id: u64,
        parent_point: &str,
    ) -> Result<(), String> {
        let resolve = |bridge: &ScriptHostBridge<'_>, kind, id| match kind {
            HandleKind::Entity => {
                let entity = EntityId(id);
                bridge
                    .entity_manager
                    .validate_handle(id)
                    .then_some(entity)
                    .ok_or_else(|| format!("runtime Entity {} was not found", id))
            }
            HandleKind::Actor => bridge
                .resolve_actor_entity(id)
                .ok_or_else(|| format!("runtime Actor {} was not found", id)),
            _ => Err(format!("{} handles cannot be attached", kind.name())),
        };

        let child = resolve(self, child_kind, child_id)?;
        let parent = resolve(self, parent_kind, parent_id)?;
        let attachment_system = self
            .attachment_system
            .as_deref_mut()
            .ok_or_else(|| "AttachmentSystem not available".to_string())?;
        let character_system = self
            .character_system
            .as_deref_mut()
            .ok_or_else(|| "CharacterSystem not available".to_string())?;

        attachment_system.attach(
            child,
            child_point,
            parent,
            parent_point,
            self.entity_manager,
            character_system,
        )
    }

    fn detach_entity(&mut self, child_kind: HandleKind, child_id: u64) -> Result<bool, String> {
        let child = match child_kind {
            HandleKind::Entity => {
                if !self.entity_manager.validate_handle(child_id) {
                    return Err(format!("runtime Entity {} was not found", child_id));
                }
                EntityId(child_id)
            }
            HandleKind::Actor => self
                .resolve_actor_entity(child_id)
                .ok_or_else(|| format!("runtime Actor {} was not found", child_id))?,
            _ => {
                return Err(format!("{} handles cannot be detached", child_kind.name()));
            }
        };

        let attachment_system = self
            .attachment_system
            .as_deref_mut()
            .ok_or_else(|| "AttachmentSystem not available".to_string())?;
        let character_system = self
            .character_system
            .as_deref_mut()
            .ok_or_else(|| "CharacterSystem not available".to_string())?;
        Ok(attachment_system.detach(child, self.entity_manager, character_system))
    }

    fn destroy_entity_runtime(&mut self, id: u64) {
        let entity_id = EntityId(id);

        let authored_actor_id = self
            .actor_system
            .as_deref()
            .and_then(|system| system.authored_actor_id_for_entity(entity_id));

        if let (Some(attachment_system), Some(character_system)) = (
            self.attachment_system.as_deref_mut(),
            self.character_system.as_deref_mut(),
        ) {
            attachment_system.remove_entity(entity_id, self.entity_manager, character_system);
        }

        if let Some(character_system) = self.character_system.as_deref_mut() {
            character_system.destroy_entity(entity_id);
        }

        if let Some(actor_system) = self.actor_system.as_deref_mut() {
            actor_system.remove_actor(entity_id);
        }

        self.pending_spawns
            .retain(|(_, pending_id)| *pending_id != id);
        self.entity_manager.remove_entity(entity_id);
        self.dynamic_properties.remove(&(HandleKind::Entity, id));

        if let Some(actor_id) = authored_actor_id {
            self.dynamic_properties
                .remove(&(HandleKind::Actor, actor_id));
        }
    }

    fn drain_pending_destroys(&mut self) -> Vec<u64> {
        std::mem::take(self.pending_destroys)
    }

    fn remove_dynamic_properties_for_handle(&mut self, kind: HandleKind, id: u64) {
        self.dynamic_properties.remove(&(kind, id));
    }

    fn set_valid_entity_declarations(&mut self, decls: Arc<std::collections::HashSet<String>>) {
        ScriptHostBridge::set_valid_entity_declarations(self, decls)
    }

    fn drain_pending_spawns(&mut self) -> Vec<(String, u64)> {
        ScriptHostBridge::drain_pending_spawns(self)
    }

    fn lookup_light(&self, x: i32, y: i32, z: i32) -> Option<u64> {
        ScriptHostBridge::lookup_light(self, x, y, z)
    }

    fn is_light_enabled(&self, id: u64) -> Option<bool> {
        ScriptHostBridge::is_light_enabled(self, id)
    }

    fn set_light_enabled(&mut self, id: u64, enabled: bool) {
        ScriptHostBridge::set_light_enabled(self, id, enabled);
    }

    fn is_collision_events_enabled(&self, id: u64) -> bool {
        ScriptHostBridge::is_collision_events_enabled(self, id)
    }

    fn create_runtime_cell(&mut self, cell_type: &str) -> Result<(HandleKind, u64), String> {
        ScriptHostBridge::create_runtime_cell(self, cell_type)
    }

    fn move_runtime_cell(&mut self, id: u64, x: i32, y: i32, z: i32) -> Result<(), String> {
        ScriptHostBridge::move_runtime_cell(self, id, x, y, z)
    }

    fn delete_cell(&mut self, id: u64) -> Result<(), String> {
        ScriptHostBridge::delete_cell(self, id)
    }

    fn get_all_cells_of_class(&self, class_name: &str) -> Vec<u64> {
        ScriptHostBridge::get_all_cells_of_class(self, class_name)
    }

    fn find_objects(&self, query: &str) -> Vec<(HandleKind, u64)> {
        ScriptHostBridge::find_objects(self, query)
    }

    fn get_children(&self, kind: HandleKind, id: u64) -> Vec<(HandleKind, u64)> {
        ScriptHostBridge::get_children(self, kind, id)
    }

    fn get_parent(&self, kind: HandleKind, id: u64) -> Option<(HandleKind, u64)> {
        ScriptHostBridge::get_parent(self, kind, id)
    }

    fn get_cell_object(&self, cell_id: u64) -> Option<(HandleKind, u64)> {
        ScriptHostBridge::get_cell_object(self, cell_id)
    }

    fn get_actor_module_path(&self, actor_id: u64, module_name: &str) -> Option<String> {
        ScriptHostBridge::get_actor_module_path(self, actor_id, module_name)
    }

    fn actor_exists(&self, actor_id: u64) -> bool {
        ScriptHostBridge::actor_exists(self, actor_id)
    }

    fn get_property(&self, kind: HandleKind, id: u64, name: &str) -> Result<Option<Value>, String> {
        ScriptHostBridge::get_property(self, kind, id, name)
    }

    fn set_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: &str,
        value: Value,
    ) -> Result<bool, String> {
        ScriptHostBridge::set_property(self, kind, id, name, value)
    }

    fn call_method(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: &str,
        arguments: &[Value],
    ) -> Result<Option<Value>, String> {
        ScriptHostBridge::call_method(self, kind, id, name, arguments)
    }

    fn is_audio_playing(&self, id: u64) -> Option<bool> {
        ScriptHostBridge::is_audio_playing(self, id)
    }

    fn is_audio_paused(&self, id: u64) -> Option<bool> {
        ScriptHostBridge::is_audio_paused(self, id)
    }

    fn is_audio_looped(&self, id: u64) -> Option<bool> {
        ScriptHostBridge::is_audio_looped(self, id)
    }

    fn get_audio_volume(&self, id: u64) -> Option<f32> {
        ScriptHostBridge::get_audio_volume(self, id)
    }

    fn set_audio_playing(&mut self, id: u64, playing: bool) {
        ScriptHostBridge::set_audio_playing(self, id, playing);
    }

    fn set_audio_looped(&mut self, id: u64, looped: bool) {
        ScriptHostBridge::set_audio_looped(self, id, looped);
    }

    fn set_audio_volume(&mut self, id: u64, volume: f32) {
        ScriptHostBridge::set_audio_volume(self, id, volume);
    }

    fn audio_play(&mut self, id: u64) {
        ScriptHostBridge::audio_play(self, id);
    }

    fn audio_stop(&mut self, id: u64) {
        ScriptHostBridge::audio_stop(self, id);
    }

    fn audio_pause(&mut self, id: u64) {
        ScriptHostBridge::audio_pause(self, id);
    }

    fn set_attribute(&mut self, id: u64, key: String, value: Value) -> Result<(), String> {
        ScriptHostBridge::set_attribute(self, id, key, value)
    }

    fn remove_attribute(&mut self, id: u64, key: &str) -> Result<(), String> {
        ScriptHostBridge::remove_attribute(self, id, key)
    }

    fn cell_exists(&self, id: u64) -> bool {
        ScriptHostBridge::cell_exists(self, id)
    }

    fn entity_exists(&self, id: u64) -> bool {
        ScriptHostBridge::entity_exists(self, id)
    }

    fn get_script_property(
        &self,
        kind: HandleKind,
        id: u64,
        name: &str,
    ) -> Option<DynamicProperty> {
        ScriptHostBridge::get_script_property(self, kind, id, name)
    }

    fn set_script_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: String,
        value: Value,
        owner_script: Option<String>,
    ) {
        ScriptHostBridge::set_script_property(self, kind, id, name, value, owner_script);
    }

    fn remove_dynamic_properties_by_owner(&mut self, owner_script: &str) {
        ScriptHostBridge::remove_dynamic_properties_by_owner(self, owner_script);
    }

    fn find_ui_element(&self, document: Option<&str>, key: &str) -> Option<u64> {
        self.runtime_ui.find(document, key)
    }

    fn create_ui_element(&mut self, element_type: &str) -> Result<u64, String> {
        ScriptHostBridge::create_ui_element(self, element_type)
    }

    fn delete_ui_element(&mut self, id: u64) -> Result<(), String> {
        ScriptHostBridge::delete_ui_element(self, id)
    }

    fn get_ui_property(&self, id: u64, name: &str) -> Result<Option<Value>, String> {
        ScriptHostBridge::get_ui_property(self, id, name)
    }

    fn set_ui_property(&mut self, id: u64, name: &str, value: Value) -> Result<bool, String> {
        ScriptHostBridge::set_ui_property(self, id, name, value)
    }

    fn drain_ui_clicks(&mut self) -> Vec<u64> {
        ScriptHostBridge::drain_ui_clicks(self)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:13 -05:00
    fn get_ui_symbolic_event_dispatch(
        &self,
        id: u64,
        event: &str,
    ) -> Option<crate::engine::ui::UiSymbolicEventDispatch> {
        ScriptHostBridge::get_ui_symbolic_event_dispatch(self, id, event)
    }

    fn get_input_move_vector(&self) -> [f32; 2] {
        ScriptHostBridge::get_input_move_vector(self)
    }

    fn is_input_jump_pressed(&self) -> bool {
        ScriptHostBridge::is_input_jump_pressed(self)
    }

    fn get_input_orbit_delta(&self) -> [f32; 2] {
        ScriptHostBridge::get_input_orbit_delta(self)
    }

    fn is_input_key_down(&self, key: &str) -> bool {
        ScriptHostBridge::is_input_key_down(self, key)
    }

    fn get_camera_horizontal_basis(&self) -> ([f32; 3], [f32; 3]) {
        ScriptHostBridge::get_camera_horizontal_basis(self)
    }

    fn set_camera_position(&mut self, position: [f32; 3]) {
        ScriptHostBridge::set_camera_position(self, position);
    }

    fn set_camera_target(&mut self, target: [f32; 3]) {
        ScriptHostBridge::set_camera_target(self, target);
    }

    fn set_camera_orientation(&mut self, yaw: f32, pitch: f32) {
        ScriptHostBridge::set_camera_orientation(self, yaw, pitch);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
    fn get_camera_script(&self) -> String {
        self.world.active_camera_script().to_string()
    }

    fn set_camera_script(&mut self, name: &str) -> Result<(), String> {
        self.world.set_runtime_camera_script(name)
    }

    fn resolve_camera_collision(&self, target: [f32; 3], desired: [f32; 3]) -> [f32; 3] {
        ScriptHostBridge::resolve_camera_collision(self, target, desired)
    }

    fn get_player_actor_id(&self) -> Option<u64> {
        ScriptHostBridge::get_player_actor_id(self)
    }

    fn get_script_actor_id_for_entity(&self, entity_id: u64) -> Option<u64> {
        self.actor_system
            .as_deref()?
            .script_actor_id_for_entity(EntityId(entity_id))
    }

    fn get_player_position(&self) -> Option<[f32; 3]> {
        ScriptHostBridge::get_player_position(self)
    }

    fn set_player_horizontal_velocity(&mut self, velocity_x: f32, velocity_z: f32) {
        ScriptHostBridge::set_player_horizontal_velocity(self, velocity_x, velocity_z);
    }

    fn set_player_facing_direction(&mut self, direction_x: f32, direction_z: f32) {
        ScriptHostBridge::set_player_facing_direction(self, direction_x, direction_z);
    }

    fn select_player_animation(&mut self, animation: &str) {
        ScriptHostBridge::select_player_animation(self, animation);
    }

    fn is_player_grounded(&self) -> bool {
        ScriptHostBridge::is_player_grounded(self)
    }

    fn apply_player_vertical_impulse(&mut self, impulse: f32) {
        ScriptHostBridge::apply_player_vertical_impulse(self, impulse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::CharacterSystem;
    use crate::scripting::api::EngineHost;
    use crate::scripting::value::{HandleKind, Value};
    use crate::world::WorldCoord;
    use crate::world::cell::{AttributeValue, CellType};

    fn make_bridge<'a>(
        entity_manager: &'a mut EntityManager,
        world: &'a mut World,
        mouse: &'a mut MouseController,
        dynamic_properties: &'a mut std::collections::HashMap<
            (HandleKind, u64),
            std::collections::BTreeMap<String, DynamicProperty>,
        >,
        pending_events: &'a mut Vec<(String, Vec<Value>)>,
        test_results: &'a mut std::collections::BTreeMap<String, bool>,
        pending_enable_scripts: &'a mut Vec<String>,
        pending_disable_scripts: &'a mut Vec<String>,
        runtime_ui: &'a mut crate::engine::ui::RuntimeUi,
        pending_spawns: &'a mut Vec<(String, u64)>,
        pending_destroys: &'a mut Vec<u64>,
        character_system: Option<&'a mut CharacterSystem>,
    ) -> ScriptHostBridge<'a> {
        ScriptHostBridge {
            entity_manager,
            attachment_system: None,
            world,
            mouse,
            dynamic_properties,
            pending_events,
            test_results,
            pending_enable_scripts,
            pending_disable_scripts,
            runtime_ui,
            viewport_size: [1280.0, 720.0],
            keys_down: None,
            move_input: glam::Vec2::ZERO,
            jump_requested: false,
            orbit_delta: [0.0, 0.0],
            character_system,
            actor_system: None,
            gameplay_camera: None,
            valid_entity_declarations: None,
            pending_spawns,
            pending_destroys,
            project_path: None,
            save_state: None,
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:43 -05:00
    #[test]
    fn associated_character_package_and_animation_are_visible_to_actor_and_entity_scripts() {
        let mut entities = EntityManager::new();
        let mut world = World::new();
        let mut characters = CharacterSystem::new();
        let mut actors = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Player", Vec3::ZERO);
        let entity_id = entities.create_entity("Player");
        let character_id = characters.spawn_character(Vec3::ZERO, None);
        characters.associate_entity(entity_id, character_id);
        let package_name = characters
            .get_character_for_entity(entity_id)
            .unwrap()
            .package_name
            .clone();
        assert!(
            entities
                .get_entity(entity_id)
                .unwrap()
                .package_name
                .is_none()
        );

        actors.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: Some(package_name.clone()),
            initial_position: Vec3::ZERO,
        });

        let mut mouse = MouseController::default();
        let mut properties = std::collections::HashMap::new();
        let mut events = Vec::new();
        let mut results = std::collections::BTreeMap::new();
        let mut enable = Vec::new();
        let mut disable = Vec::new();
        let mut ui = crate::engine::ui::RuntimeUi::new();
        let mut spawns = Vec::new();
        let mut destroys = Vec::new();
        let mut bridge = make_bridge(
            &mut entities,
            &mut world,
            &mut mouse,
            &mut properties,
            &mut events,
            &mut results,
            &mut enable,
            &mut disable,
            &mut ui,
            &mut spawns,
            &mut destroys,
            Some(&mut characters),
        );
        bridge.actor_system = Some(&mut actors);

        for (kind, id) in [
            (HandleKind::Actor, actor_id),
            (HandleKind::Entity, entity_id.0),
        ] {
            assert_eq!(
                bridge.get_property(kind, id, "package").unwrap(),
                Some(Value::String(package_name.clone()))
            );
        }
        assert_eq!(
            bridge
                .get_property(HandleKind::Actor, actor_id, "animation")
                .unwrap(),
            Some(Value::Handle {
                kind: HandleKind::EntityAnimation,
                id: entity_id.0,
            })
        );
        assert_eq!(
            bridge
                .get_property(HandleKind::Entity, entity_id.0, "animation")
                .unwrap(),
            Some(Value::Handle {
                kind: HandleKind::EntityAnimation,
                id: entity_id.0,
            })
        );

        bridge
            .entity_manager
            .get_entity_mut(entity_id)
            .unwrap()
            .package_name = Some("generic_override".into());
        assert_eq!(
            bridge
                .get_property(HandleKind::Actor, actor_id, "package")
                .unwrap(),
            Some(Value::String("generic_override".into())),
        );
    }

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    #[test]
    fn terrain_generate_uses_runtime_layer_and_shared_validation() {
        let mut entities = EntityManager::new();
        let mut world = World::new();
        let mut mouse = MouseController::default();
        let mut properties = std::collections::HashMap::new();
        let mut events = Vec::new();
        let mut results = std::collections::BTreeMap::new();
        let mut enable = Vec::new();
        let mut disable = Vec::new();
        let mut runtime = crate::engine::ui::RuntimeUi::new();
        let mut spawns = Vec::new();
        let mut destroys = Vec::new();
        let mut bridge = make_bridge(
            &mut entities,
            &mut world,
            &mut mouse,
            &mut properties,
            &mut events,
            &mut results,
            &mut enable,
            &mut disable,
            &mut runtime,
            &mut spawns,
            &mut destroys,
            None,
        );

        bridge.terrain_generate([2, 0, 3], [8, 16, 9]).unwrap();
        assert!(bridge.world.terrain.runtime.is_some());
        assert!(bridge.world.terrain.authored.is_none());

        let error = bridge.terrain_generate([0, 0, 0], [0, 1, 1]).unwrap_err();
        assert!(error.contains("terrain.generate failed") && error.contains("x"));

        bridge.world.clear_runtime_state();
        assert!(bridge.world.terrain.runtime.is_none());
        assert!(bridge.world.terrain.authored.is_none());
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:15 -05:00
    #[test]
    fn terrain_set_voxels_installs_runtime_programmatic_payload() {
        let mut entities = EntityManager::new();
        let mut world = World::new();
        let mut mouse = MouseController::default();
        let mut properties = std::collections::HashMap::new();
        let mut events = Vec::new();
        let mut results = std::collections::BTreeMap::new();
        let mut enable = Vec::new();
        let mut disable = Vec::new();
        let mut runtime = crate::engine::ui::RuntimeUi::new();
        let mut spawns = Vec::new();
        let mut destroys = Vec::new();
        let mut bridge = make_bridge(
            &mut entities,
            &mut world,
            &mut mouse,
            &mut properties,
            &mut events,
            &mut results,
            &mut enable,
            &mut disable,
            &mut runtime,
            &mut spawns,
            &mut destroys,
            None,
        );

        bridge
            .terrain_set_voxels(
                [0, 0, 0],
                [1, 1, 1],
                crate::world::LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
                vec!["rock".into()],
                vec![(0, 1.0)],
            )
            .unwrap();

        let result = bridge.world.terrain.runtime.as_ref().unwrap();
        assert_eq!(
            result.generator,
            crate::world::TerrainGeneratorType::Programmatic
        );
        let crate::world::TerrainData::Voxel(grid) = &result.data else {
            panic!("terrain.set_voxels must create voxel Terrain");
        };
        assert_eq!(grid.voxels[0].occupancy, 255);
        assert!(bridge.world.terrain.authored.is_none());
    }

    #[test]
    fn authored_ui_lookup_returns_normal_script_handle() {
        use crate::engine::ui_authoring::{
            AuthoredUiDocument, AuthoredUiNode, populate_runtime_ui_named,
        };
        use crate::scripting::{
            ast::Program,
            interpreter::{Interpreter, ScriptInstance},
            source::SourceSpan,
            value::Scope,
        };
        let mut document = AuthoredUiDocument::default();
        document.nodes.push(AuthoredUiNode {
            id: 42,
            element_type: "Text".into(),
            key: "health_text".into(),
            ..Default::default()
        });
        let mut runtime = crate::engine::ui::RuntimeUi::new();
        populate_runtime_ui_named(&mut runtime, &document, "game_hud").unwrap();
        let mut entities = EntityManager::new();
        let mut world = World::new();
        let mut mouse = MouseController::default();
        let mut properties = std::collections::HashMap::new();
        let mut events = Vec::new();
        let mut results = std::collections::BTreeMap::new();
        let mut enable = Vec::new();
        let mut disable = Vec::new();
        let mut spawns = Vec::new();
        let mut destroys = Vec::new();
        let mut bridge = make_bridge(
            &mut entities,
            &mut world,
            &mut mouse,
            &mut properties,
            &mut events,
            &mut results,
            &mut enable,
            &mut disable,
            &mut runtime,
            &mut spawns,
            &mut destroys,
            None,
        );
        let mut host = crate::scripting::api::HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };
        let mut interpreter = Interpreter::new(Program {
            span: SourceSpan::new(0, 0),
            declarations: vec![],
            statements: vec![],
        });
        let mut instance = ScriptInstance::new_empty();
        let mut scopes = vec![Scope::new()];
        let result = crate::scripting::stdlib::call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "ui",
            "get",
            &[
                Value::String("game_hud".into()),
                Value::String("health_text".into()),
            ],
            &mut host,
        )
        .unwrap()
        .unwrap();
        let (kind, id) = result.as_handle().unwrap();
        assert_eq!(kind, HandleKind::Ui);
        host.engine
            .set_ui_property(id, "text", Value::String("75 / 100".into()))
            .unwrap();
        assert!(
            matches!(host.engine.get_ui_property(id, "text").unwrap(), Some(Value::String(text)) if text == "75 / 100")
        );
        let missing = crate::scripting::stdlib::call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "ui",
            "find",
            &[Value::String("missing".into())],
            &mut host,
        )
        .unwrap();
        assert!(matches!(missing, Some(Value::Nil)));
        assert_eq!(document.node(42).unwrap().text, "");
    }

    #[test]
    fn test_actor_handle_resolves_authored_actor_to_runtime_entity() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Door_A", Vec3::new(1.0, 2.0, 3.0));
        let entity_id = entity_manager.create_entity("Door_A");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: None,
            initial_position: Vec3::new(1.0, 2.0, 3.0),
        });

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        assert_eq!(bridge.resolve_actor_entity(actor_id), Some(entity_id));

        assert!(bridge.actor_exists(actor_id));
        assert!(!bridge.actor_exists(actor_id + 1));

        assert_eq!(HandleKind::Actor.name(), "Actor");
    }

    #[test]
    fn test_actor_handle_exposes_runtime_state_without_exposing_entity_id() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Door_A", Vec3::new(1.0, 2.0, 3.0));
        let entity_id = entity_manager.create_entity("Door_A");

        entity_manager.set_position(entity_id, Vec3::new(10.0, 20.0, 30.0));
        entity_manager.set_rotation(entity_id, Vec3::new(0.0, 90.0, 0.0));
        entity_manager.set_scale(entity_id, Vec3::new(2.0, 3.0, 4.0));
        entity_manager.set_visible(entity_id, false);
        entity_manager.set_enabled(entity_id, true);

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: None,
            initial_position: Vec3::new(1.0, 2.0, 3.0),
        });

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        assert_eq!(
            bridge
                .get_property(HandleKind::Actor, actor_id, "id")
                .unwrap(),
            Some(Value::Number(actor_id as f64))
        );

        assert_eq!(
            bridge
                .get_property(HandleKind::Actor, actor_id, "name")
                .unwrap(),
            Some(Value::String("Door_A".to_string()))
        );

        let position = bridge
            .get_property(HandleKind::Actor, actor_id, "position")
            .unwrap()
            .unwrap();

        let position = position.as_basket().unwrap();
        let position = position.borrow();

        assert_eq!(
            position.elements,
            vec![
                Value::Number(10.0),
                Value::Number(20.0),
                Value::Number(30.0),
            ]
        );

        assert_eq!(
            bridge
                .get_property(HandleKind::Actor, actor_id, "visible")
                .unwrap(),
            Some(Value::Bool(false))
        );

        assert_eq!(
            bridge
                .get_property(HandleKind::Actor, actor_id, "enabled")
                .unwrap(),
            Some(Value::Bool(true))
        );

        // The public ID is the persistent Actor ID, not the runtime Entity ID.
        assert_ne!(actor_id, entity_id.0);
    }

    #[test]
    fn test_actor_handle_mutates_backing_runtime_entity() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Mover", Vec3::ZERO);
        let entity_id = entity_manager.create_entity("Mover");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        assert!(
            bridge
                .set_property(
                    HandleKind::Actor,
                    actor_id,
                    "position",
                    Value::array(vec![
                        Value::Number(5.0),
                        Value::Number(6.0),
                        Value::Number(7.0),
                    ]),
                )
                .unwrap()
        );

        assert_eq!(
            bridge.entity_manager.get_position(entity_id),
            Some(Vec3::new(5.0, 6.0, 7.0))
        );

        bridge
            .call_method(
                HandleKind::Actor,
                actor_id,
                "translate",
                &[Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)],
            )
            .unwrap();

        assert_eq!(
            bridge.entity_manager.get_position(entity_id),
            Some(Vec3::new(6.0, 8.0, 10.0))
        );

        bridge
            .set_property(HandleKind::Actor, actor_id, "visible", Value::Bool(false))
            .unwrap();

        assert_eq!(bridge.entity_manager.is_visible(entity_id), Some(false));
    }

    #[test]
    fn test_authored_actor_discovery_returns_actor_handle() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Coin", Vec3::ZERO);
        let entity_id = entity_manager.create_entity("Coin");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        let results = bridge.find_objects("Coin");

        assert_eq!(results, vec![(HandleKind::Actor, actor_id)]);

        assert!(!results.contains(&(HandleKind::Entity, entity_id.0)));
    }

    #[test]
    fn test_actor_module_name_resolves_attached_module_path() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Player", Vec3::ZERO);
        let entity_id = entity_manager.create_entity("Player");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/Player/controller.aeo",
            ));

        world.rebuild_actor_module_index();

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        assert_eq!(
            bridge.get_actor_module_path(actor_id, "controller"),
            Some("scripts/Player/controller.aeo".to_string())
        );

        assert_eq!(bridge.get_actor_module_path(actor_id, "missing"), None);
    }

    #[test]
    fn test_parent_destroy_removes_actor_and_invalidates_attached_module() {
        use crate::scripting::api::HostContext;
        use crate::scripting::interpreter::Interpreter;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;

        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let actor_id = world.add_actor("Door", Vec3::ZERO);
        let entity_id = entity_manager.create_entity("Door");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id,
            authored_actor_id: Some(actor_id),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        let gate_actor_id = world.add_actor("Gate", Vec3::ZERO);
        let gate_entity_id = entity_manager.create_entity("Gate");
        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: gate_entity_id,
            authored_actor_id: Some(gate_actor_id),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        let empty_program = Parser::new(
            Lexer::new("")
                .tokenize()
                .expect("empty source should tokenize"),
        )
        .parse()
        .expect("empty source should parse");

        let module_program = Parser::new(
            Lexer::new(
                r#"
fn destroy_parent() {
    self.parent.destroy()
}

fn ping() {
    return 1
}

fn destroy_via_entity() {
    get_entity("Gate").destroy()
}
"#,
            )
            .tokenize()
            .expect("Module should tokenize"),
        )
        .parse()
        .expect("Module should parse");

        let mut interpreter = Interpreter::new(empty_program);

        interpreter
            .loaded_scripts
            .insert("scripts/door.aeo".to_string(), module_program);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };

        let module = interpreter
            .get_or_create_attached_module_instance("scripts/door.aeo", actor_id, &mut host)
            .expect("attached Module should instantiate");

        interpreter
            .call_object_method_direct(&module, "destroy_parent", vec![], &mut host)
            .expect("self.parent.destroy() should succeed");

        assert_eq!(
            host.engine
                .call_method(HandleKind::Actor, actor_id, "is_valid", &[],)
                .unwrap(),
            Some(Value::Bool(false))
        );

        assert!(!host.engine.entity_exists(entity_id.0));

        let error = interpreter
            .call_object_method_direct(&module, "ping", vec![], &mut host)
            .expect_err("destroyed Module reference must be invalid");

        assert!(
            error.contains("has been destroyed"),
            "unexpected error: {}",
            error
        );

        let resolved_again = interpreter
            .get_or_create_attached_module_instance("scripts/door.aeo", actor_id, &mut host)
            .unwrap();

        assert_eq!(resolved_again, Value::Nil);

        let gate_module = interpreter
            .get_or_create_attached_module_instance("scripts/door.aeo", gate_actor_id, &mut host)
            .expect("Gate Module should instantiate");
        interpreter
            .call_object_method_direct(&gate_module, "destroy_via_entity", vec![], &mut host)
            .expect("Entity.destroy() should succeed for an Actor-backed Entity");

        assert!(!host.engine.entity_exists(gate_entity_id.0));
        assert_eq!(
            interpreter
                .get_or_create_attached_module_instance(
                    "scripts/door.aeo",
                    gate_actor_id,
                    &mut host,
                )
                .unwrap(),
            Value::Nil,
            "destroying an Actor through its Entity handle must invalidate its Module"
        );
    }

    #[test]
    fn test_script_host_bridge_attributes_get() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();

        let coord = WorldCoord::new(5, 5, 5);
        let cell_id = world.set_cell(coord, CellType::Block);

        if let Some(cell) = world.get_mut(coord) {
            cell.attributes
                .insert("health".to_string(), AttributeValue::Number(100.0));

            cell.attributes
                .insert("is_boss".to_string(), AttributeValue::Bool(false));

            cell.attributes.insert(
                "tag".to_string(),
                AttributeValue::String("enemy".to_string()),
            );
        }

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        let value = bridge
            .get_property(HandleKind::Cell, cell_id, "attributes")
            .unwrap()
            .expect("attributes property should exist");

        let map = value.as_map().unwrap();
        let map = map.borrow();

        assert_eq!(
            map.get(&MapKey::String("health".to_string())),
            Some(&Value::Number(100.0))
        );

        assert_eq!(
            map.get(&MapKey::String("is_boss".to_string())),
            Some(&Value::Bool(false))
        );

        assert_eq!(
            map.get(&MapKey::String("tag".to_string())),
            Some(&Value::String("enemy".to_string()))
        );
    }

    #[test]
    fn test_entity_physics_capability_properties() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();

        let entity_id = entity_manager.create_entity("PhysicsEntity");

        {
            let physics = entity_manager
                .get_physics_mut(entity_id)
                .expect("physics should exist");

            physics.enabled = true;
            physics.anchored = false;
            physics.mass = 3.0;
            physics.gravity_scale = 0.5;
            physics.size = Vec3::new(1.0, 2.0, 3.0);
            physics.velocity = Vec3::new(4.0, 5.0, 6.0);
            physics.grounded = true;
        }

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        let enabled = bridge
            .get_property(HandleKind::EntityPhysics, entity_id.0, "enabled")
            .unwrap();

        assert_eq!(enabled, Some(Value::Bool(true)));

        let mass = bridge
            .get_property(HandleKind::EntityPhysics, entity_id.0, "mass")
            .unwrap();

        assert_eq!(mass, Some(Value::Number(3.0)));

        let grounded = bridge
            .get_property(HandleKind::EntityPhysics, entity_id.0, "grounded")
            .unwrap();

        assert_eq!(grounded, Some(Value::Bool(true)));

        let velocity = bridge
            .get_property(HandleKind::EntityPhysics, entity_id.0, "velocity")
            .unwrap()
            .unwrap();

        let velocity = velocity.as_basket().unwrap();
        let velocity = velocity.borrow();

        assert_eq!(velocity.elements.len(), 3);
        assert_eq!(velocity.elements[0], Value::Number(4.0));
        assert_eq!(velocity.elements[1], Value::Number(5.0));
        assert_eq!(velocity.elements[2], Value::Number(6.0));
    }

    #[test]
    fn test_entity_physics_capability_methods() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();

        let entity_id = entity_manager.create_entity("PhysicsEntity");

        {
            let physics = entity_manager
                .get_physics_mut(entity_id)
                .expect("physics should exist");

            physics.enabled = true;
            physics.anchored = false;
        }

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        let result = bridge
            .call_method(
                HandleKind::EntityPhysics,
                entity_id.0,
                "apply_impulse",
                &[Value::array(vec![
                    Value::Number(2.0),
                    Value::Number(4.0),
                    Value::Number(6.0),
                ])],
            )
            .unwrap();

        assert_eq!(result, Some(Value::Nil));

        assert_eq!(
            bridge
                .entity_manager
                .get_physics(entity_id)
                .unwrap()
                .velocity,
            Vec3::new(2.0, 4.0, 6.0)
        );

        bridge
            .call_method(
                HandleKind::EntityPhysics,
                entity_id.0,
                "apply_force",
                &[Value::array(vec![
                    Value::Number(5.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                ])],
            )
            .unwrap();

        assert_eq!(
            bridge
                .entity_manager
                .get_physics(entity_id)
                .unwrap()
                .accumulated_force,
            Vec3::new(5.0, 0.0, 0.0)
        );

        bridge
            .call_method(HandleKind::EntityPhysics, entity_id.0, "stop", &[])
            .unwrap();

        assert_eq!(
            bridge
                .entity_manager
                .get_physics(entity_id)
                .unwrap()
                .velocity,
            Vec3::ZERO
        );

        assert_eq!(
            bridge
                .entity_manager
                .get_physics(entity_id)
                .unwrap()
                .accumulated_force,
            Vec3::ZERO
        );
    }

    #[test]
    fn test_script_host_bridge_runtime_attribute_override() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();

        let coord = WorldCoord::new(0, 0, 0);
        let cell_id = world.set_cell(coord, CellType::Block);

        if let Some(cell) = world.get_mut(coord) {
            cell.attributes.insert(
                "test".to_string(),
                AttributeValue::String("authored".to_string()),
            );
        }

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        let attrs = bridge
            .get_property(HandleKind::Cell, cell_id, "attributes")
            .unwrap()
            .unwrap();

        let map = attrs.as_map().unwrap();

        assert_eq!(
            map.borrow().get(&MapKey::String("test".to_string())),
            Some(&Value::String("authored".to_string()))
        );

        bridge
            .set_attribute(
                cell_id,
                "test".to_string(),
                Value::String("runtime".to_string()),
            )
            .unwrap();

        let attrs = bridge
            .get_property(HandleKind::Cell, cell_id, "attributes")
            .unwrap()
            .unwrap();

        let map = attrs.as_map().unwrap();

        assert_eq!(
            map.borrow().get(&MapKey::String("test".to_string())),
            Some(&Value::String("runtime".to_string()))
        );

        assert_eq!(
            bridge.world.get(coord).unwrap().attributes.get("test"),
            Some(&AttributeValue::String("authored".to_string()))
        );

        bridge.remove_attribute(cell_id, "test").unwrap();

        let attrs = bridge
            .get_property(HandleKind::Cell, cell_id, "attributes")
            .unwrap()
            .unwrap();

        let map = attrs.as_map().unwrap();

        assert_eq!(
            map.borrow().get(&MapKey::String("test".to_string())),
            Some(&Value::String("authored".to_string()))
        );
    }

    #[test]
    fn test_player_set_position_updates_character_once() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        let player_char_id = character_system
            .spawn_player(&world, None)
            .expect("player should spawn");

        let player_id = entity_manager.create_entity("Player");
        character_system.associate_entity(player_id, player_char_id);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        let target = Vec3::new(10.0, 20.0, 30.0);

        bridge.set_position(player_id.0, target);

        assert_eq!(bridge.entity_manager.get_position(player_id), Some(target));

        assert_eq!(
            bridge.character_system.as_deref().and_then(|system| {
                system
                    .get_active_player()
                    .map(|player| player.transform.position)
            }),
            Some(target)
        );
    }

    #[test]
    fn test_non_player_set_position_does_not_move_character() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        character_system
            .spawn_player(&world, None)
            .expect("player should spawn");

        let original_position = character_system
            .get_active_player()
            .unwrap()
            .transform
            .position;

        let entity_id = entity_manager.create_entity("SomethingElse");

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        bridge.set_position(entity_id.0, Vec3::new(100.0, 100.0, 100.0));

        assert_eq!(
            bridge.character_system.as_deref().and_then(|system| {
                system
                    .get_active_player()
                    .map(|player| player.transform.position)
            }),
            Some(original_position)
        );
    }

    #[test]
    fn test_mouse_controller_script_properties() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.mouse.set_play_defaults();

        assert_eq!(bridge.mouse.cursor_visible, false);
        assert_eq!(bridge.mouse.screen_locked, true);

        assert_eq!(
            bridge
                .get_property(HandleKind::Mouse, 0, "setCursorVisible",)
                .unwrap(),
            Some(Value::Bool(false))
        );

        assert_eq!(
            bridge
                .get_property(HandleKind::Mouse, 0, "setScreenLocked",)
                .unwrap(),
            Some(Value::Bool(true))
        );

        assert!(
            bridge
                .set_property(HandleKind::Mouse, 0, "setCursorVisible", Value::Bool(true),)
                .unwrap()
        );

        assert!(
            bridge
                .set_property(HandleKind::Mouse, 0, "setScreenLocked", Value::Bool(false),)
                .unwrap()
        );

        assert_eq!(bridge.mouse.cursor_visible, true);
        assert_eq!(bridge.mouse.screen_locked, false);
    }

    #[test]
    fn test_entity_runtime_properties() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        let character_id = character_system
            .spawn_player(&world, None)
            .expect("character should spawn");

        let entity_id = entity_manager.create_entity("Villager");
        entity_manager.set_position(entity_id, Vec3::new(1.0, 2.0, 3.0));

        character_system.associate_entity(entity_id, character_id);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        let id = bridge
            .get_property(HandleKind::Entity, entity_id.0, "id")
            .unwrap();

        assert_eq!(id, Some(Value::Number(entity_id.0 as f64)));

        let name = bridge
            .get_property(HandleKind::Entity, entity_id.0, "name")
            .unwrap();

        assert_eq!(name, Some(Value::String("Villager".to_string())));

        let position = bridge
            .get_property(HandleKind::Entity, entity_id.0, "position")
            .unwrap()
            .unwrap();

        let position = position.as_basket().unwrap();
        let position = position.borrow();

        assert_eq!(position.elements.len(), 3);

        let velocity = bridge
            .get_property(HandleKind::Entity, entity_id.0, "velocity")
            .unwrap()
            .unwrap();

        let velocity = velocity.as_basket().unwrap();
        let velocity = velocity.borrow();

        assert_eq!(velocity.elements.len(), 3);

        let grounded = bridge
            .get_property(HandleKind::Entity, entity_id.0, "grounded")
            .unwrap();

        assert!(grounded.is_some());

        let animation = bridge
            .get_property(HandleKind::Entity, entity_id.0, "animation")
            .unwrap();

        assert!(animation.is_some());
    }

    #[test]
    fn test_entity_velocity_facing_and_health_controls() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        let character_id = character_system
            .spawn_player(&world, None)
            .expect("character should spawn");

        let entity_id = entity_manager.create_entity("Villager");

        character_system.associate_entity(entity_id, character_id);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        assert!(
            bridge
                .set_property(
                    HandleKind::Entity,
                    entity_id.0,
                    "velocity",
                    Value::array(vec![
                        Value::Number(2.0),
                        Value::Number(0.0),
                        Value::Number(-3.0),
                    ]),
                )
                .unwrap()
        );

        assert!(
            bridge
                .set_property(
                    HandleKind::Entity,
                    entity_id.0,
                    "facing",
                    Value::array(vec![Value::Number(1.0), Value::Number(0.0),]),
                )
                .unwrap()
        );

        assert!(
            bridge
                .set_property(
                    HandleKind::Entity,
                    entity_id.0,
                    "health",
                    Value::Number(75.0),
                )
                .unwrap()
        );

        let damage = bridge
            .call_method(
                HandleKind::Entity,
                entity_id.0,
                "damage",
                &[Value::Number(10.0)],
            )
            .unwrap()
            .unwrap();

        assert_eq!(damage, Value::Number(65.0));

        let health = bridge
            .get_property(HandleKind::Entity, entity_id.0, "health")
            .unwrap()
            .unwrap();

        assert_eq!(health, Value::Number(65.0));
    }

    #[test]
    fn test_entity_jump_and_destroy() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        let character_id = character_system
            .spawn_player(&world, None)
            .expect("character should spawn");

        let entity_id = entity_manager.create_entity("Villager");

        character_system.associate_entity(entity_id, character_id);

        if let Some(character) = character_system
            .get_active_characters_mut()
            .find(|character| character.id == character_id)
        {
            character.movement.is_grounded = true;
        }

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        let jump = bridge
            .call_method(
                HandleKind::Entity,
                entity_id.0,
                "jump",
                &[Value::Number(8.0)],
            )
            .unwrap();

        assert_eq!(jump, Some(Value::Nil));

        let velocity = bridge
            .get_property(HandleKind::Entity, entity_id.0, "velocity")
            .unwrap()
            .unwrap();

        let velocity = velocity.as_basket().unwrap();
        let velocity = velocity.borrow();

        assert_eq!(velocity.elements[1], Value::Number(8.0));

        let destroyed = bridge
            .call_method(HandleKind::Entity, entity_id.0, "destroy", &[])
            .unwrap();

        assert_eq!(destroyed, Some(Value::Bool(true)));

        assert!(!bridge.entity_manager.validate_handle(entity_id.0));
        assert_eq!(bridge.pending_destroys.as_slice(), &[entity_id.0]);
    }

    #[test]
    fn test_entity_find_path_dispatch() {
        let mut world = World::new();

        for x in 0..4 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }

        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();

        let character_id = character_system.spawn_character(Vec3::new(0.0, 1.0, 0.0), None);

        let entity_id = entity_manager.create_entity("Villager");

        character_system.associate_entity(entity_id, character_id);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        let result = bridge
            .call_method(
                HandleKind::Entity,
                entity_id.0,
                "find_path",
                &[Value::array(vec![
                    Value::Number(3.0),
                    Value::Number(1.0),
                    Value::Number(0.0),
                ])],
            )
            .unwrap();

        assert!(result.is_some());
        assert!(!matches!(result, Some(Value::Nil)));
    }

    #[test]
    fn entity_animation_handle_starts_a_masked_action_layer() {
        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();
        let entity_id = entity_manager.create_entity("Player");
        let character_id = character_system.spawn_character(Vec3::ZERO, None);
        character_system.associate_entity(entity_id, character_id);
        let character = character_system
            .get_character_for_entity(entity_id)
            .unwrap();
        let clip_name = character.package.clips[0].name.clone();
        let joint_name = character.package.skeleton.joints[0].name.clone();

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();
        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        let result = bridge
            .call_method(
                HandleKind::EntityAnimation,
                entity_id.0,
                "play_layer",
                &[
                    Value::String("action".into()),
                    Value::String(clip_name).into(),
                    Value::String(joint_name).into(),
                ],
            )
            .unwrap();
        assert_eq!(result, Some(Value::Bool(true)));
    }

    #[test]
    fn test_proj_find_is_recursive_singular_and_plural() {
        use crate::project::ProjectHierarchy;
        use crate::scripting::api::HostContext;
        use crate::scripting::interpreter::Interpreter;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;

        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let first_actor = world.add_actor("Coin", Vec3::ZERO);
        let second_actor = world.add_actor("Coin", Vec3::ONE);

        let first_entity = entity_manager.create_entity("Coin_A");
        let second_entity = entity_manager.create_entity("Coin_B");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: first_entity,
            authored_actor_id: Some(first_actor),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: second_entity,
            authored_actor_id: Some(second_actor),
            package_name: None,
            initial_position: Vec3::ONE,
        });

        let mut hierarchy = ProjectHierarchy::new();

        let dungeon = hierarchy.create_folder(None, "Dungeon").unwrap();

        let collectibles = hierarchy
            .create_folder(Some(dungeon), "Collectibles")
            .unwrap();

        hierarchy
            .create_actor_node(Some(collectibles), first_actor)
            .unwrap();

        hierarchy
            .create_actor_node(Some(collectibles), second_actor)
            .unwrap();

        let source = r#"
entity Test {
    fn singular() {
        return proj.find("Coin")
    }

    fn plural() {
        return proj.find.all("Coin")
    }

    fn missing() {
        return proj.find("Missing")
    }
}
"#;

        let program = Parser::new(
            Lexer::new(source)
                .tokenize()
                .expect("source should tokenize"),
        )
        .parse()
        .expect("source should parse");

        let mut interpreter = Interpreter::new(program);

        let actor_names = world
            .actors
            .iter()
            .map(|(id, actor)| (*id, actor.identity.clone()))
            .collect();

        interpreter.set_project_scope(hierarchy, actor_names);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 999_999, &mut host)
            .unwrap();

        let singular = interpreter
            .call(&mut instance, "singular", vec![], &mut host)
            .unwrap();

        assert_eq!(
            singular,
            Value::Handle {
                kind: HandleKind::Actor,
                id: first_actor,
            }
        );

        let plural = interpreter
            .call(&mut instance, "plural", vec![], &mut host)
            .unwrap();

        let plural = plural.as_basket().unwrap();
        let plural = plural.borrow();

        assert_eq!(
            plural.elements,
            vec![
                Value::Handle {
                    kind: HandleKind::Actor,
                    id: first_actor,
                },
                Value::Handle {
                    kind: HandleKind::Actor,
                    id: second_actor,
                },
            ]
        );

        drop(plural);

        let missing = interpreter
            .call(&mut instance, "missing", vec![], &mut host)
            .unwrap();

        assert_eq!(missing, Value::Nil);

        let warnings = interpreter
            .output()
            .iter()
            .filter(|record| record.severity == crate::scripting::log::LogSeverity::Warning)
            .map(|record| record.message.clone())
            .collect::<Vec<_>>();

        assert!(
            warnings
                .iter()
                .any(|message| { message.contains("expected 1 match, found 2; using [0]") })
        );

        assert!(
            warnings
                .iter()
                .any(|message| { message.contains("expected 1 match, found 0; returning nil") })
        );
    }

    #[test]
    fn test_project_paths_and_scoped_find_use_current_subtree() {
        use crate::project::ProjectHierarchy;
        use crate::scripting::api::HostContext;
        use crate::scripting::interpreter::Interpreter;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;

        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        let direct_coin = world.add_actor("Coin", Vec3::ZERO);
        let nested_coin = world.add_actor("Coin", Vec3::ONE);
        let outside_coin = world.add_actor("Coin", Vec3::new(2.0, 0.0, 0.0));

        let direct_entity = entity_manager.create_entity("DirectCoin");
        let nested_entity = entity_manager.create_entity("NestedCoin");
        let outside_entity = entity_manager.create_entity("OutsideCoin");

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: direct_entity,
            authored_actor_id: Some(direct_coin),
            package_name: None,
            initial_position: Vec3::ZERO,
        });

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: nested_entity,
            authored_actor_id: Some(nested_coin),
            package_name: None,
            initial_position: Vec3::ONE,
        });

        actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: outside_entity,
            authored_actor_id: Some(outside_coin),
            package_name: None,
            initial_position: Vec3::new(2.0, 0.0, 0.0),
        });

        let mut hierarchy = ProjectHierarchy::new();

        let room1 = hierarchy.create_folder(None, "Room1").unwrap();

        hierarchy
            .create_actor_node(Some(room1), direct_coin)
            .unwrap();

        let deep = hierarchy.create_folder(Some(room1), "Deep").unwrap();

        hierarchy
            .create_actor_node(Some(deep), nested_coin)
            .unwrap();

        let room2 = hierarchy.create_folder(None, "Room2").unwrap();

        hierarchy
            .create_actor_node(Some(room2), outside_coin)
            .unwrap();

        let source = r#"
entity Test {
    fn direct() {
        return proj.Room1.Coin
    }

    fn scoped_one() {
        const room = proj.Room1
        return room.find("Coin")
    }

    fn scoped_all() {
        const room = proj.Room1
        return room.find.all("Coin")
    }

    fn missing() {
        return proj.Room1.Missing
    }
}
"#;

        let program = Parser::new(
            Lexer::new(source)
                .tokenize()
                .expect("source should tokenize"),
        )
        .parse()
        .expect("source should parse");

        let mut interpreter = Interpreter::new(program);

        let actor_names = world
            .actors
            .iter()
            .map(|(id, actor)| (*id, actor.identity.clone()))
            .collect();

        interpreter.set_project_scope(hierarchy, actor_names);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );

        bridge.actor_system = Some(&mut actor_system);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 999_998, &mut host)
            .unwrap();

        // Exact paths inspect direct children only.
        let direct = interpreter
            .call(&mut instance, "direct", vec![], &mut host)
            .unwrap();

        assert_eq!(
            direct,
            Value::Handle {
                kind: HandleKind::Actor,
                id: direct_coin,
            }
        );

        // Singular scoped find recursively searches Room1 only.
        let scoped_one = interpreter
            .call(&mut instance, "scoped_one", vec![], &mut host)
            .unwrap();

        assert_eq!(
            scoped_one,
            Value::Handle {
                kind: HandleKind::Actor,
                id: direct_coin,
            }
        );

        // Plural scoped find includes direct + nested Room1 Actors,
        // but excludes the matching Actor under Room2.
        let scoped_all = interpreter
            .call(&mut instance, "scoped_all", vec![], &mut host)
            .unwrap();

        let scoped_all = scoped_all.as_basket().unwrap();
        let scoped_all = scoped_all.borrow();

        assert_eq!(
            scoped_all.elements,
            vec![
                Value::Handle {
                    kind: HandleKind::Actor,
                    id: direct_coin,
                },
                Value::Handle {
                    kind: HandleKind::Actor,
                    id: nested_coin,
                },
            ]
        );

        drop(scoped_all);

        let missing = interpreter
            .call(&mut instance, "missing", vec![], &mut host)
            .unwrap();

        assert_eq!(missing, Value::Nil);

        assert!(interpreter.output().iter().any(|record| {
            record.severity == crate::scripting::log::LogSeverity::Warning
                && record
                    .message
                    .contains("expected 1 match, found 2; using [0]")
        }));
    }

    #[test]
    fn player_root_resolves_to_runtime_actor_context() {
        use crate::scripting::api::HostContext;
        use crate::scripting::interpreter::Interpreter;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;

        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);
        world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);
        world.set_cell(WorldCoord::new(2, 0, 0), CellType::Block);

        let character_id = character_system
            .spawn_player(&world, None)
            .expect("player should spawn");

        let player_entity = entity_manager.create_entity("Player");

        character_system.associate_entity(player_entity, character_id);

        character_system.sync_entity_positions(&mut entity_manager);

        let initial_position = entity_manager
            .get_position(player_entity)
            .unwrap_or(Vec3::ZERO);

        let player_actor_id = actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: player_entity,
            authored_actor_id: None,
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
            package_name: Some("Rogue".to_string()),
            initial_position,
        });

        let source = r#"
entity Test {
    fn player_id() {
        return player.id
    }

    fn player_name() {
        return player.name
    }

    fn drive() {
        player.set_horizontal_velocity(3, 4)
        player.set_facing_direction(1, 0)
        return player.facing
    }

    fn impulse() {
        player.apply_vertical_impulse(7)
        return player.is_grounded()
    }

    fn begin_navigation() {
        return player.navigate_to([2.5, 1, 0.5], 2.5, 0.1)
    }

    fn navigation_active() {
        return player.is_navigating()
    }

    fn cancel_navigation() {
        return player.cancel_navigation()
    }
}
"#;

        let program = Parser::new(
            Lexer::new(source)
                .tokenize()
                .expect("source should tokenize"),
        )
        .parse()
        .expect("source should parse");

        let mut interpreter = Interpreter::new(program);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        bridge.actor_system = Some(&mut actor_system);

        assert_eq!(bridge.get_player_actor_id(), Some(player_actor_id));

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 999_997, &mut host)
            .unwrap();

        assert_eq!(
            interpreter
                .call(&mut instance, "player_id", vec![], &mut host,)
                .unwrap(),
            Value::Number(player_actor_id as f64)
        );

        assert_eq!(
            interpreter
                .call(&mut instance, "player_name", vec![], &mut host,)
                .unwrap(),
            Value::String("Player".to_string())
        );

        let facing = interpreter
            .call(&mut instance, "drive", vec![], &mut host)
            .unwrap();

        let facing = facing.as_basket().unwrap();
        let facing = facing.borrow();

        assert_eq!(
            facing.elements,
            vec![Value::Number(1.0), Value::Number(0.0),]
        );

        drop(facing);

        assert_eq!(
            interpreter
                .call(&mut instance, "impulse", vec![], &mut host,)
                .unwrap(),
            Value::Bool(false)
        );

        assert_eq!(
            interpreter
                .call(&mut instance, "begin_navigation", vec![], &mut host)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            interpreter
                .call(&mut instance, "navigation_active", vec![], &mut host)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            interpreter
                .call(&mut instance, "cancel_navigation", vec![], &mut host)
                .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            interpreter
                .call(&mut instance, "navigation_active", vec![], &mut host)
                .unwrap(),
            Value::Bool(false)
        );

        drop(host);

        let character_system = bridge.character_system.as_deref().unwrap();

        assert_eq!(
            character_system.get_entity_velocity(player_entity),
            Some(Vec3::new(3.0, 7.0, 4.0))
        );
    }

    #[test]
    fn player_is_nil_when_no_active_player_actor_exists() {
        use crate::scripting::api::HostContext;
        use crate::scripting::interpreter::Interpreter;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;

        let source = r#"
entity Test {
    fn has_no_player() {
        return player == nil
    }
}
"#;
        let program = Parser::new(
            Lexer::new(source)
                .tokenize()
                .expect("source should tokenize"),
        )
        .parse()
        .expect("source should parse");
        let mut interpreter = Interpreter::new(program);

        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            None,
        );
        assert_eq!(bridge.get_player_actor_id(), None);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };
        let mut instance = interpreter
            .instantiate_entity("Test", 999_995, &mut host)
            .unwrap();

        assert_eq!(
            interpreter
                .call(&mut instance, "has_no_player", vec![], &mut host)
                .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn player_find_resolves_player_modules_with_actor_parent() {
        use crate::scripting::api::HostContext;
        use crate::scripting::interpreter::Interpreter;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;

        fn parse(source: &str) -> crate::scripting::ast::Program {
            Parser::new(
                Lexer::new(source)
                    .tokenize()
                    .expect("source should tokenize"),
            )
            .parse()
            .expect("source should parse")
        }

        let mut world = World::new();
        let mut entity_manager = EntityManager::new();
        let mut character_system = CharacterSystem::new();
        let mut actor_system = crate::actor::ActorSystem::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        let character_id = character_system
            .spawn_player(&world, None)
            .expect("player should spawn");

        let player_entity = entity_manager.create_entity("Player");

        character_system.associate_entity(player_entity, character_id);

        character_system.sync_entity_positions(&mut entity_manager);

        let player_actor_id = actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: player_entity,
            authored_actor_id: None,
            package_name: Some("Rogue".to_string()),
            initial_position: Vec3::ZERO,
        });

        let source = r#"
entity Test {
    fn direct() {
        return player.controller.parent.id
    }

    fn singular() {
        return player.find("combat").parent.id
    }

    fn plural_count() {
        return basket.len(
            player.find.all("combat")
        )
    }
}
"#;

        let mut interpreter = Interpreter::new(parse(source));

        interpreter.loaded_scripts.insert(
            "playerscripts/Controller/controller.aeo".to_string(),
            parse(
                r#"
fn move() {
    return self.parent.id
}
"#,
            ),
        );

        interpreter.loaded_scripts.insert(
            "playerscripts/Combat/A/combat.aeo".to_string(),
            parse(
                r#"
fn attack() {
    return self.parent.id
}
"#,
            ),
        );

        interpreter.loaded_scripts.insert(
            "playerscripts/Combat/B/combat.aeo".to_string(),
            parse(
                r#"
fn attack() {
    return self.parent.id
}
"#,
            ),
        );

        interpreter.set_player_module_paths(vec![
            "playerscripts/Controller/controller.aeo".to_string(),
            "playerscripts/Combat/A/combat.aeo".to_string(),
            "playerscripts/Combat/B/combat.aeo".to_string(),
        ]);

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = make_bridge(
            &mut entity_manager,
            &mut world,
            &mut mouse,
            &mut dynamic_properties,
            &mut pending_events,
            &mut test_results,
            &mut pending_enable_scripts,
            &mut pending_disable_scripts,
            &mut runtime_ui,
            &mut pending_spawns,
            &mut pending_destroys,
            Some(&mut character_system),
        );

        bridge.actor_system = Some(&mut actor_system);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut bridge,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 999_996, &mut host)
            .unwrap();

        assert_eq!(
            interpreter
                .call(&mut instance, "direct", vec![], &mut host,)
                .unwrap(),
            Value::Number(player_actor_id as f64)
        );

        assert_eq!(
            interpreter
                .call(&mut instance, "singular", vec![], &mut host,)
                .unwrap(),
            Value::Number(player_actor_id as f64)
        );

        assert_eq!(
            interpreter
                .call(&mut instance, "plural_count", vec![], &mut host,)
                .unwrap(),
            Value::Number(2.0)
        );

        assert!(interpreter.output().iter().any(|record| {
            record.severity == crate::scripting::log::LogSeverity::Warning
                && record
                    .message
                    .contains("expected 1 match, found 2; using [0]")
        }));
    }

    #[test]
    fn save_namespace_round_trips_typed_values_and_commits() {
        use crate::engine::save_state::{SaveState, SaveValue};
        use crate::scripting::ast::Program;
        use crate::scripting::interpreter::{Interpreter, ScriptInstance};
        use crate::scripting::source::SourceSpan;

        let root = std::env::temp_dir().join(format!(
            "worldkiln_script_save_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("Project");
        let save_root = root.join("SaveData");
        std::fs::create_dir_all(&project).unwrap();
        let mut save_state = SaveState::load_from_root(&project, &save_root).unwrap();

        let mut entities = EntityManager::new();
        let mut world = World::new();
        let mut mouse = MouseController::default();
        let mut properties = std::collections::HashMap::new();
        let mut events = Vec::new();
        let mut results = std::collections::BTreeMap::new();
        let mut enable = Vec::new();
        let mut disable = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut spawns = Vec::new();
        let mut destroys = Vec::new();

        {
            let mut bridge = make_bridge(
                &mut entities,
                &mut world,
                &mut mouse,
                &mut properties,
                &mut events,
                &mut results,
                &mut enable,
                &mut disable,
                &mut runtime_ui,
                &mut spawns,
                &mut destroys,
                None,
            );
            bridge.save_state = Some(&mut save_state);

            let mut host = crate::scripting::api::HostContext {
                delta_time: 0.0,
                engine: &mut bridge,
            };
            let mut interpreter = Interpreter::new(Program {
                span: SourceSpan::new(0, 0),
                declarations: Vec::new(),
                statements: Vec::new(),
            });
            let mut instance = ScriptInstance::new_empty();
            let mut scopes = Vec::new();
            let call = |interpreter: &mut Interpreter,
                        instance: &mut ScriptInstance,
                        scopes: &mut Vec<crate::scripting::value::Scope>,
                        host: &mut crate::scripting::api::HostContext,
                        method: &str,
                        args: &[Value]| {
                crate::scripting::stdlib::call_stdlib_function(
                    interpreter,
                    instance,
                    scopes,
                    "save",
                    method,
                    args,
                    host,
                )
                .unwrap()
                .unwrap()
            };

            assert_eq!(
                call(
                    &mut interpreter,
                    &mut instance,
                    &mut scopes,
                    &mut host,
                    "get",
                    &[Value::String("score".into())]
                ),
                Value::Nil
            );
            call(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host,
                "set",
                &[Value::String("score".into()), Value::Number(42.0)],
            );
            call(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host,
                "set",
                &[Value::String("ready".into()), Value::Bool(true)],
            );
            call(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host,
                "set",
                &[Value::String("name".into()), Value::String("Kiln".into())],
            );
            assert_eq!(
                call(
                    &mut interpreter,
                    &mut instance,
                    &mut scopes,
                    &mut host,
                    "has",
                    &[Value::String("score".into())]
                ),
                Value::Bool(true)
            );

            let unsupported = crate::scripting::stdlib::call_stdlib_function(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                "save",
                "set",
                &[
                    Value::String("bad".into()),
                    Value::array(vec![Value::Number(1.0)]),
                ],
                &mut host,
            );
            assert!(unsupported.is_err());

            call(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host,
                "commit",
                &[],
            );
        }

        let loaded = SaveState::load_from_root(&project, &save_root).unwrap();
        assert_eq!(loaded.get("score"), Some(&SaveValue::Number(42.0)));
        assert_eq!(loaded.get("ready"), Some(&SaveValue::Bool(true)));
        assert_eq!(loaded.get("name"), Some(&SaveValue::String("Kiln".into())));

        std::fs::remove_dir_all(root).ok();
    }
}
