// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path};

pub const DOCUMENT_VERSION: u32 = 1;
pub const MAX_NODES: usize = 512;
const MAX_KEYS: usize = 256;
const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectDocument {
    pub version: u32,
    pub id: String,
    pub name: String,
    pub roots: Vec<EffectNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmitterKind {
    #[default]
    Sprite,
    Ring,
    Ribbon,
    Track,
    Mesh,
}
impl EmitterKind {
    pub const MENU: [Self; 5] = [
        Self::Sprite,
        Self::Ring,
        Self::Ribbon,
        Self::Track,
        Self::Mesh,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sprite => "Sprite",
            Self::Ring => "Ring",
            Self::Ribbon => "Ribbon",
            Self::Track => "Track",
            Self::Mesh => "Mesh",
        }
    }
    pub const fn supports_existing_effect(self) -> bool {
        matches!(self, Self::Sprite | Self::Ring | Self::Ribbon | Self::Track)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpawnShape {
    #[default]
    Point,
    Circle,
    Sphere,
    Line,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectNode {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub kind: EmitterKind,
    pub enabled: bool,
    pub emission_rate: Option<f32>,
    pub lifetime_seconds: Option<f32>,
    pub texture: Option<String>,
    #[serde(default)]
    pub spawn_shape: SpawnShape,
    #[serde(default)]
    pub spawn_radius: f32,
    #[serde(default)]
    pub velocity: [f32; 3],
    #[serde(default)]
    pub acceleration: [f32; 3],
    #[serde(default)]
    pub drag: f32,
    #[serde(default)]
    pub color_r: Vec<CurveKey>,
    #[serde(default)]
    pub color_g: Vec<CurveKey>,
    #[serde(default)]
    pub color_b: Vec<CurveKey>,
    pub size: Vec<CurveKey>,
    pub opacity: Vec<CurveKey>,
    pub children: Vec<EffectNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurveKey {
    pub time: f32,
    pub value: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurveChannel {
    Size,
    Opacity,
    Red,
    Green,
    Blue,
}

impl EffectNode {
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            kind: EmitterKind::Sprite,
            enabled: true,
            emission_rate: Some(20.0),
            lifetime_seconds: Some(2.0),
            texture: None,
            color_r: Vec::new(),
            color_g: Vec::new(),
            color_b: Vec::new(),
            spawn_shape: SpawnShape::Point,
            spawn_radius: 0.0,
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            drag: 0.0,
            size: Vec::new(),
            opacity: Vec::new(),
            children: Vec::new(),
        }
    }
}

const MAX_UNDO_STEPS: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct EffectEditHistory {
    undo: Vec<EffectDocument>,
    redo: Vec<EffectDocument>,
    pending: Option<EffectDocument>,
}

impl EffectEditHistory {
    pub fn begin(&mut self, document: &EffectDocument) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some(document.clone());
        true
    }

    pub fn commit(&mut self, document: &mut EffectDocument) -> Result<bool, String> {
        let before = self.pending.take().ok_or("No active effect edit")?;
        if let Err(error) = document.validate() {
            *document = before;
            return Err(error);
        }
        if *document == before {
            return Ok(false);
        }
        if self.undo.len() == MAX_UNDO_STEPS {
            self.undo.remove(0);
        }
        self.undo.push(before);
        self.redo.clear();
        Ok(true)
    }

    pub fn cancel(&mut self, document: &mut EffectDocument) -> bool {
        let Some(before) = self.pending.take() else {
            return false;
        };
        *document = before;
        true
    }

    pub fn undo(&mut self, document: &mut EffectDocument) -> bool {
        if self.pending.is_some() {
            return false;
        }
        let Some(before) = self.undo.pop() else {
            return false;
        };
        self.redo.push(std::mem::replace(document, before));
        true
    }

    pub fn redo(&mut self, document: &mut EffectDocument) -> bool {
        if self.pending.is_some() {
            return false;
        }
        let Some(next) = self.redo.pop() else {
            return false;
        };
        if self.undo.len() == MAX_UNDO_STEPS {
            self.undo.remove(0);
        }
        self.undo.push(std::mem::replace(document, next));
        true
    }

    pub fn undo_steps(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_steps(&self) -> usize {
        self.redo.len()
    }
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

impl EffectDocument {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            version: DOCUMENT_VERSION,
            id: id.into(),
            name: name.into(),
            roots: vec![],
        }
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("Effect document is too large".into());
        }
        let data: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        data.validate()?;
        Ok(data)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("Effect document exceeds the supported saved size".into());
        }
        Ok(bytes)
    }

    pub fn node(&self, id: u64) -> Option<&EffectNode> {
        fn find(nodes: &[EffectNode], id: u64) -> Option<&EffectNode> {
            for node in nodes {
                if node.id == id {
                    return Some(node);
                }
                if let Some(found) = find(&node.children, id) {
                    return Some(found);
                }
            }
            None
        }
        find(&self.roots, id)
    }

    fn node_mut(&mut self, id: u64) -> Option<&mut EffectNode> {
        fn find(nodes: &mut [EffectNode], id: u64) -> Option<&mut EffectNode> {
            for node in nodes {
                if node.id == id {
                    return Some(node);
                }
                if let Some(found) = find(&mut node.children, id) {
                    return Some(found);
                }
            }
            None
        }
        find(&mut self.roots, id)
    }

    fn change_structure<T>(
        &mut self,
        change: impl FnOnce(&mut EffectDocument) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut staged = self.clone();
        let result = change(&mut staged)?;
        staged.validate()?;
        *self = staged;
        Ok(result)
    }

    fn take_node(nodes: &mut Vec<EffectNode>, id: u64) -> Option<EffectNode> {
        if let Some(position) = nodes.iter().position(|node| node.id == id) {
            return Some(nodes.remove(position));
        }
        for node in nodes {
            if let Some(removed) = Self::take_node(&mut node.children, id) {
                return Some(removed);
            }
        }
        None
    }

    pub fn insert_node(&mut self, parent_id: Option<u64>, node: EffectNode) -> Result<(), String> {
        self.change_structure(|staged| {
            match parent_id {
                Some(id) => staged
                    .node_mut(id)
                    .ok_or("Unknown effect parent")?
                    .children
                    .push(node),
                None => staged.roots.push(node),
            }
            Ok(())
        })
    }

    pub fn remove_node(&mut self, id: u64) -> Result<EffectNode, String> {
        self.change_structure(|staged| {
            Self::take_node(&mut staged.roots, id).ok_or_else(|| "Unknown effect node".into())
        })
    }

    pub fn reparent_node(&mut self, id: u64, new_parent_id: Option<u64>) -> Result<(), String> {
        if new_parent_id == Some(id) {
            return Err("An effect node cannot parent itself".into());
        }
        self.change_structure(|staged| {
            let node = Self::take_node(&mut staged.roots, id).ok_or("Unknown effect node")?;
            match new_parent_id {
                Some(parent) => staged
                    .node_mut(parent)
                    .ok_or("Unknown or cyclic effect parent")?
                    .children
                    .push(node),
                None => staged.roots.push(node),
            }
            Ok(())
        })
    }

    pub fn set_spawn_motion(
        &mut self,
        id: u64,
        shape: SpawnShape,
        radius: f32,
        velocity: [f32; 3],
        acceleration: [f32; 3],
        drag: f32,
    ) -> Result<(), String> {
        if !radius.is_finite()
            || !(0.0..=1000.0).contains(&radius)
            || !drag.is_finite()
            || !(0.0..=100.0).contains(&drag)
            || velocity
                .iter()
                .chain(acceleration.iter())
                .any(|v| !v.is_finite() || v.abs() > 1000.0)
        {
            return Err("Invalid sprite spawn, motion or drag parameters".into());
        }
        let node = self.node_mut(id).ok_or("Unknown Effect node")?;
        node.spawn_shape = shape;
        node.spawn_radius = radius;
        node.velocity = velocity;
        node.acceleration = acceleration;
        node.drag = drag;
        Ok(())
    }

    pub fn rename_node(&mut self, id: u64, name: &str) -> Result<(), String> {
        if name.trim().is_empty()
            || name.len() > 128
            || name != name.trim()
            || name.chars().any(char::is_control)
        {
            return Err("Invalid Effect node name".into());
        }
        let mut pending: Vec<_> = self.roots.iter().collect();
        while let Some(node) = pending.pop() {
            if node.id != id && node.name == name {
                return Err("Duplicate Effect node name".into());
            }
            pending.extend(node.children.iter());
        }
        self.node_mut(id).ok_or("Unknown Effect node")?.name = name.to_owned();
        Ok(())
    }

    pub fn set_texture(
        &mut self,
        id: u64,
        texture: Option<String>,
    ) -> Result<Option<String>, String> {
        if texture.as_deref().is_some_and(|p| !valid_resource_path(p)) {
            return Err("Unsafe effect texture path".into());
        }
        let node = self.node_mut(id).ok_or("Unknown effect node")?;
        Ok(std::mem::replace(&mut node.texture, texture))
    }

    pub fn set_curve(
        &mut self,
        id: u64,
        channel: CurveChannel,
        keys: Vec<CurveKey>,
    ) -> Result<Vec<CurveKey>, String> {
        validate_curve_keys(&keys)?;
        if channel != CurveChannel::Size && keys.iter().any(|key| key.value > 1.0) {
            return Err("Color and opacity curve values must be normalized".into());
        }
        let node = self.node_mut(id).ok_or("Unknown effect node")?;
        let current = match channel {
            CurveChannel::Size => &mut node.size,
            CurveChannel::Opacity => &mut node.opacity,
            CurveChannel::Red => &mut node.color_r,
            CurveChannel::Green => &mut node.color_g,
            CurveChannel::Blue => &mut node.color_b,
        };
        Ok(std::mem::replace(current, keys))
    }

    pub fn set_emission_rate(&mut self, node_id: u64, rate: f32) -> Result<Option<f32>, String> {
        if !rate.is_finite() || !(0.0..=100_000.0).contains(&rate) {
            return Err("Invalid emission rate".into());
        }
        let node = self.node_mut(node_id).ok_or("Unknown effect node")?;
        Ok(node.emission_rate.replace(rate))
    }

    pub fn set_lifetime_seconds(
        &mut self,
        node_id: u64,
        seconds: f32,
    ) -> Result<Option<f32>, String> {
        if !seconds.is_finite() || !(0.001..=1_000.0).contains(&seconds) {
            return Err("Invalid particle lifetime".into());
        }
        let node = self.node_mut(node_id).ok_or("Unknown effect node")?;
        Ok(node.lifetime_seconds.replace(seconds))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != DOCUMENT_VERSION {
            return Err("Unsupported effect document version".into());
        }
        if self.id.is_empty()
            || self.id.len() > 128
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
        {
            return Err("Invalid effect ID".into());
        }
        if self.name.trim().is_empty() || self.name.len() > 256 {
            return Err("Invalid effect name".into());
        }
        let mut seen = HashSet::new();
        let mut pending: Vec<(&EffectNode, usize)> =
            self.roots.iter().map(|n| (n, 1usize)).collect();
        while let Some((node, depth)) = pending.pop() {
            if depth > 32 || seen.len() >= MAX_NODES {
                return Err("Effect hierarchy limit reached".into());
            }
            if node.id == 0 || !seen.insert(node.id) {
                return Err("Duplicate or zero node ID".into());
            }
            if node.name.trim().is_empty() || node.name.len() > 128 {
                return Err("Invalid effect node name".into());
            }
            if node
                .emission_rate
                .is_some_and(|n| !n.is_finite() || !(0.0..=100_000.0).contains(&n))
                || node
                    .lifetime_seconds
                    .is_some_and(|n| !n.is_finite() || !(0.001..=1_000.0).contains(&n))
            {
                return Err("Invalid emission or lifetime".into());
            }
            if let Some(texture) = &node.texture {
                if !valid_resource_path(texture) {
                    return Err("Unsafe effect texture path".into());
                }
            }
            for curve in [
                &node.size,
                &node.opacity,
                &node.color_r,
                &node.color_g,
                &node.color_b,
            ] {
                validate_curve_keys(curve)?;
            }
            for channel in [&node.opacity, &node.color_r, &node.color_g, &node.color_b] {
                if channel.iter().any(|key| key.value > 1.0) {
                    return Err("Color and opacity curve values must be normalized".into());
                }
            }
            pending.extend(node.children.iter().map(|child| (child, depth + 1)));
        }
        Ok(())
    }
}

fn validate_curve_keys(curve: &[CurveKey]) -> Result<(), String> {
    if curve.len() > MAX_KEYS {
        return Err("Too many effect curve keys".into());
    }
    let mut prev = -1.0;
    for key in curve {
        if !key.time.is_finite()
            || !key.value.is_finite()
            || !(0.0..=1.0).contains(&key.time)
            || key.time <= prev
            || !(0.0..=1000.0).contains(&key.value)
        {
            return Err("Invalid effect curve keys".into());
        }
        prev = key.time;
    }
    Ok(())
}

pub fn valid_resource_path(value: &str) -> bool {
    let p = Path::new(value);
    !value.is_empty()
        && value.len() <= 1024
        && !value.contains('\\')
        && !value.contains(':')
        && !value.chars().any(char::is_control)
        && p.components().all(|c| matches!(c, Component::Normal(_)))
        && p.extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("png"))
}

pub fn save_new_document(path: &Path, document: &EffectDocument) -> Result<(), String> {
    use std::io::Write;
    let bytes = document.to_json()?;
    if path.extension().and_then(|s| s.to_str()) != Some("json") {
        return Err("Effect source must have a .json extension".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Cannot create effect source: {e}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("Cannot save effect source: {error}"));
    }
    Ok(())
}

pub fn read_document(path: &Path) -> Result<EffectDocument, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_DOCUMENT_BYTES as u64 {
        return Err("Effect document is too large".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    EffectDocument::from_json(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fire() -> EffectDocument {
        let mut doc = EffectDocument::new("test.fire", "Fire");
        for (id, name, texture) in [
            (1, "Flames", "Texture/fire_tex.png"),
            (2, "Smoke", "Texture/smoke_tex.png"),
        ] {
            doc.roots.push(EffectNode {
                id,
                kind: EmitterKind::Sprite,
                name: name.into(),
                enabled: true,
                emission_rate: Some(18.0),
                lifetime_seconds: Some(1.5),
                texture: Some(texture.into()),
                color_r: Vec::new(),
                color_g: Vec::new(),
                color_b: Vec::new(),
                spawn_shape: SpawnShape::Point,
                spawn_radius: 0.0,
                velocity: [0.0; 3],
                acceleration: [0.0; 3],
                drag: 0.0,
                size: vec![
                    CurveKey {
                        time: 0.0,
                        value: 0.5,
                    },
                    CurveKey {
                        time: 1.0,
                        value: 1.5,
                    },
                ],
                opacity: vec![
                    CurveKey {
                        time: 0.0,
                        value: 1.0,
                    },
                    CurveKey {
                        time: 1.0,
                        value: 0.0,
                    },
                ],
                children: Vec::new(),
            });
        }
        doc
    }

    #[test]
    fn hierarchy_edits_preserve_ids_and_rollback_invalid_parenting() {
        let mut doc = EffectDocument::new("fx.structure", "Structure");
        doc.insert_node(None, EffectNode::new(1, "Root")).unwrap();
        doc.insert_node(Some(1), EffectNode::new(2, "Smoke"))
            .unwrap();
        doc.insert_node(Some(1), EffectNode::new(3, "Sparks"))
            .unwrap();
        let original = doc.clone();
        assert!(
            doc.reparent_node(1, Some(2)).is_err(),
            "cannot create an ancestor cycle"
        );
        assert!(
            doc.reparent_node(2, Some(2)).is_err(),
            "node cannot parent itself"
        );
        assert!(
            doc.insert_node(Some(999), EffectNode::new(4, "Unknown parent"))
                .is_err()
        );
        assert!(
            doc.insert_node(None, EffectNode::new(2, "Duplicate ID"))
                .is_err()
        );
        assert!(doc.remove_node(999).is_err());
        assert_eq!(doc, original, "all rejected edits must be transactional");

        doc.reparent_node(2, None).unwrap();
        assert_eq!(
            doc.roots.iter().map(|n| n.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(doc.roots[0].children[0].id, 3);
        doc.reparent_node(3, Some(2)).unwrap();
        assert_eq!(doc.roots[1].children[0].id, 3);
        let removed = doc.remove_node(2).unwrap();
        assert_eq!(removed.id, 2);
        assert_eq!(removed.children[0].id, 3);
        assert_eq!(doc.roots.len(), 1);
        doc.insert_node(Some(1), removed).unwrap();
        let encoded = doc.to_json().unwrap();
        assert_eq!(EffectDocument::from_json(&encoded).unwrap(), doc);
    }

    #[test]
    fn hierarchy_depth_limit_rejects_changes_without_losing_nodes() {
        let mut doc = EffectDocument::new("fx.deep", "Deep");
        doc.insert_node(None, EffectNode::new(1, "Root")).unwrap();
        for id in 2..=32 {
            doc.insert_node(Some(id - 1), EffectNode::new(id, "Child"))
                .unwrap();
        }
        let before = doc.clone();
        assert!(
            doc.insert_node(Some(32), EffectNode::new(33, "Too Deep"))
                .is_err()
        );
        assert_eq!(doc, before);
        doc.validate().unwrap();
    }

    #[test]
    fn texture_and_curve_edits_validate_only_changed_fields() {
        let mut doc = EffectDocument::new("fx.curves", "Curves");
        doc.insert_node(None, EffectNode::new(1, "Flame")).unwrap();
        assert_eq!(
            doc.set_texture(1, Some("Texture/fire.png".into())).unwrap(),
            None
        );
        let prior = doc.clone();
        assert!(doc.set_texture(1, Some("../unsafe.png".into())).is_err());
        assert!(
            doc.set_texture(404, Some("Texture/valid.png".into()))
                .is_err()
        );
        assert_eq!(doc, prior);

        let size = vec![
            CurveKey {
                time: 0.0,
                value: 0.5,
            },
            CurveKey {
                time: 0.5,
                value: 2.0,
            },
            CurveKey {
                time: 1.0,
                value: 0.1,
            },
        ];
        assert!(
            doc.set_curve(1, CurveChannel::Size, size.clone())
                .unwrap()
                .is_empty()
        );
        let old = doc.set_curve(1, CurveChannel::Size, vec![]).unwrap();
        assert_eq!(old, size, "return the exact prior keys for undo");
        doc.set_curve(1, CurveChannel::Size, old).unwrap();
        let snapshot = doc.clone();
        for bad in [
            vec![CurveKey {
                time: f32::NAN,
                value: 2.0,
            }],
            vec![CurveKey {
                time: 1.0,
                value: f32::INFINITY,
            }],
            vec![
                CurveKey {
                    time: 0.5,
                    value: 1.0,
                },
                CurveKey {
                    time: 0.5,
                    value: 0.0,
                },
            ],
            vec![CurveKey {
                time: -0.1,
                value: 1.0,
            }],
        ] {
            assert!(doc.set_curve(1, CurveChannel::Opacity, bad).is_err());
            assert_eq!(doc, snapshot);
        }
        assert!(
            doc.set_curve(
                1,
                CurveChannel::Size,
                vec![
                    CurveKey {
                        time: 0.0,
                        value: 1.0
                    };
                    MAX_KEYS + 1
                ]
            )
            .is_err()
        );
        assert_eq!(doc, snapshot);
        assert_eq!(
            EffectDocument::from_json(&doc.to_json().unwrap()).unwrap(),
            doc
        );
    }

    #[test]
    fn edit_history_coalesces_drag_reverses_and_cancels_without_serialization() {
        let mut doc = EffectDocument::new("fx.history", "History");
        doc.insert_node(None, EffectNode::new(1, "Flame")).unwrap();
        let original = doc.clone();
        let mut history = EffectEditHistory::default();
        assert!(history.begin(&doc));
        assert!(!history.begin(&doc), "one pending drag at a time");
        for rate in 10..=500 {
            doc.set_emission_rate(1, rate as f32).unwrap();
        }
        doc.set_curve(
            1,
            CurveChannel::Size,
            vec![
                CurveKey {
                    time: 0.0,
                    value: 0.5,
                },
                CurveKey {
                    time: 1.0,
                    value: 3.0,
                },
            ],
        )
        .unwrap();
        assert!(history.commit(&mut doc).unwrap());
        assert_eq!(history.undo_steps(), 1, "many drag updates become one undo");
        assert_eq!(history.redo_steps(), 0);
        let edited = doc.clone();
        assert!(history.undo(&mut doc));
        assert_eq!(doc, original);
        assert!(history.redo(&mut doc));
        assert_eq!(doc, edited);
        assert!(!history.redo(&mut doc));

        assert!(history.begin(&doc));
        doc.reparent_node(1, None).unwrap();
        assert!(
            !history.commit(&mut doc).unwrap(),
            "no-op must not enter history"
        );
        assert_eq!(history.undo_steps(), 1);
        assert!(history.begin(&doc));
        doc.set_emission_rate(1, 20.0).unwrap();
        assert!(history.cancel(&mut doc));
        assert_eq!(doc, edited);
        assert!(!history.cancel(&mut doc));
    }

    #[test]
    fn edit_history_rejects_invalid_commit_and_bounds_undo_memory() {
        let mut doc = EffectDocument::new("fx.historycap", "History Capacity");
        doc.insert_node(None, EffectNode::new(1, "Flame")).unwrap();
        let mut history = EffectEditHistory::default();
        let before = doc.clone();
        history.begin(&doc);
        doc.roots[0].name.clear();
        assert!(history.commit(&mut doc).is_err());
        assert_eq!(doc, before);
        assert_eq!(history.undo_steps(), 0);

        for rate in 1..=100 {
            history.begin(&doc);
            doc.set_emission_rate(1, rate as f32).unwrap();
            history.commit(&mut doc).unwrap();
        }
        assert_eq!(history.undo_steps(), MAX_UNDO_STEPS);
        assert!(history.undo(&mut doc));
        assert_eq!(history.redo_steps(), 1);
        assert!(history.begin(&doc));
        doc.set_emission_rate(1, 999.0).unwrap();
        history.commit(&mut doc).unwrap();
        assert_eq!(history.redo_steps(), 0, "new branch discards redo");
    }

    #[test]
    #[ignore = "manual backend curve drag benchmark; no UI responsiveness claim"]
    fn profile_incremental_curve_drag_without_serialization() {
        use std::time::Instant;
        for count in [1_usize, 50, 200, 500] {
            let mut doc = EffectDocument::new("profile", "Curve Drag");
            for id in 1..=count as u64 {
                doc.roots.push(EffectNode::new(id, format!("Node-{id}")));
            }
            doc.validate().unwrap();
            let mut history = EffectEditHistory::default();
            assert!(history.begin(&doc));
            let mut samples = Vec::with_capacity(250);
            for i in 0..250 {
                let start = Instant::now();
                doc.set_curve(
                    1,
                    CurveChannel::Opacity,
                    vec![
                        CurveKey {
                            time: 0.0,
                            value: i as f32 / 250.0,
                        },
                        CurveKey {
                            time: 1.0,
                            value: 0.0,
                        },
                    ],
                )
                .unwrap();
                samples.push(start.elapsed().as_secs_f64() * 1_000.0);
            }
            assert!(history.commit(&mut doc).unwrap());
            assert_eq!(history.undo_steps(), 1);
            samples.sort_by(|a, b| a.total_cmp(b));
            println!(
                "FX_CURVE_INCREMENTAL nodes={count} samples=250 p50_ms={:.5} p95_ms={:.5} worst_ms={:.5}",
                samples[125], samples[237], samples[249]
            );
        }
    }

    #[test]
    fn rust_custom_fire_document_roundtrips() {
        let document = fire();
        let bytes = document.to_json().unwrap();
        assert_eq!(document, EffectDocument::from_json(&bytes).unwrap());
        let folder = std::env::temp_dir().join(format!(
            "wk-fx-authoring-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("test.fire.json");
        save_new_document(&path, &document).unwrap();
        assert_eq!(read_document(&path).unwrap(), document);
        assert!(save_new_document(&path, &document).is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn standalone_fire_sprite_compiles_without_an_existing_creator_template() {
        let mut doc = EffectDocument::new("standalone", "Standalone Ember");
        doc.roots.push(EffectNode {
            kind: EmitterKind::Sprite,
            id: 1,
            name: "Solo Ember".into(),
            enabled: true,
            emission_rate: Some(30.0),
            lifetime_seconds: Some(2.0),
            texture: Some("Texture/fire_tex.png".into()),
            color_r: Vec::new(),
            color_g: Vec::new(),
            color_b: Vec::new(),
            spawn_shape: SpawnShape::Point,
            spawn_radius: 0.0,
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            drag: 0.0,
            size: vec![],
            opacity: vec![],
            children: vec![],
        });
        let bytes = doc.compile_new_fire_sprite().unwrap();
        let effect = super::super::fx_export::EfkEfc::parse(&bytes).unwrap();
        let restored =
            EffectDocument::from_effekseer_template("standalone", "Standalone Ember", &effect)
                .unwrap();
        assert_eq!(restored.roots.len(), 1);
        assert_eq!(restored.roots[0].name, "Solo Ember");
        assert_eq!(restored.roots[0].emission_rate, Some(30.0));
        assert_eq!(restored.roots[0].lifetime_seconds, Some(2.0));
        assert_eq!(bytes, doc.compile_new_fire_sprite().unwrap());
        doc.roots[0].size.push(CurveKey {
            time: 0.5,
            value: 2.0,
        });
        assert!(doc.compile_new_fire_sprite().is_ok());
    }

    #[test]
    fn standalone_multilayer_fire_compiles_and_preserves_independent_parameters() {
        let mut doc = EffectDocument::new("new-fire", "New Fire");
        for index in 0..4 {
            doc.roots.push(EffectNode {
                kind: EmitterKind::Sprite,
                id: index + 1,
                name: format!("Layer {index}"),
                enabled: true,
                emission_rate: Some(20.0 + index as f32 * 10.0),
                lifetime_seconds: Some(index as f32 + 1.0),
                texture: Some("Texture/fire_tex.png".into()),
                color_r: Vec::new(),
                color_g: Vec::new(),
                color_b: Vec::new(),
                spawn_shape: SpawnShape::Point,
                spawn_radius: 0.0,
                velocity: [0.0; 3],
                acceleration: [0.0; 3],
                drag: 0.0,
                size: vec![],
                opacity: vec![],
                children: vec![],
            });
        }
        let compiled = doc.compile_new_fire_sprite().unwrap();
        let parsed = super::super::fx_export::EfkEfc::parse(&compiled).unwrap();
        let restored =
            EffectDocument::from_effekseer_template("new-fire", "New Fire", &parsed).unwrap();
        assert_eq!(restored.roots.len(), 4);
        for (index, node) in restored.roots.iter().enumerate() {
            assert_eq!(node.name, format!("Layer {index}"));
            assert_eq!(node.emission_rate, Some(20.0 + index as f32 * 10.0));
            assert_eq!(node.lifetime_seconds, Some(index as f32 + 1.0));
        }
        assert_eq!(compiled, doc.compile_new_fire_sprite().unwrap());
        assert_eq!(doc.to_json().unwrap(), restored.to_json().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn rejects_invalid_ids_paths_and_curve_order() {
        let mut doc = fire();
        doc.roots[0].texture = Some("../../outside.png".into());
        assert!(doc.validate().is_err());
        doc.roots[0].texture = Some("Texture/fire_tex.png".into());
        doc.roots[0].size.swap(0, 1);
        assert!(doc.validate().is_err());
        doc.roots[0].size.swap(0, 1);
        doc.roots[1].id = 1;
        assert!(doc.validate().is_err());
    }

    #[test]
    #[ignore = "Manual authoring performance measurement"]
    fn profile_effect_document_scaling_and_repeated_edits() {
        use std::time::Instant;
        for count in [1_usize, 50, 200, 500] {
            let mut doc = EffectDocument::new("benchmark", "Performance");
            for index in 0..count {
                doc.roots.push(EffectNode {
                    kind: EmitterKind::Sprite,
                    id: index as u64 + 1,
                    name: format!("Node-{index}"),
                    enabled: true,
                    emission_rate: Some(25.0),
                    lifetime_seconds: Some(2.0),
                    texture: Some("Texture/fire_tex.png".into()),
                    color_r: Vec::new(),
                    color_g: Vec::new(),
                    color_b: Vec::new(),
                    spawn_shape: SpawnShape::Point,
                    spawn_radius: 0.0,
                    velocity: [0.0; 3],
                    acceleration: [0.0; 3],
                    drag: 0.0,
                    size: (0..32)
                        .map(|i| CurveKey {
                            time: i as f32 / 31.0,
                            value: 1.0,
                        })
                        .collect(),
                    opacity: (0..32)
                        .map(|i| CurveKey {
                            time: i as f32 / 31.0,
                            value: 1.0 - i as f32 / 31.0,
                        })
                        .collect(),
                    children: vec![],
                });
            }
            let mut samples = Vec::with_capacity(250);
            for iteration in 0..250 {
                let start = Instant::now();
                doc.roots[iteration % count].emission_rate = Some((iteration % 500) as f32);
                let json = doc.to_json().unwrap();
                let restored = EffectDocument::from_json(&json).unwrap();
                assert_eq!(restored.roots.len(), count);
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(|a, b| a.total_cmp(b));
            println!(
                "FX_AUTH_PERF nodes={count} samples=250 p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
                samples[125], samples[237], samples[249]
            );
        }
    }

    #[test]
    fn incremental_emission_edit_preserves_hierarchy_and_rejects_bad_input() {
        let mut document = fire();
        document.roots[0].children.push(EffectNode {
            kind: EmitterKind::Sprite,
            id: 3,
            name: "Embers".into(),
            enabled: true,
            emission_rate: Some(4.0),
            lifetime_seconds: Some(1.0),
            texture: None,
            color_r: Vec::new(),
            color_g: Vec::new(),
            color_b: Vec::new(),
            spawn_shape: SpawnShape::Point,
            spawn_radius: 0.0,
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            drag: 0.0,
            size: vec![],
            opacity: vec![],
            children: vec![],
        });
        assert_eq!(document.set_emission_rate(3, 40.0).unwrap(), Some(4.0));
        assert_eq!(document.set_lifetime_seconds(3, 2.5).unwrap(), Some(1.0));
        assert_eq!(document.roots[0].children[0].emission_rate, Some(40.0));
        assert_eq!(document.roots[0].children[0].lifetime_seconds, Some(2.5));
        document.roots[0].children[0].emission_rate = None;
        assert_eq!(document.set_emission_rate(3, 40.0).unwrap(), None);
        let before = document.clone();
        assert!(document.set_emission_rate(3, f32::NAN).is_err());
        assert!(document.set_emission_rate(3, 100_001.0).is_err());
        assert!(document.set_lifetime_seconds(3, 0.0).is_err());
        assert!(document.set_emission_rate(999, 2.0).is_err());
        assert_eq!(document, before);
        assert_eq!(
            EffectDocument::from_json(&document.to_json().unwrap()).unwrap(),
            document
        );
    }

    #[test]
    #[ignore = "Manual incremental property latency measurement"]
    fn profile_incremental_node_edits_without_serialization() {
        use std::time::Instant;
        for count in [1usize, 50, 200, 500] {
            let mut doc = EffectDocument::new("profiling", "Editing");
            for index in 0..count {
                doc.roots.push(EffectNode {
                    kind: EmitterKind::Sprite,
                    id: index as u64 + 1,
                    name: format!("Node-{index}"),
                    enabled: true,
                    emission_rate: Some(20.0),
                    lifetime_seconds: Some(2.0),
                    texture: None,
                    color_r: Vec::new(),
                    color_g: Vec::new(),
                    color_b: Vec::new(),
                    spawn_shape: SpawnShape::Point,
                    spawn_radius: 0.0,
                    velocity: [0.0; 3],
                    acceleration: [0.0; 3],
                    drag: 0.0,
                    size: vec![],
                    opacity: vec![],
                    children: vec![],
                });
            }
            let mut samples = Vec::with_capacity(5_000);
            for index in 0..5_000 {
                let started = Instant::now();
                let id = (index % count) as u64 + 1;
                doc.set_emission_rate(id, (index % 300) as f32 + 1.0)
                    .unwrap();
                samples.push(started.elapsed().as_secs_f64() * 1_000.0);
            }
            samples.sort_by(|a, b| a.total_cmp(b));
            println!(
                "FX_INCREMENTAL_PERF nodes={count} samples=5000 p50_us={:.2} p95_us={:.2} max_us={:.2}",
                samples[2500], samples[4750], samples[4999]
            );
        }
    }

    #[test]
    fn oversized_but_valid_authoring_graph_cannot_save_unreadable_json() {
        let mut doc = EffectDocument::new("limit.test", "Document Size Safety");
        let keys: Vec<_> = (0..MAX_KEYS)
            .map(|i| CurveKey {
                time: i as f32 / (MAX_KEYS - 1) as f32,
                value: 1.0,
            })
            .collect();
        for i in 0..MAX_NODES {
            doc.roots.push(EffectNode {
                kind: EmitterKind::Sprite,
                id: i as u64 + 1,
                name: format!("Emitter-{i}"),
                enabled: true,
                emission_rate: Some(20.0),
                lifetime_seconds: Some(2.0),
                texture: None,
                color_r: Vec::new(),
                color_g: Vec::new(),
                color_b: Vec::new(),
                spawn_shape: SpawnShape::Point,
                spawn_radius: 0.0,
                velocity: [0.0; 3],
                acceleration: [0.0; 3],
                drag: 0.0,
                size: keys.clone(),
                opacity: keys.clone(),
                children: vec![],
            });
        }
        assert!(doc.validate().is_ok());
        assert!(doc.to_json().is_err());
    }

    #[test]
    fn malformed_and_unsupported_documents_fail_closed() {
        assert!(EffectDocument::from_json(b"{broken").is_err());
        let mut doc = fire();
        doc.version = 999;
        assert!(doc.to_json().is_err());
        assert!(EffectDocument::from_json(&vec![b'x'; 4 * 1024 * 1024 + 1]).is_err());
    }
}

impl EffectDocument {
    pub fn from_effekseer_template(
        id: &str,
        name: &str,
        effect: &super::fx_export::EfkEfc,
    ) -> Result<Self, String> {
        let tree = effect.editor_tree()?;
        let parent = tree
            .iter()
            .find(|v| v.name == "EffekseerProject")
            .ok_or("Missing Effekseer editor root")?;
        let children = parent
            .child("Root")
            .and_then(|v| v.child("Children"))
            .ok_or("Missing effect nodes")?;
        fn nodes(
            source: &[super::fx_export::EditorElement],
            id: &mut u64,
        ) -> Result<Vec<EffectNode>, String> {
            source
                .iter()
                .filter(|x| x.name == "Node")
                .map(|source| -> Result<EffectNode, String> {
                    let native_type = source.child("DrawingValues")
                        .and_then(|drawing| drawing.child_value("Type"));
                    let emitter_kind = match native_type {
                        None | Some("2") => EmitterKind::Sprite,
                        Some("4") if source.child_value("AeoFXRenderer")==Some("Ring")
                            && source.child_value("AeoFXStableId").is_some()
                            => EmitterKind::Ring,
                        Some("3") if source.child_value("AeoFXRenderer")==Some("Ribbon")
                            && source.child_value("AeoFXStableId").is_some()
                            => EmitterKind::Ribbon,
                        Some("6") if source.child_value("AeoFXRenderer")==Some("Track")
                            && source.child_value("AeoFXStableId").is_some()
                            => EmitterKind::Track,
                        Some(other) => return Err(format!(
                            "Unsupported Effekseer renderer type {other}; native playback is available, but its authoring serializer has not been verified"
                        )),
                    };
                    let assigned = match source.child_value("AeoFXStableId") {
                        Some(text) => text
                            .parse::<u64>()
                            .map_err(|_| "Invalid AeoFX stable node ID")?,
                        None => *id,
                    };
                    *id = (*id).max(assigned.saturating_add(1));
                    let texture = source
                        .child("RendererCommonValues")
                        .and_then(|v| v.child_value("ColorTexture"))
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned);
                    let common = source.child("CommonValues");
                    let uniform = |element: Option<&super::fx_export::EditorElement>| {
                        let element = element?;
                        let min = element.child_value("Min")?.parse::<f32>().ok()?;
                        let max = element.child_value("Max")?.parse::<f32>().ok()?;
                        if !min.is_finite() || min <= 0.0 || min != max {
                            return None;
                        }
                        Some(min)
                    };
                    let lifetime_seconds = uniform(common.and_then(|c| c.child("Life")))
                        .map(|frames| frames / 60.0)
                        .filter(|seconds| (0.001..=1000.0).contains(seconds));
                    let emission_rate = uniform(
                        common
                            .and_then(|c| c.child("Generation"))
                            .and_then(|generation| generation.child("GenerationTime")),
                    )
                    .map(|frames| {
                        let rate = 60.0 / frames;
                        let integer_rate = rate.round();
                        if (rate - integer_rate).abs() <= rate.abs().max(1.0) * 0.000001 {
                            integer_rate
                        } else {
                            rate
                        }
                    })
                    .filter(|rate| rate.is_finite() && (0.0..=100_000.0).contains(rate));
                    let curve = |label: &str| -> Result<Vec<CurveKey>, String> {
                        let Some(curve) = source.child(label) else {
                            return Ok(Vec::new());
                        };
                        if curve.children.is_empty() || curve.children.len() > MAX_KEYS {
                            return Err("Invalid AeoFX curve key count".into());
                        }
                        let mut keys = Vec::with_capacity(curve.children.len());
                        for key in &curve.children {
                            if key.name != "Key" {
                                return Err("Invalid AeoFX curve entry".into());
                            }
                            let time = key
                                .child_value("Time")
                                .ok_or("Missing AeoFX curve time")?
                                .parse::<f32>()
                                .map_err(|_| "Invalid AeoFX curve time")?;
                            let value = key
                                .child_value("Value")
                                .ok_or("Missing AeoFX curve value")?
                                .parse::<f32>()
                                .map_err(|_| "Invalid AeoFX curve value")?;
                            keys.push(CurveKey { time, value });
                        }
                        Ok(keys)
                    };
                    let size = curve("AeoFXSizeCurve")?;
                    let opacity = curve("AeoFXOpacityCurve")?;
                    let color_r = curve("AeoFXRedCurve")?;
                    let color_g = curve("AeoFXGreenCurve")?;
                    let color_b = curve("AeoFXBlueCurve")?;
                    let float_field = |label: &str| -> Result<f32, String> {
                        let val = source.child_value(label).unwrap_or("0");
                        val.parse::<f32>()
                            .map_err(|_| format!("Invalid AeoFX {label}"))
                    };
                    let spawn_shape = match source.child_value("AeoFXSpawnShape").unwrap_or("Point")
                    {
                        "Point" => SpawnShape::Point,
                        "Circle" => SpawnShape::Circle,
                        "Sphere" => SpawnShape::Sphere,
                        "Line" => SpawnShape::Line,
                        _ => return Err("Unsupported AeoFX spawn shape".into()),
                    };
                    let spawn_radius = float_field("AeoFXSpawnRadius")?;
                    let velocity = [
                        float_field("AeoFXVelocityX")?,
                        float_field("AeoFXVelocityY")?,
                        float_field("AeoFXVelocityZ")?,
                    ];
                    let acceleration = [
                        float_field("AeoFXAccelerationX")?,
                        float_field("AeoFXAccelerationY")?,
                        float_field("AeoFXAccelerationZ")?,
                    ];
                    let drag = float_field("AeoFXDrag")?;
                    let children = source
                        .child("Children")
                        .map(|group| nodes(&group.children, id))
                        .transpose()?
                        .unwrap_or_default();
                    Ok(EffectNode {
                        id: assigned,
                        name: source.child_value("Name").unwrap_or("Emitter").into(),
                        kind: emitter_kind,
                        enabled: true,
                        emission_rate,
                        lifetime_seconds,
                        texture,
                        color_r,
                        color_g,
                        color_b,
                        spawn_shape,
                        spawn_radius,
                        velocity,
                        acceleration,
                        drag,
                        size,
                        opacity,
                        children,
                    })
                })
                .collect()
        }
        let mut next_id = 1;
        let mut document = Self::new(id, name);
        document.roots = nodes(&children.children, &mut next_id)?;
        let links: Vec<(u64, u64)> = children
            .children
            .iter()
            .filter(|entry| entry.name == "Node")
            .filter_map(|entry| {
                let id = entry.child_value("AeoFXStableId")?.parse::<u64>().ok()?;
                let parent = entry.child_value("AeoFXParentId")?.parse::<u64>().ok()?;
                Some((id, parent))
            })
            .collect();
        for (id, parent) in links {
            document.reparent_node(id, Some(parent))?;
        }
        document.validate()?;
        Ok(document)
    }

    pub fn compile_new_fire_sprite(&self) -> Result<Vec<u8>, String> {
        super::fx_standalone::compile_standalone_sprites(self)
    }

    pub fn compile_template_texture_variant(
        &self,
        template: &super::fx_export::EfkEfc,
    ) -> Result<Vec<u8>, String> {
        use std::collections::BTreeMap;
        self.validate()?;
        let existing = Self::from_effekseer_template(&self.id, &self.name, template)?;
        let prepared = if self.roots.len() > existing.roots.len() {
            let names: Vec<&str> = self.roots[existing.roots.len()..]
                .iter()
                .map(|node| node.name.as_str())
                .collect();
            template.append_builtin_fire_sprites(&names)?
        } else {
            template.clone()
        };
        let baseline = Self::from_effekseer_template(&self.id, &self.name, &prepared)?;
        let mut choices = BTreeMap::<String, String>::new();
        let mut patches = Vec::<super::fx_export::TemplateNodePatch>::new();
        fn compare(
            edited: &[EffectNode],
            original: &[EffectNode],
            choices: &mut BTreeMap<String, String>,
            patches: &mut Vec<super::fx_export::TemplateNodePatch>,
        ) -> Result<(), String> {
            if edited.len() != original.len() {
                return Err("Node structure is not yet exportable".into());
            }
            for (now, before) in edited.iter().zip(original) {
                if now.id != before.id
                    || now.name != before.name
                    || now.enabled != before.enabled
                    || now.size != before.size
                    || now.opacity != before.opacity
                {
                    return Err("Uncompiled effect parameter change".into());
                }
                let mut patch = super::fx_export::TemplateNodePatch::default();
                if now.lifetime_seconds != before.lifetime_seconds {
                    let seconds = now
                        .lifetime_seconds
                        .ok_or("Cannot remove a runtime lifetime")?;
                    let frames = (seconds * 60.0).round().max(1.0) as i32;
                    patch.lifetime_frames = Some(frames);
                }
                if now.emission_rate != before.emission_rate {
                    let rate = now
                        .emission_rate
                        .ok_or("Cannot remove a runtime emission rate")?;
                    if rate <= 0.0 {
                        return Err("Runtime emission rate must be positive".into());
                    }
                    patch.generation_interval_frames = Some(60.0 / rate);
                }
                patches.push(patch);
                if let (Some(old), Some(new)) = (&before.texture, &now.texture) {
                    if let Some(other) = choices.insert(old.clone(), new.clone()) {
                        if other != *new {
                            return Err("Shared texture requires consistent assignment".into());
                        }
                    }
                } else if now.texture != before.texture {
                    return Err("Cannot add or remove runtime texture slots".into());
                }
                compare(&now.children, &before.children, choices, patches)?;
            }
            Ok(())
        }
        compare(&self.roots, &baseline.roots, &mut choices, &mut patches)?;
        let overrides: Vec<(&str, &str)> = choices
            .iter()
            .filter(|(old, new)| old != new)
            .map(|(old, new)| (old.as_str(), new.as_str()))
            .collect();
        let with_textures = super::fx_export::rebind_textures(&prepared, &overrides)?;
        let compiled = with_textures.with_template_particle_settings(&patches)?;
        compiled.encode()
    }
}
