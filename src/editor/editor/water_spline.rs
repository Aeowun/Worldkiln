// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::transform_gizmo::{TransformMode, project};
use super::{Editor, EditorTool, RightPanelTab};
use crate::renderer::camera::CameraController;
use crate::world::World;
use crate::world::terrain::{WaterBody, WaterBodyKind, WaterControlPoint};
use egui::{Align2, Color32, Context, Pos2, Rect, Stroke, Vec2};
use glam::Vec3;

#[cfg(test)]
mod unified_terrain_water_ui_tests {
    use super::*;
    #[test]
    fn unified_terrain_shore_undo_restores_ocean_and_body_without_remeshing() {
        let mut world = World::new();
        let mut editor = Editor::new();
        let custom = world.terrain.config.add_material_layer();
        let before = WaterEditSnapshot::capture(&world.terrain.config, None);
        world.terrain.config.shore_material_layer = custom.clone();
        editor.push_water_undo(before);
        let revision = world.terrain.full_render_revision();
        assert!(editor.water_undo_redo(&mut world, false));
        assert_eq!(world.terrain.config.shore_material_layer, "subsurface");
        assert!(editor.water_undo_redo(&mut world, true));
        assert_eq!(world.terrain.config.shore_material_layer, custom);
        assert_eq!(world.terrain.full_render_revision(), revision);
        world.terrain.config.water_bodies.push(WaterBody::new(
            1,
            WaterBodyKind::Lake,
            [0.0, 0.0],
            0.0,
        ));
        editor.water_select(1);
        let before = WaterEditSnapshot::capture(&world.terrain.config, Some(1));
        world.terrain.config.water_bodies[0].shore_material_layer = custom.clone();
        editor.push_water_undo(before);
        assert!(editor.water_undo_redo(&mut world, false));
        assert_eq!(
            world.terrain.config.water_bodies[0].shore_material_layer,
            "subsurface"
        );
        assert_eq!(world.terrain.full_render_revision(), revision);
    }
    #[test]
    fn unified_terrain_lake_and_river_properties_show_shared_shore_targets() {
        let mut world = World::new();
        let mut editor = Editor::new();
        let ctx = egui::Context::default();
        for kind in [WaterBodyKind::Lake, WaterBodyKind::River] {
            world.terrain.config.water_bodies = vec![WaterBody::new(1, kind, [0.0, 0.0], 0.0)];
            editor.water_select(1);
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.render_properties_content(ui, &mut world, &None)
                });
            });
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some(t.galley.text())
                    } else {
                        None
                    }
                })
                .collect();
            assert!(labels.contains(&"Shore Material"));
            assert!(labels.contains(&"Subsurface"));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum WaterHit {
    Point(u64, usize),
    Segment(u64, usize, f32),
    Body(u64),
}

#[derive(Clone, Debug)]
pub(crate) struct WaterSplineDrag {
    pub before: WaterEditSnapshot,
    pub body_id: u64,
    pub indices: Vec<usize>,
    pub initial_points: Vec<WaterControlPoint>,
    pub preview_points: Vec<WaterControlPoint>,
    pub start_pointer: Pos2,
    pub mode: TransformMode,
    pub plane_y: f32,
    pub extruded: bool,
    pub axis_lock: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WaterEditSnapshot {
    pub bodies: Vec<WaterBody>,
    pub ocean_enabled: bool,
    pub ocean_level: f32,
    pub ocean_appearance: crate::world::terrain::TerrainWaterSettings,
    pub custom_water_preset: Option<crate::world::terrain::TerrainWaterSettings>,
    pub ocean_infinite: bool,
    pub ocean_material: Option<String>,
    pub ocean_shore: String,
    pub selection: Option<u64>,
}
impl WaterEditSnapshot {
    pub fn capture(config: &crate::world::terrain::TerrainConfig, selection: Option<u64>) -> Self {
        Self {
            bodies: config.water_bodies.clone(),
            ocean_enabled: config.ocean_enabled,
            ocean_level: config.plains.water_level,
            ocean_appearance: config.water,
            custom_water_preset: config.water_custom_preset,
            ocean_infinite: config.infinite_ocean,
            ocean_material: config.water_material_asset.clone(),
            ocean_shore: config.shore_material_layer.clone(),
            selection,
        }
    }
    fn apply(self, config: &mut crate::world::terrain::TerrainConfig) -> Option<u64> {
        config.water_bodies = self.bodies;
        config.ocean_enabled = self.ocean_enabled;
        config.plains.water_level = self.ocean_level;
        config.water = self.ocean_appearance;
        config.water_custom_preset = self.custom_water_preset;
        config.infinite_ocean = self.ocean_infinite;
        config.water_material_asset = self.ocean_material;
        config.shore_material_layer = config.resolved_layer_id(&self.ocean_shore).to_owned();
        for body in &mut config.water_bodies {
            if let Some(id) = config
                .material_role_replacements
                .get(&body.shore_material_layer)
            {
                body.shore_material_layer = id.clone();
            }
        }
        self.selection
            .filter(|id| config.water_bodies.iter().any(|b| b.id == *id))
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WaterSegmentMenu {
    pub body_id: u64,
    pub segment: usize,
    pub t: f32,
    pub screen: Pos2,
}

fn permitted(editor: &Editor) -> bool {
    editor.mode == crate::engine::EditorMode::Editor
        && editor.current_tool == EditorTool::Select
        && !editor.ui_mode
        && !editor.show_script_workspace
        && !editor.navigation.surface.is_settings()
        && editor.viewport_rect.is_positive()
}
fn point_screen(
    cam: &CameraController,
    viewport: Rect,
    point: WaterControlPoint,
    elevation: f32,
    river: bool,
) -> Option<Pos2> {
    let y = elevation + if river { point.height_offset } else { 0.0 };
    project(
        cam,
        viewport,
        Vec3::new(point.position[0], y, point.position[1]),
    )
}
fn move_axes(
    cam: &CameraController,
    viewport: Rect,
    point: WaterControlPoint,
    elevation: f32,
    river: bool,
) -> Option<(Pos2, Vec2, Vec2)> {
    let y = elevation + if river { point.height_offset } else { 0.0 };
    let position = Vec3::new(point.position[0], y, point.position[1]);
    let center = project(cam, viewport, position)?;
    let x = project(cam, viewport, position + Vec3::X)?;
    let z = project(cam, viewport, position + Vec3::Z)?;
    let screen_x = x - center;
    let screen_z = z - center;
    if screen_x.length_sq() < 0.0001 || screen_z.length_sq() < 0.0001 {
        return None;
    }
    Some((
        center,
        screen_x.normalized() * 28.0,
        screen_z.normalized() * 28.0,
    ))
}
fn move_handle_axis(
    cam: &CameraController,
    viewport: Rect,
    point: WaterControlPoint,
    elevation: f32,
    river: bool,
    pointer: Pos2,
) -> Option<usize> {
    let (center, x, z) = move_axes(cam, viewport, point, elevation, river)?;
    let a = segment_distance(pointer, center, center + x);
    let b = segment_distance(pointer, center, center + z);
    let x_hit = a.1 >= 0.28 && a.0 <= 8.0;
    let z_hit = b.1 >= 0.28 && b.0 <= 8.0;
    match (x_hit, z_hit) {
        (true, true) => Some(if a.0 <= b.0 { 0 } else { 1 }),
        (true, false) => Some(0),
        (false, true) => Some(1),
        _ => None,
    }
}
fn segment_distance(pointer: Pos2, a: Pos2, b: Pos2) -> (f32, f32) {
    let axis = b - a;
    let denom = axis.length_sq();
    if denom < 0.0001 {
        return (f32::MAX, 0.0);
    }
    let t = ((pointer - a).dot(axis) / denom).clamp(0.0, 1.0);
    ((pointer - (a + axis * t)).length(), t)
}
pub(crate) fn hit_test(editor: &Editor, world: &World, pointer: Pos2) -> Option<WaterHit> {
    if !permitted(editor) || !editor.viewport_rect.contains(pointer) {
        return None;
    }
    let bodies = &world.terrain.config.water_bodies;
    if let Some(id) = editor.water_selected_body {
        if let Some(body) = bodies.iter().find(|b| b.id == id) {
            for &j in &editor.water_selected_points {
                let Some(point) = body.points.get(j).copied() else {
                    continue;
                };
                let Some(center) = point_screen(
                    &editor.camera,
                    editor.viewport_rect,
                    point,
                    body.elevation,
                    body.kind == WaterBodyKind::River,
                ) else {
                    continue;
                };
                let delta = pointer - center;
                let dx = delta.x.abs();
                let dy = delta.y.abs();
                let hit = match editor.transform_mode {
                    TransformMode::Move => move_handle_axis(
                        &editor.camera,
                        editor.viewport_rect,
                        point,
                        body.elevation,
                        body.kind == WaterBodyKind::River,
                        pointer,
                    )
                    .is_some(),
                    TransformMode::Rotate => (delta.length() - 19.0).abs() <= 7.0,
                    TransformMode::Scale => (dx.max(dy) - 14.5).abs() <= 7.0,
                    TransformMode::Select => false,
                };
                if hit {
                    return Some(WaterHit::Point(id, j));
                }
            }
        }
    }
    let mut best: Option<(f32, WaterHit)> = None;
    for body in bodies.iter().filter(|b| b.visible) {
        let river = body.kind == WaterBodyKind::River;
        for (j, point) in body.points.iter().copied().enumerate() {
            let Some(screen) = point_screen(
                &editor.camera,
                editor.viewport_rect,
                point,
                body.elevation,
                river,
            ) else {
                continue;
            };
            let distance = (pointer - screen).length();
            if distance < 13.0 && best.as_ref().is_none_or(|(d, _)| distance < *d) {
                best = Some((distance, WaterHit::Point(body.id, j)));
            }
        }
    }
    if best.is_some() {
        return best.map(|(_, hit)| hit);
    }
    for body in bodies.iter().filter(|b| b.visible) {
        let samples = body.spline_samples();
        if samples.len() < 2 {
            continue;
        }
        let n = if body.kind == WaterBodyKind::Lake {
            samples.len()
        } else {
            samples.len() - 1
        };
        for i in 0..n {
            let a = samples[i].0;
            let b = samples[(i + 1) % samples.len()].0;
            let ay = body.elevation
                + if body.kind == WaterBodyKind::River {
                    samples[i].2
                } else {
                    0.0
                };
            let by = body.elevation
                + if body.kind == WaterBodyKind::River {
                    samples[(i + 1) % samples.len()].2
                } else {
                    0.0
                };
            let (Some(pa), Some(pb)) = (
                project(
                    &editor.camera,
                    editor.viewport_rect,
                    Vec3::new(a[0], ay, a[1]),
                ),
                project(
                    &editor.camera,
                    editor.viewport_rect,
                    Vec3::new(b[0], by, b[1]),
                ),
            ) else {
                continue;
            };
            let (distance, t) = segment_distance(pointer, pa, pb);
            if distance < 9.0 && best.as_ref().is_none_or(|(d, _)| distance < *d) {
                let segment = (i / 8).min(body.points.len() - 1);
                let fraction = ((i % 8) as f32 + t) / 8.0;
                best = Some((distance, WaterHit::Segment(body.id, segment, fraction)));
            }
        }
    }
    if let Some((_, hit)) = best {
        return Some(hit);
    }
    let viewport = editor.viewport_rect;
    let (origin, direction) = crate::editor::grid::picking::screen_ray(
        pointer.x - viewport.left(),
        pointer.y - viewport.top(),
        viewport.width(),
        viewport.height(),
        &editor.camera,
    )?;
    let mut nearest: Option<(f32, u64)> = None;
    for body in bodies.iter().filter(|b| b.visible) {
        if direction.y.abs() < 0.0001 {
            continue;
        }
        let t = (body.elevation - origin.y) / direction.y;
        if t <= 0.0 || !t.is_finite() {
            continue;
        }
        let q = origin + direction * t;
        if body.signed_coverage(q.x, q.z) > 0.0 && nearest.is_none_or(|(distance, _)| t < distance)
        {
            nearest = Some((t, body.id));
        }
    }
    nearest.map(|(_, id)| WaterHit::Body(id))
}
fn point_plane(editor: &Editor, pos: Pos2, y: f32) -> Option<[f32; 2]> {
    let rect = editor.viewport_rect;
    let (origin, ray) = crate::editor::grid::picking::screen_ray(
        pos.x - rect.left(),
        pos.y - rect.top(),
        rect.width(),
        rect.height(),
        &editor.camera,
    )?;
    if ray.y.abs() < 0.00001 {
        return None;
    }
    let t = (y - origin.y) / ray.y;
    if !t.is_finite() || t <= 0.0 || t > 10000.0 {
        return None;
    }
    let p = origin + ray * t;
    p.is_finite().then_some([p.x, p.z])
}
impl Editor {
    pub(crate) fn water_select(&mut self, id: u64) {
        self.water_selected_body = Some(id);
        self.water_selected_points.clear();
        self.shell.properties_visible = true;
        self.show_properties_window = true;
        self.right_panel_tab = RightPanelTab::Inspector;
        self.current_tool = EditorTool::Select;
        self.transform_mode = TransformMode::Move;
        self.sculpt_tool = crate::world::terrain::SculptTool::None;
        self.selected_actor_id = None;
        self.selected_source_id = None;
        self.selected_coord = None;
        self.selected_coords.clear();
        self.water_segment_menu = None;
        self.water_spline_drag = None;
    }
    pub(crate) fn water_pointer_down(
        &mut self,
        world: &mut World,
        pointer: Pos2,
        ctrl: bool,
        alt: bool,
    ) -> bool {
        let Some(hit) = hit_test(self, world, pointer) else {
            self.water_selected_body = None;
            self.water_selected_points.clear();
            return false;
        };
        let (body_id, index) = match hit {
            WaterHit::Point(id, j) => (id, Some(j)),
            WaterHit::Segment(id, _, _) => (id, None),
            WaterHit::Body(id) => (id, None),
        };
        if self.water_selected_body != Some(body_id) {
            self.water_select(body_id);
        }
        let Some(index) = index else {
            return true;
        };
        if ctrl {
            if let Some(i) = self.water_selected_points.iter().position(|&j| j == index) {
                self.water_selected_points.remove(i);
            } else {
                self.water_selected_points.push(index);
            }
            return true;
        }
        if !self.water_selected_points.contains(&index) {
            self.water_selected_points = vec![index];
        }
        let before = WaterEditSnapshot::capture(&world.terrain.config, self.water_selected_body);
        let mut selected = self.water_selected_points.clone();
        let Some(body) = world
            .terrain
            .config
            .water_bodies
            .iter()
            .find(|body| body.id == body_id)
        else {
            return true;
        };
        let mut initial_points = body.points.clone();
        let extruded =
            alt && self.transform_mode == TransformMode::Move && initial_points.len() < 128;
        if extruded {
            let point = initial_points[index];
            initial_points.insert(index + 1, point);
            selected = vec![index + 1];
            self.water_selected_points = selected.clone();
        }
        let plane_y = body.elevation
            + if body.kind == WaterBodyKind::River {
                initial_points[selected[0]].height_offset
            } else {
                0.0
            };
        let axis_lock = if self.transform_mode == TransformMode::Move {
            move_handle_axis(
                &self.camera,
                self.viewport_rect,
                body.points[index],
                body.elevation,
                body.kind == WaterBodyKind::River,
                pointer,
            )
        } else {
            None
        };
        self.water_spline_drag = Some(WaterSplineDrag {
            before,
            body_id,
            indices: selected,
            preview_points: initial_points.clone(),
            initial_points,
            start_pointer: pointer,
            mode: self.transform_mode,
            plane_y,
            extruded,
            axis_lock,
        });
        true
    }
    pub(crate) fn water_pointer_move(&mut self, world: &mut World, pointer: Pos2) {
        let Some(drag) = self.water_spline_drag.as_ref() else {
            return;
        };
        let Some(body) = world
            .terrain
            .config
            .water_bodies
            .iter()
            .find(|body| body.id == drag.body_id)
        else {
            return;
        };
        let river = body.kind == WaterBodyKind::River;
        let elevation = body.elevation;
        let mode = drag.mode;
        let initial = drag.initial_points.clone();
        let selected = drag.indices.clone();
        let start = drag.start_pointer;
        let y = drag.plane_y;
        let axis_lock = drag.axis_lock;
        let rect = self.viewport_rect;
        let translation = if mode == TransformMode::Move {
            point_plane(self, start, y).zip(point_plane(self, pointer, y))
        } else {
            None
        };
        let anchor_screen = if mode != TransformMode::Move {
            point_screen(&self.camera, rect, initial[selected[0]], elevation, river)
        } else {
            None
        };
        let Some(preview) = self.water_spline_drag.as_mut() else {
            return;
        };
        if mode == TransformMode::Move {
            let Some((a, b)) = translation else {
                return;
            };
            for &j in &selected {
                let base = initial[j].position;
                preview.preview_points[j].position = [
                    (base[0]
                        + if axis_lock == Some(1) {
                            0.0
                        } else {
                            b[0] - a[0]
                        })
                    .clamp(-100000.0, 100000.0),
                    (base[1]
                        + if axis_lock == Some(0) {
                            0.0
                        } else {
                            b[1] - a[1]
                        })
                    .clamp(-100000.0, 100000.0),
                ];
            }
        } else if matches!(mode, TransformMode::Rotate | TransformMode::Scale) {
            let Some(center) = anchor_screen else {
                return;
            };
            let old = start - center;
            let now = pointer - center;
            if old.length() < 0.5 || now.length() < 0.5 {
                return;
            }
            let theta = now.y.atan2(now.x) - old.y.atan2(old.x);
            let scale = (now.length() / old.length()).clamp(0.0, 25.0);
            for &j in &selected {
                let point = initial[j];
                let tangent = point.tangent;
                let magnitude = (tangent[0].hypot(tangent[1])).max(2.0);
                let old_angle = if tangent == [0.0, 0.0] {
                    point.flow_angle
                } else {
                    tangent[1].atan2(tangent[0])
                };
                let angle = if mode == TransformMode::Rotate {
                    old_angle + theta
                } else {
                    old_angle
                };
                let length = if mode == TransformMode::Scale {
                    (magnitude * scale).clamp(0.0, 10000.0)
                } else {
                    magnitude
                };
                preview.preview_points[j].tangent = [length * angle.cos(), length * angle.sin()];
                if mode == TransformMode::Rotate {
                    preview.preview_points[j].flow_angle = angle;
                }
            }
        }
    }
    pub(crate) fn water_finish_drag(&mut self, world: &mut World) {
        let Some(drag) = self.water_spline_drag.take() else {
            return;
        };
        if drag.preview_points == drag.initial_points {
            if drag.extruded {
                self.water_selected_points.clear();
            }
            return;
        }
        let Some(body) = world
            .terrain
            .config
            .water_bodies
            .iter_mut()
            .find(|body| body.id == drag.body_id)
        else {
            return;
        };
        let mut candidate = body.clone();
        candidate.points = drag.preview_points;
        if candidate.validate().is_err() {
            return;
        }
        self.push_water_undo(drag.before);
        *body = candidate;
        world.terrain.mark_full_render_dirty();
        self.needs_save = true;
    }
    pub(crate) fn water_cancel_drag(&mut self, _world: &mut World) {
        if self.water_spline_drag.take().is_some() {
            self.water_selected_points.clear();
        }
    }
    pub(crate) fn water_delete_selected(&mut self, world: &mut World) -> bool {
        let Some(id) = self.water_selected_body else {
            return false;
        };
        if self.water_selected_points.is_empty() {
            return false;
        }
        let Some(body) = world
            .terrain
            .config
            .water_bodies
            .iter_mut()
            .find(|body| body.id == id)
        else {
            return false;
        };
        let min = if body.kind == WaterBodyKind::Lake {
            3
        } else {
            2
        };
        let mut selected = self.water_selected_points.clone();
        selected.sort_unstable();
        selected.dedup();
        if body.points.len() < min + selected.len() {
            return true;
        }
        let before = WaterEditSnapshot::capture(&world.terrain.config, self.water_selected_body);
        let body = world
            .terrain
            .config
            .water_bodies
            .iter_mut()
            .find(|body| body.id == id)
            .unwrap();
        for j in selected.into_iter().rev() {
            body.points.remove(j);
        }
        self.push_water_undo(before);
        self.water_selected_points.clear();
        world.terrain.mark_full_render_dirty();
        self.needs_save = true;
        true
    }
    pub(crate) fn push_water_undo(&mut self, before: WaterEditSnapshot) {
        self.water_undo.push(before);
        if self.water_undo.len() > 64 {
            self.water_undo.remove(0);
        }
        self.water_redo.clear();
    }
    pub(crate) fn water_undo_redo(&mut self, world: &mut World, redo: bool) -> bool {
        let current = WaterEditSnapshot::capture(&world.terrain.config, self.water_selected_body);
        let source = if redo {
            &self.water_redo
        } else {
            &self.water_undo
        };
        let Some(previous) = source.last() else {
            return false;
        };
        let ocean_changed = previous.ocean_enabled != current.ocean_enabled
            || previous.ocean_level != current.ocean_level
            || previous.ocean_material != current.ocean_material
            || previous.ocean_appearance != current.ocean_appearance
            || previous.custom_water_preset != current.custom_water_preset
            || previous.ocean_infinite != current.ocean_infinite
            || previous.ocean_shore != current.ocean_shore;
        if self.water_selected_body.is_none()
            && !ocean_changed
            && previous.bodies == current.bodies
            && !redo
        {
            return false;
        }
        let (source, destination) = if redo {
            (&mut self.water_redo, &mut self.water_undo)
        } else {
            (&mut self.water_undo, &mut self.water_redo)
        };
        let previous = source.pop().unwrap();
        let mut old_geometry = current.bodies.clone();
        let mut new_geometry = previous.bodies.clone();
        for body in old_geometry.iter_mut().chain(new_geometry.iter_mut()) {
            body.appearance = None;
            body.material_asset = None;
            body.shore_material_layer.clear();
        }
        let geometry_changed = old_geometry != new_geometry
            || current.ocean_enabled != previous.ocean_enabled
            || current.ocean_infinite != previous.ocean_infinite
            || current.ocean_level != previous.ocean_level;
        destination.push(current);
        if destination.len() > 64 {
            destination.remove(0);
        }
        self.water_selected_body = previous.apply(&mut world.terrain.config);
        self.water_selected_points.clear();
        self.water_spline_drag = None;
        if geometry_changed {
            world.terrain.mark_full_render_dirty();
        }
        self.needs_save = true;
        true
    }
    pub(crate) fn water_context_down(&mut self, world: &World, pointer: Pos2) -> bool {
        if let Some(WaterHit::Segment(body_id, segment, t)) = hit_test(self, world, pointer) {
            self.water_select(body_id);
            self.water_segment_menu = Some(WaterSegmentMenu {
                body_id,
                segment,
                t,
                screen: pointer,
            });
            true
        } else {
            false
        }
    }
    pub(crate) fn draw_water_spline_overlay(&mut self, ctx: &Context, world: &mut World) {
        if !permitted(self) {
            return;
        }
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("water_spline_gizmo"),
            ))
            .with_clip_rect(self.viewport_rect);
        let accent = ctx.style().visuals.selection.stroke.color;
        let white = Color32::WHITE;
        for authoritative in world
            .terrain
            .config
            .water_bodies
            .iter()
            .filter(|b| b.visible)
        {
            let preview = self
                .water_spline_drag
                .as_ref()
                .filter(|drag| drag.body_id == authoritative.id)
                .map(|drag| {
                    let mut body = authoritative.clone();
                    body.points = drag.preview_points.clone();
                    body
                });
            let body = preview.as_ref().unwrap_or(authoritative);
            let selected = self.water_selected_body == Some(body.id);
            let samples = body.spline_samples();
            let mut projection = Vec::with_capacity(samples.len());
            for (pos, _, _) in &samples {
                projection.push(project(
                    &self.camera,
                    self.viewport_rect,
                    Vec3::new(
                        pos[0],
                        body.elevation
                            + if body.kind == WaterBodyKind::River {
                                samples[projection.len()].2
                            } else {
                                0.0
                            },
                        pos[1],
                    ),
                ));
            }
            let count = if body.kind == WaterBodyKind::Lake {
                projection.len()
            } else {
                projection.len().saturating_sub(1)
            };
            for i in 0..count {
                if let (Some(a), Some(b)) = (projection[i], projection[(i + 1) % projection.len()])
                {
                    painter.line_segment(
                        [a, b],
                        Stroke::new(
                            if selected { 2.2 } else { 0.8 },
                            if selected {
                                accent
                            } else {
                                Color32::from_rgba_unmultiplied(206, 217, 230, 58)
                            },
                        ),
                    );
                }
            }
            if !selected {
                continue;
            }
            for (j, point) in body.points.iter().copied().enumerate() {
                let Some(position) = point_screen(
                    &self.camera,
                    self.viewport_rect,
                    point,
                    body.elevation,
                    body.kind == WaterBodyKind::River,
                ) else {
                    continue;
                };
                let picked = self.water_selected_points.contains(&j);
                painter.circle_filled(
                    position,
                    if picked { 7.5 } else { 5.5 },
                    if picked { accent } else { white },
                );
                painter.circle_stroke(
                    position,
                    if picked { 9.5 } else { 7.0 },
                    Stroke::new(1.0, Color32::BLACK),
                );
                if picked && self.transform_mode == TransformMode::Move {
                    if let Some((center, x, z)) = move_axes(
                        &self.camera,
                        self.viewport_rect,
                        point,
                        body.elevation,
                        body.kind == WaterBodyKind::River,
                    ) {
                        painter.arrow(center, x, Stroke::new(2.0, accent));
                        painter.arrow(center, z, Stroke::new(2.0, accent));
                    }
                } else if picked && self.transform_mode == TransformMode::Rotate {
                    painter.circle_stroke(position, 19.0, Stroke::new(2.0, accent));
                } else if picked && self.transform_mode == TransformMode::Scale {
                    painter.rect_stroke(
                        Rect::from_center_size(position, Vec2::splat(29.0)),
                        0.0,
                        Stroke::new(2.0, accent),
                    );
                }
            }
        }
        if let Some(menu) = self.water_segment_menu {
            let mut add = false;
            let mut close = false;
            egui::Window::new("Water Spline")
                .id(egui::Id::new("water_segment_popup"))
                .fixed_pos(menu.screen)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    add = ui.button("Add Spline Point Here").clicked();
                    close = ui.button("Cancel").clicked();
                });
            if add {
                let before =
                    WaterEditSnapshot::capture(&world.terrain.config, self.water_selected_body);
                if let Some(body) = world
                    .terrain
                    .config
                    .water_bodies
                    .iter_mut()
                    .find(|b| b.id == menu.body_id)
                {
                    if body.points.len() < 128 {
                        if let Some(point) = body.sample_segment(menu.segment, menu.t) {
                            let index = menu.segment + 1;
                            body.points.insert(index, point);
                            self.water_selected_points = vec![index];
                            self.push_water_undo(before);
                            world.terrain.mark_full_render_dirty();
                            self.needs_save = true;
                        }
                    }
                }
            }
            if add || close {
                self.water_segment_menu = None;
            }
        }
        if self.water_selected_body.is_some() {
            painter.text(
                self.viewport_rect.left_top() + Vec2::new(12.0, 12.0),
                Align2::LEFT_TOP,
                "Water Spline: W Move  E Rotate  R Tangents  Ctrl multi-select  Alt+W duplicate",
                egui::FontId::proportional(12.0),
                white,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spline_hit_test_respects_viewport_and_body_point_picking() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let body = WaterBody::new(4, WaterBodyKind::Lake, [0.0, 0.0], 0.0);
        let point = body.points[0];
        world.terrain.config.water_bodies.push(body);
        editor.viewport_rect =
            Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(1280.0, 720.0));
        editor.camera.target = Vec3::ZERO;
        let projected =
            point_screen(&editor.camera, editor.viewport_rect, point, 0.0, false).unwrap();
        assert_eq!(
            hit_test(&editor, &world, projected),
            Some(WaterHit::Point(4, 0))
        );
        assert_eq!(hit_test(&editor, &world, egui::pos2(0.0, 0.0)), None);
    }
    #[test]
    fn water_gizmo_drag_is_undoable_and_alt_click_does_not_leave_duplicate() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let body = WaterBody::new(20, WaterBodyKind::Lake, [0.0, 0.0], 0.0);
        let starting = body.points[0];
        world.terrain.config.water_bodies.push(body);
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        editor.camera.target = Vec3::new(starting.position[0], 0.0, starting.position[1]);
        let p = point_screen(&editor.camera, editor.viewport_rect, starting, 0.0, false).unwrap();
        assert!(editor.water_pointer_down(&mut world, p, false, false));
        editor.water_pointer_move(&mut world, p + egui::vec2(45.0, 0.0));
        assert_eq!(
            world.terrain.config.water_bodies[0].points[0].position, starting.position,
            "drag preview must not trigger a world remesh"
        );
        editor.water_finish_drag(&mut world);
        assert_ne!(
            world.terrain.config.water_bodies[0].points[0].position,
            starting.position
        );
        assert!(editor.water_undo_redo(&mut world, false));
        assert_eq!(
            world.terrain.config.water_bodies[0].points[0].position,
            starting.position
        );
        assert!(editor.water_undo_redo(&mut world, true));
        let moved = world.terrain.config.water_bodies[0].points[0];
        let position =
            point_screen(&editor.camera, editor.viewport_rect, moved, 0.0, false).unwrap();
        assert!(editor.water_pointer_down(&mut world, position, false, true));
        assert_eq!(world.terrain.config.water_bodies[0].points.len(), 4);
        assert_eq!(
            editor
                .water_spline_drag
                .as_ref()
                .unwrap()
                .preview_points
                .len(),
            5
        );
        editor.water_finish_drag(&mut world);
        assert_eq!(
            world.terrain.config.water_bodies[0].points.len(),
            4,
            "Alt click without dragging must not create coincident spline vertices"
        );
    }
    #[test]
    fn water_selection_preserves_terrain_panel_and_supports_ctrl_multi_select() {
        let mut editor = Editor::new();
        let mut world = World::new();
        let body = WaterBody::new(30, WaterBodyKind::Lake, [0.0, 0.0], 0.0);
        world.terrain.config.water_bodies.push(body);
        editor.left_panel_tab = super::super::LeftPanelTab::Terrain;
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        for j in 0..2 {
            let point = world.terrain.config.water_bodies[0].points[j];
            editor.camera.target = Vec3::new(point.position[0], 0.0, point.position[1]);
            let pos =
                point_screen(&editor.camera, editor.viewport_rect, point, 0.0, false).unwrap();
            assert!(editor.water_pointer_down(&mut world, pos, true, false));
        }
        assert_eq!(editor.water_selected_points.len(), 2);
        assert_eq!(editor.left_panel_tab, super::super::LeftPanelTab::Terrain);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
    }

    #[test]
    fn selecting_water_point_does_not_modify_project_until_drag() {
        let mut editor = Editor::new();
        let mut world = World::new();
        world.terrain.config.water_bodies.push(WaterBody::new(
            5,
            WaterBodyKind::River,
            [0.0, 0.0],
            0.0,
        ));
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        let point = world.terrain.config.water_bodies[0].points[0];
        let p = point_screen(&editor.camera, editor.viewport_rect, point, 0.0, true).unwrap();
        let original = world.terrain.config.water_bodies.clone();
        assert!(editor.water_pointer_down(&mut world, p, false, false));
        editor.water_finish_drag(&mut world);
        assert_eq!(world.terrain.config.water_bodies, original);
        assert!(editor.water_undo.is_empty());
    }
}
