// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{Renderer, shader, terrain};
use crate::world::terrain::TerrainWaterSettings;
use std::{collections::HashMap, sync::Arc};
const WATER_PREVIEW_VERTEX: &str = r#"#version 330 core
out vec3 v_world_pos;
out float v_foam;
out float v_depth;
out float v_wave_energy;
out vec2 v_flow;
void main() {
    vec2 p = vec2(gl_VertexID==1 ? 3.0 : -1.0, gl_VertexID==2 ? 3.0 : -1.0);
    vec2 uv=p*0.5+0.5;
    v_world_pos=vec3((uv.x-0.5)*8.0,0.0,(uv.y-0.5)*8.0);
    v_depth=clamp(uv.y,0.0,1.0);
    v_foam=(1.0-smoothstep(0.0,0.3,uv.y))*0.35;
    v_wave_energy=0.65;
    v_flow=vec2(0.0);
    gl_Position=vec4(p,0.0,1.0);
}
"#;
#[derive(Clone)]
pub(super) struct WaterPreviewProgram {
    pub program: u32,
    locations: HashMap<&'static str, i32>,
}
impl WaterPreviewProgram {
    fn new() -> Result<Self, String> {
        let program =
            shader::try_create_program(WATER_PREVIEW_VERTEX, terrain::WATER_FRAGMENT_SHADER)?;
        let names: &[&'static str] = &[
            "u_sky",
            "u_stars_water_layer_enabled",
            "u_stars_water_layer_texture",
            "u_camera_position",
            "u_time",
            "u_camera_underwater",
            "u_wave_height",
            "u_wave_speed",
            "u_wave_length",
            "u_wave_direction",
            "u_absorption_strength",
            "u_absorption_color",
            "u_scattering_strength",
            "u_scattering_color",
            "u_foam_amount",
            "u_foam_scale",
            "u_shallow_color",
            "u_deep_color",
            "u_foam_color",
            "u_underwater_color",
            "u_opacity",
            "u_underwater_opacity",
            "u_reflection_strength",
            "u_specular_strength",
            "u_atmosphere_enabled",
            "u_fog_color",
            "u_fog_start",
            "u_fog_end",
        ];
        let locations = names
            .iter()
            .map(|name| (*name, super::resources::get_uniform_location(program, name)))
            .collect();
        Ok(Self { program, locations })
    }
    fn at(&self, name: &str) -> i32 {
        self.locations.get(name).copied().unwrap_or(-1)
    }
}
#[derive(Clone)]
struct WaterDraw {
    shader: WaterPreviewProgram,
    vao: u32,
    sky: u32,
    layer: Option<u32>,
    look: TerrainWaterSettings,
    time: f32,
}
impl WaterDraw {
    fn paint(&self, info: egui::PaintCallbackInfo) {
        let bounds = info.viewport_in_pixels();
        let clip = info.clip_rect_in_pixels();
        let x0 = bounds.left_px.max(clip.left_px);
        let y0 = bounds.from_bottom_px.max(clip.from_bottom_px);
        let x1 = (bounds.left_px + bounds.width_px).min(clip.left_px + clip.width_px);
        let y1 =
            (bounds.from_bottom_px + bounds.height_px).min(clip.from_bottom_px + clip.height_px);
        if x0 >= x1 || y0 >= y1 || self.vao == 0 || self.shader.program == 0 {
            return;
        }
        let at = |name: &str| self.shader.at(name);
        let rgb = |name: &str, c: [f32; 3]| unsafe {
            gl::Uniform3f(at(name), c[0], c[1], c[2]);
        };
        let scalar = |name: &str, v: f32| unsafe {
            gl::Uniform1f(at(name), v);
        };
        let l = self.look;
        unsafe {
            let mut previous_viewport = [0i32; 4];
            gl::GetIntegerv(gl::VIEWPORT, previous_viewport.as_mut_ptr());
            gl::Viewport(
                bounds.left_px,
                bounds.from_bottom_px,
                bounds.width_px,
                bounds.height_px,
            );
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(x0, y0, x1 - x0, y1 - y0);
            gl::Disable(gl::DEPTH_TEST);
            gl::Disable(gl::CULL_FACE);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::UseProgram(self.shader.program);
            gl::Uniform1i(at("u_sky"), 0);
            gl::Uniform1i(
                at("u_stars_water_layer_enabled"),
                i32::from(self.layer.is_some()),
            );
            gl::Uniform1i(at("u_stars_water_layer_texture"), 14);
            if let Some(layer) = self.layer {
                gl::ActiveTexture(gl::TEXTURE14);
                gl::BindTexture(gl::TEXTURE_2D, layer);
                gl::ActiveTexture(gl::TEXTURE0);
            }
            gl::Uniform1i(at("u_camera_underwater"), 0);
            gl::Uniform1i(at("u_atmosphere_enabled"), 0);
            rgb("u_camera_position", [1.8, 3.3, 3.7]);
            rgb("u_shallow_color", l.shallow_color);
            rgb("u_deep_color", l.deep_color);
            rgb("u_foam_color", l.foam_color);
            rgb("u_underwater_color", l.underwater_color);
            rgb("u_absorption_color", l.absorption_color);
            rgb("u_scattering_color", l.scattering_color);
            scalar("u_time", self.time);
            scalar("u_wave_height", l.wave_height);
            scalar("u_wave_speed", l.wave_speed);
            scalar("u_wave_length", l.wave_length);
            scalar("u_wave_direction", l.wave_direction_degrees.to_radians());
            scalar("u_absorption_strength", l.absorption_strength);
            scalar("u_scattering_strength", l.scattering_strength);
            scalar("u_foam_amount", l.foam_amount);
            scalar("u_foam_scale", l.foam_scale);
            scalar("u_opacity", l.opacity);
            scalar("u_underwater_opacity", l.underwater_opacity);
            scalar("u_reflection_strength", l.reflection_strength);
            scalar("u_specular_strength", l.specular_strength);
            scalar("u_fog_start", 0.0);
            scalar("u_fog_end", 1.0);
            rgb("u_fog_color", [0.0; 3]);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_CUBE_MAP, self.sky);
            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
            gl::BindVertexArray(0);
            gl::UseProgram(0);
            gl::Viewport(
                previous_viewport[0],
                previous_viewport[1],
                previous_viewport[2],
                previous_viewport[3],
            );
        }
    }
}
impl Renderer {
    pub(crate) fn water_material_preview(
        &self,
        rect: egui::Rect,
        look: TerrainWaterSettings,
    ) -> Result<egui::PaintCallback, String> {
        self.water_material_preview_with_layers(rect, look, None)
    }
    pub(crate) fn water_material_preview_with_layers(
        &self,
        rect: egui::Rect,
        look: TerrainWaterSettings,
        layer: Option<u32>,
    ) -> Result<egui::PaintCallback, String> {
        let mut saved = self.water_preview_program.borrow_mut();
        if saved.is_none() {
            *saved = Some(WaterPreviewProgram::new());
        }
        let shader = saved
            .as_ref()
            .expect("water shader cache")
            .as_ref()
            .map_err(Clone::clone)?
            .clone();
        drop(saved);
        let sky = self.get_cubemap(crate::project::sky_material::DEFAULT_SKY_ID);
        let draw = WaterDraw {
            shader,
            vao: self.block_vao,
            sky,
            layer,
            look,
            time: self.water_started.elapsed().as_secs_f32(),
        };
        Ok(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, _| draw.paint(info))),
        })
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn preview_reuses_worldkiln_water_fragment_shader() {
        let code = super::terrain::WATER_FRAGMENT_SHADER;
        assert!(code.contains("vec3 body = mix(shallow, deep, depth_mix)"));
        assert!(super::WATER_PREVIEW_VERTEX.contains("v_world_pos"));
        assert!(code.contains("u_opacity"));
    }
}
