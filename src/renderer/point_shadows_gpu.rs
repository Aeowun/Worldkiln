// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::chunk::CameraFrustum;
use super::{PointLightData, Renderer, SHADOW_RES};
use crate::world::World;
use glam::{Mat4, Vec3};
use std::time::{Duration, Instant};

pub(super) const FACES_PER_LIGHT: usize = 6;
pub(super) const SHADOW_TILE_SIZE: i32 = 512;
const TILES_PER_AXIS: i32 = SHADOW_RES / SHADOW_TILE_SIZE;
const TILES_PER_LAYER: usize = (TILES_PER_AXIS * TILES_PER_AXIS) as usize;
const NEAR: f32 = 0.1;
const ALL_FACES: u8 = (1 << FACES_PER_LIGHT) - 1;

const MAX_SHADOW_FACES_PER_FRAME: usize = 1;
const SHADOW_CPU_SUBMISSION_BUDGET: Duration = Duration::from_micros(1300);

#[derive(Default)]
pub(super) struct PointShadowCache {
    entries: Vec<CachedLight>,
    cursor: usize,
}

#[derive(Clone)]
struct CachedLight {
    id: u64,
    position: Vec3,
    range: f32,
    geometry_revision: u64,
    dynamic_stamp: u64,
    ready_faces: u8,
    dirty_faces: u8,
    next_face: usize,
}
impl Default for CachedLight {
    fn default() -> Self {
        Self {
            id: 0,
            position: Vec3::ZERO,
            range: 0.0,
            geometry_revision: 0,
            dynamic_stamp: 0,
            ready_faces: 0,
            dirty_faces: ALL_FACES,
            next_face: 0,
        }
    }
}
impl PointShadowCache {
    fn invalidate_all(&mut self) {
        self.entries.clear();
        self.cursor = 0;
    }
    fn reconcile(
        &mut self,
        lights: &[PointLightData],
        geometry_revision: u64,
        mut dynamic_stamp: impl FnMut(&PointLightData) -> u64,
    ) {
        let number = lights.iter().filter(|light| light.shadow_slot >= 0).count();
        self.entries.resize_with(number, CachedLight::default);
        for light in lights.iter().filter(|light| light.shadow_slot >= 0) {
            let slot = light.shadow_slot as usize;
            let stamp = dynamic_stamp(light);
            let cache = &mut self.entries[slot];
            if cache.id != light.id {
                *cache = CachedLight {
                    id: light.id,
                    position: light.position,
                    range: light.range,
                    geometry_revision,
                    dynamic_stamp: stamp,
                    ..CachedLight::default()
                };
                continue;
            }
            let moved = cache.position != light.position || cache.range != light.range;
            let changed =
                cache.geometry_revision != geometry_revision || cache.dynamic_stamp != stamp;
            if moved || changed {
                cache.position = light.position;
                cache.range = light.range;
                cache.geometry_revision = geometry_revision;
                cache.dynamic_stamp = stamp;
                cache.dirty_faces |= ALL_FACES;
            }
        }
    }
    fn next_dirty(&mut self) -> Option<(usize, usize)> {
        let len = self.entries.len();
        if len == 0 {
            return None;
        }
        for offset in 0..len {
            let index = (self.cursor + offset) % len;
            let entry = &mut self.entries[index];
            if entry.dirty_faces == 0 {
                continue;
            }
            for step in 0..FACES_PER_LIGHT {
                let face = (entry.next_face + step) % FACES_PER_LIGHT;
                if entry.dirty_faces & (1 << face) == 0 {
                    continue;
                }
                entry.next_face = (face + 1) % FACES_PER_LIGHT;
                self.cursor = (index + 1) % len;
                return Some((index, face));
            }
        }
        None
    }
    fn mark_rendered(&mut self, slot: usize, face: usize) {
        let entry = &mut self.entries[slot];
        entry.ready_faces |= 1 << face;
        entry.dirty_faces &= !(1 << face);
    }
    fn ready(&self, slot: usize) -> bool {
        self.entries
            .get(slot)
            .is_some_and(|entry| entry.ready_faces == ALL_FACES)
    }
}

pub(super) fn face_target(face: usize) -> (Vec3, Vec3) {
    match face {
        0 => (Vec3::X, -Vec3::Y),
        1 => (-Vec3::X, -Vec3::Y),
        2 => (Vec3::Y, Vec3::Z),
        3 => (-Vec3::Y, -Vec3::Z),
        4 => (Vec3::Z, -Vec3::Y),
        _ => (-Vec3::Z, -Vec3::Y),
    }
}

pub(super) fn assign_shadow_slots(
    lights: &mut [PointLightData],
    camera: Vec3,
    camera_distance: f32,
    enabled: bool,
) -> usize {
    let mut count = 0_usize;
    for light in lights {
        light.shadow_slot = -1;
        if enabled
            && light.casts_shadows
            && camera_distance.is_finite()
            && camera_distance > 0.0
            && light.position.distance_squared(camera) <= camera_distance.powi(2)
            && light.range.is_finite()
            && light.range > NEAR + 0.01
        {
            light.shadow_slot = i32::try_from(count).expect("Point Light shadow slot overflow");
            count += 1;
        }
    }
    count
}

impl Renderer {
    pub(super) fn prepare_point_shadows(
        &self,
        world: &World,
        camera: Vec3,
        enabled: bool,
        lights: &mut [PointLightData],
    ) {
        let count = assign_shadow_slots(
            lights,
            camera,
            world.lighting.point_shadow_camera_distance,
            enabled,
        );
        if !enabled {
            return;
        }
        let tiles = count
            .checked_mul(FACES_PER_LIGHT)
            .expect("Point Light shadow tile count overflow");
        let required_layers = 1 + tiles.div_ceil(TILES_PER_LAYER);
        assert!(
            required_layers <= i32::MAX as usize,
            "Point Light shadow atlas exceeds integer range"
        );
        let mut allocated = self.point_shadow_layers.borrow_mut();
        unsafe {
            if *allocated < required_layers as i32 {
                let mut max_layers = 0_i32;
                gl::GetIntegerv(gl::MAX_ARRAY_TEXTURE_LAYERS, &mut max_layers);
                assert!(
                    required_layers <= max_layers as usize,
                    "Nearby shadow Lights exceed GPU atlas capacity; shorten the camera shadow distance"
                );
                let next_layers = required_layers.next_power_of_two().min(max_layers as usize);
                gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.shadow_depth_tex);
                gl::TexImage3D(
                    gl::TEXTURE_2D_ARRAY,
                    0,
                    gl::DEPTH_COMPONENT24 as i32,
                    SHADOW_RES,
                    SHADOW_RES,
                    next_layers as i32,
                    0,
                    gl::DEPTH_COMPONENT,
                    gl::UNSIGNED_INT,
                    std::ptr::null(),
                );
                *allocated = next_layers as i32;
                self.point_shadow_cache.borrow_mut().invalidate_all();
            }
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.shadow_fbo);
            gl::FramebufferTextureLayer(
                gl::FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                self.shadow_depth_tex,
                0,
                0,
            );
            let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);
            assert_eq!(
                status,
                gl::FRAMEBUFFER_COMPLETE,
                "Point Light shadow atlas framebuffer is incomplete: 0x{status:04X}"
            );
        }
    }

    pub(super) fn render_point_shadow_faces<S, F>(
        &self,
        world: &World,
        lights: &mut [PointLightData],
        dynamic_stamp: S,
        mut draw: F,
    ) where
        S: FnMut(&PointLightData) -> u64,
        F: FnMut(&Self, &CameraFrustum),
    {
        let geometry_revision =
            world.render_revision() ^ world.terrain.render_revision().rotate_left(29);
        let mut cache = self.point_shadow_cache.borrow_mut();
        cache.reconcile(lights, geometry_revision, dynamic_stamp);
        let eligible: Vec<(usize, usize)> = lights
            .iter()
            .enumerate()
            .filter_map(|(index, light)| {
                (light.shadow_slot >= 0).then_some((light.shadow_slot as usize, index))
            })
            .collect();
        let begin = Instant::now();
        let mut submitted = 0_usize;
        while submitted < MAX_SHADOW_FACES_PER_FRAME
            && (submitted == 0 || begin.elapsed() < SHADOW_CPU_SUBMISSION_BUDGET)
        {
            let Some((slot, face)) = cache.next_dirty() else {
                break;
            };
            let Some((_, light_index)) = eligible.iter().find(|(candidate, _)| *candidate == slot)
            else {
                continue;
            };
            let light = &lights[*light_index];
            let projection =
                Mat4::perspective_rh_gl(std::f32::consts::FRAC_PI_2, 1.0, NEAR, light.range);
            let tile = slot * FACES_PER_LIGHT + face;
            let layer = 1 + tile / TILES_PER_LAYER;
            let inside = tile % TILES_PER_LAYER;
            let x = (inside as i32 % TILES_PER_AXIS) * SHADOW_TILE_SIZE;
            let y = (inside as i32 / TILES_PER_AXIS) * SHADOW_TILE_SIZE;
            let (direction, up) = face_target(face);
            let view_projection =
                projection * Mat4::look_at_rh(light.position, light.position + direction, up);
            unsafe {
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.shadow_fbo);
                gl::UseProgram(self.shadow_program);
                gl::Enable(gl::SCISSOR_TEST);
                gl::FramebufferTextureLayer(
                    gl::FRAMEBUFFER,
                    gl::DEPTH_ATTACHMENT,
                    self.shadow_depth_tex,
                    0,
                    layer as i32,
                );
                gl::Viewport(x, y, SHADOW_TILE_SIZE, SHADOW_TILE_SIZE);
                gl::Scissor(x, y, SHADOW_TILE_SIZE, SHADOW_TILE_SIZE);
                gl::Clear(gl::DEPTH_BUFFER_BIT);
                gl::UniformMatrix4fv(
                    self.shadow_light_space_matrix_location,
                    1,
                    gl::FALSE,
                    view_projection.to_cols_array().as_ptr(),
                );
            }
            let frustum = CameraFrustum::from_view_projection(view_projection);
            draw(self, &frustum);
            cache.mark_rendered(slot, face);
            submitted += 1;
        }
        unsafe {
            gl::Disable(gl::SCISSOR_TEST);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }
        for light in lights.iter_mut().filter(|light| light.shadow_slot >= 0) {
            if !cache.ready(light.shadow_slot as usize) {
                light.shadow_slot = -1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_lights(count: usize) -> Vec<PointLightData> {
        (0..count)
            .map(|id| PointLightData {
                id: (id + 1) as u64,
                casts_shadows: true,
                shadow_slot: -1,
                position: Vec3::new(id as f32, 0.0, 0.0),
                color: Vec3::ONE,
                intensity: 1.0,
                range: 60.0,
            })
            .collect()
    }
    #[test]
    fn camera_range_gates_shadows_not_illumination_without_a_fixed_light_cap() {
        let mut lights = test_lights(40);
        assert_eq!(
            assign_shadow_slots(&mut lights, Vec3::ZERO, 100.0, true),
            40
        );
        assert_eq!(lights.last().unwrap().shadow_slot, 39);
        assert_eq!(assign_shadow_slots(&mut lights, Vec3::ZERO, 10.0, true), 11);
        assert_eq!(lights[11].shadow_slot, -1);
        lights[2].casts_shadows = false;
        assert_eq!(assign_shadow_slots(&mut lights, Vec3::ZERO, 10.0, true), 10);
        assert_eq!(
            assign_shadow_slots(&mut lights, Vec3::ZERO, 100.0, false),
            0
        );
        assert_eq!(lights.len(), 40);
    }
    #[test]
    fn shadow_cache_renders_six_faces_once_for_unchanged_scene() {
        let mut cache = PointShadowCache::default();
        let mut lights = test_lights(1);
        assign_shadow_slots(&mut lights, Vec3::ZERO, 100.0, true);
        cache.reconcile(&lights, 5, |_| 10);
        for _ in 0..6 {
            let (slot, face) = cache.next_dirty().unwrap();
            cache.mark_rendered(slot, face);
        }
        assert!(cache.ready(0));
        assert!(cache.next_dirty().is_none());
        cache.reconcile(&lights, 5, |_| 10);
        assert!(
            cache.next_dirty().is_none(),
            "static shadows must never redraw without changes"
        );
        cache.reconcile(&lights, 6, |_| 10);
        assert!(
            cache.next_dirty().is_some(),
            "world geometry change invalidates shadows"
        );
        assert!(
            cache.ready(0),
            "changed face can retain prior depth while refresh is scheduled"
        );
    }
    #[test]
    fn shadow_cache_round_robin_updates_all_lights_without_starvation() {
        let mut lights = test_lights(40);
        assign_shadow_slots(&mut lights, Vec3::ZERO, 100.0, true);
        let mut cache = PointShadowCache::default();
        cache.reconcile(&lights, 5, |_| 1);
        let mut slots = std::collections::HashSet::new();
        for _ in 0..40 {
            let (slot, face) = cache.next_dirty().unwrap();
            assert_eq!(face, 0);
            slots.insert(slot);
            cache.mark_rendered(slot, face);
        }
        assert_eq!(slots.len(), 40);
    }
    #[test]
    fn face_transforms_cover_every_positive_and_negative_axis() {
        let expected = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z];
        for (index, axis) in expected.iter().enumerate() {
            let (direction, up) = face_target(index);
            assert_eq!(direction, *axis);
            assert!(direction.dot(up).abs() < 1.0e-6);
        }
    }
    #[test]
    fn point_shadow_tile_packing_does_not_limit_scene_light_count_to_sixteen() {
        assert_eq!(17_usize * FACES_PER_LIGHT / TILES_PER_LAYER, 6);
        assert_eq!(SHADOW_RES % SHADOW_TILE_SIZE, 0);
    }
}
