// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{
    Editor, HistorySnapshot,
    ai_project::{AgentSettings, ProjectAccess},
    ai_script,
};
use crate::{
    editor::script_editor::ScriptDocument,
    project::{ProjectHierarchy, ProjectNodeKind, ScriptRoot},
    world::{CellType, World, WorldCoord},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct FileState {
    disk: Option<String>,
    source: Option<String>,
    dirty: bool,
}
impl FileState {
    fn capture(editor: &Editor, path: &Path) -> Result<Self, String> {
        let disk = if path.exists() {
            Some(fs::read_to_string(path).map_err(|e| e.to_string())?)
        } else {
            None
        };
        let doc = editor.script_editor.open_documents.get(path);
        Ok(Self {
            disk,
            source: doc.map(|d| d.source.clone()),
            dirty: doc.is_some_and(|d| d.dirty),
        })
    }
    fn matches(&self, editor: &Editor, path: &Path) -> bool {
        let Ok(current) = Self::capture(editor, path) else {
            return false;
        };
        current.disk == self.disk && current.source == self.source && current.dirty == self.dirty
    }
}
pub(super) struct AgentCheckpoint {
    root: PathBuf,
    before: HistorySnapshot,
    after: HistorySnapshot,
    file: Option<(PathBuf, FileState, FileState)>,
    history_len: usize,
}
pub(super) struct AgentFileChange {
    id: u64,
    root: PathBuf,
    path: PathBuf,
    before: FileState,
    after: FileState,
    before_scene: HistorySnapshot,
    after_scene: HistorySnapshot,
}
fn line_delta(before: &str, after: &str) -> (usize, usize) {
    let a: Vec<_> = before.lines().collect();
    let b: Vec<_> = after.lines().collect();
    if a.len().saturating_mul(b.len()) <= 1_000_000 {
        let mut row = vec![0; b.len() + 1];
        for old in &a {
            let mut diagonal = 0;
            for (i, new) in b.iter().enumerate() {
                let previous = row[i + 1];
                row[i + 1] = if old == new {
                    diagonal + 1
                } else {
                    row[i].max(row[i + 1])
                };
                diagonal = previous;
            }
        }
        (b.len() - row[b.len()], a.len() - row[b.len()])
    } else {
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        (b.len() - prefix - suffix, a.len() - prefix - suffix)
    }
}
fn script_nodes(hierarchy: &ProjectHierarchy, relative: &str) -> Vec<crate::project::ProjectNode> {
    hierarchy
        .nodes
        .values()
        .filter(
            |n| matches!(&n.kind,ProjectNodeKind::Script { script_path } if script_path==relative),
        )
        .cloned()
        .collect()
}
fn retarget_file_history(snapshot: &mut HistorySnapshot, relative: &str, before: &HistorySnapshot) {
    let ids = snapshot.5.find_script_nodes(relative);
    for id in ids {
        snapshot.5.nodes.remove(&id);
    }
    for node in script_nodes(&before.5, relative) {
        if node
            .parent_id
            .is_none_or(|parent| snapshot.5.contains(parent))
        {
            snapshot.5.nodes.insert(node.id, node);
        }
    }
    snapshot
        .4
        .retain(|binding| binding.normalized_script_path() != relative);
    snapshot.4.extend(
        before
            .4
            .iter()
            .filter(|b| b.normalized_script_path() == relative)
            .cloned(),
    );
}
fn scene_matches(snapshot: &HistorySnapshot, editor: &Editor, world: &World) -> bool {
    snapshot.0 == world.cells
        && snapshot.1 == world.sources
        && snapshot.2 == world.script_bindings
        && snapshot.3 == world.actors
        && snapshot.4 == world.actor_script_bindings
        && snapshot.5 == editor.project_hierarchy
}
fn restore_scene(snapshot: &HistorySnapshot, editor: &mut Editor, world: &mut World) {
    world.cells = snapshot.0.clone();
    world.sources = snapshot.1.clone();
    world.script_bindings = snapshot.2.clone();
    world.actors = snapshot.3.clone();
    world.actor_script_bindings = snapshot.4.clone();
    editor.project_hierarchy = snapshot.5.clone();
    world.rebuild_id_mapping();
    world.rebuild_actor_module_index();
    editor.clear_selection();
    editor.clear_hover();
    editor.sanitize_project_explorer_selection();
    editor.needs_save = true;
}
fn string<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args[name]
        .as_str()
        .ok_or_else(|| format!("{name} must be a string."))
}
fn id(args: &Value, name: &str) -> Result<u64, String> {
    args[name]
        .as_u64()
        .ok_or_else(|| format!("{name} must be a nonnegative integer."))
}
fn vec3(value: &Value) -> Result<glam::Vec3, String> {
    let values = value
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or("Expected exactly three numbers.")?;
    let mut xyz = [0.0; 3];
    for (i, v) in values.iter().enumerate() {
        let n = v.as_f64().ok_or("Expected a finite number.")?;
        if !n.is_finite() || n.abs() > 1_000_000.0 {
            return Err("Value outside supported world range.".into());
        }
        xyz[i] = n as f32;
    }
    Ok(glam::Vec3::from_array(xyz))
}

fn cell_build_plan(world: &World, args: &Value) -> Result<Vec<(WorldCoord, CellType)>, String> {
    let entries: Vec<Value> = if let Some(cells) = args.get("cells") {
        if args.get("position").is_some()
            || args.get("positions").is_some()
            || args.get("cell_type").is_some()
        {
            return Err("Supply cells or cell_type with position/positions, exclusively.".into());
        }
        cells.as_array().ok_or("cells must be an array.")?.clone()
    } else {
        let positions = match (args.get("position"), args.get("positions")) {
            (Some(position), None) => vec![position.clone()],
            (None, Some(positions)) => positions
                .as_array()
                .ok_or("positions must be an array.")?
                .clone(),
            _ => return Err("Supply either position or positions, exclusively.".into()),
        };
        positions
            .into_iter()
            .map(|position| json!({"cell_type":args["cell_type"],"position":position}))
            .collect()
    };
    if entries.is_empty() || entries.len() > 256 {
        return Err("A Cell build must contain 1–256 Cells.".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut plan = Vec::with_capacity(entries.len());
    for entry in entries {
        let kind = match string(&entry, "cell_type")? {
            "Block" => CellType::Block,
            "FxBlock" => CellType::FxBlock,
            "Light" => CellType::Light,
            "SpawnPoint" => CellType::SpawnPoint,
            "AudioEmitter" => CellType::AudioEmitter,
            "ParticleEmitter" => CellType::ParticleEmitter,
            _ => return Err("Unsupported authored Cell type.".into()),
        };
        let values = entry["position"]
            .as_array()
            .filter(|v| v.len() == 3)
            .ok_or("Cells require exactly three integer grid coordinates.")?;
        let mut xyz = [0i32; 3];
        for (index, value) in values.iter().enumerate() {
            xyz[index] = value
                .as_i64()
                .filter(|n| (-1_000_000..=1_000_000).contains(n))
                .ok_or("Cells require integer grid coordinates within the supported world range.")?
                as i32;
        }
        let coord = WorldCoord::new(xyz[0], xyz[1], xyz[2]);
        if !seen.insert(coord) {
            return Err("Duplicate Cell coordinate in batch.".into());
        }
        if world.get(coord).is_some() {
            return Err("Cell coordinate already occupied; no Cells created.".into());
        }
        plan.push((coord, kind));
    }
    Ok(plan)
}
fn project_script_validation(
    access: &ProjectAccess,
    source: &str,
    kind: ai_script::ScriptKind,
    path: &str,
) -> Value {
    let isolated = super::ai_agent::validation_report(source, kind, path);
    let needs_module_index = isolated["diagnostics"].as_array().is_some_and(|items| {
        items.iter().any(|item| {
            item["message"].as_str().is_some_and(|message| {
                message.contains("does not resolve to an indexed project Module")
            })
        })
    });
    if !needs_module_index {
        return isolated;
    }
    let modules = (|| -> Result<std::collections::BTreeSet<String>, String> {
        let mut modules = std::collections::BTreeSet::new();
        for root in ["scripts", "playerscripts"] {
            let listing =
                access.execute("list_project_files", &json!({"path":root,"recursive":true}));
            if listing == "ERROR: File or directory not found." {
                continue;
            }
            if listing.starts_with("ERROR:") || listing.contains("results truncated") {
                return Err(format!(
                    "Project Module index unavailable for {root}: {listing}"
                ));
            }
            for entry in listing.lines() {
                let logical = entry.replace('\\', "/").to_ascii_lowercase();
                if logical.ends_with(".aeo") && logical.starts_with(&format!("{root}/")) {
                    modules.insert(logical);
                }
            }
        }
        Ok(modules)
    })();
    match modules {
        Ok(modules) => {
            super::ai_agent::validation_report_with_modules(source, kind, path, Some(&modules))
        }
        Err(message) => json!({
            "ok":false, "path":path, "static_validation":"failed",
            "phases":{"project_imports":"failed"},
            "diagnostic":message,
            "diagnostics":[{"path":path,"phase":"project_imports",
                "category":"project_index","severity":"error","message":message}],
            "saved":false, "runtime_tested":false
        }),
    }
}

fn script_read_analysis(
    editor: &Editor,
    world: &World,
    access: &ProjectAccess,
    relative: &str,
    source: &str,
) -> Value {
    let attached_actor = editor
        .project_hierarchy
        .find_script_nodes(relative)
        .iter()
        .find_map(|id| editor.project_hierarchy.attached_actor_id(*id))
        .or_else(|| {
            world.actor_script_bindings.iter().find_map(|binding| {
                (binding.normalized_script_path() == relative).then_some(binding.actor_id)
            })
        });
    let cell_bound = world
        .script_bindings
        .iter()
        .any(|binding| binding.normalized_script_path() == relative);
    let kind = if attached_actor.is_some() {
        ai_script::ScriptKind::Attached
    } else if cell_bound {
        ai_script::ScriptKind::Cell
    } else {
        ai_script::ScriptKind::Global
    };
    let mut report = project_script_validation(access, source, kind, relative);
    report["checker"] = json!("Worldkiln lexer, parser, semantic and built-in API validator");
    report["attached_actor_id"] = json!(attached_actor);
    report["reference_tools"] = json!(["browse_aeoscript_knowledge", "read_current_aeoscript_api"]);
    report
}
fn atomic_replace(path: &Path, source: &str, expected_disk: Option<&str>) -> Result<(), String> {
    use std::io::Write;
    let temp = path.with_extension(format!("aeo.agent-{}.tmp", std::process::id()));
    let mut owned = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        owned = true;
        file.write_all(source.as_bytes())
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        let disk = fs::read_to_string(path).ok();
        if disk.as_deref() != expected_disk {
            return Err("File changed on disk; no changes applied.".into());
        }
        fs::rename(&temp, path).map_err(|e| format!("Atomic script save failed: {e}"))
    })();
    if owned && temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    result
}

impl Editor {
    fn reveal_agent_script(&mut self, node_id: u64, world: &World) {
        if let Some(node) = self.project_hierarchy.node(node_id).cloned() {
            self.select_project_explorer_node(node_id, &node, world);
            let mut parent = node.parent_id;
            let mut seen = std::collections::HashSet::new();
            while let Some(id) = parent.filter(|id| seen.insert(*id)) {
                self.expanded_project_nodes.insert(id);
                parent = self.project_hierarchy.node(id).and_then(|n| n.parent_id);
            }
        }
        self.navigate_to(super::feature_nav::Feature::ScriptEditor);
    }

    pub(crate) fn reset_agent_project(&mut self, root: &Option<PathBuf>) {
        if self
            .agent_undo
            .last()
            .is_some_and(|c| root.as_ref() != Some(&c.root))
            || self
                .agent_redo
                .last()
                .is_some_and(|c| root.as_ref() != Some(&c.root))
        {
            self.agent_undo.clear();
            self.agent_redo.clear();
        }
        if self
            .agent_file_changes
            .last()
            .is_some_and(|c| root.as_ref() != Some(&c.root))
        {
            self.agent_file_changes.clear();
        }
    }
    pub(crate) fn revert_agent_file(
        &mut self,
        world: &mut World,
        root: &Path,
        change_id: u64,
    ) -> Result<String, String> {
        if self.mode != super::EditorMode::Editor {
            return Err("Stop Play before reverting an agent file.".into());
        }
        let index = self
            .agent_file_changes
            .iter()
            .position(|c| c.id == change_id)
            .ok_or("File change is no longer available.")?;
        let change = &self.agent_file_changes[index];
        if change.root != root || !change.after.matches(self, &change.path) {
            return Err("File changed after agent edit; no revert applied.".into());
        }
        let relative = change
            .path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        ProjectAccess::open(root)?.script_path(&relative)?;
        if script_nodes(&self.project_hierarchy, &relative)
            != script_nodes(&change.after_scene.5, &relative)
        {
            return Err("Script hierarchy changed after agent edit; no revert applied.".into());
        }
        let mut candidate = self.create_snapshot(world);
        retarget_file_history(&mut candidate, &relative, &change.before_scene);
        candidate
            .5
            .validate()
            .map_err(|e| format!("Cannot restore script hierarchy: {e}"))?;
        match &change.before.disk {
            Some(disk) => atomic_replace(&change.path, disk, change.after.disk.as_deref())?,
            None => fs::remove_file(&change.path).map_err(|e| e.to_string())?,
        }
        let change = self.agent_file_changes.remove(index);
        self.restore_agent_document(&change.path, &change.before);
        self.project_hierarchy = candidate.5;
        self.synchronize_project_hierarchy_bindings(world);
        for snapshot in self
            .history
            .undo_stack
            .iter_mut()
            .chain(self.history.redo_stack.iter_mut())
        {
            retarget_file_history(snapshot, &relative, &change.before_scene);
        }
        self.agent_undo.retain(|c| {
            c.file
                .as_ref()
                .is_none_or(|(path, _, _)| path != &change.path)
        });
        self.agent_redo.retain(|c| {
            c.file
                .as_ref()
                .is_none_or(|(path, _, _)| path != &change.path)
        });
        for checkpoint in self.agent_undo.iter_mut().chain(self.agent_redo.iter_mut()) {
            retarget_file_history(&mut checkpoint.before, &relative, &change.before_scene);
            retarget_file_history(&mut checkpoint.after, &relative, &change.before_scene);
        }
        for remaining in &mut self.agent_file_changes {
            retarget_file_history(&mut remaining.before_scene, &relative, &change.before_scene);
            retarget_file_history(&mut remaining.after_scene, &relative, &change.before_scene);
        }
        if let Some(view) = self
            .ai_chat
            .file_changes
            .iter_mut()
            .find(|v| v.id == change_id)
        {
            view.reverted = true;
        }
        self.script_editor
            .refresh_scripts(&Some(root.to_path_buf()));
        self.needs_save = true;
        Ok(format!("Reverted {relative}; other file changes retained."))
    }
    pub(crate) fn agent_change_is_latest(&self, world: &World) -> bool {
        self.agent_undo.last().is_some_and(|c| {
            c.history_len == self.history.undo_stack.len() && scene_matches(&c.after, self, world)
        })
    }
    pub(crate) fn agent_redo_is_latest(&self, world: &World) -> bool {
        self.agent_redo.last().is_some_and(|c| {
            c.history_len == self.history.undo_stack.len() + 1
                && scene_matches(&c.before, self, world)
        })
    }
    pub(crate) fn undo_agent_change(
        &mut self,
        world: &mut World,
        root: &Path,
    ) -> Result<String, String> {
        if self.mode != super::EditorMode::Editor {
            return Err("Stop Play before undoing agent changes.".into());
        }
        let checkpoint = self.agent_undo.last().ok_or("No agent change to undo.")?;
        if checkpoint.root != root || !self.agent_change_is_latest(world) {
            return Err("Later scene changes exist; undo those first.".into());
        }
        if let Some((path, before, after)) = &checkpoint.file {
            if !after.matches(self, path) {
                return Err(
                    "Script changed since agent edit; undo would overwrite user changes.".into(),
                );
            }
            ProjectAccess::open(root)?.script_path(
                &path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy(),
            )?;
            match &before.disk {
                Some(disk) => atomic_replace(path, disk, after.disk.as_deref())?,
                None => fs::remove_file(path).map_err(|e| e.to_string())?,
            }
        }
        let checkpoint = self.agent_undo.pop().unwrap();
        if let Some((path, before, _)) = &checkpoint.file {
            self.restore_agent_document(path, before);
            self.sync_agent_file_history(path, before, &checkpoint.before);
        }
        restore_scene(&checkpoint.before, self, world);
        if let Some(snapshot) = self.history.undo_stack.pop() {
            self.history.redo_stack.push(checkpoint.after.clone());
            drop(snapshot);
        }
        self.agent_redo.push(checkpoint);
        self.script_editor
            .refresh_scripts(&Some(root.to_path_buf()));
        Ok("Undid latest agent change and restored its previous source/dirty state.".into())
    }
    pub(crate) fn redo_agent_change(
        &mut self,
        world: &mut World,
        root: &Path,
    ) -> Result<String, String> {
        if self.mode != super::EditorMode::Editor {
            return Err("Stop Play before redoing agent changes.".into());
        }
        let checkpoint = self.agent_redo.last().ok_or("No agent change to redo.")?;
        if checkpoint.root != root || !self.agent_redo_is_latest(world) {
            return Err("Later edits prevent agent redo.".into());
        }
        if let Some((path, before, after)) = &checkpoint.file {
            if !before.matches(self, path) {
                return Err("Script changed after undo; no redo applied.".into());
            }
            ProjectAccess::open(root)?.script_path(
                &path
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy(),
            )?;
            if let Some(disk) = &after.disk {
                atomic_replace(path, disk, before.disk.as_deref())?;
            }
        }
        let checkpoint = self.agent_redo.pop().unwrap();
        if let Some((path, _, after)) = &checkpoint.file {
            self.restore_agent_document(path, after);
            self.sync_agent_file_history(path, after, &checkpoint.after);
        }
        restore_scene(&checkpoint.after, self, world);
        self.history.redo_stack.pop();
        self.history.push_undo_entry(checkpoint.before.clone());
        self.agent_undo.push(checkpoint);
        self.script_editor
            .refresh_scripts(&Some(root.to_path_buf()));
        Ok("Redid latest agent change.".into())
    }
    fn restore_agent_document(&mut self, path: &Path, state: &FileState) {
        self.script_editor.open_documents.remove(path);
        if let Some(source) = &state.source {
            let mut doc = ScriptDocument::new(path.to_path_buf(), source.clone());
            doc.original_source = state.disk.clone().unwrap_or_default();
            doc.dirty = state.dirty;
            self.script_editor
                .open_documents
                .insert(path.to_path_buf(), doc);
            self.script_editor.active_document = Some(path.to_path_buf());
        } else if self.script_editor.active_document.as_deref() == Some(path) {
            self.script_editor.active_document = None;
        }
    }
    fn sync_agent_file_history(&mut self, path: &Path, state: &FileState, scene: &HistorySnapshot) {
        if let Some(change) = self
            .agent_file_changes
            .iter_mut()
            .rev()
            .find(|c| c.path == path)
        {
            change.after = state.clone();
            change.after_scene = scene.clone();
            if let Some(view) = self
                .ai_chat
                .file_changes
                .iter_mut()
                .find(|v| v.id == change.id)
            {
                view.after = state
                    .source
                    .clone()
                    .or_else(|| state.disk.clone())
                    .unwrap_or_default();
                (view.added, view.removed) = line_delta(&view.before, &view.after);
                view.kept = false;
                view.reverted = state.disk == change.before.disk
                    && state.source == change.before.source
                    && state.dirty == change.before.dirty;
            }
        }
    }
    pub(crate) fn execute_agent_tool(
        &mut self,
        world: &mut World,
        root: &Path,
        settings: &AgentSettings,
        approved: bool,
        name: &str,
        args: &Value,
    ) -> Result<(String, bool), String> {
        if !super::ai_project::permitted(settings.access, name, approved) {
            return Err("Tool not authorized by current Agent Access.".into());
        }
        if super::ai_project::capability(name)
            .is_some_and(|c| c != super::ai_project::Capability::Read)
            && self.mode != super::EditorMode::Editor
            && name != "set_play_mode"
        {
            return Err("Agent editing is unavailable during Play.".into());
        }
        let access = ProjectAccess::open(root)?;
        if let Some(result) = self.native_ui_operation(name, args) {
            return result;
        }
        if let Some(result) = self.native_authoring_operation(world, root, name, args) {
            return result;
        }
        match name {
            "set_play_mode" => {
                let playing = args["playing"].as_bool().ok_or("playing must be boolean")?;
                self.commit_pending_inspector_history();
                self.mode = if playing {
                    super::EditorMode::Play
                } else {
                    super::EditorMode::Editor
                };
                return Ok((json!({"requested_mode":if playing {"Play"}else{"Editor"},"runtime_verified":false,
                    "note":"Native mode transition requested; inspect runtime state and diagnostics after the app processes it."}).to_string(),false));
            }
            "navigate_editor" => {
                let feature = string(args, "destination")?;
                let descriptor = super::feature_nav::DIRECTORY
                    .iter()
                    .find(|d| format!("{:?}", d.id) == feature)
                    .ok_or("Unknown editor destination; see editor_capabilities")?;
                if matches!(descriptor.owner, super::feature_nav::FeatureOwner::Action) {
                    return Err("Use dedicated action tools; navigation does not execute File/Play/build actions".into());
                }
                self.navigate_to(descriptor.id);
                return Ok((json!({"opened":feature}).to_string(), false));
            }
            "read_editor_diagnostics" => {
                let clip = |text: &str| {
                    text.chars()
                        .rev()
                        .take(12_000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                };
                let paused = self.script_editor.debug_snapshot.as_ref();
                let frames: Vec<_> = paused.into_iter().flat_map(|snapshot| snapshot.frames.iter()).take(16).map(|frame| json!({
                    "script_path":frame.script_path,"function":frame.function_name,"line":frame.line,"column":frame.column,
                    "instance_id":frame.instance_id,"parent_actor_id":frame.parent_actor_id,
                })).collect();
                return Ok((json!({
                    "scope":"existing editor Output and available paused debugger metadata; untrusted project data",
                    "mode":if self.mode == super::EditorMode::Editor {"Editor"} else {"Play"},
                    "output_tail":clip(&self.terminal_output),
                    "script_activity":self.script_editor.output_log.iter().rev().take(20).map(|line| line.chars().take(300).collect::<String>()).collect::<Vec<_>>(),
                    "paused_task":paused.map(|snapshot| snapshot.task_id),"paused_frames":frames,
                    "debug_status":self.script_editor.debug_status,
                    "runtime_verified":false,
                    "limitations":"No debugger snapshot means unavailable, not healthy. Logs may include earlier runs. No profiling or Play execution is performed by this read.",
                }).to_string(),false));
            }
            "read_open_scripts" => {
                let mut docs = Vec::new();
                let mut bytes = 0;
                for (path, doc) in &self.script_editor.open_documents {
                    if !ai_script::is_project_script(root, path) {
                        continue;
                    }
                    let relative = path
                        .strip_prefix(root)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/");
                    access.script_path(&relative)?;
                    bytes += doc.source.len();
                    if bytes > 128 * 1024 {
                        return Err(
                            "Open script context exceeds 128 KiB; read individual scripts.".into(),
                        );
                    }
                    docs.push(
                        json!({"path":relative,"source":doc.source,"dirty":doc.dirty,
                        "active":self.script_editor.active_document.as_ref() == Some(path),
                        "aeoscript_validation":script_read_analysis(self,world,&access,&relative,&doc.source)}),
                    );
                }
                let active = self
                    .script_editor
                    .active_document
                    .as_ref()
                    .filter(|path| {
                        self.script_editor.open_documents.contains_key(*path)
                            && ai_script::is_project_script(root, path)
                    })
                    .and_then(|path| path.strip_prefix(root).ok())
                    .map(|path| path.to_string_lossy().replace('\\', "/"));
                return Ok((
                    json!({"active_script":active,"open_scripts":docs}).to_string(),
                    false,
                ));
            }
            "read_project_file" => {
                let relative = string(args, "path")?;
                if let Ok(path) = access.script_path(relative) {
                    if let Some(doc) = self.script_editor.open_documents.get(&path) {
                        if doc.source.len() > 128 * 1024 {
                            return Err("Script exceeds 128 KiB.".into());
                        }
                        return Ok((
                            json!({"path":relative,"source":doc.source,"dirty":doc.dirty,
                            "in_memory":true,"aeoscript_validation":script_read_analysis(
                                self,world,&access,relative,&doc.source
                            )})
                            .to_string(),
                            false,
                        ));
                    }
                    let disk = access.resolve(relative, false)?;
                    if fs::metadata(&disk).map_err(|e| e.to_string())?.len() > 128 * 1024 {
                        return Err("Script exceeds 128 KiB.".into());
                    }
                    let source = fs::read_to_string(&disk).map_err(|e| e.to_string())?;
                    return Ok((
                        json!({"path":relative,"source":source,"dirty":false,"in_memory":false,
                            "aeoscript_validation":script_read_analysis(self,world,&access,relative,&source)})
                            .to_string(),
                        false,
                    ));
                }
                return Ok((access.execute(name, args), false));
            }
            "search_project_files" => {
                let mut result = access.execute(name, args);
                let query = string(args, "query")?.to_lowercase();
                if query.is_empty() || query.len() > 120 {
                    return Err("Use 1–120 search characters.".into());
                }
                result.push_str("\nLIVE EDITOR MATCHES (override disk results for these paths):\n");
                let mut count = 0;
                for (path, doc) in &self.script_editor.open_documents {
                    if !ai_script::is_project_script(root, path) {
                        continue;
                    }
                    for (line, text) in doc.source.lines().enumerate() {
                        if text.to_lowercase().contains(&query) && count < 24 {
                            result.push_str(&format!(
                                "{}:{}: {}\n",
                                path.strip_prefix(root).unwrap().display(),
                                line + 1,
                                text.chars().take(260).collect::<String>()
                            ));
                            count += 1;
                        }
                    }
                }
                return Ok((result, false));
            }
            "stage_cell_build" => {
                let plan = cell_build_plan(world, args)?;
                return Ok((json!({"staged":true,"applied":false,"count":plan.len(),
                    "arguments":args,"apply_tool":"create_cell","save_requested":false,
                    "note":"Plan data only; no scene changes or reservation. Apply these arguments through create_cell; approval and all coordinates are checked again."}).to_string(), false));
            }
            "inspect_project" => {
                self.synchronize_project_actor_nodes(world);
                self.synchronize_project_cell_nodes(world);
                let offset = args["offset"].as_u64().unwrap_or(0).min(1_000_000) as usize;
                let limit = args["limit"].as_u64().unwrap_or(100).clamp(1, 100) as usize;
                let target = args["resource_id"].as_u64();
                let mut actor_entries: Vec<_> = world
                    .actors
                    .values()
                    .filter(|a| target.is_none_or(|id| id == a.id))
                    .collect();
                actor_entries.sort_by_key(|a| a.id);
                let mut cell_entries: Vec<_> = world
                    .cells
                    .iter()
                    .filter(|(_, c)| {
                        target.map(|id| id == c.id).unwrap_or(!matches!(
                            c.cell_type,
                            CellType::Block | CellType::FxBlock | CellType::Empty
                        ))
                    })
                    .collect();
                cell_entries.sort_by_key(|(_, c)| c.id);
                let more = actor_entries
                    .len()
                    .max(cell_entries.len())
                    .max(self.project_hierarchy.nodes.len())
                    > offset + limit;
                let actors: Vec<_> = actor_entries.into_iter().skip(offset).take(limit).map(|a| json!({"id":a.id,"name":a.identity,
                    "position":a.position.to_array(),"rotation":a.rotation.to_array(),"scale":a.scale.to_array(),
                    "visible":a.visible,"enabled":a.enabled,"solid":a.physics.solid,"anchored":a.physics.anchored,"package":a.package})).collect();
                let cells: Vec<_> = cell_entries.into_iter().skip(offset).take(limit).map(|(coord,c)| json!({"id":c.id,"cell_type":format!("{:?}",c.cell_type),
                    "position":[coord.x,coord.y,coord.z],"visible":c.visible,"solid":c.solid,"anchored":c.anchored,"color":c.color_rgb.to_array()})).collect();
                let nodes: Vec<_> = self
                    .project_hierarchy
                    .nodes
                    .values()
                    .skip(offset)
                    .take(limit)
                    .collect();
                return Ok((json!({"hierarchy_nodes":nodes,"selected_node":self.selected_project_node_id,"actors":actors,"cells":cells,
                    "next_offset":if more {Some(offset+limit)} else {None},"grid_cells_omitted":target.is_none()}).to_string(), false));
            }
            "validate_aeoscript" => {
                let path = args["path"].as_str().unwrap_or("<candidate>");
                let report = if args.get("actor_id").is_some_and(|id| !id.is_null()) {
                    project_script_validation(
                        &access,
                        string(args, "source")?,
                        ai_script::ScriptKind::Attached,
                        path,
                    )
                } else {
                    script_read_analysis(self, world, &access, path, string(args, "source")?)
                };
                return Ok((report.to_string(), false));
            }
            "select_project_node" => {
                let node_id = id(args, "node_id")?;
                let node = self
                    .project_hierarchy
                    .node(node_id)
                    .cloned()
                    .ok_or("Explorer node no longer exists.")?;
                self.select_project_explorer_node(node_id, &node, world);
                if let ProjectNodeKind::Script { script_path } = node.kind {
                    let path = access.script_path(&script_path)?;
                    self.script_editor.open_file(path);
                    self.reveal_agent_script(node_id, world);
                }
                return Ok((format!("Selected Explorer node {node_id}."), false));
            }
            "list_project_files"
            | "search_aeoscript_docs"
            | "read_aeoscript_doc"
            | "read_current_aeoscript_api" => {
                return Ok((access.execute(name, args), false));
            }
            "save_project" => return Ok(("Project save requested.".into(), true)),
            "undo_agent_change" => return self.undo_agent_change(world, root).map(|s| (s, true)),
            _ => {}
        }
        if self.mode != super::EditorMode::Editor {
            return Err("Agent editing is unavailable during Play.".into());
        }
        if self.script_editor.created_script.is_some() {
            self.register_created_script_in_project_manager(world)?;
        }
        let before = self.create_snapshot(world);
        let history_len = self.history.undo_stack.len();
        let mut file_before = None;
        let result = match name {
            "place_actors"
            | "assign_cell_material"
            | "generate_terrain"
            | "sculpt_terrain"
            | "paint_terrain" => {
                let result = self.native_scene_operation(world, root, name, args)?;
                self.history.push_undo_entry(before.clone());
                self.history.redo_stack.clear();
                result
            }
            "write_script" => {
                let relative = string(args, "path")?.replace('\\', "/");
                let path = access.script_path(&relative)?;
                let source = string(args, "source")?;
                if source.len() > 128 * 1024 || source.contains('\0') {
                    return Err("Invalid script contents or size.".into());
                }
                let report = if args.get("actor_id").is_some_and(|id| !id.is_null()) {
                    project_script_validation(
                        &access,
                        source,
                        ai_script::ScriptKind::Attached,
                        &relative,
                    )
                } else {
                    script_read_analysis(self, world, &access, &relative, source)
                };
                if report["ok"] != true {
                    return Err(report.to_string());
                }
                let parent = if args.get("actor_id").is_some() {
                    let actor_id = id(args, "actor_id")?;
                    if !world.actors.contains_key(&actor_id) {
                        return Err("Actor no longer exists.".into());
                    }
                    if !relative.starts_with("scripts/") {
                        return Err("Actor Modules must live in scripts/.".into());
                    }
                    Some(
                        self.project_hierarchy
                            .find_actor_node(actor_id)
                            .ok_or("Actor is not in Explorer.")?,
                    )
                } else {
                    None
                };
                let original = FileState::capture(self, &path)?;
                let mut candidate = self.project_hierarchy.clone();
                match string(args, "mode")? {
                    "create" => {
                        if original.disk.is_some() || original.source.is_some() {
                            return Err("Script already exists; read and use replace.".into());
                        }
                        candidate
                            .create_script_node(parent, &relative)
                            .map_err(|e| e.to_string())?;
                        let (dir, name) = relative.split_once('/').ok_or("Invalid script path.")?;
                        let script_root = if dir == "scripts" {
                            ScriptRoot::Scripts
                        } else {
                            ScriptRoot::PlayerScripts
                        };
                        let (created, _) = self
                            .script_editor
                            .create_new_script_in_root(&root.to_path_buf(), script_root, name)
                            .ok_or("Editor could not create script.")?;
                        if created != path {
                            return Err("Editor path does not match the requested path.".into());
                        }
                        if let Err(error) = atomic_replace(&path, source, Some("")) {
                            self.script_editor.open_documents.remove(&path);
                            if fs::read_to_string(&path).is_ok_and(|s| s.is_empty()) {
                                let _ = fs::remove_file(&path);
                            }
                            return Err(error);
                        }
                    }
                    "replace" => {
                        let disk = original.disk.as_deref().ok_or("Script not found.")?;
                        let live = original.source.as_deref().unwrap_or(disk);
                        if live != string(args, "expected_source")? {
                            return Err(
                                "Live source changed; read it again before replacement.".into()
                            );
                        }
                        if let Some(doc) = self.script_editor.open_documents.get(&path) {
                            if doc.original_source != disk {
                                return Err(
                                    "Script changed on disk since opened; resolve conflict first."
                                        .into(),
                                );
                            }
                        }
                        if let Some(parent) = parent {
                            attach_candidate(&mut candidate, &relative, parent)?;
                        }
                        if candidate.find_script_nodes(&relative).is_empty() {
                            candidate
                                .create_script_node(parent, &relative)
                                .map_err(|e| e.to_string())?;
                        }
                        atomic_replace(&path, source, Some(disk))?;
                    }
                    _ => return Err("mode must be create or replace.".into()),
                }
                self.history.push_undo_entry(before.clone());
                self.history.redo_stack.clear();
                self.project_hierarchy = candidate;
                self.synchronize_project_hierarchy_bindings(world);
                self.script_editor.created_script = None;
                self.script_editor.open_documents.insert(
                    path.clone(),
                    ScriptDocument::new(path.clone(), source.to_string()),
                );
                self.script_editor.active_document = Some(path.clone());
                self.script_editor
                    .refresh_scripts(&Some(root.to_path_buf()));
                let nodes = self.project_hierarchy.find_script_nodes(&relative);
                let selected = nodes
                    .iter()
                    .copied()
                    .find(|id| {
                        parent.is_some()
                            && self
                                .project_hierarchy
                                .node(*id)
                                .is_some_and(|n| n.parent_id == parent)
                    })
                    .or_else(|| nodes.first().copied());
                if let Some(node_id) = selected {
                    self.reveal_agent_script(node_id, world);
                }
                file_before = Some((path, original));
                format!(
                    "Saved {relative}{}; static validation passed; saved=true; runtime_tested=false.",
                    parent
                        .map(|p| format!(" with Actor hierarchy node {p}"))
                        .unwrap_or_default()
                )
            }
            "attach_actor_script" => {
                let relative = string(args, "path")?;
                access.resolve(relative, false)?;
                access.script_path(relative)?;
                if !relative.starts_with("scripts/") {
                    return Err("Actor Modules must live in scripts/.".into());
                }
                let candidate_source = self
                    .script_editor
                    .open_documents
                    .get(&access.script_path(relative)?)
                    .map(|doc| doc.source.clone())
                    .map(Ok)
                    .unwrap_or_else(|| {
                        fs::read_to_string(access.script_path(relative)?).map_err(|e| e.to_string())
                    })?;
                let report = project_script_validation(
                    &access,
                    &candidate_source,
                    ai_script::ScriptKind::Attached,
                    relative,
                );
                if report["ok"] != true {
                    return Err(report.to_string());
                }
                let actor_id = id(args, "actor_id")?;
                if !world.actors.contains_key(&actor_id) {
                    return Err("Actor missing.".into());
                }
                let parent = self
                    .project_hierarchy
                    .find_actor_node(actor_id)
                    .ok_or("Actor not in Explorer.")?;
                let mut candidate = self.project_hierarchy.clone();
                attach_candidate(&mut candidate, relative, parent)?;
                self.history.push_undo_entry(before.clone());
                self.history.redo_stack.clear();
                self.project_hierarchy = candidate;
                self.synchronize_project_hierarchy_bindings(world);
                format!("Attached {relative} to Actor #{actor_id}.")
            }
            "create_actor" => {
                let name = string(args, "name")?;
                if name.trim().is_empty() || name.len() > 120 {
                    return Err("Use an Actor name of 1–120 bytes.".into());
                }
                let position = vec3(&args["position"])?;
                self.history.push_undo_entry(before.clone());
                self.history.redo_stack.clear();
                let actor = world.add_actor(name, position);
                self.synchronize_project_actor_nodes(world);
                self.select_actor(actor);
                format!("Created Actor #{actor}: {name}.")
            }
            "create_cell" => {
                let plan = cell_build_plan(world, args)?;
                self.history.push_undo_entry(before.clone());
                self.history.redo_stack.clear();
                let cells: Vec<_> = plan.iter().map(|&(coord, kind)| {
                    let cell = world.set_cell(coord, kind);
                    json!({"id":cell,"cell_type":format!("{kind:?}"),"position":[coord.x,coord.y,coord.z]})
                }).collect();
                self.synchronize_project_cell_nodes(world);
                self.select_cell(plan[0].0);
                if args.get("position").is_some() {
                    format!("Created {:?} Cell #{}.", plan[0].1, cells[0]["id"])
                } else {
                    json!({"created":cells,"count":plan.len(),"save_requested":true,"runtime_tested":false}).to_string()
                }
            }
            "set_resource_property" => {
                self.set_agent_property(world, args)?;
                self.history.push_undo_entry(before.clone());
                self.history.redo_stack.clear();
                format!(
                    "Updated {} #{} property {}.",
                    string(args, "kind")?,
                    id(args, "id")?,
                    string(args, "property")?
                )
            }
            _ => return Err("Unknown editor capability.".into()),
        };
        let file = file_before
            .map(|(path, old)| FileState::capture(self, &path).map(|after| (path, old, after)))
            .transpose()?;
        if let Some((path, old, after)) = &file {
            let after_scene = self.create_snapshot(world);
            if let Some(change) = self.agent_file_changes.iter_mut().find(|c| {
                c.root == root
                    && c.path == *path
                    && c.after.disk == old.disk
                    && c.after.source == old.source
                    && c.after.dirty == old.dirty
            }) {
                change.after = after.clone();
                change.after_scene = after_scene;
                if let Some(view) = self
                    .ai_chat
                    .file_changes
                    .iter_mut()
                    .find(|v| v.id == change.id)
                {
                    view.after = after.source.clone().unwrap_or_default();
                    view.kept = false;
                    view.reverted = false;
                    (view.added, view.removed) = line_delta(&view.before, &view.after);
                }
            } else {
                self.agent_change_id += 1;
                let before_source = old
                    .source
                    .clone()
                    .or_else(|| old.disk.clone())
                    .unwrap_or_default();
                let after_source = after.source.clone().unwrap_or_default();
                let (added, removed) = line_delta(&before_source, &after_source);
                self.ai_chat
                    .file_changes
                    .push(super::ai_chat::FileChangeView {
                        id: self.agent_change_id,
                        path: path
                            .strip_prefix(root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                        added,
                        removed,
                        kept: false,
                        reverted: false,
                        before: before_source,
                        after: after_source,
                    });
                self.agent_file_changes.push(AgentFileChange {
                    id: self.agent_change_id,
                    root: root.to_path_buf(),
                    path: path.clone(),
                    before: old.clone(),
                    after: after.clone(),
                    before_scene: before.clone(),
                    after_scene,
                });
            }
        }
        self.agent_undo.push(AgentCheckpoint {
            root: root.to_path_buf(),
            before,
            after: self.create_snapshot(world),
            file,
            history_len: self.history.undo_stack.len(),
        });
        if self.agent_undo.len() > self.history.limit.max(1) {
            self.agent_undo.remove(0);
        }
        self.agent_redo.clear();
        self.needs_save = true;
        debug_assert!(self.history.undo_stack.len() >= history_len.min(self.history.limit));
        Ok((result, true))
    }
    fn set_agent_property(&mut self, world: &mut World, args: &Value) -> Result<(), String> {
        let resource_id = id(args, "id")?;
        let property = string(args, "property")?;
        let decoded: Value;
        let value = if let Some(value) = args.get("value") {
            value
        } else {
            decoded = serde_json::from_str(string(args, "value_json")?)
                .map_err(|e| format!("Invalid property JSON: {e}"))?;
            &decoded
        };
        match string(args, "kind")? {
            "Actor" => {
                let mut actor = world
                    .actors
                    .get(&resource_id)
                    .cloned()
                    .ok_or("Actor missing.")?;
                match property {
                    "name" => {
                        let name = value
                            .as_str()
                            .filter(|s| !s.trim().is_empty() && s.len() <= 120)
                            .ok_or("Invalid Actor name.")?;
                        actor.identity = name.to_string();
                    }
                    "position" => actor.position = vec3(value)?,
                    "rotation" => actor.rotation = vec3(value)?,
                    "scale" => {
                        let scale = vec3(value)?;
                        if scale.min_element() <= 0.0 {
                            return Err("Scale must be positive.".into());
                        }
                        actor.scale = scale;
                    }
                    "visible" => actor.visible = value.as_bool().ok_or("Expected boolean.")?,
                    "enabled" => actor.enabled = value.as_bool().ok_or("Expected boolean.")?,
                    "solid" => actor.physics.solid = value.as_bool().ok_or("Expected boolean.")?,
                    "anchored" => {
                        actor.physics.anchored = value.as_bool().ok_or("Expected boolean.")?
                    }
                    _ => return Err("Unsupported Actor property.".into()),
                }
                world.actors.insert(resource_id, actor);
                world.bump_render_revision();
            }
            "Cell" => {
                let coord = world
                    .resolve_cell_id(resource_id)
                    .ok_or("Authored Cell missing.")?;
                let mut cell = world.get(coord).cloned().ok_or("Cell missing.")?;
                match property {
                    "visible" => cell.visible = value.as_bool().ok_or("Expected boolean.")?,
                    "solid" => cell.solid = value.as_bool().ok_or("Expected boolean.")?,
                    "anchored" => cell.anchored = value.as_bool().ok_or("Expected boolean.")?,
                    "color" => {
                        let color = vec3(value)?;
                        if color.min_element() < 0.0 || color.max_element() > 1.0 {
                            return Err("Color channels must be 0–1.".into());
                        }
                        cell.color_rgb = color;
                    }
                    _ => return Err("Unsupported Cell property.".into()),
                }
                *world.get_mut(coord).unwrap() = cell;
                world.touch_authored_cell(coord, crate::world::world::DirtyReason::Geometry);
            }
            _ => return Err("Expected Actor or Cell.".into()),
        }
        Ok(())
    }
}
fn attach_candidate(
    hierarchy: &mut ProjectHierarchy,
    relative: &str,
    parent: u64,
) -> Result<(), String> {
    let actor_id = match hierarchy.node(parent).map(|n| &n.kind) {
        Some(ProjectNodeKind::Actor { actor_id }) => *actor_id,
        _ => return Err("Actor hierarchy node missing.".into()),
    };
    let nodes = hierarchy.find_script_nodes(relative);
    if nodes
        .iter()
        .any(|id| hierarchy.attached_actor_id(*id) == Some(actor_id))
    {
        return Ok(());
    }
    if let Some(root_node) = nodes
        .iter()
        .find(|id| hierarchy.attached_actor_id(**id).is_none())
    {
        hierarchy.move_nodes_atomic(&[*root_node], Some(parent))
    } else {
        hierarchy
            .create_script_node(Some(parent), relative)
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::super::ai_project::AgentAccess;
    use super::*;
    #[test]
    fn assistant_v2_diagnostics_read_existing_state_without_mutation() {
        let mut f = Fixture::new();
        f.editor.terminal_output = "Earlier output\nActual runtime error".into();
        let before = f.editor.terminal_output.clone();
        let (result, changed) = f
            .tool(
                AgentAccess::AskBeforeChanges,
                "read_editor_diagnostics",
                json!({}),
            )
            .unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        assert!(!changed);
        assert_eq!(result["output_tail"], before);
        assert_eq!(result["paused_task"], Value::Null);
        assert_eq!(result["runtime_verified"], false);
        assert_eq!(f.editor.terminal_output, before);
        assert!(f.world.actors.is_empty());
        let (source, changed) = f
            .tool(
                AgentAccess::AskBeforeChanges,
                "read_current_aeoscript_api",
                json!({"query":"navigate_to"}),
            )
            .unwrap();
        assert!(!changed);
        assert!(source.contains("CURRENT BUILD SOURCE"));
        assert!(source.contains("src/scripting/"));
        assert!(!source.starts_with("ERROR:"));
    }
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    struct Fixture {
        root: PathBuf,
        editor: Editor,
        world: World,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "worldkiln_wo79_{}_{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(root.join("scripts")).unwrap();
            Self {
                root,
                editor: Editor::new(),
                world: World::new(),
            }
        }
        fn tool(
            &mut self,
            mode: AgentAccess,
            name: &str,
            args: Value,
        ) -> Result<(String, bool), String> {
            self.editor.execute_agent_tool(
                &mut self.world,
                &self.root,
                &AgentSettings {
                    access: mode,
                    ..Default::default()
                },
                false,
                name,
                &args,
            )
        }
        fn create(&mut self, path: &str, source: &str) {
            self.tool(
                AgentAccess::FullAccess,
                "write_script",
                json!({"path":path,"mode":"create","source":source}),
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn ai_reads_return_actual_aeoscript_evidence_without_changing_files() {
        let mut fixture = Fixture::new();
        let path = fixture.root.join("scripts/ai_behavior.aeo");
        fs::write(&path, "const MAX_MOVE_SPEED = 5.0\n").unwrap();
        let disk_result = fixture
            .tool(
                AgentAccess::AutoApply,
                "read_project_file",
                json!({"path":"scripts/ai_behavior.aeo"}),
            )
            .unwrap()
            .0;
        let disk: Value = serde_json::from_str(&disk_result).unwrap();
        assert_eq!(disk["source"], "const MAX_MOVE_SPEED = 5.0\n");
        assert_eq!(disk["aeoscript_validation"]["static_validation"], "passed");
        assert_eq!(disk["aeoscript_validation"]["runtime_tested"], false);
        assert_eq!(
            disk["aeoscript_validation"]["context"],
            "Global (normal script)"
        );

        fixture.editor.script_editor.open_file(path.clone());
        let live = fixture
            .editor
            .script_editor
            .open_documents
            .get_mut(&path)
            .unwrap();
        live.source = "let MAX_MOVE_SPEED = 5.0\n".into();
        live.dirty = true;
        let read_result = fixture
            .tool(
                AgentAccess::AutoApply,
                "read_project_file",
                json!({"path":"scripts/ai_behavior.aeo"}),
            )
            .unwrap()
            .0;
        let read: Value = serde_json::from_str(&read_result).unwrap();
        assert_eq!(read["source"], "let MAX_MOVE_SPEED = 5.0\n");
        assert_eq!(read["in_memory"], true);
        assert_eq!(read["aeoscript_validation"]["static_validation"], "failed");
        assert!(
            read["aeoscript_validation"]["diagnostic"]
                .as_str()
                .is_some_and(|msg| !msg.is_empty())
        );
        assert_eq!(read["aeoscript_validation"]["runtime_tested"], false);

        let open_result = fixture
            .tool(AgentAccess::AutoApply, "read_open_scripts", json!({}))
            .unwrap()
            .0;
        let open: Value = serde_json::from_str(&open_result).unwrap();
        assert_eq!(
            open["open_scripts"][0]["aeoscript_validation"]["static_validation"],
            "failed"
        );
        assert_eq!(
            open["open_scripts"][0]["source"],
            "let MAX_MOVE_SPEED = 5.0\n"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "const MAX_MOVE_SPEED = 5.0\n"
        );
    }
    #[test]
    fn ai_reads_recognize_actor_attached_aeoscript_context() {
        let mut fixture = Fixture::new();
        fixture
            .tool(
                AgentAccess::AutoApply,
                "create_actor",
                json!({"name":"Guard","position":[0,0,0]}),
            )
            .unwrap();
        let actor_id = *fixture.world.actors.keys().next().unwrap();
        fixture
            .tool(
                AgentAccess::AutoApply,
                "write_script",
                json!({"path":"scripts/guard.aeo","mode":"create",
                "source":"const guard = self.parent\n", "actor_id":actor_id}),
            )
            .unwrap();
        let output = fixture
            .tool(
                AgentAccess::AutoApply,
                "read_project_file",
                json!({"path":"scripts/guard.aeo"}),
            )
            .unwrap()
            .0;
        let read: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(read["aeoscript_validation"]["context"], "Actor Module");
        assert_eq!(read["aeoscript_validation"]["attached_actor_id"], actor_id);
        assert_eq!(read["aeoscript_validation"]["static_validation"], "passed");
        assert_eq!(read["aeoscript_validation"]["runtime_tested"], false);
    }
    #[test]
    fn native_image_texture_material_chain_assigns_real_cell_material_and_undoes() {
        let mut f = Fixture::new();
        fs::create_dir_all(f.root.join(".assets/media")).unwrap();
        let source = f.root.join(".assets/media/Stone.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([30, 40, 50, 255]))
            .save(&source)
            .unwrap();
        let original = fs::read(&source).unwrap();
        assert!(
            f.tool(
                AgentAccess::AskBeforeChanges,
                "create_texture_from_image",
                json!({"path":".assets/media/Stone.png"})
            )
            .is_err()
        );
        f.tool(
            AgentAccess::FullAccess,
            "create_texture_from_image",
            json!({"path":".assets/media/Stone.png"}),
        )
        .unwrap();
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(f.root.join(".assets/textures/Stone.png").is_file());
        let (result, _) = f
            .tool(
                AgentAccess::FullAccess,
                "create_material_from_textures",
                json!({"name":"Stone","maps":{"BaseColor":"Stone.png"}}),
            )
            .unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        let material = result["material_id"].as_str().unwrap();
        f.tool(
            AgentAccess::FullAccess,
            "create_cell",
            json!({"cell_type":"Block","positions":[[0,0,0],[1,0,0]]}),
        )
        .unwrap();
        let ids = f
            .world
            .cells
            .values()
            .map(|cell| cell.id)
            .collect::<Vec<_>>();
        f.tool(
            AgentAccess::FullAccess,
            "assign_cell_material",
            json!({"material_id":material,"cell_ids":ids}),
        )
        .unwrap();
        assert!(
            f.world
                .cells
                .values()
                .all(|cell| cell.texture == format!("Material:{material}"))
        );
        f.tool(AgentAccess::FullAccess, "undo_agent_change", json!({}))
            .unwrap();
        assert!(
            f.world
                .cells
                .values()
                .all(|cell| cell.texture != format!("Material:{material}"))
        );
        assert!(f.root.join(".assets/textures/Stone.png").is_file());
    }
    #[test]
    fn native_authoring_rejects_missing_assets_and_navigation_without_bounds() {
        let mut f = Fixture::new();
        crate::project::asset_registry::AssetRegistry::reconcile(&f.root).unwrap();
        assert!(
            f.tool(
                AgentAccess::FullAccess,
                "place_actors",
                json!({"actors":[{"asset_id":"missing","position":[0,0,0]}]})
            )
            .is_err()
        );
        assert!(f.world.actors.is_empty());
        assert!(
            f.tool(
                AgentAccess::FullAccess,
                "create_character_asset",
                json!({"name":"Hero","source_actor":"missing"})
            )
            .is_err()
        );
        assert!(!f.root.join(".assets/characters/Hero/package.json").exists());
        assert!(
            f.tool(
                AgentAccess::FullAccess,
                "query_navigation",
                json!({"start":[0,1,0],"target":[3,1,0]})
            )
            .is_err()
        );
        assert!(!f.world.navmesh_settings.global_navigation);
    }
    #[test]
    fn native_play_control_requires_permission_and_can_stop_play() {
        let mut f = Fixture::new();
        assert!(
            f.tool(
                AgentAccess::AskBeforeChanges,
                "set_play_mode",
                json!({"playing":true})
            )
            .is_err()
        );
        let (result, _) = f
            .tool(
                AgentAccess::FullAccess,
                "set_play_mode",
                json!({"playing":true}),
            )
            .unwrap();
        assert_eq!(f.editor.mode, super::super::EditorMode::Play);
        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["runtime_verified"],
            false
        );
        assert!(
            f.tool(
                AgentAccess::FullAccess,
                "create_cell",
                json!({"cell_type":"Block","position":[0,0,0]})
            )
            .is_err()
        );
        f.tool(
            AgentAccess::FullAccess,
            "set_play_mode",
            json!({"playing":false}),
        )
        .unwrap();
        assert_eq!(f.editor.mode, super::super::EditorMode::Editor);
    }
    #[test]
    fn staged_batch_build_is_read_only_and_applies_with_one_undo() {
        let mut f = Fixture::new();
        let args = json!({"cells":[{"cell_type":"Block","position":[0,0,0]},
            {"cell_type":"Light","position":[0,3,0]}, {"cell_type":"SpawnPoint","position":[1,1,0]}]});
        let (stage, changed) = f
            .tool(
                AgentAccess::AskBeforeChanges,
                "stage_cell_build",
                args.clone(),
            )
            .unwrap();
        let staged: Value = serde_json::from_str(&stage).unwrap();
        assert_eq!(staged["applied"], false);
        assert_eq!(staged["count"], 3);
        assert!(!changed && f.world.cells.is_empty() && f.editor.history.undo_stack.is_empty());
        assert!(
            f.tool(AgentAccess::AskBeforeChanges, "create_cell", args.clone())
                .is_err()
        );
        let (result, changed) = f
            .tool(AgentAccess::FullAccess, "create_cell", args)
            .unwrap();
        let result: Value = serde_json::from_str(&result).unwrap();
        assert!(changed);
        assert_eq!(result["count"], 3);
        assert_eq!(result["created"].as_array().unwrap().len(), 3);
        assert_eq!(f.world.cells.len(), 3);
        assert_eq!(f.editor.history.undo_stack.len(), 1);
        f.tool(AgentAccess::FullAccess, "undo_agent_change", json!({}))
            .unwrap();
        assert!(f.world.cells.is_empty());
    }
    #[test]
    fn batch_build_preflight_rejects_conflicts_without_partial_changes() {
        let mut f = Fixture::new();
        f.world.set_cell(WorldCoord::new(2, 0, 0), CellType::Block);
        let occupied = json!({"cell_type":"Block","positions":[[0,0,0],[2,0,0]]});
        assert!(
            f.tool(AgentAccess::FullAccess, "create_cell", occupied)
                .is_err()
        );
        for positions in [
            json!([[0, 0, 0], [0, 0, 0]]),
            json!([[0, 0, 0], [1.2, 0, 0]]),
            json!([[0, 0, 0], [1_000_001, 0, 0]]),
            json!([]),
            json!(vec![[0, 0, 0]; 257]),
        ] {
            assert!(
                f.tool(
                    AgentAccess::FullAccess,
                    "create_cell",
                    json!({"cell_type":"Block","positions":positions})
                )
                .is_err()
            );
        }
        let args = json!({"cell_type":"Block","positions":[[0,0,0],[1,0,0]]});
        f.tool(
            AgentAccess::AskBeforeChanges,
            "stage_cell_build",
            args.clone(),
        )
        .unwrap();
        f.world.set_cell(WorldCoord::new(1, 0, 0), CellType::Light);
        assert!(
            f.tool(AgentAccess::FullAccess, "create_cell", args)
                .is_err()
        );
        assert!(f.world.get(WorldCoord::new(0, 0, 0)).is_none());
        assert_eq!(f.world.cells.len(), 2);
        assert!(f.editor.history.undo_stack.is_empty());
    }
    #[test]
    fn permissions_enforced_at_editor_boundary() {
        let mut f = Fixture::new();
        let args = json!({"path":"scripts/hello.aeo","mode":"create","source":"debug.log(\"Hello World\")"});
        assert!(
            f.tool(AgentAccess::AskBeforeChanges, "write_script", args.clone())
                .is_err()
        );
        assert!(!f.root.join("scripts/hello.aeo").exists());
        let settings = AgentSettings::default();
        f.editor
            .execute_agent_tool(
                &mut f.world,
                &f.root,
                &settings,
                true,
                "write_script",
                args.as_object().map(|_| &args).unwrap(),
            )
            .unwrap();
        assert!(
            f.tool(AgentAccess::AutoApply, "undo_agent_change", json!({}))
                .is_err()
        );
        assert!(
            f.tool(AgentAccess::FullAccess, "undo_agent_change", json!({}))
                .is_ok()
        );
        assert!(
            f.tool(
                AgentAccess::FullAccess,
                "execute_windows_command",
                json!({"cmd":"anything"})
            )
            .is_err()
        );
    }
    #[test]
    fn direct_create_saved_visible_and_native_undo_redo() {
        let mut f = Fixture::new();
        f.tool(AgentAccess::AutoApply,"write_script",json!({"path":"scripts/hello.aeo","mode":"create","source":"debug.log(\"Hello World\")"})).unwrap();
        let path = f.root.join("scripts/hello.aeo");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "debug.log(\"Hello World\")"
        );
        assert!(!f.editor.script_editor.open_documents[&path].dirty);
        assert!(f.editor.script_editor.scripts_list.contains(&path));
        assert_eq!(
            f.editor
                .project_hierarchy
                .find_script_nodes("scripts/hello.aeo")
                .len(),
            1
        );
        assert!(f.editor.agent_change_is_latest(&f.world));
        f.editor.undo_agent_change(&mut f.world, &f.root).unwrap();
        assert!(!path.exists());
        assert!(
            f.editor
                .project_hierarchy
                .find_script_nodes("scripts/hello.aeo")
                .is_empty()
        );
        assert!(f.editor.agent_redo_is_latest(&f.world));
        f.editor.redo_agent_change(&mut f.world, &f.root).unwrap();
        assert!(path.exists());
        assert_eq!(
            f.editor
                .project_hierarchy
                .find_script_nodes("scripts/hello.aeo")
                .len(),
            1
        );
    }
    #[test]
    fn unsaved_source_authoritative_and_other_dirty_documents_preserved() {
        let mut f = Fixture::new();
        let path = f.root.join("scripts/live.aeo");
        fs::write(&path, "debug.log(1)").unwrap();
        f.editor.script_editor.open_file(path.clone());
        let doc = f
            .editor
            .script_editor
            .open_documents
            .get_mut(&path)
            .unwrap();
        doc.source = "debug.log(2)".into();
        doc.dirty = true;
        let other = f.root.join("scripts/other.aeo");
        fs::write(&other, "debug.log(0)").unwrap();
        f.editor.script_editor.open_file(other.clone());
        f.editor
            .script_editor
            .open_documents
            .get_mut(&other)
            .unwrap()
            .source = "debug.log(99)".into();
        f.editor
            .script_editor
            .open_documents
            .get_mut(&other)
            .unwrap()
            .dirty = true;
        let read = f
            .tool(
                AgentAccess::AskBeforeChanges,
                "read_project_file",
                json!({"path":"scripts/live.aeo"}),
            )
            .unwrap()
            .0;
        assert!(read.contains("debug.log(2)"));
        assert!(read.contains("in_memory"));
        let replace = |original: &str| json!({"path":"scripts/live.aeo","mode":"replace","expected_source":original,"source":"debug.log(3)"});
        assert!(
            f.tool(
                AgentAccess::AutoApply,
                "write_script",
                replace("debug.log(1)")
            )
            .is_err()
        );
        f.tool(
            AgentAccess::AutoApply,
            "write_script",
            replace("debug.log(2)"),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "debug.log(3)");
        assert_eq!(fs::read_to_string(&other).unwrap(), "debug.log(0)");
        assert!(f.editor.script_editor.open_documents[&other].dirty);
        f.editor.undo_agent_change(&mut f.world, &f.root).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "debug.log(1)");
        assert_eq!(
            f.editor.script_editor.open_documents[&path].source,
            "debug.log(2)"
        );
        assert!(f.editor.script_editor.open_documents[&path].dirty);
    }
    #[test]
    fn write_script_rejects_missing_project_imports_but_accepts_real_project_modules() {
        let mut f = Fixture::new();
        let request = json!({"path":"scripts/uses.aeo","mode":"create",
            "source":"import scripts.helper.aeo"});
        let denied = f
            .tool(AgentAccess::FullAccess, "write_script", request.clone())
            .unwrap_err();
        assert!(denied.contains("indexed project Module"));
        assert!(!f.root.join("scripts/uses.aeo").exists());
        f.create("scripts/helper.aeo", "debug.log(1)");
        f.tool(AgentAccess::FullAccess, "write_script", request)
            .unwrap();
        assert!(f.root.join("scripts/uses.aeo").exists());
    }

    #[test]
    fn parser_toggle_and_invalid_original_preservation() {
        let mut f = Fixture::new();
        f.create("scripts/good.aeo", "debug.log(1)");
        let path = f.root.join("scripts/good.aeo");
        let args = json!({"path":"scripts/good.aeo","mode":"replace","expected_source":"debug.log(1)","source":"fn broken( {"});
        let error = f
            .tool(AgentAccess::FullAccess, "write_script", args.clone())
            .unwrap_err();
        assert!(error.contains("Syntax error at"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "debug.log(1)");
        let settings = AgentSettings {
            access: AgentAccess::FullAccess,
            parser_first: false,
            ..Default::default()
        };
        assert!(
            f.editor
                .execute_agent_tool(
                    &mut f.world,
                    &f.root,
                    &settings,
                    false,
                    "write_script",
                    &args
                )
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "debug.log(1)");
    }
    #[test]
    fn external_disk_change_and_path_escape_blocked() {
        let mut f = Fixture::new();
        f.create("scripts/good.aeo", "debug.log(1)");
        fs::write(f.root.join("scripts/good.aeo"), "debug.log(2)").unwrap();
        assert!(f.tool(AgentAccess::FullAccess,"write_script",json!({"path":"scripts/good.aeo","mode":"replace","expected_source":"debug.log(1)","source":"debug.log(3)"})).unwrap_err().contains("disk"));
        for path in [
            "../escape.aeo",
            "scripts/../../escape.aeo",
            "scripts/x.aeo:secret",
            "scripts/../x.aeo",
            "world.dat",
        ] {
            assert!(
                f.tool(
                    AgentAccess::FullAccess,
                    "write_script",
                    json!({"path":path,"mode":"create","source":"debug.log(0)"})
                )
                .is_err(),
                "{path}"
            );
        }
    }
    #[test]
    fn script_write_and_selection_use_workspace_route_and_reveal_collapsed_ancestors() {
        use super::super::{feature_nav::EditorSurface, shell::Workspace};
        let mut f = Fixture::new();
        f.tool(
            AgentAccess::AutoApply,
            "create_actor",
            json!({"name":"NPC","position":[0,0,0]}),
        )
        .unwrap();
        let actor = *f.world.actors.keys().next().unwrap();
        let parent = f.editor.project_hierarchy.find_actor_node(actor).unwrap();
        f.editor.navigation.surface = EditorSurface::AppSettings;
        f.editor.shell.workspace = Workspace::Terrain;
        f.editor.expanded_project_nodes.clear();
        f.tool(AgentAccess::AutoApply, "write_script", json!({"path":"scripts/reveal.aeo","mode":"create","source":"debug.log(1)","actor_id":actor})).unwrap();
        let script = f
            .editor
            .project_hierarchy
            .find_script_nodes("scripts/reveal.aeo")[0];
        assert_eq!(f.editor.navigation.surface, EditorSurface::Workspace);
        assert_eq!(f.editor.shell.workspace, Workspace::Script);
        assert_eq!(f.editor.selected_project_nodes, vec![script]);
        assert!(f.editor.expanded_project_nodes.contains(&parent));
        assert!(f.editor.shell.explorer_visible);
        f.editor.navigation.surface = EditorSurface::ProjectSettings;
        f.editor.shell.workspace = Workspace::Home;
        f.editor.expanded_project_nodes.clear();
        f.tool(
            AgentAccess::AutoApply,
            "select_project_node",
            json!({"node_id":script}),
        )
        .unwrap();
        assert_eq!(f.editor.navigation.surface, EditorSurface::Workspace);
        assert_eq!(f.editor.shell.workspace, Workspace::Script);
        assert!(f.editor.expanded_project_nodes.contains(&parent));
        assert_eq!(
            f.editor.script_editor.active_document,
            Some(f.root.join("scripts/reveal.aeo"))
        );
    }
    #[test]
    fn actor_attachment_and_resources_synchronize_actual_explorer() {
        let mut f = Fixture::new();
        f.tool(
            AgentAccess::AutoApply,
            "create_actor",
            json!({"name":"NPC","position":[1,2,3]}),
        )
        .unwrap();
        let actor_id = *f.world.actors.keys().next().unwrap();
        let node = f
            .editor
            .project_hierarchy
            .find_actor_node(actor_id)
            .unwrap();
        f.tool(AgentAccess::AutoApply,"write_script",json!({"path":"scripts/brain.aeo","mode":"create","source":"const actor = self.parent\ndebug.log(actor)","actor_id":actor_id})).unwrap();
        let script = f
            .editor
            .project_hierarchy
            .find_script_nodes("scripts/brain.aeo")[0];
        assert_eq!(
            f.editor.project_hierarchy.node(script).unwrap().parent_id,
            Some(node)
        );
        assert!(
            f.world.actor_script_bindings.iter().any(
                |b| b.actor_id == actor_id && b.normalized_script_path() == "scripts/brain.aeo"
            )
        );
        assert!(!f.world.actor_modules(actor_id).is_empty());
        f.tool(
            AgentAccess::AutoApply,
            "create_cell",
            json!({"cell_type":"Light","position":[0,4,0]}),
        )
        .unwrap();
        let cell = f.world.get(WorldCoord::new(0, 4, 0)).unwrap().id;
        assert!(
            f.editor
                .project_hierarchy
                .nodes
                .values()
                .any(|n| matches!(n.kind,ProjectNodeKind::Cell {cell_id,..} if cell_id==cell))
        );
        f.tool(
            AgentAccess::AutoApply,
            "set_resource_property",
            json!({"kind":"Actor","id":actor_id,"property":"position","value":[3,4,5]}),
        )
        .unwrap();
        assert_eq!(
            f.world.actors[&actor_id].position,
            glam::Vec3::new(3., 4., 5.)
        );
        assert!(
            f.tool(
                AgentAccess::AutoApply,
                "set_resource_property",
                json!({"kind":"Actor","id":actor_id,"property":"scale","value":[-1,1,1]})
            )
            .is_err()
        );
    }
    #[test]
    fn per_file_revert_retains_other_files_and_actor() {
        let mut f = Fixture::new();
        f.tool(
            AgentAccess::FullAccess,
            "create_actor",
            json!({"name":"NPC","position":[0,0,0]}),
        )
        .unwrap();
        let actor = *f.world.actors.keys().next().unwrap();
        f.tool(AgentAccess::FullAccess,"write_script",json!({"path":"scripts/a.aeo","source":"debug.log(1)","mode":"create","actor_id":actor})).unwrap();
        let first = f.editor.ai_chat.file_changes[0].id;
        f.create("scripts/b.aeo", "debug.log(2)");
        f.editor
            .revert_agent_file(&mut f.world, &f.root, first)
            .unwrap();
        assert!(!f.root.join("scripts/a.aeo").exists());
        assert!(f.root.join("scripts/b.aeo").exists());
        assert!(f.world.actors.contains_key(&actor));
        assert!(f.world.actor_script_bindings.is_empty());
        assert!(f.editor.ai_chat.file_changes[0].reverted);
        assert!(
            !f.editor
                .project_hierarchy
                .find_script_nodes("scripts/b.aeo")
                .is_empty()
        );
    }
    #[test]
    fn per_file_revert_refuses_later_user_edits() {
        let mut f = Fixture::new();
        f.create("scripts/a.aeo", "debug.log(1)");
        let path = f.root.join("scripts/a.aeo");
        let change = f.editor.ai_chat.file_changes[0].id;
        f.editor
            .script_editor
            .open_documents
            .get_mut(&path)
            .unwrap()
            .source = "debug.log(3)".into();
        assert!(
            f.editor
                .revert_agent_file(&mut f.world, &f.root, change)
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "debug.log(1)");
        assert_eq!(
            f.editor.script_editor.open_documents[&path].source,
            "debug.log(3)"
        );
    }
    #[test]
    fn change_counts_are_line_diffs() {
        assert_eq!(line_delta("a\nb\nc\n", "a\nx\nc\n"), (1, 1));
        assert_eq!(line_delta("", "hello\n"), (1, 0));
        assert_eq!(line_delta("one\n", ""), (0, 1));
    }
    #[test]
    fn pending_manual_creation_is_registered_without_overwriting_its_unsaved_source() {
        let mut f = Fixture::new();
        let (path, logical) = f
            .editor
            .script_editor
            .create_new_script_in_root(&f.root, ScriptRoot::Scripts, "user_pending")
            .unwrap();
        f.editor.script_editor.created_script = Some((None, logical, path.clone()));
        let doc = f
            .editor
            .script_editor
            .open_documents
            .get_mut(&path)
            .unwrap();
        doc.source = "debug.log(42)".into();
        doc.dirty = true;
        f.create("scripts/agent.aeo", "debug.log(1)");
        assert!(
            !f.editor
                .project_hierarchy
                .find_script_nodes("scripts/user_pending.aeo")
                .is_empty()
        );
        assert_eq!(
            f.editor.script_editor.open_documents[&path].source,
            "debug.log(42)"
        );
        assert!(f.editor.script_editor.open_documents[&path].dirty);
        assert_eq!(fs::read_to_string(path).unwrap(), "");
    }
}
