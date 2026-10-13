use glam::Vec3;
use std::collections::HashMap;

use crate::engine::entity::EntityId;

/// Runtime representation of an Actor during a single Play session.
///
/// `authored_actor_id` is `Some` for Actors created from authored World
/// Actors and `None` for Actors created dynamically at runtime.
#[derive(Clone, Debug)]
pub struct RuntimeActor {
    pub entity_id: EntityId,
    pub authored_actor_id: Option<u64>,
    pub package_name: Option<String>,
    pub initial_position: Vec3,
}

const RUNTIME_ACTOR_ID_START: u64 = 9_000_000_000_000_000;

/// Runtime bookkeeping for Actors.
///
/// ActorSystem does not own scripting, character movement, physics, or
/// rendering. It only tracks which runtime Entities are Actors and, for
/// authored Actors, maps Actor IDs to their runtime Entity IDs.
pub struct ActorSystem {
    actors: HashMap<EntityId, RuntimeActor>,

    /// Persistent authored Actor ID -> runtime Entity.
    actor_to_entity: HashMap<u64, EntityId>,

    /// Session-local Actor ID -> runtime Entity.
    runtime_actor_to_entity: HashMap<u64, EntityId>,

    /// Runtime Entity -> session-local Actor ID.
    entity_to_runtime_actor: HashMap<EntityId, u64>,

    next_runtime_actor_id: u64,
}

impl Default for ActorSystem {
    fn default() -> Self {
        Self {
            actors: HashMap::new(),
            actor_to_entity: HashMap::new(),
            runtime_actor_to_entity: HashMap::new(),
            entity_to_runtime_actor: HashMap::new(),
            next_runtime_actor_id: RUNTIME_ACTOR_ID_START,
        }
    }
}

impl ActorSystem {
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes all runtime Actor state.
    pub fn clear(&mut self) {
        self.actors.clear();
        self.actor_to_entity.clear();
        self.runtime_actor_to_entity.clear();
        self.entity_to_runtime_actor.clear();
        self.next_runtime_actor_id = RUNTIME_ACTOR_ID_START;
    }

    fn allocate_runtime_actor_id(&mut self) -> u64 {
        loop {
            let candidate = self.next_runtime_actor_id;

            self.next_runtime_actor_id = self
                .next_runtime_actor_id
                .checked_sub(1)
                .expect("runtime Actor ID space exhausted");

            if candidate == 0 {
                continue;
            }

            if self.actor_to_entity.contains_key(&candidate)
                || self.runtime_actor_to_entity.contains_key(&candidate)
            {
                continue;
            }

            return candidate;
        }
    }

    /// Registers or replaces a runtime Actor.
    ///
    /// Authored Actors expose their persistent authored Actor ID.
    /// Runtime-only Actors receive a session-local Actor ID.
    pub fn register_actor(&mut self, actor: RuntimeActor) -> u64 {
        let entity_id = actor.entity_id;

        let previous_runtime_id = self.entity_to_runtime_actor.remove(&entity_id);

        if let Some(runtime_id) = previous_runtime_id {
            self.runtime_actor_to_entity.remove(&runtime_id);
        }

        if let Some(previous) = self.actors.get(&entity_id) {
            if let Some(previous_actor_id) = previous.authored_actor_id {
                if self.actor_to_entity.get(&previous_actor_id) == Some(&entity_id) {
                    self.actor_to_entity.remove(&previous_actor_id);
                }
            }
        }

        let public_actor_id = if let Some(actor_id) = actor.authored_actor_id {
            if let Some(previous_entity_id) = self.actor_to_entity.insert(actor_id, entity_id) {
                if previous_entity_id != entity_id {
                    let lost_authored_identity =
                        self.actors
                            .get(&previous_entity_id)
                            .is_some_and(|previous_actor| {
                                previous_actor.authored_actor_id == Some(actor_id)
                            });

                    if lost_authored_identity {
                        if let Some(previous_actor) = self.actors.get_mut(&previous_entity_id) {
                            previous_actor.authored_actor_id = None;
                        }

                        let runtime_id = self.allocate_runtime_actor_id();

                        self.runtime_actor_to_entity
                            .insert(runtime_id, previous_entity_id);

                        self.entity_to_runtime_actor
                            .insert(previous_entity_id, runtime_id);
                    }
                }
            }

            actor_id
        } else {
            let runtime_id =
                previous_runtime_id.unwrap_or_else(|| self.allocate_runtime_actor_id());

            self.runtime_actor_to_entity.insert(runtime_id, entity_id);

            self.entity_to_runtime_actor.insert(entity_id, runtime_id);

            runtime_id
        };

        self.actors.insert(entity_id, actor);

        public_actor_id
    }

    /// Returns the runtime Actor associated with an Entity.
    pub fn get_actor(&self, entity_id: EntityId) -> Option<&RuntimeActor> {
        self.actors.get(&entity_id)
    }

    /// Returns a mutable runtime Actor.
    pub fn get_actor_mut(&mut self, entity_id: EntityId) -> Option<&mut RuntimeActor> {
        self.actors.get_mut(&entity_id)
    }

    /// Resolves either a persistent authored Actor ID or a session-local
    /// runtime Actor ID to its backing Entity.
    pub fn get_entity_for_actor(&self, actor_id: u64) -> Option<EntityId> {
        self.actor_to_entity
            .get(&actor_id)
            .copied()
            .or_else(|| self.runtime_actor_to_entity.get(&actor_id).copied())
    }

    /// Returns the public AeoScript Actor ID for a runtime Entity.
    pub fn script_actor_id_for_entity(&self, entity_id: EntityId) -> Option<u64> {
        let actor = self.actors.get(&entity_id)?;

        if let Some(authored_actor_id) = actor.authored_actor_id {
            return Some(authored_actor_id);
        }

        self.entity_to_runtime_actor.get(&entity_id).copied()
    }

    /// Returns the authored Actor ID for a runtime Entity, if it originated from an authored Actor.
    pub fn authored_actor_id_for_entity(&self, entity_id: EntityId) -> Option<u64> {
        self.get_actor(entity_id)
            .and_then(|actor| actor.authored_actor_id)
    }

    /// Returns the runtime Entity associated with an authored Actor ID.
    pub fn entity_for_authored_actor(&self, actor_id: u64) -> Option<EntityId> {
        self.get_entity_for_actor(actor_id)
    }

    /// Returns true when the runtime Entity is an Actor.
    pub fn is_actor(&self, entity_id: EntityId) -> bool {
        self.actors.contains_key(&entity_id)
    }

    /// Returns true when the runtime Entity is associated with an authored Actor.
    pub fn is_authored_actor(&self, entity_id: EntityId) -> bool {
        self.authored_actor_id_for_entity(entity_id).is_some()
    }

    /// Removes a runtime Actor and its authored Actor mapping, if any.
    pub fn remove_runtime_actor(&mut self, entity_id: EntityId) -> Option<RuntimeActor> {
        let actor = self.actors.remove(&entity_id)?;

        if let Some(actor_id) = actor.authored_actor_id {
            if self.actor_to_entity.get(&actor_id) == Some(&entity_id) {
                self.actor_to_entity.remove(&actor_id);
            }
        }

        if let Some(runtime_id) = self.entity_to_runtime_actor.remove(&entity_id) {
            self.runtime_actor_to_entity.remove(&runtime_id);
        }

        Some(actor)
    }

    /// Compatibility wrapper for remove_runtime_actor.
    pub fn remove_actor(&mut self, entity_id: EntityId) -> Option<RuntimeActor> {
        self.remove_runtime_actor(entity_id)
    }

    /// Returns all currently registered runtime Actors.
    pub fn actors(&self) -> &HashMap<EntityId, RuntimeActor> {
        &self.actors
    }

    /// Returns the authored Actor -> runtime Entity mapping.
    pub fn actor_to_entity_map(&self) -> &HashMap<u64, EntityId> {
        &self.actor_to_entity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_registration_creates_entity_and_actor_mapping() {
        let mut system = ActorSystem::new();

        let entity_id = EntityId(42);

        system.register_actor(RuntimeActor {
            entity_id,
            authored_actor_id: Some(100),
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
            package_name: Some("Rogue".to_string()),
            initial_position: Vec3::new(10.0, 1.0, 5.0),
        });

        assert!(system.is_actor(entity_id));
        assert_eq!(system.get_entity_for_actor(100), Some(entity_id));

        let actor = system
            .get_actor(entity_id)
            .expect("registered Actor should exist");

        assert_eq!(actor.entity_id, entity_id);
        assert_eq!(actor.authored_actor_id, Some(100));
        assert_eq!(actor.package_name.as_deref(), Some("Rogue"));
        assert_eq!(actor.initial_position, Vec3::new(10.0, 1.0, 5.0));
    }

    #[test]
    fn dynamic_actor_has_no_authored_actor_mapping() {
        let mut system = ActorSystem::new();

        let entity_id = EntityId(99);

        let runtime_actor_id = system.register_actor(RuntimeActor {
            entity_id,
            authored_actor_id: None,
            package_name: Some("OtherCharacterVariant".to_string()),
            initial_position: Vec3::ZERO,
        });

        assert_ne!(runtime_actor_id, entity_id.0);

        assert_eq!(
            system.get_entity_for_actor(runtime_actor_id),
            Some(entity_id)
        );

        assert_eq!(
            system.script_actor_id_for_entity(entity_id),
            Some(runtime_actor_id)
        );

        assert!(system.is_actor(entity_id));
        assert_eq!(
            system
                .get_actor(entity_id)
                .and_then(|actor| actor.authored_actor_id),
            None
        );
        assert_eq!(system.actor_to_entity_map().len(), 0);
    }

    #[test]
    fn removing_actor_removes_authored_actor_mapping() {
        let mut system = ActorSystem::new();

        let entity_id = EntityId(42);

        system.register_actor(RuntimeActor {
            entity_id,
            authored_actor_id: Some(100),
            package_name: Some("Rogue".to_string()),
            initial_position: Vec3::ZERO,
        });

        let removed = system
            .remove_actor(entity_id)
            .expect("Actor should be removed");

        assert_eq!(removed.entity_id, entity_id);
        assert!(!system.is_actor(entity_id));
        assert_eq!(system.get_entity_for_actor(100), None);
    }

    #[test]
    fn clear_removes_all_runtime_actor_state() {
        let mut system = ActorSystem::new();

        system.register_actor(RuntimeActor {
            entity_id: EntityId(1),
            authored_actor_id: Some(10),
            package_name: Some("Rogue".to_string()),
            initial_position: Vec3::ZERO,
        });

        system.register_actor(RuntimeActor {
            entity_id: EntityId(2),
            authored_actor_id: None,
            package_name: Some("custom_switch".to_string()),
            initial_position: Vec3::ONE,
        });

        system.clear();

        assert!(system.actors().is_empty());
        assert!(system.actor_to_entity_map().is_empty());
        assert!(!system.is_actor(EntityId(1)));
        assert!(!system.is_actor(EntityId(2)));
    }

    #[test]
    fn replacing_actor_cleans_previous_actor_mapping() {
        let mut system = ActorSystem::new();

        let entity_id = EntityId(42);

        system.register_actor(RuntimeActor {
            entity_id,
            authored_actor_id: Some(100),
            package_name: Some("Rogue".to_string()),
            initial_position: Vec3::ZERO,
        });

        system.register_actor(RuntimeActor {
            entity_id,
            authored_actor_id: Some(200),
            package_name: Some("OtherCharacterVariant".to_string()),
            initial_position: Vec3::ONE,
        });

        assert_eq!(system.get_entity_for_actor(100), None);
        assert_eq!(system.get_entity_for_actor(200), Some(entity_id));

        let actor = system
            .get_actor(entity_id)
            .expect("replaced Actor should exist");

        assert_eq!(actor.authored_actor_id, Some(200));
        assert_eq!(actor.package_name.as_deref(), Some("OtherCharacterVariant"));
    }

    #[test]
    fn replacing_actor_mapping_removes_stale_actor_ownership() {
        let mut system = ActorSystem::new();

        let first_entity = EntityId(1);
        let second_entity = EntityId(2);

        system.register_actor(RuntimeActor {
            entity_id: first_entity,
            authored_actor_id: Some(100),
            package_name: Some("Rogue".to_string()),
            initial_position: Vec3::ZERO,
        });

        system.register_actor(RuntimeActor {
            entity_id: second_entity,
            authored_actor_id: Some(100),
            package_name: Some("OtherCharacterVariant".to_string()),
            initial_position: Vec3::ONE,
        });

        assert_eq!(system.get_entity_for_actor(100), Some(second_entity));

        assert_eq!(
            system
                .get_actor(first_entity)
                .and_then(|actor| actor.authored_actor_id),
            None
        );

        assert_eq!(
            system
                .get_actor(second_entity)
                .and_then(|actor| actor.authored_actor_id),
            Some(100)
        );
    }
}
