// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::Editor;
use crate::{
    landscape::{
        erosion_walkers::{ErosionSettings, PgrOutcome},
        image_heightmap::{ImageHeightmapMode, ImageHeightmapSettings, ImageHeightmapSource},
        pgr_precompute::{self, PgrDirectRun, PgrPrecomputeHandle, PgrPrecomputeUpdate},
    },
    world::World,
};
use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

pub struct ImageHeightmapImportState {
    settings: ImageHeightmapSettings,
    image: Option<ImageHeightmapSource>,
    path: Option<PathBuf>,
    preview: Option<egui::TextureHandle>,
    terrain_preview: Option<egui::TextureHandle>,
    terrain_preview_settings: Option<ImageHeightmapSettings>,
    error: Option<String>,
    confirm_replace: bool,
    chunk_size: u32,
    walk_randomness: u8,
    walker_count: u32,
    deposit_mode: bool,
    precompute: Option<PgrPrecomputeHandle>,
    denoise_layer_id: Option<u64>,
    calculation_status: Option<(usize, u64, f64, f32, usize)>,
    last_run_status: Option<String>,
}
impl Default for ImageHeightmapImportState {
    fn default() -> Self {
        Self {
            settings: Default::default(),
            image: None,
            path: None,
            preview: None,
            terrain_preview: None,
            terrain_preview_settings: None,
            error: None,
            confirm_replace: false,
            chunk_size: 32,
            walk_randomness: 20,
            walker_count: 64,
            deposit_mode: true,
            precompute: None,
            denoise_layer_id: None,
            calculation_status: None,
            last_run_status: None,
        }
    }
}
impl ImageHeightmapImportState {
    pub(crate) fn take_erosion_sound(&mut self) -> Option<f32> {
        None
    }
    pub(super) fn finish_for_history(&mut self, _editor: &mut Editor, _world: &mut World) {
        self.precompute = None;
        self.denoise_layer_id = None;
    }
}

fn apply_reconstruction(
    state: &mut ImageHeightmapImportState,
    editor: &mut Editor,
    world: &mut World,
    run: PgrDirectRun,
) {
    let layer_id = state.denoise_layer_id.take();
    let layer_field = layer_id.and_then(|id| {
        world
            .terrain
            .config
            .generation_layers
            .iter()
            .find(|layer| layer.id == id)
            .and_then(|layer| layer.heightmap.as_ref())
    });
    let legacy_field = world.terrain.authored.as_ref().and_then(|result| {
        if let crate::world::terrain::TerrainData::Heightfield(field) = &result.data {
            Some(field)
        } else {
            None
        }
    });
    let Some(current) = layer_field.or(legacy_field) else {
        state.error = Some("The source heightmap no longer exists; Denoise was not applied".into());
        return;
    };
    if !run.matches_source(current) {
        state.error =
            Some("Terrain changed during Denoise; result discarded. Run Denoise again.".into());
        return;
    }
    let indices = run
        .before
        .iter()
        .zip(&run.after)
        .enumerate()
        .filter_map(|(i, (before, after))| {
            (before.to_bits() != after.to_bits()).then_some(i as u32)
        })
        .collect::<Vec<_>>();
    if indices.is_empty() {
        state.last_run_status = Some("Denoise complete; terrain unchanged.".into());
        return;
    }
    if let Some(id) = layer_id {
        let snapshot = editor.create_layer_heightfield_snapshot(world, id, &indices, &run.before);
        let Some(layer) = world
            .terrain
            .config
            .generation_layers
            .iter_mut()
            .find(|layer| layer.id == id)
        else {
            return;
        };
        let before = layer.heightmap.as_ref().unwrap().heights.clone();
        layer.heightmap.as_mut().unwrap().heights = run.after;
        if let Err(error) = world.terrain.rebuild_generation_layers() {
            if let Some(layer) = world
                .terrain
                .config
                .generation_layers
                .iter_mut()
                .find(|layer| layer.id == id)
            {
                layer.heightmap.as_mut().unwrap().heights = before;
            }
            let _ = world.terrain.rebuild_generation_layers();
            state.error = Some(format!(
                "Denoise could not rebuild the imported layer: {error}"
            ));
            return;
        }
        editor.needs_save = true;
        if let Some(snapshot) = snapshot {
            editor.history.push_undo_entry(snapshot);
            editor.history.redo_stack.clear();
        }
    } else {
        let snapshot = editor.create_heightfield_snapshot(world, &indices, &run.before);
        let Some(crate::world::TerrainResult {
            data: crate::world::terrain::TerrainData::Heightfield(current),
            ..
        }) = world.terrain.authored.as_mut()
        else {
            return;
        };
        current.heights = run.after;
        world.terrain.mark_authored_heightfield_samples_dirty(
            &indices.iter().map(|&i| i as usize).collect::<Vec<_>>(),
        );
        editor.needs_save = true;
        if let Some(snapshot) = snapshot {
            editor.history.push_undo_entry(snapshot);
            editor.history.redo_stack.clear();
        }
    }
    state.error = None;
    state.last_run_status = Some(match run.result {
        PgrOutcome::Converged => "Denoise complete. Undo is available.".into(),
        PgrOutcome::Stalled(reason) => {
            format!("Denoise finished with a partial reconstruction: {reason}. Undo is available.")
        }
        PgrOutcome::Stopped => "Denoise stopped. Undo is available.".into(),
    });
}

pub(super) fn advance_live(
    state: &mut ImageHeightmapImportState,
    editor: &mut Editor,
    world: &mut World,
    ctx: &egui::Context,
) {
    if state.precompute.is_none() {
        return;
    }
    let mut finished = None;
    if let Some(handle) = state.precompute.as_ref() {
        for _ in 0..32 {
            match handle.updates.try_recv() {
                Ok(PgrPrecomputeUpdate::Running {
                    simulated_frames,
                    changed_samples,
                    residual,
                    solver_progress,
                    bytes_used,
                }) => {
                    state.calculation_status = Some((
                        simulated_frames,
                        changed_samples,
                        residual,
                        solver_progress,
                        bytes_used,
                    ))
                }
                Ok(other) => {
                    finished = Some(other);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = Some(PgrPrecomputeUpdate::Failed(
                        "PGR background computation disconnected".into(),
                    ));
                    break;
                }
            }
        }
    }
    if let Some(message) = finished {
        state.precompute = None;
        state.calculation_status = None;
        match message {
            PgrPrecomputeUpdate::DirectComplete(run) => {
                apply_reconstruction(state, editor, world, run)
            }
            PgrPrecomputeUpdate::Failed(error) => state.error = Some(error),
            PgrPrecomputeUpdate::Cancelled => {
                state.last_run_status = Some("Denoise cancelled; terrain unchanged.".into())
            }
            PgrPrecomputeUpdate::Complete(_) => {
                state.error = Some("Unexpected PGR preview result".into())
            }
            PgrPrecomputeUpdate::Running { .. } => {}
        }
        ctx.request_repaint();
    } else {
        ctx.request_repaint_after(Duration::from_millis(80));
    }
}

pub(super) fn draw(ui: &mut egui::Ui, editor: &mut Editor, world: &mut World) {
    let state = &mut editor.image_heightmap_import;
    if ui.button("Browse Image").clicked() {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Terrain Images", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            match ImageHeightmapSource::open(&path) {
                Ok(image) => {
                    let bytes = image.preview_rgba(96);
                    let preview = egui::ColorImage::from_rgba_unmultiplied([96, 96], &bytes);
                    state.preview = Some(ui.ctx().load_texture(
                        "image_heightmap_import_preview",
                        preview,
                        egui::TextureOptions::LINEAR,
                    ));
                    state.terrain_preview = None;
                    state.terrain_preview_settings = None;
                    state.settings.width = image.width.min(256).max(2);
                    state.settings.depth = image.height.min(256).max(2);
                    state.settings.mode = if image.is_grayscale() {
                        ImageHeightmapMode::Grayscale
                    } else {
                        ImageHeightmapMode::Illustrated
                    };
                    state.image = Some(image);
                    state.path = Some(path);
                    state.error = None;
                    state.confirm_replace = false;
                }
                Err(error) => {
                    state.error = Some(error);
                    state.image = None;
                    state.path = None;
                    state.preview = None;
                    state.terrain_preview = None;
                }
            }
        }
    }
    if let Some(image) = state.image.as_ref() {
        if let Some(path) = &state.path {
            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
        }
        ui.small(format!(
            "{} x {} pixels{}",
            image.width,
            image.height,
            if image.is_16_bit {
                " - 16-bit precision"
            } else {
                ""
            }
        ));
        if let Some(preview) = &state.preview {
            ui.image((preview.id(), egui::vec2(176.0, 176.0)));
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Mode");
            ui.selectable_value(
                &mut state.settings.mode,
                ImageHeightmapMode::Grayscale,
                "Grayscale Heightmap",
            );
            ui.selectable_value(
                &mut state.settings.mode,
                ImageHeightmapMode::Illustrated,
                "Illustrated Map",
            );
        });
        if state.settings.mode == ImageHeightmapMode::Illustrated {
            ui.small(
                "Approximate land, sea and relief from colors. Refine afterward with Landscaping.",
            );
        }
        ui.horizontal(|ui| {
            ui.label("Width");
            ui.add(egui::DragValue::new(&mut state.settings.width).range(2..=i32::MAX as u32));
            ui.label("Depth");
            ui.add(egui::DragValue::new(&mut state.settings.depth).range(2..=i32::MAX as u32));
        });
        ui.horizontal(|ui| {
            ui.label("Origin X");
            ui.add(egui::DragValue::new(&mut state.settings.origin_x));
            ui.label("Z");
            ui.add(egui::DragValue::new(&mut state.settings.origin_z));
        });
        ui.horizontal(|ui| {
            ui.label("Min Elevation");
            ui.add(egui::DragValue::new(&mut state.settings.min_height));
        });
        ui.horizontal(|ui| {
            ui.label("Max Elevation");
            ui.add(egui::DragValue::new(&mut state.settings.max_height));
        });
        ui.horizontal(|ui| {
            ui.label("Sea Level");
            ui.add(egui::DragValue::new(&mut state.settings.sea_level).speed(0.25));
        });
        ui.horizontal(|ui| {
            ui.label("Heightmap Y Scale");
            ui.add(
                egui::Slider::new(&mut state.settings.vertical_scale, 0.01..=4.0)
                    .logarithmic(true)
                    .suffix("x"),
            );
        });
        ui.small("Compress mountain relief without changing the landscape footprint. 1x uses original height mapping.");
    } else {
        ui.small("Choose a PNG or JPEG to preview the heightmap.");
    }
    update_terrain_preview(ui, state);
    if let Some(preview) = &state.terrain_preview {
        ui.label("Terrain Preview");
        ui.image((preview.id(), egui::vec2(176.0, 176.0)));
    }
    if let Some(image) = state.image.as_ref() {
        let mut preview_settings = state.settings.clone();
        preview_settings.width = 128;
        preview_settings.depth = 3;
        preview_settings.noise_reduction = 0;
        if let Ok(result) = image.into_terrain(&preview_settings) {
            if let crate::world::terrain::TerrainData::Heightfield(field) = result.data {
                ui.label("Side Profile (estimated, center row)");
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(248.0, 88.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
                let span = (state.settings.max_height - state.settings.min_height).max(1) as f32;
                let h_to_y = |h: f32| {
                    rect.bottom()
                        - 4.0
                        - ((h - state.settings.min_height as f32) / span).clamp(0.0, 1.0)
                            * (rect.height() - 8.0)
                };
                let sea_y = h_to_y(state.settings.sea_level);
                painter.line_segment(
                    [
                        egui::pos2(rect.left(), sea_y),
                        egui::pos2(rect.right(), sea_y),
                    ],
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(65, 135, 190)),
                );
                let mut points = Vec::with_capacity(128);
                for i in 0..128usize {
                    points.push(egui::pos2(
                        rect.left() + rect.width() * i as f32 / 127.0,
                        h_to_y(field.heights[128 + i]),
                    ));
                }
                painter.add(egui::Shape::line(
                    points,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(115, 200, 130)),
                ));
            }
        }
    }
    ui.horizontal(|ui| {
        ui.label("Heightmap Noise Reduction");
        ui.label(format!("{}%", state.settings.noise_reduction));
    });
    ui.add(egui::Slider::new(&mut state.settings.noise_reduction, 0..=100).show_value(false));
    const EXPOSE_PGR_DENOISE_UI: bool = false;
    if EXPOSE_PGR_DENOISE_UI {
        let layer_id = editor
            .terrain_selected_generation_layer
            .filter(|id| {
                world
                    .terrain
                    .config
                    .generation_layers
                    .iter()
                    .any(|layer| layer.id == *id && layer.heightmap.is_some())
            })
            .or_else(|| {
                world
                    .terrain
                    .config
                    .generation_layers
                    .iter()
                    .find(|layer| layer.heightmap.is_some())
                    .map(|layer| layer.id)
            });
        let has_legacy_heightfield = matches!(
            world.terrain.authored.as_ref(),
            Some(crate::world::TerrainResult {
                data: crate::world::terrain::TerrainData::Heightfield(_),
                ..
            })
        );
        if layer_id.is_some() || has_legacy_heightfield {
            let working = state.precompute.is_some();
            ui.horizontal(|ui| {
                if working {
                    ui.spinner();
                    ui.label("Denoising terrain...");
                    if ui.button("Cancel").clicked() {
                        state.precompute = None;
                        state.denoise_layer_id = None;
                        state.calculation_status = None;
                        state.last_run_status =
                            Some("Denoise cancelled; terrain unchanged.".into());
                    }
                } else if ui.button("Denoise").clicked() {
                    let source = layer_id
                        .and_then(|id| {
                            world
                                .terrain
                                .config
                                .generation_layers
                                .iter()
                                .find(|layer| layer.id == id)
                                .and_then(|layer| layer.heightmap.as_ref())
                        })
                        .or_else(|| {
                            world.terrain.authored.as_ref().and_then(|result| {
                                if let crate::world::terrain::TerrainData::Heightfield(field) =
                                    &result.data
                                {
                                    Some(field)
                                } else {
                                    None
                                }
                            })
                        });
                    if let Some(field) = source {
                        let settings = ErosionSettings {
                            strength: state.settings.noise_reduction,
                            chunk_size: state.chunk_size,
                            walk_randomness: state.walk_randomness,
                            walker_count: state.walker_count,
                            deposit_mode: state.deposit_mode,
                            ..Default::default()
                        };
                        state.error = None;
                        state.last_run_status = None;
                        state.calculation_status = None;
                        match pgr_precompute::start_direct(
                            field.clone(),
                            world.terrain.config.plains.water_level,
                            settings,
                        ) {
                            Ok(handle) => {
                                state.denoise_layer_id = layer_id;
                                state.precompute = Some(handle);
                            }
                            Err(error) => state.error = Some(error),
                        }
                        ui.ctx().request_repaint();
                    }
                }
            });
            ui.add_enabled_ui(!working, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Chunk Size");
                    ui.add(egui::Slider::new(&mut state.chunk_size, 8..=128));
                });
                ui.horizontal(|ui| {
                    ui.label("Walk Randomness");
                    ui.add(egui::Slider::new(&mut state.walk_randomness, 0..=100).suffix("%"));
                });
                ui.horizontal(|ui| {
                    ui.label("Walker Count");
                    ui.add(egui::Slider::new(&mut state.walker_count, 1..=512));
                });
                ui.checkbox(&mut state.deposit_mode, "Deposit Mode");
            });
            if let Some((frames, changes, residual, progress, _bytes)) = state.calculation_status {
                if working {
                    ui.add(egui::ProgressBar::new(progress).text(format!(
                        "{}% reconstructed",
                        (progress * 100.0).floor().clamp(0.0, 99.0) as u32
                    )));
                    ui.small(format!(
                        "{frames} solver batches, {changes} sample edits, {residual:.1} residual"
                    ));
                }
            }
            if let Some(status) = &state.last_run_status {
                ui.small(status);
            }
        }
    }
    if state.path.is_some() {
        if ui.button("Import as New Terrain Layer").clicked() {
            apply_import(
                state,
                world,
                &mut editor.needs_save,
                &mut editor.terrain_selected_generation_layer,
            );
        }
    }
    if let Some(error) = &state.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
}

fn update_terrain_preview(ui: &egui::Ui, state: &mut ImageHeightmapImportState) {
    let Some(image) = state.image.as_ref() else {
        return;
    };
    let mut settings = state.settings.clone();
    settings.noise_reduction = 0;
    let longest = settings.width.max(settings.depth);
    if longest > 128 {
        let scale = 128.0 / longest as f32;
        settings.width = ((settings.width as f32 * scale).round() as u32).max(2);
        settings.depth = ((settings.depth as f32 * scale).round() as u32).max(2);
    }
    if state.terrain_preview_settings.as_ref() == Some(&settings) {
        return;
    }
    state.terrain_preview_settings = Some(settings.clone());
    state.terrain_preview = None;
    let Ok(result) = image.into_terrain(&settings) else {
        return;
    };
    let crate::world::terrain::TerrainData::Heightfield(field) = result.data else {
        return;
    };
    let mut pixels = Vec::with_capacity(96 * 96 * 4);
    for z in 0..96usize {
        for x in 0..96usize {
            let fx = x as f32 * (field.size[0] - 1) as f32 / 95.0;
            let fz = z as f32 * (field.size[2] - 1) as f32 / 95.0;
            let x0 = fx.floor() as u32;
            let z0 = fz.floor() as u32;
            let x1 = (x0 + 1).min(field.size[0] - 1);
            let z1 = (z0 + 1).min(field.size[2] - 1);
            let tx = fx - x0 as f32;
            let tz = fz - z0 as f32;
            let a = field.height(x0, z0).unwrap() * (1.0 - tx) + field.height(x1, z0).unwrap() * tx;
            let b = field.height(x0, z1).unwrap() * (1.0 - tx) + field.height(x1, z1).unwrap() * tx;
            let h = a * (1.0 - tz) + b * tz;
            let rgb = if h < state.settings.sea_level {
                let d = ((state.settings.sea_level - h)
                    / (state.settings.sea_level - state.settings.min_height as f32).max(1.0))
                .clamp(0.0, 1.0);
                [18, (140.0 - 75.0 * d) as u8, (213.0 - 85.0 * d) as u8]
            } else {
                let n = ((h - state.settings.sea_level)
                    / (state.settings.max_height as f32 - state.settings.sea_level).max(1.0))
                .clamp(0.0, 1.0);
                if n > 0.65 {
                    let v = (155.0 + 90.0 * (n - 0.65) / 0.35) as u8;
                    [v, v, (v as u16 + 4).min(255) as u8]
                } else {
                    [
                        (106.0 + 58.0 * n) as u8,
                        (166.0 - 40.0 * n) as u8,
                        (86.0 - 16.0 * n) as u8,
                    ]
                }
            };
            pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([96, 96], &pixels);
    state.terrain_preview = Some(ui.ctx().load_texture(
        "heightmap_terrain_preview",
        color,
        egui::TextureOptions::LINEAR,
    ));
}
fn apply_import(
    state: &mut ImageHeightmapImportState,
    world: &mut World,
    needs_save: &mut bool,
    selected_layer: &mut Option<u64>,
) {
    let Some(source) = state.image.as_ref() else {
        return;
    };
    match source.into_terrain(&state.settings).and_then(|result| {
        world.import_heightmap_layer_with_water_level(result, state.settings.sea_level)
    }) {
        Ok(()) => {
            world.terrain.config.plains.water_level = state.settings.sea_level;
            *selected_layer = world
                .terrain
                .config
                .generation_layers
                .last()
                .map(|layer| layer.id);
            world.terrain.config.ocean_enabled = true;
            world.terrain.mark_full_render_dirty();
            *needs_save = true;
            state.error = None;
            state.confirm_replace = false;
        }
        Err(error) => {
            state.error = Some(error);
            state.confirm_replace = false;
        }
    }
}
