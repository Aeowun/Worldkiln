// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::project::atomic_file::write_atomically;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const ANIMATION_ASSET_VERSION: u32 = 1;
pub const ANIMATION_FILE: &str = "animation.json";

pub fn registered_clips_for_rig(project: &Path, rig_id: &str) -> Vec<(String, PathBuf)> {
    use crate::project::asset_registry::{AssetRegistry, AssetType};
    let Ok(registry) = AssetRegistry::load(project) else {
        return Vec::new();
    };
    let mut eligible = vec![rig_id.to_owned()];
    if let Some(character) = registry.find(rig_id)
        && character.kind == AssetType::Character
        && let Ok(bytes) = fs::read(project.join(&character.path).join("package.json"))
        && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && let Some(owner) = json.get("source_actor").and_then(|v| v.as_str())
        && let Some(actor) = registry.by_type(AssetType::Actor).find(|candidate| {
            candidate.id.eq_ignore_ascii_case(owner)
                || candidate.name.eq_ignore_ascii_case(owner)
                || Path::new(&candidate.path)
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(owner))
        })
    {
        eligible.push(actor.id.clone());
    }
    let mut clips = registry
        .assets
        .iter()
        .filter(|a| {
            a.kind == AssetType::Animation
                && a.dependencies
                    .iter()
                    .any(|id| eligible.iter().any(|rig| rig == id))
        })
        .map(|a| (a.name.clone(), project.join(&a.path)))
        .collect::<Vec<_>>();
    clips.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then_with(|| a.1.cmp(&b.1))
    });
    clips
}

pub fn suggested_looping(name: &str) -> bool {
    let n = name.trim().to_ascii_lowercase();
    ["_loop", ".loop", "-loop", " loop", "_cycle", " cycle"]
        .iter()
        .any(|ending| n.ends_with(ending))
}

pub fn visible_clip_names(project: &Path, asset_id: &str) -> Vec<String> {
    use crate::project::asset_registry::{AssetRegistry, AssetType};
    let Ok(registry) = AssetRegistry::load(project) else {
        return Vec::new();
    };
    let Some(record) = registry.find(asset_id) else {
        return Vec::new();
    };
    let mut rig_ids = vec![record.id.clone()];
    let mut actor_record = if record.kind == AssetType::Actor {
        Some(record)
    } else {
        None
    };
    if record.kind == AssetType::Character {
        if let Ok(bytes) = std::fs::read(project.join(&record.path).join("package.json"))
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(owner) = json.get("source_actor").and_then(|v| v.as_str())
        {
            actor_record = registry.by_type(AssetType::Actor).find(|candidate| {
                candidate.id.eq_ignore_ascii_case(owner)
                    || candidate.name.eq_ignore_ascii_case(owner)
                    || Path::new(&candidate.path)
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(owner))
            });
            if let Some(actor) = actor_record {
                rig_ids.push(actor.id.clone());
            }
        }
    }
    let mut names = std::collections::BTreeMap::<String, String>::new();
    if let Some(actor) = actor_record {
        if let Ok(bytes) = std::fs::read(project.join(&actor.path).join("import.json"))
            && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(list) = json.get("clips").and_then(|v| v.as_array())
        {
            for name in list.iter().filter_map(|x| x.as_str()) {
                names.insert(name.to_ascii_lowercase(), name.to_owned());
            }
        }
    }
    for animation in registry.by_type(AssetType::Animation) {
        if animation
            .dependencies
            .iter()
            .any(|id| rig_ids.iter().any(|rig| rig == id))
        {
            names.insert(animation.name.to_ascii_lowercase(), animation.name.clone());
        }
    }
    names.into_values().collect()
}

pub fn registered_clips_for_actor(project: &Path, actor_name: &str) -> Vec<(String, PathBuf)> {
    use crate::project::asset_registry::{AssetRegistry, AssetType};
    let Ok(registry) = AssetRegistry::load(project) else {
        return Vec::new();
    };
    let Some(actor) = registry.assets.iter().find(|record| {
        record.kind == AssetType::Actor
            && (record.name.eq_ignore_ascii_case(actor_name)
                || Path::new(&record.path)
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(actor_name)))
    }) else {
        return Vec::new();
    };
    registered_clips_for_rig(project, &actor.id)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationAsset {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub source: AnimationSource,
    pub duration: f32,
    pub frames_per_second: f32,
    pub looping: bool,
    #[serde(default)]
    pub root_motion: RootMotionSettings,
    #[serde(default)]
    pub tracks: Vec<TransformTrack>,
    #[serde(default)]
    pub events: Vec<AnimationEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationSource {
    pub rig_asset_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_clip: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Step,
    #[default]
    Linear,
    Cubic,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vec3Key {
    pub time: f32,
    pub value: [f32; 3],
    #[serde(default)]
    pub interpolation: Interpolation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_tangent: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_tangent: Option<[f32; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuatKey {
    pub time: f32,
    pub value: [f32; 4],
    #[serde(default)]
    pub interpolation: Interpolation,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TransformTrack {
    pub joint_id: String,
    pub joint_name: String,
    #[serde(default)]
    pub translation: Vec<Vec3Key>,
    #[serde(default)]
    pub rotation: Vec<QuatKey>,
    #[serde(default)]
    pub scale: Vec<Vec3Key>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimationEvent {
    pub id: String,
    pub time: f32,
    pub name: String,
    #[serde(default)]
    pub payload: BTreeMap<String, EventValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventValue {
    Bool(bool),
    Number(f64),
    Text(String),
    Vec3([f32; 3]),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RootMotionSettings {
    pub enabled: bool,
    #[serde(default)]
    pub joint_id: String,
    #[serde(default = "default_axes")]
    pub translation_axes: [bool; 3],
    #[serde(default)]
    pub rotation_y: bool,
}

fn default_axes() -> [bool; 3] {
    [true, false, true]
}

impl Default for RootMotionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            joint_id: String::new(),
            translation_axes: default_axes(),
            rotation_y: false,
        }
    }
}

impl AnimationAsset {
    pub fn new(name: impl Into<String>, rig_asset_id: impl Into<String>) -> Self {
        Self {
            version: ANIMATION_ASSET_VERSION,
            id: format!("{:032x}", rand::random::<u128>()),
            name: name.into(),
            source: AnimationSource {
                rig_asset_id: rig_asset_id.into(),
                imported_clip: None,
            },
            duration: 1.0,
            frames_per_second: 30.0,
            looping: true,
            root_motion: RootMotionSettings::default(),
            tracks: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != ANIMATION_ASSET_VERSION {
            return Err(format!("Unsupported animation version {}", self.version));
        }
        if self.id.trim().is_empty() || self.name.trim().is_empty() {
            return Err("Animation ID and name are required".into());
        }
        if self.source.rig_asset_id.trim().is_empty() {
            return Err("Animation rig AssetID is required".into());
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            return Err("Animation duration must be greater than zero".into());
        }
        if !self.frames_per_second.is_finite() || self.frames_per_second <= 0.0 {
            return Err("Animation FPS must be greater than zero".into());
        }
        for track in &self.tracks {
            if track.joint_id.trim().is_empty() && track.joint_name.trim().is_empty() {
                return Err("Every animation track needs a joint binding".into());
            }
            validate_key_times(track.translation.iter().map(|key| key.time), self.duration)?;
            validate_key_times(track.rotation.iter().map(|key| key.time), self.duration)?;
            validate_key_times(track.scale.iter().map(|key| key.time), self.duration)?;
        }
        validate_key_times(self.events.iter().map(|event| event.time), self.duration)?;
        Ok(())
    }

    pub fn normalize(&mut self) {
        for track in &mut self.tracks {
            track.translation.sort_by(|a, b| a.time.total_cmp(&b.time));
            track.rotation.sort_by(|a, b| a.time.total_cmp(&b.time));
            track.scale.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
        self.events
            .sort_by(|a, b| a.time.total_cmp(&b.time).then_with(|| a.id.cmp(&b.id)));
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|error| format!("Cannot read animation: {error}"))?;
        let mut asset: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Cannot parse animation: {error}"))?;
        asset.normalize();
        asset.validate()?;
        Ok(asset)
    }

    pub fn save_authoritative(&self, project: &Path) -> Result<PathBuf, String> {
        self.validate()?;
        let path = animation_asset_path(project, &self.id)?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("Cannot serialize animation: {error}"))?;
        let _: Self = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Cannot validate animation save: {error}"))?;
        if let Err(atomic_error) = write_atomically(&path, &bytes) {
            fs::create_dir_all(path.parent().expect("animation path has parent"))
                .map_err(|error| format!("Cannot create animation folder: {error}"))?;
            fs::write(&path, &bytes).map_err(|direct_error| {
                format!(
                    "Cannot save animation: {direct_error} (atomic replacement also failed: {atomic_error})"
                )
            })?;
        }
        Ok(path)
    }
}

fn validate_key_times(times: impl IntoIterator<Item = f32>, duration: f32) -> Result<(), String> {
    let mut prior = None;
    for time in times {
        if !time.is_finite() || time < 0.0 || time > duration {
            return Err(format!(
                "Animation key time {time} is outside 0..={duration}"
            ));
        }
        if prior.is_some_and(|value| time < value) {
            return Err("Animation keys must be sorted by time".into());
        }
        prior = Some(time);
    }
    Ok(())
}

pub fn animation_asset_path(project: &Path, id: &str) -> Result<PathBuf, String> {
    let id_path = Path::new(id);
    if id.trim().is_empty()
        || id_path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || id.contains(['/', '\\'])
    {
        return Err("Animation AssetID is not a safe project path".into());
    }
    Ok(project
        .join(".assets")
        .join("animations")
        .join(id)
        .join(ANIMATION_FILE))
}

impl From<io::Error> for EventValue {
    fn from(error: io::Error) -> Self {
        Self::Text(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_animation_asset_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn authoritative_save_round_trips_a_project_animation() {
        let project = root();
        let mut asset = AnimationAsset::new("Wave", "character-id");
        asset.events.push(AnimationEvent {
            id: "event-id".into(),
            time: 0.5,
            name: "wave_peak".into(),
            payload: BTreeMap::from([("strength".into(), EventValue::Number(0.8))]),
        });
        let path = asset.save_authoritative(&project).unwrap();
        assert_eq!(AnimationAsset::load(&path).unwrap(), asset);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn animation_ids_cannot_escape_the_project_asset_folder() {
        let project = root();
        assert!(animation_asset_path(&project, "../outside").is_err());
        assert!(animation_asset_path(&project, "nested/id").is_err());
    }
}
