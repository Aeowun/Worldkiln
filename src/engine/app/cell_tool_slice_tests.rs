// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::world::terrain::{
    HeightfieldData, SculptTool, TerrainData, TerrainGenerationRequest, TerrainGeneratorType,
    TerrainResult,
};

fn terrain_app(actor: bool) -> App {
    let mut app = App::new_for_tests(800.0, 600.0);
    app.view = View::Editor;
    app.editor.viewport_rect =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    app.editor.viewport_ppp = 1.0;
    app.mouse_pos = (400.0, 300.0);
    app.editor.camera.target = glam::Vec3::new(0.5, 0.0, 0.5);
    let request = TerrainGenerationRequest {
        position: [-32, -8, -32],
        size: [64, 32, 64],
    };
    app.world.terrain.replace_authored(Some(TerrainResult {
        request,
        generator: TerrainGeneratorType::Programmatic,
        generator_implementation_version: 1,
        data: TerrainData::Heightfield(HeightfieldData {
            origin: request.position,
            size: request.size.map(|n| n as u32),
            heights: vec![0.0; 64 * 64],
        }),
    }));
    let hit = crate::editor::grid::picking::raycast_terrain_surface(
        400.0,
        300.0,
        800.0,
        600.0,
        &app.editor.camera,
        &app.world,
    )
    .unwrap();
    let front = hit.point.lerp(app.editor.camera.get_position(), 0.45);
    if actor {
        app.world
            .add_actor("UnloadedTestActor", front - glam::Vec3::Y * 0.5);
    } else {
        app.world.set_cell(
            WorldCoord::new(
                front.x.floor() as i32,
                front.y.floor() as i32,
                front.z.floor() as i32,
            ),
            CellType::Block,
        );
    }
    let scene = crate::editor::grid::picking::raycast_scene(
        400.0,
        300.0,
        800.0,
        600.0,
        &app.editor.camera,
        &app.world,
        true,
    )
    .expect("occluder must intercept scene selection");
    assert!(
        (match scene {
            SceneHit::Cell { distance, .. }
            | SceneHit::Actor { distance, .. }
            | SceneHit::Source { distance, .. } => distance,
        }) < hit.distance
    );
    app
}

#[test]
fn cell_tool_slice_cell_selection_disables_rotate_and_scale_shortcuts() {
    let mut app = App::new_for_tests(800.0, 600.0);
    app.editor.activate_transform_mode(TransformMode::Rotate);
    app.editor.select_cell(WorldCoord::new(0, 0, 0));
    assert_eq!(
        app.editor.transform_mode,
        TransformMode::Move,
        "selecting cells must leave Rotate/Scale and show the Move handles"
    );
    for mode in [TransformMode::Rotate, TransformMode::Scale] {
        app.editor.activate_transform_mode(mode);
        assert_eq!(
            app.editor.transform_mode,
            TransformMode::Move,
            "disabled cell transform must not activate through shortcuts or feature search"
        );
    }
}

#[test]
fn cell_tool_slice_terrain_picker_ignores_actor_and_cell_occluders() {
    for actor in [false, true] {
        let app = terrain_app(actor);
        let expected = crate::editor::grid::picking::raycast_terrain_surface(
            400.0,
            300.0,
            800.0,
            600.0,
            &app.editor.camera,
            &app.world,
        );
        let actual = crate::editor::grid::picking::raycast_sculpt_terrain_surface(
            400.0,
            300.0,
            800.0,
            600.0,
            &app.editor.camera,
            &app.world,
        );
        assert_eq!(
            actual, expected,
            "terrain tool must reach beneath scene occluder, actor={actor}"
        );
    }
}

#[test]
fn cell_tool_slice_all_landscape_tools_suppress_scene_hover() {
    for actor in [false, true] {
        let mut app = terrain_app(actor);
        for tool in [
            SculptTool::Raise,
            SculptTool::Lower,
            SculptTool::Flatten,
            SculptTool::FlattenRaiseOnly,
            SculptTool::FlattenCutOnly,
            SculptTool::Smooth,
            SculptTool::Paint,
            SculptTool::None,
        ] {
            if tool == SculptTool::None {
                app.editor.set_actor_brush_enabled(true);
            } else {
                app.editor.activate_sculpt_tool(tool);
            }
            app.update_hover();
            assert!(
                app.editor.hovered_terrain_hit.is_some(),
                "tool={tool:?}, actor={actor}"
            );
            assert!(app.editor.hovered_actor_id.is_none());
            assert!(app.editor.hovered_cell.is_none());
            assert!(app.editor.hovered_source_id.is_none());
        }
    }
}

fn cell_app(group: bool) -> App {
    let mut app = App::new_for_tests(1200.0, 700.0);
    app.view = View::Editor;
    app.editor.viewport_rect =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 700.0));
    let first = WorldCoord::new(0, 0, 0);
    app.world.set_cell(first, CellType::Block);
    app.editor.select_cell(first);
    if group {
        let second = WorldCoord::new(1, 0, 0);
        app.world.set_cell(second, CellType::Block);
        app.editor.selected_coords.push(second);
    }
    app.editor.activate_transform_mode(TransformMode::Move);
    app
}

fn move_pointer(app: &App, axis: TransformAxis, units: f32, start: egui::Pos2) -> egui::Pos2 {
    let pivot = app.editor.cell_move_pivot(&app.world).unwrap();
    let project = crate::editor::editor::transform_gizmo::project;
    let a = project(&app.editor.camera, app.editor.viewport_rect, pivot).unwrap();
    let b = project(
        &app.editor.camera,
        app.editor.viewport_rect,
        pivot + axis.vector(),
    )
    .unwrap();
    start + (b - a) * units
}

#[test]
fn cell_tool_slice_single_and_group_move_on_all_actor_axes_roundtrip_history() {
    for group in [false, true] {
        for axis in TransformAxis::WORLD_AXES {
            let mut app = cell_app(group);
            let coords = app.editor.selected_coords.clone();
            let ids = coords
                .iter()
                .map(|c| app.world.get(*c).unwrap().id)
                .collect::<Vec<_>>();
            let original_revision = app.world.render_revision();
            let pointer = egui::pos2(600.0, 350.0);
            let end = move_pointer(&app, axis, 2.0, pointer);
            assert!(app.begin_cell_move_preview(axis, pointer));
            app.update_cell_move_preview(end);
            let delta = app.editor.grab_state.as_ref().unwrap().delta;
            assert_eq!(
                glam::Vec3::new(delta.x as f32, delta.y as f32, delta.z as f32),
                axis.vector() * 2.0
            );
            assert_eq!(
                app.world.render_revision(),
                original_revision,
                "preview must not mutate authored world"
            );
            assert!(app.editor.grab_state.as_ref().unwrap().valid);
            assert!(app.commit_cell_move_preview());
            assert_eq!(app.editor.history.undo_stack.len(), 1);
            assert!(!app.commit_cell_move_preview(), "release commits once");
            let moved = coords
                .iter()
                .map(|c| WorldCoord::new(c.x + delta.x, c.y + delta.y, c.z + delta.z))
                .collect::<Vec<_>>();
            for (c, id) in moved.iter().zip(&ids) {
                assert_eq!(app.world.get(*c).unwrap().id, *id);
            }
            assert_eq!(app.editor.selected_coords, moved);
            app.undo_editor_history();
            for (c, id) in coords.iter().zip(&ids) {
                assert_eq!(app.world.get(*c).unwrap().id, *id);
            }
            app.redo_editor_history();
            for (c, id) in moved.iter().zip(&ids) {
                assert_eq!(app.world.get(*c).unwrap().id, *id);
            }
        }
    }
}

#[test]
fn cell_tool_slice_move_cancels_and_rejects_occupied_destinations() {
    let start = egui::pos2(600.0, 350.0);
    let mut app = cell_app(true);
    let end = move_pointer(&app, TransformAxis::X, 2.0, start);
    app.world
        .set_cell(WorldCoord::new(3, 0, 0), CellType::Block);
    let original = app.world.cells.clone();
    assert!(app.begin_cell_move_preview(TransformAxis::X, start));
    app.update_cell_move_preview(end);
    assert!(!app.editor.grab_state.as_ref().unwrap().valid);
    assert!(!app.commit_cell_move_preview());
    assert_eq!(app.world.cells, original);
    assert!(app.editor.history.undo_stack.is_empty());

    assert!(app.begin_cell_move_preview(TransformAxis::Y, start));
    let end = move_pointer(&app, TransformAxis::Y, 2.0, start);
    app.update_cell_move_preview(end);
    app.editor.cancel_cell_move_preview();
    assert!(app.editor.cell_move_preview.is_none());
    assert!(app.editor.grab_state.is_none());
    assert_eq!(app.world.cells, original);
    assert!(app.editor.history.undo_stack.is_empty());
}

#[test]
fn cell_tool_slice_move_allows_overlap_with_own_group_and_small_click_is_noop() {
    let mut app = cell_app(true);
    let start = egui::pos2(600.0, 350.0);
    let end = move_pointer(&app, TransformAxis::X, 1.0, start);
    assert!(app.begin_cell_move_preview(TransformAxis::X, start));
    app.update_cell_move_preview(end);
    assert!(app.editor.grab_state.as_ref().unwrap().valid);
    assert!(app.commit_cell_move_preview());
    assert!(app.world.get(WorldCoord::new(0, 0, 0)).is_none());
    assert!(app.world.get(WorldCoord::new(1, 0, 0)).is_some());
    assert!(app.world.get(WorldCoord::new(2, 0, 0)).is_some());
    let count = app.editor.history.undo_stack.len();
    assert!(app.begin_cell_move_preview(TransformAxis::Y, start));
    app.update_cell_move_preview(start + egui::vec2(1.0, 0.0));
    assert!(!app.commit_cell_move_preview());
    assert_eq!(app.editor.history.undo_stack.len(), count);
}

#[test]
fn cell_tool_slice_actor_move_math_and_cell_move_math_are_identical_before_grid_snap() {
    let app = cell_app(true);
    let pivot = app.editor.cell_move_pivot(&app.world).unwrap();
    let start = egui::pos2(600.0, 350.0);
    for axis in TransformAxis::WORLD_AXES {
        let actor = crate::world::Actor::new(7, "Test", pivot);
        let mut actor_preview =
            ActorTransformPreview::new(&actor, TransformMode::Move, axis, start);
        let cell_preview =
            crate::editor::editor::transform_gizmo::CellMovePreview::new(pivot, axis, start);
        let end = move_pointer(&app, axis, -3.25, start);
        actor_preview.update(end, &app.editor.camera, app.editor.viewport_rect);
        let delta = cell_preview
            .delta(end, &app.editor.camera, app.editor.viewport_rect)
            .unwrap();
        assert_eq!(
            (actor_preview.current_position - pivot).round(),
            glam::Vec3::new(delta.x as f32, delta.y as f32, delta.z as f32)
        );
    }
}

#[test]
fn cell_tool_slice_move_handles_hit_test_and_actor_controls_reenable() {
    let mut app = cell_app(true);
    let pivot = app.editor.cell_move_pivot(&app.world).unwrap();
    let project = crate::editor::editor::transform_gizmo::project;
    let origin = project(&app.editor.camera, app.editor.viewport_rect, pivot).unwrap();
    for axis in TransformAxis::WORLD_AXES {
        let end = project(
            &app.editor.camera,
            app.editor.viewport_rect,
            pivot + axis.vector(),
        )
        .unwrap();
        let direction = (end - origin).normalized();
        assert_eq!(
            hit_test_cells(&app.editor, &app.world, origin + direction * 48.0),
            Some(axis)
        );
    }
    assert!(!app.editor.transform_mode_enabled(TransformMode::Rotate));
    assert!(!app.editor.transform_mode_enabled(TransformMode::Scale));
    let actor = app.world.add_actor("Test", glam::Vec3::ZERO);
    app.editor.select_actor(actor);
    assert!(app.editor.transform_mode_enabled(TransformMode::Rotate));
    assert!(app.editor.transform_mode_enabled(TransformMode::Scale));
    app.editor.activate_transform_mode(TransformMode::Scale);
    assert_eq!(app.editor.transform_mode, TransformMode::Scale);
}

#[test]
fn cell_tool_slice_switching_to_any_terrain_brush_cancels_cell_move() {
    for actor_brush in [false, true] {
        let mut app = cell_app(true);
        let start = egui::pos2(600.0, 350.0);
        assert!(app.begin_cell_move_preview(TransformAxis::X, start));
        if actor_brush {
            app.editor.set_actor_brush_enabled(true);
        } else {
            app.editor.activate_sculpt_tool(SculptTool::Raise);
        }
        assert!(app.editor.cell_move_preview.is_none());
        assert!(app.editor.grab_state.is_none());
        assert!(app.editor.history.undo_stack.is_empty());
    }
}

#[test]
fn cell_tool_slice_numeric_shortcuts_and_ctrl_selection_keep_cell_tools_valid() {
    let mut app = cell_app(false);
    for digit in [3, 4] {
        assert!(app.editor.activate_ribbon_shortcut(digit));
        assert_eq!(app.editor.transform_mode, TransformMode::Move);
    }
    let source = app
        .world
        .add_source(CellType::Light, glam::Vec3::new(10.0, 3.0, 4.0));
    app.editor.select_source(source);
    app.editor.activate_transform_mode(TransformMode::Rotate);
    app.editor.hovered_source_id = None;
    app.editor.hovered_actor_id = None;
    app.editor.hovered_cell = Some(WorldCoord::new(0, 0, 0));
    app.keys_down.insert(KeyCode::ControlLeft);
    app.on_click();
    assert!(app.editor.selected_source_id.is_none());
    assert_eq!(app.editor.transform_mode, TransformMode::Move);
    assert_eq!(
        app.editor.selected_cell_move_coords(&app.world),
        vec![WorldCoord::new(0, 0, 0)]
    );
}

#[test]
fn generated_layer_sculpt_reticle_targets_plains_before_and_after_image_import() {
    use crate::world::terrain::{
        HeightfieldData, TerrainData, TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    };
    let mut app = App::new_for_tests(800.0, 600.0);
    app.view = View::Editor;
    app.editor.viewport_rect =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    app.editor.viewport_ppp = 1.0;
    app.mouse_pos = (400.0, 300.0);
    let plains_id = app
        .world
        .terrain
        .add_generation_layer(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 40, 24],
        })
        .unwrap();
    {
        let layer = &mut app.world.terrain.config.generation_layers[0];
        layer.settings.base_height = 10.0;
        layer.settings.relief = 0.0;
        layer.settings.water_level = 0.0;
    }
    app.world
        .terrain
        .generate_selected_layer(plains_id)
        .unwrap();
    app.editor.terrain_selected_generation_layer = Some(plains_id);
    app.editor.activate_sculpt_tool(SculptTool::Raise);
    app.editor.camera.target = glam::Vec3::new(12.5, 10.0, 12.5);
    app.editor.camera.distance = 4.0;
    app.update_hover();
    assert!(
        app.editor.hovered_terrain_hit.is_some(),
        "generated Plains must have a visible cursor target before heightmap import"
    );
    let req = TerrainGenerationRequest {
        position: [40, 0, 0],
        size: [24, 40, 24],
    };
    app.world
        .import_heightmap_layer_with_water_level(
            TerrainResult {
                request: req,
                generator: TerrainGeneratorType::Programmatic,
                generator_implementation_version: 1,
                data: TerrainData::Heightfield(HeightfieldData {
                    origin: req.position,
                    size: req.size.map(|v| v as u32),
                    heights: vec![22.0; 24 * 24],
                }),
            },
            0.0,
        )
        .unwrap();
    app.editor.terrain_selected_generation_layer = Some(plains_id);
    app.update_hover();
    assert!(
        app.editor.hovered_terrain_hit.is_some(),
        "generated Plains cursor must survive a second independent import"
    );
    app.editor.camera.target = glam::Vec3::new(52.5, 22.0, 12.5);
    app.update_hover();
    assert!(
        app.editor.hovered_terrain_hit.is_some(),
        "imported heightmap still has a visible cursor target"
    );
}
