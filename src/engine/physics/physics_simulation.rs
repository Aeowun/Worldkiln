// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::physics_body::PhysicsBodyId;
use super::physics_world::PhysicsWorld;
use crate::engine::entity::{EntityId, EntityManager};
use glam::Vec3;
use std::collections::HashSet;

#[derive(Default)]
struct ContactStageSummary {
    samples: u64,
    totals_us: [u64; 6],
    max_us: [u64; 6],
    counts: [u64; 6],
    cache_misses: u64,
}
static CONTACT_DIAG_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static CONTACT_DIAG_SUMMARY: std::sync::Mutex<ContactStageSummary> =
    std::sync::Mutex::new(ContactStageSummary {
        samples: 0,
        totals_us: [0; 6],
        max_us: [0; 6],
        counts: [0; 6],
        cache_misses: 0,
    });

fn emit_contact_stage_summary(us: [u64; 6], counts: [u64; 6], cache_miss: bool) {
    let mut diag = CONTACT_DIAG_SUMMARY.lock().unwrap();
    diag.samples += 1;
    for i in 0..6 {
        diag.totals_us[i] += us[i];
        diag.max_us[i] = diag.max_us[i].max(us[i]);
        diag.counts[i] += counts[i];
    }
    if cache_miss {
        diag.cache_misses += 1;
    }
    if diag.samples >= 120 {
        let n = diag.samples as f64;
        eprintln!(
            "[PHYSICS-CONTACT-DIAG] samples={} mean_us=[gather:{:.2} signatures:{:.2} broad:{:.2} pairs:{:.2} characters:{:.2} bookkeeping:{:.2}] max_us={:?} avg_counts=[entities:{:.1} static:{:.1} candidates:{:.1} pair_narrow_tests:{:.1} char_aabb:{:.1} char_narrow_tests:{:.1}] cache_miss_steps={}",
            diag.samples,
            diag.totals_us[0] as f64 / n,
            diag.totals_us[1] as f64 / n,
            diag.totals_us[2] as f64 / n,
            diag.totals_us[3] as f64 / n,
            diag.totals_us[4] as f64 / n,
            diag.totals_us[5] as f64 / n,
            diag.max_us,
            diag.counts[0] as f64 / n,
            diag.counts[1] as f64 / n,
            diag.counts[2] as f64 / n,
            diag.counts[3] as f64 / n,
            diag.counts[4] as f64 / n,
            diag.counts[5] as f64 / n,
            diag.cache_misses,
        );
        *diag = ContactStageSummary::default();
    }
}

pub const SIMULATION_DT: f32 = 1.0 / 60.0;

pub const MAX_PHYSICS_STEPS: u32 = 8;

pub const SLEEP_VELOCITY_THRESHOLD: f32 = 0.02;

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

const CHARACTER_GRID_CELL_STUDS: f32 = 16.0;
const CHARACTER_GRID_MAX_CELLS: i64 = 256;
fn character_grid_span(min: Vec3, max: Vec3) -> Option<(i32, i32, i32, i32)> {
    if !min.is_finite() || !max.is_finite() || min.x > max.x || min.z > max.z {
        return None;
    }
    let x0 = (min.x / CHARACTER_GRID_CELL_STUDS).floor();
    let x1 = (max.x / CHARACTER_GRID_CELL_STUDS).floor();
    let z0 = (min.z / CHARACTER_GRID_CELL_STUDS).floor();
    let z1 = (max.z / CHARACTER_GRID_CELL_STUDS).floor();
    if [x0, x1, z0, z1]
        .iter()
        .any(|&v| !v.is_finite() || v < i32::MIN as f32 + 1024.0 || v > i32::MAX as f32 - 1024.0)
    {
        return None;
    }
    let (x0, x1, z0, z1) = (x0 as i32, x1 as i32, z0 as i32, z1 as i32);
    let cells = (x1 as i64 - x0 as i64 + 1).checked_mul(z1 as i64 - z0 as i64 + 1)?;
    if cells <= 0 || cells > CHARACTER_GRID_MAX_CELLS {
        return None;
    }
    Some((x0, x1, z0, z1))
}

impl PhysicsWorld {
    pub fn step_simulation(&mut self, gravity: Vec3, dt: f32) {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(10, file!(), line!());

        let mut steps: usize = 1;
        for body in &self.bodies {
            if body.anchored || body.is_sleeping || !body.solid {
                continue;
            }
            let width = body.size.min_element().abs().max(0.05);
            let motion = (body.velocity.length() * dt + 0.5 * gravity.length() * dt * dt).max(0.0);
            steps = steps.max((motion / (width * 0.25)).ceil() as usize);
        }
        let steps = steps.min(32);
        let h = dt / steps as f32;
        for _ in 0..steps {
            let positions_before_step: Vec<Vec3> =
                self.bodies.iter().map(|body| body.position).collect();
            self.apply_gravity(gravity, h);
            self.integrate_positions(h);
            self.resolve_downward_static_crossings(&positions_before_step);
            for _ in 0..3 {
                self.resolve_static_collisions();
                self.resolve_dynamic_collisions();
                self.resolve_downward_static_crossings(&positions_before_step);
            }
            self.resolve_static_collisions();
            self.resolve_downward_static_crossings(&positions_before_step);
            self.refresh_dynamic_support();
            self.apply_resting_surface_friction(gravity, h);
        }
        self.update_sleeping(gravity);
        self.rebuild_dynamic_body_index();
    }

    pub fn apply_resting_surface_friction(&mut self, gravity: Vec3, dt: f32) {
        use super::rigid_contact::apply_surface_friction;
        for i in 0..self.bodies.len() {
            let body = &self.bodies[i];
            if body.anchored || body.is_sleeping || !body.solid || !body.gravity_participation {
                continue;
            }
            let mut support = None;
            if let Some(n) = body.static_contact_normal {
                if gravity.dot(n) < -1e-6 {
                    support = Some((
                        n,
                        self.anchored_support_friction(body.min_corner(), body.max_corner(), n),
                    ));
                }
            }
            if let Some((id, n)) = body.dynamic_contact {
                if gravity.dot(n) < -1e-6 {
                    if let Some(other) = self.bodies.iter().find(|b| b.id == id && b.solid) {
                        let combined = (body.friction * other.friction).max(0.0).sqrt();
                        support = Some((n, combined));
                    }
                }
            }
            if let Some((normal, friction)) = support {
                apply_surface_friction(&mut self.bodies[i].velocity, gravity, normal, friction, dt);
            }
        }
    }

    pub fn apply_gravity(&mut self, gravity: Vec3, dt: f32) {
        self.step_count += 1;

        for body in &mut self.bodies {
            if body.is_sleeping || body.anchored || !body.gravity_participation {
                continue;
            }

            let support_normal = body
                .static_contact_normal
                .or_else(|| body.dynamic_contact.map(|(_, normal)| normal));
            let mut acceleration = gravity;
            if let Some(normal) = support_normal.and_then(|normal| normal.try_normalize()) {
                let speed = body.velocity.dot(normal);
                let inward = acceleration.dot(normal);
                if inward < 0.0 && speed <= 0.0 {
                    acceleration -= normal * inward;
                    body.velocity -= normal * speed;
                }
            }
            body.velocity += acceleration * dt;
        }
    }

    pub fn integrate_positions(&mut self, dt: f32) {
        for body in &mut self.bodies {
            if !body.anchored && !body.is_sleeping {
                body.position += body.velocity * dt;
                if body.angular_velocity.length_squared() > 1e-10 {
                    let omega = body.angular_velocity.clamp_length_max(120.0);
                    body.rotation =
                        (glam::Quat::from_scaled_axis(omega * dt) * body.rotation).normalize();
                }
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
                let best_collision = if body.rotation.dot(glam::Quat::IDENTITY).abs() > 0.99999 {
                    self.anchored_aabb_contact(b_min, b_max)
                } else {
                    self.anchored_actor_contact(&body.collider(), body.center())
                };
                if let Some((normal, penetration)) = best_collision {
                    if penetration > 0.0001 {
                        use super::rigid_contact::{
                            AngularMotion, Motion, box_support_point, resolve_angular_with_friction,
                        };
                        let coefficient = (body.friction.max(0.0)
                            * self.anchored_support_friction(b_min, b_max, normal))
                            .sqrt();
                        let body = &mut self.bodies[i];
                        let mass = body.get_mass();
                        let mut moving = Motion {
                            position: body.position,
                            velocity: body.velocity,
                            inverse_mass: if mass.is_finite() && mass > 0.0 {
                                1.0 / mass
                            } else {
                                0.0
                            },
                        };
                        let mut static_surface = Motion {
                            position: Vec3::ZERO,
                            velocity: Vec3::ZERO,
                            inverse_mass: 0.0,
                        };
                        let center = body.center();
                        let mut angular = AngularMotion {
                            velocity: body.angular_velocity,
                            inverse_inertia: body.inverse_inertia(),
                            center,
                        };
                        let mut static_angular = AngularMotion::stationary();
                        let point =
                            box_support_point(center, body.size * 0.5, body.rotation, normal);
                        if resolve_angular_with_friction(
                            &mut moving,
                            &mut static_surface,
                            &mut angular,
                            &mut static_angular,
                            normal,
                            penetration,
                            point,
                            coefficient,
                        ) {
                            body.position = moving.position;
                            body.velocity = moving.velocity;
                            body.angular_velocity = angular.velocity;
                            latest_contact_normal = Some(normal);
                        }
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

            let current_min = body.min_corner();
            let current_max = body.max_corner();
            let previous_min = previous_position + (current_min - body.position);
            let previous_max = previous_position + (current_max - body.position);
            let swept_min = previous_min.min(current_min);
            let swept_max = previous_max.max(current_max);
            let mut crossed_surface: Option<f32> = None;
            for face in self.anchored_faces_in_aabb(swept_min, swept_max) {
                if face.normal.y < 0.5 {
                    continue;
                }
                let overlaps_x = swept_min.x < face.max.x && swept_max.x > face.min.x;
                let overlaps_z = swept_min.z < face.max.z && swept_max.z > face.min.z;
                if overlaps_x
                    && overlaps_z
                    && previous_min.y >= face.min.y - EPSILON
                    && current_min.y < face.min.y - EPSILON
                {
                    crossed_surface =
                        Some(crossed_surface.map_or(face.min.y, |top| top.max(face.min.y)));
                }
            }
            if let Some(surface_y) = crossed_surface {
                let body = &mut self.bodies[i];
                let lowest = body.min_corner().y;
                body.position.y += surface_y + EPSILON - lowest;
                if body.velocity.y < 0.0 {
                    body.velocity.y = 0.0;
                }
                body.static_contact_normal = Some(Vec3::Y);
            }
        }
    }

    pub fn resolve_dynamic_collisions(&mut self) {
        use super::rigid_contact::{
            AngularMotion, Motion, patch_center, resolve_angular_with_friction,
        };
        let count = self.bodies.len();
        for i in 0..count {
            for j in (i + 1)..count {
                let (first, second) = {
                    let (left, right) = self.bodies.split_at_mut(j);
                    (&mut left[i], &mut right[0])
                };
                if !first.solid || !second.solid || (first.anchored && second.anchored) {
                    continue;
                }
                let min_a = first.min_corner();
                let max_a = first.max_corner();
                let min_b = second.min_corner();
                let max_b = second.max_corner();
                if !Self::aabb_overlap_static(min_a, max_a, min_b, max_b) {
                    continue;
                }
                let first_collider = first.collider();
                let second_collider = second.collider();
                let Some(contact) = crate::engine::collision_geometry::contact(
                    Some(&first_collider),
                    first.center(),
                    min_a,
                    max_a,
                    Some(&second_collider),
                    second.center(),
                    min_b,
                    max_b,
                ) else {
                    continue;
                };
                let mass_a = first.get_mass();
                let mass_b = second.get_mass();
                let coefficient = (first.friction * second.friction).max(0.0).sqrt();
                let mut a = Motion {
                    position: first.position,
                    velocity: first.velocity,
                    inverse_mass: if mass_a.is_finite() && mass_a > 0.0 {
                        1.0 / mass_a
                    } else {
                        0.0
                    },
                };
                let mut b = Motion {
                    position: second.position,
                    velocity: second.velocity,
                    inverse_mass: if mass_b.is_finite() && mass_b > 0.0 {
                        1.0 / mass_b
                    } else {
                        0.0
                    },
                };
                let mut aa = AngularMotion {
                    velocity: first.angular_velocity,
                    inverse_inertia: first.inverse_inertia(),
                    center: first.center(),
                };
                let mut ab = AngularMotion {
                    velocity: second.angular_velocity,
                    inverse_inertia: second.inverse_inertia(),
                    center: second.center(),
                };
                let point = patch_center(min_a, max_a, min_b, max_b, contact.normal);
                if resolve_angular_with_friction(
                    &mut a,
                    &mut b,
                    &mut aa,
                    &mut ab,
                    contact.normal,
                    contact.depth,
                    point,
                    coefficient,
                ) {
                    first.angular_velocity = aa.velocity;
                    second.angular_velocity = ab.velocity;
                    let movement_a = (a.position - first.position).length_squared();
                    let movement_b = (b.position - second.position).length_squared();
                    first.position = a.position;
                    first.velocity = a.velocity;
                    second.position = b.position;
                    second.velocity = b.velocity;
                    if movement_a > 1e-4
                        || a.velocity.length_squared() > 1e-4
                        || aa.velocity.length_squared() > 1e-4
                    {
                        first.wake();
                    }
                    if movement_b > 1e-4
                        || b.velocity.length_squared() > 1e-4
                        || ab.velocity.length_squared() > 1e-4
                    {
                        second.wake();
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

        let frictionless_motion: Vec<bool> = self
            .bodies
            .iter()
            .map(|body| {
                if body.velocity.length_squared() <= 1e-10 {
                    return false;
                }
                if let Some(n) = body.static_contact_normal {
                    if self.anchored_support_friction(body.min_corner(), body.max_corner(), n)
                        <= 1e-6
                    {
                        return true;
                    }
                }
                if let Some((id, _)) = body.dynamic_contact {
                    if let Some(other) = self.bodies.iter().find(|other| other.id == id) {
                        return body.friction <= 1e-6 || other.friction <= 1e-6;
                    }
                }
                false
            })
            .collect();
        for (index, body) in self.bodies.iter_mut().enumerate() {
            if body.anchored {
                continue;
            }

            if body.is_sleeping {
                body.velocity = Vec3::ZERO;
                body.angular_velocity = Vec3::ZERO;
                continue;
            }
            if frictionless_motion[index] {
                body.sleep_timer = 0.0;
                continue;
            }

            if gravity.length_squared() <= 1e-8
                && (body.velocity.length_squared() > 1e-10
                    || body.angular_velocity.length_squared() > 1e-10)
            {
                body.sleep_timer = 0.0;
                continue;
            }
            if body.gravity_participation
                && gravity.length_squared() > 1e-8
                && (body.static_contact_normal.is_some() || body.dynamic_contact.is_some())
            {
                let local_up = body.rotation.conjugate() * (-gravity.normalize());
                if local_up.abs().max_element() < 0.995 {
                    body.sleep_timer = 0.0;
                    continue;
                }
            }
            if body.velocity.length() < SLEEP_VELOCITY_THRESHOLD
                && body.angular_velocity.length() < SLEEP_VELOCITY_THRESHOLD
            {
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
                        body.angular_velocity = Vec3::ZERO;
                    }
                } else {
                    body.sleep_timer = 0.0;
                }
            } else {
                body.sleep_timer = 0.0;
            }
        }
    }

    fn apply_actor_support_friction(
        physics: &mut crate::engine::entity::EntityPhysicsState,
        gravity: Vec3,
        normal: Vec3,
        friction: f32,
        dt: f32,
    ) {
        let sphere = physics.collider.as_ref().and_then(|c| {
            if c.shapes.len() != 1 {
                return None;
            }
            if let crate::engine::collision_geometry::CollisionShape::Sphere { radius, .. } =
                &c.shapes[0]
            {
                Some(radius.abs() * c.scale.abs().max_element())
            } else {
                None
            }
        });
        if let Some(radius) = sphere.filter(|r| *r > 0.0001) {
            let inverse_mass = 1.0 / physics.mass.max(0.001);
            let inv_moment = 2.5 * inverse_mass / (radius * radius);
            super::rigid_contact::apply_rolling_friction(
                &mut physics.velocity,
                &mut physics.angular_velocity,
                Vec3::splat(inv_moment),
                inverse_mass,
                gravity,
                normal,
                radius,
                friction,
                dt,
            );
        } else {
            super::rigid_contact::apply_surface_friction(
                &mut physics.velocity,
                gravity,
                normal,
                friction,
                dt,
            );
        }
    }

    pub fn step_entities(&self, entity_manager: &mut EntityManager, gravity: Vec3, dt: f32) {
        self.step_entities_excluding(entity_manager, gravity, dt, &HashSet::new());
    }

    pub fn step_entities_excluding(
        &self,
        entity_manager: &mut EntityManager,
        gravity: Vec3,
        dt: f32,
        excluded_entities: &HashSet<EntityId>,
    ) {
        self.step_entities_excluding_with_terrain(
            entity_manager,
            gravity,
            dt,
            excluded_entities,
            None,
        );
    }

    pub fn step_entities_excluding_with_terrain(
        &self,
        entity_manager: &mut EntityManager,
        gravity: Vec3,
        dt: f32,
        excluded_entities: &HashSet<EntityId>,
        terrain_world: Option<&crate::world::World>,
    ) {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(11, file!(), line!());

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
            let inverse_inertia = super::rigid_contact::inverse_box_inertia(
                physics.mass,
                physics.size,
                physics.anchored,
            );
            physics.angular_velocity += physics.accumulated_torque * inverse_inertia * dt;
            physics.accumulated_torque = Vec3::ZERO;
            let previous_position = entity.position;
            let collision_enabled = physics.collision_enabled && physics.solid;
            let step_width = (physics.size.min_element().abs() * 0.25).max(0.05);
            let travel = physics.velocity.length() * dt;
            let steps = if collision_enabled {
                ((travel / step_width).ceil() as usize).clamp(1, 32)
            } else {
                1
            };
            let h = dt / steps as f32;
            let mut grounded = false;
            for _ in 0..steps {
                entity.position += physics.velocity * h;
                if physics.angular_velocity.length_squared() > 1e-10 {
                    use glam::{EulerRot, Quat};
                    let old = Quat::from_euler(
                        EulerRot::XYZ,
                        entity.rotation.x,
                        entity.rotation.y,
                        entity.rotation.z,
                    );
                    let angular_step = physics.angular_velocity.clamp_length_max(120.0) * h;
                    let orientation = (Quat::from_scaled_axis(angular_step) * old).normalize();
                    let (x, y, z) = orientation.to_euler(EulerRot::XYZ);
                    entity.rotation = Vec3::new(x, y, z);
                    if let Some(collider) = physics.collider.as_mut() {
                        collider.rotation = entity.rotation;
                        if let Some((lo, hi)) =
                            crate::engine::collision_geometry::envelope(collider)
                        {
                            physics.size = (hi - lo).max(Vec3::splat(0.01));
                            physics.offset = (lo + hi) * 0.5;
                        }
                    }
                }
                if collision_enabled {
                    grounded |= Self::resolve_entity_static_collision(
                        entity.position,
                        physics,
                        &mut entity.position,
                        gravity,
                        self,
                    );
                }
            }
            if !collision_enabled {
                continue;
            }

            let half = physics.half_extents();
            let foot_offset = physics.offset - Vec3::Y * half.y;
            let radius = half.x.min(half.z).max(0.025);
            let start = previous_position + foot_offset;
            let end = entity.position + foot_offset;
            let terrain_hit = terrain_world
                .and_then(|world| self.sweep_terrain(world, start, end, radius, physics.size.y))
                .or_else(|| {
                    self.native_terrain_collider()
                        .map(|mesh| mesh.sweep_capsule(start, end, radius, physics.size.y))
                });
            let mut terrain_supported = false;
            if let Some((foot, contacts)) = terrain_hit {
                entity.position = foot - foot_offset;
                for contact in contacts {
                    let normal = contact.normal;
                    let into = physics.velocity.dot(normal).min(0.0);
                    physics.velocity -= normal * into;
                    if normal.y >= std::f32::consts::FRAC_1_SQRT_2 && gravity.dot(normal) < 0.0 {
                        grounded = true;
                        terrain_supported = true;
                        let coefficient = terrain_world
                            .map(|world| world.terrain.surface_friction_at(foot.x, foot.z))
                            .unwrap_or(crate::world::cell::DEFAULT_SURFACE_FRICTION);
                        let effective_gravity = gravity * physics.gravity_scale;
                        Self::apply_actor_support_friction(
                            physics,
                            effective_gravity,
                            normal,
                            coefficient,
                            dt,
                        );
                    }
                }
            }
            if grounded && !terrain_supported {
                let half = physics.half_extents();
                let center = entity.position + physics.offset;
                let normal = -gravity.normalize_or_zero();
                let friction = self.anchored_support_friction(center - half, center + half, normal);
                let effective_gravity = gravity * physics.gravity_scale;
                Self::apply_actor_support_friction(
                    physics,
                    effective_gravity,
                    normal,
                    friction,
                    dt,
                );
            }
            physics.grounded = grounded;
        }

        for entity in entity_manager.iter_mut() {
            if !entity.physics.enabled {
                continue;
            }

            if entity.physics.anchored {
                entity.physics.velocity = Vec3::ZERO;
                entity.physics.accumulated_force = Vec3::ZERO;
                entity.physics.angular_velocity = Vec3::ZERO;
                entity.physics.accumulated_torque = Vec3::ZERO;
                entity.physics.grounded = false;
            }
        }
    }

    pub fn resolve_mixed_collisions(
        &mut self,
        entity_manager: &mut EntityManager,
        excluded_entities: &HashSet<EntityId>,
        gravity: Vec3,
        dt: f32,
    ) {
        use super::rigid_contact::{
            AngularMotion, Motion, apply_surface_friction, patch_center,
            resolve_angular_with_friction,
        };
        if self.bodies.is_empty() {
            return;
        }
        let ids: Vec<_> = entity_manager
            .iter()
            .filter(|entity| {
                entity.enabled
                    && entity.physics.enabled
                    && entity.physics.solid
                    && entity.physics.collision_enabled
                    && !excluded_entities.contains(&entity.id)
            })
            .map(|entity| entity.id)
            .collect();
        for _ in 0..3 {
            self.rebuild_dynamic_body_index();
            for id in &ids {
                let Some(entity) = entity_manager.get_entity(*id) else {
                    continue;
                };
                let half = entity.physics.size * 0.5;
                let center = entity.position + entity.physics.offset;
                let min = center - half;
                let max = center + half;
                if !min.is_finite() || !max.is_finite() {
                    continue;
                }
                let candidates = self
                    .query_dynamic_bodies_in_aabb(min - Vec3::splat(0.05), max + Vec3::splat(0.05));
                for index in candidates {
                    let Some(entity) = entity_manager.get_entity(*id) else {
                        break;
                    };
                    if !entity.enabled
                        || !entity.physics.enabled
                        || !entity.physics.collision_enabled
                        || !entity.physics.solid
                    {
                        break;
                    }
                    let original = entity.position;
                    let half = entity.physics.size * 0.5;
                    let center = original + entity.physics.offset;
                    let min = center - half;
                    let max = center + half;
                    let collider = entity.physics.collider.clone();
                    let mass = entity.physics.mass;
                    let inverse_mass =
                        if entity.physics.anchored || !mass.is_finite() || mass <= 0.0 {
                            0.0
                        } else {
                            1.0 / mass
                        };
                    let mut actor = Motion {
                        position: original,
                        velocity: entity.physics.velocity,
                        inverse_mass,
                    };
                    let Some(cell) = self.bodies.get_mut(index) else {
                        continue;
                    };
                    if !cell.solid {
                        continue;
                    }
                    let cell_min = cell.min_corner();
                    let cell_max = cell.max_corner();
                    if !Self::aabb_overlap(min, max, cell_min, cell_max) {
                        continue;
                    }
                    let cell_collider = cell.collider();
                    let Some(contact) = crate::engine::collision_geometry::contact(
                        collider.as_ref(),
                        original,
                        min,
                        max,
                        Some(&cell_collider),
                        cell.center(),
                        cell_min,
                        cell_max,
                    ) else {
                        continue;
                    };
                    let cell_mass = cell.get_mass();
                    let mut block = Motion {
                        position: cell.position,
                        velocity: cell.velocity,
                        inverse_mass: if cell_mass.is_finite() && cell_mass > 0.0 {
                            1.0 / cell_mass
                        } else {
                            0.0
                        },
                    };
                    let mut actor_angular = AngularMotion {
                        velocity: entity.physics.angular_velocity,
                        inverse_inertia: super::rigid_contact::inverse_box_inertia(
                            mass,
                            entity.physics.size,
                            entity.physics.anchored,
                        ),
                        center,
                    };
                    let mut block_angular = AngularMotion {
                        velocity: cell.angular_velocity,
                        inverse_inertia: cell.inverse_inertia(),
                        center: cell.center(),
                    };
                    let point = patch_center(min, max, cell_min, cell_max, contact.normal);
                    if resolve_angular_with_friction(
                        &mut actor,
                        &mut block,
                        &mut actor_angular,
                        &mut block_angular,
                        contact.normal,
                        contact.depth,
                        point,
                        cell.friction,
                    ) {
                        if actor.inverse_mass > 0.0 && gravity.dot(contact.normal) < -1e-6 {
                            apply_surface_friction(
                                &mut actor.velocity,
                                gravity,
                                contact.normal,
                                cell.friction,
                                dt,
                            );
                        }
                        if block.inverse_mass > 0.0 && gravity.dot(-contact.normal) < -1e-6 {
                            apply_surface_friction(
                                &mut block.velocity,
                                gravity,
                                -contact.normal,
                                crate::world::cell::DEFAULT_SURFACE_FRICTION,
                                dt,
                            );
                        }
                        if actor.inverse_mass > 0.0 {
                            if let Some(e) = entity_manager.get_entity_mut(*id) {
                                e.position = actor.position;
                                e.physics.velocity = actor.velocity;
                                e.physics.angular_velocity = actor_angular.velocity;
                                if gravity.dot(contact.normal) < -0.001 {
                                    e.physics.grounded = true;
                                }
                            }
                        }
                        if block.inverse_mass > 0.0 {
                            let changed = (block.position - cell.position).length_squared() > 1e-8
                                || (block.velocity - cell.velocity).length_squared() > 1e-8;
                            cell.position = block.position;
                            cell.velocity = block.velocity;
                            cell.angular_velocity = block_angular.velocity;
                            if changed || cell.angular_velocity.length_squared() > 1e-8 {
                                cell.wake();
                            }
                        }
                    }
                }
            }
            self.resolve_static_collisions();
        }
        self.refresh_dynamic_support();
        self.rebuild_dynamic_body_index();
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

            let best_collision = if let Some(collider) = physics.collider.as_ref().filter(|c| {
                !c.shapes.is_empty()
                    && c.shapes.iter().all(|s| {
                        !matches!(
                            s,
                            crate::engine::collision_geometry::CollisionShape::Triangles { .. }
                        )
                    })
            }) {
                world.anchored_actor_contact(collider, *position)
            } else {
                world.anchored_aabb_contact(min_pos, max_pos)
            };
            let Some((normal, penetration)) = best_collision else {
                break;
            };

            if penetration <= 0.0001 {
                break;
            }

            use super::rigid_contact::{
                AngularMotion, Motion, box_support_point, resolve_angular_with_friction,
            };
            let coefficient = world.anchored_support_friction(min_pos, max_pos, normal);
            let mut moving = Motion {
                position: *position,
                velocity: physics.velocity,
                inverse_mass: if physics.mass.is_finite() && physics.mass > 0.0 {
                    1.0 / physics.mass
                } else {
                    0.0
                },
            };
            let mut static_surface = Motion {
                position: Vec3::ZERO,
                velocity: Vec3::ZERO,
                inverse_mass: 0.0,
            };
            let mut angular = AngularMotion {
                velocity: physics.angular_velocity,
                inverse_inertia: super::rigid_contact::inverse_box_inertia(
                    physics.mass,
                    physics.size,
                    physics.anchored,
                ),
                center: *position + physics.offset,
            };
            let mut static_angular = AngularMotion::stationary();
            let rotation = physics
                .collider
                .as_ref()
                .map(|shape| {
                    glam::Quat::from_euler(
                        glam::EulerRot::XYZ,
                        shape.rotation.x,
                        shape.rotation.y,
                        shape.rotation.z,
                    )
                })
                .unwrap_or(glam::Quat::IDENTITY);
            let point = box_support_point(angular.center, physics.size * 0.5, rotation, normal);
            if resolve_angular_with_friction(
                &mut moving,
                &mut static_surface,
                &mut angular,
                &mut static_angular,
                normal,
                penetration,
                point,
                coefficient,
            ) {
                *position = moving.position;
                physics.velocity = moving.velocity;
                physics.angular_velocity = angular.velocity;
                if gravity.length_squared() > 1e-6 && gravity.dot(normal) < -1e-3 {
                    grounded = true;
                }
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

    pub fn process_entity_contacts_and_collisions(
        &self,
        entity_manager: &mut EntityManager,
        character_system: Option<&mut crate::character::CharacterSystem>,
    ) -> crate::engine::entity::EntityContactResult {
        self.process_entity_contacts_impl(entity_manager, character_system, None)
    }
    pub fn process_entity_contacts_and_collisions_with_world(
        &self,
        entity_manager: &mut EntityManager,
        character_system: Option<&mut crate::character::CharacterSystem>,
        world: &crate::world::World,
    ) -> crate::engine::entity::EntityContactResult {
        self.process_entity_contacts_impl(entity_manager, character_system, Some(world))
    }
    fn process_entity_contacts_impl(
        &self,
        entity_manager: &mut EntityManager,
        character_system: Option<&mut crate::character::CharacterSystem>,
        world: Option<&crate::world::World>,
    ) -> crate::engine::entity::EntityContactResult {
        use crate::engine::entity::{EntityContactPair, EntityContactResult, EntityId};
        use std::collections::HashSet;

        let capture_detail = crate::engine::development_capture::detailed_active();
        let contact_console = *CONTACT_DIAG_ENABLED
            .get_or_init(|| std::env::var("WORLDKILN_PHYSICS_DIAG").as_deref() == Ok("1"));
        let contact_diag_on = contact_console || capture_detail;
        let mut pair_timings = crate::engine::development_capture::PairTimings::default();
        let mut contact_diag_checkpoint = std::time::Instant::now();
        let mut contact_diag_us = [0u64; 6];
        let mut contact_diag_counts = [0u64; 6];
        let mut confirmed_entity_contacts = 0u64;
        let mut confirmed_character_contacts = 0u64;
        macro_rules! contact_tick {
            ($index:expr) => {
                if contact_diag_on {
                    let now = std::time::Instant::now();
                    contact_diag_us[$index] =
                        now.duration_since(contact_diag_checkpoint).as_micros() as u64;
                    if capture_detail {
                        crate::engine::development_capture::measured(
                            "physics", ["contact_gather", "contact_cache", "contact_broadphase",
                                "contact_entity_pairs", "contact_characters", "contact_bookkeeping"][$index],
                            contact_diag_checkpoint, now.duration_since(contact_diag_checkpoint), None,
                            serde_json::json!({"counts": contact_diag_counts,
                                "count_order": ["physics_entities", "static_entities", "candidate_pairs", "entity_narrowphase_attempts", "character_candidates", "character_narrowphase_attempts"],
                                "confirmed_entity_contacts": confirmed_entity_contacts,
                                "confirmed_character_contacts": confirmed_character_contacts}),
                        );
                    }
                    contact_diag_checkpoint = now;
                }
            };
        }
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
        if contact_diag_on {
            contact_diag_counts[0] = n as u64;
        }
        contact_tick!(0);
        let character_entities: HashSet<EntityId> = character_system
            .as_ref()
            .map(|cs| cs.associated_entity_ids().collect())
            .unwrap_or_default();
        let mut current_static_signatures: Vec<_> = entity_manager
            .iter()
            .filter(|e| {
                e.enabled
                    && e.physics.enabled
                    && e.physics.collision_enabled
                    && e.physics.anchored
                    && !character_entities.contains(&e.id)
            })
            .map(|e| crate::engine::entity::StaticContactSignature {
                id: e.id,
                position: e.position,
                size: e.physics.size,
                offset: e.physics.offset,
                solid: e.physics.solid,
                collider: e.physics.collider.clone(),
            })
            .collect();
        current_static_signatures.sort_unstable_by_key(|item| item.id);
        let mut static_cache = std::mem::take(&mut entity_manager.static_contact_cache);
        let static_cache_valid = static_cache.signatures == current_static_signatures;
        if capture_detail {
            let now = std::time::Instant::now();
            crate::engine::development_capture::measured(
                "physics",
                "contact_cache_validity",
                now,
                std::time::Duration::ZERO,
                None,
                serde_json::json!({"valid": static_cache_valid}),
            );
        }
        let mut changed_static = HashSet::<EntityId>::new();
        if !static_cache_valid {
            let prior: std::collections::HashMap<_, _> =
                static_cache.signatures.iter().map(|s| (s.id, s)).collect();
            for signature in &current_static_signatures {
                if prior.get(&signature.id).is_none_or(|old| *old != signature) {
                    changed_static.insert(signature.id);
                }
            }
            let retained: HashSet<_> = current_static_signatures.iter().map(|s| s.id).collect();
            static_cache.contacts.retain(|pair| {
                retained.contains(&pair.first)
                    && retained.contains(&pair.second)
                    && !changed_static.contains(&pair.first)
                    && !changed_static.contains(&pair.second)
            });
            static_cache.signatures = current_static_signatures;
            static_cache.character_cells.clear();
            static_cache.character_large.clear();
            for signature in &static_cache.signatures {
                let center = signature.position + signature.offset;
                let half = signature.size * 0.5;
                let low = center - half;
                let high = center + half;
                if let Some((x0, x1, z0, z1)) = character_grid_span(low, high) {
                    for z in z0..=z1 {
                        for x in x0..=x1 {
                            static_cache
                                .character_cells
                                .entry((x, z))
                                .or_default()
                                .push(signature.id);
                        }
                    }
                } else {
                    static_cache.character_large.push(signature.id);
                }
            }
        }
        let mut raw_contacts: HashSet<EntityContactPair> = static_cache.contacts.clone();
        if contact_diag_on {
            contact_diag_counts[1] = static_cache.signatures.len() as u64;
        }
        contact_tick!(1);

        let mut bounds = Vec::with_capacity(n);
        for &(_, pos, size, offset, _, _, _) in &physics_entities {
            let center = pos + offset;
            bounds.push((center - size * 0.5, center + size * 0.5));
        }
        let mut center_min = Vec3::splat(f32::INFINITY);
        let mut center_max = Vec3::splat(f32::NEG_INFINITY);
        for &(min, max) in &bounds {
            let center = min * 0.5 + max * 0.5;
            center_min = center_min.min(center);
            center_max = center_max.max(center);
        }
        let spread = center_max - center_min;
        let axis = if spread.z > spread.x && spread.z >= spread.y {
            2
        } else if spread.y > spread.x {
            1
        } else {
            0
        };
        let component = |v: Vec3| match axis {
            1 => v.y,
            2 => v.z,
            _ => v.x,
        };
        let mut ordered: Vec<usize> = (0..n).collect();
        ordered
            .sort_unstable_by(|&i, &j| component(bounds[i].0).total_cmp(&component(bounds[j].0)));
        let mut active: Vec<usize> = Vec::new();
        let mut candidates: Vec<(usize, usize)> = Vec::new();
        const MAX_BROAD_PHASE_PAIRS: usize = 250_000;
        let mut exhaustive = bounds
            .iter()
            .any(|(min, max)| !min.is_finite() || !max.is_finite());
        if !exhaustive {
            for j in ordered {
                let (min_j, max_j) = bounds[j];
                active.retain(|&i| component(bounds[i].1) > component(min_j));
                if !physics_entities[j].6 {
                    continue;
                }
                for &i in &active {
                    if !physics_entities[i].6 {
                        continue;
                    }
                    if (static_cache_valid
                        || (!changed_static.contains(&physics_entities[i].0)
                            && !changed_static.contains(&physics_entities[j].0)))
                        && physics_entities[i].5
                        && physics_entities[j].5
                        && !character_entities.contains(&physics_entities[i].0)
                        && !character_entities.contains(&physics_entities[j].0)
                    {
                        continue;
                    }
                    let (min_i, max_i) = bounds[i];
                    if min_i.x < max_j.x
                        && max_i.x > min_j.x
                        && min_i.y < max_j.y
                        && max_i.y > min_j.y
                        && min_i.z < max_j.z
                        && max_i.z > min_j.z
                    {
                        candidates.push((i.min(j), i.max(j)));
                        if candidates.len() > MAX_BROAD_PHASE_PAIRS {
                            exhaustive = true;
                            break;
                        }
                    }
                }
                if exhaustive {
                    break;
                }
                active.push(j);
            }
        }
        if !exhaustive {
            candidates.sort_unstable();
        }
        if contact_diag_on {
            contact_diag_counts[2] = if exhaustive {
                n.saturating_mul(n.saturating_sub(1)) as u64 / 2
            } else {
                candidates.len() as u64
            };
        }
        contact_tick!(2);
        let pair_iter: Box<dyn Iterator<Item = (usize, usize)>> = if exhaustive {
            Box::new((0..n).flat_map(move |i| ((i + 1)..n).map(move |j| (i, j))))
        } else {
            Box::new(candidates.into_iter())
        };

        for (i, j) in pair_iter {
            let (id_a, pos_a, size_a, offset_a, solid_a, anchored_a, col_a) = physics_entities[i];
            let (id_b, pos_b, size_b, offset_b, solid_b, anchored_b, col_b) = physics_entities[j];
            let _pair_timer = pair_timings.measure(id_a.0, id_b.0, "entity_pair_visit");
            let both_static = anchored_a
                && anchored_b
                && col_a
                && col_b
                && !character_entities.contains(&id_a)
                && !character_entities.contains(&id_b);
            if both_static
                && (static_cache_valid
                    || (!changed_static.contains(&id_a) && !changed_static.contains(&id_b)))
            {
                continue;
            }

            let half_a = size_a * 0.5;
            let center_a = pos_a + offset_a;
            let min_a = center_a - half_a;
            let max_a = center_a + half_a;

            let half_b = size_b * 0.5;
            let center_b = pos_b + offset_b;
            let min_b = center_b - half_b;
            let max_b = center_b + half_b;

            let collision = if Self::aabb_overlap(min_a, max_a, min_b, max_b) {
                if contact_diag_on {
                    contact_diag_counts[3] += 1;
                }
                crate::engine::collision_geometry::contact(
                    entity_manager
                        .get_physics(id_a)
                        .and_then(|p| p.collider.as_ref()),
                    pos_a,
                    min_a,
                    max_a,
                    entity_manager
                        .get_physics(id_b)
                        .and_then(|p| p.collider.as_ref()),
                    pos_b,
                    min_b,
                    max_b,
                )
            } else {
                None
            };
            if let Some(first_contact) = collision {
                if col_a && col_b {
                    if contact_diag_on {
                        confirmed_entity_contacts += 1;
                    }
                    let pair = EntityContactPair::new(id_a, id_b);
                    raw_contacts.insert(pair);
                    if both_static {
                        static_cache.contacts.insert(pair);
                    }
                }

                if solid_a
                    && solid_b
                    && col_a
                    && col_b
                    && !character_entities.contains(&id_a)
                    && !character_entities.contains(&id_b)
                    && self.rapier_runtime.is_none()
                {
                    use super::rigid_contact::{
                        AngularMotion, Motion, patch_center, resolve_angular_with_friction,
                    };
                    let current_a = entity_manager.get_position(id_a).unwrap_or(pos_a);
                    let current_b = entity_manager.get_position(id_b).unwrap_or(pos_b);
                    let state_a = entity_manager.get_physics(id_a);
                    let state_b = entity_manager.get_physics(id_b);
                    let inv_a = state_a.map_or(0.0, |p| {
                        if anchored_a || !p.mass.is_finite() || p.mass <= 0.0 {
                            0.0
                        } else {
                            1.0 / p.mass
                        }
                    });
                    let inv_b = state_b.map_or(0.0, |p| {
                        if anchored_b || !p.mass.is_finite() || p.mass <= 0.0 {
                            0.0
                        } else {
                            1.0 / p.mass
                        }
                    });
                    let mut a = Motion {
                        position: current_a,
                        velocity: state_a.map_or(Vec3::ZERO, |p| p.velocity),
                        inverse_mass: inv_a,
                    };
                    let mut b = Motion {
                        position: current_b,
                        velocity: state_b.map_or(Vec3::ZERO, |p| p.velocity),
                        inverse_mass: inv_b,
                    };
                    let aa = state_a.and_then(|p| p.collider.as_ref());
                    let bb = state_b.and_then(|p| p.collider.as_ref());
                    let low_a = current_a + offset_a - size_a * 0.5;
                    let high_a = low_a + size_a;
                    let low_b = current_b + offset_b - size_b * 0.5;
                    let high_b = low_b + size_b;
                    if let Some(contact) = crate::engine::collision_geometry::contact(
                        aa, current_a, low_a, high_a, bb, current_b, low_b, high_b,
                    ) {
                        let mut aa = AngularMotion {
                            velocity: state_a.map_or(Vec3::ZERO, |p| p.angular_velocity),
                            inverse_inertia: state_a.map_or(Vec3::ZERO, |p| {
                                super::rigid_contact::inverse_box_inertia(
                                    p.mass, p.size, p.anchored,
                                )
                            }),
                            center: current_a + offset_a,
                        };
                        let mut ab = AngularMotion {
                            velocity: state_b.map_or(Vec3::ZERO, |p| p.angular_velocity),
                            inverse_inertia: state_b.map_or(Vec3::ZERO, |p| {
                                super::rigid_contact::inverse_box_inertia(
                                    p.mass, p.size, p.anchored,
                                )
                            }),
                            center: current_b + offset_b,
                        };
                        let point = patch_center(low_a, high_a, low_b, high_b, contact.normal);
                        if resolve_angular_with_friction(
                            &mut a,
                            &mut b,
                            &mut aa,
                            &mut ab,
                            contact.normal,
                            contact.depth,
                            point,
                            0.45,
                        ) {
                            if inv_a > 0.0 {
                                if let Some(e) = entity_manager.get_entity_mut(id_a) {
                                    e.position = a.position;
                                    e.physics.velocity = a.velocity;
                                    e.physics.angular_velocity = aa.velocity;
                                }
                            }
                            if inv_b > 0.0 {
                                if let Some(e) = entity_manager.get_entity_mut(id_b) {
                                    e.position = b.position;
                                    e.physics.velocity = b.velocity;
                                    e.physics.angular_velocity = ab.velocity;
                                }
                            }
                        }
                    }
                }
            }
        }

        contact_tick!(3);
        if let Some(cs) = character_system {
            let character_snapshots: Vec<_> = cs
                .get_active_characters()
                .filter_map(|character| {
                    let entity_id = cs.get_entity_for_character(character.id)?;

                    let (min, max) = character.collision_bounds();
                    let origin = cs
                        .collision_step_origins
                        .get(&character.id)
                        .copied()
                        .unwrap_or(character.transform.position);
                    let offset = origin - character.transform.position;
                    Some((
                        entity_id,
                        origin,
                        min.min(min + offset),
                        max.max(max + offset),
                        character.contact_collider()?,
                    ))
                })
                .collect();

            let physics_index: std::collections::HashMap<EntityId, usize> = physics_entities
                .iter()
                .enumerate()
                .map(|(i, e)| (e.0, i))
                .collect();
            let dynamic_indices: Vec<usize> = physics_entities
                .iter()
                .enumerate()
                .filter_map(|(i, e)| (!e.5 || character_entities.contains(&e.0)).then_some(i))
                .collect();
            for (char_entity_id, motion_origin, swept_char_min, swept_char_max, _initial_shape) in
                character_snapshots
            {
                let _contact_entity_span = crate::engine::development_capture::operation(
                    "physics",
                    "character_entity_contacts",
                    true,
                )
                .map(|span| span.entity(char_entity_id.0));
                let mut candidate_indices = dynamic_indices.clone();
                if let Some((x0, x1, z0, z1)) = character_grid_span(swept_char_min, swept_char_max)
                {
                    for z in z0..=z1 {
                        for x in x0..=x1 {
                            if let Some(ids) = static_cache.character_cells.get(&(x, z)) {
                                candidate_indices.extend(
                                    ids.iter().filter_map(|id| physics_index.get(id).copied()),
                                );
                            }
                        }
                    }
                    candidate_indices.extend(
                        static_cache
                            .character_large
                            .iter()
                            .filter_map(|id| physics_index.get(id).copied()),
                    );
                } else {
                    candidate_indices.extend(
                        static_cache
                            .signatures
                            .iter()
                            .filter_map(|s| physics_index.get(&s.id).copied()),
                    );
                }
                candidate_indices.sort_unstable();
                candidate_indices.dedup();
                for body_index in candidate_indices {
                    let _pair_timer = pair_timings.measure(
                        char_entity_id.0,
                        physics_entities[body_index].0.0,
                        "character_entity_pair_visit",
                    );
                    let Some(character) = cs.get_character_for_entity(char_entity_id) else {
                        continue;
                    };
                    let char_pos = character.transform.position;
                    let (char_min, char_max) = character.collision_bounds();
                    let Some(char_shape) = character.contact_collider() else {
                        continue;
                    };
                    let (entity_id, orig_pos, size, offset, solid, anchored, col_enabled) =
                        physics_entities[body_index];
                    if contact_diag_on {
                        contact_diag_counts[4] += 1;
                    }
                    if entity_id == char_entity_id || !col_enabled {
                        continue;
                    }
                    if anchored
                        && !character_entities.contains(&entity_id)
                        && !Self::aabb_overlap(
                            bounds[body_index].0,
                            bounds[body_index].1,
                            swept_char_min,
                            swept_char_max,
                        )
                    {
                        continue;
                    }
                    let pos = entity_manager.get_position(entity_id).unwrap_or(orig_pos);
                    let half_e = size * 0.5;
                    let center_e = pos + offset;
                    let min_e = center_e - half_e;
                    let max_e = center_e + half_e;

                    let character_solid = cs
                        .get_character_for_entity(char_entity_id)
                        .is_some_and(|c| c.collision_solid);
                    if anchored
                        && solid
                        && col_enabled
                        && character_solid
                        && !character_entities.contains(&entity_id)
                        && (char_pos - motion_origin).length_squared() > 1e-10
                    {
                        let obstacle = entity_manager
                            .get_physics(entity_id)
                            .and_then(|p| p.collider.as_ref());
                        if let Some((stopped, impact)) =
                            crate::engine::collision_geometry::sweep_character_wall(
                                &char_shape,
                                motion_origin,
                                char_pos,
                                obstacle,
                                pos,
                                min_e,
                                max_e,
                            )
                        {
                            if let Some(character) = cs.get_character_for_entity_mut(char_entity_id)
                            {
                                let previous = character.transform.position;
                                character.transform.position = stopped;
                                let into = character.movement.velocity.dot(impact.normal).min(0.0);
                                character.movement.velocity -= impact.normal * into;
                                let slide = character
                                    .movement
                                    .slope_slide_velocity
                                    .dot(impact.normal)
                                    .min(0.0);
                                character.movement.slope_slide_velocity -= impact.normal * slide;
                                let mut touched = HashSet::new();
                                crate::character::collision::resolve_static_voxel_collisions(
                                    character,
                                    self,
                                    &mut touched,
                                    previous,
                                );
                                if let Some(world) = world {
                                    if let Some((position, _)) =
                                        crate::character::collider::sweep_terrain(
                                            self,
                                            world,
                                            character,
                                            previous,
                                            character.transform.position,
                                        )
                                    {
                                        character.transform.position = position;
                                        crate::character::collision::resolve_static_voxel_collisions(
                                            character,self,&mut touched,previous);
                                    }
                                }
                            }
                            if let Some(now) = cs
                                .get_character_for_entity(char_entity_id)
                                .map(|c| c.transform.position)
                            {
                                let body = entity_manager
                                    .get_physics(entity_id)
                                    .and_then(|p| p.collider.as_ref());
                                if let Some(y) =
                                    crate::engine::collision_geometry::walkable_mesh_support(
                                        &char_shape,
                                        motion_origin,
                                        now,
                                        body,
                                        pos,
                                    )
                                {
                                    if let Some(character) =
                                        cs.get_character_for_entity_mut(char_entity_id)
                                    {
                                        if character.movement.velocity.y <= 0.0 {
                                            character.transform.position.y = y;
                                            character.movement.velocity.y = 0.0;
                                            character.movement.is_grounded = true;
                                        }
                                    }
                                }
                            }
                            raw_contacts.insert(EntityContactPair::new(entity_id, char_entity_id));
                            continue;
                        }
                    }

                    let contact = if Self::aabb_overlap(min_e, max_e, char_min, char_max) {
                        if contact_diag_on {
                            contact_diag_counts[5] += 1;
                        }
                        crate::engine::collision_geometry::contact(
                            entity_manager
                                .get_physics(entity_id)
                                .and_then(|p| p.collider.as_ref()),
                            pos,
                            min_e,
                            max_e,
                            Some(&char_shape),
                            char_pos,
                            char_min,
                            char_max,
                        )
                    } else {
                        None
                    };
                    if let Some(character_contact) = contact {
                        if contact_diag_on {
                            confirmed_character_contacts += 1;
                        }
                        raw_contacts.insert(EntityContactPair::new(entity_id, char_entity_id));

                        let character_is_anchored = entity_manager
                            .get_physics(char_entity_id)
                            .map(|physics| physics.enabled && physics.anchored)
                            .unwrap_or(false);

                        let character_solid = cs
                            .get_character_for_entity(char_entity_id)
                            .is_some_and(|c| c.collision_solid);
                        if solid
                            && col_enabled
                            && character_solid
                            && !character_entities.contains(&entity_id)
                        {
                            let normal = -character_contact.normal;
                            let depth = character_contact.depth;
                            let character_mass = entity_manager
                                .get_physics(char_entity_id)
                                .map_or(1.0, |p| p.mass.max(0.1));
                            let entity_mass = entity_manager
                                .get_physics(entity_id)
                                .map_or(1.0, |p| p.mass.max(0.1));
                            let char_inverse = if character_is_anchored {
                                0.0
                            } else {
                                1.0 / character_mass
                            };
                            let entity_inverse = if anchored || self.rapier_runtime.is_some() { 0.0 } else { 1.0 / entity_mass };
                            use super::rigid_contact::{Motion, resolve};
                            let mut character_motion = Motion {
                                position: char_pos,
                                velocity: cs
                                    .get_character_for_entity(char_entity_id)
                                    .map_or(Vec3::ZERO, |c| c.movement.velocity),
                                inverse_mass: char_inverse,
                            };
                            let mut entity_motion = Motion {
                                position: pos,
                                velocity: entity_manager
                                    .get_physics(entity_id)
                                    .map_or(Vec3::ZERO, |p| p.velocity),
                                inverse_mass: entity_inverse,
                            };
                            if resolve(&mut character_motion, &mut entity_motion, normal, depth) {
                                if char_inverse > 0.0 {
                                    if let Some(character) =
                                        cs.get_character_for_entity_mut(char_entity_id)
                                    {
                                        let previous = character.transform.position;
                                        character.transform.position = character_motion.position;
                                        let mut touched = HashSet::new();
                                        crate::character::collision::resolve_static_voxel_collisions(character,self,&mut touched,previous);
                                        if let Some(world) = world {
                                            if let Some((position, _)) =
                                                crate::character::collider::sweep_terrain(
                                                    self,
                                                    world,
                                                    character,
                                                    previous,
                                                    character.transform.position,
                                                )
                                            {
                                                character.transform.position = position;
                                                crate::character::collision::resolve_static_voxel_collisions(character,self,&mut touched,previous);
                                            }
                                        }
                                        character.movement.velocity = character_motion.velocity;
                                        if normal.y > 0.5 {
                                            character.movement.is_grounded = true;
                                        }
                                    }
                                }
                                if entity_inverse > 0.0 {
                                    if let Some(entity) = entity_manager.get_entity_mut(entity_id) {
                                        let previous = entity.position;
                                        entity.position = entity_motion.position;
                                        entity.physics.velocity = entity_motion.velocity;
                                        Self::resolve_entity_static_collision(
                                            previous,
                                            &mut entity.physics,
                                            &mut entity.position,
                                            Vec3::NEG_Y,
                                            self,
                                        );
                                        if let Some(world) = world {
                                            let half = entity.physics.size * 0.5;
                                            let offset = entity.physics.offset - Vec3::Y * half.y;
                                            if let Some((foot, _)) = self.sweep_terrain(
                                                world,
                                                previous + offset,
                                                entity.position + offset,
                                                half.x.max(half.z),
                                                entity.physics.size.y,
                                            ) {
                                                entity.position = foot - offset;
                                                Self::resolve_entity_static_collision(
                                                    previous,
                                                    &mut entity.physics,
                                                    &mut entity.position,
                                                    Vec3::NEG_Y,
                                                    self,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if anchored
                        && solid
                        && character_solid
                        && !character_entities.contains(&entity_id)
                    {
                        let now = cs
                            .get_character_for_entity(char_entity_id)
                            .map(|c| c.transform.position);
                        if let Some(now) = now {
                            let body = entity_manager
                                .get_physics(entity_id)
                                .and_then(|p| p.collider.as_ref());
                            if let Some(y) =
                                crate::engine::collision_geometry::walkable_mesh_support(
                                    &char_shape,
                                    motion_origin,
                                    now,
                                    body,
                                    pos,
                                )
                            {
                                if let Some(character) =
                                    cs.get_character_for_entity_mut(char_entity_id)
                                {
                                    if character.movement.velocity.y <= 0.0 {
                                        character.transform.position.y = y;
                                        character.movement.velocity.y = 0.0;
                                        character.movement.is_grounded = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            const RESTING_CHARACTER_CONTACT_SKIN: f32 = 0.01;
            for pair in &entity_manager.previous_contacts {
                if raw_contacts.contains(pair)
                    || !character_entities.contains(&pair.first)
                    || !character_entities.contains(&pair.second)
                {
                    continue;
                }
                if ![pair.first, pair.second].iter().all(|id| {
                    entity_manager.get_entity(*id).is_some_and(|entity| {
                        entity.enabled && entity.physics.enabled && entity.physics.collision_enabled
                    })
                }) {
                    continue;
                }
                let Some(first) = cs.get_character_for_entity(pair.first) else {
                    continue;
                };
                let Some(second) = cs.get_character_for_entity(pair.second) else {
                    continue;
                };
                let Some(first_shape) = first.contact_collider() else {
                    continue;
                };
                let Some(second_shape) = second.contact_collider() else {
                    continue;
                };
                let (lo_a, hi_a) = first.collision_bounds();
                let (lo_b, hi_b) = second.collision_bounds();
                let skin = Vec3::splat(RESTING_CHARACTER_CONTACT_SKIN);
                if !lo_a.cmple(hi_b + skin).all() || !lo_b.cmple(hi_a + skin).all() {
                    continue;
                }
                if crate::engine::collision_geometry::within_resting_contact_skin(
                    &first_shape,
                    first.transform.position,
                    &second_shape,
                    second.transform.position,
                    RESTING_CHARACTER_CONTACT_SKIN,
                ) {
                    raw_contacts.insert(*pair);
                }
            }
        }

        contact_tick!(4);
        if let Some(solver) = self.rapier_runtime.as_ref() {
            raw_contacts.retain(|pair| {
                if character_entities.contains(&pair.first)
                    || character_entities.contains(&pair.second)
                {
                    return true;
                }
                let anchored_a = entity_manager
                    .get_physics(pair.first)
                    .is_some_and(|p| p.anchored);
                let anchored_b = entity_manager
                    .get_physics(pair.second)
                    .is_some_and(|p| p.anchored);
                anchored_a && anchored_b
            });
            for (a, b) in solver.active_actor_contacts() {
                raw_contacts.insert(EntityContactPair::new(EntityId(a), EntityId(b)));
            }
        }
        entity_manager.static_contact_cache = static_cache;

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
        contact_tick!(5);
        if contact_console {
            emit_contact_stage_summary(contact_diag_us, contact_diag_counts, !static_cache_valid);
        }
        if capture_detail {
            let visits = pair_timings.visits;
            pair_timings.emit(|first, second| {
                let describe = |id| entity_manager.get_entity(EntityId(id)).map(|e|
                    serde_json::json!({"runtime_entity": id, "name": e.name, "package": e.package_name,
                        "physics_enabled": e.physics.enabled, "collision_enabled": e.physics.collision_enabled,
                        "solid": e.physics.solid, "anchored": e.physics.anchored}));
                serde_json::json!({"first": describe(first), "second": describe(second),
                    "second_runtime_entity": second, "timed_pair_visits": visits,
                    "retention": "top eight visits per fixed step; not all pairs"})
            });
        }
        result
    }
}

#[cfg(test)]
mod static_actor_pair_scaling_tests {
    use super::*;

    fn make_anchored_scene(count: usize, spacing: f32) -> EntityManager {
        let mut manager = EntityManager::new();
        for index in 0..count {
            let package = if index % 5 == 0 {
                "Props/Barrel"
            } else if index % 3 == 0 {
                "Trees/Palm"
            } else {
                "Trees/Oak"
            };
            let id = manager.create_entity(&format!("{package}_{index}"));
            let entity = manager.get_entity_mut(id).unwrap();
            entity.enabled = true;
            entity.physics.enabled = true;
            entity.physics.solid = true;
            entity.physics.anchored = true;
            entity.physics.collision_enabled = true;
            entity.physics.size = Vec3::new(2.0, 8.0, 2.0);
            entity.position = Vec3::new(index as f32 * spacing, 0.0, 0.0);
        }
        manager
    }

    #[test]
    #[ignore = "manual anchored tree/prop Play Mode physics scaling benchmark"]
    fn benchmark_anchored_static_actor_contacts() {
        use std::time::Instant;
        let physics = PhysicsWorld::new();
        for count in [10, 50, 200, 500] {
            let mut manager = make_anchored_scene(count, 4.0);
            let mut samples = Vec::new();
            for _ in 0..20 {
                let started = Instant::now();
                let result = physics.process_entity_contacts_and_collisions(&mut manager, None);
                samples.push(started.elapsed().as_secs_f64() * 1000.0);
                assert!(result.current.is_empty());
            }
            samples.sort_by(|a, b| a.total_cmp(b));
            println!(
                "DEV005 PLAY_ANCHORED n={count} pairs={} p50_ms={:.4} p95_ms={:.4} max_ms={:.4}",
                count * (count - 1) / 2,
                samples[9],
                samples[18],
                samples[19]
            );
        }
    }

    #[test]
    fn stationary_contact_cache_invalidates_when_source_collider_geometry_changes() {
        use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
        use crate::engine::entity::EntityContactPair;
        use std::sync::Arc;
        let mut manager = EntityManager::new();
        let a = manager.create_entity("A");
        let b = manager.create_entity("B");
        for id in [a, b] {
            let e = manager.get_entity_mut(id).unwrap();
            e.enabled = true;
            e.physics.enabled = true;
            e.physics.anchored = true;
            e.physics.size = Vec3::splat(2.0);
            e.physics.collider = Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Box {
                    min: [-0.4, -0.4, -0.4],
                    max: [0.4, 0.4, 0.4],
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            });
        }
        let physics = PhysicsWorld::new();
        let pair = EntityContactPair::new(a, b);
        assert!(
            physics
                .process_entity_contacts_and_collisions(&mut manager, None)
                .entered
                .contains(&pair)
        );
        assert!(
            physics
                .process_entity_contacts_and_collisions(&mut manager, None)
                .current
                .contains(&pair)
        );
        let collider = manager
            .get_entity_mut(b)
            .unwrap()
            .physics
            .collider
            .as_mut()
            .unwrap();
        *Arc::make_mut(&mut collider.shapes) = vec![CollisionShape::Box {
            min: [8.0, 8.0, 8.0],
            max: [9.0, 9.0, 9.0],
        }];
        assert!(
            physics
                .process_entity_contacts_and_collisions(&mut manager, None)
                .exited
                .contains(&pair)
        );
    }

    #[test]
    fn stationary_contact_cache_preserves_enter_exit_and_invalidates_on_change() {
        use crate::engine::entity::EntityContactPair;
        let physics = PhysicsWorld::new();
        let mut manager = EntityManager::new();
        let a = manager.create_entity("StaticA");
        let b = manager.create_entity("StaticB");
        let moving = manager.create_entity("Moving");
        for id in [a, b, moving] {
            let body = manager.get_entity_mut(id).unwrap();
            body.enabled = true;
            body.physics.enabled = true;
            body.physics.collision_enabled = true;
            body.physics.solid = true;
            body.physics.anchored = id != moving;
            body.physics.size = Vec3::splat(2.0);
            body.position = Vec3::ZERO;
        }
        manager.get_entity_mut(moving).unwrap().position = Vec3::new(10.0, 0.0, 0.0);
        let first = physics.process_entity_contacts_and_collisions(&mut manager, None);
        let pair_ab = EntityContactPair::new(a, b);
        assert!(first.entered.contains(&pair_ab));
        assert_eq!(manager.static_contact_cache.contacts.len(), 1);
        let repeated = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(repeated.current.contains(&pair_ab));
        assert!(repeated.entered.is_empty());
        assert!(repeated.exited.is_empty());
        manager.get_entity_mut(moving).unwrap().position = Vec3::new(0.5, 0.0, 0.0);
        let entered_dynamic = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(
            entered_dynamic
                .entered
                .contains(&EntityContactPair::new(a, moving))
        );
        assert!(entered_dynamic.current.contains(&pair_ab));
        manager.get_entity_mut(b).unwrap().position = Vec3::new(6.0, 0.0, 0.0);
        let exited = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(exited.exited.contains(&pair_ab));
        assert!(!exited.current.contains(&pair_ab));
        manager.get_entity_mut(b).unwrap().position = Vec3::ZERO;
        let reentered = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(reentered.entered.contains(&pair_ab));
        manager.get_entity_mut(b).unwrap().physics.collision_enabled = false;
        let disabled = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(disabled.exited.contains(&pair_ab));
        assert!(!disabled.current.contains(&pair_ab));
    }

    #[test]
    fn anchored_forest_along_z_preserves_contacts_and_avoids_false_pairs() {
        let physics = PhysicsWorld::new();
        let mut manager = make_anchored_scene(256, 4.0);
        for entity in manager.iter_mut() {
            entity.position = Vec3::new(0.0, 0.0, entity.position.x);
        }
        let separated = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(separated.current.is_empty());
        manager.get_entity_mut(EntityId(2)).unwrap().position = Vec3::ZERO;
        let touching = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert_eq!(touching.entered.len(), 1);
        assert!(
            touching
                .current
                .contains(&crate::engine::entity::EntityContactPair::new(
                    EntityId(1),
                    EntityId(2)
                ))
        );
        manager.get_entity_mut(EntityId(2)).unwrap().position.z = 4.0;
        let moved = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(moved.current.is_empty());
        assert_eq!(moved.exited.len(), 1);
    }

    #[test]
    #[ignore = "manual z-axis forest contact scaling benchmark"]
    fn benchmark_anchored_z_axis_forest_contacts() {
        use std::time::Instant;
        let physics = PhysicsWorld::new();
        for count in [50, 200, 500] {
            let mut manager = make_anchored_scene(count, 4.0);
            for entity in manager.iter_mut() {
                entity.position = Vec3::new(0.0, 0.0, entity.position.x);
            }
            let mut durations = Vec::with_capacity(20);
            for _ in 0..20 {
                let started = Instant::now();
                assert!(
                    physics
                        .process_entity_contacts_and_collisions(&mut manager, None)
                        .current
                        .is_empty()
                );
                durations.push(started.elapsed().as_secs_f64() * 1000.0);
            }
            durations.sort_by(|a, b| a.total_cmp(b));
            println!(
                "DEV005 PLAY_ANCHORED_Z n={count} p50_ms={:.4} p95_ms={:.4} max_ms={:.4}",
                durations[9], durations[18], durations[19]
            );
        }
    }

    #[test]
    fn broadphase_contacts_match_exhaustive_with_mixed_tree_prop_positions() {
        use crate::engine::entity::EntityContactPair;
        use std::collections::HashSet;
        let physics = PhysicsWorld::new();
        let mut manager = EntityManager::new();
        for i in 0..128usize {
            let id = manager.create_entity(&format!("TreeOrProp_{i}"));
            let entity = manager.get_entity_mut(id).unwrap();
            entity.enabled = true;
            entity.physics.enabled = true;
            entity.physics.anchored = i % 9 != 0;
            entity.physics.solid = i % 6 != 0;
            entity.physics.collision_enabled = i % 7 != 0;
            entity.physics.size = Vec3::new(
                1.0 + (i % 4) as f32,
                2.0 + (i % 3) as f32,
                1.0 + (i % 5) as f32,
            );
            entity.position = Vec3::new(
                ((i * 13) % 29) as f32 * 1.3,
                ((i * 11) % 7) as f32,
                ((i * 17) % 31) as f32 * 1.1,
            );
        }
        let snapshots: Vec<_> = manager
            .iter()
            .filter(|e| e.enabled && e.physics.enabled && e.physics.collision_enabled)
            .map(|e| {
                (
                    e.id,
                    e.position + e.physics.offset - e.physics.size * 0.5,
                    e.position + e.physics.offset + e.physics.size * 0.5,
                )
            })
            .collect();
        let mut expected = HashSet::new();
        for i in 0..snapshots.len() {
            for j in (i + 1)..snapshots.len() {
                let (a, amin, amax) = snapshots[i];
                let (b, bmin, bmax) = snapshots[j];
                if PhysicsWorld::aabb_overlap(amin, amax, bmin, bmax) {
                    expected.insert(EntityContactPair::new(a, b));
                }
            }
        }
        let actual = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert_eq!(actual.current, expected);
        assert_eq!(actual.entered, expected);
        assert!(actual.exited.is_empty());
    }

    #[test]
    fn anchored_tree_pushes_moving_prop_but_not_other_anchored_props() {
        let physics = PhysicsWorld::new();
        let mut manager = EntityManager::new();
        let tree = manager.create_entity("Tree");
        let prop = manager.create_entity("MovingProp");
        for id in [tree, prop] {
            let entity = manager.get_entity_mut(id).unwrap();
            entity.enabled = true;
            entity.physics.enabled = true;
            entity.physics.solid = true;
            entity.physics.collision_enabled = true;
            entity.physics.size = Vec3::new(2.0, 2.0, 2.0);
            entity.position = Vec3::ZERO;
            entity.physics.anchored = id == tree;
        }
        let before = manager.get_position(tree).unwrap();
        let report = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert_eq!(report.current.len(), 1);
        assert_eq!(manager.get_position(tree).unwrap(), before);
        assert_ne!(manager.get_position(prop).unwrap(), before);
    }

    #[test]
    fn anchored_static_overlaps_retain_contact_transitions() {
        let physics = PhysicsWorld::new();
        let mut manager = make_anchored_scene(3, 0.5);
        let first = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert_eq!(first.current.len(), 3);
        assert_eq!(first.entered.len(), 3);
        let second = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert_eq!(second.current.len(), 3);
        assert!(second.entered.is_empty());
        manager
            .iter_mut()
            .for_each(|entity| entity.position.x += entity.id.0 as f32 * 10.0);
        let third = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(third.current.is_empty());
        assert_eq!(third.exited.len(), 3);
    }
}

#[cfg(test)]
mod swept_static_mesh_wall_tests {
    use super::*;
    use crate::character::CharacterSystem;
    use crate::engine::{
        collision_geometry::{ColliderInstance, CollisionShape},
        entity::EntityManager,
    };
    use crate::world::World;
    use std::sync::Arc;

    #[test]
    fn character_entity_contact_pass_blocks_a_crossed_thin_imported_wall() {
        let world = World::new();
        let physics = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let player = entities.create_entity("moving character");
        let wall = entities.create_entity("imported triangle wall");
        let from = Vec3::new(-2.0, 0.0, -1.0);
        let to = Vec3::new(2.0, 0.0, 1.0);
        let mut characters = CharacterSystem::new();
        let character_id = characters.spawn_character(from, None);
        characters.associate_entity(player, character_id);
        {
            let actor = characters.get_character_for_entity_mut(player).unwrap();
            actor.transform.position = to;
        }
        characters.collision_step_origins.insert(character_id, from);
        characters.sync_entity_positions(&mut entities);
        {
            let e = entities.get_entity_mut(wall).unwrap();
            e.position = Vec3::ZERO;
            e.physics.enabled = true;
            e.physics.collision_enabled = true;
            e.physics.solid = true;
            e.physics.anchored = true;
            e.physics.size = Vec3::new(0.002, 4.0, 6.0);
            e.physics.offset = Vec3::new(0.0, 1.0, 0.0);
            e.physics.collider = Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Triangles {
                    vertices: vec![
                        [[0.0, -1.0, -3.0], [0.0, 3.0, -3.0], [0.0, -1.0, 3.0]],
                        [[0.0, 3.0, -3.0], [0.0, 3.0, 3.0], [0.0, -1.0, 3.0]],
                    ],
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            });
        }
        physics.process_entity_contacts_and_collisions_with_world(
            &mut entities,
            Some(&mut characters),
            &world,
        );
        let actor = characters.get_character_for_entity(player).unwrap();
        assert!(
            actor.transform.position.x <= -0.399,
            "an imported triangle wall failed to block Character movement: {:?}",
            actor.transform.position
        );
        assert!(
            actor.transform.position.z > -0.8,
            "wall sweep lost tangential slide: {:?}",
            actor.transform.position
        );
    }
}

#[cfg(test)]
mod angular_runtime_integration_tests {
    use super::*;
    use crate::engine::physics::{PhysicsBody, PhysicsBodyId};

    #[test]
    fn off_center_cell_collision_rotates_both_simulation_and_runtime_transform() {
        let mut physics = PhysicsWorld::new();
        let mut pusher =
            PhysicsBody::new(PhysicsBodyId(1), 1, Vec3::new(-0.89, 0.35, 0.0), Vec3::ONE);
        pusher.velocity = Vec3::new(5.0, 0.0, 0.0);
        let target = PhysicsBody::new(PhysicsBodyId(2), 2, Vec3::ZERO, Vec3::ONE);
        physics.bodies.extend([pusher, target]);
        physics.resolve_dynamic_collisions();
        assert!(
            physics.bodies[1].angular_velocity.z.abs() > 0.01,
            "off-center impact must angularly accelerate unanchored Cell"
        );
        let before = physics.bodies[1].rotation;
        physics.integrate_positions(1.0 / 60.0);
        assert_ne!(
            physics.bodies[1].rotation, before,
            "Cell orientation must integrate its angular velocity"
        );
    }

    #[test]
    fn dynamic_actor_contact_rotates_actor_without_changing_authored_world() {
        let mut entities = EntityManager::new();
        let striker = entities.create_entity("Striker");
        let target = entities.create_entity("Target");
        {
            let e = entities.get_entity_mut(striker).unwrap();
            e.position = Vec3::new(-0.89, 0.65, 0.0);
            e.physics.enabled = true;
            e.physics.anchored = false;
            e.physics.velocity = Vec3::new(5.0, 0.0, 0.0);
        }
        {
            let e = entities.get_entity_mut(target).unwrap();
            e.physics.enabled = true;
            e.physics.anchored = false;
            e.physics.size = Vec3::new(1.0, 2.0, 1.0);
            e.physics.offset = Vec3::Y;
        }
        let world = PhysicsWorld::new();
        world.process_entity_contacts_and_collisions(&mut entities, None);
        let spin = entities
            .get_entity(target)
            .unwrap()
            .physics
            .angular_velocity;
        assert!(
            spin.z.abs() > 0.01,
            "impact transfers angular velocity to the target"
        );
        let before = entities.get_rotation(target).unwrap();
        world.step_entities_excluding(
            &mut entities,
            Vec3::ZERO,
            1.0 / 60.0,
            &std::collections::HashSet::new(),
        );
        assert_ne!(
            entities.get_rotation(target).unwrap(),
            before,
            "Actor must visibly rotate during the next physics step"
        );
    }
}

#[cfg(test)]
mod rigid_stability_regressions {
    use super::*;
    use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
    use crate::engine::physics::PhysicsBody;
    use glam::Quat;
    use std::sync::Arc;

    #[test]
    fn leaning_cell_feels_gravity_and_settles_toward_its_flat_face() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::ZERO);
        let mut cell = PhysicsBody::new(PhysicsBodyId(2), 2, Vec3::new(0.0, 1.01, 0.0), Vec3::ONE);
        cell.rotation = Quat::from_rotation_z(0.30);
        cell.friction = 0.01;
        world.bodies.push(cell);
        for _ in 0..10 {
            world.step_simulation(Vec3::NEG_Y * 9.81, SIMULATION_DT);
            assert!(
                world.bodies[0].min_corner().y >= 0.995,
                "rotated Cell corner must not penetrate the floor: {:?}",
                world.bodies[0]
            );
        }
        let (_, _, z) = world.bodies[0].rotation.to_euler(glam::EulerRot::XYZ);
        assert!(
            z.abs() < 0.30,
            "gravity at an offset support must restore the flat face, not leave the Cell floating: {z}"
        );
    }
    #[test]
    fn tilted_cell_crossing_floor_keeps_rotated_corners_above_plane() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::ZERO);
        let mut cell = PhysicsBody::new(PhysicsBodyId(2), 2, Vec3::new(0.0, 2.0, 0.0), Vec3::ONE);
        cell.rotation = Quat::from_rotation_z(0.5);
        cell.velocity = Vec3::new(0.0, -70.0, 0.0);
        world.bodies.push(cell);
        world.step_simulation(Vec3::ZERO, SIMULATION_DT);
        assert!(
            world.bodies[0].min_corner().y >= 0.998,
            "floor-crossing correction must use rotated support extent"
        );
    }
    #[test]
    fn rapid_cell_stops_at_thin_anchored_wall_instead_of_tunneling() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::new(2.0, 0.0, 0.0));
        let mut cell = PhysicsBody::new(PhysicsBodyId(2), 2, Vec3::ZERO, Vec3::ONE);
        cell.velocity = Vec3::new(210.0, 0.0, 0.0);
        cell.gravity_participation = false;
        world.bodies.push(cell);
        world.step_simulation(Vec3::ZERO, SIMULATION_DT);
        assert!(
            world.bodies[0].max_corner().x <= 2.005,
            "fast dynamic Cell crossed a thin wall: {:?}",
            world.bodies[0]
        );
    }
    #[test]
    fn friction_does_not_damp_box_rotation_directly() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::ZERO);
        let mut cell = PhysicsBody::new(PhysicsBodyId(2), 2, Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        cell.velocity = Vec3::new(3.0, 0.0, 0.0);
        cell.angular_velocity = Vec3::new(0.0, 0.0, 2.0);
        cell.static_contact_normal = Some(Vec3::Y);
        world.bodies.push(cell);
        world.apply_resting_surface_friction(Vec3::NEG_Y * 9.81, SIMULATION_DT);
        assert_eq!(world.bodies[0].angular_velocity, Vec3::new(0.0, 0.0, 2.0));
        assert!(
            world.bodies[0].velocity.x < 3.0,
            "sliding friction still slows translation"
        );
    }
    #[test]
    fn resting_box_can_sleep_without_gravity_correction_waking_it() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::ZERO);
        let body = PhysicsBody::new(PhysicsBodyId(2), 2, Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        world.bodies.push(body);
        for _ in 0..90 {
            world.step_simulation(Vec3::NEG_Y * 9.81, SIMULATION_DT);
        }
        assert!(
            world.bodies[0].is_sleeping,
            "static contact corrections must not wake a stationary box every frame"
        );
        assert!(world.bodies[0].min_corner().y >= 0.998);
    }

    #[test]
    fn stacked_dynamic_cells_remain_above_anchored_floor() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::ZERO);
        world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(2),
            2,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::ONE,
        ));
        world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(3),
            3,
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::ONE,
        ));
        for _ in 0..90 {
            world.step_simulation(Vec3::NEG_Y * 9.81, SIMULATION_DT);
        }
        assert!(
            world.bodies[0].min_corner().y >= 0.998,
            "lower dynamic box must not sink into the floor"
        );
        assert!(
            world.bodies[1].min_corner().y >= 1.9,
            "upper dynamic box must not sink through its support"
        );
        assert!(world.bodies.iter().all(|b| b.position.is_finite()
            && b.velocity.is_finite()
            && b.angular_velocity.is_finite()));
    }

    #[test]
    fn fast_sphere_actor_does_not_pass_anchored_wall() {
        let mut world = PhysicsWorld::new();
        world.add_static_collider(1, Vec3::new(2.0, 0.0, 0.0));
        let mut entities = EntityManager::new();
        let id = entities.create_entity("DynamicSphere");
        {
            let entity = entities.get_entity_mut(id).unwrap();
            entity.position = Vec3::new(0.5, 0.5, 0.5);
            entity.physics.enabled = true;
            entity.physics.anchored = false;
            entity.physics.solid = true;
            entity.physics.collision_enabled = true;
            entity.physics.mass = 1.0;
            entity.physics.size = Vec3::ONE;
            entity.physics.offset = Vec3::ZERO;
            entity.physics.velocity = Vec3::new(210.0, 0.0, 0.0);
            entity.physics.collider = Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Sphere {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.5,
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            });
        }
        world.step_entities(&mut entities, Vec3::ZERO, SIMULATION_DT);
        let x = entities.get_entity(id).unwrap().position.x;
        assert!(x <= 1.51, "sphere tunneled through immovable wall: x={x}");
    }
}
