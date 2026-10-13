#[cfg(feature = "editor")]
pub mod app;

#[cfg(feature = "editor")]
pub mod install;

#[cfg(feature = "editor")]
pub mod install_ui;

#[cfg(feature = "editor")]
pub mod install_window;

pub mod attachment;
pub mod audio;
pub mod diagnostics;
pub mod entity;
pub mod entity_package;
pub mod entity_visual;
pub mod mouse;
pub mod particles;
pub mod physics;
pub mod preferences;
pub mod save_state;
pub mod ui;
// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:17 -05:00
pub mod ui_media;
// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:34 -05:00
#[cfg(windows)]
pub mod ui_video_windows;
// Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09
pub mod ui_authoring;
#[cfg(windows)]
mod ui_video_audio_windows;

#[cfg(feature = "editor")]
pub mod build;

#[cfg(feature = "editor")]
// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:03 -05:00
pub mod game_package;

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
