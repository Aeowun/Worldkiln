use glam::Vec3;

use super::ScriptHostBridge;

impl<'a> ScriptHostBridge<'a> {
    pub fn get_camera_horizontal_basis(&self) -> ([f32; 3], [f32; 3]) {
        if let Some(ref camera) = self.gameplay_camera {
            let (forward, right) = camera.get_horizontal_basis();

            (
                [forward.x, forward.y, forward.z],
                [right.x, right.y, right.z],
            )
        } else {
            ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0])
        }
    }

    pub fn set_camera_position(&mut self, position: [f32; 3]) {
        if let Some(ref mut camera) = self.gameplay_camera {
            camera.current_position = Vec3::new(position[0], position[1], position[2]);
        }
    }

    pub fn set_camera_target(&mut self, target: [f32; 3]) {
        if let Some(ref mut camera) = self.gameplay_camera {
            camera.current_target = Vec3::new(target[0], target[1], target[2]);
        }
    }

    pub fn set_camera_orientation(&mut self, yaw: f32, pitch: f32) {
        if let Some(ref mut camera) = self.gameplay_camera {
            camera.yaw = yaw;
            camera.pitch = pitch;
        }
    }

    pub fn resolve_camera_collision(&self, target: [f32; 3], desired: [f32; 3]) -> [f32; 3] {
        if let Some(ref camera) = self.gameplay_camera {
            let target = Vec3::new(target[0], target[1], target[2]);

            let desired = Vec3::new(desired[0], desired[1], desired[2]);

            let resolved = camera.resolve_collision(target, desired, self.world);

            [resolved.x, resolved.y, resolved.z]
        } else {
            desired
        }
    }
}
