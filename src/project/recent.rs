// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct RecentProjects {
    pub paths: Vec<String>,
}
