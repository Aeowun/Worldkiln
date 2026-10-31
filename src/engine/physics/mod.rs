// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub(crate) mod anchored_surface;
pub mod physics_body;
pub mod physics_simulation;
pub mod physics_world;
pub(crate) mod rapier_backend;
pub(crate) mod rapier_runtime;
pub mod raycast;
pub(crate) mod rigid_contact;
pub(crate) mod terrain_collision_cache;
pub mod triangle_collider;

#[cfg(test)]
mod physics_tests;

pub use physics_body::{CollisionRecord, PhysicsBody, PhysicsBodyId, PhysicsIdGenerator};
pub use physics_simulation::{
    MAX_PHYSICS_STEPS, PhysicsClock, SIMULATION_DT, SLEEP_TIME_THRESHOLD, SLEEP_VELOCITY_THRESHOLD,
};
pub use physics_world::PhysicsWorld;
