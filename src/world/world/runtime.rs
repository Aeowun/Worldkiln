// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::World;
use super::dirty::DirtyReason;
use crate::world::cell::{AttributeValue, Cell, CellType};
use crate::world::coordinate::WorldCoord;
use glam::Vec3;

impl World {
    pub fn set_runtime_cell_position(&mut self, id: u64, position: Vec3) -> Result<(), String> {
        if !position.is_finite() || !self.runtime_cells.contains_key(&id) {
            return Err("Runtime Attachment Cell is unavailable".into());
        }
        self.runtime_state.entry(id).or_default().source_position = Some(position);
        Ok(())
    }

    pub fn create_manifest_attachment(
        &mut self,
        template: &crate::project::character_attachment_manifest::CharacterAttachmentTemplate,
    ) -> u64 {
        use crate::project::character_attachment_manifest::AttachmentComponent as C;
        let kind = match template.component {
            C::PointLight { .. } => CellType::Light,
            C::SpatialAudio { .. } => CellType::AudioEmitter,
            C::ParticleEmitter { .. } => CellType::ParticleEmitter,
        };
        let id = self.create_runtime_cell(kind);
        if let Some(cell) = self.runtime_cells.get_mut(&id) {
            cell.entity_identity = Some(template.name.clone());
            match &template.component {
                C::PointLight {
                    color,
                    intensity,
                    range,
                    shadows,
                } => {
                    cell.light_color = Vec3::from_array(*color);
                    cell.light_intensity = *intensity;
                    cell.light_range = *range;
                    cell.light_shadows = *shadows;
                }
                C::SpatialAudio {
                    audio,
                    play_on_start,
                    looped,
                    volume,
                    min_distance,
                    max_distance,
                } => {
                    cell.audio = audio.clone().unwrap_or_default();
                    cell.playing = *play_on_start;
                    cell.looped = *looped;
                    cell.volume = *volume;
                    cell.audio_min_distance = *min_distance;
                    cell.audio_max_distance = *max_distance;
                }
                C::ParticleEmitter {
                    effect,
                    preset: _,
                    rate,
                    scale,
                    radius,
                } => {
                    cell.particles.effect_asset = effect.clone();
                    cell.particles.rate = *rate;
                    cell.particles.scale = *scale;
                    cell.particles.radius = *radius;
                }
            }
        }
        id
    }
    pub fn set_attachment_source_position(
        &mut self,
        id: u64,
        position: Vec3,
    ) -> Result<(), String> {
        if !position.is_finite()
            || !self.get_effective_cell_by_id(id).is_some_and(|cell| {
                matches!(
                    cell.cell_type,
                    CellType::Light | CellType::AudioEmitter | CellType::ParticleEmitter
                )
            })
        {
            return Err("Attachment requires a live Light, Audio Emitter or Particle Emitter with a finite position".into());
        }
        self.runtime_state.entry(id).or_default().source_position = Some(position);
        Ok(())
    }
    fn runtime_cell_uses_voxel_slot(cell_type: CellType) -> bool {
        !matches!(
            cell_type,
            CellType::Light | CellType::AudioEmitter | CellType::ParticleEmitter
        )
    }

    fn restore_runtime_coord_owner(&mut self, coord: WorldCoord, excluding_id: u64) {
        let replacement = self.runtime_id_to_coord.iter().find_map(|(id, candidate)| {
            if *id == excluding_id || *candidate != coord {
                return None;
            }
            self.runtime_cells
                .get(id)
                .is_some_and(|cell| Self::runtime_cell_uses_voxel_slot(cell.cell_type))
                .then_some(*id)
        });
        if let Some(id) = replacement {
            self.coord_to_runtime_id.insert(coord, id);
        }
    }

    pub fn set_light_enabled_runtime(&mut self, coord: WorldCoord, enabled: bool) {
        let Some(id) = self.get_effective_cell(coord).map(|cell| cell.id) else {
            return;
        };

        self.set_light_enabled_by_id_runtime(id, enabled);
    }

    pub fn set_light_enabled_by_id_runtime(&mut self, id: u64, enabled: bool) {
        if self
            .get_effective_cell_by_id(id)
            .is_none_or(|cell| cell.cell_type != CellType::Light)
        {
            return;
        }

        let runtime_state = self.runtime_state.entry(id).or_default();

        if runtime_state.light_enabled == Some(enabled) {
            return;
        }

        runtime_state.light_enabled = Some(enabled);
    }

    pub fn set_light_property_runtime(
        &mut self,
        id: u64,
        property: &str,
        value: crate::scripting::value::Value,
    ) -> Result<(), String> {
        let cell = self
            .get_effective_cell_by_id(id)
            .ok_or_else(|| "Invalid Light handle".to_string())?;
        if cell.cell_type != CellType::Light {
            return Err("Light property requires a real Light Cell".into());
        }
        use crate::scripting::value::Value;
        let state = self.runtime_state.entry(id).or_default();
        match property {
            "color" => {
                let a = value.as_basket()?;
                let b = a.borrow();
                if b.elements.len() != 3 {
                    return Err("Light.color must be [r,g,b]".into());
                }
                let mut rgb = [0_f32; 3];
                for (i, x) in b.elements.iter().enumerate() {
                    let number = x.as_number()?;
                    if !number.is_finite() || !(0.0..=1.0).contains(&number) {
                        return Err("Light.color channels must be between 0 and 1".into());
                    }
                    rgb[i] = number as f32;
                }
                state.light_color = Some(Vec3::from_array(rgb));
            }
            "intensity" | "range" => {
                let number = value.as_number()?;
                if !number.is_finite() || number < 0.0 || number > f32::MAX as f64 {
                    return Err(format!("Light.{property} must be finite and nonnegative"));
                }
                match property {
                    "intensity" => state.light_intensity = Some(number as f32),
                    _ => state.light_range = Some(number as f32),
                }
            }
            "shadows" => state.light_shadows = Some(value.as_bool()?),
            _ => return Err(format!("Unknown emitted Light property {property}")),
        }
        Ok(())
    }
    pub fn emitted_light_runtime(&self, id: u64) -> Option<(Vec3, f32, f32, bool)> {
        let cell = self.get_effective_cell_by_id(id)?;
        if cell.cell_type != CellType::Light {
            return None;
        }
        let rs = self.runtime_state.get(&id);
        Some((
            rs.and_then(|s| s.light_color).unwrap_or(cell.light_color),
            rs.and_then(|s| s.light_intensity)
                .unwrap_or(cell.light_intensity),
            rs.and_then(|s| s.light_range).unwrap_or(cell.light_range),
            rs.and_then(|s| s.light_shadows)
                .unwrap_or(cell.light_shadows),
        ))
    }

    pub fn set_cell_visible_runtime(&mut self, coord: WorldCoord, visible: bool) {
        self.bump_render_revision();
        self.mark_render_dirty(coord, DirtyReason::Geometry);

        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            let runtime_state = self.runtime_state.entry(id).or_default();

            if runtime_state.visible != Some(visible) {
                runtime_state.visible = Some(visible);
            }
        }

        self.update_spatial_index_at(coord);
    }

    pub fn runtime_audio_position(&self, id: u64) -> Option<Vec3> {
        self.runtime_state
            .get(&id)
            .and_then(|state| state.source_position)
    }

    pub fn resolve_sound_id(&self, id: u64) -> u64 {
        self.runtime_entity_audio.get(&id).copied().unwrap_or(id)
    }

    pub fn set_audio_path_runtime(&mut self, id: u64, raw_path: &str) -> Result<(), String> {
        let path = raw_path.strip_prefix("audio/").unwrap_or(raw_path);
        let rel = std::path::Path::new(path);
        if path.is_empty()
            || path.contains('\\')
            || path.contains(':')
            || !rel
                .components()
                .all(|p| matches!(p, std::path::Component::Normal(_)))
        {
            return Err("Sound.set expects a safe relative audio asset path".to_string());
        }
        let id = self.resolve_sound_id(id);
        let Some(cell) = self.get_effective_cell_by_id(id) else {
            return Err("Sound.set requires an existing AudioEmitter or Entity".to_string());
        };
        if cell.cell_type != CellType::AudioEmitter {
            return Err("Sound.set requires an AudioEmitter Cell (or an Entity)".to_string());
        }
        self.runtime_state.entry(id).or_default().audio_path = Some(path.to_string());
        Ok(())
    }

    pub fn ensure_entity_sound(&mut self, entity_id: u64, position: Vec3) -> u64 {
        if let Some(id) = self.runtime_entity_audio.get(&entity_id).copied() {
            return id;
        }
        let id = self.create_runtime_cell(CellType::AudioEmitter);
        self.runtime_entity_audio.insert(entity_id, id);
        self.runtime_state.entry(id).or_default().source_position = Some(position);
        id
    }

    pub fn sync_entity_sound_positions(&mut self, entities: &crate::engine::entity::EntityManager) {
        let tracked: Vec<(u64, u64)> = self
            .runtime_entity_audio
            .iter()
            .map(|(&entity, &emitter)| (entity, emitter))
            .collect();
        for (entity, emitter) in tracked {
            if let Some(actor) = entities.get_entity(crate::engine::entity::EntityId(entity)) {
                self.runtime_state
                    .entry(emitter)
                    .or_default()
                    .source_position = Some(actor.position);
            } else {
                self.delete_cell_runtime(emitter);
                self.runtime_entity_audio.remove(&entity);
            }
        }
    }

    pub fn set_audio_playing_runtime(&mut self, cell_id: u64, playing: bool) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            let rs = self.runtime_state.entry(cell_id).or_default();

            rs.audio_playing = Some(playing);

            if playing {
                rs.audio_paused = Some(false);
            }
        }
    }

    pub fn set_audio_paused_runtime(&mut self, cell_id: u64, paused: bool) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            let rs = self.runtime_state.entry(cell_id).or_default();

            rs.audio_paused = Some(paused);
        }
    }

    pub fn set_audio_looped_runtime(&mut self, cell_id: u64, looped: bool) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            let rs = self.runtime_state.entry(cell_id).or_default();

            rs.audio_looped = Some(looped);
        }
    }

    pub fn set_audio_pitch_runtime(&mut self, id: u64, rate: f32) -> Result<(), String> {
        if !rate.is_finite() || !(0.25..=4.0).contains(&rate) {
            return Err("Sound.pitch(rate) expects a finite multiplier from 0.25 to 4.0".into());
        }
        let id = self.resolve_sound_id(id);
        if self
            .get_effective_cell_by_id(id)
            .is_none_or(|cell| cell.cell_type != CellType::AudioEmitter)
        {
            return Err("Sound.pitch(rate) requires an AudioEmitter or Entity sound".into());
        }
        self.runtime_state.entry(id).or_default().audio_pitch = Some(rate);
        Ok(())
    }

    pub fn set_audio_volume_runtime(&mut self, cell_id: u64, volume: f32) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            let rs = self.runtime_state.entry(cell_id).or_default();

            rs.audio_volume = Some(volume);
        }
    }

    pub fn set_audio_min_distance_runtime(&mut self, cell_id: u64, distance: f32) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() && distance.is_finite() {
            let max_distance = self.get_audio_max_distance(cell_id);
            self.runtime_state
                .entry(cell_id)
                .or_default()
                .audio_min_distance = Some(distance.max(0.0).min(max_distance));
        }
    }

    pub fn set_audio_max_distance_runtime(&mut self, cell_id: u64, distance: f32) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() && distance.is_finite() {
            let min_distance = self.get_audio_min_distance(cell_id);
            self.runtime_state
                .entry(cell_id)
                .or_default()
                .audio_max_distance = Some(distance.max(min_distance));
        }
    }

    pub fn audio_play_runtime(&mut self, cell_id: u64) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            println!("[AUDIO] play requested for emitter {}", cell_id);

            let rs = self.runtime_state.entry(cell_id).or_default();

            let resuming = rs.audio_paused == Some(true);
            rs.audio_playing = Some(true);
            rs.audio_paused = Some(false);
            if !resuming {
                rs.audio_play_serial = rs.audio_play_serial.wrapping_add(1);
            }
        }
    }

    pub fn audio_stop_runtime(&mut self, cell_id: u64) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            let rs = self.runtime_state.entry(cell_id).or_default();

            rs.audio_playing = Some(false);
            rs.audio_paused = Some(false);
        }
    }

    pub fn audio_pause_runtime(&mut self, cell_id: u64) {
        let cell_id = self.resolve_sound_id(cell_id);
        if self.get_effective_cell_by_id(cell_id).is_some() {
            let rs = self.runtime_state.entry(cell_id).or_default();

            rs.audio_paused = Some(true);
        }
    }

    pub fn set_cell_color_runtime(&mut self, coord: WorldCoord, color: Vec3) {
        self.bump_render_revision();
        self.mark_render_dirty(coord, DirtyReason::MaterialOrOffset);

        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            self.runtime_state.entry(id).or_default().color_rgb = Some(color);
        }
    }

    pub fn set_cell_solid_runtime(&mut self, coord: WorldCoord, solid: bool) {
        self.bump_render_revision();
        self.mark_render_dirty(coord, DirtyReason::Geometry);

        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            self.runtime_state.entry(id).or_default().solid = Some(solid);

            self.mark_physics_dirty(id);
        }

        self.update_spatial_index_at(coord);
    }

    pub fn set_cell_anchored_runtime(&mut self, coord: WorldCoord, anchored: bool) {
        self.bump_render_revision();
        self.mark_render_dirty(coord, DirtyReason::Geometry);

        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            self.runtime_state.entry(id).or_default().anchored = Some(anchored);

            self.mark_physics_dirty(id);
        }
    }

    pub fn set_visual_offset_runtime(&mut self, coord: WorldCoord, offset: Vec3) {
        self.bump_render_revision();
        self.mark_render_dirty(coord, DirtyReason::MaterialOrOffset);

        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            self.runtime_state.entry(id).or_default().visual_offset = Some(offset);

            self.mark_physics_dirty(id);
        }
    }

    pub fn create_runtime_cell(&mut self, cell_type: CellType) -> u64 {
        self.bump_render_revision();

        let mut cell = match cell_type {
            CellType::Block => Cell::new_block(),
            CellType::Light => Cell::new_light(),
            CellType::SpawnPoint => Cell::new_spawn_point(),
            CellType::AudioEmitter => Cell::new_audio_emitter(),
            CellType::ParticleEmitter => {
                Cell::new_particle_emitter(crate::engine::particles::EmitterPreset::Fire)
            }
            _ => {
                let mut c = Cell::default();
                c.cell_type = cell_type;
                c
            }
        };

        let id = self.generate_runtime_unique_id();

        cell.id = id;

        if cell.cell_type == CellType::Light {
            self.light_ids.insert(id);
            self.editor_marker_ids.insert(id);
        } else if cell.cell_type == CellType::AudioEmitter {
            self.audio_emitter_ids.insert(id);
            self.editor_marker_ids.insert(id);
        } else if cell.cell_type == CellType::ParticleEmitter {
            self.particle_emitter_ids.insert(id);
            self.editor_marker_ids.insert(id);
        }

        self.runtime_cells.insert(id, cell);
        self.mark_physics_dirty(id);

        id
    }

    pub fn move_runtime_cell(&mut self, id: u64, new_coord: WorldCoord) -> Result<(), String> {
        if !self.runtime_cells.contains_key(&id) {
            return Err("Not a runtime cell".to_string());
        }

        self.bump_render_revision();

        let old_coord_opt = self.runtime_id_to_coord.remove(&id);

        if let Some(old_coord) = old_coord_opt {
            if self.coord_to_runtime_id.get(&old_coord) == Some(&id) {
                self.coord_to_runtime_id.remove(&old_coord);
                self.restore_runtime_coord_owner(old_coord, id);
            }
            self.mark_physics_dirty(id);
            self.mark_render_dirty(old_coord, DirtyReason::Geometry);
            self.update_spatial_index_at(old_coord);
        }

        self.runtime_id_to_coord.insert(id, new_coord);
        let uses_voxel_slot = self
            .runtime_cells
            .get(&id)
            .is_some_and(|cell| Self::runtime_cell_uses_voxel_slot(cell.cell_type));
        if uses_voxel_slot {
            self.coord_to_runtime_id.insert(new_coord, id);
        }

        self.mark_physics_dirty(id);
        self.mark_render_dirty(new_coord, DirtyReason::Geometry);
        self.update_spatial_index_at(new_coord);

        Ok(())
    }

    pub fn delete_cell_runtime(&mut self, id: u64) {
        self.bump_render_revision();

        let coord_opt = self.resolve_cell_id(id);

        if let Some(coord) = coord_opt {
            self.mark_render_dirty(coord, DirtyReason::Geometry);
        }

        self.light_ids.remove(&id);
        self.editor_marker_ids.remove(&id);
        self.audio_emitter_ids.remove(&id);
        self.particle_emitter_ids.remove(&id);

        if self.runtime_cells.contains_key(&id) {
            if let Some(coord) = self.runtime_id_to_coord.remove(&id) {
                if self.coord_to_runtime_id.get(&coord) == Some(&id) {
                    self.coord_to_runtime_id.remove(&coord);
                    self.restore_runtime_coord_owner(coord, id);
                }
                self.mark_physics_dirty(id);
            }

            self.runtime_cells.remove(&id);
            self.runtime_state.remove(&id);
        } else if self.id_to_coord.contains_key(&id) || self.sources.contains_key(&id) {
            self.runtime_state.entry(id).or_default().is_deleted = true;

            self.mark_physics_dirty(id);
        }

        if let Some(coord) = coord_opt {
            self.update_spatial_index_at(coord);
        }
    }

    pub(crate) fn generate_runtime_unique_id(&self) -> u64 {
        use chrono::Local;
        use std::hash::{Hash, Hasher};

        let mut retry_count = 0;

        loop {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();

            let timestamp = Local::now().format("%Y:%m:%d:%S:%f").to_string();

            timestamp.hash(&mut hasher);
            retry_count.hash(&mut hasher);

            let hash = hasher.finish();

            let id = 100_000_000 + (hash % 900_000_000);

            if !self.runtime_cells.contains_key(&id)
                && !self.cells.values().any(|cell| cell.id == id)
                && !self.sources.contains_key(&id)
            {
                return id;
            }

            retry_count += 1;
        }
    }

    pub fn set_attribute_runtime(&mut self, coord: WorldCoord, key: String, value: AttributeValue) {
        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            self.runtime_state
                .entry(id)
                .or_default()
                .attribute_overrides
                .insert(key, value);
        }
    }

    pub fn remove_attribute_runtime(&mut self, coord: WorldCoord, key: &str) {
        let id = self.get_effective_cell(coord).map(|cell| cell.id);

        if let Some(id) = id {
            if let Some(rs) = self.runtime_state.get_mut(&id) {
                rs.attribute_overrides.remove(key);
            }
        }
    }

    pub fn clear_runtime_state(&mut self) {
        self.terrain.replace_runtime(None);
        self.particles.clear();
        self.bump_render_revision();

        for id in self.runtime_state.keys() {
            self.physics_dirty_cells.insert(*id);

            if let Some(coord) = self.id_to_coord.get(id) {
                self.mark_render_dirty(*coord, DirtyReason::Geometry);
            }
        }

        for (id, coord) in &self.runtime_id_to_coord {
            self.physics_dirty_cells.insert(*id);
            self.mark_render_dirty(*coord, DirtyReason::Geometry);
        }

        self.runtime_post_process_changes.clear();
        self.runtime_post_process_authored = None;
        self.runtime_actor_tags.clear();
        self.invalidate_actor_tag_index();
        self.runtime_entity_audio.clear();
        self.runtime_state.clear();
        self.runtime_cells.clear();
        self.runtime_id_to_coord.clear();
        self.coord_to_runtime_id.clear();

        self.rebuild_light_and_marker_indices();
        self.rebuild_spatial_index();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_overlapped_runtime_cell_preserves_other_coordinate_owner() {
        let mut world = World::new();
        let coord = WorldCoord::new(4, 5, 6);
        let first = world.create_runtime_cell(CellType::Block);
        let second = world.create_runtime_cell(CellType::Block);
        world.move_runtime_cell(first, coord).unwrap();
        world.move_runtime_cell(second, coord).unwrap();

        world.delete_cell_runtime(first);
        assert_eq!(
            world.get_effective_cell(coord).map(|cell| cell.id),
            Some(second)
        );

        let third = world.create_runtime_cell(CellType::Block);
        world.move_runtime_cell(third, coord).unwrap();
        world.delete_cell_runtime(third);
        assert_eq!(
            world.get_effective_cell(coord).map(|cell| cell.id),
            Some(second)
        );
    }

    #[test]
    fn runtime_light_and_particle_emitter_can_share_a_center() {
        let mut world = World::new();
        let coord = WorldCoord::new(8, 9, 10);
        let light = world.create_runtime_cell(CellType::Light);
        let emitter = world.create_runtime_cell(CellType::ParticleEmitter);
        world.move_runtime_cell(light, coord).unwrap();
        world.move_runtime_cell(emitter, coord).unwrap();

        assert_eq!(world.resolve_cell_id(light), Some(coord));
        assert_eq!(world.resolve_cell_id(emitter), Some(coord));
        assert_eq!(
            world.get_effective_cell_by_id(light).unwrap().cell_type,
            CellType::Light
        );
        assert_eq!(
            world.get_effective_cell_by_id(emitter).unwrap().cell_type,
            CellType::ParticleEmitter
        );
        assert!(world.light_ids.contains(&light));
        assert!(world.particle_emitter_ids.contains(&emitter));
        assert!(!world.coord_to_runtime_id.contains_key(&coord));
    }
}

#[cfg(test)]
mod emitted_light_scripting_regressions {
    use super::*;
    use crate::scripting::value::Value;

    #[test]
    fn light_numeric_and_rgb_overrides_reach_renderer_source_without_changing_authored_state() {
        let mut world = World::new();
        let id = world.create_runtime_cell(CellType::Light);
        let authored = world.get_effective_cell_by_id(id).unwrap().clone();
        world
            .set_light_property_runtime(id, "intensity", Value::Number(12.5))
            .unwrap();
        world
            .set_light_property_runtime(id, "range", Value::Number(24.0))
            .unwrap();
        world
            .set_light_property_runtime(
                id,
                "color",
                Value::array(vec![
                    Value::Number(0.25),
                    Value::Number(0.5),
                    Value::Number(0.75),
                ]),
            )
            .unwrap();
        world
            .set_light_property_runtime(id, "shadows", Value::Bool(false))
            .unwrap();
        assert_eq!(
            world.emitted_light_runtime(id),
            Some((Vec3::new(0.25, 0.5, 0.75), 12.5, 24.0, false))
        );
        assert_eq!(
            world.get_effective_cell_by_id(id).unwrap().light_intensity,
            authored.light_intensity
        );
        assert_eq!(
            world.get_effective_cell_by_id(id).unwrap().light_color,
            authored.light_color
        );
        assert!(
            world
                .set_light_property_runtime(id, "intensity", Value::Number(-2.0))
                .is_err()
        );
        assert!(
            world
                .set_light_property_runtime(
                    id,
                    "color",
                    Value::array(vec![
                        Value::Number(2.0),
                        Value::Number(0.0),
                        Value::Number(1.0)
                    ])
                )
                .is_err()
        );
        assert_eq!(world.emitted_light_runtime(id).unwrap().1, 12.5);
        world.delete_cell_runtime(id);
        assert!(world.emitted_light_runtime(id).is_none());
    }
}
