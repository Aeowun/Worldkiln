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

        let entity_id = self.entity_manager.create_entity(entity_name);
        self.entity_manager.set_position(entity_id, position);

        if let Some(pkg_name) = package_name {
            if crate::project::is_character_package(self.project_path, pkg_name) {
                if let Some(character_system) = self.character_system.as_deref_mut() {
                    let character_id = character_system.spawn_character(position, Some(pkg_name));
                    character_system.associate_entity(entity_id, character_id);
                }
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
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
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
            }
        }
    }
}
