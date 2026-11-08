// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::PointLightData;

const LIGHT_TEXTURE_UNIT: u32 = 14;
const FLOATS_PER_LIGHT: usize = 12;

#[derive(Default)]
pub(super) struct PointLightStream {
    buffer: u32,
    texture: u32,
    max_texels: i32,
}

impl PointLightStream {
    pub(super) fn new() -> Self {
        let mut gpu = Self::default();
        unsafe {
            gl::GenBuffers(1, &mut gpu.buffer);
            gl::GenTextures(1, &mut gpu.texture);
            gl::GetIntegerv(gl::MAX_TEXTURE_BUFFER_SIZE, &mut gpu.max_texels);
            assert!(
                gpu.buffer != 0 && gpu.texture != 0 && gpu.max_texels > 0,
                "GPU does not support the Worldkiln light stream"
            );
            gl::BindBuffer(gl::TEXTURE_BUFFER, gpu.buffer);
            gl::BufferData(gl::TEXTURE_BUFFER, 32, std::ptr::null(), gl::DYNAMIC_DRAW);
            gl::BindTexture(gl::TEXTURE_BUFFER, gpu.texture);
            gl::TexBuffer(gl::TEXTURE_BUFFER, gl::RGBA32F, gpu.buffer);
            gl::BindTexture(gl::TEXTURE_BUFFER, 0);
            gl::BindBuffer(gl::TEXTURE_BUFFER, 0);
        }
        gpu
    }

    pub(super) fn upload(&self, lights: &[PointLightData], count_location: i32) {
        assert!(
            lights.len() <= i32::MAX as usize,
            "Light count exceeds shader index range"
        );
        assert!(
            lights.len().saturating_mul(3) <= self.max_texels as usize,
            "Scene exceeds the GPU texture-buffer capacity for Lights"
        );
        let mut packed = Vec::with_capacity(lights.len() * FLOATS_PER_LIGHT);
        for light in lights {
            packed.extend_from_slice(&[
                light.position.x,
                light.position.y,
                light.position.z,
                light.range,
                light.color.x,
                light.color.y,
                light.color.z,
                light.intensity,
                light.shadow_slot as f32,
                light.range,
                0.0,
                0.0,
            ]);
        }
        unsafe {
            gl::BindBuffer(gl::TEXTURE_BUFFER, self.buffer);
            gl::BufferData(
                gl::TEXTURE_BUFFER,
                (packed.len().max(FLOATS_PER_LIGHT) * std::mem::size_of::<f32>()) as isize,
                if packed.is_empty() {
                    std::ptr::null()
                } else {
                    packed.as_ptr().cast()
                },
                gl::STREAM_DRAW,
            );
            gl::BindBuffer(gl::TEXTURE_BUFFER, 0);
            gl::ActiveTexture(gl::TEXTURE0 + LIGHT_TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_BUFFER, self.texture);
            gl::Uniform1i(count_location, lights.len() as i32);
            gl::ActiveTexture(gl::TEXTURE1);
        }
    }

    pub(super) fn bind_sampler(program: u32) {
        unsafe {
            gl::UseProgram(program);
            gl::Uniform1i(
                super::resources::get_uniform_location(program, "u_point_light_data"),
                LIGHT_TEXTURE_UNIT as i32,
            );
        }
    }

    pub(super) fn free(&mut self) {
        unsafe {
            if self.texture != 0 {
                gl::DeleteTextures(1, &self.texture);
            }
            if self.buffer != 0 {
                gl::DeleteBuffers(1, &self.buffer);
            }
        }
        self.texture = 0;
        self.buffer = 0;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_light_records_are_three_vec4s_per_light() {
        assert_eq!(super::FLOATS_PER_LIGHT, 12);
        assert_eq!(super::LIGHT_TEXTURE_UNIT, 14);
    }
}
