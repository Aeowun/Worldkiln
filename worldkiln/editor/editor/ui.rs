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
use crate::world::terrain::{TerrainGenerationRequest, TerrainMode};
use std::cmp::Ordering;
use std::path::PathBuf;

const SETTINGS_NAV_WIDTH: f32 = 180.0;
const SETTINGS_LABEL_WIDTH: f32 = 230.0;
const SETTINGS_CONTROL_WIDTH: f32 = 280.0;

// Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
// Terrain lives in the 240-to-420pt Project side panel, not the Settings workspace.
// Never reserve a 230pt label plus a 280pt control inside that narrower panel.
fn terrain_panel_label_width(available: f32) -> f32 {
    (available.max(170.0) * 0.44).clamp(76.0, 165.0)
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

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
pub(crate) fn draw_terrain_settings(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    world: &mut World,
    project_path: &Option<PathBuf>,
) {
    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
    let label_width = terrain_panel_label_width(ui.available_width());
    settings_page_header(
        ui,
        "TERRAIN",
        "Create and edit independent terrain layers using the generator controls below.",
    );

    if !editor.terrain_rebuild_required
        && editor.terrain_mode_selection != world.terrain.config.mode
    {
        editor.terrain_mode_selection = world.terrain.config.mode;
    }

    // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:52 -05:00
    super::terrain_layers_ui::draw_generation_layers_ui(ui, editor, world);

    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    // The generation-layer dropdown replaces the deprecated independent
    // legacy Plains generator controls. Shared rendering/material tools remain.

    settings_section(ui, "REPRESENTATION");
    if editable_settings_combo(
        ui,
        "settings_terrain_mode",
        "Terrain Mode",
        &mut editor.terrain_mode_selection,
        &[TerrainMode::Smooth, TerrainMode::Block],
        |mode| match mode {
            TerrainMode::Smooth => "Smooth",
            TerrainMode::Block => "Block",
        },
    ) {
        if world.terrain.authored.is_some() {
            editor.terrain_rebuild_required =
                editor.terrain_mode_selection != world.terrain.config.mode;
        } else {
            world.terrain.config.mode = editor.terrain_mode_selection;
            editor.needs_save = true;
        }
    }
    if editor.terrain_rebuild_required {
        ui.colored_label(
            egui::Color32::from_rgb(244, 196, 96),
            "The Terrain Mode change requires rebuilding the representation.",
        );
        if ui.button("Rebuild Terrain").clicked() {
            world
                .terrain
                .rebuild_representation(editor.terrain_mode_selection);
            editor.terrain_rebuild_required = false;
            editor.needs_save = true;
            editor.queue_engine_sound(EngineSound::Click);
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-05 | 2026-10-06 23:14 -05:00
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
    ui.label(
        egui::RichText::new(
            "Rock Slope is the center of the automatic cliff-material transition. Slope Blend and Layer Noise break that boundary up naturally. Regenerate Terrain to apply material-layer changes; Slope Shading updates the current voxel mesh without changing materials.",
        )
        .small()
        .weak(),
    );
    if world.terrain.config.voxel_layers != layers_before {
        editor.needs_save = true;
    }

    // Worldkiln 0.9.3 | WO 0.9.3-22 | 2026-10-07 18:36 -05:00
    settings_section(ui, "GRASS");
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

    ui.label(
        egui::RichText::new(
            "Procedural grass is render-only, non-colliding, and grows on Surface terrain above water. Beach sand does not grow grass.",
        )
        .small()
        .weak(),
    );

    if world.terrain.config.grass != grass_before {
        editor.needs_save = true;
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    settings_section(ui, "WATER APPEARANCE");
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    // The global water surface is distinct from each generator layer's local
    // coastline/water threshold. Retain its control after removing the old
    // Plains-only generation panel.
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
    let water_before = world.terrain.config.water;
    let water = &mut world.terrain.config.water;
    for (label, value, range) in [
        ("Wave Height", &mut water.wave_height, 0.0..=3.0),
        ("Wave Speed", &mut water.wave_speed, 0.0..=3.0),
        ("Foam Amount", &mut water.foam_amount, 0.0..=2.0),
        ("Foam Scale", &mut water.foam_scale, 0.25..=4.0),
    ] {
        ui.horizontal(|ui| {
            ui.add_sized([label_width, 24.0], egui::Label::new(label));
            ui.add(
                egui::Slider::new(value, range)
                    .step_by(0.01)
                    .show_value(true),
            );
        });
    }
    ui.label(
        egui::RichText::new(
            "These controls update the water shader without regenerating Terrain. Surface opacity remains fixed at the approved 0.78.",
        )
        .small()
        .weak(),
    );
    if world.terrain.config.water != water_before {
        editor.needs_save = true;
    }

    settings_section(ui, "MATERIALS");
    let materials_before = world.terrain.config.materials.clone();
    for (label, target, texture) in [
        (
            "Surface",
            "terrain_surface_texture",
            &mut world.terrain.config.materials.surface,
        ),
        (
            "Subsurface",
            "terrain_subsurface_texture",
            &mut world.terrain.config.materials.subsurface,
        ),
        (
            "Rock",
            "terrain_rock_texture",
            &mut world.terrain.config.materials.rock,
        ),
        (
            "Water",
            "terrain_water_texture",
            &mut world.terrain.config.materials.water,
        ),
    ] {
        ui.label(egui::RichText::new(label).strong());
        tools::draw_texture_edit(ui, texture, target, project_path);
    }
    ui.add_space(12.0);
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    // Base Terrain remains in project data for backward compatibility; users
    // generate new Terrain through the selected layer dropdown above.
    if let Some(error) = &editor.terrain_generation_error {
        ui.colored_label(
            egui::Color32::from_rgb(235, 95, 95),
            format!("Terrain generation failed: {error}"),
        );
    }

    tools::texture_tools::draw_texture_browser(
        ui.ctx(),
        "terrain_surface_texture",
        &mut world.terrain.config.materials.surface,
        project_path,
    );
    tools::texture_tools::draw_texture_browser(
        ui.ctx(),
        "terrain_subsurface_texture",
        &mut world.terrain.config.materials.subsurface,
        project_path,
    );
    tools::texture_tools::draw_texture_browser(
        ui.ctx(),
        "terrain_rock_texture",
        &mut world.terrain.config.materials.rock,
        project_path,
    );
    tools::texture_tools::draw_texture_browser(
        ui.ctx(),
        "terrain_water_texture",
        &mut world.terrain.config.materials.water,
        project_path,
    );
    if world.terrain.config.materials != materials_before {
        editor.needs_save = true;
    }
}

#[cfg(test)]
mod terrain_panel_layout_tests {
    use super::*;

    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
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

    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
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
                    if text.galley.job.text == "TERRAIN" {
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
            "Cache: {} probes · {} updated · {} rays · {:.2} ms CPU update",
            stats.cache_probes, stats.updated_probes, stats.traced_rays, stats.cache_update_cpu_ms
        ));
        ui.label(format!(
            "GI scene: {} static voxels · {} body AABBs · {:.2} MiB packed payload",
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
                            "BETA LIGHTING · {}",
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
                                "UI frame {:.2} ms · {:.1} FPS",
                                seconds * 1000.0,
                                1.0 / seconds
                            ));
                        }

                        PerformanceDisplay::PassTimings => {
                            ui.monospace("GPU pass timings · collection pending");
                        }

                        PerformanceDisplay::RaysAndCacheStatistics => {
                            let stats = renderer.lighting_performance_snapshot();

                            ui.monospace(format!(
                                "Rays {} · probes {}/{}",
                                stats.traced_rays, stats.updated_probes, stats.cache_probes
                            ));

                            ui.monospace(format!(
                                "GI update {:.2} ms CPU · voxels {} · bodies {}",
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
        // ---------------------------------------------------------------------
        // Full-screen Play presentation
        // ---------------------------------------------------------------------
        //
        // Play mode is a game presentation surface, not an editor workspace.
        // Do not leave editor chrome around the renderer: no menu bar, toolbar,
        // explorer, inspector, navigator, output, or status bar. Escape remains
        // the explicit path back to Editor mode.
        if self.mode == crate::engine::EditorMode::Play {
            if self.script_editor.show_output {
                self.draw_output_panel(ctx);
            }
            self.viewport_rect = ctx.screen_rect();
            self.viewport_ppp = ctx.pixels_per_point();
            return;
        }

        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
        // The approved shell always precedes the active workspace. Play exits before it.
        self.draw_menu_bar(ctx);
        // Worldkiln 0.8.5 | WO 0.8.7-71 | 2026-10-09 19:52 -05:00
        // Script files and UI documents can be opened outside the tab strip
        // (Explorer, AI, Asset Browser). Route those transitions through the
        // same panel contract rather than only highlighting the tab.
        // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:52 -05:00
        // An explicitly opened settings surface wins over stale UI/Script
        // workspace flags. Do not automatically close Settings in show_ui.
        if !self.navigation.surface.is_settings() {
            if self.ui_mode && self.shell.workspace != super::shell::Workspace::Ui {
                self.activate_shell_workspace(super::shell::Workspace::Ui);
            } else if self.show_script_workspace
                && self.shell.workspace != super::shell::Workspace::Script
            {
                self.activate_shell_workspace(super::shell::Workspace::Script);
            }
        }
        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:17 -05:00
        // The six top-level destinations include Project Settings next to
        // Script. Project Settings keeps that tab row visible, so Home is one
        // click away. Engine preferences remain a separate surface.
        if self.navigation.surface != super::feature_nav::EditorSurface::AppSettings {
            self.draw_workspace_tabs(ctx);
        }
        // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:41 -05:00
        // Exactly one active central editor surface; no unrelated panels
        // remain visible behind Engine/Project Settings.
        match self.navigation.surface {
            super::feature_nav::EditorSurface::Workspace => {}
            super::feature_nav::EditorSurface::AppSettings => {
                self.draw_settings_workspace(ctx, renderer, Some(world), project_path);
                self.viewport_rect = egui::Rect::NOTHING;
                self.viewport_ppp = ctx.pixels_per_point();
                return;
            }
            super::feature_nav::EditorSurface::ProjectSettings => {
                self.draw_project_settings_workspace(ctx, world, project_path);
                self.viewport_rect = egui::Rect::NOTHING;
                self.viewport_ppp = ctx.pixels_per_point();
                return;
            }
        }

        // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
        // UI document changes are captured only while UI mode owns the editor.
        if self.ui_mode {
            let before = self.ui_document.clone();
            self.draw_ui_mode_toolbar(ctx);
            self.draw_shell_dock(ctx, project_path);
            if self.theme_settings.status_bar {
                self.draw_status_bar(ctx);
            }
            if self.shell.explorer_visible {
                self.draw_ui_left_panel(ctx, project_path);
            }
            // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
            if self.shell.properties_visible {
                if self.ai_chat.open && self.right_panel_tab == super::RightPanelTab::Ai {
                    self.draw_right_panel(ctx, world, project_path);
                } else {
                    self.draw_ui_authoring_properties(ctx, project_path);
                }
            }

            self.viewport_rect = ctx.available_rect();
            self.viewport_ppp = ctx.pixels_per_point();

            self.draw_ui_canvas(ctx, project_path);
            // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
            // UI history is keyed by document so workspace switches preserve each timeline.
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
            // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
            // Group keyboard-driven property/text edits after a short idle pause;
            // pointer gestures still commit immediately when released.
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

        // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:12 -05:00
        self.draw_shell_ribbon(ctx, world, project_path);

        // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:26 -05:00
        self.draw_asset_manager(ctx, world, project_path, renderer);

        if !self.last_show_script_workspace && self.show_script_workspace {
            self.script_editor.refresh_scripts(project_path);
        }

        self.last_show_script_workspace = self.show_script_workspace;

        self.draw_shell_dock(ctx, project_path);
        if self.theme_settings.status_bar {
            self.draw_status_bar(ctx);
        }
        if self.shell.explorer_visible {
            self.draw_left_panel(ctx, world, project_path);
        }

        if self.show_script_workspace {
            // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:50 -05:00
            // Keep chat available beside the script editor without exposing document context yet.
            if self.shell.properties_visible
                && self.ai_chat.open
                && self.right_panel_tab == super::RightPanelTab::Ai
            {
                self.draw_right_panel(ctx, world, project_path);
            }
            self.script_editor.show_ui(ctx, project_path, world);

            self.viewport_rect = egui::Rect::NOTHING;
            self.viewport_ppp = ctx.pixels_per_point();

            return;
        }

        if self.shell.properties_visible {
            self.draw_right_panel(ctx, world, project_path);
        }
        self.draw_dialogs(ctx, world);

        self.render_active_color_picker(ctx, world);

        self.viewport_rect = ctx.available_rect();
        self.viewport_ppp = ctx.pixels_per_point();

        draw_lighting_performance_overlay(ctx, renderer, self.viewport_rect);
        // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
        // Render selected Actor gizmos over the GL viewport, without
        // capturing scene input or modifying any authored project data.
        self.draw_actor_transform_gizmo(ctx, world);
        // Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:14 -05:00
        // Dock draws before side panels; accept a typed Actor drag only once
        // the final central viewport rectangle is known. Never mutate while
        // hovering or outside the canvas.
        self.handle_actor_content_drop(ctx, world, project_path);
    }

    pub(crate) fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:44 -05:00
            // The reference has a padded wordmark and centered primary menus.
            // Text inherits the configured Heading and Body typography.
            ui.add_space(6.0);
            egui::menu::bar(ui, |ui| {
                ui.add_space(20.0);
                ui.label(
                    egui::RichText::new("WORLDKILN")
                        .text_style(egui::TextStyle::Heading)
                        .strong(),
                );
                let menu_start = (ui.max_rect().width() * 0.43).clamp(160.0, 790.0);
                let gap = (menu_start - ui.cursor().left()).max(16.0);
                ui.add_space(gap);
                ui.menu_button("File", |ui| {
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
                });

                ui.menu_button("Edit", |ui| {
                    // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:44 -05:00
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
                });

                // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:44 -05:00
                // View owns window visibility. Project Settings and tools
                // have dedicated owners, never nested extra submenus.
                ui.menu_button("View", |ui| {
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
                });
                // Features click 1 -> canonical destination click 2.
                self.draw_feature_directory(ui);
                // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:04 -05:00
                // Approved layout: File / Edit / View only; preserved advanced
                // Tools and Panels menus now live inside View.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:44 -05:00
                    ui.spacing_mut().item_spacing.x = 11.0;
                    ui.spacing_mut().button_padding = egui::vec2(7.0, 5.0);
                    if ui.button("⚙").on_hover_text("Settings").clicked() {
                        self.open_engine_settings(self.settings_page);
                        self.queue_engine_sound(EngineSound::Select);
                    }
                    if ui.button("AI").on_hover_text("AI Assistant").clicked() {
                        self.navigate_to(super::feature_nav::Feature::AiAssistant);
                    }
                    if ui.button("▣").on_hover_text("Asset Manager").clicked() {
                        self.navigate_to(super::feature_nav::Feature::AssetManager);
                    }
                    if ui.button("▶").on_hover_text("Play").clicked() {
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

                // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
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

                let res_text = match self.ui_preview_resolution {
                    super::UiPreviewResolution::Res1280x720 => "1280 x 720 (HD)".to_string(),
                    super::UiPreviewResolution::Res1600x900 => "1600 x 900 (HD+)".to_string(),
                    super::UiPreviewResolution::Res1920x1080 => "1920 x 1080 (FHD)".to_string(),
                    super::UiPreviewResolution::Custom(w, h) => {
                        format!("Custom ({:.0} x {:.0})", w, h)
                    }
                };

                egui::ComboBox::from_id_source("ui_preview_res_picker")
                    .selected_text(res_text)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.ui_preview_resolution,
                            super::UiPreviewResolution::Res1280x720,
                            "1280 x 720 (HD)",
                        );
                        ui.selectable_value(
                            &mut self.ui_preview_resolution,
                            super::UiPreviewResolution::Res1600x900,
                            "1600 x 900 (HD+)",
                        );
                        ui.selectable_value(
                            &mut self.ui_preview_resolution,
                            super::UiPreviewResolution::Res1920x1080,
                            "1920 x 1080 (FHD)",
                        );
                        ui.selectable_value(
                            &mut self.ui_preview_resolution,
                            super::UiPreviewResolution::Custom(1366.0, 768.0),
                            "Custom",
                        );
                    });

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

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
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
                        ui.label("Selected: —");
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
                        "Hover: —".to_string()
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
                                    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
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
                                    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
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
        // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
        // // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 18:44 -05:00 Keep UI workspace column sizing consistent with Home.
        let panel_response=egui::SidePanel::right("ui_authoring_properties")
            .resizable(true)
            .default_width(self.shell.properties_width)
            .min_width(260.0)
            .max_width(520.0)
            .show(ctx, |ui| {
                // Worldkiln 0.8.5 | WO 0.8.7-71 | 2026-10-09 20:01 -05:00
                // UI Inspector has many editable sections; keep every action
                // accessible without expanding or clipping the right panel.
                egui::ScrollArea::vertical()
                    .id_salt("worldkiln_ui_authoring_properties_scroll")
                    .auto_shrink([false,false])
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
                        egui::RichText::new(format!("VALIDATION ({})", validation_errors.len()))
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
                        egui::RichText::new(format!("{}  [UI {}]", node.element_type, node.id))
                            .monospace()
                            .strong(),
                    );

                    ui.add_space(4.0);

                    // 1. IDENTITY
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

                    // 2. LAYOUT
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
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("X");
                            changed |= ui
                                .add(egui::DragValue::new(&mut node.position[0]).speed(1.0))
                                .changed();
                            ui.label("Y");
                            changed |= ui
                                .add(egui::DragValue::new(&mut node.position[1]).speed(1.0))
                                .changed();
                        });

                        ui.horizontal(|ui| {
                            ui.label("W");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut node.size[0])
                                        .speed(1.0)
                                        .range(1.0..=4096.0),
                                )
                                .changed();
                            ui.label("H");
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut node.size[1])
                                        .speed(1.0)
                                        .range(1.0..=4096.0),
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

                    // 3. APPEARANCE
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

                    // 4. TYPOGRAPHY
                    if node.element_type == "Text" || node.element_type == "Button" {
                        ui.label(
                            egui::RichText::new("TYPOGRAPHY")
                                .monospace()
                                .strong()
                                .color(ui.visuals().selection.bg_fill),
                        );

                        ui.label("Text");
                        if node.element_type == "Text" {
                            changed |= ui.text_edit_multiline(&mut node.text).changed();
                        } else {
                            changed |= ui.text_edit_singleline(&mut node.text).changed();
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

                    // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:20 -05:00
                    // Media controls use the SAME properties as ui.new("Image"|"Video").
                    if matches!(node.element_type.as_str(), "Image" | "Video") {
                        ui.label(egui::RichText::new("MEDIA").monospace().strong().color(ui.visuals().selection.bg_fill));
                        ui.label("Asset source: Media/filename.png or Media/filename.mp4");
                        changed |= ui.text_edit_singleline(&mut node.source).changed();

                        if node.element_type == "Video" {
                            if ui.button("Import MP4...").clicked() {
                                if let Some(path)=rfd::FileDialog::new()
                                    .add_filter("MP4 video",&["mp4"]).pick_file(){
                                    match project_path.as_deref().ok_or("No project".to_string())
                                        .and_then(|project|crate::engine::ui_media::import_mp4(project,&path)){
                                        Ok(relative)=>{
                                            node.source=relative;
                                            changed=true;
                                        },
                                        Err(error)=>eprintln!("[UI][MEDIA] {error}"),
                                    }
                                }
                            }
                        } else if ui.button("Choose imported texture...").clicked(){
                            if let Some(project)=project_path.as_deref(){
                                let library=project.join(".assets").join("textures");
                                if let Some(path)=rfd::FileDialog::new()
                                    .set_directory(&library)
                                    .add_filter("PNG or JPEG",&["png","jpg","jpeg"])
                                    .pick_file(){
                                    if let (Ok(real),Ok(dir))=(path.canonicalize(),library.canonicalize()){
                                        if let Ok(relative)=real.strip_prefix(&dir){
                                            node.source=format!("textures/{}",relative.to_string_lossy()
                                                .replace('\\',"/"));
                                            changed=true;
                                        }else{
                                            eprintln!("[UI][MEDIA] Choose a previously imported project texture.");
                                        }
                                    }
                                }
                            }
                        }

                        ui.horizontal(|ui| {
                            ui.label("Fit");
                            egui::ComboBox::from_id_source("ui_media_fit")
                                .selected_text(format!("{:?}", node.fit))
                                .show_ui(ui, |ui| {
                                    for fit in crate::engine::ui::UiMediaFit::ALL {
                                        changed |= ui.selectable_value(&mut node.fit, fit, format!("{fit:?}")).changed();
                                    }
                                });
                        });
                        if let Err(error) = crate::engine::ui_media::validate_ui_media_source(&node.source) {
                            ui.colored_label(egui::Color32::LIGHT_RED, error);
                        }
                        if node.element_type == "Video" {
                            changed |= ui.checkbox(&mut node.playing, "Playing").changed();
                            changed |= ui.checkbox(&mut node.looped, "Loop").changed();
                            ui.horizontal(|ui| {
                                ui.label("Speed");
                                changed |= ui.add(egui::Slider::new(&mut node.playback_rate, 0.25..=4.0)).changed();
                            });
                        }
                        ui.separator();
                    }

                    // 5. INTERACTION & STATE
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

                    // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:11 -05:00
                    if node.element_type == "Button" {
                        ui.add_space(6.0);
                        ui.label("On Click");

                        let current = node.events.get("on_click").cloned().unwrap_or_default();
                        let mut module_key = current.module_key;
                        let mut handler = current.handler;
                        let mut event_changed = false;

                        ui.horizontal(|ui| {
                            ui.label("Module/script key");
                            event_changed |= ui.text_edit_singleline(&mut module_key).changed();
                        });
                        ui.horizontal(|ui| {
                            ui.label("Handler");
                            event_changed |= ui.text_edit_singleline(&mut handler).changed();
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

                    // 6. ACTIONS
                    ui.label(
                        egui::RichText::new("ACTIONS")
                            .monospace()
                            .strong()
                            .color(ui.visuals().selection.bg_fill),
                    );

                    ui.horizontal(|ui| {
                        if ui.button("Duplicate").clicked() {
                            if let Ok(new_id) = self.ui_document.duplicate_node(selected_id) {
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
                    match self.ui_document.set_parent_in(
                        selected_id,
                        parent,
                        self.ui_preview_resolution.size(),
                    ) {
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
                    if self
                        .ui_document
                        .delete_node_in(selected_id, self.ui_preview_resolution.size())
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

                let preview_size = self.ui_preview_resolution.size();
                let preview_scale = (rect.width() / preview_size[0])
                    .min(rect.height() / preview_size[1])
                    .max(0.01);
                let rendered_size = egui::vec2(
                    preview_size[0] * preview_scale,
                    preview_size[1] * preview_scale,
                );
                let canvas_rect = egui::Rect::from_center_size(rect.center(), rendered_size);

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

                let mut node_ids = self.ui_document.node_ids();

                node_ids.sort_by(|left_id, right_id| {
                    let left_z = self
                        .ui_document
                        .node(*left_id)
                        .map(|node| node.z_index)
                        .unwrap_or(0.0);

                    let right_z = self
                        .ui_document
                        .node(*right_id)
                        .map(|node| node.z_index)
                        .unwrap_or(0.0);

                    left_z
                        .partial_cmp(&right_z)
                        .unwrap_or(Ordering::Equal)
                        .then_with(|| left_id.cmp(right_id))
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

                    let node_response = ui.interact(
                        node_rect,
                        egui::Id::new("ui_authoring_node").with(id),
                        egui::Sense::click_and_drag(),
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
                        });
                    }

                    if node_response.dragged() {
                        if let Some(drag) = self.ui_drag_state.clone() {
                            if drag.node_id == id {
                                let pointer = pointer_pos.unwrap_or(drag.start_pointer);

                                let mut dx = (pointer.x - drag.start_pointer.x) / preview_scale;
                                let mut dy = (pointer.y - drag.start_pointer.y) / preview_scale;

                                let desired_left = layout.position[0] + dx;
                                let desired_top = layout.position[1] + dy;
                                let mut x_targets =
                                    vec![0.0, preview_size[0] * 0.5, preview_size[0]];
                                let mut y_targets =
                                    vec![0.0, preview_size[1] * 0.5, preview_size[1]];
                                for sibling in self.ui_document.node_ids() {
                                    if sibling == id {
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
                                    desired_left + layout.size[0] * 0.5,
                                    desired_left + layout.size[0],
                                ];
                                let y_features = [
                                    desired_top,
                                    desired_top + layout.size[1] * 0.5,
                                    desired_top + layout.size[1],
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

                                let _ = self.ui_document.set_position(
                                    id,
                                    [drag.start_position[0] + dx, drag.start_position[1] + dy],
                                );

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
                                node.size
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
                        if resize.dragged() {
                            let delta = ctx.input(|i| i.pointer.delta()) / preview_scale;
                            if let Some(node) = self.ui_document.node_mut(id) {
                                let direction =
                                    if node.anchor == crate::engine::ui::UiAnchor::Stretch {
                                        -1.0
                                    } else {
                                        1.0
                                    };
                                // Adjust the anchor offset too, keeping the top-left corner fixed.
                                let (old_offset, _) =
                                    node.anchor.resolve([0.0, 0.0], [0.0, 0.0], node.size);
                                node.size = [
                                    (node.size[0] + direction * delta.x).max(0.0),
                                    (node.size[1] + direction * delta.y).max(0.0),
                                ];
                                let (new_offset, _) =
                                    node.anchor.resolve([0.0, 0.0], [0.0, 0.0], node.size);
                                node.position[0] += old_offset[0] - new_offset[0];
                                node.position[1] += old_offset[1] - new_offset[1];
                            }
                            self.needs_save = true;
                        }
                    }

                    let disabled = !node.enabled;

                    let fill = match node.element_type.as_str() {
                        "Panel" => {
                            let alpha_scale = if disabled { 0.35 } else { 1.0 };

                            egui::Color32::from_rgba_unmultiplied(
                                (node.color[0] * 255.0 * alpha_scale).clamp(0.0, 255.0) as u8,
                                (node.color[1] * 255.0 * alpha_scale).clamp(0.0, 255.0) as u8,
                                (node.color[2] * 255.0 * alpha_scale).clamp(0.0, 255.0) as u8,
                                (node.color[3] * 255.0).clamp(0.0, 255.0) as u8,
                            )
                        }

                        "Button" => egui::Color32::from_rgba_unmultiplied(
                            20,
                            23,
                            28,
                            if disabled { 80 } else { 235 },
                        ),

                        "Text" => egui::Color32::from_rgba_unmultiplied(
                            0,
                            0,
                            0,
                            if disabled { 8 } else { 20 },
                        ),

                        _ => egui::Color32::from_rgba_unmultiplied(
                            40,
                            40,
                            40,
                            if disabled { 70 } else { 180 },
                        ),
                    };

                    ui.painter()
                        .rect_filled(node_rect, node.corner_radius, fill);

                    if node.border_width > 0.0 {
                        let border = egui::Color32::from_rgba_unmultiplied(
                            (node.border_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.border_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.border_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                            (node.border_color[3] * 255.0).clamp(0.0, 255.0) as u8,
                        );

                        ui.painter().rect_stroke(
                            node_rect,
                            node.corner_radius,
                            egui::Stroke::new(node.border_width.max(1.0), border),
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
                        // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:44 -05:00
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

                            crate::engine::ui::paint_ui_text(
                                ui.painter(),
                                ctx,
                                node_rect,
                                display_text,
                                node.font_size * preview_scale,
                                node.horizontal_align,
                                node.vertical_align,
                                node.wrap,
                                node.padding.map(|value| value * preview_scale),
                                node.line_spacing * preview_scale,
                                egui::Color32::from_rgba_unmultiplied(
                                    (node.color[0] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[1] * 255.0).clamp(0.0, 255.0) as u8,
                                    (node.color[2] * 255.0).clamp(0.0, 255.0) as u8,
                                    if disabled { 100 } else { 255 },
                                ),
                            );
                        }

                        "Button" => {
                            let display_text = if node.text.is_empty() {
                                "BUTTON"
                            } else {
                                &node.text
                            };

                            crate::engine::ui::paint_ui_text(
                                ui.painter(),
                                ctx,
                                node_rect,
                                display_text,
                                node.font_size * preview_scale,
                                node.horizontal_align,
                                node.vertical_align,
                                node.wrap,
                                node.padding.map(|value| value * preview_scale),
                                node.line_spacing * preview_scale,
                                egui::Color32::WHITE,
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

                let background_response = ui.interact(
                    canvas_rect,
                    egui::Id::new("ui_authoring_background"),
                    egui::Sense::click(),
                );

                if background_response.clicked() {
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

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    pub(crate) fn generate_authored_terrain(&mut self, world: &mut World) -> bool {
        let previous_mode = world.terrain.config.mode;
        world.terrain.config.mode = self.terrain_mode_selection;
        let request = TerrainGenerationRequest {
            position: self.terrain_generation_position,
            size: self.terrain_generation_size,
        };
        match world.generate_authored_terrain(request) {
            Ok(()) => {
                self.terrain_generation_error = None;
                self.terrain_rebuild_required = false;
                self.needs_save = true;
                true
            }
            Err(error) => {
                world.terrain.config.mode = previous_mode;
                self.terrain_generation_error = Some(error.to_string());
                false
            }
        }
    }

    // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:41 -05:00
    /// Project-owned settings have a dedicated central page; every section
    /// calls its *existing* owner in panels.rs rather than duplicating widgets.
    pub(crate) fn draw_project_settings_workspace(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
    ) {
        use super::feature_nav::ProjectSection;
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
                        egui::ScrollArea::vertical()
                            .id_salt("worldkiln_project_settings_workspace")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.add_space(8.0);
                                self.render_project_settings_content(ui, world, project_path);
                                ui.add_space(12.0);
                            });
                    },
                );
            });
        });
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

                            // Keep the active Settings page readable regardless of
                            // accent choice. SelectableLabel inherits the global
                            // selection foreground, which can otherwise collapse
                            // into the accent-colored fill.
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
                                    // Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09 15:00 -05:00
                                    SettingsPage::Ai => {
                                        if self.ai_chat.draw_settings(ui, project_path) {
                                            self.close_settings();
                                            self.right_panel_tab = super::RightPanelTab::Ai;
                                        }
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
