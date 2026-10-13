use super::{Editor, RightPanelTab};
use crate::world::WorldCoord;

// Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 18:13 -05:00
// An unknown/missing package appears as None, not a phantom selection.
fn character_dropdown_label<'a>(selected: &'a str, available: &[String]) -> &'a str {
    if available.iter().any(|name| name == selected) {
        selected
    } else {
        "None"
    }
}

#[cfg(test)]
mod character_dropdown_tests {
    use super::character_dropdown_label;

    #[test]
    fn empty_or_missing_character_package_shows_none() {
        assert_eq!(character_dropdown_label("", &[]), "None");
        assert_eq!(character_dropdown_label("Rogue", &[]), "None");
        assert_eq!(
            character_dropdown_label("Missing", &["Rogue".into()]),
            "None"
        );
        assert_eq!(
            character_dropdown_label("Rogue", &["Rogue".into()]),
            "Rogue"
        );
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
#[derive(Clone, Copy, PartialEq, Eq)]
enum LandscapingToolChoice {
    Sculpt(crate::world::terrain::SculptTool),
    ActorBrush,
}

impl Editor {
    pub(crate) fn draw_right_panel(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
    ) {
        let show_prop = self.show_properties_window;
        let show_nav = self.navigation_window.is_open;
        let show_landscaping = self.show_landscaping_window;
        // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:50 -05:00
        let show_ai = self.ai_chat.open;
        if !show_prop && !show_nav && !show_landscaping && !show_ai {
            return;
        }

        // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 13:06 -05:00
        if (self.right_panel_tab == RightPanelTab::Inspector && !show_prop)
            || (self.right_panel_tab == RightPanelTab::Navigation && !show_nav)
            || (self.right_panel_tab == RightPanelTab::Landscaping && !show_landscaping)
            || (self.right_panel_tab == RightPanelTab::Ai && !show_ai)
        {
            self.right_panel_tab = if show_prop {
                RightPanelTab::Inspector
            } else if show_nav {
                RightPanelTab::Navigation
            } else if show_landscaping {
                RightPanelTab::Landscaping
            } else if show_ai {
                RightPanelTab::Ai
            } else {
                RightPanelTab::Inspector
            };
        }

        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:44 -05:00
        let frame = egui::Frame::side_top_panel(&ctx.style())
            .inner_margin(egui::Margin::symmetric(13.0, 8.0))
            .stroke(ctx.style().visuals.widgets.noninteractive.bg_stroke);
        let panel_response = egui::SidePanel::right("right_panel")
            .frame(frame)
            .resizable(true)
            .default_width(self.shell.properties_width)
            .min_width(260.0)
            .max_width(520.0)
            .show(ctx, |ui| {
                // Worldkiln 0.8.7 | WO 0.8.7-34 | 2026-10-07 21:57 -05:00
                // Respect global Theme / Panel Rounding, including rounded controls.

                // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
                ui.add_space(4.0);
                // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
                // The right dock has one clear contextual heading; other old panels
                // remain available in the overflow instead of competing with Properties.
                ui.horizontal(|ui| {
                    let name = match self.right_panel_tab {
                        RightPanelTab::Inspector => "Properties",
                        RightPanelTab::Navigation => "Navigation",
                        RightPanelTab::Landscaping => "Landscaping",
                        RightPanelTab::Ai => "AI Assistant",
                    };
                    ui.strong(name);
                    // Worldkiln 0.8.5 | WO 0.8.7-76 | 2026-10-09 22:50 -05:00
                    // This is the sole AI BETA disclosure, adjacent to the dock title.
                    if self.right_panel_tab == RightPanelTab::Ai {
                        egui::Frame::none()
                            .fill(ui.visuals().widgets.inactive.bg_fill)
                            .rounding(4.0)
                            .inner_margin(egui::Margin::symmetric(4.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("BETA")
                                        .small()
                                        .color(ui.visuals().weak_text_color()),
                                )
                                .on_hover_text(
                                    "May perform inaccurately. Verify important results.",
                                );
                            });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("⋯", |ui| {
                            if ui.button("Hide Properties panel").clicked() {
                                self.shell.properties_visible = false;
                                self.shell.save();
                                ui.close_menu();
                            }
                            ui.separator();
                            for (tab, show, label) in [
                                (RightPanelTab::Inspector, show_prop, "Properties"),
                                (RightPanelTab::Navigation, show_nav, "Navigation"),
                                (RightPanelTab::Landscaping, show_landscaping, "Landscaping"),
                                (RightPanelTab::Ai, show_ai, "AI Assistant"),
                            ] {
                                if show
                                    && ui
                                        .selectable_label(self.right_panel_tab == tab, label)
                                        .clicked()
                                {
                                    self.right_panel_tab = tab;
                                    // Worldkiln 0.8.5 | WO 0.8.7-71 | 2026-10-09 19:52 -05:00
                                    if tab == RightPanelTab::Landscaping {
                                        self.shell.explorer_visible = true;
                                        self.left_panel_tab = super::LeftPanelTab::Terrain;
                                        self.show_world_window = false;
                                    }
                                    ui.close_menu();
                                }
                            }
                        });
                    });
                });
                ui.separator();

                match self.right_panel_tab {
                    RightPanelTab::Inspector => {
                        // // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:44 -05:00 Increase field breathing room without touching
                        // Navigation or Landscaping's independently owned layouts.
                        ui.scope(|ui| {
                            let body = ui.text_style_height(&egui::TextStyle::Body);
                            ui.spacing_mut().item_spacing.y =
                                ui.spacing().item_spacing.y.max(body * 0.32);
                            self.render_properties_content(ui, world, project_path);
                        });
                    }
                    RightPanelTab::Navigation => self.render_navigation_content(ui, world),
                    RightPanelTab::Landscaping => {
                        self.render_landscaping_content(ui, world, project_path)
                    }
                    // Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09 15:00 -05:00
                    RightPanelTab::Ai => {
                        let selection = super::ai_script::selection_context(
                            &self.project_hierarchy,
                            self.selected_project_node_id,
                            self.selected_actor_id,
                            self.selected_coord.is_some(),
                        );
                        if self.ai_chat.draw(
                            ui,
                            &mut self.script_editor,
                            project_path,
                            selection,
                            &self.project_hierarchy,
                        ) {
                            self.settings_page = super::SettingsPage::Ai;
                            self.open_engine_settings(super::SettingsPage::Ai);
                        }
                        // Worldkiln 0.8.5 | WO 0.8.7-80 | 2026-10-10 00:45 -05:00
                        // Forward one-shot AI lifecycle sounds to the existing engine audio queue.
                        if let Some(sound) = self.ai_chat.take_notification() {
                            self.queue_engine_sound(sound);
                        }
                        if self.ai_chat.take_applied() {
                            self.show_script_workspace = true;
                            self.left_panel_tab = super::LeftPanelTab::Project;
                            if let Err(error) =
                                self.register_created_script_in_project_manager(world)
                            {
                                self.script_editor.push_output(format!(
                                    "AI script Project Manager error: {error}"
                                ));
                            }
                        }
                    }
                }
            });
        if ctx.input(|i| i.pointer.any_released()) {
            let width = panel_response.response.rect.width();
            if (width - self.shell.properties_width).abs() > 2.0 {
                self.shell.properties_width = width.clamp(260.0, 520.0);
                self.shell.save();
            }
        }
    }

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 13:21 -05:00
    // Worldkiln 0.9.3 | WO 0.9.3-09 | 2026-10-07 00:24 -05:00
    fn render_landscaping_content(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
    ) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading(egui::RichText::new("LANDSCAPING").monospace().strong());
                ui.label("Terrain sculpting, paint, and Actor placement tools.");
                ui.add_space(8.0);
                // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
                // Same selection as PROJECT > TERRAIN; no second layer stack.
                // A layer is unnamed unless explicitly named by the user.
                let layers = &world.terrain.config.generation_layers;
                if !layers.is_empty() {
                    if self.terrain_selected_generation_layer.is_none_or(
                        |id| !layers.iter().any(|l|l.id==id)) {
                        self.terrain_selected_generation_layer =
                            layers.iter().find(|l|l.visible && !l.locked && l.applied.is_some())
                            .or_else(||layers.first()).map(|l|l.id);
                    }
                    let selected = self.terrain_selected_generation_layer;
                    ui.horizontal(|ui| {
                        ui.label("Edit Layer");
                        let label=layers.iter().enumerate().find(|(_,l)|Some(l.id)==selected)
                            .map(|(i,l)| l.name.as_deref().filter(|s|!s.trim().is_empty())
                                .map(ToOwned::to_owned).unwrap_or_else(||format!("#{}",i+1)))
                            .unwrap_or_else(||"Select".to_owned());
                        egui::ComboBox::from_id_salt("landscaping_selected_generation_layer")
                            .selected_text(label).show_ui(ui,|ui|{
                                for (i,layer) in layers.iter().enumerate() {
                                    let name=layer.name.as_deref().filter(|v|!v.trim().is_empty())
                                        .map(ToOwned::to_owned).unwrap_or_else(||format!("#{}",i+1));
                                    ui.selectable_value(&mut self.terrain_selected_generation_layer,
                                        Some(layer.id),name);
                                }
                            });
                    });
                    if self.terrain_selected_generation_layer != selected {
                        self.finish_sculpt_stroke();
                    }
                    if let Some(layer)=layers.iter().find(|l|Some(l.id)==self.terrain_selected_generation_layer) {
                        if layer.locked { ui.colored_label(egui::Color32::YELLOW,
                            "Selected layer is locked. Unlock under PROJECT > TERRAIN."); }
                        else if !layer.visible { ui.colored_label(egui::Color32::YELLOW,
                            "Selected layer is hidden. Make it visible before editing."); }
                        else if layer.applied.is_none() { ui.colored_label(egui::Color32::YELLOW,
                            "Generate this layer under PROJECT > TERRAIN before Sculpt/Paint."); }
                    }
                } else {
                    ui.small("Editing Base Terrain. Add layers in PROJECT > TERRAIN.");
                }

                // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
                egui::CollapsingHeader::new("TOOLS")
                    .default_open(true)
                    .show(ui, |ui| {
                        // Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
                        // Flatten variants remain one Sculpt Tool with a dedicated Mode selector.
                        let current_category = if self.sculpt_tool.is_flatten() {
                            crate::world::terrain::SculptTool::Flatten
                        } else {
                            self.sculpt_tool
                        };
                        // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
                        let current_choice = if self.actor_brush_enabled {
                            LandscapingToolChoice::ActorBrush
                        } else {
                            LandscapingToolChoice::Sculpt(current_category)
                        };
                        let mut selected_tool = current_choice;
                        let tool_label = match current_choice {
                            LandscapingToolChoice::ActorBrush => "Actor Brush",
                            LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::None) => "None",
                            LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Raise) => "Raise",
                            LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Lower) => "Lower",
                            LandscapingToolChoice::Sculpt(tool) if tool.is_flatten() => "Flatten",
                            LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Smooth) => "Smooth",
                            LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Paint) => "Paint",
                            LandscapingToolChoice::Sculpt(_) => "None",
                        };

                        ui.horizontal(|ui| {
                            ui.label("Tool");
                            egui::ComboBox::from_id_salt("landscaping_sculpt_tool")
                                .selected_text(tool_label)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::None),
                                        "None",
                                    );
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Raise),
                                        "Raise",
                                    );
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Lower),
                                        "Lower",
                                    );
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Flatten),
                                        "Flatten",
                                    );
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Smooth),
                                        "Smooth",
                                    );
                                    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::Sculpt(crate::world::terrain::SculptTool::Paint),
                                        "Paint",
                                    );
                                    ui.selectable_value(
                                        &mut selected_tool,
                                        LandscapingToolChoice::ActorBrush,
                                        "Actor Brush",
                                    );
                                });
                        });

                        if selected_tool != current_choice {
                            match selected_tool {
                                LandscapingToolChoice::ActorBrush => {
                                    self.set_actor_brush_enabled(true);
                                }
                                LandscapingToolChoice::Sculpt(tool) => {
                                    self.set_actor_brush_enabled(false);
                                    self.activate_sculpt_tool(tool);
                                }
                            }
                        }

                        if self.actor_brush_enabled {
                            self.render_actor_brush_controls(ui, project_path);
                        } else if self.sculpt_tool != crate::world::terrain::SculptTool::None {

                        // Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
                        if self.sculpt_tool.is_flatten() {
                            let mut mode = self.sculpt_tool;
                            ui.horizontal(|ui| {
                                ui.label("Flatten Mode");
                                let selected_mode = match mode {
                                    crate::world::terrain::SculptTool::FlattenRaiseOnly => "Raise Only",
                                    crate::world::terrain::SculptTool::FlattenCutOnly => "Cut Only",
                                    _ => "Both",
                                };
                                egui::ComboBox::from_id_salt("landscaping_flatten_mode")
                                    .selected_text(selected_mode)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut mode,
                                            crate::world::terrain::SculptTool::FlattenRaiseOnly,
                                            "Raise Only",
                                        );
                                        ui.selectable_value(
                                            &mut mode,
                                            crate::world::terrain::SculptTool::FlattenCutOnly,
                                            "Cut Only",
                                        );
                                        ui.selectable_value(
                                            &mut mode,
                                            crate::world::terrain::SculptTool::Flatten,
                                            "Both",
                                        );
                                    });
                            });
                            if mode != self.sculpt_tool {
                                self.activate_sculpt_tool(mode);
                            }
                        }

                        ui.horizontal(|ui| {
                            ui.label("Brush Radius");
                            ui.add(
                                egui::Slider::new(&mut self.sculpt_brush_radius, 0.5..=64.0)
                                    .step_by(0.25)
                                    .clamp_to_range(true),
                            );
                        });

                        // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
                        if self.sculpt_tool == crate::world::terrain::SculptTool::Paint {
                            ui.horizontal(|ui| {
                                ui.label("Paint Material");
                                let selected = match self.terrain_paint_material_slot {
                                    crate::world::terrain::TerrainPaintMaterialSlot::Surface => {
                                        format!("Surface · {}", world.terrain.config.materials.surface)
                                    }
                                    crate::world::terrain::TerrainPaintMaterialSlot::Subsurface => {
                                        format!(
                                            "Subsurface · {}",
                                            world.terrain.config.materials.subsurface
                                        )
                                    }
                                    crate::world::terrain::TerrainPaintMaterialSlot::Rock => {
                                        format!("Rock · {}", world.terrain.config.materials.rock)
                                    }
                                    crate::world::terrain::TerrainPaintMaterialSlot::Sand => {
                                        "Sand · sand".to_string()
                                    }
                                    // Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
                                    crate::world::terrain::TerrainPaintMaterialSlot::Dirt => {
                                        "Dirt · dirt.png".to_string()
                                    }
                                };
                                egui::ComboBox::from_id_salt("landscaping_paint_material")
                                    .selected_text(selected)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.terrain_paint_material_slot,
                                            crate::world::terrain::TerrainPaintMaterialSlot::Surface,
                                            format!(
                                                "Surface · {}",
                                                world.terrain.config.materials.surface
                                            ),
                                        );
                                        ui.selectable_value(
                                            &mut self.terrain_paint_material_slot,
                                            crate::world::terrain::TerrainPaintMaterialSlot::Subsurface,
                                            format!(
                                                "Subsurface · {}",
                                                world.terrain.config.materials.subsurface
                                            ),
                                        );
                                        ui.selectable_value(
                                            &mut self.terrain_paint_material_slot,
                                            crate::world::terrain::TerrainPaintMaterialSlot::Rock,
                                            format!(
                                                "Rock · {}",
                                                world.terrain.config.materials.rock
                                            ),
                                        );
                                        ui.selectable_value(
                                            &mut self.terrain_paint_material_slot,
                                            crate::world::terrain::TerrainPaintMaterialSlot::Sand,
                                            "Sand · sand",
                                        );
                                        // Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
                                        ui.selectable_value(
                                            &mut self.terrain_paint_material_slot,
                                            crate::world::terrain::TerrainPaintMaterialSlot::Dirt,
                                            "Dirt · dirt.png",
                                        );
                                    });
                            });
                            ui.label(
                                egui::RichText::new(
                                    "Bare-bones paint changes only the top voxel material under the brush. Shape and occupancy are unchanged.",
                                )
                                .small()
                                .weak(),
                            );
                        } else {
                            ui.horizontal(|ui| {
                                ui.label("Strength");
                                ui.add(
                                    egui::Slider::new(
                                        &mut self.sculpt_strength,
                                        0.01..=crate::world::terrain::SCULPT_STRENGTH_UI_MAX,
                                    )
                                    .step_by(0.01)
                                    .clamp_to_range(true),
                                );
                            });
                        }
                        }
                    });

                // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new("Generation and Terrain configuration live in PROJECT EXPLORER > TERRAIN.")
                        .small()
                        .weak(),
                );
            });
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
    fn render_actor_brush_controls(
        &mut self,
        ui: &mut egui::Ui,
        project_path: &Option<std::path::PathBuf>,
    ) {
        let available = project_path
            .as_ref()
            .map(|path| crate::project::discover_actor_packages(path))
            .unwrap_or_else(crate::project::available_actor_package_names);
        if self
            .actor_brush_package
            .as_ref()
            .is_none_or(|selected| !available.contains(selected))
        {
            self.actor_brush_package = available.first().cloned();
        }
        ui.horizontal(|ui| {
            ui.label("Actor Package");
            let selected = self
                .actor_brush_package
                .as_deref()
                .unwrap_or("No Actor packages");
            egui::ComboBox::from_id_salt("landscaping_actor_brush_package")
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for package in &available {
                        ui.selectable_value(
                            &mut self.actor_brush_package,
                            Some(package.clone()),
                            package,
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            ui.label("Brush Radius");
            ui.add(
                egui::Slider::new(&mut self.sculpt_brush_radius, 0.5..=64.0)
                    .step_by(0.25)
                    .clamp_to_range(true),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Density");
            ui.add(
                egui::Slider::new(&mut self.actor_brush_density, 0.1..=12.0)
                    .step_by(0.1)
                    .suffix(" / 100 studs²"),
            );
        });
        for (label, value) in [
            ("Minimum Scale", &mut self.actor_brush_scale_min),
            ("Maximum Scale", &mut self.actor_brush_scale_max),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(
                    egui::DragValue::new(value)
                        .speed(0.05)
                        .clamp_range(0.05..=20.0),
                );
            });
        }
        if self.actor_brush_scale_min > self.actor_brush_scale_max {
            std::mem::swap(
                &mut self.actor_brush_scale_min,
                &mut self.actor_brush_scale_max,
            );
        }
        for (label, value) in [
            ("Minimum Tilt", &mut self.actor_brush_tilt_min_degrees),
            ("Maximum Tilt", &mut self.actor_brush_tilt_max_degrees),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(
                    egui::DragValue::new(value)
                        .speed(0.5)
                        .clamp_range(0.0..=90.0)
                        .suffix("°"),
                );
            });
        }
        if self.actor_brush_tilt_min_degrees > self.actor_brush_tilt_max_degrees {
            std::mem::swap(
                &mut self.actor_brush_tilt_min_degrees,
                &mut self.actor_brush_tilt_max_degrees,
            );
        }
        ui.checkbox(&mut self.actor_brush_random_y_rotation, "Random Y Rotation");
        // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
        ui.horizontal(|ui| {
            ui.label("Y Offset");
            ui.add(
                egui::DragValue::new(&mut self.actor_brush_y_offset)
                    .speed(0.1)
                    .clamp_range(-20.0..=20.0)
                    .suffix(" studs"),
            );
        });
        ui.label(
            egui::RichText::new(
                "Drag on Terrain to scatter Actors. Negative Y Offset sinks them; positive Y Offset raises them. Each drag is one undo/redo step.",
            )
            .small()
            .weak(),
        );
    }

    // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
    pub(crate) fn draw_output_panel(&mut self, ctx: &egui::Context) {
        if !self.script_editor.show_output && !self.script_editor.show_problems {
            return;
        }

        let script_editor_mode = self.show_script_workspace;
        let min_height = if script_editor_mode { 86.0 } else { 110.0 };
        let max_height = 560.0;
        let mut panel_height = if script_editor_mode {
            self.script_output_panel_height
        } else {
            self.output_panel_height
        };

        egui::TopBottomPanel::bottom("output_panel")
            // Keep the height in Editor state. The framework-owned resizer
            // could display the drag preview and then restore its stale panel
            // state when the pointer was released.
            .resizable(false)
            .exact_height(panel_height)
            .show(ctx, |ui| {
                let (splitter_rect, splitter_response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 7.0),
                    egui::Sense::drag(),
                );
                let splitter_response =
                    splitter_response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
                let stroke = ui.visuals().widgets.inactive.bg_stroke;
                ui.painter().line_segment(
                    [splitter_rect.left_center(), splitter_rect.right_center()],
                    stroke,
                );

                if splitter_response.dragged() {
                    let pointer_delta_y = ui.ctx().input(|input| input.pointer.delta().y);
                    panel_height = (panel_height - pointer_delta_y).clamp(min_height, max_height);
                    ui.ctx().request_repaint();
                }

                // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(!self.script_editor.show_problems, "OUTPUT")
                        .clicked()
                    {
                        self.script_editor.show_output = true;
                        self.script_editor.show_problems = false;
                    }
                    let problem_count = self.script_editor.active_problem_count();
                    let problems_label = if problem_count == 0 {
                        "PROBLEMS".to_string()
                    } else {
                        format!("PROBLEMS ({problem_count})")
                    };
                    if ui
                        .selectable_label(self.script_editor.show_problems, problems_label)
                        .clicked()
                    {
                        self.script_editor.show_output = true;
                        self.script_editor.show_problems = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text("Hide bottom panel")
                            .clicked()
                        {
                            self.script_editor.show_output = false;
                            self.script_editor.show_problems = false;
                        }
                    });
                });
                ui.separator();
                if self.script_editor.show_problems {
                    self.script_editor.draw_problems(ui);
                } else {
                    self.render_terminal_content(ui);
                }
            });

        if script_editor_mode {
            self.script_output_panel_height = panel_height;
        } else {
            self.output_panel_height = panel_height;
        }
    }

    pub(crate) fn draw_ui_right_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("ui_right_panel")
            .resizable(true)
            .default_width(260.0)
            .show(ctx, |ui| {
                // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
                // UI authoring uses the same ThemeSettings-applied egui visuals.
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.heading("UI PROPERTIES");

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("X").clicked() {
                            self.show_properties_window = false;
                        }
                    });
                });

                ui.separator();

                let Some(selected_id) = self.ui_selected_node else {
                    ui.add_space(8.0);
                    ui.label("Select a UI node.");
                    return;
                };

                let Some(node) = self.ui_document.node(selected_id).cloned() else {
                    self.ui_selected_node = None;
                    ui.label("Selected UI node no longer exists.");
                    return;
                };

                ui.label(format!("{}  #{}", node.element_type, node.id));

                ui.add_space(8.0);

                ui.collapsing("TRANSFORM", |ui| {
                    let mut x = node.position[0];
                    let mut y = node.position[1];
                    let mut width = node.size[0];
                    let mut height = node.size[1];

                    if ui
                        .add(egui::DragValue::new(&mut x).speed(1.0).prefix("X: "))
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.position[0] = x;
                            self.needs_save = true;
                        }
                    }

                    if ui
                        .add(egui::DragValue::new(&mut y).speed(1.0).prefix("Y: "))
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.position[1] = y;
                            self.needs_save = true;
                        }
                    }

                    if ui
                        .add(
                            egui::DragValue::new(&mut width)
                                .speed(1.0)
                                .range(1.0..=4096.0)
                                .prefix("W: "),
                        )
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.size[0] = width.max(1.0);
                            self.needs_save = true;
                        }
                    }

                    if ui
                        .add(
                            egui::DragValue::new(&mut height)
                                .speed(1.0)
                                .range(1.0..=4096.0)
                                .prefix("H: "),
                        )
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.size[1] = height.max(1.0);
                            self.needs_save = true;
                        }
                    }

                    ui.add_space(4.0);

                    let parent_label = match node.parent {
                        Some(parent) => format!("UI {}", parent),
                        None => "ROOT".to_string(),
                    };

                    egui::ComboBox::from_id_salt("ui_parent_combo")
                        .selected_text(parent_label)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(node.parent.is_none(), "ROOT").clicked() {
                                if self
                                    .ui_document
                                    .set_parent_preserve_absolute_position(selected_id, None)
                                    .is_ok()
                                {
                                    self.needs_save = true;
                                }

                                ui.close_menu();
                            }

                            for candidate in self.ui_document.node_ids() {
                                if candidate == selected_id {
                                    continue;
                                }

                                let is_current = node.parent == Some(candidate);

                                if ui
                                    .selectable_label(is_current, format!("UI {}", candidate))
                                    .clicked()
                                {
                                    if self
                                        .ui_document
                                        .set_parent_preserve_absolute_position(
                                            selected_id,
                                            Some(candidate),
                                        )
                                        .is_ok()
                                    {
                                        self.needs_save = true;
                                    }

                                    ui.close_menu();
                                }
                            }
                        });
                });

                ui.collapsing("DISPLAY", |ui| {
                    let mut visible = node.visible;
                    let mut enabled = node.enabled;
                    let mut z_index = node.z_index;

                    if ui.checkbox(&mut visible, "Visible").changed() {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.visible = visible;
                            self.needs_save = true;
                        }
                    }

                    if ui.checkbox(&mut enabled, "Enabled").changed() {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.enabled = enabled;
                            self.needs_save = true;
                        }
                    }

                    if ui
                        .add(egui::DragValue::new(&mut z_index).speed(0.1).prefix("Z: "))
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.z_index = z_index;
                            self.needs_save = true;
                        }
                    }
                });

                ui.collapsing("STYLE", |ui| {
                    let mut color = node.color;
                    let mut border_color = node.border_color;
                    let mut corner_radius = node.corner_radius;
                    let mut border_width = node.border_width;

                    if ui.color_edit_button_rgba_unmultiplied(&mut color).changed() {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.color = color;
                            self.needs_save = true;
                        }
                    }

                    ui.add_space(4.0);

                    if ui
                        .color_edit_button_rgba_unmultiplied(&mut border_color)
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.border_color = border_color;
                            self.needs_save = true;
                        }
                    }

                    if ui
                        .add(
                            egui::DragValue::new(&mut corner_radius)
                                .speed(0.5)
                                .range(0.0..=128.0)
                                .prefix("Corner: "),
                        )
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.corner_radius = corner_radius.max(0.0);
                            self.needs_save = true;
                        }
                    }

                    if ui
                        .add(
                            egui::DragValue::new(&mut border_width)
                                .speed(0.25)
                                .range(0.0..=32.0)
                                .prefix("Border: "),
                        )
                        .changed()
                    {
                        if let Some(current) = self.ui_document.node_mut(selected_id) {
                            current.border_width = border_width.max(0.0);
                            self.needs_save = true;
                        }
                    }
                });

                if matches!(node.element_type.as_str(), "Text" | "Button") {
                    ui.collapsing("TEXT", |ui| {
                        let mut text = node.text.clone();

                        if ui
                            .add(
                                egui::TextEdit::multiline(&mut text)
                                    .desired_rows(3)
                                    .desired_width(ui.available_width()),
                            )
                            .changed()
                        {
                            if let Some(current) = self.ui_document.node_mut(selected_id) {
                                current.text = text;
                                self.needs_save = true;
                            }
                        }
                    });
                }

                ui.add_space(10.0);

                if ui.button("DELETE NODE").clicked() {
                    if self.ui_document.delete_node(selected_id).is_ok() {
                        self.ui_selected_node = None;
                        self.needs_save = true;
                    }
                }
            });
    }

    pub(crate) fn draw_ui_left_panel(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<std::path::PathBuf>,
    ) {
        let panel_response = egui::SidePanel::left("ui_left_panel")
            .resizable(true)
            .default_width(self.shell.explorer_width)
            .min_width(185.0)
            .max_width(500.0)
            .show(ctx, |ui| {
                // Worldkiln 0.8.5 | WO 0.8.7-71 | 2026-10-09 20:01 -05:00
                // Restore the full UI asset palette AND hierarchy without
                // letting long preset/node lists push controls off screen.
                egui::ScrollArea::vertical()
                    .id_salt("worldkiln_ui_authoring_left_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
                        // UI authoring uses the same ThemeSettings-applied egui visuals.
                        ui.add_space(4.0);
                        ui.heading("UI NODES");
                        ui.separator();

                        let assets = crate::engine::ui_authoring::discover_ui_assets(
                            project_path.as_deref(),
                        );

                        if assets.is_empty() {
                            ui.label("No UI assets found.");
                            ui.add_space(4.0);
                            ui.label(".assets/UI");
                        } else {
                            for asset_name in assets {
                                let response = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(&asset_name).monospace().strong(),
                                    )
                                    .min_size(egui::vec2(ui.available_width(), 30.0)),
                                );

                                if response.drag_started() {
                                    self.ui_drag_asset = Some(asset_name.clone());
                                }

                                if response.clicked() {
                                    let presets = crate::engine::ui_authoring::builtin_presets();
                                    if presets
                                        .iter()
                                        .any(|(preset_name, _)| *preset_name == asset_name)
                                    {
                                        let ids = self.ui_document.instantiate_preset(&asset_name);
                                        if let Some(&first_id) = ids.first() {
                                            self.select_ui_node(first_id);
                                        }
                                        self.needs_save = true;
                                    } else {
                                        let position = [
                                            32.0 + self.ui_document.nodes.len() as f32 * 12.0,
                                            32.0,
                                        ];

                                        match crate::engine::ui_authoring::load_ui_asset(
                                            project_path.as_deref(),
                                            &asset_name,
                                        ) {
                                            Ok(asset) => {
                                                let id = self
                                                    .ui_document
                                                    .add_node_from_asset(&asset, position);

                                                self.select_ui_node(id);
                                                self.needs_save = true;
                                            }

                                            Err(error) => {
                                                eprintln!(
                                                    "Failed to load UI asset '{}': {}",
                                                    asset_name, error
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        ui.add_space(12.0);

                        ui.heading("HIERARCHY");
                        ui.separator();

                        if self.ui_document.nodes.is_empty() {
                            ui.label("No UI nodes yet.");
                            return;
                        }

                        let root_response = ui.add(
                            egui::Label::new(egui::RichText::new("ROOT (drop here)").monospace())
                                .sense(egui::Sense::click()),
                        );
                        if self.ui_hierarchy_drag_node.is_some() && root_response.hovered() {
                            ui.painter().rect_stroke(
                                root_response.rect.expand(2.0),
                                2.0,
                                egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
                            );
                        }
                        if root_response.hovered() && ui.input(|input| input.pointer.any_released())
                        {
                            if let Some(dragged) = self.ui_hierarchy_drag_node {
                                if self
                                    .ui_document
                                    .set_parent_in(dragged, None, self.ui_preview_resolution.size())
                                    .is_ok()
                                {
                                    self.needs_save = true;
                                }
                            }
                        }

                        for id in self.ui_document.roots() {
                            self.draw_ui_hierarchy_node(ui, id, 0);
                        }

                        let invalid_nodes = self.ui_document.invalid_hierarchy_node_ids();
                        if !invalid_nodes.is_empty() {
                            ui.add_space(6.0);
                            ui.colored_label(
                                egui::Color32::from_rgb(240, 100, 90),
                                "INVALID / UNATTACHED",
                            );
                            for id in invalid_nodes {
                                let label = self
                                    .ui_document
                                    .node(id)
                                    .map(|node| format!("{} #{}", node.name, node.id))
                                    .unwrap_or_else(|| format!("UI {id}"));
                                if ui
                                    .selectable_label(self.ui_selected_node == Some(id), label)
                                    .clicked()
                                {
                                    self.select_ui_node(id);
                                }
                            }
                        }

                        if ui.input(|input| input.pointer.any_released()) {
                            self.ui_hierarchy_drag_node = None;
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
    }

    // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:20 -05:00
    // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:48 -05:00
    // Preserve the pre-existing UI hierarchy helper; WO14 changes only Navigation diagnostics.
    fn draw_ui_hierarchy_node(&mut self, ui: &mut egui::Ui, id: u32, depth: usize) {
        let Some(node) = self.ui_document.node(id).cloned() else {
            return;
        };

        let indent = depth as f32 * 14.0;

        let row_height = 24.0;
        let (row_rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_height),
            egui::Sense::click_and_drag(),
        );
        let label_position = row_rect.left_center() + egui::vec2(indent + 6.0, 0.0);
        let selected = self.ui_selected_node == Some(id);

        if selected {
            ui.painter().rect_filled(
                row_rect,
                2.0,
                egui::Color32::from_rgba_unmultiplied(228, 91, 36, 45),
            );
        } else if response.hovered() {
            ui.painter().rect_filled(
                row_rect,
                2.0,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12),
            );
        }

        if self.ui_hierarchy_drag_node.is_some() && response.hovered() {
            ui.painter().rect_stroke(
                row_rect,
                2.0,
                egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
            );
        }

        ui.painter().text(
            label_position,
            egui::Align2::LEFT_CENTER,
            format!(
                "{} #{}",
                if node.name.is_empty() {
                    &node.element_type
                } else {
                    &node.name
                },
                node.id
            ),
            egui::FontId::monospace(12.0),
            egui::Color32::WHITE,
        );

        if response.clicked() {
            self.select_ui_node(id);
        }
        if response.drag_started() {
            self.ui_hierarchy_drag_node = Some(id);
            self.select_ui_node(id);
        }
        if response.hovered() && ui.input(|input| input.pointer.any_released()) {
            if let Some(dragged) = self.ui_hierarchy_drag_node {
                if dragged != id
                    && self
                        .ui_document
                        .set_parent_in(dragged, Some(id), self.ui_preview_resolution.size())
                        .is_ok()
                {
                    self.needs_save = true;
                }
            }
        }

        for child in self.ui_document.children_of(id) {
            self.draw_ui_hierarchy_node(ui, child, depth + 1);
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:44 -05:00
    fn render_navigation_content(&mut self, ui: &mut egui::Ui, world: &mut crate::world::World) {
        // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
        egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui, |ui| {
        self.render_navmesh_volume_controls(ui, world);
        ui.separator();
        ui.label("EDITOR CAMERA");
        ui.horizontal(|ui| {
            ui.label("X:");
            ui.add(
                egui::TextEdit::singleline(&mut self.navigation_window.x_buf).desired_width(40.0),
            );
            ui.add_space(4.0);
            ui.label("Y:");
            ui.add(
                egui::TextEdit::singleline(&mut self.navigation_window.y_buf).desired_width(40.0),
            );
            ui.add_space(4.0);
            ui.label("Z:");
            ui.add(
                egui::TextEdit::singleline(&mut self.navigation_window.z_buf).desired_width(40.0),
            );
            ui.add_space(8.0);
            if ui.button("Go").clicked() {
                let x = self.navigation_window.x_buf.parse::<i32>().unwrap_or(0);
                let y = self.navigation_window.y_buf.parse::<i32>().unwrap_or(0);
                let z = self.navigation_window.z_buf.parse::<i32>().unwrap_or(0);
                self.set_anchor(WorldCoord::new(x, y, z));
            }
        });

        ui.add_space(12.0);
        ui.separator();
        ui.label("STATIC PATH DIAGNOSTICS");
        ui.small("Use Bounds Volumes to generate on Terrain + Cells. Without volumes, existing Cell navigation is preserved.");
        ui.small("Snapshot-based ground navigation. World edits require an explicit rebuild.");

        let stale = self
            .navigation_window
            .snapshot_is_stale(world.navigation_revision());
        if stale {
            ui.colored_label(
                egui::Color32::from_rgb(244, 196, 96),
                "STALE — World revision changed. Rebuild Snapshot.",
            );
        }

        ui.add_space(6.0);
        let agent_before = self.navigation_window.agent;
        ui.horizontal(|ui| {
            ui.label("Agent width");
            ui.add(
                egui::DragValue::new(&mut self.navigation_window.agent.width_cells).range(1..=16),
            );
            ui.label("height");
            ui.add(
                egui::DragValue::new(&mut self.navigation_window.agent.height_cells).range(1..=32),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Step up");
            ui.add(
                egui::DragValue::new(&mut self.navigation_window.agent.max_step_up).range(0..=16),
            );
            ui.label("down");
            ui.add(
                egui::DragValue::new(&mut self.navigation_window.agent.max_step_down).range(0..=32),
            );
        });
        if self.navigation_window.agent != agent_before {
            self.navigation_window
                .invalidate_grid("Agent settings changed; rebuild the navigation snapshot.");
        }

        fn coordinate_row(ui: &mut egui::Ui, label: &str, value: &mut [i32; 3]) {
            ui.horizontal(|ui| {
                ui.label(label);
                for component in value {
                    ui.add(egui::DragValue::new(component).speed(1));
                }
            });
        }

        let start_before = self.navigation_window.path_start;
        let target_before = self.navigation_window.path_target;
        coordinate_row(ui, "Start ", &mut self.navigation_window.path_start);
        coordinate_row(ui, "Target", &mut self.navigation_window.path_target);
        if self.navigation_window.path_start != start_before
            || self.navigation_window.path_target != target_before
        {
            self.navigation_window
                .invalidate_path("Start/Target changed; use Find Path on the current snapshot.");
        }

        ui.horizontal(|ui| {
            if ui.button("Build Snapshot and Find Path").clicked() {
                // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
                let result = if world.navmesh_volumes.is_empty() {
                    crate::navigation::NavigationGrid::build(world, self.navigation_window.agent)
                } else {
                    crate::navigation::NavigationGrid::build_from_volumes(
                        world, self.navigation_window.agent,
                        &world.navmesh_volumes, world.navmesh_settings
                    )
                };
                match result
                {
                    Ok(grid) => {
                        self.navigation_window.debug_grid = Some(grid);
                        self.navigation_window
                            .mark_snapshot_built(world.navigation_revision());
                        self.navigation_window.solve_debug_path();
                    }
                    Err(error) => {
                        self.navigation_window
                            .invalidate_grid(format!("Snapshot failed: {error}."));
                    }
                }
            }

            let can_reuse = self.navigation_window.debug_grid.is_some()
                && !self
                    .navigation_window
                    .snapshot_is_stale(world.navigation_revision());
            if ui
                .add_enabled(can_reuse, egui::Button::new("Find Path"))
                .on_hover_text("Reuse the current fresh snapshot with Start/Target.")
                .clicked()
            {
                self.navigation_window.solve_debug_path();
            }
        });

        ui.add_space(4.0);
        if stale {
            ui.small(format!(
                "Last snapshot result (stale): {}",
                self.navigation_window.debug_status
            ));
        } else {
            ui.small(&self.navigation_window.debug_status);
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.navigation_window.show_walkable, "Walkable");
            ui.checkbox(&mut self.navigation_window.show_blocked, "Blocked");
            ui.checkbox(&mut self.navigation_window.show_path, "Path");
        });
        if ui.small_button("Clear Navigation Debug").clicked() {
            self.navigation_window.clear_debug();
        }
        });
    }

    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
    /// Volume editing lives in existing NAVIGATION panel and touches no Terrain authoring UI.
    fn render_navmesh_volume_controls(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
    ) {
        egui::CollapsingHeader::new("NAVMESH BOUNDS VOLUMES").default_open(true).show(ui, |ui| {
            let mut changed = false;
            if ui.button("+ Add Bounds Volume").clicked() {
                let id = world.navmesh_volumes.iter().map(|v| v.id).max().unwrap_or(0).saturating_add(1);
                world.navmesh_volumes.push(crate::navigation::NavMeshBoundsVolume::new(id,
                    [self.anchor.x as f32 + 0.5, self.anchor.y as f32 + 4.0, self.anchor.z as f32 + 0.5]));
                self.navigation_window.selected_volume_id = Some(id);
                changed = true;
            }
            let mut remove = None;
            for volume in &mut world.navmesh_volumes {
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.navigation_window.selected_volume_id == Some(volume.id),
                        format!("Volume {}", volume.id)).clicked() {
                        self.navigation_window.selected_volume_id = Some(volume.id);
                    }
                    changed |= ui.checkbox(&mut volume.enabled, "Enabled").changed();
                    if ui.small_button("Remove").clicked() { remove = Some(volume.id); }
                });
                if self.navigation_window.selected_volume_id == Some(volume.id) {
                    for (name,values) in [("Center", &mut volume.center),("Size", &mut volume.size)] {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            for v in values.iter_mut() {
                                let response = if name == "Size" {
                                    ui.add(egui::DragValue::new(v).speed(0.5).range(1.0..=100_000.0))
                                } else {
                                    ui.add(egui::DragValue::new(v).speed(0.5))
                                };
                                changed |= response.changed();
                            }
                        });
                    }
                }
            }
            if let Some(id) = remove {
                world.navmesh_volumes.retain(|v| v.id != id);
                if self.navigation_window.selected_volume_id == Some(id) {
                    self.navigation_window.selected_volume_id = None;
                }
                changed = true;
            }
            ui.small("Multiple volumes combine. Changes are saved with the World, independently of Terrain layers.");
            ui.separator();
            ui.label("TERRAIN WALKABILITY");
            ui.horizontal(|ui| {
                ui.label("Max slope (deg)");
                changed |= ui.add(egui::DragValue::new(&mut world.navmesh_settings.max_slope_degrees)
                    .speed(1.0).range(0.0..=88.0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Max terrain step");
                changed |= ui.add(egui::DragValue::new(&mut world.navmesh_settings.max_step_height)
                    .speed(0.1).range(0.0..=16.0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Max water depth");
                changed |= ui.add(egui::DragValue::new(&mut world.navmesh_settings.max_water_depth)
                    .speed(0.1).range(0.0..=1000.0)).changed();
            });
            if changed {
                world.bump_render_revision();
                self.navigation_window.invalidate_grid("NavMesh volumes/settings changed; rebuild snapshot.");
                self.needs_save = true;
            }
        });
    }

    pub(crate) fn render_terminal_content(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            let mut job = egui::text::LayoutJob::default();

            job.wrap.max_width = ui.available_width();

            for line in self.terminal_output.split_inclusive('\n') {
                let color = if line.contains("[ERROR]") {
                    egui::Color32::from_rgb(255, 80, 80)
                } else if line.contains("[WARNING]") {
                    egui::Color32::from_rgb(255, 220, 0)
                } else {
                    ui.visuals().text_color()
                };

                job.append(
                    line,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::TextStyle::Monospace.resolve(ui.style()),
                        color,
                        ..Default::default()
                    },
                );
            }

            for line in &self.script_editor.output_log {
                let line = format!("[SCRIPT EDITOR] {line}\n");
                job.append(
                    &line,
                    0.0,
                    egui::TextFormat {
                        font_id: egui::TextStyle::Monospace.resolve(ui.style()),
                        color: ui.visuals().text_color(),
                        ..Default::default()
                    },
                );
            }

            // Reserve controls before the log so constrained docks shrink the
            // scroll area instead of pushing Clear below the panel clip.
            ui.horizontal_wrapped(|ui| {
                if ui.button("Clear").clicked() {
                    self.clear_output();
                }

                ui.checkbox(&mut self.terminal_auto_scroll, "Auto-scroll");
            });

            egui::ScrollArea::vertical()
                .id_salt("terminal_scroll")
                .max_height(ui.available_height().max(0.0))
                .auto_shrink([false, false])
                .stick_to_bottom(self.terminal_auto_scroll)
                .show(ui, |ui| {
                    ui.add(egui::Label::new(job).selectable(true));
                });
        });
    }

    pub(crate) fn draw_left_panel(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
    ) {
        self.draw_project_explorer_panel(ctx, world, project_path);
    }

    // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
    pub(crate) fn render_cells_content(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
    ) {
        ui.separator();

        // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
        // Authored Blocks are count-only here. Building this panel must not
        // materialize or sort a coordinate per Block.
        let block_count = world.authored_cell_count(crate::world::CellType::Block);
        ui.label(format!("BLOCKS ({block_count})"));

        let mut spawn_points = world
            .iter_editor_navigator_coords(crate::world::CellType::SpawnPoint)
            .collect::<Vec<_>>();
        let mut fx_blocks = world
            .iter_editor_navigator_coords(crate::world::CellType::FxBlock)
            .collect::<Vec<_>>();
        let mut players = world
            .iter_editor_navigator_coords(crate::world::CellType::Player)
            .collect::<Vec<_>>();

        let sort_coord =
            |a: &WorldCoord, b: &WorldCoord| a.x.cmp(&b.x).then(a.y.cmp(&b.y)).then(a.z.cmp(&b.z));
        spawn_points.sort_by(sort_coord);
        fx_blocks.sort_by(sort_coord);
        players.sort_by(sort_coord);

        let mut lights = Vec::new();
        let mut audio_emitters = Vec::new();
        let mut particle_emitters = Vec::new();
        for source in world.iter_sources() {
            match source.cell.cell_type {
                crate::world::CellType::Light => lights.push(source.cell.id),
                crate::world::CellType::AudioEmitter => audio_emitters.push(source.cell.id),
                crate::world::CellType::ParticleEmitter => particle_emitters.push(source.cell.id),
                _ => {}
            }
        }
        lights.sort_unstable();
        audio_emitters.sort_unstable();
        particle_emitters.sort_unstable();

        let mut actors = world.actors.keys().copied().collect::<Vec<_>>();
        actors.sort_unstable();

        let selected_coord = self.selected_coord;
        let mut clicked_coord = None;
        let mut clicked_source = None;

        clicked_source = clicked_source.or(draw_world_source_navigator(
            ui,
            "LIGHTS",
            &lights,
            self.selected_source_id,
        ));
        clicked_source = clicked_source.or(draw_world_source_navigator(
            ui,
            "AUDIO EMITTERS",
            &audio_emitters,
            self.selected_source_id,
        ));
        clicked_source = clicked_source.or(draw_world_source_navigator(
            ui,
            "PARTICLE EMITTERS",
            &particle_emitters,
            self.selected_source_id,
        ));
        clicked_coord = clicked_coord.or(draw_world_category_navigator(
            ui,
            "SPAWN POINTS",
            &spawn_points,
            selected_coord,
        ));
        clicked_coord = clicked_coord.or(draw_world_category_navigator(
            ui,
            "FX BLOCKS",
            &fx_blocks,
            selected_coord,
        ));
        clicked_coord = clicked_coord.or(draw_world_category_navigator(
            ui,
            "PLAYERS",
            &players,
            selected_coord,
        ));

        let mut clicked_actor = None;
        egui::CollapsingHeader::new(format!("ACTORS ({})", actors.len()))
            .default_open(false)
            .show(ui, |ui| {
                for actor_id in actors.iter().copied() {
                    let label = world
                        .actors
                        .get(&actor_id)
                        .map(|actor| actor.identity.as_str())
                        .filter(|identity| !identity.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("Actor {}", actor_id));
                    if ui
                        .selectable_label(self.selected_actor_id == Some(actor_id), label)
                        .clicked()
                    {
                        clicked_actor = Some(actor_id);
                    }
                }
            });

        ui.separator();

        if let Some(coord) = clicked_coord {
            self.select_cell(coord);
        }
        if let Some(source_id) = clicked_source {
            self.select_source(source_id);
        }
        if let Some(actor_id) = clicked_actor {
            self.select_actor(actor_id);
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
    pub(crate) fn render_lighting_quick_access(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
    ) {
        let lighting_before = (
            world.lighting.shadows_enabled,
            world.lighting.global_light_enabled,
            world.lighting.global_light_direction,
            world.lighting.global_light_color,
            world.lighting.global_light_intensity,
            world.lighting.ambient_intensity,
            world.sky.enabled,
            world.sky.preset.clone(),
        );
        let atmosphere_before = (
            world.sky.atmosphere_enabled,
            world.sky.fog_start_studs,
            world.sky.fog_end_studs,
            world.sky.fog_color,
            world.sky.horizon_strength,
        );
        egui::CollapsingHeader::new("LIGHTING")
            // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:52 -05:00
            // Project-section selection is the 2nd click; expose controls.
            .default_open(true)
            .show(ui, |ui| {
                ui.checkbox(&mut world.lighting.shadows_enabled, "Shadows Enabled");

                ui.checkbox(
                    &mut world.lighting.global_light_enabled,
                    "Global Light Enabled",
                );

                ui.separator();
                ui.label("Global Light Direction");

                ui.horizontal(|ui| {
                    ui.label("X:");

                    ui.add(
                        egui::DragValue::new(&mut world.lighting.global_light_direction.x)
                            .speed(0.01),
                    );

                    ui.label("Y:");

                    ui.add(
                        egui::DragValue::new(&mut world.lighting.global_light_direction.y)
                            .speed(0.01),
                    );

                    ui.label("Z:");

                    ui.add(
                        egui::DragValue::new(&mut world.lighting.global_light_direction.z)
                            .speed(0.01),
                    );
                });

                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Global Light Color:");

                    let mut color = [
                        world.lighting.global_light_color.x,
                        world.lighting.global_light_color.y,
                        world.lighting.global_light_color.z,
                    ];

                    if ui.color_edit_button_rgb(&mut color).changed() {
                        world.lighting.global_light_color =
                            glam::Vec3::new(color[0], color[1], color[2]);
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Global Light Intensity:");

                    ui.add(
                        egui::DragValue::new(&mut world.lighting.global_light_intensity)
                            .speed(0.1)
                            .range(0.0..=f32::MAX),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Ambient Intensity:");

                    ui.add(
                        egui::DragValue::new(&mut world.lighting.ambient_intensity)
                            .speed(0.01)
                            .range(0.0..=1.0),
                    );
                });

                ui.separator();

                ui.checkbox(&mut world.sky.enabled, "Sky Enabled");

                ui.horizontal(|ui| {
                    ui.label("Sky:");

                    let mut current_preset = world.sky.preset.clone();

                    egui::ComboBox::from_id_salt("sky_preset_combo")
                        .selected_text(&current_preset)
                        .show_ui(ui, |ui| {
                            for option in &["Temperate", "Tropical", "Desert", "Snowy", "Mars"] {
                                ui.selectable_value(
                                    &mut current_preset,
                                    option.to_string(),
                                    *option,
                                );
                            }
                        });

                    if current_preset != world.sky.preset {
                        world.sky.preset = current_preset;
                        world.sky.update_preset_textures();
                    }
                });

                // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
                ui.separator();
                ui.checkbox(&mut world.sky.atmosphere_enabled, "Atmosphere Enabled");
                ui.add_enabled_ui(world.sky.atmosphere_enabled, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Fog Start");
                        ui.add(
                            egui::DragValue::new(&mut world.sky.fog_start_studs)
                                .speed(4.0)
                                .range(0.0..=100_000.0),
                        );
                        ui.label("studs");
                    });

                    ui.horizontal(|ui| {
                        ui.label("Fog End");
                        ui.add(
                            egui::DragValue::new(&mut world.sky.fog_end_studs)
                                .speed(4.0)
                                .range(1.0..=100_000.0),
                        );
                        ui.label("studs");
                    });
                    world.sky.fog_end_studs =
                        world.sky.fog_end_studs.max(world.sky.fog_start_studs + 1.0);

                    ui.horizontal(|ui| {
                        ui.label("Fog Color:");
                        let mut color = [
                            world.sky.fog_color.x,
                            world.sky.fog_color.y,
                            world.sky.fog_color.z,
                        ];
                        if ui.color_edit_button_rgb(&mut color).changed() {
                            world.sky.fog_color = glam::Vec3::new(color[0], color[1], color[2]);
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Horizon Strength");
                        ui.add(
                            egui::Slider::new(&mut world.sky.horizon_strength, 0.0..=1.0)
                                .clamp_to_range(true),
                        );
                    });
                });
            });
        let lighting_after = (
            world.lighting.shadows_enabled,
            world.lighting.global_light_enabled,
            world.lighting.global_light_direction,
            world.lighting.global_light_color,
            world.lighting.global_light_intensity,
            world.lighting.ambient_intensity,
            world.sky.enabled,
            world.sky.preset.clone(),
        );
        let atmosphere_after = (
            world.sky.atmosphere_enabled,
            world.sky.fog_start_studs,
            world.sky.fog_end_studs,
            world.sky.fog_color,
            world.sky.horizon_strength,
        );
        if lighting_before != lighting_after || atmosphere_before != atmosphere_after {
            self.needs_save = true;
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
    pub(crate) fn render_physics_quick_access(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
    ) {
        let gravity_before = world.gravity;
        egui::CollapsingHeader::new("PHYSICS")
            // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:52 -05:00
            // Project-section selection is the 2nd click; expose controls.
            .default_open(true)
            .show(ui, |ui| {
                ui.label("Gravity");

                ui.horizontal(|ui| {
                    ui.label("X:");
                    ui.add(egui::DragValue::new(&mut world.gravity.x).speed(0.1));

                    ui.label("Y:");
                    ui.add(egui::DragValue::new(&mut world.gravity.y).speed(0.1));

                    ui.label("Z:");
                    ui.add(egui::DragValue::new(&mut world.gravity.z).speed(0.1));
                });
            });
        if gravity_before != world.gravity {
            self.needs_save = true;
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
    pub(crate) fn render_project_settings_content(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
    ) {
        // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:41 -05:00
        // Project configuration is one dedicated surface. Every category
        // renders its original controls through one authoritative owner,
        // never copied into Explorer or unrelated engine settings.
        use super::feature_nav::ProjectSection;
        let selected = self.navigation.project_section;
        match selected {
            // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:13 -05:00
            // No Project Settings overview placeholder: all entries below
            // are real categories with actionable existing controls.
            ProjectSection::SaveData => {
                self.save_data_ui.render(
                    ui,
                    project_path,
                    self.mode == crate::engine::EditorMode::Editor,
                );
                return;
            }
            ProjectSection::Lighting => {
                self.render_lighting_quick_access(ui, world);
                return;
            }
            ProjectSection::Physics => {
                self.render_physics_quick_access(ui, world);
                return;
            }
            _ => {}
        }

        if selected == ProjectSection::Streaming {
            egui::CollapsingHeader::new("STREAMING")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.editor_max_distance_chunks)
                            .range(0..=64)
                            .prefix("Editor max: ")
                            .custom_formatter(|value, _| {
                                if value == 0.0 {
                                    "Unlimited".into()
                                } else {
                                    format!("{} chunks", value as i32)
                                }
                            }),
                    );

                    ui.add(
                        egui::DragValue::new(&mut self.play_max_distance_chunks)
                            .range(0..=64)
                            .prefix("Play max: ")
                            .custom_formatter(|value, _| {
                                if value == 0.0 {
                                    "Unlimited".into()
                                } else {
                                    format!("{} chunks", value as i32)
                                }
                            }),
                    );
                    world.render_distance.editor_chunks = self.editor_max_distance_chunks;
                    world.render_distance.play_chunks = self.play_max_distance_chunks;
                });
        }

        if selected == ProjectSection::Mouse {
            egui::CollapsingHeader::new("MOUSE")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Cursor:");

                        let mut visible_selected = world.cursor_visible;
                        let mut changed = false;

                        egui::ComboBox::from_id_salt("world_cursor_visible_combo")
                            .selected_text(if visible_selected {
                                "Visible"
                            } else {
                                "Hidden"
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(&mut visible_selected, false, "Hidden")
                                    .clicked()
                                {
                                    changed = true;
                                }

                                if ui
                                    .selectable_value(&mut visible_selected, true, "Visible")
                                    .clicked()
                                {
                                    changed = true;
                                }
                            });

                        if changed {
                            world.cursor_visible = visible_selected;
                            self.needs_save = true;
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Screen:");

                        let mut locked_selected = world.screen_locked;
                        let mut changed = false;

                        egui::ComboBox::from_id_salt("world_screen_locked_combo")
                            .selected_text(if locked_selected {
                                "Locked"
                            } else {
                                "Unlocked"
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(&mut locked_selected, true, "Locked")
                                    .clicked()
                                {
                                    changed = true;
                                }

                                if ui
                                    .selectable_value(&mut locked_selected, false, "Unlocked")
                                    .clicked()
                                {
                                    changed = true;
                                }
                            });

                        if changed {
                            world.screen_locked = locked_selected;
                            self.needs_save = true;
                        }
                    });
                });

            // Worldkiln 0.8.7 | WO 0.8.7-04 | 2026-10-05 13:33 -05:00
        }

        if selected == ProjectSection::Character {
            egui::CollapsingHeader::new("CHARACTER")
                .default_open(true)
                .show(ui, |ui| {
                    let available = if let Some(path) = project_path {
                        crate::project::discover_characters(path)
                    } else {
                        crate::character::package::available_package_names()
                    };

                    ui.horizontal(|ui| {
                        ui.label("Character:");

                        let mut current = world.selected_character.clone();
                        let mut changed = false;

                        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 18:11 -05:00
                        // No ghost Character when packages are missing.
                        // None is also an explicit Player-less project choice.
                        let selected_label = character_dropdown_label(&current, &available);
                        egui::ComboBox::from_id_salt("world_character_combo")
                            .selected_text(selected_label)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(current.is_empty(), "None").clicked() {
                                    current.clear();
                                    changed = true;
                                }
                                for char_name in available {
                                    if ui
                                        .selectable_value(
                                            &mut current,
                                            char_name.clone(),
                                            &char_name,
                                        )
                                        .clicked()
                                    {
                                        changed = true;
                                    }
                                }
                            });

                        if changed {
                            world.selected_character = current;
                            self.needs_save = true;
                        }
                    });

                    ui.add_space(4.0);

                    // Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
                    if ui.button("Import Character...").clicked() {
                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                            if let Some(project_dir) = project_path {
                                match crate::project::import_character_package(project_dir, &folder)
                                {
                                    Ok(character_id) => {
                                        world.selected_character = character_id.clone();
                                        self.needs_save = true;
                                        self.character_import_status = Some((
                                            format!("Imported Character '{character_id}'."),
                                            true,
                                        ));
                                    }
                                    Err(error) => {
                                        crate::engine::diagnostics::report(
                                            "ERROR",
                                            "Character Import",
                                            error.clone(),
                                        );
                                        self.character_import_status =
                                            Some((format!("Import failed: {error}"), false));
                                    }
                                }
                            } else {
                                self.character_import_status = Some((
                                    "Open a project before importing a Character.".to_string(),
                                    false,
                                ));
                            }
                        }
                    }

                    if let Some((message, success)) = &self.character_import_status {
                        let color = if *success {
                            ui.visuals().strong_text_color()
                        } else {
                            ui.visuals().error_fg_color
                        };
                        ui.colored_label(color, message);
                    }
                });
        }

        if selected == ProjectSection::Controller {
            egui::CollapsingHeader::new("CONTROLLER")
                .default_open(true)
                .show(ui, |ui| {
                    let available = if let Some(path) = project_path {
                        crate::project::discover_controllers(path)
                    } else {
                        vec!["thirdPerson_Controller".to_string()]
                    };

                    ui.horizontal(|ui| {
                        ui.label("Controller:");

                        let mut current = world.selected_controller.clone();
                        let mut changed = false;

                        egui::ComboBox::from_id_salt("world_controller_combo")
                            .selected_text(&current)
                            .show_ui(ui, |ui| {
                                for ctrl_name in available {
                                    if ui
                                        .selectable_value(
                                            &mut current,
                                            ctrl_name.clone(),
                                            &ctrl_name,
                                        )
                                        .clicked()
                                    {
                                        changed = true;
                                    }
                                }
                            });

                        if changed {
                            world.selected_controller = current;
                            self.needs_save = true;
                        }
                    });
                });
        }

        if selected == ProjectSection::Camera {
            egui::CollapsingHeader::new("CAMERA")
                .default_open(true)
                .show(ui, |ui| {
                    let available = if let Some(path) = project_path {
                        crate::project::discover_cameras(path)
                    } else {
                        vec![
                            "firstPerson".to_string(),
                            "thirdPerson".to_string(),
                            "topDown".to_string(),
                        ]
                    };

                    ui.horizontal(|ui| {
                        ui.label("Camera:");

                        let mut current = world.selected_camera.clone();
                        let mut changed = false;

                        egui::ComboBox::from_id_salt("world_camera_combo")
                            .selected_text(&current)
                            .show_ui(ui, |ui| {
                                for cam_name in available {
                                    if ui
                                        .selectable_value(&mut current, cam_name.clone(), &cam_name)
                                        .clicked()
                                    {
                                        changed = true;
                                    }
                                }
                            });

                        if changed {
                            world.selected_camera = current;
                            self.needs_save = true;
                        }
                    });
                });
        }
    }

    pub(crate) fn sanitize_actor_hierarchy_selection(&mut self, world: &crate::world::World) {
        if let Some(actor_id) = self.selected_actor_id {
            if !world.actors.contains_key(&actor_id) {
                self.selected_actor_id = None;
            }
        }

        if let Some(directory_id) = self.selected_actor_directory_id {
            if !world
                .actor_hierarchy
                .directories
                .contains_key(&directory_id)
            {
                self.selected_actor_directory_id = None;
            }
        }

        if self.selected_actor_id.is_some() && self.selected_actor_directory_id.is_some() {
            self.selected_actor_directory_id = None;
        }
    }
}

#[derive(Clone, Debug)]
enum ActorHierarchyUiAction {
    SelectDirectory(Option<u64>),
    SelectActor(u64),
    CreateDirectory {
        parent_id: Option<u64>,
        name: String,
    },
    RenameDirectory {
        directory_id: u64,
        name: String,
    },
    MoveDirectory {
        directory_id: u64,
        parent_id: Option<u64>,
    },
    DeleteDirectory {
        directory_id: u64,
    },
    MoveActor {
        actor_id: u64,
        directory_id: Option<u64>,
    },
}

fn draw_actor_hierarchy_panel(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    world: &crate::world::World,
) -> Option<ActorHierarchyUiAction> {
    let mut action = None;

    egui::CollapsingHeader::new(format!("ACTOR HIERARCHY ({})", world.actors.len()))
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name:");

                ui.add(
                    egui::TextEdit::singleline(&mut editor.actor_directory_name)
                        .desired_width(110.0)
                        .hint_text("Directory"),
                );

                let parent_id = editor.selected_actor_directory_id;

                if ui.button("ADD").clicked() {
                    action = Some(ActorHierarchyUiAction::CreateDirectory {
                        parent_id,
                        name: editor.actor_directory_name.clone(),
                    });
                }

                if let Some(directory_id) = editor.selected_actor_directory_id {
                    if ui.button("RENAME").clicked() {
                        action = Some(ActorHierarchyUiAction::RenameDirectory {
                            directory_id,
                            name: editor.actor_directory_name.clone(),
                        });
                    }
                }
            });

            if let Some(directory_id) = editor.selected_actor_directory_id {
                if let Some(directory) = world.actor_hierarchy.directories.get(&directory_id) {
                    ui.horizontal(|ui| {
                        ui.label(format!("Selected Directory: {}", directory.name));

                        if ui.button("ROOT").clicked() {
                            action = Some(ActorHierarchyUiAction::MoveDirectory {
                                directory_id,
                                parent_id: None,
                            });
                        }

                        if ui.button("DELETE").clicked() {
                            action = Some(ActorHierarchyUiAction::DeleteDirectory { directory_id });
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Rename:");

                        ui.add(
                            egui::TextEdit::singleline(&mut editor.actor_directory_name)
                                .desired_width(120.0),
                        );

                        if ui.button("RENAME").clicked()
                            && !editor.actor_directory_name.trim().is_empty()
                        {
                            action = Some(ActorHierarchyUiAction::RenameDirectory {
                                directory_id,
                                name: editor.actor_directory_name.clone(),
                            });
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Move:");

                        let mut move_target = directory.parent_id;

                        let current_label = match move_target {
                            Some(parent_id) => world
                                .actor_hierarchy
                                .directory_path(parent_id)
                                .map(|parts| parts.join("/"))
                                .unwrap_or_else(|| "Root".to_string()),
                            None => "Root".to_string(),
                        };

                        egui::ComboBox::from_id_salt(("actor_directory_move", directory_id))
                            .selected_text(current_label)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(&mut move_target, None, "Root")
                                    .clicked()
                                    && directory.parent_id.is_some()
                                {
                                    action = Some(ActorHierarchyUiAction::MoveDirectory {
                                        directory_id,
                                        parent_id: None,
                                    });
                                }

                                let mut targets: Vec<_> = world
                                    .actor_hierarchy
                                    .directories
                                    .values()
                                    .filter(|candidate| candidate.id != directory_id)
                                    .filter(|candidate| {
                                        !is_actor_directory_descendant(
                                            &world.actor_hierarchy,
                                            candidate.id,
                                            directory_id,
                                        )
                                    })
                                    .collect();

                                targets.sort_by(|a, b| {
                                    let a_path = world
                                        .actor_hierarchy
                                        .directory_path(a.id)
                                        .map(|parts| parts.join("/"))
                                        .unwrap_or_else(|| a.name.clone());

                                    let b_path = world
                                        .actor_hierarchy
                                        .directory_path(b.id)
                                        .map(|parts| parts.join("/"))
                                        .unwrap_or_else(|| b.name.clone());

                                    a_path.cmp(&b_path).then_with(|| a.id.cmp(&b.id))
                                });

                                for target in targets {
                                    let label = world
                                        .actor_hierarchy
                                        .directory_path(target.id)
                                        .map(|parts| parts.join("/"))
                                        .unwrap_or_else(|| target.name.clone());

                                    if ui
                                        .selectable_value(&mut move_target, Some(target.id), label)
                                        .clicked()
                                        && move_target != directory.parent_id
                                    {
                                        action = Some(ActorHierarchyUiAction::MoveDirectory {
                                            directory_id,
                                            parent_id: Some(target.id),
                                        });
                                    }
                                }
                            });
                    });
                } else {
                    editor.selected_actor_directory_id = None;
                }
            }

            if let Some(actor_id) = editor.selected_actor_id {
                if let Some(actor) = world.actors.get(&actor_id) {
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "Selected Actor: {}",
                            if actor.identity.trim().is_empty() {
                                format!("Actor_{}", actor.id)
                            } else {
                                actor.identity.clone()
                            }
                        ));

                        let current_directory = world.actor_hierarchy.directory_for_actor(actor_id);

                        let mut move_target = current_directory;

                        let current_label = match current_directory {
                            Some(directory_id) => world
                                .actor_hierarchy
                                .directory_path(directory_id)
                                .map(|parts| parts.join("/"))
                                .unwrap_or_else(|| "Root".to_string()),
                            None => "Root".to_string(),
                        };

                        egui::ComboBox::from_id_salt(("actor_move_directory", actor_id))
                            .selected_text(current_label)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(&mut move_target, None, "Root")
                                    .clicked()
                                    && current_directory.is_some()
                                {
                                    action = Some(ActorHierarchyUiAction::MoveActor {
                                        actor_id,
                                        directory_id: None,
                                    });
                                }

                                let mut directories: Vec<_> =
                                    world.actor_hierarchy.directories.values().collect();

                                directories.sort_by(|a, b| {
                                    let a_path = world
                                        .actor_hierarchy
                                        .directory_path(a.id)
                                        .map(|parts| parts.join("/"))
                                        .unwrap_or_else(|| a.name.clone());

                                    let b_path = world
                                        .actor_hierarchy
                                        .directory_path(b.id)
                                        .map(|parts| parts.join("/"))
                                        .unwrap_or_else(|| b.name.clone());

                                    a_path.cmp(&b_path).then_with(|| a.id.cmp(&b.id))
                                });

                                for directory in directories {
                                    let label = world
                                        .actor_hierarchy
                                        .directory_path(directory.id)
                                        .map(|parts| parts.join("/"))
                                        .unwrap_or_else(|| directory.name.clone());

                                    if ui
                                        .selectable_value(
                                            &mut move_target,
                                            Some(directory.id),
                                            label,
                                        )
                                        .clicked()
                                        && move_target != current_directory
                                    {
                                        action = Some(ActorHierarchyUiAction::MoveActor {
                                            actor_id,
                                            directory_id: Some(directory.id),
                                        });
                                    }
                                }
                            });
                    });
                } else {
                    editor.selected_actor_id = None;
                }
            }

            ui.separator();

            let root_directories = sorted_actor_directories(world, None);

            for directory_id in root_directories {
                if action.is_none() {
                    draw_actor_hierarchy_directory(ui, editor, world, directory_id, &mut action);
                }
            }

            let mut root_actors: Vec<u64> = world
                .actors
                .keys()
                .copied()
                .filter(|actor_id| {
                    !world
                        .actor_hierarchy
                        .actor_directories
                        .contains_key(actor_id)
                })
                .collect();

            root_actors.sort_by(|a, b| {
                let left = world.actors.get(a);
                let right = world.actors.get(b);

                match (left, right) {
                    (Some(a), Some(b)) => a.identity.cmp(&b.identity).then_with(|| a.id.cmp(&b.id)),
                    _ => a.cmp(b),
                }
            });

            if !root_actors.is_empty() {
                ui.separator();
                ui.label("ROOT ACTORS");

                for actor_id in root_actors {
                    draw_actor_hierarchy_actor(ui, editor, world, actor_id, &mut action);

                    if action.is_some() {
                        break;
                    }
                }
            }

            if world.actors.is_empty() && world.actor_hierarchy.directories.is_empty() {
                ui.label("No Actors or directories.");
            }
        });

    action
}

fn draw_actor_hierarchy_directory(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    world: &crate::world::World,
    directory_id: u64,
    action: &mut Option<ActorHierarchyUiAction>,
) {
    let Some(directory) = world.actor_hierarchy.directories.get(&directory_id) else {
        return;
    };

    let selected = editor.selected_actor_directory_id == Some(directory_id);

    egui::CollapsingHeader::new(format!("[DIR] {}", directory.name))
        .default_open(true)
        .show(ui, |ui| {
            if ui
                .selectable_label(
                    selected,
                    format!(
                        "{}  [{}]",
                        directory.name,
                        world
                            .actor_hierarchy
                            .directory_path(directory_id)
                            .map(|parts| parts.join("/"))
                            .unwrap_or_else(|| "Root".to_string())
                    ),
                )
                .clicked()
            {
                *action = Some(ActorHierarchyUiAction::SelectDirectory(Some(directory_id)));
            }

            let mut actor_ids: Vec<u64> = world
                .actor_hierarchy
                .actor_directories
                .iter()
                .filter_map(|(actor_id, assigned_directory)| {
                    (*assigned_directory == directory_id).then_some(*actor_id)
                })
                .filter(|actor_id| world.actors.contains_key(actor_id))
                .collect();

            actor_ids.sort_by(|a, b| {
                let left = world.actors.get(a);
                let right = world.actors.get(b);

                match (left, right) {
                    (Some(a), Some(b)) => a.identity.cmp(&b.identity).then_with(|| a.id.cmp(&b.id)),
                    _ => a.cmp(b),
                }
            });

            for actor_id in actor_ids {
                draw_actor_hierarchy_actor(ui, editor, world, actor_id, action);

                if action.is_some() {
                    return;
                }
            }

            let child_directories = sorted_actor_directories(world, Some(directory_id));

            for child_id in child_directories {
                draw_actor_hierarchy_directory(ui, editor, world, child_id, action);

                if action.is_some() {
                    return;
                }
            }
        });
}

fn draw_actor_hierarchy_actor(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    world: &crate::world::World,
    actor_id: u64,
    action: &mut Option<ActorHierarchyUiAction>,
) {
    let Some(actor) = world.actors.get(&actor_id) else {
        return;
    };

    let selected = editor.selected_actor_id == Some(actor_id);

    let label = if actor.identity.trim().is_empty() {
        format!("Actor_{}", actor.id)
    } else {
        actor.identity.clone()
    };

    if ui.selectable_label(selected, label).clicked() {
        *action = Some(ActorHierarchyUiAction::SelectActor(actor_id));
    }
}

fn sorted_actor_directories(world: &crate::world::World, parent_id: Option<u64>) -> Vec<u64> {
    let mut directories: Vec<_> = world
        .actor_hierarchy
        .directories
        .values()
        .filter(|directory| directory.parent_id == parent_id)
        .map(|directory| directory.id)
        .collect();

    directories.sort_by(|a, b| {
        let left = world.actor_hierarchy.directories.get(a);
        let right = world.actor_hierarchy.directories.get(b);

        match (left, right) {
            (Some(a), Some(b)) => a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)),
            _ => a.cmp(b),
        }
    });

    directories
}

fn is_actor_directory_descendant(
    hierarchy: &crate::world::actor_hierarchy::ActorHierarchy,
    candidate_id: u64,
    ancestor_id: u64,
) -> bool {
    let mut current = Some(candidate_id);

    while let Some(directory_id) = current {
        if directory_id == ancestor_id {
            return true;
        }

        current = hierarchy
            .directories
            .get(&directory_id)
            .and_then(|directory| directory.parent_id);
    }

    false
}

// Worldkiln 0.8.7 | WO 0.8.7-32 | 2026-10-07 16:53 -05:00
fn draw_world_category_navigator(
    ui: &mut egui::Ui,
    name: &str,
    list: &[WorldCoord],
    selected_coord: Option<WorldCoord>,
) -> Option<WorldCoord> {
    let mut clicked_coord = None;

    egui::CollapsingHeader::new(format!("{} ({})", name, list.len()))
        .default_open(false)
        .show(ui, |ui| {
            let has_entries = !list.is_empty();

            let current_index = if has_entries {
                selected_coord.and_then(|coord| list.iter().position(|entry| *entry == coord))
            } else {
                None
            };

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(has_entries, egui::Button::new("< PREV"))
                    .clicked()
                {
                    let index = match current_index {
                        Some(index) => {
                            if index == 0 {
                                list.len() - 1
                            } else {
                                index - 1
                            }
                        }

                        None => list.len() - 1,
                    };

                    clicked_coord = Some(list[index]);
                }

                if ui
                    .add_enabled(has_entries, egui::Button::new("NEXT >"))
                    .clicked()
                {
                    let index = match current_index {
                        Some(index) => (index + 1) % list.len(),
                        None => 0,
                    };

                    clicked_coord = Some(list[index]);
                }
            });
        });

    clicked_coord
}

fn draw_world_source_navigator(
    ui: &mut egui::Ui,
    name: &str,
    list: &[u64],
    selected_id: Option<u64>,
) -> Option<u64> {
    let mut clicked_id = None;
    egui::CollapsingHeader::new(format!("{} ({})", name, list.len()))
        .default_open(false)
        .show(ui, |ui| {
            let current_index =
                selected_id.and_then(|id| list.iter().position(|entry| *entry == id));
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!list.is_empty(), egui::Button::new("< PREV"))
                    .clicked()
                {
                    let index = current_index
                        .map(|index| {
                            if index == 0 {
                                list.len() - 1
                            } else {
                                index - 1
                            }
                        })
                        .unwrap_or(list.len() - 1);
                    clicked_id = Some(list[index]);
                }
                if ui
                    .add_enabled(!list.is_empty(), egui::Button::new("NEXT >"))
                    .clicked()
                {
                    let index = current_index
                        .map(|index| (index + 1) % list.len())
                        .unwrap_or(0);
                    clicked_id = Some(list[index]);
                }
            });
        });
    clicked_id
}
