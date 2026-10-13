pub mod physics_body;
pub mod physics_simulation;
pub mod physics_world;

#[cfg(test)]
mod physics_tests;

pub use physics_body::{CollisionRecord, PhysicsBody, PhysicsBodyId, PhysicsIdGenerator};
pub use physics_simulation::{
    MAX_PHYSICS_STEPS, PhysicsClock, SIMULATION_DT, SLEEP_TIME_THRESHOLD, SLEEP_VELOCITY_THRESHOLD,
};
pub use physics_world::PhysicsWorld;
