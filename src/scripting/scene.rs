// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::api::HostContext;
use super::ast::{Declaration, EntityMember, Program};
use super::binding::ScriptBinding;
use super::execution::{FiberResult, ScriptTaskId};
use super::interpreter::ScriptInstance;
use super::lexer::Lexer;
use super::parser::Parser;
use super::runtime::ScriptRuntime;
use super::source::SourceSpan;
use super::value::{DynamicProperty, HandleKind, Value};
use crate::engine::entity::{EntityId, EntityManager};
use crate::project::{IndexedModule, ProjectHierarchy, ProjectScriptIndex, ScriptRoot};
use crate::world::{CellType, World, WorldCoord};
use glam::Vec3;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use super::log::LogRecord;

#[derive(Debug)]
pub struct ScriptEntity {
    instance: ScriptInstance,
    state: EntityState,
    stopped: bool,
    script_path: Option<String>,
    pub(crate) associated_cell_id: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EntityState {
    Initial,
    Spawning(Option<ScriptTaskId>),
    Readying(Option<ScriptTaskId>),
    Active { update_task: Option<ScriptTaskId> },
    Stopped,
}

impl ScriptEntity {
    fn new(instance: ScriptInstance, script_path: Option<String>, cell_id: Option<u64>) -> Self {
        Self {
            instance,
            state: EntityState::Initial,
            stopped: false,
            script_path,
            associated_cell_id: cell_id,
        }
    }

    fn active_task(&self) -> Option<ScriptTaskId> {
        match self.state {
            EntityState::Spawning(Some(tid)) => Some(tid),
            EntityState::Readying(Some(tid)) => Some(tid),
            EntityState::Active {
                update_task: Some(tid),
            } => Some(tid),
            _ => None,
        }
    }

    fn complete_task(&mut self, id: ScriptTaskId) {
        match self.state {
            EntityState::Spawning(Some(tid)) if tid == id => {
                self.state = EntityState::Spawning(None);
            }
            EntityState::Readying(Some(tid)) if tid == id => {
                self.state = EntityState::Readying(None);
            }
            EntityState::Active {
                update_task: Some(tid),
            } if tid == id => {
                self.state = EntityState::Active { update_task: None };
            }
            _ => {}
        }
    }
}

use std::sync::Arc;

#[derive(Debug)]
pub struct ScriptScene {
    runtime: ScriptRuntime,
    pub(crate) entities: Vec<ScriptEntity>,
    pub disabled_scripts: Vec<String>,
    pub loaded_scripts: HashMap<String, Program>,
    pub top_level_spawned: HashSet<String>,
    pub valid_entity_declarations: Arc<HashSet<String>>,
    pub pending_spawns: Vec<(String, u64)>,
    pub project_script_index: Option<ProjectScriptIndex>,

    actor_module_bindings: Vec<(u64, String)>,

    actor_module_paths: HashSet<String>,

    failed_attached_modules: HashSet<(u64, String)>,

    script_load_failures: HashMap<String, String>,

    failed_scripts: HashSet<String>,

    pub(crate) player_module_paths: Vec<String>,

    player_module_path_set: HashSet<String>,

    dynamic_handler_owners: HashMap<ScriptTaskId, (HandleKind, u64)>,

    current_time: f64,
}

fn parse_named_script(path: &str, source: &str) -> Result<Program, String> {
    let tokens = Lexer::new(source).tokenize().map_err(|error| {
        format!(
            "Lexer error at {path}:{}:{} | <lex>: {}",
            error.line, error.column, error.message
        )
    })?;
    Parser::new(tokens).parse().map_err(|error| {
        format!(
            "Parser error at {path}:{}:{} | <parse>: {}",
            error.line, error.column, error.message
        )
    })
}

fn load_aeo_files_recursively(
    dir: &Path,
    project_path: &Path,
    loaded_scripts: &mut HashMap<String, Program>,
    load_failures: &mut HashMap<String, String>,
) -> Result<(), String> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?
        .flatten()
    {
        let path = entry.path();
        if path.is_dir() {
            load_aeo_files_recursively(&path, project_path, loaded_scripts, load_failures)?;
        } else if path.is_file() && path.extension().map_or(false, |ext| ext == "aeo") {
            let relative_path = path
                .strip_prefix(project_path)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string()
                .replace("\\", "/");
            match std::fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read script {:?}: {}", path, e))
                .and_then(|source| parse_named_script(&relative_path, &source))
            {
                Ok(program) => {
                    loaded_scripts.insert(relative_path, program);
                }
                Err(error) => {
                    load_failures.insert(relative_path, error);
                }
            }
        }
    }
    Ok(())
}

fn load_project_script_roots(
    project_path: &Path,
    loaded_scripts: &mut HashMap<String, Program>,
    load_failures: &mut HashMap<String, String>,
) -> Result<(), String> {
    load_aeo_files_recursively(
        &project_path.join("scripts"),
        project_path,
        loaded_scripts,
        load_failures,
    )?;

    load_aeo_files_recursively(
        &project_path.join("playerscripts"),
        project_path,
        loaded_scripts,
        load_failures,
    )?;

    Ok(())
}

fn load_profile_script_root(
    project_path: &Path,
    loaded_scripts: &mut HashMap<String, Program>,
    load_failures: &mut HashMap<String, String>,
) -> Result<(), String> {
    let Some(user_data_root) = project_path.parent() else {
        return Ok(());
    };
    load_package_aeo_files(
        &user_data_root.join("scripts"),
        "profile/scripts",
        loaded_scripts,
        load_failures,
    )
}

fn load_package_aeo_files(
    dir: &Path,
    logical_root: &str,
    loaded_scripts: &mut HashMap<String, Program>,
    load_failures: &mut HashMap<String, String>,
) -> Result<(), String> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {:?}: {}", dir, e))?
        .flatten()
    {
        let path = entry.path();

        if path.is_dir() {
            let relative = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace("\\", "/");

            let next_root = if relative.is_empty() {
                logical_root.to_string()
            } else {
                format!("{}/{}", logical_root, relative)
            };

            load_package_aeo_files(&path, &next_root, loaded_scripts, load_failures)?;
            continue;
        }

        if !path.is_file() || path.extension().map_or(true, |ext| ext != "aeo") {
            continue;
        }

        let relative = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace("\\", "/");

        let logical_path = if relative.is_empty() {
            logical_root.to_string()
        } else {
            format!("{}/{}", logical_root, relative)
        };

        match std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read script {:?}: {}", path, e))
            .and_then(|source| parse_named_script(&logical_path, &source))
        {
            Ok(program) => {
                loaded_scripts.insert(logical_path, program);
            }
            Err(error) => {
                load_failures.insert(logical_path, error);
            }
        }
    }

    Ok(())
}

fn load_asset_script_packages(
    assets_root: &Path,
    category: &str,
    loaded_scripts: &mut HashMap<String, Program>,
    load_failures: &mut HashMap<String, String>,
) -> Result<(), String> {
    let category_dir = assets_root.join(category);

    if !category_dir.exists() {
        return Ok(());
    }

    let mut packages = Vec::new();

    for entry in std::fs::read_dir(&category_dir)
        .map_err(|e| format!("Failed to read directory {:?}: {}", category_dir, e))?
        .flatten()
    {
        if entry.path().is_dir() {
            packages.push(entry.path());
        }
    }

    packages.sort();

    for package_dir in packages {
        let package_name = package_dir
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid package directory {:?}", package_dir))?;

        let logical_root = format!("{}/{}", category, package_name);

        load_package_aeo_files(&package_dir, &logical_root, loaded_scripts, load_failures)?;
    }

    Ok(())
}

impl ScriptScene {
    fn resolve_authored_ui_module_key(&self, module_key: &str) -> Result<String, String> {
        let key = module_key.trim().replace('\\', "/");
        if key.is_empty() {
            return Err("Module/script key is empty".to_string());
        }

        if let Some(index) = &self.project_script_index {
            if let Some(module) = index.find_module(&key) {
                return Ok(module.logical_path().to_string_lossy().replace('\\', "/"));
            }

            let named = index.find_modules_named(&key);
            if named.len() == 1 {
                return Ok(named[0].logical_path().to_string_lossy().replace('\\', "/"));
            }
            if named.len() > 1 {
                let matches = named
                    .iter()
                    .map(|module| module.logical_path().to_string_lossy().replace('\\', "/"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!(
                    "Module/script key '{}' is ambiguous; matches: {}",
                    module_key, matches
                ));
            }
        }

        let key_name = std::path::Path::new(&key)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(&key);
        let mut matches = self
            .loaded_scripts
            .keys()
            .filter(|path| {
                let normalized_path = path.replace('\\', "/");
                normalized_path.eq_ignore_ascii_case(&key)
                    || std::path::Path::new(&normalized_path)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .is_some_and(|stem| stem.eq_ignore_ascii_case(key_name))
            })
            .cloned()
            .collect::<Vec<_>>();
        matches.sort();
        matches.dedup();

        match matches.as_slice() {
            [path] => Ok(path.clone()),
            [] => Err(format!("Module/script key '{}' was not found", module_key)),
            _ => Err(format!(
                "Module/script key '{}' is ambiguous; matches: {}",
                module_key,
                matches.join(", ")
            )),
        }
    }

    fn dispatch_authored_ui_event(
        &mut self,
        dispatch: crate::engine::ui::UiSymbolicEventDispatch,
    ) -> Result<(), String> {
        let target = format!("{}::{}", dispatch.module_key, dispatch.handler);
        let context = format!(
            "UI document='{}' element='{}' type='{}' event='{}' target='{}'",
            dispatch.document_key,
            dispatch.element_key,
            dispatch.element_type,
            dispatch.event,
            target
        );

        let path = self
            .resolve_authored_ui_module_key(&dispatch.module_key)
            .map_err(|error| format!("Authored UI binding failed: {context}: {error}"))?;

        if self.disabled_scripts.contains(&path) {
            return Err(format!(
                "Authored UI binding failed: {context}: resolved script '{}' is disabled",
                path
            ));
        }

        let program = self.loaded_scripts.get(&path).ok_or_else(|| {
            format!(
                "Authored UI binding failed: {context}: resolved script '{}' is not loaded",
                path
            )
        })?;
        let function = program
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Function(function) if function.name == dispatch.handler => {
                    Some(function.clone())
                }
                _ => None,
            })
            .ok_or_else(|| {
                format!(
                    "Authored UI binding failed: {context}: handler '{}' was not found as a top-level function in '{}'",
                    dispatch.handler, path
                )
            })?;

        self.runtime
            .spawn_named_script_handler(&path, &function)
            .map_err(|error| format!("Authored UI binding failed: {context}: {error}"))?;

        Ok(())
    }

    pub fn set_debug_breakpoints(
        &mut self,
        breakpoints: Vec<crate::scripting::debugger::DebugBreakpoint>,
    ) {
        self.runtime.set_debug_breakpoints(breakpoints);
    }

    pub fn debug_snapshot(&self) -> Option<crate::scripting::debugger::DebugSnapshot> {
        self.runtime.debug_snapshot()
    }

    pub fn debug_command(
        &mut self,
        command: crate::scripting::debugger::DebugCommand,
    ) -> Result<(), String> {
        self.runtime.debug_command(command)
    }

    pub fn clear_debugger_runtime_state(&mut self) {
        self.runtime.clear_debugger_runtime_state();
    }

    pub fn load_from_bindings(
        project_path: &Path,
        world: &World,
        bindings: &[ScriptBinding],
        entity_manager: &mut EntityManager,
        preexisting_entities: &HashMap<u64, EntityId>,
        delta_time: f64,
    ) -> Result<Self, String> {
        let project_script_index = ProjectScriptIndex::build(project_path).map_err(|error| {
            format!(
                "Failed to build project script index for {:?}: {}",
                project_path, error
            )
        })?;

        let all_player_module_paths = project_script_index
            .all_modules()
            .into_iter()
            .filter(|module| module.root == ScriptRoot::PlayerScripts)
            .map(|module| module.logical_path().to_string_lossy().replace('\\', "/"))
            .collect::<Vec<_>>();

        let player_module_path_set = all_player_module_paths
            .iter()
            .cloned()
            .collect::<HashSet<_>>();

        let player_module_paths = all_player_module_paths;

        let project_hierarchy =
            crate::project::load_project_metadata(project_path).map_err(|error| {
                format!(
                    "Failed to load project hierarchy for {:?}: {}",
                    project_path, error
                )
            })?;

        let mut valid_bindings = Vec::new();
        let mut stale_warnings = Vec::new();
        let mut isolated_errors = Vec::new();
        let is_authored_disabled = |path: &str| {
            let normalized = Self::normalize_script_path(path);
            world
                .disabled_scripts
                .iter()
                .any(|disabled| Self::normalize_script_path(disabled) == normalized)
        };
        let mut seen_binding_targets = HashSet::new();

        for binding in bindings {
            if is_authored_disabled(&binding.script_path) {
                continue;
            }

            if !seen_binding_targets.insert(binding.target_identity) {
                return Err(format!(
                    "Multiple script bindings found for target cell ID '{}'. Each authored cell can only have one binding.",
                    binding.target_identity
                ));
            }

            if world.resolve_cell_id(binding.target_identity).is_none() {
                stale_warnings.push(format!(
                    "Stale script binding found: target cell ID '{}' does not exist in the world as an authored cell.",
                    binding.target_identity
                ));
                continue;
            }

            valid_bindings.push(binding.clone());
        }

        let mut loaded_scripts: HashMap<String, Program> = HashMap::new();
        let mut script_load_failures: HashMap<String, String> = HashMap::new();

        load_profile_script_root(project_path, &mut loaded_scripts, &mut script_load_failures)?;
        load_project_script_roots(project_path, &mut loaded_scripts, &mut script_load_failures)?;

        let project_assets = project_path.join(".assets");
        load_asset_script_packages(
            &project_assets,
            "controllers",
            &mut loaded_scripts,
            &mut script_load_failures,
        )?;
        load_asset_script_packages(
            &project_assets,
            "cameras",
            &mut loaded_scripts,
            &mut script_load_failures,
        )?;

        let mut actor_module_bindings: Vec<(u64, String)> = Vec::new();
        let mut actor_module_paths: HashSet<String> = HashSet::new();
        let mut seen_actor_module_bindings: HashSet<(u64, String)> = HashSet::new();

        for binding in &world.actor_script_bindings {
            let normalized_path = binding.normalized_script_path();
            let enabled = binding.enabled && !is_authored_disabled(&normalized_path);

            if !world.actors.contains_key(&binding.actor_id) {
                if enabled {
                    stale_warnings.push(format!(
                        "Stale Actor Module binding found: Actor ID '{}' does not exist in the world.",
                        binding.actor_id
                    ));
                }
                continue;
            }

            if !binding.is_module() {
                if enabled {
                    isolated_errors.push(format!(
                        "Actor '{}' binding '{}' is not an AeoScript Module (.aeo).",
                        binding.actor_id, binding.script_path
                    ));
                }
                continue;
            }

            actor_module_paths.insert(normalized_path.clone());

            if !loaded_scripts.contains_key(&normalized_path) {
                if enabled && !script_load_failures.contains_key(&normalized_path) {
                    isolated_errors.push(format!(
                        "Bound Actor Module '{}' not found in project script roots.",
                        binding.script_path
                    ));
                }
                continue;
            }

            if !binding.enabled {
                continue;
            }

            let identity = (binding.actor_id, normalized_path.clone());

            if seen_actor_module_bindings.insert(identity.clone()) {
                actor_module_bindings.push(identity);
            }
        }

        let mut validated_bindings = Vec::new();
        for binding in valid_bindings {
            let coord = world
                .resolve_cell_id(binding.target_identity)
                .expect("binding target existence was verified above");
            let cell = world.get(coord).expect("cell existence was verified above");
            let identity = if let Some(ref id) = cell.entity_identity {
                let trimmed = id.trim();
                if !trimmed.is_empty() {
                    trimmed.to_string()
                } else {
                    format!("{:?}", cell.cell_type)
                }
            } else {
                format!("{:?}", cell.cell_type)
            };

            let normalized_binding_path = binding.script_path.replace("\\", "/");

            let Some(program) = loaded_scripts.get(&normalized_binding_path) else {
                if !script_load_failures.contains_key(&normalized_binding_path) {
                    isolated_errors.push(format!(
                        "Bound script '{}' not found in project script roots.",
                        binding.script_path
                    ));
                }
                continue;
            };

            let has_entity = program.declarations.iter().any(|decl| {
                if let Declaration::Entity(entity) = decl {
                    entity.name == identity
                } else {
                    false
                }
            });

            if !has_entity {
                isolated_errors.push(format!(
                    "Script '{}' does not contain an entity declaration for '{}'",
                    binding.script_path, identity
                ));
                continue;
            }

            validated_bindings.push(binding);
        }
        let valid_bindings = validated_bindings;

        let mut all_declarations = Vec::new();
        for program in loaded_scripts.values() {
            all_declarations.extend(program.declarations.clone());
        }

        let combined_program = Program {
            span: SourceSpan::new(0, 0),
            declarations: all_declarations,
            statements: Vec::new(),
        };

        let mut newly_created_ids = Vec::new();

        let spawn_params: Vec<(String, u64, Option<String>, Option<u64>)> = valid_bindings
            .iter()
            .filter(|binding| binding.enabled)
            .map(|binding| {
                let coord = world.resolve_cell_id(binding.target_identity).unwrap();
                let cell = world.get(coord).unwrap();
                let identity = if let Some(ref id) = cell.entity_identity {
                    let trimmed = id.trim();
                    if !trimmed.is_empty() {
                        trimmed.to_string()
                    } else {
                        format!("{:?}", cell.cell_type)
                    }
                } else {
                    format!("{:?}", cell.cell_type)
                };

                let id = if let Some(&existing_id) =
                    preexisting_entities.get(&binding.target_identity)
                {
                    existing_id
                } else {
                    let new_id = entity_manager.create_entity(&identity);
                    entity_manager.set_position(
                        new_id,
                        Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32),
                    );
                    newly_created_ids.push(new_id);
                    new_id
                };

                (
                    identity,
                    id.0,
                    Some(binding.script_path.clone()),
                    Some(binding.target_identity),
                )
            })
            .collect();

        let scene_res = {
            let mut host = HostContext {
                delta_time,
                engine: entity_manager,
            };

            Self::new(combined_program, spawn_params, &mut host)
        };

        if let Err(e) = &scene_res {
            for id in newly_created_ids {
                entity_manager.remove_entity(id);
            }
            return Err(e.clone());
        }

        let mut scene = scene_res.unwrap();
        scene.project_script_index = Some(project_script_index);
        scene.disabled_scripts = world.disabled_scripts.clone();
        scene.script_load_failures = script_load_failures.clone();
        scene.failed_scripts = script_load_failures
            .keys()
            .filter(|path| !is_authored_disabled(path))
            .cloned()
            .collect();
        for path in &scene.failed_scripts {
            if !scene.disabled_scripts.contains(path) {
                scene.disabled_scripts.push(path.clone());
            }
        }
        scene.actor_module_bindings = actor_module_bindings;
        scene.actor_module_paths = actor_module_paths.clone();
        scene.player_module_paths = player_module_paths.clone();
        scene.player_module_path_set = player_module_path_set.clone();
        scene.set_project_hierarchy(project_hierarchy, world);

        scene.runtime.clear_event_handlers();

        for (path, program) in &loaded_scripts {
            if actor_module_paths.contains(path) || player_module_path_set.contains(path) {
                continue;
            }

            scene.runtime.add_event_handlers(path.clone(), program);
        }

        let mut top_level_spawned = HashSet::new();

        for (path, program) in &loaded_scripts {
            if world.disabled_scripts.contains(path) {
                continue;
            }

            if actor_module_paths.contains(path) || player_module_path_set.contains(path) {
                continue;
            }

            top_level_spawned.insert(path.clone());

            if let Err(e) = scene
                .runtime
                .spawn_top_level_custom(path.clone(), &program.statements)
            {
                top_level_spawned.remove(path);
                scene.failed_scripts.insert(path.clone());
                if !scene.disabled_scripts.contains(path) {
                    scene.disabled_scripts.push(path.clone());
                }
                scene.runtime.interpreter_mut().log_error(format!(
                    "Failed to spawn top-level fiber for {}: {}",
                    path, e
                ));
            }
        }

        scene.loaded_scripts = loaded_scripts.clone();
        scene.runtime.interpreter_mut().loaded_scripts = loaded_scripts.clone();
        scene
            .runtime
            .interpreter_mut()
            .set_player_module_paths(player_module_paths.clone());
        scene.top_level_spawned = top_level_spawned;

        let bound_entities: HashSet<(String, String)> = valid_bindings
            .iter()
            .map(|binding| {
                let coord = world.resolve_cell_id(binding.target_identity).unwrap();
                let cell = world.get(coord).unwrap();

                let identity = cell
                    .entity_identity
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", cell.cell_type));

                let script_path = binding.script_path.replace('\\', "/");

                (script_path, identity)
            })
            .collect();

        for (path, program) in &loaded_scripts {
            if scene.disabled_scripts.contains(path) {
                continue;
            }
            if actor_module_paths.contains(path) {
                for decl in &program.declarations {
                    if let Declaration::Entity(entity) = decl {
                        let hooks = entity
                            .members
                            .iter()
                            .filter_map(|member| match member {
                                EntityMember::Function(function) => Some(function.name.as_str()),
                                _ => None,
                            })
                            .collect::<Vec<_>>();

                        if !hooks.is_empty() {
                            scene.runtime.interpreter_mut().log_warning(format!(
                                "Actor Module '{}' contains legacy entity '{}' methods ({}). Actor Modules do not bind entity blocks, so these methods will not run. Capture the attached Actor and assign handlers to it instead, for example: const actor = self.parent; actor.on_touch = fn(other) {{ ... }}",
                                path,
                                entity.name,
                                hooks.join(", ")
                            ));
                        }
                    }
                }
                continue;
            }

            for decl in &program.declarations {
                if let Declaration::Entity(entity) = decl {
                    let hooks: Vec<&str> = entity
                        .members
                        .iter()
                        .filter_map(|m| {
                            if let EntityMember::Function(f) = m {
                                if matches!(
                                    f.name.as_str(),
                                    "on_spawn" | "on_ready" | "update" | "on_destroy"
                                ) {
                                    return Some(f.name.as_str());
                                }
                            }
                            None
                        })
                        .collect();

                    if !hooks.is_empty()
                        && !path.starts_with("controllers/")
                        && !path.starts_with("cameras/")
                        && !path.starts_with("playerscripts/")
                        && !bound_entities.contains(&(path.clone(), entity.name.clone()))
                    {
                        let warning = format!(
                            "Script '{}' declares lifecycle hooks for entity '{}', but no script binding exists for that entity. Hooks: {}",
                            path,
                            entity.name,
                            hooks.join(", ")
                        );
                        scene.runtime.interpreter_mut().log_warning(warning);
                    }
                }
            }
        }

        for warning in stale_warnings {
            scene.runtime.interpreter_mut().log_warning(warning);
        }
        for error in isolated_errors {
            scene.runtime.interpreter_mut().log_error(error);
        }
        for (path, error) in &script_load_failures {
            if !is_authored_disabled(path) {
                scene.runtime.interpreter_mut().log_error(error.clone());
            }
        }

        Ok(scene)
    }

    pub fn set_project_hierarchy(&mut self, hierarchy: ProjectHierarchy, world: &World) {
        let actor_names = world
            .actors
            .iter()
            .map(|(actor_id, actor)| {
                let name = if actor.identity.trim().is_empty() {
                    format!("Actor_{}", actor_id)
                } else {
                    actor.identity.clone()
                };

                (*actor_id, name)
            })
            .collect();

        self.runtime
            .interpreter_mut()
            .set_project_scope(hierarchy, actor_names);
    }

    pub fn new(
        program: Program,
        entities_to_spawn: Vec<(String, u64, Option<String>, Option<u64>)>,
        host: &mut HostContext,
    ) -> Result<Self, String> {
        Self::new_with_path(program, entities_to_spawn, None, host)
    }

    pub fn new_with_path(
        program: Program,
        entities_to_spawn: Vec<(String, u64, Option<String>, Option<u64>)>,
        top_level_path: Option<String>,
        host: &mut HostContext,
    ) -> Result<Self, String> {
        let has_top_level = !program.statements.is_empty();
        let mut runtime = ScriptRuntime::new(program);
        let mut entities = Vec::with_capacity(entities_to_spawn.len());

        for (name, id, script_path, cell_id) in entities_to_spawn {
            let mut instance = runtime
                .interpreter_mut()
                .instantiate_entity(&name, id, host)
                .map_err(|e| format!("Failed to instantiate script entity '{}': {}", name, e))?;
            instance.script_path = script_path.clone();
            entities.push(ScriptEntity::new(instance, script_path, cell_id));
        }

        let valid_entity_declarations_set: HashSet<String> = runtime
            .interpreter()
            .program()
            .declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Entity(e) = decl {
                    Some(e.name.clone())
                } else {
                    None
                }
            })
            .collect();

        let valid_decls_arc = Arc::new(valid_entity_declarations_set);
        host.engine
            .set_valid_entity_declarations(valid_decls_arc.clone());

        if has_top_level {
            let statements = runtime.interpreter().program().statements.clone();
            let path = top_level_path.unwrap_or_else(|| "UNKNOWN".to_string());
            runtime.spawn_top_level_custom(path, &statements)?;
        }

        Ok(Self {
            runtime,
            entities,
            disabled_scripts: Vec::new(),
            loaded_scripts: HashMap::new(),
            top_level_spawned: HashSet::new(),
            valid_entity_declarations: valid_decls_arc,
            pending_spawns: Vec::new(),
            project_script_index: None,
            actor_module_bindings: Vec::new(),
            actor_module_paths: HashSet::new(),
            failed_attached_modules: HashSet::new(),
            script_load_failures: HashMap::new(),
            failed_scripts: HashSet::new(),
            player_module_paths: Vec::new(),
            player_module_path_set: HashSet::new(),
            dynamic_handler_owners: HashMap::new(),
            current_time: 0.0,
        })
    }

    pub fn find_project_module(&self, path: &str) -> Option<&IndexedModule> {
        self.project_script_index
            .as_ref()
            .and_then(|index| index.find_module(path))
    }

    pub fn find_project_modules_named(&self, name: &str) -> Vec<&IndexedModule> {
        self.project_script_index
            .as_ref()
            .map(|index| index.find_modules_named(name))
            .unwrap_or_default()
    }

    pub fn project_module_path(&self, path: &str) -> Option<std::path::PathBuf> {
        let index = self.project_script_index.as_ref()?;
        if let Some(relative) = path.replace('\\', "/").strip_prefix("profile/scripts/") {
            let relative = Path::new(relative);
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|component| !matches!(component, std::path::Component::Normal(_)))
            {
                return None;
            }
            return Some(index.project_root.parent()?.join("scripts").join(relative));
        }
        let module = index.find_module(path)?;
        Some(index.absolute_path(module))
    }

    fn runtime_failure_location(&self, task_id: ScriptTaskId) -> Option<String> {
        let fiber = self.runtime.fiber(task_id)?;
        let (path, function, span) = fiber.last_execution_location()?;
        let path = path?;
        let position = span.and_then(|span| {
            let source = std::fs::read_to_string(self.project_module_path(path)?).ok()?;
            ((span.start as usize) <= source.len())
                .then(|| super::source_map::SourceMap::new(&source).position(span.start))
        });

        Some(match position {
            Some(position) => format!("{path}:{}:{} | {function}", position.line, position.column),
            None => format!("{path} | {function}"),
        })
    }

    fn runtime_failure_script_path(&self, task_id: ScriptTaskId) -> Option<String> {
        let fiber = self.runtime.fiber(task_id)?;
        fiber
            .last_execution_location()
            .and_then(|(path, _, _)| path.map(|path| path.replace('\\', "/")))
            .or_else(|| {
                fiber
                    .script_path
                    .as_ref()
                    .map(|path| path.replace('\\', "/"))
            })
    }

    fn annotate_runtime_failure(&mut self, task_id: ScriptTaskId, error: &str) {
        let Some(location) = self.runtime_failure_location(task_id) else {
            return;
        };
        if let Some(record) =
            self.runtime
                .interpreter_mut()
                .output
                .iter_mut()
                .rev()
                .find(|record| {
                    record.severity == super::log::LogSeverity::Error && record.message == error
                })
        {
            record.message = format!("{} at {}", record.message, location);
        }
    }

    fn quarantine_failed_script(&mut self, path: &str, host: &mut HostContext) {
        let path = path.replace('\\', "/");
        self.failed_scripts.insert(path.clone());
        self.disable_script(&path, host);
    }

    fn quarantine_event_dispatch_failures(&mut self, host: &mut HostContext) {
        let failures = self.runtime.drain_event_dispatch_failures();
        for (path, _) in failures {
            if let Some(path) = path {
                self.quarantine_failed_script(&path, host);
            }
        }
    }

    pub fn start(&mut self, host: &mut HostContext) -> Result<(), String> {
        if !self.actor_module_paths.is_empty() && !self.top_level_spawned.is_empty() {
            let results = self.runtime.tick(0.0, host)?;
            let mut failures = Vec::new();
            for (task_id, result) in results {
                match result {
                    FiberResult::Complete => {
                        self.runtime.remove_fiber(task_id)?;
                    }
                    FiberResult::Failed(error) => {
                        let path = self.runtime_failure_script_path(task_id);
                        self.annotate_runtime_failure(task_id, &error);
                        let _ = self.runtime.remove_fiber(task_id);
                        if let Some(path) = path {
                            failures.push(path);
                        }
                    }
                    _ => {}
                }
            }
            for path in failures {
                self.quarantine_failed_script(&path, host);
            }
        }
        self.ensure_attached_module_instances(host)?;
        self.runtime.adopt_pending_module_fibers()?;

        let mut lifecycle_errors = Vec::new();
        for entity in &mut self.entities {
            if let Err(error) = Self::transition_entity(&mut self.runtime, entity, host) {
                let identity = entity.instance.entity_name().to_string();
                let script = entity
                    .script_path
                    .as_deref()
                    .unwrap_or("unknown script")
                    .to_string();
                entity.state = EntityState::Stopped;
                entity.stopped = true;
                lifecycle_errors.push(format!(
                    "Script '{}' failed to start entity '{}'; only this script instance was stopped: {}",
                    script, identity, error
                ));
            }
        }

        for error in lifecycle_errors {
            self.runtime.interpreter_mut().log_error(error);
        }

        Ok(())
    }

    pub fn normalize_script_path(path: &str) -> String {
        let clean = path.replace('\\', "/");

        if clean.starts_with("scripts/")
            || clean.starts_with("playerscripts/")
            || clean.starts_with("profile/scripts/")
            || clean.starts_with("controllers/")
            || clean.starts_with("cameras/")
        {
            clean
        } else {
            format!("scripts/{}", clean)
        }
    }

    pub fn enable_script(&mut self, path: &str) {
        let norm = Self::normalize_script_path(path);
        self.disabled_scripts.retain(|s| s != &norm);
        self.failed_scripts.remove(&norm);
        self.failed_attached_modules
            .retain(|(_, script_path)| script_path != &norm);
        eprintln!("[RUNTIME] script enabled: {}", norm);

        if let Some(error) = self.script_load_failures.get(&norm).cloned() {
            self.failed_scripts.insert(norm.clone());
            self.disabled_scripts.push(norm);
            self.runtime.interpreter_mut().log_error(error);
            return;
        }

        if self.player_module_path_set.contains(&norm) || self.actor_module_paths.contains(&norm) {
            return;
        }

        if !self.top_level_spawned.contains(&norm) {
            if let Some(program) = self.loaded_scripts.get(&norm).cloned() {
                self.top_level_spawned.insert(norm.clone());
                if let Err(e) = self
                    .runtime
                    .spawn_top_level_custom(norm.clone(), &program.statements)
                {
                    self.runtime.interpreter_mut().log_error(format!(
                        "Failed to spawn top-level fiber for {}: {}",
                        norm, e
                    ));
                }
            }
        }
    }

    pub fn disable_script(&mut self, path: &str, host: &mut HostContext) {
        let norm = Self::normalize_script_path(path);
        if !self.disabled_scripts.contains(&norm) {
            self.disabled_scripts.push(norm.clone());
        }
        self.top_level_spawned.remove(&norm);
        host.engine.remove_dynamic_properties_by_owner(&norm);
        self.runtime
            .interpreter_mut()
            .cancel_observers_for_script(&norm);
        self.runtime.cancel_fibers_for_script(&norm);
        self.runtime
            .interpreter_mut()
            .evict_module_instances_for_script(&norm);
    }

    pub fn sync_script_states(&mut self, host: &mut HostContext) {
        let enabled = host.engine.drain_enabled_scripts();
        for path in enabled {
            self.enable_script(&path);
        }

        let disabled = host.engine.drain_disabled_scripts();
        for path in disabled {
            self.disable_script(&path, host);
        }
    }

    pub fn instantiate_script_object(
        &mut self,
        entity_name: &str,
        args: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        use super::ast::{Expression, ExpressionKind};
        use super::source::SourceSpan;
        use super::value::Scope;

        let callee = Expression {
            kind: ExpressionKind::Identifier(entity_name.to_string()),
            span: SourceSpan::new(0, 0),
        };
        let arg_exprs: Vec<Expression> = args
            .into_iter()
            .map(|val| Expression {
                kind: ExpressionKind::Literal(val),
                span: SourceSpan::new(0, 0),
            })
            .collect();
        let mut instance = ScriptInstance::new_empty();
        let mut scopes = vec![Scope::new()];
        self.runtime.interpreter_mut().eval_call_opt_fiber(
            &mut instance,
            &mut scopes,
            SourceSpan::new(0, 0),
            &callee,
            &arg_exprs,
            host,
            &[],
            None,
        )
    }

    pub fn instantiate_package_object(
        &mut self,
        category: &str,
        package_name: &str,
        args: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let prefix = format!("{}/{}", category, package_name);

        let mut paths: Vec<String> = self
            .loaded_scripts
            .keys()
            .filter(|path| path.starts_with(&(prefix.clone() + "/")))
            .cloned()
            .collect();

        paths.sort();

        for path in paths {
            let entity_name = self.loaded_scripts.get(&path).and_then(|program| {
                program.declarations.iter().find_map(|decl| {
                    if let Declaration::Entity(entity) = decl {
                        Some(entity.name.clone())
                    } else {
                        None
                    }
                })
            });

            if let Some(entity_name) = entity_name {
                return self.instantiate_script_object(&entity_name, args, host);
            }
        }

        Err(format!(
            "Package '{}' '{}' has no entity declaration",
            category, package_name
        ))
    }

    pub fn call_object_method(
        &mut self,
        object: &Value,
        method_name: &str,
        args: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        self.runtime
            .interpreter_mut()
            .call_object_method_direct(object, method_name, args, host)
    }

    pub fn set_fiber_profiling(&mut self, enabled: bool) {
        self.runtime.set_fiber_profiling(enabled);
    }

    pub fn take_fiber_timings(&mut self) -> Vec<(String, std::time::Duration)> {
        self.runtime.take_fiber_timings()
    }

    pub fn update(&mut self, dt: f32, host: &mut HostContext) -> Result<(), String> {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(40, file!(), line!());

        host.engine.tick_navigation_builds();
        self.current_time += dt as f64;
        for error in host.engine.tween_tick(dt as f64) {
            crate::engine::diagnostics::report("ERROR", "AeoScript Tween", error);
        }
        let mut destroy_errors = Vec::new();

        self.sync_script_states(host);
        self.quarantine_event_dispatch_failures(host);
        self.ensure_attached_module_instances(host)?;
        self.cleanup_deleted_cell_scripts(host, &mut destroy_errors);
        self.dynamic_handler_owners
            .retain(|task_id, _| self.runtime.fiber(*task_id).is_some());

        let pending = host.engine.drain_pending_events();
        for (event_name, args) in pending {
            self.dispatch_pending_event(&event_name, args, host)?;
        }

        let clicked_ui_ids = host.engine.drain_ui_clicks();
        for ui_id in clicked_ui_ids {
            if let Ok(Some(Value::Function(compiled, params, captures))) =
                host.engine.get_ui_property(ui_id, "on_click")
            {
                self.runtime.spawn_callable(
                    compiled,
                    params,
                    vec![],
                    "on_click".to_string(),
                    captures,
                )?;
                continue;
            }

            if let Some(dispatch) = host
                .engine
                .get_ui_symbolic_event_dispatch(ui_id, "on_click")
            {
                self.dispatch_authored_ui_event(dispatch)?;
            }
        }
        self.quarantine_event_dispatch_failures(host);

        let pending_destroys = host.engine.drain_pending_destroys();
        for raw_entity_id in pending_destroys {
            if let Err(error) = self.destroy_entity(EntityId(raw_entity_id), host) {
                destroy_errors.push(error);
            }
        }

        let mut pending_spawns = host.engine.drain_pending_spawns();
        pending_spawns.extend(std::mem::take(&mut self.pending_spawns));
        for (entity_name, raw_entity_id) in pending_spawns {
            let has_decl = self
                .runtime
                .interpreter()
                .program()
                .declarations
                .iter()
                .any(|decl| {
                    if let Declaration::Entity(e) = decl {
                        e.name == entity_name
                    } else {
                        false
                    }
                });

            if has_decl {
                let instance = self.runtime.interpreter_mut().instantiate_entity(
                    &entity_name,
                    raw_entity_id,
                    host,
                )?;
                let mut script_entity = ScriptEntity::new(instance, None, None);
                Self::transition_entity(&mut self.runtime, &mut script_entity, host)?;
                self.entities.push(script_entity);
            }
        }

        let tick_results = self.runtime.tick(self.current_time, host)?;

        for (callback, old, new) in self.runtime.interpreter_mut().poll_observers(host) {
            if let Value::Function(compiled, params, captures) = callback {
                self.runtime.spawn_callable(
                    compiled,
                    params,
                    vec![old, new],
                    "property_changed".to_string(),
                    captures,
                )?;
            }
        }

        let canceled_cell_tasks = self.cleanup_deleted_cell_scripts(host, &mut destroy_errors);
        let tick_results = tick_results
            .into_iter()
            .filter(|(task_id, _)| !canceled_cell_tasks.contains(task_id))
            .collect::<Vec<_>>();

        let pending_destroys_post = host.engine.drain_pending_destroys();
        for raw_entity_id in pending_destroys_post {
            if let Err(error) = self.destroy_entity(EntityId(raw_entity_id), host) {
                destroy_errors.push(error);
            }
        }

        let mut pending_spawns_post = host.engine.drain_pending_spawns();
        pending_spawns_post.extend(std::mem::take(&mut self.pending_spawns));
        for (entity_name, raw_entity_id) in pending_spawns_post {
            let has_decl = self
                .runtime
                .interpreter()
                .program()
                .declarations
                .iter()
                .any(|decl| {
                    if let Declaration::Entity(e) = decl {
                        e.name == entity_name
                    } else {
                        false
                    }
                });

            if has_decl {
                let instance = self.runtime.interpreter_mut().instantiate_entity(
                    &entity_name,
                    raw_entity_id,
                    host,
                )?;
                let mut script_entity = ScriptEntity::new(instance, None, None);
                Self::transition_entity(&mut self.runtime, &mut script_entity, host)?;
                self.entities.push(script_entity);
            }
        }

        let mut isolated_runtime_failures = Vec::new();
        for (task_id, result) in tick_results {
            let failure_script_path = if matches!(result, FiberResult::Failed(_)) {
                self.runtime_failure_script_path(task_id)
            } else {
                None
            };
            if let FiberResult::Failed(error) = &result {
                self.annotate_runtime_failure(task_id, error);
            }
            if matches!(&result, FiberResult::Complete | FiberResult::Failed(_)) {
                self.dynamic_handler_owners.remove(&task_id);
            }

            let mut handled = false;
            for entity in &mut self.entities {
                if entity.active_task() == Some(task_id) {
                    Self::sync_entity_instance_from_task(&self.runtime, entity, task_id)?;

                    match result {
                        FiberResult::Complete => {
                            self.runtime.remove_fiber(task_id)?;
                            entity.complete_task(task_id);
                            if let Err(error) =
                                Self::transition_entity(&mut self.runtime, entity, host)
                            {
                                let path = entity.script_path.clone();
                                entity.state = EntityState::Stopped;
                                entity.stopped = true;
                                self.runtime.interpreter_mut().log_error(format!(
                                    "Script '{}' failed during entity '{}' lifecycle transition: {}",
                                    path.as_deref().unwrap_or("unknown script"),
                                    entity.instance.entity_name(),
                                    error
                                ));
                                if let Some(path) = path {
                                    isolated_runtime_failures.push(path);
                                }
                            }
                        }
                        FiberResult::Failed(_) => {
                            let _ = self.runtime.remove_fiber(task_id);
                            entity.state = EntityState::Stopped;
                            entity.stopped = true;
                            if let Some(path) = failure_script_path.clone() {
                                isolated_runtime_failures.push(path);
                            }
                        }
                        _ => {}
                    }
                    handled = true;
                    break;
                }
            }

            if !handled {
                if matches!(result, FiberResult::Complete) {
                    if let Some(updated) = self
                        .runtime
                        .fiber(task_id)
                        .map(|fiber| fiber.instance().clone())
                    {
                        if let Some(entity) = self.entities.iter_mut().find(|entity| {
                            !entity.stopped
                                && entity.instance.id() == updated.id()
                                && entity.instance.id() != 0
                        }) {
                            entity.instance = updated;
                        }
                    }
                }
                match result {
                    FiberResult::Complete => {
                        self.runtime.remove_fiber(task_id)?;
                    }
                    FiberResult::Failed(_) => {
                        let _ = self.runtime.remove_fiber(task_id);
                        if let Some(path) = failure_script_path {
                            isolated_runtime_failures.push(path);
                        }
                    }
                    _ => {}
                }
            }
        }

        for path in isolated_runtime_failures {
            self.quarantine_failed_script(&path, host);
        }

        let mut update_spawn_failures = Vec::new();
        for entity in &mut self.entities {
            if let EntityState::Active { update_task: None } = entity.state {
                if !entity.stopped && Self::has_function(&self.runtime, entity, "update") {
                    match self.runtime.spawn(
                        entity.instance.clone(),
                        "update",
                        vec![Value::Number(dt as f64)],
                    ) {
                        Ok(task_id) => {
                            entity.state = EntityState::Active {
                                update_task: Some(task_id),
                            };
                        }
                        Err(error) => {
                            let path = entity.script_path.clone();
                            entity.state = EntityState::Stopped;
                            entity.stopped = true;
                            self.runtime.interpreter_mut().log_error(format!(
                                "Script '{}' failed to start update for entity '{}': {}",
                                path.as_deref().unwrap_or("unknown script"),
                                entity.instance.entity_name(),
                                error
                            ));
                            if let Some(path) = path {
                                update_spawn_failures.push(path);
                            }
                        }
                    }
                }
            }
        }
        for path in update_spawn_failures {
            self.quarantine_failed_script(&path, host);
        }

        if destroy_errors.is_empty() {
            Ok(())
        } else {
            Err(destroy_errors.join("\n"))
        }
    }

    fn ensure_attached_module_instances(&mut self, host: &mut HostContext) -> Result<(), String> {
        for (actor_id, script_path) in self.actor_module_bindings.clone() {
            if !host.engine.is_script_enabled(&script_path) {
                continue;
            }

            let failure_key = (actor_id, script_path.clone());
            if self.failed_attached_modules.contains(&failure_key) {
                continue;
            }

            if let Err(error) = self
                .runtime
                .interpreter_mut()
                .get_or_create_attached_module_instance(&script_path, actor_id, host)
            {
                self.failed_attached_modules.insert(failure_key);
                self.runtime.interpreter_mut().log_error(format!(
                    "Module '{}' failed to start for Actor {} and was stopped for this Play session; other scripts will continue: {}",
                    script_path, actor_id, error
                ));
            }
        }

        if let Some(player_actor_id) = host.engine.get_player_actor_id() {
            for script_path in self.player_module_paths.clone() {
                if !host.engine.is_script_enabled(&script_path) {
                    continue;
                }

                let failure_key = (player_actor_id, script_path.clone());
                if self.failed_attached_modules.contains(&failure_key) {
                    continue;
                }

                if let Err(error) = self
                    .runtime
                    .interpreter_mut()
                    .get_or_create_attached_module_instance(&script_path, player_actor_id, host)
                {
                    self.failed_attached_modules.insert(failure_key);
                    self.runtime.interpreter_mut().log_error(format!(
                        "Player Module '{}' failed to start for Actor {} and was stopped for this Play session; other scripts will continue: {}",
                        script_path, player_actor_id, error
                    ));
                }
            }
        }

        Ok(())
    }

    fn dispatch_pending_event(
        &mut self,
        event_name: &str,
        args: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<(), String> {
        self.dispatch_event(event_name, args.clone(), host)?;

        if let Some(Value::Handle {
            kind: HandleKind::Actor,
            id: actor_id,
        }) = args.first()
        {
            let dynamic_event = match event_name {
                "navigation_finished" => Some(("on_navigation_finished", Vec::new())),
                "navigation_failed" => Some((
                    "on_navigation_failed",
                    args.get(1).cloned().into_iter().collect(),
                )),
                _ => None,
            };
            if let Some((callback_name, callback_args)) = dynamic_event {
                self.spawn_dynamic_handler(
                    HandleKind::Actor,
                    *actor_id,
                    callback_name,
                    callback_args,
                    Some(Value::Handle {
                        kind: HandleKind::Actor,
                        id: *actor_id,
                    }),
                    host,
                )?;
            }
        }

        Ok(())
    }

    pub fn output(&self) -> &[LogRecord] {
        self.runtime.interpreter().output()
    }

    pub fn drain_output(&mut self) -> Vec<LogRecord> {
        self.runtime.interpreter_mut().drain_output()
    }

    pub fn get_entity_by_id(&self, entity_id: EntityId) -> Option<&ScriptEntity> {
        self.entities
            .iter()
            .find(|e| e.instance.id() == entity_id.0)
    }

    pub fn get_entity_mut_by_id(&mut self, entity_id: EntityId) -> Option<&mut ScriptEntity> {
        self.entities
            .iter_mut()
            .find(|e| e.instance.id() == entity_id.0)
    }

    pub fn on_entity_contacts(
        &mut self,
        contact_result: &crate::engine::entity::EntityContactResult,
        player_entity_id: Option<EntityId>,
        host: &mut HostContext,
    ) -> Result<(), String> {
        let mut pairs: Vec<_> = contact_result.entered.iter().copied().collect();
        pairs.sort_unstable_by_key(|p| (p.first.0, p.second.0));

        for pair in pairs {
            let handle_a = Value::Handle {
                kind: crate::scripting::value::HandleKind::Entity,
                id: pair.first.0,
            };
            let handle_b = Value::Handle {
                kind: crate::scripting::value::HandleKind::Entity,
                id: pair.second.0,
            };

            self.spawn_dynamic_handler(
                crate::scripting::value::HandleKind::Entity,
                pair.first.0,
                "on_touch",
                vec![handle_b.clone()],
                Some(handle_a.clone()),
                host,
            )?;

            if let Some((crate::scripting::value::HandleKind::Actor, actor_id)) = host
                .engine
                .get_parent(crate::scripting::value::HandleKind::Entity, pair.first.0)
            {
                self.spawn_dynamic_handler(
                    crate::scripting::value::HandleKind::Actor,
                    actor_id,
                    "on_touch",
                    vec![handle_b.clone()],
                    Some(Value::Handle {
                        kind: crate::scripting::value::HandleKind::Actor,
                        id: actor_id,
                    }),
                    host,
                )?;
            }

            for entity in &mut self.entities {
                if entity.instance.id() == pair.first.0
                    && !entity.stopped
                    && entity
                        .script_path
                        .as_ref()
                        .is_none_or(|path| !self.disabled_scripts.contains(path))
                {
                    if Self::has_function(&self.runtime, entity, "on_touch") {
                        self.runtime.spawn(
                            entity.instance.clone(),
                            "on_touch",
                            vec![handle_b.clone()],
                        )?;
                    }
                }
            }

            self.spawn_dynamic_handler(
                crate::scripting::value::HandleKind::Entity,
                pair.second.0,
                "on_touch",
                vec![handle_a.clone()],
                Some(handle_b.clone()),
                host,
            )?;

            if let Some((crate::scripting::value::HandleKind::Actor, actor_id)) = host
                .engine
                .get_parent(crate::scripting::value::HandleKind::Entity, pair.second.0)
            {
                self.spawn_dynamic_handler(
                    crate::scripting::value::HandleKind::Actor,
                    actor_id,
                    "on_touch",
                    vec![handle_a.clone()],
                    Some(Value::Handle {
                        kind: crate::scripting::value::HandleKind::Actor,
                        id: actor_id,
                    }),
                    host,
                )?;
            }

            for entity in &mut self.entities {
                if entity.instance.id() == pair.second.0
                    && !entity.stopped
                    && entity
                        .script_path
                        .as_ref()
                        .is_none_or(|path| !self.disabled_scripts.contains(path))
                {
                    if Self::has_function(&self.runtime, entity, "on_touch") {
                        self.runtime.spawn(
                            entity.instance.clone(),
                            "on_touch",
                            vec![handle_a.clone()],
                        )?;
                    }
                }
            }
        }

        let general_contact = self
            .runtime
            .has_active_event_handler("entity_contact", &self.disabled_scripts);
        let player_contact = self
            .runtime
            .has_active_event_handler("player_entity_contact", &self.disabled_scripts)
            && player_entity_id.is_some();
        if general_contact || player_contact {
            for pair in &contact_result.current {
                if general_contact {
                    self.dispatch_event(
                        "entity_contact",
                        vec![
                            Value::Handle {
                                kind: crate::scripting::value::HandleKind::Entity,
                                id: pair.first.0,
                            },
                            Value::Handle {
                                kind: crate::scripting::value::HandleKind::Entity,
                                id: pair.second.0,
                            },
                        ],
                        host,
                    )?;
                }
                if player_contact {
                    let other_id = if Some(pair.first) == player_entity_id {
                        Some(pair.second.0)
                    } else if Some(pair.second) == player_entity_id {
                        Some(pair.first.0)
                    } else {
                        None
                    };
                    if let Some(other_id) = other_id {
                        self.dispatch_event(
                            "player_entity_contact",
                            vec![Value::Handle {
                                kind: crate::scripting::value::HandleKind::Entity,
                                id: other_id,
                            }],
                            host,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    pub fn destroy_entity(
        &mut self,
        entity_id: EntityId,
        host: &mut HostContext,
    ) -> Result<bool, String> {
        let pos = self
            .entities
            .iter()
            .position(|e| e.instance.id() == entity_id.0);
        let mut errors = Vec::new();

        if let Some(index) = pos {
            let mut entity = self.entities.remove(index);
            if !entity.stopped {
                entity.stopped = true;
                if let Err(err) = self
                    .runtime
                    .cancel_fibers_for_instance(entity.instance.id())
                {
                    errors.push(format!(
                        "Failed to cancel tasks for entity {}: {}",
                        entity.instance.entity_name(),
                        err
                    ));
                }

                if Self::has_function(&self.runtime, &entity, "on_destroy") {
                    let mut instance = entity.instance.clone();
                    if let Err(err) = self.runtime.interpreter_mut().call(
                        &mut instance,
                        "on_destroy",
                        vec![],
                        host,
                    ) {
                        errors.push(format!(
                            "Error in on_destroy() for entity {}: {}",
                            entity.instance.entity_name(),
                            err
                        ));
                    }
                }

                entity.state = EntityState::Stopped;
            }
        }

        host.engine.destroy_entity_runtime(entity_id.0);
        if errors.is_empty() {
            Ok(true)
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn on_player_contact(
        &mut self,
        contacted_cells: &HashSet<u64>,
        host: &mut HostContext,
    ) -> Result<(), String> {
        let mut cell_ids = contacted_cells.iter().copied().collect::<Vec<_>>();
        cell_ids.sort_unstable();

        for cell_id in cell_ids {
            if !host.engine.is_collision_events_enabled(cell_id) {
                continue;
            }

            let cell_handle = Value::Handle {
                kind: crate::scripting::value::HandleKind::Cell,
                id: cell_id,
            };

            self.dispatch_event("on_touch", vec![cell_handle.clone()], host)?;

            self.spawn_dynamic_handler(
                crate::scripting::value::HandleKind::Cell,
                cell_id,
                "on_touch",
                vec![cell_handle.clone()],
                Some(cell_handle.clone()),
                host,
            )?;

            for entity in &mut self.entities {
                if entity.associated_cell_id == Some(cell_id)
                    && !entity.stopped
                    && entity
                        .script_path
                        .as_ref()
                        .is_none_or(|path| !self.disabled_scripts.contains(path))
                {
                    if Self::has_function(&self.runtime, entity, "on_touch") {
                        self.runtime.spawn(
                            entity.instance.clone(),
                            "on_touch",
                            vec![cell_handle.clone()],
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn on_character_contact(
        &mut self,
        contacts: &HashSet<(EntityId, u64)>,
        host: &mut HostContext,
    ) -> Result<(), String> {
        let mut contacts = contacts.iter().copied().collect::<Vec<_>>();
        contacts.sort_unstable_by_key(|(entity_id, cell_id)| (*cell_id, entity_id.0));

        for (entity_id, cell_id) in contacts {
            if !host.engine.is_collision_events_enabled(cell_id) {
                continue;
            }

            let cell_handle = Value::Handle {
                kind: crate::scripting::value::HandleKind::Cell,
                id: cell_id,
            };

            let touching_entity_handle = Value::Handle {
                kind: crate::scripting::value::HandleKind::Entity,
                id: entity_id.0,
            };

            self.dispatch_event("on_touch", vec![touching_entity_handle.clone()], host)?;

            self.spawn_dynamic_handler(
                crate::scripting::value::HandleKind::Cell,
                cell_id,
                "on_touch",
                vec![touching_entity_handle.clone()],
                Some(cell_handle.clone()),
                host,
            )?;

            for entity in &mut self.entities {
                if entity.associated_cell_id == Some(cell_id)
                    && !entity.stopped
                    && entity
                        .script_path
                        .as_ref()
                        .is_none_or(|path| !self.disabled_scripts.contains(path))
                {
                    if Self::has_function(&self.runtime, entity, "on_touch") {
                        self.runtime.spawn(
                            entity.instance.clone(),
                            "on_touch",
                            vec![touching_entity_handle.clone()],
                        )?;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn on_player_overlap(
        &mut self,
        cell_id: u64,
        overlapping: bool,
        host: &mut HostContext,
    ) -> Result<(), String> {
        if !host.engine.is_collision_events_enabled(cell_id) {
            return Ok(());
        }

        let cell_handle = Value::Handle {
            kind: crate::scripting::value::HandleKind::Cell,
            id: cell_id,
        };
        let overlapping_value = Value::Bool(overlapping);

        self.dispatch_event(
            "on_overlap",
            vec![overlapping_value.clone(), cell_handle.clone()],
            host,
        )?;

        self.spawn_dynamic_handler(
            crate::scripting::value::HandleKind::Cell,
            cell_id,
            "on_overlap",
            vec![overlapping_value.clone(), cell_handle.clone()],
            Some(cell_handle.clone()),
            host,
        )?;

        for entity in &mut self.entities {
            if entity.associated_cell_id == Some(cell_id)
                && !entity.stopped
                && entity
                    .script_path
                    .as_ref()
                    .is_none_or(|path| !self.disabled_scripts.contains(path))
            {
                if Self::has_function(&self.runtime, entity, "on_overlap") {
                    self.runtime.spawn(
                        entity.instance.clone(),
                        "on_overlap",
                        vec![overlapping_value.clone(), cell_handle.clone()],
                    )?;
                }
            }
        }

        Ok(())
    }

    fn spawn_dynamic_handler(
        &mut self,
        kind: crate::scripting::value::HandleKind,
        id: u64,
        name: &str,
        arguments: Vec<Value>,
        self_value: Option<Value>,
        host: &mut HostContext,
    ) -> Result<(), String> {
        let Some(DynamicProperty {
            value,
            owner_script,
        }) = host.engine.get_script_property(kind, id, name)
        else {
            return Ok(());
        };

        let Value::Function(compiled, params, captured_scopes) = value else {
            return Ok(());
        };

        let mut instance = ScriptInstance::new_empty();
        if let Some(owner) = owner_script {
            instance = instance.with_path(owner);
        }

        let fiber = self.runtime.interpreter_mut().start_direct_fiber(
            instance,
            compiled,
            params,
            arguments,
            self_value,
            name.to_string(),
            captured_scopes,
        )?;

        let task_id = self.runtime.scheduler_mut().spawn();
        self.runtime.fibers_mut().insert(task_id, fiber);
        self.dynamic_handler_owners.insert(task_id, (kind, id));

        Ok(())
    }

    fn cleanup_deleted_cell_scripts(
        &mut self,
        host: &mut HostContext,
        errors: &mut Vec<String>,
    ) -> HashSet<ScriptTaskId> {
        let mut deleted_cell_ids = self
            .entities
            .iter()
            .filter_map(|entity| entity.associated_cell_id)
            .filter(|cell_id| !host.engine.cell_exists(*cell_id))
            .collect::<HashSet<_>>();

        deleted_cell_ids.extend(
            self.dynamic_handler_owners
                .values()
                .filter_map(|(kind, cell_id)| {
                    matches!(kind, HandleKind::Cell | HandleKind::Light).then_some(*cell_id)
                })
                .filter(|cell_id| !host.engine.cell_exists(*cell_id)),
        );

        let mut canceled_tasks = HashSet::new();
        for cell_id in deleted_cell_ids {
            let mut index = 0;
            while index < self.entities.len() {
                if self.entities[index].associated_cell_id != Some(cell_id) {
                    index += 1;
                    continue;
                }

                let mut entity = self.entities.remove(index);
                canceled_tasks.extend(self.runtime.fibers().iter().filter_map(
                    |(task_id, fiber)| {
                        fiber
                            .involves_instance(entity.instance.id())
                            .then_some(*task_id)
                    },
                ));
                if let Err(error) = self
                    .runtime
                    .cancel_fibers_for_instance(entity.instance.id())
                {
                    errors.push(format!(
                        "Failed to cancel tasks for Cell script {} (Cell {}): {}",
                        entity.instance.entity_name(),
                        cell_id,
                        error
                    ));
                }

                entity.stopped = true;
                if Self::has_function(&self.runtime, &entity, "on_destroy") {
                    let mut instance = entity.instance.clone();
                    if let Err(error) = self.runtime.interpreter_mut().call(
                        &mut instance,
                        "on_destroy",
                        vec![],
                        host,
                    ) {
                        errors.push(format!(
                            "Error in on_destroy() for Cell script {} (Cell {}): {}",
                            entity.instance.entity_name(),
                            cell_id,
                            error
                        ));
                    }
                }
                entity.state = EntityState::Stopped;
            }

            let owned_tasks = self
                .dynamic_handler_owners
                .iter()
                .filter_map(|(task_id, (kind, owner_id))| {
                    (matches!(kind, HandleKind::Cell | HandleKind::Light) && *owner_id == cell_id)
                        .then_some(*task_id)
                })
                .collect::<Vec<_>>();
            for task_id in owned_tasks {
                canceled_tasks.insert(task_id);
                if let Err(error) = self.runtime.cancel(task_id) {
                    errors.push(format!(
                        "Failed to cancel callback for deleted Cell {}: {}",
                        cell_id, error
                    ));
                }
                if let Err(error) = self.runtime.remove_fiber(task_id) {
                    errors.push(format!(
                        "Failed to remove callback for deleted Cell {}: {}",
                        cell_id, error
                    ));
                }
                self.dynamic_handler_owners.remove(&task_id);
            }

            host.engine
                .remove_dynamic_properties_for_handle(HandleKind::Cell, cell_id);
            host.engine
                .remove_dynamic_properties_for_handle(HandleKind::Light, cell_id);
        }

        canceled_tasks
    }

    pub fn dispatch_event(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<(), String> {
        self.runtime
            .dispatch_event(name, arguments, host, &self.disabled_scripts)
    }

    pub fn stop(&mut self, host: &mut HostContext) -> Result<(), String> {
        let mut errors = Vec::new();

        for entity in &mut self.entities {
            if entity.stopped {
                continue;
            }
            entity.stopped = true;
            self.runtime
                .cancel_fibers_for_instance(entity.instance.id())?;

            if Self::has_function(&self.runtime, entity, "on_destroy") {
                let mut instance = entity.instance.clone();
                if let Err(err) =
                    self.runtime
                        .interpreter_mut()
                        .call(&mut instance, "on_destroy", vec![], host)
                {
                    errors.push(format!(
                        "Error in on_destroy() for entity {}: {}",
                        entity.instance.entity_name(),
                        err
                    ));
                }
            }

            entity.state = EntityState::Stopped;
        }
        self.entities.clear();
        self.runtime.interpreter_mut().clear_observers();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    fn sync_entity_instance_from_task(
        runtime: &ScriptRuntime,
        entity: &mut ScriptEntity,
        task_id: ScriptTaskId,
    ) -> Result<(), String> {
        if let Some(fiber) = runtime.fiber(task_id) {
            entity.instance = fiber.instance().clone();
            Ok(())
        } else {
            Err(format!(
                "Failed to sync instance: task {} not found",
                task_id.value()
            ))
        }
    }

    fn has_function(runtime: &ScriptRuntime, entity: &ScriptEntity, name: &str) -> bool {
        let entity_name = entity.instance.entity_name();
        runtime
            .interpreter()
            .program()
            .declarations
            .iter()
            .any(|decl| {
                if let Declaration::Entity(e) = decl {
                    if e.name == entity_name {
                        return e.members.iter().any(|m| {
                            if let EntityMember::Function(f) = m {
                                f.name == name
                            } else {
                                false
                            }
                        });
                    }
                }
                false
            })
    }

    fn transition_entity(
        runtime: &mut ScriptRuntime,
        entity: &mut ScriptEntity,
        _host: &mut HostContext,
    ) -> Result<(), String> {
        if entity.stopped {
            return Ok(());
        }

        loop {
            match entity.state {
                EntityState::Initial => {
                    if Self::has_function(runtime, entity, "on_spawn") {
                        let task_id = runtime.spawn(entity.instance.clone(), "on_spawn", vec![])?;
                        entity.state = EntityState::Spawning(Some(task_id));
                        break;
                    } else {
                        entity.state = EntityState::Spawning(None);
                    }
                }
                EntityState::Spawning(_) => {
                    if let Some(task_id) = entity.active_task() {
                        let _ = task_id;
                        break;
                    }
                    if Self::has_function(runtime, entity, "on_ready") {
                        let task_id = runtime.spawn(entity.instance.clone(), "on_ready", vec![])?;
                        entity.state = EntityState::Readying(Some(task_id));
                        break;
                    } else {
                        entity.state = EntityState::Readying(None);
                    }
                }
                EntityState::Readying(_) => {
                    if let Some(task_id) = entity.active_task() {
                        let _ = task_id;
                        break;
                    }
                    entity.state = EntityState::Active { update_task: None };
                    break;
                }
                _ => break,
            }
        }

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::entity::{EntityId, EntityManager};
    use crate::project::{IndexedModule, ProjectScriptIndex};
    use crate::scripting::api::{EngineHost, call_host_function};
    use crate::scripting::interpreter::Interpreter;
    use crate::scripting::lexer::Lexer;
    use crate::scripting::parser::Parser;
    use crate::scripting::value::HandleKind;
    use glam::Vec3;

    pub(crate) struct TestHost {
        pub(crate) entity_manager: EntityManager,
        pub(crate) world: World,
        pub(crate) pending_events: Vec<(String, Vec<Value>)>,
        pub(crate) dynamic_properties: HashMap<
            (crate::scripting::value::HandleKind, u64),
            BTreeMap<String, crate::scripting::value::DynamicProperty>,
        >,
        pub(crate) test_results: BTreeMap<String, bool>,
        pub(crate) character_system: crate::character::CharacterSystem,
        pub(crate) attachment_system: crate::engine::attachment::AttachmentSystem,
        pub(crate) valid_entity_declarations: Arc<HashSet<String>>,
        pub(crate) pending_spawns: Vec<(String, u64)>,
        pub(crate) pending_destroys: Vec<u64>,
        pub(crate) entity_actor_parents: HashMap<u64, u64>,
        pub(crate) viewport_size: [f32; 2],
        pub(crate) runtime_ui: crate::engine::ui::RuntimeUi,
        pub(crate) save_values: BTreeMap<String, Value>,
    }

    impl EngineHost for TestHost {
        fn tags_find(&self, tag: &str) -> Result<Vec<u64>, String> {
            Ok(self
                .world
                .actor_ids_with_tag(tag)
                .into_iter()
                .filter(|id| {
                    self.entity_actor_parents
                        .values()
                        .any(|actor_id| actor_id == id)
                })
                .collect())
        }
        fn sky_material_id(&self) -> Option<String> {
            self.world.sky.material_asset.clone()
        }
        fn material_layer_alpha(
            &mut self,
            material: &str,
            layer: &str,
        ) -> Result<Option<f64>, String> {
            use crate::scripting::tween::{TweenBinding, TweenValue};
            match self.world.read(&(material.into(), layer.into()), "alpha") {
                Ok(TweenValue::Scalar(alpha)) => Ok(Some(alpha)),
                Ok(_) => Err("Unexpected layer alpha type".into()),
                Err(error) if error.starts_with("Material layer not found:") => Ok(None),
                Err(error) => Err(error),
            }
        }
        fn material_layer_set_alpha(
            &mut self,
            material: &str,
            layer: &str,
            alpha: f64,
        ) -> Result<(), String> {
            use crate::scripting::tween::{TweenBinding, TweenValue};
            let target = (material.into(), layer.into());
            self.world
                .write(&target, "alpha", TweenValue::Scalar(alpha))?;
            self.world.runtime_material_tweens.cancel(&target, "alpha");
            Ok(())
        }
        fn tween_create(
            &mut self,
            material: &str,
            layer: &str,
            property: &str,
            end: crate::scripting::tween::TweenValue,
            duration: f64,
            easing: crate::scripting::tween::TweenEasing,
        ) -> Result<(), String> {
            let mut scheduler = std::mem::take(&mut self.world.runtime_material_tweens);
            let result = scheduler.create(
                &mut self.world,
                (material.into(), layer.into()),
                property.into(),
                end,
                duration,
                easing,
            );
            self.world.runtime_material_tweens = scheduler;
            result
        }
        fn tween_handle_create(
            &mut self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
            property: &str,
            value: crate::scripting::tween::TweenValue,
            time: f64,
            easing: crate::scripting::tween::TweenEasing,
        ) -> Result<(), String> {
            let mut scheduler = std::mem::take(&mut self.world.runtime_property_tweens);
            let result = {
                let mut binding = crate::scripting::tween::HandleTweenBinding { host: self };
                scheduler.create(
                    &mut binding,
                    (kind, id),
                    property.into(),
                    value,
                    time,
                    easing,
                )
            };
            self.world.runtime_property_tweens = scheduler;
            result
        }
        fn tween_tick(&mut self, dt: f64) -> Vec<String> {
            let mut scheduler = std::mem::take(&mut self.world.runtime_material_tweens);
            let mut errors = scheduler.tick(&mut self.world, dt);
            self.world.runtime_material_tweens = scheduler;
            if self.world.runtime_property_tweens.len() > 0 {
                let mut scheduler = std::mem::take(&mut self.world.runtime_property_tweens);
                {
                    let mut binding = crate::scripting::tween::HandleTweenBinding { host: self };
                    errors.extend(scheduler.tick(&mut binding, dt));
                }
                self.world.runtime_property_tweens = scheduler;
            }
            errors
        }
        fn entity_manager(&self) -> &EntityManager {
            &self.entity_manager
        }
        fn is_script_enabled(&self, path: &str) -> bool {
            let normalized_path = path.replace('\\', "/");
            !self
                .world
                .disabled_scripts
                .iter()
                .any(|disabled| disabled.replace('\\', "/") == normalized_path)
        }
        fn complete_test(&mut self, test_name: &str, passed: bool) {
            self.test_results.insert(test_name.to_string(), passed);
        }
        fn is_test_completed(&self, test_name: &str) -> Option<bool> {
            self.test_results.get(test_name).copied()
        }
        fn fire_event(&mut self, event_name: &str, args: Vec<Value>) {
            self.pending_events.push((event_name.to_string(), args));
        }

        fn drain_pending_events(&mut self) -> Vec<(String, Vec<Value>)> {
            std::mem::take(&mut self.pending_events)
        }

        fn get_viewport_size(&self) -> [f32; 2] {
            self.viewport_size
        }

        fn set_viewport_size(&mut self, width: f32, height: f32) {
            if (self.viewport_size[0] - width).abs() > 0.5
                || (self.viewport_size[1] - height).abs() > 0.5
            {
                self.viewport_size = [width, height];
                if width > 0.0 && height > 0.0 {
                    self.pending_events.push((
                        "viewport_resized".to_string(),
                        vec![Value::Number(width as f64), Value::Number(height as f64)],
                    ));
                }
            }
        }

        fn find_ui_element(&self, document: Option<&str>, key: &str) -> Option<u64> {
            self.runtime_ui.find(document, key)
        }

        fn create_ui_element(&mut self, element_type: &str) -> Result<u64, String> {
            self.runtime_ui.allocate(element_type)
        }

        fn delete_ui_element(&mut self, id: u64) -> Result<(), String> {
            self.runtime_ui.delete(id)
        }

        fn get_ui_property(&self, id: u64, name: &str) -> Result<Option<Value>, String> {
            self.runtime_ui.get_property(id, name)
        }

        fn set_ui_property(&mut self, id: u64, name: &str, value: Value) -> Result<bool, String> {
            self.world
                .runtime_property_tweens
                .cancel(&(HandleKind::Ui, id), name);
            self.runtime_ui.set_property(id, name, value)
        }

        fn drain_ui_clicks(&mut self) -> Vec<u64> {
            self.runtime_ui.drain_pending_clicks()
        }

        fn save_get(&self, key: &str) -> Result<Option<Value>, String> {
            Ok(self.save_values.get(key).cloned())
        }

        fn save_set(&mut self, key: &str, value: Value) -> Result<(), String> {
            self.save_values.insert(key.to_string(), value);
            Ok(())
        }

        fn save_has(&self, key: &str) -> Result<bool, String> {
            Ok(self.save_values.contains_key(key))
        }

        fn save_remove(&mut self, key: &str) -> Result<bool, String> {
            Ok(self.save_values.remove(key).is_some())
        }

        fn save_commit(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn get_position(&self, id: u64) -> Option<Vec3> {
            self.entity_manager.get_position(EntityId(id))
        }

        fn set_position(&mut self, id: u64, position: Vec3) {
            let entity_id = EntityId(id);
            self.entity_manager.set_position(entity_id, position);
            self.character_system
                .set_entity_position(entity_id, position);
        }

        fn is_entity_declaration_valid(&self, entity_name: &str) -> bool {
            !entity_name.trim().is_empty()
        }

        fn set_valid_entity_declarations(&mut self, decls: Arc<HashSet<String>>) {
            self.valid_entity_declarations = decls;
        }

        fn spawn_entity(
            &mut self,
            entity_name: &str,
            position: Vec3,
            package_name: Option<&str>,
        ) -> Result<u64, String> {
            if !self.is_entity_declaration_valid(entity_name) {
                return Err("Entity name cannot be empty".to_string());
            }
            let entity_id = self.entity_manager.create_entity(entity_name);
            self.entity_manager.set_position(entity_id, position);

            if let Some(package_name) = package_name {
                let character_id = self
                    .character_system
                    .spawn_character(position, Some(package_name));
                self.character_system
                    .associate_entity(entity_id, character_id);
            }

            self.pending_spawns
                .push((entity_name.to_string(), entity_id.0));

            Ok(entity_id.0)
        }

        fn spawn_character(
            &mut self,
            entity_name: &str,
            position: Vec3,
            character_package: Option<&str>,
        ) -> Result<u64, String> {
            self.spawn_entity(
                entity_name,
                position,
                Some(
                    character_package
                        .unwrap_or(crate::character::package::DEFAULT_CHARACTER_PACKAGE),
                ),
            )
        }

        fn attach_entity(
            &mut self,
            child_kind: HandleKind,
            child_id: u64,
            child_point: &str,
            parent_kind: HandleKind,
            parent_id: u64,
            parent_point: &str,
        ) -> Result<(), String> {
            let resolve = |host: &TestHost, kind: HandleKind, id: u64| match kind {
                HandleKind::Entity => host
                    .entity_manager
                    .validate_handle(id)
                    .then_some(EntityId(id))
                    .ok_or_else(|| format!("runtime Entity {} was not found", id)),
                HandleKind::Actor => host
                    .entity_actor_parents
                    .iter()
                    .find_map(|(entity_id, actor_id)| {
                        (*actor_id == id).then_some(EntityId(*entity_id))
                    })
                    .ok_or_else(|| format!("runtime Actor {} was not found", id)),
                _ => Err(format!("{} handles cannot be attached", kind.name())),
            };
            let child = resolve(self, child_kind, child_id)?;
            let parent = resolve(self, parent_kind, parent_id)?;
            self.attachment_system.attach(
                child,
                child_point,
                parent,
                parent_point,
                &mut self.entity_manager,
                &mut self.character_system,
            )
        }

        fn set_attachment_offset(
            &mut self,
            child_kind: HandleKind,
            child_id: u64,
            offset: Vec3,
        ) -> Result<(), String> {
            let child = match child_kind {
                HandleKind::Entity => EntityId(child_id),
                HandleKind::Actor => self
                    .entity_actor_parents
                    .iter()
                    .find_map(|(entity_id, actor_id)| {
                        (*actor_id == child_id).then_some(EntityId(*entity_id))
                    })
                    .ok_or_else(|| "runtime Actor not found".to_string())?,
                _ => return Err("attachment offset requires Entity or Actor".to_string()),
            };
            self.attachment_system.set_offset(child, offset)
        }

        fn detach_entity(&mut self, child_kind: HandleKind, child_id: u64) -> Result<bool, String> {
            let child = match child_kind {
                HandleKind::Entity => EntityId(child_id),
                HandleKind::Actor => self
                    .entity_actor_parents
                    .iter()
                    .find_map(|(entity_id, actor_id)| {
                        (*actor_id == child_id).then_some(EntityId(*entity_id))
                    })
                    .ok_or_else(|| format!("runtime Actor {} was not found", child_id))?,
                _ => return Err(format!("{} handles cannot be detached", child_kind.name())),
            };
            Ok(self.attachment_system.detach(
                child,
                &mut self.entity_manager,
                &mut self.character_system,
            ))
        }

        fn drain_pending_spawns(&mut self) -> Vec<(String, u64)> {
            std::mem::take(&mut self.pending_spawns)
        }

        fn drain_pending_destroys(&mut self) -> Vec<u64> {
            std::mem::take(&mut self.pending_destroys)
        }

        fn lookup_light(&self, x: i32, y: i32, z: i32) -> Option<u64> {
            let coord = WorldCoord::new(x, y, z);
            if let Some(cell) = self.world.get(coord) {
                if cell.cell_type == CellType::Light {
                    return Some(cell.id);
                }
            }
            None
        }

        fn is_light_enabled(&self, id: u64) -> Option<bool> {
            self.world
                .get_effective_cell_by_id(id)
                .filter(|cell| cell.cell_type == CellType::Light)
                .map(|_| self.world.is_light_enabled_by_id(id))
        }

        fn set_light_enabled(&mut self, id: u64, enabled: bool) {
            self.world.set_light_enabled_by_id_runtime(id, enabled);
        }

        fn is_collision_events_enabled(&self, id: u64) -> bool {
            if let Some(coord) = self.world.resolve_cell_id(id) {
                if let Some(rs) = self.world.runtime_state.get(&id) {
                    if let Some(v) = rs.collision_events_enabled {
                        return v;
                    }
                }
                if let Some(cell) = self.world.get_effective_cell(coord) {
                    return cell.collision_events_enabled;
                }
            }
            true
        }

        fn get_all_cells_of_class(&self, class_name: &str) -> Vec<u64> {
            let mut results = Vec::new();
            for cell in self.world.cells.values() {
                let matches = match class_name {
                    "Light" => cell.cell_type == CellType::Light,
                    "AudioEmitter" => cell.cell_type == CellType::AudioEmitter,
                    "Block" => cell.cell_type == CellType::Block,
                    "FxBlock" => cell.cell_type == CellType::FxBlock,
                    "SpawnPoint" => cell.cell_type == CellType::SpawnPoint,
                    "Player" => cell.cell_type == CellType::Player,
                    _ => false,
                };
                if matches {
                    results.push(cell.id);
                }
            }
            for source in self.world.iter_sources() {
                let matches = match class_name {
                    "Light" => source.cell.cell_type == CellType::Light,
                    "AudioEmitter" => source.cell.cell_type == CellType::AudioEmitter,
                    "ParticleEmitter" => source.cell.cell_type == CellType::ParticleEmitter,
                    _ => false,
                };
                if matches {
                    results.push(source.cell.id);
                }
            }
            results
        }

        fn find_objects(&self, query: &str) -> Vec<(crate::scripting::value::HandleKind, u64)> {
            let mut results = Vec::new();
            if let Some(id) = self.entity_manager.lookup_entity(query) {
                results.push((crate::scripting::value::HandleKind::Entity, id.0));
            }

            for actor in self.world.actors.values() {
                if actor.identity == query {
                    results.push((crate::scripting::value::HandleKind::Actor, actor.id));
                }
            }

            for coord in self.world.active_blocks() {
                if let Some(cell) = self.world.get(coord) {
                    if let Some(identity) = &cell.entity_identity {
                        if identity == query {
                            let kind = match cell.cell_type {
                                crate::world::CellType::Light => {
                                    crate::scripting::value::HandleKind::Light
                                }
                                _ => crate::scripting::value::HandleKind::Cell,
                            };
                            results.push((kind, cell.id));
                        }
                    }
                }
            }
            for source in self.world.iter_sources() {
                if source.cell.entity_identity.as_deref() == Some(query) {
                    let kind = if source.cell.cell_type == CellType::Light {
                        crate::scripting::value::HandleKind::Light
                    } else {
                        crate::scripting::value::HandleKind::Cell
                    };
                    results.push((kind, source.cell.id));
                }
            }
            results
        }

        fn get_children(
            &self,
            _kind: crate::scripting::value::HandleKind,
            _id: u64,
        ) -> Vec<(crate::scripting::value::HandleKind, u64)> {
            Vec::new()
        }

        fn get_parent(
            &self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
        ) -> Option<(crate::scripting::value::HandleKind, u64)> {
            (kind == HandleKind::Entity)
                .then(|| self.entity_actor_parents.get(&id).copied())
                .flatten()
                .map(|actor_id| (HandleKind::Actor, actor_id))
        }

        fn get_cell_object(
            &self,
            cell_id: u64,
        ) -> Option<(crate::scripting::value::HandleKind, u64)> {
            if let Some(coord) = self.world.resolve_cell_id(cell_id) {
                if let Some(cell) = self.world.get(coord) {
                    if let Some(identity) = &cell.entity_identity {
                        if let Some(id) = self.entity_manager.lookup_entity(identity) {
                            return Some((crate::scripting::value::HandleKind::Entity, id.0));
                        }
                    }
                }
            }
            None
        }

        fn actor_exists(&self, actor_id: u64) -> bool {
            self.world.actors.contains_key(&actor_id)
        }

        fn get_handle_attribute(
            &self,
            kind: HandleKind,
            id: u64,
            key: &str,
        ) -> Result<Option<Value>, String> {
            crate::scripting::api::record_world_query_cost(|c| c.direct_attribute_reads += 1);
            use crate::scripting::api::attribute_to_script_value;
            let invalid = || {
                format!(
                    "engine member 'attributes' is not available on handle kind {} yet",
                    kind.name()
                )
            };
            let value = match kind {
                HandleKind::Actor => self
                    .world
                    .actors
                    .get(&id)
                    .ok_or_else(invalid)?
                    .attributes
                    .get(key),
                HandleKind::Entity => self
                    .entity_manager
                    .get_attributes(EntityId(id))
                    .ok_or_else(invalid)?
                    .get(key),
                HandleKind::Cell | HandleKind::Light => {
                    let cell = self
                        .world
                        .get_effective_cell_by_id(id)
                        .ok_or_else(invalid)?;
                    self.world
                        .runtime_state
                        .get(&id)
                        .and_then(|state| state.attribute_overrides.get(key))
                        .or_else(|| cell.attributes.get(key))
                }
                _ => return Err(invalid()),
            };
            Ok(value.map(attribute_to_script_value))
        }

        fn get_property(
            &self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
            name: &str,
        ) -> Result<Option<crate::scripting::value::Value>, String> {
            use crate::scripting::value::{HandleKind, MapKey, Value};
            use crate::world::cell::AttributeValue;
            use std::collections::BTreeMap;

            match kind {
                HandleKind::Actor => {
                    let Some(actor) = self.world.actors.get(&id) else {
                        return Ok(None);
                    };
                    match name {
                        "id" | "actor_id" => return Ok(Some(Value::Number(id as f64))),
                        "is_actor" => return Ok(Some(Value::Bool(true))),
                        "name" => return Ok(Some(Value::String(actor.identity.clone()))),
                        "position" => {
                            return Ok(Some(Value::array(vec![
                                Value::Number(actor.position.x as f64),
                                Value::Number(actor.position.y as f64),
                                Value::Number(actor.position.z as f64),
                            ])));
                        }
                        "rotation" => {
                            return Ok(Some(Value::array(vec![
                                Value::Number(actor.rotation.x as f64),
                                Value::Number(actor.rotation.y as f64),
                                Value::Number(actor.rotation.z as f64),
                            ])));
                        }
                        "scale" => {
                            return Ok(Some(Value::array(vec![
                                Value::Number(actor.scale.x as f64),
                                Value::Number(actor.scale.y as f64),
                                Value::Number(actor.scale.z as f64),
                            ])));
                        }
                        "visible" => return Ok(Some(Value::Bool(actor.visible))),
                        "enabled" => return Ok(Some(Value::Bool(actor.enabled))),
                        "solid" | "can_collide" => {
                            return Ok(Some(Value::Bool(actor.physics.solid)));
                        }
                        "anchored" => return Ok(Some(Value::Bool(actor.physics.anchored))),
                        "collision_enabled" => {
                            return Ok(Some(Value::Bool(actor.physics.collision_enabled)));
                        }
                        "package" => {
                            return Ok(Some(
                                actor
                                    .package
                                    .clone()
                                    .map(Value::String)
                                    .unwrap_or(Value::Nil),
                            ));
                        }
                        "attributes" => {
                            crate::scripting::api::record_world_query_cost(|c| {
                                c.whole_attribute_maps += 1
                            });
                            let mut map = BTreeMap::new();
                            for (key, attribute) in &actor.attributes {
                                let value = match attribute {
                                    AttributeValue::Number(value) => Value::Number(*value),
                                    AttributeValue::Bool(value) => Value::Bool(*value),
                                    AttributeValue::String(value) => Value::String(value.clone()),
                                };
                                map.insert(MapKey::String(key.clone()), value);
                            }
                            return Ok(Some(Value::map(map)));
                        }
                        _ => {}
                    }
                }
                HandleKind::Cell | HandleKind::Light => {
                    if let Some(cell) = self.world.get_effective_cell_by_id(id) {
                        match name {
                            "sound" => {
                                return Ok(Some(Value::Handle {
                                    kind: HandleKind::Sound,
                                    id,
                                }));
                            }
                            "id" => return Ok(Some(Value::Number(cell.id as f64))),
                            "name" => {
                                return Ok(Some(Value::String(
                                    cell.entity_identity
                                        .clone()
                                        .unwrap_or_else(|| "Cell".to_string()),
                                )));
                            }
                            "cellType" => {
                                return Ok(Some(Value::String(format!("{:?}", cell.cell_type))));
                            }
                            "position" => {
                                if let Some(position) = self.world.source_position(id) {
                                    return Ok(Some(Value::array(vec![
                                        Value::Number(position.x as f64),
                                        Value::Number(position.y as f64),
                                        Value::Number(position.z as f64),
                                    ])));
                                }
                                if let Some(coord) = self.world.resolve_cell_id(id) {
                                    return Ok(Some(Value::array(vec![
                                        Value::Number(coord.x as f64),
                                        Value::Number(coord.y as f64),
                                        Value::Number(coord.z as f64),
                                    ])));
                                } else {
                                    return Ok(Some(Value::Nil));
                                }
                            }
                            "visible" => {
                                if let Some(coord) = self.world.resolve_cell_id(id) {
                                    return Ok(Some(Value::Bool(
                                        self.world.is_cell_visible(coord),
                                    )));
                                } else {
                                    if let Some(rs) = self.world.runtime_state.get(&id) {
                                        if let Some(v) = rs.visible {
                                            return Ok(Some(Value::Bool(v)));
                                        }
                                    }
                                    return Ok(Some(Value::Bool(cell.visible)));
                                }
                            }
                            "enabled" => {
                                if let Some(coord) = self.world.resolve_cell_id(id) {
                                    return Ok(Some(Value::Bool(
                                        self.world.is_light_enabled(coord),
                                    )));
                                } else {
                                    if let Some(rs) = self.world.runtime_state.get(&id) {
                                        if let Some(v) = rs.light_enabled {
                                            return Ok(Some(Value::Bool(v)));
                                        }
                                    }
                                    return Ok(Some(Value::Bool(cell.light_enabled)));
                                }
                            }
                            "solid" => {
                                if let Some(coord) = self.world.resolve_cell_id(id) {
                                    return Ok(Some(Value::Bool(self.world.is_cell_solid(coord))));
                                } else {
                                    if let Some(rs) = self.world.runtime_state.get(&id) {
                                        if let Some(v) = rs.solid {
                                            return Ok(Some(Value::Bool(v)));
                                        }
                                    }
                                    return Ok(Some(Value::Bool(cell.solid)));
                                }
                            }
                            "anchored" => {
                                if let Some(coord) = self.world.resolve_cell_id(id) {
                                    return Ok(Some(Value::Bool(
                                        self.world.is_cell_anchored(coord),
                                    )));
                                } else {
                                    if let Some(rs) = self.world.runtime_state.get(&id) {
                                        if let Some(v) = rs.anchored {
                                            return Ok(Some(Value::Bool(v)));
                                        }
                                    }
                                    return Ok(Some(Value::Bool(cell.anchored)));
                                }
                            }
                            "intensity" | "range" | "shadows" if kind == HandleKind::Light => {
                                let (_, intensity, range, shadows) = self
                                    .world
                                    .emitted_light_runtime(id)
                                    .ok_or_else(|| "Invalid Light handle".to_string())?;
                                return Ok(Some(match name {
                                    "intensity" => Value::Number(intensity as f64),
                                    "range" => Value::Number(range as f64),
                                    _ => Value::Bool(shadows),
                                }));
                            }
                            "color" => {
                                if kind == HandleKind::Light {
                                    let (color, _, _, _) = self
                                        .world
                                        .emitted_light_runtime(id)
                                        .ok_or_else(|| "Invalid Light handle".to_string())?;
                                    return Ok(Some(Value::array(
                                        color
                                            .to_array()
                                            .into_iter()
                                            .map(|n| Value::Number(n as f64))
                                            .collect(),
                                    )));
                                }
                                let color = if let Some(coord) = self.world.resolve_cell_id(id) {
                                    self.world.get_effective_color(coord)
                                } else {
                                    if let Some(rs) = self.world.runtime_state.get(&id) {
                                        if let Some(v) = rs.color_rgb {
                                            v
                                        } else {
                                            cell.color_rgb
                                        }
                                    } else {
                                        cell.color_rgb
                                    }
                                };
                                return Ok(Some(Value::array(vec![
                                    Value::Number(color.x as f64),
                                    Value::Number(color.y as f64),
                                    Value::Number(color.z as f64),
                                ])));
                            }
                            "offset" => {
                                let offset = if let Some(coord) = self.world.resolve_cell_id(id) {
                                    self.world.get_visual_offset(coord)
                                } else {
                                    if let Some(rs) = self.world.runtime_state.get(&id) {
                                        rs.visual_offset.unwrap_or(Vec3::ZERO)
                                    } else {
                                        Vec3::ZERO
                                    }
                                };
                                return Ok(Some(Value::array(vec![
                                    Value::Number(offset.x as f64),
                                    Value::Number(offset.y as f64),
                                    Value::Number(offset.z as f64),
                                ])));
                            }
                            "attributes" => {
                                crate::scripting::api::record_world_query_cost(|c| {
                                    c.whole_attribute_maps += 1
                                });
                                let mut map = BTreeMap::new();
                                for (key, attr) in &cell.attributes {
                                    let val = match attr {
                                        AttributeValue::Number(n) => Value::Number(*n),
                                        AttributeValue::Bool(b) => Value::Bool(*b),
                                        AttributeValue::String(s) => Value::String(s.clone()),
                                    };
                                    map.insert(MapKey::String(key.clone()), val);
                                }
                                if let Some(rs) = self.world.runtime_state.get(&id) {
                                    for (key, attr) in &rs.attribute_overrides {
                                        let val = match attr {
                                            AttributeValue::Number(n) => Value::Number(*n),
                                            AttributeValue::Bool(b) => Value::Bool(*b),
                                            AttributeValue::String(s) => Value::String(s.clone()),
                                        };
                                        map.insert(MapKey::String(key.clone()), val);
                                    }
                                }
                                return Ok(Some(Value::map(map)));
                            }
                            _ => {}
                        }
                    }
                }
                HandleKind::Entity => {
                    let entity_id = EntityId(id);
                    match name {
                        "sound" => {
                            return Ok(Some(Value::Handle {
                                kind: HandleKind::Sound,
                                id,
                            }));
                        }
                        "name" => {
                            if let Some(entity_name) = self.entity_manager.get_name(entity_id) {
                                return Ok(Some(Value::String(entity_name.to_string())));
                            }
                        }
                        _ => {}
                    }
                }
                HandleKind::Sound => match name {
                    "playing" => {
                        return Ok(Some(Value::Bool(
                            self.world.is_audio_playing(id) && !self.world.is_audio_paused(id),
                        )));
                    }
                    "looped" => {
                        return Ok(Some(Value::Bool(self.world.is_audio_looped(id))));
                    }
                    "volume" => {
                        return Ok(Some(Value::Number(self.world.get_audio_volume(id) as f64)));
                    }
                    "min_distance" | "MinDistance" => {
                        return Ok(Some(Value::Number(
                            self.world.get_audio_min_distance(id) as f64
                        )));
                    }
                    "max_distance" | "MaxDistance" => {
                        return Ok(Some(Value::Number(
                            self.world.get_audio_max_distance(id) as f64
                        )));
                    }
                    _ => {}
                },
                _ => {}
            }
            Ok(None)
        }

        fn set_property(
            &mut self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
            name: &str,
            value: crate::scripting::value::Value,
        ) -> Result<bool, String> {
            use crate::scripting::value::HandleKind;
            self.world.runtime_property_tweens.cancel(&(kind, id), name);
            if kind == HandleKind::Entity {
                match name {
                    "visible" => {
                        self.entity_manager
                            .set_visible(EntityId(id), value.as_bool()?);
                        return Ok(true);
                    }
                    "enabled" => {
                        self.entity_manager
                            .set_enabled(EntityId(id), value.as_bool()?);
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            if kind == HandleKind::Actor {
                let Some(actor) = self.world.actors.get_mut(&id) else {
                    return Err("Invalid Actor handle".to_string());
                };
                match name {
                    "visible" => {
                        let visible = value.as_bool()?;
                        actor.visible = visible;
                        if let Some(entity_id) =
                            self.entity_actor_parents
                                .iter()
                                .find_map(|(entity_id, actor_id)| {
                                    (*actor_id == id).then_some(*entity_id)
                                })
                        {
                            self.entity_manager
                                .set_visible(EntityId(entity_id), visible);
                        }
                    }
                    "enabled" => actor.enabled = value.as_bool()?,
                    "solid" | "can_collide" => actor.physics.solid = value.as_bool()?,
                    "collision_enabled" => actor.physics.collision_enabled = value.as_bool()?,
                    "anchored" => actor.physics.anchored = value.as_bool()?,
                    "position" | "rotation" | "scale" => {
                        let basket = value.as_basket()?;
                        let borrowed = basket.borrow();
                        if borrowed.elements.len() != 3 {
                            return Err(format!("Actor.{} must be [x, y, z]", name));
                        }
                        let vector = Vec3::new(
                            borrowed.elements[0].as_number()? as f32,
                            borrowed.elements[1].as_number()? as f32,
                            borrowed.elements[2].as_number()? as f32,
                        );
                        match name {
                            "position" => actor.position = vector,
                            "rotation" => actor.rotation = vector,
                            "scale" => actor.scale = vector,
                            _ => unreachable!(),
                        }
                    }
                    "id" | "actor_id" | "name" | "package" | "is_actor" => {
                        return Err(format!("Actor.{} is read-only", name));
                    }
                    "attributes" => {
                        return Err("Actor.attributes cannot be replaced directly".to_string());
                    }
                    _ => return Ok(false),
                }
                return Ok(true);
            }
            if let Some(_cell) = self.world.get_effective_cell_by_id(id) {
                match kind {
                    HandleKind::Sound => match name {
                        "playing" => {
                            let playing = value.as_bool()?;
                            self.world.set_audio_playing_runtime(id, playing);
                            return Ok(true);
                        }
                        "looped" => {
                            let looped = value.as_bool()?;
                            self.world.set_audio_looped_runtime(id, looped);
                            return Ok(true);
                        }
                        "volume" => {
                            let vol = value.as_number()? as f32;
                            self.world.set_audio_volume_runtime(id, vol);
                            return Ok(true);
                        }
                        "min_distance" | "MinDistance" => {
                            self.world
                                .set_audio_min_distance_runtime(id, value.as_number()? as f32);
                            return Ok(true);
                        }
                        "max_distance" | "MaxDistance" => {
                            self.world
                                .set_audio_max_distance_runtime(id, value.as_number()? as f32);
                            return Ok(true);
                        }
                        _ => {}
                    },
                    HandleKind::Cell | HandleKind::Light => match name {
                        "position" => {
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();
                            if borrowed.elements.len() != 3 {
                                return Err(
                                    "position must be a basket of 3 numbers [x, y, z]".to_string()
                                );
                            }
                            let x = borrowed.elements[0].as_number()? as f32;
                            let y = borrowed.elements[1].as_number()? as f32;
                            let z = borrowed.elements[2].as_number()? as f32;
                            if self.world.source(id).is_some() {
                                self.world
                                    .runtime_state
                                    .entry(id)
                                    .or_default()
                                    .source_position = Some(Vec3::new(x, y, z));
                            } else {
                                self.move_runtime_cell(id, x as i32, y as i32, z as i32)?;
                            }
                            return Ok(true);
                        }
                        "name" => {
                            let name = value.as_string()?;
                            if let Some(cell) = self.world.runtime_cells.get_mut(&id) {
                                cell.entity_identity = Some(name.to_string());
                            } else {
                                return Err(
                                    "Cannot change name of an authored cell at runtime".to_string()
                                );
                            }
                            return Ok(true);
                        }
                        "visible" => {
                            let visible = value.as_bool()?;
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_visible_runtime(coord, visible);
                            } else {
                                self.world.runtime_state.entry(id).or_default().visible =
                                    Some(visible);
                            }
                            return Ok(true);
                        }
                        "solid" => {
                            let solid = value.as_bool()?;
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_solid_runtime(coord, solid);
                            } else {
                                self.world.runtime_state.entry(id).or_default().solid = Some(solid);
                            }
                            return Ok(true);
                        }
                        "anchored" => {
                            let anchored = value.as_bool()?;
                            if let Some(coord) = self.world.resolve_cell_id(id) {
                                self.world.set_cell_anchored_runtime(coord, anchored);
                            } else {
                                self.world.runtime_state.entry(id).or_default().anchored =
                                    Some(anchored);
                            }
                            return Ok(true);
                        }
                        "intensity" | "range" | "shadows" if kind == HandleKind::Light => {
                            self.world.set_light_property_runtime(id, name, value)?;
                            return Ok(true);
                        }
                        "color" => {
                            if kind == HandleKind::Light {
                                self.world.set_light_property_runtime(id, name, value)?;
                                return Ok(true);
                            }
                            let basket = value.as_basket()?;
                            let borrowed = basket.borrow();
                            if borrowed.elements.len() == 3 {
                                let r = borrowed.elements[0].as_number()? as f32;
                                let g = borrowed.elements[1].as_number()? as f32;
                                let b = borrowed.elements[2].as_number()? as f32;
                                let color = glam::Vec3::new(r, g, b);
                                if let Some(coord) = self.world.resolve_cell_id(id) {
                                    self.world.set_cell_color_runtime(coord, color);
                                } else {
                                    self.world.runtime_state.entry(id).or_default().color_rgb =
                                        Some(color);
                                }
                            }
                            return Ok(true);
                        }
                        "enabled" => {
                            let enabled = value.as_bool()?;
                            self.world.set_light_enabled_by_id_runtime(id, enabled);
                            return Ok(true);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
            Ok(false)
        }

        fn cell_exists(&self, id: u64) -> bool {
            self.world.get_effective_cell_by_id(id).is_some()
        }

        fn entity_exists(&self, id: u64) -> bool {
            self.entity_manager.validate_handle(id)
        }

        fn get_script_property(
            &self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
            name: &str,
        ) -> Option<crate::scripting::value::DynamicProperty> {
            self.dynamic_properties
                .get(&(kind, id))
                .and_then(|m| m.get(name))
                .cloned()
        }
        fn set_script_property(
            &mut self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
            name: String,
            value: crate::scripting::value::Value,
            owner_script: Option<String>,
        ) {
            self.dynamic_properties
                .entry((kind, id))
                .or_default()
                .insert(
                    name,
                    crate::scripting::value::DynamicProperty {
                        value,
                        owner_script,
                    },
                );
        }

        fn remove_dynamic_properties_by_owner(&mut self, owner_script: &str) {
            let norm = owner_script.replace('\\', "/");
            for properties in self.dynamic_properties.values_mut() {
                properties.retain(|_, prop| prop.owner_script.as_deref() != Some(&norm));
            }
        }

        fn call_method(
            &mut self,
            kind: crate::scripting::value::HandleKind,
            id: u64,
            name: &str,
            _args: &[crate::scripting::value::Value],
        ) -> Result<Option<crate::scripting::value::Value>, String> {
            use crate::scripting::value::{HandleKind, Value};
            if kind == HandleKind::Actor {
                if name == "is_valid" {
                    return Ok(Some(Value::Bool(self.world.actors.contains_key(&id))));
                }
                if !self.world.actors.contains_key(&id) {
                    return Err("Actor method called on an invalid handle".to_string());
                }
                if name == "destroy" {
                    if !_args.is_empty() {
                        return Err("Actor.destroy() expects 0 arguments".to_string());
                    }
                    return Ok(Some(Value::Bool(self.world.actors.remove(&id).is_some())));
                }
                if name == "navigate_to" {
                    if !(1..=3).contains(&_args.len()) {
                        return Err(
                            "Actor.navigate_to() expects target [, speed [, stop_distance]]"
                                .to_string(),
                        );
                    }
                    let target = _args[0].as_basket()?;
                    if target.borrow().elements.len() != 3 {
                        return Err("Actor.navigate_to() target must be [x, y, z]".to_string());
                    }
                    return Ok(Some(Value::Bool(true)));
                }
            }
            if kind == HandleKind::Sound {
                match name {
                    "set" => {
                        if _args.len() != 1 {
                            return Err("Sound.set(path) expects one audio asset path".to_string());
                        }
                        let path = _args[0].as_string()?;
                        if self.world.get_effective_cell_by_id(id).is_none() {
                            let entity = self
                                .entity_manager
                                .get_entity(crate::engine::entity::EntityId(id))
                                .ok_or_else(|| {
                                    "Sound.set requires a valid Entity or AudioEmitter".to_string()
                                })?;
                            self.world.ensure_entity_sound(id, entity.position);
                        }
                        self.world.set_audio_path_runtime(id, path)?;
                        return Ok(Some(Value::Nil));
                    }
                    "pitch" => {
                        if _args.len() != 1 {
                            return Err("Sound.pitch(rate) expects one multiplier".to_string());
                        }
                        let rate = _args[0].as_number()?;
                        if !rate.is_finite() || !(0.25..=4.0).contains(&rate) {
                            return Err(
                                "Sound.pitch(rate) expects a finite multiplier from 0.25 to 4.0"
                                    .into(),
                            );
                        }
                        if self.world.get_effective_cell_by_id(id).is_none() {
                            let entity = self
                                .entity_manager
                                .get_entity(crate::engine::entity::EntityId(id))
                                .ok_or_else(|| {
                                    "Sound.pitch requires a valid Entity or AudioEmitter"
                                        .to_string()
                                })?;
                            self.world.ensure_entity_sound(id, entity.position);
                        }
                        self.world.set_audio_pitch_runtime(id, rate as f32)?;
                        return Ok(Some(Value::Nil));
                    }
                    "play" => {
                        self.world.audio_play_runtime(id);
                        return Ok(Some(Value::Nil));
                    }
                    "stop" => {
                        self.world.audio_stop_runtime(id);
                        return Ok(Some(Value::Nil));
                    }
                    "pause" => {
                        self.world.audio_pause_runtime(id);
                        return Ok(Some(Value::Nil));
                    }
                    _ => {}
                }
            }
            Ok(None)
        }

        fn is_audio_playing(&self, id: u64) -> Option<bool> {
            Some(self.world.is_audio_playing(id))
        }
        fn is_audio_paused(&self, id: u64) -> Option<bool> {
            Some(self.world.is_audio_paused(id))
        }
        fn is_audio_looped(&self, id: u64) -> Option<bool> {
            Some(self.world.is_audio_looped(id))
        }
        fn get_audio_volume(&self, id: u64) -> Option<f32> {
            Some(self.world.get_audio_volume(id))
        }
        fn get_audio_min_distance(&self, id: u64) -> Option<f32> {
            Some(self.world.get_audio_min_distance(id))
        }
        fn get_audio_max_distance(&self, id: u64) -> Option<f32> {
            Some(self.world.get_audio_max_distance(id))
        }
        fn set_audio_playing(&mut self, id: u64, playing: bool) {
            self.world.set_audio_playing_runtime(id, playing);
        }
        fn set_audio_looped(&mut self, id: u64, looped: bool) {
            self.world.set_audio_looped_runtime(id, looped);
        }
        fn set_audio_volume(&mut self, id: u64, volume: f32) {
            self.world.set_audio_volume_runtime(id, volume);
        }
        fn set_audio_min_distance(&mut self, id: u64, distance: f32) {
            self.world.set_audio_min_distance_runtime(id, distance);
        }
        fn set_audio_max_distance(&mut self, id: u64, distance: f32) {
            self.world.set_audio_max_distance_runtime(id, distance);
        }
        fn audio_play(&mut self, id: u64) {
            self.world.audio_play_runtime(id);
        }
        fn audio_stop(&mut self, id: u64) {
            self.world.audio_stop_runtime(id);
        }
        fn audio_pause(&mut self, id: u64) {
            self.world.audio_pause_runtime(id);
        }

        fn set_attribute(
            &mut self,
            id: u64,
            key: String,
            value: crate::scripting::value::Value,
        ) -> Result<(), String> {
            use crate::scripting::value::Value;
            use crate::world::cell::AttributeValue;
            let attr_val = match value {
                Value::Number(n) => AttributeValue::Number(n),
                Value::Bool(b) => AttributeValue::Bool(b),
                Value::String(s) => AttributeValue::String(s),
                _ => {
                    return Err(format!(
                        "Attributes only support Number, Bool, or String. Got {}",
                        value.type_name()
                    ));
                }
            };
            if let Some(actor) = self.world.actors.get_mut(&id) {
                actor.attributes.insert(key, attr_val);
                return Ok(());
            }
            if self.world.get_effective_cell_by_id(id).is_some() {
                self.world
                    .runtime_state
                    .entry(id)
                    .or_default()
                    .attribute_overrides
                    .insert(key, attr_val);
                Ok(())
            } else {
                Err("invalid cell handle for attribute assignment".to_string())
            }
        }

        fn set_handle_attribute(
            &mut self,
            kind: HandleKind,
            id: u64,
            key: String,
            value: Value,
        ) -> Result<(), String> {
            self.world
                .runtime_property_tweens
                .cancel(&(kind, id), &format!("attributes.{key}"));
            if matches!(
                kind,
                HandleKind::Cell | HandleKind::Light | HandleKind::Actor | HandleKind::Entity
            ) {
                self.set_attribute(id, key, value)
            } else {
                Err(format!("{} does not support attributes", kind.name()))
            }
        }
        fn remove_handle_attribute(
            &mut self,
            kind: HandleKind,
            id: u64,
            key: &str,
        ) -> Result<(), String> {
            self.world
                .runtime_property_tweens
                .cancel(&(kind, id), &format!("attributes.{key}"));
            if matches!(
                kind,
                HandleKind::Cell | HandleKind::Light | HandleKind::Actor | HandleKind::Entity
            ) {
                self.remove_attribute(id, key)
            } else {
                Err(format!("{} does not support attributes", kind.name()))
            }
        }

        fn remove_attribute(&mut self, id: u64, key: &str) -> Result<(), String> {
            if let Some(actor) = self.world.actors.get_mut(&id) {
                actor.attributes.remove(key);
                return Ok(());
            }
            if self.world.get_effective_cell_by_id(id).is_some() {
                if let Some(rs) = self.world.runtime_state.get_mut(&id) {
                    rs.attribute_overrides.remove(key);
                }
                Ok(())
            } else {
                Err("invalid cell handle for attribute removal".to_string())
            }
        }

        fn create_runtime_cell(
            &mut self,
            cell_type: &str,
        ) -> Result<(crate::scripting::value::HandleKind, u64), String> {
            let ct = match cell_type {
                "Block" => crate::world::CellType::Block,
                "FxBlock" => crate::world::CellType::FxBlock,
                "Player" => crate::world::CellType::Player,
                "Light" => crate::world::CellType::Light,
                "SpawnPoint" => crate::world::CellType::SpawnPoint,
                "AudioEmitter" => crate::world::CellType::AudioEmitter,
                "Empty" => return Err("Cannot create Empty cell".to_string()),
                _ => return Err(format!("Unknown cell type: {}", cell_type)),
            };

            let id = self.world.create_runtime_cell(ct);
            let kind = if ct == crate::world::CellType::Light {
                crate::scripting::value::HandleKind::Light
            } else {
                crate::scripting::value::HandleKind::Cell
            };

            Ok((kind, id))
        }

        fn move_runtime_cell(&mut self, id: u64, x: i32, y: i32, z: i32) -> Result<(), String> {
            self.world
                .move_runtime_cell(id, crate::world::WorldCoord::new(x, y, z))
        }

        fn delete_cell(&mut self, id: u64) -> Result<(), String> {
            self.world.delete_cell_runtime(id);
            self.dynamic_properties.remove(&(HandleKind::Cell, id));
            self.dynamic_properties.remove(&(HandleKind::Light, id));
            Ok(())
        }

        fn remove_dynamic_properties_for_handle(&mut self, kind: HandleKind, id: u64) {
            self.dynamic_properties.remove(&(kind, id));
        }
    }

    pub(crate) fn test_host() -> TestHost {
        TestHost {
            entity_manager: EntityManager::new(),
            pending_events: Vec::new(),
            world: World::new(),
            dynamic_properties: HashMap::new(),
            test_results: BTreeMap::new(),
            character_system: crate::character::CharacterSystem::new(),
            attachment_system: crate::engine::attachment::AttachmentSystem::default(),
            valid_entity_declarations: Arc::new(HashSet::new()),
            pending_spawns: Vec::new(),
            pending_destroys: Vec::new(),
            entity_actor_parents: HashMap::new(),
            viewport_size: [0.0, 0.0],
            runtime_ui: crate::engine::ui::RuntimeUi::new(),
            save_values: BTreeMap::new(),
        }
    }

    pub(crate) fn create_scene(source: &str, host: &mut HostContext) -> ScriptScene {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();

        for decl in &program.declarations {
            if let Declaration::Entity(_entity) = decl {}
        }

        let entities_to_spawn: Vec<(String, u64, Option<String>, Option<u64>)> = program
            .declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Entity(entity) = decl {
                    let id = host
                        .engine
                        .entity_manager()
                        .lookup_entity(&entity.name)
                        .unwrap_or(EntityId(0));
                    Some((entity.name.clone(), id.0, None, None))
                } else {
                    None
                }
            })
            .collect();

        let scene = ScriptScene::new(program, entities_to_spawn, host).unwrap();
        scene
    }

    pub(crate) fn create_scene_with_path(
        source: &str,
        path: &str,
        host: &mut HostContext,
    ) -> ScriptScene {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();

        let entities_to_spawn: Vec<(String, u64, Option<String>, Option<u64>)> = program
            .declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Entity(entity) = decl {
                    let id = host
                        .engine
                        .entity_manager()
                        .lookup_entity(&entity.name)
                        .unwrap_or(EntityId(0));
                    Some((entity.name.clone(), id.0, Some(path.to_string()), None))
                } else {
                    None
                }
            })
            .collect();

        let mut scene = ScriptScene::new_with_path(
            program.clone(),
            entities_to_spawn,
            Some(path.to_string()),
            host,
        )
        .unwrap();
        scene.loaded_scripts.insert(path.to_string(), program);
        scene
    }

    pub(crate) fn add_authored_entity(
        world: &mut World,
        coord: WorldCoord,
        cell_type: CellType,
        identity: &str,
    ) -> u64 {
        let id = world.set_cell(coord, cell_type);
        if let Some(cell) = world.get_mut(coord) {
            cell.entity_identity = Some(identity.to_string());
        }
        id
    }

    #[test]
    fn authored_ui_symbolic_handler_dispatches_exactly_once() {
        let source = r#"
fn accept_dialogue() {
    debug.log("AUTHORED_UI_CLICK")
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene_with_path(source, "scripts/dialogue.aeo", &mut host);

        scene
            .dispatch_authored_ui_event(crate::engine::ui::UiSymbolicEventDispatch {
                document_key: "dialogue_ui".to_string(),
                element_key: "accept".to_string(),
                element_type: "Button".to_string(),
                event: "on_click".to_string(),
                module_key: "dialogue".to_string(),
                handler: "accept_dialogue".to_string(),
            })
            .unwrap();

        scene.update(0.0, &mut host).unwrap();

        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message == "AUTHORED_UI_CLICK")
                .count(),
            1
        );
    }

    #[test]
    fn authored_ui_missing_target_reports_full_binding_context() {
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene("", &mut host);

        let error = scene
            .dispatch_authored_ui_event(crate::engine::ui::UiSymbolicEventDispatch {
                document_key: "main_ui".to_string(),
                element_key: "play".to_string(),
                element_type: "Button".to_string(),
                event: "on_click".to_string(),
                module_key: "missing_menu".to_string(),
                handler: "play".to_string(),
            })
            .unwrap_err();

        assert!(error.contains("document='main_ui'"));
        assert!(error.contains("element='play'"));
        assert!(error.contains("type='Button'"));
        assert!(error.contains("event='on_click'"));
        assert!(error.contains("target='missing_menu::play'"));
        assert!(error.contains("was not found"));
    }

    #[test]
    fn authored_ui_handler_must_be_zero_argument_top_level_function() {
        let source = r#"
fn needs_argument(value) {
    debug.log(value)
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene_with_path(source, "scripts/menu.aeo", &mut host);

        let error = scene
            .dispatch_authored_ui_event(crate::engine::ui::UiSymbolicEventDispatch {
                document_key: "main_ui".to_string(),
                element_key: "play".to_string(),
                element_type: "Button".to_string(),
                event: "on_click".to_string(),
                module_key: "menu".to_string(),
                handler: "needs_argument".to_string(),
            })
            .unwrap_err();

        assert!(error.contains("must accept 0 arguments"));
    }

    #[test]
    fn test_host_context_construction() {
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        assert_eq!(host.delta_time, 0.1);
        assert!(host.engine.entity_manager().lookup_entity("Any").is_none());
    }

    #[test]
    fn test_entity_position_read() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const e = get_entity("Test")
        const p = e.position
        debug.log(p[0])
        debug.log(p[1])
        debug.log(p[2])
    }
}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Test");
        th.entity_manager.set_position(id, Vec3::new(1.0, 2.0, 3.0));
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "1"));
        assert!(output.iter().any(|r| r.message == "2"));
        assert!(output.iter().any(|r| r.message == "3"));
    }

    #[test]
    fn test_entity_set_position() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const e = get_entity("Test")
        e.set_position(10.0, 20.0, 30.0)
    }
}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Test");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert_eq!(
            th.entity_manager.get_position(id),
            Some(Vec3::new(10.0, 20.0, 30.0))
        );
    }

    #[test]
    fn test_entity_translate() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const e = get_entity("Test")
        e.translate(1.0, 1.0, 1.0)
    }
}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Test");
        th.entity_manager.set_position(id, Vec3::new(5.0, 5.0, 5.0));
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert_eq!(
            th.entity_manager.get_position(id),
            Some(Vec3::new(6.0, 6.0, 6.0))
        );
    }

    #[test]
    fn test_transform_correct_target() {
        let source = r#"
entity Target {
    fn on_spawn() {
        const e = get_entity("Target")
        e.set_position(100.0, 100.0, 100.0)
    }
}
entity Other {}
"#;
        let mut th = test_host();
        let id_target = th.entity_manager.create_entity("Target");
        let id_other = th.entity_manager.create_entity("Other");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert_eq!(
            th.entity_manager.get_position(id_target),
            Some(Vec3::new(100.0, 100.0, 100.0))
        );
        assert_eq!(th.entity_manager.get_position(id_other), Some(Vec3::ZERO));
    }

    #[test]
    fn test_transform_invalid_handle_fails_safely() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const e = get_entity("Test")
        wait(1)
        e.set_position(10.0, 20.0, 30.0)
    }
}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Test");
        let mut scene = {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            let mut scene = create_scene(source, &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
            scene
        };
        th.entity_manager.remove_entity(id);
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        assert!(scene.update(1.0, &mut host).is_ok());
    }

    #[test]
    fn scene_lifecycle_full() {
        let source = r#"
entity Test {
    fn on_spawn() { debug.log("spawn") }
    fn on_ready() { debug.log("ready") }
    fn update(dt: number) { debug.log("update") }
    fn on_destroy() { debug.log("destroy") }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        assert!(scene.output().is_empty());

        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        let _ = scene.stop(&mut host);
    }

    #[test]
    fn destroying_entity_cancels_its_yielded_contact_callback() {
        let source = r#"
entity Toucher {
    fn on_touch(other) {
        wait(5)
        debug.log("TOUCH_AFTER_DESTROY")
    }
}
"#;
        let mut th = test_host();
        let toucher_id = th.entity_manager.create_entity("Toucher");
        let other_id = th.entity_manager.create_entity("Other");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let contacts = crate::engine::entity::EntityContactResult {
            entered: [crate::engine::entity::EntityContactPair::new(
                toucher_id, other_id,
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        scene
            .on_entity_contacts(&contacts, None, &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert_eq!(
            scene.runtime.fiber_count(),
            1,
            "contact callback should be yielded"
        );

        scene.destroy_entity(toucher_id, &mut host).unwrap();
        scene.update(10.0, &mut host).unwrap();

        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message == "TOUCH_AFTER_DESTROY"),
            "a callback fiber must not resume after its owning Entity is destroyed"
        );
        assert_eq!(scene.runtime.fiber_count(), 0);
    }

    #[test]
    fn scene_update_receives_dt() {
        let source = r#"
entity Test {
    fn update(dt: number) { debug.log(dt) }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.5, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "0.5"));
    }

    #[test]
    fn scene_runtime_failure_observable() {
        let source = r#"
entity Test {
    fn update(dt: number) {
        const x = 1 / 0
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        let result = scene.update(0.0, &mut host);
        assert!(result.is_ok(), "one script failure must not fail the scene");
        assert!(scene.output().iter().any(|record| {
            record.severity == super::super::log::LogSeverity::Error
                && record.message.contains("division by zero")
        }));
    }

    #[test]
    fn scene_invalid_entity_reference() {
        let source = r#"
entity Test {
    fn update(dt: number) {
        const e = get_entity("Missing")
        if e == nil {
            debug.log("not_found")
        }
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(scene.output().iter().any(|r| r.message == "not_found"));
    }

    #[test]
    fn scene_entity_validity() {
        let source = r#"
entity Test {
    fn update(dt: number) {
        const e = get_entity("Test")
        if e != nil {
            if e.is_valid() {
                debug.log("valid")
            }
        }
    }
}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Test");
        let mut scene = {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            let mut scene = create_scene(source, &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
            scene
        };

        assert!(scene.output().iter().any(|r| r.message == "valid"));
        th.entity_manager.remove_entity(id);
        scene.runtime.interpreter_mut().drain_output();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        scene.update(0.0, &mut host).unwrap();
    }

    #[test]
    fn scene_stop_prevents_execution() {
        let source = r#"
entity Test {
    fn update(dt: number) { debug.log("update") }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let _ = scene.stop(&mut host);

        scene.update(0.1, &mut host).unwrap();
        assert!(!scene.output().iter().any(|r| r.message == "update"));
    }

    #[test]
    fn update_cooperative_wait() {
        let source = r#"
entity Test {
    fn update(dt: number) {
        debug.log("start")
        wait(1)
        debug.log("end")
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(scene.output().iter().any(|r| r.message == "start"));
        assert!(!scene.output().iter().any(|r| r.message == "end"));

        scene.update(0.5, &mut host).unwrap();
        assert!(!scene.output().iter().any(|r| r.message == "end"));
        scene.update(0.5, &mut host).unwrap();
        assert!(scene.output().iter().any(|r| r.message == "end"));
    }

    #[test]
    fn test_explicit_binding_instantiates_only_requested() {
        let source = r#"
entity Player {}
entity Enemy {}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Player");
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let scene = ScriptScene::new(
            program,
            vec![("Player".to_string(), id.0, None, None)],
            &mut host,
        )
        .unwrap();

        assert_eq!(scene.entities.len(), 1);
        assert_eq!(scene.entities[0].instance.entity_name(), "Player");
    }

    #[test]
    fn test_unassigned_script_does_not_execute() {
        let source = r#"
entity Bound {
    fn on_spawn() { debug.log("bound") }
}
entity Unbound {
    fn on_spawn() { debug.log("unbound") }
}
"#;
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Bound");
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };

        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(
            program,
            vec![("Bound".to_string(), id.0, None, None)],
            &mut host,
        )
        .unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "bound"));
        assert!(!output.iter().any(|r| r.message == "unbound"));
    }

    #[test]
    fn test_multiple_bindings_remain_independent_bridge() {
        let source = r#"
entity A { fn on_spawn() { debug.log("A") } }
entity B { fn on_spawn() { debug.log("B") } }
"#;
        let mut th = test_host();
        let id_a = th.entity_manager.create_entity("A");
        let id_b = th.entity_manager.create_entity("B");
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(
            program,
            vec![
                ("A".to_string(), id_a.0, None, None),
                ("B".to_string(), id_b.0, None, None),
            ],
            &mut host,
        )
        .unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "A"));
        assert!(output.iter().any(|r| r.message == "B"));
    }

    #[test]
    fn test_missing_entity_declaration_isolated_safely_explicit() {
        let source = "entity Found {}";
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Missing",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_missing_entity_decl");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        let script_path = "test.aeo";
        std::fs::write(temp_dir.join("scripts").join(script_path), source).unwrap();

        let bindings = vec![ScriptBinding::new(
            cell_id,
            format!("scripts/{}", script_path),
        )];
        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            1.0,
        )
        .expect("an invalid binding must not prevent the scene from loading");
        assert!(scene.output().iter().any(|record| record.message.contains(
            "Script 'scripts/test.aeo' does not contain an entity declaration for 'Missing'"
        )));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_from_bindings_missing_script_is_isolated() {
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Player,
            "Player",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_missing");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();

        let bindings = vec![ScriptBinding::new(cell_id, "nonexistent.aeo")];
        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            1.0,
        )
        .expect("a missing bound script must not prevent the scene from loading");
        assert!(scene.output().iter().any(|record| {
            record
                .message
                .contains("Bound script 'nonexistent.aeo' not found")
        }));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_load_from_bindings_invalid_script_is_isolated() {
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Broken",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_invalid");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();

        let script_path = "broken.aeo";
        std::fs::write(
            temp_dir.join("scripts").join(script_path),
            "entity Broken { !!! }",
        )
        .unwrap();
        let bindings = vec![ScriptBinding::new(cell_id, script_path)];
        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            1.0,
        )
        .expect("an invalid script must not prevent the scene from loading");
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message.contains("Parser error"))
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn invalid_script_does_not_prevent_healthy_script_startup() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "worldkiln_script_failure_isolation_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/healthy.aeo"),
            "debug.log(\"healthy_started\")",
        )
        .unwrap();
        std::fs::write(temp_dir.join("scripts/broken.aeo"), "const broken = !!!").unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .expect("one malformed script must not prevent the scene from loading");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "healthy_started")
        );
        assert!(scene.output().iter().any(|record| {
            record.severity == super::super::log::LogSeverity::Error
                && record.message.contains("scripts/broken.aeo")
        }));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn invalid_disabled_script_is_silent_and_healthy_script_runs() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "worldkiln_disabled_script_silence_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/healthy.aeo"),
            "debug.log(\"healthy_started\")",
        )
        .unwrap();
        std::fs::write(temp_dir.join("scripts/broken.aeo"), "const broken = !!!").unwrap();
        th.world.disabled_scripts = vec!["scripts/broken.aeo".to_string()];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .expect("a disabled malformed script must be ignored");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "healthy_started")
        );
        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message.contains("broken.aeo"))
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn enabling_invalid_disabled_script_reports_only_that_script() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "worldkiln_enable_invalid_script_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/broken.aeo"), "const broken = !!!").unwrap();
        th.world.disabled_scripts = vec!["scripts/broken.aeo".to_string()];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        assert!(scene.output().is_empty());

        scene.enable_script("scripts/broken.aeo");

        assert!(scene.output().iter().any(|record| {
            record.severity == super::super::log::LogSeverity::Error
                && record.message.contains("scripts/broken.aeo")
        }));
        assert!(
            scene
                .disabled_scripts
                .contains(&"scripts/broken.aeo".to_string())
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn runtime_failure_quarantines_only_the_owning_script() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "worldkiln_runtime_failure_isolation_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/good.aeo"),
            "on.pulse() { debug.log(\"good_pulse\") }",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/bad.aeo"),
            "on.pulse() { const broken = 1 / 0 }",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        for _ in 0..2 {
            host.engine.fire_event("pulse", vec![]);
            scene.update(0.0, &mut host).unwrap();
        }

        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message == "good_pulse")
                .count(),
            2
        );
        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message.contains("division by zero"))
                .count(),
            1,
            "the failed script must be quarantined after its first failure"
        );
        assert!(
            scene
                .disabled_scripts
                .contains(&"scripts/bad.aeo".to_string())
        );
        assert!(
            !scene
                .disabled_scripts
                .contains(&"scripts/good.aeo".to_string())
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_get_light_api() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const l1 = get_light(10, 10, 10)
        if l1 != nil { debug.log("found_l1") }
        const l2 = get_light(0, 0, 0)
        if l2 == nil { debug.log("nil_l2") }
        const l3 = get_light(100, 100, 100)
        if l3 == nil { debug.log("nil_l3") }
    }
}
"#;
        let mut th = test_host();
        th.world
            .set_cell(WorldCoord::new(10, 10, 10), CellType::Light);
        th.world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "found_l1"));
        assert!(output.iter().any(|r| r.message == "nil_l2"));
        assert!(output.iter().any(|r| r.message == "nil_l3"));
    }

    #[test]
    fn test_light_enable_api() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const l = get_light(10, 10, 10)
        if l.is_enabled() { debug.log("enabled_init") }
        l.set_enabled(false)
        if l.is_enabled() == false { debug.log("disabled_after_set") }
        l.set_enabled(true)
        if l.is_enabled() { debug.log("enabled_after_set") }
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(10, 10, 10);
        th.world.set_cell(coord, CellType::Light);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "enabled_init"));
        assert!(output.iter().any(|r| r.message == "disabled_after_set"));
        assert!(output.iter().any(|r| r.message == "enabled_after_set"));
        assert_eq!(th.world.get(coord).unwrap().light_enabled, true);
    }

    #[test]
    fn test_light_independent() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const l1 = get_light(10, 10, 10)
        const l2 = get_light(20, 20, 20)
        l1.set_enabled(false)
        if l1.is_enabled() == false { debug.log("l1_off") }
        if l2.is_enabled() == true { debug.log("l2_on") }
    }
}
"#;
        let mut th = test_host();
        th.world
            .set_cell(WorldCoord::new(10, 10, 10), CellType::Light);
        th.world
            .set_cell(WorldCoord::new(20, 20, 20), CellType::Light);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "l1_off"));
        assert!(output.iter().any(|r| r.message == "l2_on"));
    }

    #[test]
    fn test_light_wait_loop() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const l = get_light(10, 10, 10)
        for i in [1, 2] {
            l.set_enabled(false)
            wait(1)
            l.set_enabled(true)
            wait(1)
        }
        debug.log("done")
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(10, 10, 10);
        th.world.set_cell(coord, CellType::Light);
        let mut scene = {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            let mut scene = create_scene(source, &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
            scene
        };

        {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            scene.update(0.0, &mut host).unwrap();
        }
        assert_eq!(th.world.is_light_enabled(coord), false);

        {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert_eq!(th.world.is_light_enabled(coord), true);

        {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert_eq!(th.world.is_light_enabled(coord), false);

        {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert_eq!(th.world.is_light_enabled(coord), true);

        {
            let mut host = HostContext {
                delta_time: 1.0,
                engine: &mut th,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert!(scene.output().iter().any(|r| r.message == "done"));
    }

    #[test]
    fn test_light_refetch_state() {
        let source = r#"
entity Test {
    fn update(dt: number) {
        const l1 = get_light(10, 10, 10)
        l1.set_enabled(false)
        const l2 = get_light(10, 10, 10)
        if l2.is_enabled() == false { debug.log("refetch_observed_off") }
    }
}
"#;
        let mut th = test_host();
        th.world
            .set_cell(WorldCoord::new(10, 10, 10), CellType::Light);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == "refetch_observed_off")
        );
    }

    fn write_test_script(temp_dir: &Path, file_name: &str, source: &str) {
        let _ = std::fs::remove_dir_all(temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts").join(file_name), source).unwrap();
    }

    #[test]
    fn actor_module_binding_does_not_require_entity_declaration() {
        let mut th = test_host();

        let actor_id = th.world.add_actor("Door", Vec3::new(10.0, 2.0, 5.0));

        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/door_module.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_without_entity");

        write_test_script(
            &temp_dir,
            "door_module.aeo",
            r#"
debug.log("attached", self.parent)
"#,
        );

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .expect("Actor Module must not require an entity declaration");

        assert!(scene.entities.is_empty());

        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message.starts_with("attached"))
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        scene.start(&mut host).unwrap();

        let matches = scene
            .output()
            .iter()
            .filter(|record| record.message == format!("attached Actor#{}", actor_id))
            .count();

        assert_eq!(matches, 1);

        scene.update(0.0, &mut host).unwrap();

        let matches = scene
            .output()
            .iter()
            .filter(|record| record.message == format!("attached Actor#{}", actor_id))
            .count();

        assert_eq!(matches, 1);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn get_entity_exposes_authored_actor_visibility() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("Lander", Vec3::ZERO);
        let runtime_entity = th.entity_manager.create_entity("Lander");
        th.entity_manager.set_visible(runtime_entity, true);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/visibility_probe.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_entity_authored_actor_visibility");
        write_test_script(
            &temp_dir,
            "visibility_probe.aeo",
            r#"
const target = get_entity("Lander")
debug.log("VISIBLE_BEFORE", target.visible)
target.visible = false
debug.log("VISIBLE_AFTER", target.visible)
"#,
        );

        let preexisting_entities = std::collections::HashMap::from([(actor_id, runtime_entity)]);
        th.entity_actor_parents.insert(runtime_entity.0, actor_id);
        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &preexisting_entities,
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let entity_id = host
            .engine
            .entity_manager()
            .lookup_entity("Lander")
            .expect("authored Actor has a runtime Entity");

        assert_eq!(
            host.engine.entity_manager().is_visible(entity_id),
            Some(false)
        );
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "VISIBLE_BEFORE true")
        );
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "VISIBLE_AFTER false")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn actor_module_top_level_wait_resumes() {
        let mut th = test_host();

        let actor_id = th.world.add_actor("Walker", Vec3::ZERO);

        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/wait_module.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_wait");

        write_test_script(
            &temp_dir,
            "wait_module.aeo",
            r#"
debug.log("MODULE_BEFORE_WAIT")
wait(1)
debug.log("MODULE_AFTER_WAIT", self.parent)
"#,
        );

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        scene.start(&mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "MODULE_BEFORE_WAIT")
        );

        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message.starts_with("MODULE_AFTER_WAIT"))
        );

        scene.update(0.5, &mut host).unwrap();

        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message.starts_with("MODULE_AFTER_WAIT"))
        );

        scene.update(0.5, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == format!("MODULE_AFTER_WAIT Actor#{}", actor_id))
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn actor_module_functions_keep_top_level_state_across_nested_module_calls() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("Postern", Vec3::ZERO);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/postern.aeo",
            ));

        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_actor_module_top_level_scope_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/quests")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/postern.aeo"),
            "const postern = self.parent\nfn sync_state() {\n    const stage = scripts.quests.roots.aeo.stage()\n    debug.log(postern, stage)\n}\nsync_state()\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/quests/roots.aeo"),
            "const stage_key = \"roots.stage\"\nfn stage() {\n    debug.log(stage_key)\n    return 0\n}\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == format!("Actor#{} 0", actor_id))
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }


    #[test]
    fn failing_actor_module_is_quarantined_without_blocking_other_modules() {
        let mut th = test_host();
        let broken_actor = th.world.add_actor("Broken", Vec3::ZERO);
        let healthy_actor = th.world.add_actor("Healthy", Vec3::X);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                broken_actor,
                "scripts/broken.aeo",
            ));
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                healthy_actor,
                "scripts/healthy.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_failure_isolation");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/broken.aeo"), "missing_function()").unwrap();
        std::fs::write(
            temp_dir.join("scripts/healthy.aeo"),
            "debug.log(\"HEALTHY_MODULE_STARTED\")",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        scene.start(&mut host).unwrap();
        assert!(scene.output().iter().any(|record| {
            record.message.contains("broken.aeo")
                && record.message.contains("other scripts will continue")
        }));
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "HEALTHY_MODULE_STARTED")
        );

        let failure_count = scene
            .output()
            .iter()
            .filter(|record| record.message.contains("broken.aeo"))
            .count();
        scene.update(0.0, &mut host).unwrap();
        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message.contains("broken.aeo"))
                .count(),
            failure_count,
            "a quarantined module must not fail and spam Output every frame"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn actor_module_legacy_entity_hooks_emit_actionable_warning() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("Sword", Vec3::ZERO);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/sword.aeo",
            ));
        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_legacy_warning");
        write_test_script(
            &temp_dir,
            "sword.aeo",
            r#"entity Sword { fn on_touch(other) {} }"#,
        );

        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();

        assert!(scene.output().iter().any(|record| {
            record
                .message
                .contains("Actor Modules do not bind entity blocks")
                && record.message.contains("self.parent")
        }));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn player_entity_contact_repeats_while_pair_remains_current() {
        let mut th = test_host();
        let player = th.entity_manager.create_entity("Player");
        let ghost = th.entity_manager.create_entity("Ghost");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(
            r#"on.player_entity_contact(other) {
    debug.log("SUSTAINED_CONTACT", other.id)
}
"#,
            &mut host,
        );
        scene.start(&mut host).unwrap();
        let contacts = crate::engine::entity::EntityContactResult {
            current: HashSet::from([crate::engine::entity::EntityContactPair::new(player, ghost)]),
            entered: HashSet::new(),
            exited: HashSet::new(),
        };
        for _ in 0..3 {
            scene
                .on_entity_contacts(&contacts, Some(player), &mut host)
                .unwrap();
            scene.update(0.0, &mut host).unwrap();
        }
        let received = scene
            .output()
            .iter()
            .filter(|record| record.message.contains("SUSTAINED_CONTACT"))
            .count();
        assert_eq!(
            received, 3,
            "every ongoing contact step must dispatch, not just first entry"
        );
    }

    #[test]
    fn player_entity_contact_filters_unrelated_scene_pairs() {
        let mut th = test_host();
        let player = th.entity_manager.create_entity("Player");
        let ghost = th.entity_manager.create_entity("Ghost");
        let tree = th.entity_manager.create_entity("Tree");
        let rock = th.entity_manager.create_entity("Rock");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(
            r#"on.player_entity_contact(other) {
    debug.log("PLAYER_CONTACT", other.id)
}
"#,
            &mut host,
        );
        scene.start(&mut host).unwrap();
        let contacts = crate::engine::entity::EntityContactResult {
            current: HashSet::from([
                crate::engine::entity::EntityContactPair::new(tree, rock),
                crate::engine::entity::EntityContactPair::new(player, ghost),
            ]),
            entered: HashSet::new(),
            exited: HashSet::new(),
        };
        scene
            .on_entity_contacts(&contacts, Some(player), &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();
        let count = scene
            .output()
            .iter()
            .filter(|record| record.message.contains("PLAYER_CONTACT"))
            .count();
        assert_eq!(
            count, 1,
            "unrelated contact pairs must not spawn script work"
        );
    }


    #[test]
    fn entity_contact_dispatches_attached_actor_module_handler() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("Sword", Vec3::ZERO);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/sword.aeo",
            ));
        let sword_entity = th.entity_manager.create_entity("Sword");
        let player_entity = th.entity_manager.create_entity("Player");
        th.entity_actor_parents.insert(sword_entity.0, actor_id);
        let sword_character = th
            .character_system
            .spawn_character(Vec3::ZERO, Some("Rogue"));
        {
            let sword = th
                .character_system
                .characters
                .get_mut(&sword_character)
                .unwrap();
            let mut fixture = (*sword.package).clone();
            fixture
                .metadata
                .attachment_points
                .insert("Grip".into(), "Fist.R".into());
            sword.package = std::sync::Arc::new(fixture);
        }
        let player_character = th
            .character_system
            .spawn_character(Vec3::ZERO, Some("Rogue"));
        th.character_system
            .associate_entity(sword_entity, sword_character);
        th.character_system
            .associate_entity(player_entity, player_character);

        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_contact");
        write_test_script(
            &temp_dir,
            "sword.aeo",
            r#"
const sword = self.parent
sword.on_touch = fn(other) {
    attach(sword, "Grip", other, "RightHand")
    debug.log("ACTOR_TOUCH_RAN")
}
"#,
        );
        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        let pair = crate::engine::entity::EntityContactPair::new(sword_entity, player_entity);
        let contacts = crate::engine::entity::EntityContactResult {
            current: HashSet::from([pair]),
            entered: HashSet::from([pair]),
            exited: HashSet::new(),
        };
        scene
            .on_entity_contacts(&contacts, None, &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "ACTOR_TOUCH_RAN")
        );
        let touch_count = scene
            .output()
            .iter()
            .filter(|record| record.message == "ACTOR_TOUCH_RAN")
            .count();
        let sustained_contact = crate::engine::entity::EntityContactResult {
            current: HashSet::from([pair]),
            entered: HashSet::new(),
            exited: HashSet::new(),
        };
        scene
            .on_entity_contacts(&sustained_contact, None, &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message == "ACTOR_TOUCH_RAN")
                .count(),
            touch_count,
            "on_touch must fire once when contact begins, not every fixed step"
        );
        drop(host);
        assert_eq!(
            th.attachment_system.attached_children(),
            vec![sword_entity],
            "the AeoScript attach call must create the runtime relationship"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn navigation_completion_dispatches_attached_actor_module_handler() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("Walker", Vec3::ZERO);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/walker.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_navigation_finished");
        write_test_script(
            &temp_dir,
            "walker.aeo",
            r#"
const actor = self.parent
actor.on_navigation_finished = fn() {
    debug.log("ACTOR_NAVIGATION_FINISHED")
}
"#,
        );
        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        host.engine.fire_event(
            "navigation_finished",
            vec![Value::Handle {
                kind: HandleKind::Actor,
                id: actor_id,
            }],
        );
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "ACTOR_NAVIGATION_FINISHED")
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn navigation_failure_dispatches_reason_to_attached_actor_module_handler() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("Walker", Vec3::ZERO);
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/walker.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_actor_module_navigation_failed");
        write_test_script(
            &temp_dir,
            "walker.aeo",
            r#"
const actor = self.parent
actor.on_navigation_failed = fn(reason) {
    debug.log("ACTOR_NAVIGATION_FAILED", reason)
}
"#,
        );
        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        host.engine.fire_event(
            "navigation_failed",
            vec![
                Value::Handle {
                    kind: HandleKind::Actor,
                    id: actor_id,
                },
                Value::String("stuck".to_string()),
            ],
        );
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "ACTOR_NAVIGATION_FAILED stuck")
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn same_module_file_attached_to_two_actors_creates_two_module_instances() {
        let mut th = test_host();

        let first_actor = th.world.add_actor("Door_A", Vec3::ZERO);
        let second_actor = th.world.add_actor("Door_B", Vec3::ZERO);

        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                first_actor,
                "scripts/shared_module.aeo",
            ));

        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                second_actor,
                "scripts/shared_module.aeo",
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_shared_actor_module_instances");

        write_test_script(
            &temp_dir,
            "shared_module.aeo",
            r#"
debug.log("parent", self.parent)
"#,
        );

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        assert!(scene.entities.is_empty());

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        scene.start(&mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|record| { record.message == format!("parent Actor#{}", first_actor) })
        );

        assert!(
            scene
                .output()
                .iter()
                .any(|record| { record.message == format!("parent Actor#{}", second_actor) })
        );

        assert_eq!(
            scene
                .runtime
                .interpreter()
                .module_instances
                .keys()
                .filter(|key| key.ends_with("scripts/shared_module.aeo"))
                .count(),
            2
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn disabled_actor_module_stays_attached_and_starts_when_re_enabled() {
        let mut th = test_host();
        let actor_id = th.world.add_actor("DisabledDoor", Vec3::ZERO);
        let script_path = "scripts/disabled_door.aeo";
        th.world.disabled_scripts.push(script_path.to_string());
        th.world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                script_path,
            ));

        let temp_dir = std::env::temp_dir().join("aeo_test_disabled_actor_module");
        write_test_script(
            &temp_dir,
            "disabled_door.aeo",
            "debug.log(\"attached\", self.parent)",
        );

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            !scene
                .output()
                .iter()
                .any(|record| { record.message == format!("attached Actor#{}", actor_id) })
        );

        drop(host);
        th.world.disabled_scripts.clear();
        scene.enable_script(script_path);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| { record.message == format!("attached Actor#{}", actor_id) })
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_single_authored_entity() {
        let source = r#"
entity Guard01 {
    fn on_spawn() { debug.log("spawned Guard01") }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(10, 5, 10);
        let cell_id = add_authored_entity(&mut th.world, coord, CellType::Block, "Guard01");

        let temp_dir = std::env::temp_dir().join("aeo_test_single_authored_entity");
        write_test_script(&temp_dir, "guard.aeo", source);
        let bindings = vec![ScriptBinding::new(cell_id, "scripts/guard.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        assert_eq!(scene.entities.len(), 1);
        let id = th.entity_manager.lookup_entity("Guard01").unwrap();
        assert_eq!(
            th.entity_manager.get_position(id),
            Some(Vec3::new(10.0, 5.0, 10.0))
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == "spawned Guard01")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_authored_position_transfer() {
        let source = r#"
entity Player {
    fn on_spawn() {
        const e = get_entity("Player")
        const p = e.position
        debug.log(p[0])
        debug.log(p[1])
        debug.log(p[2])
    }
}
"#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(10, 5, 20),
            CellType::Player,
            "Player",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_authored_position_transfer");
        write_test_script(&temp_dir, "player.aeo", source);
        let bindings = vec![ScriptBinding::new(cell_id, "scripts/player.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let id = th.entity_manager.lookup_entity("Player").unwrap();
        assert_eq!(
            th.entity_manager.get_position(id),
            Some(Vec3::new(10.0, 5.0, 20.0))
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "10"));
        assert!(output.iter().any(|r| r.message == "5"));
        assert!(output.iter().any(|r| r.message == "20"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_multiple_instances() {
        let source = r#"
entity Guard01 {
    fn on_spawn() {
        const e = get_entity("Guard01")
        const p = e.position
        debug.log(e.name)
        debug.log(p[0])
        debug.log(p[1])
        debug.log(p[2])
    }
}
entity Guard02 {
    fn on_spawn() {
        const e = get_entity("Guard02")
        const p = e.position
        debug.log(e.name)
        debug.log(p[0])
        debug.log(p[1])
        debug.log(p[2])
    }
}
"#;
        let mut th = test_host();
        let id1 = add_authored_entity(
            &mut th.world,
            WorldCoord::new(10, 5, 10),
            CellType::Block,
            "Guard01",
        );
        let id2 = add_authored_entity(
            &mut th.world,
            WorldCoord::new(30, 5, 10),
            CellType::Block,
            "Guard02",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_multiple_instances");
        write_test_script(&temp_dir, "guard.aeo", source);
        let bindings = vec![
            ScriptBinding::new(id1, "scripts/guard.aeo"),
            ScriptBinding::new(id2, "scripts/guard.aeo"),
        ];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        assert_eq!(scene.entities.len(), 2);
        let rid1 = th.entity_manager.lookup_entity("Guard01").unwrap();
        let rid2 = th.entity_manager.lookup_entity("Guard02").unwrap();
        assert_ne!(rid1, rid2);
        assert_eq!(
            th.entity_manager.get_position(rid1),
            Some(Vec3::new(10.0, 5.0, 10.0))
        );
        assert_eq!(
            th.entity_manager.get_position(rid2),
            Some(Vec3::new(30.0, 5.0, 10.0))
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "Guard01"));
        assert!(output.iter().any(|r| r.message == "Guard02"));
        assert!(output.iter().any(|r| r.message == "30"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_identity_isolation() {
        let source = r#"
entity Guard01 {
    fn on_spawn() { debug.log("spawned Guard01") }
}
entity Guard02 {
    fn on_spawn() { debug.log("spawned Guard02") }
}
"#;
        let mut th = test_host();
        let cell_id1 = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Guard01",
        );
        let _cell_id2 = add_authored_entity(
            &mut th.world,
            WorldCoord::new(1, 1, 1),
            CellType::Block,
            "Guard02",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_identity_isolation");
        write_test_script(&temp_dir, "guards.aeo", source);
        let bindings = vec![ScriptBinding::new(cell_id1, "scripts/guards.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        assert_eq!(scene.entities.len(), 1);
        assert_eq!(scene.entities[0].instance.entity_name(), "Guard01");
        assert!(th.entity_manager.lookup_entity("Guard02").is_none());

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == "spawned Guard01")
        );
        assert!(
            !scene
                .output()
                .iter()
                .any(|r| r.message == "spawned Guard02")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_unbound_authored_entity_does_not_spawn() {
        let mut th = test_host();
        add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Unbound",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_unbound_authored_entity");
        write_test_script(
            &temp_dir,
            "other.aeo",
            "entity Other { fn on_spawn() { debug.log(\"other\") } }",
        );

        let bindings: Vec<ScriptBinding> = Vec::new();
        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        assert!(scene.entities.is_empty());
        assert!(th.entity_manager.lookup_entity("Unbound").is_none());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_cells_without_identity_do_not_participate() {
        let mut th = test_host();
        th.world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        th.world.set_cell(WorldCoord::new(1, 1, 1), CellType::Light);
        th.world
            .set_cell(WorldCoord::new(2, 2, 2), CellType::SpawnPoint);

        let temp_dir = std::env::temp_dir().join("aeo_test_non_entity_cells");
        write_test_script(&temp_dir, "test.aeo", "entity Test {}");

        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        assert!(scene.entities.is_empty());
        assert!(th.entity_manager.lookup_entity("Test").is_none());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_duplicate_authored_identity() {
        let mut th = test_host();
        add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Player,
            "Duplicate",
        );
        add_authored_entity(
            &mut th.world,
            WorldCoord::new(1, 1, 1),
            CellType::Block,
            "Duplicate",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_duplicate_authored_identity");
        write_test_script(&temp_dir, "duplicate.aeo", "entity Duplicate {}");

        let result = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        );

        assert!(result.is_ok());
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_duplicate_binding_target_identity() {
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Guard01",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_duplicate_binding_target");
        write_test_script(&temp_dir, "guard.aeo", "entity Guard01 {}");
        let bindings = vec![
            ScriptBinding::new(cell_id, "guard.aeo"),
            ScriptBinding::new(cell_id, "guard.aeo"),
        ];

        let result = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("Multiple script bindings found for target cell ID")
        );
        assert!(th.entity_manager.lookup_entity("Guard01").is_none());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_stale_binding() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join("aeo_test_stale_binding");
        write_test_script(&temp_dir, "test.aeo", "entity NonExistent {}");
        let bindings = vec![ScriptBinding::new(99999999, "test.aeo")];

        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .expect("Stale binding should not prevent scene construction");

        assert!(scene.output().iter().any(|r| r.message.contains(
            "Stale script binding found: target cell ID '99999999' does not exist in the world as an authored cell."
        )));
        assert!(th.entity_manager.lookup_entity("NonExistent").is_none());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_discovery_and_bridge() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const lights = getAllCellsOfClass("Light")
        debug.log("Lights:", lights.len())
        if lights.len() > 0 {
            const first = lights[0]
            const obj = first:getObject()
            if obj != nil {
                debug.log("Found object:", obj.name)
            }
        }
    }
}
"#;
        let mut th = test_host();
        let coord1 = WorldCoord::new(10, 10, 10);
        let coord2 = WorldCoord::new(20, 20, 20);
        add_authored_entity(&mut th.world, coord1, CellType::Light, "Light1");
        add_authored_entity(&mut th.world, coord2, CellType::Light, "Light2");

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "Lights: 2"));
    }

    #[test]
    fn test_get_all_cells_of_class_empty() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const lights = getAllCellsOfClass("Light")
        debug.log("Lights: 0")
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "Lights: 0"));
    }

    #[test]
    fn test_light_set_enabled_on_discovered_cell() {
        let source = r#"
entity Test {
    fn on_spawn() {
        const lights = getAllCellsOfClass("Light")
        if lights.len() > 0 {
            const l = lights[0]
            l.set_enabled(false)
            if l.is_enabled() == false {
                debug.log("Discovered light disabled")
            }
        }
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(10, 10, 10);
        add_authored_entity(&mut th.world, coord, CellType::Light, "Light1");

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(
            output
                .iter()
                .any(|r| r.message == "Discovered light disabled")
        );
        assert_eq!(th.world.is_light_enabled(coord), false);
    }

    #[test]
    fn test_unattached_event_handler_loading() {
        let source = r#"
on GlobalEvent(val) {
    debug.log("Received:", val)
}
"#;
        let mut th = test_host();

        let temp_dir = std::env::temp_dir().join("aeo_test_unattached");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/global.aeo"), source).unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        scene
            .dispatch_event("GlobalEvent", vec![Value::Number(42.0)], &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message.contains("Received: 42"))
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_event_handler_not_duplicated() {
        let source = r#"
on TestEvent() {
    debug.log("Event Triggered")
}
"#;
        let mut th = test_host();

        let temp_dir = std::env::temp_dir().join("aeo_test_duplication");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/test.aeo"), source).unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        scene
            .dispatch_event("TestEvent", vec![], &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        let occurrences: Vec<_> = output
            .iter()
            .filter(|r| r.message == "Event Triggered")
            .collect();

        assert_eq!(occurrences.len(), 1, "Event should only be triggered once");
        assert_eq!(
            occurrences[0].script_path.as_deref(),
            Some("scripts/test.aeo"),
            "Event should have correct script path"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_member_assignment_bridge() {
        use crate::scripting::value::HandleKind;
        let source = r#"
entity Test {
    fn main(b: Cell) {
        b.color = [1.0, 0.2, 0.2]
        b.visible = false
        b.solid = false
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(1, 2, 3);
        th.world.set_cell(coord, crate::world::CellType::Block);
        let cell_id = th.world.get(coord).unwrap().id;
        let handle = Value::Handle {
            kind: HandleKind::Cell,
            id: cell_id,
        };

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![handle], &mut host)
            .unwrap();

        let runtime_state = th.world.runtime_state.get(&cell_id).unwrap();
        assert_eq!(
            runtime_state.color_rgb,
            Some(glam::Vec3::new(1.0, 0.2, 0.2))
        );
        assert_eq!(runtime_state.visible, Some(false));
        assert_eq!(runtime_state.solid, Some(false));
    }

    #[test]
    fn test_find_objects_universal_identity() {
        use crate::scripting::api::EngineHost;
        let mut th = test_host();

        let c1 = WorldCoord::new(1, 1, 1);
        let c2 = WorldCoord::new(2, 2, 2);
        let c3 = WorldCoord::new(3, 3, 3);

        th.world.set_cell(c1, crate::world::CellType::Block);
        th.world.get_mut(c1).unwrap().entity_identity = Some("Ghost".to_string());

        th.world.set_cell(c2, crate::world::CellType::Light);
        th.world.get_mut(c2).unwrap().entity_identity = Some("Ghost".to_string());

        th.world.set_cell(c3, crate::world::CellType::FxBlock);
        th.world.get_mut(c3).unwrap().entity_identity = Some("Ghost".to_string());

        let results = th.find_objects("Ghost");
        assert_eq!(results.len(), 3);

        let mut kinds = results.iter().map(|(k, _)| *k).collect::<Vec<_>>();
        kinds.sort_by_key(|k| format!("{:?}", k));

        use crate::scripting::value::HandleKind;
        assert!(kinds.contains(&HandleKind::Cell));
        assert!(kinds.contains(&HandleKind::Light));

        let cell_count = kinds.iter().filter(|&&k| k == HandleKind::Cell).count();
        let light_count = kinds.iter().filter(|&&k| k == HandleKind::Light).count();
        assert_eq!(cell_count, 2);
        assert_eq!(light_count, 1);
    }

    #[test]
    fn test_save_change_restore_workflow() {
        use crate::scripting::value::HandleKind;
        let source = r#"
entity Test {
    fn main() {
        const blocks = getAllCellsOfClass("Block")
        const saved = {}

        for b in blocks {
            saved[b.id] = b.color
        }

        for b in blocks {
            b.color = [1, 0, 0]
        }

        // Verify they changed to red
        for b in blocks {
            if b.color != [1, 0, 0] {
                debug.log("Error: not red")
            }
        }

        for b in blocks {
            b.color = saved[b.id]
        }
    }
}
"#;
        let mut th = test_host();
        let c1 = WorldCoord::new(1, 1, 1);
        let c2 = WorldCoord::new(2, 2, 2);
        th.world.set_cell(c1, crate::world::CellType::Block);
        th.world.set_cell(c2, crate::world::CellType::Block);
        let id1 = th.world.get(c1).unwrap().id;
        let id2 = th.world.get(c2).unwrap().id;
        th.world.get_mut(c1).unwrap().color_rgb = glam::Vec3::new(0.1, 0.2, 0.3);
        th.world.get_mut(c2).unwrap().color_rgb = glam::Vec3::new(0.4, 0.5, 0.6);

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let rs1 = th.world.runtime_state.get(&id1).unwrap();
        let rs2 = th.world.runtime_state.get(&id2).unwrap();
        assert_eq!(rs1.color_rgb, Some(glam::Vec3::new(0.1, 0.2, 0.3)));
        assert_eq!(rs2.color_rgb, Some(glam::Vec3::new(0.4, 0.5, 0.6)));
        assert!(!scene.output().iter().any(|r| r.message.contains("Error")));
    }

    #[test]
    fn test_compound_assignment() {
        let source = r#"
entity Test {
    fn main() {
        x_loc: number = 10
        x_loc += 5
        debug.log("x", x_loc)

        const a = [1, 2, 3]
        a[0] *= 10
        debug.log("a[0]", a[0])

        const m = {"val": 100}
        m["val"] -= 50
        debug.log("m.val", m["val"])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "x 15"));
        assert!(output.iter().any(|r| r.message == "a[0] 10"));
        assert!(output.iter().any(|r| r.message == "m.val 50"));
    }

    #[test]
    fn test_nested_mutations() {
        let source = r#"
entity Test {
    fn main() {
        const data = {
            "colors": [[1, 0, 0], [0, 1, 0]],
            "meta": {"id": 42}
        }

        data["colors"][0][0] = 0.5
        data["meta"]["name"] = "Ghost"

        debug.log("data.colors[0][0]", data["colors"][0][0])
        debug.log("data.meta.name", data["meta"]["name"])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "data.colors[0][0] 0.5"));
        assert!(output.iter().any(|r| r.message == "data.meta.name Ghost"));
    }

    #[test]
    fn test_map_key_types() {
        let source = r#"
entity Test {
    fn main() {
        const m_loc = {}
        m_loc[1] = "number"
        m_loc["1"] = "string"

        debug.log("m[1]", m_loc[1])
        debug.log("m['1']", m_loc["1"])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "m[1] number"));
        assert!(output.iter().any(|r| r.message == "m['1'] string"));
    }

    #[test]
    fn test_truthiness() {
        use crate::scripting::value::HandleKind;
        let source = r#"
entity Test {
    fn main(h: handle) {
        if nil { debug.log("Error: nil is true") } else { debug.log("nil is false") }
        if true { debug.log("true is true") }
        if false { debug.log("Error: false is true") }
        if h { debug.log("handle is true") }
    }
}
"#;
        let mut th = test_host();
        let handle = Value::Handle {
            kind: HandleKind::Entity,
            id: 1,
        };
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![handle], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "nil is false"));
        assert!(output.iter().any(|r| r.message == "true is true"));
        assert!(output.iter().any(|r| r.message == "handle is true"));
        assert!(!output.iter().any(|r| r.message.contains("Error")));
    }

    #[test]
    fn test_array_indexing_and_mutation() {
        let source = r#"
entity Test {
    fn main() {
        const a = [10, 20, 30]

        // Read
        debug.log("a[0]", a[0])

        // Write
        a[1] = 50
        debug.log("a[1]", a[1])

        // Compound Write
        a[2] += 10
        debug.log("a[2]", a[2])

        // Nested
        const nested = [[1, 2], [3, 4]]
        debug.log("nested[1][0]", nested[1][0])
        nested[1][0] = 99
        debug.log("nested[1][0] mutated", nested[1][0])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "a[0] 10"));
        assert!(output.iter().any(|r| r.message == "a[1] 50"));
        assert!(output.iter().any(|r| r.message == "a[2] 40"));
        assert!(output.iter().any(|r| r.message == "nested[1][0] 3"));
        assert!(
            output
                .iter()
                .any(|r| r.message == "nested[1][0] mutated 99")
        );
    }

    #[test]
    fn test_array_indexing_errors() {
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };

        let cases = [
            ("const a = [1]\na[1.5]", "non-negative integer"),
            ("const a = [1]\na[-1]", "non-negative integer"),
            ("const a = [1]\na[\"0\"]", "expected number"),
            ("const a = [1]\na[2]", "out of bounds"),
        ];

        for (code, expected_err) in cases {
            let source = format!("entity Test {{ fn main() {{\n{}\n }} }}", code);
            let mut scene = create_scene(&source, &mut host);
            scene.start(&mut host).unwrap();
            let mut instance = scene
                .runtime
                .interpreter_mut()
                .instantiate_entity("Test", 1, &mut host)
                .unwrap();
            let result =
                scene
                    .runtime
                    .interpreter_mut()
                    .call(&mut instance, "main", vec![], &mut host);
            assert!(result.is_err(), "Expected error for: {}", code);
            assert!(result.unwrap_err().contains(expected_err));
        }
    }

    #[test]
    fn test_collection_reference_semantics() {
        let source = r#"
entity Test {
    fn main() {
        const a = [1, 2, 3]
        const b = a
        b[0] = 99
        debug.log("a[0]", a[0])

        const m = {}
        const n = m
        n["x"] = 10
        debug.log("m.x", m["x"])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "a[0] 99"));
        assert!(output.iter().any(|r| r.message == "m.x 10"));
    }

    #[test]
    fn test_map_literals_generalized() {
        let source = r#"
entity Test {
    fn main() {
        const m = { 1: "number", "1": "string" }
        debug.log("m[1]", m[1])
        debug.log("m['1']", m["1"])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let mut instance = scene
            .runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        scene
            .runtime
            .interpreter_mut()
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "m[1] number"));
        assert!(output.iter().any(|r| r.message == "m['1'] string"));
    }

    #[test]
    fn test_stale_binding_regression_block_identity() {
        let mut th = test_host();
        let coord = WorldCoord::new(1, 1, 1);

        let cell_id = add_authored_entity(
            &mut th.world,
            coord,
            CellType::Block,
            "AeoScriptIntegration",
        );

        let temp_dir = std::env::temp_dir().join("aeo_test_stale_regression");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();

        std::fs::write(
            temp_dir.join("scripts/test.aeo"),
            "entity AeoScriptIntegration {}",
        )
        .unwrap();

        let bindings = vec![ScriptBinding::new(cell_id, "scripts/test.aeo")];

        let scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .expect("Binding should be valid for Block with identity");

        let output = scene.output();
        let has_stale_warning = output.iter().any(|r| {
            r.message.contains("Stale script binding") && r.message.contains(&cell_id.to_string())
        });
        assert!(
            !has_stale_warning,
            "Should not report stale binding for valid block identity"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_lifecycle_field_persistence() {
        let source = r#"
entity LifecycleTest {
    ready_set: bool = false
    update_count: number = 0

    fn on_ready() {
        ready_set = true
    }

    fn update(dt: number) {
        if ready_set {
            update_count += 1
        }
    }
}
"#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "LifecycleTest",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_lifecycle_persistence");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/test.aeo"), source).unwrap();
        let bindings = vec![ScriptBinding::new(cell_id, "scripts/test.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 1.0 / 60.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        scene.update(1.0 / 60.0, &mut host).unwrap();

        scene.update(1.0 / 60.0, &mut host).unwrap();

        scene.update(1.0 / 60.0, &mut host).unwrap();

        let entity = &scene.entities[0];
        assert_eq!(
            entity.instance.get_field("ready_set"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            entity.instance.get_field("update_count"),
            Some(&Value::Number(2.0))
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_update_increment_persistence() {
        let source = r#"
entity Counter {
    ticks: number = 0
    fn update(dt: number) {
        ticks += 1
    }
}
"#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Counter",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_counter_persistence");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/test.aeo"), source).unwrap();
        let bindings = vec![ScriptBinding::new(cell_id, "scripts/test.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();
        scene.update(0.1, &mut host).unwrap();
        assert_eq!(
            scene.entities[0].instance.get_field("ticks"),
            Some(&Value::Number(1.0))
        );

        scene.update(0.1, &mut host).unwrap();
        assert_eq!(
            scene.entities[0].instance.get_field("ticks"),
            Some(&Value::Number(2.0))
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_update_wait_persistence() {
        let source = r#"
entity WaitTest {
    finished: bool = false
    fn update(dt: number) {
        wait(0.05)
        finished = true
    }
}
"#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "WaitTest",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_update_wait");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/test.aeo"), source).unwrap();
        let bindings = vec![ScriptBinding::new(cell_id, "scripts/test.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();
        assert_eq!(
            scene.entities[0].instance.get_field("finished"),
            Some(&Value::Bool(false))
        );

        scene.update(0.1, &mut host).unwrap();

        assert_eq!(
            scene.entities[0].instance.get_field("finished"),
            Some(&Value::Bool(true))
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_update_nested_wait_persistence() {
        let source = r#"
entity NestedWait {
    val: number = 0
    fn sub() {
        wait(0.05)
        val = 1
    }
    fn update(dt: number) {
        if val == 0 {
            sub()
        }
    }
}
"#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "NestedWait",
        );
        let temp_dir = std::env::temp_dir().join("aeo_test_nested_wait");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/test.aeo"), source).unwrap();
        let bindings = vec![ScriptBinding::new(cell_id, "scripts/test.aeo")];

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &bindings,
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();
        assert_eq!(
            scene.entities[0].instance.get_field("val"),
            Some(&Value::Number(0.0))
        );

        scene.update(0.1, &mut host).unwrap();
        assert_eq!(
            scene.entities[0].instance.get_field("val"),
            Some(&Value::Number(1.0))
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_cell_attributes_aeoscript_access() {
        use crate::world::cell::AttributeValue;
        let source = r#"
entity Test {
    fn on_spawn() {
        const chests = find("Chest")
        for chest in chests {
            debug.log(chest.attributes["coins"])
            debug.log(chest.attributes["difficulty"])
            debug.log(chest.attributes["locked"])
        }
    }
}
"#;
        let mut th = test_host();
        let c1 = WorldCoord::new(0, 0, 0);
        let c2 = WorldCoord::new(1, 1, 1);

        add_authored_entity(&mut th.world, c1, CellType::Block, "Chest");
        if let Some(cell) = th.world.get_mut(c1) {
            cell.attributes
                .insert("coins".to_string(), AttributeValue::Number(100.0));
            cell.attributes.insert(
                "difficulty".to_string(),
                AttributeValue::String("hard".to_string()),
            );
            cell.attributes
                .insert("locked".to_string(), AttributeValue::Bool(true));
        }

        add_authored_entity(&mut th.world, c2, CellType::Block, "Chest");
        if let Some(cell) = th.world.get_mut(c2) {
            cell.attributes
                .insert("coins".to_string(), AttributeValue::Number(50.0));
            cell.attributes.insert(
                "difficulty".to_string(),
                AttributeValue::String("easy".to_string()),
            );
            cell.attributes
                .insert("locked".to_string(), AttributeValue::Bool(false));
        }

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();

        assert!(output.iter().any(|r| r.message == "100"));
        assert!(output.iter().any(|r| r.message == "hard"));
        assert!(output.iter().any(|r| r.message == "true"));

        assert!(output.iter().any(|r| r.message == "50"));
        assert!(output.iter().any(|r| r.message == "easy"));
        assert!(output.iter().any(|r| r.message == "false"));
    }

    #[test]
    fn test_top_level_scene_execution() {
        let source = r#"
debug.log("global startup")
"#;
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join("aeo_test_top_level");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/startup.aeo"), source).unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(scene.output().iter().any(|r| r.message == "global startup"));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn blank_and_print_only_editor_scripts_run_without_lifecycle_warnings() {
        for (name, source) in [("blank", ""), ("hello", "print(\"Hello\")")] {
            let temp_dir = std::env::temp_dir().join(format!(
                "worldkiln_editor_baseline_{name}_{}",
                std::process::id()
            ));
            let mut editor = crate::editor::script_editor::ScriptEditor::new();
            let (path, _) = editor
                .create_new_script_in_root(&temp_dir, crate::project::ScriptRoot::Scripts, name)
                .unwrap();
            assert!(std::fs::read_to_string(&path).unwrap().is_empty());
            let document = editor.open_documents.get_mut(&path).unwrap();
            document.source = source.to_string();
            document.dirty = true;
            editor.save_active();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
            assert!(editor.output_log.is_empty());

            let mut th = test_host();
            let mut scene = ScriptScene::load_from_bindings(
                &temp_dir,
                &th.world,
                &[],
                &mut th.entity_manager,
                &std::collections::HashMap::new(),
                0.0,
            )
            .unwrap();
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut th,
            };
            scene.start(&mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
            assert!(
                scene.output().iter().all(|record| record.severity
                    != crate::scripting::log::LogSeverity::Warning
                    && record.severity != crate::scripting::log::LogSeverity::Error),
                "unexpected output: {:?}",
                scene.output()
            );
            if name == "hello" {
                assert_eq!(
                    scene
                        .output()
                        .iter()
                        .filter(|r| r.message == "Hello")
                        .count(),
                    1
                );
            } else {
                assert!(scene.output().is_empty());
            }
            std::fs::remove_dir_all(&temp_dir).unwrap();
        }
    }

    #[test]
    fn test_unattached_lifecycle_warning() {
        let source = r#"
entity Unattached {
    fn on_ready() {
        debug.log("ready")
    }
    fn update(dt: number) {}
}
"#;
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join("aeo_test_unattached_warning");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(temp_dir.join("scripts/unattached.aeo"), source).unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(
            |r| r.severity == crate::scripting::log::LogSeverity::Warning
                && r.message.contains("no script binding exists")
        ));
        assert!(!output.iter().any(|r| r.message == "ready"));
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn playerscripts_do_not_execute_as_standalone_modules() {
        let mut th = test_host();

        let temp_dir = std::env::temp_dir().join("aeo_test_playerscripts_are_player_modules");

        let _ = std::fs::remove_dir_all(&temp_dir);

        std::fs::create_dir_all(temp_dir.join("playerscripts")).unwrap();

        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();

        std::fs::write(
            temp_dir.join("playerscripts/combat.aeo"),
            r#"
debug.log("PLAYER_MODULE_RAN")
"#,
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &std::collections::HashMap::new(),
            0.0,
        )
        .unwrap();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        scene.update(0.0, &mut host).unwrap();

        assert!(
            !scene
                .output()
                .iter()
                .any(|record| { record.message == "PLAYER_MODULE_RAN" })
        );

        assert_eq!(
            scene.player_module_paths,
            vec!["playerscripts/combat.aeo".to_string()]
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_authored_attribute_preservation_regression() {
        use crate::world::cell::AttributeValue;
        let source = r#"
entity Test {
    fn on_spawn() {
        const c = find("Test")[0]
        c.attributes["testBool"] = false
        c.attributes["testNum"] = 42
        c.attributes["Test"] = "runtime"

        debug.log("runtime_bool", c.attributes["testBool"])
        debug.log("runtime_num", c.attributes["testNum"])
        debug.log("runtime_str", c.attributes["Test"])
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(0, 0, 0);
        add_authored_entity(&mut th.world, coord, CellType::Block, "Test");

        if let Some(cell) = th.world.get_mut(coord) {
            cell.attributes
                .insert("testBool".to_string(), AttributeValue::Bool(true));
            cell.attributes
                .insert("testNum".to_string(), AttributeValue::Number(10.0));
            cell.attributes.insert(
                "Test".to_string(),
                AttributeValue::String("original".to_string()),
            );
        }

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "runtime_bool false"));
        assert!(output.iter().any(|r| r.message == "runtime_num 42"));
        assert!(output.iter().any(|r| r.message == "runtime_str runtime"));

        let cell_id = th.world.get(coord).unwrap().id;
        {
            let cell = th.world.get(coord).unwrap();
            assert_eq!(
                cell.attributes.get("testBool"),
                Some(&AttributeValue::Bool(true))
            );
            assert_eq!(
                cell.attributes.get("testNum"),
                Some(&AttributeValue::Number(10.0))
            );
            assert_eq!(
                cell.attributes.get("Test"),
                Some(&AttributeValue::String("original".to_string()))
            );
        }

        th.world.clear_runtime_state();

        let mut dynamic_properties = std::collections::HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_enable_scripts = Vec::new();
        let mut pending_disable_scripts = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = crate::engine::mouse::MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();
        let mut bridge = crate::scripting::host::ScriptHostBridge {
            game_state: None,
            entity_manager: &mut th.entity_manager,
            attachment_system: None,
            world: &mut th.world,
            mouse: &mut mouse,
            dynamic_properties: &mut dynamic_properties,
            pending_events: &mut pending_events,
            test_results: &mut test_results,
            pending_enable_scripts: &mut pending_enable_scripts,
            pending_disable_scripts: &mut pending_disable_scripts,
            runtime_ui: &mut runtime_ui,
            viewport_size: [0.0, 0.0],
            keys_down: None,
            move_input: glam::Vec2::ZERO,
            jump_requested: false,
            orbit_delta: [0.0, 0.0],
            character_system: None,
            actor_system: None,
            gameplay_camera: None,
            valid_entity_declarations: None,
            pending_spawns: &mut pending_spawns,
            pending_destroys: &mut pending_destroys,
            project_path: None,
            save_state: None,
        };
        let effective = bridge
            .get_property(
                crate::scripting::value::HandleKind::Cell,
                cell_id,
                "attributes",
            )
            .unwrap()
            .unwrap();
        let map = effective.as_map().unwrap();
        let borrowed = map.borrow();
        assert_eq!(
            borrowed.get(&crate::scripting::value::MapKey::String(
                "testBool".to_string()
            )),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            borrowed.get(&crate::scripting::value::MapKey::String(
                "testNum".to_string()
            )),
            Some(&Value::Number(10.0))
        );
        assert_eq!(
            borrowed.get(&crate::scripting::value::MapKey::String("Test".to_string())),
            Some(&Value::String("original".to_string()))
        );
    }

    #[test]
    fn test_math_random_api() {
        let source = r#"
debug.log("r0", math.random())
debug.log("r1", math.random(1, 1))
debug.log("r3", math.random(1, 3))
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();

        let r0_str = output
            .iter()
            .find(|r| r.message.starts_with("r0"))
            .unwrap()
            .message
            .split_whitespace()
            .last()
            .unwrap();
        let r0: f64 = r0_str.parse().unwrap();
        assert!(r0 >= 0.0 && r0 < 1.0);

        assert!(output.iter().any(|r| r.message == "r1 1"));

        let r3_str = output
            .iter()
            .find(|r| r.message.starts_with("r3"))
            .unwrap()
            .message
            .split_whitespace()
            .last()
            .unwrap();
        let r3: f64 = r3_str.parse().unwrap();
        assert!(r3 == 1.0 || r3 == 2.0 || r3 == 3.0);
    }

    #[test]
    fn test_attribute_random_integration() {
        let source = r#"
const tests = find("Test")
const winner_idx = math.random(0, tests.len() - 1)
const winner_cell = tests[winner_idx]

for test in tests {
    test.attributes["winner"] = false
}

winner_cell.attributes["winner"] = true
debug.log("winner_id", winner_cell.id)
"#;
        let mut th = test_host();
        let c0 = WorldCoord::new(0, 0, 0);
        let c1 = WorldCoord::new(1, 1, 1);
        let c2 = WorldCoord::new(2, 2, 2);
        let id0 = add_authored_entity(&mut th.world, c0, CellType::Block, "Test");
        let id1 = add_authored_entity(&mut th.world, c1, CellType::Block, "Test");
        let id2 = add_authored_entity(&mut th.world, c2, CellType::Block, "Test");

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        let winner_id_str = output
            .iter()
            .find(|r| r.message.starts_with("winner_id"))
            .unwrap()
            .message
            .split_whitespace()
            .last()
            .unwrap();
        let winner_id: u64 = winner_id_str.parse::<f64>().unwrap() as u64;

        let ids = [id0, id1, id2];
        let mut true_count = 0;
        for &id in &ids {
            let coord = th.world.resolve_cell_id(id).unwrap();
            let effective_attr = th.world.get_effective_attribute(coord, "winner").unwrap();
            use crate::world::cell::AttributeValue;

            if id == winner_id {
                assert_eq!(effective_attr, AttributeValue::Bool(true));
                true_count += 1;
            } else {
                assert_eq!(effective_attr, AttributeValue::Bool(false));
            }

            let cell = th.world.get(coord).unwrap();
            assert!(
                !cell.attributes.contains_key("winner"),
                "Authored attributes must not be modified by runtime writes"
            );
        }
        assert_eq!(true_count, 1);
    }

    #[test]
    fn test_runtime_cell_creation_basic() {
        let source = r#"
const block = cell.new("Block")
block.position = [4, 5, 6]
block.color = [1, 0, 0]
block.visible = true
block.solid = true
block.anchored = true
block.attributes["owner"] = "Alpha"
block.name = "RuntimeBlock"
debug.log("created_id", block.id)
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        let id_str = output
            .iter()
            .find(|r| r.message.starts_with("created_id"))
            .unwrap()
            .message
            .split_whitespace()
            .last()
            .unwrap();
        let runtime_id: u64 = id_str.parse::<f64>().unwrap() as u64;

        assert!(th.world.runtime_cells.contains_key(&runtime_id));
        let coord = WorldCoord::new(4, 5, 6);
        assert_eq!(th.world.runtime_id_to_coord.get(&runtime_id), Some(&coord));

        assert_eq!(
            th.world.get_effective_color(coord),
            Vec3::new(1.0, 0.0, 0.0)
        );

        let cell = th.world.runtime_cells.get(&runtime_id).unwrap();
        assert_eq!(cell.entity_identity, Some("RuntimeBlock".to_string()));

        assert_eq!(
            th.world.get_effective_attribute(coord, "owner"),
            Some(crate::world::cell::AttributeValue::String(
                "Alpha".to_string()
            ))
        );
        assert!(th.world.is_cell_visible(coord));
        assert!(th.world.is_cell_solid(coord));
        assert!(th.world.is_cell_anchored(coord));

        assert!(th.world.cells.is_empty());
    }

    #[test]
    fn test_runtime_cell_types() {
        let cases = [
            ("Block", crate::scripting::value::HandleKind::Cell),
            ("FxBlock", crate::scripting::value::HandleKind::Cell),
            ("Player", crate::scripting::value::HandleKind::Cell),
            ("Light", crate::scripting::value::HandleKind::Light),
            ("SpawnPoint", crate::scripting::value::HandleKind::Cell),
        ];

        for (type_name, _) in cases {
            let mut th = test_host();
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut th,
            };

            let source = format!(
                "const c = cell.new(\"{}\")\ndebug.log(\"kind\", c.id)",
                type_name
            );
            let tokens = Lexer::new(&source).tokenize().unwrap();
            let program = Parser::new(tokens).parse().unwrap();
            let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
            scene.start(&mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();

            let id_str = scene
                .output()
                .iter()
                .find(|r| r.message.starts_with("kind"))
                .unwrap()
                .message
                .split_whitespace()
                .last()
                .unwrap();
            let id: u64 = id_str.parse::<f64>().unwrap() as u64;
            let cell = th.world.runtime_cells.get(&id).expect(type_name);
            assert_eq!(format!("{:?}", cell.cell_type), type_name);
        }

        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let source = "cell.new(\"Empty\")";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message.contains("Cannot create Empty cell"))
        );
    }

    #[test]
    fn test_runtime_reset_creation() {
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let source = "const c = cell.new(\"Block\")\nc.position = [1, 2, 3]";
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(!th.world.runtime_cells.is_empty());

        th.world.clear_runtime_state();
        assert!(th.world.runtime_cells.is_empty());
        assert!(th.world.runtime_id_to_coord.is_empty());
        assert!(th.world.coord_to_runtime_id.is_empty());
    }

    #[test]
    fn authored_source_handle_reads_and_writes_fractional_runtime_position() {
        let mut th = test_host();
        let authored = Vec3::new(1.0, 11.2, 0.0);
        let id = th.world.add_source(CellType::Light, authored);
        th.world.source_mut(id).unwrap().cell.entity_identity = Some("Lamp".into());
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let source = r#"
const lamp = find("Lamp")[0]
debug.log("before", lamp.position[0], lamp.position[1], lamp.position[2])
lamp.position = [2.5, 1.125, 55.75]
"#;
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert_eq!(
            th.world.source_position(id),
            Some(Vec3::new(2.5, 1.125, 55.75))
        );
        assert_eq!(th.world.source(id).unwrap().position, authored);
        th.world.clear_runtime_state();
        assert_eq!(th.world.source_position(id), Some(authored));
    }

    #[test]
    fn test_runtime_cell_deletion_authored() {
        use crate::world::cell::AttributeValue;
        let source = r#"
const green = find("Green")[0]
cell.delete(green)
debug.log("deleted")
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(-5, 18, 1);
        let cell_id = add_authored_entity(&mut th.world, coord, CellType::Block, "Green");

        if let Some(cell) = th.world.get_mut(coord) {
            cell.attributes
                .insert("test".to_string(), AttributeValue::Bool(true));
        }

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(scene.output().iter().any(|r| r.message == "deleted"));

        assert!(th.world.resolve_cell_id(cell_id).is_none());
        assert!(th.world.get_effective_cell(coord).is_none());
        assert!(!th.world.active_effective_blocks().contains(&coord));

        assert!(th.world.cells.contains_key(&coord));
        let auth_cell = th.world.get(coord).unwrap();
        assert_eq!(auth_cell.entity_identity, Some("Green".to_string()));
        assert_eq!(
            auth_cell.attributes.get("test"),
            Some(&AttributeValue::Bool(true))
        );

        th.world.clear_runtime_state();

        assert_eq!(th.world.resolve_cell_id(cell_id), Some(coord));
        assert!(th.world.get_effective_cell(coord).is_some());
    }

    #[test]
    fn test_runtime_cell_deletion_runtime_created() {
        let source = r#"
const block = cell.new("Block")
debug.log("created_id", block.id)
cell.delete(block)
debug.log("deleted")
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let output = scene.output();
        let id_str = output
            .iter()
            .find(|r| r.message.starts_with("created_id"))
            .unwrap()
            .message
            .split_whitespace()
            .last()
            .unwrap();
        let runtime_id: u64 = id_str.parse::<f64>().unwrap() as u64;

        assert!(output.iter().any(|r| r.message == "deleted"));

        assert!(!th.world.runtime_cells.contains_key(&runtime_id));
        assert!(th.world.resolve_cell_id(runtime_id).is_none());
    }

    #[test]
    fn test_runtime_cell_deletion_repeated() {
        let source = r#"
const block = cell.new("Block")
cell.delete(block)
cell.delete(block)
debug.log("ok")
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        let mut scene = ScriptScene::new(program, vec![], &mut host).unwrap();
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(scene.output().iter().any(|r| r.message == "ok"));
    }

    #[test]
    fn test_cell_contact_event_authored() {
        let source = r#"
entity Spike {
    fn on_touch(c) {
        debug.log("Touched Spike:", c.id)
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(0, 0, 0);
        let cell_id = add_authored_entity(&mut th.world, coord, CellType::Block, "Spike");

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.entities[0].associated_cell_id = Some(cell_id);

        scene.start(&mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);

        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == format!("Touched Spike: {}", cell_id))
        );
    }

    #[test]
    fn test_cell_contact_event_runtime_created_global() {
        let source = r#"
on on_touch(c) {
    debug.log("Touched Cell:", c.id)
}
"#;
        let mut th = test_host();
        let cell_id = th.world.create_runtime_cell(CellType::Block);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);

        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == format!("Touched Cell: {}", cell_id))
        );
    }

    #[test]
    fn test_cell_contact_event_script_deleted_during_callback() {
        let source = r#"
entity Trap {
    fn on_touch(c) {
        debug.log("Trap touched")
        cell.delete(c)
    }
}
"#;
        let mut th = test_host();
        let coord = WorldCoord::new(0, 0, 0);
        let cell_id = add_authored_entity(&mut th.world, coord, CellType::Block, "Trap");

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.entities[0].associated_cell_id = Some(cell_id);
        scene.start(&mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);

        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(scene.output().iter().any(|r| r.message == "Trap touched"));
        assert!(
            th.world.resolve_cell_id(cell_id).is_none(),
            "Cell should be deleted"
        );
    }

    #[test]
    fn test_cell_contact_event_no_on_touch() {
        let source = r#"
entity Silent {
    fn on_ready() { debug.log("ready") }
}
"#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(0, 0, 0),
            CellType::Block,
            "Silent",
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.entities[0].associated_cell_id = Some(cell_id);
        scene.start(&mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);

        assert!(scene.on_player_contact(&contacted, &mut host).is_ok());
        scene.update(0.0, &mut host).unwrap();
    }

    #[test]
    fn test_dynamic_event_handler_assignment() {
        let mut th = test_host();
        let cell_id = th.world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        let source = format!(
            r#"
const door = cell.get({})
door.on_touch = fn(c) {{
    debug.log("Dynamic Touch:", c.id)
    c.visible = false
}}
"#,
            cell_id
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(&source, &mut host);
        scene.start(&mut host).unwrap();

        scene.update(0.0, &mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);
        scene.on_player_contact(&contacted, &mut host).unwrap();

        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == format!("Dynamic Touch: {}", cell_id))
        );
        assert_eq!(th.world.is_cell_visible(WorldCoord::new(0, 0, 0)), false);
    }

    #[test]
    fn test_dynamic_method_call_with_self() {
        let mut th = test_host();
        let cell_id = th.world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        let source = format!(
            r#"
const door = cell.get({})
door.open = fn() {{
    debug.log("Opening door:", self.id)
    self.visible = false
}}
door:open()
"#,
            cell_id
        );

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(&source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == format!("Opening door: {}", cell_id))
        );
        assert_eq!(th.world.is_cell_visible(WorldCoord::new(0, 0, 0)), false);
    }

    #[test]
    fn test_dynamic_touch_handler_captures_lexical_scope() {
        let source = r#"
const doors = find("Door")

for door in doors {
    door.on_touch = fn(c) {
        debug.log("Door touched:", c.id)

        for target in doors {
            target.visible = false
            target.solid = false
        }

        wait(1)

        for target in doors {
            target.visible = true
            target.solid = true
        }
    }
}
"#;

        let mut th = test_host();
        let first = th.world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        th.world
            .get_mut(WorldCoord::new(0, 0, 0))
            .unwrap()
            .entity_identity = Some("Door".to_string());
        let _second = th.world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);
        th.world
            .get_mut(WorldCoord::new(1, 0, 0))
            .unwrap()
            .entity_identity = Some("Door".to_string());

        let first_coord = WorldCoord::new(0, 0, 0);
        let second_coord = WorldCoord::new(1, 0, 0);

        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut th,
            };

            let mut scene = create_scene(source, &mut host);
            scene.start(&mut host).unwrap();

            scene.update(0.0, &mut host).unwrap();

            let mut contacted = HashSet::new();
            contacted.insert(first);
            scene.on_player_contact(&contacted, &mut host).unwrap();

            scene.update(0.0, &mut host).unwrap();
            scene
        };

        assert!(!th.world.is_cell_visible(first_coord));
        assert!(!th.world.is_cell_visible(second_coord));
        assert!(!th.world.is_cell_solid(first_coord));
        assert!(!th.world.is_cell_solid(second_coord));
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == format!("Door touched: {}", first))
        );

        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut th,
            };
            scene.update(1.0, &mut host).unwrap();
        }

        assert!(th.world.is_cell_visible(first_coord));
        assert!(th.world.is_cell_visible(second_coord));
        assert!(th.world.is_cell_solid(first_coord));
        assert!(th.world.is_cell_solid(second_coord));
    }

    #[test]
    fn test_cell_overlap_events() {
        let source = r#"
entity Trigger {
    fn on_overlap(overlapping, c) {
        if overlapping {
            debug.log("Entered:", c.id)
        } else {
            debug.log("Exited:", c.id)
        }
    }
}
"#;
        let mut th = test_host();
        let cell_id = th.world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.entities[0].associated_cell_id = Some(cell_id);
        scene.start(&mut host).unwrap();

        scene.on_player_overlap(cell_id, true, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == format!("Entered: {}", cell_id))
        );

        scene.on_player_overlap(cell_id, false, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|r| r.message == format!("Exited: {}", cell_id))
        );
    }

    #[test]
    fn test_play_stop_play_lifecycle_reset() {
        let mut test_results = std::collections::BTreeMap::new();
        let mut pending_events = Vec::new();
        let mut pending_enable = Vec::new();
        let mut pending_disable = Vec::new();
        let mut dynamic_properties = std::collections::HashMap::new();
        let mut authored_disabled_scripts: Vec<String> = Vec::new();

        let mut world = World::new();
        world.disabled_scripts = vec!["initial.aeo".to_string()];
        let mut em = EntityManager::new();

        authored_disabled_scripts = world.disabled_scripts.clone();

        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut mouse = crate::engine::mouse::MouseController::default();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();
        {
            let mut bridge = crate::scripting::host::ScriptHostBridge {
                game_state: None,
                entity_manager: &mut em,
                attachment_system: None,
                world: &mut world,
                mouse: &mut mouse,
                dynamic_properties: &mut dynamic_properties,
                pending_events: &mut pending_events,
                test_results: &mut test_results,
                pending_enable_scripts: &mut pending_enable,
                pending_disable_scripts: &mut pending_disable,
                runtime_ui: &mut runtime_ui,
                viewport_size: [0.0, 0.0],
                keys_down: None,
                move_input: glam::Vec2::ZERO,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: None,
                actor_system: None,
                gameplay_camera: None,
                valid_entity_declarations: None,
                pending_spawns: &mut pending_spawns,
                pending_destroys: &mut pending_destroys,
                project_path: None,
                save_state: None,
            };
            bridge.complete_test("leak_test", true);
            bridge.fire_event("leak_event", vec![]);
            bridge.disable_script("runtime_disabled.aeo");
        }

        test_results.clear();
        pending_events.clear();
        pending_enable.clear();
        pending_disable.clear();
        dynamic_properties.clear();
        world.disabled_scripts = std::mem::take(&mut authored_disabled_scripts);

        assert!(test_results.is_empty(), "Test results should be cleared");
        assert!(
            pending_events.is_empty(),
            "Pending events should be cleared"
        );
        assert_eq!(
            world.disabled_scripts,
            vec!["initial.aeo".to_string()],
            "World script state should be restored"
        );
        assert!(
            authored_disabled_scripts.is_empty(),
            "Snapshot should be consumed"
        );
    }

    #[test]
    fn test_audio_emitter_creation_and_defaults() {
        use crate::world::Cell;
        let cell = Cell::new_audio_emitter();
        assert_eq!(cell.cell_type, CellType::AudioEmitter);
        assert_eq!(cell.texture, "speaker");
        assert_eq!(cell.audio, "");
        assert_eq!(cell.playing, false);
        assert_eq!(cell.looped, false);
        assert_eq!(cell.volume, 1.0);
        assert_eq!(cell.visible, false);
        assert_eq!(cell.solid, false);
    }

    #[test]
    fn test_audio_emitter_persistence_full() {
        use crate::world::persistence::{load_world, save_world};
        use std::fs;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let project = std::env::temp_dir().join(format!(
            "worldkiln_script_audio_persistence_{}_{}",
            std::process::id(),
            timestamp
        ));

        fs::create_dir_all(&project).unwrap();

        let path = project.join("world.dat");

        let mut world = World::new();

        let coord = WorldCoord::new(1, 2, 3);
        let id = world.add_source(
            CellType::AudioEmitter,
            glam::Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32),
        );

        if let Some(source) = world.source_mut(id) {
            source.cell.audio = "battle/sword-unsheathe.wav".to_string();
            source.cell.playing = true;
            source.cell.looped = true;
            source.cell.volume = 0.75;
        }

        save_world(&world, &path).unwrap();

        let mut loaded_world = World::new();

        load_world(&mut loaded_world, &path).unwrap();

        let loaded = &loaded_world.source(id).unwrap().cell;

        assert_eq!(loaded.id, id);
        assert_eq!(loaded.cell_type, CellType::AudioEmitter);
        assert_eq!(loaded.audio, "battle/sword-unsheathe.wav");
        assert!(loaded.playing);
        assert!(loaded.looped);
        assert!((loaded.volume - 0.75).abs() < 1e-6);

        fs::remove_dir_all(project).ok();
    }

    #[test]
    fn test_audio_emitter_three_authored_configurations() {
        use crate::world::persistence::{load_world, save_world};
        use std::fs;
        use std::path::Path;

        let mut world = World::new();

        let c1 = WorldCoord::new(0, 0, 0);
        let id1 = world.add_source(
            CellType::AudioEmitter,
            glam::Vec3::new(c1.x as f32, c1.y as f32, c1.z as f32),
        );
        if let Some(source) = world.source_mut(id1) {
            source.cell.audio = "misc/ambient.wav".to_string();
            source.cell.playing = true;
            source.cell.looped = true;
            source.cell.volume = 1.0;
        }

        let c2 = WorldCoord::new(1, 0, 0);
        let id2 = world.add_source(
            CellType::AudioEmitter,
            glam::Vec3::new(c2.x as f32, c2.y as f32, c2.z as f32),
        );
        if let Some(source) = world.source_mut(id2) {
            source.cell.audio = "battle/swing.wav".to_string();
            source.cell.playing = false;
            source.cell.looped = false;
            source.cell.volume = 0.9;
        }

        let c3 = WorldCoord::new(2, 0, 0);
        let id3 = world.add_source(
            CellType::AudioEmitter,
            glam::Vec3::new(c3.x as f32, c3.y as f32, c3.z as f32),
        );
        if let Some(source) = world.source_mut(id3) {
            source.cell.audio = "interface/alarm.wav".to_string();
            source.cell.playing = false;
            source.cell.looped = true;
            source.cell.volume = 0.5;
        }

        let path = Path::new("test_audio_configs.dat");
        save_world(&world, path).unwrap();

        let mut loaded = World::new();
        load_world(&mut loaded, path).unwrap();

        let l1 = &loaded.source(id1).unwrap().cell;
        assert_eq!(l1.playing, true);
        assert_eq!(l1.looped, true);

        let l2 = &loaded.source(id2).unwrap().cell;
        assert_eq!(l2.playing, false);
        assert_eq!(l2.looped, false);

        let l3 = &loaded.source(id3).unwrap().cell;
        assert_eq!(l3.playing, false);
        assert_eq!(l3.looped, true);

        fs::remove_file(path).ok();
    }

    #[test]
    fn test_aeoscript_sound_object_and_methods() {
        let source = r#"
const speaker = find("Speaker")[0]
const sound = speaker.sound
sound.set("audio/ghost_attack_slash.wav")
sound.play()
sound.volume = 0.5
sound.looped = true
sound.MinDistance = 4
sound.MaxDistance = 80
"#;

        let mut th = test_host();
        let coord = WorldCoord::new(0, 0, 0);
        let cell_id = th.world.set_cell(coord, CellType::AudioEmitter);
        if let Some(cell) = th.world.get_mut(coord) {
            cell.entity_identity = Some("Speaker".to_string());
            cell.audio = "world/door.wav".to_string();
            cell.playing = false;
            cell.looped = false;
            cell.volume = 1.0;
        }

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        assert_eq!(host.engine.is_audio_playing(cell_id), Some(false));

        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert_eq!(host.engine.is_audio_playing(cell_id), Some(true));
        assert_eq!(host.engine.is_audio_looped(cell_id), Some(true));
        assert_eq!(host.engine.get_audio_volume(cell_id), Some(0.5));
        assert_eq!(host.engine.get_audio_min_distance(cell_id), Some(4.0));
        assert_eq!(host.engine.get_audio_max_distance(cell_id), Some(80.0));

        host.engine.audio_pause(cell_id);
        assert_eq!(host.engine.is_audio_paused(cell_id), Some(true));

        host.engine.audio_stop(cell_id);
        assert_eq!(host.engine.is_audio_playing(cell_id), Some(false));
        drop(host);
        assert_eq!(th.world.get_audio_path(cell_id), "ghost_attack_slash.wav");
    }

    #[test]
    fn entity_sound_set_uses_transient_following_emitter() {
        let mut th = test_host();
        let id = th.entity_manager.create_entity("Ghost");
        th.entity_manager.get_entity_mut(id).unwrap().position = Vec3::new(1.0, 2.0, 3.0);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(
            r#"const ghost = get_entity("Ghost")
ghost.sound.set("audio/ghost_attack_slash.wav")
ghost.sound.play()
"#,
            &mut host,
        );
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        drop(host);
        let emitter = th.world.resolve_sound_id(id.0);
        assert_ne!(emitter, id.0);
        assert_eq!(th.world.get_audio_path(id.0), "ghost_attack_slash.wav");
        assert!(th.world.is_audio_playing(id.0));
        th.entity_manager.get_entity_mut(id).unwrap().position = Vec3::new(9.0, 8.0, 7.0);
        th.world.sync_entity_sound_positions(&th.entity_manager);
        assert_eq!(
            th.world.runtime_audio_position(emitter),
            Some(Vec3::new(9.0, 8.0, 7.0))
        );
        assert!(
            th.world
                .set_audio_path_runtime(emitter, "../not_audio.wav")
                .is_err()
        );
        th.world.clear_runtime_state();
        assert_eq!(th.world.resolve_sound_id(id.0), id.0);
    }

    #[test]
    fn test_aeoscript_find_and_multiple_speakers() {
        let source = r#"
const speakers = find("Speakers")
for speaker in speakers {
    speaker.sound.play()
}
"#;

        let mut th = test_host();

        let c1 = WorldCoord::new(0, 0, 0);
        let id1 = th.world.set_cell(c1, CellType::AudioEmitter);
        if let Some(cell) = th.world.get_mut(c1) {
            cell.entity_identity = Some("Speakers".to_string());
            cell.audio = "battle/spell.wav".to_string();
        }

        let c2 = WorldCoord::new(1, 0, 0);
        let id2 = th.world.set_cell(c2, CellType::AudioEmitter);
        if let Some(cell) = th.world.get_mut(c2) {
            cell.entity_identity = Some("Speakers".to_string());
            cell.audio = "battle/magic1.wav".to_string();
        }

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };

        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert_eq!(host.engine.is_audio_playing(id1), Some(true));
        assert_eq!(host.engine.is_audio_playing(id2), Some(true));
    }

    #[test]
    fn test_runtime_audio_state_reset_and_authored_preservation() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        let id = world.set_cell(coord, CellType::AudioEmitter);

        if let Some(cell) = world.get_mut(coord) {
            cell.audio = "world/door.wav".to_string();
            cell.playing = false;
            cell.looped = false;
            cell.volume = 1.0;
        }

        world.audio_play_runtime(id);
        world.set_audio_looped_runtime(id, true);
        world.set_audio_volume_runtime(id, 0.25);

        assert_eq!(world.is_audio_playing(id), true);
        assert_eq!(world.is_audio_looped(id), true);
        assert_eq!(world.get_audio_volume(id), 0.25);

        let authored = world.get(coord).unwrap();
        assert_eq!(authored.playing, false);
        assert_eq!(authored.looped, false);
        assert_eq!(authored.volume, 1.0);

        world.clear_runtime_state();

        assert_eq!(world.is_audio_playing(id), false);
        assert_eq!(world.is_audio_looped(id), false);
        assert_eq!(world.get_audio_volume(id), 1.0);
    }

    #[test]
    fn test_audio_emitter_startup_behavior() {
        let mut world = World::new();

        let c1 = WorldCoord::new(0, 0, 0);
        let id1 = world.set_cell(c1, CellType::AudioEmitter);
        if let Some(cell) = world.get_mut(c1) {
            cell.playing = true;
        }

        let c2 = WorldCoord::new(1, 0, 0);
        let id2 = world.set_cell(c2, CellType::AudioEmitter);
        if let Some(cell) = world.get_mut(c2) {
            cell.playing = false;
        }

        assert_eq!(world.is_audio_playing(id1), true);
        assert_eq!(world.is_audio_playing(id2), false);
    }

    #[test]
    fn test_script_spawn_npc_valid_declaration() {
        let source = r#"
entity Villager {
    fn on_spawn() {
        debug.log("Villager on_spawn")
    }
    fn on_ready() {
        debug.log("Villager on_ready")
    }
    fn update(dt) {
        debug.log("Villager update")
    }
}

entity Spawner {
    fn on_spawn() {
        const npc = spawn("Villager", [10, 1, 5])
        debug.log("Spawned ID:", npc.id)
    }
}
"#;
        let mut th = test_host();
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene = create_scene(source, &mut host);
            scene.start(&mut host).unwrap();

            scene.update(0.1, &mut host).unwrap();
            scene
        };

        let villager_id = th
            .entity_manager
            .lookup_entity("Villager")
            .expect("Villager entity created");
        assert_eq!(
            th.entity_manager.get_position(villager_id),
            Some(Vec3::new(10.0, 1.0, 5.0))
        );
        let villager_character = th
            .character_system
            .get_character_for_entity(villager_id)
            .expect("two-argument spawn uses the default character package");
        assert_eq!(villager_character.package_name, "Rogue");

        {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            scene.update(0.1, &mut host).unwrap();
        }
        let output = scene.output();
        assert!(output.iter().any(|r| r.message == "Villager on_spawn"));
    }

    #[test]
    fn test_script_spawn_npc_uses_requested_character_package() {
        let source = r#"
entity Spawner {
    fn on_spawn() {
        spawn("HeroNPC", [10, 1, 5], "Rogue")
    }
}
"#;

        let mut th = test_host();

        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };

        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.1, &mut host).unwrap();

        let npc_id = th
            .entity_manager
            .lookup_entity("HeroNPC")
            .expect("HeroNPC entity created");

        let character = th
            .character_system
            .get_character_for_entity(npc_id)
            .expect("HeroNPC has a runtime character");

        assert_eq!(character.package_name, "Rogue");
    }

    #[test]
    fn test_script_spawn_npc_empty_name_fails() {
        let source = r#"
entity Spawner {
    fn on_spawn() {
        spawn("", [10, 1, 5])
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message.contains("Entity name cannot be empty"))
        );
    }

    #[test]
    fn test_script_spawn_npc_event_driven_without_entity_block() {
        let source = r#"
on on_villager_spawn(npc) {
    debug.log("NPC Event Received:", npc.name)
}

entity Spawner {
    fn on_spawn() {
        const villager = spawn("Villager", [10, 1, 5])
        event.fire("on_villager_spawn", villager)
    }
}
"#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.1, &mut host).unwrap();
        scene.update(0.1, &mut host).unwrap();

        let villager_id = th
            .entity_manager
            .lookup_entity("Villager")
            .expect("Villager entity created");
        assert_eq!(
            th.entity_manager.get_position(villager_id),
            Some(Vec3::new(10.0, 1.0, 5.0))
        );
        assert!(th.character_system.has_characters());

        let output = scene.output();
        assert!(
            output
                .iter()
                .any(|r| r.message.contains("NPC Event Received: Villager"))
        );
    }

    #[test]
    fn test_multiple_npc_spawns_and_cleanup() {
        let source = r#"
entity Guard {
    fn on_spawn() {
        debug.log("Guard spawned")
    }
}

entity Spawner {
    fn on_spawn() {
        const g1 = spawn("Guard", [10, 1, 5])
        const g2 = spawn("Guard", [14, 1, 5])
        debug.log("G1 ID:", g1.id)
        debug.log("G2 ID:", g2.id)
    }
}
"#;
        let mut th = test_host();
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene = create_scene(source, &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
            scene
        };

        assert_eq!(th.character_system.get_active_characters().count(), 2);

        {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let _ = scene.stop(&mut host);
        }
        th.character_system.clear();
        th.entity_manager.clear();

        assert!(!th.character_system.has_characters());
        assert!(!th.entity_manager.validate_handle(1));
    }

    #[test]
    fn entity_is_valid_returns_false_for_destroyed_entity() {
        let mut em = EntityManager::new();
        let entity_id = em.create_entity("TestEntity");
        let id_raw = entity_id.0;
        em.remove_entity(entity_id);

        let mut world = World::new();
        let mut mouse = crate::engine::mouse::MouseController::default();
        let mut dynamic_properties = HashMap::new();
        let mut pending_events = Vec::new();
        let mut test_results = BTreeMap::new();
        let mut pending_enable = Vec::new();
        let mut pending_disable = Vec::new();
        let mut runtime_ui = crate::engine::ui::RuntimeUi::new();
        let mut pending_spawns = Vec::new();
        let mut pending_destroys = Vec::new();

        let mut bridge = crate::scripting::host::ScriptHostBridge {
            game_state: None,
            entity_manager: &mut em,
            attachment_system: None,
            world: &mut world,
            mouse: &mut mouse,
            dynamic_properties: &mut dynamic_properties,
            pending_events: &mut pending_events,
            test_results: &mut test_results,
            pending_enable_scripts: &mut pending_enable,
            pending_disable_scripts: &mut pending_disable,
            runtime_ui: &mut runtime_ui,
            viewport_size: [800.0, 600.0],
            keys_down: None,
            move_input: glam::Vec2::ZERO,
            jump_requested: false,
            orbit_delta: [0.0, 0.0],
            character_system: None,
            actor_system: None,
            gameplay_camera: None,
            valid_entity_declarations: None,
            pending_spawns: &mut pending_spawns,
            pending_destroys: &mut pending_destroys,
            project_path: None,
            save_state: None,
        };

        use crate::scripting::api::EngineHost;
        let res = bridge
            .call_method(HandleKind::Entity, id_raw, "is_valid", &[])
            .unwrap();
        assert_eq!(res, Some(Value::Bool(false)));
    }

    #[test]
    fn cell_delete_rejects_wrong_handle_kind() {
        let source = r#"
            entity Test {
                fn on_spawn() {}
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        let wrong_kinds = vec![
            Value::Handle {
                kind: HandleKind::Entity,
                id: 1,
            },
            Value::Handle {
                kind: HandleKind::Ui,
                id: 1,
            },
            Value::Handle {
                kind: HandleKind::Sound,
                id: 1,
            },
            Value::Handle {
                kind: HandleKind::Mouse,
                id: 1,
            },
        ];

        for bad_handle in wrong_kinds {
            let mut inst = ScriptInstance::new_empty();
            let mut scopes = vec![];
            let res = crate::scripting::stdlib::call_stdlib_function(
                scene.runtime.interpreter_mut(),
                &mut inst,
                &mut scopes,
                "cell",
                "delete",
                &[bad_handle],
                &mut host,
            );
            assert!(
                res.is_err(),
                "cell.delete must reject non-cell/light handles"
            );
        }

        let cell_handle = Value::Handle {
            kind: HandleKind::Cell,
            id: 999,
        };
        let mut inst1 = ScriptInstance::new_empty();
        let mut scopes1 = vec![];
        let res_cell = crate::scripting::stdlib::call_stdlib_function(
            scene.runtime.interpreter_mut(),
            &mut inst1,
            &mut scopes1,
            "cell",
            "delete",
            &[cell_handle],
            &mut host,
        );
        assert!(res_cell.is_ok(), "cell.delete must accept Cell handle");

        let light_handle = Value::Handle {
            kind: HandleKind::Light,
            id: 999,
        };
        let mut inst2 = ScriptInstance::new_empty();
        let mut scopes2 = vec![];
        let res_light = crate::scripting::stdlib::call_stdlib_function(
            scene.runtime.interpreter_mut(),
            &mut inst2,
            &mut scopes2,
            "cell",
            "delete",
            &[light_handle],
            &mut host,
        );
        assert!(res_light.is_ok(), "cell.delete must accept Light handle");
    }

    #[test]
    fn queued_event_start_failure_is_isolated_and_reported_once() {
        let source = r#"
            on my_event(a) {
                debug.log(a)
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        host.engine.fire_event("my_event", vec![]);

        scene.update(0.1, &mut host).unwrap();
        host.engine.fire_event("my_event", vec![]);
        scene.update(0.1, &mut host).unwrap();
        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message.contains("expected 1 argument"))
                .count(),
            1,
            "a failed event handler must be removed instead of failing every dispatch"
        );
    }

    #[test]
    fn viewport_resized_dispatches_event_only_on_actual_change() {
        let mut th = test_host();
        let temp_dir =
            std::env::temp_dir().join(format!("Worldkiln_viewport_resize_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/hud.aeo"),
            "resized: bool = false\nlast_w: number = 0\non.viewport_resized(w, h) {\n    resized = true\n    last_w = w\n}\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();

        host.engine.set_viewport_size(1280.0, 720.0);
        scene.update(0.0, &mut host).unwrap();

        host.engine.set_viewport_size(1280.0, 720.0);
        assert_eq!(host.engine.get_viewport_size(), [1280.0, 720.0]);

        host.engine.set_viewport_size(1920.0, 1080.0);
        scene.update(0.0, &mut host).unwrap();
        assert_eq!(host.engine.get_viewport_size(), [1920.0, 1080.0]);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn nested_module_runtime_error_reports_callee_file_and_line() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_script_error_line_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/interact.aeo"),
            "on check() {\n    scripts.conversation.aeo.is_open()\n}\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/conversation.aeo"),
            "fn is_open() {\n    return missing_open_state\n}\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.dispatch_event("check", vec![], &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        let error = &scene
            .output()
            .iter()
            .find(|record| record.message.contains("missing_open_state"))
            .expect("the isolated script error must remain observable")
            .message;
        assert!(error.contains("scripts/conversation.aeo:2"), "{error}");
        assert!(error.contains("is_open"), "{error}");
        assert!(
            error.contains("unknown variable 'missing_open_state'"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn nested_module_function_can_read_its_top_level_state() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_module_top_level_state_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/interact.aeo"),
            "on check() {\n    scripts.conversation.aeo.is_open()\n}\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/conversation.aeo"),
            "open: bool = false\nfn is_open() { return open }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.dispatch_event("check", vec![], &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn caller_keeps_its_top_level_scope_after_nested_module_call() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_nested_module_caller_scope_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/equipment")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/hud.aeo"),
            "bag_item: string = \"SATCHEL\"\nfn refresh_bag() {\n    const owned = scripts.equipment.blade.aeo.owns_blade()\n    debug.log(bag_item, owned)\n}\non refresh() { refresh_bag() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/equipment/blade.aeo"),
            "fn owns_blade() { return true }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.dispatch_event("refresh", vec![], &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "SATCHEL true")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn caller_keeps_top_level_state_after_two_nested_module_calls() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_nested_module_scope_chain_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/systems")).unwrap();
        std::fs::create_dir_all(temp_dir.join("scripts/data")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/hud.aeo"),
            "bag_item: string = \"SATCHEL\"\nfn refresh_bag() {\n    const inventory = scripts.systems.inventory.aeo\n    const text = inventory.summary()\n    debug.log(bag_item, text)\n}\non refresh() { refresh_bag() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/systems/inventory.aeo"),
            "fn summary() {\n    const items = scripts.data.items.aeo\n    const ids = items.all_ids()\n    return \"Empty \" + ids.len()\n}\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/data/items.aeo"),
            "fn all_ids() { return [\"key\", \"bread\"] }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.dispatch_event("refresh", vec![], &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "SATCHEL Empty 2")
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn caller_runtime_error_after_nested_module_call_reports_caller_file() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_nested_module_caller_error_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/systems")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/hud.aeo"),
            "fn refresh_bag() {\n    const text = scripts.systems.inventory.aeo.summary()\n    return missing_hud_state\n}\non refresh() { refresh_bag() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/systems/inventory.aeo"),
            "fn summary() { return \"Empty\" }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.dispatch_event("refresh", vec![], &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        let error = &scene
            .output()
            .iter()
            .find(|record| record.message.contains("missing_hud_state"))
            .expect("the isolated script error must remain observable")
            .message;
        assert!(error.contains("scripts/hud.aeo:3"), "{error}");
        assert!(error.contains("refresh_bag"), "{error}");
        assert!(
            error.contains("unknown variable 'missing_hud_state'"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn event_handler_keeps_function_ownership_across_nested_dialogue_calls() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_nested_dialogue_ownership_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/dialogue")).unwrap();
        std::fs::create_dir_all(temp_dir.join("scripts/systems")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/interact.aeo"),
            "on.keypress(key, down) {\n    if down == false { return }\n    const talk = scripts.dialogue.blacksmith.aeo\n    if talk.is_open() {\n        talk.advance()\n        return\n    }\n    talk.show()\n}\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/blacksmith.aeo"),
            "fn is_open() { return scripts.dialogue.presenter.aeo.is_open() }\nfn show() {\n    if scripts.systems.economy.aeo.can_afford(20) { debug.log(\"trade\") }\n}\nfn advance() { scripts.dialogue.presenter.aeo.close() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/presenter.aeo"),
            "open: bool = false\nfn is_open() { return open }\nfn close() { open = false }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/systems/economy.aeo"),
            "fn can_afford(amount) { return amount <= 20 }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene
            .dispatch_event(
                "keypress",
                vec![Value::String("E".into()), Value::Bool(true)],
                &mut host,
            )
            .unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "trade")
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn equipment_presenter_keeps_top_level_state_after_system_call() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_equipment_presenter_scope_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/equipment")).unwrap();
        std::fs::create_dir_all(temp_dir.join("scripts/systems")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/equipment/main_hand_visual.aeo"),
            "main_hand_entity: string? = \"hatchet\"\nshown_item: string? = \"worn_hatchet\"\nfn clear_visual() {\n    debug.log(main_hand_entity, shown_item)\n    main_hand_entity = nil\n    shown_item = nil\n}\nfn sync_visual() {\n    const item_id = scripts.systems.equipment.aeo.equipped(\"main_hand\")\n    if item_id == nil {\n        clear_visual()\n        return false\n    }\n    return true\n}\nsync_visual()\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/systems/equipment.aeo"),
            "fn equipped(slot_id) { return nil }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "hatchet worn_hatchet")
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn interaction_router_keeps_same_named_conversation_methods_isolated() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_conversation_router_ownership_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/dialogue")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/interact.aeo"),
            "on.keypress(key, down) {\n    if down == false || key != \"E\" { return }\n    const presenter = scripts.dialogue.presenter.aeo\n    const steward = scripts.dialogue.steward.aeo\n    const forester = scripts.dialogue.forester.aeo\n    if presenter.is_open() {\n        presenter.close()\n        return\n    }\n    if steward.is_open() {\n        steward.advance()\n        return\n    }\n    if forester.is_open() {\n        forester.advance()\n        return\n    }\n    steward.show()\n    forester.show()\n}\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/presenter.aeo"),
            "open: bool = false\nfn is_open() { return open }\nfn show(name, message) { debug.log(name, message) }\nfn close() { open = false }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/steward.aeo"),
            "fn is_open() { return false }\nfn show() { scripts.dialogue.presenter.aeo.show(\"EDRIN\", \"watch\") }\nfn advance() { scripts.dialogue.presenter.aeo.close() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/forester.aeo"),
            "fn is_open() { return false }\nfn show() { scripts.dialogue.presenter.aeo.show(\"ROWAN\", \"roots\") }\nfn advance() { scripts.dialogue.presenter.aeo.close() }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        scene
            .dispatch_event(
                "keypress",
                vec![Value::String("E".into()), Value::Bool(true)],
                &mut host,
            )
            .unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "EDRIN watch")
        );
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "ROWAN roots")
        );
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn modules_with_same_function_name_keep_code_arity_and_top_level_state_isolated() {
        let mut th = test_host();
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_module_function_ownership_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("scripts/equipment")).unwrap();
        std::fs::create_dir_all(temp_dir.join("scripts/dialogue")).unwrap();
        std::fs::write(
            temp_dir.join("scripts/hud.aeo"),
            "bag_item: string = \"SATCHEL\"\nfn refresh_bag() { debug.log(\"hud\", bag_item) }\non refresh_hud() { refresh_bag() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/equipment/blade.aeo"),
            "bag_item: string = \"BLADE\"\nfn refresh_bag() { debug.log(\"blade\", bag_item) }\non refresh_blade() { refresh_bag() }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/blacksmith.aeo"),
            "fn keypress(key, pressed) { debug.log(key, pressed) }\non smith_input() { keypress(\"E\", true) }\n",
        )
        .unwrap();
        std::fs::write(
            temp_dir.join("scripts/dialogue/presenter.aeo"),
            "fn keypress() { debug.log(\"presenter\") }\non presenter_input() { keypress() }\n",
        )
        .unwrap();

        let mut scene = ScriptScene::load_from_bindings(
            &temp_dir,
            &th.world,
            &[],
            &mut th.entity_manager,
            &HashMap::new(),
            0.0,
        )
        .unwrap();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.start(&mut host).unwrap();
        for event in [
            "refresh_hud",
            "refresh_blade",
            "smith_input",
            "presenter_input",
        ] {
            scene.dispatch_event(event, vec![], &mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
        }

        let messages: Vec<_> = scene
            .output()
            .iter()
            .map(|record| record.message.as_str())
            .collect();
        assert!(messages.contains(&"hud SATCHEL"), "{messages:?}");
        assert!(messages.contains(&"blade BLADE"), "{messages:?}");
        assert!(messages.contains(&"E true"), "{messages:?}");
        assert!(messages.contains(&"presenter"), "{messages:?}");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_complete_rejects_extra_arguments() {
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut interpreter = Interpreter::new(crate::scripting::ast::Program {
            span: crate::scripting::source::SourceSpan::new(0, 0),
            declarations: vec![],
            statements: vec![],
        });

        let res_api_extra = call_host_function(
            &mut host,
            "complete_test",
            &[
                Value::String("x".to_string()),
                Value::Bool(true),
                Value::String("extra".to_string()),
            ],
        );
        assert!(
            res_api_extra.is_err(),
            "complete_test must reject 3 arguments"
        );

        let res_api_ok = call_host_function(
            &mut host,
            "complete_test",
            &[Value::String("x".to_string()), Value::Bool(true)],
        );
        assert!(
            res_api_ok.is_ok(),
            "complete_test with 2 arguments must succeed"
        );

        let mut inst3 = ScriptInstance::new_empty();
        let mut scopes3 = vec![];
        let res_stdlib_extra = crate::scripting::stdlib::call_stdlib_function(
            &mut interpreter,
            &mut inst3,
            &mut scopes3,
            "test",
            "complete",
            &[
                Value::String("x".to_string()),
                Value::Bool(true),
                Value::String("extra".to_string()),
            ],
            &mut host,
        );
        assert!(
            res_stdlib_extra.is_err(),
            "test.complete must reject 3 arguments"
        );

        let mut inst4 = ScriptInstance::new_empty();
        let mut scopes4 = vec![];
        let res_stdlib_ok = crate::scripting::stdlib::call_stdlib_function(
            &mut interpreter,
            &mut inst4,
            &mut scopes4,
            "test",
            "complete",
            &[Value::String("x".to_string()), Value::Bool(true)],
            &mut host,
        );
        assert!(
            res_stdlib_ok.is_ok(),
            "test.complete with 2 arguments must succeed"
        );
    }

    #[test]
    fn scheduler_removes_terminal_tasks() {
        let source = r#"
            entity Test {
                fn on_spawn() {}
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        scene.update(0.1, &mut host).unwrap();

        assert_eq!(scene.runtime.fiber_count(), 0);
        assert_eq!(scene.runtime.scheduler().task_count(), 0);
    }

    #[test]
    fn repeated_update_does_not_grow_scheduler_forever() {
        let source = r#"
            entity T {
                fn update(dt) {
                    const x = 1
                }
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        for _ in 0..10_000 {
            scene.update(0.1, &mut host).unwrap();
        }

        assert!(
            scene.runtime.scheduler().task_count() <= 1,
            "scheduler task count must remain bounded by active entities (got {})",
            scene.runtime.scheduler().task_count()
        );
        assert!(
            scene.runtime.fiber_count() <= 1,
            "runtime fiber count must remain bounded by active entities (got {})",
            scene.runtime.fiber_count()
        );
    }

    #[test]
    fn dynamic_callback_records_assigning_script() {
        let source = r#"
            const handle = cell.new("Block")
            handle.on_touch = fn(c) { const x = 1 }
        "#;
        let mut th = test_host();
        {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene = create_scene_with_path(source, "scripts/owner.aeo", &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
        }

        let (_, prop) = th
            .dynamic_properties
            .values()
            .next()
            .expect("dynamic properties should exist")
            .iter()
            .next()
            .expect("on_touch property should exist");
        assert_eq!(prop.owner_script.as_deref(), Some("scripts/owner.aeo"));
        assert!(matches!(prop.value, Value::Function(_, _, _)));
    }

    #[test]
    fn dynamic_callback_fiber_inherits_owner() {
        let source = r#"
            const handle = cell.new("Block")
            handle.on_touch = fn(c) { const x = 1 }
        "#;
        let mut th = test_host();
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene = create_scene_with_path(source, "scripts/owner.aeo", &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
            scene
        };

        let (&(kind, id), _) = th.dynamic_properties.iter().next().unwrap();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        scene
            .spawn_dynamic_handler(kind, id, "on_touch", vec![Value::Nil], None, &mut host)
            .unwrap();

        let fibers = scene.runtime.fibers();
        assert_eq!(fibers.len(), 1);
        let fiber = fibers.values().next().unwrap();
        assert_eq!(fiber.script_path.as_deref(), Some("scripts/owner.aeo"));
    }

    #[test]
    fn disabled_script_cannot_execute_owned_dynamic_callback() {
        let source = r#"
            const handle = cell.new("Block")
            handle.on_touch = fn(c) { wait(10) }
        "#;
        let mut th = test_host();
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene = create_scene_with_path(source, "scripts/owner.aeo", &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
            scene
        };

        let (&(kind, id), _) = th.dynamic_properties.iter().next().unwrap();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        scene
            .spawn_dynamic_handler(kind, id, "on_touch", vec![Value::Nil], None, &mut host)
            .unwrap();
        assert_eq!(scene.runtime.fiber_count(), 1);

        scene.disable_script("scripts/owner.aeo", &mut host);
        assert_eq!(scene.runtime.fiber_count(), 0);

        scene.update(15.0, &mut host).unwrap();
        assert_eq!(scene.runtime.fiber_count(), 0);
    }

    #[test]
    fn disabled_script_drops_owned_dynamic_callbacks() {
        let mut th = test_host();

        let source_a = r#"
            const h1 = cell.new("Block")
            h1.on_touch = fn(c) { const x = 1 }
        "#;
        let source_b = r#"
            const h2 = cell.new("Block")
            h2.on_touch = fn(c) { const y = 2 }
        "#;

        let (mut scene_a, mut scene_b) = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene_a = create_scene_with_path(source_a, "scripts/script_a.aeo", &mut host);
            scene_a.start(&mut host).unwrap();
            scene_a.update(0.1, &mut host).unwrap();

            let mut scene_b = create_scene_with_path(source_b, "scripts/script_b.aeo", &mut host);
            scene_b.start(&mut host).unwrap();
            scene_b.update(0.1, &mut host).unwrap();
            (scene_a, scene_b)
        };

        let initial_props: Vec<_> = th
            .dynamic_properties
            .values()
            .flat_map(|m| m.values())
            .collect();
        assert_eq!(initial_props.len(), 2);

        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        scene_a.disable_script("scripts/script_a.aeo", &mut host);

        let remaining_props: Vec<_> = th
            .dynamic_properties
            .values()
            .flat_map(|m| m.values())
            .collect();
        assert_eq!(
            remaining_props.len(),
            1,
            "Only Script B's callback should remain"
        );
        assert_eq!(
            remaining_props[0].owner_script.as_deref(),
            Some("scripts/script_b.aeo")
        );
    }

    #[test]
    fn disabled_script_does_not_affect_other_script_fibers() {
        let mut th = test_host();

        let source_a = r#"
            const h1 = cell.new("Block")
            h1.on_touch = fn(c) { wait(10) }
        "#;
        let source_b = r#"
            const h2 = cell.new("Block")
            h2.on_touch = fn(c) { wait(10) }
        "#;

        let (mut scene_a, mut scene_b) = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene_a = create_scene_with_path(source_a, "scripts/script_a.aeo", &mut host);
            scene_a.start(&mut host).unwrap();
            scene_a.update(0.1, &mut host).unwrap();

            let mut scene_b = create_scene_with_path(source_b, "scripts/script_b.aeo", &mut host);
            scene_b.start(&mut host).unwrap();
            scene_b.update(0.1, &mut host).unwrap();
            (scene_a, scene_b)
        };

        let a_key = th
            .dynamic_properties
            .iter()
            .find(|(_, m)| {
                m.values()
                    .any(|p| p.owner_script.as_deref() == Some("scripts/script_a.aeo"))
            })
            .map(|(&k, _)| k)
            .unwrap();
        let b_key = th
            .dynamic_properties
            .iter()
            .find(|(_, m)| {
                m.values()
                    .any(|p| p.owner_script.as_deref() == Some("scripts/script_b.aeo"))
            })
            .map(|(&k, _)| k)
            .unwrap();

        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };

        scene_a
            .spawn_dynamic_handler(
                a_key.0,
                a_key.1,
                "on_touch",
                vec![Value::Nil],
                None,
                &mut host,
            )
            .unwrap();
        scene_b
            .spawn_dynamic_handler(
                b_key.0,
                b_key.1,
                "on_touch",
                vec![Value::Nil],
                None,
                &mut host,
            )
            .unwrap();

        assert_eq!(
            scene_a.runtime.fiber_count() + scene_b.runtime.fiber_count(),
            2
        );

        scene_a.disable_script("scripts/script_a.aeo", &mut host);

        let b_fibers: Vec<_> = scene_b
            .runtime
            .fibers()
            .values()
            .filter(|f| f.script_path.as_deref() == Some("scripts/script_b.aeo"))
            .collect();
        assert_eq!(b_fibers.len(), 1, "Script B's fiber must remain active");
    }

    #[test]
    fn re_enable_script_recreates_dynamic_callback() {
        let source = r#"
            const h = cell.new("Block")
            h.on_touch = fn(c) { const x = 1 }
        "#;
        let mut th = test_host();
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            let mut scene = create_scene_with_path(source, "scripts/owner.aeo", &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
            scene
        };

        let initial_props: Vec<_> = th
            .dynamic_properties
            .values()
            .flat_map(|m| m.values())
            .collect();
        assert_eq!(initial_props.len(), 1);

        {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            scene.disable_script("scripts/owner.aeo", &mut host);
        }

        let disabled_props: Vec<_> = th
            .dynamic_properties
            .values()
            .flat_map(|m| m.values())
            .collect();
        assert_eq!(disabled_props.len(), 0);

        {
            let mut host = HostContext {
                delta_time: 0.1,
                engine: &mut th,
            };
            scene.enable_script("scripts/owner.aeo");
            scene.start(&mut host).unwrap();
            scene.update(0.1, &mut host).unwrap();
        }

        let recreated_props: Vec<_> = th
            .dynamic_properties
            .values()
            .flat_map(|m| m.values())
            .collect();
        assert_eq!(recreated_props.len(), 1);
        assert_eq!(
            recreated_props[0].owner_script.as_deref(),
            Some("scripts/owner.aeo")
        );
    }

    #[test]
    fn on_destroy_error_is_observable() {
        let source_a = r#"
            entity EntityA {
                fn on_destroy() {
                    const error = 1 / 0
                }
            }
        "#;
        let source_b = r#"
            entity EntityB {
                fn on_destroy() {
                    cell.new("Block")
                }
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene_a = create_scene(source_a, &mut host);
        scene_a.start(&mut host).unwrap();
        let mut scene_b = create_scene(source_b, &mut host);
        scene_b.start(&mut host).unwrap();

        scene_a.entities.append(&mut scene_b.entities);

        let res = scene_a.stop(&mut host);
        assert!(res.is_err(), "stop() must return Err when on_destroy fails");
        let err_msg = res.unwrap_err();
        assert!(err_msg.contains("EntityA"), "Error must identify EntityA");

        assert!(
            scene_a.entities.is_empty(),
            "Entities must be cleared during stop"
        );
    }

    #[test]
    fn destroy_entity_reports_on_destroy_error_after_runtime_cleanup() {
        let source = r#"
            entity Test {
                fn on_destroy() {
                    const error = 1 / 0
                }
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        let entity_id = scene.entities[0].instance.id();

        let result = scene.destroy_entity(EntityId(entity_id), &mut host);
        assert!(
            result
                .as_ref()
                .is_err_and(|err| err.contains("on_destroy") && err.contains("Test")),
            "individual destruction must report on_destroy failures: {result:?}"
        );
        drop(host);
        assert!(scene.entities.is_empty());
        assert!(
            !th.entity_manager.validate_handle(entity_id),
            "runtime entity must be removed even when on_destroy fails"
        );
    }

    #[test]
    fn queued_entity_destruction_continues_after_on_destroy_failure() {
        let source = r#"
            entity Broken {
                fn on_destroy() {
                    const error = 1 / 0
                }
            }
            entity Healthy { }
        "#;
        let mut th = test_host();
        let broken_id = th.entity_manager.create_entity("Broken").0;
        let healthy_id = th.entity_manager.create_entity("Healthy").0;
        th.pending_destroys.extend([broken_id, healthy_id]);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        let result = scene.update(0.0, &mut host);

        assert!(result.is_err_and(|err| err.contains("Broken") && err.contains("on_destroy")));
        assert!(
            scene.entities.is_empty(),
            "a failing callback must not discard later queued destruction requests"
        );
    }

    #[test]
    fn deleted_cell_stops_its_bound_script_and_calls_on_destroy() {
        let source = r#"
            entity CellScript {
                fn update(dt) { debug.log("CELL_UPDATE") }
                fn on_destroy() { debug.log("CELL_DESTROYED") }
            }
        "#;
        let mut th = test_host();
        let cell_id = add_authored_entity(
            &mut th.world,
            WorldCoord::new(1, 0, 0),
            CellType::Block,
            "CellScript",
        );
        th.entity_manager.create_entity("CellScript");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.entities[0].associated_cell_id = Some(cell_id);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        host.engine.delete_cell(cell_id).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        assert!(scene.entities.is_empty());
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "CELL_DESTROYED")
        );
        assert_eq!(
            scene
                .output()
                .iter()
                .filter(|record| record.message == "CELL_UPDATE")
                .count(),
            1,
            "deleted Cell script must not receive further update callbacks"
        );
    }

    #[test]
    fn entity_destroy_request_runs_bound_on_destroy_and_cancels_tasks() {
        let source = r#"
            entity Target {
                fn on_spawn() {
                    wait(10)
                    debug.log("TARGET_AFTER_DESTROY")
                }
                fn on_destroy() { debug.log("TARGET_DESTROYED") }
            }
        "#;
        let mut th = test_host();
        let entity_id = th.entity_manager.create_entity("Target");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert_eq!(scene.runtime.fiber_count(), 1);

        drop(host);
        th.entity_manager.remove_entity(entity_id);
        th.pending_destroys.push(entity_id.0);
        let mut host = HostContext {
            delta_time: 20.0,
            engine: &mut th,
        };
        scene.update(20.0, &mut host).unwrap();

        assert!(scene.entities.is_empty());
        assert_eq!(scene.runtime.fiber_count(), 0);
        assert!(
            scene
                .output()
                .iter()
                .any(|record| record.message == "TARGET_DESTROYED")
        );
        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message == "TARGET_AFTER_DESTROY")
        );
    }

    #[test]
    fn deleting_cell_cancels_its_suspended_dynamic_handler() {
        let source = r#"
            const handle = cell.new("Block")
            handle.on_touch = fn(c) {
                wait(5)
                debug.log("AFTER_CELL_DELETE")
            }
        "#;
        let mut th = test_host();
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut th,
            };
            let mut scene = create_scene_with_path(source, "scripts/cell_handler.aeo", &mut host);
            scene.start(&mut host).unwrap();
            scene.update(0.0, &mut host).unwrap();
            scene
        };
        let (&(kind, cell_id), _) = th
            .dynamic_properties
            .iter()
            .next()
            .expect("Cell callback should be registered");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene
            .spawn_dynamic_handler(kind, cell_id, "on_touch", vec![Value::Nil], None, &mut host)
            .unwrap();
        scene.update(0.0, &mut host).unwrap();
        assert_eq!(scene.runtime.fiber_count(), 1);

        drop(host);
        th.world.delete_cell_runtime(cell_id);
        let mut host = HostContext {
            delta_time: 10.0,
            engine: &mut th,
        };
        scene.update(10.0, &mut host).unwrap();
        drop(host);

        assert_eq!(scene.runtime.fiber_count(), 0);
        assert!(!th.dynamic_properties.contains_key(&(kind, cell_id)));
        assert!(
            !scene
                .output()
                .iter()
                .any(|record| record.message == "AFTER_CELL_DELETE")
        );
    }

    #[test]
    fn on_destroy_is_synchronous_and_non_yielding() {
        let source = r#"
            entity Test {
                fn on_destroy() {
                    wait(1)
                }
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        let res = scene.stop(&mut host);
        assert!(res.is_err(), "yielding inside on_destroy must return Err");
        let err_msg = res.unwrap_err();
        assert!(
            err_msg.contains("yielding operation"),
            "Error must report synchronous call context constraint (got {})",
            err_msg
        );
        assert_eq!(scene.runtime.fiber_count(), 0);
    }

    #[test]
    fn hierarchy_contract_remains_explicitly_undefined() {
        let th = test_host();
        use crate::scripting::api::EngineHost;
        assert_eq!(th.get_parent(HandleKind::Entity, 1), None);
        assert!(th.get_children(HandleKind::Entity, 1).is_empty());
    }

    #[test]
    fn map_key_normalizes_negative_zero() {
        let pos_zero = Value::Number(0.0).as_map_key().unwrap();
        let neg_zero = Value::Number(-0.0).as_map_key().unwrap();
        assert_eq!(pos_zero, neg_zero);
    }

    #[test]
    fn map_retrieval_treats_zero_and_negative_zero_as_same_key() {
        let source = r#"
            const m = {}
            m[0.0] = "zero_value"
            test.complete("zero_test", m[-0.0] == "zero_value")
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        scene.update(0.1, &mut host).unwrap();

        assert_eq!(th.test_results.get("zero_test"), Some(&true));
    }

    #[test]
    fn nan_map_key_is_rejected() {
        let nan_res = Value::Number(f64::NAN).as_map_key();
        assert!(nan_res.is_err());
    }

    #[test]
    fn legacy_scene_scope_and_event_scope_are_consistent() {
        let source = r#"
            const shared_counter = 10
            on test_event() {
                debug.log("event_ran")
            }
        "#;
        let mut th = test_host();
        let mut host = HostContext {
            delta_time: 0.1,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();

        scene
            .dispatch_event("test_event", vec![], &mut host)
            .unwrap();
        scene.update(0.1, &mut host).unwrap();

        let val = scene.runtime.top_level_scope().get("shared_counter");
        assert_eq!(val, Some(Value::Number(10.0)));
    }

    #[test]
    fn test_project_script_roots_load_both_scripts_and_playerscripts() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_scene_script_roots_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        if root.exists() {
            std::fs::remove_dir_all(&root).ok();
        }

        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("playerscripts")).unwrap();

        std::fs::write(root.join("scripts/main.aeo"), "entity Main {}").unwrap();

        std::fs::write(
            root.join("playerscripts/controller.aeo"),
            "entity Controller {}",
        )
        .unwrap();

        let mut loaded = HashMap::new();
        let mut failures = HashMap::new();

        load_project_script_roots(&root, &mut loaded, &mut failures)
            .expect("Both project script roots should load");
        assert!(failures.is_empty());

        assert!(loaded.contains_key("scripts/main.aeo"));
        assert!(loaded.contains_key("playerscripts/controller.aeo"));

        assert_eq!(loaded.len(), 2);

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn profile_scripts_load_as_standalone_global_modules() {
        let user_data = std::env::temp_dir().join(format!(
            "worldkiln_profile_scripts_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = user_data.join("Project");
        std::fs::create_dir_all(user_data.join("scripts")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            user_data.join("scripts/combat.aeo"),
            "on.keypress(key, down) { return }",
        )
        .unwrap();

        let mut loaded = HashMap::new();
        let mut failures = HashMap::new();
        load_profile_script_root(&project, &mut loaded, &mut failures).unwrap();
        assert!(failures.is_empty());

        assert!(loaded.contains_key("profile/scripts/combat.aeo"));
        std::fs::remove_dir_all(user_data).ok();
    }

    #[test]
    fn test_normalize_script_path_preserves_playerscripts_root() {
        assert_eq!(
            ScriptScene::normalize_script_path("playerscripts/controller.aeo"),
            "playerscripts/controller.aeo"
        );
    }
}

#[cfg(test)]
mod stars_tween_scene_tests {
    use super::tests::{create_scene, test_host};
    use crate::project::material::{
        MaterialCatalog, MaterialDefinition, MaterialDocument, MaterialLayer, MaterialLayerBlend,
        MaterialOutput,
    };
    use crate::scripting::api::HostContext;
    use std::collections::BTreeMap;

    #[test]
    fn stars_aeoscript_sky_layer_alpha_tweens_through_live_reference() {
        let source = r#"
fn tweenStart(targetObject, propertyName, targetValue, duration) {
    engine.tweens.create({
        target: targetObject,
        property: propertyName,
        endValue: targetValue,
        time: duration,
        easing: "quadOut"
    })
}
entity Test {
    fn on_spawn() {
        const sky = proj.sky
        if sky != nil {
            const starLayer = sky.material.find_layer("Stars")
            if starLayer != nil {
                starLayer.alpha = 0.0
                tweenStart(starLayer, "alpha", 1.0, 2.0)
            }
        }
    }
}
"#;
        let mut host_state = test_host();
        host_state.entity_manager.create_entity("Test");
        let mut sky = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Sky(crate::project::sky_material::SkyGradient::default()),
        });
        sky.layers.push(MaterialLayer {
            name: "Stars".into(),
            texture: "stars.png".into(),
            opacity: 0.6,
            blend: MaterialLayerBlend::Normal,
        });
        let material_id = sky.id.clone();
        let mut documents = BTreeMap::new();
        documents.insert("Sky.MAT".into(), sky);
        host_state.world.sky.material_asset = Some(material_id.clone());
        host_state.world.runtime_material_catalog =
            Some(MaterialCatalog::from_documents(documents));
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut host_state,
        };
        let mut scene = create_scene(source, &mut host);
        scene.start(&mut host).unwrap();
        for _ in 0..3 {
            scene.update(0.0, &mut host).unwrap();
        }
        assert_eq!(
            host.engine
                .material_layer_alpha(&material_id, "Stars")
                .unwrap(),
            Some(0.0)
        );
        scene.update(1.0, &mut host).unwrap();
        let mid = host
            .engine
            .material_layer_alpha(&material_id, "Stars")
            .unwrap()
            .unwrap();
        assert!(
            (mid - 0.75).abs() < 0.0001,
            "expected quadOut midpoint; got {mid}"
        );
        scene.update(1.0, &mut host).unwrap();
        assert_eq!(
            host.engine
                .material_layer_alpha(&material_id, "Stars")
                .unwrap(),
            Some(1.0)
        );
    }
}

#[cfg(test)]
mod generic_handle_tween_scene_tests {
    use super::tests::{create_scene, test_host};
    use crate::scripting::api::{EngineHost, HostContext};
    use crate::scripting::value::Value;

    fn numbers(value: Value) -> Vec<f64> {
        let basket = value.as_basket().expect("expected numeric vector");
        let out = basket
            .borrow()
            .elements
            .iter()
            .map(|v| v.as_number().unwrap())
            .collect();
        out
    }

    #[test]
    fn aeoscript_ui_handle_vector_tween_uses_same_live_property_setter() {
        let script = r#"
entity Test {
    fn on_spawn() {
        const panel = ui.new("Panel")
        panel.position = [0, 0]
        engine.tweens.create({
            target: panel,
            property: "position",
            endValue: [100, 40],
            time: 2.0,
            easing: "linear"
        })
    }
}
"#;
        let mut state = test_host();
        state.entity_manager.create_entity("Test");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut state,
        };
        let mut scene = create_scene(script, &mut host);
        scene.start(&mut host).unwrap();
        for _ in 0..4 {
            scene.update(0.0, &mut host).unwrap();
        }
        let start = numbers(host.engine.get_ui_property(1, "position").unwrap().unwrap());
        assert_eq!(start, vec![0.0, 0.0]);
        scene.update(1.0, &mut host).unwrap();
        let mid = numbers(host.engine.get_ui_property(1, "position").unwrap().unwrap());
        assert_eq!(mid, vec![50.0, 20.0]);
        scene.update(1.0, &mut host).unwrap();
        let end = numbers(host.engine.get_ui_property(1, "position").unwrap().unwrap());
        assert_eq!(end, vec![100.0, 40.0]);
    }

    #[test]
    fn aeoscript_ui_direct_assignment_overrides_running_tween() {
        let script = r#"
entity Test {
    fn on_spawn() {
        const panel=ui.new("Panel")
        panel.position=[0,0]
        engine.tweens.create({
            target:panel,
            property:"position",
            endValue:[100,40],
            time:2.0
        })
        panel.position=[7,9]
    }
}
"#;
        let mut state = test_host();
        state.entity_manager.create_entity("Test");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut state,
        };
        let mut scene = create_scene(script, &mut host);
        scene.start(&mut host).unwrap();
        for _ in 0..4 {
            scene.update(0.0, &mut host).unwrap();
        }
        scene.update(2.0, &mut host).unwrap();
        let end = numbers(host.engine.get_ui_property(1, "position").unwrap().unwrap());
        assert_eq!(end, vec![7.0, 9.0]);
    }
}

#[cfg(test)]
mod light_tween_scene_tests {
    use super::tests::{create_scene, test_host};
    use crate::scripting::api::HostContext;

    #[test]
    fn aeoscript_tweens_real_emitted_light_intensity_and_color() {
        let script = r#"
entity Test {
    fn on_spawn() {
        const lamp = cell.new("Light")
        lamp.intensity = 0.0
        lamp.color = [1, 0, 0]
        engine.tweens.create({
            target: lamp, property:"intensity", endValue:10.0, time:2.0
        })
        engine.tweens.create({
            target: lamp, property:"color", endValue:[0,0,1], time:2.0
        })
    }
}
"#;
        let mut state = test_host();
        state.entity_manager.create_entity("Test");
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            let mut scene = create_scene(script, &mut host);
            scene.start(&mut host).unwrap();
            for _ in 0..4 {
                scene.update(0.0, &mut host).unwrap();
            }
            scene
        };
        let light_id = state
            .world
            .iter_light_ids()
            .next()
            .expect("runtime Light was not created");
        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        let (color, intensity, _, _) = state.world.emitted_light_runtime(light_id).unwrap();
        assert!(
            (intensity - 5.0).abs() < 0.01,
            "intermediate emitted intensity {intensity}"
        );
        assert!(
            (color.x - 0.5).abs() < 0.01 && (color.z - 0.5).abs() < 0.01,
            "intermediate emitted RGB {color:?}"
        );
        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        let (color, intensity, _, _) = state.world.emitted_light_runtime(light_id).unwrap();
        assert!((intensity - 10.0).abs() < 0.01);
        assert!((color.x).abs() < 0.01 && (color.z - 1.0).abs() < 0.01);
    }
}

#[cfg(test)]
mod numeric_attribute_tween_scene_tests {
    use super::tests::{create_scene, test_host};
    use crate::scripting::api::HostContext;
    use crate::world::cell::AttributeValue;
    #[test]
    fn scalar_custom_attribute_uses_the_existing_live_attribute_store() {
        let script = r#"
entity Test {
    fn on_spawn() {
        const block=cell.new("Block")
        block.attributes["energy"]=0
        engine.tweens.create({
            target:block, property:"attributes.energy", endValue:20, time:2.0
        })
    }
}
"#;
        let mut state = test_host();
        state.entity_manager.create_entity("Test");
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            let mut scene = create_scene(script, &mut host);
            scene.start(&mut host).unwrap();
            for _ in 0..4 {
                scene.update(0.0, &mut host).unwrap();
            }
            scene
        };
        let id = *state
            .world
            .runtime_cells
            .keys()
            .next()
            .expect("No runtime Block");
        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert_eq!(
            state.world.runtime_state[&id]
                .attribute_overrides
                .get("energy"),
            Some(&AttributeValue::Number(10.0))
        );
        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert_eq!(
            state.world.runtime_state[&id]
                .attribute_overrides
                .get("energy"),
            Some(&AttributeValue::Number(20.0))
        );
    }
}

#[cfg(test)]
mod numeric_audio_tween_scene_tests {
    use super::tests::{create_scene, test_host};
    use crate::scripting::api::HostContext;
    #[test]
    fn one_tween_api_changes_actual_audio_emitter_gain() {
        let script = r#"
entity Test {
    fn on_spawn() {
        const emitter=cell.new("AudioEmitter")
        emitter.sound.volume=0.0
        engine.tweens.create({
            target:emitter.sound, property:"volume", endValue:0.8,
            time:2.0,easing:"linear"
        })
    }
}
"#;
        let mut state = test_host();
        state.entity_manager.create_entity("Test");
        let mut scene = {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            let mut scene = create_scene(script, &mut host);
            scene.start(&mut host).unwrap();
            for _ in 0..4 {
                scene.update(0.0, &mut host).unwrap();
            }
            scene
        };
        let id = *state
            .world
            .runtime_cells
            .keys()
            .next()
            .expect("No runtime AudioEmitter");
        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert!((state.world.get_audio_volume(id) - 0.4).abs() < 0.0001);
        {
            let mut host = HostContext {
                delta_time: 0.0,
                engine: &mut state,
            };
            scene.update(1.0, &mut host).unwrap();
        }
        assert!((state.world.get_audio_volume(id) - 0.8).abs() < 0.0001);
    }
}
