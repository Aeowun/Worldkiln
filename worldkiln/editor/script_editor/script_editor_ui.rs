use super::ScriptEditor;
use super::script_editor_files::script_root_for_path;
use super::script_editor_highlighting::{
    aeoscript_editor_theme, aeoscript_syntax, byte_offset_to_char_index, char_index_to_line,
    line_start_char_index,
};
use crate::project::ScriptRoot;
use egui::{Color32, RichText};
use egui_code_editor::CodeEditor;
use std::path::PathBuf;

pub struct SearchResult {
    pub path: PathBuf,
    pub line: usize,
    pub text: String,
}

const SCRIPT_API_DOC_LINKS: &[(&str, &str)] = &[
    ("Reference Index", "https://aeowun.com/docs/reference/"),
    (
        "Global Functions",
        "https://aeowun.com/docs/reference/globals/",
    ),
    ("Cells", "https://aeowun.com/docs/reference/cell/"),
    ("UI", "https://aeowun.com/docs/reference/ui/"),
    (
        "Script Control",
        "https://aeowun.com/docs/reference/script/",
    ),
    (
        "Actors & Modules",
        "https://aeowun.com/docs/reference/project/",
    ),
    ("Events", "https://aeowun.com/docs/reference/event/"),
    ("Physics", "https://aeowun.com/docs/reference/physics/"),
    ("Math", "https://aeowun.com/docs/reference/math/"),
    ("Baskets", "https://aeowun.com/docs/reference/basket/"),
    ("Strings", "https://aeowun.com/aeoscript/api/string/"),
    ("Entities", "https://aeowun.com/aeoscript/api/entity/"),
];

fn dotted_symbol_at_char_index(source: &str, char_index: usize) -> Option<String> {
    let chars: Vec<char> = source.chars().collect();

    if chars.is_empty() {
        return None;
    }

    let is_symbol_char =
        |character: char| character.is_ascii_alphanumeric() || character == '_' || character == '.';

    let mut index = char_index.min(chars.len().saturating_sub(1));

    // Galley cursors describe insertion points. When the click lands just after
    // a symbol, prefer the symbol immediately to the left.
    if !is_symbol_char(chars[index]) {
        if index == 0 || !is_symbol_char(chars[index - 1]) {
            return None;
        }

        index -= 1;
    }

    let mut start = index;
    while start > 0 && is_symbol_char(chars[start - 1]) {
        start -= 1;
    }

    let mut end = index + 1;
    while end < chars.len() && is_symbol_char(chars[end]) {
        end += 1;
    }

    let symbol: String = chars[start..end].iter().collect();
    let symbol = symbol.trim_matches('.');

    if symbol.is_empty() {
        None
    } else {
        Some(symbol.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScriptApiDocTarget {
    label: &'static str,
    url: &'static str,
    description: &'static str,
}

fn script_api_doc_for_symbol(symbol: &str) -> Option<ScriptApiDocTarget> {
    let parts: Vec<&str> = symbol.split('.').filter(|part| !part.is_empty()).collect();
    let root = *parts.first()?;
    let leaf = *parts.last()?;

    let direct = match root {
        "math" => Some(ScriptApiDocTarget {
            label: "Math",
            url: "https://aeowun.com/docs/reference/math/",
            description: "Numeric helpers for clamping, interpolation, trigonometry, rounding, powers, and random values.",
        }),
        "basket" => Some(ScriptApiDocTarget {
            label: "Basket",
            url: "https://aeowun.com/docs/reference/basket/",
            description: "Create and manipulate zero-indexed reference-backed collections, including insert, remove, sort, clone, and concat.",
        }),
        "string" => Some(ScriptApiDocTarget {
            label: "String",
            url: "https://aeowun.com/aeoscript/api/string/",
            description: "Inspect and transform Unicode text with length, case conversion, reverse, and split helpers.",
        }),
        "cell" | "light" => Some(ScriptApiDocTarget {
            label: "Cells",
            url: "https://aeowun.com/docs/reference/cell/",
            description: "Create, query, inspect, and modify runtime or authored World cells and cell-backed lights.",
        }),
        "entity" => Some(ScriptApiDocTarget {
            label: "Entities",
            url: "https://aeowun.com/aeoscript/api/entity/",
            description: "Look up live runtime Entities and work with their movement, health, animation, appearance, physics, and attributes.",
        }),
        "physics" => Some(ScriptApiDocTarget {
            label: "Physics",
            url: "https://aeowun.com/docs/reference/physics/",
            description: "Read and manipulate runtime physics state, apply forces and impulses, or perform supported collision queries.",
        }),
        "ui" | "control" => Some(ScriptApiDocTarget {
            label: "UI",
            url: "https://aeowun.com/docs/reference/ui/",
            description: "Create, find, configure, and remove runtime UI controls including panels, text, buttons, anchors, and callbacks.",
        }),
        "script" => Some(ScriptApiDocTarget {
            label: "Script Control",
            url: "https://aeowun.com/docs/reference/script/",
            description: "Enable, disable, and inspect script execution state for project scripts and player scripts.",
        }),
        "event" | "on" => Some(ScriptApiDocTarget {
            label: "Events",
            url: "https://aeowun.com/docs/reference/event/",
            description: "Fire gameplay events and respond to engine or script events such as keypress, touch, overlap, and navigation.",
        }),
        "proj" => Some(ScriptApiDocTarget {
            label: "Project Discovery",
            url: "https://aeowun.com/docs/reference/project/",
            description: "Find authored Actors, Cells, folders, and project-scoped objects through the authored project hierarchy.",
        }),
        "player" | "scripts" | "playerscripts" | "self" | "object" | "folder" => {
            Some(ScriptApiDocTarget {
                label: "Actors & Modules",
                url: "https://aeowun.com/docs/reference/project/",
                description: "Work with player modules, reusable project modules, Actor-bound script context, and authored object references.",
            })
        }
        "input" => Some(ScriptApiDocTarget {
            label: "Input",
            url: "https://aeowun.com/docs/reference/",
            description: "Read movement, jump, orbit, and physical-key input from the active runtime input state.",
        }),
        "camera" => Some(ScriptApiDocTarget {
            label: "Camera",
            url: "https://aeowun.com/docs/reference/",
            description: "Read the horizontal camera basis and set runtime camera position, target, or orientation.",
        }),
        "get" => Some(ScriptApiDocTarget {
            label: "Runtime Handles",
            url: "https://aeowun.com/docs/reference/globals/",
            description: "Resolve engine-provided runtime handles such as the mouse interface used by player scripts.",
        }),
        "test" => Some(ScriptApiDocTarget {
            label: "Automated Test API",
            url: "https://aeowun.com/docs/reference/",
            description: "Complete named in-game tests and inspect completion, pass state, and aggregate test results.",
        }),
        "save" => Some(ScriptApiDocTarget {
            label: "Typed Save State",
            url: "https://aeowun.com/docs/reference/",
            description: "Read, stage, remove, and atomically commit typed persistent save values for the current project.",
        }),
        "debug" => Some(ScriptApiDocTarget {
            label: "Debug Output",
            url: "https://aeowun.com/docs/reference/globals/",
            description: "Write structured diagnostic messages to Worldkiln's script output and diagnostics surfaces.",
        }),
        "time" => Some(ScriptApiDocTarget {
            label: "Frame Timing",
            url: "https://aeowun.com/docs/reference/globals/",
            description: "Access runtime frame timing such as time.delta for frame-rate-independent script behavior.",
        }),
        _ => None,
    };

    if direct.is_some() {
        return direct;
    }

    // Capability/member paths often begin with a user variable, so recognize
    // distinctive API members anywhere in a dotted symbol.
    if parts.contains(&"physics")
        || matches!(
            leaf,
            "apply_force" | "apply_impulse" | "wake" | "gravity_scale"
        )
    {
        return Some(ScriptApiDocTarget {
            label: "Physics",
            url: "https://aeowun.com/docs/reference/physics/",
            description: "Read and manipulate runtime physics state, including forces, impulses, velocity, collision, mass, and gravity.",
        });
    }

    if parts.contains(&"sound") || matches!(leaf, "play" | "pause" | "looped" | "volume") {
        return Some(ScriptApiDocTarget {
            label: "Audio",
            url: "https://aeowun.com/docs/reference/",
            description: "Control supported Sound handles, including playback, pause, stop, looping, and volume.",
        });
    }

    if parts.contains(&"animation")
        || matches!(leaf, "select_animation" | "play_layer" | "animation_name")
    {
        return Some(ScriptApiDocTarget {
            label: "Animation",
            url: "https://aeowun.com/aeoscript/api/entity/",
            description: "Select character animations or drive supported Entity animation-layer playback.",
        });
    }

    if matches!(
        leaf,
        "navigate_to"
            | "cancel_navigation"
            | "is_navigating"
            | "set_facing_direction"
            | "set_horizontal_velocity"
            | "apply_vertical_impulse"
    ) {
        return Some(ScriptApiDocTarget {
            label: "Actors & Navigation",
            url: "https://aeowun.com/docs/reference/project/",
            description: "Control Actor or player movement, facing, navigation requests, and navigation state.",
        });
    }

    if matches!(
        leaf,
        "damage" | "heal" | "jump" | "find_path" | "health" | "max_health" | "alive" | "grounded"
    ) {
        return Some(ScriptApiDocTarget {
            label: "Entities",
            url: "https://aeowun.com/aeoscript/api/entity/",
            description: "Inspect or modify live Entity gameplay state such as health, movement, grounding, jumping, and pathfinding.",
        });
    }

    if matches!(
        leaf,
        "position"
            | "rotation"
            | "scale"
            | "visible"
            | "enabled"
            | "anchored"
            | "attributes"
            | "is_valid"
            | "destroy"
            | "translate"
            | "set_position"
    ) {
        return Some(ScriptApiDocTarget {
            label: "Engine Handles",
            url: "https://aeowun.com/docs/reference/",
            description: "Common engine-handle properties and methods for inspecting, moving, enabling, hiding, or destroying runtime objects.",
        });
    }

    if matches!(
        leaf,
        "spawn"
            | "attach"
            | "detach"
            | "find"
            | "get_entity"
            | "get_light"
            | "getAllCellsOfClass"
            | "wait"
            | "enable_script"
            | "disable_script"
            | "is_script_enabled"
            | "fire_event"
            | "complete_test"
            | "is_test_completed"
            | "test_passed"
            | "test_summary"
    ) {
        return Some(ScriptApiDocTarget {
            label: "Global Functions",
            url: "https://aeowun.com/docs/reference/globals/",
            description: "Core AeoScript globals for lookup, spawning, attachment, timing, compatibility control, events, and testing.",
        });
    }

    None
}

impl ScriptEditor {
    pub fn show_ui(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<PathBuf>,
        world: &mut crate::world::World,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            // Worldkiln 0.8.7 | WO 0.8.7-34 | 2026-10-07 21:57 -05:00
            // Respect the global window theme.

            self.draw_workspace(ui, project_path, world);
        });

        self.draw_search_results_window(ctx);

        self.draw_new_script_dialog(ctx, project_path);
        self.draw_rename_dialog(ctx, project_path, world);
        self.draw_delete_dialog(ctx, project_path, world);
        self.draw_dirty_dialog(ctx, project_path, world);
    }

    fn draw_search_results_window(&mut self, ctx: &egui::Context) {
        if !self.show_search || self.search_results.is_empty() {
            return;
        }

        let mut open_path = None;

        egui::Window::new(format!("SEARCH RESULTS ({})", self.search_results.len()))
            .resizable(true)
            .default_width(520.0)
            .default_height(260.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for result in &self.search_results {
                        let filename = result
                            .path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();

                        let label = format!("{}:{}  {}", filename, result.line, result.text.trim());

                        if ui.selectable_label(false, label).clicked() {
                            open_path = Some(result.path.clone());
                        }
                    }
                });
            });

        if let Some(path) = open_path {
            self.open_file(path);
        }
    }

    fn draw_new_script_dialog(&mut self, ctx: &egui::Context, project_path: &Option<PathBuf>) {
        if !self.show_new_script_dialog {
            return;
        }

        egui::Window::new("New Script")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Source:");

                    ui.radio_value(
                        &mut self.new_script_root,
                        ScriptRoot::Scripts,
                        "Project Scripts (scripts/)",
                    );

                    ui.radio_value(
                        &mut self.new_script_root,
                        ScriptRoot::PlayerScripts,
                        "Player Scripts (playerscripts/)",
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Path:");

                    ui.text_edit_singleline(&mut self.new_script_name);
                });

                ui.label(
                    RichText::new("Example: Dungeon/systems/doors")
                        .small()
                        .color(Color32::from_gray(140)),
                );

                ui.horizontal(|ui| {
                    let create = ui.button("Create").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Enter));

                    if create {
                        if let Some(root) = project_path {
                            let name = self.new_script_name.clone();
                            if let Some((path, logical_path)) =
                                self.create_new_script_in_root(root, self.new_script_root, &name)
                            {
                                self.created_script =
                                    Some((self.new_script_parent, logical_path, path));
                            }

                            self.show_new_script_dialog = false;
                            self.new_script_name.clear();
                        }
                    }

                    if ui.button("Cancel").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Escape))
                    {
                        self.show_new_script_dialog = false;
                        self.new_script_name.clear();
                    }
                });
            });
    }

    fn draw_rename_dialog(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<PathBuf>,
        world: &mut crate::world::World,
    ) {
        if !self.show_rename_dialog {
            return;
        }

        egui::Window::new("Rename Script")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("New Name:");
                    ui.text_edit_singleline(&mut self.rename_new_name);
                });

                ui.horizontal(|ui| {
                    let rename = ui.button("Rename").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Enter));

                    if rename {
                        if let (Some(root), Some(target)) =
                            (project_path, self.rename_target.clone())
                        {
                            self.rename_script_with_bindings(
                                root,
                                target,
                                &self.rename_new_name.clone(),
                                world,
                            );

                            self.show_rename_dialog = false;
                            self.rename_new_name.clear();
                            self.rename_target = None;
                        }
                    }

                    if ui.button("Cancel").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Escape))
                    {
                        self.show_rename_dialog = false;
                        self.rename_new_name.clear();
                        self.rename_target = None;
                    }
                });
            });
    }

    fn draw_delete_dialog(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<PathBuf>,
        world: &crate::world::World,
    ) {
        let Some(path) = self.deleting_path.clone() else {
            return;
        };

        if self.closing_path.as_ref() == Some(&path) {
            return;
        }

        egui::Window::new("Confirm Delete")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Are you sure you want to delete {:?}?",
                    path.file_name().unwrap_or_default()
                ));

                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    let delete = ui.button("Delete").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Enter));

                    if delete {
                        let dirty = self
                            .open_documents
                            .get(&path)
                            .is_some_and(|document| document.dirty);

                        if dirty {
                            self.closing_path = Some(path.clone());
                        } else {
                            if self.delete_script_if_unbound(project_path, path.clone(), world) {
                                self.deleting_path = None;
                            }
                        }
                    }

                    if ui.button("Cancel").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Escape))
                    {
                        self.deleting_path = None;
                    }
                });
            });
    }

    fn draw_dirty_dialog(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<PathBuf>,
        world: &crate::world::World,
    ) {
        let Some(path) = self.closing_path.clone() else {
            return;
        };

        let is_deletion = self.deleting_path.as_ref() == Some(&path);

        egui::Window::new(if is_deletion {
            "Delete: Unsaved Changes"
        } else {
            "Unsaved Changes"
        })
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label(format!(
                "Save changes to {} before {}?",
                path.file_name().unwrap_or_default().to_string_lossy(),
                if is_deletion { "deleting" } else { "closing" }
            ));

            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    self.apply_dirty_response(project_path, path.clone(), true, false, world);
                }

                if ui.button("Don't Save").clicked() {
                    self.apply_dirty_response(project_path, path.clone(), false, false, world);
                }

                if ui.button("Cancel").clicked()
                    || ui.input(|input| input.key_pressed(egui::Key::Escape))
                {
                    self.apply_dirty_response(project_path, path.clone(), false, true, world);
                }
            });
        });
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:56 -05:00
    fn draw_debugger_bar(&mut self, ui: &mut egui::Ui) {
        let paused = self.debug_snapshot.is_some();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("DEBUGGER").strong());
            ui.label(if paused { "Paused" } else { "Running / idle" });

            if ui
                .add_enabled(paused, egui::Button::new("Continue"))
                .clicked()
            {
                self.pending_debug_command =
                    Some(crate::scripting::debugger::DebugCommand::Continue);
            }
            if ui
                .add_enabled(paused, egui::Button::new("Step Into"))
                .clicked()
            {
                self.pending_debug_command =
                    Some(crate::scripting::debugger::DebugCommand::StepInto);
            }
            if ui
                .add_enabled(paused, egui::Button::new("Step Over"))
                .clicked()
            {
                self.pending_debug_command =
                    Some(crate::scripting::debugger::DebugCommand::StepOver);
            }
            let can_step_out = self
                .debug_snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.frames.len() > 1);
            if ui
                .add_enabled(can_step_out, egui::Button::new("Step Out"))
                .clicked()
            {
                self.pending_debug_command =
                    Some(crate::scripting::debugger::DebugCommand::StepOut);
            }

            if let Some(frame) = self
                .debug_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.frames.first())
            {
                ui.separator();
                ui.label(format!(
                    "{}:{} — {}",
                    frame.script_path,
                    frame.line.unwrap_or(1),
                    frame.function_name
                ));
            }
        });

        if let Some(status) = &self.debug_status {
            ui.small(status);
        }
    }

    fn draw_debugger_inspector(&mut self, ui: &mut egui::Ui, project_path: &Option<PathBuf>) {
        ui.label(RichText::new("DEBUGGER").strong());
        let Some(snapshot) = self.debug_snapshot.clone() else {
            ui.small("Start Play and hit a breakpoint to inspect AeoScript state.");
            return;
        };

        let mut select_frame = None;
        egui::CollapsingHeader::new(format!("CALL STACK ({})", snapshot.frames.len()))
            .default_open(true)
            .show(ui, |ui| {
                for (index, frame) in snapshot.frames.iter().enumerate() {
                    let location = frame
                        .line
                        .map(|line| format!("{}:{}", frame.script_path, line))
                        .unwrap_or_else(|| frame.script_path.clone());
                    if ui
                        .selectable_label(
                            self.debug_selected_frame == index,
                            format!("{} — {}", frame.function_name, location),
                        )
                        .clicked()
                    {
                        select_frame = Some(index);
                    }
                }
            });

        if let Some(index) = select_frame {
            self.select_debug_frame(index, project_path.as_deref());
        }

        let selected = snapshot.frames.get(
            self.debug_selected_frame
                .min(snapshot.frames.len().saturating_sub(1)),
        );
        if let Some(frame) = selected {
            egui::CollapsingHeader::new("VARIABLES")
                .default_open(true)
                .show(ui, |ui| {
                    for (label, values) in [
                        ("Locals", &frame.locals),
                        ("Captured", &frame.captured),
                        ("Instance", &frame.instance_fields),
                    ] {
                        ui.label(RichText::new(label).small().strong());
                        if values.is_empty() {
                            ui.small("(none)");
                        } else {
                            for variable in values {
                                ui.horizontal_wrapped(|ui| {
                                    ui.monospace(&variable.name);
                                    ui.label("=");
                                    ui.monospace(&variable.display);
                                    ui.weak(format!("({})", variable.value_type));
                                });
                            }
                        }
                    }
                    if let Some(parent) = frame.parent_actor_id {
                        ui.label(format!("parent Actor: {}", parent));
                    }
                    ui.label(format!("self: {}#{}", frame.entity_name, frame.instance_id));
                });
        }

        egui::CollapsingHeader::new("WATCHES")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.debug_watch_input)
                            .hint_text("variable name"),
                    );
                    if ui.button("Add").clicked() {
                        let name = self.debug_watch_input.trim().to_string();
                        let valid = !name.is_empty()
                            && name.chars().enumerate().all(|(index, c)| {
                                c == '_'
                                    || c.is_ascii_alphabetic()
                                    || (index > 0 && c.is_ascii_digit())
                            });
                        if valid && !self.debug_watches.contains(&name) {
                            self.debug_watches.push(name);
                            self.debug_watch_input.clear();
                        } else if !valid {
                            self.debug_status = Some(
                                "Watches are limited to side-effect-free variable names in WO 0.8.7-17."
                                    .to_string(),
                            );
                        }
                    }
                });

                let watches = self.debug_watches.clone();
                let mut remove = None;
                for (index, name) in watches.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.monospace(name);
                        ui.label(
                            self.watched_variable(name)
                                .map(|value| value.display.as_str())
                                .unwrap_or("<not in selected frame>"),
                        );
                        if ui.small_button("×").clicked() {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    self.debug_watches.remove(index);
                }
                ui.small("Arbitrary expressions are deferred; watches do not execute script code.");
            });
    }

    fn draw_workspace(
        &mut self,
        ui: &mut egui::Ui,
        project_path: &Option<PathBuf>,
        world: &mut crate::world::World,
    ) {
        let mut action_close = None;

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("SCRIPT EDITOR").strong());
                ui.separator();

                if ui
                    .button("New Script")
                    .on_hover_text("Create a new .aeo script")
                    .clicked()
                {
                    self.new_script_root = project_path
                        .as_ref()
                        .and_then(|root| {
                            self.active_document
                                .as_ref()
                                .and_then(|path| script_root_for_path(root, path))
                        })
                        .unwrap_or(ScriptRoot::Scripts);

                    self.show_new_script_dialog = true;
                }

                if ui
                    .button("Save")
                    .on_hover_text("Save the active document (Ctrl+S)")
                    .clicked()
                {
                    self.save_active();
                }

                if ui
                    .button("Save All")
                    .on_hover_text("Save all open documents (Ctrl+Shift+S)")
                    .clicked()
                {
                    self.save_all();
                }

                if ui
                    .button("Refresh")
                    .on_hover_text("Refresh the script list from disk")
                    .clicked()
                {
                    self.refresh_scripts(project_path);
                }

                if let Some(active_path) = self.active_document.clone() {
                    ui.separator();

                    if ui
                        .button("Rename")
                        .on_hover_text("Rename the active script")
                        .clicked()
                    {
                        self.show_rename_dialog = true;
                        self.rename_target = Some(active_path.clone());
                        self.rename_new_name = active_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                    }

                    if ui
                        .button("Delete")
                        .on_hover_text("Delete the active script from disk")
                        .clicked()
                    {
                        self.deleting_path = Some(active_path.clone());
                    }

                    if ui
                        .button("Close")
                        .on_hover_text("Close the active document")
                        .clicked()
                    {
                        action_close = Some(active_path);
                    }
                }

                ui.separator();

                ui.menu_button("API Docs", |ui| {
                    ui.label(
                        RichText::new("AEOSCRIPT REFERENCE")
                            .small()
                            .strong()
                            .color(Color32::from_gray(140)),
                    );
                    ui.separator();

                    for (label, url) in SCRIPT_API_DOC_LINKS {
                        if ui.button(*label).clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(*url));
                            ui.close_menu();
                        }
                    }
                });

                ui.separator();

                ui.toggle_value(&mut self.show_search, "Search");

                if self.show_search {
                    ui.label("Find:");

                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search_query)
                            .hint_text("Search in all scripts...")
                            .desired_width(180.0),
                    );

                    if response.changed() {
                        self.perform_search();
                    }
                }
            });

            ui.separator();

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:56 -05:00
            self.draw_debugger_bar(ui);
            ui.separator();

            let main_height = ui.available_height().max(100.0);

            ui.horizontal(|ui| {
                let inspector_width = 240.0;

                let editor_width = (ui.available_width() - inspector_width - 20.0).max(100.0);

                ui.allocate_ui(egui::vec2(editor_width, main_height), |ui| {
                    self.draw_editor(ui, project_path);
                });

                ui.separator();

                ui.allocate_ui(egui::vec2(inspector_width, main_height), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("inspector_scroll")
                        .show(ui, |ui| {
                            self.draw_inspector(ui, world, project_path);
                        });
                });
            });
        });

        if let Some(path) = action_close {
            if !self.close_document(&path) {
                self.closing_path = Some(path);
            }
        }
    }

    fn draw_editor(&mut self, ui: &mut egui::Ui, project_path: &Option<PathBuf>) {
        let Some(path) = self.active_document.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label("No script open.");
            });
            return;
        };

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:56 -05:00
        let logical_path = project_path
            .as_deref()
            .and_then(|root| Self::logical_script_path(root, &path));
        let breakpoint_lines = logical_path
            .as_deref()
            .map(|logical| {
                self.breakpoints
                    .iter()
                    .filter(|breakpoint| breakpoint.script_path == logical)
                    .map(|breakpoint| breakpoint.line)
                    .collect::<std::collections::BTreeSet<_>>()
            })
            .unwrap_or_default();
        let current_execution_line = self
            .debug_snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.frames.first())
            .filter(|frame| {
                logical_path
                    .as_deref()
                    .is_some_and(|logical| frame.script_path == logical)
            })
            .and_then(|frame| frame.line);
        let mut breakpoint_click = None;
        // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:55 -05:00
        let mut project_problems_dirty = false;

        let Some(document) = self.open_documents.get_mut(&path) else {
            ui.centered_and_justified(|ui| {
                ui.label("No script open.");
            });
            return;
        };

        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let filename = path.file_name().unwrap_or_default().to_string_lossy();

                let header_color = if ui.visuals().dark_mode {
                    ui.visuals().text_color()
                } else {
                    Color32::from_rgb(135, 58, 22)
                };

                ui.label(RichText::new(filename).strong().color(header_color));

                if document.dirty {
                    ui.label(
                        RichText::new("(modified)")
                            .italics()
                            .color(Color32::from_rgb(228, 91, 36)),
                    );
                }
            });

            let available_height = ui.available_height();

            let mut editor_scroll = egui::ScrollArea::vertical()
                .id_salt("editor_v_scroll")
                .max_height(available_height);

            if let Some(offset) = self.editor_scroll_to_y.take() {
                editor_scroll = editor_scroll.vertical_scroll_offset(offset.max(0.0));
            }

            editor_scroll.show(ui, |ui| {
                ui.push_id(&path, |ui| {
                    let line_count = document.source_map.line_count().max(1);

                    // Worldkiln 0.8.7 | WO 0.8.7-28 | 2026-10-06 12:40 -05:00
                    // A maintained egui-native code editor now owns the source text and numbered
                    // line presentation together. Worldkiln no longer lays out a second gutter
                    // beside TextEdit and attempts to keep both widgets vertically synchronized.
                    let mut code_editor = CodeEditor::default()
                        .id_source(format!("aeoscript:{}", path.to_string_lossy()))
                        .with_rows(line_count)
                        .with_ui_fontsize(ui)
                        .with_theme(aeoscript_editor_theme(ui.visuals().dark_mode))
                        .with_syntax(aeoscript_syntax())
                        .with_numlines(true)
                        .vscroll(false)
                        .auto_shrink(false);

                    let output = code_editor.show(ui, &mut document.source);
                    let response = &output.response;

                    // Debug markers are overlays derived from the actual text galley. Their
                    // vertical position and click-to-line mapping therefore use the same text
                    // model that renders the source instead of an independently accumulated
                    // line height.
                    let marker_x = response.rect.left() - 9.0;
                    let marker_rect = egui::Rect::from_min_max(
                        egui::pos2(response.rect.left() - 18.0, response.rect.top()),
                        egui::pos2(response.rect.left(), response.rect.bottom()),
                    );
                    let marker_id = ui.make_persistent_id(("aeoscript_breakpoint_margin", &path));
                    let marker_response = ui.interact(marker_rect, marker_id, egui::Sense::click());

                    if marker_response.hovered() {
                        ui.output_mut(|output| {
                            output.cursor_icon = egui::CursorIcon::PointingHand;
                        });
                    }

                    if marker_response
                        .clone()
                        .on_hover_text("Click to toggle a source breakpoint")
                        .clicked()
                    {
                        if let Some(pointer) = marker_response.interact_pointer_pos() {
                            let local_pos = egui::vec2(0.0, pointer.y - output.galley_pos.y);
                            let cursor = output.galley.cursor_from_pos(local_pos);
                            breakpoint_click =
                                Some(char_index_to_line(&document.source, cursor.ccursor.index));
                        }
                    }

                    for line in breakpoint_lines
                        .iter()
                        .copied()
                        .chain(current_execution_line)
                        .collect::<std::collections::BTreeSet<_>>()
                    {
                        let Some(char_index) = line_start_char_index(&document.source, line) else {
                            continue;
                        };
                        let caret = output
                            .galley
                            .pos_from_ccursor(egui::text::CCursor::new(char_index));
                        let center_y = output.galley_pos.y + caret.center().y;

                        if current_execution_line == Some(line) {
                            ui.painter().text(
                                egui::pos2(marker_x, center_y),
                                egui::Align2::CENTER_CENTER,
                                "▶",
                                egui::FontId::monospace(11.0),
                                Color32::from_rgb(232, 145, 48),
                            );
                        } else {
                            ui.painter().circle_filled(
                                egui::pos2(marker_x, center_y),
                                3.5,
                                Color32::from_rgb(210, 70, 70),
                            );
                        }
                    }

                    // Ctrl+click a recognized AeoScript API symbol to open
                    // its documentation. Pointer coordinates are converted
                    // through the actual TextEdit galley so the link hit-test
                    // follows the editor's rendered text exactly.
                    let api_doc_under_pointer = response.hover_pos().and_then(|pointer| {
                        let local_pos = pointer - output.galley_pos;
                        let cursor = output.galley.cursor_from_pos(local_pos);
                        let symbol =
                            dotted_symbol_at_char_index(&document.source, cursor.ccursor.index)?;
                        let target = script_api_doc_for_symbol(&symbol)?;

                        Some((symbol, target))
                    });

                    let docs_modifier =
                        ui.input(|input| input.modifiers.ctrl || input.modifiers.command);

                    match api_doc_under_pointer.as_ref() {
                        Some((symbol, _)) => {
                            if self.api_doc_hover_symbol.as_deref() != Some(symbol.as_str()) {
                                self.api_doc_hover_symbol = Some(symbol.clone());
                                self.api_doc_hover_started = Some(std::time::Instant::now());
                            }
                        }
                        None => {
                            self.api_doc_hover_symbol = None;
                            self.api_doc_hover_started = None;
                        }
                    }

                    let hover_elapsed = self
                        .api_doc_hover_started
                        .map(|started| started.elapsed())
                        .unwrap_or_default();

                    let hover_ready = api_doc_under_pointer.is_some()
                        && hover_elapsed >= std::time::Duration::from_secs(1);

                    if api_doc_under_pointer.is_some() && !hover_ready {
                        let remaining =
                            std::time::Duration::from_secs(1).saturating_sub(hover_elapsed);
                        ui.ctx().request_repaint_after(remaining);
                    }

                    if let Some((symbol, target)) = api_doc_under_pointer {
                        // Recognition is immediate so API text feels interactive.
                        // The help tooltip still waits one second to avoid visual noise
                        // while the user is simply moving through the editor.
                        ui.output_mut(|output| {
                            output.cursor_icon = egui::CursorIcon::PointingHand;
                        });

                        if hover_ready {
                            if docs_modifier {
                                response.clone().on_hover_text_at_pointer(format!(
                                    "{} API\n{}\n\n{}\n\nCtrl+click to open",
                                    target.label, symbol, target.description,
                                ));
                            } else {
                                response
                                    .clone()
                                    .on_hover_text_at_pointer("Hold Ctrl to view more");
                            }
                        }

                        if docs_modifier && response.clicked() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(target.url));
                        }
                    }

                    let mut clear_cursor_request = false;

                    if let Some((request_path, byte_offset)) = &self.cursor_request {
                        if request_path == &path {
                            response.request_focus();

                            let mut state =
                                egui::text_edit::TextEditState::load(ui.ctx(), response.id)
                                    .unwrap_or_default();

                            let char_offset =
                                byte_offset_to_char_index(&document.source, *byte_offset);

                            state
                                .cursor
                                .set_char_range(Some(egui::text::CCursorRange::one(
                                    egui::text::CCursor::new(char_offset),
                                )));

                            state.store(ui.ctx(), response.id);

                            clear_cursor_request = true;
                        }
                    }

                    if clear_cursor_request {
                        self.cursor_request = None;
                    }

                    if response.changed() {
                        document.dirty = document.source != document.original_source;

                        document.reparse();
                        // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:55 -05:00
                        project_problems_dirty = true;
                    }

                    // Any keyboard input while the editor owns focus keeps the insertion
                    // point visible. The next frame drives the outer vertical scroll area.
                    let key_activity = ui.input(|input| {
                        input.events.iter().any(|event| {
                            matches!(
                                event,
                                egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
                            )
                        })
                    });

                    if response.has_focus() && key_activity {
                        if let Some(cursor_range) = output.cursor_range {
                            let caret = output.galley.pos_from_cursor(&cursor_range.primary);

                            let target = (caret.center().y - available_height * 0.65).max(0.0);

                            self.editor_scroll_to_y = Some(target);
                            ui.ctx().request_repaint();
                        }
                    }
                });
            });
        });

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:56 -05:00
        if let (Some(line), Some(project_root)) = (breakpoint_click, project_path.as_deref()) {
            if let Some(source) = self
                .open_documents
                .get(&path)
                .map(|document| document.source.clone())
            {
                if let Err(error) = self.toggle_breakpoint(project_root, &path, &source, line) {
                    self.debug_status = Some(error);
                }
            }
        }

        // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:55 -05:00
        if project_problems_dirty {
            self.refresh_project_problems(project_path.as_deref());
        }
    }

    fn draw_inspector(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
        project_path: &Option<PathBuf>,
    ) {
        ui.vertical(|ui| {
            ui.label(RichText::new("INSPECTOR").strong());
            ui.separator();

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:56 -05:00
            self.draw_debugger_inspector(ui, project_path);
            ui.separator();

            let Some(path) = self.active_document.as_ref() else {
                ui.centered_and_justified(|ui| {
                    ui.label("Select a script to view details.");
                });
                return;
            };

            let Some(document) = self.open_documents.get(path) else {
                return;
            };

            ui.label(RichText::new("SCRIPT").strong());

            ui.indent("script_info", |ui| {
                let display_path = project_path
                    .as_ref()
                    .and_then(|root| path.strip_prefix(root).ok())
                    .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                ui.label(format!("File: {}", display_path));

                let relative_path = if let Some(root) = project_path {
                    path.strip_prefix(root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string()
                } else {
                    path.to_string_lossy().to_string()
                };

                let relative_path = relative_path.replace('\\', "/");

                let mut enabled = !world.disabled_scripts.contains(&relative_path);

                if ui
                    .checkbox(&mut enabled, "Script Enabled")
                    .on_hover_text(
                        "If disabled, this script will not execute \
                         lifecycle hooks or events for ANY attached cell.",
                    )
                    .changed()
                {
                    if enabled {
                        world
                            .disabled_scripts
                            .retain(|entry| entry != &relative_path);
                    } else if !world.disabled_scripts.contains(&relative_path) {
                        world.disabled_scripts.push(relative_path);
                    }
                }
            });

            ui.add_space(8.0);
            ui.label(RichText::new("ENTITIES").strong());

            ui.indent("entities_info", |ui| {
                let Some(program) = &document.program else {
                    ui.label(RichText::new("(parse failed)").color(Color32::RED));
                    return;
                };

                for declaration in &program.declarations {
                    if let crate::scripting::ast::Declaration::Entity(entity) = declaration {
                        ui.label(RichText::new(&entity.name).strong());

                        ui.indent("lifecycle", |ui| {
                            for member in &entity.members {
                                let crate::scripting::ast::EntityMember::Function(function) =
                                    member
                                else {
                                    continue;
                                };

                                let lifecycle = matches!(
                                    function.name.as_str(),
                                    "on_spawn" | "on_ready" | "update" | "on_destroy"
                                );

                                ui.horizontal(|ui| {
                                    ui.label(if lifecycle { "⚡" } else { "ƒ" });
                                    ui.label(&function.name);
                                });
                            }
                        });
                    }
                }
            });
        });
    }

    // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:55 -05:00
    pub(crate) fn active_problem_count(&self) -> usize {
        self.project_problem_count()
    }

    pub(crate) fn draw_problems_window(&mut self, ctx: &egui::Context) {
        if !self.show_problems {
            return;
        }

        let count = self.active_problem_count();
        let title = if count == 0 {
            "PROBLEMS".to_string()
        } else {
            format!("PROBLEMS ({count})")
        };
        let mut open = true;

        egui::Window::new(title)
            .id(egui::Id::new("script_problems_window"))
            .open(&mut open)
            .resizable(true)
            .default_width(720.0)
            .default_height(260.0)
            .min_width(360.0)
            .min_height(120.0)
            .show(ctx, |ui| self.draw_problems(ui));

        self.show_problems = open;
    }

    // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:55 -05:00
    // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
    pub(crate) fn draw_problems(&mut self, ui: &mut egui::Ui) {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

        let problems = self.project_problems.clone();
        if problems.is_empty() {
            ui.label("No project script problems.");
            return;
        }

        let mut clicked = None;
        egui::ScrollArea::vertical()
            .id_salt("problems_scroll")
            .show(ui, |ui| {
                for (index, problem) in problems.iter().enumerate() {
                    let color = match problem.severity {
                        crate::scripting::diagnostic::DiagnosticSeverity::Error => {
                            if ui.visuals().dark_mode {
                                Color32::RED
                            } else {
                                Color32::from_rgb(170, 25, 25)
                            }
                        }
                        crate::scripting::diagnostic::DiagnosticSeverity::Warning => {
                            if ui.visuals().dark_mode {
                                Color32::from_rgb(240, 190, 70)
                            } else {
                                Color32::from_rgb(176, 104, 0)
                            }
                        }
                        _ => ui.visuals().text_color(),
                    };

                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(color, format!("{:?}", problem.severity));
                        // Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:29 -05:00
                        let label = format!(
                            "[{:?}] {}:{}:{}  {}",
                            problem.kind,
                            problem.logical_path,
                            problem.line,
                            problem.column,
                            problem.message
                        );
                        if ui.selectable_label(false, label).clicked() {
                            clicked = Some(index);
                        }
                    });
                }
            });

        if let Some(index) = clicked {
            if let Err(error) = self.open_project_problem(index) {
                self.output_log
                    .push(format!("Problem navigation failed: {error}"));
            }
        }
    }
}
fn draw_script_tree_nodes(
    ui: &mut egui::Ui,
    tree: &crate::project::ScriptTree,
    open_documents: &std::collections::HashMap<PathBuf, super::ScriptDocument>,
    active_document: &Option<PathBuf>,
    action_open: &mut Option<PathBuf>,
) {
    for node in &tree.children {
        draw_script_tree_node(
            ui,
            node,
            &tree.root,
            open_documents,
            active_document,
            action_open,
        );

        if action_open.is_some() {
            break;
        }
    }
}

fn draw_script_tree_node(
    ui: &mut egui::Ui,
    node: &crate::project::ScriptTreeNode,
    root: &std::path::Path,
    open_documents: &std::collections::HashMap<PathBuf, super::ScriptDocument>,
    active_document: &Option<PathBuf>,
    action_open: &mut Option<PathBuf>,
) {
    match node.kind {
        crate::project::ScriptTreeNodeKind::Directory => {
            egui::CollapsingHeader::new(format!("📁 {}", node.name))
                .default_open(false)
                .show(ui, |ui| {
                    for child in &node.children {
                        draw_script_tree_node(
                            ui,
                            child,
                            root,
                            open_documents,
                            active_document,
                            action_open,
                        );

                        if action_open.is_some() {
                            break;
                        }
                    }
                });
        }

        crate::project::ScriptTreeNodeKind::Module => {
            let path = root.join(&node.relative_path);

            let active = active_document.as_ref() == Some(&path);

            let dirty = open_documents
                .get(&path)
                .is_some_and(|document| document.dirty);

            let label = if dirty {
                format!("{} *", node.name)
            } else {
                node.name.clone()
            };

            let response = ui
                .selectable_label(active, label)
                .on_hover_text(node.relative_path.to_string_lossy());

            if response.clicked() {
                *action_open = Some(path);
            }
        }
    }
}

#[cfg(test)]
mod api_doc_tests {
    use super::{dotted_symbol_at_char_index, script_api_doc_for_symbol};

    #[test]
    fn dotted_symbol_lookup_uses_the_clicked_api_path() {
        let source = "const speed = math.max(1, 2)";
        let byte_index = source.find("max").unwrap() + 1;
        let char_index = source[..byte_index].chars().count();

        assert_eq!(
            dotted_symbol_at_char_index(source, char_index).as_deref(),
            Some("math.max")
        );
    }

    #[test]
    fn api_doc_routing_handles_namespaces_globals_and_capabilities() {
        let math = script_api_doc_for_symbol("math.max").unwrap();
        assert_eq!(math.label, "Math");
        assert_eq!(math.url, "https://aeowun.com/docs/reference/math/");
        assert!(!math.description.is_empty());

        let global = script_api_doc_for_symbol("wait").unwrap();
        assert_eq!(global.label, "Global Functions");
        assert_eq!(global.url, "https://aeowun.com/docs/reference/globals/");
        assert!(!global.description.is_empty());

        let physics = script_api_doc_for_symbol("enemy.physics.apply_impulse").unwrap();
        assert_eq!(physics.label, "Physics");
        assert_eq!(physics.url, "https://aeowun.com/docs/reference/physics/");
        assert!(!physics.description.is_empty());

        assert_eq!(
            script_api_doc_for_symbol("input.is_key_down")
                .unwrap()
                .label,
            "Input"
        );
        assert_eq!(
            script_api_doc_for_symbol("camera.set_target")
                .unwrap()
                .label,
            "Camera"
        );
        assert_eq!(
            script_api_doc_for_symbol("save.commit").unwrap().label,
            "Typed Save State"
        );
        assert_eq!(
            script_api_doc_for_symbol("test.summary").unwrap().label,
            "Automated Test API"
        );
        assert_eq!(
            script_api_doc_for_symbol("guard.navigate_to")
                .unwrap()
                .label,
            "Actors & Navigation"
        );
        assert_eq!(
            script_api_doc_for_symbol("speaker.sound.play")
                .unwrap()
                .label,
            "Audio"
        );
        assert_eq!(
            script_api_doc_for_symbol("enemy.damage").unwrap().label,
            "Entities"
        );
        assert_eq!(script_api_doc_for_symbol("local_variable"), None);
    }
}
