// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::world::terrain::{
    HeightfieldData, LayerBrushEdit, SculptTool, TerrainData, TerrainGenerationRequest,
    TerrainGeneratorType, TerrainResult, TerrainSculptRequest,
};

fn field(size: u32) -> TerrainResult {
    let request = TerrainGenerationRequest {
        position: [0, 0, 0],
        size: [size as i32, 48, size as i32],
    };
    TerrainResult {
        request,
        generator: TerrainGeneratorType::Programmatic,
        generator_implementation_version: 1,
        data: TerrainData::Heightfield(HeightfieldData {
            origin: request.position,
            size: request.size.map(|n| n as u32),
            heights: (0..size * size)
                .map(|i| 12.0 + ((i % size) % 7) as f32 * 0.25)
                .collect(),
        }),
    }
}

#[test]
fn terrain_edit_perf_imported_stroke_undo_redo_stays_local() {
    for tool in [
        SculptTool::Raise,
        SculptTool::Lower,
        SculptTool::Flatten,
        SculptTool::Smooth,
    ] {
        let mut world = crate::world::World::new();
        world.import_authored_heightfield(field(144)).unwrap();
        let id = world.terrain.config.generation_layers[0].id;
        let mut editor = Editor::new();
        editor.sculpt_tool = tool;
        let original = world.terrain.effective().unwrap().clone();
        let full = world.terrain.full_render_revision();
        let far = world.terrain.render_chunk_revision([2, 2]);
        editor.sculpt_stroke_snapshot = Some(editor.create_layer_brush_snapshot(&world, id));
        for center in [[65.0, 12.0, 65.0], [66.0, 12.0, 65.0]] {
            editor.capture_brush_columns(&world, center, 5.0);
            assert!(
                world
                    .terrain
                    .apply_generation_layer_brush(
                        id,
                        LayerBrushEdit::Sculpt(TerrainSculptRequest {
                            tool,
                            center,
                            radius: 5.0,
                            strength: 2.0,
                            flatten_target: Some(16.0)
                        })
                    )
                    .unwrap(),
                "{tool:?}"
            );
        }
        let edited = world.terrain.effective().unwrap().clone();
        let undo = editor.sculpt_stroke_snapshot.take().unwrap().6;
        let redo = undo.counterpart(&world.terrain);
        undo.apply(&mut world.terrain);
        assert_eq!(world.terrain.effective(), Some(&original), "undo {tool:?}");
        assert_eq!(
            world.terrain.full_render_revision(),
            full,
            "brush undo must not invalidate the entire landscape ({tool:?})"
        );
        assert_eq!(world.terrain.render_chunk_revision([2, 2]), far);
        assert!(
            world.terrain.config.generation_layers[0]
                .brush_edits
                .is_empty()
        );
        let undo = redo.counterpart(&world.terrain);
        redo.apply(&mut world.terrain);
        assert_eq!(world.terrain.effective(), Some(&edited), "redo {tool:?}");
        assert_eq!(world.terrain.full_render_revision(), full);
        assert_eq!(
            world.terrain.config.generation_layers[0].brush_edits.len(),
            2
        );
        world.terrain.rebuild_generation_layers().unwrap();
        assert_eq!(
            world.terrain.effective(),
            Some(&edited),
            "journal replay {tool:?}"
        );
        drop(undo);
    }
}

#[test]
fn terrain_edit_perf_standalone_heightfield_captures_only_brush_samples() {
    let mut world = crate::world::World::new();
    world.terrain.replace_authored(Some(field(513)));
    let mut editor = Editor::new();
    editor.sculpt_tool = SculptTool::Raise;
    let snapshot = editor.create_brush_snapshot(&world);
    assert!(
        !matches!(snapshot.6, TerrainHistorySnapshot::Full(_)),
        "starting a tiny heightfield brush must not copy the entire heightmap"
    );
    editor.sculpt_stroke_snapshot = Some(snapshot);
    editor.capture_brush_columns(&world, [64.5, 12.0, 64.5], 2.0);
    let undo = editor.sculpt_stroke_snapshot.take().unwrap().6;
    let original = world.terrain.effective().unwrap().clone();
    world.sculpt_authored_terrain(&TerrainSculptRequest {
        tool: SculptTool::Raise,
        center: [64.5, 12.0, 64.5],
        radius: 2.0,
        strength: 1.0,
        flatten_target: None,
    });
    let redo = undo.counterpart(&world.terrain);
    let edited = world.terrain.effective().unwrap().clone();
    undo.apply(&mut world.terrain);
    assert_eq!(world.terrain.effective(), Some(&original));
    redo.apply(&mut world.terrain);
    assert_eq!(world.terrain.effective(), Some(&edited));
}

#[test]
fn terrain_edit_perf_editor_history_keeps_unrelated_scene_objects() {
    let mut world = crate::world::World::new();
    world.terrain.replace_authored(Some(field(129)));
    let mut editor = Editor::new();
    editor.sculpt_tool = SculptTool::Raise;
    let untouched = crate::world::WorldCoord::new(200, 2, 200);
    world.set_cell(untouched, crate::world::CellType::Block);
    editor.sculpt_stroke_snapshot = Some(editor.create_brush_snapshot(&world));
    editor.capture_brush_columns(&world, [64.5, 12.0, 64.5], 3.0);
    assert!(world.sculpt_authored_terrain(&TerrainSculptRequest {
        tool: SculptTool::Raise,
        center: [64.5, 12.0, 64.5],
        radius: 3.0,
        strength: 2.0,
        flatten_target: None
    }));
    editor.sculpt_stroke_active = true;
    editor.sculpt_stroke_mutated = true;
    editor.finish_sculpt_stroke();
    assert!(editor.undo_heightfield_history(&mut world));
    assert!(world.get(untouched).is_some());
    assert!(editor.redo_heightfield_history(&mut world));
    assert!(world.get(untouched).is_some());
}
