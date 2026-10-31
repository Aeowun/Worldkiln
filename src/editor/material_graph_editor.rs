// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{SurfaceMaterialEditor, preview_cube};
use crate::project::material::{
    CustomPin, GraphLink, GraphNode, GraphOperation, MaterialDefinition, MaterialGraph, PinType,
};
use std::collections::BTreeMap;

const NODE_WIDTH: f32 = 185.0;
const HEADER: f32 = 27.0;
const PIN_ROW: f32 = 21.0;

fn initial_graph(editor: &SurfaceMaterialEditor) -> MaterialGraph {
    if editor.is_post_process {
        return MaterialGraph {
            output_node: 2,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::SceneColor,
                    position: [50.0, 80.0],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::PostProcessOutput,
                    position: [330.0, 80.0],
                },
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "color".into(),
                target_node: 2,
                target_pin: "color".into(),
            }],
        };
    }
    if let Some(water) = editor.water_look {
        return MaterialGraph {
            output_node: 4,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::Color {
                        value: [
                            water.shallow_color[0],
                            water.shallow_color[1],
                            water.shallow_color[2],
                            1.0,
                        ],
                    },
                    position: [32.0, 32.0],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::Scalar {
                        value: water.wave_height,
                    },
                    position: [32.0, 140.0],
                },
                GraphNode {
                    id: 3,
                    operation: GraphOperation::Scalar {
                        value: water.opacity,
                    },
                    position: [32.0, 236.0],
                },
                GraphNode {
                    id: 4,
                    operation: GraphOperation::WaterOutput,
                    position: [325.0, 75.0],
                },
            ],
            links: vec![
                GraphLink {
                    source_node: 1,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "color".into(),
                },
                GraphLink {
                    source_node: 2,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "wave_height".into(),
                },
                GraphLink {
                    source_node: 3,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "opacity".into(),
                },
            ],
        };
    }
    MaterialGraph {
        output_node: 4,
        nodes: vec![
            GraphNode {
                id: 1,
                operation: GraphOperation::Color {
                    value: editor.color,
                },
                position: [32.0, 32.0],
            },
            GraphNode {
                id: 2,
                operation: GraphOperation::Scalar {
                    value: editor.roughness,
                },
                position: [32.0, 140.0],
            },
            GraphNode {
                id: 3,
                operation: GraphOperation::Scalar {
                    value: editor.metallic,
                },
                position: [32.0, 236.0],
            },
            GraphNode {
                id: 4,
                operation: GraphOperation::SurfaceOutput,
                position: [325.0, 75.0],
            },
        ],
        links: vec![
            GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 4,
                target_pin: "base_color".into(),
            },
            GraphLink {
                source_node: 2,
                source_pin: "value".into(),
                target_node: 4,
                target_pin: "roughness".into(),
            },
            GraphLink {
                source_node: 3,
                source_pin: "value".into(),
                target_node: 4,
                target_pin: "metallic".into(),
            },
        ],
    }
}

fn name(operation: &GraphOperation) -> &'static str {
    match operation {
        GraphOperation::Scalar { .. } => "Scalar",
        GraphOperation::Vec2 { .. } => "Vector 2",
        GraphOperation::Vec3 { .. } => "Vector 3",
        GraphOperation::Color { .. } => "Color",
        GraphOperation::Uv => "UV",
        GraphOperation::TextureSample { .. } => "Texture Sample",
        GraphOperation::Add { .. } => "Add",
        GraphOperation::Multiply { .. } => "Multiply",
        GraphOperation::Mix { .. } => "Mix",
        GraphOperation::LayerBlend => "Layer Blend",
        GraphOperation::WaterWave => "Water Wave",
        GraphOperation::CustomFunction { .. } => "Custom Function",
        GraphOperation::SurfaceOutput => "Surface Output",
        GraphOperation::WaterOutput => "Water Output",
        GraphOperation::SkyOutput => "Sky Output",
        GraphOperation::SceneColor => "Scene Color",
        GraphOperation::SceneDepth => "Scene Depth",
        GraphOperation::SceneNormal => "Scene Normal",
        GraphOperation::ObjectMask => "Object Mask",
        GraphOperation::ScreenUv => "Screen UV",
        GraphOperation::Time => "Time",
        GraphOperation::Parameter { .. } => "Parameter",
        GraphOperation::PostProcessOutput => "Post Process Output",
    }
}

fn add(graph: &mut MaterialGraph, operation: GraphOperation) {
    if graph.nodes.len() >= 256 {
        return;
    }
    let id = graph
        .nodes
        .iter()
        .map(|node| node.id)
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    graph.nodes.push(GraphNode {
        id,
        operation,
        position: [70.0 + (id % 7) as f32 * 17.0, 90.0 + (id % 7) as f32 * 31.0],
    });
}

fn update(editor: &mut SurfaceMaterialEditor, graph: MaterialGraph) {
    match editor.draft.edit_graph(Some(graph)) {
        Ok(()) => editor.graph_error = None,
        Err(error) => editor.graph_error = Some(error),
    }
}

fn palette(
    ui: &mut egui::Ui,
    editor: &mut SurfaceMaterialEditor,
    graph: &mut MaterialGraph,
    edited: &mut bool,
) {
    let search = ui.add(
        egui::TextEdit::singleline(&mut editor.graph_search)
            .hint_text("Search nodes")
            .desired_width(ui.available_width().min(420.0)),
    );
    if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Space)) {
        search.request_focus();
    }
    let choices: &[(&str, &[(&str, fn() -> GraphOperation)])] = &[
        (
            "Inputs",
            &[
                ("Scalar", || GraphOperation::Scalar { value: 0.5 }),
                ("Vector 2", || GraphOperation::Vec2 { value: [0.0, 0.0] }),
                ("Vector 3", || GraphOperation::Vec3 {
                    value: [0.0, 0.0, 0.0],
                }),
                ("Color", || GraphOperation::Color { value: [1.0; 4] }),
                ("UV", || GraphOperation::Uv),
            ],
        ),
        (
            "Textures",
            &[("Texture Sample", || GraphOperation::TextureSample {
                slot: "BaseColor".into(),
            })],
        ),
        (
            "Math",
            &[
                ("Add", || GraphOperation::Add {
                    value_type: PinType::Scalar,
                }),
                ("Multiply", || GraphOperation::Multiply {
                    value_type: PinType::Scalar,
                }),
                ("Mix", || GraphOperation::Mix {
                    value_type: PinType::Color,
                }),
            ],
        ),
        ("Layers", &[("Layer Blend", || GraphOperation::LayerBlend)]),
        ("Water", &[("Water Wave", || GraphOperation::WaterWave)]),
        ("Outputs", &[]),
        (
            "Post Process",
            &[
                ("Scene Color", || GraphOperation::SceneColor),
                ("Scene Depth", || GraphOperation::SceneDepth),
                ("Scene Normal", || GraphOperation::SceneNormal),
                ("Object Mask", || GraphOperation::ObjectMask),
                ("Screen UV", || GraphOperation::ScreenUv),
                ("Time", || GraphOperation::Time),
                ("Parameter", || GraphOperation::Parameter {
                    name: "Intensity".into(),
                    value_type: PinType::Scalar,
                }),
            ],
        ),
        (
            "Advanced",
            &[("Custom Function", || GraphOperation::CustomFunction {
                source: "result = clamp(value, 0.0, 1.0);".into(),
                inputs: vec![CustomPin {
                    name: "value".into(),
                    value_type: PinType::Scalar,
                }],
                outputs: vec![CustomPin {
                    name: "result".into(),
                    value_type: PinType::Scalar,
                }],
            })],
        ),
    ];
    let query = editor.graph_search.trim().to_lowercase();
    let visible = choices
        .iter()
        .filter(|(category, items)| {
            (*category != "Post Process" || editor.is_post_process)
                && items
                    .iter()
                    .any(|(label, _)| query.is_empty() || label.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    let width = ui.available_width();
    let column_count = if width >= 1400.0 {
        4
    } else if width >= 950.0 {
        3
    } else if width >= 550.0 {
        2
    } else {
        1
    };
    ui.columns(column_count, |columns| {
        for (index, (category, items)) in visible.into_iter().enumerate() {
            let column = &mut columns[index % column_count];
            column.strong(*category);
            column.horizontal_wrapped(|ui| {
                for (label, factory) in *items {
                    if !query.is_empty() && !label.to_lowercase().contains(&query) {
                        continue;
                    }
                    if ui.button(*label).clicked() {
                        add(graph, factory());
                        *edited = true;
                    }
                }
            });
        }
    });
}

fn node_inspector(
    ui: &mut egui::Ui,
    graph: &mut MaterialGraph,
    selected: Option<u32>,
    edited: &mut bool,
) {
    if let Some(index) = graph
        .nodes
        .iter()
        .position(|node| Some(node.id) == selected)
    {
        let node = &mut graph.nodes[index];
        ui.strong("Node Properties");
        ui.label(name(&node.operation));
        match &mut node.operation {
            GraphOperation::Scalar { value } => {
                *edited |= ui.add(egui::DragValue::new(value).speed(0.01)).changed();
            }
            GraphOperation::Color { value } => {
                *edited |= ui.color_edit_button_rgba_unmultiplied(value).changed();
            }
            GraphOperation::Vec2 { value } => {
                for part in value.iter_mut() {
                    *edited |= ui.add(egui::DragValue::new(part).speed(0.01)).changed();
                }
            }
            GraphOperation::Vec3 { value } => {
                for part in value.iter_mut() {
                    *edited |= ui.add(egui::DragValue::new(part).speed(0.01)).changed();
                }
            }
            GraphOperation::TextureSample { slot } => {
                egui::ComboBox::from_id_salt(("graph_slot", node.id))
                    .selected_text(slot.as_str())
                    .show_ui(ui, |ui| {
                        for kind in ["BaseColor", "NormalGL", "Roughness", "AO", "Height", "ARM"] {
                            *edited |= ui.selectable_value(slot, kind.to_string(), kind).changed();
                        }
                    });
            }
            GraphOperation::Add { value_type }
            | GraphOperation::Multiply { value_type }
            | GraphOperation::Mix { value_type } => {
                egui::ComboBox::from_id_salt(("graph_kind", node.id))
                    .selected_text(format!("{value_type:?}"))
                    .show_ui(ui, |ui| {
                        for kind in [
                            PinType::Scalar,
                            PinType::Vec2,
                            PinType::Vec3,
                            PinType::Color,
                        ] {
                            *edited |= ui
                                .selectable_value(value_type, kind, format!("{kind:?}"))
                                .changed();
                        }
                    });
            }
            GraphOperation::Parameter { name, value_type } => {
                *edited |= ui.text_edit_singleline(name).changed();
                egui::ComboBox::from_id_salt(("parameter_type", node.id))
                    .selected_text(format!("{value_type:?}"))
                    .show_ui(ui, |ui| {
                        for ty in [
                            PinType::Scalar,
                            PinType::Vec2,
                            PinType::Vec3,
                            PinType::Color,
                        ] {
                            *edited |= ui
                                .selectable_value(value_type, ty, format!("{ty:?}"))
                                .changed();
                        }
                    });
            }
            GraphOperation::CustomFunction { source, .. } => {
                *edited |= ui
                    .add(egui::TextEdit::multiline(source).desired_rows(8))
                    .changed();
            }
            _ => {}
        }
    } else {
        ui.strong("Graph Details");
        ui.label(format!("{} nodes", graph.nodes.len()));
        ui.label(format!("{} connections", graph.links.len()));
    }
}

fn draw_canvas(
    ui: &mut egui::Ui,
    editor: &mut SurfaceMaterialEditor,
    graph: &mut MaterialGraph,
    edited: &mut bool,
    dragging: &mut bool,
) {
    let inspector_width = 210.0_f32.min(ui.available_width() * 0.38);
    let canvas_width = (ui.available_width() - inspector_width - 9.0).max(190.0);
    let canvas_height = ui.available_height().max(240.0);
    ui.horizontal(|ui| {
        let (canvas, _) = ui.allocate_exact_size(
            egui::vec2(canvas_width, canvas_height),
            egui::Sense::hover(),
        );
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 4.0, ui.visuals().widgets.noninteractive.bg_fill);

        let inside = ui.input(|i| i.pointer.hover_pos().is_some_and(|p| canvas.contains(p)));
        if inside && ui.input(|i| i.pointer.middle_down()) {
            editor.graph_pan += ui.input(|i| i.pointer.delta());
        }
        if inside {
            let wheel = ui.input(|i| i.raw_scroll_delta.y);
            if wheel != 0.0 {
                editor.graph_zoom =
                    (editor.graph_zoom * (1.0 + wheel.signum() * 0.08)).clamp(0.45, 2.5);
            }
        }

        let zoom = editor.graph_zoom;
        let origin = canvas.min + editor.graph_pan;
        let mut inputs = BTreeMap::<(u32, String), (egui::Pos2, PinType)>::new();
        let mut outputs = BTreeMap::<(u32, String), (egui::Pos2, PinType)>::new();
        for node in &graph.nodes {
            let Ok((input_types, output_types)) = node.operation.pins() else {
                continue;
            };
            let min = egui::pos2(
                origin.x + node.position[0] * zoom,
                origin.y + node.position[1] * zoom,
            );
            let rows = input_types.len().max(output_types.len()).max(1);
            let bounds = egui::Rect::from_min_size(
                min,
                egui::vec2(
                    NODE_WIDTH * zoom,
                    (HEADER + PIN_ROW * rows as f32 + 12.0) * zoom,
                ),
            );
            let selected = editor.graph_selected == Some(node.id);
            let border = if selected {
                ui.visuals().selection.stroke
            } else {
                ui.visuals().widgets.noninteractive.bg_stroke
            };
            painter.rect(bounds, 5.0, ui.visuals().panel_fill, border);
            painter.rect_filled(
                egui::Rect::from_min_size(min, egui::vec2(bounds.width(), HEADER * zoom)),
                5.0,
                ui.visuals().widgets.active.bg_fill,
            );
            painter.text(
                min + egui::vec2(9.0, 6.0) * zoom,
                egui::Align2::LEFT_TOP,
                name(&node.operation),
                egui::FontId::proportional(13.0 * zoom),
                ui.visuals().text_color(),
            );
            for (index, (label, kind)) in input_types.iter().enumerate() {
                let pos = egui::pos2(
                    bounds.left(),
                    bounds.top() + (HEADER + 10.0 + PIN_ROW * index as f32) * zoom,
                );
                painter.circle_filled(pos, 4.5 * zoom, pin_color(*kind));
                painter.text(
                    pos + egui::vec2(9.0 * zoom, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(10.0 * zoom),
                    ui.visuals().text_color(),
                );
                inputs.insert((node.id, label.clone()), (pos, *kind));
            }
            for (index, (label, kind)) in output_types.iter().enumerate() {
                let pos = egui::pos2(
                    bounds.right(),
                    bounds.top() + (HEADER + 10.0 + PIN_ROW * index as f32) * zoom,
                );
                painter.circle_filled(pos, 4.5 * zoom, pin_color(*kind));
                painter.text(
                    pos - egui::vec2(9.0 * zoom, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    label,
                    egui::FontId::proportional(10.0 * zoom),
                    ui.visuals().text_color(),
                );
                outputs.insert((node.id, label.clone()), (pos, *kind));
            }
        }
        for link in &graph.links {
            if let (Some((start, _)), Some((end, _))) = (
                outputs.get(&(link.source_node, link.source_pin.clone())),
                inputs.get(&(link.target_node, link.target_pin.clone())),
            ) {
                painter.line_segment(
                    [*start, *end],
                    egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                );
            }
        }
        for ((id, label), (pos, kind)) in &outputs {
            let hit = egui::Rect::from_center_size(*pos, egui::Vec2::splat(17.0 * zoom));
            if ui
                .interact(hit, ui.id().with(("out", id, label)), egui::Sense::click())
                .clicked()
            {
                editor.graph_link_from = Some((*id, label.clone(), *kind));
            }
        }
        for ((id, label), (pos, kind)) in &inputs {
            let hit = egui::Rect::from_center_size(*pos, egui::Vec2::splat(17.0 * zoom));
            if ui
                .interact(hit, ui.id().with(("in", id, label)), egui::Sense::click())
                .clicked()
            {
                if let Some((source_id, source_pin, source_kind)) = editor.graph_link_from.take() {
                    if source_kind != *kind {
                        editor.graph_error = Some("Incompatible graph pin types".into());
                    } else {
                        graph
                            .links
                            .retain(|link| !(link.target_node == *id && link.target_pin == *label));
                        graph.links.push(GraphLink {
                            source_node: source_id,
                            source_pin,
                            target_node: *id,
                            target_pin: label.clone(),
                        });
                        *edited = true;
                    }
                }
            }
        }
        for node in &mut graph.nodes {
            let p = egui::pos2(
                origin.x + node.position[0] * zoom,
                origin.y + node.position[1] * zoom,
            );
            let body = egui::Rect::from_min_size(p, egui::vec2(NODE_WIDTH * zoom, HEADER * zoom));
            let response = ui.interact(
                body,
                ui.id().with(("node", node.id)),
                egui::Sense::click_and_drag(),
            );
            if response.clicked() {
                editor.graph_selected = Some(node.id);
            }
            if response.dragged() {
                *dragging = true;
                let step = ui.input(|i| i.pointer.delta());
                node.position[0] += step.x / zoom;
                node.position[1] += step.y / zoom;
                editor.graph_selected = Some(node.id);
                *edited = true;
            }
        }

        ui.separator();
        ui.allocate_ui_with_layout(
            egui::vec2(inspector_width, canvas_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                node_inspector(ui, graph, editor.graph_selected, edited);
            },
        );
    });
}
fn pin_color(kind: PinType) -> egui::Color32 {
    match kind {
        PinType::Scalar => egui::Color32::LIGHT_GREEN,
        PinType::Vec2 => egui::Color32::LIGHT_BLUE,
        PinType::Vec3 => egui::Color32::YELLOW,
        PinType::Color => egui::Color32::LIGHT_RED,
    }
}

pub(super) fn show(
    ui: &mut egui::Ui,
    editor: &mut SurfaceMaterialEditor,
    sky: &crate::project::sky_material::SkyGradient,
    renderer: &crate::renderer::Renderer,
) {
    if let MaterialDefinition::Instance { parent, .. } = &editor.draft.document().definition {
        let parent = parent.clone();
        ui.label("This instance inherits its parent graph.");
        if ui.button("Open Parent Material").clicked() {
            editor.graph_parent_request = Some(parent);
        }
        let catalog = renderer.material_snapshot();
        if let Ok(Some(graph)) = catalog.graph(&editor.draft.document().id) {
            ui.strong("Graph Details");
            ui.label(format!(
                "{} nodes · {} connections",
                graph.nodes.len(),
                graph.links.len()
            ));
        }
        return;
    }
    let graph_for_preview = editor.draft.document().graph.as_ref();
    if editor.is_post_process {
        ui.strong("Post Process");
    } else if let Some(water) = editor.water_look {
        ui.strong("WATER PREVIEW");
        let look = match graph_for_preview {
            None => water,
            Some(graph) => match crate::project::material::evaluate_graph_water_for_preview(
                graph,
                water,
                &editor.texture_maps,
            ) {
                Ok(evaluated) => evaluated,
                Err(error) => {
                    editor.graph_error = Some(format!(
                        "This output is not supported by the current renderer. {error}"
                    ));
                    water
                }
            },
        };
        let width = ui.available_width().max(200.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 160.0), egui::Sense::hover());
        match renderer.water_material_preview(rect, look) {
            Ok(preview) => {
                ui.painter().add(preview);
            }
            Err(error) => {
                editor.graph_error = Some(error);
            }
        }
    } else {
        ui.strong(if editor.landscape_assigned {
            "LANDSCAPE PREVIEW"
        } else {
            "MATERIAL PREVIEW"
        });
        if let Some(error) = preview_cube(ui, editor, sky, renderer, 0.3, graph_for_preview) {
            editor.graph_error = Some(error);
        }
    }
    if editor.draft.document().graph.is_none() {
        ui.label("This material has no graph.");
        ui.label("Creating a graph preserves this material's current values.");
        if ui.button("Create Graph").clicked() {
            let initial = initial_graph(editor);
            update(editor, initial);
        }
        return;
    }
    let mut graph = editor
        .draft
        .document()
        .graph
        .clone()
        .expect("graph checked");
    let mut changed = false;
    palette(ui, editor, &mut graph, &mut changed);
    let mut dragging = false;
    draw_canvas(ui, editor, &mut graph, &mut changed, &mut dragging);
    if let Some(selected) = editor.graph_selected {
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) && selected != graph.output_node {
            graph.nodes.retain(|node| node.id != selected);
            graph
                .links
                .retain(|link| link.source_node != selected && link.target_node != selected);
            editor.graph_selected = None;
            changed = true;
        }
    }
    if changed {
        if dragging {
            match editor.draft.edit_graph_transient(graph) {
                Ok(()) => editor.graph_error = None,
                Err(error) => editor.graph_error = Some(error),
            }
        } else {
            update(editor, graph);
        }
    }
    if !dragging {
        editor.draft.finish_graph_drag();
    }
    ui.separator();
    if let Some(error) = &editor.graph_error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("Material shader error: {error}"),
        );
    } else {
        ui.small("No graph errors");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::material::MaterialOutput;
    #[test]
    fn initial_water_graph_saves_and_drives_existing_water_material() {
        use crate::project::material::{self, MaterialDefinition, MaterialDocument};
        let root = std::env::temp_dir().join(format!(
            "wkmat012-graph-water-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let parent = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(Default::default()),
        });
        material::save(&root, "Water.MAT", &parent, None).unwrap();
        let mut editor = SurfaceMaterialEditor::open(&root, "Water.MAT").unwrap();
        let graph = initial_graph(&editor);
        graph
            .validate(
                &MaterialOutput::Water(editor.water_look.unwrap()),
                &BTreeMap::new(),
            )
            .unwrap();
        assert!(matches!(
            graph
                .nodes
                .iter()
                .find(|n| n.id == graph.output_node)
                .unwrap()
                .operation,
            GraphOperation::WaterOutput
        ));
        editor.draft.edit_graph(Some(graph)).unwrap();
        assert!(editor.dirty());
        let catalog = editor.save().unwrap();
        assert!(catalog.graph(&parent.id).unwrap().is_some());
        assert!(catalog.water(&parent.id).is_some());
        assert!(material::read(&root, "Water.MAT").unwrap().graph.is_some());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn initial_surface_graph_retains_creator_values() {
        let temp =
            std::env::temp_dir().join(format!("material-graph-ui-{:032x}", rand::random::<u128>()));
        let editor = SurfaceMaterialEditor::create(&temp).unwrap();
        let graph = initial_graph(&editor);
        graph
            .validate(
                &MaterialOutput::SurfacePbr {
                    base_color: editor.color,
                    roughness: editor.roughness,
                    metallic: editor.metallic,
                },
                &BTreeMap::new(),
            )
            .unwrap();
        std::fs::remove_dir_all(temp).unwrap();
    }
}
