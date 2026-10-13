use super::{Editor, EditorTool, LeftPanelTab};
use crate::project::{ProjectNode, ProjectNodeId, ProjectNodeKind, ScriptRoot};
use crate::world::World;
use egui::{Color32, RichText};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProjectExplorerDragPayload {
    node_id: ProjectNodeId,
}

// Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
// egui may inherit a screen-wide paint clip for SidePanel descendants.
// Clamp the Terrain controls to the actual left-panel rectangle, preventing
// their overflow from painting a line into the OpenGL viewport.
pub(super) fn clip_project_panel_to_bounds(ui: &mut egui::Ui) {
    ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
}

impl Editor {
    // Worldkiln 0.8.7 | WO 0.8.7-67 | 2026-10-09:
    // Apply AI and manual script creations through exactly the same validated hierarchy path.
    pub(crate) fn register_created_script_in_project_manager(
        &mut self,
        world: &mut World,
    ) -> Result<Option<ProjectNodeId>, String> {
        let Some((parent_id, logical_path, path)) = self.script_editor.created_script.take() else {
            return Ok(None);
        };
        // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
        // Player Scripts always belongs to the real playerscripts/ root.
        let parent_id = if logical_path.starts_with("playerscripts/") {
            None
        } else {
            parent_id
        };
        let mut candidate = self.project_hierarchy.clone();
        let new_id = match candidate.create_script_node(parent_id, &logical_path) {
            Ok(id) => id,
            Err(error) => {
                // Do not discard the creation record on failure: recoverable after user fixes hierarchy.
                self.script_editor.created_script = Some((parent_id, logical_path, path));
                return Err(format!(
                    "Could not register script in Project Manager: {error}"
                ));
            }
        };
        self.push_project_hierarchy_undo_snapshot(world);
        self.project_hierarchy = candidate;
        if let Some(pid) = parent_id {
            self.expanded_project_nodes.insert(pid);
        }
        self.expanded_project_nodes.insert(new_id);
        self.selected_project_node_id = Some(new_id);
        self.selected_project_nodes = vec![new_id];
        self.clear_selection();
        self.synchronize_project_hierarchy_bindings(world);
        self.needs_save = true;
        Ok(Some(new_id))
    }
    pub(crate) fn draw_project_explorer_panel(
        &mut self,
        ctx: &egui::Context,
        world: &mut World,
        project_path: &Option<PathBuf>,
    ) {
        self.synchronize_project_actor_nodes(world);
        self.synchronize_project_cell_nodes(world);
        self.sanitize_project_explorer_selection();

        // All ScriptEditor creation paths (including AI) share the Project Manager transaction.
        let _ = self.register_created_script_in_project_manager(world);

        let mut script_to_open = None;

        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:44 -05:00
        // Inset contents and draw the side divider from the active UI theme.
        let frame = egui::Frame::side_top_panel(&ctx.style())
            .inner_margin(egui::Margin::symmetric(13.0, 8.0))
            .stroke(ctx.style().visuals.widgets.noninteractive.bg_stroke);
        let panel_response=egui::SidePanel::left("left_panel")
            .frame(frame)
            .resizable(true)
            .default_width(self.shell.explorer_width)
            .min_width(240.0)
            // Content must never be able to grow this panel over the viewport.
            // In particular, TextEdit reports a large desired width while it is
            // receiving text unless the containing panel has an upper bound.
            .max_width(500.0)
            .show(ctx, |ui| {
                clip_project_panel_to_bounds(ui);
                // Worldkiln 0.8.5 | WO 0.8.7-71 | 2026-10-09 19:52 -05:00
                // The previous shell only changed LeftPanelTab::Terrain;
                // it never rendered the actual generation-layer controls.
                // Reuse Worldkiln's real Terrain settings instead of a copy.
                if self.left_panel_tab == LeftPanelTab::Terrain {
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        ui.strong("Terrain Layers");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Explorer").on_hover_text("Back to Project Explorer").clicked() {
                                self.left_panel_tab = LeftPanelTab::Project;
                            }
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("worldkiln_left_terrain_layers")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            super::ui::draw_terrain_settings(ui, self, world, project_path);
                        });
                    return;
                }
                // Worldkiln 0.8.7 | WO 0.8.7-34 | 2026-10-07 21:57 -05:00
                // Respect global Theme / Panel Rounding, including rounded controls.

                let panel_rect = ui.max_rect();
                if ui.input(|i| i.pointer.any_pressed()) {
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        if panel_rect.contains(pos) {
                            self.project_explorer_focused = true;
                        } else {
                            self.project_explorer_focused = false;
                        }
                    }
                }

                // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
                ui.add_space(7.0);
                // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:04 -05:00
                // One Explorer header. World uses the REAL project hierarchy
                // and drag/drop tree below; no duplicate shortcut tree.
                ui.horizontal(|ui| {
                    ui.strong("Explorer");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        ui.menu_button("⋯",|ui|{
                            if ui.button("Hide Explorer").clicked(){
                                self.shell.explorer_visible=false;self.shell.save();ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Expand all").clicked(){self.expand_all_project_nodes();ui.close_menu();}
                            if ui.button("Collapse all").clicked(){self.collapse_all_project_nodes();ui.close_menu();}

                        });
                        ui.menu_button("+",|ui|{
                            if ui.button("New Folder").clicked(){
                                self.project_explorer_folder_parent=self.selected_project_node_id
                                    .filter(|id|self.project_hierarchy.contains(*id));
                                self.project_explorer_folder_name.clear();
                                self.project_explorer_folder_error=None;
                                self.show_project_explorer_folder_dialog=true;
                                ui.close_menu();
                            }
                            if ui.button("New Script").clicked(){
                                self.show_script_workspace=true;
                                self.script_editor.show_new_script_dialog=true;
                                self.script_editor.new_script_parent=self.selected_project_node_id;
                                self.script_editor.new_script_root=self.selected_project_script_root();
                                ui.close_menu();
                            }
                            if ui.button("Place Actor").clicked(){
                                self.activate_editor_tool(EditorTool::Build);
                                self.build_is_actor=true;
                                if let Some(path)=project_path {
                                    let available=crate::project::discover_actor_packages(path);
                                    if self.build_actor_package.is_none(){
                                        self.build_actor_package=available.first().cloned();
                                    }
                                }
                                ui.close_menu();
                            }
                        });
                        if ui.small_button("⌕").on_hover_text("Search Explorer").clicked(){
                            self.shell.explorer_search_visible=!self.shell.explorer_search_visible;
                            if !self.shell.explorer_search_visible {
                                self.project_explorer_search.clear();
                            }
                        }
                    });
                });
                ui.separator();
                if self.shell.explorer_search_visible || !self.project_explorer_search.is_empty() {
                    ui.horizontal(|ui|{
                        let result=ui.add(egui::TextEdit::singleline(&mut self.project_explorer_search)
                            .desired_width(ui.available_width()-24.0).hint_text("Search World and project..."));
                        if result.changed(){self.expand_search_matches(world);}
                        if ui.small_button("×").on_hover_text("Clear search").clicked(){
                            self.project_explorer_search.clear();
                            self.shell.explorer_search_visible=false;
                        }
                    });
                }
                if let Some((message,expires_at))=&self.project_explorer_status {
                    if ctx.input(|input|input.time)<=*expires_at {
                        ui.small(message);
                    } else {self.project_explorer_status=None;}
                }
                // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
                // One project identity, two real file/scene roots. No category buckets.
                let project_title = project_path.as_ref()
                    .and_then(|path| path.file_name())
                    .map(|part| part.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Project".to_owned());
                egui::Frame::none()
                    .fill(ui.visuals().widgets.inactive.bg_fill)
                    .rounding(3.0)
                    .inner_margin(egui::Margin::symmetric(4.0, 3.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(project_title).strong());
                        });
                    });
                // Reserve space so the second section stays visible even with a large Project tree.
                let tree_height = (ui.available_height() * 0.58).clamp(120.0, 440.0);
                let project_open = egui::CollapsingHeader::new("Project")
                    .id_salt("explorer_project_section")
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("project_explorer_tree")
                            .auto_shrink([false, true])
                            .max_height(tree_height)
                            .show(ui, |ui| {
                                // Dropping into this section restores the Project root.
                                if egui::DragAndDrop::has_payload_of_type::<ProjectExplorerDragPayload>(ui.ctx()) {
                                    let (rect, root) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width(), 22.0), egui::Sense::hover());
                                    ui.painter().rect_stroke(rect, 2.0,
                                        egui::Stroke::new(1.0, ui.visuals().widgets.inactive.bg_stroke.color));
                                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER,
                                        "Drop at Project root", egui::FontId::proportional(11.0),
                                        ui.visuals().weak_text_color());
                                    if let Some(payload) = root.dnd_release_payload::<ProjectExplorerDragPayload>() {
                                        self.drop_into_project_root(payload.node_id, world, project_path, ui.ctx());
                                    }
                                }
                                let roots = self.sorted_project_children(None, world).into_iter()
                                    .filter(|id| self.project_hierarchy.node(*id).is_some_and(|node|
                                        !matches!(&node.kind, ProjectNodeKind::Script { script_path }
                                            if script_path.starts_with("playerscripts/"))))
                                    .collect::<Vec<_>>();
                                let count = roots.len();
                                let mut ancestry = Vec::new();
                                for (index, node_id) in roots.into_iter().enumerate() {
                                    ancestry.push(index + 1 < count);
                                    self.draw_project_explorer_node(ui, node_id, 0, &mut ancestry,
                                        world, project_path, &mut script_to_open);
                                    ancestry.pop();
                                }
                            });
                    });
                if let Some(payload) = project_open.header_response.dnd_release_payload::<ProjectExplorerDragPayload>() {
                    self.drop_into_project_root(payload.node_id, world, project_path, ui.ctx());
                }
                let player_open = egui::CollapsingHeader::new("Player Scripts")
                    .id_salt("explorer_player_scripts_section")
                    .default_open(true)
                    .show(ui, |ui| {
                        let roots = self.sorted_project_children(None, world).into_iter()
                            .filter(|id| self.project_hierarchy.node(*id).is_some_and(|node|
                                matches!(&node.kind, ProjectNodeKind::Script { script_path }
                                    if script_path.starts_with("playerscripts/"))))
                            .collect::<Vec<_>>();
                        egui::ScrollArea::vertical()
                            .id_salt("project_explorer_player_tree")
                            .max_height(tree_height * 0.45)
                            .show(ui, |ui| {
                                let count = roots.len();
                                let mut ancestry = Vec::new();
                                for (index, node_id) in roots.into_iter().enumerate() {
                                    ancestry.push(index + 1 < count);
                                    self.draw_project_explorer_node(ui, node_id, 0, &mut ancestry,
                                        world, project_path, &mut script_to_open);
                                    ancestry.pop();
                                }
                                // WO 0.8.7-78: The whole Player Scripts section is
                                // a drop target, not a 22px slot inside this scroller.
                                // Drop handling follows the CollapsingHeader below.
                            });
                    });
                // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-10 09:05 -05:00
                // A CollapsingHeader only exposes its label as a drop response;
                // the old 22px scroller slot left the empty Explorer area inert.
                // Register a non-layout, hover-only target across the entire
                // expanded section (including blank space and existing rows).
                // While collapsed, the header itself remains a valid target.
                if egui::DragAndDrop::has_payload_of_type::<ProjectExplorerDragPayload>(ui.ctx()) {
                    let mut drop_rect = player_open.header_response.rect;
                    if player_open.body_response.is_some() {
                        drop_rect.max.x = ui.max_rect().max.x;
                        drop_rect.max.y = ui.clip_rect().max.y;
                    }
                    drop_rect = drop_rect.intersect(ui.clip_rect());
                    if drop_rect.is_positive() {
                        let drop_response = ui.interact(
                            drop_rect,
                            ui.id().with("explorer_player_scripts_full_drop"),
                            egui::Sense::hover(),
                        );
                        if let Some(payload) = drop_response.dnd_hover_payload::<ProjectExplorerDragPayload>() {
                            let is_script = self.project_hierarchy.node(payload.node_id)
                                .is_some_and(|node| matches!(&node.kind,
                                    ProjectNodeKind::Script { script_path }
                                    if !script_path.starts_with("playerscripts/")));
                            let stroke = if is_script {
                                egui::Stroke::new(1.5, ui.visuals().selection.stroke.color)
                            } else {
                                egui::Stroke::new(1.5, ui.visuals().error_fg_color)
                            };
                            ui.painter().rect_stroke(drop_rect.shrink(1.0), 3.0, stroke);
                            if let Some(payload) = drop_response.dnd_release_payload::<ProjectExplorerDragPayload>() {
                                self.move_script_to_player_scripts(payload.node_id, world, project_path, ui.ctx());
                            }
                        }
                    }
                }
                player_open.header_response.context_menu(|ui| {
                    if ui.button("New Player Script").clicked() {
                        self.show_script_workspace = true;
                        self.script_editor.show_new_script_dialog = true;
                        self.script_editor.new_script_parent = None;
                        self.script_editor.new_script_root = ScriptRoot::PlayerScripts;
                        ui.close_menu();
                    }
                });
            });
        if ctx.input(|i| i.pointer.any_released()) {
            let width = panel_response.response.rect.width();
            if (width - self.shell.explorer_width).abs() > 2.0 {
                self.shell.explorer_width = width.clamp(185.0, 500.0);
                self.shell.save();
            }
        }

        if let Some(ref path) = script_to_open {
            self.script_editor.open_file(path.clone());
            self.show_script_workspace = true;
        }

        self.handle_explorer_keyboard_input(ctx, world, project_path, &mut script_to_open);

        self.draw_project_explorer_folder_dialog(ctx, world);
        self.draw_project_explorer_rename_dialog(ctx, world, project_path);
        self.draw_project_explorer_delete_dialog(ctx, world, project_path);
        self.draw_ui_document_dialog(ctx);
    }

    /// Keeps the Project Explorer's Actor references aligned with the World.
    /// Actors can be placed directly with the Build tool, outside the
    /// Project Explorer's own controls, so reconcile before displaying or
    /// entering Play (where `proj` lookups use this hierarchy).
    pub(crate) fn synchronize_project_actor_nodes(&mut self, world: &World) {
        let actor_ids = world
            .actors
            .keys()
            .copied()
            .collect::<std::collections::HashSet<_>>();

        let stale_nodes = self
            .project_hierarchy
            .nodes
            .values()
            .filter_map(|node| match node.kind {
                ProjectNodeKind::Actor { actor_id } if !actor_ids.contains(&actor_id) => {
                    Some(node.id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut changed = false;
        for node_id in stale_nodes {
            self.project_hierarchy.remove_node(node_id);
            changed = true;
        }

        let mut actor_ids = actor_ids.into_iter().collect::<Vec<_>>();
        actor_ids.sort_unstable();

        for actor_id in actor_ids {
            if self.project_hierarchy.find_actor_node(actor_id).is_none()
                && self
                    .project_hierarchy
                    .create_actor_node(None, actor_id)
                    .is_ok()
            {
                changed = true;
            }
        }

        if changed {
            self.sanitize_project_explorer_selection();
            self.needs_save = true;
        }
    }

    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    // Indexed authoring markers, not millions of blocks; preserve folder placement.
    pub(crate) fn synchronize_project_cell_nodes(&mut self, world: &World) {
        // Revision-gated: no tree traversal or allocation on unchanged frames.
        // Explicitly reset this cache after loading a different project.
        if self.project_cell_sync_revision == Some(world.render_revision) {
            return;
        }
        let kinds = [
            crate::world::CellType::Light,
            crate::world::CellType::AudioEmitter,
            crate::world::CellType::ParticleEmitter,
            crate::world::CellType::SpawnPoint,
            crate::world::CellType::Player,
        ];
        let mut ids = std::collections::HashSet::new();
        for kind in kinds {
            for coord in world.iter_editor_navigator_coords(kind) {
                if let Some(cell) = world.get(coord) {
                    if cell.cell_type == kind && cell.id != 0 {
                        ids.insert(cell.id);
                    }
                }
            }
        }
        // Source-backed lights/emitters share the World ID namespace with Cells.
        for source in world.iter_sources() {
            if kinds.contains(&source.source_type()) {
                ids.insert(source.cell.id);
            }
        }
        let existing = self
            .project_hierarchy
            .nodes
            .values()
            .filter_map(|node| match node.kind {
                ProjectNodeKind::Cell { cell_id, .. } => Some(cell_id),
                _ => None,
            })
            .collect::<std::collections::HashSet<_>>();
        let stale = self
            .project_hierarchy
            .nodes
            .values()
            .filter_map(|n| match n.kind {
                ProjectNodeKind::Cell { cell_id, .. } if !ids.contains(&cell_id) => Some(n.id),
                _ => None,
            })
            .collect::<Vec<_>>();
        for id in stale {
            self.project_hierarchy.remove_node(id);
            self.needs_save = true;
        }
        for cell_id in ids.difference(&existing).copied() {
            if self
                .project_hierarchy
                .create_cell_node(None, cell_id)
                .is_ok()
            {
                self.needs_save = true;
            }
        }
        self.project_cell_sync_revision = Some(world.render_revision);
    }

    pub(crate) fn cell_node_title(cell_id: u64, label: &Option<String>, world: &World) -> String {
        if let Some(label) = label.as_ref().filter(|s| !s.trim().is_empty()) {
            return label.clone();
        }
        let cell_type = world
            .resolve_cell_id(cell_id)
            .and_then(|pos| world.get(pos).map(|cell| cell.cell_type))
            .or_else(|| world.source(cell_id).map(|source| source.source_type()));
        let kind = cell_type
            .map(|cell_type| match cell_type {
                crate::world::CellType::Light => "Light",
                crate::world::CellType::AudioEmitter => "AudioEmitter",
                crate::world::CellType::ParticleEmitter => "ParticleEmitter",
                crate::world::CellType::SpawnPoint => "SpawnPoint",
                crate::world::CellType::Player => "Player",
                _ => "Cell",
            })
            .unwrap_or("MissingCell");
        format!("{kind}_{cell_id}")
    }

    fn draw_ui_document_library(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("UI").strong().size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("+ UI")
                    .on_hover_text("Create a UI document")
                    .clicked()
                {
                    self.ui_document_dialog_rename_id = None;
                    self.ui_document_dialog_name.clear();
                    self.ui_document_dialog_error = None;
                    self.show_ui_document_dialog = true;
                }
            });
        });

        let documents: Vec<(u32, String, String)> = self
            .ui_workspace
            .documents
            .iter()
            .map(|entry| (entry.id, entry.name.clone(), entry.key.clone()))
            .collect();

        egui::ScrollArea::vertical()
            .id_salt("project_ui_documents")
            .max_height(112.0)
            .show(ui, |ui| {
                for (id, name, key) in documents {
                    let selected = self.ui_mode && self.ui_workspace.active_document_id == id;
                    let response = ui
                        .selectable_label(selected, format!("▣  {}", name))
                        .on_hover_text(format!("UI ID: {}", key));

                    if response.clicked() {
                        if let Err(error) = self.open_ui_document(id) {
                            self.project_explorer_status =
                                Some((error, ui.ctx().input(|input| input.time) + 4.0));
                        }
                    }

                    response.context_menu(|ui| {
                        if ui.button("Open UI Editor").clicked() {
                            let _ = self.open_ui_document(id);
                            ui.close_menu();
                        }
                        if ui.button("Rename").clicked() {
                            self.ui_document_dialog_rename_id = Some(id);
                            self.ui_document_dialog_name = name.clone();
                            self.ui_document_dialog_error = None;
                            self.show_ui_document_dialog = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.label(RichText::new(format!("UI ID: {}", key)).monospace().small());
                    });
                }
            });
    }

    fn draw_ui_document_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_ui_document_dialog {
            return;
        }

        let renaming = self.ui_document_dialog_rename_id.is_some();
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new(if renaming { "RENAME UI" } else { "NEW UI" })
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(if renaming {
                    "Rename this UI document. Its script ID remains stable."
                } else {
                    "Create a named UI document."
                });
                ui.add_space(6.0);
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.ui_document_dialog_name)
                        .hint_text("Game HUD")
                        .desired_width(f32::INFINITY),
                );
                response.request_focus();
                if let Some(error) = &self.ui_document_dialog_error {
                    ui.colored_label(Color32::from_rgb(235, 100, 90), error);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(if renaming { "Rename" } else { "Create" })
                        .clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|input| input.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                });
            });

        if submit {
            let result = if let Some(id) = self.ui_document_dialog_rename_id {
                self.ui_workspace
                    .rename_document(id, &self.ui_document_dialog_name)
                    .map(|_| id)
            } else {
                self.sync_active_ui_document();
                self.ui_workspace
                    .create_document(&self.ui_document_dialog_name)
            };

            match result {
                Ok(id) => {
                    if !renaming {
                        let _ = self.ui_workspace.set_active_document(id);
                        self.ui_document = self
                            .ui_workspace
                            .document(id)
                            .map(|entry| entry.document.clone())
                            .unwrap_or_default();
                        self.enter_ui_mode();
                    }
                    self.needs_save = true;
                    self.show_ui_document_dialog = false;
                    self.ui_document_dialog_rename_id = None;
                    self.ui_document_dialog_name.clear();
                    self.ui_document_dialog_error = None;
                }
                Err(error) => self.ui_document_dialog_error = Some(error),
            }
        } else if cancel {
            self.show_ui_document_dialog = false;
            self.ui_document_dialog_rename_id = None;
            self.ui_document_dialog_name.clear();
            self.ui_document_dialog_error = None;
        }
    }

    fn draw_project_explorer_node(
        &mut self,
        ui: &mut egui::Ui,
        node_id: ProjectNodeId,
        depth: usize,
        ancestor_has_more: &mut Vec<bool>,
        world: &mut World,
        project_path: &Option<PathBuf>,
        script_to_open: &mut Option<PathBuf>,
    ) {
        let Some(node) = self.project_hierarchy.node(node_id).cloned() else {
            return;
        };

        if !self.project_node_matches_filter(node_id, world, &self.project_explorer_search) {
            return;
        }

        let children = self.sorted_project_children(Some(node_id), world);
        let has_children = !children.is_empty();
        let searching = !self.project_explorer_search.trim().is_empty();

        let expanded = self.expanded_project_nodes.contains(&node_id) || searching;
        let selected = self.selected_project_node_id == Some(node_id)
            || self.selected_project_nodes.contains(&node_id);

        let row_height = 26.0;
        let indent_step = 16.0;
        let arrow_width = 18.0;
        let icon_width = 18.0;
        let gap = 5.0;

        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_height),
            egui::Sense::click_and_drag(),
        );

        let is_hovered = response.hovered();
        let painter = ui.painter();

        // Selection / Hover background
        if selected {
            let selection_bg = Color32::from_rgba_unmultiplied(45, 90, 160, 200);
            painter.rect_filled(rect, 3.0, selection_bg);
        } else if is_hovered {
            let hover_bg = Color32::from_rgba_unmultiplied(255, 255, 255, 15);
            painter.rect_filled(rect, 3.0, hover_bg);
        }

        // Guide lines for ancestors
        let start_x = rect.min.x;
        for d in 0..depth {
            if d < ancestor_has_more.len() && ancestor_has_more[d] {
                let line_x = start_x + (d as f32 * indent_step) + (arrow_width / 2.0);
                let line_top = egui::pos2(line_x, rect.min.y);
                let line_bottom = egui::pos2(line_x, rect.max.y);
                painter.line_segment(
                    [line_top, line_bottom],
                    egui::Stroke::new(1.0_f32, Color32::from_gray(50)),
                );
            }
        }

        let indent_x = start_x + (depth as f32 * indent_step);

        // Arrow area
        let arrow_rect = egui::Rect::from_min_size(
            egui::pos2(indent_x, rect.min.y),
            egui::vec2(arrow_width, row_height),
        );

        if has_children {
            let arrow_response = ui.interact(
                arrow_rect,
                ui.id().with("arrow").with(node_id),
                egui::Sense::click(),
            );

            if arrow_response.clicked() {
                if expanded {
                    self.expanded_project_nodes.remove(&node_id);
                } else {
                    self.expanded_project_nodes.insert(node_id);
                }
            }

            let arrow_str = if expanded { "▾" } else { "▸" };
            let arrow_color = if arrow_response.hovered() {
                Color32::WHITE
            } else {
                Color32::from_gray(160)
            };

            painter.text(
                arrow_rect.center(),
                egui::Align2::CENTER_CENTER,
                arrow_str,
                egui::FontId::monospace(13.0),
                arrow_color,
            );
        }

        // Icon area
        let icon_x = indent_x + arrow_width;
        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(icon_x, rect.min.y),
            egui::vec2(icon_width, row_height),
        );

        let (icon_symbol, name, icon_color) = self.project_node_display_info(&node, world);

        painter.text(
            icon_rect.center(),
            egui::Align2::CENTER_CENTER,
            icon_symbol,
            egui::FontId::proportional(13.0),
            icon_color,
        );

        // Name text
        let text_x = icon_x + icon_width + gap;
        let text_pos = egui::pos2(text_x, rect.min.y + (row_height - 15.0) / 2.0);
        let text_color = if selected {
            Color32::WHITE
        } else {
            Color32::from_gray(220)
        };

        painter.text(
            text_pos,
            egui::Align2::LEFT_TOP,
            &name,
            egui::FontId::proportional(13.0),
            text_color,
        );

        response.dnd_set_drag_payload(ProjectExplorerDragPayload { node_id });

        if response.drag_started() && !self.selected_project_nodes.contains(&node_id) {
            self.select_project_explorer_node(node_id, &node, world);
        }

        if let Some(payload) = response.dnd_hover_payload::<ProjectExplorerDragPayload>() {
            let ids = self.project_drag_node_ids(payload.node_id);
            let valid = self.can_move_project_nodes(&ids, Some(node_id));
            let color = if valid {
                Color32::from_rgba_unmultiplied(70, 145, 100, 105)
            } else {
                Color32::from_rgba_unmultiplied(170, 65, 65, 80)
            };
            painter.rect_filled(rect, 3.0, color);
            painter.rect_stroke(
                rect.shrink(1.0),
                3.0,
                egui::Stroke::new(
                    1.0_f32,
                    if valid {
                        Color32::from_rgb(100, 205, 135)
                    } else {
                        Color32::from_rgb(205, 100, 100)
                    },
                ),
            );

            if valid && has_children && !expanded {
                let now = ui.input(|input| input.time);
                match self.project_explorer_drag_hover {
                    Some((hovered_id, started_at)) if hovered_id == node_id => {
                        if now - started_at >= 0.65 {
                            self.expanded_project_nodes.insert(node_id);
                            self.project_explorer_drag_hover = None;
                        }
                    }
                    _ => self.project_explorer_drag_hover = Some((node_id, now)),
                }
            }

            if valid {
                if let Some(payload) = response.dnd_release_payload::<ProjectExplorerDragPayload>()
                {
                    self.move_project_drag(payload.node_id, Some(node_id), world);
                }
            }
        }

        // Selection / click handling
        if response.clicked() {
            self.project_explorer_focused = true;
            let modifiers = ui.input(|input| input.modifiers);
            self.select_project_explorer_node_with_modifiers(node_id, &node, world, modifiers);

            if !modifiers.ctrl && !modifiers.shift {
                if let ProjectNodeKind::Script { script_path } = &node.kind {
                    if let Some(root) = project_path {
                        let path = root.join(script_path);

                        if path.exists() {
                            *script_to_open = Some(path);
                        } else if let Some(path) =
                            self.script_editor.indexed_module_path(script_path)
                        {
                            *script_to_open = Some(path);
                        }
                    }
                }
            }
        }

        response.context_menu(|ui| {
            self.draw_explorer_context_menu(
                ui,
                node_id,
                &node,
                world,
                project_path,
                script_to_open,
            );
        });

        // Recursive children
        if expanded && has_children {
            let total_children = children.len();
            for (i, child_id) in children.into_iter().enumerate() {
                let is_last = i == total_children - 1;
                ancestor_has_more.push(!is_last);
                self.draw_project_explorer_node(
                    ui,
                    child_id,
                    depth + 1,
                    ancestor_has_more,
                    world,
                    project_path,
                    script_to_open,
                );
                ancestor_has_more.pop();
            }
        }
    }

    fn draw_explorer_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        node_id: ProjectNodeId,
        node: &ProjectNode,
        world: &mut World,
        project_path: &Option<PathBuf>,
        script_to_open: &mut Option<PathBuf>,
    ) {
        ui.label(RichText::new("PROJECT ITEM").small().strong());
        ui.separator();

        if matches!(node.kind, ProjectNodeKind::Script { .. }) {
            if ui.button("Open").clicked() {
                if let ProjectNodeKind::Script { ref script_path } = node.kind {
                    if let Some(root) = project_path {
                        let path = root.join(script_path);
                        if path.exists() {
                            *script_to_open = Some(path);
                        }
                    }
                }
                ui.close_menu();
            }
        }

        if ui.button("New Folder").clicked() {
            self.project_explorer_folder_parent = Some(node_id);
            self.project_explorer_folder_name.clear();
            self.project_explorer_folder_error = None;
            self.show_project_explorer_folder_dialog = true;
            ui.close_menu();
        }

        if ui.button("New Script").clicked() {
            self.show_script_workspace = true;
            self.script_editor.show_new_script_dialog = true;
            self.script_editor.new_script_parent = Some(node_id);
            self.script_editor.new_script_root = self.selected_project_script_root();
            ui.close_menu();
        }

        ui.separator();

        if ui.button("Rename (F2)").clicked() {
            self.start_rename_project_node(node_id, world);
            ui.close_menu();
        }

        if ui.button("Delete (Del)").clicked() {
            self.start_delete_project_node(node_id);
            ui.close_menu();
        }

        ui.separator();

        if ui.button("Copy").clicked() {
            self.project_explorer_clipboard_node_id = Some(node_id);
            self.set_project_explorer_status(ui.ctx(), "Project item copied");
            ui.close_menu();
        }

        if ui.button("Duplicate").clicked() {
            self.duplicate_project_node(node_id, node.parent_id, world, project_path, ui.ctx());
            ui.close_menu();
        }

        let clipboard_valid = self
            .project_explorer_clipboard_node_id
            .is_some_and(|id| self.project_hierarchy.contains(id));
        if ui
            .add_enabled(clipboard_valid, egui::Button::new("Paste Into"))
            .clicked()
        {
            if let Some(source_id) = self.project_explorer_clipboard_node_id {
                self.duplicate_project_node(
                    source_id,
                    Some(node_id),
                    world,
                    project_path,
                    ui.ctx(),
                );
            }
            ui.close_menu();
        }

        ui.separator();

        if ui.button("Move to Root").clicked() {
            self.move_project_drag(node_id, None, world);
            ui.close_menu();
        }
    }

    fn project_node_display_info(
        &self,
        node: &crate::project::ProjectNode,
        world: &World,
    ) -> (&'static str, String, Color32) {
        match &node.kind {
            ProjectNodeKind::Folder { name } => (
                "▰",
                name.clone(),
                Color32::from_rgb(230, 180, 80), // Amber / Gold
            ),

            ProjectNodeKind::Actor { actor_id } => {
                let name = world
                    .actors
                    .get(actor_id)
                    .map(|actor| {
                        if actor.identity.trim().is_empty() {
                            format!("Actor_{}", actor.id)
                        } else {
                            actor.identity.clone()
                        }
                    })
                    .unwrap_or_else(|| format!("Missing Actor {}", actor_id));

                ("●", name, Color32::from_rgb(80, 200, 220)) // Cyan
            }

            ProjectNodeKind::Cell { cell_id, label } => (
                "◆",
                Self::cell_node_title(*cell_id, label, world),
                Color32::from_rgb(135, 198, 142),
            ),
            ProjectNodeKind::Script { script_path } => {
                let name = Path::new(script_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(script_path)
                    .to_string();

                ("◆", name, Color32::from_rgb(180, 140, 240)) // Purple / Violet
            }
        }
    }

    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    pub(crate) fn select_project_explorer_node(
        &mut self,
        node_id: ProjectNodeId,
        node: &crate::project::ProjectNode,
        world: &World,
    ) {
        self.selected_project_node_id = Some(node_id);
        self.selected_project_nodes = vec![node_id];

        match node.kind {
            ProjectNodeKind::Actor { actor_id } => {
                if world.actors.contains_key(&actor_id) {
                    self.select_actor(actor_id);
                } else {
                    self.clear_selection();
                }
            }

            ProjectNodeKind::Cell { cell_id, .. } => {
                if let Some(coord) = world.resolve_cell_id(cell_id) {
                    self.select_cell(coord);
                } else if world.source(cell_id).is_some() {
                    self.select_source(cell_id);
                } else {
                    self.clear_selection();
                    self.reveal_selected_properties();
                }
            }
            ProjectNodeKind::Folder { .. } | ProjectNodeKind::Script { .. } => {
                self.clear_selection();
                self.reveal_selected_properties();
            }
        }
    }

    fn select_project_explorer_node_with_modifiers(
        &mut self,
        node_id: ProjectNodeId,
        node: &ProjectNode,
        world: &World,
        modifiers: egui::Modifiers,
    ) {
        if modifiers.shift {
            let visible = self.visible_nodes_flattened(world);
            let anchor = self
                .selected_project_node_id
                .and_then(|id| visible.iter().position(|candidate| *candidate == id));
            let clicked = visible.iter().position(|candidate| *candidate == node_id);

            if let (Some(anchor), Some(clicked)) = (anchor, clicked) {
                let (start, end) = if anchor <= clicked {
                    (anchor, clicked)
                } else {
                    (clicked, anchor)
                };
                self.selected_project_nodes = visible[start..=end].to_vec();
                self.selected_project_node_id = Some(node_id);
            } else {
                self.select_project_explorer_node(node_id, node, world);
                return;
            }
        } else if modifiers.ctrl {
            if let Some(index) = self
                .selected_project_nodes
                .iter()
                .position(|selected| *selected == node_id)
            {
                self.selected_project_nodes.remove(index);
                self.selected_project_node_id = self.selected_project_nodes.last().copied();
            } else {
                self.selected_project_nodes.push(node_id);
                self.selected_project_node_id = Some(node_id);
            }
        } else {
            self.select_project_explorer_node(node_id, node, world);
            return;
        }

        if let Some(primary_id) = self.selected_project_node_id {
            if let Some(primary) = self.project_hierarchy.node(primary_id).cloned() {
                match primary.kind {
                    ProjectNodeKind::Actor { actor_id } if world.actors.contains_key(&actor_id) => {
                        self.select_actor(actor_id);
                    }
                    _ => {
                        self.clear_selection();
                        self.reveal_selected_properties();
                    }
                }
            }
        } else {
            self.clear_selection();
        }
    }

    fn push_project_hierarchy_undo_snapshot(&mut self, world: &World) {
        // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
        // Explorer structure edits do not snapshot potentially huge Terrain data.
        let snapshot = self.create_snapshot_without_terrain(world);
        self.history.push_undo_entry(snapshot);
        self.history.redo_stack.clear();
    }

    pub fn synchronize_project_hierarchy_bindings(&mut self, world: &mut World) {
        let enabled_by_pair: std::collections::HashMap<(u64, String), bool> = world
            .actor_script_bindings
            .iter()
            .map(|binding| {
                (
                    (binding.actor_id, binding.normalized_script_path()),
                    binding.enabled,
                )
            })
            .collect();

        let mut expected = Vec::new();
        for node in self.project_hierarchy.nodes.values() {
            let ProjectNodeKind::Script { script_path } = &node.kind else {
                continue;
            };
            let Some(actor_id) = self.project_hierarchy.attached_actor_id(node.id) else {
                continue;
            };
            let normalized_path = script_path.replace('\\', "/");
            let enabled = enabled_by_pair
                .get(&(actor_id, normalized_path.clone()))
                .copied()
                .unwrap_or(true);
            expected.push(crate::scripting::binding::ActorScriptBinding {
                actor_id,
                script_path: normalized_path,
                enabled,
            });
        }

        expected.sort_by(|left, right| {
            left.actor_id
                .cmp(&right.actor_id)
                .then_with(|| left.script_path.cmp(&right.script_path))
        });
        expected.dedup_by(|left, right| {
            left.actor_id == right.actor_id && left.script_path == right.script_path
        });

        world.actor_script_bindings = expected;
        world.rebuild_actor_module_index();
    }

    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    // A drop between Project and Player Scripts moves a REAL .aeo source file.
    fn relocate_project_script_root(
        &mut self,
        node_id: ProjectNodeId,
        target: ScriptRoot,
        world: &mut World,
        project_path: &Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        let operation = (|| -> Result<(), String> {
            let root = project_path.as_ref().ok_or("No project is open")?;
            let node = self
                .project_hierarchy
                .node(node_id)
                .ok_or("Project Explorer node missing")?
                .clone();
            let ProjectNodeKind::Script { script_path } = node.kind else {
                return Err("Only scripts can be moved between script roots".into());
            };
            if script_path.starts_with(&format!("{}/", target.directory_name())) {
                return Ok(());
            }
            if self.project_drag_node_ids(node_id).len() != 1 {
                return Err("Move one script at a time between script roots".into());
            }
            if self.project_hierarchy.find_script_nodes(&script_path).len() != 1 {
                return Err("Script has multiple Explorer references; detach duplicates before moving its source".into());
            }
            if world.script_bindings.iter().any(|b| {
                b.normalized_script_path()
                    .eq_ignore_ascii_case(&script_path)
            }) {
                return Err(
                    "Script is attached to a Cell; detach its Cell binding before moving roots"
                        .into(),
                );
            }
            if self.project_hierarchy.attached_actor_id(node_id).is_some() {
                return Err(
                    "Detach this script from its Actor before changing script roots".into(),
                );
            }
            let filename = Path::new(&script_path)
                .file_name()
                .ok_or("Missing script file name")?;
            let new_path = format!("{}/{}", target.directory_name(), filename.to_string_lossy());
            let mut candidate = self.project_hierarchy.clone();
            candidate.move_node(node_id, None)?;
            candidate.replace_script_path(&script_path, &new_path)?;
            // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
            // Preflight only small hierarchies. Never clone full World history
            // snapshots just to move a script in large authored projects.
            for (_, _, _, _, _, history_tree, _) in self
                .history
                .undo_stack
                .iter()
                .chain(self.history.redo_stack.iter())
            {
                let mut check = history_tree.clone();
                check.replace_script_path(&script_path, &new_path)?;
            }
            self.script_editor
                .move_script_root(root, root.join(&script_path), target)?;
            // Source move is a filesystem action, not a scene undo operation.
            self.project_hierarchy = candidate;
            for (_, _, bindings, _, actor_bindings, history_tree, _) in self
                .history
                .undo_stack
                .iter_mut()
                .chain(self.history.redo_stack.iter_mut())
            {
                history_tree
                    .replace_script_path(&script_path, &new_path)
                    .expect("Hierarchy path updates were prevalidated");
                for binding in bindings {
                    if binding
                        .normalized_script_path()
                        .eq_ignore_ascii_case(&script_path)
                    {
                        binding.script_path = new_path.clone();
                    }
                }
                for binding in actor_bindings {
                    if binding
                        .normalized_script_path()
                        .eq_ignore_ascii_case(&script_path)
                    {
                        binding.script_path = new_path.clone();
                    }
                }
            }
            for disabled in &mut world.disabled_scripts {
                if disabled.eq_ignore_ascii_case(&script_path) {
                    *disabled = new_path.clone();
                }
            }
            self.synchronize_project_hierarchy_bindings(world);
            self.selected_project_node_id = Some(node_id);
            self.selected_project_nodes = vec![node_id];
            self.needs_save = true;
            Ok(())
        })();
        match operation {
            Ok(()) => self.set_project_explorer_status(ctx, "Script moved to its new script root"),
            Err(error) => self.set_project_explorer_status(ctx, error),
        }
    }

    fn move_script_to_player_scripts(
        &mut self,
        node_id: ProjectNodeId,
        world: &mut World,
        project_path: &Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.relocate_project_script_root(
            node_id,
            ScriptRoot::PlayerScripts,
            world,
            project_path,
            ctx,
        );
    }

    fn drop_into_project_root(
        &mut self,
        node_id: ProjectNodeId,
        world: &mut World,
        project_path: &Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        let from_player = self.project_hierarchy.node(node_id).is_some_and(|node| {
            matches!(&node.kind, ProjectNodeKind::Script { script_path }
                if script_path.starts_with("playerscripts/"))
        });
        if from_player {
            self.move_script_to_project_scripts(node_id, world, project_path, ctx);
        } else {
            self.move_project_drag(node_id, None, world);
        }
    }

    fn move_script_to_project_scripts(
        &mut self,
        node_id: ProjectNodeId,
        world: &mut World,
        project_path: &Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.relocate_project_script_root(node_id, ScriptRoot::Scripts, world, project_path, ctx);
    }

    fn project_drag_node_ids(&self, dragged_id: ProjectNodeId) -> Vec<ProjectNodeId> {
        if self.selected_project_nodes.contains(&dragged_id) {
            self.selected_project_nodes.clone()
        } else {
            vec![dragged_id]
        }
    }

    fn can_move_project_nodes(
        &self,
        node_ids: &[ProjectNodeId],
        new_parent_id: Option<ProjectNodeId>,
    ) -> bool {
        // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
        // A Player Script cannot be dragged into an ordinary project folder
        // without also moving its source out of playerscripts/.
        if new_parent_id.is_some()
            && node_ids.iter().any(|id| {
                self.project_hierarchy.node(*id).is_some_and(|node| {
                    matches!(&node.kind, ProjectNodeKind::Script { script_path }
                    if script_path.starts_with("playerscripts/"))
                })
            })
        {
            return false;
        }
        // A normal script dropped onto a player module is not a filesystem move.
        // Only the Player Scripts root drop zone may perform that operation.
        if new_parent_id.is_some_and(|id| {
            self.project_hierarchy.node(id).is_some_and(|node| {
                matches!(&node.kind, ProjectNodeKind::Script { script_path }
                if script_path.starts_with("playerscripts/"))
            })
        }) {
            return false;
        }
        let mut candidate = self.project_hierarchy.clone();
        candidate.move_nodes_atomic(node_ids, new_parent_id).is_ok()
    }

    fn move_project_drag(
        &mut self,
        dragged_id: ProjectNodeId,
        new_parent_id: Option<ProjectNodeId>,
        world: &mut World,
    ) {
        let node_ids = self.project_drag_node_ids(dragged_id);
        if !self.can_move_project_nodes(&node_ids, new_parent_id) {
            return;
        }
        let mut candidate = self.project_hierarchy.clone();

        match candidate.move_nodes_atomic(&node_ids, new_parent_id) {
            Ok(()) => {
                self.push_project_hierarchy_undo_snapshot(world);
                self.project_hierarchy = candidate;
                self.selected_project_nodes = node_ids;
                self.selected_project_node_id = Some(dragged_id);
                if let Some(parent_id) = new_parent_id {
                    self.expanded_project_nodes.insert(parent_id);
                }
                self.synchronize_project_hierarchy_bindings(world);
                self.needs_save = true;
            }
            Err(_) => {}
        }
    }

    fn duplicate_project_node(
        &mut self,
        source_id: ProjectNodeId,
        parent_id: Option<ProjectNodeId>,
        world: &mut World,
        project_path: &Option<PathBuf>,
        ctx: &egui::Context,
    ) {
        let Some(source) = self.project_hierarchy.node(source_id).cloned() else {
            self.set_project_explorer_status(ctx, "Project item not found");
            return;
        };

        match source.kind {
            ProjectNodeKind::Script { script_path } => {
                let Some(project_root) = project_path.as_ref() else {
                    self.set_project_explorer_status(ctx, "Duplicate failed: No project is open.");
                    return;
                };

                let source_path = project_root.join(&script_path);

                let Some((new_source_path, new_logical_path)) = self
                    .script_editor
                    .duplicate_script(project_root, source_path)
                else {
                    self.set_project_explorer_status(ctx, "Script duplication failed");
                    return;
                };

                let mut candidate = self.project_hierarchy.clone();

                match candidate.create_script_node(parent_id, &new_logical_path) {
                    Ok(new_id) => {
                        self.push_project_hierarchy_undo_snapshot(world);

                        self.project_hierarchy = candidate;
                        self.selected_project_node_id = Some(new_id);
                        self.selected_project_nodes = vec![new_id];

                        if let Some(parent_id) = parent_id {
                            self.expanded_project_nodes.insert(parent_id);
                        }

                        self.expanded_project_nodes.insert(new_id);

                        self.synchronize_project_hierarchy_bindings(world);
                        self.needs_save = true;

                        self.set_project_explorer_status(ctx, "Script duplicated");
                    }

                    Err(error) => {
                        // The source file was created first. Roll it back if
                        // hierarchy insertion fails.
                        if let Err(cleanup_error) = std::fs::remove_file(&new_source_path) {
                            self.script_editor.push_output(format!(
                                "Duplicate rollback failed for {:?}: {}",
                                new_source_path, cleanup_error
                            ));
                        }

                        self.script_editor.refresh_scripts(project_path);

                        self.set_project_explorer_status(ctx, error);
                    }
                }
            }

            ProjectNodeKind::Folder { .. }
            | ProjectNodeKind::Actor { .. }
            | ProjectNodeKind::Cell { .. } => {
                let mut candidate = self.project_hierarchy.clone();

                match candidate.duplicate_node(source_id, parent_id) {
                    Ok(new_id) => {
                        self.push_project_hierarchy_undo_snapshot(world);

                        self.project_hierarchy = candidate;
                        self.selected_project_node_id = Some(new_id);
                        self.selected_project_nodes = vec![new_id];

                        if let Some(parent_id) = parent_id {
                            self.expanded_project_nodes.insert(parent_id);
                        }

                        self.expanded_project_nodes.insert(new_id);

                        self.synchronize_project_hierarchy_bindings(world);
                        self.needs_save = true;

                        self.set_project_explorer_status(ctx, "Project item duplicated");
                    }

                    Err(error) => {
                        self.set_project_explorer_status(ctx, error);
                    }
                }
            }
        }
    }

    fn delete_project_node(
        &mut self,
        node_id: ProjectNodeId,
        world: &mut World,
        project_path: &Option<PathBuf>,
    ) -> Result<(), String> {
        let root = self
            .project_hierarchy
            .node(node_id)
            .cloned()
            .ok_or_else(|| "Project item not found".to_string())?;

        let mut subtree_ids = vec![node_id];
        subtree_ids.extend(self.project_hierarchy.descendants_of(Some(node_id)));

        let mut actor_ids = Vec::new();
        let mut cell_coords = Vec::new();
        let mut source_ids = Vec::new();
        let mut script_paths = Vec::new();

        for id in &subtree_ids {
            let Some(node) = self.project_hierarchy.node(*id) else {
                continue;
            };

            match &node.kind {
                ProjectNodeKind::Actor { actor_id } => actor_ids.push(*actor_id),
                ProjectNodeKind::Script { script_path } => {
                    script_paths.push(script_path.replace('\\', "/"));
                }
                ProjectNodeKind::Cell { cell_id, .. } => {
                    if let Some(coord) = world.resolve_cell_id(*cell_id) {
                        // Cell deletion is a scene change, not merely Explorer unlinking.
                        cell_coords.push(coord);
                    } else if world.source(*cell_id).is_some() {
                        source_ids.push(*cell_id);
                    }
                }
                ProjectNodeKind::Folder { .. } => {}
            }
        }

        actor_ids.sort_unstable();
        actor_ids.dedup();
        script_paths.sort();
        script_paths.dedup();

        let delete_script_sources = !matches!(root.kind, ProjectNodeKind::Actor { .. });

        if delete_script_sources && !script_paths.is_empty() {
            let project_root = project_path
                .as_ref()
                .ok_or_else(|| "Delete failed: No project is open.".to_string())?;

            for logical_path in &script_paths {
                let source_path = project_root.join(logical_path);

                // Missing source means the Explorer reference is already stale;
                // allow the hierarchy cleanup to continue.
                if source_path.exists()
                    && !self
                        .script_editor
                        .delete_script(project_path, source_path.clone())
                {
                    return Err(format!("Failed to delete '{}'", logical_path));
                }
            }

            let deleted_paths = script_paths
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>();

            // Deleting a Script deletes the source itself, so every Explorer
            // reference and authored binding to that path is now invalid.
            let stale_script_nodes = self
                .project_hierarchy
                .nodes
                .values()
                .filter_map(|node| match &node.kind {
                    ProjectNodeKind::Script { script_path }
                        if deleted_paths.contains(&script_path.replace('\\', "/")) =>
                    {
                        Some(node.id)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            for stale_node in stale_script_nodes {
                self.project_hierarchy.remove_node(stale_node);
            }

            world
                .script_bindings
                .retain(|binding| !deleted_paths.contains(&binding.normalized_script_path()));

            world
                .disabled_scripts
                .retain(|script_path| !deleted_paths.contains(&script_path.replace('\\', "/")));
        }

        // World::remove_actor() also removes Actor module bindings and
        // rebuilds the Actor module index.
        for actor_id in actor_ids {
            world.remove_actor(actor_id);
        }

        for coord in cell_coords {
            if let Some(cell) = world.get(coord) {
                let id = cell.id;
                world
                    .script_bindings
                    .retain(|binding| binding.target_identity != id);
            }
            world.set_cell(coord, crate::world::CellType::Empty);
        }
        for source_id in source_ids {
            world
                .script_bindings
                .retain(|binding| binding.target_identity != source_id);
            world.remove_source(source_id);
        }
        self.project_hierarchy.remove_node(node_id);
        self.synchronize_project_hierarchy_bindings(world);
        self.sanitize_project_explorer_selection();

        if self
            .selected_actor_id
            .is_some_and(|actor_id| !world.actors.contains_key(&actor_id))
        {
            self.selected_actor_id = None;
        }

        self.needs_save = true;

        Ok(())
    }

    fn set_project_explorer_status(&mut self, ctx: &egui::Context, message: impl Into<String>) {
        let now = ctx.input(|input| input.time);
        self.set_project_explorer_status_with_time(message, 4.0, now);
    }

    fn set_project_explorer_status_with_time(
        &mut self,
        message: impl Into<String>,
        duration: f64,
        now: f64,
    ) {
        self.project_explorer_status = Some((message.into(), now + duration));
    }

    fn project_node_display_name(&self, node_id: ProjectNodeId, world: &World) -> String {
        self.project_hierarchy
            .node(node_id)
            .map(|node| self.project_node_display_info(node, world).1)
            .unwrap_or_default()
    }

    pub fn sorted_project_children(
        &self,
        parent_id: Option<ProjectNodeId>,
        world: &World,
    ) -> Vec<ProjectNodeId> {
        let mut children = self.project_hierarchy.children_of(parent_id);

        children.sort_by(|left, right| {
            let left_node = self.project_hierarchy.node(*left);
            let right_node = self.project_hierarchy.node(*right);

            let left_rank = left_node.map_or(99, |node| match node.kind {
                ProjectNodeKind::Folder { .. } => 0,
                ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. } => 1,
                ProjectNodeKind::Script { .. } => 2,
            });

            let right_rank = right_node.map_or(99, |node| match node.kind {
                ProjectNodeKind::Folder { .. } => 0,
                ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. } => 1,
                ProjectNodeKind::Script { .. } => 2,
            });

            left_rank
                .cmp(&right_rank)
                .then_with(|| {
                    self.project_node_display_name(*left, world)
                        .to_ascii_lowercase()
                        .cmp(
                            &self
                                .project_node_display_name(*right, world)
                                .to_ascii_lowercase(),
                        )
                })
                .then_with(|| left.cmp(right))
        });

        children
    }

    pub fn project_node_matches_filter(
        &self,
        node_id: ProjectNodeId,
        world: &World,
        query: &str,
    ) -> bool {
        let query = query.trim().to_ascii_lowercase();

        if query.is_empty() {
            return true;
        }

        let Some(node) = self.project_hierarchy.node(node_id) else {
            return false;
        };

        let name = self.project_node_display_info(node, world).1;

        if name.to_ascii_lowercase().contains(&query) {
            return true;
        }

        if let ProjectNodeKind::Script { script_path } = &node.kind {
            if script_path.to_ascii_lowercase().contains(&query) {
                return true;
            }
        }

        self.project_hierarchy
            .children_of(Some(node_id))
            .into_iter()
            .any(|child_id| self.project_node_matches_filter(child_id, world, &query))
    }

    fn expand_search_matches(&mut self, world: &World) {
        let query = self.project_explorer_search.trim();

        if query.is_empty() {
            return;
        }

        let matching_ancestors: Vec<ProjectNodeId> = self
            .project_hierarchy
            .nodes
            .values()
            .filter_map(|node| {
                let matching = self.project_node_matches_filter(node.id, world, query);

                if matching { Some(node.id) } else { None }
            })
            .collect();

        for node_id in matching_ancestors {
            self.expanded_project_nodes.insert(node_id);
        }
    }

    pub fn sanitize_project_explorer_selection(&mut self) {
        if let Some(id) = self.selected_project_node_id {
            if !self.project_hierarchy.contains(id) {
                self.selected_project_node_id = None;
            }
        }

        self.selected_project_nodes
            .retain(|id| self.project_hierarchy.contains(*id));

        if let Some(id) = self.selected_project_node_id {
            if !self.selected_project_nodes.contains(&id) {
                self.selected_project_nodes = vec![id];
            }
        } else {
            self.selected_project_nodes.clear();
        }

        self.expanded_project_nodes
            .retain(|id| self.project_hierarchy.contains(*id));
    }

    pub fn selected_project_script_root(&self) -> ScriptRoot {
        let Some(node_id) = self.selected_project_node_id else {
            return ScriptRoot::Scripts;
        };

        let Some(node) = self.project_hierarchy.node(node_id) else {
            return ScriptRoot::Scripts;
        };

        if let ProjectNodeKind::Script { script_path } = &node.kind {
            if script_path.starts_with("playerscripts/") {
                return ScriptRoot::PlayerScripts;
            }
        }

        ScriptRoot::Scripts
    }

    fn handle_explorer_keyboard_input(
        &mut self,
        ctx: &egui::Context,
        world: &World,
        project_path: &Option<PathBuf>,
        script_to_open: &mut Option<PathBuf>,
    ) {
        if !self.project_explorer_focused {
            return;
        }

        if ctx.wants_keyboard_input() {
            return;
        }

        let Some(selected_id) = self.selected_project_node_id else {
            return;
        };

        let visible = self.visible_nodes_flattened(world);
        let current_idx = visible.iter().position(|&id| id == selected_id);

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if let Some(idx) = current_idx {
                if idx + 1 < visible.len() {
                    let next_id = visible[idx + 1];
                    if let Some(node) = self.project_hierarchy.node(next_id).cloned() {
                        self.select_project_explorer_node(next_id, &node, world);
                    }
                }
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if let Some(idx) = current_idx {
                if idx > 0 {
                    let prev_id = visible[idx - 1];
                    if let Some(node) = self.project_hierarchy.node(prev_id).cloned() {
                        self.select_project_explorer_node(prev_id, &node, world);
                    }
                }
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if self.expanded_project_nodes.contains(&selected_id) {
                self.expanded_project_nodes.remove(&selected_id);
            } else if let Some(parent_id) = self.project_hierarchy.parent_of(selected_id) {
                if let Some(parent_node) = self.project_hierarchy.node(parent_id).cloned() {
                    self.select_project_explorer_node(parent_id, &parent_node, world);
                }
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            let children = self.sorted_project_children(Some(selected_id), world);
            if !children.is_empty() {
                if !self.expanded_project_nodes.contains(&selected_id) {
                    self.expanded_project_nodes.insert(selected_id);
                } else {
                    let first_child = children[0];
                    if let Some(child_node) = self.project_hierarchy.node(first_child).cloned() {
                        self.select_project_explorer_node(first_child, &child_node, world);
                    }
                }
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(node) = self.project_hierarchy.node(selected_id) {
                if let ProjectNodeKind::Script { script_path } = &node.kind {
                    if let Some(root) = project_path {
                        let path = root.join(script_path);
                        if path.exists() {
                            *script_to_open = Some(path);
                        } else if let Some(path) =
                            self.script_editor.indexed_module_path(script_path)
                        {
                            *script_to_open = Some(path);
                        }
                    }
                }
            }
        } else if ctx.input(|i| i.key_pressed(egui::Key::F2)) {
            self.start_rename_project_node(selected_id, world);
        } else if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.start_delete_project_node(selected_id);
        }
    }

    pub fn visible_nodes_flattened(&self, world: &World) -> Vec<ProjectNodeId> {
        let mut result = Vec::new();
        let roots = self.sorted_project_children(None, world);
        for root_id in roots {
            self.collect_visible_nodes_flattened(root_id, world, &mut result);
        }
        result
    }

    fn collect_visible_nodes_flattened(
        &self,
        node_id: ProjectNodeId,
        world: &World,
        out: &mut Vec<ProjectNodeId>,
    ) {
        if !self.project_node_matches_filter(node_id, world, &self.project_explorer_search) {
            return;
        }
        out.push(node_id);

        let searching = !self.project_explorer_search.trim().is_empty();
        let expanded = self.expanded_project_nodes.contains(&node_id) || searching;

        if expanded {
            let children = self.sorted_project_children(Some(node_id), world);
            for child_id in children {
                self.collect_visible_nodes_flattened(child_id, world, out);
            }
        }
    }

    fn draw_project_explorer_folder_dialog(&mut self, ctx: &egui::Context, world: &mut World) {
        if !self.show_project_explorer_folder_dialog {
            return;
        }

        let mut create = false;
        let mut cancel = false;

        egui::Window::new("NEW FOLDER")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new("Create a folder in Project Explorer.").strong());
                ui.add_space(6.0);

                ui.label("Name:");
                let edit_resp = ui.text_edit_singleline(&mut self.project_explorer_folder_name);
                if edit_resp.changed() {
                    self.project_explorer_folder_error = None;
                }

                if let Some(err) = &self.project_explorer_folder_error {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(err)
                            .small()
                            .color(Color32::from_rgb(255, 100, 100)),
                    );
                }

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("Create").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        create = true;
                    }

                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.show_project_explorer_folder_dialog = false;
            self.project_explorer_folder_name.clear();
            self.project_explorer_folder_parent = None;
            self.project_explorer_folder_error = None;
            return;
        }

        if create {
            let name = self.project_explorer_folder_name.trim().to_string();
            if name.is_empty() {
                self.project_explorer_folder_error =
                    Some("Folder name cannot be empty.".to_string());
                return;
            }

            if name.contains('/') || name.contains('\\') {
                self.project_explorer_folder_error =
                    Some("Folder name cannot contain '/' or '\\'.".to_string());
                return;
            }

            let mut candidate = self.project_hierarchy.clone();
            if let Ok(new_id) = candidate.create_folder(self.project_explorer_folder_parent, &name)
            {
                self.push_project_hierarchy_undo_snapshot(world);
                self.project_hierarchy = candidate;
                if let Some(parent_id) = self.project_explorer_folder_parent {
                    self.expanded_project_nodes.insert(parent_id);
                }

                self.expanded_project_nodes.insert(new_id);
                self.selected_project_node_id = Some(new_id);
                self.selected_project_nodes = vec![new_id];
                self.clear_selection();
                self.needs_save = true;

                self.show_project_explorer_folder_dialog = false;
                self.project_explorer_folder_name.clear();
                self.project_explorer_folder_parent = None;
                self.project_explorer_folder_error = None;
            } else {
                self.project_explorer_folder_error = Some("Failed to create folder.".to_string());
            }
        }
    }

    pub fn start_rename_project_node(&mut self, node_id: ProjectNodeId, world: &World) {
        if let Some(node) = self.project_hierarchy.node(node_id) {
            self.project_explorer_rename_node_id = Some(node_id);

            self.project_explorer_rename_name = match &node.kind {
                ProjectNodeKind::Folder { name } => name.clone(),

                ProjectNodeKind::Actor { actor_id } => world
                    .actors
                    .get(actor_id)
                    .map(|actor| actor.identity.clone())
                    .unwrap_or_else(|| format!("Actor_{}", actor_id)),

                ProjectNodeKind::Cell { cell_id, label } => {
                    Self::cell_node_title(*cell_id, label, world)
                }
                ProjectNodeKind::Script { script_path } => Path::new(script_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(script_path)
                    .to_string(),
            };

            self.project_explorer_rename_error = None;
            self.show_project_explorer_rename_dialog = true;
        }
    }

    fn apply_project_node_rename(
        &mut self,
        node_id: ProjectNodeId,
        new_name: &str,
        world: &mut World,
        project_path: &Option<PathBuf>,
    ) -> Result<(), String> {
        let new_name = new_name.trim();

        if new_name.is_empty() {
            return Err("Name cannot be empty.".to_string());
        }

        let node = self
            .project_hierarchy
            .node(node_id)
            .cloned()
            .ok_or_else(|| "Project node not found".to_string())?;

        match node.kind {
            ProjectNodeKind::Folder { .. } => {
                let mut candidate = self.project_hierarchy.clone();

                candidate.rename_folder(node_id, new_name)?;

                self.push_project_hierarchy_undo_snapshot(world);
                self.project_hierarchy = candidate;
            }

            ProjectNodeKind::Cell { .. } => {
                let mut candidate = self.project_hierarchy.clone();
                candidate.rename_cell_node(node_id, new_name)?;
                self.push_project_hierarchy_undo_snapshot(world);
                self.project_hierarchy = candidate;
            }

            ProjectNodeKind::Actor { actor_id } => {
                if !world.actors.contains_key(&actor_id) {
                    return Err(format!("Actor {} does not exist", actor_id));
                }

                self.push_project_hierarchy_undo_snapshot(world);

                let actor = world
                    .actors
                    .get_mut(&actor_id)
                    .expect("Actor existence checked above");

                actor.identity = new_name.to_string();
            }

            ProjectNodeKind::Script { script_path } => {
                let root = project_path.as_ref().ok_or("No project is open.")?;
                let filename = if new_name.ends_with(".aeo") {
                    new_name.to_string()
                } else {
                    format!("{new_name}.aeo")
                };
                if filename == ".aeo"
                    || !crate::editor::script_editor::script_editor_files::valid_script_filename(
                        &filename,
                    )
                {
                    return Err("Invalid filename. Rename only changes the file name.".into());
                }
                let new_path = Path::new(&script_path).with_file_name(filename);
                let new_logical_path = new_path.to_string_lossy().replace('\\', "/");
                if new_logical_path == script_path {
                    return Ok(());
                }

                // Validate all references before touching disk. File renames are
                // not world undo operations: old snapshots must follow the file.
                let mut candidate = self.project_hierarchy.clone();
                candidate.replace_script_path(&script_path, &new_logical_path)?;
                let mut undo = self.history.undo_stack.clone();
                let mut redo = self.history.redo_stack.clone();
                for (_, _, bindings, _, actor_bindings, hierarchy, _) in
                    undo.iter_mut().chain(redo.iter_mut())
                {
                    hierarchy.replace_script_path(&script_path, &new_logical_path)?;
                    for binding in bindings {
                        if binding
                            .normalized_script_path()
                            .eq_ignore_ascii_case(&script_path)
                        {
                            binding.script_path = new_logical_path.clone();
                        }
                    }
                    for binding in actor_bindings {
                        if binding
                            .normalized_script_path()
                            .eq_ignore_ascii_case(&script_path)
                        {
                            binding.script_path = new_logical_path.clone();
                        }
                    }
                }
                let log_len = self.script_editor.output_log.len();
                if !self.script_editor.rename_script_with_bindings(
                    root,
                    root.join(&script_path),
                    new_name,
                    world,
                ) {
                    return Err(self
                        .script_editor
                        .output_log
                        .get(log_len)
                        .cloned()
                        .unwrap_or_else(|| "Script rename failed.".into()));
                }
                self.project_hierarchy = candidate;
                self.history.undo_stack = undo;
                self.history.redo_stack = redo;
            }
        }

        self.needs_save = true;

        Ok(())
    }

    fn draw_project_explorer_rename_dialog(
        &mut self,
        ctx: &egui::Context,
        world: &mut World,
        project_path: &Option<PathBuf>,
    ) {
        if !self.show_project_explorer_rename_dialog {
            return;
        }

        let mut rename = false;
        let mut cancel = false;

        egui::Window::new("Rename Node")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new("Enter new name:").strong());
                ui.add_space(6.0);

                let text_resp = ui.text_edit_singleline(&mut self.project_explorer_rename_name);
                if text_resp.changed() {
                    self.project_explorer_rename_error = None;
                }

                if let Some(err) = &self.project_explorer_rename_error {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(err)
                            .small()
                            .color(Color32::from_rgb(255, 100, 100)),
                    );
                }

                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("Rename").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        rename = true;
                    }

                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.show_project_explorer_rename_dialog = false;
            self.project_explorer_rename_node_id = None;
            self.project_explorer_rename_name.clear();
            self.project_explorer_rename_error = None;
            return;
        }

        if rename {
            let Some(node_id) = self.project_explorer_rename_node_id else {
                self.show_project_explorer_rename_dialog = false;
                return;
            };

            let new_name = self.project_explorer_rename_name.trim().to_string();

            if let Err(error) =
                self.apply_project_node_rename(node_id, &new_name, world, project_path)
            {
                self.project_explorer_rename_error = Some(error);

                return;
            }

            self.show_project_explorer_rename_dialog = false;
            self.project_explorer_rename_node_id = None;
            self.project_explorer_rename_name.clear();
            self.project_explorer_rename_error = None;
        }
    }

    pub fn start_delete_project_node(&mut self, node_id: ProjectNodeId) {
        self.project_explorer_delete_node_id = Some(node_id);
        self.show_project_explorer_delete_dialog = true;
    }

    fn draw_project_explorer_delete_dialog(
        &mut self,
        ctx: &egui::Context,
        world: &mut World,
        project_path: &Option<PathBuf>,
    ) {
        if !self.show_project_explorer_delete_dialog {
            return;
        }

        if !self.theme_settings.confirm_destructive_actions {
            if let Some(node_id) = self.project_explorer_delete_node_id.take() {
                match self.delete_project_node(node_id, world, project_path) {
                    Ok(()) => {
                        self.set_project_explorer_status(ctx, "Project item deleted");
                    }
                    Err(error) => {
                        self.set_project_explorer_status(ctx, error);
                    }
                }
            }
            self.show_project_explorer_delete_dialog = false;
            return;
        }

        let mut delete = false;
        let mut cancel = false;

        egui::Window::new("Confirm Delete")
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let is_actor = self
                    .project_explorer_delete_node_id
                    .and_then(|id| self.project_hierarchy.node(id))
                    .is_some_and(|node| matches!(node.kind, ProjectNodeKind::Actor { .. }));
                let prompt = if is_actor {
                    "Remove this Actor reference from Project Explorer? The authored Actor will be removed in the World."
                } else {
                    "Remove this Explorer item and its subtree? Script source files will be deleted from disk."
                };
                ui.label(RichText::new(prompt).strong());
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Enter))
                    {
                        delete = true;
                    }

                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.show_project_explorer_delete_dialog = false;
            self.project_explorer_delete_node_id = None;
            return;
        }

        if delete {
            if let Some(node_id) = self.project_explorer_delete_node_id.take() {
                match self.delete_project_node(node_id, world, project_path) {
                    Ok(()) => {
                        self.set_project_explorer_status(ctx, "Project item deleted");
                    }

                    Err(error) => {
                        self.set_project_explorer_status(ctx, error);
                    }
                }
            }

            self.show_project_explorer_delete_dialog = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    #[test]
    fn explorer_uses_actual_project_name_and_only_two_sections() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let ctx = egui::Context::default();
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                editor.draw_project_explorer_panel(
                    ctx,
                    &mut world,
                    &Some(std::path::PathBuf::from("C:/Games/Marooned")),
                )
            },
        );
        let labels = out
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.job.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        for required in ["Explorer", "Marooned", "Project", "Player Scripts"] {
            assert!(
                labels.iter().any(|label| label == required),
                "missing {required}: {labels:?}"
            );
        }
        for removed in [
            "World",
            "Actors",
            "Cells",
            "Project Assets",
            "Other Assets",
            "UI",
        ] {
            assert!(
                !labels.iter().any(|label| label == removed),
                "old category {removed} still visible"
            );
        }
    }

    #[test]
    fn unified_explorer_indexes_cells_and_sources_in_user_folders() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let coord = crate::world::WorldCoord::new(4, 2, 9);
        let light_id = world.set_cell(coord, crate::world::CellType::Light);
        let audio_id = world.add_source(
            crate::world::CellType::AudioEmitter,
            glam::Vec3::new(5.0, 2.0, 9.0),
        );
        editor.synchronize_project_cell_nodes(&world);
        let light = editor.project_hierarchy.find_cell_node(light_id).unwrap();
        let audio = editor.project_hierarchy.find_cell_node(audio_id).unwrap();
        assert_ne!(light, audio);
        let folder = editor
            .project_hierarchy
            .create_folder(None, "GameWorld")
            .unwrap();
        let subfolder = editor
            .project_hierarchy
            .create_folder(Some(folder), "My Lights")
            .unwrap();
        editor
            .project_hierarchy
            .move_node(light, Some(subfolder))
            .unwrap();
        editor
            .project_hierarchy
            .move_node(audio, Some(folder))
            .unwrap();
        editor
            .project_hierarchy
            .rename_cell_node(light, "Entrance Glow")
            .unwrap();
        editor.synchronize_project_cell_nodes(&world);
        assert_eq!(editor.project_hierarchy.parent_of(light), Some(subfolder));
        assert_eq!(editor.project_hierarchy.parent_of(audio), Some(folder));
        assert_eq!(
            editor.project_node_display_name(light, &world),
            "Entrance Glow"
        );
        assert_eq!(
            editor.project_node_display_name(audio, &world),
            format!("AudioEmitter_{audio_id}")
        );
        let node = editor.project_hierarchy.node(light).unwrap().clone();
        editor.select_project_explorer_node(light, &node, &world);
        assert_eq!(editor.selected_coord, Some(coord));
        let node = editor.project_hierarchy.node(audio).unwrap().clone();
        editor.select_project_explorer_node(audio, &node, &world);
        assert_eq!(editor.selected_source_id, Some(audio_id));
        let encoded = serde_json::to_string(&editor.project_hierarchy).unwrap();
        let restored: crate::project::ProjectHierarchy = serde_json::from_str(&encoded).unwrap();
        assert!(restored.validate().is_ok());
        assert_eq!(restored.parent_of(light), Some(subfolder));
        world.set_cell(coord, crate::world::CellType::Empty);
        editor.synchronize_project_cell_nodes(&world);
        assert!(editor.project_hierarchy.node(light).is_none());
        assert!(editor.project_hierarchy.node(audio).is_some());
        assert_eq!(
            world
                .iter_editor_navigator_coords(crate::world::CellType::Block)
                .count(),
            0
        );
        editor
            .delete_project_node(audio, &mut world, &None)
            .unwrap();
        assert!(world.source(audio_id).is_none());
        assert!(editor.project_hierarchy.node(audio).is_none());
    }

    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-10 09:05 -05:00
    // This drives real egui pointer events; direct move_script_to_player_scripts
    // tests cannot catch an Explorer drop zone that is only 22 pixels tall.
    #[test]
    fn drag_script_into_empty_player_scripts_space_moves_real_file() {
        let root = std::env::temp_dir().join(format!(
            "wo78_player_dnd_{}_{}",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("dnd")
                .replace(':', "_")
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("playerscripts")).unwrap();
        std::fs::write(root.join("scripts/test.aeo"), "debug.log(\"drag test\")").unwrap();
        let mut editor = Editor::new();
        let mut world = World::new();
        let node_id = editor
            .project_hierarchy
            .create_script_node(None, "scripts/test.aeo")
            .unwrap();
        let ctx = egui::Context::default();

        let draw = |events: Vec<egui::Event>, time: f64, editor: &mut Editor, world: &mut World| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1050.0, 800.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ctx| editor.draw_project_explorer_panel(ctx, world, &Some(root.clone())),
            )
        };

        let output = draw(vec![], 0.0, &mut editor, &mut world);
        let find_text = |text: &str| -> egui::Pos2 {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(galley) if galley.galley.job.text == text => Some(galley.pos),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {text} in Explorer"))
        };
        let source = find_text("test.aeo") + egui::vec2(12.0, 8.0);
        let header = find_text("Player Scripts");
        // The original bug: this blank region is >22px below the header.
        let target = egui::pos2(header.x + 75.0, header.y + 110.0);
        let pointer = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        draw(
            vec![egui::Event::PointerMoved(source)],
            0.1,
            &mut editor,
            &mut world,
        );
        draw(vec![pointer(source, true)], 0.2, &mut editor, &mut world);
        draw(
            vec![egui::Event::PointerMoved(source + egui::vec2(16.0, 0.0))],
            0.3,
            &mut editor,
            &mut world,
        );
        draw(
            vec![egui::Event::PointerMoved(target)],
            0.4,
            &mut editor,
            &mut world,
        );
        draw(vec![pointer(target, false)], 0.5, &mut editor, &mut world);
        assert!(
            !root.join("scripts/test.aeo").exists(),
            "Drag to blank Player Scripts space did not move source: {:?}",
            editor.project_explorer_status
        );
        assert_eq!(
            std::fs::read_to_string(root.join("playerscripts/test.aeo")).unwrap(),
            "debug.log(\"drag test\")"
        );
        assert!(
            matches!(&editor.project_hierarchy.node(node_id).unwrap().kind,
            ProjectNodeKind::Script { script_path } if script_path == "playerscripts/test.aeo")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn player_script_move_never_overwrites_destination_or_rebinds_actor() {
        let root =
            std::env::temp_dir().join(format!("wo78_script_collision_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("playerscripts")).unwrap();
        std::fs::write(root.join("scripts/logic.aeo"), "original").unwrap();
        std::fs::write(root.join("playerscripts/logic.aeo"), "keep").unwrap();
        let mut editor = Editor::new();
        let mut world = World::new();
        let script = editor
            .project_hierarchy
            .create_script_node(None, "scripts/logic.aeo")
            .unwrap();
        editor.move_script_to_player_scripts(
            script,
            &mut world,
            &Some(root.clone()),
            &egui::Context::default(),
        );
        assert_eq!(
            std::fs::read_to_string(root.join("scripts/logic.aeo")).unwrap(),
            "original"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("playerscripts/logic.aeo")).unwrap(),
            "keep"
        );
        std::fs::remove_file(root.join("playerscripts/logic.aeo")).unwrap();
        let actor = editor
            .project_hierarchy
            .create_actor_node(None, 55)
            .unwrap();
        editor
            .project_hierarchy
            .move_node(script, Some(actor))
            .unwrap();
        editor.move_script_to_player_scripts(
            script,
            &mut world,
            &Some(root.clone()),
            &egui::Context::default(),
        );
        assert!(root.join("scripts/logic.aeo").exists());
        assert!(!root.join("playerscripts/logic.aeo").exists());
        assert_eq!(editor.project_hierarchy.parent_of(script), Some(actor));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn player_scripts_move_real_script_files_and_preserve_unsaved_drafts() {
        let root = std::env::temp_dir().join(format!("wo78_player_script_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::create_dir_all(root.join("playerscripts")).unwrap();
        let source = root.join("scripts").join("movement.aeo");
        std::fs::write(&source, "debug.log(\"saved\")").unwrap();
        let mut editor = Editor::new();
        let mut world = World::new();
        let node = editor
            .project_hierarchy
            .create_script_node(None, "scripts/movement.aeo")
            .unwrap();
        editor.selected_project_node_id = Some(node);
        editor.selected_project_nodes = vec![node];
        editor.script_editor.open_file(source.clone());
        {
            let doc = editor
                .script_editor
                .open_documents
                .get_mut(&source)
                .unwrap();
            doc.source = "debug.log(\"unsaved\")".into();
            doc.dirty = true;
        }
        let ctx = egui::Context::default();
        editor.move_script_to_player_scripts(node, &mut world, &Some(root.clone()), &ctx);
        let moved = root.join("playerscripts/movement.aeo");
        assert!(!source.exists());
        assert_eq!(
            std::fs::read_to_string(&moved).unwrap(),
            "debug.log(\"saved\")"
        );
        assert_eq!(editor.script_editor.active_document, Some(moved.clone()));
        assert_eq!(
            editor.script_editor.open_documents[&moved].source,
            "debug.log(\"unsaved\")"
        );
        assert!(editor.script_editor.open_documents[&moved].dirty);
        assert!(matches!(&editor.project_hierarchy.node(node).unwrap().kind,
            ProjectNodeKind::Script { script_path } if script_path == "playerscripts/movement.aeo"));
        let index = crate::project::ProjectScriptIndex::build(&root).unwrap();
        assert!(index.contains_path("playerscripts/movement.aeo"));
        editor.drop_into_project_root(node, &mut world, &Some(root.clone()), &ctx);
        assert!(source.exists());
        assert!(!moved.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_empty_explorer() {
        let editor = Editor::new();
        let world = World::new();

        let visible = editor.visible_nodes_flattened(&world);
        assert!(visible.is_empty());
        assert_eq!(editor.project_hierarchy.nodes.len(), 0);
    }

    #[test]
    fn test_root_ordering_folder_actor_script() {
        let mut editor = Editor::new();
        let mut world = World::new();

        let script_id = editor
            .project_hierarchy
            .create_script_node(None, "scripts/z_script.aeo")
            .unwrap();

        let actor_id_val = 100;
        world.actors.insert(
            actor_id_val,
            crate::world::Actor::new(actor_id_val, "TestActor", glam::Vec3::ZERO),
        );
        let actor_id = editor
            .project_hierarchy
            .create_actor_node(None, actor_id_val)
            .unwrap();

        let folder_id = editor
            .project_hierarchy
            .create_folder(None, "A_Folder")
            .unwrap();

        let roots = editor.sorted_project_children(None, &world);
        assert_eq!(roots, vec![folder_id, actor_id, script_id]);
    }

    #[test]
    fn test_search_match_and_ancestor_visibility() {
        let mut editor = Editor::new();
        let world = World::new();

        let folder_id = editor
            .project_hierarchy
            .create_folder(None, "Dungeon")
            .unwrap();

        let script_id = editor
            .project_hierarchy
            .create_script_node(Some(folder_id), "scripts/combat.aeo")
            .unwrap();

        assert!(editor.project_node_matches_filter(folder_id, &world, "combat"));
        assert!(editor.project_node_matches_filter(script_id, &world, "combat"));

        assert!(!editor.project_node_matches_filter(folder_id, &world, "nonexistent"));
        assert!(!editor.project_node_matches_filter(script_id, &world, "nonexistent"));
    }

    #[test]
    fn test_selected_and_expanded_node_sanitation() {
        let mut editor = Editor::new();

        let folder_id = editor
            .project_hierarchy
            .create_folder(None, "Dungeon")
            .unwrap();

        editor.selected_project_node_id = Some(folder_id);
        editor.selected_project_nodes = vec![folder_id];
        editor.expanded_project_nodes.insert(folder_id);

        editor.sanitize_project_explorer_selection();
        assert_eq!(editor.selected_project_node_id, Some(folder_id));

        editor.project_hierarchy.remove_node(folder_id);
        editor.sanitize_project_explorer_selection();

        assert_eq!(editor.selected_project_node_id, None);
        assert!(editor.selected_project_nodes.is_empty());
        assert!(!editor.expanded_project_nodes.contains(&folder_id));
    }

    #[test]
    fn test_script_actor_folder_selection() {
        let mut editor = Editor::new();
        let mut world = World::new();

        let folder_id = editor
            .project_hierarchy
            .create_folder(None, "Dungeon")
            .unwrap();

        let folder_node = editor.project_hierarchy.node(folder_id).cloned().unwrap();
        editor.select_project_explorer_node(folder_id, &folder_node, &world);

        assert_eq!(editor.selected_project_node_id, Some(folder_id));
        assert!(editor.show_properties_window);

        let actor_id_val = 200;
        world.actors.insert(
            actor_id_val,
            crate::world::Actor::new(actor_id_val, "TestActor2", glam::Vec3::ZERO),
        );
        let actor_node_id = editor
            .project_hierarchy
            .create_actor_node(None, actor_id_val)
            .unwrap();

        let actor_node = editor
            .project_hierarchy
            .node(actor_node_id)
            .cloned()
            .unwrap();
        editor.select_project_explorer_node(actor_node_id, &actor_node, &world);

        assert_eq!(editor.selected_project_node_id, Some(actor_node_id));
        assert_eq!(editor.selected_actor_id, Some(actor_id_val));
    }

    // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:18 -05:00
    #[test]
    fn project_explorer_selection_reveals_properties_after_panels_hidden() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let folder = editor
            .project_hierarchy
            .create_folder(None, "Actors")
            .unwrap();
        let script = editor
            .project_hierarchy
            .create_script_node(Some(folder), "scripts/test.aeo")
            .unwrap();
        let actor_id = 400;
        world.actors.insert(
            actor_id,
            crate::world::Actor::new(actor_id, "Actor", glam::Vec3::ZERO),
        );
        let actor = editor
            .project_hierarchy
            .create_actor_node(None, actor_id)
            .unwrap();
        for id in [folder, script, actor] {
            editor.show_properties_window = false;
            editor.shell.properties_visible = false;
            editor.right_panel_tab = super::super::RightPanelTab::Landscaping;
            let node = editor.project_hierarchy.node(id).unwrap().clone();
            editor.select_project_explorer_node(id, &node, &world);
            assert!(editor.show_properties_window);
            assert!(editor.shell.properties_visible);
            assert_eq!(
                editor.right_panel_tab,
                super::super::RightPanelTab::Inspector
            );
            assert_eq!(editor.selected_project_node_id, Some(id));
        }
    }

    #[test]
    fn test_binding_reconciliation_tracks_hierarchy_attachment() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let actor_a = editor.project_hierarchy.create_actor_node(None, 1).unwrap();
        let actor_b = editor.project_hierarchy.create_actor_node(None, 2).unwrap();
        let logic = editor
            .project_hierarchy
            .create_folder(Some(actor_a), "Logic")
            .unwrap();
        let script = editor
            .project_hierarchy
            .create_script_node(Some(logic), "scripts/combat.aeo")
            .unwrap();

        editor.synchronize_project_hierarchy_bindings(&mut world);
        assert_eq!(world.actor_script_bindings.len(), 1);
        assert_eq!(world.actor_script_bindings[0].actor_id, 1);

        editor.move_project_drag(script, Some(actor_b), &mut world);
        assert_eq!(world.actor_script_bindings.len(), 1);
        assert_eq!(world.actor_script_bindings[0].actor_id, 2);

        editor.move_project_drag(script, None, &mut world);
        assert!(world.actor_script_bindings.is_empty());
    }

    #[test]
    fn test_script_below_script_is_not_a_separate_actor_binding() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let actor = editor.project_hierarchy.create_actor_node(None, 1).unwrap();
        let controller = editor
            .project_hierarchy
            .create_script_node(Some(actor), "scripts/controller.aeo")
            .unwrap();
        editor
            .project_hierarchy
            .create_script_node(Some(controller), "scripts/combat.aeo")
            .unwrap();

        editor.synchronize_project_hierarchy_bindings(&mut world);

        assert_eq!(world.actor_script_bindings.len(), 1);
        assert_eq!(
            world.actor_script_bindings[0].script_path,
            "scripts/controller.aeo"
        );
    }

    #[test]
    fn test_same_script_can_bind_to_two_different_actors() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let actor_a = editor.project_hierarchy.create_actor_node(None, 1).unwrap();
        let actor_b = editor.project_hierarchy.create_actor_node(None, 2).unwrap();
        editor
            .project_hierarchy
            .create_script_node(Some(actor_a), "scripts/combat.aeo")
            .unwrap();
        editor
            .project_hierarchy
            .create_script_node(Some(actor_b), "scripts/combat.aeo")
            .unwrap();

        editor.synchronize_project_hierarchy_bindings(&mut world);

        assert_eq!(world.actor_script_bindings.len(), 2);
        assert_eq!(world.actor_script_bindings[0].actor_id, 1);
        assert_eq!(world.actor_script_bindings[1].actor_id, 2);
    }

    #[test]
    fn test_reconciliation_preserves_disabled_binding_when_pair_survives() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let actor = editor.project_hierarchy.create_actor_node(None, 1).unwrap();
        let folder_a = editor
            .project_hierarchy
            .create_folder(Some(actor), "A")
            .unwrap();
        let folder_b = editor
            .project_hierarchy
            .create_folder(Some(actor), "B")
            .unwrap();
        let script = editor
            .project_hierarchy
            .create_script_node(Some(folder_a), "scripts/combat.aeo")
            .unwrap();
        world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding {
                actor_id: 1,
                script_path: "scripts/combat.aeo".to_string(),
                enabled: false,
            });

        editor.move_project_drag(script, Some(folder_b), &mut world);

        assert_eq!(world.actor_script_bindings.len(), 1);
        assert!(!world.actor_script_bindings[0].enabled);
    }

    #[test]
    fn test_delete_reference_removes_subtree_and_binding_but_not_source_file() {
        let test_root = std::env::temp_dir().join(format!(
            "aeo_explorer_delete_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(test_root.join("scripts")).unwrap();
        let source_path = test_root.join("scripts/combat.aeo");
        std::fs::write(&source_path, "fn main() {}\n").unwrap();

        let mut editor = Editor::new();
        let mut world = World::new();
        let actor = editor.project_hierarchy.create_actor_node(None, 1).unwrap();
        let parent = editor
            .project_hierarchy
            .create_script_node(Some(actor), "scripts/combat.aeo")
            .unwrap();
        let child = editor
            .project_hierarchy
            .create_script_node(Some(parent), "scripts/helper.aeo")
            .unwrap();
        editor.synchronize_project_hierarchy_bindings(&mut world);

        editor
            .delete_project_node(parent, &mut world, &Some(test_root.clone()))
            .unwrap();

        assert!(!editor.project_hierarchy.contains(parent));
        assert!(!editor.project_hierarchy.contains(child));
        assert!(world.actor_script_bindings.is_empty());
        assert!(!source_path.exists());
        std::fs::remove_dir_all(test_root).ok();
    }

    #[test]
    fn test_project_explorer_actor_delete_removes_authored_actor() {
        let mut editor = Editor::new();
        let mut world = World::new();

        let actor_id = world.add_actor("TestActor", glam::Vec3::ZERO);

        let actor_node = editor
            .project_hierarchy
            .create_actor_node(None, actor_id)
            .unwrap();

        editor
            .delete_project_node(actor_node, &mut world, &None)
            .unwrap();

        assert!(!world.actors.contains_key(&actor_id));
        assert!(editor.project_hierarchy.node(actor_node).is_none());
    }

    #[test]
    fn test_move_snapshot_restores_hierarchy_and_bindings_for_undo() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let actor = editor.project_hierarchy.create_actor_node(None, 1).unwrap();
        let script = editor
            .project_hierarchy
            .create_script_node(Some(actor), "scripts/combat.aeo")
            .unwrap();
        editor.synchronize_project_hierarchy_bindings(&mut world);

        editor.move_project_drag(script, None, &mut world);
        assert!(world.actor_script_bindings.is_empty());

        let (_, _, _, _, previous_bindings, previous_hierarchy, _) =
            editor.history.undo_stack.pop().unwrap();
        world.actor_script_bindings = previous_bindings;
        editor.project_hierarchy = previous_hierarchy;

        assert_eq!(editor.project_hierarchy.parent_of(script), Some(actor));
        assert_eq!(world.actor_script_bindings.len(), 1);
    }

    #[test]
    fn actor_rename_uses_current_identity_not_actor_id() {
        let mut editor = Editor::new();
        let mut world = World::new();

        let actor_id = world.add_actor("DungeonKey", glam::Vec3::ZERO);

        let actor_node_id = editor
            .project_hierarchy
            .create_actor_node(None, actor_id)
            .unwrap();

        editor.start_rename_project_node(actor_node_id, &world);

        assert_eq!(editor.project_explorer_rename_name, "DungeonKey");

        assert_ne!(
            editor.project_explorer_rename_name,
            format!("Actor_{}", actor_id)
        );
    }

    #[test]
    fn actor_hierarchy_tracks_actors_added_and_removed_outside_explorer() {
        let mut editor = Editor::new();
        let mut world = World::new();

        let actor_id = world.add_actor("PlacedActor", glam::Vec3::ZERO);
        assert!(editor.project_hierarchy.find_actor_node(actor_id).is_none());

        editor.synchronize_project_actor_nodes(&world);

        let actor_node = editor
            .project_hierarchy
            .find_actor_node(actor_id)
            .expect("newly placed Actor should be searchable in the project tree");
        assert!(editor.needs_save);

        let module_node = editor
            .project_hierarchy
            .create_script_node(Some(actor_node), "scripts/placed_actor.aeo")
            .unwrap();
        world
            .actor_script_bindings
            .push(crate::scripting::binding::ActorScriptBinding::new(
                actor_id,
                "scripts/placed_actor.aeo",
            ));

        world.remove_actor(actor_id);
        editor.synchronize_project_actor_nodes(&world);

        assert!(editor.project_hierarchy.node(actor_node).is_none());
        assert!(editor.project_hierarchy.node(module_node).is_none());
        assert!(world.actor_script_bindings.is_empty());
    }

    #[test]
    fn explorer_script_rename_preserves_documents_bindings_selection_and_history() {
        for script_root in ["scripts", "playerscripts"] {
            let root = std::env::temp_dir().join(format!(
                "wo05_explorer_rename_{script_root}_{}",
                std::process::id()
            ));
            std::fs::create_dir_all(root.join(script_root).join("nested")).unwrap();
            let old = format!("{script_root}/nested/old.aeo");
            let new = format!("{script_root}/nested/new.aeo");
            std::fs::write(root.join(&old), "print(\"saved\")").unwrap();
            let mut editor = Editor::new();
            let mut world = World::new();
            let actor = world.add_actor("Coin", glam::Vec3::ZERO);
            let actor_node = editor
                .project_hierarchy
                .create_actor_node(None, actor)
                .unwrap();
            let node = editor
                .project_hierarchy
                .create_script_node(Some(actor_node), &old)
                .unwrap();
            let global = editor
                .project_hierarchy
                .create_script_node(None, &old)
                .unwrap();
            world
                .script_bindings
                .push(crate::scripting::binding::ScriptBinding::new(42, &old));
            world
                .actor_script_bindings
                .push(crate::scripting::binding::ActorScriptBinding::new(
                    actor, &old,
                ));
            world.disabled_scripts.push(old.clone());
            world.rebuild_actor_module_index();
            editor.selected_project_node_id = Some(node);
            editor.selected_project_nodes = vec![node];
            editor.script_editor.open_file(root.join(&old));
            let document = editor
                .script_editor
                .open_documents
                .get_mut(&root.join(&old))
                .unwrap();
            document.source = "print(\"unsaved\")".into();
            document.dirty = true;
            editor.push_project_hierarchy_undo_snapshot(&world);
            editor
                .history
                .redo_stack
                .push(editor.history.undo_stack[0].clone());
            editor.start_rename_project_node(node, &world);
            assert_eq!(editor.project_explorer_rename_name, "old");
            editor
                .apply_project_node_rename(node, "new", &mut world, &Some(root.clone()))
                .unwrap();
            assert!(!root.join(&old).exists());
            assert_eq!(
                std::fs::read_to_string(root.join(&new)).unwrap(),
                "print(\"saved\")"
            );
            assert_eq!(editor.script_editor.active_document, Some(root.join(&new)));
            let document = &editor.script_editor.open_documents[&root.join(&new)];
            assert!(document.dirty);
            assert_eq!(document.source, "print(\"unsaved\")");
            assert_eq!(document.path, root.join(&new));
            assert_eq!(world.script_bindings[0].script_path, new);
            assert_eq!(world.actor_script_bindings[0].script_path, new);
            assert_eq!(world.disabled_scripts, vec![new.clone()]);
            assert_eq!(editor.selected_project_node_id, Some(node));
            assert_eq!(editor.selected_project_nodes, vec![node]);
            for id in [node, global] {
                assert_eq!(
                    editor.project_hierarchy.node(id).unwrap().kind,
                    ProjectNodeKind::Script {
                        script_path: new.clone()
                    }
                );
            }
            for (_, _, bindings, _, actor_bindings, hierarchy, _) in editor
                .history
                .undo_stack
                .iter()
                .chain(&editor.history.redo_stack)
            {
                assert_eq!(bindings[0].script_path, new);
                assert_eq!(actor_bindings[0].script_path, new);
                assert_eq!(
                    hierarchy.node(node).unwrap().kind,
                    ProjectNodeKind::Script {
                        script_path: new.clone()
                    }
                );
            }
            assert!(editor.needs_save);
            assert_eq!(
                editor.script_editor.indexed_module_path(&new),
                Some(root.join(&new))
            );
            editor.script_editor.save_active();
            assert_eq!(
                std::fs::read_to_string(root.join(&new)).unwrap(),
                "print(\"unsaved\")"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn explorer_script_rename_failures_and_noop_preserve_original() {
        let root = std::env::temp_dir().join(format!(
            "wo05_explorer_rename_failures_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        std::fs::write(root.join("scripts/old.aeo"), "print(\"old\")").unwrap();
        std::fs::write(root.join("scripts/taken.aeo"), "print(\"taken\")").unwrap();
        let mut editor = Editor::new();
        let mut world = World::new();
        let node = editor
            .project_hierarchy
            .create_script_node(None, "scripts/old.aeo")
            .unwrap();
        editor.script_editor.open_file(root.join("scripts/old.aeo"));
        for name in [
            "",
            "../escape",
            "folder/name",
            "bad:name",
            "taken.aeo",
            ".aeo",
        ] {
            assert!(
                editor
                    .apply_project_node_rename(node, name, &mut world, &Some(root.clone()))
                    .is_err(),
                "accepted {name}"
            );
            assert_eq!(
                editor.project_hierarchy.node(node).unwrap().kind,
                ProjectNodeKind::Script {
                    script_path: "scripts/old.aeo".into()
                }
            );
            assert_eq!(
                editor.script_editor.active_document,
                Some(root.join("scripts/old.aeo"))
            );
            assert!(!editor.needs_save);
        }
        assert!(
            editor
                .apply_project_node_rename(node, "new", &mut world, &None)
                .is_err()
        );
        editor
            .apply_project_node_rename(node, "old.aeo", &mut world, &Some(root.clone()))
            .unwrap();
        assert!(!editor.needs_save);
        assert_eq!(
            std::fs::read_to_string(root.join("scripts/old.aeo")).unwrap(),
            "print(\"old\")"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("scripts/taken.aeo")).unwrap(),
            "print(\"taken\")"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn actor_rename_preserves_actor_id_node_and_module_attachment() {
        let mut editor = Editor::new();
        let mut world = World::new();

        let actor_id = world.add_actor("Coin", glam::Vec3::ZERO);

        let actor_node_id = editor
            .project_hierarchy
            .create_actor_node(None, actor_id)
            .unwrap();

        editor
            .project_hierarchy
            .create_script_node(Some(actor_node_id), "scripts/coin_behavior.aeo")
            .unwrap();

        editor.synchronize_project_hierarchy_bindings(&mut world);

        assert_eq!(world.actor_script_bindings.len(), 1);

        assert_eq!(world.actor_script_bindings[0].actor_id, actor_id);

        editor
            .apply_project_node_rename(actor_node_id, "GoldCoin", &mut world, &None)
            .unwrap();

        let actor = world
            .actors
            .get(&actor_id)
            .expect("renamed Actor must retain its ID");

        assert_eq!(actor.id, actor_id);
        assert_eq!(actor.identity, "GoldCoin");

        let node = editor
            .project_hierarchy
            .node(actor_node_id)
            .expect("Actor Project node must remain");

        assert_eq!(node.kind, ProjectNodeKind::Actor { actor_id });

        assert_eq!(world.actor_script_bindings.len(), 1);

        assert_eq!(world.actor_script_bindings[0].actor_id, actor_id);

        assert_eq!(
            world.actor_script_bindings[0].script_path,
            "scripts/coin_behavior.aeo"
        );

        assert_eq!(
            editor.project_node_display_name(actor_node_id, &world,),
            "GoldCoin"
        );
    }
}
