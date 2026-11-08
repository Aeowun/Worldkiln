// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct RecentProjects {
    pub paths: Vec<String>,
}
