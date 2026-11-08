// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptBinding {
    pub target_identity: u64,

    pub script_path: String,

    pub enabled: bool,
}

impl ScriptBinding {
    pub fn new(target_identity: u64, script_path: impl Into<String>) -> Self {
        Self {
            target_identity,
            script_path: script_path.into(),
            enabled: true,
        }
    }

    pub fn normalized_script_path(&self) -> String {
        self.script_path.replace('\\', "/")
    }

    pub fn module_name(&self) -> String {
        module_name_from_path(&self.script_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorScriptBinding {
    pub actor_id: u64,

    pub script_path: String,

    pub enabled: bool,
}

impl ActorScriptBinding {
    pub fn new(actor_id: u64, script_path: impl Into<String>) -> Self {
        Self {
            actor_id,
            script_path: script_path.into(),
            enabled: true,
        }
    }

    pub fn normalized_script_path(&self) -> String {
        self.script_path.replace('\\', "/")
    }

    pub fn module_name(&self) -> String {
        module_name_from_path(&self.script_path)
    }

    pub fn is_module(&self) -> bool {
        Path::new(self.script_path.trim())
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("aeo"))
    }
}

fn module_name_from_path(path: &str) -> String {
    Path::new(path.replace('\\', "/").as_str())
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binding_construction() {
        let binding = ScriptBinding::new(1001, "scripts/player.aeo");

        assert_eq!(binding.target_identity, 1001);
        assert_eq!(binding.script_path, "scripts/player.aeo");
        assert_eq!(binding.module_name(), "player");
    }

    #[test]
    fn test_binding_equality() {
        let b1 = ScriptBinding::new(1, "path/a.aeo");
        let b2 = ScriptBinding::new(1, "path/a.aeo");
        let b3 = ScriptBinding::new(2, "path/a.aeo");
        let b4 = ScriptBinding::new(1, "path/b.aeo");

        assert_eq!(b1, b2);
        assert_ne!(b1, b3);
        assert_ne!(b1, b4);
    }

    #[test]
    fn test_serialization_round_trip() {
        let original = ScriptBinding::new(12345678, "assets/enemy.aeo");

        let serialized = serde_json::to_string(&original).expect("Should serialize");

        let deserialized: ScriptBinding =
            serde_json::from_str(&serialized).expect("Should deserialize");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_multiple_bindings_in_collection() {
        let mut bindings = Vec::new();

        bindings.push(ScriptBinding::new(1001, "script1.aeo"));
        bindings.push(ScriptBinding::new(2002, "script2.aeo"));

        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].target_identity, 1001);
        assert_eq!(bindings[1].target_identity, 2002);

        let serialized = serde_json::to_string(&bindings).unwrap();

        let deserialized: Vec<ScriptBinding> = serde_json::from_str(&serialized).unwrap();

        assert_eq!(bindings, deserialized);
    }

    #[test]
    fn test_actor_module_path_helpers() {
        let binding = ActorScriptBinding::new(42, r"scripts\Dungeon\combat.aeo");

        assert_eq!(
            binding.normalized_script_path(),
            "scripts/Dungeon/combat.aeo"
        );

        assert_eq!(binding.module_name(), "combat");
        assert!(binding.is_module());
    }

    #[test]
    fn test_actor_module_name_ignores_directory() {
        let binding = ActorScriptBinding::new(42, "scripts/Dungeon/systems/doors.aeo");

        assert_eq!(binding.module_name(), "doors");
    }

    #[test]
    fn test_actor_non_module_path() {
        let binding = ActorScriptBinding::new(42, "scripts/Dungeon/readme.txt");

        assert_eq!(binding.module_name(), "readme");
        assert!(!binding.is_module());
    }
}
