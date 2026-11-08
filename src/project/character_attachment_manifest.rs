// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CHARACTER_MANIFEST_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterManifest {
    pub version: u32,
    #[serde(default)]
    pub attachments: Vec<CharacterAttachmentTemplate>,
}

impl Default for CharacterManifest {
    fn default() -> Self {
        Self {
            version: CHARACTER_MANIFEST_VERSION,
            attachments: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterAttachmentTemplate {
    pub id: String,
    pub name: String,
    pub joint_target: Option<String>,
    pub local_origin_offset: [f32; 3],
    pub local_rotation: [f32; 4],
    pub enabled: bool,
    pub component: AttachmentComponent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AttachmentComponent {
    PointLight {
        color: [f32; 3],
        intensity: f32,
        range: f32,
        shadows: bool,
    },
    SpatialAudio {
        audio: Option<String>,
        play_on_start: bool,
        looped: bool,
        volume: f32,
        min_distance: f32,
        max_distance: f32,
    },
    ParticleEmitter {
        effect: Option<String>,
        preset: String,
        rate: f32,
        scale: f32,
        radius: f32,
    },
}

impl CharacterAttachmentTemplate {
    pub fn point_light(name: &str, joint: &str) -> Self {
        Self {
            id: format!("attachment-{:032x}", rand::random::<u128>()),
            name: name.into(),
            joint_target: (!joint.trim().is_empty() && joint != "Actor Origin")
                .then(|| joint.into()),
            local_origin_offset: [0.0; 3],
            local_rotation: [0.0, 0.0, 0.0, 1.0],
            enabled: true,
            component: AttachmentComponent::PointLight {
                color: [1.0; 3],
                intensity: 5.0,
                range: 10.0,
                shadows: true,
            },
        }
    }
}

impl CharacterManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != CHARACTER_MANIFEST_VERSION {
            return Err("Unsupported Character Manifest version.".into());
        }
        let mut names = std::collections::HashSet::new();
        let mut ids = std::collections::HashSet::new();
        for row in &self.attachments {
            let name = row.name.trim();
            if name.is_empty() || !names.insert(name.to_ascii_lowercase()) {
                return Err("Attachment names must be unique and cannot be blank.".into());
            }
            if row.id.trim().is_empty() || !ids.insert(row.id.clone()) {
                return Err("Attachment IDs must be unique and cannot be blank.".into());
            }
            if row.local_origin_offset.iter().any(|v| !v.is_finite())
                || row.local_rotation.iter().any(|v| !v.is_finite())
            {
                return Err("Enter a valid value within the supported range.".into());
            }
            match &row.component {
                AttachmentComponent::PointLight {
                    color,
                    intensity,
                    range,
                    ..
                } if color
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    || !intensity.is_finite()
                    || *intensity < 0.0
                    || !range.is_finite()
                    || *range < 0.0 =>
                {
                    return Err("Enter a valid value within the supported range.".into());
                }
                AttachmentComponent::SpatialAudio {
                    volume,
                    min_distance,
                    max_distance,
                    ..
                } if !volume.is_finite()
                    || *volume < 0.0
                    || !min_distance.is_finite()
                    || *min_distance < 0.0
                    || !max_distance.is_finite()
                    || max_distance < min_distance =>
                {
                    return Err("Enter a valid value within the supported range.".into());
                }
                AttachmentComponent::ParticleEmitter {
                    rate,
                    scale,
                    radius,
                    ..
                } if !rate.is_finite()
                    || *rate < 0.0
                    || !scale.is_finite()
                    || *scale <= 0.0
                    || !radius.is_finite()
                    || *radius < 0.0 =>
                {
                    return Err("Enter a valid value within the supported range.".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}

pub fn path(project: &Path) -> PathBuf {
    project.join(".worldkiln/character-attachments.json")
}
pub fn load(project: &Path) -> Result<CharacterManifest, String> {
    let file = path(project);
    if !file.exists() {
        return Ok(CharacterManifest::default());
    }
    let manifest: CharacterManifest = serde_json::from_slice(
        &std::fs::read(&file)
            .map_err(|e| format!("Character Attachments could not be loaded: {e}"))?,
    )
    .map_err(|e| format!("Character Attachments could not be loaded: {e}"))?;
    manifest.validate()?;
    Ok(manifest)
}
pub fn load_usable_for_play(project: &Path) -> Result<(CharacterManifest, Vec<String>), String> {
    let file = path(project);
    if !file.exists() {
        return Ok((CharacterManifest::default(), Vec::new()));
    }
    let data: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&file)
            .map_err(|e| format!("Character Attachments could not be loaded: {e}"))?,
    )
    .map_err(|e| format!("Character Attachments could not be loaded: {e}"))?;
    let version = data.get("version").and_then(|v| v.as_u64());
    if version != Some(CHARACTER_MANIFEST_VERSION as u64) {
        return Err("Unsupported Character Manifest version.".into());
    }
    let rows = data
        .get("attachments")
        .and_then(|v| v.as_array())
        .ok_or("Character Manifest attachments must be a list")?;
    let mut valid = CharacterManifest::default();
    let mut warnings = Vec::new();
    for (index, raw) in rows.iter().enumerate() {
        match serde_json::from_value::<CharacterAttachmentTemplate>(raw.clone()) {
            Ok(template) => {
                let mut candidate = valid.clone();
                candidate.attachments.push(template);
                match candidate.validate() {
                    Ok(()) => valid = candidate,
                    Err(reason) => {
                        warnings.push(format!("Manifest row {} skipped: {}", index + 1, reason))
                    }
                }
            }
            Err(reason) => warnings.push(format!("Manifest row {} skipped: {}", index + 1, reason)),
        }
    }
    Ok((valid, warnings))
}

pub fn save(project: &Path, manifest: &CharacterManifest) -> Result<(), String> {
    manifest.validate()?;
    let file = path(project);
    std::fs::create_dir_all(file.parent().unwrap())
        .map_err(|e| format!("Character Attachments could not be saved: {e}"))?;
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|e| format!("Character Attachments could not be saved: {e}"))?;
    crate::project::atomic_file::write_atomically(&file, &bytes)
        .map_err(|e| format!("Character Attachments could not be saved: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trip_preserves_order_and_component_values() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_manifest_{:032x}",
            rand::random::<u128>()
        ));
        let manifest = CharacterManifest {
            version: CHARACTER_MANIFEST_VERSION,
            attachments: vec![CharacterAttachmentTemplate::point_light(
                "Lantern",
                "RightHand",
            )],
        };
        save(&root, &manifest).unwrap();
        assert_eq!(load(&root).unwrap(), manifest);
        std::fs::remove_dir_all(root).ok();
    }
}
