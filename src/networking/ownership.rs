// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

pub type PlayerId = u64;
pub type CharacterId = u64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterState {
    pub owner: PlayerId,
    pub position: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnershipError {
    InvalidPlayer,
    AlreadyConnected,
    AlreadyOwnsCharacter,
    UnknownCharacter,
    NotOwner,
    InvalidMovement,
}

#[derive(Default)]
pub struct CharacterOwnership {
    players: HashMap<PlayerId, CharacterId>,
    characters: HashMap<CharacterId, CharacterState>,
    next_id: CharacterId,
}

impl CharacterOwnership {
    pub fn join(&mut self, player: PlayerId) -> Result<CharacterId, OwnershipError> {
        if player == 0 {
            return Err(OwnershipError::InvalidPlayer);
        }
        if self.players.contains_key(&player) {
            return Err(OwnershipError::AlreadyConnected);
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("network Character IDs exhausted");
        let id = self.next_id;
        self.players.insert(player, id);
        self.characters.insert(
            id,
            CharacterState {
                owner: player,
                position: [0.0; 3],
            },
        );
        Ok(id)
    }

    pub fn move_owned(
        &mut self,
        player: PlayerId,
        character: CharacterId,
        delta: [f32; 3],
    ) -> Result<[f32; 3], OwnershipError> {
        if !delta.iter().all(|axis| axis.is_finite()) {
            return Err(OwnershipError::InvalidMovement);
        }
        let state = self
            .characters
            .get_mut(&character)
            .ok_or(OwnershipError::UnknownCharacter)?;
        if state.owner != player || self.players.get(&player) != Some(&character) {
            return Err(OwnershipError::NotOwner);
        }
        let next = std::array::from_fn(|i| state.position[i] + delta[i]);
        if !next.iter().all(|axis: &f32| axis.is_finite()) {
            return Err(OwnershipError::InvalidMovement);
        }
        state.position = next;
        Ok(next)
    }

    pub fn character(&self, character: CharacterId) -> Option<CharacterState> {
        self.characters.get(&character).copied()
    }

    pub fn disconnect(&mut self, player: PlayerId) -> Option<CharacterId> {
        let id = self.players.remove(&player)?;
        self.characters.remove(&id);
        Some(id)
    }

    pub fn count(&self) -> usize {
        self.characters.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separate_owners_move_only_their_characters() {
        let mut world = CharacterOwnership::default();
        let a = world.join(11).unwrap();
        let b = world.join(22).unwrap();
        assert_ne!(a, b);
        assert_eq!(
            world.move_owned(11, a, [1.0, 0.0, 0.0]),
            Ok([1.0, 0.0, 0.0])
        );
        assert_eq!(
            world.move_owned(22, b, [0.0, 2.0, 0.0]),
            Ok([0.0, 2.0, 0.0])
        );
        assert_eq!(world.character(a).unwrap().position, [1.0, 0.0, 0.0]);
        assert_eq!(world.character(b).unwrap().position, [0.0, 2.0, 0.0]);
    }

    #[test]
    fn rejects_other_players_input_without_mutating_state() {
        let mut world = CharacterOwnership::default();
        let a = world.join(11).unwrap();
        world.join(22).unwrap();
        assert_eq!(
            world.move_owned(22, a, [500.0, 0.0, 0.0]),
            Err(OwnershipError::NotOwner)
        );
        assert_eq!(
            world.move_owned(99, a, [500.0, 0.0, 0.0]),
            Err(OwnershipError::NotOwner)
        );
        assert_eq!(world.character(a).unwrap().position, [0.0; 3]);
    }

    #[test]
    fn disconnect_removes_owned_character_and_stale_commands() {
        let mut world = CharacterOwnership::default();
        let a = world.join(11).unwrap();
        let b = world.join(22).unwrap();
        assert_eq!(world.disconnect(11), Some(a));
        assert_eq!(world.disconnect(11), None);
        assert_eq!(world.count(), 1);
        assert!(world.character(a).is_none());
        assert_eq!(
            world.move_owned(11, a, [1.0, 0.0, 0.0]),
            Err(OwnershipError::UnknownCharacter)
        );
        assert!(world.character(b).is_some());
    }

    #[test]
    fn rejects_invalid_registration_and_movement() {
        let mut world = CharacterOwnership::default();
        assert_eq!(world.join(0), Err(OwnershipError::InvalidPlayer));
        let a = world.join(11).unwrap();
        assert_eq!(world.join(11), Err(OwnershipError::AlreadyConnected));
        assert_eq!(
            world.move_owned(11, a, [f32::NAN, 0.0, 0.0]),
            Err(OwnershipError::InvalidMovement)
        );
        assert_eq!(
            world.move_owned(11, a, [f32::INFINITY, 0.0, 0.0]),
            Err(OwnershipError::InvalidMovement)
        );
    }
}
