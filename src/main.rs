// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actor;
mod animation;
mod character;
mod editor;
mod engine;
mod game;
#[cfg(feature = "kilngraph")]
mod kilngraph;
mod landscape;
mod navigation;
mod project;
mod renderer;
mod scripting;
mod window_shell;
mod world;

use glutin::config::ConfigTemplateBuilder;
use glutin::context::ContextAttributesBuilder;
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use glutin_winit::DisplayBuilder;

use std::ffi::CString;
use std::num::NonZeroU32;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;

use winit::event::{DeviceEvent, Event, WindowEvent};

use winit::event_loop::EventLoop;
use winit::raw_window_handle::HasWindowHandle;

use winit::window::WindowAttributes;
use engine::mouse::WindowMouseCapture;

use engine::app::App;
use engine::install::{
    CANONICAL_EXECUTABLE_NAME, LEGACY_EXECUTABLE_NAME, aeowun_install_dir, aeowun_install_dir_from,
    cleanup_legacy_executable, ensure_license_installed, executable_role, install_assets_from_zip,
    install_engine_executable, install_package_from_zip, install_userdata_from_zip,
    installer_bootstrap_required, installer_required, is_development_build_arg,
    is_install_check_complete_arg, missing_install_items,
};

fn show_game_bootstrap_error(message: &str) {
    let _ = rfd::MessageDialog::new()
        .set_title("Worldkiln Game")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

fn main() {
    let current_exe =
        std::env::current_exe().expect("Failed to determine current Worldkiln executable");

    match engine::game_package::probe_embedded_game(&current_exe) {
        Ok(Some(package)) => {
            let title = package
                .manifest
                .export_profile
                .as_ref()
                .map(|p| p.display_title.clone())
                .unwrap_or_else(|| package.manifest.game_name.clone());
            let profile = package.manifest.export_profile.clone();

            match engine::game_package::prepare_embedded_game(&current_exe, &package) {
                Ok(prepared) => {
                    if let Err(error) = std::env::set_current_dir(&prepared.runtime_root) {
                        show_game_bootstrap_error(&format!(
                            "The game runtime cache could not be activated.\n\n{}",
                            error
                        ));
                        return;
                    }

                    game::runner::run_game(prepared.game_data, title, profile);
                }
                Err(error) => {
                    show_game_bootstrap_error(&format!(
                        "This Worldkiln game could not be prepared.\n\n{}\n\nIf this is the first launch, check your internet connection and retry.",
                        error
                    ));
                }
            }
            return;
        }
        Ok(None) => {}
        Err(error) => {
            show_game_bootstrap_error(&format!(
                "This executable contains a damaged Worldkiln game package.\n\n{}",
                error
            ));
            return;
        }
    }
    let development_build =
        cfg!(debug_assertions) || std::env::args_os().any(|arg| is_development_build_arg(&arg));

    let editor_dev_preview = cfg!(debug_assertions)
        && std::env::var("WORLDKILN_EDITOR_DEV_PREVIEW").as_deref() == Ok("1");

    #[cfg(target_os = "windows")]
    {
        let install_dir =
            aeowun_install_dir().expect("Failed to determine Aeowun installation directory");

        let role = executable_role(&current_exe, &install_dir);
        let install_complete_requested =
            std::env::args_os().any(|arg| is_install_check_complete_arg(&arg));
        if !editor_dev_preview
            && installer_bootstrap_required(
                role,
                installer_required(&current_exe, &install_dir),
                install_complete_requested,
            )
        {
            engine::install_window::run_installer_window(current_exe, install_dir)
                .expect("Worldkiln installation failed");

            return;
        }
    }

    let exe_dir = current_exe
        .parent()
        .expect("Executable has no parent directory");
    let working_dir = if editor_dev_preview {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
    } else {
        exe_dir
    };
    std::env::set_current_dir(working_dir).expect("Failed to set working directory for Worldkiln");

    let event_loop = EventLoop::new().expect("Failed to create event loop");

    let window_attributes = if editor_dev_preview {
        WindowAttributes::default()
            .with_title(concat!(
                "Worldkiln V",
                env!("CARGO_PKG_VERSION"),
                " — Editor Preview"
            ))
            .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 900.0))
            .with_fullscreen(None)
            .with_decorations(false)
    } else {
        WindowAttributes::default()
            .with_title(concat!("Worldkiln V", env!("CARGO_PKG_VERSION")))
            .with_decorations(false)
            .with_maximized(true)
    };

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));

    let (window, gl_config) = display_builder
        .build(&event_loop, ConfigTemplateBuilder::new(), |configs| {
            configs
                .max_by_key(|config| config.num_samples())
                .expect("No OpenGL configurations available")
        })
        .expect("Failed to create OpenGL display");

    let window = window.expect("Failed to create window");
    let mut window_shell = window_shell::WindowShell::default();
    window_shell.update(&window);
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

    if std::env::var_os("WORLDKILN_09_UNCAPPED_PRESENT_TEST").is_some() {
        if let Err(error) = surface.set_swap_interval(&context, SwapInterval::DontWait) {
            eprintln!("[Worldkiln perf] Uncapped presentation unavailable: {error}");
        } else {
            eprintln!("[Worldkiln perf] Uncapped presentation mode active");
        }
    }
    gl::load_with(|symbol| {
        let symbol = CString::new(symbol).unwrap();

        gl_config.display().get_proc_address(&symbol).cast()
    });

    let mut app = App::new(size.width as f32, size.height as f32);
    app.development_build = development_build;
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

    let mut window_mouse = WindowMouseCapture::default();

    let perf_trace =
        editor_dev_preview && std::env::var("WORLDKILN_PERF_TRACE").as_deref() == Ok("1");
    let mut perf_window = std::time::Instant::now();
    let mut perf_frames = 0u64;
    let mut perf_stages = [0.0_f64; 5];
    let mut perf_frame_ms = Vec::<f64>::new();

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent { window_id, event } => {
                if window_id != window.id() {
                    return;
                }
                window_shell.update(&window);
                if let WindowEvent::CursorMoved { position, .. } = &event {
                    window_shell.cursor_moved(&window, *position);
                }
                if matches!(
                    &event,
                    WindowEvent::MouseInput {
                        state: winit::event::ElementState::Pressed,
                        button: winit::event::MouseButton::Left,
                        ..
                    }
                ) && window_shell.begin_resize(&window)
                {
                    return;
                }
                if app.handle_play_shortcut(&event) { return; }
                app.editor.ai_chat.capture_native_paste(&event,egui_state.egui_input());
                let response = egui_state.on_window_event(&window, &event);

                match &event {
                    WindowEvent::CloseRequested => {
                        if app.can_close_editor() {
                            window_mouse.release(&window);
                            elwt.exit();
                        }
                    }

                    WindowEvent::Resized(size) => {
                        let width = NonZeroU32::new(size.width.max(1)).unwrap();

                        let height = NonZeroU32::new(size.height.max(1)).unwrap();

                        surface.resize(&context, width, height);
                    }

                    _ => {}
                }

                if let WindowEvent::Focused(focused) = &event {
                    window_mouse.focus_changed(*focused);
                    if !focused {
                        window_mouse.release(&window);
                    }
                }
                if !response.consumed || matches!(&event, WindowEvent::Focused(false)) {
                    app.on_window_event(&event, egui_state.egui_ctx(), &window);
                }
            }

            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if window_mouse.focused() {
                    app.on_mouse_motion(delta.0, delta.1);
                }
            }

            Event::AboutToWait => {
                #[cfg(feature = "kilngraph")]
                crate::kilngraph::begin_frame(
                    if app.view != engine::View::Editor { 1 } else if app.editor.mode == engine::EditorMode::Play { 3 } else { 2 },
                    app.project_manager.current_project.as_deref(),
                );
                #[cfg(feature = "kilngraph")]
                let kg_root_source = line!() + 1;
                let profile_begin = std::time::Instant::now();
                #[cfg(feature = "kilngraph")]
                let kg_update_source = line!() + 1;
                app.update(egui_state.egui_ctx());
                let profile_update = std::time::Instant::now();

                if let Err(error) = window_mouse.apply(&window, app.mouse.mode()) {
                    eprintln!("[Mouse] {error}");
                }

                #[cfg(feature = "kilngraph")]
                let kg_render_source = line!() + 1;
                app.render();
                let profile_render = std::time::Instant::now();

                #[cfg(feature = "kilngraph")]
                let kg_ui_source = line!() + 1;
                let input = egui_state.take_egui_input(&window);

                egui_state.egui_ctx().begin_pass(input);

                app.update_ui(egui_state.egui_ctx());
                app.editor.ai_chat.clear_native_paste();
                app.draw_application_border(egui_state.egui_ctx(), window.is_maximized());
                window_shell.update(&window);
                if let Some(action) = app.window_action.take() {
                    match action {
                        engine::app::WindowAction::Drag => {
                            let _ = window.drag_window();
                        }
                        engine::app::WindowAction::MaximizeRestore => {
                            window.set_maximized(!window.is_maximized());
                        }
                        engine::app::WindowAction::Minimize => {
                            window.set_minimized(true);
                        }
                        engine::app::WindowAction::Close => {
                            if app.can_close_editor() {
                                elwt.exit();
                            }
                            return;
                        }
                    }
                }
                if app.exit_requested {
                    app.exit_requested = false;
                    if app.can_close_editor() {
                        elwt.exit();
                        return;
                    }
                }
                let mut full_output = egui_state.egui_ctx().end_pass();
                if let Some(cursor) = window_shell.resize_cursor() {
                    full_output.platform_output.cursor_icon = cursor;
                }
                egui_state.handle_platform_output(&window, full_output.platform_output);

                let profile_ui = std::time::Instant::now();
                #[cfg(feature = "kilngraph")]
                let kg_paint_source = line!() + 1;
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

                let profile_paint = std::time::Instant::now();
                #[cfg(feature = "kilngraph")]
                let kg_swap_source = line!() + 1;
                surface
                    .swap_buffers(&context)
                    .expect("Failed to swap buffers");
                #[cfg(feature = "kilngraph")]
                kilngraph::record_frame(
                    [profile_begin, profile_update, profile_render, profile_ui, profile_paint, std::time::Instant::now()],
                    [kg_root_source, kg_update_source, kg_render_source, kg_ui_source, kg_paint_source, kg_swap_source],
                );
                if app.view == engine::View::Editor {
                    let profile_swap = std::time::Instant::now();
                    let ms = |later: std::time::Instant, earlier: std::time::Instant| {
                        (later - earlier).as_secs_f64() * 1000.0
                    };
                    app.renderer.record_editor_host_frame([
                        ms(profile_update, profile_begin),
                        ms(profile_render, profile_update),
                        ms(profile_ui, profile_render),
                        ms(profile_paint, profile_ui),
                        ms(profile_swap, profile_paint),
                    ]);
                }

                if perf_trace {
                    let done = std::time::Instant::now();
                    let points = [profile_begin, profile_update, profile_render, profile_ui, profile_paint, done];
                    for i in 0..5 {
                        perf_stages[i] += (points[i + 1] - points[i]).as_secs_f64() * 1000.0;
                    }
                    perf_frames += 1;
                    perf_frame_ms.push((done - profile_begin).as_secs_f64() * 1000.0);
                    if perf_window.elapsed() >= std::time::Duration::from_secs(3) {
                        perf_frame_ms.sort_by(f64::total_cmp);
                        let count = perf_frame_ms.len();
                        let p95 = perf_frame_ms[count.saturating_mul(95) / 100];
                        let phase = if app.view != engine::View::Editor { "Home/Loading" }
                            else if app.editor.mode == engine::EditorMode::Play { "Play" }
                            else { "Editor" };
                        eprintln!(
                            "[PERF TRACE] phase={phase} fps={:.1} mean_ms={:.2} p95_ms={:.2} update={:.2} render={:.2} ui={:.2} paint={:.2} swap={:.2} frames={perf_frames}",
                            perf_frames as f64 / perf_window.elapsed().as_secs_f64(),
                            perf_stages.iter().sum::<f64>() / perf_frames as f64,
                            p95,
                            perf_stages[0] / perf_frames as f64,
                            perf_stages[1] / perf_frames as f64,
                            perf_stages[2] / perf_frames as f64,
                            perf_stages[3] / perf_frames as f64,
                            perf_stages[4] / perf_frames as f64,
                        );
                        perf_window = done;
                        perf_frames = 0;
                        perf_stages = [0.0; 5];
                        perf_frame_ms.clear();
                    }
                }
                window.request_redraw();
            }

            _ => {}
        })
        .expect("Event loop failed");
}

#[cfg(test)]
mod install_bootstrap_tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_install_dir(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_nanos();

        std::env::temp_dir().join(format!(
            "Worldkiln_{test_name}_{}_{}",
            std::process::id(),
            nonce
        ))
    }

    #[test]
    fn aeowun_install_dir_uses_exact_directory_name() {
        let base = PathBuf::from("LocalAppData");
        let install_dir = aeowun_install_dir_from(&base);

        assert_eq!(install_dir, base.join("Aeowun"));
    }

    #[test]
    fn missing_install_items_reports_only_missing_entries() {
        let install_dir = temp_install_dir("missing_install_items");

        fs::create_dir_all(install_dir.join("UserData"))
            .expect("failed to create UserData test directory");
        fs::write(install_dir.join("LICENSE"), b"test license")
            .expect("failed to create LICENSE test file");

        let missing = missing_install_items(&install_dir);

        assert_eq!(missing, vec![".assets"]);

        fs::remove_dir_all(&install_dir).expect("failed to clean up install test directory");
    }

    #[test]
    fn install_engine_executable_overwrites_exe_without_touching_user_data() {
        let test_root = temp_install_dir("install_engine_executable");
        let source_dir = test_root.join("download");
        let install_dir = test_root.join("Aeowun");
        let source_exe = source_dir.join("AeoEngine.exe");
        let target_exe = install_dir.join("Worldkiln.exe");
        let user_data_file = install_dir.join("UserData").join("preset.txt");

        fs::create_dir_all(&source_dir).expect("failed to create source test directory");
        fs::create_dir_all(install_dir.join("UserData"))
            .expect("failed to create UserData test directory");

        fs::write(&source_exe, b"new engine").expect("failed to create source executable");
        fs::write(&target_exe, b"old engine").expect("failed to create existing executable");
        fs::write(&user_data_file, b"user preset").expect("failed to create UserData test file");

        let installed_exe = install_engine_executable(&source_exe, &install_dir)
            .expect("engine executable installation failed");

        assert_eq!(installed_exe, target_exe);
        assert_eq!(
            fs::read(&target_exe).expect("failed to read installed executable"),
            b"new engine"
        );
        assert_eq!(
            fs::read(&user_data_file).expect("failed to read UserData test file"),
            b"user preset"
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up install test directory");
    }

    #[test]
    fn install_userdata_from_zip_installs_userdata_directory() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("install_userdata_zip");
        let install_dir = test_root.join("Aeowun");
        let zip_path = test_root.join("UserData.zip");

        fs::create_dir_all(&install_dir).expect("failed to create install test directory");

        {
            let zip_file = fs::File::create(&zip_path).expect("failed to create UserData test ZIP");
            let mut zip_writer = zip::ZipWriter::new(zip_file);

            zip_writer
                .start_file("UserData/preset.txt", SimpleFileOptions::default())
                .expect("failed to create UserData ZIP entry");

            zip_writer
                .write_all(b"default preset")
                .expect("failed to write UserData ZIP entry");

            zip_writer
                .finish()
                .expect("failed to finish UserData test ZIP");
        }

        install_userdata_from_zip(&zip_path, &install_dir)
            .expect("failed to install UserData from ZIP");

        assert_eq!(
            fs::read(install_dir.join("UserData").join("preset.txt"))
                .expect("failed to read installed UserData"),
            b"default preset"
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up UserData install test");
    }

    #[test]
    fn install_userdata_from_zip_does_not_overwrite_existing_userdata() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("preserve_userdata");
        let install_dir = test_root.join("Aeowun");
        let userdata_dir = install_dir.join("UserData");
        let zip_path = test_root.join("UserData.zip");

        fs::create_dir_all(&userdata_dir).expect("failed to create existing UserData");

        fs::write(userdata_dir.join("preset.txt"), b"user preset")
            .expect("failed to create existing preset");

        {
            let zip_file = fs::File::create(&zip_path).expect("failed to create UserData test ZIP");
            let mut zip_writer = zip::ZipWriter::new(zip_file);

            zip_writer
                .start_file("UserData/preset.txt", SimpleFileOptions::default())
                .expect("failed to create UserData ZIP entry");

            zip_writer
                .write_all(b"default preset")
                .expect("failed to write UserData ZIP entry");

            zip_writer
                .finish()
                .expect("failed to finish UserData test ZIP");
        }

        install_userdata_from_zip(&zip_path, &install_dir)
            .expect("UserData preservation check failed");

        assert_eq!(
            fs::read(userdata_dir.join("preset.txt")).expect("failed to read preserved preset"),
            b"user preset"
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up UserData preservation test");
    }

    #[test]
    fn install_assets_from_zip_installs_assets_directory() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("install_assets_zip");
        let install_dir = test_root.join("Aeowun");
        let zip_path = test_root.join("Assets.zip");

        fs::create_dir_all(&install_dir).expect("failed to create install test directory");

        {
            let zip_file = fs::File::create(&zip_path).expect("failed to create assets test ZIP");
            let mut zip_writer = zip::ZipWriter::new(zip_file);

            zip_writer
                .start_file(".assets/version.txt", SimpleFileOptions::default())
                .expect("failed to create .assets version entry");

            zip_writer
                .write_all(env!("CARGO_PKG_VERSION").as_bytes())
                .expect("failed to write .assets version entry");

            zip_writer
                .start_file(".assets/textures/test.png", SimpleFileOptions::default())
                .expect("failed to create .assets ZIP entry");

            zip_writer
                .write_all(b"test texture")
                .expect("failed to write .assets ZIP entry");

            zip_writer
                .finish()
                .expect("failed to finish assets test ZIP");
        }

        install_assets_from_zip(&zip_path, &install_dir)
            .expect("failed to install .assets from ZIP");

        assert_eq!(
            fs::read(
                install_dir
                    .join(".assets")
                    .join("textures")
                    .join("test.png")
            )
            .expect("failed to read installed asset"),
            b"test texture"
        );

        assert_eq!(
            fs::read_to_string(install_dir.join(".assets").join("version.txt"))
                .expect("failed to read installed asset version")
                .trim(),
            env!("CARGO_PKG_VERSION")
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up assets install test");
    }

    #[test]
    fn install_assets_from_zip_adds_missing_assets_without_overwriting_existing_assets() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("smart_assets_update");
        let install_dir = test_root.join("Aeowun");
        let assets_dir = install_dir.join(".assets");
        let textures_dir = assets_dir.join("textures");
        let existing_asset = textures_dir.join("existing.png");
        let custom_asset = textures_dir.join("custom.png");
        let new_asset = textures_dir.join("new.png");
        let zip_path = test_root.join("Assets.zip");

        fs::create_dir_all(&textures_dir).expect("failed to create existing .assets directory");

        fs::write(&existing_asset, b"user version").expect("failed to create existing asset");

        fs::write(&custom_asset, b"user custom asset").expect("failed to create custom asset");

        fs::write(assets_dir.join("version.txt"), b"old-version")
            .expect("failed to create old asset version");

        {
            let zip_file = fs::File::create(&zip_path).expect("failed to create assets test ZIP");

            let mut zip_writer = zip::ZipWriter::new(zip_file);

            zip_writer
                .start_file(".assets/version.txt", SimpleFileOptions::default())
                .expect("failed to create version ZIP entry");

            zip_writer
                .write_all(env!("CARGO_PKG_VERSION").as_bytes())
                .expect("failed to write version ZIP entry");

            zip_writer
                .start_file(
                    ".assets/textures/existing.png",
                    SimpleFileOptions::default(),
                )
                .expect("failed to create existing asset ZIP entry");

            zip_writer
                .write_all(b"package version")
                .expect("failed to write existing asset ZIP entry");

            zip_writer
                .start_file(".assets/textures/new.png", SimpleFileOptions::default())
                .expect("failed to create new asset ZIP entry");

            zip_writer
                .write_all(b"new package asset")
                .expect("failed to write new asset ZIP entry");

            zip_writer
                .finish()
                .expect("failed to finish assets test ZIP");
        }

        install_assets_from_zip(&zip_path, &install_dir).expect("smart .assets update failed");

        assert_eq!(
            fs::read(&existing_asset).expect("failed to read preserved existing asset"),
            b"user version"
        );

        assert_eq!(
            fs::read(&custom_asset).expect("failed to read preserved custom asset"),
            b"user custom asset"
        );

        assert_eq!(
            fs::read(&new_asset).expect("failed to read newly installed asset"),
            b"new package asset"
        );

        assert_eq!(
            fs::read_to_string(assets_dir.join("version.txt"))
                .expect("failed to read updated asset version")
                .trim(),
            env!("CARGO_PKG_VERSION")
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up smart assets update test");
    }

    #[test]
    fn package_install_adds_missing_content_without_overwriting_existing_content() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("smart_package_install");
        let install_dir = test_root.join("Aeowun");
        let zip_path = test_root.join("Package.zip");

        let assets_dir = install_dir.join(".assets");
        let userdata_dir = install_dir.join("UserData");

        fs::create_dir_all(assets_dir.join("textures")).expect("failed to create installed assets");

        fs::create_dir_all(userdata_dir.join("MyGame"))
            .expect("failed to create installed UserData");

        fs::write(
            assets_dir.join("textures").join("existing.png"),
            b"user asset",
        )
        .expect("failed to create existing asset");

        fs::write(userdata_dir.join("MyGame").join("world.dat"), b"user world")
            .expect("failed to create user project");

        fs::write(assets_dir.join("version.txt"), b"old-version")
            .expect("failed to write old version");

        {
            let zip_file = fs::File::create(&zip_path).expect("failed to create test ZIP");

            let mut zip_writer = zip::ZipWriter::new(zip_file);

            zip_writer
                .start_file(".assets/version.txt", SimpleFileOptions::default())
                .expect("failed to create version entry");

            zip_writer
                .write_all(env!("CARGO_PKG_VERSION").as_bytes())
                .expect("failed to write version entry");

            zip_writer
                .start_file(
                    ".assets/textures/existing.png",
                    SimpleFileOptions::default(),
                )
                .expect("failed to create existing asset entry");

            zip_writer
                .write_all(b"package asset")
                .expect("failed to write package asset");

            zip_writer
                .start_file(".assets/textures/new.png", SimpleFileOptions::default())
                .expect("failed to create new asset entry");

            zip_writer
                .write_all(b"new asset")
                .expect("failed to write new asset");

            zip_writer
                .start_file("UserData/MyGame/world.dat", SimpleFileOptions::default())
                .expect("failed to create existing world entry");

            zip_writer
                .write_all(b"package world")
                .expect("failed to write package world");

            zip_writer
                .start_file(
                    "UserData/NewProject/project.json",
                    SimpleFileOptions::default(),
                )
                .expect("failed to create new project entry");

            zip_writer
                .write_all(b"new project")
                .expect("failed to write new project");

            zip_writer
                .start_file("LICENSE", SimpleFileOptions::default())
                .expect("failed to create LICENSE entry");

            zip_writer
                .write_all(b"license")
                .expect("failed to write LICENSE");

            zip_writer.finish().expect("failed to finish package ZIP");
        }

        install_package_from_zip(&zip_path, &install_dir)
            .expect("smart package installation failed");

        assert_eq!(
            fs::read(assets_dir.join("textures").join("existing.png"))
                .expect("failed to read existing asset"),
            b"user asset"
        );

        assert_eq!(
            fs::read(assets_dir.join("textures").join("new.png"))
                .expect("failed to read new asset"),
            b"new asset"
        );

        assert_eq!(
            fs::read(userdata_dir.join("MyGame").join("world.dat"))
                .expect("failed to read existing user world"),
            b"user world"
        );

        assert_eq!(
            fs::read(userdata_dir.join("NewProject").join("project.json"))
                .expect("failed to read new project"),
            b"new project"
        );

        assert_eq!(
            fs::read_to_string(assets_dir.join("version.txt"))
                .expect("failed to read version")
                .trim(),
            env!("CARGO_PKG_VERSION")
        );

        fs::remove_dir_all(&test_root).expect("failed to clean package installation test");
    }

    #[test]
    fn package_install_resolves_nested_wrapper_directory_package_root() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("nested_package_install");
        let install_dir = test_root.join("Aeowun");
        let zip_path = test_root.join("Package.zip");

        fs::create_dir_all(&test_root).expect("failed to create test root");

        {
            let zip_file = fs::File::create(&zip_path).expect("failed to create test ZIP");
            let mut zip_writer = zip::ZipWriter::new(zip_file);

            zip_writer
                .start_file("UserData/.assets/version.txt", SimpleFileOptions::default())
                .expect("failed to create version entry");

            zip_writer
                .write_all(env!("CARGO_PKG_VERSION").as_bytes())
                .expect("failed to write version entry");

            zip_writer
                .start_file(
                    "UserData/.assets/textures/test.png",
                    SimpleFileOptions::default(),
                )
                .expect("failed to create asset entry");

            zip_writer
                .write_all(b"test asset")
                .expect("failed to write asset");

            zip_writer.finish().expect("failed to finish package ZIP");
        }

        install_package_from_zip(&zip_path, &install_dir)
            .expect("nested package installation failed");

        assert_eq!(
            fs::read(
                install_dir
                    .join(".assets")
                    .join("textures")
                    .join("test.png")
            )
            .expect("failed to read installed asset"),
            b"test asset"
        );

        assert_eq!(
            fs::read_to_string(install_dir.join(".assets").join("version.txt"))
                .expect("failed to read version")
                .trim(),
            env!("CARGO_PKG_VERSION")
        );

        fs::remove_dir_all(&test_root).expect("failed to clean nested package installation test");
    }

    #[test]
    fn older_downloaded_package_installs_as_development_build() {
        use crate::engine::install::PackageInstallState;
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let test_root = temp_install_dir("development_package_install");
        let install_dir = test_root.join("Aeowun");
        let zip_path = test_root.join("Package.zip");

        fs::create_dir_all(&test_root).expect("failed to create test root");
        let zip_file = fs::File::create(&zip_path).expect("failed to create package ZIP");
        let mut zip_writer = zip::ZipWriter::new(zip_file);
        zip_writer
            .start_file(".assets/version.txt", SimpleFileOptions::default())
            .expect("failed to create package version entry");
        zip_writer
            .write_all(b"0.7.9")
            .expect("failed to write older package version");
        zip_writer
            .start_file(".assets/test.txt", SimpleFileOptions::default())
            .expect("failed to create package asset entry");
        zip_writer
            .write_all(b"development asset")
            .expect("failed to write package asset");
        zip_writer.finish().expect("failed to finish package ZIP");

        let state = install_package_from_zip(&zip_path, &install_dir)
            .expect("older package should be accepted for development");

        assert_eq!(state, PackageInstallState::DevelopmentBuild);
        assert_eq!(
            fs::read_to_string(install_dir.join(".assets").join("version.txt"))
                .expect("failed to read installed package version"),
            "0.7.9"
        );

        fs::remove_dir_all(&test_root).expect("failed to clean development package test");
    }

    #[test]
    fn installer_required_only_when_exe_or_required_packages_need_installation() {
        let test_root = temp_install_dir("installer_required");
        let install_dir = test_root.join("Aeowun");

        fs::create_dir_all(install_dir.join("UserData"))
            .expect("failed to create UserData test directory");

        fs::create_dir_all(install_dir.join(".assets"))
            .expect("failed to create .assets test directory");

        fs::write(
            install_dir.join(".assets").join("version.txt"),
            env!("CARGO_PKG_VERSION"),
        )
        .expect("failed to create current asset version");

        fs::write(install_dir.join("LICENSE"), b"license")
            .expect("failed to create LICENSE test file");

        let installed_exe = install_dir.join("Worldkiln.exe");

        fs::write(&installed_exe, b"engine").expect("failed to create installed executable");

        assert!(
            !installer_required(&installed_exe, &install_dir),
            "installed executable with required packages must skip installer"
        );

        let downloaded_exe = test_root.join("Installer.exe");

        fs::write(&downloaded_exe, b"engine").expect("failed to create downloaded executable");

        assert!(
            installer_required(&downloaded_exe, &install_dir),
            "external executable must enter installer"
        );

        fs::remove_dir_all(install_dir.join(".assets")).expect("failed to remove test assets");

        assert!(
            installer_required(&installed_exe, &install_dir),
            "missing package version must trigger a package download"
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up installer detection test");
    }

    #[test]
    fn stale_assets_version_requires_installer() {
        let test_root = temp_install_dir("stale_assets_version");
        let install_dir = test_root.join("Aeowun");

        fs::create_dir_all(install_dir.join("UserData"))
            .expect("failed to create UserData test directory");

        fs::create_dir_all(install_dir.join(".assets"))
            .expect("failed to create .assets test directory");

        fs::write(install_dir.join(".assets").join("version.txt"), b"0.8.4")
            .expect("failed to create stale asset version");

        fs::write(install_dir.join("LICENSE"), b"license").expect("failed to create LICENSE");

        let installed_exe = install_dir.join("Worldkiln.exe");

        fs::write(&installed_exe, b"engine").expect("failed to create installed executable");

        assert!(
            installer_required(&installed_exe, &install_dir),
            "stale .assets version must require installer"
        );

        fs::remove_dir_all(&test_root).expect("failed to clean up stale asset version test");
    }

    #[test]
    fn legacy_only_installation_migrates_to_canonical_and_cleans_up() {
        let test_root = temp_install_dir("legacy_only_migration");
        let install_dir = test_root.join("Aeowun");
        let downloaded_exe = test_root.join("AeoEngine.exe");
        let legacy_exe = install_dir.join("AeoEngine.exe");
        let canonical_exe = install_dir.join("Worldkiln.exe");
        let user_data = install_dir.join("UserData").join("project.dat");

        fs::create_dir_all(install_dir.join("UserData")).expect("failed to create UserData dir");
        fs::write(&legacy_exe, b"legacy engine").expect("failed to write legacy exe");
        fs::write(&downloaded_exe, b"new engine").expect("failed to write downloaded exe");
        fs::write(&user_data, b"user project data").expect("failed to write user data");

        assert!(
            installer_required(&downloaded_exe, &install_dir),
            "installer required when legacy exe present and canonical missing"
        );

        let installed = install_engine_executable(&downloaded_exe, &install_dir)
            .expect("installation of canonical executable failed");
        assert_eq!(installed, canonical_exe);
        assert!(canonical_exe.exists());

        cleanup_legacy_executable(&install_dir).expect("cleanup of legacy exe failed");
        assert!(!legacy_exe.exists());
        assert_eq!(
            fs::read(&user_data).expect("user data read failed"),
            b"user project data"
        );

        fs::remove_dir_all(&test_root).expect("clean test root failed");
    }

    #[test]
    fn both_executables_present_canonical_wins_and_cleans_legacy() {
        let test_root = temp_install_dir("both_executables_migration");
        let install_dir = test_root.join("Aeowun");
        let legacy_exe = install_dir.join("AeoEngine.exe");
        let canonical_exe = install_dir.join("Worldkiln.exe");

        fs::create_dir_all(install_dir.join("UserData")).unwrap();
        fs::create_dir_all(install_dir.join(".assets")).unwrap();
        fs::write(
            install_dir.join(".assets").join("version.txt"),
            env!("CARGO_PKG_VERSION"),
        )
        .unwrap();
        fs::write(install_dir.join("LICENSE"), b"license").unwrap();
        fs::write(&legacy_exe, b"old legacy").unwrap();
        fs::write(&canonical_exe, b"canonical engine").unwrap();

        assert!(
            installer_required(&canonical_exe, &install_dir),
            "installer required when legacy exe still present"
        );

        cleanup_legacy_executable(&install_dir).unwrap();
        assert!(!legacy_exe.exists());
        assert!(canonical_exe.exists());

        assert!(
            !installer_required(&canonical_exe, &install_dir),
            "installer not required after legacy exe cleaned up"
        );

        fs::remove_dir_all(&test_root).unwrap();
    }
}
