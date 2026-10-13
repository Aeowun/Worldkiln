use super::{App, View};

impl App {
    pub fn render(&self) {
        match self.view {
            View::Splash => {
                self.renderer.render_home();
            }

            View::Home => {
                self.renderer.render_home();
            }

            View::Editor => {
                self.renderer.render_editor(
                    &self.editor,
                    &self.world,
                    &self.physics_world,
                    &self.entity_manager,
                    &self.character_system,
                    &self.gameplay_camera,
                    if self.is_left_mouse_down {
                        self.drag_start_coord
                    } else {
                        None
                    },
                );
            }
        }
    }
}
