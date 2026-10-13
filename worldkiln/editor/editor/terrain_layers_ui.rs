// Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:50 -05:00
// Generic non-destructive layer stack for the Terrain panel only.
// No Navigation panel, Navigation state, or other editor panel modifications.
use super::Editor;
use crate::world::World;
use crate::world::terrain::{
    GenerationLayer, GeneratorSettings, Landform, TerrainGenerationRequest,
};

fn label_for(index: usize, name: Option<&str>) -> String {
    match name.map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => value.to_owned(),
        None => format!("#{}", index + 1), // Ordinal only; not an assigned or persisted name.
    }
}

pub(super) fn draw_generation_layers_ui(ui: &mut egui::Ui, editor: &mut Editor, world: &mut World) {
    ui.add_space(8.0);
    ui.heading("GENERATION LAYERS");
    ui.small("Each layer generates independently. Select a layer to edit it; the original Terrain remains intact.");
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    ui.small("Sculpt or Paint the selected generated layer under LANDSCAPING. Locked or hidden layers cannot be edited.");
    let mut list_action: Option<(u64, &'static str)> = None;
    let layers = &world.terrain.config.generation_layers;
    if layers.is_empty() {
        ui.weak("No generation layers. Add one to begin.");
    } else {
        egui::ScrollArea::vertical()
            .id_salt("terrain_generation_layers_list")
            .max_height(180.0)
            .show(ui, |ui| {
                for (index, layer) in layers.iter().enumerate() {
                    let selected = editor.terrain_selected_generation_layer == Some(layer.id);
                    let title = label_for(index, layer.name.as_deref());
                    ui.horizontal_wrapped(|ui| {
                        if ui.selectable_label(selected, title).clicked() {
                            editor.terrain_selected_generation_layer = Some(layer.id);
                        }
                        if ui
                            .small_button(if layer.visible { "◉" } else { "○" })
                            .on_hover_text("Show or hide generated layer")
                            .clicked()
                        {
                            list_action = Some((layer.id, "visibility"));
                        }
                        if ui
                            .small_button(if layer.locked { "🔒" } else { "♧" })
                            .on_hover_text("Lock or unlock layer generation and removal")
                            .clicked()
                        {
                            list_action = Some((layer.id, "lock"));
                        }
                        if ui.small_button("↑").on_hover_text("Move up").clicked() {
                            list_action = Some((layer.id, "up"));
                        }
                        if ui.small_button("↓").on_hover_text("Move down").clicked() {
                            list_action = Some((layer.id, "down"));
                        }
                    });
                }
            });
    }
    if let Some((id, action)) = list_action {
        let index = world
            .terrain
            .config
            .generation_layers
            .iter()
            .position(|l| l.id == id);
        if let Some(index) = index {
            match action {
                "visibility" => {
                    let visible = !world.terrain.config.generation_layers[index].visible;
                    match world.terrain.set_generation_layer_visible(id, visible) {
                        Ok(()) => {
                            editor.needs_save = true;
                            editor.terrain_generation_error = None;
                        }
                        Err(error) => editor.terrain_generation_error = Some(error),
                    }
                }
                "lock" => {
                    let l = &mut world.terrain.config.generation_layers[index];
                    l.locked = !l.locked;
                    editor.needs_save = true;
                }
                "up" | "down" => {
                    let next = if action == "up" {
                        index.checked_sub(1)
                    } else {
                        (index + 1 < world.terrain.config.generation_layers.len())
                            .then_some(index + 1)
                    };
                    if let Some(next) = next {
                        world.terrain.config.generation_layers.swap(index, next);
                        match world.terrain.rebuild_generation_layers() {
                            Ok(()) => {
                                editor.needs_save = true;
                                editor.terrain_generation_error = None;
                            }
                            Err(error) => {
                                world.terrain.config.generation_layers.swap(index, next);
                                editor.terrain_generation_error = Some(error);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    ui.horizontal_wrapped(|ui| {
        if ui.button("+ Add Layer").clicked() {
            let request = TerrainGenerationRequest {
                position: editor.terrain_generation_position,
                size: editor.terrain_generation_size,
            };
            match world.terrain.add_generation_layer(request) {
                Ok(id) => {
                    editor.terrain_selected_generation_layer = Some(id);
                    editor.terrain_generation_error = None;
                    editor.needs_save = true;
                }
                Err(error) => editor.terrain_generation_error = Some(error),
            }
        }
        if let Some(id) = editor.terrain_selected_generation_layer {
            let selected = world
                .terrain
                .config
                .generation_layers
                .iter()
                .position(|l| l.id == id);
            if let Some(index) = selected {
                if ui
                    .button("Duplicate")
                    .on_hover_text("Copy selected settings into a new unnamed layer")
                    .clicked()
                {
                    if let Some(next) = world
                        .terrain
                        .config
                        .generation_layers
                        .iter()
                        .map(|l| l.id)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                    {
                        let mut duplicate = world.terrain.config.generation_layers[index].clone();
                        duplicate.id = next;
                        duplicate.name = None;
                        duplicate.locked = false;
                        world.terrain.config.generation_layers.push(duplicate);
                        editor.terrain_selected_generation_layer = Some(next);
                        if let Err(error) = world.terrain.rebuild_generation_layers() {
                            world.terrain.config.generation_layers.pop();
                            editor.terrain_generation_error = Some(error);
                            editor.terrain_selected_generation_layer = Some(id);
                        } else {
                            editor.needs_save = true;
                        }
                    }
                }
                if ui.button("Delete").clicked() {
                    editor.terrain_pending_delete_layer = Some(id);
                }
            }
        }
    });

    if let Some(id) = editor.terrain_pending_delete_layer {
        ui.colored_label(
            egui::Color32::from_rgb(244, 196, 96),
            "Delete this generated layer? Its settings and generated contribution will be lost.",
        );
        ui.horizontal(|ui| {
            if ui.button("Confirm Delete").clicked() {
                match world.terrain.delete_generation_layer(id) {
                    Ok(()) => {
                        editor.terrain_pending_delete_layer = None;
                        editor.terrain_selected_generation_layer = None;
                        editor.terrain_generation_error = None;
                        editor.needs_save = true;
                    }
                    Err(error) => editor.terrain_generation_error = Some(error),
                }
            }
            if ui.button("Cancel").clicked() {
                editor.terrain_pending_delete_layer = None;
            }
        });
    }

    let Some(selected_id) = editor.terrain_selected_generation_layer else {
        ui.small("Choose a layer to see its generator controls.");
        return;
    };
    let Some(index) = world
        .terrain
        .config
        .generation_layers
        .iter()
        .position(|l| l.id == selected_id)
    else {
        editor.terrain_selected_generation_layer = None;
        return;
    };
    ui.separator();
    let layer = &mut world.terrain.config.generation_layers[index];
    let mut new_name = layer.name.clone().unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label("Name (optional)");
        if ui.text_edit_singleline(&mut new_name).changed() {
            layer.name = if new_name.trim().is_empty() {
                None
            } else {
                Some(new_name.trim().to_owned())
            };
            editor.needs_save = true;
        }
    });
    // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:16 -05:00
    // Expose persistent material/sculpt overrides where the user expects
    // to find their layer state, rather than making edits look transient.
    ui.small(format!(
        "{} saved Sculpt/Paint edits (replayed when this layer generates)",
        layer.brush_edits.len()
    ));
    if layer.locked {
        ui.label("Layer locked: unlock it to change generator settings.");
        return;
    }
    let before = layer.settings.clone();
    let generator_settings = &mut layer.settings;
    egui::ComboBox::from_label("Generator")
        .selected_text(match generator_settings.kind {
            Landform::Plains => "Plains",
            Landform::Hills => "Hills",
            Landform::Mountains => "Mountains",
            Landform::Islands => "Islands",
            Landform::Archipelago => "Archipelago",
        })
        .show_ui(ui, |ui| {
            for (kind, label) in [
                (Landform::Plains, "Plains"),
                (Landform::Hills, "Hills"),
                (Landform::Mountains, "Mountains"),
                (Landform::Islands, "Islands"),
                (Landform::Archipelago, "Archipelago"),
            ] {
                ui.selectable_value(&mut generator_settings.kind, kind, label);
            }
        });
    ui.label("Region (world coordinates)");
    ui.horizontal_wrapped(|ui| {
        ui.label("Origin");
        for v in &mut generator_settings.request.position {
            ui.add(egui::DragValue::new(v).speed(1));
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Size");
        for v in &mut generator_settings.request.size {
            ui.add(egui::DragValue::new(v).speed(1).range(1..=8192));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Seed");
        ui.add(egui::DragValue::new(&mut generator_settings.seed).speed(1));
    });
    let params = [
        ("Base height", &mut generator_settings.base_height, 0.2_f64),
        ("Relief", &mut generator_settings.relief, 0.2),
        ("Noise scale", &mut generator_settings.frequency, 0.0005),
        ("Water level", &mut generator_settings.water_level, 0.2),
        ("Feature radius", &mut generator_settings.radius, 0.5),
        ("Beach width", &mut generator_settings.beach_width, 0.5),
        ("Coast variation", &mut generator_settings.coast_noise, 0.01),
    ];
    for (name, value, speed) in params {
        ui.horizontal(|ui| {
            ui.label(name);
            ui.add(egui::DragValue::new(value).speed(speed));
        });
    }
    ui.horizontal(|ui| {
        ui.label("Peaks / islands");
        ui.add(egui::DragValue::new(&mut generator_settings.count).range(1..=24));
    });
    if *generator_settings != before {
        editor.needs_save = true;
    }
    if layer.applied.as_ref() != Some(&layer.settings) {
        ui.weak("Settings changed. Generate to apply this layer.");
    }
    ui.small("Water shading and materials are shared by the current Terrain. This layer controls shape and shoreline.");
    if ui.button("Generate Selected Layer").clicked() {
        match world.terrain.generate_selected_layer(selected_id) {
            Ok(()) => {
                editor.terrain_generation_error = None;
                editor.needs_save = true;
            }
            Err(error) => editor.terrain_generation_error = Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_display_name_does_not_save_a_name() {
        let layer = GenerationLayer::new(
            2,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 8, 8],
            },
        );
        assert_eq!(label_for(0, layer.name.as_deref()), "#1");
        assert!(layer.name.is_none());
        assert_eq!(label_for(4, Some("My Terrain")), "My Terrain");
    }
}
