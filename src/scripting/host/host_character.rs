// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::sync::Arc;

use glam::Vec3;

use super::ScriptHostBridge;
use crate::character::TargetAnimation;

impl<'a> ScriptHostBridge<'a> {
    pub fn is_entity_declaration_valid(&self, entity_name: &str) -> bool {
        !entity_name.trim().is_empty()
    }

    pub fn spawn_entity(
        &mut self,
        entity_name: &str,
        position: Vec3,
        package_name: Option<&str>,
    ) -> Result<u64, String> {
        if !self.is_entity_declaration_valid(entity_name) {
            return Err("Entity name cannot be empty".to_string());
        }

        let character_package_name = package_name
            .filter(|name| crate::project::is_character_package(self.project_path, name));
        if let Some(name) = character_package_name {
            let character_system = self
                .character_system
                .as_deref()
                .ok_or_else(|| "CharacterSystem unavailable for runtime spawn".to_string())?;
            let package = crate::character::package::resolve_package(name)
                .ok_or_else(|| format!("Character package not loaded: {name}"))?;
            if !crate::character::spawning::runtime_character_spawn_is_clear(
                self.world,
                self.entity_manager,
                character_system,
                position,
                &package.collision,
            ) {
                return Err(
                    "spawn position obstructed: no collision-free Character clearance".to_string(),
                );
            }
        }
        let entity_id = self.entity_manager.create_entity(entity_name);
        self.entity_manager.set_position(entity_id, position);

        if let Some(pkg_name) = package_name {
            if character_package_name.is_some() {
                let character_system = self
                    .character_system
                    .as_deref_mut()
                    .expect("checked before allocating runtime Character");
                let character_id = character_system.spawn_character(position, Some(pkg_name));
                character_system.associate_entity(entity_id, character_id);
            } else {
                self.entity_manager.apply_package(entity_id, pkg_name);
            }
        }

        if let Some(actor_system) = self.actor_system.as_deref_mut() {
            let runtime_actor = crate::actor::RuntimeActor {
                entity_id,
                authored_actor_id: None,
                package_name: package_name.map(|s| s.to_string()),
                initial_position: position,
            };

            actor_system.register_actor(runtime_actor);
        }

        self.pending_spawns
            .push((entity_name.to_string(), entity_id.0));

        Ok(entity_id.0)
    }

    pub fn spawn_character(
        &mut self,
        entity_name: &str,
        position: Vec3,
        character_package: Option<&str>,
    ) -> Result<u64, String> {
        let pkg = character_package.or(Some(crate::character::package::DEFAULT_CHARACTER_PACKAGE));
        self.spawn_entity(entity_name, position, pkg)
    }

    pub fn set_valid_entity_declarations(&mut self, decls: Arc<std::collections::HashSet<String>>) {
        self.valid_entity_declarations = Some(decls);
    }

    pub fn drain_pending_spawns(&mut self) -> Vec<(String, u64)> {
        std::mem::take(self.pending_spawns)
    }

    pub fn get_player_actor_id(&self) -> Option<u64> {
        let character_system = self.character_system.as_deref()?;
        let player = character_system.get_active_player()?;

        let entity_id = character_system.get_entity_for_character(player.id)?;

        self.actor_system
            .as_deref()?
            .script_actor_id_for_entity(entity_id)
    }

    pub fn get_player_position(&self) -> Option<[f32; 3]> {
        let system = self.character_system.as_deref()?;
        let player = system.get_active_player()?;

        let position = player.transform.position;

        Some([position.x, position.y, position.z])
    }

    pub fn is_player_swimming(&self) -> bool {
        self.character_system
            .as_deref()
            .and_then(|system| system.get_active_player())
            .is_some_and(|character| character.movement.is_swimming)
    }

    pub fn set_player_velocity(&mut self, velocity: [f32; 3]) -> Result<(), String> {
        let value = Vec3::from_array(velocity);
        if !value.is_finite() {
            return Err("player.set_velocity requires finite components".into());
        }
        if let Some(character) = self
            .character_system
            .as_deref_mut()
            .and_then(|c| c.get_active_player_mut())
        {
            character.movement.velocity = value;
        }
        Ok(())
    }

    pub fn get_player_velocity(&self) -> Option<[f32; 3]> {
        self.character_system
            .as_deref()?
            .get_active_player()
            .map(|c| c.movement.velocity.to_array())
    }

    pub fn set_player_gravity_scale(&mut self, scale: f32) -> Result<(), String> {
        if !scale.is_finite() || !(-100.0..=100.0).contains(&scale) {
            return Err("player.set_gravity_scale expects a finite value from -100 to 100".into());
        }
        if let Some(character) = self
            .character_system
            .as_deref_mut()
            .and_then(|c| c.get_active_player_mut())
        {
            character.movement.gravity_scale = scale;
        }
        Ok(())
    }

    pub fn get_player_gravity_scale(&self) -> Option<f32> {
        self.character_system
            .as_deref()?
            .get_active_player()
            .map(|c| c.movement.gravity_scale)
    }

    pub fn set_player_controller_enabled(&mut self, enabled: bool) {
        if let Some(character) = self
            .character_system
            .as_deref_mut()
            .and_then(|c| c.get_active_player_mut())
        {
            character.movement.controller_enabled = enabled;
        }
    }

    pub fn is_player_controller_enabled(&self) -> bool {
        self.character_system
            .as_deref()
            .and_then(|c| c.get_active_player())
            .is_none_or(|c| c.movement.controller_enabled)
    }

    pub fn set_player_horizontal_velocity(&mut self, velocity_x: f32, velocity_z: f32) {
        if let Some(system) = self.character_system.as_deref_mut() {
            if let Some(player) = system.get_active_player_mut() {
                player.movement.velocity.x = velocity_x;
                player.movement.velocity.z = velocity_z;
            }
        }
    }

    pub fn set_player_facing_direction(&mut self, direction_x: f32, direction_z: f32) {
        if let Some(system) = self.character_system.as_deref_mut() {
            if let Some(player) = system.get_active_player_mut() {
                let angle = f32::atan2(direction_x, direction_z);

                player.transform.rotation = glam::Quat::from_rotation_y(angle);
            }
        }
    }

    pub fn select_player_animation(&mut self, animation: &str) {
        if let Some(system) = self.character_system.as_deref_mut() {
            if let Some(player) = system.get_active_player_mut() {
                let target_animation = match animation {
                    "Walk" => TargetAnimation::Walk,
                    "Idle" => TargetAnimation::Idle,
                    "Run" => TargetAnimation::Run,
                    other => TargetAnimation::Named(other.to_string()),
                };

                player.animation_override = Some(target_animation.clone());
                player
                    .animation_controller
                    .select_animation(target_animation);
            }
        }
    }

    pub fn is_player_grounded(&self) -> bool {
        if let Some(system) = self.character_system.as_deref() {
            if let Some(player) = system.get_active_player() {
                return player.movement.is_grounded;
            }
        }

        true
    }

    pub fn apply_player_vertical_impulse(&mut self, impulse: f32) {
        if let Some(system) = self.character_system.as_deref_mut() {
            if let Some(player) = system.get_active_player_mut() {
                player.movement.velocity.y = impulse;
                player.movement.is_grounded = false;
                if player.movement.swim_mode == crate::character::movement::SwimMode::Surface
                    && impulse > 0.0
                {
                    player.movement.surface_jump_remaining = 0.52;
                    player.movement.swim_mode = crate::character::movement::SwimMode::Dry;
                    player.movement.is_swimming = false;
                    player.movement.surface_lock_blend = 0.0;
                }
            }
        }
    }
}
