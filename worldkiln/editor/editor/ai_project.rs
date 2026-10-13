// Worldkiln 0.8.5 | WO 0.8.7-63 | 2026-10-09
// Project-scoped, bounded, read-only tools. Never follow symlinks/reparse points.
use serde_json::{Value, json};
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
use serde::{Deserialize, Serialize};
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

// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
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
        | "read_aeoscript_doc"
        | "read_open_scripts"
        | "inspect_project"
        | "validate_aeoscript"
        | "select_project_node" => Some(Capability::Read),
        "write_script"
        | "attach_actor_script"
        | "create_actor"
        | "create_cell"
        | "set_resource_property"
        | "save_project" => Some(Capability::Edit),
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
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:09 -05:00
    // Preserve editor spelling: Windows canonical paths can have an extended prefix.
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
        // Deny every traversed reparse/symlink component, including directory junctions.
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
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    // Validate every existing ancestor before any editor command may create directories.
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
            "search_aeoscript_docs" => Ok(super::ai_knowledge::search(
                args["query"].as_str().unwrap_or(""),
                5,
            )),
            "read_aeoscript_doc" => {
                Ok(super::ai_knowledge::read(args["id"].as_str().unwrap_or("")))
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
                // Windows junctions are handled as reparse points by resolve and explicit metadata below.
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
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    let mut specs = json!([
        {"name":"list_project_files","description":"List accessible text/code files and folders inside the current active Worldkiln project. path is project-relative, empty for project root. recursive true to walk subfolders (bounded).","parameters":{"type":"object","properties":{"path":{"type":"string","description":"Relative directory e.g. scripts or empty for root"},"recursive":{"type":"boolean"}},"required":["path"]}},
        {"name":"read_project_file","description":"Read up to 180 numbered lines from a project-relative text/code file up to 32 KiB. Use start_line for following pages. Binary files and secrets inaccessible.","parameters":{"type":"object","properties":{"path":{"type":"string"},"start_line":{"type":"integer","description":"1-based first line"}},"required":["path"]}},
        {"name":"search_project_files","description":"Search bounded project text/code files case-insensitively, returning filename/line/short excerpts.","parameters":{"type":"object","properties":{"query":{"type":"string","description":"Case-insensitive literal text to find"}},"required":["query"]}},
        {"name":"search_aeoscript_docs","description":"Search indexed Aeowun/Worldkiln AeoScript reference, UI, modules, global script startup, Cells, Actors and tutorials. Returns citations, examples and source-code file pointers. Read relevant docs before proposing an API.","parameters":{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}},
        {"name":"read_aeoscript_doc","description":"Read an indexed Worldkiln documentation page found by search_aeoscript_docs. Use its id or full https://aeowun.com/ URL.","parameters":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}
    ]);
    let tools = [
        (
            "read_open_scripts",
            "Read current in-memory open scripts, including unsaved contents and the active path.",
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
            "Run the real lexer/parser and return line/column diagnostics. Parsing is not runtime testing.",
            json!({"source":{"type":"string"}}),
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
            "Create an authored Cell at an empty integer grid coordinate and save. Supported types: Block, FxBlock, Light, SpawnPoint, AudioEmitter, ParticleEmitter. Never overwrite occupied coordinates.",
            json!({"cell_type":{"type":"string"},"position":{"type":"array","items":{"type":"integer"}}}),
            vec!["cell_type", "position"],
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
    specs
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_and_secrets_are_blocked() {
        let temp =
            std::env::temp_dir().join(format!("worldkiln_ai_project_{}", std::process::id()));
        fs::create_dir_all(temp.join("scripts")).unwrap();
        fs::write(temp.join("scripts/main.aeo"), "print(\"found\")\n").unwrap();
        fs::write(temp.join(".env"), "PASSWORD=test").unwrap();
        let access = ProjectAccess::open(&temp).unwrap();
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
        // Windows symlink creation may be unavailable without Developer Mode; if it succeeds, verify denial.
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
