use glam::Vec3;
use std::collections::BTreeMap;

use super::cell::AttributeValue;

/// Authored physics defaults for an Actor.
///
/// These are project/world data. Runtime physics state is created separately
/// when the Actor enters Play mode.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorPhysicsConfig {
    pub enabled: bool,
    pub solid: bool,
    pub anchored: bool,
    pub collision_enabled: bool,
    pub mass: f32,

    pub collision_size: Vec3,
    pub collision_offset: Vec3,
}

impl Default for ActorPhysicsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            solid: true,
            anchored: true,
            collision_enabled: true,
            mass: 1.0,

            collision_size: Vec3::ONE,
            collision_offset: Vec3::ZERO,
        }
    }
}

/// Authored Actor.
///
/// Actors are first-class authored World objects. They are not Cells.
///
/// An Actor becomes a Runtime Entity when Play starts. Runtime state such as
/// velocity, active overlaps, animation playback, and runtime physics is not
/// stored here.
#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub id: u64,
    pub identity: String,

    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    pub visible: bool,
    pub enabled: bool,

    /// Package/asset used to construct the Actor's runtime appearance.
    ///
    /// A package may describe a Character, animated object, static object, or
    /// another future Actor appearance source.
    pub package: Option<String>,

    pub physics: ActorPhysicsConfig,

    pub attributes: BTreeMap<String, AttributeValue>,
}

impl Actor {
    pub fn new(id: u64, identity: impl Into<String>, position: Vec3) -> Self {
        Self {
            id,
            identity: identity.into(),
            position,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            visible: true,
            enabled: true,
            package: None,
            physics: ActorPhysicsConfig::default(),
            attributes: BTreeMap::new(),
        }
    }
}

impl Default for Actor {
    fn default() -> Self {
        Self::new(0, String::new(), Vec3::ZERO)
    }
}
