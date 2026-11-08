// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
pub mod asset_search;
pub mod audio_tools;
pub mod texture_tools;

pub use audio_tools::draw_audio_edit;
pub use texture_tools::draw_texture_edit;

use super::editor::{ColorTarget, Editor, EditorTool};
use crate::engine::EditorMode;
use egui::{Color32, RichText, Ui};
use glam::Vec3;
use std::collections::BTreeMap;
use std::path::PathBuf;

const BUILD_TEXTURE_BROWSER_TARGET: &str = "build_template.texture";
const BUILD_AUDIO_BROWSER_TARGET: &str = "build_template.audio";

pub(crate) fn refresh_asset_catalogs(ctx: &egui::Context, project_path: &std::path::Path) {
    crate::engine::entity_package::reload_packages(Some(project_path));
    crate::character::package::reload_packages(Some(project_path));
    let project_path = Some(project_path.to_path_buf());
    texture_tools::refresh_texture_catalog(ctx, &project_path);
    audio_tools::refresh_audio_catalog(ctx, &project_path);
}

fn draw_fx_type_drop(
    ui: &mut Ui,
    settings: &mut crate::engine::particles::EmitterSettings,
    project_path: Option<&std::path::Path>,
) {
    ui.label("FX Type");
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(72.0).min(270.0), 38.0),
        egui::Sense::hover(),
    );
    let hover = response.dnd_hover_payload::<crate::editor::editor::shell::FxContentDragPayload>();
    let builtin_hover =
        response.dnd_hover_payload::<crate::editor::editor::shell::BuiltinFxContentDragPayload>();
    let color = if hover.is_some() || builtin_hover.is_some() {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().widgets.inactive.bg_stroke.color
    };
    ui.painter().rect_filled(
        rect,
        ui.visuals().widgets.inactive.rounding,
        ui.visuals().widgets.inactive.bg_fill,
    );
    ui.painter().rect_stroke(
        rect,
        ui.visuals().widgets.inactive.rounding,
        egui::Stroke::new(1.0, color),
    );
    let name = if settings.effect_asset.is_none() {
        settings.preset.display_name()
    } else {
        settings
            .fx_asset_id()
            .and_then(|id| id.rsplit('/').next())
            .unwrap_or("")
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        name,
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    if let (Some(root), Some(payload)) = (
        project_path,
        response.dnd_release_payload::<crate::editor::editor::shell::FxContentDragPayload>(),
    ) {
        if root == payload.project_root.as_path()
            && crate::project::fx_assets::resolve(root, &payload.asset_id).is_some()
        {
            settings.effect_asset = Some(payload.asset_id.clone());
        }
    }
    if let (Some(root), Some(payload)) = (
        project_path,
        response.dnd_release_payload::<crate::editor::editor::shell::BuiltinFxContentDragPayload>(),
    ) {
        if root == payload.project_root.as_path() {
            settings.preset = payload.preset;
            settings.effect_asset = None;
        }
    }
}

pub fn draw_particle_settings(
    ui: &mut Ui,
    settings: &mut crate::engine::particles::EmitterSettings,
    project_path: Option<&std::path::Path>,
) -> bool {
    let before = settings.clone();
    ui.checkbox(&mut settings.enabled, "Emitting");
    draw_fx_type_drop(ui, settings, project_path);
    for (label, value, range) in [
        ("Particles / second", &mut settings.rate, 0.0..=1000.0),
        ("Effect scale", &mut settings.scale, 0.01..=20.0),
        ("Source radius", &mut settings.radius, 0.0..=20.0),
    ] {
        ui.horizontal(|ui| {
            ui.label(label);
            ui.add(egui::DragValue::new(value).speed(0.05).range(range));
        });
    }

    *settings != before
}

fn keep_or_choose_actor_package(current: &mut Option<String>, available: &[String]) {
    if current
        .as_ref()
        .is_some_and(|selected| available.iter().any(|package| package == selected))
    {
        return;
    }

    *current = available.first().cloned();
}

fn actor_package_family(package: &str) -> String {
    let mut parts = package
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() > 1
        && matches!(
            parts[0].to_ascii_lowercase().as_str(),
            "actor" | "character" | "entity" | "imported" | "prop"
        )
    {
        parts.remove(0);
    }
    parts
        .first()
        .map(|part| part.to_ascii_lowercase())
        .unwrap_or_else(|| "other".to_string())
}

fn actor_package_families(packages: &[String], search: &str) -> BTreeMap<String, Vec<String>> {
    let search = search.trim().to_ascii_lowercase();
    let mut families = BTreeMap::<String, Vec<String>>::new();
    for package in packages {
        let family = actor_package_family(package);
        if !search.is_empty()
            && !package.to_ascii_lowercase().contains(&search)
            && !family.contains(&search)
        {
            continue;
        }
        families.entry(family).or_default().push(package.clone());
    }
    for packages in families.values_mut() {
        packages.sort_by_key(|package| package.to_ascii_lowercase());
    }
    families
}

fn draw_actor_package_tree(
    ui: &mut Ui,
    id: &'static str,
    label: &str,
    packages: &[String],
    search: &str,
    selected: Option<&str>,
    choose: &mut Option<String>,
) {
    let families = actor_package_families(packages, search);
    let result_count = families.values().map(Vec::len).sum::<usize>();
    egui::CollapsingHeader::new(format!("{label} ({result_count})"))
        .id_salt(id)
        .default_open(true)
        .show(ui, |ui| {
            if result_count == 0 {
                ui.small("No matching assets");
                return;
            }
            for (family, packages) in families {
                egui::CollapsingHeader::new(format!("{family} ({})", packages.len()))
                    .id_salt((id, family.as_str()))
                    .default_open(!search.trim().is_empty())
                    .show(ui, |ui| {
                        for package in packages {
                            if ui
                                .selectable_label(selected == Some(package.as_str()), &package)
                                .clicked()
                            {
                                *choose = Some(package);
                            }
                        }
                    });
            }
        });
}

fn draw_actor_asset_browser(ctx: &egui::Context, editor: &mut Editor) {
    if !editor.show_actor_asset_browser {
        return;
    }

    let static_packages = crate::engine::entity_package::available_package_names();
    let character_packages = crate::character::package::available_package_names();
    let selected = editor.build_actor_package.clone();
    let mut open = editor.show_actor_asset_browser;
    let mut choose = None;

    egui::Window::new("Actor Assets")
        .open(&mut open)
        .default_size(egui::vec2(430.0, 560.0))
        .min_size(egui::vec2(340.0, 360.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                ui.add(
                    egui::TextEdit::singleline(&mut editor.actor_asset_search)
                        .desired_width(260.0)
                        .hint_text("ship, tree, character..."),
                );
                if ui.button("Clear").clicked() {
                    editor.actor_asset_search.clear();
                }
            });
            ui.small("Browse by asset type, then by filename family. Click an asset to use it for Build: Actor.");
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("actor_asset_hierarchy")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_actor_package_tree(
                        ui,
                        "static_actor_assets",
                        "Static Actors",
                        &static_packages,
                        &editor.actor_asset_search,
                        selected.as_deref(),
                        &mut choose,
                    );
                    draw_actor_package_tree(
                        ui,
                        "character_actor_assets",
                        "Characters",
                        &character_packages,
                        &editor.actor_asset_search,
                        selected.as_deref(),
                        &mut choose,
                    );
                });
        });

    if let Some(package) = choose {
        editor.build_actor_package = Some(package);
        editor.build_is_actor = true;
        open = false;
    }
    editor.show_actor_asset_browser = open;
}

fn parse_rgb_text(text: &str) -> Option<Vec3> {
    let channels = text
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<f32>().ok())
        .collect::<Option<Vec<_>>>()?;
    if channels.len() != 3
        || channels
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return None;
    }
    Some(Vec3::new(channels[0], channels[1], channels[2]))
}

pub fn draw_color_edit(ui: &mut Ui, editor: &mut Editor, target: ColorTarget, color: &mut Vec3) {
    ui.horizontal(|ui| {
        let mut text = format!("{:.3}, {:.3}, {:.3}", color.x, color.y, color.z);

        let response = ui.add(
            egui::TextEdit::singleline(&mut text)
                .desired_width(140.0)
                .hint_text("R, G, B"),
        );

        if response.changed() {
            if let Some(valid_color) = parse_rgb_text(&text) {
                *color = valid_color;
            }
        }

        response.context_menu(|ui| {
            if ui.button("Copy RGB Value").clicked() {
                ui.output_mut(|output| {
                    output.copied_text = format!("{:.3}, {:.3}, {:.3}", color.x, color.y, color.z);
                });

                ui.close_menu();
            }
        });

        ui.label("|");

        let is_open = editor.active_color_target == Some(target);

        let button = if is_open {
            egui::Button::new(RichText::new(" --- ").color(Color32::WHITE))
                .fill(Color32::from_rgb(0, 100, 200))
        } else {
            egui::Button::new(" --- ")
        };

        if ui.add(button).clicked() {
            editor.active_color_target = if is_open { None } else { Some(target) };
        }
    });
}

pub fn draw_tool_bar(ui: &mut Ui, editor: &mut Editor, project_path: &Option<PathBuf>) {
    ui.horizontal(|ui| {
        if ui
            .selectable_label(editor.show_script_workspace, "Scripts")
            .clicked()
        {
            editor.show_script_workspace = !editor.show_script_workspace;
        }

        if ui
            .selectable_label(editor.asset_manager.open, "Assets")
            .clicked()
        {
            editor.asset_manager.open = !editor.asset_manager.open;
        }

        ui.separator();

        if editor.show_script_workspace {
            ui.label(RichText::new("Script Workspace Active").italics());
            return;
        }

        ui.label(RichText::new("Mode").strong());

        if ui
            .selectable_label(editor.mode == EditorMode::Editor, "Editor")
            .clicked()
        {
            editor.mode = EditorMode::Editor;
        }

        if ui
            .selectable_label(editor.mode == EditorMode::Play, "Play")
            .on_hover_text(format!(
                "Toggle Play ({})",
                editor.theme_settings.play_bindings().0.label()
            ))
            .clicked()
        {
            editor.mode = EditorMode::Play;
        }

        ui.separator();

        ui.label(RichText::new("Tool").strong());

        draw_tool_button(ui, editor, EditorTool::Navigate, "Navigate");
        draw_tool_button(ui, editor, EditorTool::Select, "Select");
        draw_tool_button(ui, editor, EditorTool::Build, "Build");
        draw_tool_button(ui, editor, EditorTool::Erase, "Erase");

        ui.separator();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Picking:").strong());

            let text = if editor.plane_picking {
                "Plane [ ON ]"
            } else {
                "Plane [ OFF ]"
            };

            if ui.button(text).clicked() {
                editor.plane_picking = !editor.plane_picking;
            }
        });
    });
}

pub(crate) fn draw_build_context_controls(
    ui: &mut Ui,
    editor: &mut Editor,
    project_path: &Option<PathBuf>,
) {
    if editor.current_tool != EditorTool::Build {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.add_space(10.0);
        ui.strong("Build");
        let build_title = if editor.build_is_actor {
            "Build: Actor".to_string()
        } else {
            format!("Build: {:?}", editor.build_template.cell_type)
        };

        ui.menu_button(RichText::new(build_title).strong(), |ui| {
            if ui
                .selectable_label(
                    !editor.build_is_actor
                        && editor.build_template.cell_type == crate::world::CellType::Block,
                    "Cube",
                )
                .clicked()
            {
                editor.build_is_actor = false;
                editor.build_template = crate::world::Cell::new_block();
                ui.close_menu();
            }

            if ui
                .selectable_label(editor.build_is_actor, "Actor")
                .clicked()
            {
                editor.build_is_actor = true;
                let available = if let Some(path) = project_path {
                    crate::engine::entity_package::available_package_names()
                } else {
                    crate::project::available_actor_package_names()
                };
                if editor
                    .build_actor_package
                    .as_ref()
                    .is_some_and(|name| !available.contains(name))
                {
                    editor.build_actor_package = None;
                }
                ui.close_menu();
            }

            if ui
                .selectable_label(
                    !editor.build_is_actor
                        && editor.build_template.cell_type == crate::world::CellType::SpawnPoint,
                    "Spawn point",
                )
                .clicked()
            {
                editor.build_is_actor = false;
                editor.build_template = crate::world::Cell::new_spawn_point();
                ui.close_menu();
            }

            if ui
                .selectable_label(
                    !editor.build_is_actor
                        && editor.build_template.cell_type == crate::world::CellType::Light,
                    "Light",
                )
                .clicked()
            {
                editor.build_is_actor = false;
                editor.build_template = crate::world::Cell::new_light();
                ui.close_menu();
            }

            if ui
                .selectable_label(
                    !editor.build_is_actor
                        && editor.build_template.cell_type
                            == crate::world::CellType::ParticleEmitter,
                    "Emitter",
                )
                .clicked()
            {
                editor.build_is_actor = false;
                editor.build_template = crate::world::Cell::new_particle_emitter(
                    crate::engine::particles::EmitterPreset::Fire,
                );
                editor.build_template.particles =
                    crate::engine::particles::EmitterSettings::blank_fx_emitter();
                ui.close_menu();
            }

            if ui
                .selectable_label(
                    !editor.build_is_actor
                        && editor.build_template.cell_type == crate::world::CellType::AudioEmitter,
                    "Audio Emitter",
                )
                .clicked()
            {
                editor.build_is_actor = false;
                editor.build_template = crate::world::Cell::new_audio_emitter();
                ui.close_menu();
            }
        });

        if editor.build_is_actor {
            let available = if let Some(path) = project_path {
                crate::engine::entity_package::available_package_names()
            } else {
                crate::project::available_actor_package_names()
            };
            crate::editor::editor::asset_browser::draw_actor_package_drop(
                ui,
                project_path.as_deref(),
                &mut editor.build_actor_package,
                &available,
            );
            ui.label("Stud snap");
            egui::ComboBox::from_id_salt("actor_build_snap_increment")
                .selected_text(format!("{} stud", editor.actor_build_snap))
                .show_ui(ui, |ui| {
                    for increment in [0.25_f32, 0.5, 1.0, 2.0, 4.0] {
                        ui.selectable_value(
                            &mut editor.actor_build_snap,
                            increment,
                            format!("{increment}"),
                        );
                    }
                });
            ui.radio_value(&mut editor.actor_build_surface, true, "Surface");
            ui.radio_value(&mut editor.actor_build_surface, false, "Grid plane");
            egui::ComboBox::from_id_salt("actor_build_rotation_step")
                .selected_text(format!("{}° step", editor.actor_build_rotation_step))
                .show_ui(ui, |ui| {
                    for angle in [15.0_f32, 45.0, 90.0] {
                        ui.selectable_value(
                            &mut editor.actor_build_rotation_step,
                            angle,
                            format!("{angle}°"),
                        );
                    }
                });
            if ui.small_button("Rotate -").clicked() {
                editor.actor_build_yaw_degrees = (editor.actor_build_yaw_degrees
                    - editor.actor_build_rotation_step)
                    .rem_euclid(360.0);
            }
            ui.label(format!("{:.0}°", editor.actor_build_yaw_degrees));
            if ui.small_button("Rotate +").clicked() {
                editor.actor_build_yaw_degrees = (editor.actor_build_yaw_degrees
                    + editor.actor_build_rotation_step)
                    .rem_euclid(360.0);
            }
            ui.checkbox(&mut editor.actor_build_check_overlap, "Check Actor overlap");
        } else {
            use crate::world::CellType;
            if matches!(
                editor.build_template.cell_type,
                CellType::Block | CellType::FxBlock | CellType::Player | CellType::SpawnPoint
            ) {
                if crate::editor::editor::asset_browser::draw_project_asset_drop(
                    ui,
                    project_path.as_deref(),
                    &mut editor.build_template.texture,
                    crate::editor::editor::asset_browser::AssetDropKind::SurfaceMaterial,
                ) {
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new("wo_asm_002_build_material_drop"),
                            Some(editor.build_template.texture.clone()),
                        )
                    });
                }
                ui.checkbox(&mut editor.build_template.visible, "Visible");
                ui.checkbox(&mut editor.build_template.solid, "Solid");
                ui.checkbox(&mut editor.build_template.anchored, "Anchored");
                ui.menu_button("Color", |ui| {
                    let mut color = editor.build_template.color_rgb;
                    draw_color_edit(ui, editor, ColorTarget::Build, &mut color);
                    editor.build_template.color_rgb = color;
                });
            } else if editor.build_template.cell_type == CellType::AudioEmitter {
                crate::editor::editor::asset_browser::draw_project_asset_drop(
                    ui,
                    project_path.as_deref(),
                    &mut editor.build_template.audio,
                    crate::editor::editor::asset_browser::AssetDropKind::Audio,
                );
            } else if editor.build_template.cell_type == CellType::ParticleEmitter {
                ui.label("Rendering");
                ui.checkbox(&mut editor.build_template.particles.enabled, "Emitting");
                draw_fx_type_drop(
                    ui,
                    &mut editor.build_template.particles,
                    project_path.as_deref(),
                );
                for (label, value, range) in [
                    (
                        "Particles / second",
                        &mut editor.build_template.particles.rate,
                        0.0..=1000.0,
                    ),
                    (
                        "Effect scale",
                        &mut editor.build_template.particles.scale,
                        0.01..=20.0,
                    ),
                    (
                        "Source radius",
                        &mut editor.build_template.particles.radius,
                        0.0..=20.0,
                    ),
                ] {
                    ui.label(label);
                    ui.add(egui::DragValue::new(value).speed(0.05).range(range));
                }
            }
        }
        ui.checkbox(&mut editor.plane_picking, "Plane picking");
    });
}

pub(crate) fn draw_build_asset_browsers(
    ctx: &egui::Context,
    editor: &mut Editor,
    project_path: &Option<PathBuf>,
) -> Option<String> {
    let _ = (editor, project_path);
    ctx.data_mut(|data| {
        let id = egui::Id::new("wo_asm_002_build_material_drop");
        let selected = data.get_temp::<Option<String>>(id).flatten();
        data.insert_temp(id, None::<String>);
        selected
    })
}

fn draw_tool_button(ui: &mut Ui, editor: &mut Editor, tool: EditorTool, text: &str) {
    let active = editor.editor_tool_is_active(tool);

    let button = if active {
        egui::Button::new(RichText::new(text).color(Color32::WHITE))
            .fill(Color32::from_rgb(0, 100, 200))
    } else {
        egui::Button::new(text)
    };

    if ui.add(button).clicked() {
        editor.activate_editor_tool(tool);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        actor_package_families, actor_package_family, keep_or_choose_actor_package, parse_rgb_text,
    };
    use glam::Vec3;

    #[test]
    fn rgb_numeric_input_rejects_malformed_nonfinite_and_out_of_range_channels() {
        assert_eq!(
            parse_rgb_text("0.125, 0.5, 1"),
            Some(Vec3::new(0.125, 0.5, 1.0))
        );
        assert_eq!(
            parse_rgb_text("0.125 0.500 1"),
            Some(Vec3::new(0.125, 0.5, 1.0))
        );
        for invalid in [
            "NaN, 0.2, 0.3",
            "0.2, inf, 0.3",
            "0.2, -1, 0.3",
            "0.2, 1.01, 0.3",
            "0.2, invalid, 0.3",
            "0.2, 0.3",
            "0.2, 0.3, 0.4, 0.5",
            "0.2, 0.3, 0.4x",
        ] {
            assert_eq!(parse_rgb_text(invalid), None, "should reject {invalid:?}");
        }
    }

    #[test]
    fn actor_package_selection_survives_toolbar_redraws() {
        let available = vec!["default".to_string(), "sword".to_string()];
        let mut selected = Some("sword".to_string());

        keep_or_choose_actor_package(&mut selected, &available);

        assert_eq!(selected.as_deref(), Some("sword"));
    }

    #[test]
    fn missing_actor_package_falls_back_to_first_available_package() {
        let available = vec!["default".to_string(), "sword".to_string()];
        let mut selected = Some("removed".to_string());

        keep_or_choose_actor_package(&mut selected, &available);

        assert_eq!(selected.as_deref(), Some("default"));
    }

    #[test]
    fn actor_asset_browser_builds_semantic_nested_families() {
        assert_eq!(actor_package_family("ship-pirate-large"), "ship");
        assert_eq!(
            actor_package_family("actor_scarlet_lander_wreck"),
            "scarlet"
        );
        assert_eq!(actor_package_family("Rogue"), "rogue");

        let packages = vec![
            "ship-pirate-large".to_string(),
            "ship-wreck".to_string(),
            "flag-pirate".to_string(),
        ];
        let families = actor_package_families(&packages, "pirate");
        assert_eq!(
            families.keys().cloned().collect::<Vec<_>>(),
            vec!["flag", "ship"]
        );
        assert_eq!(families["ship"], vec!["ship-pirate-large"]);
    }
}
