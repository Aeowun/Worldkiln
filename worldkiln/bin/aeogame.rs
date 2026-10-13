#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use glutin::config::ConfigTemplateBuilder;
use glutin::context::ContextAttributesBuilder;
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;

use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use winit::event::{DeviceEvent, Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{CursorGrabMode, Fullscreen, WindowAttributes};

use Worldkiln::game::GameRuntime;

fn game_data_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    game_data_path_for_executable(&exe)
}

// Packaged runtimes resolve data beside their executable without installer/profile decisions.
fn game_data_path_for_executable(exe: &std::path::Path) -> Option<PathBuf> {
    let exe_dir = exe.parent()?;
    let game_data = exe_dir.join("GameData");

    if game_data.is_dir() {
        Some(game_data)
    } else {
        None
    }
}

#[cfg(test)]
mod startup_tests {
    use super::*;

    #[test]
    fn packaged_runtime_resolves_gamedata_without_install_state() {
        let root =
            std::env::temp_dir().join(format!("Worldkiln_RuntimeRole_{}", std::process::id()));
        let data = root.join("GameData");
        std::fs::create_dir_all(&data).unwrap();
        // Runtime identity survives the build's user-facing executable rename.
        assert_eq!(
            game_data_path_for_executable(&root.join("Hearthvale.exe")),
            Some(data.clone())
        );
        std::fs::remove_dir_all(&data).unwrap();
        assert_eq!(
            game_data_path_for_executable(&root.join("Hearthvale.exe")),
            None
        );
        std::fs::remove_dir(root).unwrap();
    }
}

fn main() {
    let game_data = game_data_path();

    if game_data.is_some() {
        let exe_dir = std::env::current_exe()
            .expect("Failed to determine executable path")
            .parent()
            .expect("Executable has no parent directory")
            .to_path_buf();

        std::env::set_current_dir(&exe_dir).expect("Failed to set packaged game working directory");
    }

    let event_loop = EventLoop::new().expect("Failed to create event loop");

    let window_attributes = WindowAttributes::default()
        .with_title("Worldkiln Game")
        .with_fullscreen(Some(Fullscreen::Borderless(None)));

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));

    let (window, gl_config) = display_builder
        .build(&event_loop, ConfigTemplateBuilder::new(), |configs| {
            configs
                .max_by_key(|config| config.num_samples())
                .expect("No OpenGL configurations available")
        })
        .expect("Failed to create OpenGL display");

    let window = window.expect("Failed to create window");

    let raw_window_handle = window
        .window_handle()
        .expect("Failed to get window handle")
        .as_raw();

    let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));

    let not_current_context = unsafe {
        gl_config
            .display()
            .create_context(&gl_config, &context_attributes)
            .expect("Failed to create OpenGL context")
    };

    let size = window.inner_size();

    let width = NonZeroU32::new(size.width.max(1)).unwrap();
    let height = NonZeroU32::new(size.height.max(1)).unwrap();

    let surface_attributes =
        SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window_handle, width, height);

    let surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &surface_attributes)
            .expect("Failed to create window surface")
    };

    let context = not_current_context
        .make_current(&surface)
        .expect("Failed to make OpenGL context current");

    gl::load_with(|symbol| {
        let symbol = CString::new(symbol).unwrap();
        gl_config.display().get_proc_address(&symbol).cast()
    });

    let mut game_runtime = GameRuntime::new(size.width as f32, size.height as f32);

    if let Some(data_path) = game_data {
        game_runtime.load_project(data_path);
    }

    let splash_started = Instant::now();

    let glow_context = unsafe {
        Arc::new(glow::Context::from_loader_function(|symbol| {
            let symbol = CString::new(symbol).unwrap();
            gl_config.display().get_proc_address(&symbol).cast()
        }))
    };

    let mut egui_glow = egui_glow::Painter::new(glow_context.clone(), "", None, false)
        .expect("failed to create egui painter");

    let mut egui_state = egui_winit::State::new(
        egui::Context::default(),
        egui::viewport::ViewportId::ROOT,
        &window,
        Some(window.scale_factor() as f32),
        None,
        None,
    );

    let mut last_applied_mouse: Option<(bool, bool)> = None;

    let mut last_frame_time = std::time::Instant::now();

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent { event, .. } => {
                let response = egui_state.on_window_event(&window, &event);

                match &event {
                    WindowEvent::CloseRequested => {
                        game_runtime.stop_scripting();
                        elwt.exit();
                    }

                    WindowEvent::Resized(size) => {
                        let width = NonZeroU32::new(size.width.max(1)).unwrap();
                        let height = NonZeroU32::new(size.height.max(1)).unwrap();
                        surface.resize(&context, width, height);
                    }

                    _ => {}
                }

                if !response.consumed {
                    game_runtime.on_window_event(&event, egui_state.egui_ctx(), &window);
                }
            }

            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                game_runtime.on_mouse_motion(delta.0, delta.1);
            }

            Event::AboutToWait => {
                let now = std::time::Instant::now();
                let dt = now.duration_since(last_frame_time).as_secs_f32();
                last_frame_time = now;

                let splash_active = splash_started.elapsed().as_secs_f32() < 7.0;

                if !splash_active {
                    game_runtime.update(dt.min(0.1));
                }

                let current_mouse = (
                    game_runtime.mouse.cursor_visible,
                    game_runtime.mouse.screen_locked,
                );

                if last_applied_mouse != Some(current_mouse) {
                    let grab_mode = if game_runtime.mouse.screen_locked {
                        CursorGrabMode::Locked
                    } else {
                        CursorGrabMode::None
                    };

                    if let Err(_err) = window.set_cursor_grab(grab_mode) {
                        if game_runtime.mouse.screen_locked {
                            let _ = window.set_cursor_grab(CursorGrabMode::Confined);
                        }
                    }

                    window.set_cursor_visible(game_runtime.mouse.cursor_visible);

                    last_applied_mouse = Some(current_mouse);
                }

                if !splash_active {
                    game_runtime.render();
                }

                let input = egui_state.take_egui_input(&window);

                egui_state.egui_ctx().begin_pass(input);

                if splash_active {
                    draw_worldkiln_splash(egui_state.egui_ctx(), splash_started);
                } else {
                    game_runtime.render_runtime_ui(egui_state.egui_ctx());
                }

                let full_output = egui_state.egui_ctx().end_pass();
                egui_state.handle_platform_output(&window, full_output.platform_output);

                let paint_jobs = egui_state
                    .egui_ctx()
                    .tessellate(full_output.shapes, full_output.pixels_per_point);

                unsafe {
                    for (id, image_delta) in &full_output.textures_delta.set {
                        egui_glow.set_texture(*id, image_delta);
                    }

                    let size = window.inner_size();

                    egui_glow.paint_primitives(
                        [size.width, size.height],
                        full_output.pixels_per_point,
                        &paint_jobs,
                    );

                    for id in &full_output.textures_delta.free {
                        egui_glow.free_texture(*id);
                    }
                }

                surface
                    .swap_buffers(&context)
                    .expect("Failed to swap buffers");

                window.request_redraw();
            }

            _ => {}
        })
        .expect("Event loop failed");
}
fn draw_worldkiln_splash(ctx: &egui::Context, started: Instant) {
    ctx.request_repaint();

    let elapsed = started.elapsed().as_secs_f32();
    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    let void = egui::Color32::from_rgb(1, 1, 2);
    let text_dim = egui::Color32::from_rgb(92, 98, 108);
    let text_bright = egui::Color32::from_rgb(255, 255, 255);

    painter.rect_filled(rect, 0.0, void);

    let presents_start = 0.25;
    let presents_fade_in = 0.35;
    let presents_fade_out_start = 1.15;
    let presents_fade_out = 0.45;

    let mut presents_alpha = 0.0;

    if elapsed >= presents_start {
        presents_alpha = ((elapsed - presents_start) / presents_fade_in).clamp(0.0, 1.0);
    }

    if elapsed >= presents_fade_out_start {
        presents_alpha *=
            (1.0 - (elapsed - presents_fade_out_start) / presents_fade_out).clamp(0.0, 1.0);
    }

    if presents_alpha > 0.0 {
        let color = egui::Color32::from_rgba_unmultiplied(
            text_dim.r(),
            text_dim.g(),
            text_dim.b(),
            (presents_alpha * 255.0) as u8,
        );

        painter.text(
            rect.center() + egui::vec2(0.0, -18.0),
            egui::Align2::CENTER_CENTER,
            "AEOWUN PRESENTS...",
            egui::FontId::monospace(60.0),
            color,
        );
    }

    let title_start = 1.45;
    let stagger = 0.12;
    let reveal_duration = 0.38;

    let title = "WORLDKILN";
    let font = egui::FontId::monospace(52.0);

    let char_width = painter
        .layout_no_wrap("M".to_string(), font.clone(), text_bright)
        .size()
        .x;

    let total_width = char_width * title.chars().count() as f32;

    let start_x = rect.center().x - total_width * 0.5 + char_width * 0.5;

    let base_y = rect.center().y + 20.0;

    for (index, character) in title.chars().enumerate() {
        let char_start = title_start + index as f32 * stagger;

        let progress = ((elapsed - char_start) / reveal_duration).clamp(0.0, 1.0);

        if progress <= 0.0 {
            continue;
        }

        let eased = 1.0 - (1.0 - progress).powi(3);

        let x = start_x + index as f32 * char_width - (1.0 - eased) * 28.0;

        let y = base_y;

        let alpha = (eased * 255.0) as u8;

        let color = egui::Color32::from_rgba_unmultiplied(
            text_bright.r(),
            text_bright.g(),
            text_bright.b(),
            alpha,
        );

        painter.text(
            egui::pos2(x, y),
            egui::Align2::CENTER_CENTER,
            character.to_string(),
            font.clone(),
            color,
        );

        if progress < 1.0 {
            let reaction = eased.powf(0.75);

            for dust_index in 0..20 {
                let seed = index as f32 * 31.7 + dust_index as f32 * 17.3;

                let spread_x = seed.sin() * 7.0 + (dust_index as f32 % 5.0 - 2.0) * 1.8;

                let spread_y = seed.cos() * 5.0 + (dust_index as f32 % 4.0 - 1.5) * 2.0;

                let backward_distance = reaction * (10.0 + dust_index as f32 * 3.4);

                let upward_distance = reaction * reaction * (8.0 + dust_index as f32 * 2.1);

                let dust_x = x - backward_distance + spread_x;

                let dust_y = y - upward_distance + spread_y;

                let dust_alpha = ((1.0 - progress) * 80.0).clamp(0.0, 255.0) as u8;

                let dust_size = 0.35 + (dust_index % 3) as f32 * 0.15;

                painter.circle_filled(
                    egui::pos2(dust_x, dust_y),
                    dust_size,
                    egui::Color32::from_rgba_unmultiplied(210, 210, 210, dust_alpha),
                );
            }
        }
    }
}
