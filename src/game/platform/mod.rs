pub mod desktop;

use std::path::{Path, PathBuf};

use crate::engine::audio::AudioSystem;
use crate::engine::save_state::SaveState;
use crate::renderer::Renderer;

pub trait GameRuntimeServices {
    fn create_renderer(&self, width: f32, height: f32) -> Renderer;
    fn create_audio(&self) -> AudioSystem;
    fn load_save_state(&self, project: &Path) -> Result<SaveState, String>;
    fn authored_world_path(&self, project: &Path) -> PathBuf;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameLifecycleEvent {
    Suspend,
    Resume,
    Shutdown,
}

pub struct DesktopRuntimeServices;

impl GameRuntimeServices for DesktopRuntimeServices {
    fn create_renderer(&self, width: f32, height: f32) -> Renderer {
        Renderer::new(width, height)
    }
    fn create_audio(&self) -> AudioSystem {
        AudioSystem::new()
    }
    fn load_save_state(&self, project: &Path) -> Result<SaveState, String> {
        SaveState::load(project).map_err(|error| error.to_string())
    }
    fn authored_world_path(&self, project: &Path) -> PathBuf {
        project.join("world.dat")
    }
}
