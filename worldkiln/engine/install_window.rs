use std::collections::VecDeque;
use std::ffi::CString;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use glutin::config::ConfigTemplateBuilder;
use glutin::context::ContextAttributesBuilder;
use glutin::display::GetGlDisplay;
use glutin::prelude::*;
use glutin::surface::{SurfaceAttributesBuilder, WindowSurface};
use glutin_winit::DisplayBuilder;

use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::EventLoop;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowAttributes;

use super::install::{
    CANONICAL_EXECUTABLE_NAME, DEVELOPMENT_BUILD_ARG, INSTALL_CHECK_COMPLETE_ARG,
    PackageInstallState, create_desktop_shortcut, ensure_license_installed,
    ensure_package_installed, install_engine_executable, installer_launch_arguments,
    is_installer_self_cleanup_eligible, package_install_required, schedule_installer_self_cleanup,
};

use super::install_ui::{
    INSTALLER_SPLASH_COMPLETE_SECS, InstallerUserAction, draw_installer_screen,
    draw_installer_splash,
};

const STATUS_MIN_HOLD: Duration = Duration::from_millis(450);
const STATUS_FADE_OUT: Duration = Duration::from_millis(250);
const READY_HOLD: Duration = Duration::from_millis(800);
const CHECK_STEP_PAUSE: Duration = Duration::from_millis(350);

#[derive(Clone, Copy)]
struct StatusEntry {
    text: &'static str,
    active: bool,
}

enum InstallerMessage {
    Status(StatusEntry),
    Finished(Result<PackageInstallState, String>),
}

struct StatusStream {
    current: Option<StatusEntry>,
    pending: VecDeque<StatusEntry>,
    current_started: Instant,
    fade_out_started: Option<Instant>,
}

impl StatusStream {
    fn new() -> Self {
        Self {
            current: None,
            pending: VecDeque::new(),
            current_started: Instant::now(),
            fade_out_started: None,
        }
    }

    fn push(&mut self, entry: StatusEntry) {
        self.pending.push_back(entry);
    }

    fn update(&mut self, now: Instant) {
        if self.current.is_none() {
            if let Some(next) = self.pending.pop_front() {
                self.current = Some(next);
                self.current_started = now;
                self.fade_out_started = None;
            }

            return;
        }

        if let Some(fade_started) = self.fade_out_started {
            if now.duration_since(fade_started) >= STATUS_FADE_OUT {
                self.current = self.pending.pop_front();
                self.current_started = now;
                self.fade_out_started = None;
            }

            return;
        }

        if !self.pending.is_empty() && now.duration_since(self.current_started) >= STATUS_MIN_HOLD {
            self.fade_out_started = Some(now);
        }
    }

    fn current_view(&self, now: Instant) -> (&'static str, bool, f32, Option<f32>) {
        let Some(current) = self.current else {
            return ("", false, 0.0, None);
        };

        let age = now.duration_since(self.current_started).as_secs_f32();

        let fade_age = self
            .fade_out_started
            .map(|started| now.duration_since(started).as_secs_f32());

        (current.text, current.active, age, fade_age)
    }

    fn is_settled_on(&self, text: &str) -> bool {
        self.current
            .map(|entry| entry.text == text)
            .unwrap_or(false)
            && self.pending.is_empty()
            && self.fade_out_started.is_none()
    }
}

fn send_status(sender: &mpsc::Sender<InstallerMessage>, text: &'static str, active: bool) {
    let _ = sender.send(InstallerMessage::Status(StatusEntry { text, active }));
}

fn pause_between_checks() {
    std::thread::sleep(CHECK_STEP_PAUSE);
}

fn perform_installation(
    current_exe: PathBuf,
    install_dir: PathBuf,
    sender: mpsc::Sender<InstallerMessage>,
) -> Result<PackageInstallState, String> {
    send_status(&sender, "PREPARING INSTALL DIRECTORY", true);

    send_status(&sender, "COPYING WORLDKILN.EXE", true);

    install_engine_executable(&current_exe, &install_dir)?;
    ensure_license_installed(&install_dir)?;

    send_status(&sender, "WORLDKILN INSTALLED", false);

    pause_between_checks();

    send_status(&sender, "CHECKING PACKAGE", true);

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
    let install_state = if package_install_required(&install_dir) {
        send_status(&sender, "PACKAGE UPDATE REQUIRED", false);

        send_status(&sender, "INSTALLING PACKAGE", true);

        let state = ensure_package_installed(&install_dir)?;

        if state == PackageInstallState::DevelopmentBuild {
            send_status(&sender, "DEVELOPMENT BUILD", false);
        } else {
            send_status(&sender, "PACKAGE INSTALLED", false);
        }
        state
    } else {
        send_status(&sender, "PACKAGE UPDATE NOT REQUIRED", false);
        PackageInstallState::Current
    };

    pause_between_checks();

    let installed_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);

    let development_build =
        cfg!(debug_assertions) || install_state == PackageInstallState::DevelopmentBuild;

    #[cfg(target_os = "windows")]
    {
        send_status(&sender, "CREATING DESKTOP SHORTCUT", true);

        create_desktop_shortcut(&installed_exe, development_build)?;

        send_status(&sender, "SHORTCUT READY", false);

        pause_between_checks();
    }

    send_status(&sender, "FINALIZING", true);
    pause_between_checks();
    send_status(&sender, "READY", false);

    Ok(install_state)
}

pub(crate) fn run_installer_window(
    current_exe: PathBuf,
    install_dir: PathBuf,
) -> Result<(), String> {
    let installed_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);

    let event_loop =
        EventLoop::new().map_err(|err| format!("Failed to create installer event loop: {err}"))?;

    let window_attributes = WindowAttributes::default()
        .with_title(concat!(
            "Worldkiln V",
            env!("CARGO_PKG_VERSION"),
            " Installer"
        ))
        .with_inner_size(LogicalSize::new(720.0, 420.0))
        .with_resizable(false);

    let display_builder = DisplayBuilder::new().with_window_attributes(Some(window_attributes));

    let (window, gl_config) = display_builder
        .build(&event_loop, ConfigTemplateBuilder::new(), |configs| {
            configs
                .max_by_key(|config| config.num_samples())
                .expect("No OpenGL configurations available")
        })
        .map_err(|err| format!("Failed to create installer display: {err}"))?;

    let window = window.ok_or_else(|| "Failed to create installer window".to_string())?;

    let raw_window_handle = window
        .window_handle()
        .map_err(|err| format!("Failed to get installer window handle: {err}"))?
        .as_raw();

    let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));

    let not_current_context = unsafe {
        gl_config
            .display()
            .create_context(&gl_config, &context_attributes)
            .map_err(|err| format!("Failed to create installer OpenGL context: {err}"))?
    };

    let size = window.inner_size();

    let width = NonZeroU32::new(size.width.max(1))
        .ok_or_else(|| "Invalid installer window width".to_string())?;

    let height = NonZeroU32::new(size.height.max(1))
        .ok_or_else(|| "Invalid installer window height".to_string())?;

    let surface_attributes =
        SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window_handle, width, height);

    let surface = unsafe {
        gl_config
            .display()
            .create_window_surface(&gl_config, &surface_attributes)
            .map_err(|err| format!("Failed to create installer surface: {err}"))?
    };

    let context = not_current_context
        .make_current(&surface)
        .map_err(|err| format!("Failed to activate installer OpenGL context: {err}"))?;

    gl::load_with(|symbol| {
        let symbol = CString::new(symbol).unwrap();

        gl_config.display().get_proc_address(&symbol).cast()
    });

    let glow_context = unsafe {
        Arc::new(glow::Context::from_loader_function(|symbol| {
            let symbol = CString::new(symbol).unwrap();

            gl_config.display().get_proc_address(&symbol).cast()
        }))
    };

    let mut egui_glow = egui_glow::Painter::new(glow_context, "", None, false)
        .map_err(|err| format!("Failed to create installer painter: {err}"))?;

    let mut egui_state = egui_winit::State::new(
        egui::Context::default(),
        egui::viewport::ViewportId::ROOT,
        &window,
        Some(window.scale_factor() as f32),
        None,
        None,
    );

    let (sender, receiver) = mpsc::channel::<InstallerMessage>();

    let started = Instant::now();

    let mut worker_started = false;

    let mut statuses = StatusStream::new();

    let mut installation_succeeded = false;
    let mut installation_failed = false;
    let mut failure_detail: Option<String> = None;
    let mut ready_started: Option<Instant> = None;

    let launch_installed = Arc::new(AtomicBool::new(false));
    let launch_installed_in_loop = launch_installed.clone();
    let install_completed = Arc::new(AtomicBool::new(false));
    let install_completed_in_loop = install_completed.clone();
    let development_build = Arc::new(AtomicBool::new(false));
    let development_build_in_loop = development_build.clone();
    let current_exe_in_loop = current_exe.clone();
    let install_dir_in_loop = install_dir.clone();

    event_loop
        .run(move |event, elwt| match event {
            Event::WindowEvent { event, .. } => {
                let response = egui_state.on_window_event(&window, &event);

                match event {
                    WindowEvent::CloseRequested => {
                        elwt.exit();
                    }

                    WindowEvent::Resized(size) => {
                        let width = NonZeroU32::new(size.width.max(1)).unwrap();

                        let height = NonZeroU32::new(size.height.max(1)).unwrap();

                        surface.resize(&context, width, height);
                    }

                    _ => {
                        let _ = response;
                    }
                }
            }

            Event::AboutToWait => {
                let now = Instant::now();
                let elapsed = now.duration_since(started).as_secs_f32();

                if elapsed >= INSTALLER_SPLASH_COMPLETE_SECS && !worker_started {
                    worker_started = true;
                    let sender = sender.clone();
                    let current_exe = current_exe_in_loop.clone();
                    let install_dir = install_dir_in_loop.clone();

                    std::thread::spawn(move || {
                        let result = perform_installation(current_exe, install_dir, sender.clone());

                        let _ = sender.send(InstallerMessage::Finished(result));
                    });
                }

                while let Ok(message) = receiver.try_recv() {
                    match message {
                        InstallerMessage::Status(entry) => {
                            statuses.push(entry);
                        }

                        InstallerMessage::Finished(Ok(install_state)) => {
                            installation_succeeded = true;
                            install_completed_in_loop.store(true, Ordering::SeqCst);
                            development_build_in_loop.store(
                                cfg!(debug_assertions)
                                    || install_state == PackageInstallState::DevelopmentBuild,
                                Ordering::SeqCst,
                            );
                        }

                        InstallerMessage::Finished(Err(error)) => {
                            eprintln!("[INSTALL ERROR] {error}");

                            installation_failed = true;
                            failure_detail = Some(error);

                            statuses.push(StatusEntry {
                                text: "INSTALL FAILED",
                                active: false,
                            });
                        }
                    }
                }

                statuses.update(now);

                let show_completion_prompt =
                    installation_succeeded && statuses.is_settled_on("READY");

                let input = egui_state.take_egui_input(&window);

                egui_state.egui_ctx().begin_pass(input);

                if elapsed < INSTALLER_SPLASH_COMPLETE_SECS {
                    draw_installer_splash(egui_state.egui_ctx(), elapsed);
                } else {
                    let (status, status_active, status_age, fade_out_age) =
                        statuses.current_view(now);

                    let user_action = draw_installer_screen(
                        egui_state.egui_ctx(),
                        elapsed - INSTALLER_SPLASH_COMPLETE_SECS,
                        status,
                        status_age,
                        status_active,
                        fade_out_age,
                        show_completion_prompt,
                        failure_detail.as_deref(),
                    );

                    if installation_failed {
                        match user_action {
                            // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
                            Some(InstallerUserAction::Retry) => {
                                installation_failed = false;
                                installation_succeeded = false;
                                failure_detail = None;
                                worker_started = false;
                                statuses = StatusStream::new();
                                statuses.push(StatusEntry {
                                    text: "RETRYING INSTALLATION",
                                    active: true,
                                });
                            }
                            Some(InstallerUserAction::Bypass) => {
                                installation_failed = false;
                                installation_succeeded = true;
                                failure_detail = None;
                                install_completed_in_loop.store(true, Ordering::SeqCst);
                                development_build_in_loop
                                    .store(cfg!(debug_assertions), Ordering::SeqCst);
                                statuses = StatusStream::new();
                                statuses.push(StatusEntry {
                                    text: "PACKAGE SKIPPED BY USER",
                                    active: false,
                                });
                                statuses.push(StatusEntry {
                                    text: "READY",
                                    active: false,
                                });
                            }
                            _ => {}
                        }
                    }

                    if show_completion_prompt {
                        match user_action {
                            Some(InstallerUserAction::Launch) => {
                                launch_installed_in_loop.store(true, Ordering::SeqCst);
                                elwt.exit();
                                return;
                            }
                            Some(InstallerUserAction::Close) => {
                                launch_installed_in_loop.store(false, Ordering::SeqCst);
                                elwt.exit();
                                return;
                            }
                            Some(InstallerUserAction::Retry)
                            | Some(InstallerUserAction::Bypass)
                            | None => {}
                        }
                    }
                }

                let full_output = egui_state.egui_ctx().end_pass();

                egui_state.handle_platform_output(&window, full_output.platform_output);

                let paint_jobs = egui_state
                    .egui_ctx()
                    .tessellate(full_output.shapes, full_output.pixels_per_point);

                unsafe {
                    let size = window.inner_size();

                    gl::Viewport(0, 0, size.width as i32, size.height as i32);

                    gl::ClearColor(0.0, 0.0, 0.0, 1.0);
                    gl::Clear(gl::COLOR_BUFFER_BIT);

                    for (id, image_delta) in &full_output.textures_delta.set {
                        egui_glow.set_texture(*id, image_delta);
                    }

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
                    .expect("Failed to swap installer buffers");

                if !installation_failed {
                    window.request_redraw();
                } else {
                    window.request_redraw();
                }
            }

            _ => {}
        })
        .map_err(|err| format!("Installer event loop failed: {err}"))?;

    // A failed install must leave the downloaded installer available for retry.
    // Only a successful external Installer.exe is allowed to self-delete.
    if install_completed.load(Ordering::SeqCst)
        && is_installer_self_cleanup_eligible(&current_exe, &install_dir)
    {
        if let Err(err) = schedule_installer_self_cleanup(&current_exe) {
            eprintln!("[INSTALL CLEANUP WARNING] {err}");
        }
    }

    if launch_installed.load(Ordering::SeqCst) {
        let is_dev = development_build.load(Ordering::SeqCst);
        let mut command = Command::new(&installed_exe);
        for arg in installer_launch_arguments(is_dev) {
            command.arg(arg);
        }
        command.spawn().map_err(|err| {
            format!(
                "Failed to launch installed Worldkiln at {}: {err}",
                installed_exe.display()
            )
        })?;
    }

    Ok(())
}
