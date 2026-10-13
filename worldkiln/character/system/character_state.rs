use super::CharacterSystem;
use crate::character::character::Character;
use glam::Vec3;

impl CharacterSystem {
    /// Spawns a generic character at an explicitly supplied world position.
    pub fn spawn_character(&mut self, position: Vec3, package_name: Option<&str>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let character =
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
            Character::new_from_package(id, position, package_name.unwrap_or(crate::character::package::DEFAULT_CHARACTER_PACKAGE));

        self.characters.insert(id, character);

        id
    }

    /// Returns all active runtime characters.
    pub fn get_active_characters(&self) -> impl Iterator<Item = &Character> {
        self.characters.values()
    }

    /// Returns all active runtime characters mutably.
    pub fn get_active_characters_mut(&mut self) -> impl Iterator<Item = &mut Character> {
        self.characters.values_mut()
    }

    /// Returns the authoritative active player.
    ///
    /// Only `spawn_player` assigns ownership. Authored package-backed Actors
    /// remain ordinary Characters, including when no Spawn Point is authored.
    pub fn get_active_player(&self) -> Option<&Character> {
        self.active_player_id
            .and_then(|id| self.characters.get(&id))
    }

    /// Returns only the explicitly selected runtime Player ID.
    ///
    /// Rendering and gameplay share the same explicit ownership contract.
    pub fn explicit_active_player_id(&self) -> Option<u64> {
        self.active_player_id
    }

    /// Returns the authoritative active player mutably.
    pub fn get_active_player_mut(&mut self) -> Option<&mut Character> {
        self.active_player_id
            .and_then(|id| self.characters.get_mut(&id))
    }

    /// Destroys the active player character.
    pub fn destroy_active_player(&mut self) -> bool {
        let Some(character_id) = self.active_player_id.take() else {
            return false;
        };

        if let Some(entity_id) = self.character_to_entity.remove(&character_id) {
            self.entity_to_character.remove(&entity_id);
        }

        self.characters.remove(&character_id).is_some()
    }

    /// Clears all runtime character state.
    pub fn clear(&mut self) {
        self.characters.clear();
        self.entity_to_character.clear();
        self.character_to_entity.clear();
        self.kinematic_entities.clear();
        self.navigation_followers.clear();
        self.navigation_finished.clear();
        self.navigation_failed.clear();
        *self.navigation_cache.borrow_mut() = None;
        self.active_player_id = None;

        self.next_id = 1;
    }

    pub fn has_characters(&self) -> bool {
        !self.characters.is_empty()
    }

    /// Explicitly teleports the active player.
    pub fn set_active_position(&mut self, position: Vec3) -> bool {
        let Some(character) = self.get_active_player_mut() else {
            return false;
        };

        character.transform.position = position;

        character.movement.velocity = Vec3::ZERO;

        character.movement.is_grounded = false;

        true
    }
}
