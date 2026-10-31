// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::world::{CellType, WorldCoord};
use glam::{Quat, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicsBodyId(pub u64);

pub struct CollisionRecord {
    pub body_id: PhysicsBodyId,
    pub block_coord: WorldCoord,
}

#[derive(Debug, Clone)]
pub struct PhysicsBody {
    pub id: PhysicsBodyId,
    pub cell_id: u64,
    pub cell_type: CellType,
    pub position: Vec3,
    pub velocity: Vec3,
    pub rotation: Quat,
    pub angular_velocity: Vec3,
    pub size: Vec3,
    pub color_rgb: Vec3,
    pub texture: String,
    pub density: f32,
    pub friction: f32,
    pub visible: bool,
    pub anchored: bool,
    pub solid: bool,
    pub gravity_participation: bool,
    pub is_sleeping: bool,

    pub sleep_timer: f32,

    pub static_contact_normal: Option<Vec3>,

    pub dynamic_contact: Option<(PhysicsBodyId, Vec3)>,
}

impl PhysicsBody {
    pub fn new(id: PhysicsBodyId, cell_id: u64, position: Vec3, size: Vec3) -> Self {
        Self {
            id,
            cell_id,
            cell_type: CellType::Empty,
            position,
            velocity: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            angular_velocity: Vec3::ZERO,
            size,
            color_rgb: Vec3::new(0.5, 0.5, 0.5),
            texture: "None".to_string(),
            density: 1.0,
            friction: crate::world::cell::DEFAULT_SURFACE_FRICTION,
            visible: true,
            anchored: false,
            solid: true,
            gravity_participation: true,
            is_sleeping: false,
            sleep_timer: 0.0,
            static_contact_normal: None,
            dynamic_contact: None,
        }
    }

    pub fn wake(&mut self) {
        self.is_sleeping = false;
        self.sleep_timer = 0.0;
    }

    pub fn get_mass(&self) -> f32 {
        if self.anchored {
            0.0
        } else {
            let volume = self.size.x * self.size.y * self.size.z;
            volume * self.density
        }
    }

    pub fn center(&self) -> Vec3 {
        self.position + self.size * 0.5
    }

    pub fn collider(&self) -> crate::engine::collision_geometry::ColliderInstance {
        use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
        let (rx, ry, rz) = self.rotation.to_euler(glam::EulerRot::XYZ);
        let half = self.size * 0.5;
        ColliderInstance {
            shapes: std::sync::Arc::new(vec![CollisionShape::Box {
                min: (-half).to_array(),
                max: half.to_array(),
            }]),
            rotation: Vec3::new(rx, ry, rz),
            scale: Vec3::ONE,
        }
    }

    pub fn inverse_inertia(&self) -> Vec3 {
        super::rigid_contact::inverse_box_inertia(self.get_mass(), self.size, self.anchored)
    }

    pub fn min_corner(&self) -> Vec3 {
        let rotation = glam::Mat3::from_quat(self.rotation);
        let half = self.size * 0.5;
        let extent = rotation.x_axis.abs() * half.x
            + rotation.y_axis.abs() * half.y
            + rotation.z_axis.abs() * half.z;
        self.center() - extent
    }

    pub fn max_corner(&self) -> Vec3 {
        let rotation = glam::Mat3::from_quat(self.rotation);
        let half = self.size * 0.5;
        let extent = rotation.x_axis.abs() * half.x
            + rotation.y_axis.abs() * half.y
            + rotation.z_axis.abs() * half.z;
        self.center() + extent
    }
}

pub struct PhysicsIdGenerator {
    pub(crate) next_id: u64,
}

impl PhysicsIdGenerator {
    pub fn new() -> Self {
        Self { next_id: 1 }
    }

    pub fn next(&mut self) -> PhysicsBodyId {
        let id = PhysicsBodyId(self.next_id);
        self.next_id += 1;
        id
    }
}
