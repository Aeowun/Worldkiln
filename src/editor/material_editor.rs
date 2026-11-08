// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::project::material::MaterialOutput;
use crate::world::terrain::TerrainWaterSettings;

pub(crate) fn draw_water_parameters(ui: &mut egui::Ui, look: &mut TerrainWaterSettings) {
    ui.collapsing("Water Appearance", |ui| {
        ui.horizontal(|ui| {
            ui.label("Shallow Water Color");
            ui.color_edit_button_rgb(&mut look.shallow_color);
        });
        ui.horizontal(|ui| {
            ui.label("Deep Water Color");
            ui.color_edit_button_rgb(&mut look.deep_color);
        });
        ui.horizontal(|ui| {
            ui.label("Foam Color");
            ui.color_edit_button_rgb(&mut look.foam_color);
        });
        ui.horizontal(|ui| {
            ui.label("Underwater Color");
            ui.color_edit_button_rgb(&mut look.underwater_color);
        });
        ui.horizontal(|ui| {
            ui.label("Absorption Color");
            ui.color_edit_button_rgb(&mut look.absorption_color);
        });
        ui.horizontal(|ui| {
            ui.label("Scattering Color");
            ui.color_edit_button_rgb(&mut look.scattering_color);
        });
        for (label, value, range) in [
            ("Wave Height", &mut look.wave_height, 0.0..=10.0),
            ("Wave Speed", &mut look.wave_speed, 0.0..=3.0),
            ("Wave Length", &mut look.wave_length, 1.0..=500.0),
            (
                "Wave Direction",
                &mut look.wave_direction_degrees,
                -360.0..=360.0,
            ),
            ("Wave Steepness", &mut look.wave_steepness, 0.0..=1.0),
            ("Absorption", &mut look.absorption_strength, 0.0..=5.0),
            ("Scattering", &mut look.scattering_strength, 0.0..=5.0),
            ("Foam Amount", &mut look.foam_amount, 0.0..=2.0),
            ("Foam Scale", &mut look.foam_scale, 0.25..=4.0),
            ("Opacity", &mut look.opacity, 0.0..=1.0),
            (
                "Underwater Opacity",
                &mut look.underwater_opacity,
                0.0..=1.0,
            ),
            ("Reflection", &mut look.reflection_strength, 0.0..=2.0),
            ("Sun Highlight", &mut look.specular_strength, 0.0..=2.0),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(egui::Slider::new(value, range));
            });
        }
    });
    ui.collapsing("Underwater Post Processing", |ui| {
        ui.checkbox(
            &mut look.underwater_effects_enabled,
            "Enable underwater effects",
        );
        ui.horizontal(|ui| {
            ui.label("Fog Tint");
            ui.color_edit_button_rgb(&mut look.underwater_fog_color);
        });
        for (label, value, range) in [
            ("Fog Density", &mut look.underwater_fog_density, 0.0..=0.5),
            ("Depth Fog", &mut look.underwater_depth_fog, 0.0..=0.1),
            (
                "Depth Limit",
                &mut look.underwater_depth_fog_limit,
                0.0..=1.0,
            ),
            ("Fog Limit", &mut look.underwater_fog_limit, 0.0..=1.0),
            ("Sky Tint", &mut look.underwater_sky_tint, 0.0..=1.0),
        ] {
            ui.horizontal(|ui| {
                ui.label(label);
                ui.add(egui::Slider::new(value, range));
            });
        }
    });
}

use crate::project::sky_material::{SkyGradient, SkyInterpolation, SkyStop};

#[derive(Clone, PartialEq)]
struct SkyEditSnapshot {
    gradient: SkyGradient,
    selected: usize,
    document: Option<crate::project::material::MaterialDocument>,
}
#[derive(Clone)]
pub(crate) struct SkyMaterialPreview {
    pub project: Option<std::path::PathBuf>,
    pub name: String,
    gradient: SkyGradient,
    selected: usize,
    material_draft: Option<crate::project::material_draft::MaterialDraft>,
    texture_choices: Vec<String>,
    overlay_preview: std::cell::RefCell<Option<(u64, egui::TextureHandle)>>,
    pub status: Option<(String, bool)>,
    undo_history: Vec<SkyEditSnapshot>,
    redo_history: Vec<SkyEditSnapshot>,
    editing_drag: bool,
    history_current: Option<SkyEditSnapshot>,
}
impl std::fmt::Debug for SkyMaterialPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkyMaterialPreview")
            .field("name", &self.name)
            .finish()
    }
}
impl Default for SkyMaterialPreview {
    fn default() -> Self {
        Self {
            project: None,
            name: "New Sky.MAT".into(),
            gradient: SkyGradient::default(),
            selected: 2,
            material_draft: None,
            texture_choices: Vec::new(),
            overlay_preview: std::cell::RefCell::new(None),
            status: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            editing_drag: false,
            history_current: None,
        }
    }
}
impl SkyMaterialPreview {
    pub(crate) fn for_project(project: &std::path::Path) -> Result<Self, String> {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        for suffix in 0..10000 {
            let name = if suffix == 0 {
                "New Sky.MAT".to_owned()
            } else {
                format!("New Sky_{suffix}.MAT")
            };
            if crate::project::material::file_path(project, &name)?.exists() {
                continue;
            }
            let definition = MaterialDefinition::Asset {
                material: MaterialOutput::Sky(SkyGradient::default()),
            };
            let mut draft =
                crate::project::material_draft::MaterialDraft::create(project, &name, definition)?;
            draft.save(project)?;
            return Self::open(project, &name);
        }
        Err("No available Sky Material filename".into())
    }
    pub(crate) fn open(project: &std::path::Path, name: &str) -> Result<Self, String> {
        let draft = crate::project::material_draft::MaterialDraft::open(project, name)?;
        Self::from_draft(project, name, draft)
    }
    pub(crate) fn open_with_snapshot(
        project: &std::path::Path,
        name: &str,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> Result<Self, String> {
        let draft = crate::project::material_draft::MaterialDraft::open_with_snapshot(
            project, name, catalog,
        )?;
        Self::from_draft(project, name, draft)
    }
    fn from_draft(
        project: &std::path::Path,
        name: &str,
        draft: crate::project::material_draft::MaterialDraft,
    ) -> Result<Self, String> {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        let MaterialDefinition::Asset {
            material: MaterialOutput::Sky(gradient),
        } = &draft.document().definition
        else {
            return Err("Only Sky Materials can be opened in the Sky Material Editor".into());
        };
        Ok(Self {
            project: Some(project.to_owned()),
            name: name.to_owned(),
            gradient: gradient.clone(),
            selected: 0,
            material_draft: Some(draft),
            texture_choices: Vec::new(),
            overlay_preview: std::cell::RefCell::new(None),
            status: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            editing_drag: false,
            history_current: None,
        })
    }
    fn snapshot(&self) -> SkyEditSnapshot {
        SkyEditSnapshot {
            gradient: self.gradient.clone(),
            selected: self.selected,
            document: self
                .material_draft
                .as_ref()
                .map(|draft| draft.document().clone()),
        }
    }
    fn begin_history_frame(&mut self) {
        if self.history_current.is_none() {
            self.history_current = Some(self.snapshot());
        }
    }
    fn track_edits(&mut self, pointer_down: bool) {
        let Some(previous) = self.history_current.take() else {
            return;
        };
        let changed = self.gradient != previous.gradient
            || self.selected != previous.selected
            || self.material_draft.as_ref().map(|d| d.document()) != previous.document.as_ref();
        if changed {
            if !self.editing_drag {
                self.undo_history.push(previous);
                if self.undo_history.len() > 64 {
                    self.undo_history.remove(0);
                }
                self.redo_history.clear();
            }
            self.history_current = Some(self.snapshot());
            self.editing_drag = pointer_down;
        } else {
            self.history_current = Some(previous);
            if !pointer_down {
                self.editing_drag = false;
            }
        }
    }
    fn navigate_history(&mut self, redo: bool) -> bool {
        self.editing_drag = false;
        let entry = if redo {
            self.redo_history.pop()
        } else {
            self.undo_history.pop()
        };
        let Some(snapshot) = entry else { return false };
        let current = self.snapshot();
        if let (Some(draft), Some(document)) =
            (self.material_draft.as_mut(), snapshot.document.as_ref())
        {
            if let Err(error) = draft.restore_editor_snapshot(document.clone()) {
                self.status = Some((error, false));
                return false;
            }
        }
        if redo {
            self.undo_history.push(current);
        } else {
            self.redo_history.push(current);
        }
        self.gradient = snapshot.gradient;
        self.selected = snapshot.selected;
        self.overlay_preview.borrow_mut().take();
        self.history_current = Some(self.snapshot());
        self.status = None;
        true
    }
    pub(crate) fn accept_background_save(
        &mut self,
        request: &crate::project::material_draft::MaterialSaveRequest,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> Result<(), String> {
        self.material_draft
            .as_mut()
            .ok_or("Sky Material draft is unavailable")?
            .accept_saved(request, catalog)?;
        Ok(())
    }
    pub(crate) fn material_id(&self) -> Option<&str> {
        self.material_draft
            .as_ref()
            .map(|draft| draft.document().id.as_str())
    }
    pub(crate) fn is_dirty(&self) -> bool {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        self.material_draft.as_ref().is_none_or(|draft| {
            draft.is_dirty()
                || draft.document().definition
                    != MaterialDefinition::Asset {
                        material: MaterialOutput::Sky(self.gradient.clone()),
                    }
        })
    }
    pub(crate) fn prepare_save_request(
        &mut self,
    ) -> Result<crate::project::material_draft::MaterialSaveRequest, String> {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        self.gradient.validate()?;
        let draft = self
            .material_draft
            .as_mut()
            .ok_or("Sky Material draft is unavailable")?;
        draft.edit(MaterialDefinition::Asset {
            material: MaterialOutput::Sky(self.gradient.clone()),
        })?;
        Ok(draft.save_request())
    }
    pub(crate) fn save(
        &mut self,
        project: &std::path::Path,
    ) -> Result<crate::project::material::MaterialCatalog, String> {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        self.gradient.validate()?;
        let draft = self
            .material_draft
            .as_mut()
            .ok_or("Sky Material draft is unavailable")?;
        draft.edit(MaterialDefinition::Asset {
            material: MaterialOutput::Sky(self.gradient.clone()),
        })?;
        draft.save(project)
    }
    fn sample(&self, h: f32) -> [f32; 3] {
        self.gradient.sample(h)
    }
    fn add_stop(&mut self) {
        if self.gradient.stops.len() >= 16 {
            return;
        }
        let mut widest = 0;
        for i in 0..self.gradient.stops.len() - 1 {
            if self.gradient.stops[i + 1].position - self.gradient.stops[i].position
                > self.gradient.stops[widest + 1].position - self.gradient.stops[widest].position
            {
                widest = i;
            }
        }
        let p =
            (self.gradient.stops[widest].position + self.gradient.stops[widest + 1].position) * 0.5;
        let color = self.sample(p);
        self.selected = widest + 1;
        self.gradient
            .stops
            .insert(self.selected, SkyStop { position: p, color });
    }
    fn set_stop_position(&mut self, index: usize, p: f32) {
        if index == 0 || index + 1 >= self.gradient.stops.len() {
            return;
        }
        let min = if index == 0 {
            0.0
        } else {
            self.gradient.stops[index - 1].position + 0.001
        };
        let max = if index + 1 == self.gradient.stops.len() {
            1.0
        } else {
            self.gradient.stops[index + 1].position - 0.001
        };
        self.gradient.stops[index].position = p.clamp(min, max);
    }
}

fn sky_color(rgb: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(
        (rgb[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgb[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}
type SkyLayerTask = (
    u64,
    Vec<(std::path::PathBuf, crate::project::material::MaterialLayer)>,
);
struct SkyLayerWorker {
    send: std::sync::mpsc::Sender<SkyLayerTask>,
    receive: std::sync::mpsc::Receiver<(u64, Option<Vec<u8>>)>,
    pending: std::collections::HashSet<u64>,
    ready: std::collections::HashMap<u64, Option<Vec<u8>>>,
    order: std::collections::VecDeque<u64>,
}
impl SkyLayerWorker {
    fn new(ctx: egui::Context) -> Self {
        let (send, input) = std::sync::mpsc::channel::<SkyLayerTask>();
        let (output, receive) = std::sync::mpsc::channel::<(u64, Option<Vec<u8>>)>();
        std::thread::Builder::new()
            .name("worldkiln-sky-layer-preview".into())
            .spawn(move || {
                while let Ok((key, layers)) = input.recv() {
                    let pixels = compose_sky_layers(&layers);
                    if output.send((key, pixels)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("sky layer preview decoder");
        Self {
            send,
            receive,
            pending: Default::default(),
            ready: Default::default(),
            order: Default::default(),
        }
    }
}

fn compose_sky_layers(
    layers: &[(std::path::PathBuf, crate::project::material::MaterialLayer)],
) -> Option<Vec<u8>> {
    use crate::project::material::MaterialLayerBlend;
    const W: usize = 256;
    const H: usize = 128;
    let mut pixels = vec![[0.0f32; 4]; W * H];
    for (path, layer) in layers {
        let source = image::open(path)
            .ok()?
            .resize_exact(
                W as u32,
                (H * 2) as u32,
                image::imageops::FilterType::Triangle,
            )
            .to_rgba8();
        for y in 0..H {
            for x in 0..W {
                let src = source.get_pixel(x as u32, y as u32).0;
                let dest = &mut pixels[y * W + x];
                let alpha = layer.opacity * src[3] as f32 / 255.0;
                let out = alpha + dest[3] * (1.0 - alpha);
                let weight = if out > 0.000001 { alpha / out } else { 0.0 };
                for c in 0..3 {
                    let incoming = src[c] as f32 / 255.0;
                    let mixed = match layer.blend {
                        MaterialLayerBlend::Normal => incoming,
                        MaterialLayerBlend::Multiply => dest[c] * incoming,
                        MaterialLayerBlend::Add => (dest[c] + incoming).min(1.0),
                        MaterialLayerBlend::Screen => 1.0 - (1.0 - dest[c]) * (1.0 - incoming),
                    };
                    dest[c] = dest[c] * (1.0 - weight) + mixed * weight;
                }
                dest[3] = out;
            }
        }
    }
    Some(
        pixels
            .into_iter()
            .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
            .collect(),
    )
}

fn sky_layer_preview(ui: &mut egui::Ui, draft: &SkyMaterialPreview, rect: egui::Rect) {
    use std::hash::{Hash, Hasher};
    let Some(project) = draft.project.as_deref() else {
        return;
    };
    let Some(document) = draft.material_draft.as_ref().map(|d| d.document()) else {
        return;
    };
    if document.layers.is_empty() {
        draft.overlay_preview.borrow_mut().take();
        return;
    }
    let Ok(encoded) = serde_json::to_vec(&document.layers) else {
        return;
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project.hash(&mut hasher);
    encoded.hash(&mut hasher);
    let key = hasher.finish();

    let cached = draft
        .overlay_preview
        .borrow()
        .as_ref()
        .is_some_and(|(hash, _)| *hash == key);
    if !cached {
        static WORKER: std::sync::OnceLock<std::sync::Mutex<SkyLayerWorker>> =
            std::sync::OnceLock::new();
        let worker =
            WORKER.get_or_init(|| std::sync::Mutex::new(SkyLayerWorker::new(ui.ctx().clone())));
        let mut worker = worker.lock().unwrap_or_else(|err| err.into_inner());
        for _ in 0..4 {
            let Ok((completed, pixels)) = worker.receive.try_recv() else {
                break;
            };
            worker.pending.remove(&completed);
            if worker.order.len() >= 8 {
                if let Some(old) = worker.order.pop_front() {
                    worker.ready.remove(&old);
                }
            }
            worker.order.push_back(completed);
            worker.ready.insert(completed, pixels);
        }
        if let Some(image) = worker.ready.get(&key) {
            if let Some(rgba) = image {
                let handle = ui.ctx().load_texture(
                    format!("sky_stars_{}", document.id),
                    egui::ColorImage::from_rgba_unmultiplied([256, 128], rgba),
                    egui::TextureOptions::LINEAR,
                );
                *draft.overlay_preview.borrow_mut() = Some((key, handle));
            }
        } else if !worker.pending.contains(&key) && worker.pending.len() < 3 {
            let inputs = document
                .layers
                .iter()
                .map(|layer| {
                    crate::project::texture_material::texture_path(project, &layer.texture)
                        .map(|path| (path, layer.clone()))
                })
                .collect::<Result<Vec<_>, _>>();
            if let Ok(inputs) = inputs {
                if worker.send.send((key, inputs)).is_ok() {
                    worker.pending.insert(key);
                }
            }
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
    if let Some((hash, texture)) = draft.overlay_preview.borrow().as_ref() {
        if *hash == key {
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

#[cfg(test)]
mod sky_async_texture_preview_tests {
    use super::*;
    use crate::project::material::{MaterialLayer, MaterialLayerBlend};
    #[test]
    fn sky_preview_composition_preserves_blends_and_does_not_decode_on_queue() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln-sky-layer-preview-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let red = root.join("red.png");
        let blue = root.join("blue.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(&red)
            .unwrap();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255]))
            .save(&blue)
            .unwrap();
        let make = |opacity, blend| MaterialLayer {
            name: "Layer".into(),
            texture: "Media/test.png".into(),
            opacity,
            blend,
        };
        let layers = vec![
            (red.clone(), make(1.0, MaterialLayerBlend::Normal)),
            (blue.clone(), make(0.5, MaterialLayerBlend::Normal)),
        ];
        let result = compose_sky_layers(&layers).unwrap();
        assert_eq!(result.len(), 256 * 128 * 4);
        assert!(result[0] >= 127 && result[0] <= 128);
        assert_eq!(result[1], 0);
        assert!(result[2] >= 127 && result[2] <= 128);
        assert_eq!(result[3], 255);
        let worker = SkyLayerWorker::new(egui::Context::default());
        let start = std::time::Instant::now();
        worker.send.send((87231, layers)).unwrap();
        assert!(start.elapsed() < std::time::Duration::from_millis(100));
        let (key, async_result) = worker
            .receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(key, 87231);
        assert_eq!(async_result.unwrap(), result);
        assert!(
            compose_sky_layers(&[(
                root.join("missing.png"),
                make(1.0, MaterialLayerBlend::Normal)
            )])
            .is_none()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}

fn sky_preview(ui: &mut egui::Ui, draft: &SkyMaterialPreview, height: f32) {
    let width = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    for row in 0..height.ceil() as usize {
        let fraction_from_top = (row as f32 + 0.5) / height;
        let p = 1.0 - fraction_from_top;
        let line = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + row as f32),
            egui::vec2(rect.width(), 1.3),
        );
        ui.painter()
            .rect_filled(line, 0.0, sky_color(draft.sample(p)));
    }
    sky_layer_preview(ui, draft, rect);
    ui.painter().rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    ui.painter().text(
        rect.left_top() + egui::vec2(12.0, 12.0),
        egui::Align2::LEFT_TOP,
        "ZENITH",
        egui::FontId::monospace(11.0),
        egui::Color32::WHITE,
    );
    ui.painter().text(
        rect.left_bottom() + egui::vec2(12.0, -12.0),
        egui::Align2::LEFT_BOTTOM,
        "HORIZON",
        egui::FontId::monospace(11.0),
        egui::Color32::WHITE,
    );
}
fn sky_gradient_bar(ui: &mut egui::Ui, draft: &mut SkyMaterialPreview) {
    let width = ui.available_width().max(120.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 57.0), egui::Sense::hover());
    let track = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(10.0, 5.0),
        egui::pos2(rect.right() - 10.0, rect.top() + 27.0),
    );
    let mut gradient_mesh = egui::Mesh::default();
    for column in 0..track.width().ceil() as usize {
        let p = (column as f32 + 0.5) / track.width();
        let stripe = egui::Rect::from_min_size(
            egui::pos2(track.left() + column as f32, track.top()),
            egui::vec2(1.3, track.height()),
        );
        gradient_mesh.add_colored_rect(stripe, sky_color(draft.sample(p)));
    }
    ui.painter().add(egui::Shape::mesh(gradient_mesh));
    ui.painter().rect_stroke(
        track,
        2.0,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
    for index in 0..draft.gradient.stops.len() {
        let x = track.left() + draft.gradient.stops[index].position * track.width();
        let handle = egui::Rect::from_center_size(
            egui::pos2(x, track.bottom() + 12.0),
            egui::vec2(17.0, 19.0),
        );
        let handle_id = ui.id().with(("sky_stop", index));
        let response = ui.interact(handle, handle_id, egui::Sense::click_and_drag());
        if response.clicked() || response.drag_started() {
            draft.selected = index;
        }
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                draft.set_stop_position(index, (pointer.x - track.left()) / track.width());
                draft.selected = index;
                ui.ctx().request_repaint();
            }
        }
        let center = egui::pos2(
            track.left() + draft.gradient.stops[index].position * track.width(),
            handle.center().y,
        );
        let accent = ui.visuals().selection.bg_fill;
        ui.painter()
            .circle_filled(center, 7.0, sky_color(draft.gradient.stops[index].color));
        ui.painter().circle_stroke(
            center,
            8.0,
            egui::Stroke::new(
                if draft.selected == index { 2.5 } else { 1.0 },
                if draft.selected == index {
                    accent
                } else {
                    egui::Color32::WHITE
                },
            ),
        );
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SkyEditorAction {
    None,
    Return,
    Save,
    Apply,
}

pub(crate) fn draw_sky_material_editor(
    ui: &mut egui::Ui,
    draft: &mut SkyMaterialPreview,
) -> SkyEditorAction {
    let mut action = SkyEditorAction::None;
    let mut restored = if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
        draft.navigate_history(ui.input(|i| i.modifiers.shift))
    } else if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y)) {
        draft.navigate_history(true)
    } else {
        false
    };
    draft.begin_history_frame();
    ui.horizontal(|ui| {
        if ui.button("Return to Materials").clicked() {
            action = SkyEditorAction::Return;
        }
        ui.separator();
        ui.label(egui::RichText::new(&draft.name).strong().size(19.0));
        ui.label(egui::RichText::new("Sky Material").color(ui.visuals().weak_text_color()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!draft.is_dirty(), egui::Button::new("Apply to World"))
                .clicked()
            {
                action = SkyEditorAction::Apply;
            }
            if ui.button("Save").clicked() {
                action = SkyEditorAction::Save;
            }
            if ui
                .add_enabled(!draft.redo_history.is_empty(), egui::Button::new("Redo"))
                .on_hover_text("Redo the last Material edit")
                .clicked()
            {
                restored = draft.navigate_history(true);
            }
            if ui
                .add_enabled(!draft.undo_history.is_empty(), egui::Button::new("Undo"))
                .on_hover_text("Undo the last Material edit")
                .clicked()
            {
                restored = draft.navigate_history(false);
            }
            if draft.is_dirty() {
                ui.label(
                    egui::RichText::new("Unsaved")
                        .small()
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("sky_material_editor_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.strong("LIVE SKY PREVIEW");
            ui.add_space(5.0);
            let height = (ui.available_height() * 0.56).clamp(190.0, 460.0);
            sky_preview(ui, draft, height);
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.strong("BLEND MAP");
                ui.label(format!("{} color stops", draft.gradient.stops.len()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            draft.gradient.stops.len() > 2
                                && draft.selected > 0
                                && draft.selected + 1 < draft.gradient.stops.len(),
                            egui::Button::new("Remove"),
                        )
                        .clicked()
                    {
                        draft.gradient.stops.remove(draft.selected);
                        draft.selected = draft.selected.min(draft.gradient.stops.len() - 1);
                    }
                    if ui
                        .add_enabled(draft.gradient.stops.len() < 16, egui::Button::new("Add"))
                        .clicked()
                    {
                        draft.add_stop();
                    }
                });
            });
            sky_gradient_bar(ui, draft);
            ui.add_space(12.0);
            ui.separator();
            if draft.selected >= draft.gradient.stops.len() {
                draft.selected = draft.gradient.stops.len().saturating_sub(1);
            }
            if !draft.gradient.stops.is_empty() {
                let index = draft.selected;
                ui.horizontal(|ui| {
                    ui.label("Stop Position");
                    let minimum = if index == 0 {
                        0.0
                    } else {
                        draft.gradient.stops[index - 1].position + 0.001
                    };
                    let maximum = if index + 1 == draft.gradient.stops.len() {
                        1.0
                    } else {
                        draft.gradient.stops[index + 1].position - 0.001
                    };
                    let mut value = draft.gradient.stops[index].position * 100.0;
                    if ui
                        .add_enabled(
                            index > 0 && index + 1 < draft.gradient.stops.len(),
                            egui::Slider::new(&mut value, (minimum * 100.0)..=(maximum * 100.0))
                                .suffix("%"),
                        )
                        .changed()
                    {
                        draft.set_stop_position(index, value / 100.0);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Stop Color");
                    ui.color_edit_button_rgb(&mut draft.gradient.stops[index].color);
                    let color = sky_color(draft.gradient.stops[index].color);
                    ui.monospace(format!(
                        "#{:02X}{:02X}{:02X}",
                        color.r(),
                        color.g(),
                        color.b()
                    ));
                });
            }
            ui.horizontal(|ui| {
                ui.label("Interpolation");
                egui::ComboBox::from_id_salt("sky_gradient_interpolation")
                    .selected_text(draft.gradient.interpolation.label())
                    .show_ui(ui, |ui| {
                        for mode in [SkyInterpolation::Linear, SkyInterpolation::SmoothLinear] {
                            ui.selectable_value(
                                &mut draft.gradient.interpolation,
                                mode,
                                mode.label(),
                            );
                        }
                    });
            });
            ui.separator();
            if let Some(material_draft) = draft.material_draft.as_mut() {
                if let Err(error) = draw_shared_material_layers(
                    ui,
                    material_draft,
                    &draft.texture_choices,
                    "sky",
                    false,
                    draft.project.as_deref(),
                ) {
                    draft.status = Some((error, false));
                }
            }
        });
    if let Some((message, success)) = &draft.status {
        ui.colored_label(
            if *success {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::LIGHT_RED
            },
            message,
        );
    }
    if !restored {
        draft.track_edits(ui.input(|i| i.pointer.any_down()));
    }
    action
}

#[path = "material_editor_water.rs"]
mod material_editor_water;
#[path = "material_graph_editor.rs"]
mod material_graph_editor;

#[derive(Clone, Debug, PartialEq)]
struct SurfaceEditSnapshot {
    document: crate::project::material::MaterialDocument,
    color: [f32; 4],
    roughness: f32,
    metallic: f32,
    water_look: Option<TerrainWaterSettings>,
}
#[derive(Clone, Debug)]
pub(crate) struct SurfaceMaterialEditor {
    pub project: std::path::PathBuf,
    pub name: String,
    pub draft: crate::project::material_draft::MaterialDraft,
    pub color: [f32; 4],
    pub roughness: f32,
    pub metallic: f32,
    pub texture_maps: std::collections::BTreeMap<String, String>,
    texture_choices: Vec<String>,
    saved_color: [f32; 4],
    saved_roughness: f32,
    saved_metallic: f32,
    pub water_look: Option<TerrainWaterSettings>,
    saved_water_look: Option<TerrainWaterSettings>,
    pub landscape_assigned: bool,
    pub is_post_process: bool,
    pub status: Option<(String, bool)>,
    pub graph_view: bool,
    pub graph_palette_open: bool,
    pub graph_search: String,
    pub graph_selected: Option<u32>,
    pub graph_pan: egui::Vec2,
    pub graph_zoom: f32,
    pub graph_link_from: Option<(u32, String, crate::project::material::PinType)>,
    pub graph_error: Option<String>,
    pub graph_parent_request: Option<String>,
    undo_history: Vec<SurfaceEditSnapshot>,
    redo_history: Vec<SurfaceEditSnapshot>,
    editing_drag: bool,
    history_current: Option<SurfaceEditSnapshot>,
}
impl SurfaceMaterialEditor {
    pub fn open(project: &std::path::Path, name: &str) -> Result<Self, String> {
        use crate::project::material::MaterialCatalog;
        let catalog = MaterialCatalog::load(project);
        Self::open_with_snapshot(project, name, &catalog)
    }
    pub fn open_with_snapshot(
        project: &std::path::Path,
        name: &str,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> Result<Self, String> {
        use crate::project::material::MaterialOutput;
        let draft = crate::project::material_draft::MaterialDraft::open_with_snapshot(
            project, name, catalog,
        )?;
        let mut is_post_process = false;
        let (base_color, roughness, metallic, water_look) =
            match catalog.resolve(&draft.document().id)? {
                MaterialOutput::SurfacePbr {
                    base_color,
                    roughness,
                    metallic,
                } => (base_color, roughness, metallic, None),
                MaterialOutput::Water(look) => ([1.0; 4], 0.5, 0.0, Some(look)),
                MaterialOutput::PostProcess => {
                    is_post_process = true;
                    ([1.0; 4], 0.5, 0.0, None)
                }
                MaterialOutput::Sky(_) => {
                    return Err("Sky Material uses the existing Sky Material Editor".into());
                }
            };
        let texture_maps = catalog.texture_maps(&draft.document().id)?;
        Ok(Self {
            project: project.to_owned(),
            name: name.to_owned(),
            draft,
            color: base_color,
            roughness,
            metallic,
            texture_maps,
            texture_choices: Vec::new(),
            saved_color: base_color,
            saved_roughness: roughness,
            saved_metallic: metallic,
            water_look,
            saved_water_look: water_look,
            landscape_assigned: false,
            is_post_process,
            status: None,
            graph_view: false,
            graph_palette_open: false,
            graph_search: String::new(),
            graph_selected: None,
            graph_pan: egui::Vec2::ZERO,
            graph_zoom: 1.0,
            graph_link_from: None,
            graph_error: None,
            graph_parent_request: None,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            editing_drag: false,
            history_current: None,
        })
    }
    pub fn create(project: &std::path::Path) -> Result<Self, String> {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        for suffix in 0..10000 {
            let name = if suffix == 0 {
                "New Surface.MAT".to_owned()
            } else {
                format!("New Surface_{suffix}.MAT")
            };
            if crate::project::material::file_path(project, &name)?.exists() {
                continue;
            }
            let definition = MaterialDefinition::Asset {
                material: MaterialOutput::SurfacePbr {
                    base_color: [0.8, 0.8, 0.8, 1.0],
                    roughness: 0.5,
                    metallic: 0.0,
                },
            };
            let mut draft =
                crate::project::material_draft::MaterialDraft::create(project, &name, definition)?;
            draft.save(project)?;
            return Self::open(project, &name);
        }
        Err("No available Surface Material name".into())
    }
    pub fn create_post_process(project: &std::path::Path) -> Result<Self, String> {
        use crate::project::material::{
            GraphLink, GraphNode, GraphOperation, MaterialDefinition, MaterialGraph, MaterialOutput,
        };
        for suffix in 0..10000 {
            let name = if suffix == 0 {
                "New Post Process.MAT".into()
            } else {
                format!("New Post Process_{suffix}.MAT")
            };
            if crate::project::material::file_path(project, &name)?.exists() {
                continue;
            }
            let definition = MaterialDefinition::Asset {
                material: MaterialOutput::PostProcess,
            };
            let mut draft =
                crate::project::material_draft::MaterialDraft::create(project, &name, definition)?;
            draft.edit_graph(Some(MaterialGraph {
                output_node: 2,
                nodes: vec![
                    GraphNode {
                        id: 1,
                        operation: GraphOperation::SceneColor,
                        position: [30.0, 70.0],
                    },
                    GraphNode {
                        id: 2,
                        operation: GraphOperation::PostProcessOutput,
                        position: [300.0, 70.0],
                    },
                ],
                links: vec![GraphLink {
                    source_node: 1,
                    source_pin: "color".into(),
                    target_node: 2,
                    target_pin: "color".into(),
                }],
            }))?;
            draft.save(project)?;
            return Self::open(project, &name);
        }
        Err("No available Post Process Material name".into())
    }
    pub fn create_water(project: &std::path::Path) -> Result<Self, String> {
        use crate::project::material::{MaterialDefinition, MaterialOutput};
        for suffix in 0..10000 {
            let name = if suffix == 0 {
                "New Water.MAT".to_owned()
            } else {
                format!("New Water_{suffix}.MAT")
            };
            if crate::project::material::file_path(project, &name)?.exists() {
                continue;
            }
            let definition = MaterialDefinition::Asset {
                material: MaterialOutput::Water(
                    crate::world::terrain::TerrainWaterSettings::default(),
                ),
            };
            let mut draft =
                crate::project::material_draft::MaterialDraft::create(project, &name, definition)?;
            draft.save(project)?;
            return Self::open(project, &name);
        }
        Err("No available Water Material name".into())
    }
    fn snapshot(&self) -> SurfaceEditSnapshot {
        SurfaceEditSnapshot {
            document: self.draft.document().clone(),
            color: self.color,
            roughness: self.roughness,
            metallic: self.metallic,
            water_look: self.water_look,
        }
    }
    fn begin_history_frame(&mut self) {
        if self.history_current.is_none() {
            self.history_current = Some(self.snapshot());
        }
    }
    fn track_edits(&mut self, pointer_down: bool) {
        let Some(previous) = self.history_current.take() else {
            return;
        };
        let changed = self.color != previous.color
            || self.roughness != previous.roughness
            || self.metallic != previous.metallic
            || self.water_look != previous.water_look
            || self.draft.document() != &previous.document;
        if changed {
            if !self.editing_drag {
                self.undo_history.push(previous);
                if self.undo_history.len() > 64 {
                    self.undo_history.remove(0);
                }
                self.redo_history.clear();
            }
            self.history_current = Some(self.snapshot());
            self.editing_drag = pointer_down;
        } else {
            self.history_current = Some(previous);
            if !pointer_down {
                self.editing_drag = false;
            }
        }
    }
    fn navigate_history(&mut self, redo: bool) -> bool {
        self.editing_drag = false;
        let item = if redo {
            self.redo_history.pop()
        } else {
            self.undo_history.pop()
        };
        let Some(previous) = item else { return false };
        let now = self.snapshot();
        if let Err(error) = self
            .draft
            .restore_editor_snapshot(previous.document.clone())
        {
            self.status = Some((error, false));
            return false;
        }
        if redo {
            self.undo_history.push(now);
        } else {
            self.redo_history.push(now);
        }
        self.color = previous.color;
        self.roughness = previous.roughness;
        self.metallic = previous.metallic;
        self.water_look = previous.water_look;
        self.texture_maps = self.draft.document().textures.clone();
        self.history_current = Some(self.snapshot());
        self.graph_error = None;
        self.status = None;
        true
    }
    pub fn accept_background_save(
        &mut self,
        request: &crate::project::material_draft::MaterialSaveRequest,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> Result<(), String> {
        self.draft.accept_saved(request, catalog)?;
        match catalog.resolve(&request.document.id)? {
            crate::project::material::MaterialOutput::SurfacePbr {
                base_color,
                roughness,
                metallic,
            } => {
                self.saved_color = base_color;
                self.saved_roughness = roughness;
                self.saved_metallic = metallic;
            }
            crate::project::material::MaterialOutput::PostProcess => {}
            crate::project::material::MaterialOutput::Water(look) => {
                self.saved_water_look = Some(look);
            }
            _ => return Err("Saved Material output changed unexpectedly".into()),
        }
        self.texture_maps = self.draft.document().textures.clone();
        Ok(())
    }
    pub fn dirty(&self) -> bool {
        self.draft.is_dirty()
            || self.water_look != self.saved_water_look
            || self.color != self.saved_color
            || self.roughness != self.saved_roughness
            || self.metallic != self.saved_metallic
    }
    pub fn save(&mut self) -> Result<crate::project::material::MaterialCatalog, String> {
        let baseline = crate::project::material::MaterialCatalog::load(&self.project);
        self.prepare_for_save(&baseline)?;
        let catalog = self.draft.save(&self.project)?;
        self.apply_published(&catalog)?;
        Ok(catalog)
    }
    pub fn prepare_for_save(
        &mut self,
        baseline: &crate::project::material::MaterialCatalog,
    ) -> Result<(), String> {
        if self.water_look.is_some() {
            return material_editor_water::prepare_water(self, baseline);
        }
        use crate::project::material::{
            MaterialCatalog, MaterialDefinition, MaterialOutput, Parameter, ParameterValue,
        };
        let definition = match &self.draft.document().definition {
            MaterialDefinition::Asset { .. } => MaterialDefinition::Asset {
                material: MaterialOutput::SurfacePbr {
                    base_color: self.color,
                    roughness: self.roughness,
                    metallic: self.metallic,
                },
            },
            MaterialDefinition::Instance { parent, overrides } => {
                let MaterialOutput::SurfacePbr {
                    base_color: parent_color,
                    roughness: parent_roughness,
                    metallic: parent_metallic,
                } = baseline.resolve(parent)?
                else {
                    return Err("Parent Material does not resolve to Surface PBR".into());
                };
                let mut next = overrides.clone();
                if self.color != self.saved_color {
                    if self.color == parent_color {
                        next.remove(&Parameter::BaseColor);
                    } else {
                        next.insert(Parameter::BaseColor, ParameterValue::Color4(self.color));
                    }
                }
                if self.roughness != self.saved_roughness {
                    if self.roughness == parent_roughness {
                        next.remove(&Parameter::Roughness);
                    } else {
                        next.insert(Parameter::Roughness, ParameterValue::Scalar(self.roughness));
                    }
                }
                if self.metallic != self.saved_metallic {
                    if self.metallic == parent_metallic {
                        next.remove(&Parameter::Metallic);
                    } else {
                        next.insert(Parameter::Metallic, ParameterValue::Scalar(self.metallic));
                    }
                }
                MaterialDefinition::Instance {
                    parent: parent.clone(),
                    overrides: next,
                }
            }
        };
        self.draft.edit(definition)?;
        Ok(())
    }
    pub fn apply_published(
        &mut self,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> Result<(), String> {
        if self.water_look.is_some() {
            return material_editor_water::apply_saved_water(self, catalog);
        }
        if let Ok(MaterialOutput::SurfacePbr {
            base_color,
            roughness,
            metallic,
        }) = catalog.resolve(&self.draft.document().id)
        {
            self.color = base_color;
            self.roughness = roughness;
            self.metallic = metallic;
        }
        if let Ok(texture_maps) = catalog.texture_maps(&self.draft.document().id) {
            self.texture_maps = texture_maps;
        }
        self.saved_color = self.color;
        self.saved_roughness = self.roughness;
        self.saved_metallic = self.metallic;
        Ok(())
    }
}

pub(crate) fn surface_preview_sky(
    world_sky_id: Option<&str>,
    catalog: &crate::project::material::MaterialCatalog,
) -> crate::project::sky_material::SkyGradient {
    world_sky_id
        .and_then(|id| catalog.sky(id))
        .unwrap_or_else(|| crate::project::sky_material::default_sky().clone())
}

fn preview_cube(
    ui: &mut egui::Ui,
    material: &SurfaceMaterialEditor,
    sky: &crate::project::sky_material::SkyGradient,
    renderer: &crate::renderer::Renderer,
    ratio: f32,
    graph: Option<&crate::project::material::MaterialGraph>,
) -> Option<String> {
    let width = ui.available_width().max(200.0);
    let height = (ui.available_height() * ratio).clamp(140.0, 450.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let paint = ui.painter_at(rect);
    let mut sky_mesh = egui::Mesh::default();
    for y in 0..height.ceil() as usize {
        let h = 1.0 - (y as f32 / height).clamp(0.0, 1.0);
        let rgb = sky.sample(h);
        let col = egui::Color32::from_rgb(
            (rgb[0] * 255.0) as u8,
            (rgb[1] * 255.0) as u8,
            (rgb[2] * 255.0) as u8,
        );
        sky_mesh.add_colored_rect(
            egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + y as f32),
                egui::vec2(width, 1.1),
            ),
            col,
        );
    }
    paint.add(egui::Shape::mesh(sky_mesh));
    let preview_id = renderer.preview_layered_material(
        &material.project,
        material.draft.document(),
        material.color,
        material.roughness,
        material.metallic,
    );
    let map = preview_id
        .as_deref()
        .or_else(|| material.texture_maps.get("BaseColor").map(String::as_str));
    let preview_color = if preview_id.is_some() {
        [1.0; 4]
    } else {
        material.color
    };
    let base = crate::project::material::MaterialOutput::SurfacePbr {
        base_color: material.color,
        roughness: material.roughness,
        metallic: material.metallic,
    };
    let result = renderer.surface_material_preview(
        rect,
        preview_color,
        material.roughness,
        material.metallic,
        map,
        graph.map(|graph| (graph, &base, &material.texture_maps)),
        material.landscape_assigned,
    );
    let error = match result {
        Ok(callback) => {
            paint.add(callback);
            None
        }
        Err(error) => {
            if let Ok(callback) = renderer.surface_material_preview(
                rect,
                preview_color,
                material.roughness,
                material.metallic,
                map,
                None,
                material.landscape_assigned,
            ) {
                paint.add(callback);
            }
            Some(error)
        }
    };
    paint.text(
        rect.left_bottom() + egui::vec2(12.0, -12.0),
        egui::Align2::LEFT_BOTTOM,
        "Material preview · stylized lighting",
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
    error
}
fn available_material_textures(project: &std::path::Path) -> Vec<String> {
    let root = project.join(".assets/textures");
    let mut choices = Vec::new();
    if let Ok(found) = crate::project::fs_walk::scan(&root, 8, 250_000) {
        for (path, is_dir) in found {
            if choices.len() >= 4096 {
                break;
            }
            if is_dir
                || !path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
            {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(&root) {
                choices.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    choices.sort();
    choices
}

fn draw_material_layers(ui: &mut egui::Ui, editor: &mut SurfaceMaterialEditor) {
    let result = draw_shared_material_layers(
        ui,
        &mut editor.draft,
        &editor.texture_choices,
        "surface",
        true,
        Some(&editor.project),
    );
    match result {
        Ok(()) => editor.texture_maps = editor.draft.document().textures.clone(),
        Err(error) => editor.status = Some((error, false)),
    }
}

fn draw_shared_material_layers(
    ui: &mut egui::Ui,
    draft: &mut crate::project::material_draft::MaterialDraft,
    _textures: &[String],
    context: &str,
    show_base: bool,
    project: Option<&std::path::Path>,
) -> Result<(), String> {
    use crate::project::material::{MaterialDefinition, MaterialLayer, MaterialLayerBlend};
    if !matches!(
        draft.document().definition,
        MaterialDefinition::Asset { .. }
    ) {
        return Ok(());
    }
    let mut maps = draft.document().textures.clone();
    let mut base = maps.get("BaseColor").cloned().unwrap_or_default();
    if show_base {
        ui.label("Base Texture");
        let mut picked = base.clone();
        if crate::editor::editor::asset_browser::draw_project_asset_drop(
            ui,
            project,
            &mut picked,
            crate::editor::editor::asset_browser::AssetDropKind::Texture,
        ) {
            base = picked;
        }
        if !base.is_empty() && ui.small_button("Clear Texture").clicked() {
            base.clear();
        }
        if base != maps.get("BaseColor").cloned().unwrap_or_default() {
            if base.is_empty() {
                maps.remove("BaseColor");
            } else {
                maps.insert("BaseColor".into(), base);
            }
            draft.edit_texture_maps(maps)?;
        }
    }
    ui.separator();
    let mut layers = draft.document().layers.clone();
    let previous = layers.clone();
    let pending_id = egui::Id::new((
        "material_pending_layer",
        context,
        project,
        draft.document().id.as_str(),
    ));
    let mut pending = ui
        .ctx()
        .data(|data| data.get_temp::<Option<MaterialLayer>>(pending_id))
        .flatten();
    ui.horizontal(|ui| {
        ui.strong("Texture Layers");
        if ui.button("+ Add Layer").clicked() && pending.is_none() {
            let mut suffix = layers.len() + 1;
            while layers
                .iter()
                .any(|layer| layer.name == format!("Layer {suffix}"))
            {
                suffix += 1;
            }
            pending = Some(MaterialLayer {
                name: format!("Layer {suffix}"),
                texture: String::new(),
                opacity: 1.0,
                blend: MaterialLayerBlend::Normal,
            });
        }
    });
    if let Some(layer) = pending.as_mut() {
        let mut cancel = false;
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.strong("New Layer");
                ui.text_edit_singleline(&mut layer.name);
                cancel = ui.small_button("Cancel").clicked();
            });
            crate::editor::editor::asset_browser::draw_project_asset_drop(
                ui,
                project,
                &mut layer.texture,
                crate::editor::editor::asset_browser::AssetDropKind::Texture,
            );
        });
        if cancel {
            pending = None;
        }
    }
    if pending
        .as_ref()
        .is_some_and(|layer| !layer.texture.is_empty())
    {
        layers.push(pending.take().unwrap());
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(pending_id, pending));
    let mut operation: Option<(usize, usize)> = None;
    let mut removed: Option<usize> = None;
    for index in 0..layers.len() {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut layers[index].name);
                if ui
                    .small_button("Up")
                    .on_hover_text("Move layer up")
                    .clicked()
                    && index > 0
                {
                    operation = Some((index, index - 1));
                }
                if ui
                    .small_button("Down")
                    .on_hover_text("Move layer down")
                    .clicked()
                    && index + 1 < layers.len()
                {
                    operation = Some((index, index + 1));
                }
                if ui
                    .small_button("Del")
                    .on_hover_text("Remove material layer")
                    .clicked()
                {
                    removed = Some(index);
                }
            });
            crate::editor::editor::asset_browser::draw_project_asset_drop(
                ui,
                project,
                &mut layers[index].texture,
                crate::editor::editor::asset_browser::AssetDropKind::Texture,
            );
            ui.horizontal(|ui| {
                ui.label("Blend Mode");
                egui::ComboBox::from_id_salt(("stars_layer_blend", context, index))
                    .selected_text(match layers[index].blend {
                        MaterialLayerBlend::Normal => "Normal",
                        MaterialLayerBlend::Multiply => "Multiply",
                        MaterialLayerBlend::Add => "Add",
                        MaterialLayerBlend::Screen => "Screen",
                    })
                    .show_ui(ui, |ui| {
                        for (label, mode) in [
                            ("Normal", MaterialLayerBlend::Normal),
                            ("Multiply", MaterialLayerBlend::Multiply),
                            ("Add", MaterialLayerBlend::Add),
                            ("Screen", MaterialLayerBlend::Screen),
                        ] {
                            ui.selectable_value(&mut layers[index].blend, mode, label);
                        }
                    });
            });
            ui.add(egui::Slider::new(&mut layers[index].opacity, 0.0..=1.0).text("Opacity"));
        });
    }
    if let Some((from, to)) = operation {
        layers.swap(from, to);
    }
    if let Some(index) = removed {
        layers.remove(index);
    }
    if layers != previous {
        draft.edit_layers(layers)?;
    }
    Ok(())
}

pub(crate) fn draw_surface_material_editor(
    ui: &mut egui::Ui,
    material: &mut SurfaceMaterialEditor,
    sky: &crate::project::sky_material::SkyGradient,
    renderer: &crate::renderer::Renderer,
) -> SkyEditorAction {
    let mut action = SkyEditorAction::None;
    let mut restored = if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
        material.navigate_history(ui.input(|i| i.modifiers.shift))
    } else if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Y)) {
        material.navigate_history(true)
    } else {
        false
    };
    material.begin_history_frame();
    ui.horizontal(|ui| {
        if ui.button("Return to Materials").clicked() {
            action = SkyEditorAction::Return;
        }
        ui.separator();
        ui.heading(&material.name);
        ui.label(if material.is_post_process {
            "Post Process"
        } else if material.water_look.is_some() {
            "Water Material"
        } else if material.landscape_assigned {
            "Landscape Material"
        } else {
            "Surface Material"
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Save").clicked() {
                action = SkyEditorAction::Save;
            }
            if ui
                .add_enabled(!material.redo_history.is_empty(), egui::Button::new("Redo"))
                .on_hover_text("Redo the last Material edit")
                .clicked()
            {
                restored = material.navigate_history(true);
            }
            if ui
                .add_enabled(!material.undo_history.is_empty(), egui::Button::new("Undo"))
                .on_hover_text("Undo the last Material edit")
                .clicked()
            {
                restored = material.navigate_history(false);
            }
            if material.dirty() {
                ui.label("Unsaved changes");
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut material.graph_view, false, "Parameters");
        ui.selectable_value(&mut material.graph_view, true, "Graph");
        ui.small("BETA");
    });
    ui.separator();
    if material.graph_view {
        material_graph_editor::show(ui, material, sky, renderer);
    } else if material.is_post_process {
        ui.strong("Post Process");
        ui.selectable_value(&mut material.graph_view, true, "Graph");
    } else if material.water_look.is_some() {
        egui::ScrollArea::vertical()
            .id_salt("water_material_editor_scroll")
            .show(ui, |ui| {
                ui.strong("WATER PREVIEW");
                if let Some(look) = material.water_look {
                    let width = ui.available_width().max(200.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 210.0), egui::Sense::hover());
                    let overlay = renderer
                        .preview_layered_material(
                            &material.project,
                            material.draft.document(),
                            [0.0; 4],
                            0.5,
                            0.0,
                        )
                        .map(|id| renderer.get_texture(&id));
                    match renderer.water_material_preview_with_layers(rect, look, overlay) {
                        Ok(preview) => {
                            ui.painter().add(preview);
                        }
                        Err(error) => {
                            material.graph_error = Some(error.clone());
                            ui.colored_label(
                                ui.visuals().error_fg_color,
                                format!("Material shader error: {error}"),
                            );
                        }
                    }
                }
                if let Some(look) = material.water_look.as_mut() {
                    draw_water_parameters(ui, look);
                }
                ui.separator();
                if let Err(error) = draw_shared_material_layers(
                    ui,
                    &mut material.draft,
                    &material.texture_choices,
                    "water",
                    false,
                    Some(&material.project),
                ) {
                    material.status = Some((error, false));
                }
            });
    } else {
        egui::ScrollArea::vertical().id_salt("surface_material_editor_scroll").show(ui,|ui|{
        ui.strong(if material.landscape_assigned {"LANDSCAPE PREVIEW"} else {"MATERIAL PREVIEW"});
        let _ = preview_cube(ui,material,sky,renderer,0.55,None);
        ui.add_space(12.0);
        ui.heading("SURFACE APPEARANCE");
        draw_material_layers(ui, material);
        if !material.texture_maps.is_empty() {
            ui.separator();
            ui.strong("Assigned Texture Maps");
            for (channel, path) in &material.texture_maps {
                ui.horizontal(|ui| {
                    ui.label(channel);
                    ui.monospace(path);
                });
            }
            ui.small("BaseColor textures render in this preview; remaining texture map assignments are preserved when saving.");
            ui.separator();
        }
        ui.horizontal(|ui|{
            ui.label("Base Color");
            let mut rgb=[material.color[0],material.color[1],material.color[2]];
            if ui.color_edit_button_rgb(&mut rgb).changed(){material.color[..3].copy_from_slice(&rgb);}
        });
        ui.horizontal(|ui|{
            ui.label("Hue / Saturation / Value");
            let mut hsv=egui::ecolor::Hsva::from(egui::Color32::from_rgb(
                (material.color[0]*255.0) as u8,(material.color[1]*255.0) as u8,(material.color[2]*255.0) as u8
            ));
            let mut changed=false;
            changed|=ui.add(egui::Slider::new(&mut hsv.h,0.0..=1.0).text("Hue")).changed();
            changed|=ui.add(egui::Slider::new(&mut hsv.s,0.0..=1.0).text("Sat")).changed();
            changed|=ui.add(egui::Slider::new(&mut hsv.v,0.0..=1.0).text("Value")).changed();
            if changed {
                let rgb=egui::Color32::from(hsv);
                material.color[..3].copy_from_slice(&[rgb.r() as f32/255.0,rgb.g() as f32/255.0,rgb.b() as f32/255.0]);
            }
        });
        ui.horizontal(|ui|{
            ui.label("Alpha");
            ui.add(egui::Slider::new(&mut material.color[3],0.0..=1.0));
            ui.small("Stored in Base Color; runtime transparency parity is pending.");
        });
        ui.horizontal(|ui|{
            ui.label("Roughness");
            ui.add(egui::Slider::new(&mut material.roughness,0.0..=1.0));
            ui.label("Metallic");
            ui.add(egui::Slider::new(&mut material.metallic,0.0..=1.0));
        });
    });
    }
    if let Some((status, ok)) = &material.status {
        ui.colored_label(
            if *ok {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::LIGHT_RED
            },
            status,
        );
    }
    if !restored {
        material.track_edits(ui.input(|i| i.pointer.any_down()));
    }
    action
}

#[cfg(test)]
mod surface_material_editor_tests {
    use super::{SurfaceMaterialEditor, surface_preview_sky};
    use crate::project::material::{
        self, MaterialCatalog, MaterialDefinition, MaterialDocument, MaterialOutput, Parameter,
        ParameterValue,
    };
    use std::collections::BTreeMap;

    struct TemporaryProject(std::path::PathBuf);
    impl TemporaryProject {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("wk-mat013-{:032x}", rand::random::<u128>()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TemporaryProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn surface_instance_edits_keep_parent_maps_and_unrelated_overrides() {
        let project = TemporaryProject::new();
        let mut parent = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.2, 0.3, 0.4, 1.0],
                roughness: 0.5,
                metallic: 0.1,
            },
        });
        parent
            .textures
            .insert("BaseColor".into(), "textures/example.png".into());
        material::save(&project.0, "parent.MAT", &parent, None).unwrap();

        let instance = MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent.id.clone(),
            overrides: BTreeMap::from([(Parameter::Metallic, ParameterValue::Scalar(0.8))]),
        });
        material::save(&project.0, "child.MAT", &instance, None).unwrap();

        let mut editor = SurfaceMaterialEditor::open(&project.0, "child.MAT").unwrap();
        assert_eq!(editor.metallic, 0.8);
        assert_eq!(editor.roughness, 0.5);
        assert_eq!(
            editor.texture_maps.get("BaseColor").map(String::as_str),
            Some("textures/example.png")
        );
        assert!(!editor.dirty());

        editor.roughness = 0.3;
        assert!(editor.dirty());
        editor.save().unwrap();
        assert!(!editor.dirty());
        let saved = material::read(&project.0, "child.MAT").unwrap();
        let MaterialDefinition::Instance {
            parent: saved_parent,
            overrides,
        } = saved.definition
        else {
            panic!("Saving a Material Instance must not flatten its parent");
        };
        assert_eq!(saved_parent, parent.id);
        assert_eq!(
            overrides.get(&Parameter::Roughness),
            Some(&ParameterValue::Scalar(0.3))
        );
        assert_eq!(
            overrides.get(&Parameter::Metallic),
            Some(&ParameterValue::Scalar(0.8))
        );
        assert!(!overrides.contains_key(&Parameter::BaseColor));
        assert_eq!(material::read(&project.0, "parent.MAT").unwrap(), parent);

        editor.metallic = 0.1;
        editor.save().unwrap();
        let saved = material::read(&project.0, "child.MAT").unwrap();
        let MaterialDefinition::Instance { overrides, .. } = saved.definition else {
            panic!("Instance definition unexpectedly changed");
        };
        assert!(!overrides.contains_key(&Parameter::Metallic));
        assert_eq!(
            overrides.get(&Parameter::Roughness),
            Some(&ParameterValue::Scalar(0.3))
        );
    }

    #[test]
    fn surface_preview_reuses_world_sky_selection_and_default_fallback() {
        let mut authored = crate::project::sky_material::default_sky().clone();
        authored.stops[0].color = [0.15, 0.32, 0.67];
        let document = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Sky(authored.clone()),
        });
        let catalog = MaterialCatalog::from_documents(BTreeMap::from([(
            "authored_sky.MAT".into(),
            document.clone(),
        )]));
        assert_eq!(surface_preview_sky(Some(&document.id), &catalog), authored);
        assert_eq!(
            surface_preview_sky(Some("missing-material"), &catalog),
            *crate::project::sky_material::default_sky()
        );
        assert_eq!(
            surface_preview_sky(None, &catalog),
            *crate::project::sky_material::default_sky()
        );
    }

    #[test]
    fn surface_asset_save_keeps_texture_maps_and_identity() {
        let project = TemporaryProject::new();
        let mut asset = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.8, 0.7, 0.6, 1.0],
                roughness: 0.4,
                metallic: 0.0,
            },
        });
        asset
            .textures
            .insert("NormalGL".into(), "textures/normal.png".into());
        material::save(&project.0, "normal.MAT", &asset, None).unwrap();
        let mut editor = SurfaceMaterialEditor::open(&project.0, "normal.MAT").unwrap();
        editor.color[0] = 0.5;
        editor.save().unwrap();
        let saved = material::read(&project.0, "normal.MAT").unwrap();
        assert_eq!(saved.id, asset.id);
        assert_eq!(saved.textures, asset.textures);
        assert!(!editor.dirty());
    }

    #[test]
    fn surface_instance_refreshes_untouched_values_after_parent_changes() {
        let project = TemporaryProject::new();
        let mut parent = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.2, 0.3, 0.4, 1.0],
                roughness: 0.5,
                metallic: 0.1,
            },
        });
        material::save(&project.0, "base.MAT", &parent, None).unwrap();
        let child = MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent.id.clone(),
            overrides: BTreeMap::new(),
        });
        material::save(&project.0, "derived.MAT", &child, None).unwrap();
        let mut editor = SurfaceMaterialEditor::open(&project.0, "derived.MAT").unwrap();

        let previous = std::fs::read(material::file_path(&project.0, "base.MAT").unwrap()).unwrap();
        parent.definition = MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.9, 0.8, 0.7, 1.0],
                roughness: 0.65,
                metallic: 0.3,
            },
        };
        parent
            .textures
            .insert("NormalGL".into(), "textures/updated-normal.png".into());
        material::save(&project.0, "base.MAT", &parent, Some(&previous)).unwrap();

        editor.roughness = 0.2;
        editor.save().unwrap();
        assert_eq!(editor.color, [0.9, 0.8, 0.7, 1.0]);
        assert_eq!(editor.roughness, 0.2);
        assert_eq!(editor.metallic, 0.3);
        assert_eq!(
            editor.texture_maps.get("NormalGL").map(String::as_str),
            Some("textures/updated-normal.png")
        );
        assert!(!editor.dirty());
        let saved = material::read(&project.0, "derived.MAT").unwrap();
        let MaterialDefinition::Instance { overrides, .. } = saved.definition else {
            panic!("Child must remain an instance");
        };
        assert_eq!(overrides.len(), 1);
        assert_eq!(
            overrides.get(&Parameter::Roughness),
            Some(&ParameterValue::Scalar(0.2))
        );
    }

    #[test]
    fn surface_editor_rejects_concurrent_save_without_losing_disk_changes() {
        let project = TemporaryProject::new();
        let asset = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.4, 0.5, 0.6, 1.0],
                roughness: 0.7,
                metallic: 0.2,
            },
        });
        material::save(&project.0, "shared.MAT", &asset, None).unwrap();
        let mut first = SurfaceMaterialEditor::open(&project.0, "shared.MAT").unwrap();
        let mut second = SurfaceMaterialEditor::open(&project.0, "shared.MAT").unwrap();

        first.roughness = 0.1;
        first.save().unwrap();
        second.metallic = 0.9;
        let error = second.save().unwrap_err();
        assert!(error.contains("changed on disk"), "{error}");
        assert!(second.dirty());
        let document = material::read(&project.0, "shared.MAT").unwrap();
        assert_eq!(document.id, asset.id);
        assert_eq!(
            document.definition,
            MaterialDefinition::Asset {
                material: MaterialOutput::SurfacePbr {
                    base_color: [0.4, 0.5, 0.6, 1.0],
                    roughness: 0.1,
                    metallic: 0.2,
                },
            }
        );
    }

    #[test]
    fn surface_editor_rejects_deleted_instance_parent() {
        let project = TemporaryProject::new();
        let parent = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.6, 0.5, 0.4, 1.0],
                roughness: 0.5,
                metallic: 0.0,
            },
        });
        material::save(&project.0, "parent.MAT", &parent, None).unwrap();
        let child = MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent.id.clone(),
            overrides: BTreeMap::new(),
        });
        material::save(&project.0, "child.MAT", &child, None).unwrap();
        std::fs::remove_file(material::file_path(&project.0, "parent.MAT").unwrap()).unwrap();
        let error = SurfaceMaterialEditor::open(&project.0, "child.MAT").unwrap_err();
        assert!(
            error.contains("Missing parent Material") || error.contains("Missing Material"),
            "{error}"
        );
        assert_eq!(material::read(&project.0, "child.MAT").unwrap(), child);
    }
}

#[cfg(test)]
mod instant_material_creation_tests {
    use super::*;
    #[test]
    fn new_surface_water_and_sky_are_files_before_editor_save() {
        let root =
            std::env::temp_dir().join(format!("wkmat012-create-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let surface = SurfaceMaterialEditor::create(&root).unwrap();
        let water = SurfaceMaterialEditor::create_water(&root).unwrap();
        let sky = SkyMaterialPreview::for_project(&root).unwrap();
        assert!(
            crate::project::material::file_path(&root, &surface.name)
                .unwrap()
                .is_file()
        );
        assert!(
            crate::project::material::file_path(&root, &water.name)
                .unwrap()
                .is_file()
        );
        assert!(
            crate::project::material::file_path(&root, &sky.name)
                .unwrap()
                .is_file()
        );
        assert!(water.water_look.is_some());
        let surface_id = surface.draft.document().id.clone();
        let water_id = water.draft.document().id.clone();
        drop(surface);
        drop(water);
        drop(sky);
        let reopened = SurfaceMaterialEditor::open(&root, "New Water.MAT").unwrap();
        assert_eq!(reopened.draft.document().id, water_id);
        assert_eq!(
            SurfaceMaterialEditor::open(&root, "New Surface.MAT")
                .unwrap()
                .draft
                .document()
                .id,
            surface_id
        );
        assert!(SkyMaterialPreview::open(&root, "New Sky.MAT").is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod stars_material_editor_tests {
    use super::SurfaceMaterialEditor;
    use crate::project::material::{self, MaterialDefinition, MaterialOutput};
    #[test]
    fn stars_create_surface_persists_before_editor_save() {
        let root =
            std::env::temp_dir().join(format!("wk-stars-editor-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let created = SurfaceMaterialEditor::create(&root).unwrap();
        let path = material::file_path(&root, &created.name).unwrap();
        assert!(path.exists());
        let reopened = material::read(&root, &created.name).unwrap();
        assert_eq!(reopened.id, created.draft.document().id);
        let MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr { base_color, .. },
        } = reopened.definition
        else {
            panic!("Expected surface material")
        };
        assert_eq!(base_color, [0.8, 0.8, 0.8, 1.0]);
        drop(created);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod mat014_save_and_undo_tests {
    use super::*;
    use crate::project::material::{MaterialDefinition, MaterialDocument, MaterialOutput};
    use std::collections::BTreeMap;
    fn project() -> std::path::PathBuf {
        let project =
            std::env::temp_dir().join(format!("mat014-workflow-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&project).unwrap();
        project
    }
    #[test]
    fn surface_parameter_undo_redo_restores_working_values_without_disk_save() {
        let root = project();
        let mut editor = SurfaceMaterialEditor::create(&root).unwrap();
        let before = editor.snapshot();
        editor.begin_history_frame();
        editor.color = [0.1, 0.2, 0.3, 1.0];
        editor.track_edits(false);
        assert!(editor.dirty());
        assert!(editor.navigate_history(false));
        assert_eq!(editor.color, before.color);
        assert!(!editor.dirty());
        assert!(editor.navigate_history(true));
        assert_eq!(editor.color, [0.1, 0.2, 0.3, 1.0]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn undo_after_save_restores_pre_edit_surface_and_can_be_saved_again() {
        let root = project();
        let mut editor = SurfaceMaterialEditor::create(&root).unwrap();
        let original = editor.color;
        editor.begin_history_frame();
        editor.color = [0.1, 0.6, 0.3, 1.0];
        editor.track_edits(false);
        editor.save().unwrap();
        assert!(!editor.dirty());
        assert!(editor.navigate_history(false));
        assert_eq!(editor.color, original);
        assert!(editor.dirty());
        editor.save().unwrap();
        let saved = crate::project::material::read(&root, &editor.name).unwrap();
        assert_eq!(
            saved.definition,
            MaterialDefinition::Asset {
                material: MaterialOutput::SurfacePbr {
                    base_color: original,
                    roughness: 0.5,
                    metallic: 0.0
                }
            }
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sky_gradient_undo_redo_restores_unsaved_values() {
        let root = project();
        let mut editor = SkyMaterialPreview::for_project(&root).unwrap();
        let before = editor.snapshot();
        editor.begin_history_frame();
        editor.gradient.stops[1].color = [0.2, 0.5, 0.8];
        editor.track_edits(false);
        assert!(editor.is_dirty());
        assert!(editor.navigate_history(false));
        assert_eq!(editor.gradient, before.gradient);
        assert!(!editor.is_dirty());
        assert!(editor.navigate_history(true));
        assert_eq!(editor.gradient.stops[1].color, [0.2, 0.5, 0.8]);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn undo_after_sky_save_restores_gradient_for_resave() {
        let root = project();
        let mut editor = SkyMaterialPreview::for_project(&root).unwrap();
        let original = editor.gradient.clone();
        editor.begin_history_frame();
        editor.gradient.stops[1].color = [0.1, 0.2, 0.9];
        editor.track_edits(false);
        editor.save(&root).unwrap();
        assert!(!editor.is_dirty());
        assert!(editor.navigate_history(false));
        assert_eq!(editor.gradient, original);
        assert!(editor.is_dirty());
        editor.save(&root).unwrap();
        let saved = crate::project::material::read(&root, &editor.name).unwrap();
        assert_eq!(
            saved.definition,
            MaterialDefinition::Asset {
                material: MaterialOutput::Sky(original)
            }
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn background_save_conflict_protection_and_subsequent_edits() {
        let root = project();
        let mut editor = SurfaceMaterialEditor::create(&root).unwrap();
        editor.color = [0.2, 0.3, 0.4, 1.0];
        let baseline = crate::project::material::MaterialCatalog::load(&root);
        editor.prepare_for_save(&baseline).unwrap();
        let request = editor.draft.save_request();
        editor.color = [0.8, 0.3, 0.4, 1.0];
        let worker_request = request.clone();
        let catalog = std::thread::spawn(move || worker_request.run())
            .join()
            .unwrap()
            .unwrap();
        editor.accept_background_save(&request, &catalog).unwrap();
        assert_eq!(editor.color, [0.8, 0.3, 0.4, 1.0]);
        assert!(editor.dirty());
        assert_eq!(
            catalog.resolve(&request.document.id).unwrap(),
            MaterialOutput::SurfacePbr {
                base_color: [0.2, 0.3, 0.4, 1.0],
                roughness: 0.5,
                metallic: 0.0
            }
        );
        assert!(request.run().is_err());
        editor.prepare_for_save(&catalog).unwrap();
        let request2 = editor.draft.save_request();
        let catalog2 = request2.run().unwrap();
        editor.accept_background_save(&request2, &catalog2).unwrap();
        assert!(!editor.dirty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sky_open_with_published_catalog_preserves_document_and_identity() {
        let root = project();
        let sky = SkyMaterialPreview::for_project(&root).unwrap();
        let catalog = crate::project::material::MaterialCatalog::load(&root);
        let second = SkyMaterialPreview::open_with_snapshot(&root, &sky.name, &catalog).unwrap();
        assert_eq!(second.material_id(), sky.material_id());
        assert_eq!(second.gradient, sky.gradient);
        assert!(second.undo_history.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "Windows-only performance probe: cold catalog scan, warm Sky open, save IO"]
    fn material_sky_open_save_scaling_probe() {
        use std::time::Instant;
        for count in [1usize, 64, 256, 512] {
            let root = project();
            let material_root = root.join(".assets/materials");
            std::fs::create_dir_all(&material_root).unwrap();
            let sky = MaterialDocument::new(MaterialDefinition::Asset {
                material: MaterialOutput::Sky(crate::project::sky_material::SkyGradient::default()),
            });
            std::fs::write(
                material_root.join("sky.json"),
                serde_json::to_vec_pretty(&sky).unwrap(),
            )
            .unwrap();
            for i in 0..count {
                let doc = MaterialDocument::new(MaterialDefinition::Asset {
                    material: MaterialOutput::SurfacePbr {
                        base_color: [0.5; 4],
                        roughness: 0.5,
                        metallic: 0.0,
                    },
                });
                std::fs::write(
                    material_root.join(format!("mat_{i:04}.json")),
                    serde_json::to_vec_pretty(&doc).unwrap(),
                )
                .unwrap();
            }
            let t = Instant::now();
            let snapshot = crate::project::material::MaterialCatalog::load(&root);
            let cold = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut edit = SkyMaterialPreview::open_with_snapshot(&root, "sky", &snapshot).unwrap();
            let warm = t.elapsed().as_secs_f64() * 1000.0;
            edit.gradient.stops[0].color = [0.2, 0.1, 0.8];
            let t = Instant::now();
            let request = edit.prepare_save_request().unwrap();
            let ui_prep = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let catalog = request.run().unwrap();
            let worker = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            edit.accept_background_save(&request, &catalog).unwrap();
            let ui_accept = t.elapsed().as_secs_f64() * 1000.0;
            assert!(!edit.is_dirty());
            eprintln!(
                "MAT014_TIMING docs={} catalog_cold_ms={:.3} sky_warm_open_ms={:.3} save_ui_prepare_ms={:.3} save_worker_ms={:.3} save_ui_accept_ms={:.3}",
                count + 1,
                cold,
                warm,
                ui_prep,
                worker,
                ui_accept
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
