use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct RecentProjects {
    pub paths: Vec<String>,
}
