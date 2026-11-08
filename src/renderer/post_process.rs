// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::project::material::graph_post_process::{self, EffectShader};
use crate::project::material::{MaterialCatalog, MaterialOutput, ParameterValue, PinType};
use crate::project::post_process_pipeline::{
    PostProcessMask, PostProcessObject, PostProcessPipeline, PostProcessRuntime, PostProcessStage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::path::Path;
use std::ptr;

const VERTEX: &str = r#"#version 330 core
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;
const MASK_VERTEX: &str = r#"#version 330 core
layout(location=0) in vec3 a_position;
uniform mat4 u_view_projection;
uniform mat4 u_model;
void main() { gl_Position=u_view_projection*u_model*vec4(a_position,1.0); }
"#;
const MASK_FRAGMENT: &str = r#"#version 330 core
uniform float u_channel;
out vec4 out_color;
void main() { out_color=vec4(u_channel/255.0,0.0,0.0,1.0); }
"#;

pub(super) struct MaskBindings {
    model: i32,
    channel: i32,
}
impl MaskBindings {
    pub unsafe fn new(program: u32, view_projection: glam::Mat4) -> Self {
        gl::UseProgram(program);
        let projection = gl::GetUniformLocation(program, c"u_view_projection".as_ptr());
        gl::UniformMatrix4fv(
            projection,
            1,
            gl::FALSE,
            view_projection.to_cols_array().as_ptr(),
        );
        Self {
            model: gl::GetUniformLocation(program, c"u_model".as_ptr()),
            channel: gl::GetUniformLocation(program, c"u_channel".as_ptr()),
        }
    }
    pub unsafe fn set(&self, model: glam::Mat4, channel: u8) {
        gl::UniformMatrix4fv(self.model, 1, gl::FALSE, model.to_cols_array().as_ptr());
        gl::Uniform1f(self.channel, channel as f32);
    }
}

struct CompiledEffect {
    program: u32,
    shader: EffectShader,
    needs_depth: bool,
    needs_mask: bool,
}
#[derive(Default)]
pub(super) struct ScenePostProcess {
    pub authored: PostProcessPipeline,
    pub runtime: PostProcessRuntime,
    programs: BTreeMap<String, Result<CompiledEffect, String>>,
    warned_skips: BTreeSet<String>,
    framebuffers: [u32; 2],
    textures: [u32; 2],
    depth_texture: u32,
    depth_attachment: u32,
    mask_framebuffer: u32,
    mask_texture: u32,
    mask_program: u32,
    unavailable: bool,
    vao: u32,
    width: i32,
    height: i32,
}
impl ScenePostProcess {
    pub fn load(&mut self, root: Option<&Path>) {
        self.runtime.reset();
        self.warned_skips.clear();
        self.authored = root
            .and_then(|p| match PostProcessPipeline::load(p) {
                Ok(pipeline) => Some(pipeline),
                Err(error) => {
                    crate::engine::diagnostics::report("ERROR", "Post Processing", error);
                    None
                }
            })
            .unwrap_or_else(PostProcessPipeline::new);
        self.invalidate_programs();
    }
    pub fn reload(&mut self, root: Option<&Path>) {
        self.load(root);
    }
    pub fn invalidate_programs(&mut self) {
        for compiled in self.programs.values() {
            if let Ok(compiled) = compiled {
                if compiled.program != 0 && gl::DeleteProgram::is_loaded() {
                    unsafe {
                        gl::DeleteProgram(compiled.program);
                    }
                }
            }
        }
        self.programs.clear();
    }
    unsafe fn ensure_targets(&mut self, w: i32, h: i32) -> Result<(), String> {
        if self.framebuffers[0] != 0 && self.width == w && self.height == h {
            return Ok(());
        }
        self.free_targets();
        if self.unavailable && self.width == w && self.height == h {
            return Err("Post-process framebuffer unavailable".into());
        }
        if self.vao == 0 {
            gl::GenVertexArrays(1, &mut self.vao);
        }
        for i in 0..2 {
            gl::GenFramebuffers(1, &mut self.framebuffers[i]);
            gl::GenTextures(1, &mut self.textures[i]);
            gl::BindTexture(gl::TEXTURE_2D, self.textures[i]);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                w,
                h,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                ptr::null(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.framebuffers[i]);
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                self.textures[i],
                0,
            );
            if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                self.free_targets();
                self.unavailable = true;
                self.width = w;
                self.height = h;
                return Err("Post-process scene-color framebuffer is incomplete".into());
            }
        }
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        gl::BindTexture(gl::TEXTURE_2D, 0);
        self.width = w;
        self.height = h;
        self.unavailable = false;
        Ok(())
    }
    unsafe fn ensure_depth_target(&mut self) -> Result<(), String> {
        if self.depth_texture != 0 {
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.framebuffers[0]);
            gl::FramebufferTexture2D(
                gl::DRAW_FRAMEBUFFER,
                self.depth_attachment,
                gl::TEXTURE_2D,
                self.depth_texture,
                0,
            );
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            return Ok(());
        }
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        let mut depth_bits = 0;
        let mut stencil_bits = 0;
        gl::GetFramebufferAttachmentParameteriv(
            gl::FRAMEBUFFER,
            gl::DEPTH,
            gl::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE,
            &mut depth_bits,
        );
        gl::GetFramebufferAttachmentParameteriv(
            gl::FRAMEBUFFER,
            gl::STENCIL,
            gl::FRAMEBUFFER_ATTACHMENT_STENCIL_SIZE,
            &mut stencil_bits,
        );
        if depth_bits <= 0 {
            return Err("Post-process Scene Depth requires a depth buffer".into());
        }
        let packed = depth_bits == 24 && stencil_bits > 0;
        let internal = if packed {
            gl::DEPTH24_STENCIL8
        } else if depth_bits <= 16 {
            gl::DEPTH_COMPONENT16
        } else if depth_bits <= 24 {
            gl::DEPTH_COMPONENT24
        } else {
            gl::DEPTH_COMPONENT32F
        };
        let (format, kind, attachment) = if packed {
            (
                gl::DEPTH_STENCIL,
                gl::UNSIGNED_INT_24_8,
                gl::DEPTH_STENCIL_ATTACHMENT,
            )
        } else if internal == gl::DEPTH_COMPONENT32F {
            (gl::DEPTH_COMPONENT, gl::FLOAT, gl::DEPTH_ATTACHMENT)
        } else {
            (gl::DEPTH_COMPONENT, gl::UNSIGNED_INT, gl::DEPTH_ATTACHMENT)
        };
        gl::GenTextures(1, &mut self.depth_texture);
        gl::BindTexture(gl::TEXTURE_2D, self.depth_texture);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            internal as i32,
            self.width,
            self.height,
            0,
            format,
            kind,
            ptr::null(),
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_COMPARE_MODE, gl::NONE as i32);
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.framebuffers[0]);
        gl::FramebufferTexture2D(
            gl::FRAMEBUFFER,
            attachment,
            gl::TEXTURE_2D,
            self.depth_texture,
            0,
        );
        self.depth_attachment = attachment;
        let ready = gl::CheckFramebufferStatus(gl::FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE;
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        gl::BindTexture(gl::TEXTURE_2D, 0);
        if !ready {
            gl::DeleteTextures(1, &self.depth_texture);
            self.depth_texture = 0;
            return Err("Post-process depth attachment is incomplete".into());
        }
        Ok(())
    }
    unsafe fn ensure_mask_target(&mut self) -> Result<(), String> {
        if self.mask_framebuffer != 0 {
            return Ok(());
        }
        self.mask_program = super::shader::try_create_program(MASK_VERTEX, MASK_FRAGMENT)?;
        gl::GenFramebuffers(1, &mut self.mask_framebuffer);
        gl::GenTextures(1, &mut self.mask_texture);
        gl::BindTexture(gl::TEXTURE_2D, self.mask_texture);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA8 as i32,
            self.width,
            self.height,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            ptr::null(),
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::BindFramebuffer(gl::FRAMEBUFFER, self.mask_framebuffer);
        gl::FramebufferTexture2D(
            gl::FRAMEBUFFER,
            gl::COLOR_ATTACHMENT0,
            gl::TEXTURE_2D,
            self.mask_texture,
            0,
        );
        let ok = gl::CheckFramebufferStatus(gl::FRAMEBUFFER) == gl::FRAMEBUFFER_COMPLETE;
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        gl::BindTexture(gl::TEXTURE_2D, 0);
        if !ok {
            self.free_targets();
            return Err("Post-process mask framebuffer incomplete".into());
        }
        Ok(())
    }
    unsafe fn free_targets(&mut self) {
        if self.mask_program != 0 && gl::DeleteProgram::is_loaded() {
            gl::DeleteProgram(self.mask_program);
        }
        self.mask_program = 0;
        if self.mask_framebuffer != 0 && gl::DeleteFramebuffers::is_loaded() {
            gl::DeleteFramebuffers(1, &self.mask_framebuffer);
        }
        if self.mask_texture != 0 && gl::DeleteTextures::is_loaded() {
            gl::DeleteTextures(1, &self.mask_texture);
        }
        self.mask_framebuffer = 0;
        self.mask_texture = 0;
        if self.depth_texture != 0 && gl::DeleteTextures::is_loaded() {
            gl::DeleteTextures(1, &self.depth_texture);
        }
        self.depth_texture = 0;
        self.depth_attachment = 0;
        for i in 0..2 {
            if self.framebuffers[i] != 0 && gl::DeleteFramebuffers::is_loaded() {
                gl::DeleteFramebuffers(1, &self.framebuffers[i]);
            }
            if self.textures[i] != 0 && gl::DeleteTextures::is_loaded() {
                gl::DeleteTextures(1, &self.textures[i]);
            }
            self.framebuffers[i] = 0;
            self.textures[i] = 0;
        }
        self.width = 0;
        self.height = 0;
    }
    fn compiled<'a>(
        &'a mut self,
        id: &str,
        catalog: &MaterialCatalog,
    ) -> Result<&'a CompiledEffect, String> {
        if !self.programs.contains_key(id) {
            let result = (|| {
                if catalog.resolve(id)? != MaterialOutput::PostProcess {
                    return Err("Assigned Material is not a Post Process Material".into());
                }
                let graph = catalog
                    .graph(id)?
                    .ok_or("Post Process Material has no graph")?;
                let shader = graph_post_process::compile(graph)?;
                let program = super::shader::try_create_program(VERTEX, &shader.fragment)?;
                Ok(CompiledEffect {
                    program,
                    shader,
                    needs_depth: graph.nodes.iter().any(|node| {
                        matches!(
                            node.operation,
                            crate::project::material::GraphOperation::SceneDepth
                                | crate::project::material::GraphOperation::SceneNormal
                        )
                    }),
                    needs_mask: graph.nodes.iter().any(|node| {
                        matches!(
                            node.operation,
                            crate::project::material::GraphOperation::ObjectMask
                        )
                    }),
                })
            })();
            self.programs.insert(id.into(), result);
        }
        self.programs
            .get(id)
            .unwrap()
            .as_ref()
            .map_err(Clone::clone)
    }
    pub(super) fn composite<F>(
        &mut self,
        viewport: (i32, i32, i32, i32),
        catalog: &MaterialCatalog,
        time: f32,
        inverse_projection: [f32; 16],
        draw_mask: F,
    ) -> Result<(), String>
    where
        F: FnOnce(u32, &BTreeMap<PostProcessObject, u8>),
    {
        let (x, y, w, h) = viewport;
        if w <= 0 || h <= 0 {
            return Ok(());
        }
        let passes = self.runtime.resolved(&self.authored);
        let mut valid = Vec::new();
        for pass in passes.iter().filter(|p| p.enabled) {
            if pass.stage != PostProcessStage::AfterTonemapping {
                if self.warned_skips.insert(format!("{}:stage", pass.id)) {
                    crate::engine::diagnostics::report(
                        "WARN",
                        "Post Processing",
                        format!("{}: Before Tonemapping passes are not implemented", pass.id),
                    );
                }
                continue;
            }
            if let Err(error) = self.compiled(&pass.material_id, catalog) {
                if self
                    .warned_skips
                    .insert(format!("{}:invalid:{}:{error}", pass.id, pass.material_id))
                {
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Post Processing",
                        format!("{}: {error}", pass.id),
                    );
                }
                continue;
            }
            valid.push(pass);
        }
        if valid.is_empty() {
            return Ok(());
        }
        let needs_mask = valid.iter().any(|pass| {
            pass.mask != PostProcessMask::All
                || self
                    .programs
                    .get(&pass.material_id)
                    .and_then(|v| v.as_ref().ok())
                    .is_some_and(|effect| effect.needs_mask)
        });
        let needs_depth = needs_mask
            || valid.iter().any(|pass| {
                self.programs
                    .get(&pass.material_id)
                    .and_then(|v| v.as_ref().ok())
                    .is_some_and(|effect| effect.needs_depth)
            });
        let assignments = if needs_mask {
            self.runtime.effective_object_channels(&self.authored)
        } else {
            BTreeMap::new()
        };
        unsafe {
            self.ensure_targets(w, h)?;
            if needs_depth {
                self.ensure_depth_target()?;
            }
            if needs_mask {
                self.ensure_mask_target()?;
            }
            let scissor = gl::IsEnabled(gl::SCISSOR_TEST) == gl::TRUE;
            let mut viewport_before = [0_i32; 4];
            gl::GetIntegerv(gl::VIEWPORT, viewport_before.as_mut_ptr());
            gl::Disable(gl::SCISSOR_TEST);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.framebuffers[0]);
            gl::BlitFramebuffer(
                x,
                y,
                x + w,
                y + h,
                0,
                0,
                w,
                h,
                gl::COLOR_BUFFER_BIT | if needs_depth { gl::DEPTH_BUFFER_BIT } else { 0 },
                gl::NEAREST,
            );
            let error = gl::GetError();
            if needs_depth {
                gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.framebuffers[0]);
                gl::FramebufferTexture2D(
                    gl::DRAW_FRAMEBUFFER,
                    self.depth_attachment,
                    gl::TEXTURE_2D,
                    0,
                    0,
                );
            }
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            if error != gl::NO_ERROR {
                if scissor {
                    gl::Enable(gl::SCISSOR_TEST);
                }
                return Err(format!("Scene capture failed: {error:#x}"));
            }
            if needs_mask {
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.mask_framebuffer);
                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER,
                    self.depth_attachment,
                    gl::TEXTURE_2D,
                    self.depth_texture,
                    0,
                );
                if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                    gl::FramebufferTexture2D(
                        gl::FRAMEBUFFER,
                        self.depth_attachment,
                        gl::TEXTURE_2D,
                        0,
                        0,
                    );
                    gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                    gl::Viewport(
                        viewport_before[0],
                        viewport_before[1],
                        viewport_before[2],
                        viewport_before[3],
                    );
                    if scissor {
                        gl::Enable(gl::SCISSOR_TEST);
                    }
                    return Err("Post-process mask depth attachment incomplete".into());
                }
                gl::Viewport(0, 0, w, h);
                let clear = [0.0f32, 0.0, 0.0, 0.0];
                gl::ClearBufferfv(gl::COLOR, 0, clear.as_ptr());
                if !assignments.is_empty() {
                    gl::Disable(gl::BLEND);
                    gl::Disable(gl::CULL_FACE);
                    gl::Enable(gl::DEPTH_TEST);
                    gl::DepthMask(gl::FALSE);
                    let mut previous_depth = 0;
                    gl::GetIntegerv(gl::DEPTH_FUNC, &mut previous_depth);
                    gl::DepthFunc(gl::LEQUAL);
                    gl::UseProgram(self.mask_program);
                    draw_mask(self.mask_program, &assignments);
                    gl::DepthFunc(previous_depth as u32);
                }
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.mask_framebuffer);
                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER,
                    self.depth_attachment,
                    gl::TEXTURE_2D,
                    0,
                    0,
                );
                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            }
            gl::Disable(gl::SCISSOR_TEST);
            gl::Disable(gl::DEPTH_TEST);
            gl::DepthMask(gl::FALSE);
            gl::Disable(gl::CULL_FACE);
            gl::Disable(gl::BLEND);
            gl::BindVertexArray(self.vao);
            let total = valid.len();
            let mut source = 0usize;
            for (index, pass) in valid.into_iter().enumerate() {
                let final_pass = index + 1 == total;
                if final_pass {
                    gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                    gl::Viewport(x, y, w, h);
                    gl::Enable(gl::SCISSOR_TEST);
                    gl::Scissor(x, y, w, h);
                } else {
                    gl::BindFramebuffer(gl::FRAMEBUFFER, self.framebuffers[1 - source]);
                    gl::Viewport(0, 0, w, h);
                }
                let effect = self
                    .programs
                    .get(&pass.material_id)
                    .unwrap()
                    .as_ref()
                    .unwrap();
                gl::UseProgram(effect.program);
                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, self.textures[source]);
                gl::Uniform1i(
                    gl::GetUniformLocation(effect.program, c"u_scene_color".as_ptr()),
                    0,
                );
                if needs_depth {
                    gl::ActiveTexture(gl::TEXTURE1);
                    gl::BindTexture(gl::TEXTURE_2D, self.depth_texture);
                    gl::Uniform1i(
                        gl::GetUniformLocation(effect.program, c"u_scene_depth".as_ptr()),
                        1,
                    );
                    gl::UniformMatrix4fv(
                        gl::GetUniformLocation(effect.program, c"u_inverse_projection".as_ptr()),
                        1,
                        gl::FALSE,
                        inverse_projection.as_ptr(),
                    );
                    gl::ActiveTexture(gl::TEXTURE0);
                }
                gl::Uniform1f(
                    gl::GetUniformLocation(effect.program, c"u_time".as_ptr()),
                    time,
                );
                let (mask_mode, mask_channel) = match pass.mask {
                    PostProcessMask::All => (0, 0.0),
                    PostProcessMask::Only(channel) => (1, channel as f32),
                    PostProcessMask::Except(channel) => (2, channel as f32),
                };
                gl::Uniform1i(
                    gl::GetUniformLocation(effect.program, c"u_mask_mode".as_ptr()),
                    mask_mode,
                );
                gl::Uniform1f(
                    gl::GetUniformLocation(effect.program, c"u_mask_channel".as_ptr()),
                    mask_channel,
                );
                if needs_mask {
                    gl::ActiveTexture(gl::TEXTURE2);
                    gl::BindTexture(gl::TEXTURE_2D, self.mask_texture);
                    gl::Uniform1i(
                        gl::GetUniformLocation(effect.program, c"u_scene_mask".as_ptr()),
                        2,
                    );
                    gl::ActiveTexture(gl::TEXTURE0);
                }
                for uniform in &effect.shader.uniforms {
                    let name = CString::new(uniform.shader_name.as_str()).unwrap();
                    let location = gl::GetUniformLocation(effect.program, name.as_ptr());
                    let data = uniform
                        .parameter_name
                        .as_ref()
                        .and_then(|name| pass.parameters.get(name))
                        .map(|value| match value {
                            ParameterValue::Scalar(v) => [*v, 0.0, 0.0, 0.0],
                            ParameterValue::Vec2(v) => [v[0], v[1], 0.0, 0.0],
                            ParameterValue::Boolean(v) => [f32::from(*v as u8), 0.0, 0.0, 0.0],
                            ParameterValue::Color3(v) => [v[0], v[1], v[2], 0.0],
                            ParameterValue::Color4(v) => *v,
                        })
                        .unwrap_or(uniform.value);
                    match uniform.kind {
                        PinType::Scalar => gl::Uniform1f(location, data[0]),
                        PinType::Vec2 => gl::Uniform2f(location, data[0], data[1]),
                        PinType::Vec3 => gl::Uniform3f(location, data[0], data[1], data[2]),
                        PinType::Color => {
                            gl::Uniform4f(location, data[0], data[1], data[2], data[3])
                        }
                    }
                }
                gl::DrawArrays(gl::TRIANGLES, 0, 3);
                source = 1 - source;
            }
            gl::BindVertexArray(0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            if needs_depth {
                gl::ActiveTexture(gl::TEXTURE1);
                gl::BindTexture(gl::TEXTURE_2D, 0);
                gl::ActiveTexture(gl::TEXTURE0);
            }
            if needs_mask {
                gl::ActiveTexture(gl::TEXTURE2);
                gl::BindTexture(gl::TEXTURE_2D, 0);
                gl::ActiveTexture(gl::TEXTURE0);
            }
            gl::UseProgram(0);
            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::DEPTH_TEST);
            gl::Enable(gl::CULL_FACE);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            if !scissor {
                gl::Disable(gl::SCISSOR_TEST);
            }
            gl::Viewport(
                viewport_before[0],
                viewport_before[1],
                viewport_before[2],
                viewport_before[3],
            );
        }
        Ok(())
    }
    pub fn free(&mut self) {
        unsafe {
            self.invalidate_programs();
            self.free_targets();
            if self.vao != 0 && gl::DeleteVertexArrays::is_loaded() {
                gl::DeleteVertexArrays(1, &self.vao);
            }
        }
        self.vao = 0;
    }
}

#[cfg(all(test, target_os = "windows"))]
mod gpu_smoke {
    use super::*;
    use crate::project::material::{
        GraphLink, GraphNode, GraphOperation, MaterialDefinition, MaterialDocument, MaterialGraph,
        MaterialOutput,
    };
    use crate::project::post_process_pipeline::{
        PostProcessEffect, PostProcessObject, PostProcessObjectKind,
    };
    use glutin::{
        config::ConfigTemplateBuilder,
        context::ContextAttributesBuilder,
        display::GetGlDisplay,
        prelude::*,
        surface::{SurfaceAttributesBuilder, WindowSurface},
    };
    use glutin_winit::DisplayBuilder;
    use std::{collections::BTreeMap, ffi::CString, num::NonZeroU32};
    use winit::platform::windows::EventLoopBuilderExtWindows;
    use winit::{
        event_loop::EventLoop, raw_window_handle::HasWindowHandle, window::WindowAttributes,
    };

    #[test]
    #[ignore = "requires a local OpenGL 3.3 Windows graphics context"]
    fn hidden_gpu_post_process_smoke() {
        let mut loop_builder = EventLoop::builder();
        loop_builder.with_any_thread(true);
        let loop_ = loop_builder.build().unwrap();
        let attributes = WindowAttributes::default()
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(32.0, 32.0));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attributes))
            .build(
                &loop_,
                ConfigTemplateBuilder::new()
                    .with_depth_size(24)
                    .with_stencil_size(8),
                |configs| configs.min_by_key(|config| config.num_samples()).unwrap(),
            )
            .expect("Create hidden OpenGL test window");
        let window = window.expect("Hidden OpenGL window");
        let handle = window.window_handle().unwrap().as_raw();
        let context = unsafe {
            config.display().create_context(
                &config,
                &ContextAttributesBuilder::new().build(Some(handle)),
            )
        }
        .unwrap();
        let surface = unsafe {
            config.display().create_window_surface(
                &config,
                &SurfaceAttributesBuilder::<WindowSurface>::new().build(
                    handle,
                    NonZeroU32::new(32).unwrap(),
                    NonZeroU32::new(32).unwrap(),
                ),
            )
        }
        .unwrap();
        let _current = context.make_current(&surface).unwrap();
        gl::load_with(|name| {
            config
                .display()
                .get_proc_address(&CString::new(name).unwrap())
                .cast()
        });
        let mut material = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::PostProcess,
        });
        material.graph = Some(MaterialGraph {
            output_node: 2,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::Parameter {
                        name: "tint".into(),
                        value_type: PinType::Color,
                    },
                    position: [0.0, 0.0],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::PostProcessOutput,
                    position: [0.0, 0.0],
                },
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 2,
                target_pin: "color".into(),
            }],
        });
        let catalog = MaterialCatalog::from_documents(BTreeMap::from([(
            "GpuTest".to_owned(),
            material.clone(),
        )]));
        assert_eq!(
            catalog.resolve(&material.id).unwrap(),
            MaterialOutput::PostProcess
        );
        let mut stage = ScenePostProcess::default();
        stage.authored.passes.push(PostProcessEffect {
            id: "gpu_test".into(),
            material_id: material.id,
            enabled: true,
            stage: PostProcessStage::AfterTonemapping,
            mask: PostProcessMask::All,
            parameters: BTreeMap::from([(
                "tint".into(),
                ParameterValue::Color4([0.2, 0.4, 0.8, 1.0]),
            )]),
        });
        let read_center = || {
            let mut pixel = [0u8; 4];
            unsafe {
                gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
                gl::ReadPixels(
                    16,
                    16,
                    1,
                    1,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    pixel.as_mut_ptr().cast(),
                );
            }
            pixel
        };
        let clear_scene = || unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::Viewport(0, 0, 32, 32);
            gl::Disable(gl::SCISSOR_TEST);
            gl::Enable(gl::DEPTH_TEST);
            gl::DepthMask(gl::TRUE);
            gl::ClearDepth(0.5);
            gl::ClearColor(0.1, 0.15, 0.2, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        };
        clear_scene();
        stage
            .composite(
                (0, 0, 32, 32),
                &catalog,
                0.0,
                glam::Mat4::IDENTITY.to_cols_array(),
                |_, _| {},
            )
            .expect("GPU scene-color composition");
        let pixel = read_center();
        assert!(
            (pixel[0] as i32 - 51).abs() <= 4
                && (pixel[1] as i32 - 102).abs() <= 4
                && (pixel[2] as i32 - 204).abs() <= 4,
            "Post Process Material must replace scene pixel: {pixel:?}"
        );

        let authored_before = stage.authored.clone();
        stage
            .runtime
            .set_parameter(
                &stage.authored,
                "gpu_test",
                "tint",
                ParameterValue::Color4([0.7, 0.2, 0.1, 1.0]),
            )
            .unwrap();
        clear_scene();
        stage
            .composite(
                (0, 0, 32, 32),
                &catalog,
                0.0,
                glam::Mat4::IDENTITY.to_cols_array(),
                |_, _| {},
            )
            .unwrap();
        let changed = read_center();
        assert!(
            (changed[0] as i32 - 179).abs() <= 4 && (changed[2] as i32 - 26).abs() <= 4,
            "Transient typed GPU Material parameter was ignored: {changed:?}"
        );
        stage
            .runtime
            .set_enabled(&stage.authored, "gpu_test", false)
            .unwrap();
        clear_scene();
        stage
            .composite(
                (0, 0, 32, 32),
                &catalog,
                0.0,
                glam::Mat4::IDENTITY.to_cols_array(),
                |_, _| {},
            )
            .unwrap();
        let disabled = read_center();
        assert!(
            (disabled[0] as i32 - 26).abs() <= 4 && (disabled[2] as i32 - 51).abs() <= 4,
            "Disabled post-process pass changed scene pixels: {disabled:?}"
        );
        assert_eq!(
            stage.authored, authored_before,
            "Play overrides must not mutate saved effect inputs"
        );
        stage.runtime.reset();

        let object = PostProcessObject {
            kind: PostProcessObjectKind::Actor,
            id: 42,
        };
        stage.authored.set_object_channel(object, 1).unwrap();
        stage.authored.passes[0].mask = PostProcessMask::Only(1);
        let vertices: [f32; 9] = [-1.0, -1.0, 0.0, 3.0, -1.0, 0.0, -1.0, 3.0, 0.0];
        let (mut vao, mut vbo) = (0, 0);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&vertices) as isize,
                vertices.as_ptr().cast(),
                gl::STATIC_DRAW,
            );
            gl::EnableVertexAttribArray(0);
            gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, 12, ptr::null());
            gl::BindVertexArray(0);
        }
        let render_mask = |program: u32, channels: &BTreeMap<PostProcessObject, u8>| unsafe {
            let uniforms = MaskBindings::new(program, glam::Mat4::IDENTITY);
            uniforms.set(glam::Mat4::IDENTITY, channels[&object]);
            gl::BindVertexArray(vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
            gl::BindVertexArray(0);
        };
        clear_scene();
        stage
            .composite(
                (0, 0, 32, 32),
                &catalog,
                0.0,
                glam::Mat4::IDENTITY.to_cols_array(),
                render_mask,
            )
            .expect("GPU object mask composition");
        let included = read_center();
        assert!(
            (included[2] as i32 - 204).abs() <= 4,
            "Mask channel 1 must include marked geometry: {included:?}"
        );
        stage.authored.passes[0].mask = PostProcessMask::Only(2);
        clear_scene();
        stage
            .composite(
                (0, 0, 32, 32),
                &catalog,
                0.0,
                glam::Mat4::IDENTITY.to_cols_array(),
                render_mask,
            )
            .expect("GPU excluded object mask composition");
        let excluded = read_center();
        assert!(
            excluded[2] < 100,
            "Different mask channel must preserve the input scene: {excluded:?}"
        );
        unsafe {
            gl::DeleteBuffers(1, &vbo);
            gl::DeleteVertexArrays(1, &vao);
            assert_eq!(
                gl::GetError(),
                gl::NO_ERROR,
                "OpenGL errors during postprocessing"
            );
        }
        stage.free();
    }
}
