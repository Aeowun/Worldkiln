// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::Editor;
use crate::editor::editor::{
    feature_nav::{EditorSurface, Feature},
    shell::Workspace,
};
use crate::world::World;
use std::path::{Path, PathBuf};

fn frame(
    ctx: &egui::Context,
    editor: &mut Editor,
    world: &mut World,
    root: &Path,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.render_project_settings_content(ui, world, &Some(root.to_path_buf()));
            });
        },
    )
}

fn click(ctx: &egui::Context, editor: &mut Editor, world: &mut World, root: &Path, label: &str) {
    frame(ctx, editor, world, root, vec![]);
    let output = frame(ctx, editor, world, root, vec![]);
    let pos = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing button {label}"));
    for pressed in [true, false] {
        frame(
            ctx,
            editor,
            world,
            root,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

fn root() -> PathBuf {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("wk_asset003_pointer_{tick}"));
    std::fs::create_dir_all(&root).unwrap();
    root
}




