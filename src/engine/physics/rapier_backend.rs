use glam::{Quat, Vec3};
use rapier3d::prelude::{
    ColliderBuilder, PhysicsWorld as RapierWorld, RigidBodyBuilder, RigidBodyHandle, Vector,
};
use std::collections::HashMap;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug)]
pub(crate) enum BodyShape {
    Box { half_extents: Vec3 },
    Sphere { radius: f32 },
    CapsuleY { half_height: f32, radius: f32 },
}

impl BodyShape {
    fn collider(self) -> Option<ColliderBuilder> {
        match self {
            Self::Box { half_extents: half } if half.is_finite() && half.min_element() > 0.0 => {
                Some(ColliderBuilder::cuboid(half.x, half.y, half.z))
            }
            Self::Sphere { radius } if radius.is_finite() && radius > 0.0 => {
                Some(ColliderBuilder::ball(radius))
            }
            Self::CapsuleY {
                half_height,
                radius,
            } if half_height.is_finite()
                && radius.is_finite()
                && half_height >= 0.0
                && radius > 0.0 =>
            {
                Some(ColliderBuilder::capsule_y(half_height, radius))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct BodyState {
    pub translation: Vec3,
    pub rotation: Quat,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub sleeping: bool,
}

pub(crate) struct RigidSolver {
    world: RapierWorld,
    handles: HashMap<u64, RigidBodyHandle>,
}
impl RigidSolver {
    pub fn new(gravity: Vec3, dt: f32) -> Self {
        let mut world = RapierWorld::new();
        world.gravity = Vector::new(gravity.x, gravity.y, gravity.z);
        world.integration_parameters.dt = dt;
        Self {
            world,
            handles: HashMap::new(),
        }
    }

    pub fn add_static(&mut self, shape: BodyShape, center: Vec3, friction: f32) -> bool {
        let Some(collider) = shape.collider() else {
            return false;
        };
        if !center.is_finite() {
            return false;
        }
        self.world.insert_collider(
            collider
                .translation(Vector::new(center.x, center.y, center.z))
                .friction(friction.max(0.0)),
            None,
        );
        true
    }

    pub fn add_dynamic(
        &mut self,
        id: u64,
        shape: BodyShape,
        center: Vec3,
        mass_density: f32,
        friction: f32,
        linear_velocity: Vec3,
        rotation: Quat,
        ccd: bool,
    ) -> bool {
        if self.handles.contains_key(&id)
            || !center.is_finite()
            || !linear_velocity.is_finite()
            || !rotation.is_finite()
            || !mass_density.is_finite()
            || mass_density <= 0.0
        {
            return false;
        }
        let Some(collider) = shape.collider() else {
            return false;
        };
        let body = RigidBodyBuilder::dynamic()
            .translation(Vector::new(center.x, center.y, center.z))
            .rotation(rapier3d::prelude::AngVector::new(0.0, 0.0, 0.0))
            .linvel(Vector::new(
                linear_velocity.x,
                linear_velocity.y,
                linear_velocity.z,
            ))
            .ccd_enabled(ccd);
        let collider = collider.density(mass_density).friction(friction.max(0.0));
        let (handle, _) = self.world.insert(body, collider);
        self.handles.insert(id, handle);
        if let Some(rigid) = self.world.bodies.get_mut(handle) {
            let unit = rapier3d::na::UnitQuaternion::from_quaternion(
                rapier3d::na::Quaternion::new(rotation.w, rotation.x, rotation.y, rotation.z),
            );
            rigid.set_rotation(unit.into(), true);
        }
        true
    }

    pub fn step(&mut self, gravity: Vec3) {
        self.world.gravity = Vector::new(gravity.x, gravity.y, gravity.z);
        self.world.step();
    }

    pub fn state(&self, id: u64) -> Option<BodyState> {
        let body = self.world.bodies.get(*self.handles.get(&id)?)?;
        let translation = body.translation();
        let rotation = body.rotation();
        let linear = body.linvel();
        let angular = body.angvel();
        Some(BodyState {
            translation: Vec3::new(translation.x, translation.y, translation.z),
            rotation: Quat::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
            linear_velocity: Vec3::new(linear.x, linear.y, linear.z),
            angular_velocity: Vec3::new(angular.x, angular.y, angular.z),
            sleeping: body.is_sleeping(),
        })
    }

    pub fn add_body(
        &mut self,
        id: u64,
        parts: Vec<ColliderBuilder>,
        center: Vec3,
        rotation: Quat,
        mass: f32,
        density: f32,
        friction: f32,
        velocity: Vec3,
        dynamic: bool,
        kinematic: bool,
    ) -> bool {
        if self.handles.contains_key(&id)
            || !center.is_finite()
            || !rotation.is_finite()
            || !velocity.is_finite()
        {
            return false;
        }
        let kind = if kinematic {
            RigidBodyBuilder::kinematic_position_based()
        } else if dynamic {
            RigidBodyBuilder::dynamic()
        } else {
            RigidBodyBuilder::fixed()
        };
        let angle = rotation.to_scaled_axis();
        let mut builder = kind
            .translation(Vector::new(center.x, center.y, center.z))
            .rotation(Vector::new(angle.x, angle.y, angle.z))
            .linvel(Vector::new(velocity.x, velocity.y, velocity.z))
            .ccd_enabled(dynamic);
        let mut parts = parts.into_iter();
        let handle = if let Some(first) = parts.next() {
            let (handle, _) = self.world.insert(
                builder,
                first
                    .density(density.max(0.001))
                    .friction(friction.max(0.0)),
            );
            handle
        } else {
            if dynamic {
                builder = builder.additional_mass(mass.max(0.001));
            }
            self.world.insert_body(builder)
        };
        for part in parts {
            self.world.insert_collider(
                part.density(density.max(0.001)).friction(friction.max(0.0)),
                Some(handle),
            );
        }
        self.handles.insert(id, handle);
        true
    }
    pub fn change_gravity_scale(&mut self, id: u64, scale: f32) {
        if !scale.is_finite() {
            return;
        }
        if let Some(handle) = self.handles.get(&id).copied() {
            if let Some(b) = self.world.bodies.get_mut(handle) {
                b.set_gravity_scale(scale, true);
            }
        }
    }

    pub fn set_body_mode(&mut self, id: u64, dynamic: bool, kinematic: bool) {
        use rapier3d::prelude::RigidBodyType;
        let target = if kinematic {
            RigidBodyType::KinematicPositionBased
        } else if dynamic {
            RigidBodyType::Dynamic
        } else {
            RigidBodyType::Fixed
        };
        if let Some(handle) = self.handles.get(&id).copied() {
            if let Some(body) = self.world.bodies.get_mut(handle) {
                if body.body_type() != target {
                    body.set_body_type(target, true);
                }
            }
        }
    }

    pub fn contains(&self, id: u64) -> bool {
        self.handles.contains_key(&id)
    }

    pub fn retain_dynamic(&mut self, wanted: &std::collections::HashSet<u64>) {
        let obsolete: Vec<u64> = self
            .handles
            .keys()
            .copied()
            .filter(|id| !wanted.contains(id))
            .collect();
        for id in obsolete {
            if let Some(handle) = self.handles.remove(&id) {
                self.world.remove_body(handle);
            }
        }
    }

    pub fn apply_motion(
        &mut self,
        id: u64,
        center: Vec3,
        orientation: Quat,
        linear_velocity: Vec3,
        angular_velocity: Vec3,
        force: Vec3,
        torque: Vec3,
        gravity_delta: Vec3,
        last_published: Option<BodyState>,
    ) {
        let Some(&handle) = self.handles.get(&id) else {
            return;
        };
        let Some(body) = self.world.bodies.get_mut(handle) else {
            return;
        };
        let external_position = last_published
            .is_none_or(|state| (center - state.translation).length_squared() > 0.000001);
        let external_rotation =
            last_published.is_none_or(|state| (orientation.dot(state.rotation)).abs() < 0.99999);
        let external_linear = last_published.is_none_or(|state| {
            (linear_velocity - state.linear_velocity).length_squared() > 0.000001
        });
        let external_angular = last_published.is_none_or(|state| {
            (angular_velocity - state.angular_velocity).length_squared() > 0.000001
        });
        if external_position {
            body.set_translation(Vector::new(center.x, center.y, center.z), true);
        }
        if external_rotation {
            body.set_rotation(
                rapier3d::prelude::Rotation::from_xyzw(
                    orientation.x,
                    orientation.y,
                    orientation.z,
                    orientation.w,
                ),
                true,
            );
        }
        if external_linear {
            body.set_linvel(
                Vector::new(linear_velocity.x, linear_velocity.y, linear_velocity.z),
                true,
            );
        }
        if external_angular {
            body.set_angvel(
                Vector::new(angular_velocity.x, angular_velocity.y, angular_velocity.z),
                true,
            );
        }
        if force.is_finite() && force.length_squared() > 1e-12 {
            body.add_force(Vector::new(force.x, force.y, force.z), true);
        }
        if torque.is_finite() && torque.length_squared() > 1e-12 {
            body.add_torque(Vector::new(torque.x, torque.y, torque.z), true);
        }
        if gravity_delta.is_finite() && gravity_delta.length_squared() > 1e-12 {
            let f = gravity_delta * body.mass();
            body.add_force(Vector::new(f.x, f.y, f.z), true);
        }
    }

    pub fn add_static_actor(
        &mut self,
        id: u64,
        shape: BodyShape,
        center: Vec3,
        orientation: Quat,
        friction: f32,
    ) -> bool {
        if self.handles.contains_key(&id) {
            return true;
        }
        let Some(collider) = shape.collider() else {
            return false;
        };
        if !center.is_finite() {
            return false;
        }
        let body = RigidBodyBuilder::fixed()
            .translation(Vector::new(center.x, center.y, center.z))
            .rotation(Vector::new(
                orientation.to_scaled_axis().x,
                orientation.to_scaled_axis().y,
                orientation.to_scaled_axis().z,
            ));
        let (handle, _) = self
            .world
            .insert(body, collider.friction(friction.max(0.0)));
        self.handles.insert(id, handle);
        true
    }

    pub fn sync_fixed_actor(&mut self, id: u64, center: Vec3, rotation: Quat) {
        if let Some(handle) = self.handles.get(&id).copied() {
            if let Some(body) = self.world.bodies.get_mut(handle) {
                if (body.translation() - center).length_squared() > 1e-10 {
                    body.set_translation(center, true);
                }
                if body.rotation().dot(rotation).abs() < 0.999999 {
                    body.set_rotation(rotation, true);
                }
            }
        }
    }

    pub fn add_kinematic_actor(
        &mut self,
        id: u64,
        shape: BodyShape,
        center: Vec3,
        rotation: Quat,
    ) -> bool {
        if self.handles.contains_key(&id) {
            return true;
        }
        let Some(collider) = shape.collider() else {
            return false;
        };
        let body = RigidBodyBuilder::kinematic_position_based()
            .translation(Vector::new(center.x, center.y, center.z))
            .rotation(Vector::new(
                rotation.to_scaled_axis().x,
                rotation.to_scaled_axis().y,
                rotation.to_scaled_axis().z,
            ));
        let (handle, _) = self.world.insert(body, collider);
        self.handles.insert(id, handle);
        true
    }

    pub fn sync_kinematic_actor(&mut self, id: u64, center: Vec3, rotation: Quat) {
        if let Some(handle) = self.handles.get(&id).copied() {
            if let Some(body) = self.world.bodies.get_mut(handle) {
                body.set_next_kinematic_translation(Vector::new(center.x, center.y, center.z));
                body.set_next_kinematic_rotation(rapier3d::prelude::Rotation::from_xyzw(
                    rotation.x, rotation.y, rotation.z, rotation.w,
                ));
            }
        }
    }

    pub fn move_character_against_actors(
        &mut self,
        id: u64,
        from: Vec3,
        target: Vec3,
        orientation: Quat,
        mut velocity: Vec3,
        mass: f32,
        actor_keys: &HashSet<u64>,
        max_walkable_slope_degrees: f32,
) -> (Vec3, Vec3, bool) {
        use rapier3d::parry::query::{ShapeCastOptions, cast_shapes};
        use rapier3d::prelude::{Pose, Rotation};

        let Some(&handle) = self.handles.get(&id) else {
            return (target, velocity, false);
        };
        let body = &self.world.bodies[handle];
        let parts: Vec<_> = body
            .colliders()
            .iter()
            .filter_map(|handle| {
                let collider = self.world.colliders.get(*handle)?;
                Some((
                    *collider.position_wrt_parent()?,
                    collider.shared_shape().clone(),
                ))
            })
            .collect();
        if parts.is_empty() || !from.is_finite() || !target.is_finite() {
            return (target, velocity, false);
        }
        let rotation =
            Rotation::from_xyzw(orientation.x, orientation.y, orientation.z, orientation.w);
        let mut position = from;
        let mut remaining = target - from;
        let mut grounded = false;
        for _ in 0..8 {
            if remaining.length_squared() < 1e-12 {
                break;
            }
            let pose = Pose::from_parts(Vector::new(position.x, position.y, position.z), rotation);
            let travel = Vector::new(remaining.x, remaining.y, remaining.z);
            let mut closest = None;
            let mut closest_time = 1.0_f32;
            for (&key, &other_handle) in &self.handles {
                if key == id || !actor_keys.contains(&key) {
                    continue;
                }
                let other = &self.world.bodies[other_handle];
                for other_collider in other.colliders() {
                    let collider = &self.world.colliders[*other_collider];
                    let Some(local_pose) = collider.position_wrt_parent() else {
                        continue;
                    };
                    let other_pose = *other.position() * *local_pose;
                    let other_bounds = collider.shape().compute_aabb(&other_pose);
                    for (local, shape) in &parts {
                        let start = pose * *local;
                        let end = Pose::from_translation(travel) * start;
                        let start_bounds = shape.compute_aabb(&start);
                        let end_bounds = shape.compute_aabb(&end);
                        let low = start_bounds.mins.min(end_bounds.mins) - Vector::splat(0.003);
                        let high = start_bounds.maxs.max(end_bounds.maxs) + Vector::splat(0.003);
                        if other_bounds.mins.x > high.x
                            || other_bounds.maxs.x < low.x
                            || other_bounds.mins.y > high.y
                            || other_bounds.maxs.y < low.y
                            || other_bounds.mins.z > high.z
                            || other_bounds.maxs.z < low.z
                        {
                            continue;
                        }
                        let options = ShapeCastOptions {
                            max_time_of_impact: closest_time,
                            target_distance: 0.002,
                            stop_at_penetration: false,
                            ..Default::default()
                        };
                        let Ok(Some(hit)) = cast_shapes(
                            &start,
                            travel,
                            shape.as_ref(),
                            &other_pose,
                            Vector::ZERO,
                            collider.shape(),
                            options,
                        ) else {
                            continue;
                        };
                        let n = other_pose.rotation * hit.normal2;
                        let normal = Vec3::new(n.x, n.y, n.z).normalize_or_zero();
                        if normal.length_squared() < 0.5 || remaining.dot(normal) >= -1e-7 {
                            continue;
                        }
                        let point = other_pose * hit.witness2;
                        closest_time = hit.time_of_impact;
                        closest = Some((other_handle, normal, point));
                    }
                }
            }
            let Some((other_handle, normal, point)) = closest else {
                position += remaining;
                break;
            };
            position += remaining * closest_time + normal * 0.0001;
            remaining *= 1.0 - closest_time;
            let steep = normal.y > 0.0
                && !crate::character::collision::contact_is_walkable(normal, max_walkable_slope_degrees, false);
            remaining -= normal * remaining.dot(normal).min(0.0);
            if steep {
                let wall = Vec3::new(normal.x, 0.0, normal.z).normalize_or_zero();
                remaining -= wall * remaining.dot(wall).min(0.0);
                if target.y <= from.y {
                    remaining.y = remaining.y.min(0.0);
                }
            }
            grounded |= crate::character::collision::contact_is_walkable(
                normal, max_walkable_slope_degrees, false
            ) && velocity.y <= 0.0;
            let was_rising = velocity.y > 0.0;
            let other = &mut self.world.bodies[other_handle];
            if other.is_dynamic() {
                let v = other.velocity_at_point(point);
                let relative = velocity - Vec3::new(v.x, v.y, v.z);
                let into = relative.dot(normal).min(0.0);
                let char_inverse = 1.0 / mass.max(0.1);
                let other_inverse = 1.0 / other.mass().max(0.001);
                let impulse = -into / (char_inverse + other_inverse);
                velocity += normal * (impulse * char_inverse);
                let push = -normal * impulse;
                other.apply_impulse_at_point(Vector::new(push.x, push.y, push.z), point, true);
            } else {
                velocity -= normal * velocity.dot(normal).min(0.0);
            }
            if steep && !was_rising {
                velocity.y = velocity.y.min(0.0);
            }
        }
        if let Some(body) = self.world.bodies.get_mut(handle) {
            body.set_translation(Vector::new(from.x, from.y, from.z), true);
            body.set_rotation(rotation, true);
        }
        (position, velocity, grounded)
    }

    pub fn add_static_triangles(&mut self, triangles: &[[Vec3; 3]], friction: f32) -> bool {
        let mut pts = Vec::with_capacity(triangles.len() * 3);
        let mut indices = Vec::with_capacity(triangles.len());
        for tri in triangles {
            if tri.iter().any(|p| !p.is_finite()) {
                continue;
            }
            let base = pts.len() as u32;
            for p in tri {
                pts.push(Vector::new(p.x, p.y, p.z));
            }
            indices.push([base, base + 1, base + 2]);
        }
        if indices.is_empty() {
            return false;
        }
        let Ok(collider) = ColliderBuilder::trimesh(pts, indices) else {
            return false;
        };
        self.world
            .insert_collider(collider.friction(friction.max(0.0)), None);
        true
    }

    pub fn add_terrain_patch(
        &mut self,
        corners: [Vec3; 4],
        friction: f32,
    ) -> Option<rapier3d::prelude::ColliderHandle> {
        let v: Vec<Vector> = corners.iter().map(|p| Vector::new(p.x, p.y, p.z)).collect();
        if v.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let triangles = vec![[0, 2, 1], [0, 3, 2]];
        let Ok(collider) = ColliderBuilder::trimesh(v, triangles) else {
            return None;
        };
        Some(
            self.world
                .insert_collider(collider.friction(friction.max(0.0)), None),
        )
    }
    pub fn remove_terrain_patch(&mut self, handle: rapier3d::prelude::ColliderHandle) {
        self.world.remove_collider(handle);
    }

    pub fn active_actor_contacts(&self) -> std::collections::HashSet<(u64, u64)> {
        let reverse: HashMap<_, _> = self
            .handles
            .iter()
            .map(|(&id, &handle)| (handle, id))
            .collect();
        let mut pairs = std::collections::HashSet::new();
        for pair in self.world.narrow_phase.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            let a = self
                .world
                .colliders
                .get(pair.collider1)
                .and_then(|c| c.parent())
                .and_then(|h| reverse.get(&h))
                .copied();
            let b = self
                .world
                .colliders
                .get(pair.collider2)
                .and_then(|c| c.parent())
                .and_then(|h| reverse.get(&h))
                .copied();
            let (Some(a), Some(b)) = (a, b) else { continue };
            if a >> 63 == 1 && b >> 63 == 1 {
                pairs.insert((a & !(1u64 << 63), b & !(1u64 << 63)));
            }
        }
        pairs
    }

    pub fn supported(&self, id: u64, gravity: Vec3) -> bool {
        let Some(body_handle) = self.handles.get(&id).copied() else {
            return false;
        };
        let down = gravity.normalize_or_zero();
        if down.length_squared() < 1e-8 {
            return false;
        }
        for pair in self.world.narrow_phase.contact_pairs() {
            if !pair.has_any_active_contact() {
                continue;
            }
            let a = self
                .world
                .colliders
                .get(pair.collider1)
                .and_then(|c| c.parent());
            let b = self
                .world
                .colliders
                .get(pair.collider2)
                .and_then(|c| c.parent());
            if a != Some(body_handle) && b != Some(body_handle) {
                continue;
            }
            for manifold in pair.manifolds() {
                let n = if a == Some(body_handle) {
                    -manifold.data.normal
                } else {
                    manifold.data.normal
                };
                let support_normal = Vec3::new(n.x, n.y, n.z);
                if support_normal.dot(-down) > 0.5 {
                    return true;
                }
            }
        }
        false
    }

    #[cfg(test)]
    fn contact_count(&self) -> usize {
        self.world.narrow_phase.contact_pairs().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> RigidSolver {
        let mut s = RigidSolver::new(Vec3::NEG_Y * 9.81, 1.0 / 60.0);
        assert!(s.add_static(
            BodyShape::Box {
                half_extents: Vec3::new(30.0, 0.5, 30.0)
            },
            Vec3::new(0.0, -0.5, 0.0),
            0.7
        ));
        s
    }
    fn cube() -> BodyShape {
        BodyShape::Box {
            half_extents: Vec3::splat(0.5),
        }
    }

    #[test]
    fn tilted_box_topples_without_floating() {
        let mut s = world();
        assert!(s.add_dynamic(
            1,
            cube(),
            Vec3::new(0.0, 0.74, 0.0),
            1.0,
            0.2,
            Vec3::ZERO,
            Quat::from_rotation_z(0.45),
            true
        ));
        for _ in 0..240 {
            s.step(Vec3::NEG_Y * 9.81);
        }
        let b = s.state(1).unwrap();
        assert!(b.translation.is_finite() && b.rotation.is_finite());
        assert!(
            b.translation.y > 0.49 && b.translation.y < 0.8,
            "tilted box must be supported by floor: {b:?}"
        );
        assert!(
            b.rotation.to_euler(glam::EulerRot::XYZ).2.abs() < 0.3,
            "gravity should restore flat stable support: {b:?}"
        );
    }

    #[test]
    fn settled_stack_stays_out_of_floor_for_thousands_of_steps() {
        let mut s = world();
        for i in 0..4 {
            assert!(s.add_dynamic(
                i + 1,
                cube(),
                Vec3::new(0.0, 0.52 + i as f32, 0.0),
                1.0,
                0.8,
                Vec3::ZERO,
                Quat::IDENTITY,
                true
            ));
        }
        for _ in 0..2000 {
            s.step(Vec3::NEG_Y * 9.81);
        }
        for id in 1..=4 {
            let b = s.state(id).unwrap();
            assert!(
                b.translation.y > 0.48 && b.translation.is_finite(),
                "stack body {id}: {b:?}"
            );
            assert!(b.linear_velocity.is_finite() && b.angular_velocity.is_finite());
        }
        assert!(s.contact_count() > 0);
    }

    #[test]
    fn fast_sphere_cannot_cross_thin_wall() {
        let mut s = RigidSolver::new(Vec3::ZERO, 1.0 / 60.0);
        assert!(s.add_static(
            BodyShape::Box {
                half_extents: Vec3::new(0.05, 2.0, 2.0)
            },
            Vec3::new(2.0, 1.0, 0.0),
            0.5
        ));
        assert!(s.add_dynamic(
            7,
            BodyShape::Sphere { radius: 0.4 },
            Vec3::new(0.0, 1.0, 0.0),
            1.0,
            0.3,
            Vec3::new(150.0, 0.0, 0.0),
            Quat::IDENTITY,
            true
        ));
        for _ in 0..10 {
            s.step(Vec3::ZERO);
        }
        let b = s.state(7).unwrap();
        assert!(b.translation.x < 1.70, "fast sphere tunneled: {b:?}");
    }

    #[test]
    fn zero_gravity_preserves_free_body_momentum() {
        let mut s = RigidSolver::new(Vec3::ZERO, 1.0 / 60.0);
        assert!(s.add_dynamic(
            3,
            BodyShape::Sphere { radius: 0.5 },
            Vec3::ZERO,
            1.0,
            0.0,
            Vec3::new(3.0, 0.0, 0.0),
            Quat::IDENTITY,
            false
        ));
        for _ in 0..100 {
            s.step(Vec3::ZERO);
        }
        let b = s.state(3).unwrap();
        assert!(
            (b.translation.x - 5.0).abs() < 0.05 && (b.linear_velocity.x - 3.0).abs() < 0.05,
            "unexpected drag without support/gravity: {b:?}"
        );
    }

    #[test]
    fn sphere_capsule_box_interactions_keep_finite_contact_state() {
        let mut s = world();
        assert!(s.add_dynamic(
            1,
            BodyShape::Sphere { radius: 0.5 },
            Vec3::new(-1.0, 1.0, 0.0),
            1.0,
            0.5,
            Vec3::new(6.0, 0.0, 0.0),
            Quat::IDENTITY,
            true
        ));
        assert!(s.add_dynamic(
            2,
            BodyShape::CapsuleY {
                half_height: 0.6,
                radius: 0.22
            },
            Vec3::new(0.5, 1.0, 0.0),
            1.0,
            0.6,
            Vec3::ZERO,
            Quat::IDENTITY,
            true
        ));
        assert!(s.add_dynamic(
            3,
            cube(),
            Vec3::new(1.5, 0.8, 0.0),
            1.0,
            0.6,
            Vec3::ZERO,
            Quat::IDENTITY,
            true
        ));
        for _ in 0..400 {
            s.step(Vec3::NEG_Y * 9.81);
        }
        for i in 1..=3 {
            let b = s.state(i).unwrap();
            assert!(
                b.translation.is_finite() && b.rotation.is_finite() && b.translation.y > -0.5,
                "body {i}: {b:?}"
            );
        }
    }
}
