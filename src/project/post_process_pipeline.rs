// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::material::ParameterValue;

pub const POST_PROCESS_PIPELINE_VERSION: u32 = 1;
const MAX_PASSES: usize = 32;
const MAX_PARAMETERS_PER_PASS: usize = 64;
const MAX_OBJECT_ASSIGNMENTS: usize = 65536;
const MAX_PIPELINE_BYTES: usize = 2 * 1024 * 1024;
const MAX_ABSOLUTE_VALUE: f32 = 1_000_000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostProcessStage {
    BeforeTonemapping,
    #[default]
    AfterTonemapping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "channel", rename_all = "snake_case")]
pub enum PostProcessMask {
    All,
    Only(u8),
    Except(u8),
}

impl Default for PostProcessMask {
    fn default() -> Self {
        Self::All
    }
}

impl PostProcessMask {
    pub fn accepts(self, assigned_channel: u8) -> bool {
        match self {
            Self::All => true,
            Self::Only(channel) => channel != 0 && channel == assigned_channel,
            Self::Except(channel) => channel != assigned_channel,
        }
    }

    fn validate(self) -> Result<(), String> {
        match self {
            Self::Only(1..=8) | Self::Except(1..=8) | Self::All => Ok(()),
            _ => Err("Post-process mask channel must be between 1 and 8".into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PostProcessObjectKind {
    Actor,
    Cell,
    Character,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessObject {
    pub kind: PostProcessObjectKind,
    pub id: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessEffect {
    pub id: String,
    pub material_id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub stage: PostProcessStage,
    #[serde(default)]
    pub mask: PostProcessMask,
    #[serde(default)]
    pub parameters: BTreeMap<String, ParameterValue>,
}

impl PostProcessEffect {
    pub fn validate(&self) -> Result<(), String> {
        validate_name(&self.id)?;
        validate_name(&self.material_id)?;
        self.mask.validate()?;
        if self.parameters.len() > MAX_PARAMETERS_PER_PASS {
            return Err("Post-process effect exceeds the parameter limit".into());
        }
        for (key, value) in &self.parameters {
            validate_name(key)?;
            validate_value(value).map_err(|e| format!("Parameter {key}: {e}"))?;
        }
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    {
        return Err("Post-process ID/parameter name contains unsupported characters".into());
    }
    Ok(())
}

fn validate_value(value: &ParameterValue) -> Result<(), String> {
    let finite = |v: f32| v.is_finite() && v.abs() <= MAX_ABSOLUTE_VALUE;
    let valid = match value {
        ParameterValue::Scalar(v) => finite(*v),
        ParameterValue::Vec2(v) => v.iter().copied().all(finite),
        ParameterValue::Boolean(_) => true,
        ParameterValue::Color3(v) => v.iter().copied().all(finite),
        ParameterValue::Color4(v) => v.iter().copied().all(finite),
    };
    if valid {
        Ok(())
    } else {
        Err("Parameter values must be finite and bounded".into())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessPipeline {
    pub version: u32,
    #[serde(default)]
    pub passes: Vec<PostProcessEffect>,
    #[serde(default, with = "mask_map_json")]
    pub object_masks: BTreeMap<PostProcessObject, u8>,
}

mod mask_map_json {
    use super::*;
    #[derive(Serialize, Deserialize)]
    struct Entry {
        object: PostProcessObject,
        channel: u8,
    }
    pub fn serialize<S>(
        map: &BTreeMap<PostProcessObject, u8>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let entries: Vec<_> = map
            .iter()
            .map(|(&object, &channel)| Entry { object, channel })
            .collect();
        entries.serialize(serializer)
    }
    pub fn deserialize<'de, D>(deserializer: D) -> Result<BTreeMap<PostProcessObject, u8>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let entries = Vec::<Entry>::deserialize(deserializer)?;
        if entries.len() > MAX_OBJECT_ASSIGNMENTS {
            return Err(D::Error::custom("Post-process object mask limit exceeded"));
        }
        let mut map = BTreeMap::new();
        for entry in entries {
            if map.insert(entry.object, entry.channel).is_some() {
                return Err(D::Error::custom("Duplicate post-process object mask"));
            }
        }
        Ok(map)
    }
}

impl Default for PostProcessPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl PostProcessPipeline {
    pub fn new() -> Self {
        Self {
            version: POST_PROCESS_PIPELINE_VERSION,
            passes: Vec::new(),
            object_masks: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != POST_PROCESS_PIPELINE_VERSION {
            return Err("Unsupported post-process pipeline version".into());
        }
        if self.passes.len() > MAX_PASSES {
            return Err("Post-process pipeline exceeds 32 passes".into());
        }
        if self.object_masks.len() > MAX_OBJECT_ASSIGNMENTS {
            return Err("Post-process pipeline exceeds the object mask limit".into());
        }
        let mut ids = BTreeSet::new();
        for pass in &self.passes {
            pass.validate()?;
            if !ids.insert(pass.id.as_str()) {
                return Err(format!("Duplicate post-process pass {}", pass.id));
            }
        }
        if self.object_masks.values().any(|v| !(1..=8).contains(v)) {
            return Err("Object mask channel must be between 1 and 8".into());
        }
        Ok(())
    }

    pub fn object_channel(&self, object: PostProcessObject) -> u8 {
        self.object_masks.get(&object).copied().unwrap_or(0)
    }

    pub fn set_object_channel(
        &mut self,
        object: PostProcessObject,
        channel: u8,
    ) -> Result<(), String> {
        if channel > 8 {
            return Err("Object mask channel must be between 0 and 8".into());
        }
        if channel == 0 {
            self.object_masks.remove(&object);
        } else {
            if !self.object_masks.contains_key(&object)
                && self.object_masks.len() >= MAX_OBJECT_ASSIGNMENTS
            {
                return Err("Post-process pipeline exceeds the object mask limit".into());
            }
            self.object_masks.insert(object, channel);
        }
        Ok(())
    }

    pub fn ordered_enabled(
        &self,
        stage: PostProcessStage,
    ) -> impl Iterator<Item = &PostProcessEffect> {
        self.passes
            .iter()
            .filter(move |pass| pass.enabled && pass.stage == stage)
    }

    pub fn path(project: &Path) -> std::path::PathBuf {
        project
            .join(".assets")
            .join("post_process")
            .join("pipeline.json")
    }

    pub fn load(project: &Path) -> Result<Self, String> {
        let path = Self::path(project);
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::new()),
            Err(e) => return Err(format!("Reading post-process pipeline: {e}")),
        };
        if data.len() > MAX_PIPELINE_BYTES {
            return Err("Post-process pipeline file is too large".into());
        }
        let pipeline: Self = serde_json::from_slice(&data)
            .map_err(|e| format!("Invalid post-process pipeline: {e}"))?;
        pipeline.validate()?;
        Ok(pipeline)
    }

    pub fn save(&self, project: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_PIPELINE_BYTES {
            return Err("Post-process pipeline exceeds file size limit".into());
        }
        super::atomic_file::write_atomically(&Self::path(project), &bytes)
            .map_err(|e| format!("Saving post-process pipeline: {e}"))
    }
}

impl Default for PostProcessEffect {
    fn default() -> Self {
        Self {
            id: String::new(),
            material_id: String::new(),
            enabled: false,
            stage: PostProcessStage::AfterTonemapping,
            mask: PostProcessMask::All,
            parameters: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PostProcessCommand {
    SetParameter {
        pass_id: String,
        name: String,
        value: ParameterValue,
    },
    SetEnabled {
        pass_id: String,
        enabled: bool,
    },
    SetObjectChannel {
        object: PostProcessObject,
        channel: u8,
    },
}

#[derive(Clone, Debug, Default)]
pub struct PostProcessRuntime {
    parameter_overrides: BTreeMap<(String, String), ParameterValue>,
    enabled_overrides: BTreeMap<String, bool>,
    object_overrides: BTreeMap<PostProcessObject, u8>,
}

impl PostProcessRuntime {
    pub fn set_parameter(
        &mut self,
        authored: &PostProcessPipeline,
        pass_id: &str,
        name: &str,
        value: ParameterValue,
    ) -> Result<(), String> {
        let pass = authored
            .passes
            .iter()
            .find(|p| p.id == pass_id)
            .ok_or("Unknown post-process pass")?;
        let original = pass
            .parameters
            .get(name)
            .ok_or("Unknown post-process parameter")?;
        if std::mem::discriminant(original) != std::mem::discriminant(&value) {
            return Err("Post-process parameter type mismatch".into());
        }
        validate_value(&value)?;
        self.parameter_overrides
            .insert((pass_id.into(), name.into()), value);
        Ok(())
    }

    pub fn set_enabled(
        &mut self,
        authored: &PostProcessPipeline,
        pass_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        if !authored.passes.iter().any(|p| p.id == pass_id) {
            return Err("Unknown post-process pass".into());
        }
        self.enabled_overrides.insert(pass_id.into(), enabled);
        Ok(())
    }

    pub fn set_object_channel(
        &mut self,
        object: PostProcessObject,
        channel: u8,
    ) -> Result<(), String> {
        if channel > 8 {
            return Err("Object mask channel must be between 0 and 8".into());
        }
        if !self.object_overrides.contains_key(&object)
            && self.object_overrides.len() >= MAX_OBJECT_ASSIGNMENTS
        {
            return Err("Runtime post-process object mask limit exceeded".into());
        }
        self.object_overrides.insert(object, channel);
        Ok(())
    }

    pub fn object_channel(&self, authored: &PostProcessPipeline, object: PostProcessObject) -> u8 {
        self.object_overrides
            .get(&object)
            .copied()
            .unwrap_or_else(|| authored.object_channel(object))
    }

    pub fn effective_object_channels(
        &self,
        authored: &PostProcessPipeline,
    ) -> BTreeMap<PostProcessObject, u8> {
        let mut effective = authored.object_masks.clone();
        for (&object, &channel) in &self.object_overrides {
            if channel == 0 {
                effective.remove(&object);
            } else {
                effective.insert(object, channel);
            }
        }
        effective
    }

    pub fn effect_parameters(
        &self,
        authored: &PostProcessEffect,
    ) -> BTreeMap<String, ParameterValue> {
        let mut params = authored.parameters.clone();
        for (name, value) in &mut params {
            if let Some(override_value) = self
                .parameter_overrides
                .get(&(authored.id.clone(), name.clone()))
            {
                *value = override_value.clone();
            }
        }
        params
    }

    pub fn resolved(&self, authored: &PostProcessPipeline) -> Vec<PostProcessEffect> {
        authored
            .passes
            .iter()
            .map(|pass| {
                let mut effective = pass.clone();
                effective.enabled = self
                    .enabled_overrides
                    .get(&pass.id)
                    .copied()
                    .unwrap_or(pass.enabled);
                effective.parameters = self.effect_parameters(pass);
                effective
            })
            .collect()
    }

    pub fn reset(&mut self) {
        self.parameter_overrides.clear();
        self.enabled_overrides.clear();
        self.object_overrides.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass() -> PostProcessEffect {
        PostProcessEffect {
            id: "grain".into(),
            material_id: "mat-012345".into(),
            enabled: true,
            stage: PostProcessStage::AfterTonemapping,
            mask: PostProcessMask::Only(2),
            parameters: BTreeMap::from([("strength".into(), ParameterValue::Scalar(0.4))]),
        }
    }

    #[test]
    fn zero_effects_preserve_legacy_scene() {
        let pipeline = PostProcessPipeline::new();
        assert!(pipeline.validate().is_ok());
        assert_eq!(
            pipeline
                .ordered_enabled(PostProcessStage::AfterTonemapping)
                .count(),
            0
        );
    }

    #[test]
    fn object_masks_are_namespaced_bounded_and_optional() {
        let mut pipeline = PostProcessPipeline::new();
        let actor = PostProcessObject {
            kind: PostProcessObjectKind::Actor,
            id: 42,
        };
        let cell = PostProcessObject {
            kind: PostProcessObjectKind::Cell,
            id: 42,
        };
        pipeline.set_object_channel(actor, 2).unwrap();
        assert_eq!(pipeline.object_channel(actor), 2);
        assert_eq!(pipeline.object_channel(cell), 0);
        assert!(PostProcessMask::Only(2).accepts(pipeline.object_channel(actor)));
        assert!(!PostProcessMask::Only(2).accepts(pipeline.object_channel(cell)));
        assert!(pipeline.set_object_channel(cell, 9).is_err());
        pipeline.set_object_channel(actor, 0).unwrap();
        assert_eq!(pipeline.object_channel(actor), 0);
    }

    #[test]
    fn rejects_duplicate_passes_nonfinite_values_and_invalid_channels() {
        let mut pipeline = PostProcessPipeline::new();
        pipeline.passes = vec![pass(), pass()];
        assert!(pipeline.validate().unwrap_err().contains("Duplicate"));
        pipeline.passes.pop();
        pipeline.passes[0].mask = PostProcessMask::Only(9);
        assert!(pipeline.validate().is_err());
        pipeline.passes[0].mask = PostProcessMask::All;
        pipeline.passes[0]
            .parameters
            .insert("strength".into(), ParameterValue::Scalar(f32::NAN));
        assert!(pipeline.validate().is_err());
    }

    #[test]
    fn runtime_changes_are_transient_and_typed() {
        let mut authored = PostProcessPipeline::new();
        authored.passes.push(pass());
        let mut runtime = PostProcessRuntime::default();
        runtime
            .set_parameter(&authored, "grain", "strength", ParameterValue::Scalar(0.9))
            .unwrap();
        assert!(
            runtime
                .set_parameter(
                    &authored,
                    "grain",
                    "strength",
                    ParameterValue::Boolean(true)
                )
                .is_err()
        );
        runtime.set_enabled(&authored, "grain", false).unwrap();
        let changed = runtime.resolved(&authored);
        assert!(!changed[0].enabled);
        assert_eq!(
            changed[0].parameters["strength"],
            ParameterValue::Scalar(0.9)
        );
        assert_eq!(
            authored.passes[0].parameters["strength"],
            ParameterValue::Scalar(0.4)
        );
        runtime.reset();
        assert_eq!(runtime.resolved(&authored), authored.passes);
    }

    #[test]
    fn authored_and_play_mask_channels_resolve_without_mutating_assets() {
        let mut authored = PostProcessPipeline::new();
        let actor = PostProcessObject {
            kind: PostProcessObjectKind::Actor,
            id: 42,
        };
        let cell = PostProcessObject {
            kind: PostProcessObjectKind::Cell,
            id: 42,
        };
        authored.set_object_channel(actor, 2).unwrap();
        let mut runtime = PostProcessRuntime::default();
        runtime.set_object_channel(actor, 0).unwrap();
        runtime.set_object_channel(cell, 7).unwrap();
        assert_eq!(runtime.object_channel(&authored, actor), 0);
        assert_eq!(
            runtime.effective_object_channels(&authored).get(&actor),
            None
        );
        assert_eq!(
            runtime.effective_object_channels(&authored).get(&cell),
            Some(&7)
        );
        assert_eq!(authored.object_channel(actor), 2);
        assert!(PostProcessMask::Only(7).accepts(7));
        assert!(!PostProcessMask::Only(7).accepts(0));
        assert!(!PostProcessMask::Except(7).accepts(7));
        runtime.reset();
        assert_eq!(runtime.object_channel(&authored, actor), 2);
        assert!(
            runtime
                .effective_object_channels(&authored)
                .get(&cell)
                .is_none()
        );
    }

    #[test]
    fn saved_pipeline_preserves_order_and_mask_assignments() {
        let root =
            std::env::temp_dir().join(format!("wk-postprocess-{:032x}", rand::random::<u128>()));
        let mut authored = PostProcessPipeline::new();
        authored.passes.push(pass());
        authored.passes.push(PostProcessEffect {
            id: "next".into(),
            material_id: "mat-next".into(),
            stage: PostProcessStage::BeforeTonemapping,
            ..PostProcessEffect::default()
        });
        authored
            .set_object_channel(
                PostProcessObject {
                    kind: PostProcessObjectKind::Actor,
                    id: 1,
                },
                2,
            )
            .unwrap();
        authored.save(&root).unwrap();
        assert_eq!(PostProcessPipeline::load(&root).unwrap(), authored);
        let file = PostProcessPipeline::path(&root);
        std::fs::write(&file, b"{invalid").unwrap();
        assert!(PostProcessPipeline::load(&root).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
