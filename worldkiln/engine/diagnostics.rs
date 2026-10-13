//! Release-visible subsystem diagnostics. This queue belongs to the engine
//! thread, like the renderer/audio systems; it is never authored or saved data.
use std::cell::RefCell;
use std::collections::VecDeque;

const MAX_EVENTS: usize = 256;
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Default)]
struct Diagnostics {
    pending: VecDeque<String>,
    recent: VecDeque<String>,
}

thread_local! {
    static DIAGNOSTICS: RefCell<Diagnostics> = RefCell::default();
}

pub fn report(severity: &str, category: &str, message: String) {
    DIAGNOSTICS.with(|queue| {
        let mut queue = queue.borrow_mut();
        // Renderer failures can recur every frame. Suppress identical events
        // across drains until Clear or eviction from the bounded recent set.
        let mut message = message.replace('\\', "/");
        truncate_tail(&mut message, 8192);
        let key = format!("[{severity}] [{category}] {message}");
        if queue.recent.iter().any(|recent| recent == &key) {
            return;
        }
        if queue.recent.len() == MAX_EVENTS {
            queue.recent.pop_front();
        }
        queue.recent.push_back(key.clone());
        if queue.pending.len() == MAX_EVENTS {
            queue.pending.pop_front();
        }
        queue.pending.push_back(format!(
            "[{}] {key}\n",
            chrono::Local::now().format("%H:%M:%S")
        ));
    });
}

pub fn drain() -> Vec<String> {
    DIAGNOSTICS.with(|queue| queue.borrow_mut().pending.drain(..).collect())
}

pub fn clear() {
    DIAGNOSTICS.with(|queue| *queue.borrow_mut() = Diagnostics::default());
}

fn truncate_tail(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(" [truncated]");
    }
}

pub fn retain_output(text: &mut String) {
    if text.len() <= MAX_OUTPUT_BYTES {
        return;
    }
    let mut start = text.len() - MAX_OUTPUT_BYTES;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    // Prefer whole lines, unless a single oversized message fills the buffer.
    if let Some(newline) = text[start..].find('\n') {
        if start + newline + 1 < text.len() {
            start += newline + 1;
        }
    }
    text.drain(..start);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "editor")]
    #[test]
    fn repeated_script_callback_errors_are_suppressed_until_clear() {
        let mut editor = crate::editor::Editor::new();
        for seconds in 0..60 {
            editor.append_output(&format!(
                "[12:00:{seconds:02}] [ERROR] [Script] scripts/bad.aeo:2:5 | update: bad callback\n"
            ));
        }
        assert_eq!(editor.terminal_output.matches("bad callback").count(), 1);
        editor.clear_output();
        editor.append_output(
            "[12:02:00] [ERROR] [Script] scripts/bad.aeo:2:5 | update: bad callback\n",
        );
        assert_eq!(editor.terminal_output.matches("bad callback").count(), 1);
    }

    #[cfg(feature = "editor")]
    #[test]
    fn subsystem_errors_open_output_and_render_at_supported_viewport_sizes() {
        for (width, height) in [
            (1280.0, 720.0),
            (1600.0, 900.0),
            (1920.0, 1080.0),
            (800.0, 600.0),
        ] {
            clear();
            let mut editor = crate::editor::Editor::new();
            editor.mode = crate::engine::EditorMode::Play;
            editor.script_editor.show_output = false;
            report(
                "ERROR",
                "Audio",
                "wo04-visible-error: check Audio Path".into(),
            );
            editor.collect_diagnostics();
            assert!(editor.script_editor.show_output);
            let renderer = crate::renderer::Renderer::new_for_tests(width, height);
            let mut world = crate::world::World::new();
            let ctx = egui::Context::default();
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    editor.show_ui(ctx, &mut world, &None, &renderer);
                },
            );
            for label in ["wo04-visible-error", "Clear"] {
                assert!(output.shapes.iter().any(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text.contains(label)
                        && text.visual_bounding_rect().intersects(screen)
                        && text.visual_bounding_rect().intersects(clipped.clip_rect))
                }), "missing visible {label} at {width}x{height}");
            }
            editor.mode = crate::engine::EditorMode::Editor;
            assert!(editor.terminal_output.contains("wo04-visible-error"));
        }
    }

    #[test]
    fn diagnostics_are_bounded_and_suppress_repeated_frame_failures() {
        clear();
        for _ in 0..100 {
            report("ERROR", "Asset", "missing .assets\\texture.png".into());
        }
        let messages = drain();
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("[ERROR] [Asset] missing .assets/texture.png"));
        report("ERROR", "Asset", "missing .assets\\texture.png".into());
        assert!(drain().is_empty());
        for i in 0..1000 {
            report("WARNING", "Renderer", format!("bad texture {i}"));
        }
        assert_eq!(drain().len(), MAX_EVENTS);
        clear();
        report("ERROR", "Asset", "missing .assets\\texture.png".into());
        assert_eq!(drain().len(), 1);
        let mut text = "é\n".repeat(MAX_OUTPUT_BYTES);
        retain_output(&mut text);
        assert!(text.len() <= MAX_OUTPUT_BYTES);
        assert!(text.ends_with("é\n"));
    }
}
