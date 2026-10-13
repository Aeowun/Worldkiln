pub mod script_editor_documents;
pub mod script_editor_files;
pub mod script_editor_highlighting;
// Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:48 -05:00
pub mod script_editor_problems;
pub mod script_editor_ui;

pub use script_editor_documents::ScriptDocument;
pub use script_editor_ui::SearchResult;
// Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 03:07 -05:00
pub use script_editor_problems::{ProjectProblem, ProjectProblemKind};
// Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:28 -05:00

use crate::project::{IndexedModule, ProjectNodeId, ProjectScriptIndex, ScriptRoot, ScriptTree};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

pub struct ScriptEditor {
    pub open_documents: HashMap<PathBuf, ScriptDocument>,
    pub active_document: Option<PathBuf>,

    /// Compatibility flat list for existing editor systems.
    pub scripts_list: Vec<PathBuf>,

    /// Flat compatibility list for PlayerScripts.
    pub playerscripts_list: Vec<PathBuf>,

    /// Structured project script hierarchy.
    pub scripts_tree: Option<ScriptTree>,

    /// Structured PlayerScripts hierarchy.
    pub playerscripts_tree: Option<ScriptTree>,

    /// Cached project-wide script index used for editor lookup.
    pub project_script_index: Option<ProjectScriptIndex>,

    pub show_new_script_dialog: bool,
    pub new_script_name: String,
    pub new_script_root: ScriptRoot,
    pub new_script_parent: Option<ProjectNodeId>,
    pub created_script: Option<(Option<ProjectNodeId>, String, PathBuf)>,

    pub show_rename_dialog: bool,
    pub rename_target: Option<PathBuf>,
    pub rename_new_name: String,

    pub search_query: String,
    pub show_search: bool,
    pub search_results: Vec<SearchResult>,

    pub closing_path: Option<PathBuf>,
    pub deleting_path: Option<PathBuf>,

    pub show_problems: bool,
    // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:48 -05:00
    pub project_problems: Vec<ProjectProblem>,
    /// Visibility for the editor-wide, bottom-docked Output panel.
    pub show_output: bool,
    /// Script-editor activity messages, rendered alongside runtime output.
    pub output_log: Vec<String>,
    output_revision: u64,

    pub cursor_request: Option<(PathBuf, usize)>,

    /// One-frame vertical scroll request for keeping the active caret visible.
    pub editor_scroll_to_y: Option<f32>,

    /// Symbol currently held under the pointer for delayed API help.
    pub api_doc_hover_symbol: Option<String>,
    /// Start time for the current API-symbol hover.
    pub api_doc_hover_started: Option<Instant>,

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:36 -05:00
    pub breakpoints: Vec<crate::scripting::debugger::DebugBreakpoint>,
    pub debug_snapshot: Option<crate::scripting::debugger::DebugSnapshot>,
    pub pending_debug_command: Option<crate::scripting::debugger::DebugCommand>,
    pub debug_selected_frame: usize,
    pub debug_watch_input: String,
    pub debug_watches: Vec<String>,
    pub debug_status: Option<String>,
}

impl ScriptEditor {
    pub fn new() -> Self {
        Self {
            open_documents: HashMap::new(),
            active_document: None,

            scripts_list: Vec::new(),
            playerscripts_list: Vec::new(),

            scripts_tree: None,
            playerscripts_tree: None,
            project_script_index: None,

            show_new_script_dialog: false,
            new_script_name: String::new(),
            new_script_root: ScriptRoot::Scripts,
            new_script_parent: None,
            created_script: None,

            show_rename_dialog: false,
            rename_target: None,
            rename_new_name: String::new(),

            search_query: String::new(),
            show_search: false,
            search_results: Vec::new(),

            closing_path: None,
            deleting_path: None,

            show_problems: false,
            // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:48 -05:00
            project_problems: Vec::new(),
            show_output: true,
            output_log: Vec::new(),
            output_revision: 0,

            cursor_request: None,
            editor_scroll_to_y: None,

            api_doc_hover_symbol: None,
            api_doc_hover_started: None,

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:36 -05:00
            breakpoints: Vec::new(),
            debug_snapshot: None,
            pending_debug_command: None,
            debug_selected_frame: 0,
            debug_watch_input: String::new(),
            debug_watches: Vec::new(),
            debug_status: None,
        }
    }

    pub(crate) fn push_output(&mut self, message: String) {
        self.output_log.push(message);
        self.output_revision = self.output_revision.wrapping_add(1);
    }

    pub(crate) fn clear_output_log(&mut self) {
        if self.output_log.is_empty() {
            return;
        }
        self.output_log.clear();
        self.output_revision = self.output_revision.wrapping_add(1);
    }

    pub(crate) fn trim_output_log(&mut self, max_lines: usize) {
        let count = self.output_log.len();
        if count <= max_lines {
            return;
        }
        self.output_log.drain(..count - max_lines);
        self.output_revision = self.output_revision.wrapping_add(1);
    }

    pub(crate) fn output_revision(&self) -> u64 {
        self.output_revision
    }

    pub fn reset_project_session(&mut self) {
        self.open_documents.clear();
        self.active_document = None;

        self.scripts_list.clear();
        self.playerscripts_list.clear();

        self.scripts_tree = None;
        self.playerscripts_tree = None;
        self.project_script_index = None;

        self.show_new_script_dialog = false;
        self.new_script_name.clear();
        self.new_script_root = ScriptRoot::Scripts;
        self.new_script_parent = None;
        self.created_script = None;

        self.show_rename_dialog = false;
        self.rename_target = None;
        self.rename_new_name.clear();

        self.search_query.clear();
        self.show_search = false;
        self.search_results.clear();
        // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:48 -05:00
        self.project_problems.clear();

        self.closing_path = None;
        self.deleting_path = None;

        self.cursor_request = None;
        self.editor_scroll_to_y = None;

        self.api_doc_hover_symbol = None;
        self.api_doc_hover_started = None;

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:36 -05:00
        self.breakpoints.clear();
        self.clear_debug_runtime_state();
        self.debug_watches.clear();
        self.debug_watch_input.clear();
    }

    pub fn script_tree(&self) -> Option<&ScriptTree> {
        self.scripts_tree.as_ref()
    }

    pub fn playerscripts_tree(&self) -> Option<&ScriptTree> {
        self.playerscripts_tree.as_ref()
    }

    pub fn project_script_index(&self) -> Option<&ProjectScriptIndex> {
        self.project_script_index.as_ref()
    }

    pub fn indexed_module_path(&self, logical_path: &str) -> Option<PathBuf> {
        let index = self.project_script_index.as_ref()?;
        let module = index.find_module(logical_path)?;

        Some(index.absolute_path(module))
    }

    pub fn indexed_modules(&self) -> Vec<IndexedModule> {
        self.project_script_index
            .as_ref()
            .map(ProjectScriptIndex::all_modules)
            .unwrap_or_default()
    }

    pub fn indexed_script_paths(&self) -> Vec<PathBuf> {
        if let Some(index) = self.project_script_index.as_ref() {
            return index
                .all_modules()
                .into_iter()
                .map(|module| index.absolute_path(&module))
                .collect();
        }

        let mut paths = self.scripts_list.clone();
        paths.extend(self.playerscripts_list.iter().cloned());
        paths.sort();
        paths
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:36 -05:00
    pub fn runtime_breakpoints(&self) -> Vec<crate::scripting::debugger::DebugBreakpoint> {
        self.breakpoints.clone()
    }

    pub fn clear_debug_runtime_state(&mut self) {
        self.debug_snapshot = None;
        self.pending_debug_command = None;
        self.debug_selected_frame = 0;
        self.debug_status = None;
    }

    pub fn logical_script_path(
        project_path: &std::path::Path,
        path: &std::path::Path,
    ) -> Option<String> {
        path.strip_prefix(project_path).ok().map(|relative| {
            crate::scripting::debugger::normalize_script_path(&relative.to_string_lossy())
        })
    }

    pub fn toggle_breakpoint(
        &mut self,
        project_path: &std::path::Path,
        path: &std::path::Path,
        source: &str,
        line: usize,
    ) -> Result<Option<crate::scripting::debugger::DebugBreakpoint>, String> {
        let logical = Self::logical_script_path(project_path, path)
            .ok_or_else(|| "Breakpoint target is outside the current project.".to_string())?;
        let resolved = crate::scripting::debugger::resolve_breakpoint(&logical, source, line)?;

        if let Some(index) = self.breakpoints.iter().position(|existing| {
            existing.script_path == resolved.script_path
                && existing.span_start == resolved.span_start
        }) {
            self.breakpoints.remove(index);
            self.debug_status = Some(format!(
                "Removed breakpoint at {}:{}",
                resolved.script_path, resolved.line
            ));
            return Ok(None);
        }

        self.breakpoints.push(resolved.clone());
        self.breakpoints.sort();
        self.breakpoints.dedup();
        self.debug_status = Some(format!(
            "Breakpoint set at {}:{}",
            resolved.script_path, resolved.line
        ));
        Ok(Some(resolved))
    }

    pub fn has_breakpoint(&self, logical_path: &str, line: usize) -> bool {
        let logical = crate::scripting::debugger::normalize_script_path(logical_path);
        self.breakpoints
            .iter()
            .any(|breakpoint| breakpoint.script_path == logical && breakpoint.line == line)
    }

    pub fn rename_breakpoint_path(&mut self, old_path: &str, new_path: &str) {
        let old_path = crate::scripting::debugger::normalize_script_path(old_path);
        let new_path = crate::scripting::debugger::normalize_script_path(new_path);
        for breakpoint in &mut self.breakpoints {
            if breakpoint.script_path.eq_ignore_ascii_case(&old_path) {
                breakpoint.script_path = new_path.clone();
            }
        }
        self.breakpoints.sort();
        self.breakpoints.dedup();
    }

    pub fn remove_breakpoints_for_path(&mut self, path: &str) {
        let path = crate::scripting::debugger::normalize_script_path(path);
        self.breakpoints
            .retain(|breakpoint| !breakpoint.script_path.eq_ignore_ascii_case(&path));
    }

    pub fn set_debug_snapshot(
        &mut self,
        mut snapshot: Option<crate::scripting::debugger::DebugSnapshot>,
        project_path: Option<&std::path::Path>,
    ) {
        if let (Some(snapshot), Some(project_path)) = (&mut snapshot, project_path) {
            for frame in &mut snapshot.frames {
                let absolute = project_path.join(&frame.script_path);
                let source = self
                    .open_documents
                    .get(&absolute)
                    .map(|document| document.source.clone())
                    .or_else(|| std::fs::read_to_string(&absolute).ok());
                if let (Some(source), Some(span)) = (source, frame.span) {
                    let (line, column) = crate::scripting::debugger::source_position(&source, span);
                    frame.line = Some(line);
                    frame.column = Some(column);
                }
            }
        }

        let was_paused = self.debug_snapshot.is_some();
        self.debug_snapshot = snapshot;
        if self.debug_snapshot.is_none() {
            self.debug_selected_frame = 0;
        } else if !was_paused {
            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:01 -05:00
            self.debug_selected_frame = 0;
            self.select_debug_frame(0, project_path);
        }
    }

    pub fn select_debug_frame(
        &mut self,
        frame_index: usize,
        project_path: Option<&std::path::Path>,
    ) {
        let Some(snapshot) = &self.debug_snapshot else {
            return;
        };
        let Some(frame) = snapshot.frames.get(frame_index) else {
            return;
        };
        let script_path = frame.script_path.clone();
        let span = frame.span;

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:50 -05:00
        self.debug_selected_frame = frame_index;
        if let Some(project_path) = project_path {
            let path = project_path.join(script_path);
            if path.is_file() {
                self.open_file(path.clone());
                if let Some(span) = span {
                    self.cursor_request = Some((path, span.start as usize));
                }
            }
        }
    }

    pub fn watched_variable(
        &self,
        name: &str,
    ) -> Option<&crate::scripting::debugger::DebugVariable> {
        let snapshot = self.debug_snapshot.as_ref()?;
        let frame = snapshot.frames.get(self.debug_selected_frame)?;
        frame
            .locals
            .iter()
            .rev()
            .find(|variable| variable.name == name)
            .or_else(|| {
                frame
                    .captured
                    .iter()
                    .rev()
                    .find(|variable| variable.name == name)
            })
            .or_else(|| {
                frame
                    .instance_fields
                    .iter()
                    .find(|variable| variable.name == name)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::script_editor_highlighting::{
        aeoscript_syntax, byte_offset_to_char_index, char_index_to_line, line_start_char_index,
    };
    use super::*;
    use std::fs;

    #[test]
    fn test_script_filenames_sorted() {
        let test_dir = PathBuf::from(format!("TestProject_Scripts_{}", std::process::id()));
        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&scripts_dir).unwrap();

        fs::write(scripts_dir.join("z.aeo"), "").unwrap();
        fs::write(scripts_dir.join("a.aeo"), "").unwrap();
        fs::write(scripts_dir.join("m.aeo"), "").unwrap();
        fs::write(scripts_dir.join("ignored.txt"), "").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));

        assert_eq!(editor.scripts_list.len(), 3);
        assert_eq!(editor.scripts_list[0].file_name().unwrap(), "a.aeo");
        assert_eq!(editor.scripts_list[1].file_name().unwrap(), "m.aeo");
        assert_eq!(editor.scripts_list[2].file_name().unwrap(), "z.aeo");

        assert!(editor.scripts_tree.is_some());

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_playerscripts_tree_is_loaded() {
        let test_dir = PathBuf::from(format!("TestProject_PlayerScripts_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon")).unwrap();
        fs::create_dir_all(test_dir.join("playerscripts/Controller")).unwrap();
        fs::create_dir_all(test_dir.join("playerscripts/Camera")).unwrap();

        fs::write(test_dir.join("scripts/Dungeon/main.aeo"), "").unwrap();

        fs::write(test_dir.join("playerscripts/Controller/TopDown.aeo"), "").unwrap();

        fs::write(test_dir.join("playerscripts/Camera/TopDown.aeo"), "").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));

        assert_eq!(editor.scripts_list.len(), 1);
        assert_eq!(editor.playerscripts_list.len(), 2);

        let tree = editor.playerscripts_tree.as_ref().unwrap();

        assert_eq!(
            tree.module_paths(),
            vec![
                PathBuf::from("Camera/TopDown.aeo"),
                PathBuf::from("Controller/TopDown.aeo"),
            ]
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_editor_uses_project_script_index_for_lookup() {
        let test_dir = PathBuf::from(format!("TestProject_IndexedLookup_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon")).unwrap();
        fs::create_dir_all(test_dir.join("playerscripts/Controller")).unwrap();

        let script_path = test_dir.join("scripts/Dungeon/main.aeo");
        let player_path = test_dir.join("playerscripts/Controller/topdown.aeo");

        fs::write(&script_path, "").unwrap();
        fs::write(&player_path, "").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(editor.project_script_index().is_some());

        assert_eq!(
            editor.indexed_module_path("scripts/Dungeon/main.aeo"),
            Some(script_path.clone())
        );

        assert_eq!(
            editor.indexed_module_path("playerscripts/Controller/topdown.aeo"),
            Some(player_path.clone())
        );

        assert_ne!(
            editor.indexed_module_path("scripts/Dungeon/main.aeo"),
            editor.indexed_module_path("playerscripts/Controller/topdown.aeo")
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_script_selection_opens_document() {
        let test_dir = PathBuf::from(format!("TestProject_Open_{}", std::process::id()));
        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&scripts_dir).unwrap();

        let path = scripts_dir.join("open_test.aeo");

        fs::write(&path, "script content").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(path.clone());

        assert_eq!(editor.active_document, Some(path.clone()));
        assert!(editor.open_documents.contains_key(&path));
        assert_eq!(
            editor.open_documents.get(&path).unwrap().source,
            "script content"
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_save_behavior() {
        let test_dir = PathBuf::from(format!("TestProject_Save_{}", std::process::id()));
        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&scripts_dir).unwrap();

        let path = scripts_dir.join("save_test.aeo");

        fs::write(&path, "initial").unwrap();

        let mut document = ScriptDocument::new(path.clone(), "initial".to_string());

        assert!(!document.dirty);

        document.source = "changed".to_string();
        document.dirty = true;

        document.save().expect("Save should succeed");

        assert!(!document.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "changed");

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_diagnostics_integration() {
        let source = "entity {";

        let document = ScriptDocument::new(PathBuf::from("error.aeo"), source.to_string());

        assert!(!document.diagnostics.is_empty());
        assert!(document.diagnostics.has_errors());

        let diagnostic = &document.diagnostics.as_slice()[0];

        assert_eq!(
            diagnostic.severity,
            crate::scripting::diagnostic::DiagnosticSeverity::Error
        );

        assert!(diagnostic.message.contains("Expected"));
    }

    #[test]
    fn test_new_script_starts_blank_in_both_roots() {
        let test_dir = PathBuf::from(format!("TestProject_Template_{}", std::process::id()));
        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&scripts_dir).unwrap();

        let mut editor = ScriptEditor::new();

        editor.create_new_script(&test_dir, "PlayerScript");

        let path = scripts_dir.join("PlayerScript.aeo");

        assert!(path.exists());

        let content = fs::read_to_string(&path).unwrap();

        assert!(content.is_empty());

        let document = ScriptDocument::new(path, content);

        assert!(document.diagnostics.is_empty());
        assert!(!editor.open_documents[&document.path].dirty);

        let (player_path, _) = editor
            .create_new_script_in_root(
                &test_dir,
                crate::project::ScriptRoot::PlayerScripts,
                "Controller/Player",
            )
            .unwrap();
        assert!(fs::read_to_string(&player_path).unwrap().is_empty());
        assert!(editor.open_documents[&player_path].diagnostics.is_empty());

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_search_results() {
        let test_dir = PathBuf::from(format!("TestProject_Search_{}", std::process::id()));
        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&scripts_dir).unwrap();

        fs::write(scripts_dir.join("one.aeo"), "hello world").unwrap();
        fs::write(scripts_dir.join("two.aeo"), "goodbye").unwrap();
        fs::write(scripts_dir.join("three.txt"), "hello hidden").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));

        editor.search_query = "hello".to_string();
        editor.perform_search();

        assert_eq!(editor.search_results.len(), 1);

        assert_eq!(
            editor.search_results[0].path.file_name().unwrap(),
            "one.aeo"
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_inspector_metadata_updates_after_edit() {
        let source = "entity test { fn on_spawn() {} fn on_destroy() {} }";

        let mut document = ScriptDocument::new(PathBuf::from("test.aeo"), source.to_string());

        let mut has_spawn = false;
        let mut has_destroy = false;

        if let Some(program) = &document.program {
            if let crate::scripting::ast::Declaration::Entity(entity) = &program.declarations[0] {
                for member in &entity.members {
                    if let crate::scripting::ast::EntityMember::Function(function) = member {
                        if function.name == "on_spawn" {
                            has_spawn = true;
                        }

                        if function.name == "on_destroy" {
                            has_destroy = true;
                        }
                    }
                }
            }
        }

        assert!(has_spawn);
        assert!(has_destroy);

        document.source = "entity test { fn on_spawn() {} }".to_string();

        document.reparse();

        let mut has_spawn = false;
        let mut has_destroy = false;

        if let Some(program) = &document.program {
            if let crate::scripting::ast::Declaration::Entity(entity) = &program.declarations[0] {
                for member in &entity.members {
                    if let crate::scripting::ast::EntityMember::Function(function) = member {
                        if function.name == "on_spawn" {
                            has_spawn = true;
                        }

                        if function.name == "on_destroy" {
                            has_destroy = true;
                        }
                    }
                }
            }
        }

        assert!(has_spawn);
        assert!(!has_destroy);
    }

    #[test]
    fn test_delete_dirty_save_failure_aborts_deletion() {
        let test_dir = PathBuf::from(format!("TestProject_DeleteFailure_{}", std::process::id()));
        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(&scripts_dir).unwrap();

        let path = scripts_dir.join("bug.aeo");

        fs::write(&path, "initial content").unwrap();

        let mut editor = ScriptEditor::new();

        let mut document = ScriptDocument::new(path.clone(), "initial content".to_string());

        document.source = "dirty content".to_string();
        document.dirty = true;

        editor.open_documents.insert(path.clone(), document);

        editor.deleting_path = Some(path.clone());
        editor.closing_path = Some(path.clone());

        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();

        let world = crate::world::World::new();

        editor.apply_dirty_response(&Some(test_dir.clone()), path.clone(), true, false, &world);

        assert!(path.exists());
        assert!(editor.open_documents.contains_key(&path));

        assert_eq!(editor.closing_path, Some(path.clone()));
        assert_eq!(editor.deleting_path, Some(path.clone()));

        assert!(
            editor
                .output_log
                .iter()
                .any(|line| line.contains("Save failed"))
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_editor_syntax_detects_aeoscript_core_words() {
        let syntax = aeoscript_syntax();

        assert!(syntax.is_keyword("entity"));
        assert!(syntax.is_keyword("fn"));
        assert!(syntax.is_keyword("const"));
        assert!(syntax.is_type("number"));
        assert!(syntax.is_special("spawn"));
    }

    // Worldkiln 0.8.7 | WO 0.8.7-28 | 2026-10-06 12:40 -05:00
    #[test]
    fn test_editor_source_line_identity_is_stable() {
        let source = "// DUNGEON ENEMY BRAIN\nconst value = 1\nprint(value)";

        assert_eq!(line_start_char_index(source, 1), Some(0));
        assert_eq!(char_index_to_line(source, 0), 1);
        assert_eq!(
            char_index_to_line(source, line_start_char_index(source, 3).unwrap()),
            3
        );
    }

    #[test]
    fn test_byte_offset_to_character_index() {
        let source = "éPlayer";

        assert_eq!(byte_offset_to_char_index(source, 0), 0);
        assert_eq!(byte_offset_to_char_index(source, 2), 1);
        assert_eq!(byte_offset_to_char_index(source, source.len()), 7);
    }

    #[test]
    fn test_duplicate_nested_module_filenames_remain_distinct_documents() {
        let test_dir = PathBuf::from(format!(
            "TestProject_DuplicateModules_{}",
            std::process::id()
        ));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        let first_dir = test_dir.join("scripts/One");
        let second_dir = test_dir.join("scripts/Two");

        fs::create_dir_all(&first_dir).unwrap();
        fs::create_dir_all(&second_dir).unwrap();

        let first = first_dir.join("combat.aeo");
        let second = second_dir.join("combat.aeo");

        fs::write(&first, "entity FirstCombat {}").unwrap();
        fs::write(&second, "entity SecondCombat {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(first.clone());
        editor.open_file(second.clone());

        assert_eq!(editor.open_documents.len(), 2);
        assert!(editor.open_documents.contains_key(&first));
        assert!(editor.open_documents.contains_key(&second));

        assert_eq!(
            editor.open_documents.get(&first).unwrap().source,
            "entity FirstCombat {}"
        );

        assert_eq!(
            editor.open_documents.get(&second).unwrap().source,
            "entity SecondCombat {}"
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_nested_script_tree_preserves_root_and_directory_paths() {
        let test_dir = PathBuf::from(format!("TestProject_TreePaths_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon/Systems")).unwrap();
        fs::create_dir_all(test_dir.join("playerscripts/Player")).unwrap();

        let script_path = test_dir.join("scripts/Dungeon/Systems/doors.aeo");
        let player_path = test_dir.join("playerscripts/Player/movement.aeo");

        fs::write(&script_path, "").unwrap();
        fs::write(&player_path, "").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));

        let scripts_tree = editor.scripts_tree.as_ref().unwrap();
        let playerscripts_tree = editor.playerscripts_tree.as_ref().unwrap();

        assert_eq!(
            scripts_tree.find_module("Dungeon/Systems/doors.aeo"),
            Some(PathBuf::from("Dungeon/Systems/doors.aeo"))
        );

        assert_eq!(
            playerscripts_tree.find_module("Player/movement.aeo"),
            Some(PathBuf::from("Player/movement.aeo"))
        );

        assert!(!scripts_tree.contains_path("Player/movement.aeo"));
        assert!(!playerscripts_tree.contains_path("Dungeon/Systems/doors.aeo"));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_refresh_rebuilds_structured_trees_without_losing_open_documents() {
        let test_dir = PathBuf::from(format!("TestProject_TreeRefresh_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        let nested = test_dir.join("scripts/Dungeon");
        fs::create_dir_all(&nested).unwrap();

        let existing = nested.join("existing.aeo");
        let added = nested.join("added.aeo");

        fs::write(&existing, "entity Existing {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(existing.clone());

        fs::write(&added, "entity Added {}").unwrap();

        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(editor.open_documents.contains_key(&existing));
        assert_eq!(editor.active_document, Some(existing.clone()));
        assert!(editor.scripts_list.contains(&existing));
        assert!(editor.scripts_list.contains(&added));

        let tree = editor.scripts_tree.as_ref().unwrap();

        assert_eq!(
            tree.find_module("Dungeon/existing.aeo"),
            Some(PathBuf::from("Dungeon/existing.aeo"))
        );

        assert_eq!(
            tree.find_module("Dungeon/added.aeo"),
            Some(PathBuf::from("Dungeon/added.aeo"))
        );

        fs::remove_dir_all(&test_dir).ok();
    }
    #[test]
    fn test_nested_modules_open_with_full_paths() {
        let test_dir = PathBuf::from(format!("TestProject_NestedOpen_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        let nested = test_dir.join("scripts/Dungeon/systems");
        fs::create_dir_all(&nested).unwrap();

        let path = nested.join("combat.aeo");

        fs::write(&path, "entity Combat {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(path.clone());

        assert_eq!(editor.active_document, Some(path.clone()));
        assert!(editor.open_documents.contains_key(&path));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_scripts_and_playerscripts_remain_separate_roots() {
        let test_dir = PathBuf::from(format!("TestProject_TwoRoots_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon")).unwrap();
        fs::create_dir_all(test_dir.join("playerscripts/Player")).unwrap();

        let scripts_path = test_dir.join("scripts/Dungeon/combat.aeo");
        let playerscripts_path = test_dir.join("playerscripts/Player/movement.aeo");

        fs::write(&scripts_path, "entity Combat {}").unwrap();
        fs::write(&playerscripts_path, "entity Movement {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));

        assert_eq!(editor.scripts_list, vec![scripts_path.clone()]);

        assert_eq!(editor.playerscripts_list, vec![playerscripts_path.clone()]);

        editor.open_file(scripts_path.clone());
        editor.open_file(playerscripts_path.clone());

        assert!(editor.open_documents.contains_key(&scripts_path));
        assert!(editor.open_documents.contains_key(&playerscripts_path));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_create_nested_script_preserves_relative_path() {
        let test_dir = PathBuf::from(format!("TestProject_CreateNested_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let mut editor = ScriptEditor::new();

        editor.create_new_script(&test_dir, "Dungeon/systems/combat");

        let path = test_dir.join("scripts/Dungeon/systems/combat.aeo");

        assert!(path.exists());
        assert!(editor.scripts_list.contains(&path));
        assert_eq!(editor.active_document, Some(path.clone()));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_rename_nested_script_stays_in_same_directory() {
        let test_dir = PathBuf::from(format!("TestProject_RenameNested_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        let nested = test_dir.join("scripts/Dungeon/systems");
        fs::create_dir_all(&nested).unwrap();

        let old_path = nested.join("combat.aeo");
        let new_path = nested.join("combat_new.aeo");

        fs::write(&old_path, "entity Combat {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(old_path.clone());

        editor.rename_script(&test_dir, old_path.clone(), "combat_new");

        assert!(!old_path.exists());
        assert!(new_path.exists());
        assert_eq!(editor.active_document, Some(new_path.clone()));
        assert!(editor.open_documents.contains_key(&new_path));
        assert!(editor.scripts_list.contains(&new_path));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_refresh_preserves_open_nested_documents() {
        let test_dir = PathBuf::from(format!("TestProject_RefreshNested_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        let nested = test_dir.join("scripts/Dungeon");
        fs::create_dir_all(&nested).unwrap();

        let path = nested.join("combat.aeo");

        fs::write(&path, "entity Combat {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(path.clone());

        editor.open_documents.get_mut(&path).unwrap().source =
            "entity Combat { fn update(dt: number) {} }".to_string();

        editor.open_documents.get_mut(&path).unwrap().dirty = true;

        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(editor.open_documents.contains_key(&path));
        assert!(editor.open_documents.get(&path).unwrap().dirty);
        assert_eq!(editor.active_document, Some(path.clone()));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_player_script_file_operations_preserve_root() {
        let test_dir = PathBuf::from(format!("TestProject_PlayerOps_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("playerscripts/Controller")).unwrap();

        let mut editor = ScriptEditor::new();

        editor.create_new_script_in_root(
            &test_dir,
            crate::project::ScriptRoot::PlayerScripts,
            "Controller/TopDown",
        );

        let created = test_dir.join("playerscripts/Controller/TopDown.aeo");

        assert!(created.exists());
        assert!(!test_dir.join("scripts/Controller/TopDown.aeo").exists());

        editor.rename_script(&test_dir, created.clone(), "Camera");

        let renamed = test_dir.join("playerscripts/Controller/Camera.aeo");

        assert!(!created.exists());
        assert!(renamed.exists());

        editor.open_file(renamed.clone());

        assert_eq!(editor.active_document, Some(renamed.clone()));

        // Search authored content rather than relying on a creation template.
        editor.open_documents.get_mut(&renamed).unwrap().source =
            "print(\"player search sentinel\")".to_string();
        editor.open_documents.get_mut(&renamed).unwrap().dirty = true;
        editor.save_active();
        editor.search_query = "player search sentinel".to_string();
        editor.perform_search();

        assert_eq!(editor.search_results.len(), 1);
        assert_eq!(editor.search_results[0].path, renamed);

        editor.delete_script(&Some(test_dir.clone()), renamed.clone());

        assert!(!renamed.exists());
        assert!(editor.open_documents.get(&renamed).is_none());

        if let Some(index) = editor.project_script_index() {
            assert!(!index.contains_path("playerscripts/Controller/Camera.aeo"));
        }

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_duplicate_script_names_remain_root_aware() {
        let test_dir = PathBuf::from(format!("TestProject_DuplicateRoots_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon")).unwrap();
        fs::create_dir_all(test_dir.join("playerscripts/Controller")).unwrap();

        let scripts_path = test_dir.join("scripts/Dungeon/TopDown.aeo");
        let playerscripts_path = test_dir.join("playerscripts/Controller/TopDown.aeo");

        fs::write(&scripts_path, "shared_root_test").unwrap();
        fs::write(&playerscripts_path, "shared_root_test").unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(
            editor
                .indexed_module_path("scripts/Dungeon/TopDown.aeo")
                .is_some()
        );

        assert!(
            editor
                .indexed_module_path("playerscripts/Controller/TopDown.aeo")
                .is_some()
        );

        editor.search_query = "shared_root_test".to_string();
        editor.perform_search();

        assert_eq!(editor.search_results.len(), 2);

        assert!(
            editor
                .search_results
                .iter()
                .any(|result| result.path == scripts_path)
        );

        assert!(
            editor
                .search_results
                .iter()
                .any(|result| result.path == playerscripts_path)
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_script_file_operations_reject_path_traversal() {
        let test_dir = PathBuf::from(format!(
            "TestProject_ScriptTraversal_{}",
            std::process::id()
        ));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let mut editor = ScriptEditor::new();

        editor.create_new_script_in_root(
            &test_dir,
            crate::project::ScriptRoot::Scripts,
            "../escape",
        );

        assert!(!test_dir.join("escape.aeo").exists());

        editor.create_new_script_in_root(
            &test_dir,
            crate::project::ScriptRoot::PlayerScripts,
            "Controller/../escape",
        );

        assert!(!test_dir.join("playerscripts/escape.aeo").exists());

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_refresh_rebuilds_index_without_dropping_open_documents() {
        let test_dir = PathBuf::from(format!("TestProject_RefreshIndex_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon")).unwrap();

        let path = test_dir.join("scripts/Dungeon/main.aeo");

        fs::write(&path, "original").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(path.clone());

        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(editor.open_documents.contains_key(&path));
        assert_eq!(editor.active_document, Some(path.clone()));

        assert_eq!(
            editor.indexed_module_path("scripts/Dungeon/main.aeo"),
            Some(path)
        );

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_rename_updates_script_binding_reference() {
        let test_dir = PathBuf::from(format!("TestProject_RenameBinding_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let old_path = test_dir.join("scripts/old.aeo");

        fs::write(&old_path, "entity Old {}").unwrap();

        let mut world = crate::world::World::new();
        world
            .script_bindings
            .push(crate::scripting::binding::ScriptBinding::new(
                42,
                "scripts/old.aeo",
            ));

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(
            editor.rename_script_with_bindings(&test_dir, old_path.clone(), "new", &mut world,)
        );

        assert_eq!(world.script_bindings[0].script_path, "scripts/new.aeo");

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_delete_blocks_bound_script() {
        let test_dir = PathBuf::from(format!("TestProject_DeleteBound_{}", std::process::id()));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let path = test_dir.join("scripts/bound.aeo");

        fs::write(&path, "entity Bound {}").unwrap();

        let mut world = crate::world::World::new();

        world
            .script_bindings
            .push(crate::scripting::binding::ScriptBinding::new(
                1,
                "scripts/bound.aeo",
            ));

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));

        assert!(!editor.delete_script_if_unbound(&Some(test_dir.clone()), path.clone(), &world,));

        assert!(path.exists());

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_index_lookup_keeps_duplicate_module_names_distinct() {
        let test_dir = PathBuf::from(format!(
            "TestProject_DuplicateModuleLookup_{}",
            std::process::id()
        ));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts/Dungeon")).unwrap();

        fs::create_dir_all(test_dir.join("playerscripts/Controller")).unwrap();

        fs::write(
            test_dir.join("scripts/Dungeon/player.aeo"),
            "entity DungeonPlayer {}",
        )
        .unwrap();

        fs::write(
            test_dir.join("playerscripts/Controller/player.aeo"),
            "entity PlayerController {}",
        )
        .unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));

        let index = editor
            .project_script_index()
            .expect("Project index should exist");

        let matches = index.find_modules_named("player");

        assert_eq!(matches.len(), 2);

        assert!(matches.iter().any(|module| {
            module.logical_path() == PathBuf::from("scripts/Dungeon/player.aeo")
        }));

        assert!(matches.iter().any(|module| {
            module.logical_path() == PathBuf::from("playerscripts/Controller/player.aeo")
        }));

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_duplicate_script_creates_distinct_source_and_dirty_state() {
        let test_dir = PathBuf::from(format!(
            "TestProject_DuplicateIndependent_{}",
            std::process::id()
        ));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let original_path = test_dir.join("scripts/test.aeo");

        fs::write(&original_path, "entity Test {\n    fn on_spawn() {}\n}\n").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(original_path.clone());

        // Simulate an unsaved edit in the original.
        {
            let original = editor.open_documents.get_mut(&original_path).unwrap();

            original.source.push_str("// unsaved original edit\n");
            original.dirty = true;
        }

        let (copy_path, logical_path) = editor
            .duplicate_script(&test_dir, original_path.clone())
            .expect("duplicate should succeed");

        assert_eq!(logical_path, "scripts/test_copy.aeo");
        assert_ne!(copy_path, original_path);

        assert!(original_path.exists());
        assert!(copy_path.exists());

        // Duplicate receives the current visible source.
        assert_eq!(
            fs::read_to_string(&copy_path).unwrap(),
            editor.open_documents[&original_path].source
        );

        editor.open_file(copy_path.clone());

        {
            let copy = editor.open_documents.get_mut(&copy_path).unwrap();

            copy.source.push_str("// copy-only edit\n");
            copy.dirty = true;
        }

        // Editing the copy must not modify the original document.
        assert!(
            !editor.open_documents[&original_path]
                .source
                .contains("copy-only edit")
        );

        assert!(editor.open_documents[&original_path].dirty);
        assert!(editor.open_documents[&copy_path].dirty);

        // Save only the copy.
        editor.active_document = Some(copy_path.clone());
        editor.save_active();

        // Saving the copy must not clear the original's dirty state.
        assert!(editor.open_documents[&original_path].dirty);
        assert!(!editor.open_documents[&copy_path].dirty);

        fs::remove_dir_all(test_dir).ok();
    }
    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:25 -05:00
    #[test]
    fn debugger_breakpoint_line_mapping_handles_long_documents_without_source_mutation() {
        let test_dir = PathBuf::from(format!(
            "TestProject_DebuggerLongScript_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(test_dir.join("scripts")).unwrap();
        let path = test_dir.join("scripts/long.aeo");

        let mut source = String::from("fn run() {\n");
        for index in 0..250 {
            source.push_str(&format!("    const value_{index} = {index}\n"));
        }
        source.push_str("}\n");
        fs::write(&path, &source).unwrap();

        let mut editor = ScriptEditor::new();
        editor.open_file(path.clone());
        let before = editor.open_documents[&path].source.clone();

        let added = editor
            .toggle_breakpoint(&test_dir, &path, &before, 201)
            .unwrap()
            .unwrap();

        assert_eq!(added.script_path, "scripts/long.aeo");
        assert_eq!(added.line, 201);
        assert!(editor.has_breakpoint("scripts/long.aeo", 201));
        assert_eq!(editor.open_documents[&path].source, before);

        editor.close_document(&path);
        editor.open_file(path.clone());
        assert!(editor.has_breakpoint("scripts/long.aeo", 201));

        let _ = fs::remove_dir_all(test_dir);
    }

    #[test]
    fn debugger_breakpoints_follow_rename_and_drop_on_delete() {
        let test_dir = PathBuf::from(format!(
            "TestProject_DebuggerRenameDelete_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(test_dir.join("scripts")).unwrap();
        let old = test_dir.join("scripts/old.aeo");
        let source = "fn run() {\n    debug.log(1)\n}\n";
        fs::write(&old, source).unwrap();

        let mut editor = ScriptEditor::new();
        editor.refresh_scripts(&Some(test_dir.clone()));
        editor
            .toggle_breakpoint(&test_dir, &old, source, 2)
            .unwrap();
        let (old_logical, new_logical) =
            editor.rename_script(&test_dir, old.clone(), "new").unwrap();
        assert_eq!(old_logical, "scripts/old.aeo");
        assert_eq!(new_logical, "scripts/new.aeo");
        assert!(editor.has_breakpoint("scripts/new.aeo", 2));

        let new_path = test_dir.join("scripts/new.aeo");
        assert!(editor.delete_script(&Some(test_dir.clone()), new_path));
        assert!(!editor.has_breakpoint("scripts/new.aeo", 2));

        let _ = fs::remove_dir_all(test_dir);
    }

    #[test]
    fn test_reset_project_session_clears_project_owned_script_state() {
        let test_dir = PathBuf::from(format!(
            "TestProject_ResetScriptSession_{}",
            std::process::id()
        ));

        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }

        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let path = test_dir.join("scripts/project_a.aeo");
        fs::write(&path, "entity ProjectA {}").unwrap();

        let mut editor = ScriptEditor::new();

        editor.refresh_scripts(&Some(test_dir.clone()));
        editor.open_file(path.clone());

        editor.rename_target = Some(path.clone());
        editor.rename_new_name = "renamed".to_string();

        editor.deleting_path = Some(path.clone());
        editor.closing_path = Some(path.clone());

        editor.cursor_request = Some((path.clone(), 4));

        editor
            .breakpoints
            .push(crate::scripting::debugger::DebugBreakpoint {
                script_path: "scripts/project_a.aeo".to_string(),
                line: 1,
                span_start: 0,
            });
        editor.debug_watches.push("value".to_string());

        editor.search_query = "ProjectA".to_string();
        editor.perform_search();

        assert!(editor.active_document.is_some());
        assert!(!editor.open_documents.is_empty());
        assert!(editor.project_script_index.is_some());

        editor.reset_project_session();

        assert!(editor.open_documents.is_empty());
        assert!(editor.active_document.is_none());

        assert!(editor.scripts_list.is_empty());
        assert!(editor.playerscripts_list.is_empty());

        assert!(editor.scripts_tree.is_none());
        assert!(editor.playerscripts_tree.is_none());
        assert!(editor.project_script_index.is_none());

        assert!(editor.created_script.is_none());
        assert!(editor.new_script_parent.is_none());

        assert!(editor.rename_target.is_none());
        assert!(editor.rename_new_name.is_empty());

        assert!(editor.search_query.is_empty());
        assert!(editor.search_results.is_empty());
        assert!(!editor.show_search);

        assert!(editor.closing_path.is_none());
        assert!(editor.deleting_path.is_none());
        assert!(editor.cursor_request.is_none());

        assert!(editor.breakpoints.is_empty());
        assert!(editor.debug_watches.is_empty());
        assert!(editor.debug_snapshot.is_none());

        fs::remove_dir_all(test_dir).ok();
    }
}
