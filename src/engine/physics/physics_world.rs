// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::anchored_surface::{AnchoredCollider, AnchoredFace, build_anchored_colliders};
use super::physics_body::{CollisionRecord, PhysicsBody, PhysicsBodyId, PhysicsIdGenerator};
use crate::world::{CellType, ChunkCoord, World, WorldCoord};
use glam::Vec3;
use std::collections::{HashMap, HashSet};

pub(crate) const DYNAMIC_BROADPHASE_CELL_SIZE: f32 = 2.0;

pub struct PhysicsWorld {
    pub bodies: Vec<PhysicsBody>,

    pub static_colliders: Vec<(u64, Vec3)>,
    pub static_colliders_by_chunk: HashMap<ChunkCoord, Vec<(u64, Vec3)>>,
    pub(super) static_friction: HashMap<u64, f32>,
    anchored_collision: Vec<AnchoredCollider>,
    #[cfg(test)]
    pub(crate) static_index_rebuild_count: u64,

    pub(crate) dynamic_bodies_by_cell: HashMap<(i32, i32, i32), Vec<usize>>,

    pub(crate) id_gen: PhysicsIdGenerator,
    pub(crate) rapier_runtime: Option<super::rapier_backend::RigidSolver>,
    pub(crate) rapier_published: HashMap<u64, super::rapier_backend::BodyState>,
    pub(crate) rapier_terrain_patches: HashMap<(i32, i32), rapier3d::prelude::ColliderHandle>,

    pub(crate) step_count: u64,

    native_terrain_collider: Option<super::triangle_collider::TriangleCollider>,
    pub(super) terrain_tiles:
        std::cell::RefCell<HashMap<[u32; 2], super::terrain_collision_cache::CachedTerrainTile>>,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            bodies: Vec::new(),
            static_colliders: Vec::new(),
            static_colliders_by_chunk: HashMap::new(),
            static_friction: HashMap::new(),
            anchored_collision: Vec::new(),
            #[cfg(test)]
            static_index_rebuild_count: 0,
            dynamic_bodies_by_cell: HashMap::new(),
            id_gen: PhysicsIdGenerator::new(),
            rapier_runtime: None,
            rapier_published: HashMap::new(),
            rapier_terrain_patches: HashMap::new(),
            step_count: 0,
            native_terrain_collider: None,
            terrain_tiles: std::cell::RefCell::new(HashMap::new()),
        }
    }

    pub fn set_native_terrain_mesh(&mut self, mesh: Option<&crate::landscape::IndexedMesh>) {
        self.native_terrain_collider = mesh.map(super::triangle_collider::TriangleCollider::new);
        self.rapier_runtime = None;
        self.rapier_published.clear();
        self.rapier_terrain_patches.clear();
    }

    pub fn native_terrain_collider(&self) -> Option<&super::triangle_collider::TriangleCollider> {
        self.native_terrain_collider.as_ref()
    }

    pub fn rebuild_static_chunk_index(&mut self) {
        #[cfg(test)]
        {
            self.static_index_rebuild_count += 1;
        }
        self.anchored_collision = build_anchored_colliders(&self.static_colliders);
        self.rapier_runtime = None;
        self.rapier_published.clear();
        self.rapier_terrain_patches.clear();
        self.static_colliders_by_chunk.clear();

        for &(cell_id, pos) in &self.static_colliders {
            let coord = WorldCoord::from_vec3(pos);
            let chunk_coord = ChunkCoord::from_world_coord(coord);

            self.static_colliders_by_chunk
                .entry(chunk_coord)
                .or_default()
                .push((cell_id, pos));
        }
    }

    pub fn query_static_colliders_in_aabb(&self, min_pos: Vec3, max_pos: Vec3) -> Vec<(u64, Vec3)> {
        let _query = crate::engine::development_capture::operation(
            "physics",
            "query_static_colliders_in_aabb",
            true,
        );
        let min_coord = WorldCoord::from_vec3(min_pos);
        let max_coord = WorldCoord::from_vec3(max_pos);

        let min_chunk = ChunkCoord::from_world_coord(min_coord);
        let max_chunk = ChunkCoord::from_world_coord(max_coord);

        let mut results = Vec::new();

        for cx in min_chunk.x..=max_chunk.x {
            for cy in min_chunk.y..=max_chunk.y {
                for cz in min_chunk.z..=max_chunk.z {
                    let chunk_coord = ChunkCoord::new(cx, cy, cz);

                    if let Some(list) = self.static_colliders_by_chunk.get(&chunk_coord) {
                        for &(id, pos) in list {
                            if pos.x >= min_pos.x - 1.0
                                && pos.x <= max_pos.x + 1.0
                                && pos.y >= min_pos.y - 1.0
                                && pos.y <= max_pos.y + 1.0
                                && pos.z >= min_pos.z - 1.0
                                && pos.z <= max_pos.z + 1.0
                            {
                                results.push((id, pos));
                            }
                        }
                    }
                }
            }
        }

        results
    }

    pub fn anchored_faces_in_aabb(&self, min: Vec3, max: Vec3) -> Vec<AnchoredFace> {
        self.anchored_collision
            .iter()
            .filter(|group| {
                group.min.x <= max.x
                    && group.max.x >= min.x
                    && group.min.y <= max.y
                    && group.max.y >= min.y
                    && group.min.z <= max.z
                    && group.max.z >= min.z
            })
            .flat_map(|group| group.faces.iter())
            .filter(|face| {
                face.min.x <= max.x
                    && face.max.x >= min.x
                    && face.min.y <= max.y
                    && face.max.y >= min.y
                    && face.min.z <= max.z
                    && face.max.z >= min.z
            })
            .copied()
            .collect()
    }
    pub fn anchored_collider_count(&self) -> usize {
        self.anchored_collision.len()
    }

    pub fn anchored_floor_at(&self, foot: Vec3, radius: f32, tolerance: f32) -> bool {
        self.anchored_collision
            .iter()
            .filter(|group| {
                group.min.x <= foot.x + radius
                    && group.max.x >= foot.x - radius
                    && group.min.z <= foot.z + radius
                    && group.max.z >= foot.z - radius
                    && group.min.y <= foot.y + tolerance
                    && group.max.y >= foot.y - tolerance
            })
            .flat_map(|group| group.faces.iter())
            .any(|face| {
                face.normal.y > 0.5
                    && (face.min.y - foot.y).abs() <= tolerance
                    && face.max.x > foot.x - radius
                    && face.min.x < foot.x + radius
                    && face.max.z > foot.z - radius
                    && face.min.z < foot.z + radius
            })
    }
    pub fn anchored_aabb_contact(&self, min: Vec3, max: Vec3) -> Option<(Vec3, f32)> {
        let mut selected: Option<(Vec3, f32)> = None;
        for face in self.anchored_faces_in_aabb(min, max) {
            let n = face.normal;
            let crossed = if n.x.abs() > 0.5 {
                min.y < face.max.y
                    && max.y > face.min.y
                    && min.z < face.max.z
                    && max.z > face.min.z
                    && min.x < face.min.x
                    && max.x > face.min.x
            } else if n.y.abs() > 0.5 {
                min.x < face.max.x
                    && max.x > face.min.x
                    && min.z < face.max.z
                    && max.z > face.min.z
                    && min.y < face.min.y
                    && max.y > face.min.y
            } else {
                min.x < face.max.x
                    && max.x > face.min.x
                    && min.y < face.max.y
                    && max.y > face.min.y
                    && min.z < face.min.z
                    && max.z > face.min.z
            };
            if !crossed {
                continue;
            }
            let depth = if n.x < -0.5 {
                max.x - face.min.x
            } else if n.x > 0.5 {
                face.min.x - min.x
            } else if n.y < -0.5 {
                max.y - face.min.y
            } else if n.y > 0.5 {
                face.min.y - min.y
            } else if n.z < -0.5 {
                max.z - face.min.z
            } else {
                face.min.z - min.z
            };
            if depth > 0.0 && selected.is_none_or(|(_, best)| depth < best) {
                selected = Some((n, depth));
            }
        }
        selected
    }
    pub(crate) fn anchored_actor_contact(
        &self,
        collider: &crate::engine::collision_geometry::ColliderInstance,
        position: Vec3,
    ) -> Option<(Vec3, f32)> {
        use crate::engine::collision_geometry;
        let (local_min, local_max) = collision_geometry::envelope(collider)?;
        let min = position + local_min;
        let max = position + local_max;
        let mut chosen: Option<(Vec3, f32)> = None;

        for face in self.anchored_faces_in_aabb(min, max) {
            let normal = face.normal;
            let plane = face.min;
            let along_min = if normal.x < -0.5 || normal.y < -0.5 || normal.z < -0.5 {
                max.dot(normal)
            } else {
                min.dot(normal)
            };
            let along_max = if normal.x < -0.5 || normal.y < -0.5 || normal.z < -0.5 {
                min.dot(normal)
            } else {
                max.dot(normal)
            };
            let crossing = along_min < plane.dot(normal) && along_max > plane.dot(normal);
            if !crossing {
                continue;
            }
            let (a, b, c, d) = if normal.x.abs() > 0.5 {
                let x = face.min.x;
                (
                    Vec3::new(x, face.min.y, face.min.z),
                    Vec3::new(x, face.max.y, face.min.z),
                    Vec3::new(x, face.max.y, face.max.z),
                    Vec3::new(x, face.min.y, face.max.z),
                )
            } else if normal.y.abs() > 0.5 {
                let y = face.min.y;
                (
                    Vec3::new(face.min.x, y, face.min.z),
                    Vec3::new(face.max.x, y, face.min.z),
                    Vec3::new(face.max.x, y, face.max.z),
                    Vec3::new(face.min.x, y, face.max.z),
                )
            } else {
                let z = face.min.z;
                (
                    Vec3::new(face.min.x, face.min.y, z),
                    Vec3::new(face.max.x, face.min.y, z),
                    Vec3::new(face.max.x, face.max.y, z),
                    Vec3::new(face.min.x, face.max.y, z),
                )
            };
            let mut corners = [a, b, c, d];
            if (corners[1] - corners[0])
                .cross(corners[2] - corners[0])
                .dot(normal)
                < 0.0
            {
                corners.swap(1, 3);
            }
            let mut depth = 0.0f32;
            for triangle in [
                [corners[0], corners[1], corners[2]],
                [corners[0], corners[2], corners[3]],
            ] {
                if let Some(hit) =
                    collision_geometry::against_triangle(collider, position, triangle)
                {
                    depth = depth.max(hit.depth);
                }
            }
            if depth > 0.0001 && chosen.is_none_or(|(_, best)| depth < best) {
                chosen = Some((normal, depth));
            }
        }
        chosen
    }

    fn dynamic_grid_coord(value: f32) -> i32 {
        (value / DYNAMIC_BROADPHASE_CELL_SIZE).floor() as i32
    }

    pub fn rebuild_dynamic_body_index(&mut self) {
        self.dynamic_bodies_by_cell.clear();

        for (body_index, body) in self.bodies.iter().enumerate() {
            let min = body.min_corner();
            let max = body.max_corner();

            let min_x = Self::dynamic_grid_coord(min.x);
            let max_x = Self::dynamic_grid_coord(max.x);
            let min_y = Self::dynamic_grid_coord(min.y);
            let max_y = Self::dynamic_grid_coord(max.y);
            let min_z = Self::dynamic_grid_coord(min.z);
            let max_z = Self::dynamic_grid_coord(max.z);

            for x in min_x..=max_x {
                for y in min_y..=max_y {
                    for z in min_z..=max_z {
                        self.dynamic_bodies_by_cell
                            .entry((x, y, z))
                            .or_default()
                            .push(body_index);
                    }
                }
            }
        }
    }

    pub fn query_dynamic_bodies_in_aabb(&self, min_pos: Vec3, max_pos: Vec3) -> Vec<usize> {
        let _query = crate::engine::development_capture::operation(
            "physics",
            "query_dynamic_bodies_in_aabb",
            true,
        );
        let min_x = Self::dynamic_grid_coord(min_pos.x);
        let max_x = Self::dynamic_grid_coord(max_pos.x);
        let min_y = Self::dynamic_grid_coord(min_pos.y);
        let max_y = Self::dynamic_grid_coord(max_pos.y);
        let min_z = Self::dynamic_grid_coord(min_pos.z);
        let max_z = Self::dynamic_grid_coord(max_pos.z);

        let mut seen = HashSet::new();
        let mut results = Vec::new();

        for x in min_x..=max_x {
            for y in min_y..=max_y {
                for z in min_z..=max_z {
                    let Some(indices) = self.dynamic_bodies_by_cell.get(&(x, y, z)) else {
                        continue;
                    };

                    for &index in indices {
                        if seen.insert(index) {
                            results.push(index);
                        }
                    }
                }
            }
        }

        results
    }

    pub(crate) fn anchored_support_friction(&self, min: Vec3, max: Vec3, normal: Vec3) -> f32 {
        let candidates =
            self.query_static_colliders_in_aabb(min - Vec3::splat(0.025), max + Vec3::splat(0.025));
        let mut total = 0.0f32;
        let mut area = 0.0f32;
        for (id, pos) in candidates {
            let lo = pos;
            let hi = pos + Vec3::ONE;
            let (distance, patch_area) = if normal.y.abs() > 0.5 {
                let plane = if normal.y > 0.0 { hi.y } else { lo.y };
                let edge = if normal.y > 0.0 { min.y } else { max.y };
                (
                    edge - plane,
                    (max.x.min(hi.x) - min.x.max(lo.x)).max(0.0)
                        * (max.z.min(hi.z) - min.z.max(lo.z)).max(0.0),
                )
            } else if normal.x.abs() > 0.5 {
                let plane = if normal.x > 0.0 { hi.x } else { lo.x };
                let edge = if normal.x > 0.0 { min.x } else { max.x };
                (
                    edge - plane,
                    (max.y.min(hi.y) - min.y.max(lo.y)).max(0.0)
                        * (max.z.min(hi.z) - min.z.max(lo.z)).max(0.0),
                )
            } else {
                let plane = if normal.z > 0.0 { hi.z } else { lo.z };
                let edge = if normal.z > 0.0 { min.z } else { max.z };
                (
                    edge - plane,
                    (max.x.min(hi.x) - min.x.max(lo.x)).max(0.0)
                        * (max.y.min(hi.y) - min.y.max(lo.y)).max(0.0),
                )
            };
            if distance.abs() <= 0.025 && patch_area > 0.0 {
                total += patch_area
                    * self
                        .static_friction
                        .get(&id)
                        .copied()
                        .unwrap_or(crate::world::cell::DEFAULT_SURFACE_FRICTION);
                area += patch_area;
            }
        }
        if area > 0.0 {
            total / area
        } else {
            crate::world::cell::DEFAULT_SURFACE_FRICTION
        }
    }

    pub(crate) fn check_body_touching_dynamic(
        &self,
        body: &PhysicsBody,
        other: &PhysicsBody,
        normal_other_to_body: Vec3,
    ) -> bool {
        let epsilon = 0.01;

        let test_min = body.min_corner() - normal_other_to_body * epsilon;
        let test_max = body.max_corner() - normal_other_to_body * epsilon;

        Self::aabb_overlap_static(test_min, test_max, other.min_corner(), other.max_corner())
    }

    pub(crate) fn check_body_touching_static(&self, body: &PhysicsBody, normal: Vec3) -> bool {
        let min = body.min_corner() - normal * 0.01;
        let max = body.max_corner() - normal * 0.01;
        self.anchored_faces_in_aabb(min, max)
            .into_iter()
            .any(|face| {
                if face.normal.dot(normal) < 0.99 {
                    return false;
                }
                if normal.x.abs() > 0.5 {
                    min.x < face.min.x
                        && max.x > face.min.x
                        && min.y < face.max.y
                        && max.y > face.min.y
                        && min.z < face.max.z
                        && max.z > face.min.z
                } else if normal.y.abs() > 0.5 {
                    min.y < face.min.y
                        && max.y > face.min.y
                        && min.x < face.max.x
                        && max.x > face.min.x
                        && min.z < face.max.z
                        && max.z > face.min.z
                } else {
                    min.z < face.min.z
                        && max.z > face.min.z
                        && min.x < face.max.x
                        && max.x > face.min.x
                        && min.y < face.max.y
                        && max.y > face.min.y
                }
            })
    }
    pub fn check_static_collisions(&self) -> Vec<CollisionRecord> {
        let _query = crate::engine::development_capture::operation(
            "physics",
            "check_static_collisions",
            true,
        );
        let mut collisions = Vec::new();

        for body in &self.bodies {
            if !body.solid {
                continue;
            }

            let candidates =
                self.query_static_colliders_in_aabb(body.min_corner(), body.max_corner());

            for (_cell_id, v_min) in candidates {
                let v_max = v_min + Vec3::ONE;

                if Self::aabb_overlap_static(body.min_corner(), body.max_corner(), v_min, v_max) {
                    collisions.push(CollisionRecord {
                        body_id: body.id,
                        block_coord: WorldCoord::from_vec3(v_min),
                    });
                }
            }
        }

        collisions
    }

    pub(crate) fn aabb_overlap_static(min_a: Vec3, max_a: Vec3, min_b: Vec3, max_b: Vec3) -> bool {
        min_a.x < max_b.x
            && max_a.x > min_b.x
            && min_a.y < max_b.y
            && max_a.y > min_b.y
            && min_a.z < max_b.z
            && max_a.z > min_b.z
    }

    pub fn register_from_world(&mut self, world: &World) {
        self.rapier_runtime = None;
        self.rapier_published.clear();
        self.rapier_terrain_patches.clear();
        self.bodies.clear();
        self.static_colliders.clear();
        self.static_colliders_by_chunk.clear();
        self.static_friction.clear();
        self.anchored_collision.clear();
        self.dynamic_bodies_by_cell.clear();

        self.id_gen = PhysicsIdGenerator::new();
        self.step_count = 0;
        self.terrain_tiles.borrow_mut().clear();
        self.native_terrain_collider = None;

        for coord in world.active_blocks() {
            let Some(cell) = world.get(coord) else {
                continue;
            };

            if cell.cell_type == CellType::Light {
                continue;
            }

            let solid = world.is_cell_solid(coord);
            self.static_friction.insert(cell.id, cell.friction());

            if solid {
                let anchored = world.is_cell_anchored(coord);

                if anchored {
                    let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                        + world.get_visual_offset(coord);

                    self.static_colliders.push((cell.id, pos));
                } else {
                    let id = self.id_gen.next();

                    let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                        + world.get_visual_offset(coord);

                    let mut body = PhysicsBody::new(id, cell.id, pos, Vec3::ONE);

                    body.cell_type = cell.cell_type;
                    body.visible = world.is_cell_visible(coord);
                    body.solid = solid;
                    body.anchored = anchored;
                    body.texture = cell.texture.clone();
                    body.color_rgb = world.get_effective_color(coord);
                    body.friction = cell.friction();

                    self.bodies.push(body);
                }
            }
        }

        self.rebuild_static_chunk_index();
        self.rebuild_dynamic_body_index();

        println!(
            "Physics: Registered {} dynamic bodies and {} static colliders.",
            self.bodies.len(),
            self.static_colliders.len()
        );
    }

    pub fn add_static_collider(&mut self, cell_id: u64, position: Vec3) {
        self.static_colliders.push((cell_id, position));
        self.rebuild_static_chunk_index();
    }

    pub fn sync_with_world(&mut self, world: &mut World) {
        let dirty_ids: Vec<u64> = world.physics_dirty_cells.drain().collect();

        if dirty_ids.is_empty() {
            return;
        }

        let previous_static = self.static_colliders.clone();
        let previous_friction = self.static_friction.clone();
        for cell_id in dirty_ids {
            self.static_friction.remove(&cell_id);
            if let Some(coord) = world.resolve_cell_id(cell_id) {
                if let Some(cell) = world.get_effective_cell_by_id(cell_id) {
                    if cell.cell_type == CellType::Light {
                        self.static_colliders.retain(|(id, _)| *id != cell_id);
                        self.bodies.retain(|b| b.cell_id != cell_id);
                        continue;
                    }

                    let anchored = world.is_cell_anchored(coord);
                    let solid = world.is_cell_solid(coord);
                    self.static_friction.insert(cell_id, cell.friction());

                    if anchored {
                        self.bodies.retain(|b| b.cell_id != cell_id);

                        if solid {
                            let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                                + world.get_visual_offset(coord);

                            if let Some(existing) = self
                                .static_colliders
                                .iter_mut()
                                .find(|(id, _)| *id == cell_id)
                            {
                                existing.1 = pos;
                            } else {
                                self.static_colliders.push((cell_id, pos));
                            }
                        } else {
                            self.static_colliders.retain(|(id, _)| *id != cell_id);
                        }
                    } else {
                        self.static_colliders.retain(|(id, _)| *id != cell_id);

                        if solid {
                            if let Some(body) =
                                self.bodies.iter_mut().find(|b| b.cell_id == cell_id)
                            {
                                body.solid = true;
                                body.visible = world.is_cell_visible(coord);
                                body.texture = cell.texture.clone();
                                body.color_rgb = world.get_effective_color(coord);
                                body.friction = cell.friction();
                            } else {
                                let id = self.id_gen.next();

                                let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                                    + world.get_visual_offset(coord);

                                let mut body = PhysicsBody::new(id, cell_id, pos, Vec3::ONE);

                                body.cell_type = cell.cell_type;
                                body.visible = world.is_cell_visible(coord);
                                body.solid = true;
                                body.anchored = false;
                                body.texture = cell.texture.clone();
                                body.color_rgb = world.get_effective_color(coord);
                                body.friction = cell.friction();

                                self.bodies.push(body);
                            }
                        } else {
                            self.bodies.retain(|b| b.cell_id != cell_id);
                        }
                    }
                } else {
                    self.static_colliders.retain(|(id, _)| *id != cell_id);
                    self.bodies.retain(|b| b.cell_id != cell_id);
                }
            } else {
                self.static_colliders.retain(|(id, _)| *id != cell_id);
                self.bodies.retain(|b| b.cell_id != cell_id);
            }
        }

        if self.static_colliders != previous_static || self.static_friction != previous_friction {
            self.rebuild_static_chunk_index();
        }
        self.rebuild_dynamic_body_index();
    }
}

#[cfg(test)]
mod anchored_actor_shape_tests {
    use super::*;
    use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
    use std::sync::Arc;

    #[test]
    fn anchored_cell_uses_actual_oriented_actor_box_not_enclosing_aabb() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(42, Vec3::ZERO);
        let bar = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-1.0, -0.1, -0.05],
                max: [1.0, 0.1, 0.05],
            }]),
            rotation: Vec3::new(0.0, std::f32::consts::FRAC_PI_4, 0.0),
            scale: Vec3::ONE,
        };
        let position = Vec3::new(1.16, 0.5, 1.16);
        assert!(
            world.anchored_actor_contact(&bar, position).is_none(),
            "a bounding box overlapping empty space must not generate an Actor contact"
        );

        let box_collider = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-0.5, -0.25, -0.25],
                max: [0.5, 0.25, 0.25],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let (normal, depth) = world
            .anchored_actor_contact(&box_collider, Vec3::new(1.3, 0.5, 0.5))
            .expect("a real Actor/Cell intersection must report a contact");
        assert_eq!(normal, Vec3::X);
        assert!(depth > 0.0);
    }
}
