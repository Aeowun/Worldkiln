use super::CharacterSystem;
use crate::engine::entity::EntityId;

impl CharacterSystem {
    /// Returns current health.
    pub fn get_entity_health(&self, entity_id: EntityId) -> Option<f32> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.health)
    }

    /// Returns maximum health.
    pub fn get_entity_max_health(&self, entity_id: EntityId) -> Option<f32> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.max_health)
    }

    /// Sets current health, clamped to [0, max_health].
    pub fn set_entity_health(&mut self, entity_id: EntityId, health: f32) -> bool {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.health = health.clamp(0.0, character.max_health);

        true
    }

    /// Sets maximum health and clamps current health if necessary.
    pub fn set_entity_max_health(&mut self, entity_id: EntityId, max_health: f32) -> bool {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.max_health = max_health.max(1.0);

        character.health = character.health.min(character.max_health);

        true
    }

    /// Applies damage and returns the resulting health.
    pub fn damage_entity(&mut self, entity_id: EntityId, amount: f32) -> Option<f32> {
        let character = self.get_character_for_entity_mut(entity_id)?;

        character.health = (character.health - amount.max(0.0)).max(0.0);

        Some(character.health)
    }

    /// Heals the character and returns the resulting health.
    pub fn heal_entity(&mut self, entity_id: EntityId, amount: f32) -> Option<f32> {
        let character = self.get_character_for_entity_mut(entity_id)?;

        character.health = (character.health + amount.max(0.0)).min(character.max_health);

        Some(character.health)
    }

    /// Returns whether the character is alive.
    pub fn is_entity_alive(&self, entity_id: EntityId) -> Option<bool> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.health > 0.0)
    }
}
