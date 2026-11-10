// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
#[cfg(feature = "editor")]
pub mod app;
#[cfg(feature = "editor")]
pub(crate) mod member_services;

#[cfg(feature = "editor")]
pub mod install;

#[cfg(feature = "editor")]
pub mod install_ui;

#[cfg(feature = "editor")]
pub mod install_window;

pub mod attachment;
pub mod audio;
pub mod collision_geometry;
pub mod development_capture;
pub mod development_test_run;
pub mod diagnostics;
pub mod entity;
pub mod entity_package;
pub mod entity_visual;
pub mod input;
pub mod mouse;
pub mod particles;
pub mod physics;
pub mod preferences;
#[cfg(feature = "editor")]
pub mod telemetry;
#[cfg(feature = "editor")]
pub(crate) mod runtime_scene_analysis;
pub mod save_state;
pub mod spatial_world;
pub mod ui;
pub mod ui_authoring;
pub mod ui_compositor;
pub mod ui_media;
#[cfg(windows)]
mod ui_video_audio_windows;
#[cfg(windows)]
pub mod ui_video_windows;

#[cfg(feature = "editor")]
pub mod build;

#[cfg(feature = "editor")]
pub mod build_platform;
pub mod game_package;
#[cfg(feature = "editor")]
mod linux_game_build;

#[cfg(feature = "editor")]
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum View {
    Splash,
    Home,
    Editor,
}

#[cfg(feature = "editor")]
#[derive(PartialEq, Clone, Copy, Debug, Default)]
pub enum EditorMode {
    #[default]
    Editor,
    Play,
}

#[cfg(feature = "editor")]
pub(crate) mod progress_widget;
