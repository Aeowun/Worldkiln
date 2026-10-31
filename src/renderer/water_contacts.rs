// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{Renderer, WaterWake, WaterWakeState};
use crate::world::{World, WorldCoord};
use glam::{Vec2, Vec3, Vec4};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
pub(super) struct WaterContact {
    pub bounds: Vec4,
    pub level: f32,
    pub bottom: f32,
    pub top: f32,
    pub last_wet: f32,
    pub surface_contact: bool,
}
fn floor_sample(
    world: &World,
    x: f32,
    z: f32,
    cache: &mut HashMap<(u32, u32), Option<f32>>,
) -> Option<f32> {
    if cache.len() > 8192 {
        cache.clear();
    }
    *cache
        .entry((x.to_bits(), z.to_bits()))
        .or_insert_with(|| world.terrain.surface_height_at(x, z))
}
fn wave_height(
    world: &World,
    x: f32,
    z: f32,
    time: f32,
    look: &crate::world::terrain::TerrainWaterSettings,
    cache: &mut HashMap<(u32, u32), Option<f32>>,
) -> Option<(f32, f32)> {
    for body in &world.terrain.config.water_bodies {
        if !body.visible || body.signed_coverage(x, z) <= 0.0 {
            continue;
        }
        let surface = body.surface_y_at(x, z);
        let sampled = floor_sample(world, x, z, cache);
        let floor = sampled.unwrap_or(surface - body.depth);
        if body.vertical_bounds_at(x, z, floor).is_some() {
            return Some((surface, sampled.unwrap_or(surface - 30.0)));
        }
    }
    if !world.terrain.config.ocean_enabled {
        return None;
    }
    let base =
        world.terrain.config.plains.water_level + crate::world::terrain::ocean_tide_offset(time);
    let floor = floor_sample(world, x, z, cache)?;
    if floor >= base + look.wave_height.max(0.0) {
        return None;
    }
    let angle = look.wave_direction_degrees.to_radians();
    let k = std::f32::consts::TAU / look.wave_length.max(1.0);
    let seed = world.terrain.config.plains.seed.rem_euclid(65536) as f32;
    let hash = |v: f32| {
        (v.mul_add(17.197, 13.123).sin() * 43758.5453)
            .fract()
            .rem_euclid(1.0)
    };
    let secondary = hash(seed + 0.123);
    let angle2 = angle + 0.43 + secondary * 0.47;
    let angle3 = angle - 0.91 + hash(seed + 7.0) * 0.39;
    let p1 = -(x * angle.cos() + z * angle.sin()) * k + time * look.wave_speed;
    let p2 = -(x * angle2.cos() + z * angle2.sin()) * k * 1.71
        + time * look.wave_speed * 1.18
        + secondary * std::f32::consts::TAU;
    let p3 = -(x * angle3.cos() + z * angle3.sin()) * k * 0.58
        + time * look.wave_speed * 0.74
        + hash(seed + 5.0) * std::f32::consts::TAU;
    let height = p1.sin() * 0.72 + p2.sin() * 0.19 + p3.sin() * 0.09;
    if floor > base {
        if !super::terrain::ADVANCED_SHORELINE_ENABLED {
            return None;
        }
        let visible = shore_swash_alpha(floor - base, look.wave_height, height);
        return (visible >= 0.018).then_some((floor + 0.045, floor));
    }
    let depth = (base + look.wave_height.max(0.0) - floor).max(0.0);
    let normalized_depth = smoothstep(0.0, 16.0, depth);
    let nearshore = smoothstep(0.0, 0.20, normalized_depth);
    Some((base + height * look.wave_height * nearshore, floor))
}
fn shore_swash_alpha(elevation: f32, wave_height: f32, swell: f32) -> f32 {
    let reach = (wave_height.max(0.0) * 0.62).min(5.0);
    if reach <= 0.01 || elevation > reach + 0.2 {
        return 0.0;
    }
    let front = reach * (0.5 + 0.5 * swell).clamp(0.0, 1.0);
    (1.0 - smoothstep(front - 0.22, front + 0.14, elevation)) * smoothstep(0.0, 0.12, wave_height)
}
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
const CELL_CONTACT_BUDGET: usize = 128;
fn retain_nearest_cells(candidates: &mut Vec<(WorldCoord, Vec3)>, camera: Vec3) {
    if candidates.len() <= CELL_CONTACT_BUDGET {
        return;
    }
    let distance = |p: Vec3| Vec2::new(p.x - camera.x, p.z - camera.z).length_squared();
    candidates.select_nth_unstable_by(CELL_CONTACT_BUDGET, |a, b| {
        distance(a.1).total_cmp(&distance(b.1))
    });
    candidates.truncate(CELL_CONTACT_BUDGET);
}
impl WaterWakeState {
    pub(super) fn contact_collider(
        &mut self,
        id: u64,
        min: Vec3,
        max: Vec3,
        world: &World,
        time: f32,
        look: &crate::world::terrain::TerrainWaterSettings,
    ) {
        if !min.is_finite() || !max.is_finite() || min.cmpgt(max).any() {
            return;
        }
        let center = (min + max) * 0.5;
        let xz = Vec2::new(center.x, center.z);
        let half = Vec2::new((max.x - min.x) * 0.5, (max.z - min.z) * 0.5);
        let mut level = None;
        let mut crossing = false;
        for (sx, sz) in [
            (0.0, 0.0),
            (-1.0, -1.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (1.0, 1.0),
            (-1.0, 0.0),
            (1.0, 0.0),
            (0.0, -1.0),
            (0.0, 1.0),
        ] {
            let x = xz.x + sx * half.x;
            let z = xz.y + sz * half.y;
            if let Some((y, floor)) = wave_height(world, x, z, time, look, &mut self.floor_samples)
            {
                if min.y <= y + 0.10 && max.y >= floor {
                    level = Some(y);
                    crossing = max.y >= y - 0.10;
                    if crossing {
                        break;
                    }
                }
            }
        }
        if let Some(surface) = level {
            let existing = self.contacts.get(&id).copied();
            self.contacts.insert(
                id,
                WaterContact {
                    bounds: Vec4::new(min.x, min.z, max.x, max.z),
                    level: surface,
                    bottom: min.y,
                    top: max.y,
                    last_wet: time,
                    surface_contact: crossing,
                },
            );
            if let Some((last, when)) = self.previous.get(&id).copied() {
                let delta = center - last;
                let dt = time - when;
                let horizontal = Vec2::new(delta.x, delta.z);
                if dt > 0.07 && dt < 2.0 && (horizontal.length() > 0.14 || delta.y.abs() > 0.3) {
                    self.trails.push(WaterWake {
                        start: Vec2::new(last.x, last.z),
                        end: xz,
                        born: time,
                        power: (delta.length() / dt / 5.0).clamp(0.2, 1.0),
                    });
                }
            } else if existing.is_none() {
            }
        } else if let Some(contact) = self.contacts.get_mut(&id) {
            contact.bounds = Vec4::new(min.x, min.z, max.x, max.z);
            contact.bottom = min.y;
            contact.top = max.y;
        }
        if self.previous.get(&id).is_none_or(|(old, t)| {
            time - *t > 1.5 || (time - *t > 0.08 && (center - *old).length() > 0.12)
        }) {
            self.previous.insert(id, (center, time));
        }
    }
    pub(super) fn finish_contacts(&mut self, time: f32, seen: &HashSet<u64>) {
        self.contacts
            .retain(|id, c| seen.contains(id) && time - c.last_wet < 12.0);
        self.previous.retain(|id, _| seen.contains(id));
        self.trails.retain(|w| time - w.born < 4.0);
        if self.trails.len() > 24 {
            self.trails.drain(..self.trails.len() - 24);
        }
        if self.contacts.len() > 256 {
            let mut keys = self
                .contacts
                .iter()
                .map(|(k, v)| (*k, v.last_wet))
                .collect::<Vec<_>>();
            keys.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            let keep = keys
                .iter()
                .take(256)
                .map(|(k, _)| *k)
                .collect::<HashSet<_>>();
            self.contacts.retain(|id, _| keep.contains(id));
        }
    }
}
impl Renderer {
    pub(super) fn refresh_water_contacts(
        &self,
        world: &World,
        camera: Vec3,
        entities: &crate::engine::entity::EntityManager,
        characters: &crate::character::CharacterSystem,
    ) {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(31, file!(), line!());

        let time = self.water_started.elapsed().as_secs_f32();
        let look = world
            .terrain
            .config
            .resolved_water(&self.materials.borrow());
        let mut state = self.water_wakes.borrow_mut();
        state.contact_camera = camera;
        let revision = world.terrain.render_revision();
        if state.floor_revision != revision {
            state.floor_samples.clear();
            state.floor_revision = revision;
        }
        let mut seen = HashSet::new();
        let runtime_ids = entities
            .iter()
            .map(|entity| entity.id.0)
            .collect::<HashSet<_>>();
        let near = |position: Vec3| position.distance_squared(camera) < (110.0 * 110.0);
        for actor in world.actors.values() {
            if runtime_ids.contains(&actor.id)
                || !actor.visible
                || !actor.enabled
                || !actor.physics.enabled
                || !actor.physics.collision_enabled
                || !actor.physics.solid
            {
                continue;
            }
            let center = actor.position + actor.physics.collision_offset;
            if !near(center) {
                continue;
            }
            let extent = actor.physics.collision_size * actor.scale.abs() * 0.5;
            let id = actor.id;
            seen.insert(id);
            state.contact_collider(id, center - extent, center + extent, world, time, &look);
        }
        let mut low = f32::INFINITY;
        let mut high = f32::NEG_INFINITY;
        if world.terrain.config.ocean_enabled {
            let level = world.terrain.config.plains.water_level
                + crate::world::terrain::ocean_tide_offset(time);
            low = low.min(level - look.wave_height.abs() - 2.0);
            high = high.max(level + look.wave_height.abs() + 2.0);
        }
        for body in &world.terrain.config.water_bodies {
            if !body.visible {
                continue;
            }
            low = low.min(body.elevation - body.depth - 2.0);
            high = high.max(body.maximum_surface_y() + 2.0);
        }
        if low <= high {
            let min = WorldCoord::from_vec3(Vec3::new(camera.x - 70.0, low, camera.z - 70.0));
            let max = WorldCoord::from_vec3(Vec3::new(camera.x + 70.0, high, camera.z + 70.0));
            let mut candidates = world.query_solid_coords_in_aabb(min, max);
            retain_nearest_cells(&mut candidates, camera);
            for (coord, position) in candidates {
                let Some(cell) = world.get_effective_cell(coord) else {
                    continue;
                };
                let id = cell.id | (1u64 << 61);
                seen.insert(id);
                state.contact_collider(id, position, position + Vec3::ONE, world, time, &look);
            }
        }
        for character in characters.get_active_characters() {
            let p = character.transform.position;
            if !near(p) {
                continue;
            }
            let r = character.collision.radius.max(0.01);
            let h = character.collision.height.max(0.01);
            let id = character.id | (1u64 << 63);
            seen.insert(id);
            state.contact_collider(
                id,
                p - Vec3::new(r, 0.0, r),
                p + Vec3::new(r, h, r),
                world,
                time,
                &look,
            );
        }
        for entity in entities.iter() {
            if !entity.visible
                || !entity.enabled
                || !entity.physics.enabled
                || !entity.physics.solid
                || !entity.physics.collision_enabled
                || !near(entity.position)
            {
                continue;
            }
            let center = entity.position + entity.physics.offset;
            let extent = entity.physics.size * entity.scale.abs() * 0.5;
            let id = entity.id.0 | (1u64 << 62);
            seen.insert(id);
            state.contact_collider(id, center - extent, center + extent, world, time, &look);
        }
        state.finish_contacts(time, &seen);
    }
    pub(super) unsafe fn bind_water_contact_uniforms(&self, program: u32) {
        let state = self.water_wakes.borrow();
        let now = self.water_started.elapsed().as_secs_f32();
        let mut contacts = state.contacts.values().copied().collect::<Vec<_>>();
        let near = |c: &WaterContact| {
            let middle = Vec2::new(
                (c.bounds.x + c.bounds.z) * 0.5,
                (c.bounds.y + c.bounds.w) * 0.5,
            );
            middle.distance_squared(Vec2::new(state.contact_camera.x, state.contact_camera.z))
        };
        contacts.sort_by(|a, b| {
            near(a)
                .total_cmp(&near(b))
                .then_with(|| b.last_wet.total_cmp(&a.last_wet))
        });
        let n = contacts.len().min(8);
        let mut bounds = [0.0f32; 96];
        let mut props = [0.0f32; 96];
        for (i, c) in contacts.iter().take(n).enumerate() {
            bounds[i * 4..i * 4 + 4].copy_from_slice(&c.bounds.to_array());
            props[i * 4..i * 4 + 4].copy_from_slice(&[
                c.level,
                c.bottom,
                c.top,
                if c.surface_contact {
                    (now - c.last_wet).max(0.0)
                } else {
                    -(now - c.last_wet).max(0.0) - 0.001
                },
            ]);
        }
        unsafe {
            let loc = |name| super::resources::get_uniform_location(program, name);
            gl::Uniform1i(loc("u_contact_count"), n as i32);
            if n > 0 {
                gl::Uniform4fv(loc("u_contact_bounds[0]"), n as i32, bounds.as_ptr());
                gl::Uniform4fv(loc("u_contact_props[0]"), n as i32, props.as_ptr());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wake_path_samples_preserve_turns_without_stamping_new_vs() {
        let mut state = WaterWakeState::default();
        state.record(7, Vec3::new(0.0, 0.0, 0.0), 0.0, 0.0);
        state.record(7, Vec3::new(1.0, 0.0, 0.0), 0.25, 0.0);
        state.record(7, Vec3::new(1.0, 0.0, 1.0), 0.50, 0.0);
        assert_eq!(state.trails.len(), 2);
        let a = state.trails[0];
        let b = state.trails[1];
        assert!((a.end - b.start).length() < 0.0001);
        assert_eq!(a.start, Vec2::ZERO);
        assert_eq!(b.end, Vec2::new(1.0, 1.0));
    }
    #[test]
    fn nearest_cell_budget_preserves_close_pillars_in_dense_shoreline() {
        let mut cells = (0..500)
            .map(|n| (WorldCoord::new(n, 0, 0), Vec3::new(n as f32, 0.0, 0.0)))
            .collect::<Vec<_>>();
        retain_nearest_cells(&mut cells, Vec3::new(495.0, 0.0, 0.0));
        assert_eq!(cells.len(), CELL_CONTACT_BUDGET);
        assert!(cells.iter().any(|(coord, _)| coord.x == 495));
        assert!(!cells.iter().any(|(coord, _)| coord.x == 0));
    }
    #[test]
    fn ocean_shoreline_taper_fades_wave_before_clipped_water_edge() {
        assert_eq!(smoothstep(0.0, 0.20, smoothstep(0.0, 16.0, 0.0)), 0.0);
        let shallow = smoothstep(0.0, 0.20, smoothstep(0.0, 16.0, 1.0));
        let deeper = smoothstep(0.0, 0.20, smoothstep(0.0, 16.0, 6.0));
        assert!(shallow >= 0.0 && shallow < deeper && deeper > 0.99);
        let water = super::super::terrain::WATER_VERTEX_SHADER;
        assert!(water.contains("ocean_wave_field(world.xz,u_time)*u_wave_height*shoreline_fade"));
        assert!(water.contains("sideways*shoreline_fade"));
    }
    #[test]
    fn beach_swatch_follows_wave_front_and_disappears_in_calm() {
        assert_eq!(shore_swash_alpha(0.3, 0.0, 1.0), 0.0);
        assert_eq!(shore_swash_alpha(3.0, 1.0, -1.0), 0.0);
        let wave = shore_swash_alpha(0.3, 1.0, 1.0);
        assert!(wave > 0.8);
        assert!(shore_swash_alpha(0.3, 1.0, -1.0) < wave);
        assert_eq!(shore_swash_alpha(6.0, 10.0, 1.0), 0.0);
    }
    #[test]
    fn collider_crossing_waterline_produces_contact_but_dry_collider_does_not() {
        let mut world = World::new();
        world.terrain.config.ocean_enabled = false;
        let lake = crate::world::terrain::WaterBody::new(
            9,
            crate::world::terrain::WaterBodyKind::Lake,
            [0.0, 0.0],
            12.0,
        );
        let elevation = lake.surface_y_at(0.0, 0.0);
        world.terrain.config.water_bodies.push(lake);
        let look = world.terrain.config.water;
        let mut wakes = WaterWakeState::default();
        wakes.contact_collider(
            1,
            Vec3::new(-0.5, elevation - 2.0, -0.5),
            Vec3::new(0.5, elevation + 3.0, 0.5),
            &world,
            0.0,
            &look,
        );
        assert!(wakes.contacts.contains_key(&1));
        wakes.contact_collider(
            2,
            Vec3::new(-0.5, elevation + 5.0, -0.5),
            Vec3::new(0.5, elevation + 7.0, 0.5),
            &world,
            0.0,
            &look,
        );
        assert!(!wakes.contacts.contains_key(&2));
        assert!(wakes.trails.is_empty());
    }
}
