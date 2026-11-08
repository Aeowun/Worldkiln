// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::Vec3;

use crate::engine::entity::{EntityId, EntityManager};
use crate::scripting::value::HandleKind;
use crate::world::CellType;

use super::ScriptHostBridge;

impl<'a> ScriptHostBridge<'a> {
    pub fn entity_manager_ref(&self) -> &EntityManager {
        self.entity_manager
    }

    pub fn get_position(&self, id: u64) -> Option<Vec3> {
        let entity_id = EntityId(id);

        if let Some(character_system) = self.character_system.as_deref() {
            if let Some(character) = character_system.get_character_for_entity(entity_id) {
                return Some(character.transform.position);
            }
        }

        self.entity_manager.get_position(entity_id)
    }

    pub fn set_position(&mut self, id: u64, position: Vec3) {
        let entity_id = EntityId(id);

        self.entity_manager.set_position(entity_id, position);

        if let Some(character_system) = self.character_system.as_deref_mut() {
            character_system.set_entity_position(entity_id, position);
        }
    }

    pub fn entity_exists(&self, id: u64) -> bool {
        self.entity_manager.validate_handle(id)
    }

    pub fn resolve_actor_entity(&self, actor_id: u64) -> Option<EntityId> {
        let entity_id = self
            .actor_system
            .as_deref()
            .and_then(|system| system.get_entity_for_actor(actor_id))?;

        self.entity_manager
            .validate_handle(entity_id.0)
            .then_some(entity_id)
    }

    pub fn actor_exists(&self, actor_id: u64) -> bool {
        self.resolve_actor_entity(actor_id).is_some()
    }

    pub fn get_actor_module_path(&self, actor_id: u64, module_name: &str) -> Option<String> {
        if !self.actor_exists(actor_id) {
            return None;
        }

        let module = self.world.actor_module(actor_id, module_name)?;

        if !module.enabled {
            return None;
        }

        Some(module.script_path)
    }

    pub fn entity_character_id(&self, entity_id: EntityId) -> Option<u64> {
        self.character_system
            .as_deref()
            .and_then(|system| system.get_character_id_for_entity(entity_id))
    }

    pub fn get_children(&self, _kind: HandleKind, _id: u64) -> Vec<(HandleKind, u64)> {
        Vec::new()
    }

    pub fn get_parent(&self, kind: HandleKind, id: u64) -> Option<(HandleKind, u64)> {
        if kind != HandleKind::Entity {
            return None;
        }

        let actor_id = self
            .actor_system
            .as_deref()?
            .authored_actor_id_for_entity(EntityId(id))?;

        Some((HandleKind::Actor, actor_id))
    }

    pub fn get_cell_object(&self, cell_id: u64) -> Option<(HandleKind, u64)> {
        let coord = self.world.resolve_cell_id(cell_id)?;

        let cell = self.world.get_effective_cell(coord)?;

        let identity = cell.entity_identity.as_ref()?;

        let entity_id = self.entity_manager.lookup_entity(identity)?;

        Some((HandleKind::Entity, entity_id.0))
    }

    pub fn find_objects(&self, query: &str) -> Vec<(HandleKind, u64)> {
        let mut results = Vec::new();

        for actor in self.world.actors.values() {
            let identity = {
                let trimmed = actor.identity.trim();

                if !trimmed.is_empty() {
                    trimmed.to_string()
                } else {
                    format!("Actor_{}", actor.id)
                }
            };

            if identity == query && self.actor_exists(actor.id) {
                results.push((HandleKind::Actor, actor.id));
            }
        }

        if results.is_empty() {
            if let Some(entity_id) = self.entity_manager.lookup_entity(query) {
                results.push((HandleKind::Entity, entity_id.0));
            }
        }

        for coord in self.world.iter_active_effective_coords() {
            if let Some(cell) = self.world.get_effective_cell(coord) {
                if let Some(identity) = &cell.entity_identity {
                    if identity == query {
                        let kind = match cell.cell_type {
                            CellType::Light => HandleKind::Light,
                            _ => HandleKind::Cell,
                        };

                        results.push((kind, cell.id));
                    }
                }
            }
        }

        for source in self.world.iter_sources() {
            if source.cell.entity_identity.as_deref() == Some(query)
                && self
                    .world
                    .get_effective_cell_by_id(source.cell.id)
                    .is_some()
            {
                let kind = if source.cell.cell_type == CellType::Light {
                    HandleKind::Light
                } else {
                    HandleKind::Cell
                };
                results.push((kind, source.cell.id));
            }
        }

        for cell in self.world.runtime_cells.values() {
            if cell.entity_identity.as_deref() != Some(query)
                || self.world.get_effective_cell_by_id(cell.id).is_none()
            {
                continue;
            }
            let kind = if cell.cell_type == CellType::Light {
                HandleKind::Light
            } else {
                HandleKind::Cell
            };
            if !results.contains(&(kind, cell.id)) {
                results.push((kind, cell.id));
            }
        }

        results
    }
}
