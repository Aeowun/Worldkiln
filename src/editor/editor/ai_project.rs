// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
const MAX_ENTRIES: usize = 1600;
const MAX_DEPTH: usize = 8;
const MAX_FILE_BYTES: u64 = 32 * 1024;
const MAX_SEARCH_FILES: usize = 250;
const MAX_MATCHES: usize = 24;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentAccess {
    #[default]
    AskBeforeChanges,
    AutoApply,
    FullAccess,
}
impl AgentAccess {
    pub fn label(self) -> &'static str {
        match self {
            Self::AskBeforeChanges => "Ask Before Changes",
            Self::AutoApply => "Auto Apply",
            Self::FullAccess => "Full Access",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    pub access: AgentAccess,
    pub parser_first: bool,
    pub auto_correct: bool,
    pub max_corrections: u8,
}
impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            access: AgentAccess::default(),
            parser_first: true,
            auto_correct: true,
            max_corrections: 3,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Capability {
    Read,
    Edit,
    Full,
}
pub fn capability(name: &str) -> Option<Capability> {
    match name {
        "list_project_files"
        | "read_project_file"
        | "search_project_files"
        | "search_aeoscript_docs"
        | "browse_aeoscript_knowledge"
        | "search_live_aeowun_docs"
        | "read_live_aeowun_doc"
        | "read_aeoscript_doc"
        | "read_open_scripts"
        | "read_current_aeoscript_api"
        | "read_editor_diagnostics"
        | "inspect_project"
        | "stage_cell_build"
        | "editor_capabilities"
        | "list_project_assets"
        | "inspect_asset"
        | "plan_asset_import"
        | "query_navigation"
        | "read_runtime_state"
        | "read_ui_document"
        | "navigate_editor"
        | "validate_aeoscript"
        | "select_project_node" => Some(Capability::Read),
        "write_script"
        | "generate_image"
        | "generate_actor"
        | "attach_actor_script"
        | "create_actor"
        | "create_cell"
        | "set_resource_property"
        | "save_project" => Some(Capability::Edit),
        "import_project_assets"
        | "create_character_asset"
        | "create_texture_from_image"
        | "create_material_from_textures"
        | "place_actors"
        | "assign_cell_material"
        | "generate_terrain"
        | "sculpt_terrain"
        | "paint_terrain"
        | "set_play_mode" => Some(Capability::Edit),
        "create_ui_document" | "write_ui_document" | "undo_ui_document" => Some(Capability::Edit),
        "undo_agent_change" => Some(Capability::Full),
        _ => None,
    }
}
pub fn permitted(access: AgentAccess, name: &str, approved: bool) -> bool {
    match capability(name) {
        Some(Capability::Read) => true,
        Some(Capability::Edit) => access != AgentAccess::AskBeforeChanges || approved,
        Some(Capability::Full) => {
            access == AgentAccess::FullAccess
                || (access == AgentAccess::AskBeforeChanges && approved)
        }
        None => false,
    }
}

#[derive(Clone)]
pub struct ProjectAccess {
    root: PathBuf,
    editor_root: PathBuf,
    pub display: String,
}
fn denied(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == ".git"
        || name == "target"
        || name == "node_modules"
        || name == "builds"
        || name == "dist"
        || name == "cache"
        || name == ".cache"
        || name == "saveData".to_ascii_lowercase()
        || name.starts_with(".env")
        || name.contains("secret")
        || name.contains("credential")
        || name.contains("token")
        || name.contains("private")
        || name == "id_rsa"
        || name == "id_ed25519"
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.ends_with(".p12")
        || name.ends_with(".pfx")
        || name.ends_with(".sqlite")
        || name.ends_with(".db")
}
fn allowed_text(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "aeo"
            | "json"
            | "jsonc"
            | "toml"
            | "md"
            | "txt"
            | "yml"
            | "yaml"
            | "ron"
            | "ini"
            | "cfg"
            | "csv"
            | "xml"
            | "rs"
            | "wgsl"
            | "glsl"
            | "vert"
            | "frag"
            | "lua"
            | "js"
            | "ts"
            | "html"
            | "css"
            | "py"
            | "manifest"
            | "log"
    )
}
impl ProjectAccess {
    pub fn open(folder: &Path) -> Result<Self, String> {
        let root = folder
            .canonicalize()
            .map_err(|e| format!("Project path unavailable: {e}"))?;
        if !root.is_dir() {
            return Err("No open project directory.".into());
        }
        let display = root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let editor_root = if folder.is_absolute() {
            folder.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(folder)
        };
        Ok(Self {
            root,
            editor_root,
            display,
        })
    }
    pub(super) fn resolve(&self, relative: &str, folder: bool) -> Result<PathBuf, String> {
        if relative.len() > 512 {
            return Err("Path too long.".into());
        }
        let rel = Path::new(relative);
        if rel.is_absolute() {
            return Err("Only project-relative paths are allowed.".into());
        }
        for component in rel.components() {
            let Component::Normal(name) = component else {
                return Err("Invalid relative path.".into());
            };
            if denied(&name.to_string_lossy()) {
                return Err("Protected project path.".into());
            }
        }
        let path = self.root.join(rel);
        let canon = path
            .canonicalize()
            .map_err(|_| "File or directory not found.".to_owned())?;
        if !canon.starts_with(&self.root) {
            return Err("Path escapes the active project.".into());
        }
        let mut current = self.root.clone();
        for part in rel.components() {
            current.push(part);
            let meta =
                fs::symlink_metadata(&current).map_err(|_| "Path unavailable.".to_owned())?;
            if meta.file_type().is_symlink() || meta.file_attributes_reparse_point() {
                return Err("Links and junctions are not accessible.".into());
            }
        }
        if folder && !canon.is_dir() {
            return Err("Not a directory.".into());
        }
        if !folder && !canon.is_file() {
            return Err("Not a file.".into());
        }
        Ok(canon)
    }
    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }
    pub(super) fn script_path(&self, relative: &str) -> Result<PathBuf, String> {
        let relative = relative.replace('\\', "/");
        let rel = Path::new(&relative);
        if relative.len() > 240
            || relative.contains(':')
            || relative.contains('\0')
            || !super::ai_script::is_project_script(&self.root, &self.root.join(rel))
        {
            return Err("Use an .aeo path beneath scripts/ or playerscripts/.".into());
        }
        let mut path = self.root.clone();
        for part in rel.components() {
            let Component::Normal(name) = part else {
                return Err("Invalid script path.".into());
            };
            let text = name.to_string_lossy();
            if denied(&text)
                || text.starts_with('.')
                || text.ends_with('.')
                || text.ends_with(' ')
                || text.contains(':')
            {
                return Err("Protected or invalid script path.".into());
            }
            path.push(name);
            if path.exists() || fs::symlink_metadata(&path).is_ok() {
                let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                if meta.file_type().is_symlink()
                    || meta.file_attributes_reparse_point()
                    || !path
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                        .starts_with(&self.root)
                {
                    return Err("Links, junctions and project escapes are not editable.".into());
                }
            }
        }
        Ok(self.editor_root.join(rel))
    }
    pub fn execute(&self, name: &str, args: &Value) -> String {
        let result = match name {
            "list_project_files" => self.list(
                args["path"].as_str().unwrap_or(""),
                args["recursive"].as_bool().unwrap_or(false),
            ),
            "read_project_file" => self.read(
                args["path"].as_str().unwrap_or(""),
                args["start_line"].as_u64().unwrap_or(1) as usize,
            ),
            "search_project_files" => self.search(args["query"].as_str().unwrap_or("")),
            "browse_aeoscript_knowledge" => Ok(super::ai_knowledge::browse_catalog(
                args["path"].as_str().unwrap_or("root"),
                args["query"].as_str().unwrap_or(""),
            )),
            "search_aeoscript_docs" => Ok(super::ai_knowledge::search(
                args["query"].as_str().unwrap_or(""),
                5,
            )),
            "read_aeoscript_doc" => {
                Ok(super::ai_knowledge::read(args["id"].as_str().unwrap_or("")))
            }
            "read_current_aeoscript_api" => Ok(super::ai_knowledge::current_api(
                args["query"].as_str().unwrap_or(""),
            )),
            "search_live_aeowun_docs" => {
                super::ai_knowledge::search_live_docs(args["query"].as_str().unwrap_or(""))
            }
            "read_live_aeowun_doc" => {
                super::ai_knowledge::read_live_doc(args["path"].as_str().unwrap_or(""))
            }
            _ => Err("Unknown or disallowed project tool.".into()),
        };
        match result {
            Ok(value) => value,
            Err(e) => format!("ERROR: {e}"),
        }
    }
    fn list(&self, relative: &str, recursive: bool) -> Result<String, String> {
        let path = self.resolve(relative, true)?;
        let mut queue = VecDeque::from([(path, 0usize)]);
        let mut entries = Vec::new();
        while let Some((folder, depth)) = queue.pop_front() {
            let mut children = fs::read_dir(&folder)
                .map_err(|e| format!("Directory unreadable: {e}"))?
                .filter_map(Result::ok)
                .collect::<Vec<_>>();
            children.sort_by_key(|e| e.file_name());
            for item in children {
                if entries.len() >= MAX_ENTRIES {
                    entries.push("... results truncated (1600 entries)".into());
                    break;
                }
                let name = item.file_name().to_string_lossy().to_string();
                if denied(&name) {
                    continue;
                }
                let Ok(kind) = item.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                let path = item.path();
                if fs::symlink_metadata(&path).is_ok_and(|m| m.file_attributes_reparse_point()) {
                    continue;
                }
                if kind.is_dir() {
                    entries.push(format!("{}/", self.rel(&path)));
                    if recursive && depth < MAX_DEPTH {
                        queue.push_back((path, depth + 1));
                    }
                } else if kind.is_file() && allowed_text(&path) {
                    entries.push(self.rel(&path));
                }
            }
            if entries.len() >= MAX_ENTRIES {
                break;
            }
        }
        Ok(if entries.is_empty() {
            "No accessible text files or directories.".into()
        } else {
            entries.join("\n")
        })
    }
    fn read(&self, relative: &str, start: usize) -> Result<String, String> {
        let path = self.resolve(relative, false)?;
        if !allowed_text(&path) {
            return Err("Only approved text/code file types are readable.".into());
        }
        let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
        if size > MAX_FILE_BYTES {
            return Err("File is over 32 KiB. Use search or inspect a smaller file.".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let text = String::from_utf8(bytes).map_err(|_| "Not a UTF-8 text file.".to_owned())?;
        if text.contains('\0') {
            return Err("Binary content is not readable.".into());
        }
        let first = start.max(1);
        if first > text.lines().count().max(1) {
            return Err("Start line past end of file.".into());
        }
        let lines = text
            .lines()
            .enumerate()
            .skip(first - 1)
            .take(180)
            .map(|(i, line)| format!("{:>4}: {}", i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!(
            "FILE: {}\nLINES from {} (up to 180):\n{}",
            self.rel(&path),
            first,
            lines
        ))
    }
    fn search(&self, query: &str) -> Result<String, String> {
        let q = query.trim();
        if q.is_empty() || q.len() > 120 {
            return Err("Use a search term of 1-120 characters.".into());
        }
        let mut files = Vec::new();
        let mut queue = VecDeque::from([(self.root.clone(), 0usize)]);
        while let Some((folder, depth)) = queue.pop_front() {
            let Ok(children) = fs::read_dir(&folder) else {
                continue;
            };
            for item in children.flatten() {
                let name = item.file_name().to_string_lossy().to_string();
                if denied(&name) {
                    continue;
                }
                let Ok(kind) = item.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                let path = item.path();
                if fs::symlink_metadata(&path).is_ok_and(|m| m.file_attributes_reparse_point()) {
                    continue;
                }
                if kind.is_dir() && depth < MAX_DEPTH {
                    queue.push_back((path, depth + 1));
                } else if kind.is_file() && allowed_text(&path) {
                    files.push(path);
                    if files.len() >= MAX_SEARCH_FILES {
                        break;
                    }
                }
            }
            if files.len() >= MAX_SEARCH_FILES {
                break;
            }
        }
        files.sort();
        let mut results = Vec::new();
        for file in &files {
            if fs::metadata(file).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
                continue;
            }
            let Ok(text) = fs::read_to_string(file) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if line.to_ascii_lowercase().contains(&q.to_ascii_lowercase()) {
                    let line = line.chars().take(260).collect::<String>();
                    results.push(format!("{}:{}: {}", self.rel(file), i + 1, line));
                    if results.len() >= MAX_MATCHES {
                        break;
                    }
                }
            }
            if results.len() >= MAX_MATCHES {
                break;
            }
        }
        Ok(if results.is_empty() {
            format!("No matches in {} scanned text files.", files.len())
        } else {
            format!(
                "Matches ({} scanned text files, capped at {}):\n{}",
                files.len(),
                MAX_MATCHES,
                results.join("\n")
            )
        })
    }
}
trait Reparse {
    fn file_attributes_reparse_point(&self) -> bool;
}
impl Reparse for fs::Metadata {
    #[cfg(windows)]
    fn file_attributes_reparse_point(&self) -> bool {
        use std::os::windows::fs::MetadataExt;
        self.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    fn file_attributes_reparse_point(&self) -> bool {
        false
    }
}
pub fn tool_specs() -> Value {
    let mut specs = json!([
        {"name":"list_project_files","description":"List accessible text/code files and folders inside the current active Worldkiln project. path is project-relative, empty for project root. recursive true to walk subfolders (bounded).","parameters":{"type":"object","properties":{"path":{"type":"string","description":"Relative directory e.g. scripts or empty for root"},"recursive":{"type":"boolean"}},"required":["path"]}},
        {"name":"read_project_file","description":"Read project text/code. For .aeo scripts the editor returns actual live source (including unsaved edits), Actor/Cell context and Worldkiln lexer/parser/semantic/API validation: this is more reliable than assuming JavaScript/Lua/Python rules. For uncertain AeoScript API syntax, the optional catalogue/docs tools provide engine-specific examples.","parameters":{"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","description":"1-based first line"}},"required":["path"]}},
        {"name":"search_project_files","description":"Search bounded project text/code files case-insensitively, returning filename/line/short excerpts.","parameters":{"type":"object","properties":{"query":{"type":"string","description":"Case-insensitive literal text to find"}},"required":["query"]}},
        {"name":"browse_aeoscript_knowledge","description":"Optional on-demand progressive Worldkiln knowledge browser. Start path=root to see ONLY Web and Catalogue options. Open catalog for UI/NAV/Player/Actor/Cell/Modules/Language categories; each subpath gives a short overview and further PATH options. Only a final selected example displays code, checked against the current parser and semantics. Opening web shows how to use the separate live/offline search tools. NEVER auto-browse all pages or inject the full catalogue; stop as soon as the task is understood. Can optionally search catalogue titles with path=catalog/search and query.","parameters":{"type":"object","properties":{"path":{"type":"string","description":"Exactly one PATH from a previous browse result, default root"}, "query":{"type":"string","description":"Only used for path=catalog/search; omit otherwise"}}}},
        {"name":"search_aeoscript_docs","description":"Search indexed Aeowun/Worldkiln AeoScript reference, UI, modules, global script startup, Cells, Actors and tutorials. Returns citations, examples and source-code file pointers. Read relevant docs before proposing an API.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}},
        {"name":"read_aeoscript_doc","description":"Read an indexed Worldkiln documentation page found by search_aeoscript_docs. Use its id or full https://aeowun.com/ URL.","parameters":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
        {"name":"search_live_aeowun_docs","description":"On-demand search of the official Aeowun documentation index. Returns up to five matching page titles, paths and URLs ONLY; does not fetch page contents. Search only when unsure about a specific syntax/API/workflow, not before every script. Engine source is authoritative.","parameters":{"type":"object","properties":{"query":{"type":"string","description":"Brief generic documentation topic, no user/project data"}},"required":["query"]}},
        {"name":"read_live_aeowun_doc","description":"On demand, read only ONE short excerpt from a live documentation page listed by search_live_aeowun_docs. Pass the exact PATH, never an arbitrary URL. Advisory source, not verified engine behavior.","parameters":{"type":"object","properties":{"path":{"type":"string","description":"Exact /docs/, /aeoscript/, or /worldkiln/ PATH returned by search_live_aeowun_docs"}},"required":["path"]}}
    ]);
    let tools = [
        (
            "generate_image",
            "BETA features — may NOT work properly. Generate a procedural PNG image/texture with vetted image_generator.py, not trained text-to-image synthesis or arbitrary Python. Python 3 must be installed. Saves a new project generated/name.png without overwriting. No Material or scene changes. Patterns solid/checker/noise/brick/gradient; palette is RGBA byte colors; optional rect/circle shapes use pixel bounds [x,y,width,height]. Report actual tool evidence and BETA warning.",
            json!({"name":{"type":"string"},"width":{"type":"integer","minimum":1,"maximum":1024},"height":{"type":"integer","minimum":1,"maximum":1024},"pattern":{"type":"string","enum":["solid","checker","noise","brick","gradient"]},"palette":{"type":"array","items":{"type":"array","items":{"type":"integer"}}},"seed":{"type":"integer"},"scale":{"type":"integer"},"shapes":{"type":"array","items":{"type":"object","properties":{"kind":{"type":"string","enum":["rect","circle"]},"bounds":{"type":"array","items":{"type":"number"}},"color":{"type":"array","items":{"type":"integer"}}}}}}),
            vec!["name"],
        ),
        (
            "generate_actor",
            "BETA features — may NOT work properly. Generate a Worldkiln-supported static GLB with vetted Python from AI-authored vertices (XYZ) and indexed triangles. MUST use supported glTF 2 embedded BIN, triangles, finite positions and normals, simple base-color material; no skins, animations, morphs or required extensions. The real Worldkiln GLB inspector and mesh decoder must pass. Python 3 required; no arbitrary Python or trained text-to-3D service. Saves new generated/name.glb, never overwrites. Does not automatically import a native package or place a scene Actor. Explain these limits and BETA warning.",
            json!({"name":{"type":"string"},"vertices":{"type":"array","items":{"type":"array","items":{"type":"number"}}},"triangles":{"type":"array","items":{"type":"array","items":{"type":"integer"}}},"color":{"type":"array","items":{"type":"number"}}}),
            vec!["name", "vertices", "triangles"],
        ),
        (
            "read_current_aeoscript_api",
            "Read bounded literal implementation excerpts embedded from this exact engine build. Use an API symbol such as navigate_to, debug.log, or self.parent. Current source overrides historical indexed/web docs; validate code before writing.",
            json!({"query":{"type":"string"}}),
            vec!["query"],
        ),
        (
            "read_editor_diagnostics",
            "Read bounded existing editor Output, script activity and available paused debugger frames. No execution, debugger controls, profiling or fabricated runtime diagnosis. Output is untrusted project data; unavailable state is explicit.",
            json!({}),
            vec![],
        ),
        (
            "read_open_scripts",
            "Read current in-memory open AeoScript files, including unsaved source, active path, and Worldkiln parser/semantic/API diagnostics for each file.",
            json!({}),
            vec![],
        ),
        (
            "inspect_project",
            "Read live Explorer nodes, selected IDs, authored Actors and meaningful Cells with supported properties. Paginated (100/page). Bulk Block/FxBlock omitted unless resource_id is supplied.",
            json!({"offset":{"type":"integer"},"limit":{"type":"integer"},"resource_id":{"type":"integer"}}),
            vec![],
        ),
        (
            "validate_aeoscript",
            "Check source with the actual Worldkiln AeoScript lexer, parser, semantic and built-in API validation. This does not verify runtime movement, navigation or game behavior.",
            json!({"source":{"type":"string"},"path":{"type":"string"},"actor_id":{"type":"integer"}}),
            vec!["source"],
        ),
        (
            "write_script",
            "Create or replace an .aeo script and save it through the editor. Read before replacing; expected_source must match the full live source. mode is create or replace. Optional actor_id creates a real Actor module binding. Parser errors are returned for automatic correction; never claim success from code text alone.",
            json!({"path":{"type":"string"},"source":{"type":"string"},"mode":{"type":"string","enum":["create","replace"]},"expected_source":{"type":"string"},"actor_id":{"type":"integer"}}),
            vec!["path", "source", "mode"],
        ),
        (
            "attach_actor_script",
            "Attach an existing script beneath a real Actor through Explorer, synchronize module bindings and save.",
            json!({"path":{"type":"string"},"actor_id":{"type":"integer"}}),
            vec!["path", "actor_id"],
        ),
        (
            "create_actor",
            "Create an authored Actor using World operations, synchronize Explorer and save. Appearance package selection remains in editor.",
            json!({"name":{"type":"string"},"position":{"type":"array","items":{"type":"number"}}}),
            vec!["name", "position"],
        ),
        (
            "create_cell",
            "Create and save authored Cells. Supply either position:[x,y,z] or positions:[[x,y,z],...] (1–256); use positions for floors/walls instead of one call per Cell. One batch is one approval, undo entry and save request. All coordinates must be empty, distinct integers; invalid batches create nothing. Returns created IDs and coordinates. Types: Block, FxBlock, Light, SpawnPoint, AudioEmitter, ParticleEmitter.",
            json!({"cell_type":{"type":"string"},"position":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer"}},"positions":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer"}}}}),
            vec!["cell_type"],
        ),
        (
            "set_resource_property",
            "Change one supported authored property and save. Actor: name, position, rotation, scale, visible, enabled, solid, anchored. Cell: visible, solid, anchored, color. Use an ID from inspect_project.",
            json!({"kind":{"type":"string","enum":["Actor","Cell"]},"id":{"type":"integer"},"property":{"type":"string"},"value_json":{"type":"string","description":"JSON encoded property value, e.g. true, [1,2,3], or a quoted name"}}),
            vec!["kind", "id", "property", "value_json"],
        ),
        (
            "select_project_node",
            "Select a real Explorer node and open its Script or Properties using existing editor navigation.",
            json!({"node_id":{"type":"integer"}}),
            vec!["node_id"],
        ),
        (
            "save_project",
            "Save the current World and hierarchy. Agent script edits already save their affected documents; unrelated unsaved scripts remain untouched.",
            json!({}),
            vec![],
        ),
        (
            "undo_agent_change",
            "Undo the latest agent editor change if its files and affected scene state have not changed. Full Access or explicit approval required; preserve later user edits.",
            json!({}),
            vec![],
        ),
    ];
    for (name, description, properties, required) in tools {
        specs
            .as_array_mut()
            .unwrap()
            .push(json!({"name":name,"description":description,
            "parameters":{"type":"object","properties":properties,"required":required}}));
    }
    let cell = specs
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|tool| tool["name"] == "create_cell")
        .unwrap();
    cell["parameters"]["properties"]["cells"] = json!({"type":"array","minItems":1,"maxItems":256,
        "items":{"type":"object","properties":{"cell_type":{"type":"string"},"position":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"integer"}}},"required":["cell_type","position"]}});
    cell["parameters"]["required"] = json!([]);
    cell["description"] = json!(
        "Apply a Cell build and request saving. Supply cells:[{cell_type,position:[x,y,z]},...] for mixed types, or cell_type plus position/positions. Batch limit 256. Stage proposed builds with stage_cell_build first. Entire plan is checked again before mutation; occupied, duplicate, invalid coordinates reject the whole batch. One batch uses one approval, undo entry and save request. Returns IDs/coordinates. No overwrites. Types: Block, FxBlock, Light, SpawnPoint, AudioEmitter, ParticleEmitter."
    );
    let mut stage = cell.clone();
    stage["name"] = json!("stage_cell_build");
    stage["description"] = json!(
        "Stage a proposed Cell build as plan data before applying it. Same arguments as create_cell; up to 256 mixed Cells or a homogeneous positions array. Checks types, exact integer coordinates, duplicates and occupancy. Returns the complete arguments to review/apply; does not mutate or save the world, allocate IDs, create undo history or reserve space. Apply through create_cell with normal approval and fresh conflict checks."
    );
    specs.as_array_mut().unwrap().push(stage);
    specs
        .as_array_mut()
        .unwrap()
        .extend(super::ai_authoring::tool_specs());
    specs
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browse_catalog_is_a_read_only_discoverable_agent_tool() {
        assert!(matches!(
            capability("browse_aeoscript_knowledge"),
            Some(Capability::Read)
        ));
        let specs = tool_specs();
        assert!(
            specs
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "browse_aeoscript_knowledge"
                    && tool["parameters"]["properties"]["path"]["type"] == "string")
        );
    }
    #[test]
    fn traversal_and_secrets_are_blocked() {
        let temp =
            std::env::temp_dir().join(format!("worldkiln_ai_project_{}", std::process::id()));
        fs::create_dir_all(temp.join("scripts")).unwrap();
        fs::write(temp.join("scripts/main.aeo"), "print(\"found\")\n").unwrap();
        fs::write(temp.join(".env"), "PASSWORD=test").unwrap();
        let access = ProjectAccess::open(&temp).unwrap();
        let options = access.execute("browse_aeoscript_knowledge", &json!({"path":"root"}));
        assert!(options.contains("PATH: web") && options.contains("PATH: catalog"));
        assert!(!options.contains("AEOSCRIPT:"));
        assert!(
            access
                .execute("read_project_file", &json!({"path":"scripts/main.aeo"}))
                .contains("found")
        );
        assert!(
            access
                .execute("search_project_files", &json!({"query":"found"}))
                .contains("main.aeo")
        );
        assert!(
            access
                .execute("read_project_file", &json!({"path":"../secret.txt"}))
                .contains("ERROR")
        );
        assert!(
            access
                .execute("read_project_file", &json!({"path":".env"}))
                .contains("ERROR")
        );
        assert!(
            !access
                .execute("list_project_files", &json!({"path":"","recursive":true}))
                .contains(".env")
        );
        fs::remove_dir_all(temp).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn links_and_junctions_blocked() {
        let temp = std::env::temp_dir().join(format!("worldkiln_ai_links_{}", std::process::id()));
        fs::create_dir_all(&temp).unwrap();
        fs::write(temp.join("ok.aeo"), "good").unwrap();
        if std::os::windows::fs::symlink_file(temp.join("ok.aeo"), temp.join("link.aeo")).is_ok() {
            let access = ProjectAccess::open(&temp).unwrap();
            assert!(
                access
                    .execute("read_project_file", &json!({"path":"link.aeo"}))
                    .contains("ERROR")
            );
        }
        fs::remove_dir_all(temp).unwrap();
    }
}
