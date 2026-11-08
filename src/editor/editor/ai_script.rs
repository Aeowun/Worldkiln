// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::editor::script_editor::{ScriptDocument, ScriptEditor};
use crate::project::{ProjectHierarchy, ProjectNodeId, ProjectNodeKind, ScriptRoot};
use crate::scripting::{lexer::Lexer, parser::Parser};
use crate::world::{World, WorldCoord};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
pub(super) const CANONICAL_LANGUAGE_DOC: &str = include_str!("aeoscript_reference.md");
pub(super) const LANGUAGE_CONTRACT_PATH: &str = "src/editor/editor/aeoscript_reference.md";

pub(super) fn language_contract() -> Result<&'static str, String> {
    const BEGIN: &str = "<!-- AI-LANGUAGE-CONTRACT:BEGIN -->";
    const END: &str = "<!-- AI-LANGUAGE-CONTRACT:END -->";
    if CANONICAL_LANGUAGE_DOC.matches(BEGIN).count() != 1
        || CANONICAL_LANGUAGE_DOC.matches(END).count() != 1
    {
        return Err("Canonical AeoScript syntax contract is missing or ambiguous.".into());
    }
    let contract = CANONICAL_LANGUAGE_DOC
        .split_once(BEGIN)
        .unwrap()
        .1
        .split_once(END)
        .ok_or("Canonical AeoScript syntax contract is incomplete.")?
        .0
        .trim();
    if contract.is_empty() || contract.len() > 4_000 {
        return Err("Canonical AeoScript syntax contract must contain 1–4000 UTF-8 bytes.".into());
    }
    Ok(contract)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptKind {
    #[default]
    Global,
    Attached,
    Cell,
}
impl ScriptKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global (normal script)",
            Self::Attached => "Actor Module",
            Self::Cell => "Cell Entity binding",
        }
    }
    pub fn instructions(self) -> &'static str {
        match self {
            Self::Global => {
                "TARGET: a NORMAL independent project script, not attached to any object. It is executed top-down when Play loads. For a welcome message the complete correct program is debug.log(\"Welcome to Worldkiln!\"). Do NOT put this in an on start() handler, fn on_ready(), entity declaration, or uncalled function."
            }
            Self::Attached => {
                "TARGET: ACTOR Module. The selected Actor is the intended parent in the Project Manager hierarchy. Use ordinary top-level Module code, e.g. const actor = self.parent followed by actor.on_touch = fn(other) { ... }. DO NOT use an entity {...} block or fn on_ready() for Actor Modules. Actual attachment is a Project Manager hierarchy action, not generated syntax; the AI can only claim parenting if the editor operation succeeds."
            }
            Self::Cell => {
                "TARGET: legacy CELL-bound script. This is distinct from Actor Modules: Cell bindings use persistent Cell ID + .aeo path and matching entity NAME with lifecycle methods like on_ready/update. self.parent is NOT a Cell handle in the current engine. Creating a script alone does not bind it; unless editor Cell binding is actually executed, explain that Cell Inspector attachment remains necessary."
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SelectionContext {
    pub actor: Option<(u64, ProjectNodeId)>,
    pub cell_selected: bool,
}
pub fn selection_context(
    hierarchy: &ProjectHierarchy,
    selected_node: Option<ProjectNodeId>,
    selected_actor: Option<u64>,
    cell_selected: bool,
) -> SelectionContext {
    let from_node = selected_node.and_then(|id| {
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = hierarchy.node(node_id)?;
            match node.kind {
                ProjectNodeKind::Actor { actor_id } => return Some((actor_id, node_id)),
                ProjectNodeKind::Script { .. } => {
                    let actor_id = hierarchy.attached_actor_id(node_id)?;
                    return hierarchy
                        .find_actor_node(actor_id)
                        .map(|actor_node| (actor_id, actor_node));
                }
                ProjectNodeKind::Folder { .. } => current = node.parent_id,
                ProjectNodeKind::Cell { .. } => return None,
            }
        }
        None
    });
    let actor = selected_actor
        .and_then(|actor_id| {
            hierarchy
                .find_actor_node(actor_id)
                .map(|node| (actor_id, node))
        })
        .or(from_node);
    SelectionContext {
        actor,
        cell_selected,
    }
}
pub fn selection_summary(
    world: &World,
    hierarchy: &ProjectHierarchy,
    scripts: &ScriptEditor,
    project_root: Option<&Path>,
    selection: SelectionContext,
    selected_coord: Option<WorldCoord>,
    selected_node: Option<ProjectNodeId>,
) -> String {
    fn short(text: &str) -> String {
        text.chars().take(160).collect()
    }
    fn location(hierarchy: &ProjectHierarchy, node: ProjectNodeId) -> String {
        let mut folders = Vec::new();
        let mut current = hierarchy.node(node).and_then(|entry| entry.parent_id);
        for _ in 0..12 {
            let Some(entry) = current.and_then(|id| hierarchy.node(id)) else {
                break;
            };
            if let ProjectNodeKind::Folder { name } = &entry.kind {
                folders.push(short(name));
            }
            current = entry.parent_id;
        }
        folders.reverse();
        if folders.is_empty() {
            "Project".into()
        } else {
            folders.join("/")
        }
    }
    let mut snapshot = json!({"scope":"current project and live editor selection",
        "note":"Metadata only. Read script source through tools when needed. Null active_script means no open active project script; selected_script is separate and is never an open document by implication.",
        "engine_version":env!("CARGO_PKG_VERSION"),
        "project_root":project_root.map(|path| path.to_string_lossy().to_string()),
        "context_origin":"live Worldkiln editor snapshot; authored state, not runtime proof",
        "active_script":Value::Null,
        "selected_script":Value::Null});
    if let Some((id, actor_node)) = selection.actor {
        if let Some(actor) = world.actors.get(&id) {
            let modules: Vec<Value> = world
                .actor_modules(id)
                .iter()
                .take(12)
                .map(|module| {
                    json!({"path":short(&module.script_path),
                    "module":short(&module.module_name),"enabled":module.enabled})
                })
                .collect();
            snapshot["actor"] = json!({
                "id":id,"name":short(&actor.identity),"kind":"Actor",
                "location":location(hierarchy, actor_node),
                "world_position":actor.position.to_array(),
                "rotation":actor.rotation.to_array(),
                "scale":actor.scale.to_array(),
                "package":actor.package.as_deref().map(short),
                "visible":actor.visible,"enabled":actor.enabled,
                "solid":actor.physics.solid,"anchored":actor.physics.anchored,
                "attached_scripts":modules,
            });
        }
    }
    let cell_coord = selected_coord.or_else(|| {
        let entry = hierarchy.node(selected_node?)?;
        if let ProjectNodeKind::Cell { cell_id, .. } = entry.kind {
            world.id_to_coord.get(&cell_id).copied()
        } else {
            None
        }
    });
    if let Some(coord) = cell_coord {
        if let Some(cell) = world.cells.get(&coord) {
            let node = hierarchy.find_cell_node(cell.id);
            let label = node.and_then(|id| hierarchy.node(id)).and_then(|entry| {
                if let ProjectNodeKind::Cell { label, .. } = &entry.kind {
                    label.as_deref().map(short)
                } else {
                    None
                }
            });
            snapshot["cell"] = json!({
                "id":cell.id,"name":label.or_else(|| cell.entity_identity.as_deref().map(short)),
                "kind":format!("{:?}",cell.cell_type),
                "location":node.map(|id| location(hierarchy,id)).unwrap_or_else(||"World".into()),
                "grid_position":[coord.x,coord.y,coord.z],
                "world_position":world.sources.get(&cell.id)
                    .map(|source| source.position.to_array())
                    .unwrap_or([coord.x as f32,coord.y as f32,coord.z as f32]),
                "visible":cell.visible,"solid":cell.solid,"anchored":cell.anchored,
                "color":cell.color_rgb.to_array(),
                "texture":short(&cell.texture),
            });
        }
    }
    if snapshot.get("cell").is_none() {
        if let Some(node_id) = selected_node {
            if let Some(ProjectNodeKind::Cell { cell_id, label, .. }) =
                hierarchy.node(node_id).map(|node| &node.kind)
            {
                if let Some(source) = world.sources.get(cell_id) {
                    let cell = &source.cell;
                    snapshot["cell"] = json!({
                        "id":cell.id,"name":label.as_deref().map(short)
                            .or_else(|| cell.entity_identity.as_deref().map(short)),
                        "kind":format!("{:?}",cell.cell_type),
                        "location":location(hierarchy,node_id),
                        "grid_position":Value::Null,
                        "world_position":source.position.to_array(),
                        "visible":cell.visible,"solid":cell.solid,"anchored":cell.anchored,
                        "color":cell.color_rgb.to_array(),
                    });
                }
            }
        }
    }
    if let Some(node_id) = selected_node {
        if let Some(node) = hierarchy.node(node_id) {
            if let ProjectNodeKind::Script { script_path } = &node.kind {
                snapshot["selected_script"] = json!({
                    "path":short(script_path),
                    "location":location(hierarchy,node_id),
                    "attached_actor_id":hierarchy.attached_actor_id(node_id),
                });
            }
        }
    }
    if let (Some(root), Some(active)) = (project_root, scripts.active_document.as_ref()) {
        if let Ok(relative) = active.strip_prefix(root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            if is_project_script(root, active) && scripts.open_documents.contains_key(active) {
                let attachment = hierarchy
                    .find_script_nodes(&relative)
                    .iter()
                    .find_map(|id| hierarchy.attached_actor_id(*id));
                snapshot["active_script"] = json!({
                    "path":short(&relative),
                    "dirty":scripts.open_documents.get(active).is_some_and(|doc|doc.dirty),
                    "attached_actor_id":attachment,
                    "source_available_via":"read_project_file or read_open_scripts",
                });
            }
        }
    }
    snapshot.to_string()
}

pub fn inferred_kind(request: &str, context: SelectionContext) -> ScriptKind {
    let text = request.to_ascii_lowercase();
    if [
        "main menu",
        "pause menu",
        "menu screen",
        "user interface",
        "hud",
        "game-wide",
        "global script",
        "startup",
        "save system",
    ]
    .iter()
    .any(|t| text.contains(t))
    {
        return ScriptKind::Global;
    }
    if context.actor.is_some() {
        return ScriptKind::Attached;
    }
    if context.cell_selected
        && [
            "this cell",
            "selected cell",
            "the cell",
            "cell script",
            "cell behavior",
        ]
        .iter()
        .any(|t| text.contains(t))
    {
        return ScriptKind::Cell;
    }
    ScriptKind::Global
}
pub fn actor_target_still_valid(
    hierarchy: &ProjectHierarchy,
    actor_id: u64,
    node_id: ProjectNodeId,
) -> bool {
    matches!(hierarchy.node(node_id).map(|n|&n.kind),
        Some(ProjectNodeKind::Actor{actor_id:current}) if *current==actor_id)
}

const MAX_SCRIPT_BYTES: usize = 128 * 1024;
pub fn extract_aeoscript(reply: &str) -> Result<String, String> {
    let mut blocks = Vec::new();
    let mut active: Option<(String, String)> = None;
    for line in reply.lines() {
        let stripped = line.trim();
        if let Some(fence) = stripped.strip_prefix("```") {
            if let Some((lang, body)) = active.take() {
                if !body.trim().is_empty() {
                    blocks.push((lang, body));
                }
            } else {
                active = Some((fence.trim().to_ascii_lowercase(), String::new()));
            }
        } else if let Some((_, body)) = active.as_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    let mut aeoscripts = blocks
        .into_iter()
        .filter(|(lang, _)| matches!(lang.as_str(), "aeo" | "aeoscript" | "worldkiln-aeoscript"))
        .map(|(_, text)| text)
        .collect::<Vec<_>>();
    if aeoscripts.len() != 1 {
        return Err("Expected exactly one completed AeoScript code block (```aeoscript).".into());
    }
    let code = aeoscripts.remove(0);
    if code.len() > MAX_SCRIPT_BYTES {
        return Err("Script exceeds 128 KiB.".into());
    }
    Ok(code)
}
pub fn syntax_check(source: &str) -> Result<(), String> {
    if source.trim().is_empty() {
        return Err("Script is empty.".into());
    }
    if source.len() > MAX_SCRIPT_BYTES {
        return Err("Script exceeds 128 KiB.".into());
    }
    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|e| format!("Lexer error at {}:{}: {}", e.line, e.column, e.message))?;
    Parser::new(tokens)
        .parse()
        .map_err(|e| format!("Syntax error at {}:{}: {}", e.line, e.column, e.message))?;
    Ok(())
}
pub fn validate_for_context(source: &str, kind: ScriptKind) -> Result<(), String> {
    syntax_check(source)?;
    use crate::scripting::ast::Declaration;
    if kind == ScriptKind::Cell {
        let ast = Parser::new(Lexer::new(source).tokenize().map_err(|e| e.message)?)
            .parse()
            .map_err(|e| e.message)?;
        if !ast
            .declarations
            .iter()
            .any(|d| matches!(d, Declaration::Entity(_)))
        {
            return Err("A legacy Cell-bound script must define an entity with the Cell's identity; an ordinary Actor Module or self.parent cannot bind to a Cell.".into());
        }
        return Ok(());
    }
    let tokens = Lexer::new(source).tokenize().map_err(|e| e.message)?;
    let program = Parser::new(tokens).parse().map_err(|e| e.message)?;
    if kind == ScriptKind::Attached {
        if program
            .declarations
            .iter()
            .any(|d| matches!(d, Declaration::Entity(_)))
        {
            return Err("Actor-attached Modules do NOT invoke entity {...} lifecycle. Use self.parent and Actor handlers at the top level.".into());
        }
        if program.statements.is_empty() && program.declarations.iter().any(|d|matches!(d,Declaration::Function(f) if matches!(f.name.as_str(),"on_ready"|"on_spawn"|"start"|"on_start"))){
            return Err("Uncalled startup/lifecycle functions do not run in Actor Modules. Use top-level code with const actor = self.parent.".into());
        }
    }
    for declaration in &program.declarations {
        match declaration {
            Declaration::Event(event)
                if matches!(
                    event.name.as_str(),
                    "start" | "on_start" | "ready" | "on_ready"
                ) =>
            {
                return Err(format!(
                    "'on {}(...)' is an event listener, not a global Play startup hook. \
                    Normal scripts execute top-level statements directly, e.g. \
                    debug.log(\"Welcome to Worldkiln!\").",
                    event.name,
                ));
            }
            Declaration::Function(function)
                if program.statements.is_empty()
                    && matches!(function.name.as_str(), "start" | "on_start" | "on_ready") =>
            {
                return Err(format!(
                    "Declaring fn {}() alone does not run it when Play starts. \
                    Put startup statements at the top level or explicitly call your function.",
                    function.name
                ));
            }
            _ => {}
        }
    }
    if program.statements.is_empty()
        && !program.declarations.is_empty()
        && program
            .declarations
            .iter()
            .all(|decl| matches!(decl, Declaration::Entity(_)))
    {
        return Err("An entity-only declaration requires a compatible Cell binding (legacy Entity lifecycle). For ordinary scripts use top-level statements; for Actor modules use self.parent and Actor handlers.".into());
    }
    Ok(())
}

pub fn is_project_script(root: &Path, path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("aeo") {
        return false;
    }
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let mut parts = relative.components();
    let Some(top) = parts.next() else {
        return false;
    };
    matches!(top.as_os_str().to_str(), Some("scripts" | "playerscripts")) && parts.next().is_some()
}
pub fn create_script(
    editor: &mut ScriptEditor,
    project_root: &Path,
    root: ScriptRoot,
    name: &str,
    source: &str,
) -> Result<PathBuf, String> {
    syntax_check(source)?;
    let name = name.trim();
    if name.is_empty() || name.len() > 120 {
        return Err("Enter a short script name.".into());
    }
    if name.starts_with('.') || name.contains("..") {
        return Err("Invalid script path component.".into());
    }
    let Some((path, logical)) =
        editor.create_new_script_in_root(&project_root.to_path_buf(), root, name)
    else {
        return Err("Could not create script. Name may be invalid or already in use.".into());
    };
    editor.created_script = Some((None, logical, path.clone()));
    let Some(doc) = editor.open_documents.get_mut(&path) else {
        return Err("Created script could not be opened in editor.".into());
    };
    doc.source = source.to_owned();
    doc.dirty = true;
    doc.reparse();
    editor.active_document = Some(path.clone());
    editor.push_output(format!("AI draft staged in {} (UNSAVED).", path.display()));
    Ok(path)
}
pub fn replace_active(
    editor: &mut ScriptEditor,
    root: &Path,
    expected: &Path,
    original: &str,
    source: &str,
) -> Result<(), String> {
    syntax_check(source)?;
    if !is_project_script(root, expected) {
        return Err("Not a project script.".into());
    }
    if editor.active_document.as_deref() != Some(expected) {
        return Err("The active script changed.".into());
    }
    let Some(doc) = editor.open_documents.get_mut(expected) else {
        return Err("Script no longer open.".into());
    };
    if doc.source != original {
        return Err(
            "The original script was edited since the AI draft; no changes applied.".into(),
        );
    }
    doc.source = source.to_owned();
    doc.dirty = true;
    doc.reparse();
    editor.push_output(format!(
        "AI replacement staged in {} (UNSAVED).",
        expected.display()
    ));
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_v2_active_context_never_guesses() {
        let root = PathBuf::from("C:/wk-assistant-context");
        let world = World::new();
        let mut hierarchy = ProjectHierarchy::new();
        let selected = hierarchy
            .create_script_node(None, "scripts/selected.aeo")
            .unwrap();
        let mut editor = ScriptEditor::new();
        let path = root.join("scripts/retained.aeo");
        editor.scripts_list.push(root.join("scripts/first.aeo"));
        for state in 0..4 {
            editor.active_document = if state == 0 { None } else { Some(path.clone()) };
            if state >= 2 {
                editor.open_documents.insert(
                    path.clone(),
                    ScriptDocument::new(path.clone(), "debug.log(1)".into()),
                );
            }
            let project = if state == 3 {
                Path::new("C:/another-project")
            } else {
                &root
            };
            let snapshot: Value = serde_json::from_str(&selection_summary(
                &world,
                &hierarchy,
                &editor,
                Some(project),
                SelectionContext::default(),
                None,
                Some(selected),
            ))
            .unwrap();
            if state == 2 {
                assert_eq!(snapshot["active_script"]["path"], "scripts/retained.aeo");
            } else {
                assert_eq!(
                    snapshot.get("active_script"),
                    Some(&Value::Null),
                    "state {state}"
                );
            }
            assert_eq!(snapshot["selected_script"]["path"], "scripts/selected.aeo");
        }
    }
    #[test]
    fn selected_actor_cell_and_active_script_context_are_project_scoped() {
        let root = std::env::temp_dir().join("wk_ai_context_metadata");
        let mut world = World::new();
        let mut hierarchy = ProjectHierarchy::new();
        let actor_node = hierarchy.create_actor_node(None, 73).unwrap();
        let mut guard = crate::world::Actor::new(73, "Guard", glam::Vec3::new(10.0, 2.0, -4.0));
        guard.package = Some("Pirate_NPC".into());
        world.actors.insert(73, guard);
        let script_node = hierarchy
            .create_script_node(Some(actor_node), "scripts/ai_behavior.aeo")
            .unwrap();
        let mut editor = ScriptEditor::new();
        let path = root.join("scripts").join("ai_behavior.aeo");
        let mut doc = ScriptDocument::new(path.clone(), "secret script contents".into());
        doc.dirty = true;
        editor.active_document = Some(path.clone());
        editor.open_documents.insert(path, doc);
        let actor_text = selection_summary(
            &world,
            &hierarchy,
            &editor,
            Some(&root),
            SelectionContext {
                actor: Some((73, actor_node)),
                cell_selected: false,
            },
            None,
            Some(script_node),
        );
        let actor: Value = serde_json::from_str(&actor_text).unwrap();
        assert_eq!(actor["actor"]["name"], "Guard");
        assert_eq!(actor["actor"]["world_position"], json!([10.0, 2.0, -4.0]));
        assert_eq!(actor["actor"]["package"], "Pirate_NPC");
        assert_eq!(actor["active_script"]["path"], "scripts/ai_behavior.aeo");
        assert_eq!(actor["active_script"]["dirty"], true);
        assert_eq!(actor["active_script"]["attached_actor_id"], 73);
        assert_eq!(actor["selected_script"]["attached_actor_id"], 73);
        assert!(!actor_text.contains("secret script contents"));
        let coord = WorldCoord::new(4, 3, -1);
        let mut cell = crate::world::Cell::new_light();
        cell.id = 81;
        cell.entity_identity = Some("Beacon".into());
        world.cells.insert(coord, cell);
        world.id_to_coord.insert(81, coord);
        let cell_node = hierarchy.create_cell_node(None, 81).unwrap();
        hierarchy.rename_cell_node(cell_node, "Lantern").unwrap();
        let cell_text = selection_summary(
            &world,
            &hierarchy,
            &editor,
            Some(&root),
            SelectionContext::default(),
            Some(coord),
            Some(cell_node),
        );
        let cell: Value = serde_json::from_str(&cell_text).unwrap();
        assert_eq!(cell["cell"]["id"], 81);
        assert_eq!(cell["cell"]["name"], "Lantern");
        assert_eq!(cell["cell"]["kind"], "Light");
        assert_eq!(cell["cell"]["grid_position"], json!([4, 3, -1]));
        assert!(
            cell.get("actor").is_none(),
            "Other selections must not leak"
        );
        let source_node = hierarchy.create_cell_node(None, 82).unwrap();
        world.sources.insert(
            82,
            crate::world::AuthoredSource::new(
                82,
                crate::world::CellType::Light,
                glam::Vec3::new(3.25, 8.0, -1.5),
            ),
        );
        let source_text = selection_summary(
            &world,
            &hierarchy,
            &editor,
            Some(&root),
            SelectionContext::default(),
            None,
            Some(source_node),
        );
        let source: Value = serde_json::from_str(&source_text).unwrap();
        assert_eq!(source["cell"]["world_position"], json!([3.25, 8.0, -1.5]));
        assert!(source["cell"]["grid_position"].is_null());
    }
    #[test]
    fn extracts_only_aeoscript() {
        let source = "Use:\n```aeoscript\nfn test() {\n print(\"hi\")\n}\n```\nThis logs.";
        let found = extract_aeoscript(source).unwrap();
        assert!(syntax_check(&found).is_ok());
        assert!(extract_aeoscript("```js\nalert(1)\n```").is_err());
        assert!(extract_aeoscript("```aeoscript\nprint(1)").is_err());
    }
    #[test]
    fn syntax_validation() {
        assert!(syntax_check("fn start() {\n print(\"hello\")\n}").is_ok());
        assert!(syntax_check("fn start( {\n").is_err());
    }
    #[test]
    fn create_is_unsaved_and_collision_safe() {
        let root = std::env::temp_dir().join(format!("wkiln_ai_create_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut editor = ScriptEditor::new();
        let path = create_script(
            &mut editor,
            &root,
            ScriptRoot::Scripts,
            "AI_Test",
            "fn test() {\n print(\"hi\")\n}",
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert!(editor.open_documents[&path].dirty);
        assert!(
            create_script(
                &mut editor,
                &root,
                ScriptRoot::Scripts,
                "AI_Test",
                "print(\"hi\")"
            )
            .is_err()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn replace_checks_staleness_and_no_disk_write() {
        let root = std::env::temp_dir().join(format!("wkiln_ai_replace_{}", std::process::id()));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        let path = root.join("scripts/target.aeo");
        std::fs::write(&path, "print(\"before\")").unwrap();
        let mut editor = ScriptEditor::new();
        editor.open_file(path.clone());
        assert!(replace_active(&mut editor, &root, &path, "stale", "print(\"after\")").is_err());
        replace_active(
            &mut editor,
            &root,
            &path,
            "print(\"before\")",
            "print(\"after\")",
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "print(\"before\")");
        assert!(editor.open_documents[&path].dirty);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn global_startup_is_not_an_event_listener() {
        assert!(
            validate_for_context("debug.log(\"Welcome to Worldkiln!\")", ScriptKind::Global)
                .is_ok()
        );
        assert!(syntax_check("on start() {\n    debug.log(\"Hello\")\n}").is_ok());
        assert!(
            validate_for_context(
                "on start() {\n    debug.log(\"Hello\")\n}",
                ScriptKind::Global
            )
            .is_err()
        );
        assert!(
            validate_for_context(
                "on start() {\n    debug.log(\"Hello\")\n}",
                ScriptKind::Attached
            )
            .is_err()
        );
    }
    #[test]
    fn bare_ready_function_does_not_run_globally() {
        assert!(
            validate_for_context(
                "fn on_ready() {\n    debug.log(\"Hello\")\n}",
                ScriptKind::Global
            )
            .is_err()
        );
        assert!(
            validate_for_context(
                "fn on_ready() {\n    debug.log(\"Hello\")\n}",
                ScriptKind::Attached
            )
            .is_err()
        );
    }
    #[test]
    fn legitimate_global_custom_event_is_supported() {
        let code =
            "on score_changed(score) {\n    debug.log(score)\n}\nevent.fire(\"score_changed\", 5)";
        assert!(validate_for_context(code, ScriptKind::Global).is_ok());
    }

    #[test]
    fn ai_created_actor_module_becomes_real_project_manager_binding() {
        let root = std::env::temp_dir().join(format!("wkiln_ai_bind_test_{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut editor = super::super::Editor::new();
        let mut world = crate::world::World::new();
        let actor_node = editor
            .project_hierarchy
            .create_actor_node(None, 109)
            .unwrap();
        let path = create_script(
            &mut editor.script_editor,
            &root,
            ScriptRoot::Scripts,
            "AI_Actor_Controller",
            "const actor = self.parent\nactor.on_touch = fn(other) { debug.log(other) }",
        )
        .unwrap();
        let (original_parent, logical, record_path) =
            editor.script_editor.created_script.take().unwrap();
        assert_eq!(original_parent, None);
        assert_eq!(record_path, path);
        assert!(
            world.actor_script_bindings.is_empty(),
            "file creation must not magically attach"
        );
        editor.script_editor.created_script = Some((Some(actor_node), logical, path.clone()));
        let new_node = editor
            .register_created_script_in_project_manager(&mut world)
            .unwrap()
            .unwrap();
        assert_eq!(
            editor.project_hierarchy.attached_actor_id(new_node),
            Some(109)
        );
        assert!(
            world
                .actor_script_bindings
                .iter()
                .any(|binding| binding.actor_id == 109
                    && binding.normalized_script_path() == "scripts/AI_Actor_Controller.aeo")
        );
        assert!(editor.needs_save);
        assert!(editor.script_editor.open_documents[&path].dirty);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moved_or_removed_actor_target_must_be_revalidated_before_apply() {
        let mut h = ProjectHierarchy::new();
        let a = h.create_actor_node(None, 7).unwrap();
        assert!(actor_target_still_valid(&h, 7, a));
        assert!(!actor_target_still_valid(&h, 8, a));
        h.nodes.remove(&a);
        assert!(!actor_target_still_valid(&h, 7, a));
    }
    #[test]
    fn inference_uses_real_project_manager_selection() {
        let mut hierarchy = ProjectHierarchy::new();
        let actor = hierarchy.create_actor_node(None, 42).unwrap();
        let folder = hierarchy.create_folder(Some(actor), "Scripts").unwrap();
        let script = hierarchy
            .create_script_node(Some(folder), "scripts/brain.aeo")
            .unwrap();
        let ctx = selection_context(&hierarchy, Some(script), None, false);
        assert_eq!(ctx.actor, Some((42, actor)));
        assert_eq!(
            inferred_kind("Add touch behavior to this NPC", ctx),
            ScriptKind::Attached
        );
        assert_eq!(
            inferred_kind("Help me design a main menu", ctx),
            ScriptKind::Global
        );
        assert!(actor_target_still_valid(&hierarchy, 42, actor));
        assert!(!actor_target_still_valid(&hierarchy, 999, actor));
        let plain = selection_context(&hierarchy, None, None, false);
        assert_eq!(inferred_kind("Make a script", plain), ScriptKind::Global);
    }
    #[test]
    fn actor_module_requires_real_actor_semantics() {
        assert!(
            validate_for_context(
                "const actor = self.parent\nactor.on_touch = fn(other) { debug.log(other) }",
                ScriptKind::Attached
            )
            .is_ok()
        );
        assert!(
            validate_for_context(
                "entity Enemy {\n fn on_ready() { debug.log(\"ready\") }\n}",
                ScriptKind::Attached
            )
            .is_err()
        );
        assert!(validate_for_context("const actor = self.parent", ScriptKind::Cell).is_err());
    }
}
