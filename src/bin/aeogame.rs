// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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

use winit::event::{DeviceEvent, Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{CursorGrabMode, Fullscreen, WindowAttributes};

use Worldkiln::game::GameRuntime;

use Worldkiln::game::input::GameInputEvent;
use Worldkiln::game::platform::GameLifecycleEvent;
use Worldkiln::game::platform::desktop::translate_window_event;
fn game_data_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    game_data_path_for_executable(&exe)
}

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

    let profile = game_data
        .as_ref()
        .and_then(|directory| {
            let bytes = std::fs::read(directory.join(".worldkiln-export-profile.json")).ok()?;
            serde_json::from_slice::<Worldkiln::engine::game_package::GameExportProfile>(&bytes)
                .ok()
        })
        .filter(|profile| profile.target_platform == "linux");
    let event_loop = EventLoop::new().expect("Failed to create event loop");

    let game_title = profile
        .as_ref()
        .map(|p| p.display_title.as_str())
        .unwrap_or("Worldkiln Game");
    let requested_mode = profile
        .as_ref()
        .map(|p| p.window_mode.as_str())
        .unwrap_or("borderless_fullscreen");
    let requested_resolution = profile
        .as_ref()
        .and_then(|p| p.resolution)
        .unwrap_or([1280, 720]);
    let window_attributes = WindowAttributes::default()
        .with_title(game_title)
        .with_inner_size(winit::dpi::PhysicalSize::new(
            requested_resolution[0],
            requested_resolution[1],
        ))
        .with_fullscreen(if requested_mode == "windowed" {
            None
        } else {
            Some(Fullscreen::Borderless(None))
        });

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
                        game_runtime.on_lifecycle(GameLifecycleEvent::Shutdown);
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
                    if let Some(input) = translate_window_event(
                        &event,
                        egui_state.egui_ctx().wants_keyboard_input(),
                        egui_state.egui_ctx().wants_pointer_input(),
                    ) {
                        game_runtime.on_input(input);
                    }
                }
            }

            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                game_runtime.on_input(GameInputEvent::PointerMotion {
                    dx: delta.0,
                    dy: delta.1,
                });
            }

            Event::AboutToWait => {
                let now = std::time::Instant::now();
                let dt = now.duration_since(last_frame_time).as_secs_f32();
                last_frame_time = now;

                game_runtime.update(dt);

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

                game_runtime.render();

                let input = egui_state.take_egui_input(&window);

                egui_state.egui_ctx().begin_pass(input);

                game_runtime.render_runtime_ui(egui_state.egui_ctx());

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
