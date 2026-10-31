// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::scripting::value::{HandleKind, Value};
use crate::world::cell::AttributeValue;
use crate::world::{CellType, WorldCoord};

use super::ScriptHostBridge;

impl<'a> ScriptHostBridge<'a> {
    pub fn lookup_light(&self, x: i32, y: i32, z: i32) -> Option<u64> {
        let coord = WorldCoord::new(x, y, z);

        if let Some(cell) = self.world.get(coord) {
            if cell.cell_type == CellType::Light {
                return Some(cell.id);
            }
        }

        None
    }

    pub fn is_light_enabled(&self, id: u64) -> Option<bool> {
        self.world
            .get_effective_cell_by_id(id)
            .filter(|cell| cell.cell_type == CellType::Light)
            .map(|_| self.world.is_light_enabled_by_id(id))
    }

    pub fn set_light_enabled(&mut self, id: u64, enabled: bool) {
        self.world.set_light_enabled_by_id_runtime(id, enabled);
    }

    pub fn is_collision_events_enabled(&self, id: u64) -> bool {
        if let Some(coord) = self.world.resolve_cell_id(id) {
            if let Some(runtime_state) = self.world.runtime_state.get(&id) {
                if let Some(enabled) = runtime_state.collision_events_enabled {
                    return enabled;
                }
            }

            if let Some(cell) = self.world.get_effective_cell(coord) {
                return cell.collision_events_enabled;
            }
        }

        true
    }

    pub fn create_runtime_cell(&mut self, cell_type: &str) -> Result<(HandleKind, u64), String> {
        let cell_type = match cell_type {
            "Block" => CellType::Block,
            "FxBlock" => CellType::FxBlock,
            "Player" => CellType::Player,
            "Light" => CellType::Light,
            "SpawnPoint" => CellType::SpawnPoint,
            "AudioEmitter" => CellType::AudioEmitter,
            "ParticleEmitter" => CellType::ParticleEmitter,
            "Empty" => {
                return Err("Cannot create Empty cell".to_string());
            }
            _ => {
                return Err(format!("Unknown cell type: {}", cell_type));
            }
        };

        let id = self.world.create_runtime_cell(cell_type);

        let kind = if cell_type == CellType::Light {
            HandleKind::Light
        } else {
            HandleKind::Cell
        };

        Ok((kind, id))
    }

    pub fn move_runtime_cell(&mut self, id: u64, x: i32, y: i32, z: i32) -> Result<(), String> {
        let coord = WorldCoord::new(x, y, z);
        self.world.move_runtime_cell(id, coord)
    }

    pub fn delete_cell(&mut self, id: u64) -> Result<(), String> {
        self.world.delete_cell_runtime(id);
        self.dynamic_properties.remove(&(HandleKind::Cell, id));
        self.dynamic_properties.remove(&(HandleKind::Light, id));
        Ok(())
    }

    pub fn get_all_cells_of_class(&self, class_name: &str) -> Vec<u64> {
        let mut results = Vec::new();

        for coord in self.world.iter_active_effective_coords() {
            if let Some(cell) = self.world.get_effective_cell(coord) {
                let matches = match class_name {
                    "Light" => cell.cell_type == CellType::Light,
                    "AudioEmitter" => cell.cell_type == CellType::AudioEmitter,
                    "ParticleEmitter" => cell.cell_type == CellType::ParticleEmitter,
                    "Block" => cell.cell_type == CellType::Block,
                    "FxBlock" => cell.cell_type == CellType::FxBlock,
                    "SpawnPoint" => cell.cell_type == CellType::SpawnPoint,
                    "Player" => cell.cell_type == CellType::Player,
                    _ => false,
                };

                if matches {
                    results.push(cell.id);
                }
            }
        }

        for source in self.world.iter_sources() {
            let matches = match class_name {
                "Light" => source.cell.cell_type == CellType::Light,
                "AudioEmitter" => source.cell.cell_type == CellType::AudioEmitter,
                "ParticleEmitter" => source.cell.cell_type == CellType::ParticleEmitter,
                _ => false,
            };
            if matches
                && self
                    .world
                    .get_effective_cell_by_id(source.cell.id)
                    .is_some()
                && !results.contains(&source.cell.id)
            {
                results.push(source.cell.id);
            }
        }

        for cell in self.world.runtime_cells.values() {
            let matches = match class_name {
                "Light" => cell.cell_type == CellType::Light,
                "AudioEmitter" => cell.cell_type == CellType::AudioEmitter,
                "ParticleEmitter" => cell.cell_type == CellType::ParticleEmitter,
                "Block" => cell.cell_type == CellType::Block,
                "FxBlock" => cell.cell_type == CellType::FxBlock,
                "SpawnPoint" => cell.cell_type == CellType::SpawnPoint,
                "Player" => cell.cell_type == CellType::Player,
                _ => false,
            };
            if matches
                && self.world.get_effective_cell_by_id(cell.id).is_some()
                && !results.contains(&cell.id)
            {
                results.push(cell.id);
            }
        }

        results
    }

    pub fn set_attribute(&mut self, id: u64, key: String, value: Value) -> Result<(), String> {
        if self.world.get_effective_cell_by_id(id).is_some() {
            let attribute = match value {
                Value::Number(n) => AttributeValue::Number(n),
                Value::Bool(b) => AttributeValue::Bool(b),
                Value::String(s) => AttributeValue::String(s),

                _ => {
                    return Err(format!(
                        "Cell attributes only support Number, Bool, or String. Got {}",
                        value.type_name()
                    ));
                }
            };

            self.world
                .runtime_state
                .entry(id)
                .or_default()
                .attribute_overrides
                .insert(key, attribute);

            Ok(())
        } else {
            Err("invalid cell handle for attribute assignment".to_string())
        }
    }

    pub fn remove_attribute(&mut self, id: u64, key: &str) -> Result<(), String> {
        if self.world.get_effective_cell_by_id(id).is_some() {
            if let Some(runtime_state) = self.world.runtime_state.get_mut(&id) {
                runtime_state.attribute_overrides.remove(key);
            }

            Ok(())
        } else {
            Err("invalid cell handle for attribute removal".to_string())
        }
    }

    pub fn set_handle_attribute(
        &mut self,
        kind: HandleKind,
        id: u64,
        key: String,
        value: Value,
    ) -> Result<(), String> {
        self.world
            .runtime_property_tweens
            .cancel(&(kind, id), &format!("attributes.{key}"));
        if matches!(kind, HandleKind::Cell | HandleKind::Light) {
            return self.set_attribute(id, key, value);
        }

        if !matches!(kind, HandleKind::Actor | HandleKind::Entity) {
            return Err(format!("{} does not support attributes", kind.name()));
        }

        let attribute = match value {
            Value::Number(number) => AttributeValue::Number(number),
            Value::Bool(boolean) => AttributeValue::Bool(boolean),
            Value::String(string) => AttributeValue::String(string),
            other => {
                return Err(format!(
                    "{} attributes only support Number, Bool, or String. Got {}",
                    kind.name(),
                    other.type_name()
                ));
            }
        };

        let entity_id = if kind == HandleKind::Actor {
            self.resolve_actor_entity(id)
                .ok_or_else(|| format!("invalid {} handle for attribute assignment", kind.name()))?
        } else {
            crate::engine::entity::EntityId(id)
        };

        self.entity_manager
            .set_attribute(entity_id, key, attribute)
            .then_some(())
            .ok_or_else(|| format!("invalid {} handle for attribute assignment", kind.name()))
    }

    pub fn remove_handle_attribute(
        &mut self,
        kind: HandleKind,
        id: u64,
        key: &str,
    ) -> Result<(), String> {
        self.world
            .runtime_property_tweens
            .cancel(&(kind, id), &format!("attributes.{key}"));
        if matches!(kind, HandleKind::Cell | HandleKind::Light) {
            return self.remove_attribute(id, key);
        }

        if !matches!(kind, HandleKind::Actor | HandleKind::Entity) {
            return Err(format!("{} does not support attributes", kind.name()));
        }

        let entity_id = if kind == HandleKind::Actor {
            self.resolve_actor_entity(id)
                .ok_or_else(|| format!("invalid {} handle for attribute removal", kind.name()))?
        } else {
            crate::engine::entity::EntityId(id)
        };

        if self.entity_manager.get_entity(entity_id).is_some() {
            self.entity_manager.remove_attribute(entity_id, key);
            Ok(())
        } else {
            Err(format!(
                "invalid {} handle for attribute removal",
                kind.name()
            ))
        }
    }

    pub fn cell_exists(&self, id: u64) -> bool {
        self.world.get_effective_cell_by_id(id).is_some()
    }
}
