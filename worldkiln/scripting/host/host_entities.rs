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

        // CharacterSystem is authoritative for runtime character position.
        if let Some(character_system) = self.character_system.as_deref() {
            if let Some(character) = character_system.get_character_for_entity(entity_id) {
                return Some(character.transform.position);
            }
        }

        // Non-character entities use the script-facing EntityManager state.
        self.entity_manager.get_position(entity_id)
    }

    /// Handles the existing generic AeoScript `entity.set_position()` API.
    ///
    /// This is the ONLY place where a script-requested entity position change
    /// is forwarded into CharacterSystem.
    ///
    /// Normal character movement never comes through this function.
    pub fn set_position(&mut self, id: u64, position: Vec3) {
        let entity_id = EntityId(id);

        // Always update the script-facing entity representation.
        self.entity_manager.set_position(entity_id, position);

        // An entity explicitly calling set_position() is a deliberate
        // teleport request. Forward that operation to the authoritative
        // CharacterSystem representation.
        if let Some(character_system) = self.character_system.as_deref_mut() {
            character_system.set_entity_position(entity_id, position);
        }
    }

    pub fn entity_exists(&self, id: u64) -> bool {
        self.entity_manager.validate_handle(id)
    }

    /// Resolves a persistent authored Actor ID to the internal runtime Entity
    /// used by the engine for the current Play session.
    ///
    /// AeoScript Actor handles carry Actor IDs, never Entity IDs.
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
        // An authored attachment does not imply that its Actor still exists
        // in the current runtime session.
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

        // Authored Actors are exposed to AeoScript as Actor handles.
        //
        // The handle ID is the persistent authored Actor ID. Runtime Entity IDs
        // remain internal implementation details.
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

        // Preserve compatibility for runtime-only or legacy Entities.
        //
        // If an authored Actor matched, do not expose the backing Entity as a
        // second public object with the same identity.
        if results.is_empty() {
            if let Some(entity_id) = self.entity_manager.lookup_entity(query) {
                results.push((HandleKind::Entity, entity_id.0));
            }
        }

        // Cells remain their own public scripting objects.
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
            if source.cell.entity_identity.as_deref() == Some(query) {
                let kind = if source.cell.cell_type == CellType::Light {
                    HandleKind::Light
                } else {
                    HandleKind::Cell
                };
                results.push((kind, source.cell.id));
            }
        }

        results
    }
}
