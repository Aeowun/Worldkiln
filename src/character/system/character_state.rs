// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::CharacterSystem;
use crate::character::character::Character;
use glam::Vec3;

impl CharacterSystem {
    pub fn spawn_character(&mut self, position: Vec3, package_name: Option<&str>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let character = Character::new_from_package(
            id,
            position,
            package_name.unwrap_or(crate::character::package::DEFAULT_CHARACTER_PACKAGE),
        );

        self.characters.insert(id, character);

        id
    }

    pub fn get_active_characters(&self) -> impl Iterator<Item = &Character> {
        self.characters.values()
    }

    pub fn get_active_characters_mut(&mut self) -> impl Iterator<Item = &mut Character> {
        self.characters.values_mut()
    }

    pub fn get_active_player(&self) -> Option<&Character> {
        self.active_player_id
            .and_then(|id| self.characters.get(&id))
    }

    pub fn explicit_active_player_id(&self) -> Option<u64> {
        self.active_player_id
    }

    pub fn get_active_player_mut(&mut self) -> Option<&mut Character> {
        self.active_player_id
            .and_then(|id| self.characters.get_mut(&id))
    }

    pub fn destroy_active_player(&mut self) -> bool {
        let Some(character_id) = self.active_player_id.take() else {
            return false;
        };

        if let Some(entity_id) = self.character_to_entity.remove(&character_id) {
            self.cancel_entity_navigation(entity_id);
            self.entity_to_character.remove(&entity_id);
        }

        self.characters.remove(&character_id).is_some()
    }

    pub fn clear(&mut self) {
        self.characters.clear();
        self.entity_to_character.clear();
        self.character_to_entity.clear();
        self.kinematic_entities.clear();
        self.navigation_followers.clear();
        self.navigation_pending.clear();
        *self.navigation_builds.borrow_mut() = Default::default();
        self.navigation_finished.clear();
        self.navigation_failed.clear();
        *self.navigation_cache.borrow_mut() = None;
        self.active_player_id = None;

        self.next_id = 1;
    }

    pub fn has_characters(&self) -> bool {
        !self.characters.is_empty()
    }

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
