use crate::world::{CellType, WorldCoord};
use glam::Vec3;

// Separate runtime ID from grid coordinates because bodies move in continuous space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicsBodyId(pub u64);

// This records a detected overlap between a dynamic body and a static block.
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
    pub size: Vec3,
    pub color_rgb: Vec3,
    pub texture: String,
    pub density: f32,
    pub visible: bool,
    pub anchored: bool,
    pub solid: bool,
    pub gravity_participation: bool,
    pub is_sleeping: bool,

    // Accumulates time spent below the linear velocity threshold.
    pub sleep_timer: f32,

    // Stores the normal of the surface the body is currently resting against.
    // normal points from the static block toward this body.
    pub static_contact_normal: Option<Vec3>,

    // ID and normal of a dynamic body currently supporting this one.
    // normal points from the support toward this body.
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
            size,
            color_rgb: Vec3::new(0.5, 0.5, 0.5),
            texture: "None".to_string(),
            density: 1.0,
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

    // Wake the body and reset its sleep timer.
    pub fn wake(&mut self) {
        self.is_sleeping = false;
        self.sleep_timer = 0.0;
    }

    // Mass is derived from volume. Anchored objects report 0 (infinite mass).
    pub fn get_mass(&self) -> f32 {
        if self.anchored {
            0.0
        } else {
            let volume = self.size.x * self.size.y * self.size.z;
            volume * self.density
        }
    }

    // Position is the minimum corner (X, Y, Z).
    pub fn min_corner(&self) -> Vec3 {
        self.position
    }

    pub fn max_corner(&self) -> Vec3 {
        self.position + self.size
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
