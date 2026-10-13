use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::ui::RuntimeUi;
use crate::project::engine_assets_dir;
use crate::scripting::value::Value;

pub const AUTHORED_UI_VERSION: u32 = 1;
pub const AUTHORED_UI_WORKSPACE_VERSION: u32 = 2;

fn apply_legacy_button_typography_defaults(value: &mut serde_json::Value) {
    fn normalize_nodes(nodes: Option<&mut Vec<serde_json::Value>>) {
        let Some(nodes) = nodes else {
            return;
        };
        for node in nodes {
            let Some(object) = node.as_object_mut() else {
                continue;
            };
            if object.get("element_type").and_then(|value| value.as_str()) != Some("Button") {
                continue;
            }
            // Text historically defaulted to top-left, while buttons have always
            // rendered as centered controls. Preserve an explicitly authored value,
            // but migrate legacy button nodes where the fields did not yet exist.
            object
                .entry("horizontal_align")
                .or_insert_with(|| serde_json::Value::String("Center".to_string()));
            object
                .entry("vertical_align")
                .or_insert_with(|| serde_json::Value::String("Center".to_string()));
        }
    }

    if let Some(documents) = value
        .get_mut("documents")
        .and_then(serde_json::Value::as_array_mut)
    {
        for document in documents {
            normalize_nodes(
                document
                    .get_mut("document")
                    .and_then(|document| document.get_mut("nodes"))
                    .and_then(serde_json::Value::as_array_mut),
            );
        }
    } else {
        normalize_nodes(
            value
                .get_mut("nodes")
                .and_then(serde_json::Value::as_array_mut),
        );
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthoredUiWorkspace {
    pub version: u32,
    pub next_document_id: u32,
    pub active_document_id: u32,
    pub documents: Vec<AuthoredUiWorkspaceDocument>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthoredUiWorkspaceDocument {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub key: String,
    pub document: AuthoredUiDocument,
}

impl Default for AuthoredUiWorkspace {
    fn default() -> Self {
        Self {
            version: AUTHORED_UI_WORKSPACE_VERSION,
            next_document_id: 2,
            active_document_id: 1,
            documents: vec![AuthoredUiWorkspaceDocument {
                id: 1,
                name: "Main UI".to_string(),
                key: "main_ui".to_string(),
                document: AuthoredUiDocument::default(),
            }],
        }
    }
}

impl AuthoredUiWorkspace {
    pub fn active_document(&self) -> Option<&AuthoredUiDocument> {
        self.document(self.active_document_id)
            .map(|entry| &entry.document)
    }

    pub fn document(&self, id: u32) -> Option<&AuthoredUiWorkspaceDocument> {
        self.documents.iter().find(|entry| entry.id == id)
    }

    pub fn document_mut(&mut self, id: u32) -> Option<&mut AuthoredUiWorkspaceDocument> {
        self.documents.iter_mut().find(|entry| entry.id == id)
    }

    pub fn active_name(&self) -> &str {
        self.document(self.active_document_id)
            .map(|entry| entry.name.as_str())
            .unwrap_or("UI")
    }

    pub fn create_document(&mut self, name: &str) -> Result<u32, String> {
        let name = self.validate_name(name, None)?;
        let id = self.next_document_id.max(1);
        self.next_document_id = id.saturating_add(1);
        self.documents.push(AuthoredUiWorkspaceDocument {
            id,
            key: self.unique_key_for_name(&name, None),
            name,
            document: AuthoredUiDocument::default(),
        });
        Ok(id)
    }

    pub fn rename_document(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = self.validate_name(name, Some(id))?;
        let document = self
            .document_mut(id)
            .ok_or_else(|| format!("unknown UI document {}", id))?;
        document.name = name;
        Ok(())
    }

    pub fn set_active_document(&mut self, id: u32) -> Result<(), String> {
        if self.document(id).is_none() {
            return Err(format!("unknown UI document {}", id));
        }
        self.active_document_id = id;
        Ok(())
    }

    fn validate_name(&self, name: &str, except_id: Option<u32>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("UI name cannot be empty.".to_string());
        }
        if name.len() > 64 {
            return Err("UI name must be 64 characters or fewer.".to_string());
        }
        if self
            .documents
            .iter()
            .any(|entry| Some(entry.id) != except_id && entry.name.eq_ignore_ascii_case(name))
        {
            return Err(format!("A UI named '{}' already exists.", name));
        }
        Ok(name.to_string())
    }

    fn unique_key_for_name(&self, name: &str, except_id: Option<u32>) -> String {
        let mut key = String::new();
        let mut previous_was_separator = false;
        for character in name.chars() {
            if character.is_ascii_alphanumeric() || character == '_' {
                key.push(character.to_ascii_lowercase());
                previous_was_separator = false;
            } else if !key.is_empty() && !previous_was_separator {
                key.push('_');
                previous_was_separator = true;
            }
        }
        let base = key.trim_matches('_').to_string();
        let base = if base.is_empty() {
            "ui".to_string()
        } else if base.as_bytes()[0].is_ascii_digit() {
            format!("ui_{}", base)
        } else {
            base
        };
        let mut candidate = base.clone();
        let mut suffix = 2;
        while self
            .documents
            .iter()
            .any(|entry| Some(entry.id) != except_id && entry.key.eq_ignore_ascii_case(&candidate))
        {
            candidate = format!("{}_{}", base, suffix);
            suffix += 1;
        }
        candidate
    }

    pub fn load_from_project(project_path: &Path) -> Result<Self, String> {
        let path = project_path.join("ui.json");
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read {}: {}", path.display(), error))?;
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;
        apply_legacy_button_typography_defaults(&mut value);

        if value.get("documents").is_some() {
            let mut workspace: Self = serde_json::from_value(value)
                .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;
            workspace.normalize();
            return Ok(workspace);
        }

        let document: AuthoredUiDocument = serde_json::from_value(value)
            .map_err(|error| format!("Failed to parse legacy {}: {}", path.display(), error))?;
        Ok(Self {
            version: AUTHORED_UI_WORKSPACE_VERSION,
            next_document_id: 2,
            active_document_id: 1,
            documents: vec![AuthoredUiWorkspaceDocument {
                id: 1,
                name: "Main UI".to_string(),
                key: "main_ui".to_string(),
                document,
            }],
        })
    }

    pub fn save_to_project(&self, project_path: &Path) -> Result<(), String> {
        if !project_path.is_dir() {
            return Err(format!(
                "Project directory does not exist: {}",
                project_path.display()
            ));
        }
        let path = project_path.join("ui.json");
        let content = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Failed to serialize UI workspace: {}", error))?;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
        crate::project::atomic_file::write_atomically(&path, content.as_bytes())
            .map_err(|error| format!("Failed to write {}: {}", path.display(), error))
    }

    fn normalize(&mut self) {
        self.version = AUTHORED_UI_WORKSPACE_VERSION;
        if self.documents.is_empty() {
            *self = Self::default();
            return;
        }
        self.documents.sort_by_key(|entry| entry.id);
        let ids_and_names: Vec<(u32, String)> = self
            .documents
            .iter()
            .map(|entry| (entry.id, entry.name.clone()))
            .collect();
        for (id, name) in ids_and_names {
            let needs_key = self
                .document(id)
                .is_some_and(|entry| entry.key.trim().is_empty());
            if needs_key {
                let key = self.unique_key_for_name(&name, Some(id));
                if let Some(entry) = self.document_mut(id) {
                    entry.key = key;
                }
            }
        }
        self.next_document_id = self
            .documents
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(self.next_document_id);
        if self.document(self.active_document_id).is_none() {
            self.active_document_id = self.documents[0].id;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthoredUiDocument {
    pub version: u32,
    pub next_id: u32,
    pub nodes: Vec<AuthoredUiNode>,
}

impl Default for AuthoredUiDocument {
    fn default() -> Self {
        Self {
            version: AUTHORED_UI_VERSION,
            next_id: 1,
            nodes: Vec::new(),
        }
    }
}

fn default_font_size() -> f32 {
    14.0
}

fn default_wrap() -> bool {
    true
}

fn default_element_type() -> String {
    "Panel".to_string()
}

fn default_true() -> bool {
    true
}

fn default_playback_rate() -> f32 {
    1.0
}

// Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:08 -05:00
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoredUiEventBinding {
    #[serde(default)]
    pub module_key: String,
    #[serde(default)]
    pub handler: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthoredUiNode {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub anchor: crate::engine::ui::UiAnchor,
    #[serde(default)]
    pub id: u32,
    #[serde(default = "default_element_type")]
    pub element_type: String,
    #[serde(default)]
    pub position: [f32; 2],
    #[serde(default)]
    pub size: [f32; 2],
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub color: [f32; 4],
    #[serde(default)]
    pub parent: Option<u32>,
    #[serde(default)]
    pub z_index: f32,
    #[serde(default)]
    pub corner_radius: f32,
    #[serde(default)]
    pub border_width: f32,
    #[serde(default)]
    pub border_color: [f32; 4],
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub horizontal_align: crate::engine::ui::UiHorizontalAlign,
    #[serde(default)]
    pub vertical_align: crate::engine::ui::UiVerticalAlign,
    #[serde(default = "default_wrap")]
    pub wrap: bool,
    #[serde(default)]
    pub padding: [f32; 4],
    #[serde(default)]
    pub line_spacing: f32,
    // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:19 -05:00
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub fit: crate::engine::ui::UiMediaFit,
    #[serde(default)]
    pub playing: bool,
    #[serde(default)]
    pub looped: bool,
    #[serde(default = "default_playback_rate")]
    pub playback_rate: f32,
    // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:08 -05:00
    #[serde(default)]
    pub events: std::collections::BTreeMap<String, AuthoredUiEventBinding>,
}

impl Default for AuthoredUiNode {
    fn default() -> Self {
        Self {
            name: String::new(),
            key: String::new(),
            anchor: Default::default(),
            id: 0,
            element_type: "Panel".to_string(),
            position: [32.0, 32.0],
            size: [240.0, 120.0],
            visible: true,
            enabled: true,
            color: [0.12, 0.12, 0.15, 0.85],
            parent: None,
            z_index: 0.0,
            corner_radius: 4.0,
            border_width: 1.0,
            border_color: [1.0, 1.0, 1.0, 0.10],
            text: String::new(),
            font_size: 14.0,
            horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
            vertical_align: crate::engine::ui::UiVerticalAlign::Top,
            wrap: true,
            padding: [0.0, 0.0, 0.0, 0.0],
            line_spacing: 0.0,
            source: String::new(),
            fit: Default::default(),
            playing: false,
            looped: false,
            playback_rate: 1.0,
            // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:35 -05:00
            events: std::collections::BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiAssetDefinition {
    pub element_type: String,

    #[serde(default)]
    pub position: Option<[f32; 2]>,

    #[serde(default)]
    pub size: Option<[f32; 2]>,

    #[serde(default)]
    pub visible: Option<bool>,

    #[serde(default)]
    pub enabled: Option<bool>,

    #[serde(default)]
    pub color: Option<[f32; 4]>,

    #[serde(default)]
    pub z_index: Option<f32>,

    #[serde(default)]
    pub corner_radius: Option<f32>,

    #[serde(default)]
    pub border_width: Option<f32>,

    #[serde(default)]
    pub border_color: Option<[f32; 4]>,

    #[serde(default)]
    pub text: Option<String>,

    #[serde(default)]
    pub font_size: Option<f32>,

    #[serde(default)]
    pub horizontal_align: Option<crate::engine::ui::UiHorizontalAlign>,

    #[serde(default)]
    pub vertical_align: Option<crate::engine::ui::UiVerticalAlign>,

    #[serde(default)]
    pub wrap: Option<bool>,

    #[serde(default)]
    pub padding: Option<[f32; 4]>,

    #[serde(default)]
    pub line_spacing: Option<f32>,
    // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:19 -05:00
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub fit: Option<crate::engine::ui::UiMediaFit>,
    #[serde(default)]
    pub playing: Option<bool>,
    #[serde(default)]
    pub looped: Option<bool>,
    #[serde(default)]
    pub playback_rate: Option<f32>,
}

impl AuthoredUiDocument {
    /// Returns authoring errors that would make the hierarchy ambiguous or the
    /// serialized UI unsafe to edit. The editor keeps these visible instead of
    /// silently omitting malformed nodes from the hierarchy or canvas.
    pub fn validation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let mut ids = HashSet::new();
        let mut keys = std::collections::HashMap::<&str, usize>::new();

        for node in &self.nodes {
            if !ids.insert(node.id) {
                errors.push(format!("UI {} has a duplicate numeric ID", node.id));
            }

            let key = node.key.trim();
            if key.is_empty() {
                errors.push(format!(
                    "UI {} ({}) has an empty script key",
                    node.id, node.name
                ));
            } else {
                *keys.entry(key).or_default() += 1;
                if !key
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
                {
                    errors.push(format!(
                        "UI {} has invalid script key '{}'; use letters, digits, and underscores",
                        node.id, node.key
                    ));
                }
            }

            let valid_dimensions = node
                .size
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0)
                && (node.anchor == crate::engine::ui::UiAnchor::Stretch
                    || node.size.iter().all(|value| *value > 0.0));
            if !valid_dimensions {
                errors.push(format!(
                    "UI {} ({}) has invalid dimensions [{}, {}]",
                    node.id, node.name, node.size[0], node.size[1]
                ));
            }

            // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:19 -05:00
            if matches!(node.element_type.as_str(), "Image" | "Video") {
                if let Err(message) =
                    crate::engine::ui_media::validate_ui_media_source(&node.source)
                {
                    errors.push(format!("UI {} ({}) source: {message}", node.id, node.key));
                }
                if node.element_type == "Video"
                    && !node.source.is_empty()
                    && !node.source.to_ascii_lowercase().ends_with(".mp4")
                {
                    errors.push(format!(
                        "UI {} ({}) Video source must be MP4",
                        node.id, node.key
                    ));
                }
                if node.element_type == "Image"
                    && !node.source.is_empty()
                    && node.source.to_ascii_lowercase().ends_with(".mp4")
                {
                    errors.push(format!(
                        "UI {} ({}) Image source must be PNG/JPEG",
                        node.id, node.key
                    ));
                }
                if node.element_type == "Image"
                    && (node.playing || node.looped || node.playback_rate != 1.0)
                {
                    errors.push(format!(
                        "UI {} ({}) Image has Video-only playback properties",
                        node.id, node.key
                    ));
                }
                if !node.playback_rate.is_finite()
                    || !(0.0..=4.0).contains(&node.playback_rate)
                    || node.playback_rate == 0.0
                {
                    errors.push(format!(
                        "UI {} ({}) invalid playback rate",
                        node.id, node.key
                    ));
                }
            }
            // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:08 -05:00
            for (event, binding) in &node.events {
                if node.element_type != "Button" || event != "on_click" {
                    errors.push(format!(
                        "UI {} ({}) has unsupported authored event '{}' on {}",
                        node.id, node.key, event, node.element_type
                    ));
                    continue;
                }
                if binding.module_key.trim().is_empty() {
                    errors.push(format!(
                        "UI {} ({}) Button on_click has an empty Module/script key",
                        node.id, node.key
                    ));
                }
                let handler = binding.handler.trim();
                let valid_handler = !handler.is_empty()
                    && handler.chars().enumerate().all(|(index, c)| {
                        c == '_' || c.is_ascii_alphabetic() || (index > 0 && c.is_ascii_digit())
                    });
                if !valid_handler {
                    errors.push(format!(
                        "UI {} ({}) Button on_click handler '{}' is not a valid AeoScript function name",
                        node.id, node.key, binding.handler
                    ));
                }
            }

            if let Some(parent) = node.parent {
                if !self.nodes.iter().any(|candidate| candidate.id == parent) {
                    errors.push(format!(
                        "UI {} ({}) references missing parent UI {}",
                        node.id, node.name, parent
                    ));
                } else if let Err(error) = self.ensure_no_cycle(node.id, parent) {
                    errors.push(format!("UI {} ({}): {}", node.id, node.name, error));
                }
            }
        }

        for (key, count) in keys {
            if count > 1 {
                errors.push(format!(
                    "Script key '{}' is used by {} UI elements",
                    key, count
                ));
            }
        }

        errors.sort();
        errors.dedup();
        errors
    }

    pub fn node(&self, id: u32) -> Option<&AuthoredUiNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut AuthoredUiNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    pub fn node_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self.nodes.iter().map(|node| node.id).collect();
        ids.sort_unstable();
        ids
    }

    pub fn roots(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .nodes
            .iter()
            .filter(|node| node.parent.is_none())
            .map(|node| node.id)
            .collect();

        ids.sort_unstable();
        ids
    }

    pub fn invalid_hierarchy_node_ids(&self) -> Vec<u32> {
        let mut invalid = Vec::new();
        for node in &self.nodes {
            let mut visited = HashSet::new();
            let mut current = node.id;
            loop {
                if !visited.insert(current) {
                    invalid.push(node.id);
                    break;
                }
                let Some(current_node) = self.node(current) else {
                    invalid.push(node.id);
                    break;
                };
                match current_node.parent {
                    Some(parent) => current = parent,
                    None => break,
                }
            }
        }
        invalid.sort_unstable();
        invalid.dedup();
        invalid
    }

    pub fn children_of(&self, parent_id: u32) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .nodes
            .iter()
            .filter(|node| node.parent == Some(parent_id))
            .map(|node| node.id)
            .collect();

        ids.sort_unstable();
        ids
    }

    pub fn resolve_layout_in(
        &self,
        id: u32,
        viewport: [f32; 2],
    ) -> Result<crate::engine::ui::UiResolvedLayout, String> {
        let mut runtime = RuntimeUi::new();
        for node in &self.nodes {
            let common = crate::engine::ui::UiCommon {
                anchor: node.anchor,
                position: node.position,
                size: node.size,
                parent: node.parent,
                visible: node.visible,
                enabled: node.enabled,
                z_index: node.z_index,
                ..Default::default()
            };
            runtime
                .elements
                .insert(node.id, crate::engine::ui::UiElement::Panel(common));
        }
        runtime.resolve_layout_in(id as u64, viewport)
    }

    pub fn absolute_position(&self, id: u32) -> Result<[f32; 2], String> {
        Ok(self.resolve_layout_in(id, [1920.0, 1080.0])?.position)
    }

    pub fn duplicate_node(&mut self, id: u32) -> Result<u32, String> {
        let Some(source) = self.node(id).cloned() else {
            return Err(format!("UI node {} not found", id));
        };

        let new_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let mut duplicate = source.clone();
        duplicate.id = new_id;
        duplicate.name = format!("{} (Copy)", source.name);
        duplicate.position = [source.position[0] + 16.0, source.position[1] + 16.0];

        let mut key = if source.key.is_empty() {
            format!("ui_{new_id}")
        } else {
            format!("{}_copy", source.key)
        };

        let mut count = 1;
        while self.nodes.iter().any(|n| n.key == key) {
            count += 1;
            key = format!("{}_copy{count}", source.key);
        }
        duplicate.key = key;

        self.nodes.push(duplicate);
        self.nodes.sort_by_key(|n| n.id);
        Ok(new_id)
    }

    pub fn bring_forward(&mut self, id: u32) {
        if let Some(node) = self.node_mut(id) {
            node.z_index += 1.0;
        }
    }

    pub fn send_backward(&mut self, id: u32) {
        if let Some(node) = self.node_mut(id) {
            node.z_index -= 1.0;
        }
    }

    pub fn instantiate_preset(&mut self, preset_name: &str) -> Vec<u32> {
        let presets = builtin_presets();
        let Some((_, template_nodes)) = presets.into_iter().find(|(name, _)| *name == preset_name)
        else {
            return Vec::new();
        };

        let mut id_map = std::collections::HashMap::<u32, u32>::new();
        let mut created_ids = Vec::new();

        for mut node in template_nodes {
            let old_id = node.id;
            let new_id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);

            id_map.insert(old_id, new_id);
            node.id = new_id;

            let mut key = node.key.clone();
            let mut count = 1;
            while self.nodes.iter().any(|existing| existing.key == key) {
                count += 1;
                key = format!("{}_{count}", node.key);
            }
            node.key = key;

            if let Some(old_parent) = node.parent {
                node.parent = id_map.get(&old_parent).copied();
            }

            created_ids.push(new_id);
            self.nodes.push(node);
        }

        self.nodes.sort_by_key(|node| node.id);
        created_ids
    }

    pub fn add_node_from_asset(&mut self, asset: &UiAssetDefinition, position: [f32; 2]) -> u32 {
        let mut node = AuthoredUiNode::default();

        node.id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        node.element_type = asset.element_type.clone();
        node.name = format!("{} {}", node.element_type, node.id);
        node.key = format!("ui_{}", node.id);
        node.position = asset.position.unwrap_or(position);
        node.size = asset.size.unwrap_or(node.size);
        node.visible = asset.visible.unwrap_or(node.visible);
        node.enabled = asset.enabled.unwrap_or(node.enabled);
        node.color = asset.color.unwrap_or(node.color);
        node.z_index = asset.z_index.unwrap_or(node.z_index);
        node.corner_radius = asset.corner_radius.unwrap_or(node.corner_radius);
        node.border_width = asset.border_width.unwrap_or(node.border_width);
        node.border_color = asset.border_color.unwrap_or(node.border_color);
        node.text = asset.text.clone().unwrap_or_default();
        node.font_size = asset.font_size.unwrap_or(node.font_size);
        node.horizontal_align =
            asset
                .horizontal_align
                .unwrap_or(if node.element_type == "Button" {
                    crate::engine::ui::UiHorizontalAlign::Center
                } else {
                    crate::engine::ui::UiHorizontalAlign::Left
                });
        node.vertical_align = asset
            .vertical_align
            .unwrap_or(if node.element_type == "Button" {
                crate::engine::ui::UiVerticalAlign::Center
            } else {
                crate::engine::ui::UiVerticalAlign::Top
            });
        node.wrap = asset.wrap.unwrap_or(node.wrap);
        node.padding = asset.padding.unwrap_or(node.padding);
        node.line_spacing = asset.line_spacing.unwrap_or(node.line_spacing);
        node.source = asset.source.clone().unwrap_or_default();
        node.fit = asset.fit.unwrap_or_default();
        node.playing = asset.playing.unwrap_or(false);
        node.looped = asset.looped.unwrap_or(false);
        node.playback_rate = asset.playback_rate.unwrap_or(1.0);

        if asset.position.is_none() {
            node.position = position;
        }

        self.nodes.push(node);

        self.nodes.sort_by_key(|entry| entry.id);

        self.next_id = self
            .nodes
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);

        self.nodes
            .last()
            .map(|entry| entry.id)
            .unwrap_or_else(|| self.next_id.saturating_sub(1))
    }

    pub fn delete_node(&mut self, id: u32) -> Result<(), String> {
        self.delete_node_in(id, [1920.0, 1080.0])
    }

    pub fn delete_node_in(&mut self, id: u32, viewport: [f32; 2]) -> Result<(), String> {
        self.node(id)
            .ok_or_else(|| format!("unknown UI node {}", id))?;
        // Resolve children before removing the parent so failure is atomic.
        let children: Vec<_> = self
            .children_of(id)
            .into_iter()
            .map(|child| {
                self.resolve_layout_in(child, viewport)
                    .map(|layout| (child, layout))
            })
            .collect::<Result<_, _>>()?;
        self.nodes.retain(|node| node.id != id);
        for (child, layout) in children {
            let node = self.node_mut(child).unwrap();
            node.parent = None;
            node.anchor = Default::default();
            node.position = layout.position;
            node.size = layout.size;
        }
        Ok(())
    }

    pub fn set_parent_preserve_absolute_position(
        &mut self,
        child_id: u32,
        parent_id: Option<u32>,
    ) -> Result<(), String> {
        self.set_parent_in(child_id, parent_id, [1920.0, 1080.0])
    }

    pub fn set_parent_in(
        &mut self,
        child_id: u32,
        parent_id: Option<u32>,
        viewport: [f32; 2],
    ) -> Result<(), String> {
        if parent_id == Some(child_id) {
            return Err("a UI node cannot parent itself".to_string());
        }

        if let Some(parent) = parent_id {
            if self.node(parent).is_none() {
                return Err(format!("unknown UI parent {}", parent));
            }

            self.ensure_no_cycle(child_id, parent)?;
        }

        let layout = self.resolve_layout_in(child_id, viewport)?;
        let absolute = layout.position;

        let parent_absolute = match parent_id {
            Some(parent) => self.resolve_layout_in(parent, viewport)?.position,
            None => [0.0, 0.0],
        };

        let node = self
            .node_mut(child_id)
            .ok_or_else(|| format!("unknown UI node {}", child_id))?;

        node.parent = parent_id;
        node.anchor = Default::default();
        node.size = layout.size;
        node.position = [
            absolute[0] - parent_absolute[0],
            absolute[1] - parent_absolute[1],
        ];

        Ok(())
    }

    fn ensure_no_cycle(&self, child_id: u32, parent_id: u32) -> Result<(), String> {
        let mut visited = HashSet::new();
        let mut current = parent_id;

        loop {
            if !visited.insert(current) {
                return Err(format!("cyclic UI hierarchy detected at node {}", current));
            }

            if current == child_id {
                return Err("setting this parent would create a cyclic UI hierarchy".to_string());
            }

            let node = self
                .node(current)
                .ok_or_else(|| format!("unknown UI node {}", current))?;

            match node.parent {
                Some(parent) => current = parent,
                None => break,
            }
        }

        Ok(())
    }

    pub fn set_position(&mut self, id: u32, position: [f32; 2]) -> Result<(), String> {
        let node = self
            .node_mut(id)
            .ok_or_else(|| format!("unknown UI node {}", id))?;

        node.position = position;
        Ok(())
    }

    pub fn load_from_project(project_path: &Path) -> Result<Self, String> {
        let path = project_path.join("ui.json");

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read {}: {}", path.display(), error))?;

        let mut value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;
        apply_legacy_button_typography_defaults(&mut value);
        let mut document: Self = serde_json::from_value(value)
            .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;

        if document.version == 0 {
            document.version = AUTHORED_UI_VERSION;
        }

        document.next_id = document
            .nodes
            .iter()
            .map(|node| node.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(document.next_id);

        Ok(document)
    }

    pub fn save_to_project(&self, project_path: &Path) -> Result<(), String> {
        if !project_path.is_dir() {
            return Err(format!(
                "Project directory does not exist: {}",
                project_path.display()
            ));
        }

        let path = project_path.join("ui.json");

        let content = serde_json::to_string_pretty(self)
            .map_err(|error| format!("Failed to serialize UI document: {}", error))?;

        fs::write(&path, content)
            .map_err(|error| format!("Failed to write {}: {}", path.display(), error))
    }
}

pub fn builtin_presets() -> Vec<(&'static str, Vec<AuthoredUiNode>)> {
    vec![
        (
            "Dialogue Panel",
            vec![
                AuthoredUiNode {
                    id: 1,
                    element_type: "Panel".to_string(),
                    name: "Dialogue Panel".to_string(),
                    key: "hearthvale_dialogue_panel".to_string(),
                    anchor: crate::engine::ui::UiAnchor::BottomCenter,
                    position: [0.0, -40.0],
                    size: [800.0, 180.0],
                    color: [0.08, 0.09, 0.12, 0.90],
                    corner_radius: 8.0,
                    border_width: 1.0,
                    border_color: [1.0, 1.0, 1.0, 0.15],
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 2,
                    element_type: "Text".to_string(),
                    name: "Speaker".to_string(),
                    key: "hearthvale_speaker".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [24.0, 16.0],
                    size: [300.0, 28.0],
                    text: "STEWARD EDRIN".to_string(),
                    font_size: 18.0,
                    color: [0.95, 0.65, 0.20, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Top,
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 3,
                    element_type: "Text".to_string(),
                    name: "Speech Body".to_string(),
                    key: "hearthvale_speech".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [24.0, 48.0],
                    size: [752.0, 100.0],
                    text: "The Broken Watch has gone dark.".to_string(),
                    font_size: 16.0,
                    wrap: true,
                    line_spacing: 2.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Top,
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 4,
                    element_type: "Text".to_string(),
                    name: "Prompt".to_string(),
                    key: "hearthvale_prompt".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::BottomRight,
                    position: [-24.0, -16.0],
                    size: [200.0, 24.0],
                    text: "[E] CONTINUE".to_string(),
                    font_size: 13.0,
                    color: [0.70, 0.70, 0.70, 0.80],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Right,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Bottom,
                    ..Default::default()
                },
            ],
        ),
        (
            "Top Bar",
            vec![
                AuthoredUiNode {
                    id: 1,
                    element_type: "Panel".to_string(),
                    name: "Top Bar".to_string(),
                    key: "top_bar".to_string(),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [0.0, 0.0],
                    size: [1920.0, 48.0],
                    color: [0.06, 0.07, 0.09, 0.95],
                    border_width: 0.0,
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 2,
                    element_type: "Text".to_string(),
                    name: "Title".to_string(),
                    key: "top_bar_title".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::CenterLeft,
                    position: [24.0, 0.0],
                    size: [300.0, 30.0],
                    text: "HEARTHVALE".to_string(),
                    font_size: 16.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Center,
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 3,
                    element_type: "Text".to_string(),
                    name: "Status".to_string(),
                    key: "top_bar_status".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::CenterRight,
                    position: [-24.0, 0.0],
                    size: [300.0, 30.0],
                    text: "LOCATION: THE WATCH".to_string(),
                    font_size: 14.0,
                    color: [0.80, 0.80, 0.80, 0.90],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Right,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Center,
                    ..Default::default()
                },
            ],
        ),
        (
            "Quest Panel",
            vec![
                AuthoredUiNode {
                    id: 1,
                    element_type: "Panel".to_string(),
                    name: "Quest Panel".to_string(),
                    key: "quest_panel".to_string(),
                    anchor: crate::engine::ui::UiAnchor::TopRight,
                    position: [-32.0, 64.0],
                    size: [280.0, 140.0],
                    color: [0.08, 0.09, 0.12, 0.80],
                    corner_radius: 6.0,
                    border_width: 1.0,
                    border_color: [1.0, 1.0, 1.0, 0.10],
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 2,
                    element_type: "Text".to_string(),
                    name: "Quest Title".to_string(),
                    key: "quest_title".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [16.0, 12.0],
                    size: [248.0, 24.0],
                    text: "QUEST OBJECTIVES".to_string(),
                    font_size: 15.0,
                    color: [0.95, 0.70, 0.25, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Top,
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 3,
                    element_type: "Text".to_string(),
                    name: "Objective List".to_string(),
                    key: "quest_objectives".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [16.0, 40.0],
                    size: [248.0, 80.0],
                    text: "- Speak with Steward Edrin\n- Retrieve the key\n- Open the gate"
                        .to_string(),
                    font_size: 13.0,
                    wrap: true,
                    line_spacing: 2.0,
                    color: [0.90, 0.90, 0.90, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Top,
                    ..Default::default()
                },
            ],
        ),
        (
            "Satchel Panel",
            vec![
                AuthoredUiNode {
                    id: 1,
                    element_type: "Panel".to_string(),
                    name: "Satchel Panel".to_string(),
                    key: "satchel_panel".to_string(),
                    anchor: crate::engine::ui::UiAnchor::BottomRight,
                    position: [-32.0, -40.0],
                    size: [320.0, 240.0],
                    color: [0.08, 0.09, 0.12, 0.90],
                    corner_radius: 8.0,
                    border_width: 1.0,
                    border_color: [1.0, 1.0, 1.0, 0.12],
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 2,
                    element_type: "Text".to_string(),
                    name: "Satchel Title".to_string(),
                    key: "satchel_title".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [16.0, 16.0],
                    size: [288.0, 24.0],
                    text: "SATCHEL".to_string(),
                    font_size: 16.0,
                    color: [0.90, 0.90, 0.90, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Top,
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 3,
                    element_type: "Text".to_string(),
                    name: "Satchel Content".to_string(),
                    key: "satchel_content".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::TopLeft,
                    position: [16.0, 48.0],
                    size: [288.0, 160.0],
                    text: "Empty".to_string(),
                    font_size: 14.0,
                    color: [0.60, 0.60, 0.60, 0.80],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Left,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Top,
                    ..Default::default()
                },
            ],
        ),
        (
            "Center Prompt",
            vec![
                AuthoredUiNode {
                    id: 1,
                    element_type: "Panel".to_string(),
                    name: "Center Prompt Panel".to_string(),
                    key: "center_prompt_panel".to_string(),
                    anchor: crate::engine::ui::UiAnchor::Center,
                    position: [0.0, 120.0],
                    size: [220.0, 44.0],
                    color: [0.06, 0.07, 0.10, 0.85],
                    corner_radius: 22.0,
                    border_width: 1.0,
                    border_color: [1.0, 1.0, 1.0, 0.20],
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 2,
                    element_type: "Text".to_string(),
                    name: "Prompt Text".to_string(),
                    key: "center_prompt_text".to_string(),
                    parent: Some(1),
                    anchor: crate::engine::ui::UiAnchor::Center,
                    position: [0.0, 0.0],
                    size: [220.0, 44.0],
                    text: "[E] TALK".to_string(),
                    font_size: 15.0,
                    color: [1.0, 1.0, 1.0, 1.0],
                    horizontal_align: crate::engine::ui::UiHorizontalAlign::Center,
                    vertical_align: crate::engine::ui::UiVerticalAlign::Center,
                    ..Default::default()
                },
            ],
        ),
    ]
}

pub fn discover_ui_assets(project_path: Option<&Path>) -> Vec<String> {
    let mut names = HashSet::new();

    for (preset_name, _) in builtin_presets() {
        names.insert(preset_name.to_string());
    }
    // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:19 -05:00
    names.insert("Image".to_string());
    names.insert("Video".to_string());

    let engine_root = engine_assets_dir().join("UI");
    collect_ui_asset_names(&engine_root, &mut names);

    if let Some(project_path) = project_path {
        let project_root = project_path.join(".assets").join("UI");
        collect_ui_asset_names(&project_root, &mut names);
    }

    let mut names: Vec<String> = names.into_iter().collect();
    names.sort();
    names
}

pub fn load_ui_asset(project_path: Option<&Path>, name: &str) -> Result<UiAssetDefinition, String> {
    if matches!(name, "Image" | "Video") {
        return serde_json::from_value(serde_json::json!({
            "element_type": name,
            "size": [480, 270],
            "source": "",
            "fit": "Contain",
            "playing": false,
            "looped": false,
            "playback_rate": 1.0
        }))
        .map_err(|error| format!("Cannot instantiate {name} UI asset: {error}"));
    }
    let mut candidates = Vec::<PathBuf>::new();

    if let Some(project_path) = project_path {
        candidates.push(
            project_path
                .join(".assets")
                .join("UI")
                .join(format!("{name}.json")),
        );
    }

    candidates.push(engine_assets_dir().join("UI").join(format!("{name}.json")));

    for path in candidates {
        if !path.is_file() {
            continue;
        }

        let content = fs::read_to_string(&path)
            .map_err(|error| format!("Failed to read {}: {}", path.display(), error))?;

        let asset: UiAssetDefinition = serde_json::from_str(&content)
            .map_err(|error| format!("Failed to parse {}: {}", path.display(), error))?;

        return Ok(asset);
    }

    Err(format!("UI asset '{}' was not found.", name))
}

fn collect_ui_asset_names(root: &Path, output: &mut HashSet<String>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
        {
            continue;
        }

        if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
            output.insert(stem.to_string());
        }
    }
}

pub fn populate_runtime_ui(
    runtime_ui: &mut RuntimeUi,
    document: &AuthoredUiDocument,
) -> Result<(), String> {
    runtime_ui.clear();

    let mut id_map = std::collections::HashMap::<u32, u64>::new();

    let mut nodes = document.nodes.clone();
    nodes.sort_by_key(|node| node.id);

    for node in &nodes {
        let runtime_id = runtime_ui.allocate(&node.element_type)?;
        runtime_ui.set_property(runtime_id, "key", Value::String(node.key.clone()))?;
        runtime_ui
            .elements
            .get_mut(&(runtime_id as u32))
            .unwrap()
            .common_mut()
            .anchor = node.anchor;

        runtime_ui.set_property(
            runtime_id,
            "position",
            Value::array(vec![
                Value::Number(node.position[0] as f64),
                Value::Number(node.position[1] as f64),
            ]),
        )?;

        runtime_ui.set_property(
            runtime_id,
            "size",
            Value::array(vec![
                Value::Number(node.size[0] as f64),
                Value::Number(node.size[1] as f64),
            ]),
        )?;

        runtime_ui.set_property(runtime_id, "visible", Value::Bool(node.visible))?;
        runtime_ui.set_property(runtime_id, "enabled", Value::Bool(node.enabled))?;

        runtime_ui.set_property(
            runtime_id,
            "color",
            Value::array(
                node.color
                    .iter()
                    .map(|value| Value::Number(*value as f64))
                    .collect(),
            ),
        )?;

        runtime_ui.set_property(runtime_id, "z_index", Value::Number(node.z_index as f64))?;

        runtime_ui.set_property(
            runtime_id,
            "corner_radius",
            Value::Number(node.corner_radius as f64),
        )?;

        runtime_ui.set_property(
            runtime_id,
            "border_width",
            Value::Number(node.border_width as f64),
        )?;

        runtime_ui.set_property(
            runtime_id,
            "border_color",
            Value::array(
                node.border_color
                    .iter()
                    .map(|value| Value::Number(*value as f64))
                    .collect(),
            ),
        )?;

        if matches!(node.element_type.as_str(), "Text" | "Button") {
            runtime_ui.set_property(runtime_id, "text", Value::String(node.text.clone()))?;
            runtime_ui.set_property(
                runtime_id,
                "font_size",
                Value::Number(node.font_size as f64),
            )?;
            runtime_ui.set_property(
                runtime_id,
                "horizontal_align",
                Value::String(format!("{:?}", node.horizontal_align)),
            )?;
            runtime_ui.set_property(
                runtime_id,
                "vertical_align",
                Value::String(format!("{:?}", node.vertical_align)),
            )?;
            runtime_ui.set_property(runtime_id, "wrap", Value::Bool(node.wrap))?;
            runtime_ui.set_property(
                runtime_id,
                "padding",
                Value::array(vec![
                    Value::Number(node.padding[0] as f64),
                    Value::Number(node.padding[1] as f64),
                    Value::Number(node.padding[2] as f64),
                    Value::Number(node.padding[3] as f64),
                ]),
            )?;
            runtime_ui.set_property(
                runtime_id,
                "line_spacing",
                Value::Number(node.line_spacing as f64),
            )?;
        }

        // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:19 -05:00
        if matches!(node.element_type.as_str(), "Image" | "Video") {
            runtime_ui.set_property(runtime_id, "source", Value::String(node.source.clone()))?;
            runtime_ui.set_property(runtime_id, "fit", Value::String(format!("{:?}", node.fit)))?;
            if node.element_type == "Video" {
                runtime_ui.set_property(runtime_id, "playing", Value::Bool(node.playing))?;
                runtime_ui.set_property(runtime_id, "looped", Value::Bool(node.looped))?;
                runtime_ui.set_property(
                    runtime_id,
                    "playback_rate",
                    Value::Number(node.playback_rate as f64),
                )?;
            }
        }
        // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:08 -05:00
        if node.element_type == "Button" {
            if let Some(binding) = node.events.get("on_click") {
                runtime_ui.set_symbolic_event_binding(
                    runtime_id,
                    "on_click",
                    Some(crate::engine::ui::UiSymbolicEventBinding {
                        module_key: binding.module_key.trim().to_string(),
                        handler: binding.handler.trim().to_string(),
                    }),
                )?;
            }
        }

        id_map.insert(node.id, runtime_id);
    }

    for node in &nodes {
        if let Some(parent_id) = node.parent {
            let runtime_id = *id_map
                .get(&node.id)
                .ok_or_else(|| format!("missing runtime UI node for {}", node.id))?;

            let runtime_parent_id = *id_map.get(&parent_id).ok_or_else(|| {
                format!(
                    "UI node {} references missing parent {}",
                    node.id, parent_id
                )
            })?;

            runtime_ui.set_property(
                runtime_id,
                "parent",
                Value::Number(runtime_parent_id as f64),
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn ui_document_parent_preserves_absolute_position() {
        let mut document = AuthoredUiDocument::default();

        let panel = AuthoredUiNode {
            id: 1,
            position: [100.0, 50.0],
            ..Default::default()
        };

        let text = AuthoredUiNode {
            id: 2,
            element_type: "Text".to_string(),
            position: [25.0, 10.0],
            ..Default::default()
        };

        document.nodes = vec![panel, text];
        document.next_id = 3;

        assert_eq!(document.absolute_position(2).unwrap(), [25.0, 10.0]);

        document
            .set_parent_preserve_absolute_position(2, Some(1))
            .unwrap();

        assert_eq!(document.absolute_position(2).unwrap(), [25.0, 10.0]);
        assert_eq!(document.node(2).unwrap().position, [-75.0, -40.0]);
    }

    #[test]
    fn ui_document_rejects_parent_cycles() {
        let mut document = AuthoredUiDocument::default();

        document.nodes = vec![
            AuthoredUiNode {
                id: 1,
                ..Default::default()
            },
            AuthoredUiNode {
                id: 2,
                ..Default::default()
            },
        ];
        document.next_id = 3;

        document
            .set_parent_preserve_absolute_position(2, Some(1))
            .unwrap();

        assert!(
            document
                .set_parent_preserve_absolute_position(1, Some(2))
                .is_err()
        );
    }

    #[test]
    fn ui_document_save_and_load_round_trip() {
        let root =
            std::env::temp_dir().join(format!("WorldkilnUiAuthoring_{}", std::process::id()));

        if root.exists() {
            let _ = fs::remove_dir_all(&root);
        }

        fs::create_dir_all(&root).unwrap();

        let mut document = AuthoredUiDocument::default();
        document.nodes.push(AuthoredUiNode {
            id: 1,
            element_type: "Text".to_string(),
            text: "HELLO".to_string(),
            ..Default::default()
        });
        document.next_id = 2;

        document.save_to_project(&root).unwrap();

        let loaded = AuthoredUiDocument::load_from_project(&root).unwrap();

        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].text, "HELLO");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn authored_ui_populates_runtime_ui() {
        let document = AuthoredUiDocument {
            version: AUTHORED_UI_VERSION,
            next_id: 3,
            nodes: vec![
                AuthoredUiNode {
                    id: 1,
                    element_type: "Panel".to_string(),
                    ..Default::default()
                },
                AuthoredUiNode {
                    id: 2,
                    element_type: "Text".to_string(),
                    text: "Hello".to_string(),
                    position: [10.0, 20.0],
                    parent: Some(1),
                    ..Default::default()
                },
            ],
        };

        let mut runtime_ui = RuntimeUi::new();

        populate_runtime_ui(&mut runtime_ui, &document).unwrap();

        assert_eq!(runtime_ui.elements.len(), 2);

        let layout = runtime_ui.resolve_layout(2).unwrap();

        assert_eq!(layout.position, [42.0, 52.0]);
    }

    #[test]
    fn ui_workspace_migrates_legacy_document() {
        let root = std::env::temp_dir().join(format!(
            "WorldkilnUiWorkspaceMigration_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut legacy = AuthoredUiDocument::default();
        legacy.nodes.push(AuthoredUiNode {
            id: 1,
            text: "LEGACY".to_string(),
            ..Default::default()
        });
        legacy.next_id = 2;
        legacy.save_to_project(&root).unwrap();

        let workspace = AuthoredUiWorkspace::load_from_project(&root).unwrap();
        assert_eq!(workspace.documents.len(), 1);
        assert_eq!(workspace.documents[0].name, "Main UI");
        assert_eq!(workspace.documents[0].key, "main_ui");
        assert_eq!(workspace.documents[0].document.nodes[0].text, "LEGACY");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ui_workspace_creates_unique_stable_script_keys() {
        let mut workspace = AuthoredUiWorkspace::default();
        let hud = workspace.create_document("Game HUD").unwrap();
        let second = workspace.create_document("Game-HUD").unwrap();

        assert_eq!(workspace.document(hud).unwrap().key, "game_hud");
        assert_eq!(workspace.document(second).unwrap().key, "game_hud_2");

        workspace.rename_document(hud, "Player HUD").unwrap();
        assert_eq!(workspace.document(hud).unwrap().name, "Player HUD");
        assert_eq!(workspace.document(hud).unwrap().key, "game_hud");
    }

    #[test]
    fn ui_workspace_round_trip_preserves_documents_and_active_id() {
        let root = std::env::temp_dir().join(format!(
            "WorldkilnUiWorkspaceRoundTrip_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut workspace = AuthoredUiWorkspace::default();
        let hud = workspace.create_document("Game HUD").unwrap();
        workspace.set_active_document(hud).unwrap();
        workspace
            .document_mut(hud)
            .unwrap()
            .document
            .nodes
            .push(AuthoredUiNode {
                id: 1,
                text: "HEALTH".to_string(),
                ..Default::default()
            });
        workspace.save_to_project(&root).unwrap();

        let loaded = AuthoredUiWorkspace::load_from_project(&root).unwrap();
        assert_eq!(loaded.active_document_id, hud);
        assert_eq!(loaded.documents.len(), 2);
        assert_eq!(loaded.document(hud).unwrap().key, "game_hud");
        assert_eq!(
            loaded.document(hud).unwrap().document.nodes[0].text,
            "HEALTH"
        );

        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod responsive_authoring_tests {
    use super::*;
    use crate::engine::ui::UiAnchor;
    #[test]
    fn deleting_parent_preserves_child_bounds() {
        let mut doc = AuthoredUiDocument::default();
        doc.nodes = vec![
            AuthoredUiNode {
                id: 1,
                anchor: UiAnchor::BottomRight,
                ..Default::default()
            },
            AuthoredUiNode {
                id: 2,
                parent: Some(1),
                anchor: UiAnchor::Stretch,
                position: [4.0, 4.0],
                size: [4.0, 4.0],
                ..Default::default()
            },
        ];
        let before = doc.resolve_layout_in(2, [800.0, 600.0]).unwrap();
        doc.delete_node_in(1, [800.0, 600.0]).unwrap();
        let after = doc.resolve_layout_in(2, [800.0, 600.0]).unwrap();
        assert_eq!(before.position, after.position);
        assert_eq!(before.size, after.size);
        assert_eq!(doc.node(2).unwrap().parent, None);
    }

    #[test]
    fn validation_reports_keys_dimensions_missing_parents_and_cycles() {
        let mut doc = AuthoredUiDocument::default();
        doc.nodes = vec![
            AuthoredUiNode {
                id: 1,
                key: "same".to_string(),
                parent: Some(2),
                ..Default::default()
            },
            AuthoredUiNode {
                id: 2,
                key: "same".to_string(),
                parent: Some(1),
                ..Default::default()
            },
            AuthoredUiNode {
                id: 3,
                key: String::new(),
                parent: Some(99),
                size: [0.0, f32::NAN],
                ..Default::default()
            },
        ];

        let errors = doc.validation_errors().join("\n");
        assert!(errors.contains("Script key 'same' is used by 2 UI elements"));
        assert!(errors.contains("empty script key"));
        assert!(errors.contains("invalid dimensions"));
        assert!(errors.contains("missing parent UI 99"));
        assert!(errors.contains("cyclic UI hierarchy"));
        assert_eq!(doc.invalid_hierarchy_node_ids(), vec![1, 2, 3]);
    }

    #[test]
    fn valid_stretch_margins_are_not_reported_as_invalid_dimensions() {
        let mut doc = AuthoredUiDocument::default();
        doc.nodes.push(AuthoredUiNode {
            id: 1,
            key: "full_screen".to_string(),
            anchor: UiAnchor::Stretch,
            size: [0.0, 0.0],
            ..Default::default()
        });

        assert!(doc.validation_errors().is_empty());
    }

    #[test]
    fn every_builtin_preset_instantiates_as_an_editable_valid_hierarchy() {
        let expected = [
            "Dialogue Panel",
            "Top Bar",
            "Quest Panel",
            "Satchel Panel",
            "Center Prompt",
        ];
        let available: Vec<_> = builtin_presets()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(available, expected);

        for preset in expected {
            let mut doc = AuthoredUiDocument::default();
            let created = doc.instantiate_preset(preset);
            assert!(!created.is_empty(), "{preset} created no elements");
            assert!(
                doc.validation_errors().is_empty(),
                "{preset} was invalid: {:?}",
                doc.validation_errors()
            );
            assert_eq!(doc.nodes.len(), created.len());
        }
    }

    #[test]
    fn duplicate_gets_a_unique_key_and_preserves_parenting() {
        let mut doc = AuthoredUiDocument::default();
        doc.nodes = vec![
            AuthoredUiNode {
                id: 1,
                key: "panel".to_string(),
                ..Default::default()
            },
            AuthoredUiNode {
                id: 2,
                key: "label".to_string(),
                parent: Some(1),
                ..Default::default()
            },
        ];
        doc.next_id = 3;

        let duplicate = doc.duplicate_node(2).unwrap();
        let duplicate = doc.node(duplicate).unwrap();
        assert_eq!(duplicate.key, "label_copy");
        assert_eq!(duplicate.parent, Some(1));
        assert!(doc.validation_errors().is_empty());
    }
    #[test]
    fn authored_and_runtime_layout_match_and_runtime_changes_are_isolated() {
        let mut doc = AuthoredUiDocument::default();
        doc.nodes.push(AuthoredUiNode {
            id: 1,
            anchor: UiAnchor::Center,
            ..Default::default()
        });
        let mut runtime = RuntimeUi::new();
        populate_runtime_ui(&mut runtime, &doc).unwrap();
        assert_eq!(
            doc.resolve_layout_in(1, [800.0, 600.0]).unwrap(),
            runtime.resolve_layout_in(1, [800.0, 600.0]).unwrap()
        );
        runtime.elements.get_mut(&1).unwrap().common_mut().visible = false;
        assert!(doc.node(1).unwrap().visible);
        let mut legacy = serde_json::to_value(&doc.nodes[0]).unwrap();
        legacy.as_object_mut().unwrap().remove("anchor");
        legacy.as_object_mut().unwrap().remove("name");
        assert_eq!(
            serde_json::from_value::<AuthoredUiNode>(legacy)
                .unwrap()
                .anchor,
            UiAnchor::TopLeft
        );
    }

    #[test]
    fn legacy_document_without_typography_deserializes_and_populates_runtime() {
        let json = r#"{
            "id": 10,
            "element_type": "Text",
            "position": [10.0, 20.0],
            "size": [200.0, 50.0],
            "visible": true,
            "enabled": true,
            "color": [1.0, 1.0, 1.0, 1.0],
            "text": "LEGACY TEXT"
        }"#;

        let node: AuthoredUiNode = serde_json::from_str(json).unwrap();
        assert_eq!(node.font_size, 14.0);
        assert_eq!(
            node.horizontal_align,
            crate::engine::ui::UiHorizontalAlign::Left
        );
        assert_eq!(node.vertical_align, crate::engine::ui::UiVerticalAlign::Top);
        assert!(node.wrap);
        assert_eq!(node.padding, [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(node.line_spacing, 0.0);

        let doc = AuthoredUiDocument {
            version: AUTHORED_UI_VERSION,
            next_id: 11,
            nodes: vec![node],
        };

        let mut runtime = RuntimeUi::new();
        populate_runtime_ui(&mut runtime, &doc).unwrap();

        assert_eq!(
            runtime.get_property(1, "font_size").unwrap(),
            Some(crate::scripting::value::Value::Number(14.0))
        );
        assert_eq!(
            runtime.get_property(1, "horizontal_align").unwrap(),
            Some(crate::scripting::value::Value::String("Left".into()))
        );
    }

    #[test]
    fn loading_legacy_button_preserves_centered_button_defaults() {
        let root = std::env::temp_dir().join(format!(
            "WorldkilnLegacyButtonTypography_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("ui.json"),
            r#"{
                "version": 1,
                "next_id": 2,
                "nodes": [{
                    "id": 1,
                    "element_type": "Button",
                    "size": [160.0, 44.0],
                    "visible": true,
                    "enabled": true
                }]
            }"#,
        )
        .unwrap();

        let workspace = AuthoredUiWorkspace::load_from_project(&root).unwrap();
        let button = &workspace.active_document().unwrap().nodes[0];
        assert_eq!(
            button.horizontal_align,
            crate::engine::ui::UiHorizontalAlign::Center
        );
        assert_eq!(
            button.vertical_align,
            crate::engine::ui::UiVerticalAlign::Center
        );

        let _ = fs::remove_dir_all(root);
    }
}

/// Instantiate a document with a stable namespace for script lookup.
pub fn populate_runtime_ui_named(
    runtime: &mut RuntimeUi,
    document: &AuthoredUiDocument,
    key: &str,
) -> Result<(), String> {
    // Build off to the side: invalid keys or hierarchy never leave half a screen.
    let mut replacement = RuntimeUi::new();
    replacement.document_key = key.to_string();
    populate_runtime_ui(&mut replacement, document)?;
    replacement.document_key = key.to_string();
    *runtime = replacement;
    Ok(())
}

// Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:08 -05:00
#[cfg(test)]
mod authored_ui_event_binding_tests {
    use super::*;

    #[test]
    fn button_event_binding_round_trips_and_populates_runtime_symbolic_dispatch() {
        let root =
            std::env::temp_dir().join(format!("WorldkilnUiEventBinding_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let mut document = AuthoredUiDocument::default();
        let mut events = std::collections::BTreeMap::new();
        events.insert(
            "on_click".to_string(),
            AuthoredUiEventBinding {
                module_key: "dialogue".to_string(),
                handler: "advance_dialogue".to_string(),
            },
        );
        document.nodes.push(AuthoredUiNode {
            id: 1,
            key: "next_button".to_string(),
            element_type: "Button".to_string(),
            events,
            ..Default::default()
        });
        document.next_id = 2;
        document.save_to_project(&root).unwrap();

        let loaded = AuthoredUiDocument::load_from_project(&root).unwrap();
        let binding = loaded.nodes[0].events.get("on_click").unwrap();
        assert_eq!(binding.module_key, "dialogue");
        assert_eq!(binding.handler, "advance_dialogue");

        let mut runtime = RuntimeUi::new();
        populate_runtime_ui_named(&mut runtime, &loaded, "dialogue_ui").unwrap();
        let dispatch = runtime.symbolic_event_dispatch(1, "on_click").unwrap();
        assert_eq!(dispatch.document_key, "dialogue_ui");
        assert_eq!(dispatch.element_key, "next_button");
        assert_eq!(dispatch.module_key, "dialogue");
        assert_eq!(dispatch.handler, "advance_dialogue");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn authored_event_validation_rejects_invalid_or_unsupported_targets() {
        let mut document = AuthoredUiDocument::default();
        let mut events = std::collections::BTreeMap::new();
        events.insert(
            "on_click".to_string(),
            AuthoredUiEventBinding {
                module_key: String::new(),
                handler: "123 bad".to_string(),
            },
        );
        document.nodes.push(AuthoredUiNode {
            id: 1,
            key: "bad_button".to_string(),
            element_type: "Button".to_string(),
            events,
            ..Default::default()
        });

        let errors = document.validation_errors().join("\n");
        assert!(errors.contains("empty Module/script key"));
        assert!(errors.contains("not a valid AeoScript function name"));
    }
}

#[cfg(test)]
mod wo52_authoring_media_tests {
    use super::*;
    #[test]
    fn image_video_authored_document_roundtrips_into_play_mode() {
        let image = AuthoredUiNode {
            id: 1,
            key: "title".into(),
            element_type: "Image".into(),
            source: "textures/title.png".into(),
            fit: crate::engine::ui::UiMediaFit::Cover,
            ..Default::default()
        };
        let video = AuthoredUiNode {
            id: 2,
            key: "cinematic".into(),
            element_type: "Video".into(),
            source: "Media/intro.mp4".into(),
            playing: true,
            looped: false,
            ..Default::default()
        };
        let doc = AuthoredUiDocument {
            version: AUTHORED_UI_VERSION,
            next_id: 3,
            nodes: vec![image, video],
        };
        let json = serde_json::to_string(&doc).unwrap();
        let restored: AuthoredUiDocument = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.nodes[0].source, "textures/title.png");
        assert_eq!(restored.nodes[1].source, "Media/intro.mp4");
        let mut rt = RuntimeUi::new();
        populate_runtime_ui(&mut rt, &restored).unwrap();
        assert_eq!(
            rt.get_property(1, "source").unwrap(),
            Some(crate::scripting::value::Value::String(
                "textures/title.png".into()
            ))
        );
        assert_eq!(
            rt.get_property(2, "playing").unwrap(),
            Some(crate::scripting::value::Value::Bool(true))
        );
    }
}
