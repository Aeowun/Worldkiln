// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Motion {
    pub position: Vec3,
    pub velocity: Vec3,
    pub inverse_mass: f32,
}

pub(crate) fn resolve(a: &mut Motion, b: &mut Motion, normal: Vec3, depth: f32) -> bool {
    resolve_with_friction(a, b, normal, depth, 0.45)
}

pub(crate) fn resolve_with_friction(
    a: &mut Motion,
    b: &mut Motion,
    normal: Vec3,
    depth: f32,
    coefficient: f32,
) -> bool {
    let total = a.inverse_mass + b.inverse_mass;
    if !normal.is_finite()
        || !depth.is_finite()
        || total <= 0.0
        || depth <= 0.0
        || normal.length_squared() <= 1e-10
    {
        return false;
    }
    let n = normal.normalize();
    let penetration = depth;
    let correction = n * (penetration / total);
    a.position += correction * a.inverse_mass;
    b.position -= correction * b.inverse_mass;

    let relative = a.velocity - b.velocity;
    let speed = relative.dot(n);
    if speed < 0.0 {
        
        let impulse = -speed / total;
        a.velocity += n * (impulse * a.inverse_mass);
        b.velocity -= n * (impulse * b.inverse_mass);

        let tangent_velocity = relative - n * speed;
        if tangent_velocity.length_squared() > 1e-10 {
            let tangent = tangent_velocity.normalize();
            let friction_impulse = (-tangent_velocity.length() / total).clamp(
                -coefficient.max(0.0) * impulse,
                coefficient.max(0.0) * impulse,
            );
            a.velocity += tangent * (friction_impulse * a.inverse_mass);
            b.velocity -= tangent * (friction_impulse * b.inverse_mass);
        }
    }
    true
}

pub(crate) fn box_support_point(
    center: Vec3,
    half: Vec3,
    rotation: glam::Quat,
    outward: Vec3,
) -> Vec3 {
    let Some(normal) = outward.try_normalize() else {
        return center;
    };
    let mut best = f32::INFINITY;
    let mut selected = [Vec3::ZERO; 8];
    for (i, p) in selected.iter_mut().enumerate() {
        let local = Vec3::new(
            if i & 1 == 0 { -half.x } else { half.x },
            if i & 2 == 0 { -half.y } else { half.y },
            if i & 4 == 0 { -half.z } else { half.z },
        );
        *p = center + rotation * local;
        best = best.min(p.dot(normal));
    }
    let mut sum = Vec3::ZERO;
    let mut count = 0u32;
    for p in selected {
        if p.dot(normal) <= best + 0.0001 {
            sum += p;
            count += 1;
        }
    }
    if count > 0 {
        sum / count as f32
    } else {
        center
    }
}

pub(crate) fn inverse_box_inertia(mass: f32, size: Vec3, anchored: bool) -> Vec3 {
    if anchored || !mass.is_finite() || mass <= 0.0 || !size.is_finite() {
        return Vec3::ZERO;
    }
    let m = mass / 12.0;
    let (x, y, z) = (
        size.x.abs().max(0.001),
        size.y.abs().max(0.001),
        size.z.abs().max(0.001),
    );
    Vec3::new(
        1.0 / (m * (y * y + z * z)),
        1.0 / (m * (x * x + z * z)),
        1.0 / (m * (x * x + y * y)),
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AngularMotion {
    pub velocity: Vec3,
    pub inverse_inertia: Vec3,
    pub center: Vec3,
}
impl AngularMotion {
    pub fn stationary() -> Self {
        Self {
            velocity: Vec3::ZERO,
            inverse_inertia: Vec3::ZERO,
            center: Vec3::ZERO,
        }
    }
}
pub(crate) fn patch_center(
    min_a: Vec3,
    max_a: Vec3,
    min_b: Vec3,
    max_b: Vec3,
    normal: Vec3,
) -> Vec3 {
    let lo = min_a.max(min_b);
    let hi = max_a.min(max_b);
    let center = (lo + hi) * 0.5;
    if !center.is_finite() {
        return (min_a + max_a + min_b + max_b) * 0.25;
    }
    let _ = normal;
    center
}
fn angular_term(ra: Vec3, rb: Vec3, n: Vec3, a: &AngularMotion, b: &AngularMotion) -> f32 {
    let ka = ra.cross(n) * a.inverse_inertia;
    let kb = rb.cross(n) * b.inverse_inertia;
    (ka.cross(ra) + kb.cross(rb)).dot(n).max(0.0)
}
fn impulse(
    a: &mut Motion,
    b: &mut Motion,
    aa: &mut AngularMotion,
    ab: &mut AngularMotion,
    ra: Vec3,
    rb: Vec3,
    j: Vec3,
) {
    a.velocity += j * a.inverse_mass;
    b.velocity -= j * b.inverse_mass;
    aa.velocity += (ra.cross(j)) * aa.inverse_inertia;
    ab.velocity -= (rb.cross(j)) * ab.inverse_inertia;
}

pub(crate) fn resolve_angular_with_friction(
    a: &mut Motion,
    b: &mut Motion,
    aa: &mut AngularMotion,
    ab: &mut AngularMotion,
    normal: Vec3,
    depth: f32,
    point: Vec3,
    coefficient: f32,
) -> bool {
    let total = a.inverse_mass + b.inverse_mass;
    if total <= 0.0
        || !normal.is_finite()
        || !point.is_finite()
        || !depth.is_finite()
        || depth <= 0.0
        || normal.length_squared() < 1e-10
    {
        return false;
    }
    let n = normal.normalize();
    let separation = n * (depth / total);
    a.position += separation * a.inverse_mass;
    b.position -= separation * b.inverse_mass;

    let ra = point - aa.center;
    let rb = point - ab.center;
    let rel = a.velocity + aa.velocity.cross(ra) - b.velocity - ab.velocity.cross(rb);
    let inward = rel.dot(n);
    if inward < -1e-6 {
        let effective_mass = total + angular_term(ra, rb, n, aa, ab);
        if effective_mass <= 1e-9 {
            return true;
        }
        
        let magnitude = -inward / effective_mass;
        impulse(a, b, aa, ab, ra, rb, n * magnitude);

        let vt = a.velocity + aa.velocity.cross(ra) - b.velocity - ab.velocity.cross(rb);
        let tangent = vt - n * vt.dot(n);
        if let Some(t) = tangent.try_normalize() {
            let denom = total + angular_term(ra, rb, t, aa, ab);
            let wanted = -tangent.length() / denom.max(1e-9);
            let cap = coefficient.max(0.0) * magnitude;
            impulse(a, b, aa, ab, ra, rb, t * wanted.clamp(-cap, cap));
        }
    }
    true
}

pub(crate) fn apply_rolling_friction(
    linear: &mut Vec3,
    angular: &mut Vec3,
    inverse_inertia: Vec3,
    inverse_mass: f32,
    gravity: Vec3,
    normal: Vec3,
    radius: f32,
    coefficient: f32,
    dt: f32,
) {
    if !linear.is_finite()
        || !angular.is_finite()
        || !gravity.is_finite()
        || !normal.is_finite()
        || !radius.is_finite()
        || !coefficient.is_finite()
        || inverse_mass <= 0.0
        || radius <= 0.0
        || dt <= 0.0
    {
        return;
    }
    let Some(n) = normal.try_normalize() else {
        return;
    };
    let load = (-gravity.dot(n)).max(0.0);
    if load <= 1e-6 || coefficient <= 0.0 {
        return;
    }
    let arm = -n * radius;
    let contact_velocity = *linear + angular.cross(arm);
    let slip = contact_velocity - n * contact_velocity.dot(n);
    if let Some(t) = slip.try_normalize() {
        let leverage = arm.cross(t);
        let denom = inverse_mass + (leverage * inverse_inertia).cross(arm).dot(t).max(0.0);
        if denom > 1e-9 {
            let mass = 1.0 / inverse_mass;
            let cap = coefficient.max(0.0) * mass * load * dt;
            let j = (-slip.length() / denom).clamp(-cap, cap);
            *linear += t * (j * inverse_mass);
            *angular += arm.cross(t * j) * inverse_inertia;
        }
    }
    let tangent_speed = (*linear - n * linear.dot(n)).length();
    if tangent_speed > 1e-6 {
        let loss = (0.012 * coefficient.min(5.0) * load * dt).min(tangent_speed);
        let tangential = *linear - n * linear.dot(n);
        *linear -= tangential * (loss / tangent_speed);
    }
}

pub(crate) fn apply_surface_friction(
    velocity: &mut Vec3,
    gravity: Vec3,
    normal: Vec3,
    friction: f32,
    dt: f32,
) {
    if !velocity.is_finite()
        || !normal.is_finite()
        || !gravity.is_finite()
        || !friction.is_finite()
        || dt <= 0.0
        || !dt.is_finite()
        || friction <= 0.0
        || normal.length_squared() < 1e-8
    {
        return;
    }
    let n = normal.normalize();
    let load = (-gravity.dot(n)).max(0.0);
    if load <= 1e-6 {
        return;
    }
    let tangent = *velocity - n * velocity.dot(n);
    let speed = tangent.length();
    if speed <= 1e-7 {
        return;
    }
    let loss = (friction.clamp(0.0, 100.0) * load * dt).min(speed);
    *velocity -= tangent * (loss / speed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oblique_impulse_transfers_both_horizontal_axes_by_mass() {
        let mut a = Motion {
            position: Vec3::ZERO,
            velocity: Vec3::new(-4.0, 0.0, -3.0),
            inverse_mass: 1.0,
        };
        let mut b = Motion {
            position: Vec3::new(0.3, 0.0, 0.3),
            velocity: Vec3::ZERO,
            inverse_mass: 0.5,
        };
        assert!(resolve(&mut a, &mut b, Vec3::new(1.0, 0.0, 1.0), 0.4));
        assert!(
            b.velocity.x < -0.1 && b.velocity.z < -0.1,
            "diagonal momentum: {:?}",
            b.velocity
        );
        assert!(a.position.x > 0.0 && a.position.z > 0.0);
        assert!(b.position.x < 0.3 && b.position.z < 0.3);
    }

    #[test]
    fn actor_pushes_dynamic_cell_with_diagonal_velocity() {
        use crate::engine::entity::EntityManager;
        use crate::engine::physics::{PhysicsBody, PhysicsBodyId, PhysicsWorld};
        use std::collections::HashSet;
        let mut world = PhysicsWorld::new();
        let mut cell = PhysicsBody::new(PhysicsBodyId(1), 10, Vec3::ZERO, Vec3::ONE);
        cell.density = 2.0;
        world.bodies.push(cell);
        world.rebuild_dynamic_body_index();

        let mut actors = EntityManager::new();
        let id = actors.create_entity("MovableBox");
        actors.set_position(id, Vec3::new(-0.35, 0.5, 0.5));
        let physics = actors.get_physics_mut(id).unwrap();
        physics.enabled = true;
        physics.solid = true;
        physics.collision_enabled = true;
        physics.anchored = false;
        physics.mass = 1.0;
        physics.size = Vec3::ONE;
        physics.velocity = Vec3::new(3.0, 0.0, 2.0);

        world.resolve_mixed_collisions(&mut actors, &HashSet::new(), Vec3::NEG_Y, 1.0 / 60.0);
        assert!(world.bodies[0].position.x > 0.01);
        assert!(world.bodies[0].velocity.x > 0.1);
        assert!(
            world.bodies[0].velocity.z > 0.01,
            "contact friction must carry off-axis motion into the Cell"
        );
        assert!(actors.get_position(id).unwrap().x < -0.35);
    }

    #[test]
    fn resting_surface_friction_responds_to_gravity_and_coefficient() {
        let mut sliding = Vec3::new(3.0, 0.0, 2.0);
        apply_surface_friction(&mut sliding, Vec3::ZERO, Vec3::Y, 10.0, 1.0);
        assert_eq!(sliding, Vec3::new(3.0, 0.0, 2.0));
        apply_surface_friction(&mut sliding, Vec3::NEG_Y * 9.0, Vec3::Y, 0.0, 1.0);
        assert_eq!(sliding, Vec3::new(3.0, 0.0, 2.0));
        apply_surface_friction(&mut sliding, Vec3::NEG_Y * 9.0, Vec3::Y, 1.0, 1.0);
        assert_eq!(sliding, Vec3::ZERO);
    }

    #[test]
    fn resting_friction_preserves_velocity_normal_to_surface() {
        let mut velocity = Vec3::new(3.0, 1.5, 0.0);
        apply_surface_friction(&mut velocity, Vec3::NEG_Y * 9.0, Vec3::Y, 1.0, 1.0);
        assert!((velocity.y - 1.5).abs() < 1e-5);
        assert!(velocity.x.abs() < 1e-5);
    }

    #[test]
    fn anchored_body_does_not_move_when_struck() {
        let mut a = Motion {
            position: Vec3::ZERO,
            velocity: Vec3::new(-2.0, 0.0, -1.0),
            inverse_mass: 1.0,
        };
        let mut b = Motion {
            position: Vec3::new(0.5, 0.0, 0.5),
            velocity: Vec3::ZERO,
            inverse_mass: 0.0,
        };
        assert!(resolve(&mut a, &mut b, Vec3::new(1.0, 0.0, 1.0), 0.2));
        assert_eq!(b.position, Vec3::new(0.5, 0.0, 0.5));
        assert_eq!(b.velocity, Vec3::ZERO);
    }
}

#[cfg(test)]
mod angular_contact_tests {
    use super::*;
    #[test]
    fn off_center_impact_rotates_a_tall_body() {
        let mut a = Motion {
            position: Vec3::new(-0.4, 0.0, 0.0),
            velocity: Vec3::new(6.0, 0.0, 0.0),
            inverse_mass: 1.0,
        };
        let mut b = Motion {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            inverse_mass: 1.0,
        };
        let mut aa = AngularMotion {
            center: Vec3::new(-0.4, 1.0, 0.0),
            velocity: Vec3::ZERO,
            inverse_inertia: inverse_box_inertia(1.0, Vec3::ONE, false),
        };
        let mut ab = AngularMotion {
            center: Vec3::new(0.5, 1.0, 0.5),
            velocity: Vec3::ZERO,
            inverse_inertia: inverse_box_inertia(1.0, Vec3::new(1.0, 2.0, 1.0), false),
        };
        assert!(resolve_angular_with_friction(
            &mut a,
            &mut b,
            &mut aa,
            &mut ab,
            Vec3::NEG_X,
            0.1,
            Vec3::new(0.0, 1.8, 0.5),
            0.4
        ));
        assert!(
            ab.velocity.z.abs() > 0.01,
            "off-center collision must create torque"
        );
        assert!(b.velocity.x > 0.0, "impact transfers linear momentum");
    }
    #[test]
    fn rolling_friction_spins_instead_of_only_stopping() {
        let mut v = Vec3::new(6.0, 0.0, 0.0);
        let mut w = Vec3::ZERO;
        apply_rolling_friction(
            &mut v,
            &mut w,
            inverse_box_inertia(2.0, Vec3::splat(1.0), false),
            0.5,
            Vec3::NEG_Y * 9.8,
            Vec3::Y,
            0.5,
            0.8,
            1.0 / 60.0,
        );
        assert!(w.z.abs() > 0.01 && v.x < 6.0);
    }
    #[test]
    fn zero_gravity_preserves_linear_and_angular_momentum() {
        let mut v = Vec3::new(6.0, 0.0, 0.0);
        let mut w = Vec3::new(0.0, 0.0, 2.0);
        apply_rolling_friction(
            &mut v,
            &mut w,
            Vec3::ONE,
            1.0,
            Vec3::ZERO,
            Vec3::Y,
            0.5,
            5.0,
            0.5,
        );
        assert_eq!(v, Vec3::new(6.0, 0.0, 0.0));
        assert_eq!(w, Vec3::new(0.0, 0.0, 2.0));
    }
}
