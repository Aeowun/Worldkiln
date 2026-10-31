// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::engine::collision_geometry::CollisionMode;
use glam::Vec3;
use std::collections::{BTreeMap, BTreeSet};

use super::cell::AttributeValue;

#[derive(Clone, Debug, PartialEq)]
pub struct ActorPhysicsConfig {
    pub enabled: bool,
    pub solid: bool,
    pub anchored: bool,
    pub collision_enabled: bool,
    pub mass: f32,

    pub collision_size: Vec3,
    pub collision_offset: Vec3,
    pub collision_mode: CollisionMode,
    pub collision_mode_explicit: bool,
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
            collision_mode: CollisionMode::Box,
            collision_mode_explicit: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Actor {
    pub id: u64,
    pub identity: String,

    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    pub visible: bool,
    pub enabled: bool,

    pub package: Option<String>,

    pub physics: ActorPhysicsConfig,

    pub attributes: BTreeMap<String, AttributeValue>,
    pub tags: BTreeSet<String>,
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
            tags: BTreeSet::new(),
        }
    }
}

impl Default for Actor {
    fn default() -> Self {
        Self::new(0, String::new(), Vec3::ZERO)
    }
}
