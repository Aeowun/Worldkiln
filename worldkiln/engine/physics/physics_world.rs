use super::physics_body::{CollisionRecord, PhysicsBody, PhysicsBodyId, PhysicsIdGenerator};
use crate::world::{CellType, ChunkCoord, World, WorldCoord};
use glam::Vec3;
use std::collections::{HashMap, HashSet};

pub(crate) const DYNAMIC_BROADPHASE_CELL_SIZE: f32 = 2.0;

pub struct PhysicsWorld {
    pub bodies: Vec<PhysicsBody>,

    // Static colliders are blocks that are solid and anchored.
    // They are stored with their effective runtime position.
    pub static_colliders: Vec<(u64, Vec3)>,
    pub static_colliders_by_chunk: HashMap<ChunkCoord, Vec<(u64, Vec3)>>,

    /// Broad-phase index for dynamic physics bodies.
    ///
    /// The index is rebuilt once per simulation step after dynamic bodies
    /// have been integrated and resolved.
    pub(crate) dynamic_bodies_by_cell: HashMap<(i32, i32, i32), Vec<usize>>,

    pub(crate) id_gen: PhysicsIdGenerator,

    // Step counter used to throttle console output.
    pub(crate) step_count: u64,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            bodies: Vec::new(),
            static_colliders: Vec::new(),
            static_colliders_by_chunk: HashMap::new(),
            dynamic_bodies_by_cell: HashMap::new(),
            id_gen: PhysicsIdGenerator::new(),
            step_count: 0,
        }
    }

    pub fn rebuild_static_chunk_index(&mut self) {
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

    fn dynamic_grid_coord(value: f32) -> i32 {
        (value / DYNAMIC_BROADPHASE_CELL_SIZE).floor() as i32
    }

    /// Rebuilds the dynamic-body broad-phase index.
    ///
    /// This is intentionally rebuilt once per simulation step rather than
    /// maintained incrementally while bodies move.
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

    /// Returns dynamic bodies whose broad-phase cells overlap an AABB.
    ///
    /// Exact geometric overlap is still checked by the caller.
    pub fn query_dynamic_bodies_in_aabb(&self, min_pos: Vec3, max_pos: Vec3) -> Vec<usize> {
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

    // Touching helper for dynamic bodies. normal_other_to_body points from other to body.
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

    // Touching helper for static blocks.
    pub(crate) fn check_body_touching_static(&self, body: &PhysicsBody, normal: Vec3) -> bool {
        let epsilon = 0.01;

        let test_min = body.min_corner() - normal * epsilon;
        let test_max = body.max_corner() - normal * epsilon;

        let candidates = self.query_static_colliders_in_aabb(test_min, test_max);

        for (_cell_id, v_min) in &candidates {
            let v_max = v_min + Vec3::ONE;

            if Self::aabb_overlap_static(test_min, test_max, *v_min, v_max) {
                return true;
            }
        }

        false
    }

    pub fn check_static_collisions(&self) -> Vec<CollisionRecord> {
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

    /// Populate static colliders and dynamic bodies from World data.
    pub fn register_from_world(&mut self, world: &World) {
        self.bodies.clear();
        self.static_colliders.clear();
        self.static_colliders_by_chunk.clear();
        self.dynamic_bodies_by_cell.clear();

        self.id_gen = PhysicsIdGenerator::new();
        self.step_count = 0;

        for coord in world.active_blocks() {
            let Some(cell) = world.get(coord) else {
                continue;
            };

            if cell.cell_type == CellType::Light {
                continue;
            }

            let solid = world.is_cell_solid(coord);

            if solid {
                let anchored = world.is_cell_anchored(coord);

                if anchored {
                    let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                        + world.get_visual_offset(coord);

                    self.static_colliders.push((cell.id, pos));
                } else {
                    let id = self.id_gen.next();

                    let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32);

                    let mut body = PhysicsBody::new(id, cell.id, pos, Vec3::ONE);

                    // Authoritative Property Transfer
                    body.cell_type = cell.cell_type;
                    body.visible = world.is_cell_visible(coord);
                    body.solid = solid;
                    body.anchored = anchored;
                    body.texture = cell.texture.clone();
                    body.color_rgb = world.get_effective_color(coord);

                    self.bodies.push(body);
                }
            }
        }

        // One bulk rebuild after all static colliders have been registered.
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

    /// Reconciles the physics simulation state with the World's effective state
    /// (authored data + runtime overrides).
    pub fn sync_with_world(&mut self, world: &mut World) {
        // 1. Process dirty cells (Create/Update/Remove static colliders and dynamic bodies).
        // This is O(number of changes) and handles all static-collider transitions.
        let dirty_ids: Vec<u64> = world.physics_dirty_cells.drain().collect();

        if dirty_ids.is_empty() {
            return;
        }

        for cell_id in dirty_ids {
            if let Some(coord) = world.resolve_cell_id(cell_id) {
                if let Some(cell) = world.get_effective_cell_by_id(cell_id) {
                    if cell.cell_type == CellType::Light {
                        self.static_colliders.retain(|(id, _)| *id != cell_id);
                        self.bodies.retain(|b| b.cell_id != cell_id);
                        continue;
                    }

                    let anchored = world.is_cell_anchored(coord);
                    let solid = world.is_cell_solid(coord);

                    if anchored {
                        // Transition/Reconcile Static
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
                        // Transition/Reconcile Dynamic
                        self.static_colliders.retain(|(id, _)| *id != cell_id);

                        if solid {
                            if let Some(body) =
                                self.bodies.iter_mut().find(|b| b.cell_id == cell_id)
                            {
                                body.solid = true;
                                body.visible = world.is_cell_visible(coord);
                                body.texture = cell.texture.clone();
                                body.color_rgb = world.get_effective_color(coord);
                            } else {
                                let id = self.id_gen.next();

                                let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32);

                                let mut body = PhysicsBody::new(id, cell_id, pos, Vec3::ONE);

                                body.cell_type = cell.cell_type;
                                body.visible = world.is_cell_visible(coord);
                                body.solid = true;
                                body.anchored = false;
                                body.texture = cell.texture.clone();
                                body.color_rgb = world.get_effective_color(coord);

                                self.bodies.push(body);
                            }
                        } else {
                            self.bodies.retain(|b| b.cell_id != cell_id);
                        }
                    }
                } else {
                    // ID no longer resolves to a cell
                    // (e.g. Empty or removed runtime cell).
                    self.static_colliders.retain(|(id, _)| *id != cell_id);
                    self.bodies.retain(|b| b.cell_id != cell_id);
                }
            } else {
                // Cell was deleted.
                self.static_colliders.retain(|(id, _)| *id != cell_id);
                self.bodies.retain(|b| b.cell_id != cell_id);
            }
        }

        self.rebuild_static_chunk_index();
        self.rebuild_dynamic_body_index();
    }
}
