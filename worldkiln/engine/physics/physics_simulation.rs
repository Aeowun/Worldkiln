use super::physics_body::PhysicsBodyId;
use super::physics_world::PhysicsWorld;
use crate::engine::entity::{EntityId, EntityManager};
use glam::Vec3;
use std::collections::HashSet;

// 60Hz fixed timestep keeps simulation stable and deterministic regardless of frame rate.
pub const SIMULATION_DT: f32 = 1.0 / 60.0;

// Limit catch up steps to 8 per frame to avoid the 'spiral of death' during stalls.
pub const MAX_PHYSICS_STEPS: u32 = 8;

// Velocity below which a body is considered "at rest" for sleep calculation.
pub const SLEEP_VELOCITY_THRESHOLD: f32 = 0.02;

// Time in seconds a body must stay below the threshold to fall asleep.
pub const SLEEP_TIME_THRESHOLD: f32 = 0.5;

pub struct PhysicsClock {
    pub accumulator: f32,
}

impl PhysicsClock {
    pub fn new() -> Self {
        Self { accumulator: 0.0 }
    }

    pub fn reset(&mut self) {
        self.accumulator = 0.0;
    }

    pub fn update<F>(&mut self, dt: f32, mut step_fn: F)
    where
        F: FnMut(f32),
    {
        let dt = dt.min(0.25);
        self.accumulator += dt;

        let mut steps_processed = 0;

        while self.accumulator >= SIMULATION_DT {
            if steps_processed >= MAX_PHYSICS_STEPS {
                self.accumulator = 0.0;
                break;
            }

            step_fn(SIMULATION_DT);
            self.accumulator -= SIMULATION_DT;
            steps_processed += 1;
        }
    }
}

impl PhysicsWorld {
    // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
    // Keep every Cell-physics caller on the same ordered solver pipeline. The
    // remembered pre-step positions are also the authority for floor crossings
    // that endpoint-only overlap tests cannot see.
    pub fn step_simulation(&mut self, gravity: Vec3, dt: f32) {
        let positions_before_step: Vec<Vec3> =
            self.bodies.iter().map(|body| body.position).collect();

        self.apply_gravity(gravity, dt);
        self.integrate_positions(dt);
        self.resolve_downward_static_crossings(&positions_before_step);

        // Dynamic separation can create a new static penetration. Alternate
        // the two constraints so a later pair cannot undo an earlier floor
        // correction, then finish on the immovable static geometry.
        for _ in 0..3 {
            self.resolve_static_collisions();
            self.resolve_dynamic_collisions();
            self.resolve_downward_static_crossings(&positions_before_step);
        }
        self.resolve_static_collisions();
        self.resolve_downward_static_crossings(&positions_before_step);
        self.refresh_dynamic_support();
        self.update_sleeping(gravity);
        self.rebuild_dynamic_body_index();
    }

    pub fn apply_gravity(&mut self, gravity: Vec3, dt: f32) {
        self.step_count += 1;

        for body in &mut self.bodies {
            if body.is_sleeping || body.anchored || !body.gravity_participation {
                continue;
            }

            let mut effective_gravity = gravity;

            // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
            // Project gravity against each actual contact independently. A
            // floor normal plus a neighboring Block's side normal must not be
            // averaged into a diagonal; doing so converts downward gravity
            // into lateral acceleration and walks dense drops off their floor.
            if let Some(normal) = body.static_contact_normal {
                let gravity_into_surface = gravity.dot(normal);
                if gravity_into_surface < 0.0 {
                    effective_gravity -= normal * gravity_into_surface;
                }
            }

            if let Some((_, normal)) = body.dynamic_contact {
                let gravity_into_surface = gravity.dot(normal);
                if gravity_into_surface < 0.0 {
                    effective_gravity -= normal * gravity_into_surface;
                }
            }

            body.velocity += effective_gravity * dt;
        }

        if self.step_count % 60 == 0 {
            for (i, body) in self.bodies.iter().enumerate() {
                println!(
                    "Physics Debug | Body[{}] Pos: ({:.3}, {:.3}, {:.3}) Vel: ({:.3}, {:.3}, {:.3}) Solid: {} Anchored: {} Sleep: {}",
                    i,
                    body.position.x,
                    body.position.y,
                    body.position.z,
                    body.velocity.x,
                    body.velocity.y,
                    body.velocity.z,
                    body.solid,
                    body.anchored,
                    body.is_sleeping,
                );
            }
        }
    }

    pub fn integrate_positions(&mut self, dt: f32) {
        for body in &mut self.bodies {
            if !body.anchored && !body.is_sleeping {
                body.position += body.velocity * dt;
            }
        }
    }

    pub fn resolve_static_collisions(&mut self) {
        for i in 0..self.bodies.len() {
            if !self.bodies[i].solid || self.bodies[i].anchored {
                continue;
            }

            let mut latest_contact_normal: Option<Vec3> = None;

            for _ in 0..4 {
                let body = &self.bodies[i];
                let b_min = body.min_corner();
                let b_max = body.max_corner();
                let b_center = (b_min + b_max) * 0.5;

                let mut best_collision: Option<(Vec3, f32)> = None;
                let mut min_overlap = f32::MAX;

                let candidates = self.query_static_colliders_in_aabb(
                    b_min - Vec3::splat(0.5),
                    b_max + Vec3::splat(0.5),
                );

                for (_cell_id, v_min) in &candidates {
                    let v_max = v_min + Vec3::ONE;
                    let v_center = v_min + Vec3::new(0.5, 0.5, 0.5);

                    if Self::aabb_overlap_static(b_min, b_max, *v_min, v_max) {
                        let overlap_x = b_max.x.min(v_max.x) - b_min.x.max(v_min.x);
                        let overlap_y = b_max.y.min(v_max.y) - b_min.y.max(v_min.y);
                        let overlap_z = b_max.z.min(v_max.z) - b_min.z.max(v_min.z);

                        // Worldkiln 0.8.7 | WO 0.8.7-24 | 2026-10-06 15:03 -05:00
                        // RF11's vertical-column preference must only apply to bodies that
                        // are actually moving primarily along Y. Applying it solely from X/Z
                        // alignment misclassifies wall contacts (including sideways gravity)
                        // as floor contacts when both centers share the same height.
                        let vertical_motion_dominant = body.velocity.y.abs()
                            > body.velocity.x.abs().max(body.velocity.z.abs()) + 1.0e-6;
                        let (axis_normal, depth) = if vertical_motion_dominant
                            && (b_center.x - v_center.x).abs() < 0.4
                            && (b_center.z - v_center.z).abs() < 0.4
                        {
                            let n = if b_center.y < v_center.y {
                                Vec3::NEG_Y
                            } else {
                                Vec3::Y
                            };

                            (n, overlap_y)
                        } else if overlap_x < overlap_y && overlap_x < overlap_z {
                            let n = if b_center.x < v_center.x {
                                Vec3::NEG_X
                            } else {
                                Vec3::X
                            };

                            (n, overlap_x)
                        } else if overlap_y < overlap_z {
                            let n = if b_center.y < v_center.y {
                                Vec3::NEG_Y
                            } else {
                                Vec3::Y
                            };

                            (n, overlap_y)
                        } else {
                            let n = if b_center.z < v_center.z {
                                Vec3::NEG_Z
                            } else {
                                Vec3::Z
                            };

                            (n, overlap_z)
                        };

                        if depth < min_overlap {
                            min_overlap = depth;
                            best_collision = Some((axis_normal, depth));
                        }
                    }
                }

                if let Some((normal, penetration)) = best_collision {
                    if penetration > 0.0001 {
                        let body = &mut self.bodies[i];
                        let epsilon = 0.0001;

                        // Worldkiln 0.8.7 | WO 0.8.7-24 | 2026-10-06 15:03 -05:00
                        // Resolve the selected static MTV in one pass. The previous 0.1
                        // cap left deep but valid wall penetrations unresolved after the
                        // four-iteration solver budget (for example -0.1 inside a unit
                        // wall only reached -0.5 instead of the separating -1.0).
                        let correction_dist = penetration + epsilon;

                        body.position += normal * correction_dist;

                        let v_in = body.velocity.dot(normal);

                        if v_in < 0.0 {
                            body.velocity -= normal * v_in;
                        }

                        latest_contact_normal = Some(normal);
                        body.wake();
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if let Some(n) = latest_contact_normal {
                self.bodies[i].static_contact_normal = Some(n);
            } else if let Some(n) = self.bodies[i].static_contact_normal {
                if !self.check_body_touching_static(&self.bodies[i], n) {
                    self.bodies[i].static_contact_normal = None;
                }
            }
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
    // Catch downward fixed-step crossings that endpoint-only overlap tests miss.
    fn resolve_downward_static_crossings(&mut self, positions_before_step: &[Vec3]) {
        const EPSILON: f32 = 0.0001;

        for i in 0..self.bodies.len() {
            let body = &self.bodies[i];
            if !body.solid || body.anchored {
                continue;
            }

            let Some(&previous_position) = positions_before_step.get(i) else {
                continue;
            };

            if body.position.y >= previous_position.y - EPSILON {
                continue;
            }

            let previous_min = previous_position;
            let previous_max = previous_position + body.size;
            let current_min = body.min_corner();
            let current_max = body.max_corner();
            let swept_min = previous_min.min(current_min);
            let swept_max = previous_max.max(current_max);
            let candidates = self.query_static_colliders_in_aabb(swept_min, swept_max);
            let mut crossed_surface: Option<f32> = None;

            for (_cell_id, static_min) in candidates {
                let static_max = static_min + Vec3::ONE;
                let overlaps_x = swept_min.x < static_max.x && swept_max.x > static_min.x;
                let overlaps_z = swept_min.z < static_max.z && swept_max.z > static_min.z;
                let started_above = previous_min.y >= static_max.y - EPSILON;
                let crossed_top = current_min.y < static_max.y - EPSILON;

                if overlaps_x && overlaps_z && started_above && crossed_top {
                    crossed_surface = Some(
                        crossed_surface.map_or(static_max.y, |surface| surface.max(static_max.y)),
                    );
                }
            }

            if let Some(surface_y) = crossed_surface {
                let body = &mut self.bodies[i];
                body.position.y = surface_y + EPSILON;
                if body.velocity.y < 0.0 {
                    body.velocity.y = 0.0;
                }
                body.static_contact_normal = Some(Vec3::Y);
                body.wake();
            }
        }
    }

    pub fn resolve_dynamic_collisions(&mut self) {
        let n = self.bodies.len();

        for i in 0..n {
            for j in (i + 1)..n {
                let (body_a, body_b) = {
                    let (left, right) = self.bodies.split_at_mut(j);
                    (&mut left[i], &mut right[0])
                };

                if !body_a.solid || !body_b.solid || (body_a.anchored && body_b.anchored) {
                    continue;
                }

                let min_a = body_a.min_corner();
                let max_a = body_a.max_corner();
                let center_a = (min_a + max_a) * 0.5;

                let min_b = body_b.min_corner();
                let max_b = body_b.max_corner();
                let center_b = (min_b + max_b) * 0.5;

                if Self::aabb_overlap_static(min_a, max_a, min_b, max_b) {
                    let overlap_x = max_a.x.min(max_b.x) - min_a.x.max(min_b.x);
                    let overlap_y = max_a.y.min(max_b.y) - min_a.y.max(min_b.y);
                    let overlap_z = max_a.z.min(max_b.z) - min_a.z.max(min_b.z);

                    let (normal, penetration) = if (center_a.x - center_b.x).abs() < 0.4
                        && (center_a.z - center_b.z).abs() < 0.4
                    {
                        let n = if center_a.y < center_b.y {
                            Vec3::NEG_Y
                        } else {
                            Vec3::Y
                        };

                        (n, overlap_y)
                    } else if overlap_x < overlap_y && overlap_x < overlap_z {
                        let n = if center_a.x < center_b.x {
                            Vec3::NEG_X
                        } else {
                            Vec3::X
                        };

                        (n, overlap_x)
                    } else if overlap_y < overlap_z {
                        let n = if center_a.y < center_b.y {
                            Vec3::NEG_Y
                        } else {
                            Vec3::Y
                        };

                        (n, overlap_y)
                    } else {
                        let n = if center_a.z < center_b.z {
                            Vec3::NEG_Z
                        } else {
                            Vec3::Z
                        };

                        (n, overlap_z)
                    };

                    // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
                    // Exact neighboring columns can acquire tiny horizontal overlap while
                    // constraints iterate. Only separate them horizontally while closing.
                    let is_horizontal = normal.y.abs() < 0.5;
                    let relative_velocity = body_a.velocity - body_b.velocity;
                    let closing_velocity = relative_velocity.dot(normal);

                    if penetration > 0.0001 && (!is_horizontal || closing_velocity < -0.001) {
                        let epsilon = 0.0001;

                        if body_a.anchored {
                            body_b.position -= normal * (penetration + epsilon);
                            let v_b_in = body_b.velocity.dot(-normal);
                            if v_b_in < 0.0 {
                                body_b.velocity -= (-normal) * v_b_in;
                            }
                        } else if body_b.anchored {
                            body_a.position += normal * (penetration + epsilon);
                            let v_a_in = body_a.velocity.dot(normal);
                            if v_a_in < 0.0 {
                                body_a.velocity -= normal * v_a_in;
                            }
                        } else if is_horizontal {
                            let correction = normal * (penetration * 0.5 + epsilon);
                            body_a.position += correction;
                            body_b.position -= correction;

                            let v_a_in = body_a.velocity.dot(normal);
                            if v_a_in < 0.0 {
                                body_a.velocity -= normal * v_a_in;
                            }
                            let v_b_in = body_b.velocity.dot(-normal);
                            if v_b_in < 0.0 {
                                body_b.velocity -= (-normal) * v_b_in;
                            }
                        } else {
                            // Worldkiln 0.8.7 | WO 0.8.7-24 | 2026-10-06 15:03 -05:00
                            // Vertical stack resolution separates to exact touching. Adding
                            // epsilon at every support layer accumulates a visible solver skin
                            // through tall stacks (0.0001 per layer) even after they sleep.
                            // Exact touching is already a non-overlap under the strict AABB
                            // predicate and refresh_dynamic_support deliberately recognizes it.
                            if center_a.y >= center_b.y {
                                body_a.position.y += penetration;
                                if body_a.velocity.y < 0.0 {
                                    body_a.velocity.y = 0.0;
                                }
                                if body_b.velocity.y < 0.0 {
                                    body_b.velocity.y = 0.0;
                                }
                            } else {
                                body_b.position.y += penetration;
                                if body_b.velocity.y < 0.0 {
                                    body_b.velocity.y = 0.0;
                                }
                                if body_a.velocity.y < 0.0 {
                                    body_a.velocity.y = 0.0;
                                }
                            }
                        }

                        body_a.wake();
                        body_b.wake();
                    }
                }
            }
        }
    }

    pub fn refresh_dynamic_support(&mut self) {
        let n = self.bodies.len();
        let mut new_supports: Vec<Option<(PhysicsBodyId, Vec3)>> = vec![None; n];

        for i in 0..n {
            for j in (i + 1)..n {
                let min_a = self.bodies[i].min_corner();
                let max_a = self.bodies[i].max_corner();
                let center_a = (min_a + max_a) * 0.5;

                let min_b = self.bodies[j].min_corner();
                let max_b = self.bodies[j].max_corner();
                let center_b = (min_b + max_b) * 0.5;

                let epsilon = 0.01;

                let overlap_x = max_a.x.min(max_b.x) - min_a.x.max(min_b.x);
                let overlap_y = max_a.y.min(max_b.y) - min_a.y.max(min_b.y);
                let overlap_z = max_a.z.min(max_b.z) - min_a.z.max(min_b.z);

                let ax = overlap_x.abs() < epsilon && overlap_y > 0.001 && overlap_z > 0.001;

                let ay = overlap_y.abs() < epsilon && overlap_x > 0.001 && overlap_z > 0.001;

                let az = overlap_z.abs() < epsilon && overlap_x > 0.001 && overlap_y > 0.001;

                if ax || ay || az {
                    // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
                    // The supported body is always the body on the positive side
                    // of the contact axis, so its support normal must point along
                    // that positive axis independent of pair iteration order.
                    let (supported_idx, supporting_idx, support_normal) = if ax {
                        if center_a.x < center_b.x {
                            (j, i, Vec3::X)
                        } else {
                            (i, j, Vec3::X)
                        }
                    } else if ay {
                        if center_a.y < center_b.y {
                            (j, i, Vec3::Y)
                        } else {
                            (i, j, Vec3::Y)
                        }
                    } else if center_a.z < center_b.z {
                        (j, i, Vec3::Z)
                    } else {
                        (i, j, Vec3::Z)
                    };

                    new_supports[supported_idx] =
                        Some((self.bodies[supporting_idx].id, support_normal));
                }
            }
        }

        for i in 0..n {
            let old = self.bodies[i].dynamic_contact;
            let new = new_supports[i];

            if old != new {
                self.bodies[i].wake();
            }

            self.bodies[i].dynamic_contact = new;
        }
    }

    pub fn update_sleeping(&mut self, gravity: Vec3) {
        for i in 0..self.bodies.len() {
            let mut should_wake = false;

            if let Some((support_id, _normal)) = self.bodies[i].dynamic_contact {
                if let Some(support) = self.bodies.iter().find(|b| b.id == support_id) {
                    if !support.is_sleeping {
                        should_wake = true;
                    }
                } else {
                    should_wake = true;
                }
            }

            if should_wake {
                self.bodies[i].wake();
            }
        }

        for body in &mut self.bodies {
            if body.anchored {
                continue;
            }

            if body.is_sleeping {
                body.velocity = Vec3::ZERO;
                continue;
            }

            if body.velocity.length() < SLEEP_VELOCITY_THRESHOLD {
                let mut is_supported = false;

                if body.gravity_participation && gravity.length_squared() > 1e-6 {
                    if let Some(n) = body.static_contact_normal {
                        if gravity.dot(n) < -1e-3 {
                            is_supported = true;
                        }
                    }

                    if !is_supported {
                        if let Some((_, n)) = body.dynamic_contact {
                            if gravity.dot(n) < -1e-3 {
                                is_supported = true;
                            }
                        }
                    }
                } else {
                    is_supported = true;
                }

                if is_supported {
                    body.sleep_timer += SIMULATION_DT;

                    if body.sleep_timer >= SLEEP_TIME_THRESHOLD {
                        body.is_sleeping = true;
                        body.velocity = Vec3::ZERO;
                    }
                } else {
                    body.sleep_timer = 0.0;
                }
            } else {
                body.sleep_timer = 0.0;
            }
        }
    }

    /// Simulates generic Entity physics against the existing static voxel
    /// colliders.
    ///
    /// CharacterSystem is deliberately not involved here.
    pub fn step_entities(&self, entity_manager: &mut EntityManager, gravity: Vec3, dt: f32) {
        self.step_entities_excluding(entity_manager, gravity, dt, &HashSet::new());
    }

    /// Simulates generic Entity physics while leaving externally-owned
    /// transforms alone. Character-backed Actors use this path so their
    /// visible Character pose and their script/contact Entity cannot drift.
    pub fn step_entities_excluding(
        &self,
        entity_manager: &mut EntityManager,
        gravity: Vec3,
        dt: f32,
        excluded_entities: &HashSet<EntityId>,
    ) {
        let entity_ids: Vec<EntityId> = entity_manager
            .iter()
            .filter(|entity| {
                entity.enabled
                    && entity.physics.enabled
                    && !entity.physics.anchored
                    && !excluded_entities.contains(&entity.id)
            })
            .map(|entity| entity.id)
            .collect();

        for entity_id in entity_ids {
            let Some(entity) = entity_manager.get_entity_mut(entity_id) else {
                continue;
            };

            let physics = &mut entity.physics;

            physics.grounded = false;

            if physics.mass <= 0.0 || !physics.mass.is_finite() {
                physics.stop();
                continue;
            }

            let acceleration =
                gravity * physics.gravity_scale + physics.accumulated_force / physics.mass;

            physics.velocity += acceleration * dt;
            physics.accumulated_force = Vec3::ZERO;

            entity.position += physics.velocity * dt;

            if !physics.collision_enabled || !physics.solid {
                continue;
            }

            let grounded = Self::resolve_entity_static_collision(
                entity.position,
                physics,
                &mut entity.position,
                gravity,
                self,
            );

            physics.grounded = grounded;
        }

        for entity in entity_manager.iter_mut() {
            if !entity.physics.enabled {
                continue;
            }

            if entity.physics.anchored {
                entity.physics.velocity = Vec3::ZERO;
                entity.physics.accumulated_force = Vec3::ZERO;
                entity.physics.grounded = false;
            }
        }
    }

    fn resolve_entity_static_collision(
        initial_position: Vec3,
        physics: &mut crate::engine::entity::EntityPhysicsState,
        position: &mut Vec3,
        gravity: Vec3,
        world: &PhysicsWorld,
    ) -> bool {
        let mut grounded = false;

        for _ in 0..4 {
            let half_extents = physics.half_extents();
            let center = *position + physics.offset;
            let min_pos = center - half_extents;
            let max_pos = center + half_extents;

            let candidates = world.query_static_colliders_in_aabb(
                min_pos - Vec3::splat(0.25),
                max_pos + Vec3::splat(0.25),
            );

            let mut best_collision: Option<(Vec3, f32)> = None;
            let mut smallest_penetration = f32::MAX;

            for (_cell_id, collider_min) in candidates {
                let collider_max = collider_min + Vec3::ONE;
                let collider_center = (collider_min + collider_max) * 0.5;

                if !Self::aabb_overlap(min_pos, max_pos, collider_min, collider_max) {
                    continue;
                }

                let overlap_x = max_pos.x.min(collider_max.x) - min_pos.x.max(collider_min.x);

                let overlap_y = max_pos.y.min(collider_max.y) - min_pos.y.max(collider_min.y);

                let overlap_z = max_pos.z.min(collider_max.z) - min_pos.z.max(collider_min.z);

                let (normal, penetration) = if overlap_x < overlap_y && overlap_x < overlap_z {
                    let normal = if center.x < collider_center.x {
                        Vec3::NEG_X
                    } else {
                        Vec3::X
                    };

                    (normal, overlap_x)
                } else if overlap_y < overlap_z {
                    let normal = if center.y < collider_center.y {
                        Vec3::NEG_Y
                    } else {
                        Vec3::Y
                    };

                    (normal, overlap_y)
                } else {
                    let normal = if center.z < collider_center.z {
                        Vec3::NEG_Z
                    } else {
                        Vec3::Z
                    };

                    (normal, overlap_z)
                };

                if penetration < smallest_penetration {
                    smallest_penetration = penetration;
                    best_collision = Some((normal, penetration));
                }
            }

            let Some((normal, penetration)) = best_collision else {
                break;
            };

            if penetration <= 0.0001 {
                break;
            }

            *position += normal * (penetration + 0.0001);

            let inward_velocity = physics.velocity.dot(normal);

            if inward_velocity < 0.0 {
                physics.velocity -= normal * inward_velocity;
            }

            if gravity.length_squared() > 1e-6 && gravity.dot(normal) < -1e-3 {
                grounded = true;
            }
        }

        let _ = initial_position;

        grounded
    }

    fn aabb_overlap(min_a: Vec3, max_a: Vec3, min_b: Vec3, max_b: Vec3) -> bool {
        min_a.x < max_b.x
            && max_a.x > min_b.x
            && min_a.y < max_b.y
            && max_a.y > min_b.y
            && min_a.z < max_b.z
            && max_a.z > min_b.z
    }

    /// Evaluates generic Entity-to-Entity and Entity-to-Character collisions and contacts.
    ///
    /// Resolves solid penetration between unanchored/anchored Entities and tracks
    /// transition state (current, entered, exited) in `entity_manager.last_contact_result`.
    pub fn process_entity_contacts_and_collisions(
        &self,
        entity_manager: &mut EntityManager,
        character_system: Option<&mut crate::character::CharacterSystem>,
    ) -> crate::engine::entity::EntityContactResult {
        use crate::engine::entity::{EntityContactPair, EntityContactResult, EntityId};
        use std::collections::HashSet;

        let mut raw_contacts = HashSet::new();

        // 1. Gather active physics Entities
        let physics_entities: Vec<(EntityId, Vec3, Vec3, Vec3, bool, bool, bool)> = entity_manager
            .iter()
            .filter(|e| e.enabled && e.physics.enabled)
            .map(|e| {
                (
                    e.id,
                    e.position,
                    e.physics.size,
                    e.physics.offset,
                    e.physics.solid,
                    e.physics.anchored,
                    e.physics.collision_enabled,
                )
            })
            .collect();

        let n = physics_entities.len();

        // 2. Entity ↔ Entity overlap & solid collision resolution
        for i in 0..n {
            for j in (i + 1)..n {
                let (id_a, pos_a, size_a, offset_a, solid_a, anchored_a, col_a) =
                    physics_entities[i];
                let (id_b, pos_b, size_b, offset_b, solid_b, anchored_b, col_b) =
                    physics_entities[j];

                let half_a = size_a * 0.5;
                let center_a = pos_a + offset_a;
                let min_a = center_a - half_a;
                let max_a = center_a + half_a;

                let half_b = size_b * 0.5;
                let center_b = pos_b + offset_b;
                let min_b = center_b - half_b;
                let max_b = center_b + half_b;

                if Self::aabb_overlap(min_a, max_a, min_b, max_b) {
                    if col_a && col_b {
                        raw_contacts.insert(EntityContactPair::new(id_a, id_b));
                    }

                    // Resolve solid penetration
                    if solid_a && solid_b && col_a && col_b {
                        let pos_a_curr = entity_manager.get_position(id_a).unwrap_or(pos_a);
                        let pos_b_curr = entity_manager.get_position(id_b).unwrap_or(pos_b);

                        let center_a_c = pos_a_curr + offset_a;
                        let center_b_c = pos_b_curr + offset_b;

                        let min_a_c = center_a_c - half_a;
                        let max_a_c = center_a_c + half_a;
                        let min_b_c = center_b_c - half_b;
                        let max_b_c = center_b_c + half_b;

                        let overlap_x = max_a_c.x.min(max_b_c.x) - min_a_c.x.max(min_b_c.x);
                        let overlap_y = max_a_c.y.min(max_b_c.y) - min_a_c.y.max(min_b_c.y);
                        let overlap_z = max_a_c.z.min(max_b_c.z) - min_a_c.z.max(min_b_c.z);

                        if overlap_x > 0.0001 && overlap_y > 0.0001 && overlap_z > 0.0001 {
                            let (normal, depth) = if overlap_x < overlap_y && overlap_x < overlap_z
                            {
                                let n = if center_a_c.x < center_b_c.x {
                                    Vec3::NEG_X
                                } else {
                                    Vec3::X
                                };
                                (n, overlap_x)
                            } else if overlap_y < overlap_z {
                                let n = if center_a_c.y < center_b_c.y {
                                    Vec3::NEG_Y
                                } else {
                                    Vec3::Y
                                };
                                (n, overlap_y)
                            } else {
                                let n = if center_a_c.z < center_b_c.z {
                                    Vec3::NEG_Z
                                } else {
                                    Vec3::Z
                                };
                                (n, overlap_z)
                            };

                            let epsilon = 0.0001;

                            if anchored_a && !anchored_b {
                                if let Some(eb) = entity_manager.get_entity_mut(id_b) {
                                    eb.position -= normal * (depth + epsilon);
                                    let v_in = eb.physics.velocity.dot(-normal);
                                    if v_in < 0.0 {
                                        eb.physics.velocity -= (-normal) * v_in;
                                    }
                                }
                            } else if anchored_b && !anchored_a {
                                if let Some(ea) = entity_manager.get_entity_mut(id_a) {
                                    ea.position += normal * (depth + epsilon);
                                    let v_in = ea.physics.velocity.dot(normal);
                                    if v_in < 0.0 {
                                        ea.physics.velocity -= normal * v_in;
                                    }
                                }
                            } else if !anchored_a && !anchored_b {
                                let correction = normal * (depth * 0.5 + epsilon);

                                if let Some(ea) = entity_manager.get_entity_mut(id_a) {
                                    ea.position += correction;
                                    let v_in = ea.physics.velocity.dot(normal);
                                    if v_in < 0.0 {
                                        ea.physics.velocity -= normal * v_in;
                                    }
                                }
                                if let Some(eb) = entity_manager.get_entity_mut(id_b) {
                                    eb.position -= correction;
                                    let v_in = eb.physics.velocity.dot(-normal);
                                    if v_in < 0.0 {
                                        eb.physics.velocity -= (-normal) * v_in;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 3. Entity ↔ Character overlap and solid collision resolution
        if let Some(cs) = character_system {
            let character_snapshots: Vec<_> = cs
                .get_active_characters()
                .filter_map(|character| {
                    let entity_id = cs.get_entity_for_character(character.id)?;

                    Some((
                        entity_id,
                        character.transform.position,
                        character.collision.radius,
                        character.collision.height,
                    ))
                })
                .collect();

            for (char_entity_id, char_pos, radius, height) in character_snapshots {
                let char_min = char_pos - Vec3::new(radius, 0.0, radius);
                let char_max = char_pos + Vec3::new(radius, height, radius);
                let char_center = char_pos + Vec3::new(0.0, height * 0.5, 0.0);

                for &(entity_id, orig_pos, size, offset, solid, anchored, col_enabled) in
                    &physics_entities
                {
                    if entity_id == char_entity_id || !col_enabled {
                        continue;
                    }

                    let pos = entity_manager.get_position(entity_id).unwrap_or(orig_pos);
                    let half_e = size * 0.5;
                    let center_e = pos + offset;
                    let min_e = center_e - half_e;
                    let max_e = center_e + half_e;

                    if Self::aabb_overlap(min_e, max_e, char_min, char_max) {
                        raw_contacts.insert(EntityContactPair::new(entity_id, char_entity_id));

                        let character_is_anchored = entity_manager
                            .get_physics(char_entity_id)
                            .map(|physics| physics.enabled && physics.anchored)
                            .unwrap_or(false);

                        if solid && col_enabled && anchored && !character_is_anchored {
                            let overlap_x = char_max.x.min(max_e.x) - char_min.x.max(min_e.x);
                            let overlap_y = char_max.y.min(max_e.y) - char_min.y.max(min_e.y);
                            let overlap_z = char_max.z.min(max_e.z) - char_min.z.max(min_e.z);

                            if overlap_x > 0.0001 && overlap_y > 0.0001 && overlap_z > 0.0001 {
                                let (normal, depth) =
                                    if overlap_x < overlap_y && overlap_x < overlap_z {
                                        (
                                            if char_center.x < center_e.x {
                                                Vec3::NEG_X
                                            } else {
                                                Vec3::X
                                            },
                                            overlap_x,
                                        )
                                    } else if overlap_y < overlap_z {
                                        (
                                            if char_center.y < center_e.y {
                                                Vec3::NEG_Y
                                            } else {
                                                Vec3::Y
                                            },
                                            overlap_y,
                                        )
                                    } else {
                                        (
                                            if char_center.z < center_e.z {
                                                Vec3::NEG_Z
                                            } else {
                                                Vec3::Z
                                            },
                                            overlap_z,
                                        )
                                    };

                                if let Some(character) =
                                    cs.get_character_for_entity_mut(char_entity_id)
                                {
                                    const EPSILON: f32 = 0.0001;

                                    character.transform.position += normal * (depth + EPSILON);

                                    let inward_velocity = character.movement.velocity.dot(normal);

                                    if inward_velocity < 0.0 {
                                        character.movement.velocity -= normal * inward_velocity;
                                    }

                                    if normal.y > 0.5 {
                                        character.movement.is_grounded = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. Compute transitions
        let entered: HashSet<EntityContactPair> = raw_contacts
            .difference(&entity_manager.previous_contacts)
            .copied()
            .collect();

        let exited: HashSet<EntityContactPair> = entity_manager
            .previous_contacts
            .difference(&raw_contacts)
            .copied()
            .collect();

        entity_manager.previous_contacts = raw_contacts.clone();

        let result = EntityContactResult {
            current: raw_contacts,
            entered,
            exited,
        };

        entity_manager.last_contact_result = result.clone();
        result
    }
}
