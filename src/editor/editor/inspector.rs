// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{ColorTarget, Editor};
use crate::editor::tools;
use crate::world::AttributeValue;
use egui::RichText;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttributeType {
    Number,
    Bool,
    String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyChange {
    LightColor(glam::Vec3),
    Particles(crate::engine::particles::EmitterSettings),
    LightIntensity(f32),
    LightRange(f32),
    LightShadows(bool),
    LightEnabled(bool),
    Solid(bool),
    Anchored(bool),
    Friction(f32),
    Visible(bool),
    ColorRgb(glam::Vec3),
    Texture(String),
    Audio(String),
    AudioPlaying(bool),
    AudioLooped(bool),
    AudioLoopInterval(f32),
    AudioVolume(f32),
    AudioMinDistance(f32),
    AudioMaxDistance(f32),
    CollisionEventsEnabled(bool),
    EntityIdentity(Option<String>),
    AttributeSet(String, AttributeValue),
    AttributeRemove(String),
}

fn cell_supports_generic_physics(cell_type: crate::world::CellType) -> bool {
    matches!(
        cell_type,
        crate::world::CellType::Block
            | crate::world::CellType::FxBlock
            | crate::world::CellType::Player
            | crate::world::CellType::SpawnPoint
    )
}

fn cell_supports_generic_rendering(cell_type: crate::world::CellType) -> bool {
    matches!(
        cell_type,
        crate::world::CellType::Block
            | crate::world::CellType::FxBlock
            | crate::world::CellType::Player
            | crate::world::CellType::SpawnPoint
    )
}

#[derive(Clone, Debug)]
enum ActorModuleInspectorAction {
    Attach(String),
    Detach(String),
    SetEnabled(String, bool),
    Open(std::path::PathBuf),
}

impl Editor {
    pub fn apply_property_changes(
        &mut self,
        world: &mut crate::world::World,
        changes: &[PropertyChange],
    ) {
        if changes.is_empty() {
            return;
        }

        let candidate_history = self
            .pending_inspector_history
            .is_none()
            .then(|| self.create_snapshot_without_terrain(world));
        let selected_coords = self
            .selected_coords
            .iter()
            .copied()
            .chain(self.selected_coord)
            .collect::<std::collections::HashSet<_>>();
        let mut authored_change = false;

        for &c in &selected_coords {
            let mut update_spatial_index = false;
            let mut render_reason = None;

            let changed = {
                if let Some(other_cell) = world.get_mut(c) {
                    let before = other_cell.clone();
                    for change in changes {
                        match change {
                            PropertyChange::Particles(settings) => {
                                if other_cell.cell_type == crate::world::CellType::ParticleEmitter {
                                    other_cell.particles = settings.clone();
                                }
                            }
                            PropertyChange::LightColor(color)
                                if other_cell.cell_type == crate::world::CellType::Light
                                    && color.is_finite() =>
                            {
                                other_cell.light_color = *color;
                            }
                            PropertyChange::LightIntensity(intensity)
                                if other_cell.cell_type == crate::world::CellType::Light
                                    && intensity.is_finite()
                                    && *intensity >= 0.0 =>
                            {
                                other_cell.light_intensity = *intensity;
                            }
                            PropertyChange::LightRange(range)
                                if other_cell.cell_type == crate::world::CellType::Light
                                    && range.is_finite()
                                    && *range >= 0.0 =>
                            {
                                other_cell.light_range = *range;
                            }
                            PropertyChange::LightShadows(shadows)
                                if other_cell.cell_type == crate::world::CellType::Light =>
                            {
                                other_cell.light_shadows = *shadows;
                            }
                            PropertyChange::LightEnabled(enabled)
                                if other_cell.cell_type == crate::world::CellType::Light =>
                            {
                                other_cell.light_enabled = *enabled;
                            }
                            PropertyChange::LightColor(_)
                            | PropertyChange::LightIntensity(_)
                            | PropertyChange::LightRange(_)
                            | PropertyChange::LightShadows(_)
                            | PropertyChange::LightEnabled(_) => {}
                            PropertyChange::Solid(solid) => {
                                other_cell.solid = *solid;
                                update_spatial_index = true;
                                render_reason = Some(crate::world::DirtyReason::Geometry);
                            }
                            PropertyChange::Anchored(anchored) => {
                                other_cell.anchored = *anchored;
                                render_reason = Some(crate::world::DirtyReason::Geometry);
                            }
                            PropertyChange::Friction(value) => {
                                let _ = other_cell.set_friction(*value);
                            }
                            PropertyChange::Visible(visible) => {
                                other_cell.visible = *visible;
                                update_spatial_index = true;
                                render_reason = Some(crate::world::DirtyReason::Geometry);
                            }
                            PropertyChange::ColorRgb(color) => {
                                other_cell.color_rgb = *color;
                                if render_reason != Some(crate::world::DirtyReason::Geometry) {
                                    render_reason =
                                        Some(crate::world::DirtyReason::MaterialOrOffset);
                                }
                            }
                            PropertyChange::Texture(texture) => {
                                other_cell.texture = texture.clone();
                                if render_reason != Some(crate::world::DirtyReason::Geometry) {
                                    render_reason =
                                        Some(crate::world::DirtyReason::MaterialOrOffset);
                                }
                            }
                            PropertyChange::Audio(audio) => other_cell.audio = audio.clone(),
                            PropertyChange::AudioPlaying(playing) => other_cell.playing = *playing,
                            PropertyChange::AudioLooped(looped) => other_cell.looped = *looped,
                            PropertyChange::AudioLoopInterval(interval) => {
                                other_cell.loop_interval = if interval.is_finite() {
                                    interval.max(0.0)
                                } else {
                                    0.0
                                };
                            }
                            PropertyChange::AudioVolume(volume) => other_cell.volume = *volume,
                            PropertyChange::AudioMinDistance(distance) => {
                                other_cell.audio_min_distance = distance
                                    .max(0.0)
                                    .min(other_cell.audio_max_distance.max(0.0));
                            }
                            PropertyChange::AudioMaxDistance(distance) => {
                                other_cell.audio_max_distance =
                                    distance.max(other_cell.audio_min_distance.max(0.0));
                            }
                            PropertyChange::CollisionEventsEnabled(enabled) => {
                                other_cell.collision_events_enabled = *enabled
                            }
                            PropertyChange::EntityIdentity(identity) => {
                                other_cell.entity_identity = identity.clone()
                            }
                            PropertyChange::AttributeSet(name, value) => {
                                other_cell.attributes.insert(name.clone(), value.clone());
                            }
                            PropertyChange::AttributeRemove(name) => {
                                other_cell.attributes.remove(name);
                            }
                        }
                    }
                    *other_cell != before
                } else {
                    false
                }
            };
            authored_change |= changed;
            if changed
                && changes
                    .iter()
                    .any(|change| matches!(change, PropertyChange::Friction(_)))
            {
                if let Some(cell) = world.get(c) {
                    let id = cell.id;
                    world.mark_physics_dirty(id);
                }
            }

            if changed && let Some(reason) = render_reason {
                world.touch_authored_cell(c, reason);
            } else if changed && update_spatial_index {
                world.update_spatial_index_at(c);
            }
        }

        if authored_change {
            self.needs_save = true;
            if let Some(snapshot) = candidate_history {
                self.pending_inspector_history = Some(snapshot);
            }
        }
    }

    pub(crate) fn render_properties_content(
        &mut self,
        ui: &mut egui::Ui,
        world: &mut crate::world::World,
        project_path: &Option<std::path::PathBuf>,
    ) {
        if let Some(id) = self.water_selected_body {
            let inherited_water_look = world.terrain.config.water;
            let shore_layers = world.terrain.config.terrain_material_layers();
            let before = super::water_spline::WaterEditSnapshot::capture(
                &world.terrain.config,
                self.water_selected_body,
            );
            if let Some(body) = world
                .terrain
                .config
                .water_bodies
                .iter_mut()
                .find(|body| body.id == id)
            {
                let original = body.clone();
                ui.heading(match body.kind {
                    crate::world::terrain::WaterBodyKind::Lake => "Water Body Lake",
                    crate::world::terrain::WaterBodyKind::River => "Water Body River",
                });
                ui.horizontal(|ui| {
                    ui.label("Surface Height");
                    ui.add(egui::DragValue::new(&mut body.elevation).speed(0.1));
                });
                ui.checkbox(&mut body.visible, "Visible");
                if super::ui::water_material_picker(
                    ui,
                    project_path.as_deref(),
                    &mut body.material_asset,
                    body.id,
                ) {
                    body.appearance = body.material_asset.as_deref().and_then(|id| {
                        project_path
                            .as_deref()
                            .map(crate::project::material::MaterialCatalog::load)
                            .and_then(|catalog| catalog.water(id))
                    });
                    self.needs_save = true;
                }
                super::terrain_material_ui::shore_selector(
                    ui,
                    ("inspector_shore", body.id),
                    &mut body.shore_material_layer,
                    &shore_layers,
                );
                ui.collapsing("Water Volume & Shore",|ui|{
                    ui.horizontal(|ui|{
                        ui.label("Maximum Depth");
                        ui.add(egui::DragValue::new(&mut body.depth)
                            .speed(0.25).range(0.1..=10000.0))
                            .on_hover_text("Maximum water-column depth in studs; shallower terrain floor always wins");
                    });

                });
                if body.material_asset.is_none() {
                    let mut look = body.appearance.unwrap_or(inherited_water_look);
                    let original_look = look;
                    crate::editor::asset_management::material_editor::draw_water_parameters(
                        ui, &mut look,
                    );
                    if look != original_look {
                        body.appearance = Some(look);
                        self.needs_save = true;
                    }
                }
                if body.kind == crate::world::terrain::WaterBodyKind::Lake {
                    ui.label("Lake perimeter stays at one uniform height.");
                } else {
                    ui.label("River points can change height and width independently.");
                }
                ui.label("Left-click a white point. W Move · E Rotate · R Tangent Scale.");
                ui.label("Ctrl+click multi-select · Alt+W drag duplicate · Delete removes.");
                if !self.water_selected_points.is_empty() {
                    ui.separator();
                    ui.label(format!(
                        "Selected points: {}",
                        self.water_selected_points.len()
                    ));
                    for index in &self.water_selected_points {
                        if let Some(point) = body.points.get_mut(*index) {
                            ui.label(format!("Point {}", index + 1));
                            if body.kind == crate::world::terrain::WaterBodyKind::River {
                                ui.horizontal(|ui| {
                                    ui.label("Width");
                                    ui.add(
                                        egui::DragValue::new(&mut point.width)
                                            .speed(0.25)
                                            .range(0.1..=4096.0),
                                    );
                                    ui.label("Height");
                                    ui.add(
                                        egui::DragValue::new(&mut point.height_offset)
                                            .speed(0.1)
                                            .range(-10000.0..=10000.0),
                                    );
                                });
                            }
                            if body.kind == crate::world::terrain::WaterBodyKind::River {
                                ui.horizontal(|ui| {
                                    ui.label("Channel Depth");
                                    let mut depth = point.channel_depth.unwrap_or(body.depth);
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut depth)
                                                .speed(0.25)
                                                .range(0.1..=10000.0),
                                        )
                                        .changed()
                                    {
                                        point.channel_depth = Some(depth);
                                    }
                                    if point.channel_depth.is_some()
                                        && ui.small_button("Reset").clicked()
                                    {
                                        point.channel_depth = None;
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Current Speed (studs/s)");
                                    ui.add(egui::Slider::new(&mut point.flow_speed, 0.0..=40.0));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Current Direction (°)");
                                    let mut degrees = point.flow_angle.to_degrees();
                                    if ui
                                        .add(egui::Slider::new(&mut degrees, -180.0..=180.0))
                                        .changed()
                                    {
                                        point.flow_angle = degrees.to_radians();
                                    }
                                });
                            }
                            ui.horizontal(|ui| {
                                ui.label("Tangent X/Z");
                                ui.add(egui::DragValue::new(&mut point.tangent[0]).speed(0.1));
                                ui.add(egui::DragValue::new(&mut point.tangent[1]).speed(0.1));
                            });
                        }
                    }
                }
                if *body != original {
                    let geometry_changed = body.elevation != original.elevation
                        || body.visible != original.visible
                        || body.points != original.points;
                    if geometry_changed {
                        world.terrain.mark_full_render_dirty();
                    }
                    self.needs_save = true;
                    self.push_water_undo(before);
                }
                return;
            }
            self.water_selected_body = None;
            self.water_selected_points.clear();
        }
        if self.selected_coord != self.last_selected_coord {
            self.attribute_add_name.clear();
            self.attribute_add_type = AttributeType::String;
            self.attribute_add_value = AttributeValue::String(String::new());
            self.attribute_add_error = None;
            self.last_selected_coord = self.selected_coord;
        }

        if let Some(source_id) = self.selected_source_id {
            let source_before = world.source(source_id).cloned();
            let audio_browser_target = format!("inspector.source.{source_id}.audio");
            let attachment_node = self.project_hierarchy.find_cell_node(source_id);
            let mut attachment_binding = attachment_node
                .and_then(|node| self.project_hierarchy.cell_attachment(node).cloned());
            let attachment_before = attachment_binding.clone();
            let attachment_actor_name = attachment_binding.as_ref().and_then(|binding| {
                world
                    .actors
                    .get(&binding.parent_actor_id)
                    .map(|actor| actor.identity.clone())
            });
            let attachment_joints: Vec<String> = attachment_binding
                .as_ref()
                .and_then(|binding| world.actors.get(&binding.parent_actor_id))
                .and_then(|actor| actor.package.as_deref())
                .and_then(crate::character::package::resolve_package)
                .map(|package| {
                    package
                        .skeleton
                        .joints
                        .iter()
                        .map(|joint| joint.name.clone())
                        .collect()
                })
                .unwrap_or_default();

            if let Some(source) = world.source_mut(source_id) {
                egui::ScrollArea::vertical()
                    .id_source("prop_scroll_source")
                    .show(ui, |ui| {
                        ui.heading("SOURCE PROPERTIES");
                        ui.separator();

                        egui::CollapsingHeader::new("IDENTITY")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("ID:");
                                    ui.label(
                                        RichText::new(format!("{:08}", source.cell.id)).monospace(),
                                    );
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Type:");
                                    ui.label(format!("{:?}", source.cell.cell_type));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Identity:");
                                    let identity =
                                        source.cell.entity_identity.get_or_insert_with(String::new);
                                    ui.text_edit_singleline(identity);
                                    if identity.trim().is_empty() {
                                        source.cell.entity_identity = None;
                                    }
                                });
                            });

                        egui::CollapsingHeader::new("TRANSFORM   BETA")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.label("Position:");
                                ui.horizontal(|ui| {
                                    ui.label("X:");
                                    ui.add_enabled(
                                        attachment_binding.is_none(),
                                        egui::DragValue::new(&mut source.position.x).speed(0.1),
                                    );
                                    ui.label("Y:");
                                    ui.add_enabled(
                                        attachment_binding.is_none(),
                                        egui::DragValue::new(&mut source.position.y).speed(0.1),
                                    );
                                    ui.label("Z:");
                                    ui.add_enabled(
                                        attachment_binding.is_none(),
                                        egui::DragValue::new(&mut source.position.z).speed(0.1),
                                    );
                                });
                            });

                        if let Some(binding) = attachment_binding.as_mut() {
                            egui::CollapsingHeader::new("ATTACHMENT")
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Parent Actor");
                                        ui.label(
                                            attachment_actor_name
                                                .as_deref()
                                                .unwrap_or("Missing Actor"),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Joint Target");
                                        let selected = match binding.joint_target.as_deref() {
                                            Some(joint)
                                                if !attachment_joints
                                                    .iter()
                                                    .any(|name| name == joint) =>
                                            {
                                                format!("Missing: {joint}")
                                            }
                                            Some(joint) => joint.to_string(),
                                            None => "Actor Origin".to_string(),
                                        };
                                        ui.add_enabled_ui(!attachment_joints.is_empty(), |ui| {
                                            egui::ComboBox::from_id_salt((
                                                "attachment_joint",
                                                source_id,
                                            ))
                                            .selected_text(selected)
                                            .show_ui(
                                                ui,
                                                |ui| {
                                                    ui.selectable_value(
                                                        &mut binding.joint_target,
                                                        None,
                                                        "Actor Origin",
                                                    );
                                                    for joint in &attachment_joints {
                                                        ui.selectable_value(
                                                            &mut binding.joint_target,
                                                            Some(joint.clone()),
                                                            joint,
                                                        );
                                                    }
                                                },
                                            );
                                        });
                                    });
                                    ui.label("Local Origin Offset");
                                    ui.horizontal(|ui| {
                                        for (label, value) in ["X", "Y", "Z"]
                                            .into_iter()
                                            .zip(binding.local_origin_offset.iter_mut())
                                        {
                                            ui.label(format!("{label}:"));
                                            ui.add(egui::DragValue::new(value).speed(0.1));
                                        }
                                    });
                                    let quaternion =
                                        glam::Quat::from_array(binding.local_rotation).normalize();
                                    let (x, y, z) = quaternion.to_euler(glam::EulerRot::XYZ);
                                    let mut degrees =
                                        [x.to_degrees(), y.to_degrees(), z.to_degrees()];
                                    ui.label("Local Rotation");
                                    let mut changed = false;
                                    ui.horizontal(|ui| {
                                        for (label, value) in
                                            ["X", "Y", "Z"].into_iter().zip(degrees.iter_mut())
                                        {
                                            ui.label(format!("{label}:"));
                                            changed |= ui
                                                .add(
                                                    egui::DragValue::new(value)
                                                        .speed(1.0)
                                                        .suffix("°"),
                                                )
                                                .changed();
                                        }
                                    });
                                    if changed {
                                        binding.local_rotation = glam::Quat::from_euler(
                                            glam::EulerRot::XYZ,
                                            degrees[0].to_radians(),
                                            degrees[1].to_radians(),
                                            degrees[2].to_radians(),
                                        )
                                        .to_array();
                                    }
                                });
                        }

                        match source.cell.cell_type {
                            crate::world::CellType::Light => {
                                egui::CollapsingHeader::new("LIGHT")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        ui.checkbox(&mut source.cell.light_enabled, "Enabled");
                                        ui.horizontal(|ui| {
                                            ui.label("Color:");
                                            let mut color = source.cell.light_color.to_array();
                                            if ui.color_edit_button_rgb(&mut color).changed() {
                                                source.cell.light_color =
                                                    glam::Vec3::from_array(color);
                                            }
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Intensity:");
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut source.cell.light_intensity,
                                                )
                                                .speed(0.1)
                                                .range(0.0..=f32::MAX),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Range:");
                                            ui.add(
                                                egui::DragValue::new(&mut source.cell.light_range)
                                                    .speed(0.1)
                                                    .range(0.0..=f32::MAX),
                                            );
                                        });
                                        ui.checkbox(&mut source.cell.light_shadows, "Shadows");
                                    });
                            }
                            crate::world::CellType::AudioEmitter => {
                                egui::CollapsingHeader::new("AUDIO")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        tools::draw_audio_edit(
                                            ui,
                                            &mut source.cell.audio,
                                            project_path,
                                            &audio_browser_target,
                                        );
                                        ui.checkbox(&mut source.cell.playing, "Playing");
                                        ui.checkbox(&mut source.cell.looped, "Looped");
                                        ui.horizontal(|ui| {
                                            ui.label("Loop Interval (s):");
                                            ui.label(
                                                RichText::new("BETA")
                                                    .small()
                                                    .monospace()
                                                    .strong()
                                                    .color(egui::Color32::from_rgb(244, 196, 96)),
                                            );
                                            ui.add(
                                                egui::DragValue::new(
                                                    &mut source.cell.loop_interval,
                                                )
                                                .speed(0.1)
                                                .range(0.0..=3600.0),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("Volume:");
                                            ui.add(
                                                egui::DragValue::new(&mut source.cell.volume)
                                                    .speed(0.05)
                                                    .range(0.0..=2.0),
                                            );
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("MinDistance");
                                            if ui
                                                .add(
                                                    egui::DragValue::new(
                                                        &mut source.cell.audio_min_distance,
                                                    )
                                                    .speed(0.5)
                                                    .range(0.0..=source.cell.audio_max_distance),
                                                )
                                                .changed()
                                            {
                                                source.cell.audio_min_distance = source
                                                    .cell
                                                    .audio_min_distance
                                                    .max(0.0)
                                                    .min(source.cell.audio_max_distance);
                                            }
                                        });
                                        ui.horizontal(|ui| {
                                            ui.label("MaxDistance");
                                            if ui
                                                .add(
                                                    egui::DragValue::new(
                                                        &mut source.cell.audio_max_distance,
                                                    )
                                                    .speed(1.0)
                                                    .range(
                                                        source.cell.audio_min_distance..=100_000.0,
                                                    ),
                                                )
                                                .changed()
                                            {
                                                source.cell.audio_max_distance = source
                                                    .cell
                                                    .audio_max_distance
                                                    .max(source.cell.audio_min_distance);
                                            }
                                        });
                                    });
                            }
                            crate::world::CellType::ParticleEmitter => {
                                egui::CollapsingHeader::new("PARTICLES")
                                    .default_open(true)
                                    .show(ui, |ui| {
                                        tools::draw_particle_settings(
                                            ui,
                                            &mut source.cell.particles,
                                            project_path.as_deref(),
                                        );
                                    });
                            }
                            _ => {}
                        }
                    });
            }

            if attachment_binding != attachment_before {
                if let (Some(node_id), Some(binding)) = (attachment_node, attachment_binding) {
                    if let Some(crate::project::ProjectNode {
                        kind: crate::project::ProjectNodeKind::Cell { attachment, .. },
                        ..
                    }) = self.project_hierarchy.node_mut(node_id)
                    {
                        *attachment = Some(binding);
                        self.needs_save = true;
                    }
                }
            }

            if source_before.as_ref() != world.source(source_id) {
                self.needs_save = true;
            }
            return;
        }

        let selected_actor_ids = self.selected_actor_ids();
        if selected_actor_ids.len() > 1 {
            let actors: Vec<_> = selected_actor_ids
                .iter()
                .filter_map(|id| world.actors.get(id))
                .collect();
            ui.heading(format!("{} Actors Selected", actors.len()));
            ui.label("Common properties (Mixed means values differ).");
            ui.separator();
            let all_visible = actors.iter().all(|actor| actor.visible);
            let any_visible = actors.iter().any(|actor| actor.visible);
            let all_enabled = actors.iter().all(|actor| actor.enabled);
            let any_enabled = actors.iter().any(|actor| actor.enabled);
            let all_collision = actors.iter().all(|actor| actor.physics.collision_enabled);
            let any_collision = actors.iter().any(|actor| actor.physics.collision_enabled);
            let mut change = None;
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Visible: {}",
                    if all_visible {
                        "On"
                    } else if any_visible {
                        "Mixed"
                    } else {
                        "Off"
                    }
                ));
                if ui.button("On").clicked() {
                    change = Some((0, true));
                }
                if ui.button("Off").clicked() {
                    change = Some((0, false));
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Enabled: {}",
                    if all_enabled {
                        "On"
                    } else if any_enabled {
                        "Mixed"
                    } else {
                        "Off"
                    }
                ));
                if ui.button("On").clicked() {
                    change = Some((1, true));
                }
                if ui.button("Off").clicked() {
                    change = Some((1, false));
                }
            });
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Collision: {}",
                    if all_collision {
                        "On"
                    } else if any_collision {
                        "Mixed"
                    } else {
                        "Off"
                    }
                ));
                if ui.button("On").clicked() {
                    change = Some((2, true));
                }
                if ui.button("Off").clicked() {
                    change = Some((2, false));
                }
            });
            let mut transform_change = None;
            if let Some(primary) = actors.first() {
                ui.separator();
                ui.label("Shared transform values (editing an axis sets it for all).");
                for axis in 0..3 {
                    let axis_name = ["X", "Y", "Z"][axis];
                    let original = primary.position[axis];
                    let mixed = actors
                        .iter()
                        .any(|actor| (actor.position[axis] - original).abs() > 0.0001);
                    let mut value = original;
                    ui.horizontal(|ui| {
                        ui.label(format!("Position {axis_name}:"));
                        if mixed {
                            ui.label("Mixed");
                        }
                        if ui
                            .add(egui::DragValue::new(&mut value).speed(0.1))
                            .changed()
                        {
                            transform_change = Some((0, axis, value));
                        }
                    });
                }
                for axis in 0..3 {
                    let axis_name = ["X", "Y", "Z"][axis];
                    let original = primary.scale[axis];
                    let mixed = actors
                        .iter()
                        .any(|actor| (actor.scale[axis] - original).abs() > 0.0001);
                    let mut value = original;
                    ui.horizontal(|ui| {
                        ui.label(format!("Scale {axis_name}:"));
                        if mixed {
                            ui.label("Mixed");
                        }
                        if ui
                            .add(
                                egui::DragValue::new(&mut value)
                                    .speed(0.01)
                                    .range(0.01..=10000.0),
                            )
                            .changed()
                        {
                            transform_change = Some((1, axis, value));
                        }
                    });
                }
            }
            if let Some((kind, axis, value)) = transform_change {
                let snapshot = self.create_snapshot_without_terrain(world);
                self.history.push_undo_entry(snapshot);
                self.history.redo_stack.clear();
                for id in &selected_actor_ids {
                    if let Some(actor) = world.actors.get_mut(id) {
                        if kind == 0 {
                            actor.position[axis] = value;
                        } else {
                            actor.scale[axis] = value;
                        }
                    }
                }
                self.needs_save = true;
            }
            if let Some((property, value)) = change {
                let snapshot = self.create_snapshot_without_terrain(world);
                self.history.push_undo_entry(snapshot);
                self.history.redo_stack.clear();
                for id in selected_actor_ids {
                    if let Some(actor) = world.actors.get_mut(&id) {
                        match property {
                            0 => actor.visible = value,
                            1 => actor.enabled = value,
                            _ => actor.physics.collision_enabled = value,
                        }
                    }
                }
                self.needs_save = true;
            }
            return;
        }

        if let Some(actor_id) = self.selected_actor_id {
            let form_target = egui::Id::new("actor_attribute_form_target");
            if ui.ctx().data(|data| data.get_temp::<u64>(form_target)) != Some(actor_id) {
                self.attribute_add_name.clear();
                self.attribute_add_type = AttributeType::String;
                self.attribute_add_value = AttributeValue::String(String::new());
                self.attribute_add_error = None;
                self.tag_add_name.clear();
                self.tag_add_error = None;
                ui.ctx()
                    .data_mut(|data| data.insert_temp(form_target, actor_id));
            }
            let actor_before = world.actors.get(&actor_id).cloned();
            let mut module_actions: Vec<ActorModuleInspectorAction> = Vec::new();
            let attached_modules = world.actor_modules(actor_id).to_vec();

            {
                if let Some(actor) = world.actors.get_mut(&actor_id) {
                    egui::ScrollArea::vertical()
                        .id_source("prop_scroll_actor")
                        .show(ui, |ui| {
                            self.render_selected_post_process_mask(
                                ui,project_path.as_deref(),
                                crate::project::post_process_pipeline::PostProcessObject {
                                    kind: crate::project::post_process_pipeline::PostProcessObjectKind::Actor,
                                    id:actor_id,
                                }
                            );
                            ui.label(
                                egui::RichText::new(format!("Selected Actor · {}", actor.identity))
                                    .small(),
                            );
                            ui.separator();

                            egui::CollapsingHeader::new("IDENTITY")
                                .id_salt("wo68_actor_identity")
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("ID:");
                                        ui.label(
                                            RichText::new(format!("{:08}", actor.id)).monospace(),
                                        );
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label("Identity:");
                                        ui.text_edit_singleline(&mut actor.identity);
                                    });

                                    ui.checkbox(&mut actor.visible, "Visible");
                                    ui.checkbox(&mut actor.enabled, "Enabled");
                                });

                            egui::CollapsingHeader::new("TRANSFORM")
                                .id_salt("wo68_actor_transform")
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.label("Position:");
                                    ui.horizontal(|ui| {
                                        ui.label("X:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.position.x).speed(0.1),
                                        );
                                        ui.label("Y:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.position.y).speed(0.1),
                                        );
                                        ui.label("Z:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.position.z).speed(0.1),
                                        );
                                    });

                                    ui.label("Rotation:");
                                    let mut rotation_deg = glam::Vec3::new(
                                        actor.rotation.x.to_degrees(),
                                        actor.rotation.y.to_degrees(),
                                        actor.rotation.z.to_degrees(),
                                    );
                                    let rotation_changed = ui
                                        .horizontal(|ui| {
                                            let mut changed = false;
                                            ui.label("X:");
                                            changed |= ui
                                                .add(
                                                    egui::DragValue::new(&mut rotation_deg.x)
                                                        .speed(1.0),
                                                )
                                                .changed();
                                            ui.label("Y:");
                                            changed |= ui
                                                .add(
                                                    egui::DragValue::new(&mut rotation_deg.y)
                                                        .speed(1.0),
                                                )
                                                .changed();
                                            ui.label("Z:");
                                            changed |= ui
                                                .add(
                                                    egui::DragValue::new(&mut rotation_deg.z)
                                                        .speed(1.0),
                                                )
                                                .changed();
                                            changed
                                        })
                                        .inner;

                                    if rotation_changed {
                                        actor.rotation = glam::Vec3::new(
                                            rotation_deg.x.to_radians(),
                                            rotation_deg.y.to_radians(),
                                            rotation_deg.z.to_radians(),
                                        );
                                    }

                                    ui.label("Scale:");
                                    ui.horizontal(|ui| {
                                        ui.label("X:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.scale.x)
                                                .speed(0.05)
                                                .clamp_range(0.01..=1000.0),
                                        );
                                        ui.label("Y:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.scale.y)
                                                .speed(0.05)
                                                .clamp_range(0.01..=1000.0),
                                        );
                                        ui.label("Z:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.scale.z)
                                                .speed(0.05)
                                                .clamp_range(0.01..=1000.0),
                                        );
                                    });
                                });

                            egui::CollapsingHeader::new("PACKAGE")
                                .id_salt("wo68_actor_package")
                                .default_open(false)
                                .show(ui, |ui| {
                                    let available = if let Some(path) = project_path {
                                        crate::engine::entity_package::available_package_names()
                                    } else {
                                        crate::project::available_actor_package_names()
                                    };

                                    let package_changed =
                                        super::asset_browser::draw_actor_package_drop(
                                            ui, project_path.as_deref(),
                                            &mut actor.package, &available,
                                        );
                                    let cleared = actor.package.is_some()
                                        && ui.small_button("Clear Actor").clicked();
                                    if cleared {
                                        actor.package = None;
                                    }

                                    if package_changed {
                                        if let Some(pkg_name) = &actor.package {
                                            if let Some(pkg) =
                                                crate::character::package::resolve_package(pkg_name)
                                            {
                                                actor.physics.collision_size = glam::Vec3::new(
                                                    pkg.collision.radius * 2.0,
                                                    pkg.collision.height,
                                                    pkg.collision.radius * 2.0,
                                                );
                                                actor.physics.collision_offset = glam::Vec3::new(
                                                    0.0,
                                                    pkg.collision.height * 0.5,
                                                    0.0,
                                                );
                                            } else if let Some(pkg) =
                                                crate::engine::entity_package::get_package(pkg_name)
                                            {
                                                if let Some((min, max)) = pkg.triangle_mesh_bounds()
                                                {
                                                    crate::engine::collision_geometry::set_actor_package_collision(actor,&pkg);
                                                } else if let Some(char_cap) = &pkg.character {
                                                    actor.physics.collision_size = glam::Vec3::new(
                                                        char_cap.radius * 2.0,
                                                        char_cap.height,
                                                        char_cap.radius * 2.0,
                                                    );
                                                    actor.physics.collision_offset =
                                                        glam::Vec3::new(
                                                            0.0,
                                                            char_cap.height * 0.5,
                                                            0.0,
                                                        );
                                                }
                                            }
                                        }
                                    }
                                });

                            if let Some(package_name)=actor.package.as_deref() {
                                let source_is_glb=project_path.as_deref().is_some_and(|root|{
                                    ["entities","characters"].iter().any(|category|
                                        root.join(".assets").join(category).join(package_name).join("source.glb").is_file())
                                });
                                if source_is_glb {
                                    let mut materials=Vec::<String>::new();
                                    if let Some(pkg)=crate::engine::entity_package::get_package(package_name) {
                                        materials.extend(pkg.appearance.geometry.iter().map(|part|part.material.clone()));
                                    } else if let Some(pkg)=crate::character::package::get_package(package_name) {
                                        materials.extend(pkg.geometry.iter().map(|part|part.material.clone()));
                                    }
                                    materials.sort();materials.dedup();
                                    egui::CollapsingHeader::new("MODEL MATERIALS")
                                        .default_open(true).show(ui,|ui| {
                                        if materials.len()<2 {
                                            ui.small("No alternate source materials for this model.");
                                        } else {
                                            let key=crate::engine::entity_visual::MODEL_MATERIAL_CHOICE;
                                            let mut choice=actor.attributes.get(key)
                                                .and_then(|v|match v {crate::world::AttributeValue::String(s)=>Some(s.clone()),_=>None})
                                                .filter(|name| materials.contains(name))
                                                .unwrap_or_default();
                                            egui::ComboBox::from_id_salt(("wk_material_choice",actor.id))
                                                .selected_text(if choice.is_empty(){"Original materials"}else{&choice})
                                                .show_ui(ui,|ui|{
                                                    ui.selectable_value(&mut choice,String::new(),"Original materials");
                                                    for material in &materials {
                                                        ui.selectable_value(&mut choice,material.clone(),material);
                                                    }
                                                });
                                            if choice.is_empty(){actor.attributes.remove(key);}
                                            else if materials.contains(&choice) {
                                                actor.attributes.insert(key.to_string(),crate::world::AttributeValue::String(choice));
                                            }
                                            ui.small("Uses only materials in this GLB; applies the selected material to the model.");
                                        }
                                    });
                                }
                            }
                            egui::CollapsingHeader::new("PHYSICS")
                                .id_salt("wo68_actor_physics")
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.checkbox(&mut actor.physics.enabled, "Physics Enabled");
                                    ui.checkbox(&mut actor.physics.solid, "Solid");
                                    ui.checkbox(&mut actor.physics.anchored, "Anchored");
                                    ui.checkbox(
                                        &mut actor.physics.collision_enabled,
                                        "Collision Enabled",
                                    );

                                    ui.horizontal(|ui| {
                                        ui.label("Mass:");
                                        ui.add(
                                            egui::DragValue::new(&mut actor.physics.mass)
                                                .speed(0.1)
                                                .clamp_range(0.1..=1000.0),
                                        );
                                    });

                                    ui.separator();
                                    ui.label(RichText::new("COLLIDER").strong());
                                    let old_mode=actor.physics.collision_mode;
                                    egui::ComboBox::from_id_salt(("actor_collision_mode",actor.id))
                                        .selected_text(match crate::engine::collision_geometry::resolved_actor_collision_mode(actor) {
                                            crate::engine::collision_geometry::CollisionMode::Auto => "Asset geometry",
                                            crate::engine::collision_geometry::CollisionMode::Box => "Custom box",
                                            crate::engine::collision_geometry::CollisionMode::Disabled => "No collision",
                                        })
                                        .show_ui(ui,|ui|{
                                            use crate::engine::collision_geometry::CollisionMode;
                                            if ui.selectable_value(&mut actor.physics.collision_mode,CollisionMode::Auto,"Asset geometry").clicked() {actor.physics.collision_mode_explicit=true;}
                                            if ui.selectable_value(&mut actor.physics.collision_mode,CollisionMode::Box,"Custom box").clicked() {actor.physics.collision_mode_explicit=true;}
                                            if ui.selectable_value(&mut actor.physics.collision_mode,CollisionMode::Disabled,"No collision").clicked() {actor.physics.collision_mode_explicit=true;}
                                        });
                                    if old_mode!=actor.physics.collision_mode
                                        && actor.physics.collision_mode==crate::engine::collision_geometry::CollisionMode::Auto {
                                        if let Some(pkg)=actor.package.as_deref()
                                            .and_then(crate::engine::entity_package::get_package) {
                                            crate::engine::collision_geometry::set_actor_package_collision(actor,&pkg);
                                        }
                                    }
                                    if actor.physics.collision_mode==crate::engine::collision_geometry::CollisionMode::Auto {
                                        if let Some(pkg)=actor.package.as_deref()
                                            .and_then(crate::engine::entity_package::get_package) {
                                            let shapes=pkg.collision_shapes();
                                            ui.small(format!("{} collision primitives; highlighted in the viewport",shapes.len()));
                                        }
                                    }
                                    let collision_before=(actor.physics.collision_size,actor.physics.collision_offset);
                                    ui.label("Size:");
                                    ui.horizontal(|ui| {
                                        ui.label("X:");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut actor.physics.collision_size.x,
                                            )
                                            .speed(0.05)
                                            .clamp_range(0.01..=1000.0),
                                        );
                                        ui.label("Y:");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut actor.physics.collision_size.y,
                                            )
                                            .speed(0.05)
                                            .clamp_range(0.01..=1000.0),
                                        );
                                        ui.label("Z:");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut actor.physics.collision_size.z,
                                            )
                                            .speed(0.05)
                                            .clamp_range(0.01..=1000.0),
                                        );
                                    });

                                    ui.label("Offset:");
                                    ui.horizontal(|ui| {
                                        ui.label("X:");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut actor.physics.collision_offset.x,
                                            )
                                            .speed(0.05),
                                        );
                                        ui.label("Y:");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut actor.physics.collision_offset.y,
                                            )
                                            .speed(0.05),
                                        );
                                        ui.label("Z:");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut actor.physics.collision_offset.z,
                                            )
                                            .speed(0.05),
                                        );
                                    });
                                    if collision_before!=(actor.physics.collision_size,actor.physics.collision_offset) {
                                        actor.physics.collision_mode=crate::engine::collision_geometry::CollisionMode::Box;
                                    }
                                });

                            egui::CollapsingHeader::new("ATTRIBUTES")
                                .id_salt("actor_custom_attributes")
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let name_width = (ui.available_width()
                                            - 80.0 - ui.spacing().item_spacing.x).max(20.0);
                                        ui.add(egui::TextEdit::singleline(&mut self.attribute_add_name)
                                            .desired_width(name_width).hint_text("Name"));
                                        let previous = self.attribute_add_type;
                                        egui::ComboBox::from_id_salt("actor_attr_type_combo")
                                            .selected_text(format!("{:?}", self.attribute_add_type))
                                            .width(80.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.attribute_add_type, AttributeType::Number, "Number");
                                                ui.selectable_value(&mut self.attribute_add_type, AttributeType::Bool, "Bool");
                                                ui.selectable_value(&mut self.attribute_add_type, AttributeType::String, "String");
                                            });
                                        if previous != self.attribute_add_type {
                                            self.attribute_add_value = match self.attribute_add_type {
                                                AttributeType::Number => AttributeValue::Number(0.0),
                                                AttributeType::Bool => AttributeValue::Bool(false),
                                                AttributeType::String => AttributeValue::String(String::new()),
                                            };
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        ui.label("Initial Value:");
                                        match &mut self.attribute_add_value {
                                            AttributeValue::Number(value) => { ui.add(egui::DragValue::new(value).speed(0.1)); }
                                            AttributeValue::Bool(value) => { ui.checkbox(value, ""); }
                                            AttributeValue::String(value) => {
                                                ui.add(egui::TextEdit::singleline(value)
                                                    .desired_width(ui.available_width()));
                                            }
                                        }
                                    });
                                    if ui.button("+ Add Attribute").clicked() {
                                        let name = self.attribute_add_name.trim().to_string();
                                        if name.is_empty() {
                                            self.attribute_add_error = Some("Name cannot be empty.".into());
                                        } else if actor.attributes.contains_key(&name) {
                                            self.attribute_add_error = Some(format!("'{}' already exists.", name));
                                        } else {
                                            actor.attributes.insert(name, self.attribute_add_value.clone());
                                            self.attribute_add_name.clear();
                                            self.attribute_add_type = AttributeType::String;
                                            self.attribute_add_value = AttributeValue::String(String::new());
                                            self.attribute_add_error = None;
                                        }
                                    }
                                    if let Some(error) = &self.attribute_add_error {
                                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                                    }
                                    ui.separator();
                                    let mut remove = None;
                                    for (name, value) in &mut actor.attributes {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(RichText::new(name).strong());
                                            ui.label("|");
                                            match value {
                                                AttributeValue::Number(value) => { ui.add(egui::DragValue::new(value).speed(0.1)); }
                                                AttributeValue::Bool(value) => { ui.checkbox(value, ""); }
                                                AttributeValue::String(value) => {
                                                    let width = (ui.available_width() - 64.0).clamp(40.0, 150.0);
                                                    ui.add(egui::TextEdit::singleline(value).desired_width(width));
                                                }
                                            }
                                            if ui.button("Remove").clicked() {
                                                remove = Some(name.clone());
                                            }
                                        });
                                    }
                                    if let Some(name) = remove {
                                        actor.attributes.remove(&name);
                                    }
                                });

                            egui::CollapsingHeader::new("TAGS")
                                .id_salt("actor_tags")
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(egui::TextEdit::singleline(&mut self.tag_add_name)
                                            .desired_width((ui.available_width() - 85.0).max(40.0))
                                            .hint_text("Tag name"));
                                        if ui.button("+ Add Tag").clicked() {
                                            let name = self.tag_add_name.trim().to_string();
                                            if name.is_empty() {
                                                self.tag_add_error = Some("Name cannot be empty.".into());
                                            } else if actor.tags.contains(&name) {
                                                self.tag_add_error = Some(format!("'{}' already exists.", name));
                                            } else {
                                                actor.tags.insert(name);
                                                self.tag_add_name.clear();
                                                self.tag_add_error = None;
                                            }
                                        }
                                    });
                                    if let Some(error) = &self.tag_add_error {
                                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                                    }
                                    if actor.tags.is_empty() {
                                        ui.label("No tags");
                                    }
                                    let mut remove = None;
                                    for tag in &actor.tags {
                                        ui.horizontal(|ui| {
                                            ui.label(tag);
                                            if ui.button("Remove").clicked() {
                                                remove = Some(tag.clone());
                                            }
                                        });
                                    }
                                    if let Some(name) = remove {
                                        actor.tags.remove(&name);
                                    }
                                });

                            egui::CollapsingHeader::new("MODULES")
                                .id_salt("wo68_actor_modules")
                                .default_open(false)
                                .show(ui, |ui| {
                                    let available_modules: Vec<(String, std::path::PathBuf)> = self
                                        .script_editor
                                        .indexed_modules()
                                        .into_iter()
                                        .filter_map(|module| {
                                            let index =
                                                self.script_editor.project_script_index()?;

                                            let relative_path = module
                                                .logical_path()
                                                .to_string_lossy()
                                                .replace('\\', "/");

                                            let absolute_path = index.absolute_path(&module);

                                            Some((relative_path, absolute_path))
                                        })
                                        .collect();

                                    if attached_modules.is_empty() {
                                        ui.label("No Modules attached.");
                                    } else {
                                        for module in &attached_modules {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(&module.module_name).strong(),
                                                );

                                                ui.label(
                                                    RichText::new(&module.script_path)
                                                        .monospace()
                                                        .small(),
                                                );

                                                let mut enabled = module.enabled;

                                                if ui.checkbox(&mut enabled, "Enabled").changed() {
                                                    module_actions.push(
                                                        ActorModuleInspectorAction::SetEnabled(
                                                            module.script_path.clone(),
                                                            enabled,
                                                        ),
                                                    );
                                                }

                                                if ui.button("OPEN").clicked() {
                                                    if let Some(root) = project_path {
                                                        module_actions.push(
                                                            ActorModuleInspectorAction::Open(
                                                                root.join(&module.script_path),
                                                            ),
                                                        );
                                                    }
                                                }

                                                if ui.button("REMOVE").clicked() {
                                                    module_actions.push(
                                                        ActorModuleInspectorAction::Detach(
                                                            module.script_path.clone(),
                                                        ),
                                                    );
                                                }
                                            });
                                        }
                                    }

                                    ui.separator();

                                    let mut module_path = String::new();
                                    if super::asset_browser::draw_project_asset_drop(
                                        ui,
                                        project_path.as_deref(),
                                        &mut module_path,
                                        super::asset_browser::AssetDropKind::Script,
                                    ) {
                                        let eligible = available_modules.iter()
                                            .any(|(path, _)| path.replace(char::from(92), "/")
                                                == module_path);
                                        let already_attached = attached_modules.iter()
                                            .any(|module| module.script_path
                                                .replace(char::from(92), "/") == module_path);
                                        if eligible && !already_attached {
                                            module_actions.push(
                                                ActorModuleInspectorAction::Attach(module_path)
                                            );
                                        }
                                    }
                                    if available_modules.is_empty() {
                                        ui.label("No .aeo Modules found.");
                                    }
                                });
                        });
                }
            }

            if actor_before.as_ref() != world.actors.get(&actor_id) {
                self.needs_save = true;
            }

            for action in module_actions {
                match action {
                    ActorModuleInspectorAction::Attach(script_path) => {
                        let already_attached = world.actor_script_bindings.iter().any(|binding| {
                            binding.actor_id == actor_id
                                && binding.normalized_script_path() == script_path
                        });

                        if !already_attached {
                            world.actor_script_bindings.push(
                                crate::scripting::binding::ActorScriptBinding::new(
                                    actor_id,
                                    script_path,
                                ),
                            );

                            self.needs_save = true;
                        }
                    }

                    ActorModuleInspectorAction::Detach(script_path) => {
                        let before = world.actor_script_bindings.len();

                        world.actor_script_bindings.retain(|binding| {
                            !(binding.actor_id == actor_id
                                && binding.normalized_script_path() == script_path)
                        });

                        if world.actor_script_bindings.len() != before {
                            self.needs_save = true;
                        }
                    }

                    ActorModuleInspectorAction::SetEnabled(script_path, enabled) => {
                        if let Some(binding) =
                            world.actor_script_bindings.iter_mut().find(|binding| {
                                binding.actor_id == actor_id
                                    && binding.normalized_script_path() == script_path
                            })
                        {
                            binding.enabled = enabled;
                            self.needs_save = true;
                        }
                    }

                    ActorModuleInspectorAction::Open(path) => {
                        self.script_editor.open_file(path);
                        self.show_script_workspace = true;
                    }
                }
            }

            world.rebuild_actor_module_index();

            return;
        }

        if let Some(coord) = self.selected_coord {
            let mut cell_opt = world.get(coord).cloned();
            let mut changes = Vec::new();
            let selected_cell_id = cell_opt.as_ref().map(|cell| cell.id).unwrap_or(0);
            let audio_browser_target = format!("inspector.cell.{selected_cell_id}.audio");
            let texture_browser_target = if cell_opt.as_ref().is_some_and(|cell| {
                matches!(
                    cell.cell_type,
                    crate::world::CellType::Block | crate::world::CellType::FxBlock
                )
            }) {
                format!("inspector.block.{selected_cell_id}.texture")
            } else {
                format!("inspector.cell.{selected_cell_id}.texture")
            };

            egui::ScrollArea::vertical()
                .id_source("prop_scroll")
                .show(ui, |ui| {
                    if let Some(cell)=cell_opt.as_ref() {
                        self.render_selected_post_process_mask(
                            ui,project_path.as_deref(),
                            crate::project::post_process_pipeline::PostProcessObject {
                                kind:crate::project::post_process_pipeline::PostProcessObjectKind::Cell,
                                id:cell.id,
                            },
                        );
                    }
                    egui::CollapsingHeader::new("IDENTITY")
                        .default_open(true)
                        .show(ui, |ui| {
                            if let Some(ref mut cell) = cell_opt {
                                ui.horizontal(|ui| {
                                    ui.label("ID:");
                                    ui.label(RichText::new(format!("{:08}", cell.id)).monospace());
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Type:");
                                    ui.label(format!("{:?}", cell.cell_type));
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Identity:");
                                    let mut identity =
                                        cell.entity_identity.clone().unwrap_or_default();
                                    if ui
                                        .add(
                                            egui::TextEdit::singleline(&mut identity)
                                                .desired_width(120.0),
                                        )
                                        .changed()
                                    {
                                        let new_identity = if identity.trim().is_empty() {
                                            None
                                        } else {
                                            Some(identity)
                                        };
                                        cell.entity_identity = new_identity.clone();
                                        changes.push(PropertyChange::EntityIdentity(new_identity));
                                    }
                                });
                            } else {
                                ui.label("Type: Empty");
                            }
                        });

                    egui::CollapsingHeader::new("SCRIPT")
                        .default_open(true)
                        .show(ui, |ui| {
                            if let Some(ref cell) = cell_opt {
                                let identity = if let Some(ref id) = cell.entity_identity {
                                    id.clone()
                                } else {
                                    format!("{:?}", cell.cell_type)
                                };

                                let binding_index = world
                                    .script_bindings
                                    .iter()
                                    .position(|b| b.target_identity == cell.id);

                                ui.horizontal(|ui| {
                                    ui.label("Current Identity:");
                                    ui.label(RichText::new(&identity).monospace());
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Bound Script:");
                                    if let Some(idx) = binding_index {
                                        let script_path = &world.script_bindings[idx].script_path;
                                        let label = RichText::new(script_path).strong();

                                        let mut exists = true;
                                        if let Some(root) = project_path {
                                            if !root.join(script_path).exists() {
                                                exists = false;
                                            }
                                        }

                                        if !exists {
                                            ui.label(
                                                label.color(egui::Color32::from_rgb(255, 100, 100)),
                                            );
                                            ui.label(
                                                RichText::new("[missing/stale]")
                                                    .color(egui::Color32::from_rgb(255, 100, 100))
                                                    .small(),
                                            );
                                        } else {
                                            ui.label(label);
                                        }

                                        ui.add_space(8.0);
                                        let mut enabled = world.script_bindings[idx].enabled;
                                        if ui.checkbox(&mut enabled, "Enabled").changed() {
                                            world.script_bindings[idx].enabled = enabled;
                                            self.needs_save = true;
                                        }
                                    } else {
                                        ui.label("None");
                                    }
                                });

                                ui.add_space(4.0);

                                ui.horizontal(|ui| {
                                    let mut script_path = String::new();
                                    if super::asset_browser::draw_project_asset_drop(
                                        ui,
                                        project_path.as_deref(),
                                        &mut script_path,
                                        super::asset_browser::AssetDropKind::Script,
                                    ) {
                                        let new_binding =
                                            crate::scripting::binding::ScriptBinding::new(
                                                cell.id,
                                                script_path,
                                            );
                                        let binding_index = world
                                            .script_bindings
                                            .iter()
                                            .position(|binding| binding.target_identity == cell.id);
                                        if let Some(index) = binding_index {
                                            world.script_bindings[index] = new_binding;
                                        } else {
                                            world.script_bindings.push(new_binding);
                                        }
                                        self.needs_save = true;
                                    }

                                    let binding_index = world
                                        .script_bindings
                                        .iter()
                                        .position(|b| b.target_identity == cell.id);

                                    if binding_index.is_some()
                                        && ui.button("Remove Script").clicked()
                                    {
                                        if let Some(idx) = world
                                            .script_bindings
                                            .iter()
                                            .position(|b| b.target_identity == cell.id)
                                        {
                                            world.script_bindings.remove(idx);
                                            self.needs_save = true;
                                        }
                                    }
                                });
                            }

                            ui.add_space(8.0);
                            ui.label(RichText::new("All Script Bindings:").small().heading());

                            let mut remove_idx = None;

                            for (idx, b) in world.script_bindings.iter().enumerate() {
                                let cell_at_coord = world
                                    .resolve_cell_id(b.target_identity)
                                    .and_then(|coord| world.get(coord));

                                let identity_display = if let Some(cell) = cell_at_coord {
                                    let name = if let Some(ref id) = cell.entity_identity {
                                        id.clone()
                                    } else {
                                        format!("{:?}", cell.cell_type)
                                    };

                                    format!("{} [ID {}]", name, b.target_identity)
                                } else {
                                    format!("ID {}", b.target_identity)
                                };

                                ui.horizontal(|ui| {
                                    if cell_at_coord.is_none() {
                                        ui.label(
                                            RichText::new(format!(
                                                "[STALE] {} -> {}",
                                                identity_display, b.script_path
                                            ))
                                            .color(egui::Color32::from_rgb(255, 140, 0))
                                            .small(),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new(format!(
                                                "{} -> {}",
                                                identity_display, b.script_path
                                            ))
                                            .color(egui::Color32::GRAY)
                                            .small(),
                                        );
                                    }

                                    if ui
                                        .small_button("Remove")
                                        .on_hover_text("Remove this authored binding")
                                        .clicked()
                                    {
                                        remove_idx = Some(idx);
                                    }
                                });
                            }

                            if let Some(idx) = remove_idx {
                                world.script_bindings.remove(idx);
                                self.needs_save = true;
                            }
                        });

                    if let Some(ref mut cell) = cell_opt {
                        if cell.cell_type == crate::world::CellType::Light {
                            egui::CollapsingHeader::new("LIGHT")
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Type:");
                                        ui.label("Point");
                                    });

                                    let mut enabled = cell.light_enabled;
                                    if ui.checkbox(&mut enabled, "Enabled").changed() {
                                        cell.light_enabled = enabled;
                                        changes.push(PropertyChange::LightEnabled(enabled));
                                    }

                                    ui.horizontal(|ui| {
                                        ui.label("Color:");
                                        let mut color = cell.light_color;

                                        tools::draw_color_edit(
                                            ui,
                                            self,
                                            ColorTarget::PropertyLight(coord),
                                            &mut color,
                                        );

                                        if color != cell.light_color {
                                            cell.light_color = color;
                                            changes.push(PropertyChange::LightColor(color));
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label("Intensity:");
                                        let mut intensity = cell.light_intensity;

                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut intensity)
                                                    .speed(0.1)
                                                    .range(0.0..=f32::MAX),
                                            )
                                            .changed()
                                        {
                                            cell.light_intensity = intensity;
                                            changes.push(PropertyChange::LightIntensity(intensity));
                                        }
                                    });

                                    ui.horizontal(|ui| {
                                        ui.label("Range:");
                                        let mut range = cell.light_range;

                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut range)
                                                    .speed(0.1)
                                                    .range(0.0..=f32::MAX),
                                            )
                                            .changed()
                                        {
                                            cell.light_range = range;
                                            changes.push(PropertyChange::LightRange(range));
                                        }
                                    });

                                    let mut shadows = cell.light_shadows;

                                    if ui.checkbox(&mut shadows, "Shadows").changed() {
                                        cell.light_shadows = shadows;
                                        changes.push(PropertyChange::LightShadows(shadows));
                                    }
                                });
                        }

                        if cell.cell_type == crate::world::CellType::ParticleEmitter {
                            egui::CollapsingHeader::new("PARTICLES")
                                .default_open(true)
                                .show(ui, |ui| {
                                    if tools::draw_particle_settings(
                                        ui,
                                        &mut cell.particles,
                                        project_path.as_deref(),
                                    ) {
                                        changes.push(PropertyChange::Particles(
                                            cell.particles.clone(),
                                        ));
                                    }
                                });
                        }

                        if cell.cell_type == crate::world::CellType::AudioEmitter {
                            egui::CollapsingHeader::new("AUDIO")
                                .default_open(true)
                                .show(ui, |ui| {
                                    let mut audio = cell.audio.clone();

                                    tools::draw_audio_edit(
                                        ui,
                                        &mut audio,
                                        project_path,
                                        &audio_browser_target,
                                    );

                                    if audio != cell.audio {
                                        cell.audio = audio.clone();
                                        changes.push(PropertyChange::Audio(audio));
                                    }

                                    let mut playing = cell.playing;

                                    if ui.checkbox(&mut playing, "Playing").changed() {
                                        cell.playing = playing;
                                        changes.push(PropertyChange::AudioPlaying(playing));
                                    }

                                    let mut looped = cell.looped;

                                    if ui.checkbox(&mut looped, "Looped").changed() {
                                        cell.looped = looped;
                                        changes.push(PropertyChange::AudioLooped(looped));
                                    }

                                    let mut loop_interval = cell.loop_interval;
                                    ui.horizontal(|ui| {
                                        ui.label("Loop Interval (s):");
                                        ui.label(
                                            RichText::new("BETA")
                                                .small()
                                                .monospace()
                                                .strong()
                                                .color(egui::Color32::from_rgb(244, 196, 96)),
                                        );

                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut loop_interval)
                                                    .speed(0.1)
                                                    .range(0.0..=3600.0),
                                            )
                                            .changed()
                                        {
                                            loop_interval = if loop_interval.is_finite() {
                                                loop_interval.max(0.0)
                                            } else {
                                                0.0
                                            };

                                            cell.loop_interval = loop_interval;
                                            changes.push(PropertyChange::AudioLoopInterval(
                                                loop_interval,
                                            ));
                                        }
                                    });

                                    ui.small("0 seconds preserves immediate continuous looping.");

                                    let mut volume = cell.volume;

                                    ui.horizontal(|ui| {
                                        ui.label("Volume:");

                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut volume)
                                                    .speed(0.05)
                                                    .range(0.0..=2.0),
                                            )
                                            .changed()
                                        {
                                            cell.volume = volume;
                                            changes.push(PropertyChange::AudioVolume(volume));
                                        }
                                    });

                                    let mut min_distance = cell.audio_min_distance;
                                    ui.horizontal(|ui| {
                                        ui.label("MinDistance");
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut min_distance)
                                                    .speed(0.5)
                                                    .range(0.0..=cell.audio_max_distance),
                                            )
                                            .changed()
                                        {
                                            min_distance =
                                                min_distance.max(0.0).min(cell.audio_max_distance);
                                            cell.audio_min_distance = min_distance;
                                            changes.push(PropertyChange::AudioMinDistance(
                                                min_distance,
                                            ));
                                        }
                                    });

                                    let mut max_distance = cell.audio_max_distance;
                                    ui.horizontal(|ui| {
                                        ui.label("MaxDistance");
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut max_distance)
                                                    .speed(1.0)
                                                    .range(cell.audio_min_distance..=100_000.0),
                                            )
                                            .changed()
                                        {
                                            max_distance =
                                                max_distance.max(cell.audio_min_distance);
                                            cell.audio_max_distance = max_distance;
                                            changes.push(PropertyChange::AudioMaxDistance(
                                                max_distance,
                                            ));
                                        }
                                    });
                                });
                        }

                        if cell_supports_generic_physics(cell.cell_type) {
                            egui::CollapsingHeader::new("PHYSICS")
                                .default_open(true)
                                .show(ui, |ui| {
                                    let mut solid = cell.solid;

                                    if ui.checkbox(&mut solid, "Solid").changed() {
                                        cell.solid = solid;
                                        changes.push(PropertyChange::Solid(solid));
                                    }

                                    let mut anchored = cell.anchored;

                                    if ui.checkbox(&mut anchored, "Anchored").changed() {
                                        cell.anchored = anchored;
                                        changes.push(PropertyChange::Anchored(anchored));
                                    }

                                    let mut friction = cell.friction();
                                    ui.horizontal(|ui| {
                                        ui.label("Friction");
                                        if ui.add(
                                            egui::DragValue::new(&mut friction)
                                                .speed(0.05)
                                                .range(0.0..=100.0)
                                        ).changed() && cell.set_friction(friction).is_ok() {
                                            changes.push(PropertyChange::Friction(friction));
                                        }
                                    });

                                    let mut collision_events = cell.collision_events_enabled;

                                    if ui
                                        .checkbox(&mut collision_events, "Collision Events")
                                        .changed()
                                    {
                                        cell.collision_events_enabled = collision_events;
                                        changes.push(PropertyChange::CollisionEventsEnabled(
                                            collision_events,
                                        ));
                                    }
                                });
                        }

                        if cell_supports_generic_rendering(cell.cell_type) {
                            egui::CollapsingHeader::new("RENDERING")
                                .default_open(true)
                                .show(ui, |ui| {
                                    let mut visible = cell.visible;

                                    if ui.checkbox(&mut visible, "Visible").changed() {
                                        cell.visible = visible;
                                        changes.push(PropertyChange::Visible(visible));
                                    }

                                    ui.horizontal(|ui| {
                                        ui.label("Color:");

                                        let mut color = cell.color_rgb;

                                        tools::draw_color_edit(
                                            ui,
                                            self,
                                            ColorTarget::PropertyColor(coord),
                                            &mut color,
                                        );

                                        if color != cell.color_rgb {
                                            cell.color_rgb = color;
                                            changes.push(PropertyChange::ColorRgb(color));
                                        }
                                    });

                                    let mut texture = cell.texture.clone();

                                    super::asset_browser::draw_project_asset_drop(
                                        ui,
                                        project_path.as_deref(),
                                        &mut texture,
                                        super::asset_browser::AssetDropKind::SurfaceMaterial,
                                    );

                                    if texture != cell.texture {
                                        cell.texture = texture.clone();
                                        changes.push(PropertyChange::Texture(texture));
                                    }
                                });
                        }

                        egui::CollapsingHeader::new("ATTRIBUTES")
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("+ Add Attribute").clicked() {
                                        let name = self.attribute_add_name.trim().to_string();

                                        if name.is_empty() {
                                            self.attribute_add_error =
                                                Some("Name cannot be empty.".to_string());
                                        } else if cell.attributes.contains_key(&name) {
                                            self.attribute_add_error =
                                                Some(format!("'{}' already exists.", name));
                                        } else {
                                            changes.push(PropertyChange::AttributeSet(
                                                name,
                                                self.attribute_add_value.clone(),
                                            ));

                                            self.attribute_add_name.clear();
                                            self.attribute_add_type = AttributeType::String;
                                            self.attribute_add_value =
                                                AttributeValue::String(String::new());
                                            self.attribute_add_error = None;
                                        }
                                    }

                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.attribute_add_name)
                                            .desired_width(100.0)
                                            .hint_text("Name"),
                                    );

                                    let prev_type = self.attribute_add_type;

                                    egui::ComboBox::from_id_salt("attr_type_combo")
                                        .selected_text(format!("{:?}", self.attribute_add_type))
                                        .width(80.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(
                                                &mut self.attribute_add_type,
                                                AttributeType::Number,
                                                "Number",
                                            );
                                            ui.selectable_value(
                                                &mut self.attribute_add_type,
                                                AttributeType::Bool,
                                                "Bool",
                                            );
                                            ui.selectable_value(
                                                &mut self.attribute_add_type,
                                                AttributeType::String,
                                                "String",
                                            );
                                        });

                                    if self.attribute_add_type != prev_type {
                                        self.attribute_add_value = match self.attribute_add_type {
                                            AttributeType::Number => AttributeValue::Number(0.0),
                                            AttributeType::Bool => AttributeValue::Bool(false),
                                            AttributeType::String => {
                                                AttributeValue::String(String::new())
                                            }
                                        };
                                    }
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Initial Value:");

                                    match &mut self.attribute_add_value {
                                        AttributeValue::Number(n) => {
                                            ui.add(egui::DragValue::new(n).speed(0.1));
                                        }
                                        AttributeValue::Bool(b) => {
                                            ui.checkbox(b, "");
                                        }
                                        AttributeValue::String(s) => {
                                            ui.add(
                                                egui::TextEdit::singleline(s).desired_width(100.0),
                                            );
                                        }
                                    }
                                });

                                if let Some(ref err) = self.attribute_add_error {
                                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                                }

                                ui.separator();

                                let mut attr_to_remove = None;

                                for (name, value) in &cell.attributes {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(name).strong());
                                        ui.add_space(4.0);
                                        ui.label("|");
                                        ui.add_space(4.0);

                                        let mut new_value = value.clone();

                                        let changed = match &mut new_value {
                                            AttributeValue::Number(n) => {
                                                ui.add(egui::DragValue::new(n).speed(0.1)).changed()
                                            }
                                            AttributeValue::Bool(b) => ui.checkbox(b, "").changed(),
                                            AttributeValue::String(s) => {
                                                ui.text_edit_singleline(s).changed()
                                            }
                                        };

                                        if changed {
                                            changes.push(PropertyChange::AttributeSet(
                                                name.clone(),
                                                new_value,
                                            ));
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.button("Remove").clicked() {
                                                    attr_to_remove = Some(name.clone());
                                                }
                                            },
                                        );
                                    });
                                }

                                if let Some(name) = attr_to_remove {
                                    changes.push(PropertyChange::AttributeRemove(name));
                                }
                            });
                    }
                });

            self.apply_property_changes(world, &changes);
        } else {
            if let Some(node) = self
                .selected_project_node_id
                .and_then(|id| self.project_hierarchy.node(id))
            {
                ui.heading("PROJECT ITEM");
                ui.separator();
                match &node.kind {
                    crate::project::ProjectNodeKind::Folder { name } => {
                        ui.label("Folder");
                        ui.label(egui::RichText::new(name).strong());
                        ui.label(format!(
                            "Children: {}",
                            self.project_hierarchy.children_of(Some(node.id)).len()
                        ));
                    }
                    crate::project::ProjectNodeKind::Script { script_path } => {
                        ui.label("Script");
                        ui.monospace(script_path);
                        ui.label("Open and rename scripts through Project Explorer.");
                    }
                    crate::project::ProjectNodeKind::Cell { cell_id, .. } => {
                        ui.label(format!("Cell ID: {cell_id}"));
                        ui.label("This Cell is no longer available in the scene.");
                    }
                    crate::project::ProjectNodeKind::Actor { actor_id } => {
                        ui.label(format!("Actor ID: {actor_id}"));
                        ui.label("Actor is not currently available in the scene.");
                    }
                }
                ui.small(format!("Project node: {}", node.id));
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Select an Actor, block, source, or project item.");
                });
            }
        }

        self.finish_inspector_history_if_idle(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{CellType, World, WorldCoord};
    use glam::Vec3;

    #[test]
    fn selecting_project_folder_displays_actual_project_item_metadata() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let folder = editor
            .project_hierarchy
            .create_folder(None, "My Folder")
            .unwrap();
        let script = editor
            .project_hierarchy
            .create_script_node(Some(folder), "scripts/example.aeo")
            .unwrap();
        editor.selected_project_node_id = Some(folder);
        let ctx = egui::Context::default();
        let render = |editor: &mut Editor, world: &mut World| {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.render_properties_content(ui, world, &None);
                    });
                },
            );
            output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some(t.galley.job.text.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        let folder_labels = render(&mut editor, &mut world);
        assert!(folder_labels.iter().any(|s| s == "PROJECT ITEM"));
        assert!(folder_labels.iter().any(|s| s == "My Folder"));
        assert!(folder_labels.iter().any(|s| s == "Children: 1"));
        editor.selected_project_node_id = Some(script);
        let script_labels = render(&mut editor, &mut world);
        assert!(script_labels.iter().any(|s| s == "scripts/example.aeo"));
    }

    #[test]
    fn special_emitters_hide_generic_rendering_and_physics() {
        for cell_type in [
            CellType::Light,
            CellType::AudioEmitter,
            CellType::ParticleEmitter,
        ] {
            assert!(!cell_supports_generic_rendering(cell_type));
            assert!(!cell_supports_generic_physics(cell_type));
        }

        for cell_type in [
            CellType::Block,
            CellType::FxBlock,
            CellType::Player,
            CellType::SpawnPoint,
        ] {
            assert!(cell_supports_generic_rendering(cell_type));
            assert!(cell_supports_generic_physics(cell_type));
        }
    }

    #[test]
    fn rendered_inspector_omits_cell_transform_but_keeps_actor_transform() {
        let mut world = World::new();
        let cell_coord = WorldCoord::new(1, 2, 3);
        world.set_cell(cell_coord, CellType::Block);
        let mut editor = Editor::new();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));
        let render_text = |editor: &mut Editor, world: &mut World| {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.render_properties_content(ui, world, &None);
                    });
                },
            );
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        editor.select_cell(cell_coord);
        let cell_text = render_text(&mut editor, &mut world);
        assert!(cell_text.contains("IDENTITY"));
        for removed in [
            "TRANSFORM",
            "World Position",
            "APPLY POSITION",
            "Cells remain grid-positioned",
            "Position editing is available",
        ] {
            assert!(
                !cell_text.contains(removed),
                "Cell Inspector still rendered {removed:?}: {cell_text}"
            );
        }

        let actor_id = world.add_actor("Actor", Vec3::new(1.5, 2.0, 3.5));
        editor.select_actor(actor_id);
        let actor_text = render_text(&mut editor, &mut world);
        for preserved in ["TRANSFORM", "Position:", "Rotation:", "Scale:"] {
            assert!(
                actor_text.contains(preserved),
                "Actor Inspector lost {preserved:?}: {actor_text}"
            );
        }

        let source_position = Vec3::new(2.5, 1.125, 55.75);
        let source_id = world.add_source(CellType::Light, source_position);
        editor.select_source(source_id);
        let source_text = render_text(&mut editor, &mut world);
        for preserved in ["SOURCE PROPERTIES", "TRANSFORM", "Position:", "LIGHT"] {
            assert!(
                source_text.contains(preserved),
                "Source Inspector lost {preserved:?}: {source_text}"
            );
        }
        assert_eq!(world.source(source_id).unwrap().position, source_position);
    }

    #[test]
    fn beta_feature_inspector_labels() {
        let mut world = World::new();
        let mut editor = Editor::new();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 900.0));

        let render_text = |editor: &mut Editor, world: &mut World| {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        editor.render_properties_content(ui, world, &None);
                    });
                },
            );
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let light_id = world.add_source(CellType::Light, Vec3::new(1.0, 2.0, 3.0));
        editor.select_source(light_id);
        let light_text = render_text(&mut editor, &mut world);
        assert!(
            light_text.contains("TRANSFORM   BETA")
                || (light_text.contains("TRANSFORM") && light_text.contains("BETA")),
            "Light freeform Transform header must carry BETA badge: {light_text}"
        );

        let audio_id = world.add_source(CellType::AudioEmitter, Vec3::new(4.0, 5.0, 6.0));
        editor.select_source(audio_id);
        let audio_text = render_text(&mut editor, &mut world);
        assert!(
            audio_text.contains("TRANSFORM   BETA")
                || (audio_text.contains("TRANSFORM") && audio_text.contains("BETA")),
            "AudioEmitter freeform Transform header must carry BETA badge: {audio_text}"
        );
        assert!(
            audio_text.contains("Loop Interval") && audio_text.contains("BETA"),
            "AudioEmitter Loop Interval field must carry BETA badge: {audio_text}"
        );

        let particle_id = world.add_source(CellType::ParticleEmitter, Vec3::new(7.0, 8.0, 9.0));
        editor.select_source(particle_id);
        let particle_text = render_text(&mut editor, &mut world);
        assert!(
            particle_text.contains("TRANSFORM   BETA")
                || (particle_text.contains("TRANSFORM") && particle_text.contains("BETA")),
            "ParticleEmitter freeform Transform header must carry BETA badge: {particle_text}"
        );

        let block_coord = WorldCoord::new(10, 10, 10);
        world.set_cell(block_coord, CellType::Block);
        editor.select_cell(block_coord);
        let block_text = render_text(&mut editor, &mut world);
        assert!(
            !block_text.contains("TRANSFORM"),
            "Block cell must omit TRANSFORM section: {block_text}"
        );
        assert!(
            !block_text.contains("BETA"),
            "Block cell must not carry BETA badges: {block_text}"
        );

        let spawn_coord = WorldCoord::new(20, 20, 20);
        world.set_cell(spawn_coord, CellType::SpawnPoint);
        editor.select_cell(spawn_coord);
        let spawn_text = render_text(&mut editor, &mut world);
        assert!(
            !spawn_text.contains("BETA"),
            "Spawn Point cell must not carry BETA badges: {spawn_text}"
        );

        let actor_id = world.add_actor("TestActor", Vec3::new(1.0, 1.0, 1.0));
        editor.select_actor(actor_id);
        let actor_text = render_text(&mut editor, &mut world);
        assert!(
            actor_text.contains("TRANSFORM"),
            "Actor Inspector must retain stable TRANSFORM section: {actor_text}"
        );
        assert!(
            !actor_text.contains("TRANSFORM   BETA"),
            "Actor TRANSFORM section must NOT be marked BETA: {actor_text}"
        );
    }

    #[test]
    fn test_inspector_script_binding_lookup() {
        let mut world = crate::world::World::new();

        let coord = crate::world::WorldCoord::new(1, 1, 1);
        let cell_id = world.set_cell(coord, crate::world::CellType::Player);

        let binding = world
            .script_bindings
            .iter()
            .find(|b| b.target_identity == cell_id);
        assert!(binding.is_none());

        world
            .script_bindings
            .push(crate::scripting::binding::ScriptBinding::new(
                cell_id,
                "scripts/test.aeo",
            ));

        let binding = world
            .script_bindings
            .iter()
            .find(|b| b.target_identity == cell_id);
        assert!(binding.is_some());
        assert_eq!(binding.unwrap().script_path, "scripts/test.aeo");
    }

    #[test]
    fn test_script_binding_removal() {
        let mut world = crate::world::World::new();
        let cell_id = 12345678;

        world
            .script_bindings
            .push(crate::scripting::binding::ScriptBinding::new(
                cell_id,
                "scripts/nonexistent.aeo",
            ));

        assert!(
            world
                .script_bindings
                .iter()
                .any(|b| b.target_identity == cell_id)
        );

        if let Some(idx) = world
            .script_bindings
            .iter()
            .position(|b| b.target_identity == cell_id)
        {
            world.script_bindings.remove(idx);
        }

        assert!(
            !world
                .script_bindings
                .iter()
                .any(|b| b.target_identity == cell_id)
        );
    }

    #[test]
    fn test_multi_select_property_application_regression() {
        let mut world = World::new();

        let coord_a = WorldCoord::new(1, 0, 0);
        let coord_b = WorldCoord::new(2, 0, 0);
        let coord_c = WorldCoord::new(3, 0, 0);
        let coord_d = WorldCoord::new(4, 0, 0);
        let coord_x = WorldCoord::new(9, 9, 9);

        world.set_cell(coord_a, CellType::Block);
        world.set_cell(coord_b, CellType::Block);
        world.set_cell(coord_c, CellType::Block);
        world.set_cell(coord_d, CellType::Block);
        world.set_cell(coord_x, CellType::Block);

        if let Some(cell) = world.get_mut(coord_a) {
            cell.texture = "tex_a".to_string();
            cell.color_rgb = Vec3::new(0.1, 0.1, 0.1);
            cell.solid = true;
            cell.anchored = false;
        }

        if let Some(cell) = world.get_mut(coord_b) {
            cell.texture = "tex_b".to_string();
            cell.color_rgb = Vec3::new(0.2, 0.2, 0.2);
            cell.solid = false;
            cell.anchored = true;
        }

        if let Some(cell) = world.get_mut(coord_c) {
            cell.texture = "tex_c".to_string();
            cell.color_rgb = Vec3::new(0.3, 0.3, 0.3);
            cell.solid = true;
            cell.anchored = true;
        }

        if let Some(cell) = world.get_mut(coord_d) {
            cell.texture = "tex_d".to_string();
            cell.color_rgb = Vec3::new(0.4, 0.4, 0.4);
            cell.solid = false;
            cell.anchored = false;
        }

        if let Some(cell) = world.get_mut(coord_x) {
            cell.texture = "tex_x".to_string();
            cell.color_rgb = Vec3::new(0.9, 0.9, 0.9);
            cell.solid = true;
            cell.anchored = true;
        }

        let mut editor = Editor::new();
        editor.selected_coords = vec![coord_d, coord_b, coord_a, coord_c];
        editor.selected_coord = Some(coord_c);

        assert_ne!(world.get(coord_a).unwrap().texture, "target_tex");
        assert_ne!(world.get(coord_b).unwrap().texture, "target_tex");
        assert_ne!(world.get(coord_c).unwrap().texture, "target_tex");
        assert_ne!(world.get(coord_d).unwrap().texture, "target_tex");
        assert_ne!(world.get(coord_x).unwrap().texture, "target_tex");

        let changes = vec![
            PropertyChange::Texture("target_tex".to_string()),
            PropertyChange::ColorRgb(Vec3::new(0.7, 0.7, 0.7)),
            PropertyChange::Solid(true),
            PropertyChange::Anchored(true),
        ];

        editor.apply_property_changes(&mut world, &changes);

        let cell_a = world.get(coord_a).unwrap();
        assert_eq!(cell_a.texture, "target_tex");
        assert_eq!(cell_a.color_rgb, Vec3::new(0.7, 0.7, 0.7));
        assert_eq!(cell_a.solid, true);
        assert_eq!(cell_a.anchored, true);

        let cell_b = world.get(coord_b).unwrap();
        assert_eq!(cell_b.texture, "target_tex");
        assert_eq!(cell_b.color_rgb, Vec3::new(0.7, 0.7, 0.7));
        assert_eq!(cell_b.solid, true);
        assert_eq!(cell_b.anchored, true);

        let cell_c = world.get(coord_c).unwrap();
        assert_eq!(cell_c.texture, "target_tex");
        assert_eq!(cell_c.color_rgb, Vec3::new(0.7, 0.7, 0.7));
        assert_eq!(cell_c.solid, true);
        assert_eq!(cell_c.anchored, true);

        let cell_d = world.get(coord_d).unwrap();
        assert_eq!(cell_d.texture, "target_tex");
        assert_eq!(cell_d.color_rgb, Vec3::new(0.7, 0.7, 0.7));
        assert_eq!(cell_d.solid, true);
        assert_eq!(cell_d.anchored, true);

        let cell_x = world.get(coord_x).unwrap();
        assert_eq!(cell_x.texture, "tex_x");
        assert_eq!(cell_x.color_rgb, Vec3::new(0.9, 0.9, 0.9));
        assert_eq!(cell_x.solid, true);
        assert_eq!(cell_x.anchored, true);

        editor.selected_coords = vec![coord_a];
        editor.selected_coord = Some(coord_a);

        let single_changes = vec![PropertyChange::Texture("single_tex".to_string())];
        editor.apply_property_changes(&mut world, &single_changes);

        assert_eq!(world.get(coord_a).unwrap().texture, "single_tex");
        assert_eq!(world.get(coord_b).unwrap().texture, "target_tex");
        assert_eq!(world.get(coord_c).unwrap().texture, "target_tex");
        assert_eq!(world.get(coord_d).unwrap().texture, "target_tex");
    }

    #[test]
    fn invalid_light_inspector_values_do_not_mutate_or_record_history() {
        let mut world = World::new();
        let light = WorldCoord::new(5, 2, 1);
        let block = WorldCoord::new(6, 2, 1);
        world.set_cell(light, CellType::Light);
        world.set_cell(block, CellType::Block);
        let original_light = world.get(light).unwrap().clone();
        let original_block = world.get(block).unwrap().clone();
        let mut editor = Editor::new();
        editor.selected_coords = vec![light, block];
        editor.selected_coord = Some(light);

        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            editor.apply_property_changes(
                &mut world,
                &[
                    PropertyChange::LightIntensity(invalid),
                    PropertyChange::LightRange(invalid),
                ],
            );
        }
        editor.apply_property_changes(
            &mut world,
            &[
                PropertyChange::LightColor(Vec3::new(f32::NAN, 0.0, 1.0)),
                PropertyChange::LightColor(Vec3::new(0.0, f32::INFINITY, 1.0)),
            ],
        );
        assert_eq!(world.get(light).unwrap(), &original_light);
        assert_eq!(world.get(block).unwrap(), &original_block);
        assert!(editor.pending_inspector_history.is_none());
        editor.commit_pending_inspector_history();
        assert!(editor.history.undo_stack.is_empty());

        editor.apply_property_changes(
            &mut world,
            &[
                PropertyChange::LightIntensity(3.5),
                PropertyChange::LightRange(24.0),
                PropertyChange::LightColor(Vec3::new(0.4, 0.5, 0.6)),
                PropertyChange::LightEnabled(false),
            ],
        );
        let changed = world.get(light).unwrap();
        assert_eq!(changed.light_intensity, 3.5);
        assert_eq!(changed.light_range, 24.0);
        assert_eq!(changed.light_color, Vec3::new(0.4, 0.5, 0.6));
        assert!(!changed.light_enabled);
        assert_eq!(world.get(block).unwrap(), &original_block);
        assert!(editor.pending_inspector_history.is_some());
    }

    #[test]
    fn inspector_property_updates_share_one_pending_world_transaction() {
        let mut world = World::new();
        let coord = WorldCoord::new(3, 1, 2);
        world.set_cell(coord, CellType::Block);
        let mut editor = Editor::new();
        editor.select_cell(coord);
        let redo_seed = editor.create_snapshot_without_terrain(&world);
        editor.history.redo_stack.push(redo_seed);

        editor.apply_property_changes(&mut world, &[PropertyChange::Texture("first.png".into())]);
        editor.apply_property_changes(
            &mut world,
            &[PropertyChange::ColorRgb(Vec3::new(0.2, 0.3, 0.4))],
        );

        assert!(editor.history.undo_stack.is_empty());
        assert!(editor.pending_inspector_history.is_some());
        assert!(editor.needs_save);

        editor.commit_pending_inspector_history();
        assert_eq!(editor.history.undo_stack.len(), 1);
        assert!(editor.history.redo_stack.is_empty());
        assert_eq!(world.get(coord).unwrap().texture, "first.png");
        assert_eq!(
            world.get(coord).unwrap().color_rgb,
            Vec3::new(0.2, 0.3, 0.4)
        );

        editor.apply_property_changes(&mut world, &[PropertyChange::Texture("first.png".into())]);
        editor.commit_pending_inspector_history();
        assert_eq!(
            editor.history.undo_stack.len(),
            1,
            "no-op edits add no history"
        );
    }

    #[test]
    fn test_additive_multi_select() {
        let mut world = World::new();

        let c1 = WorldCoord::new(1, 0, 0);
        let c2 = WorldCoord::new(2, 0, 0);
        let c3 = WorldCoord::new(3, 0, 0);

        world.set_cell(c1, CellType::Block);
        world.set_cell(c2, CellType::Block);
        world.set_cell(c3, CellType::Block);

        let mut editor = Editor::new();

        editor.selected_coords = vec![c1];
        editor.selected_coord = Some(c1);

        if !editor.selected_coords.contains(&c2) {
            editor.selected_coords.push(c2);
            editor.selected_coord = Some(c2);
        }

        assert_eq!(editor.selected_coords, vec![c1, c2]);

        if !editor.selected_coords.contains(&c3) {
            editor.selected_coords.push(c3);
            editor.selected_coord = Some(c3);
        }

        assert_eq!(editor.selected_coords, vec![c1, c2, c3]);

        editor.selected_coords.retain(|c| *c != c2);

        assert_eq!(editor.selected_coords, vec![c1, c3]);
    }
}
