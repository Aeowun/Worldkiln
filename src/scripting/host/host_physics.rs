// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::ScriptHostBridge;
use crate::engine::physics::raycast::{self, RayHit, RayQuery};

impl<'a> ScriptHostBridge<'a> {
    pub fn ray_query(&self, query: RayQuery) -> Result<Vec<RayHit>, String> {
        raycast::query(
            self.world,
            self.entity_manager,
            self.actor_system.as_deref(),
            self.character_system.as_deref(),
            &query,
        )
    }
}
