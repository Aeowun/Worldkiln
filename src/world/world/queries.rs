// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::World;
use crate::world::cell::{AttributeValue, Cell, ChunkCoord};
use crate::world::coordinate::WorldCoord;
use glam::Vec3;
use std::collections::BTreeMap;

impl World {
    pub fn water_current_at(&self, world_position: Vec3) -> Vec3 {
        if !world_position.is_finite() {
            return Vec3::ZERO;
        }
        let mut strongest = Vec3::ZERO;
        for body in &self.terrain.config.water_bodies {
            if !body.visible || body.kind != crate::world::terrain::WaterBodyKind::River {
                continue;
            }
            let (min, max) = body.bounds();
            if world_position.x < min[0]
                || world_position.x > max[0]
                || world_position.z < min[1]
                || world_position.z > max[1]
            {
                continue;
            }
            let floor = self
                .terrain
                .surface_height_at(world_position.x, world_position.z)
                .unwrap_or_else(|| {
                    body.surface_y_at(world_position.x, world_position.z) - body.depth
                });
            if body.contains_volume_point(world_position.to_array(), floor) {
                let v = body.flow_at(world_position.x, world_position.z);
                let candidate = Vec3::new(v[0], 0.0, v[1]);
                if candidate.length_squared() > strongest.length_squared() {
                    strongest = candidate;
                }
            }
        }
        strongest
    }

    pub fn swimming_surface_at(&self, x: f32, z: f32, time: f32) -> Option<f32> {
        self.swimming_surface_with_appearance(x, z, time, None)
    }
    pub fn swimming_surface_with_appearance(
        &self,
        x: f32,
        z: f32,
        time: f32,
        look: Option<&crate::world::terrain::TerrainWaterSettings>,
    ) -> Option<f32> {
        if !(x.is_finite() && z.is_finite() && time.is_finite()) {
            return None;
        }
        let floor = self.terrain.surface_height_at(x, z);
        let mut surface = None::<f32>;
        for body in &self.terrain.config.water_bodies {
            if !body.visible || body.signed_coverage(x, z) <= 0.0 {
                continue;
            }
            let y = body.surface_y_at(x, z);
            let bed = floor.unwrap_or(y - body.depth);
            if bed < y {
                surface = Some(surface.map_or(y, |s| s.max(y)));
            }
        }
        if self.terrain.config.ocean_enabled {
            let level = self.terrain.config.plains.water_level;
            let tide = crate::world::terrain::ocean_tide_offset(time);
            if floor.is_some_and(|bed| bed < level + tide) {
                let appearance = look.unwrap_or(&self.terrain.config.water);
                let depth = (level + tide - floor.unwrap_or(level)).max(0.0);
                let smooth = |a: f32, b: f32, v: f32| {
                    let p = ((v - a) / (b - a)).clamp(0.0, 1.0);
                    p * p * (3.0 - 2.0 * p)
                };
                let shoreline_fade = smooth(0.0, 0.20, smooth(0.0, 16.0, depth));
                let wave = crate::world::terrain::ocean_height_with_appearance(
                    x,
                    z,
                    time,
                    &self.terrain.config,
                    appearance,
                );
                let y = level + tide + wave * shoreline_fade;
                surface = Some(surface.map_or(y, |s| s.max(y)));
            }
        }
        surface
    }

    pub fn get(&self, coord: WorldCoord) -> Option<&Cell> {
        self.cells.get(&coord)
    }

    pub fn iter_authored_cells(&self) -> impl Iterator<Item = (WorldCoord, &Cell)> {
        self.cells.iter().map(|(coord, cell)| (*coord, cell))
    }

    pub fn source(&self, id: u64) -> Option<&crate::world::AuthoredSource> {
        self.sources.get(&id)
    }

    pub fn source_mut(&mut self, id: u64) -> Option<&mut crate::world::AuthoredSource> {
        self.sources.get_mut(&id)
    }

    pub fn iter_sources(&self) -> impl Iterator<Item = &crate::world::AuthoredSource> {
        self.sources.values()
    }

    pub fn light_world_position(&self, id: u64) -> Option<Vec3> {
        if self.get_effective_cell_by_id(id)?.cell_type != crate::world::CellType::Light {
            return None;
        }
        if let Some(position) = self
            .runtime_state
            .get(&id)
            .and_then(|state| state.source_position)
        {
            return Some(position);
        }
        if let Some(source) = self.sources.get(&id) {
            return Some(source.position);
        }
        self.resolve_cell_id(id).map(|coord| {
            Vec3::new(
                coord.x as f32 + 0.5,
                coord.y as f32 + 0.5,
                coord.z as f32 + 0.5,
            )
        })
    }

    pub fn source_position(&self, id: u64) -> Option<Vec3> {
        self.sources.get(&id).map(|source| {
            self.runtime_state
                .get(&id)
                .and_then(|state| state.source_position)
                .unwrap_or(source.position)
        })
    }

    pub fn resident_authored_cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn authored_cell_count(&self, cell_type: crate::world::CellType) -> usize {
        self.authored_cell_type_counts
            .get(&cell_type)
            .copied()
            .unwrap_or(0)
    }

    pub fn iter_editor_navigator_coords(
        &self,
        cell_type: crate::world::CellType,
    ) -> impl Iterator<Item = WorldCoord> + '_ {
        self.editor_navigator_coords
            .get(&cell_type)
            .into_iter()
            .flat_map(|coords| coords.iter().copied())
    }

    pub fn resolve_cell_id(&self, id: u64) -> Option<WorldCoord> {
        if let Some(coord) = self.id_to_coord.get(&id) {
            if let Some(rs) = self.runtime_state.get(&id) {
                if rs.is_deleted {
                    return None;
                }
            }
            return Some(*coord);
        }
        self.runtime_id_to_coord.get(&id).cloned()
    }

    pub fn is_light_enabled(&self, coord: WorldCoord) -> bool {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(enabled) = rs.light_enabled {
                    return enabled;
                }
            }
            return cell.light_enabled;
        }
        false
    }

    pub fn is_light_enabled_by_id(&self, id: u64) -> bool {
        let Some(cell) = self.get_effective_cell_by_id(id) else {
            return false;
        };
        self.runtime_state
            .get(&id)
            .and_then(|state| state.light_enabled)
            .unwrap_or(cell.light_enabled)
    }

    pub fn is_cell_visible(&self, coord: WorldCoord) -> bool {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(visible) = rs.visible {
                    return visible;
                }
            }
            return cell.visible;
        }
        false
    }

    pub fn is_audio_playing(&self, cell_id: u64) -> bool {
        let cell_id = self.resolve_sound_id(cell_id);
        if let Some(cell) = self.get_effective_cell_by_id(cell_id) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(playing) = rs.audio_playing {
                    return playing;
                }
            }
            return cell.playing;
        }
        false
    }

    pub fn is_audio_paused(&self, cell_id: u64) -> bool {
        let cell_id = self.resolve_sound_id(cell_id);
        if let Some(cell) = self.get_effective_cell_by_id(cell_id) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(paused) = rs.audio_paused {
                    return paused;
                }
            }
        }
        false
    }

    pub fn is_audio_looped(&self, cell_id: u64) -> bool {
        let cell_id = self.resolve_sound_id(cell_id);
        if let Some(cell) = self.get_effective_cell_by_id(cell_id) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(looped) = rs.audio_looped {
                    return looped;
                }
            }
            return cell.looped;
        }
        false
    }

    pub fn get_audio_loop_interval(&self, cell_id: u64) -> f32 {
        let cell_id = self.resolve_sound_id(cell_id);
        self.get_effective_cell_by_id(cell_id)
            .map(|cell| {
                if cell.loop_interval.is_finite() {
                    cell.loop_interval.max(0.0)
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0)
    }

    pub fn get_audio_volume(&self, cell_id: u64) -> f32 {
        let cell_id = self.resolve_sound_id(cell_id);
        if let Some(cell) = self.get_effective_cell_by_id(cell_id) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(vol) = rs.audio_volume {
                    return vol;
                }
            }
            return cell.volume;
        }
        1.0
    }

    pub fn get_audio_pitch(&self, cell_id: u64) -> f32 {
        let id = self.resolve_sound_id(cell_id);
        self.runtime_state
            .get(&id)
            .and_then(|s| s.audio_pitch)
            .unwrap_or(1.0)
    }

    pub fn get_audio_play_serial(&self, cell_id: u64) -> u64 {
        let id = self.resolve_sound_id(cell_id);
        self.runtime_state
            .get(&id)
            .map(|s| s.audio_play_serial)
            .unwrap_or(0)
    }

    pub fn get_audio_min_distance(&self, cell_id: u64) -> f32 {
        let cell_id = self.resolve_sound_id(cell_id);
        let Some(cell) = self.get_effective_cell_by_id(cell_id) else {
            return crate::world::cell::DEFAULT_AUDIO_MIN_DISTANCE;
        };
        let value = self
            .runtime_state
            .get(&cell.id)
            .and_then(|state| state.audio_min_distance)
            .unwrap_or(cell.audio_min_distance);
        if value.is_finite() {
            value.max(0.0)
        } else {
            crate::world::cell::DEFAULT_AUDIO_MIN_DISTANCE
        }
    }

    pub fn get_audio_max_distance(&self, cell_id: u64) -> f32 {
        let cell_id = self.resolve_sound_id(cell_id);
        let Some(cell) = self.get_effective_cell_by_id(cell_id) else {
            return crate::world::cell::DEFAULT_AUDIO_MAX_DISTANCE;
        };
        let value = self
            .runtime_state
            .get(&cell.id)
            .and_then(|state| state.audio_max_distance)
            .unwrap_or(cell.audio_max_distance);
        let min_distance = self.get_audio_min_distance(cell_id);
        if value.is_finite() {
            value.max(min_distance)
        } else {
            crate::world::cell::DEFAULT_AUDIO_MAX_DISTANCE.max(min_distance)
        }
    }

    pub fn get_audio_path(&self, cell_id: u64) -> String {
        let cell_id = self.resolve_sound_id(cell_id);
        if let Some(cell) = self.get_effective_cell_by_id(cell_id) {
            if let Some(path) = self
                .runtime_state
                .get(&cell_id)
                .and_then(|r| r.audio_path.as_ref())
            {
                return path.clone();
            }
            return cell.audio.clone();
        }
        String::new()
    }

    pub fn get_effective_color(&self, coord: WorldCoord) -> Vec3 {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(color) = rs.color_rgb {
                    return color;
                }
            }
            return cell.color_rgb;
        }
        Vec3::ZERO
    }

    pub fn is_cell_solid(&self, coord: WorldCoord) -> bool {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(solid) = rs.solid {
                    return solid;
                }
            }
            return cell.solid;
        }
        false
    }

    pub fn is_cell_anchored(&self, coord: WorldCoord) -> bool {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(anchored) = rs.anchored {
                    return anchored;
                }
            }
            return cell.anchored;
        }
        false
    }

    pub fn get_visual_offset(&self, coord: WorldCoord) -> Vec3 {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(offset) = rs.visual_offset {
                    return offset;
                }
            }
        }
        Vec3::ZERO
    }

    pub fn get_effective_attributes(&self, coord: WorldCoord) -> BTreeMap<String, AttributeValue> {
        let mut result = BTreeMap::new();
        if let Some(cell) = self.cells.get(&coord) {
            result.extend(cell.attributes.clone());
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                result.extend(rs.attribute_overrides.clone());
            }
        }
        result
    }

    pub fn get_effective_attribute(&self, coord: WorldCoord, key: &str) -> Option<AttributeValue> {
        if let Some(cell) = self.get_effective_cell(coord) {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if let Some(val) = rs.attribute_overrides.get(key) {
                    return Some(val.clone());
                }
            }
            return cell.attributes.get(key).cloned();
        }
        None
    }

    pub fn get_effective_cell(&self, coord: WorldCoord) -> Option<&Cell> {
        if !self.coord_to_runtime_id.is_empty() {
            if let Some(id) = self.coord_to_runtime_id.get(&coord) {
                return self.runtime_cells.get(id);
            }
        }
        if let Some(cell) = self.cells.get(&coord) {
            if !self.runtime_state.is_empty() {
                if let Some(rs) = self.runtime_state.get(&cell.id) {
                    if rs.is_deleted {
                        return None;
                    }
                }
            }
            return Some(cell);
        }
        None
    }

    pub fn get_effective_cell_by_id(&self, id: u64) -> Option<&Cell> {
        if let Some(source) = self.sources.get(&id) {
            if self
                .runtime_state
                .get(&id)
                .is_some_and(|state| state.is_deleted)
            {
                return None;
            }
            return Some(&source.cell);
        }
        if !self.runtime_cells.is_empty() {
            if let Some(cell) = self.runtime_cells.get(&id) {
                return Some(cell);
            }
        }
        if let Some(coord) = self.id_to_coord.get(&id) {
            if !self.runtime_state.is_empty() {
                if let Some(rs) = self.runtime_state.get(&id) {
                    if rs.is_deleted {
                        return None;
                    }
                }
            }
            return self.cells.get(coord);
        }
        None
    }

    pub fn active_effective_blocks(&self) -> Vec<WorldCoord> {
        let mut coords: std::collections::HashSet<WorldCoord> = std::collections::HashSet::new();
        for (coord, cell) in &self.cells {
            if let Some(rs) = self.runtime_state.get(&cell.id) {
                if rs.is_deleted {
                    continue;
                }
            }
            coords.insert(*coord);
        }
        coords.extend(self.runtime_id_to_coord.values().cloned());
        coords.into_iter().collect()
    }

    pub fn iter_active_effective_coords(&self) -> impl Iterator<Item = WorldCoord> + '_ {
        self.cells
            .iter()
            .filter_map(|(coord, cell)| {
                (!self
                    .runtime_state
                    .get(&cell.id)
                    .is_some_and(|state| state.is_deleted))
                .then_some(*coord)
            })
            .chain(self.runtime_id_to_coord.values().copied())
    }

    pub fn iter_audio_emitter_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.audio_emitter_ids.iter().copied()
    }

    pub fn iter_light_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.light_ids.iter().copied().filter(move |&id| {
            if let Some(cell) = self.get_effective_cell_by_id(id) {
                cell.cell_type == crate::world::CellType::Light
            } else {
                false
            }
        })
    }

    pub fn iter_editor_marker_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.editor_marker_ids.iter().copied()
    }

    pub fn active_blocks(&self) -> Vec<WorldCoord> {
        self.cells.keys().cloned().collect()
    }

    pub fn authored_cells_in_chunk(&self, chunk_coord: ChunkCoord) -> Vec<(WorldCoord, Cell)> {
        self.cells
            .iter()
            .filter(|(coord, _)| ChunkCoord::from_world_coord(**coord) == chunk_coord)
            .map(|(coord, cell)| (*coord, cell.clone()))
            .collect()
    }
}

#[cfg(test)]
mod water_current_query_tests {
    use super::*;
    use crate::world::terrain::{WaterBody, WaterBodyKind};

    #[test]
    fn runtime_river_current_respects_volume_banks_floor_and_visibility() {
        let mut world = World::new();
        let mut river = WaterBody::new(7, WaterBodyKind::River, [0.0, 0.0], 10.0);
        for point in &mut river.points {
            point.flow_speed = 4.0;
        }
        world.terrain.config.water_bodies.push(river);
        let in_water = world.water_current_at(Vec3::new(0.0, 9.0, 0.0));
        assert!((in_water.x - 4.0).abs() < 0.001);
        assert!(in_water.z.abs() < 0.001);
        assert_eq!(
            world.water_current_at(Vec3::new(0.0, 11.0, 0.0)),
            Vec3::ZERO
        );
        assert_eq!(
            world.water_current_at(Vec3::new(0.0, -25.0, 0.0)),
            Vec3::ZERO
        );
        assert_eq!(
            world.water_current_at(Vec3::new(0.0, 9.0, 20.0)),
            Vec3::ZERO
        );
        world.terrain.config.water_bodies[0].visible = false;
        assert_eq!(world.water_current_at(Vec3::new(0.0, 9.0, 0.0)), Vec3::ZERO);
    }

    #[test]
    fn lakes_and_invalid_coordinates_do_not_apply_river_current() {
        let mut world = World::new();
        world.terrain.config.water_bodies.push(WaterBody::new(
            8,
            WaterBodyKind::Lake,
            [0.0, 0.0],
            10.0,
        ));
        assert_eq!(world.water_current_at(Vec3::new(0.0, 9.0, 0.0)), Vec3::ZERO);
        assert_eq!(
            world.water_current_at(Vec3::new(f32::NAN, 9.0, 0.0)),
            Vec3::ZERO
        );
    }
}
