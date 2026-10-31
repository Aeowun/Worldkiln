// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::Editor;
use crate::world::terrain::{TerrainMaterialLayer, TerrainState};

pub(crate) fn shore_selector(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash,
    selection: &mut String,
    layers: &[TerrainMaterialLayer],
) -> bool {
    let before = selection.clone();
    ui.horizontal(|ui| {
        ui.label("Shore Material");
        let current = layers
            .iter()
            .find(|l| l.id == *selection)
            .map(|l| l.name.as_str())
            .unwrap_or(selection);
        egui::ComboBox::from_id_salt(salt)
            .selected_text(current)
            .show_ui(ui, |ui| {
                for layer in layers {
                    ui.selectable_value(selection, layer.id.clone(), &layer.name);
                }
            });
    });
    *selection != before
}

#[cfg(test)]
mod unified_terrain_ui_tests {
    use super::*;
    #[test]
    fn unified_terrain_base_paint_undo_redo_restores_weights_without_geometry_changes() {
        let mut world = crate::world::World::new();
        let mut editor = Editor::new();
        editor.sculpt_tool = crate::world::terrain::SculptTool::Paint;
        let id = world.terrain.config.add_material_layer();
        world
            .terrain
            .generate_authored(crate::world::terrain::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [32, 40, 32],
            })
            .unwrap();
        let geometry = world.terrain.authored.clone();
        let revision = world.terrain.full_render_revision();
        let before = editor.create_brush_snapshot(&world).6;
        assert!(
            world
                .terrain
                .paint_base_and_record(&crate::world::terrain::TerrainPaintRequest {
                    center: [16.0, 16.0, 16.0],
                    radius: 3.0,
                    material: format!("layer:{id}"),
                    strength: 0.4,
                    falloff: Some(0.3)
                })
        );
        let weights = world.terrain.paint_weights.pages.clone();
        let after = before.counterpart(&world.terrain);
        before.apply(&mut world.terrain);
        assert!(world.terrain.paint_weights.pages.is_empty());
        after.apply(&mut world.terrain);
        assert_eq!(world.terrain.paint_weights.pages, weights);
        assert_eq!(world.terrain.authored, geometry);
        assert_eq!(world.terrain.full_render_revision(), revision);
    }
    fn labels(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some((t.galley.text().to_owned(), t.visual_bounding_rect()))
                } else {
                    None
                }
            })
            .collect()
    }
    #[test]
    fn unified_terrain_ui_add_layer_and_live_names_use_shared_collection() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut terrain = TerrainState::default();
        let frame = |events: Vec<egui::Event>, editor: &mut Editor, terrain: &mut TerrainState| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| draw_layers(ui, editor, terrain, None));
                },
            )
        };
        let output = frame(Vec::new(), &mut editor, &mut terrain);
        let text = labels(&output);
        let position = text
            .iter()
            .find(|(text, _)| text == "+ Add Layer")
            .unwrap()
            .1
            .center();
        let events = vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ];
        frame(events, &mut editor, &mut terrain);
        frame(
            vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            &mut editor,
            &mut terrain,
        );
        assert_eq!(terrain.config.material_layers.len(), 4);
        assert_eq!(
            terrain.config.material_layers.last().unwrap().name,
            "Layer 4"
        );
        assert!(editor.needs_save);
        let id = terrain.config.material_layers.last().unwrap().id.clone();
        terrain.config.material_layers.last_mut().unwrap().name = "Custom Shore".into();
        terrain.config.move_material_layer(&id, -3);
        let choices = terrain.config.terrain_material_layers();
        let mut selection = id.clone();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                shore_selector(ui, "test_shore", &mut selection, &choices);
            });
        });
        let text = labels(&output);
        assert!(text.iter().any(|(t, _)| t == "Shore Material"));
        assert!(text.iter().any(|(t, _)| t == "Custom Shore"));
        assert_eq!(selection, id);
        assert!(!text.iter().any(|(t, _)| t.contains("Beach Layer")));
    }
    #[test]
    fn unified_terrain_ui_missing_references_stay_visible_and_report_once() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut terrain = TerrainState::default();
        terrain.config.ensure_material_layers();
        terrain.config.material_layers[0].material = "Material:missing".into();
        terrain.config.shore_material_layer = "missing-target".into();
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| draw_layers(ui, &mut editor, &mut terrain, None));
            });
        }
        assert_eq!(
            terrain.config.material_layers[0].material,
            "Material:missing"
        );
        assert_eq!(terrain.config.shore_material_layer, "missing-target");
        assert_eq!(
            editor
                .terminal_output
                .matches("Terrain material missing:")
                .count(),
            1
        );
        assert_eq!(
            editor
                .terminal_output
                .matches("Terrain layer missing:")
                .count(),
            1
        );
    }
    #[test]
    fn unified_terrain_ui_remove_requires_existing_replacement_and_updates_selection() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut terrain = TerrainState::default();
        let id = terrain.config.add_material_layer();
        terrain.config.shore_material_layer = id.clone();
        editor.terrain_paint_layer_id = id.clone();
        let frame = |state: Option<(String, String)>,
                     events: Vec<egui::Event>,
                     editor: &mut Editor,
                     terrain: &mut TerrainState| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Some(state) = state.clone() {
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(ui.id().with("terrain_material_remove"), state)
                            });
                        }
                        draw_layers(ui, editor, terrain, None);
                    });
                },
            )
        };
        for replacement in ["missing-target", "subsurface"] {
            frame(
                Some((id.clone(), replacement.into())),
                Vec::new(),
                &mut editor,
                &mut terrain,
            );
            let output = frame(None, Vec::new(), &mut editor, &mut terrain);
            let position = labels(&output)
                .iter()
                .rev()
                .find(|(t, _)| t == "Remove")
                .unwrap()
                .1
                .center();
            frame(
                None,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Default::default(),
                    },
                ],
                &mut editor,
                &mut terrain,
            );
            frame(
                None,
                vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
                &mut editor,
                &mut terrain,
            );
            assert_eq!(
                terrain.config.material_layers.len(),
                if replacement == "subsurface" { 3 } else { 4 }
            );
        }
        assert_eq!(terrain.config.shore_material_layer, "subsurface");
        assert_eq!(editor.terrain_paint_layer_id, "subsurface");
    }
}

pub(crate) fn draw_layers(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    terrain: &mut TerrainState,
    project_path: Option<&std::path::Path>,
) {
    let before = terrain.config.material_layers.clone();
    terrain.config.migrate_material_layers();
    let mut movement = None;
    let mut removal = None;
    let count = terrain.config.material_layers.len();
    egui::ScrollArea::vertical()
        .id_salt("terrain_material_layers_scroll")
        .max_height(360.0)
        .show(ui, |ui| {
            for (index, layer) in terrain.config.material_layers.iter_mut().enumerate() {
                ui.push_id(&layer.id, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut layer.name)
                            .desired_width(ui.available_width()),
                    );
                    if super::asset_browser::draw_project_asset_drop(
                        ui,
                        project_path,
                        &mut layer.material,
                        super::asset_browser::AssetDropKind::SurfaceMaterial,
                    ) {
                        if let Some(project) = project_path {
                            let cache_id = egui::Id::new(("terrain_material_valid_ids", project));
                            ui.ctx().data_mut(|d| {
                                d.remove::<std::sync::Arc<std::collections::BTreeSet<String>>>(
                                    cache_id,
                                )
                            });
                        }
                    }
                    if !layer.material.is_empty() && ui.small_button("Clear Material").clicked() {
                        layer.material.clear();
                    }
                    ui.horizontal(|ui| {
                        ui.label("Friction");
                        ui.add(
                            egui::DragValue::new(&mut layer.friction)
                                .speed(0.05)
                                .range(0.0..=100.0),
                        );
                    });
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(index > 0, egui::Button::new("Move Up"))
                            .clicked()
                        {
                            movement = Some((layer.id.clone(), -1));
                        }
                        if ui
                            .add_enabled(index + 1 < count, egui::Button::new("Move Down"))
                            .clicked()
                        {
                            movement = Some((layer.id.clone(), 1));
                        }
                        if ui
                            .add_enabled(count > 1, egui::Button::new("Remove"))
                            .clicked()
                        {
                            removal = Some(layer.id.clone());
                        }
                    });
                });
            }
        });
    if let Some((id, delta)) = movement {
        terrain.config.move_material_layer(&id, delta);
    }
    if ui.button("+ Add Layer").clicked() {
        terrain.config.add_material_layer();
    }
    let state_id = ui.id().with("terrain_material_remove");
    if let Some(id) = removal {
        ui.ctx()
            .data_mut(|d| d.insert_temp(state_id, (id, String::new())));
    }
    if let Some((id, mut replacement)) = ui.ctx().data(|d| d.get_temp::<(String, String)>(state_id))
    {
        let mut remove = false;
        let mut cancel = false;
        egui::Window::new("Remove Layer")
            .id(state_id)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(
                    terrain
                        .config
                        .material_layers
                        .iter()
                        .find(|l| l.id == id)
                        .map(|l| l.name.as_str())
                        .unwrap_or(&id),
                );
                egui::ComboBox::from_id_salt("replacement_layer")
                    .selected_text(
                        terrain
                            .config
                            .material_layers
                            .iter()
                            .find(|l| l.id == replacement)
                            .map(|l| l.name.as_str())
                            .unwrap_or("Replacement Layer"),
                    )
                    .show_ui(ui, |ui| {
                        for layer in &terrain.config.material_layers {
                            if layer.id != id {
                                ui.selectable_value(
                                    &mut replacement,
                                    layer.id.clone(),
                                    &layer.name,
                                );
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    remove = ui
                        .add_enabled(
                            terrain
                                .config
                                .material_layers
                                .iter()
                                .any(|l| l.id == replacement && l.id != id),
                            egui::Button::new("Remove"),
                        )
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if remove {
            match terrain.remove_material_layer(&id, &replacement) {
                Ok(()) => {
                    if editor.terrain_paint_layer_id == id {
                        editor.terrain_paint_layer_id = replacement.clone();
                    }
                    editor.needs_save = true;
                }
                Err(error) => editor.append_output(&format!("{error}\n")),
            }
        }
        ui.ctx().data_mut(|d| {
            if remove || cancel {
                d.remove::<(String, String)>(state_id);
            } else {
                d.insert_temp(state_id, (id, replacement));
            }
        });
    }
    if terrain.config.material_layers != before {
        editor.needs_save = true;
    }
    let valid_ids: std::sync::Arc<std::collections::BTreeSet<String>> =
        if let Some(project) = project_path {
            let cache_id = egui::Id::new(("terrain_material_valid_ids", project));
            if let Some(found) = ui.ctx().data(|d| {
                d.get_temp::<std::sync::Arc<std::collections::BTreeSet<String>>>(cache_id)
            }) {
                found
            } else {
                let catalog = crate::project::material::MaterialCatalog::load(project);
                let ids: std::collections::BTreeSet<String> = catalog
                    .documents()
                    .values()
                    .filter_map(|document| {
                        matches!(
                            catalog.resolve(&document.id),
                            Ok(crate::project::material::MaterialOutput::SurfacePbr { .. })
                        )
                        .then(|| format!("Material:{}", document.id))
                    })
                    .collect();
                let found = std::sync::Arc::new(ids);
                ui.ctx()
                    .data_mut(|d| d.insert_temp(cache_id, found.clone()));
                found
            }
        } else {
            std::sync::Arc::new(std::collections::BTreeSet::new())
        };
    let mut missing: Vec<_> = terrain
        .config
        .material_layers
        .iter()
        .filter(|l| !l.material.is_empty() && !valid_ids.contains(&l.material))
        .map(|l| format!("Terrain material missing: {} ({})\n", l.name, l.material))
        .collect();
    let mut references = vec![terrain.config.shore_material_layer.clone()];
    references.extend(
        terrain
            .config
            .water_bodies
            .iter()
            .map(|b| b.shore_material_layer.clone()),
    );
    references.extend(
        terrain
            .config
            .native_base_paint_strokes
            .iter()
            .filter_map(|r| r.material.strip_prefix("layer:").map(str::to_owned)),
    );
    for id in references {
        if !terrain
            .config
            .material_layers
            .iter()
            .any(|l| l.id == terrain.config.resolved_layer_id(&id))
        {
            missing.push(format!("Terrain layer missing: {id}\n"));
        }
    }
    missing.sort();
    missing.dedup();
    let key = ui.id().with("terrain_missing_materials");
    if ui.ctx().data(|d| d.get_temp::<Vec<String>>(key)).as_ref() != Some(&missing) {
        for message in &missing {
            editor.append_output(message);
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, missing));
    }
}
