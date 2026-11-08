// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{ColorTarget, Editor, EditorTool, History, SettingsPage};
use crate::editor::tools;
use crate::engine::audio::{EngineAudioSettings, EngineSound};
use crate::renderer::Renderer;
use crate::renderer::settings::{
    ApplyBehavior, BounceLighting, CacheUpdateRate, DebugView, Denoising, DynamicGeometryGi,
    GiResolution, IndirectDiffuse, LightingMethod, LightingQuality, PerformanceDisplay,
    SceneQueryBackend, ShadowMethod, TemporalReuse,
};
use crate::world::World;
use crate::world::terrain::TerrainGenerationRequest;
use std::cmp::Ordering;
use std::path::PathBuf;

const SETTINGS_NAV_WIDTH: f32 = 180.0;
const SETTINGS_LABEL_WIDTH: f32 = 230.0;
const SETTINGS_CONTROL_WIDTH: f32 = 280.0;

fn terrain_panel_label_width(available: f32) -> f32 {
    (available.max(170.0) * 0.44).clamp(76.0, 165.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaterPresetKind {
    Lagoon,
    Tropical,
    OpenOcean,
    ChoppyCoast,
    Storm,
    Clear,
    Murky,
    Cinematic,
    Legacy,
    SavedCustom,
}
impl WaterPresetKind {
    const BUILTIN: [Self; 9] = [
        Self::Lagoon,
        Self::Tropical,
        Self::OpenOcean,
        Self::ChoppyCoast,
        Self::Storm,
        Self::Clear,
        Self::Murky,
        Self::Cinematic,
        Self::Legacy,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Lagoon => "Calm Lagoon",
            Self::Tropical => "Tropical Shallows",
            Self::OpenOcean => "Open Ocean",
            Self::ChoppyCoast => "Choppy Coast",
            Self::Storm => "Storm Swell",
            Self::Clear => "Clear Underwater",
            Self::Murky => "Murky Deep",
            Self::Cinematic => "Cinematic Tropical",
            Self::Legacy => "Legacy Default",
            Self::SavedCustom => "Saved Project Preset",
        }
    }
    fn underwater(self) -> bool {
        matches!(
            self,
            Self::Tropical | Self::Storm | Self::Clear | Self::Murky | Self::Cinematic
        )
    }
    fn apply(
        self,
        current: crate::world::terrain::TerrainWaterSettings,
        saved: Option<crate::world::terrain::TerrainWaterSettings>,
        include_direction: bool,
        include_underwater: bool,
    ) -> Option<crate::world::terrain::TerrainWaterSettings> {
        use crate::world::terrain::TerrainWaterSettings;
        let mut v = match self {
            Self::SavedCustom => saved?,
            Self::Legacy => TerrainWaterSettings::default(),
            _ => {
                let mut v = current;
                let p = match self {
                    Self::Lagoon => (
                        0.20, 0.45, 62.0, 0.05, 0.18, 1.5, 1.5, 0.40, 0.25, 0.65, 0.85,
                    ),
                    Self::Tropical => (
                        0.55, 0.72, 30.0, 0.20, 0.52, 2.0, 1.2, 0.64, 0.42, 1.05, 0.84,
                    ),
                    Self::OpenOcean => {
                        (1.35, 1.10, 80.0, 0.28, 0.55, 2.1, 2.0, 0.78, 0.60, 1.0, 0.9)
                    }
                    Self::ChoppyCoast => (
                        1.20, 1.50, 18.0, 0.78, 1.50, 2.9, 2.3, 0.95, 0.37, 1.25, 0.9,
                    ),
                    Self::Storm => (3.5, 2.25, 34.0, 0.90, 2.0, 3.5, 3.5, 1.2, 0.20, 0.55, 0.97),
                    Self::Clear => (
                        0.45, 0.55, 45.0, 0.11, 0.30, 1.7, 0.7, 0.32, 0.35, 0.82, 0.75,
                    ),
                    Self::Murky => (
                        0.70, 0.68, 48.0, 0.18, 0.35, 1.8, 3.7, 1.35, 0.18, 0.38, 0.98,
                    ),
                    Self::Cinematic => (
                        0.72, 0.80, 33.0, 0.34, 0.90, 2.3, 1.2, 0.70, 0.48, 1.15, 0.83,
                    ),
                    _ => unreachable!(),
                };
                (
                    v.wave_height,
                    v.wave_speed,
                    v.wave_length,
                    v.wave_steepness,
                    v.foam_amount,
                    v.foam_scale,
                    v.absorption_strength,
                    v.scattering_strength,
                    v.reflection_strength,
                    v.specular_strength,
                    v.opacity,
                ) = p;
                v
            }
        };
        if !include_direction {
            v.wave_direction_degrees = current.wave_direction_degrees;
        }
        if !include_underwater {
            v.underwater_color = current.underwater_color;
            v.underwater_opacity = current.underwater_opacity;
            v.underwater_effects_enabled = current.underwater_effects_enabled;
            v.underwater_fog_color = current.underwater_fog_color;
            v.underwater_fog_density = current.underwater_fog_density;
            v.underwater_depth_fog = current.underwater_depth_fog;
            v.underwater_depth_fog_limit = current.underwater_depth_fog_limit;
            v.underwater_fog_limit = current.underwater_fog_limit;
            v.underwater_sky_tint = current.underwater_sky_tint;
        } else {
            match self {
                Self::Tropical | Self::Cinematic => {
                    v.underwater_effects_enabled = true;
                    v.underwater_fog_color = [0.04, 0.39, 0.53];
                    v.underwater_color = [0.045, 0.28, 0.38];
                    v.underwater_opacity = 0.94;
                    v.underwater_fog_density = 0.048;
                    v.underwater_depth_fog = 0.013;
                    v.underwater_depth_fog_limit = 0.34;
                    v.underwater_fog_limit = 0.88;
                    v.underwater_sky_tint = 0.78;
                }
                Self::Clear => {
                    v.underwater_effects_enabled = true;
                    v.underwater_fog_color = [0.08, 0.47, 0.55];
                    v.underwater_fog_density = 0.014;
                    v.underwater_depth_fog = 0.004;
                    v.underwater_depth_fog_limit = 0.13;
                    v.underwater_fog_limit = 0.65;
                }
                Self::Murky | Self::Storm => {
                    v.underwater_effects_enabled = true;
                    v.underwater_fog_color = [0.018, 0.10, 0.13];
                    v.underwater_fog_density = 0.22;
                    v.underwater_depth_fog = 0.06;
                    v.underwater_depth_fog_limit = 0.85;
                    v.underwater_fog_limit = 1.0;
                    v.underwater_opacity = 1.0;
                }
                _ => {}
            }
        }
        debug_assert!(v.validate().is_ok());
        Some(v)
    }
}
fn water_preset_controls(
    ui: &mut egui::Ui,
    config: &mut crate::world::terrain::TerrainConfig,
    project_path: Option<&std::path::Path>,
) -> bool {
    let id = egui::Id::new((
        "wk015_water_preset",
        project_path.map(|p| p.to_string_lossy().into_owned()),
    ));
    let mut selected = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Option<WaterPresetKind>>(id).flatten());
    let mut direction = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(id.with("direction")).unwrap_or(false));
    let mut underwater = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(id.with("underwater")).unwrap_or(false));
    let chosen = selected.unwrap_or(WaterPresetKind::Legacy);
    let mut label = chosen.label();
    if let Some(applied) = ui
        .ctx()
        .data_mut(|d| d.get_temp::<crate::world::terrain::TerrainWaterSettings>(id.with("applied")))
    {
        if config.water != applied {
            label = "Custom (modified)";
        }
    } else if selected.is_none() {
        label = "Custom";
    }
    ui.horizontal(|ui| {
        ui.label("Preset");
        egui::ComboBox::from_id_salt(id.with("menu"))
            .width(ui.available_width().max(120.0))
            .selected_text(label)
            .show_ui(ui, |ui| {
                for kind in WaterPresetKind::BUILTIN {
                    if ui
                        .selectable_label(selected == Some(kind), kind.label())
                        .clicked()
                    {
                        selected = Some(kind);
                    }
                }
                if config.water_custom_preset.is_some()
                    && ui
                        .selectable_label(
                            selected == Some(WaterPresetKind::SavedCustom),
                            "Saved Project Preset",
                        )
                        .clicked()
                {
                    selected = Some(WaterPresetKind::SavedCustom);
                }
            });
    });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut direction, "Include direction");
        ui.checkbox(&mut underwater, "Include underwater");
    });
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let enabled = selected.is_some();
        if ui
            .add_enabled(enabled, egui::Button::new("Apply Preset"))
            .clicked()
        {
            if let Some(v) = chosen.apply(
                config.water,
                config.water_custom_preset,
                direction,
                underwater,
            ) {
                if v != config.water {
                    config.water = v;
                    changed = true;
                }
                ui.ctx().data_mut(|d| d.insert_temp(id.with("applied"), v));
            }
        }
        if ui.button("Save Project Preset").clicked() {
            if config.water_custom_preset != Some(config.water) {
                config.water_custom_preset = Some(config.water);
                changed = true;
            }
            selected = Some(WaterPresetKind::SavedCustom);
            ui.ctx()
                .data_mut(|d| d.insert_temp(id.with("applied"), config.water));
        }
        if ui.button("Reset Water Defaults").clicked() {
            let default = crate::world::terrain::TerrainWaterSettings::default();
            if config.water != default {
                config.water = default;
                changed = true;
            }
            selected = Some(WaterPresetKind::Legacy);
            ui.ctx()
                .data_mut(|d| d.insert_temp(id.with("applied"), default));
        }
    });
    ui.ctx().data_mut(|d| {
        d.insert_temp(id, selected);
        d.insert_temp(id.with("direction"), direction);
        d.insert_temp(id.with("underwater"), underwater);
    });
    ui.small("Presets affect project water, not referenced Material assets.");
    changed
}

fn settings_row_widths(ui: &egui::Ui) -> (f32, f32) {
    let available = ui.available_width();
    let full_row =
        SETTINGS_LABEL_WIDTH + SETTINGS_CONTROL_WIDTH + ui.spacing().item_spacing.x + 8.0;
    let label = if available >= full_row {
        SETTINGS_LABEL_WIDTH
    } else {
        terrain_panel_label_width(available)
    };
    let control = SETTINGS_CONTROL_WIDTH
        .min((available - label - ui.spacing().item_spacing.x - 8.0).max(70.0));
    (label, control)
}

fn settings_page_header(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.heading(egui::RichText::new(title).monospace().strong().size(18.0));

    ui.add_space(4.0);

    ui.label(egui::RichText::new(description).color(egui::Color32::from_gray(145)));

    ui.add_space(12.0);
}

fn settings_section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);

    ui.label(
        egui::RichText::new(title)
            .monospace()
            .strong()
            .color(egui::Color32::from_gray(190)),
    );

    ui.separator();
    ui.add_space(4.0);
}

fn settings_combo(
    ui: &mut egui::Ui,
    id: &'static str,
    label: &str,
    selected: &str,
    options: &[&str],
) {
    let (label_width, control_width) = settings_row_widths(ui);
    ui.horizontal(|ui| {
        ui.add_sized([label_width, 24.0], egui::Label::new(label));

        let mut current = options
            .iter()
            .position(|option| *option == selected)
            .unwrap_or(0);

        ui.add_enabled_ui(false, |ui| {
            egui::ComboBox::from_id_salt(id)
                .width(control_width)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (index, option) in options.iter().enumerate() {
                        ui.selectable_value(&mut current, index, *option);
                    }
                });
        });
    });
}

fn settings_toggle(ui: &mut egui::Ui, label: &str, enabled: bool) {
    ui.horizontal(|ui| {
        ui.add_sized([SETTINGS_LABEL_WIDTH, 24.0], egui::Label::new(label));

        let mut value = enabled;
        let text = if enabled { "On" } else { "Off" };

        ui.add_enabled(false, egui::Checkbox::new(&mut value, text));
    });
}

fn settings_slider(ui: &mut egui::Ui, label: &str, value: f32, min: f32, max: f32, suffix: &str) {
    ui.horizontal(|ui| {
        ui.add_sized([SETTINGS_LABEL_WIDTH, 24.0], egui::Label::new(label));

        let mut value = value;

        ui.add_enabled(
            false,
            egui::Slider::new(&mut value, min..=max)
                .show_value(true)
                .suffix(suffix),
        );
    });
}

fn settings_button(ui: &mut egui::Ui, label: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.add_sized([SETTINGS_LABEL_WIDTH, 24.0], egui::Label::new(label));

        ui.add_enabled(false, egui::Button::new(text));
    });
}

#[allow(dead_code)]
fn draw_lighting_settings_placeholder(ui: &mut egui::Ui) {
    settings_page_header(
        ui,
        "LIGHTING",
        "Lighting pipeline, global illumination, shadows, scene queries, and diagnostics.",
    );

    settings_section(ui, "PIPELINE");

    settings_combo(
        ui,
        "settings_lighting_method",
        "Lighting Method",
        "Legacy",
        &[
            "Legacy",
            "Two-Level Radiance Cache",
            "Split Radiance Cascades",
        ],
    );

    settings_combo(
        ui,
        "settings_shadow_method",
        "Shadow Method",
        "Legacy Shadow Map",
        &["Off", "Legacy Shadow Map", "Cached Directional Clipmaps"],
    );

    settings_combo(
        ui,
        "settings_lighting_quality",
        "Lighting Quality",
        "Balanced",
        &["Performance", "Balanced", "Quality", "Custom"],
    );

    settings_section(ui, "GLOBAL ILLUMINATION");

    settings_toggle(ui, "Indirect Diffuse", false);

    settings_combo(
        ui,
        "settings_bounce_lighting",
        "Bounce Lighting",
        "Single Bounce",
        &["Single Bounce", "Multiple Bounce"],
    );

    settings_toggle(ui, "Temporal Reuse", false);

    settings_combo(
        ui,
        "settings_dynamic_geometry_gi",
        "Dynamic Geometry GI",
        "Off",
        &["Off", "Bodies Only", "Bodies + Actors + Characters"],
    );

    settings_combo(
        ui,
        "settings_gi_resolution",
        "GI Resolution",
        "Half",
        &["Quarter", "Half", "Full"],
    );

    settings_section(ui, "SCENE QUERY & CACHE");

    settings_combo(
        ui,
        "settings_scene_query_backend",
        "Scene Query Backend",
        "Automatic",
        &["Automatic", "Chunk Grid DDA", "Occupied-Chunk BVH"],
    );

    settings_combo(
        ui,
        "settings_cache_update_rate",
        "Cache Update Rate",
        "Medium",
        &["Low", "Medium", "High", "Every Frame"],
    );

    settings_combo(
        ui,
        "settings_denoising",
        "Denoising",
        "Off",
        &["Off", "Spatial", "Spatial + Temporal"],
    );

    settings_section(ui, "DIAGNOSTICS");

    settings_combo(
        ui,
        "settings_lighting_debug_view",
        "Debug View",
        "Final Lighting",
        &[
            "Final Lighting",
            "Direct Only",
            "Indirect Only",
            "Albedo",
            "Normals",
            "Depth",
            "Screen Probes",
            "World Cache",
            "Ray Hits",
            "Cache Invalidations",
        ],
    );

    settings_combo(
        ui,
        "settings_lighting_performance",
        "Performance Display",
        "Off",
        &[
            "Off",
            "Frame Time",
            "Pass Timings",
            "Rays and Cache Statistics",
        ],
    );

    settings_combo(
        ui,
        "settings_lighting_apply",
        "Apply Behavior",
        "Apply Now when supported",
        &["Apply Now when supported", "Restart Required"],
    );
}

fn editable_settings_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &'static str,
    label: &str,
    value: &mut T,
    options: &[T],
    option_label: impl Fn(T) -> &'static str,
) -> bool {
    let before = *value;
    let (label_width, control_width) = settings_row_widths(ui);
    ui.horizontal(|ui| {
        ui.add_sized([label_width, 24.0], egui::Label::new(label));
        egui::ComboBox::from_id_salt(id)
            .width(control_width)
            .selected_text(option_label(*value))
            .show_ui(ui, |ui| {
                for &option in options {
                    ui.selectable_value(value, option, option_label(option));
                }
            });
    });
    *value != before
}

fn editable_settings_toggle(ui: &mut egui::Ui, label: &str, value: &mut bool) -> bool {
    let before = *value;
    let (label_width, _) = settings_row_widths(ui);
    ui.horizontal(|ui| {
        ui.add_sized([label_width, 24.0], egui::Label::new(label));
        ui.checkbox(value, if *value { "On" } else { "Off" });
    });
    *value != before
}

pub(super) fn water_material_picker(
    ui: &mut egui::Ui,
    project: Option<&std::path::Path>,
    selected: &mut Option<String>,
    _slot: u64,
) -> bool {
    let mut assignment = selected.clone().unwrap_or_default();
    let mut changed = super::asset_browser::draw_project_asset_drop(
        ui,
        project,
        &mut assignment,
        super::asset_browser::AssetDropKind::WaterMaterial,
    );
    if changed {
        *selected = Some(assignment);
    }
    if selected.is_some() && ui.small_button("Use Default Water").clicked() {
        *selected = None;
        changed = true;
    }
    changed
}

pub(crate) fn draw_terrain_settings(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    world: &mut World,
    project_path: &Option<PathBuf>,
) {
    let label_width = terrain_panel_label_width(ui.available_width());

    ui.collapsing("Import Heightmap", |ui| {
        super::image_heightmap_ui::draw(ui, editor, world);
    });

    ui.collapsing("Generate", |ui| {
        super::terrain_layers_ui::draw_generation_layers_ui(ui, editor, world);
    });

    ui.collapsing("Layers", |ui| {
        settings_section(ui, "PROCEDURAL LAYERS");
        let layers_before = world.terrain.config.voxel_layers;
        let layers = &mut world.terrain.config.voxel_layers;
        for (label, value, speed, range) in [
            (
                "Surface Depth",
                &mut layers.surface_depth_studs,
                0.25,
                0.0..=64.0,
            ),
            (
                "Subsurface Depth",
                &mut layers.subsurface_depth_studs,
                0.25,
                0.0..=128.0,
            ),
            (
                "Rock Slope",
                &mut layers.rock_slope_degrees,
                0.25,
                0.0..=90.0,
            ),
            (
                "Slope Blend",
                &mut layers.rock_slope_blend_degrees,
                0.25,
                0.0..=90.0,
            ),
            (
                "Layer Noise Scale",
                &mut layers.material_noise_scale,
                0.001,
                0.001..=1.0,
            ),
            (
                "Layer Noise Strength",
                &mut layers.material_noise_strength_studs,
                0.1,
                0.0..=32.0,
            ),
            (
                "Slope Shading",
                &mut layers.slope_shading_strength,
                0.01,
                0.0..=0.75,
            ),
        ] {
            ui.horizontal(|ui| {
                ui.add_sized([label_width, 24.0], egui::Label::new(label));
                ui.add(egui::DragValue::new(value).speed(speed).clamp_range(range));
            });
        }

        if world.terrain.config.voxel_layers != layers_before {
            editor.needs_save = true;
        }

        settings_section(ui, "LANDSCAPE BLENDING");
        let blend_before = world.terrain.config.material_blend;
        let blend = &mut world.terrain.config.material_blend;
        for (name, value, range) in [
            ("Rock Starts", &mut blend.rock_start, 0.05..=0.90),
            ("Rock Full", &mut blend.rock_end, 0.10..=1.0),
            ("Dirt Blend", &mut blend.dirt_strength, 0.0..=0.5),
            ("Beach Width", &mut blend.beach_width, 1.0..=18.0),
        ] {
            ui.horizontal(|ui| {
                ui.label(name);
                ui.add(egui::Slider::new(value, range).step_by(0.01));
            });
        }
        blend.rock_end = blend.rock_end.max(blend.rock_start + 0.01);
        if world.terrain.config.material_blend != blend_before {
            editor.needs_save = true;
        }
    });
    ui.collapsing("Grass", |ui| {
        let grass_before = world.terrain.config.grass;
        let grass = &mut world.terrain.config.grass;

        ui.horizontal(|ui| {
            ui.add_sized([label_width, 24.0], egui::Label::new("Enabled"));
            let enabled_label = if grass.enabled { "On" } else { "Off" };
            ui.checkbox(&mut grass.enabled, enabled_label);
        });

        ui.add_enabled_ui(grass.enabled, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized([label_width, 24.0], egui::Label::new("Grass Density"));
                ui.add(
                    egui::Slider::new(&mut grass.density, 0.0..=1.0)
                        .step_by(0.01)
                        .show_value(true),
                );
            });
            ui.horizontal(|ui| {
                ui.add_sized([label_width, 24.0], egui::Label::new("Grass Length"));
                ui.add(
                    egui::Slider::new(&mut grass.length_studs, 0.25..=4.0)
                        .step_by(0.05)
                        .show_value(true)
                        .suffix(" studs"),
                );
            });
        });

        if world.terrain.config.grass != grass_before {
            editor.needs_save = true;
        }
    });
    ui.collapsing("Water Bodies", |ui| {
        let original = world.terrain.config.water_bodies.clone();
        let shore_layers = world.terrain.config.terrain_material_layers();
        let bodies = &mut world.terrain.config.water_bodies;
        let mut delete_body = None;
        for (index, body) in bodies.iter_mut().enumerate() {
            let kind = match body.kind {
                crate::world::terrain::WaterBodyKind::Lake => "Lake",
                crate::world::terrain::WaterBodyKind::River => "River",
            };
            let title = body
                .name
                .as_ref()
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("{kind} {}", body.id));
            let active = editor.water_selected_body == Some(body.id);
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(active, format!("{title}  \u{00b7}  Select"))
                    .clicked()
                {
                    editor.water_select(body.id);
                }
            });
            egui::CollapsingHeader::new(format!("{title} Settings"))
                .id_salt(("water_body", body.id))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut body.visible, "Visible");
                        if ui.button("Delete").clicked() {
                            delete_body = Some(index);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        let mut name = body.name.clone().unwrap_or_default();
                        if ui.text_edit_singleline(&mut name).changed() {
                            body.name = if name.trim().is_empty() {
                                None
                            } else {
                                Some(name)
                            };
                        }
                        ui.label("Height");
                        ui.add(
                            egui::DragValue::new(&mut body.elevation)
                                .speed(0.1)
                                .range(-10000.0..=10000.0),
                        );
                    });

                    if water_material_picker(ui,project_path.as_deref(),&mut body.material_asset,body.id) {
                        body.appearance=body.material_asset.as_deref().and_then(|id|
                            project_path.as_deref().map(crate::project::material::MaterialCatalog::load)
                                .and_then(|catalog|catalog.water(id)));
                        editor.needs_save=true;
                    }
                    super::terrain_material_ui::shore_selector(
                        ui,
                        ("body_shore", body.id),
                        &mut body.shore_material_layer,
                        &shore_layers,
                    );
                    ui.collapsing("Advanced Point Coordinates", |ui| {
                        let minimum = if body.kind == crate::world::terrain::WaterBodyKind::Lake {
                            3
                        } else {
                            2
                        };
                        let allow_remove = body.points.len() > minimum;
                        let mut delete_point = None;
                        for (j, point) in body.points.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("Point {}", j + 1));
                                ui.label("X");
                                ui.add(egui::DragValue::new(&mut point.position[0]).speed(0.25));
                                ui.label("Z");
                                ui.add(egui::DragValue::new(&mut point.position[1]).speed(0.25));
                                if body.kind == crate::world::terrain::WaterBodyKind::River {
                                    ui.label("Width");
                                    ui.add(
                                        egui::DragValue::new(&mut point.width)
                                            .speed(0.25)
                                            .range(0.1..=4096.0),
                                    );
                                }
                                if ui
                                    .add_enabled(allow_remove, egui::Button::new("Remove"))
                                    .clicked()
                                {
                                    delete_point = Some(j);
                                }
                            });
                        }
                        if let Some(j) = delete_point {
                            body.points.remove(j);
                        }
                        if body.points.len() < 128 && ui.button("Add Point").clicked() {
                            if let Some(last) = body.points.last().copied() {
                                let mut next = last;
                                if body.kind == crate::world::terrain::WaterBodyKind::River {
                                    next.position[0] += 4.0;
                                } else {
                                    let first = body.points[0].position;
                                    next.position = [
                                        (last.position[0] + first[0]) * 0.5,
                                        (last.position[1] + first[1]) * 0.5,
                                    ];
                                }
                                body.points.push(next);
                            }
                        }
                    });
                });
        }
        if let Some(i) = delete_body {
            bodies.remove(i);
        }
        if *bodies != original {
            let shape_changed = original.len() != bodies.len()
                || original.iter().zip(bodies.iter()).any(|(old, new)| {
                    old.id != new.id
                        || old.kind != new.kind
                        || old.visible != new.visible
                        || old.elevation != new.elevation
                        || old.points != new.points
                });
            if shape_changed {
                world.terrain.mark_full_render_dirty();
            }
            editor.needs_save = true;
        }

        settings_section(ui, "WATER APPEARANCE");
        let water_before = super::water_spline::WaterEditSnapshot::capture(
            &world.terrain.config,
            editor.water_selected_body,
        );

        let shore_layers = world.terrain.config.terrain_material_layers();
        if super::terrain_material_ui::shore_selector(
            ui,
            "ocean_shore",
            &mut world.terrain.config.shore_material_layer,
            &shore_layers,
        ) {
            editor.needs_save = true;
        }
        if ui
            .checkbox(&mut world.terrain.config.ocean_enabled, "Ocean enabled")
            .changed()
        {
            world.terrain.mark_full_render_dirty();
            editor.needs_save = true;
        }
        ui.horizontal(|ui| {
            ui.add_sized([label_width, 24.0], egui::Label::new("World Water Level"));
            if ui
                .add(egui::DragValue::new(&mut world.terrain.config.plains.water_level).speed(0.25))
                .changed()
            {
                world.terrain.mark_full_render_dirty();
                editor.needs_save = true;
            }
        });
        if water_material_picker(ui,project_path.as_deref(),&mut world.terrain.config.water_material_asset,0) {
            editor.needs_save=true;
            world.terrain.mark_full_render_dirty();
        }
        if world.terrain.config.water_material_asset.is_none() {
            if water_preset_controls(ui,&mut world.terrain.config,project_path.as_deref()) {
                editor.needs_save=true;
                world.terrain.mark_full_render_dirty();
            }
        } else {
            ui.small("Water presets are available for project water. Detach the Water Material to use them.");
        }
        let original = world.terrain.config.water;
        if world.terrain.config.water_material_asset.is_none() {
            crate::editor::asset_management::material_editor::draw_water_parameters(
                ui,
                &mut world.terrain.config.water,
            );
        }
        if world.terrain.config.water != original {
            editor.needs_save = true;
        }
        if water_before
            != super::water_spline::WaterEditSnapshot::capture(
                &world.terrain.config,
                editor.water_selected_body,
            )
        {
            editor.push_water_undo(water_before);
        }
    });
    ui.collapsing("Materials", |ui| {
        super::terrain_material_ui::draw_layers(
            ui,
            editor,
            &mut world.terrain,
            project_path.as_deref(),
        );
        ui.add_space(12.0);
        if let Some(error) = &editor.terrain_generation_error {
            ui.colored_label(
                egui::Color32::from_rgb(235, 95, 95),
                format!("Terrain generation failed: {error}"),
            );
        }
    });
}
#[cfg(test)]
mod terrain_panel_layout_tests {
    use super::*;

    #[test]
    fn water_preset_values_are_in_supported_engine_ranges() {
        use crate::world::terrain::TerrainWaterSettings;
        let default = TerrainWaterSettings::default();
        for preset in WaterPresetKind::BUILTIN {
            for with_underwater in [false, true] {
                let look = preset.apply(default, None, true, with_underwater).unwrap();
                assert!(look.validate().is_ok(), "{:?} invalid", preset);
            }
        }
    }
    #[test]
    fn water_preset_does_not_touch_creator_direction_or_underwater_by_default() {
        use crate::world::terrain::TerrainWaterSettings;
        let mut base = TerrainWaterSettings::default();
        base.wave_direction_degrees = -220.0;
        base.underwater_fog_color = [0.27, 0.31, 0.41];
        base.underwater_fog_density = 0.12;
        base.underwater_opacity = 0.93;
        let updated = WaterPresetKind::Storm
            .apply(base, None, false, false)
            .unwrap();
        assert_eq!(updated.wave_direction_degrees, -220.0);
        assert_eq!(updated.underwater_fog_color, base.underwater_fog_color);
        assert_eq!(updated.underwater_fog_density, base.underwater_fog_density);
        assert_eq!(updated.underwater_opacity, base.underwater_opacity);
        assert_ne!(updated.wave_height, base.wave_height);
    }
    #[test]
    fn project_water_custom_preset_survives_serde_and_undo_snapshot() {
        use crate::world::terrain::TerrainConfig;
        let mut config = TerrainConfig::default();
        assert!(!config.infinite_ocean);
        assert!(config.water_custom_preset.is_none());
        config.water.wave_height = 2.3;
        config.water_custom_preset = Some(config.water);
        config.infinite_ocean = true;
        let serialized = serde_json::to_string(&config).unwrap();
        let restored: TerrainConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored.water_custom_preset, config.water_custom_preset);
        assert!(restored.infinite_ocean);
        assert_eq!(restored.water, config.water);
        let mut old_json = serde_json::to_value(&config).unwrap();
        let object = old_json.as_object_mut().unwrap();
        object.remove("infinite_ocean");
        object.remove("water_custom_preset");
        let legacy: TerrainConfig = serde_json::from_value(old_json).unwrap();
        assert!(!legacy.infinite_ocean);
        assert!(legacy.water_custom_preset.is_none());
        let snapshot = super::super::water_spline::WaterEditSnapshot::capture(&config, None);
        let mut world = World::new();
        world.terrain.config = config;
        let mut editor = Editor::new();
        editor.push_water_undo(snapshot);
        world.terrain.config.water_custom_preset = None;
        world.terrain.config.infinite_ocean = false;
        assert!(editor.water_undo_redo(&mut world, false));
        assert!(world.terrain.config.infinite_ocean);
        assert!(world.terrain.config.water_custom_preset.is_some());
    }
    #[test]
    fn terrain_controls_fit_side_panels_without_expanding_into_viewport() {
        for available in [240.0, 290.0, 350.0, 420.0] {
            let label = terrain_panel_label_width(available);
            assert!(
                label <= available * 0.5,
                "label expanded panel at {available}"
            );
            assert!(
                available - label >= 120.0,
                "no room for terrain control at {available}"
            );
        }
    }

    #[test]
    fn terrain_side_panel_keeps_its_paint_clip_inside_viewport_boundary() {
        for width in [240.0_f32, 290.0, 420.0] {
            let ctx = egui::Context::default();
            let mut editor = Editor::new();
            let mut world = World::new();
            let mut panel_edge = 0.0_f32;
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::left("terrain_side_panel_test")
                        .resizable(true)
                        .default_width(width)
                        .min_width(240.0)
                        .max_width(420.0)
                        .show(ctx, |ui| {
                            panel_edge = ui.max_rect().right();
                            super::super::project_explorer::clip_project_panel_to_bounds(ui);
                            egui::ScrollArea::vertical()
                                .id_salt("terrain_panel_test_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    draw_terrain_settings(ui, &mut editor, &mut world, &None);
                                });
                        });
                    assert!(
                        ctx.available_rect().left() >= panel_edge - 1.0,
                        "Terrain tab consumed viewport at width {width}"
                    );
                },
            );
            assert!(panel_edge > 200.0);
            let mut saw_terrain_label = false;
            for painted in &output.shapes {
                if let egui::Shape::Text(text) = &painted.shape {
                    if text.galley.job.text == "Generate" {
                        saw_terrain_label = true;
                        assert!(
                            painted.clip_rect.right() <= panel_edge + 1.0,
                            "Terrain text paint leaked across side-panel edge at {width}"
                        );
                    }
                }
            }
            assert!(saw_terrain_label);
        }
    }
}

fn draw_lighting_settings(ui: &mut egui::Ui, renderer: &Renderer) {
    let mut requested = renderer.lighting_state().requested;
    let mut changed = false;

    settings_page_header(
        ui,
        "LIGHTING",
        "Lighting pipeline, global illumination, shadows, scene queries, and diagnostics.",
    );
    settings_section(ui, "PIPELINE");
    changed |= editable_settings_combo(
        ui,
        "settings_lighting_method",
        "Lighting Method",
        &mut requested.lighting_method,
        LightingMethod::ALL,
        |value| match value {
            LightingMethod::Legacy => "Legacy",
            LightingMethod::TwoLevelRadianceCache => "Two-Level Radiance Cache (BETA)",
            LightingMethod::SplitRadianceCascades => "Split Radiance Cascades (BETA)",
        },
    );
    changed |= editable_settings_combo(
        ui,
        "settings_shadow_method",
        "Shadow Method",
        &mut requested.shadow_method,
        ShadowMethod::ALL,
        |value| match value {
            ShadowMethod::Off => "Off",
            ShadowMethod::LegacyShadowMap => "Legacy Shadow Map",
            ShadowMethod::CachedDirectionalClipmaps => "Cached Directional Clipmaps (BETA)",
        },
    );
    changed |= editable_settings_combo(
        ui,
        "settings_lighting_quality",
        "Lighting Quality (BETA)",
        &mut requested.lighting_quality,
        LightingQuality::ALL,
        LightingQuality::label,
    );

    settings_section(ui, "GLOBAL ILLUMINATION");
    let mut indirect_diffuse = requested.indirect_diffuse == IndirectDiffuse::On;

    if editable_settings_toggle(ui, "Indirect Diffuse (BETA)", &mut indirect_diffuse) {
        requested.indirect_diffuse = if indirect_diffuse {
            IndirectDiffuse::On
        } else {
            IndirectDiffuse::Off
        };
        changed = true;
    }

    changed |= editable_settings_combo(
        ui,
        "settings_bounce_lighting",
        "Bounce Lighting (BETA)",
        &mut requested.bounce_lighting,
        BounceLighting::ALL,
        BounceLighting::label,
    );

    let mut temporal_reuse = requested.temporal_reuse == TemporalReuse::On;

    if editable_settings_toggle(ui, "Temporal Reuse (BETA)", &mut temporal_reuse) {
        requested.temporal_reuse = if temporal_reuse {
            TemporalReuse::On
        } else {
            TemporalReuse::Off
        };
        changed = true;
    }

    changed |= editable_settings_combo(
        ui,
        "settings_dynamic_geometry_gi",
        "Dynamic Geometry GI",
        &mut requested.dynamic_geometry_gi,
        DynamicGeometryGi::ALL,
        |value| match value {
            DynamicGeometryGi::Off => "Off",
            DynamicGeometryGi::BodiesOnly => "Bodies Only (BETA)",
            DynamicGeometryGi::BodiesActorsCharacters => "Bodies + Actors + Characters (BETA)",
        },
    );

    changed |= editable_settings_combo(
        ui,
        "settings_gi_resolution",
        "GI Resolution (BETA)",
        &mut requested.gi_resolution,
        GiResolution::ALL,
        GiResolution::label,
    );

    settings_section(ui, "SCENE QUERY & CACHE");
    changed |= editable_settings_combo(
        ui,
        "settings_scene_query_backend",
        "Scene Query Backend (BETA)",
        &mut requested.scene_query_backend,
        SceneQueryBackend::ALL,
        SceneQueryBackend::label,
    );

    changed |= editable_settings_combo(
        ui,
        "settings_cache_update_rate",
        "Cache Update Rate (BETA)",
        &mut requested.cache_update_rate,
        CacheUpdateRate::ALL,
        CacheUpdateRate::label,
    );

    changed |= editable_settings_combo(
        ui,
        "settings_denoising",
        "Denoising",
        &mut requested.denoising,
        Denoising::ALL,
        |value| match value {
            Denoising::Off => "Off",
            Denoising::Spatial => "Spatial (BETA)",
            Denoising::SpatialTemporal => "Spatial + Temporal (BETA)",
        },
    );

    settings_section(ui, "DIAGNOSTICS");
    changed |= editable_settings_combo(
        ui,
        "settings_lighting_debug_view",
        "Debug View",
        &mut requested.debug_view,
        DebugView::ALL,
        |value| match value {
            DebugView::FinalLighting => "Final Lighting",
            DebugView::DirectOnly => "Direct Only",
            DebugView::IndirectOnly => "Indirect Only (BETA)",
            DebugView::Albedo => "Albedo",
            DebugView::Normals => "Normals",
            DebugView::Depth => "Depth",
            DebugView::ScreenProbes => "Screen Probes (BETA)",
            DebugView::WorldCache => "World Cache (BETA)",
            DebugView::RayHits => "Ray Hits (BETA)",
            DebugView::CacheInvalidations => "Cache Invalidations (BETA)",
        },
    );

    changed |= editable_settings_combo(
        ui,
        "settings_lighting_performance",
        "Performance Display",
        &mut requested.performance_display,
        PerformanceDisplay::ALL,
        |value| match value {
            PerformanceDisplay::Off => "Off",
            PerformanceDisplay::FrameTime => "Frame Time",
            PerformanceDisplay::PassTimings => "Pass Timings (BETA)",
            PerformanceDisplay::RaysAndCacheStatistics => "Rays and Cache Statistics (BETA)",
        },
    );

    changed |= editable_settings_combo(
        ui,
        "settings_lighting_apply",
        "Apply Behavior",
        &mut requested.apply_behavior,
        ApplyBehavior::ALL,
        ApplyBehavior::label,
    );

    if changed {
        renderer.request_lighting_settings(requested);
        let _ = renderer.apply_requested_lighting_settings();
    }

    let state = renderer.lighting_state();
    settings_section(ui, "STATE");
    ui.label(format!(
        "Active: {} / {} / {}",
        state.active.lighting_method.label(),
        state.active.shadow_method.label(),
        state.active.debug_view.label()
    ));
    if state.restart_pending {
        ui.colored_label(
            egui::Color32::YELLOW,
            "Changing lighting settings requires a restart.",
        );
    }
    if let Some(error) = state.last_error {
        ui.colored_label(egui::Color32::RED, error);
    }
    let stats = renderer.lighting_performance_snapshot();
    if state.active.lighting_method != LightingMethod::Legacy {
        ui.label(format!(
            "Cache: {} probes \u{00b7} {} updated \u{00b7} {} rays \u{00b7} {:.2} ms CPU update",
            stats.cache_probes, stats.updated_probes, stats.traced_rays, stats.cache_update_cpu_ms
        ));
        ui.label(format!(
            "GI scene: {} static voxels \u{00b7} {} body AABBs \u{00b7} {:.2} MiB packed payload",
            stats.scene_voxels,
            stats.dynamic_aabbs,
            stats.scene_payload_bytes as f64 / (1024.0 * 1024.0)
        ));
    }
}

fn draw_lighting_performance_overlay(
    ctx: &egui::Context,
    renderer: &Renderer,
    viewport_rect: egui::Rect,
) {
    let settings = renderer.active_lighting_settings();

    if settings.lighting_method == LightingMethod::Legacy
        && settings.performance_display == PerformanceDisplay::Off
    {
        return;
    }

    if !viewport_rect.is_finite() || !viewport_rect.is_positive() {
        return;
    }

    const MARGIN: f32 = 12.0;
    const MAX_WIDTH: f32 = 340.0;

    let overlay_width = MAX_WIDTH.min((viewport_rect.width() - MARGIN * 2.0).max(1.0));

    let overlay_position = egui::pos2(
        viewport_rect.right() - overlay_width - MARGIN,
        viewport_rect.top() + MARGIN,
    );

    egui::Area::new(egui::Id::new("lighting_beta_diagnostics"))
        .fixed_pos(overlay_position)
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_width(overlay_width);

            egui::Frame::none()
                .fill(egui::Color32::from_rgba_unmultiplied(8, 10, 13, 220))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(228, 91, 36)))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "BETA LIGHTING \u{00b7} {}",
                            settings.lighting_method.label()
                        ))
                        .monospace()
                        .strong()
                        .color(egui::Color32::from_rgb(244, 145, 90)),
                    );

                    match settings.performance_display {
                        PerformanceDisplay::Off => {}

                        PerformanceDisplay::FrameTime => {
                            let seconds = ctx.input(|input| input.stable_dt).max(0.000_001);

                            ui.monospace(format!(
                                "UI frame {:.2} ms \u{00b7} {:.1} FPS",
                                seconds * 1000.0,
                                1.0 / seconds
                            ));
                        }

                        PerformanceDisplay::PassTimings => {
                            ui.monospace("GPU pass timings \u{00b7} collection pending");
                        }

                        PerformanceDisplay::RaysAndCacheStatistics => {
                            let stats = renderer.lighting_performance_snapshot();

                            ui.monospace(format!(
                                "Rays {} \u{00b7} probes {}/{}",
                                stats.traced_rays, stats.updated_probes, stats.cache_probes
                            ));

                            ui.monospace(format!(
                                "GI update {:.2} ms CPU \u{00b7} voxels {} \u{00b7} bodies {}",
                                stats.cache_update_cpu_ms, stats.scene_voxels, stats.dynamic_aabbs
                            ));
                        }
                    }
                });
        });
}

fn draw_editor_settings(
    ui: &mut egui::Ui,
    settings: &mut crate::engine::preferences::ThemeSettings,
    history: &mut History,
    script_editor: &mut crate::editor::script_editor::ScriptEditor,
    pending: &mut Vec<EngineSound>,
) {
    use crate::engine::preferences::*;

    let mut changed = false;

    settings_page_header(
        ui,
        "EDITOR",
        "Editor camera, workspace state, and authoring preferences.",
    );

    settings_section(ui, "CAMERA");

    if editable_settings_combo(
        ui,
        "settings_editor_camera_speed",
        "Editor Camera Speed",
        &mut settings.camera_speed_preset,
        CameraSpeedPreset::ALL,
        CameraSpeedPreset::label,
    ) {
        if settings.camera_speed_preset != CameraSpeedPreset::Custom {
            settings.camera_speed_custom = settings.camera_speed_preset.default_factor();
        }
        pending.push(EngineSound::Select);
        changed = true;
    }

    if settings.camera_speed_preset == CameraSpeedPreset::Custom {
        ui.horizontal(|ui| {
            ui.add_sized(
                [SETTINGS_LABEL_WIDTH, 24.0],
                egui::Label::new("Custom Speed Multiplier"),
            );
            if ui
                .add_sized(
                    [SETTINGS_CONTROL_WIDTH, 24.0],
                    egui::Slider::new(&mut settings.camera_speed_custom, 0.05..=3.00),
                )
                .changed()
            {
                changed = true;
            }
        });
    }

    if editable_settings_combo(
        ui,
        "settings_camera_sensitivity",
        "Camera Sensitivity",
        &mut settings.camera_sensitivity_preset,
        CameraSensitivityPreset::ALL,
        CameraSensitivityPreset::label,
    ) {
        if settings.camera_sensitivity_preset != CameraSensitivityPreset::Custom {
            settings.camera_sensitivity_custom =
                settings.camera_sensitivity_preset.default_factor();
        }
        pending.push(EngineSound::Select);
        changed = true;
    }

    if settings.camera_sensitivity_preset == CameraSensitivityPreset::Custom {
        ui.horizontal(|ui| {
            ui.add_sized(
                [SETTINGS_LABEL_WIDTH, 24.0],
                egui::Label::new("Custom Sensitivity Multiplier"),
            );
            if ui
                .add_sized(
                    [SETTINGS_CONTROL_WIDTH, 24.0],
                    egui::Slider::new(&mut settings.camera_sensitivity_custom, 0.10..=3.00),
                )
                .changed()
            {
                changed = true;
            }
        });
    }

    settings_section(ui, "WORKSPACE");

    if editable_settings_combo(
        ui,
        "settings_autosave",
        "Autosave (BETA)",
        &mut settings.autosave,
        AutosaveInterval::ALL,
        AutosaveInterval::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_undo_history",
        "Undo History",
        &mut settings.undo_history_limit,
        &[25usize, 50, 100, 250],
        |value| match value {
            25 => "25",
            50 => "50",
            100 => "100",
            250 => "250",
            _ => "100",
        },
    ) {
        history.set_limit(settings.undo_history_limit);
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_output_panel",
        "Output Panel",
        &mut script_editor.show_output,
        &[false, true],
        |visible| if visible { "Visible" } else { "Hidden" },
    ) {
        pending.push(EngineSound::Toggle);
    }

    if editable_settings_combo(
        ui,
        "settings_problems_panel",
        "Problems Panel",
        &mut script_editor.show_problems,
        &[false, true],
        |visible| if visible { "Visible" } else { "Hidden" },
    ) {
        pending.push(EngineSound::Toggle);
    }

    settings_section(ui, "VIEWPORT DIAGNOSTICS");

    if editable_settings_toggle(ui, "Show Navmesh", &mut settings.show_navmesh) {
        pending.push(EngineSound::Toggle);
        changed = true;
    }

    ui.small("Shows or hides the current navigation snapshot, blocked cells, and solved path in the editor viewport.");

    if editable_settings_toggle(
        ui,
        "Confirm Destructive Actions (BETA)",
        &mut settings.confirm_destructive_actions,
    ) {
        pending.push(EngineSound::Toggle);
        changed = true;
    }

    if changed {
        save_editor_preferences(settings);
    }
}

fn draw_theme_settings(
    ui: &mut egui::Ui,
    settings: &mut crate::engine::preferences::ThemeSettings,
    pending: &mut Vec<EngineSound>,
) {
    use crate::engine::preferences::*;

    let mut changed = false;

    settings_page_header(
        ui,
        "THEME",
        "Editor appearance, scaling, typography, contrast, and interface presentation.",
    );

    settings_section(ui, "APPEARANCE");

    if editable_settings_combo(
        ui,
        "settings_theme",
        "Theme",
        &mut settings.theme,
        EditorTheme::ALL,
        EditorTheme::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_accent",
        "Accent",
        &mut settings.accent,
        AccentTheme::ALL,
        AccentTheme::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_interface_scale",
        "Interface Scale",
        &mut settings.interface_scale,
        InterfaceScale::ALL,
        InterfaceScale::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    settings_section(ui, "TYPOGRAPHY");

    if editable_settings_combo(
        ui,
        "settings_font_size",
        "Font Size",
        &mut settings.font_size,
        ThemeFontSize::ALL,
        ThemeFontSize::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_monospace_size",
        "Monospace Size",
        &mut settings.monospace_size,
        ThemeFontSize::ALL,
        ThemeFontSize::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    settings_section(ui, "INTERFACE");

    if editable_settings_combo(
        ui,
        "settings_panel_rounding",
        "Panel Rounding",
        &mut settings.panel_rounding,
        PanelRounding::ALL,
        PanelRounding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_panel_contrast",
        "Panel Contrast",
        &mut settings.panel_contrast,
        PanelContrast::ALL,
        PanelContrast::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_separator_visibility",
        "Separator Visibility",
        &mut settings.separator_visibility,
        SeparatorVisibility::ALL,
        SeparatorVisibility::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_animation",
        "Animation",
        &mut settings.animation,
        UiAnimation::ALL,
        UiAnimation::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_toggle(ui, "Tooltips", &mut settings.tooltips) {
        pending.push(EngineSound::Toggle);
        changed = true;
    }

    if editable_settings_toggle(ui, "Status Bar", &mut settings.status_bar) {
        pending.push(EngineSound::Toggle);
        changed = true;
    }

    if changed {
        save_editor_preferences(settings);
    }
}

fn engine_volume_slider(ui: &mut egui::Ui, label: &str, value: &mut f32) -> bool {
    let before = *value;

    ui.horizontal(|ui| {
        ui.add_sized([SETTINGS_LABEL_WIDTH, 24.0], egui::Label::new(label));

        ui.add_sized(
            [SETTINGS_CONTROL_WIDTH, 24.0],
            egui::Slider::new(value, 0.0..=1.0).show_value(false),
        );

        ui.monospace(format!("{:.0}%", *value * 100.0));
    });

    *value != before
}

fn engine_sound_row(ui: &mut egui::Ui, sound: EngineSound, pending: &mut Vec<EngineSound>) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [SETTINGS_LABEL_WIDTH, 24.0],
            egui::Label::new(sound.label()),
        );

        ui.add_sized(
            [190.0, 24.0],
            egui::Label::new(egui::RichText::new(sound.file_name()).monospace()),
        );

        if ui.button("PREVIEW").clicked() {
            pending.push(sound);
        }
    });
}

fn draw_audio_settings(
    ui: &mut egui::Ui,
    settings: &mut EngineAudioSettings,
    pending: &mut Vec<EngineSound>,
) {
    settings_page_header(
        ui,
        "AUDIO",
        "Worldkiln editor and system sounds. Project and gameplay audio are not controlled here.",
    );

    settings_section(ui, "ENGINE AUDIO");

    if editable_settings_toggle(ui, "Engine Sounds", &mut settings.engine_sounds_enabled) {
        pending.push(EngineSound::Toggle);
    }

    engine_volume_slider(ui, "Engine Volume", &mut settings.engine_volume);

    settings_section(ui, "INTERFACE");

    if editable_settings_toggle(ui, "UI Sounds", &mut settings.ui_sounds_enabled) {
        pending.push(EngineSound::Toggle);
    }

    engine_volume_slider(ui, "UI Volume", &mut settings.ui_volume);

    for &sound in EngineSound::INTERFACE {
        engine_sound_row(ui, sound, pending);
    }

    settings_section(ui, "PROJECT");

    for &sound in EngineSound::PROJECT {
        engine_sound_row(ui, sound, pending);
    }

    settings_section(ui, "PLAY MODE");

    for &sound in EngineSound::PLAY_MODE {
        engine_sound_row(ui, sound, pending);
    }

    settings_section(ui, "BUILD");

    for &sound in EngineSound::BUILD {
        engine_sound_row(ui, sound, pending);
    }

    settings_section(ui, "NOTIFICATIONS");

    for &sound in EngineSound::NOTIFICATIONS {
        engine_sound_row(ui, sound, pending);
    }
}

fn draw_input_settings(
    ui: &mut egui::Ui,
    settings: &mut crate::engine::preferences::ThemeSettings,
    pending: &mut Vec<EngineSound>,
) {
    use crate::engine::preferences::*;

    let mut changed = false;

    settings_page_header(
        ui,
        "INPUT",
        "Editor navigation, camera, and selection input bindings.",
    );

    settings_section(ui, "CAMERA BINDINGS");

    if editable_settings_combo(
        ui,
        "settings_bind_forward",
        "Camera Forward",
        &mut settings.bind_camera_forward,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_backward",
        "Camera Backward",
        &mut settings.bind_camera_backward,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_left",
        "Camera Left",
        &mut settings.bind_camera_left,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_right",
        "Camera Right",
        &mut settings.bind_camera_right,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_up",
        "Camera Up",
        &mut settings.bind_camera_up,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_down",
        "Camera Down",
        &mut settings.bind_camera_down,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_orbit",
        "Orbit Camera",
        &mut settings.bind_orbit_camera,
        EditorMouseBinding::ALL,
        EditorMouseBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_free_look",
        "Free Look",
        &mut settings.bind_free_look,
        EditorMouseBinding::ALL,
        EditorMouseBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    settings_section(ui, "EDITOR BINDINGS");

    if editable_settings_combo(
        ui,
        "settings_bind_select",
        "Select",
        &mut settings.bind_select,
        EditorMouseBinding::ALL,
        EditorMouseBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    if editable_settings_combo(
        ui,
        "settings_bind_delete",
        "Delete",
        &mut settings.bind_delete,
        EditorKeyBinding::ALL,
        EditorKeyBinding::label,
    ) {
        pending.push(EngineSound::Select);
        changed = true;
    }

    let old_play = settings.bind_play;
    let old_tray = settings.bind_play_tray;
    if editable_settings_combo(
        ui,
        "settings_bind_play",
        "Toggle Play",
        &mut settings.bind_play,
        PlayShortcut::PLAY,
        PlayShortcut::label,
    ) {
        if settings.bind_play == settings.bind_play_tray {
            settings.bind_play_tray = old_play;
        }
        changed = true;
    }
    if editable_settings_combo(
        ui,
        "settings_bind_play_tray",
        "Show/hide bottom tray",
        &mut settings.bind_play_tray,
        PlayShortcut::TRAY,
        PlayShortcut::label,
    ) {
        if settings.bind_play_tray == settings.bind_play {
            settings.bind_play_tray = old_tray;
        }
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.add_sized(
            [SETTINGS_LABEL_WIDTH, 24.0],
            egui::Label::new("Reset Bindings"),
        );
        if ui
            .add_sized([SETTINGS_CONTROL_WIDTH, 24.0], egui::Button::new("Default"))
            .clicked()
        {
            settings.reset_input_bindings();
            pending.push(EngineSound::Click);
            changed = true;
        }
    });

    if changed {
        save_editor_preferences(settings);
    }
}

impl Editor {
    pub fn show_ui(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
        renderer: &Renderer,
    ) {
        macro_rules! timed_surface {
            ($operation:expr, $draw:expr) => {{
                let started = self
                    .development_ui_timing_enabled
                    .then(std::time::Instant::now);
                let result = $draw;
                if let Some(started) = started {
                    self.development_ui_spans
                        .push(($operation, started.elapsed()));
                }
                result
            }};
        }
        if self.mode == crate::engine::EditorMode::Editor {
            self.advance_heightfield_erosion(world, ctx);
        }
        timed_surface!(
            "asset_import_background",
            self.advance_hidden_asset_imports(ctx, project_path, renderer)
        );
        timed_surface!(
            "asset_import_progress",
            self.draw_global_import_progress(ctx)
        );
        if self.mode == crate::engine::EditorMode::Play {
            crate::renderer::effekseer::set_preview(None, false, 0);
            timed_surface!("play_bottom_tray", self.draw_shell_dock(ctx, project_path));
            self.viewport_rect = ctx.available_rect();
            self.viewport_ppp = ctx.pixels_per_point();
            return;
        }

        if self.effect_visible
            && self.effect_workspace.as_ref().is_some_and(|workspace| {
                project_path.as_deref().is_none_or(|root| {
                    crate::project::fx_assets::id_from_path(root, &workspace.path).is_none()
                })
            })
        {
            self.effect_visible = false;
        }
        let preview = self.effect_workspace.as_ref().filter(|_| {
            self.effect_visible
                && !self.asset_manager.open
                && !self.navigation.surface.is_settings()
        });
        crate::renderer::effekseer::set_preview(
            preview.map(|workspace| workspace.preview_path.as_deref().unwrap_or(&workspace.path)),
            preview.is_some_and(|workspace| workspace.playing),
            preview.map_or(0, |workspace| workspace.preview_revision),
        );
        self.observe_navigation_history();
        if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra1)) {
            self.navigate_history_back();
        } else if ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Extra2)) {
            self.navigate_history_forward();
        }
        timed_surface!("menu_bar", self.draw_menu_bar(ctx));
        if self.asset_manager.open {
            if self.ai_chat.open && self.right_panel_tab == super::RightPanelTab::Ai {
                timed_surface!(
                    "asset_browser_ai_panel",
                    self.draw_right_panel(ctx, world, project_path)
                );
            }
            timed_surface!(
                "asset_manager_browser",
                self.draw_asset_manager(ctx, world, project_path, renderer)
            );
            self.viewport_rect = egui::Rect::NOTHING;
            self.viewport_ppp = ctx.pixels_per_point();
            return;
        }
        if !self.navigation.surface.is_settings() {
            if self.ui_mode && self.shell.workspace != super::shell::Workspace::Ui {
                self.activate_shell_workspace(super::shell::Workspace::Ui);
            } else if self.show_script_workspace
                && self.shell.workspace != super::shell::Workspace::Script
            {
                self.activate_shell_workspace(super::shell::Workspace::Script);
            }
        }
        if self.navigation.surface != super::feature_nav::EditorSurface::AppSettings {
            timed_surface!("workspace_tabs", self.draw_workspace_tabs(ctx));
        }
        match self.navigation.surface {
            super::feature_nav::EditorSurface::Workspace => {}
            super::feature_nav::EditorSurface::AppSettings => {
                timed_surface!(
                    "settings_page",
                    self.draw_settings_workspace(ctx, renderer, Some(world), project_path)
                );
                self.viewport_rect = egui::Rect::NOTHING;
                self.viewport_ppp = ctx.pixels_per_point();
                return;
            }
            super::feature_nav::EditorSurface::ProjectSettings => {
                timed_surface!(
                    "project_settings_page",
                    self.draw_project_settings_workspace(ctx, world, project_path, renderer)
                );
                self.viewport_rect = egui::Rect::NOTHING;
                self.viewport_ppp = ctx.pixels_per_point();
                return;
            }
        }

        if self.effect_visible && self.effect_workspace.is_some() {
            let dock_op = if self.shell.dock_expanded {
                match self.shell.dock {
                    super::shell::DockTab::Content => "content_browser",
                    super::shell::DockTab::Output => "output_dock",
                    super::shell::DockTab::Problems => "problems_dock",
                    super::shell::DockTab::Diagnostics => "diagnostics_dock",
                }
            } else {
                "dock_collapsed"
            };
            timed_surface!(dock_op, self.draw_shell_dock(ctx, project_path));
            timed_surface!("aeofx_workspace", self.draw_aeofx_workspace(ctx));
            self.viewport_ppp = ctx.pixels_per_point();
            return;
        }
        if self.ui_mode {
            let before = self.ui_document.clone();
            timed_surface!("ui_authoring_toolbar", self.draw_ui_mode_toolbar(ctx));
            let dock_op = if self.shell.dock_expanded {
                match self.shell.dock {
                    super::shell::DockTab::Content => "content_browser",
                    super::shell::DockTab::Output => "output_dock",
                    super::shell::DockTab::Problems => "problems_dock",
                    super::shell::DockTab::Diagnostics => "diagnostics_dock",
                }
            } else {
                "dock_collapsed"
            };
            timed_surface!(dock_op, self.draw_shell_dock(ctx, project_path));
            if self.theme_settings.status_bar {
                timed_surface!("ui_status_bar", self.draw_status_bar(ctx));
            }
            if self.shell.explorer_visible {
                timed_surface!(
                    "ui_explorer_panel",
                    self.draw_ui_left_panel(ctx, project_path)
                );
            }
            if self.shell.properties_visible {
                if self.ai_chat.open && self.right_panel_tab == super::RightPanelTab::Ai {
                    timed_surface!(
                        "ui_ai_panel",
                        self.draw_right_panel(ctx, world, project_path)
                    );
                } else {
                    timed_surface!(
                        "ui_properties_panel",
                        self.draw_ui_authoring_properties(ctx, project_path)
                    );
                }
            }

            self.viewport_rect = ctx.available_rect();
            self.viewport_ppp = ctx.pixels_per_point();

            timed_surface!("ui_canvas", self.draw_ui_canvas(ctx, project_path));
            if self.ui_media_picker.is_some() {
                timed_surface!(
                    "ui_media_picker",
                    self.draw_ui_media_picker(ctx, project_path)
                );
            }
            let history = self
                .ui_document_history
                .entry(self.ui_workspace.active_document_id)
                .or_default();
            let changed =
                serde_json::to_vec(&before).ok() != serde_json::to_vec(&self.ui_document).ok();
            if changed {
                if history.edit_start.is_none() {
                    history.edit_start = Some(before);
                }
                history.redo.clear();
                history.last_change_at = ctx.input(|input| input.time);
            }
            let idle = ctx.input(|input| input.time - history.last_change_at >= 0.45);
            let commit = !ctx.input(|input| input.pointer.any_down())
                && (!ctx.wants_keyboard_input() || idle);
            if commit {
                if let Some(start) = history.edit_start.take() {
                    history.undo.push(start);
                    if history.undo.len() > 100 {
                        history.undo.remove(0);
                    }
                    history.last_change_at = 0.0;
                }
            }
            return;
        }

        timed_surface!(
            "tool_ribbon",
            self.draw_shell_ribbon(ctx, world, project_path)
        );

        if !self.last_show_script_workspace && self.show_script_workspace {
            self.script_editor.refresh_scripts(project_path);
        }

        self.last_show_script_workspace = self.show_script_workspace;

        let dock_op = if self.shell.dock_expanded {
            match self.shell.dock {
                super::shell::DockTab::Content => "content_browser",
                super::shell::DockTab::Output => "output_dock",
                super::shell::DockTab::Problems => "problems_dock",
                super::shell::DockTab::Diagnostics => "diagnostics_dock",
            }
        } else {
            "dock_collapsed"
        };
        timed_surface!(dock_op, self.draw_shell_dock(ctx, project_path));
        if self.theme_settings.status_bar {
            timed_surface!("status_bar", self.draw_status_bar(ctx));
        }
        if self.shell.explorer_visible {
            let left_op = match self.left_panel_tab {
                super::LeftPanelTab::Project => "project_explorer_panel",
                super::LeftPanelTab::Terrain => "terrain_cells_panel",
            };
            timed_surface!(left_op, self.draw_left_panel(ctx, world, project_path));
        }

        if self.show_script_workspace {
            if self.shell.properties_visible
                && self.ai_chat.open
                && self.right_panel_tab == super::RightPanelTab::Ai
            {
                timed_surface!(
                    "script_ai_panel",
                    self.draw_right_panel(ctx, world, project_path)
                );
            }
            timed_surface!(
                "script_editor_panel",
                self.script_editor.show_ui(ctx, project_path, world)
            );

            self.viewport_rect = egui::Rect::NOTHING;
            self.viewport_ppp = ctx.pixels_per_point();

            return;
        }

        if self.shell.properties_visible {
            let right_op = match self.right_panel_tab {
                super::RightPanelTab::Inspector => "properties_panel",
                super::RightPanelTab::Navigation => "navigation_panel",
                super::RightPanelTab::Landscaping => "landscaping_panel",
                super::RightPanelTab::Ai => "ai_panel",
            };
            timed_surface!(right_op, self.draw_right_panel(ctx, world, project_path));
        }
        if self.show_clear_confirmation {
            timed_surface!("clear_confirmation_dialog", self.draw_dialogs(ctx, world));
        }
        if self.active_color_target.is_some() {
            timed_surface!("color_picker", self.render_active_color_picker(ctx, world));
        }

        self.viewport_rect = ctx.available_rect();
        self.viewport_ppp = ctx.pixels_per_point();

        timed_surface!(
            "lighting_performance_overlay",
            draw_lighting_performance_overlay(ctx, renderer, self.viewport_rect)
        );
        timed_surface!(
            "actor_transform_gizmo",
            self.draw_actor_transform_gizmo(ctx, world)
        );
        timed_surface!(
            "water_spline_overlay",
            self.draw_water_spline_overlay(ctx, world)
        );
        timed_surface!(
            "viewport_box_selection",
            self.draw_box_selection_overlay(ctx)
        );
        timed_surface!(
            "actor_content_drop",
            self.handle_actor_content_drop(ctx, world, project_path)
        );
        timed_surface!(
            "fx_content_drop",
            self.handle_fx_content_drop(ctx, world, project_path)
        );
    }

    pub(crate) fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.add_space(6.0);
            egui::menu::bar(ui, |ui| {
                ui.add_space(8.0);
                let can_back = !self.navigation.previous.is_empty();
                let can_forward = !self.navigation.next.is_empty();
                let back_tip = if can_back {
                    "Go back to the previous editor location (Mouse Back)"
                } else {
                    "No previous editor location"
                };
                let forward_tip = if can_forward {
                    "Go forward to the next editor location (Mouse Forward)"
                } else {
                    "No forward editor location"
                };
                if ui
                    .add_enabled(can_back, egui::Button::new("Back"))
                    .on_hover_text(back_tip)
                    .clicked()
                {
                    self.navigate_history_back();
                }
                if ui
                    .add_enabled(can_forward, egui::Button::new("Forward"))
                    .on_hover_text(forward_tip)
                    .clicked()
                {
                    self.navigate_history_forward();
                }
                let menu_start = (ui.max_rect().width() * 0.43).clamp(160.0, 790.0);
                let gap = (menu_start - ui.cursor().left()).max(16.0);
                ui.add_space(gap);
                ui.menu_button("File", |ui| {
                    let menu_started = self
                        .development_ui_timing_enabled
                        .then(std::time::Instant::now);
                    if ui.button("Save").clicked() {
                        self.navigate_to(super::feature_nav::Feature::Save);
                        self.queue_engine_sound(EngineSound::Confirm);
                        ui.close_menu();
                    }

                    if ui.button("Build Game").clicked() {
                        self.navigate_to(super::feature_nav::Feature::BuildGame);
                        ui.close_menu();
                    }

                    ui.separator();

                    if ui.button("Exit to Home").clicked() {
                        self.navigate_to(super::feature_nav::Feature::ExitToHome);
                        ui.close_menu();
                    }
                    if let Some(started) = menu_started {
                        self.development_ui_spans
                            .push(("menu_file_open", started.elapsed()));
                    }
                });

                ui.menu_button("Edit", |ui| {
                    let menu_started = self
                        .development_ui_timing_enabled
                        .then(std::time::Instant::now);
                    if ui.button("Project Settings").clicked() {
                        self.navigate_to(super::feature_nav::Feature::ProjectSettings);
                        ui.close_menu();
                    }
                    ui.separator();
                    if self.ui_mode {
                        if ui.button("Delete Selected UI").clicked() {
                            if let Some(id) = self.ui_selected_node.take() {
                                if self.ui_document.delete_node(id).is_ok() {
                                    self.needs_save = true;
                                }
                            }

                            ui.close_menu();
                        }
                    } else if ui.button("Clear Grid").clicked() {
                        if self.theme_settings.confirm_destructive_actions {
                            self.show_clear_confirmation = true;
                            self.queue_engine_sound(EngineSound::Warning);
                        } else {
                            self.needs_clear_world = true;
                        }
                        ui.close_menu();
                    }
                    if let Some(started) = menu_started {
                        self.development_ui_spans
                            .push(("menu_edit_open", started.elapsed()));
                    }
                });

                ui.menu_button("View", |ui| {
                    let menu_started = self
                        .development_ui_timing_enabled
                        .then(std::time::Instant::now);
                    use super::feature_nav::Feature;
                    for (feature, label) in [
                        (Feature::Explorer, "Project Explorer"),
                        (Feature::Properties, "Properties"),
                        (Feature::ContentBrowser, "Content Browser"),
                        (Feature::AssetManager, "Asset Manager"),
                        (Feature::Output, "Output Log"),
                        (Feature::Problems, "Problems & Diagnostics"),
                    ] {
                        if ui.button(label).clicked() {
                            self.navigate_to(feature);
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Reset layout").clicked() {
                        self.shell = super::shell::ShellState::default();
                        self.show_properties_window = true;
                        self.left_panel_tab = super::LeftPanelTab::Project;
                        self.right_panel_tab = super::RightPanelTab::Inspector;
                        self.shell.save();
                        ui.close_menu();
                    }
                    if let Some(started) = menu_started {
                        self.development_ui_spans
                            .push(("menu_view_open", started.elapsed()));
                    }
                });
                let feature_menu_started = self
                    .development_ui_timing_enabled
                    .then(std::time::Instant::now);
                self.draw_feature_directory(ui);
                if let Some(started) = feature_menu_started {
                    self.development_ui_spans
                        .push(("menu_feature_directory_control", started.elapsed()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 11.0;
                    ui.spacing_mut().button_padding = egui::vec2(7.0, 5.0);
                    if ui.button("AI").on_hover_text("AI Assistant").clicked() {
                        self.navigate_to(super::feature_nav::Feature::AiAssistant);
                    }
                    if ui.button("AM").on_hover_text("Asset Manager").clicked() {
                        self.navigate_to(super::feature_nav::Feature::AssetManager);
                    }
                    if ui
                        .button("Play")
                        .on_hover_text(format!(
                            "Play ({})",
                            self.theme_settings.play_bindings().0.label()
                        ))
                        .clicked()
                    {
                        self.navigate_to(super::feature_nav::Feature::Play);
                    }
                });
            });
        });
    }

    pub(crate) fn draw_tool_bar(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<std::path::PathBuf>,
    ) {
        egui::TopBottomPanel::top("tool_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            tools::draw_tool_bar(ui, self, project_path);
            ui.add_space(2.0);
        });
    }

    pub(crate) fn draw_ui_mode_toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("ui_mode_toolbar").show(ctx, |ui| {
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("UI AUTHORING")
                        .monospace()
                        .strong()
                        .color(ui.visuals().selection.bg_fill),
                );

                ui.separator();

                ui.label(egui::RichText::new(self.ui_workspace.active_name()).strong());

                if let Some(document) = self
                    .ui_workspace
                    .document(self.ui_workspace.active_document_id)
                {
                    ui.label(
                        egui::RichText::new(format!("UI ID: {}", document.key))
                            .monospace()
                            .small()
                            .color(egui::Color32::from_gray(145)),
                    );
                }

                ui.separator();

                let res_text = if self.ui_preview_live {
                    "Viewport: Game Window".to_string()
                } else {
                    match self.ui_preview_resolution {
                        super::UiPreviewResolution::Res1280x720 => "1280 x 720 (HD)".to_string(),
                        super::UiPreviewResolution::Res1600x900 => "1600 x 900 (HD+)".to_string(),
                        super::UiPreviewResolution::Res1920x1080 => "1920 x 1080 (FHD)".to_string(),
                        super::UiPreviewResolution::Custom(w, h) => {
                            format!("Custom ({:.0} x {:.0})", w, h)
                        }
                    }
                };
                let old_live = self.ui_preview_live;
                let old_resolution = self.ui_preview_resolution;

                egui::ComboBox::from_id_source("ui_preview_res_picker")
                    .selected_text(res_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.ui_preview_live, true, "Game Window");
                        for (resolution, name) in [
                            (super::UiPreviewResolution::Res1280x720, "1280 x 720 (HD)"),
                            (super::UiPreviewResolution::Res1600x900, "1600 x 900 (HD+)"),
                            (
                                super::UiPreviewResolution::Res1920x1080,
                                "1920 x 1080 (FHD)",
                            ),
                            (super::UiPreviewResolution::Custom(1366.0, 768.0), "Custom"),
                        ] {
                            if ui
                                .selectable_label(
                                    !self.ui_preview_live
                                        && self.ui_preview_resolution == resolution,
                                    name,
                                )
                                .clicked()
                            {
                                self.ui_preview_live = false;
                                self.ui_preview_resolution = resolution;
                            }
                        }
                    });
                if self.ui_preview_live != old_live || self.ui_preview_resolution != old_resolution
                {
                    self.ui_preview_pan = egui::Vec2::ZERO;
                }

                egui::ComboBox::from_id_source("ui_preview_fit_picker")
                    .selected_text(if self.ui_preview_fit {
                        "Display: Fit"
                    } else {
                        "Display: 100%"
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.ui_preview_fit, true, "Fit");
                        ui.selectable_value(
                            &mut self.ui_preview_fit,
                            false,
                            "100% (middle-drag to pan)",
                        );
                    });

                if !self.ui_preview_live {
                    if let super::UiPreviewResolution::Custom(width, height) =
                        &mut self.ui_preview_resolution
                    {
                        ui.add(
                            egui::DragValue::new(width)
                                .speed(1.0)
                                .range(320.0..=7680.0)
                                .prefix("W "),
                        );
                        ui.add(
                            egui::DragValue::new(height)
                                .speed(1.0)
                                .range(240.0..=4320.0)
                                .prefix("H "),
                        );
                    }
                }

                ui.checkbox(&mut self.ui_show_safe_area, "Safe Area");

                ui.separator();

                let history = self
                    .ui_document_history
                    .entry(self.ui_workspace.active_document_id)
                    .or_default();
                let can_undo = !history.undo.is_empty();
                let can_redo = !history.redo.is_empty();
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .clicked()
                {
                    self.undo_ui_document();
                }
                if ui
                    .add_enabled(can_redo, egui::Button::new("Redo"))
                    .clicked()
                {
                    self.redo_ui_document();
                }

                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("EXIT UI MODE").clicked() {
                        self.exit_ui_mode();
                    }

                    if ui.button("SAVE").clicked() {
                        self.needs_save = true;
                    }
                });
            });

            ui.add_space(2.0);
        });
    }

    pub(crate) fn undo_ui_document(&mut self) {
        let document_id = self.ui_workspace.active_document_id;
        let document = self
            .ui_document_history
            .entry(document_id)
            .or_default()
            .undo
            .pop();
        if let Some(document) = document {
            let selected_node = self.ui_selected_node;
            let current = std::mem::replace(&mut self.ui_document, document);
            self.ui_document_history
                .entry(document_id)
                .or_default()
                .redo
                .push(current);
            self.ui_selected_node =
                selected_node.filter(|id| self.ui_document.nodes.iter().any(|node| node.id == *id));
            self.needs_save = true;
            self.sync_active_ui_document();
        }
    }

    pub(crate) fn redo_ui_document(&mut self) {
        let document_id = self.ui_workspace.active_document_id;
        let document = self
            .ui_document_history
            .entry(document_id)
            .or_default()
            .redo
            .pop();
        if let Some(document) = document {
            let selected_node = self.ui_selected_node;
            let current = std::mem::replace(&mut self.ui_document, document);
            self.ui_document_history
                .entry(document_id)
                .or_default()
                .undo
                .push(current);
            self.ui_selected_node =
                selected_node.filter(|id| self.ui_document.nodes.iter().any(|node| node.id == *id));
            self.needs_save = true;
            self.sync_active_ui_document();
        }
    }

    pub(crate) fn draw_status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.ui_mode {
                    ui.label("UI Mode");

                    ui.separator();

                    if let Some(id) = self.ui_selected_node {
                        ui.label(format!("Selected: UI {}", id));
                    } else {
                        ui.label("Selected: \u{2014}");
                    }
                } else {
                    ui.label(format!(
                        "Anchor: ({}, {}, {})",
                        self.anchor.x, self.anchor.y, self.anchor.z
                    ));

                    ui.separator();

                    let hover_text = if let Some(h) = self.hovered_cell {
                        format!("Hover: ({}, {}, {})", h.x, h.y, h.z)
                    } else {
                        "Hover: \u{2014}".to_string()
                    };

                    ui.label(hover_text);
                }
            });
        });
    }

    pub(crate) fn draw_dialogs(&mut self, ctx: &egui::Context, _world: &mut crate::world::World) {
        if self.show_clear_confirmation {
            egui::Window::new("Confirm Clear")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Are you sure you want to erase the entire built scene?");
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        if ui.button("Yes, Clear Everything").clicked() {
                            self.needs_clear_world = true;
                            self.show_clear_confirmation = false;
                        }

                        if ui.button("Cancel").clicked() {
                            self.show_clear_confirmation = false;
                        }
                    });
                });
        }
    }

    pub(crate) fn render_active_color_picker(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
    ) {
        if let Some(target) = self.active_color_target {
            let mut still_open = true;

            let title = match target {
                ColorTarget::Build => "Build Color",
                ColorTarget::PropertyColor(_) => "Block Color",
                ColorTarget::PropertyLight(_) => "Light Color",
            };

            egui::Window::new(title)
                .open(&mut still_open)
                .resizable(false)
                .collapsible(false)
                .show(ctx, |ui| {
                    match target {
                        ColorTarget::Build => {
                            let mut edit_color = [
                                self.build_template.color_rgb.x,
                                self.build_template.color_rgb.y,
                                self.build_template.color_rgb.z,
                            ];

                            if egui::color_picker::color_edit_button_rgb(ui, &mut edit_color)
                                .changed()
                            {
                                self.build_template.color_rgb = glam::Vec3::from_array(edit_color);
                            }
                        }

                        ColorTarget::PropertyColor(coord) => {
                            if let Some(cell) = world.get(coord) {
                                let mut edit_color =
                                    [cell.color_rgb.x, cell.color_rgb.y, cell.color_rgb.z];

                                if egui::color_picker::color_edit_button_rgb(ui, &mut edit_color)
                                    .changed()
                                {
                                    let new_color = glam::Vec3::from_array(edit_color);
                                    for c in self.selected_coords.clone() {
                                        if let Some(other_cell) = world.get_mut(c) {
                                            other_cell.color_rgb = new_color;
                                        }
                                        world.touch_authored_cell(
                                            c,
                                            crate::world::DirtyReason::MaterialOrOffset,
                                        );
                                    }
                                }
                            } else {
                                ui.label("Target no longer exists.");

                                if ui.button("Close").clicked() {
                                    self.active_color_target = None;
                                }
                            }
                        }

                        ColorTarget::PropertyLight(coord) => {
                            if let Some(cell) = world.get(coord) {
                                let mut edit_color =
                                    [cell.light_color.x, cell.light_color.y, cell.light_color.z];

                                if egui::color_picker::color_edit_button_rgb(ui, &mut edit_color)
                                    .changed()
                                {
                                    let new_color = glam::Vec3::from_array(edit_color);
                                    for c in self.selected_coords.clone() {
                                        if let Some(other_cell) = world.get_mut(c) {
                                            other_cell.light_color = new_color;
                                        }
                                    }
                                }
                            } else {
                                ui.label("Target no longer exists.");

                                if ui.button("Close").clicked() {
                                    self.active_color_target = None;
                                }
                            }
                        }
                    }

                    ui.add_space(8.0);

                    if ui.button("Done").clicked() {
                        self.active_color_target = None;
                    }
                });

            if !still_open {
                self.active_color_target = None;
            }
        }
    }

    pub(crate) fn draw_ui_authoring_properties(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<std::path::PathBuf>,
    ) {
        let panel_response = egui::SidePanel::right("ui_authoring_properties")
            .resizable(true)
            .default_width(self.shell.properties_width)
            .min_width(260.0)
            .max_width(520.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("worldkiln_ui_authoring_properties_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(8.0);

                        ui.label(
                            egui::RichText::new("UI PROPERTIES")
                                .monospace()
                                .strong()
                                .color(ui.visuals().selection.bg_fill),
                        );

                        ui.add_space(8.0);

                        let validation_errors = self.ui_document.validation_errors();
                        if !validation_errors.is_empty() {
                            egui::CollapsingHeader::new(
                                egui::RichText::new(format!(
                                    "VALIDATION ({})",
                                    validation_errors.len()
                                ))
                                .strong()
                                .color(egui::Color32::from_rgb(240, 100, 90)),
                            )
                            .default_open(true)
                            .show(ui, |ui| {
                                for error in validation_errors {
                                    ui.colored_label(egui::Color32::from_rgb(240, 120, 110), error);
                                }
                            });
                            ui.separator();
                        }

                        let Some(selected_id) = self.ui_selected_node else {
                            ui.label("Select a UI node.");
                            return;
                        };

                        let node_ids = self.ui_document.node_ids();
                        if self.ui_document.node(selected_id).is_none() {
                            self.ui_selected_node = None;
                            ui.label("Selected node no longer exists.");
                            return;
                        }

                        let mut changed = false;
                        let mut requested_parent: Option<Option<u32>> = None;
                        let mut delete_requested = false;

                        {
                            let Some(node) = self.ui_document.node_mut(selected_id) else {
                                return;
                            };

                            ui.label(
                                egui::RichText::new(format!(
                                    "{}  [UI {}]",
                                    node.element_type, node.id
                                ))
                                .monospace()
                                .strong(),
                            );

                            ui.add_space(4.0);

                            ui.label(
                                egui::RichText::new("IDENTITY")
                                    .monospace()
                                    .strong()
                                    .color(ui.visuals().selection.bg_fill),
                            );

                            ui.label("Display name");
                            changed |= ui.text_edit_singleline(&mut node.name).changed();

                            ui.label("Script key (unique in this document)");
                            let mut key = node.key.clone();
                            if ui.text_edit_singleline(&mut key).changed() {
                                node.key = key;
                                changed = true;
                            }
                            ui.add_space(6.0);
                            ui.separator();

                            ui.label(
                                egui::RichText::new("LAYOUT")
                                    .monospace()
                                    .strong()
                                    .color(ui.visuals().selection.bg_fill),
                            );

                            ui.label("Anchor");
                            egui::ComboBox::from_id_source("ui_anchor")
                                .selected_text(format!("{:?}", node.anchor))
                                .show_ui(ui, |ui| {
                                    for anchor in crate::engine::ui::UiAnchor::ALL {
                                        changed |= ui
                                            .selectable_value(
                                                &mut node.anchor,
                                                anchor,
                                                format!("{:?}", anchor),
                                            )
                                            .changed();
                                    }
                                });
                            if node.anchor == crate::engine::ui::UiAnchor::Stretch {
                                ui.checkbox(
                                    &mut self.ui_stretch_edges_linked,
                                    "Link all stretch margins",
                                );
                                let mut margins = [
                                    node.position[0],
                                    node.position[1],
                                    node.size[0],
                                    node.size[1],
                                ];
                                let labels = ["Left", "Top", "Right", "Bottom"];
                                for (index, label) in labels.iter().enumerate() {
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut margins[index])
                                                .speed(1.0)
                                                .range(0.0..=4096.0)
                                                .prefix(format!("{label} ")),
                                        )
                                        .changed()
                                    {
                                        if self.ui_stretch_edges_linked {
                                            let linked_margin = margins[index];
                                            margins.fill(linked_margin);
                                        }
                                        node.position = [margins[0], margins[1]];
                                        node.size = [margins[2], margins[3]];
                                        changed = true;
                                    }
                                }
                                ui.label("Margin Scale");
                                ui.horizontal(|ui| {
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.position_scale[0])
                                                .speed(0.01)
                                                .range(-4.0..=4.0)
                                                .prefix("Left "),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.position_scale[1])
                                                .speed(0.01)
                                                .range(-4.0..=4.0)
                                                .prefix("Top "),
                                        )
                                        .changed();
                                });
                                ui.horizontal(|ui| {
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.size_scale[0])
                                                .speed(0.01)
                                                .range(0.0..=4.0)
                                                .prefix("Right "),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.size_scale[1])
                                                .speed(0.01)
                                                .range(0.0..=4.0)
                                                .prefix("Bottom "),
                                        )
                                        .changed();
                                });
                            } else {
                                ui.label("Position");
                                ui.horizontal(|ui| {
                                    ui.label("Scale");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.position_scale[0])
                                                .speed(0.01)
                                                .range(-4.0..=4.0)
                                                .prefix("X "),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.position_scale[1])
                                                .speed(0.01)
                                                .range(-4.0..=4.0)
                                                .prefix("Y "),
                                        )
                                        .changed();
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Offset");
                                    ui.label("X");
                                    changed |= ui
                                        .add(egui::DragValue::new(&mut node.position[0]).speed(1.0))
                                        .changed();
                                    ui.label("Y");
                                    changed |= ui
                                        .add(egui::DragValue::new(&mut node.position[1]).speed(1.0))
                                        .changed();
                                });

                                ui.label("Size");
                                ui.horizontal(|ui| {
                                    ui.label("Scale");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.size_scale[0])
                                                .speed(0.01)
                                                .range(0.0..=4.0)
                                                .prefix("W "),
                                        )
                                        .changed();
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.size_scale[1])
                                                .speed(0.01)
                                                .range(0.0..=4.0)
                                                .prefix("H "),
                                        )
                                        .changed();
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Offset");
                                    ui.label("W");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.size[0])
                                                .speed(1.0)
                                                .range(-4096.0..=4096.0),
                                        )
                                        .changed();
                                    ui.label("H");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.size[1])
                                                .speed(1.0)
                                                .range(-4096.0..=4096.0),
                                        )
                                        .changed();
                                });
                            }

                            ui.label("Parent");
                            let current_parent = node.parent;
                            egui::ComboBox::from_id_source("ui_parent_selector")
                                .selected_text(match current_parent {
                                    Some(parent) => format!("UI {}", parent),
                                    None => "ROOT".to_string(),
                                })
                                .show_ui(ui, |ui| {
                                    if ui
                                        .selectable_label(current_parent.is_none(), "ROOT")
                                        .clicked()
                                    {
                                        requested_parent = Some(None);
                                    }

                                    for candidate in node_ids.iter().copied() {
                                        if candidate == selected_id {
                                            continue;
                                        }

                                        let selected = current_parent == Some(candidate);

                                        if ui
                                            .selectable_label(selected, format!("UI {}", candidate))
                                            .clicked()
                                        {
                                            requested_parent = Some(Some(candidate));
                                        }
                                    }
                                });

                            ui.horizontal(|ui| {
                                ui.label("Z-Index");
                                if ui
                                    .add(egui::DragValue::new(&mut node.z_index).speed(0.1))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });

                            ui.add_space(6.0);
                            ui.separator();

                            ui.label(
                                egui::RichText::new("APPEARANCE")
                                    .monospace()
                                    .strong()
                                    .color(ui.visuals().selection.bg_fill),
                            );

                            ui.horizontal(|ui| {
                                ui.label("Fill Color");
                                let mut color = node.color;
                                if ui.color_edit_button_rgba_unmultiplied(&mut color).changed() {
                                    node.color = color;
                                    changed = true;
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("Corner Radius");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut node.corner_radius)
                                            .speed(0.25)
                                            .clamp_range(0.0..=128.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label("Border Width");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut node.border_width)
                                            .speed(0.1)
                                            .clamp_range(0.0..=32.0),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            });

                            if node.border_width > 0.0 {
                                ui.horizontal(|ui| {
                                    ui.label("Border Color");
                                    let mut border_color = node.border_color;
                                    if ui
                                        .color_edit_button_rgba_unmultiplied(&mut border_color)
                                        .changed()
                                    {
                                        node.border_color = border_color;
                                        changed = true;
                                    }
                                });
                            }

                            ui.add_space(6.0);
                            ui.separator();

                            if node.element_type == "Text" || node.element_type == "Button" {
                                ui.label(
                                    egui::RichText::new("TYPOGRAPHY")
                                        .monospace()
                                        .strong()
                                        .color(ui.visuals().selection.bg_fill),
                                );

                                ui.horizontal(|ui| {
                                    ui.label("Font");
                                    egui::ComboBox::from_id_source("ui_font_family")
                                        .selected_text(format!("{:?}", node.font_family))
                                        .show_ui(ui, |ui| {
                                            for family in crate::engine::ui::UiFontFamily::ALL {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut node.font_family,
                                                        family,
                                                        format!("{:?}", family),
                                                    )
                                                    .changed();
                                            }
                                        });
                                });

                                ui.horizontal(|ui| {
                                    changed |= ui.checkbox(&mut node.bold, "Bold").changed();
                                    changed |= ui.checkbox(&mut node.italic, "Italic").changed();
                                });

                                let was_rich_text = node.rich_text;
                                if ui.checkbox(&mut node.rich_text, "Mixed Colors").changed() {
                                    if was_rich_text && !node.rich_text {
                                        node.text = crate::engine::ui::parse_ui_color_spans(
                                            &node.text,
                                            node.color,
                                        )
                                        .into_iter()
                                        .map(|span| span.text)
                                        .collect();
                                    }
                                    changed = true;
                                }

                                if node.rich_text {
                                    ui.label("Color Spans");
                                    let mut spans = crate::engine::ui::parse_ui_color_spans(
                                        &node.text,
                                        node.color,
                                    );
                                    let mut spans_changed = false;
                                    let mut remove = None;
                                    for (index, span) in spans.iter_mut().enumerate() {
                                        ui.horizontal(|ui| {
                                            spans_changed |= ui
                                                .add(
                                                    egui::TextEdit::singleline(&mut span.text)
                                                        .desired_width(140.0),
                                                )
                                                .changed();
                                            spans_changed |= ui
                                                .color_edit_button_rgba_unmultiplied(&mut span.color)
                                                .changed();
                                            if ui.button("Remove").clicked() {
                                                remove = Some(index);
                                            }
                                        });
                                    }
                                    if let Some(index) = remove {
                                        spans.remove(index);
                                        spans_changed = true;
                                    }
                                    if ui.button("+ Add Span").clicked() {
                                        spans.push(crate::engine::ui::UiColorSpan {
                                            text: String::new(),
                                            color: node.color,
                                        });
                                        spans_changed = true;
                                    }
                                    if spans_changed {
                                        node.text = crate::engine::ui::encode_ui_color_spans(&spans);
                                        changed = true;
                                    }
                                } else {
                                    ui.label("Text");
                                    if node.element_type == "Text" {
                                        changed |= ui.text_edit_multiline(&mut node.text).changed();
                                    } else {
                                        changed |= ui.text_edit_singleline(&mut node.text).changed();
                                    }
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Font Size");
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut node.font_size)
                                                .speed(0.5)
                                                .clamp_range(6.0..=120.0),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                });

                                if node.element_type == "Text"
                                    && ui.checkbox(&mut node.text_scaled, "TextScaled")
                                        .on_hover_text("Scale text to fit the node's available width and height.")
                                        .changed()
                                {
                                    changed = true;
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Align H");
                                    egui::ComboBox::from_id_source("ui_horizontal_align")
                                        .selected_text(format!("{:?}", node.horizontal_align))
                                        .show_ui(ui, |ui| {
                                            for align in crate::engine::ui::UiHorizontalAlign::ALL {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut node.horizontal_align,
                                                        align,
                                                        format!("{:?}", align),
                                                    )
                                                    .changed();
                                            }
                                        });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Align V");
                                    egui::ComboBox::from_id_source("ui_vertical_align")
                                        .selected_text(format!("{:?}", node.vertical_align))
                                        .show_ui(ui, |ui| {
                                            for align in crate::engine::ui::UiVerticalAlign::ALL {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut node.vertical_align,
                                                        align,
                                                        format!("{:?}", align),
                                                    )
                                                    .changed();
                                            }
                                        });
                                });

                                if ui.checkbox(&mut node.wrap, "Wrap Text").changed() {
                                    changed = true;
                                }

                                ui.horizontal(|ui| {
                                    ui.label("Line Spacing");
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut node.line_spacing)
                                                .speed(0.25)
                                                .clamp_range(0.0..=64.0),
                                        )
                                        .changed()
                                    {
                                        changed = true;
                                    }
                                });

                                ui.label("Padding [Left, Top, Right, Bottom]");
                                ui.horizontal(|ui| {
                                    ui.label("L");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.padding[0])
                                                .speed(0.5)
                                                .clamp_range(0.0..=256.0),
                                        )
                                        .changed();

                                    ui.label("T");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.padding[1])
                                                .speed(0.5)
                                                .clamp_range(0.0..=256.0),
                                        )
                                        .changed();
                                });
                                ui.horizontal(|ui| {
                                    ui.label("R");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.padding[2])
                                                .speed(0.5)
                                                .clamp_range(0.0..=256.0),
                                        )
                                        .changed();

                                    ui.label("B");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut node.padding[3])
                                                .speed(0.5)
                                                .clamp_range(0.0..=256.0),
                                        )
                                        .changed();
                                });

                                ui.add_space(6.0);
                                ui.separator();
                            }

                            if matches!(node.element_type.as_str(), "Image" | "Video") {
                                ui.label(
                                    egui::RichText::new("MEDIA")
                                        .monospace()
                                        .strong()
                                        .color(ui.visuals().selection.bg_fill),
                                );
                                let kind = if node.element_type == "Video" {
                                    super::asset_browser::AssetDropKind::Video
                                } else {
                                    super::asset_browser::AssetDropKind::Image
                                };
                                changed |= super::asset_browser::draw_project_asset_drop(
                                    ui, project_path.as_deref(), &mut node.source, kind,
                                );

                                ui.horizontal(|ui| {
                                    ui.label("Fit");
                                    egui::ComboBox::from_id_source("ui_media_fit")
                                        .selected_text(format!("{:?}", node.fit))
                                        .show_ui(ui, |ui| {
                                            for fit in crate::engine::ui::UiMediaFit::ALL {
                                                changed |= ui
                                                    .selectable_value(
                                                        &mut node.fit,
                                                        fit,
                                                        format!("{fit:?}"),
                                                    )
                                                    .changed();
                                            }
                                        });
                                });
                                if let Err(error) =
                                    crate::engine::ui_media::validate_ui_media_source(&node.source)
                                {
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                                }
                                if node.element_type == "Video" {
                                    changed |= ui.checkbox(&mut node.playing, "Playing").changed();
                                    changed |= ui.checkbox(&mut node.looped, "Loop").changed();
                                    ui.horizontal(|ui| {
                                        ui.label("Speed");
                                        changed |= ui
                                            .add(egui::Slider::new(
                                                &mut node.playback_rate,
                                                0.25..=4.0,
                                            ))
                                            .changed();
                                    });
                                }
                                ui.separator();
                            }

                            ui.label(
                                egui::RichText::new("INTERACTION & STATE")
                                    .monospace()
                                    .strong()
                                    .color(ui.visuals().selection.bg_fill),
                            );

                            if ui.checkbox(&mut node.visible, "Visible").changed() {
                                changed = true;
                            }

                            if ui.checkbox(&mut node.enabled, "Enabled").changed() {
                                changed = true;
                            }

                            if node.element_type == "Button" {
                                ui.add_space(6.0);
                                ui.label("On Click");

                                let current =
                                    node.events.get("on_click").cloned().unwrap_or_default();
                                let mut module_key = current.module_key;
                                let mut handler = current.handler;
                                let mut event_changed = false;

                                ui.horizontal(|ui| {
                                    ui.label("Module/script key");
                                    event_changed |=
                                        ui.text_edit_singleline(&mut module_key).changed();
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Handler");
                                    event_changed |=
                                        ui.text_edit_singleline(&mut handler).changed();
                                });

                                if event_changed {
                                    changed = true;
                                    if module_key.trim().is_empty() && handler.trim().is_empty() {
                                        node.events.remove("on_click");
                                    } else {
                                        node.events.insert(
                                            "on_click".to_string(),
                                            crate::engine::ui_authoring::AuthoredUiEventBinding {
                                                module_key,
                                                handler,
                                            },
                                        );
                                    }
                                }
                            }

                            ui.add_space(8.0);
                            ui.separator();

                            ui.label(
                                egui::RichText::new("ACTIONS")
                                    .monospace()
                                    .strong()
                                    .color(ui.visuals().selection.bg_fill),
                            );

                            ui.horizontal(|ui| {
                                if ui.button("Duplicate").clicked() {
                                    if let Ok(new_id) = self.ui_document.duplicate_node(selected_id)
                                    {
                                        self.ui_selected_node = Some(new_id);
                                        self.needs_save = true;
                                    }
                                }

                                if ui.button("Bring Forward").clicked() {
                                    self.ui_document.bring_forward(selected_id);
                                    self.needs_save = true;
                                }

                                if ui.button("Send Backward").clicked() {
                                    self.ui_document.send_backward(selected_id);
                                    self.needs_save = true;
                                }
                            });

                            ui.add_space(6.0);

                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("DELETE NODE")
                                            .monospace()
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(egui::Color32::from_rgb(120, 35, 30)),
                                )
                                .clicked()
                            {
                                delete_requested = true;
                            }
                        }

                        if let Some(parent) = requested_parent {
                            let target_size = self.ui_game_target_size(ctx);
                            match self
                                .ui_document
                                .set_parent_in(selected_id, parent, target_size)
                            {
                                Ok(()) => {
                                    changed = true;
                                }

                                Err(error) => {
                                    ui.colored_label(
                                        egui::Color32::RED,
                                        format!("Parent change failed: {}", error),
                                    );
                                }
                            }
                        }

                        if delete_requested {
                            let target_size = self.ui_game_target_size(ctx);
                            if self
                                .ui_document
                                .delete_node_in(selected_id, target_size)
                                .is_ok()
                            {
                                self.ui_selected_node = None;
                                self.needs_save = true;
                            }
                        }

                        if changed {
                            self.needs_save = true;
                            ctx.request_repaint();
                        }
                    });
            });
        if ctx.input(|i| i.pointer.any_released()) {
            let width = panel_response.response.rect.width();
            if (width - self.shell.properties_width).abs() > 2.0 {
                self.shell.properties_width = width.clamp(220.0, 520.0);
                self.shell.save();
            }
        }
    }

    pub(crate) fn draw_ui_canvas(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<std::path::PathBuf>,
    ) {
        let rect = self.viewport_rect;

        if rect.width() <= 1.0 || rect.height() <= 1.0 {
            return;
        }

        let canvas_id = egui::Id::new("ui_authoring_canvas");

        egui::Area::new(canvas_id)
            .fixed_pos(rect.min)
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());

                let interface_scale = self.theme_settings.interface_scale.factor().max(0.01);
                let preview_size = self.ui_game_target_size(ctx);
                let preview_scale = if self.ui_preview_fit {
                    (rect.width() / preview_size[0])
                        .min(rect.height() / preview_size[1])
                        .max(0.01)
                } else {
                    1.0 / interface_scale
                };
                let rendered_size = egui::vec2(
                    preview_size[0] * preview_scale,
                    preview_size[1] * preview_scale,
                );
                if self.ui_preview_fit {
                    self.ui_preview_pan = egui::Vec2::ZERO;
                } else {
                    let overflow = (rendered_size - rect.size()) * 0.5;
                    if ctx.input(|input| {
                        input.pointer.button_down(egui::PointerButton::Middle)
                            && input
                                .pointer
                                .interact_pos()
                                .is_some_and(|pos| rect.contains(pos))
                    }) {
                        self.ui_preview_pan += ctx.input(|input| input.pointer.delta());
                    }
                    self.ui_preview_pan.x = self
                        .ui_preview_pan
                        .x
                        .clamp(-overflow.x.max(0.0), overflow.x.max(0.0));
                    self.ui_preview_pan.y = self
                        .ui_preview_pan
                        .y
                        .clamp(-overflow.y.max(0.0), overflow.y.max(0.0));
                }
                let canvas_rect = egui::Rect::from_center_size(
                    rect.center() + self.ui_preview_pan,
                    rendered_size,
                );
                ui.set_clip_rect(canvas_rect.intersect(rect));

                ui.painter()
                    .rect_filled(canvas_rect, 0.0, egui::Color32::from_rgb(7, 9, 12));

                ui.painter().rect_stroke(
                    canvas_rect,
                    0.0,
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                    ),
                );

                if self.ui_show_safe_area {
                    let safe_95 = canvas_rect.shrink2(canvas_rect.size() * 0.025);
                    let safe_90 = canvas_rect.shrink2(canvas_rect.size() * 0.05);

                    ui.painter().rect_stroke(
                        safe_95,
                        0.0,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 220, 0, 100),
                        ),
                    );
                    ui.painter().rect_stroke(
                        safe_90,
                        0.0,
                        egui::Stroke::new(
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(255, 120, 0, 100),
                        ),
                    );

                    ui.painter().text(
                        safe_95.left_top() + egui::vec2(4.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        "SAFE AREA (95%)",
                        egui::FontId::monospace(10.0),
                        egui::Color32::from_rgba_unmultiplied(255, 220, 0, 160),
                    );
                }

                let center_x = canvas_rect.center().x;
                let center_y = canvas_rect.center().y;

                ui.painter().line_segment(
                    [
                        egui::pos2(center_x, canvas_rect.top()),
                        egui::pos2(center_x, canvas_rect.bottom()),
                    ],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                    ),
                );

                ui.painter().line_segment(
                    [
                        egui::pos2(canvas_rect.left(), center_y),
                        egui::pos2(canvas_rect.right(), center_y),
                    ],
                    egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                    ),
                );

                let background_response = ui.interact(
                    canvas_rect,
                    egui::Id::new("ui_authoring_background"),
                    egui::Sense::click(),
                );
                let pointer_pos = ui.input(|input| input.pointer.interact_pos());

                if let Some(asset_name) = self.ui_drag_asset.clone() {
                    if let Some(pointer) = pointer_pos {
                        if canvas_rect.contains(pointer) {
                            let size = egui::vec2(120.0, 60.0);
                            let preview = egui::Rect::from_min_size(pointer, size);

                            ui.painter().rect_filled(
                                preview,
                                4.0,
                                egui::Color32::from_rgba_unmultiplied(228, 91, 36, 70),
                            );

                            ui.painter().rect_stroke(
                                preview,
                                4.0,
                                egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
                            );

                            ui.painter().text(
                                preview.center(),
                                egui::Align2::CENTER_CENTER,
                                &asset_name,
                                egui::FontId::monospace(11.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }

                let node_ids = self.ui_document.paint_order(preview_size);
                let topmost_hit = pointer_pos.and_then(|pointer| {
                    node_ids.iter().rev().copied().find(|id| {
                        self.ui_document
                            .resolve_layout_in(*id, preview_size)
                            .ok()
                            .is_some_and(|layout| {
                                layout.visible
                                    && egui::Rect::from_min_size(
                                        egui::pos2(
                                            canvas_rect.min.x + layout.position[0] * preview_scale,
                                            canvas_rect.min.y + layout.position[1] * preview_scale,
                                        ),
                                        egui::vec2(
                                            layout.size[0].max(1.0) * preview_scale,
                                            layout.size[1].max(1.0) * preview_scale,
                                        ),
                                    )
                                    .contains(pointer)
                            })
                    })
                });

                for id in node_ids {
                    let Some(node) = self.ui_document.node(id).cloned() else {
                        continue;
                    };

                    let Ok(layout) = self.ui_document.resolve_layout_in(id, preview_size) else {
                        continue;
                    };

                    let node_position = egui::pos2(
                        canvas_rect.min.x + layout.position[0] * preview_scale,
                        canvas_rect.min.y + layout.position[1] * preview_scale,
                    );

                    let node_size = egui::vec2(
                        (layout.size[0] * preview_scale).max(1.0),
                        (layout.size[1] * preview_scale).max(1.0),
                    );

                    let node_rect = egui::Rect::from_min_size(node_position, node_size);

                    if !layout.visible {
                        continue;
                    }

                    let can_interact = topmost_hit == Some(id)
                        || self
                            .ui_drag_state
                            .as_ref()
                            .is_some_and(|drag| drag.node_id == id)
                        || self
                            .ui_resize_state
                            .as_ref()
                            .is_some_and(|drag| drag.node_id == id);
                    let node_response = ui.interact(
                        node_rect,
                        egui::Id::new("ui_authoring_node").with(id),
                        if can_interact {
                            egui::Sense::click_and_drag()
                        } else {
                            egui::Sense::hover()
                        },
                    );

                    if node_response.clicked() {
                        self.select_ui_node(id);
                    }

                    if node_response.drag_started() {
                        let pointer = pointer_pos.unwrap_or(node_rect.left_top());

                        self.select_ui_node(id);

                        self.ui_drag_state = Some(super::UiDragState {
                            node_id: id,
                            start_pointer: pointer,
                            start_position: node.position,
                            start_size: node.size,
                            start_layout_position: layout.position,
                            start_layout_size: layout.size,
                        });
                    }

                    if node_response.dragged() {
                        if let Some(drag) = self.ui_drag_state.clone() {
                            if drag.node_id == id {
                                let pointer = pointer_pos.unwrap_or(drag.start_pointer);

                                let mut dx = (pointer.x - drag.start_pointer.x) / preview_scale;
                                let mut dy = (pointer.y - drag.start_pointer.y) / preview_scale;

                                let desired_left = drag.start_layout_position[0] + dx;
                                let desired_top = drag.start_layout_position[1] + dy;
                                let mut x_targets =
                                    vec![0.0, preview_size[0] * 0.5, preview_size[0]];
                                let mut y_targets =
                                    vec![0.0, preview_size[1] * 0.5, preview_size[1]];
                                for sibling in self.ui_document.node_ids() {
                                    if sibling == id {
                                        continue;
                                    }
                                    let mut ancestor =
                                        self.ui_document.node(sibling).and_then(|node| node.parent);
                                    let mut descendant = false;
                                    for _ in 0..self.ui_document.nodes.len() {
                                        let Some(parent) = ancestor else { break };
                                        if parent == id {
                                            descendant = true;
                                            break;
                                        }
                                        ancestor = self
                                            .ui_document
                                            .node(parent)
                                            .and_then(|node| node.parent);
                                    }
                                    if descendant {
                                        continue;
                                    }
                                    if let Ok(sibling_layout) =
                                        self.ui_document.resolve_layout_in(sibling, preview_size)
                                    {
                                        x_targets.extend([
                                            sibling_layout.position[0],
                                            sibling_layout.position[0]
                                                + sibling_layout.size[0] * 0.5,
                                            sibling_layout.position[0] + sibling_layout.size[0],
                                        ]);
                                        y_targets.extend([
                                            sibling_layout.position[1],
                                            sibling_layout.position[1]
                                                + sibling_layout.size[1] * 0.5,
                                            sibling_layout.position[1] + sibling_layout.size[1],
                                        ]);
                                    }
                                }

                                let snap_distance = 8.0 / preview_scale;
                                let x_features = [
                                    desired_left,
                                    desired_left + drag.start_layout_size[0] * 0.5,
                                    desired_left + drag.start_layout_size[0],
                                ];
                                let y_features = [
                                    desired_top,
                                    desired_top + drag.start_layout_size[1] * 0.5,
                                    desired_top + drag.start_layout_size[1],
                                ];
                                let best_x = x_targets
                                    .iter()
                                    .flat_map(|target| {
                                        x_features
                                            .iter()
                                            .map(move |feature| (*target, *target - *feature))
                                    })
                                    .filter(|(_, correction)| correction.abs() <= snap_distance)
                                    .min_by(|left, right| {
                                        left.1
                                            .abs()
                                            .partial_cmp(&right.1.abs())
                                            .unwrap_or(Ordering::Equal)
                                    });
                                let best_y = y_targets
                                    .iter()
                                    .flat_map(|target| {
                                        y_features
                                            .iter()
                                            .map(move |feature| (*target, *target - *feature))
                                    })
                                    .filter(|(_, correction)| correction.abs() <= snap_distance)
                                    .min_by(|left, right| {
                                        left.1
                                            .abs()
                                            .partial_cmp(&right.1.abs())
                                            .unwrap_or(Ordering::Equal)
                                    });

                                if let Some((target, correction)) = best_x {
                                    dx += correction;
                                    let guide_x = canvas_rect.left() + target * preview_scale;
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(guide_x, canvas_rect.top()),
                                            egui::pos2(guide_x, canvas_rect.bottom()),
                                        ],
                                        egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
                                    );
                                }
                                if let Some((target, correction)) = best_y {
                                    dy += correction;
                                    let guide_y = canvas_rect.top() + target * preview_scale;
                                    ui.painter().line_segment(
                                        [
                                            egui::pos2(canvas_rect.left(), guide_y),
                                            egui::pos2(canvas_rect.right(), guide_y),
                                        ],
                                        egui::Stroke::new(1.0, ui.visuals().selection.bg_fill),
                                    );
                                }

                                if let Some(current) = self.ui_document.node_mut(id) {
                                    current.position =
                                        [drag.start_position[0] + dx, drag.start_position[1] + dy];
                                    if current.anchor == crate::engine::ui::UiAnchor::Stretch {
                                        current.size =
                                            [drag.start_size[0] - dx, drag.start_size[1] - dy];
                                    }
                                }

                                self.needs_save = true;
                                ctx.request_repaint();
                            }
                        }
                    }

                    if node_response.drag_stopped() {
                        self.ui_drag_state = None;
                    }
                    if self.ui_selected_node == Some(id) {
                        if let Some(parent_id) = node.parent {
                            if let Ok(parent_layout) =
                                self.ui_document.resolve_layout_in(parent_id, preview_size)
                            {
                                let parent_rect = egui::Rect::from_min_size(
                                    egui::pos2(
                                        canvas_rect.left()
                                            + parent_layout.position[0] * preview_scale,
                                        canvas_rect.top()
                                            + parent_layout.position[1] * preview_scale,
                                    ),
                                    egui::vec2(
                                        parent_layout.size[0] * preview_scale,
                                        parent_layout.size[1] * preview_scale,
                                    ),
                                );
                                ui.painter().rect_stroke(
                                    parent_rect,
                                    0.0,
                                    egui::Stroke::new(
                                        1.0,
                                        egui::Color32::from_rgba_unmultiplied(100, 180, 255, 150),
                                    ),
                                );
                            }
                        }

                        let parent_layout = node.parent.and_then(|parent| {
                            self.ui_document
                                .resolve_layout_in(parent, preview_size)
                                .ok()
                        });
                        let parent_position = parent_layout
                            .as_ref()
                            .map(|layout| layout.position)
                            .unwrap_or([0.0, 0.0]);
                        let parent_size = parent_layout
                            .as_ref()
                            .map(|layout| layout.size)
                            .unwrap_or(preview_size);
                        let (anchor_offset, _) = node.anchor.resolve(
                            parent_size,
                            [0.0, 0.0],
                            if node.anchor == crate::engine::ui::UiAnchor::Stretch {
                                [0.0, 0.0]
                            } else {
                                layout.size
                            },
                        );
                        let anchor_point = egui::pos2(
                            canvas_rect.left()
                                + (parent_position[0] + anchor_offset[0]) * preview_scale,
                            canvas_rect.top()
                                + (parent_position[1] + anchor_offset[1]) * preview_scale,
                        );
                        ui.painter().line_segment(
                            [anchor_point, node_rect.left_top()],
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(255, 190, 80, 150),
                            ),
                        );
                        ui.painter().circle_filled(
                            anchor_point,
                            4.0,
                            egui::Color32::from_rgb(255, 190, 80),
                        );

                        let handle = egui::Rect::from_center_size(
                            node_rect.right_bottom(),
                            egui::vec2(12.0, 12.0),
                        );
                        let resize = ui.interact(
                            handle,
                            egui::Id::new("ui_resize").with(id),
                            egui::Sense::drag(),
                        );
                        ui.painter()
                            .rect_filled(handle, 2.0, ui.visuals().selection.bg_fill);
                        if resize.hovered() || resize.dragged() {
                            ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                        if resize.drag_started() {
                            self.ui_drag_state = None;
                            self.ui_resize_state = Some(super::UiResizeState {
                                node_id: id,
                                start_pointer: pointer_pos.unwrap_or(handle.center()),
                                start_position: node.position,
                                start_size: node.size,
                            });
                        }
                        if resize.dragged() {
                            if let Some(state) = self
                                .ui_resize_state
                                .as_ref()
                                .filter(|state| state.node_id == id)
                            {
                                let pointer = pointer_pos.unwrap_or(state.start_pointer);
                                let delta = (pointer - state.start_pointer) / preview_scale;
                                if let Some(current) = self.ui_document.node_mut(id) {
                                    let stretch =
                                        current.anchor == crate::engine::ui::UiAnchor::Stretch;
                                    let direction = if stretch { -1.0 } else { 1.0 };
                                    let minimum = if stretch { 0.0 } else { 1.0 };
                                    let (old_offset, _) = current.anchor.resolve(
                                        [0.0, 0.0],
                                        [0.0, 0.0],
                                        state.start_size,
                                    );
                                    let new_size = [
                                        (state.start_size[0] + direction * delta.x).max(minimum),
                                        (state.start_size[1] + direction * delta.y).max(minimum),
                                    ];
                                    let (new_offset, _) =
                                        current.anchor.resolve([0.0, 0.0], [0.0, 0.0], new_size);
                                    current.size = new_size;
                                    current.position = [
                                        state.start_position[0] + old_offset[0] - new_offset[0],
                                        state.start_position[1] + old_offset[1] - new_offset[1],
                                    ];
                                }
                                self.needs_save = true;
                                ctx.request_repaint();
                            }
                        }
                        if resize.drag_stopped() {
                            self.ui_resize_state = None;
                        }
                    }

                    let fill = match node.element_type.as_str() {
                        "Panel" => egui::Color32::from_rgba_unmultiplied(
                            (node.color[0] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.color[1] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.color[2] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.color[3] * 255.0).clamp(0.0, 255.0) as u8,
                        ),
                        "Button" => egui::Color32::from_rgba_unmultiplied(
                            (node.background_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.background_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.background_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.background_color[3] * 255.0).clamp(0.0, 255.0) as u8,
                        ),
                        _ => egui::Color32::TRANSPARENT,
                    };

                    if fill != egui::Color32::TRANSPARENT {
                        ui.painter().rect_filled(
                            node_rect,
                            node.corner_radius * preview_scale,
                            fill,
                        );
                    }

                    if node.border_width > 0.0 {
                        let border = egui::Color32::from_rgba_unmultiplied(
                            (node.border_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.border_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.border_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.border_color[3] * 255.0).clamp(0.0, 255.0) as u8,
                        );

                        ui.painter().rect_stroke(
                            node_rect,
                            node.corner_radius * preview_scale,
                            egui::Stroke::new(node.border_width * preview_scale, border),
                        );
                    }

                    if self.ui_selected_node == Some(id) {
                        ui.painter().rect_stroke(
                            node_rect.expand(3.0),
                            node.corner_radius + 3.0,
                            egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                        );
                    }

                    match node.element_type.as_str() {
                        "Image" => {
                            crate::engine::ui_media::paint_media_frame(
                                ui.painter(),
                                ctx,
                                node_rect,
                                &node.source,
                                node.fit,
                                node.color,
                                project_path.as_deref(),
                            );
                        }
                        "Video" => {
                            crate::engine::ui_media::paint_video_frame(
                                ui.painter(),
                                ctx,
                                node_rect,
                                &node.source,
                                node.fit,
                                node.color,
                                false,
                                node.looped,
                                node.playback_rate,
                                project_path.as_deref(),
                            );
                        }
                        "Text" => {
                            let display_text = if node.text.is_empty() {
                                "TEXT"
                            } else {
                                &node.text
                            };

                            crate::engine::ui::paint_ui_text_styled(
                                ui.painter(),
                                ctx,
                                node_rect,
                                display_text,
                                node.font_size * preview_scale,
                                node.text_scaled,
                                node.horizontal_align,
                                node.vertical_align,
                                node.wrap,
                                node.padding.map(|value| value * preview_scale),
                                node.line_spacing * preview_scale,
                                egui::Color32::from_rgba_unmultiplied(
                                    (node.color[0] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[1] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[2] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[3] * 255.0).clamp(0.0, 255.0) as u8,
                                ),
                                node.font_family,
                                node.bold,
                                node.italic,
                                node.rich_text,
                            );
                        }

                        "Button" => {
                            let display_text = if node.text.is_empty() {
                                "BUTTON"
                            } else {
                                &node.text
                            };

                            crate::engine::ui::paint_ui_text_styled(
                                ui.painter(),
                                ctx,
                                node_rect,
                                display_text,
                                node.font_size * preview_scale,
                                false,
                                node.horizontal_align,
                                node.vertical_align,
                                node.wrap,
                                node.padding.map(|value| value * preview_scale),
                                node.line_spacing * preview_scale,
                                egui::Color32::from_rgba_unmultiplied(
                                    (node.color[0] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[1] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[2] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[3] * 255.0).clamp(0.0, 255.0) as u8,
                                ),
                                node.font_family,
                                node.bold,
                                node.italic,
                                node.rich_text,
                            );
                        }

                        _ => {
                            ui.painter().text(
                                node_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &node.element_type,
                                egui::FontId::monospace(12.0),
                                egui::Color32::WHITE,
                            );
                        }
                    }
                }

                if background_response.clicked() && topmost_hit.is_none() {
                    self.ui_selected_node = None;
                }

                if ui.input(|input| input.key_pressed(egui::Key::Delete)) {
                    if let Some(id) = self.ui_selected_node.take() {
                        if self.ui_document.delete_node_in(id, preview_size).is_ok() {
                            self.needs_save = true;
                        }
                    }
                }

                if ui.input(|input| {
                    input.modifiers.command_only() && input.key_pressed(egui::Key::D)
                }) {
                    if let Some(id) = self.ui_selected_node {
                        if let Ok(new_id) = self.ui_document.duplicate_node(id) {
                            self.select_ui_node(new_id);
                            self.needs_save = true;
                        }
                    }
                }

                if ui.input(|input| input.pointer.any_released()) {
                    if let Some(asset_name) = self.ui_drag_asset.take() {
                        if let Some(pointer) = pointer_pos {
                            if canvas_rect.contains(pointer) {
                                let presets = crate::engine::ui_authoring::builtin_presets();
                                if presets.iter().any(|(p_name, _)| *p_name == asset_name) {
                                    let ids = self.ui_document.instantiate_preset(&asset_name);
                                    if let Some(&first_id) = ids.first() {
                                        self.select_ui_node(first_id);
                                    }
                                    self.needs_save = true;
                                } else {
                                    let local_position = [
                                        ((pointer.x - canvas_rect.min.x) / preview_scale).max(0.0),
                                        ((pointer.y - canvas_rect.min.y) / preview_scale).max(0.0),
                                    ];

                                    match crate::engine::ui_authoring::load_ui_asset(
                                        project_path.as_deref(),
                                        &asset_name,
                                    ) {
                                        Ok(asset) => {
                                            let id = self
                                                .ui_document
                                                .add_node_from_asset(&asset, local_position);

                                            let parent = self
                                                .ui_document
                                                .node_ids()
                                                .into_iter()
                                                .filter(|candidate| *candidate != id)
                                                .filter_map(|candidate| {
                                                    let layout = self
                                                        .ui_document
                                                        .resolve_layout_in(candidate, preview_size)
                                                        .ok()?;
                                                    let candidate_rect = egui::Rect::from_min_size(
                                                        egui::pos2(
                                                            canvas_rect.left()
                                                                + layout.position[0]
                                                                    * preview_scale,
                                                            canvas_rect.top()
                                                                + layout.position[1]
                                                                    * preview_scale,
                                                        ),
                                                        egui::vec2(
                                                            layout.size[0] * preview_scale,
                                                            layout.size[1] * preview_scale,
                                                        ),
                                                    );
                                                    let z = self
                                                        .ui_document
                                                        .node(candidate)
                                                        .map(|node| node.z_index)?;
                                                    candidate_rect
                                                        .contains(pointer)
                                                        .then_some((candidate, z))
                                                })
                                                .max_by(|left, right| {
                                                    left.1
                                                        .partial_cmp(&right.1)
                                                        .unwrap_or(Ordering::Equal)
                                                        .then_with(|| left.0.cmp(&right.0))
                                                })
                                                .map(|(candidate, _)| candidate);

                                            if let Some(parent) = parent {
                                                let _ = self.ui_document.set_parent_in(
                                                    id,
                                                    Some(parent),
                                                    preview_size,
                                                );
                                            }

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
                }
            });
    }

    pub(crate) fn generate_authored_terrain(&mut self, world: &mut World) -> bool {
        let request = TerrainGenerationRequest {
            position: self.terrain_generation_position,
            size: self.terrain_generation_size,
        };
        match world.generate_authored_terrain(request) {
            Ok(()) => {
                self.terrain_generation_error = None;
                self.needs_save = true;
                true
            }
            Err(error) => {
                self.terrain_generation_error = Some(error.to_string());
                false
            }
        }
    }

    pub(crate) fn draw_project_settings_workspace(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
        renderer: &Renderer,
    ) {
        use super::feature_nav::ProjectSection;
        egui::TopBottomPanel::bottom("worldkiln_project_settings_content_browser")
            .resizable(true)
            .default_height(265.0)
            .min_height(145.0)
            .show(ctx, |ui| {
                ui.strong("CONTENT BROWSER");
                ui.separator();
                self.draw_unreal_asset_browser(ui, project_path);
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("PROJECT SETTINGS");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Back to Editor").clicked() {
                        self.close_settings();
                    }
                });
            });
            ui.separator();
            let sidebar = 190.0;
            let height = ui.available_height();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(sidebar, height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.set_width(sidebar);
                        ui.small("PROJECT");
                        ui.separator();
                        for page in ProjectSection::ALL {
                            if ui
                                .add_sized(
                                    [sidebar - 12.0, 30.0],
                                    egui::SelectableLabel::new(
                                        self.navigation.project_section == page,
                                        page.label(),
                                    ),
                                )
                                .clicked()
                            {
                                self.navigation.project_section = page;
                            }
                        }
                    },
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        if self.navigation.project_section == ProjectSection::Lighting {
                            ui.horizontal_top(|ui| {
                                let controls_width =
                                    340.0_f32.min((ui.available_width() * 0.45).max(220.0));
                                ui.allocate_ui_with_layout(
                                    egui::vec2(controls_width, height),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt("worldkiln_lighting_settings_controls")
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width(controls_width);
                                                ui.add_space(8.0);
                                                self.render_lighting_quick_access(
                                                    ui,
                                                    world,
                                                    project_path,
                                                );
                                            });
                                    },
                                );
                                ui.separator();
                                ui.vertical(|ui| {
                                    let image_size = ui.available_size().max(egui::vec2(1.0, 1.0));
                                    let now = std::time::Instant::now();
                                    let due =
                                        self.lighting_preview_last_capture.is_none_or(|last| {
                                            now.duration_since(last).as_millis() >= 100
                                        });
                                    if due && image_size.x > 4.0 && image_size.y > 4.0 {
                                        let aspect = (image_size.x / image_size.y).clamp(0.5, 4.0);
                                        let preview_width = 640;
                                        let preview_height =
                                            ((preview_width as f32 / aspect).round() as i32)
                                                .clamp(160, 640);
                                        if let Some(image) = renderer
                                            .capture_lighting_preview(preview_width, preview_height)
                                        {
                                            if let Some(texture) =
                                                self.lighting_preview_texture.as_mut()
                                            {
                                                texture.set(image, egui::TextureOptions::LINEAR);
                                            } else {
                                                self.lighting_preview_texture =
                                                    Some(ui.ctx().load_texture(
                                                        "worldkiln_lighting_scene_preview",
                                                        image,
                                                        egui::TextureOptions::LINEAR,
                                                    ));
                                            }
                                        }
                                        self.lighting_preview_last_capture = Some(now);
                                    }
                                    if let Some(texture) = &self.lighting_preview_texture {
                                        ui.add(
                                            egui::Image::new(texture).fit_to_exact_size(image_size),
                                        );
                                    } else {
                                        ui.allocate_space(image_size);
                                    }
                                });
                            });
                        } else {
                            self.lighting_preview_texture = None;
                            self.lighting_preview_last_capture = None;
                            egui::ScrollArea::vertical()
                                .id_salt("worldkiln_project_settings_workspace")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add_space(8.0);
                                    self.render_project_settings_content(ui, world, project_path);
                                    if self.navigation.project_section
                                        == ProjectSection::PostProcessing
                                    {
                                        renderer
                                            .refresh_post_process_pipeline(project_path.as_deref());
                                    }
                                    ui.add_space(12.0);
                                });
                        }
                    },
                );
            });
        });
    }

    fn draw_development_settings(ui: &mut egui::Ui, state: &mut super::DevelopmentTestRunUi) {
        use super::DevelopmentTestRunAction as Action;
        use crate::engine::development_test_run::CaptureState;

        settings_page_header(
            ui,
            "DEVELOPMENT",
            "Development performance recording controls",
        );
        settings_section(ui, "TEST RUN");

        let Some(info) = state.summary.as_ref() else {
            ui.spinner();
            return;
        };
        if matches!(info.state, CaptureState::Running | CaptureState::Paused) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }

        egui::Grid::new("development_test_run_metrics")
            .num_columns(2)
            .spacing([16.0, 8.0])
            .show(ui, |ui| {
                ui.label("Status");
                ui.label(format!("{:?}", info.state));
                ui.end_row();
                ui.label("Recording");
                ui.label(format!("{:.2}s", info.active_duration.as_secs_f64()));
                ui.end_row();
                ui.label("Events");
                ui.label(format!(
                    "{} | Discarded: {}",
                    info.event_count, info.dropped_events
                ));
                ui.end_row();
                ui.label("FPS");
                ui.label(
                    info.average_fps
                        .map_or("N/A".to_string(), |v| format!("{v:.1}")),
                );
                ui.end_row();
                ui.label("1% Low");
                ui.label(
                    info.fps_1pct_low
                        .map_or("N/A".to_string(), |v| format!("{v:.1}")),
                );
                ui.end_row();
                ui.label("P95 frame");
                ui.label(info.p95_frame_time.map_or("N/A".to_string(), |v| {
                    format!("{:.2} ms", v.as_secs_f64() * 1000.0)
                }));
                ui.end_row();
            });
        ui.add_space(12.0);
        ui.label("CAPTURE MODE");
        ui.label("Automatic slow-frame capture");
        ui.add_enabled_ui(
            matches!(info.state, CaptureState::Ready | CaptureState::Stopped) && !state.saving,
            |ui| {
                ui.horizontal(|ui| {
                    ui.label("Capture options");
                    ui.selectable_value(&mut state.config.detailed, false, "Standard");
                    ui.selectable_value(&mut state.config.detailed, true, "Detailed");
                });
                ui.horizontal(|ui| {
                    ui.label("History before / after trigger (frames)");
                    ui.add(
                        egui::DragValue::new(&mut state.config.pre_trigger_frames).range(0..=120),
                    );
                    ui.add(
                        egui::DragValue::new(&mut state.config.post_trigger_frames).range(0..=120),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Trigger when frame exceeds");
                    ui.add(
                        egui::DragValue::new(&mut state.config.threshold_ms)
                            .range(1..=1000)
                            .suffix(" ms"),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut state.config.cpu, "CPU");
                    ui.checkbox(&mut state.config.gpu, "GPU");
                    ui.checkbox(&mut state.config.scripts, "Scripts");
                    ui.checkbox(&mut state.config.resources, "Resources");
                });
            },
        );
        ui.small("Keeps configured history before and after a hitch; capped at 64 episodes. Missing GPU timing remains unavailable.");
        ui.add_space(12.0);
        settings_section(ui, "STALL DIAGNOSTICS");
        ui.add_enabled_ui(
            matches!(info.state, CaptureState::Ready | CaptureState::Stopped) && !state.saving,
            |ui| {
                ui.checkbox(&mut state.config.script_spikes, "Record individual slow script slices")
                    .on_hover_text("Writes script_spikes.csv: each long AeoScript fiber, its source file and wall time. Does not save script contents.");
                ui.horizontal(|ui| {
                    ui.label("Script slice threshold");
                    ui.add_enabled(state.config.script_spikes,
                        egui::DragValue::new(&mut state.config.script_spike_threshold_ms)
                            .range(1..=1000).suffix(" ms"));
                });
                ui.checkbox(&mut state.config.navigation_trace, "Trace native navigation calls")
                    .on_hover_text("Writes navigation_calls.csv with the invoking AeoScript and separate timings for navmesh build, refresh, path query and route generation.");
            },
        );
        ui.small("Deep diagnostics run only while Test Run is recording. Timed navigation calls are attributed to their invoking script; no extra navigation requests are made.");
        ui.label(format!(
            "Captured: {} slow script slices | {} navigation calls | {} dropped",
            info.script_spike_samples, info.navigation_call_samples, info.dropped_deep_samples
        ));
        if let Some((script, duration)) = &info.longest_script_slice {
            ui.label(format!(
                "Longest script slice: {script} — {:.1} ms",
                duration.as_secs_f64() * 1000.0
            ));
        }
        if let Some((script, operation, duration)) = &info.longest_navigation_call {
            ui.label(format!(
                "Longest native navigation: {operation} — {:.1} ms ({script})",
                duration.as_secs_f64() * 1000.0
            ));
        }
        ui.label("New report files: script_spikes.csv and navigation_calls.csv. Compare their timestamps with slow_frames.csv. Native timings measure CPU wall time; no GPU timing is inferred.");
        ui.add_space(10.0);
        ui.label(format!(
            "Slow frames: {} | Episodes: {} | Evicted: {}",
            info.slow_frame_triggers, info.slow_episodes_retained, info.slow_episodes_evicted
        ));
        ui.add_space(12.0);

        super::diagnostics_workspace::controls(ui, state);
    }

    pub(crate) fn draw_settings_workspace(
        &mut self,
        ctx: &egui::Context,
        renderer: &Renderer,
        mut world: Option<&mut World>,
        project_path: &Option<PathBuf>,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("SETTINGS").monospace().strong());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("CLOSE").clicked() {
                        self.close_settings();
                        self.queue_engine_sound(EngineSound::Click);
                    }
                });
            });

            ui.separator();

            egui::Frame::none()
                .fill(egui::Color32::from_rgb(55, 39, 12))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgb(202, 145, 42),
                ))
                .inner_margin(egui::Margin::symmetric(10.0, 7.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "BETA: Beta features are experimental and can change at any moment",
                        )
                        .strong()
                        .color(egui::Color32::from_rgb(244, 196, 96)),
                    );
                });

            ui.add_space(4.0);

            let body_height = ui.available_height();

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(SETTINGS_NAV_WIDTH, body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(SETTINGS_NAV_WIDTH);

                        ui.add_space(8.0);

                        for page in SettingsPage::ALL {
                            let selected = self.settings_page == page;

                            let label = if selected {
                                egui::RichText::new(page.label())
                                    .monospace()
                                    .strong()
                                    .color(egui::Color32::WHITE)
                            } else {
                                egui::RichText::new(page.label()).monospace().strong()
                            };

                            let response = ui.add_sized(
                                [SETTINGS_NAV_WIDTH - 8.0, 30.0],
                                egui::SelectableLabel::new(selected, label),
                            );

                            if response.clicked() {
                                self.settings_page = page;
                                self.queue_engine_sound(EngineSound::Select);
                            }
                        }
                    },
                );

                ui.separator();

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());

                                ui.add_space(8.0);

                                match self.settings_page {
                                    SettingsPage::Lighting => {
                                        draw_lighting_settings(ui, renderer);
                                    }

                                    SettingsPage::Editor => {
                                        draw_editor_settings(
                                            ui,
                                            &mut self.theme_settings,
                                            &mut self.history,
                                            &mut self.script_editor,
                                            &mut self.pending_engine_sounds,
                                        );
                                    }

                                    SettingsPage::Theme => {
                                        draw_theme_settings(
                                            ui,
                                            &mut self.theme_settings,
                                            &mut self.pending_engine_sounds,
                                        );
                                    }

                                    SettingsPage::Audio => {
                                        draw_audio_settings(
                                            ui,
                                            &mut self.engine_audio_settings,
                                            &mut self.pending_engine_sounds,
                                        );
                                    }

                                    SettingsPage::Input => {
                                        draw_input_settings(
                                            ui,
                                            &mut self.theme_settings,
                                            &mut self.pending_engine_sounds,
                                        );
                                    }
                                    SettingsPage::Ai => {
                                        if self.ai_chat.draw_settings(ui, project_path) {
                                            self.close_settings();
                                            self.right_panel_tab = super::RightPanelTab::Ai;
                                        }
                                    }
                                    SettingsPage::Development => {
                                        Self::draw_development_settings(
                                            ui,
                                            &mut self.development_test_run_ui,
                                        );
                                    }
                                }

                                ui.add_space(24.0);
                            });
                    },
                );
            });
        });
    }
}
