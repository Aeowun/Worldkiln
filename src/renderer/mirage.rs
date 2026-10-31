// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::ffi::CString;
use std::ptr;

const VERTEX: &str = r#"#version 330 core
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p * 0.5;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const FRAGMENT: &str = r#"#version 330 core
in vec2 v_uv;
out vec4 out_color;
uniform sampler2D u_color;
uniform sampler2D u_depth;
uniform vec2 u_frame_size;
uniform vec4 u_viewport; // x,y,width,height in actual framebuffer pixels
uniform float u_time;
uniform float u_pitch;
uniform float u_strength;

float hash21(vec2 p) {
    p = fract(p * vec2(123.34, 345.45));
    p += dot(p, p + 34.345);
    return fract(p.x * p.y);
}
float noise2(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2(1,0)), f.x),
               mix(hash21(i + vec2(0,1)), hash21(i + vec2(1,1)), f.x), f.y);
}
float linear_depth(float z) {
    const float n = 0.1;
    const float f = 1000.0;
    return (2.0 * n * f) / (f + n - (2.0 * z - 1.0) * (f - n));
}
void main() {
    vec2 scene_uv = gl_FragCoord.xy / u_frame_size;
    vec2 local_uv = (gl_FragCoord.xy - u_viewport.xy) / u_viewport.zw;
    float source_depth = texture(u_depth, scene_uv).r;
    float sky = step(0.999999, source_depth);
    float distance_fade = sky > 0.5 ? 0.34 :
        smoothstep(45.0, 275.0, linear_depth(source_depth));

    // Restrict shimmer to a world-horizon-aligned band and near-level views.
    float camera_gate = 1.0 - smoothstep(0.09, 0.50, abs(u_pitch));
    float horizon_y = 0.5 - tan(u_pitch) / (2.0 * tan(radians(30.0)));
    float band = 1.0 - smoothstep(0.02, 0.34, abs(local_uv.y - horizon_y));
    float effect = camera_gate * band * distance_fade * clamp(u_strength, 0.0, 2.0);
    vec2 uv = scene_uv;

    if (effect > 0.00001) {
        // Scrolling world-independent noise at two scales, limited to a few pixels.
        vec2 field = local_uv * vec2(38.0, 26.0);
        float a = noise2(field + vec2(u_time * 0.27, u_time * -0.17));
        float b = noise2(field * 2.63 + vec2(u_time * -0.43, u_time * 0.31));
        float c = noise2(field * 0.43 + vec2(u_time * 0.09, u_time * -0.23));
        vec2 delta_px = vec2((a - 0.5) * 2.2 + (b - 0.5) * 1.25,
                             (b - 0.5) * 0.8 + (c - 0.5) * 1.2);
        vec2 proposed = clamp(scene_uv + effect * delta_px / u_frame_size,
            (u_viewport.xy + vec2(0.5)) / u_frame_size,
            (u_viewport.xy + u_viewport.zw - vec2(0.5)) / u_frame_size);
        float sample_depth = texture(u_depth, proposed).r;
        // Reject foreground/background edge disocclusions.
        float sample_distance = linear_depth(sample_depth);
        float source_distance = linear_depth(source_depth);
        float mismatch = abs(sample_distance - source_distance);
        float edge_gate = 1.0 - smoothstep(1.0, max(2.0, source_distance * 0.12), mismatch);
        uv = mix(scene_uv, proposed, edge_gate);
    }
    // Sky Material color is sampled from the same completed scene, not rebuilt.
    out_color = texture(u_color, uv);
}
"#;

#[derive(Default)]
pub(super) struct MiragePass {
    framebuffer: u32,
    color: u32,
    depth: u32,
    program: u32,
    vao: u32,
    width: i32,
    height: i32,
    capture_failed: bool,
}

impl MiragePass {
    pub(super) unsafe fn target(&mut self, width: i32, height: i32) -> Option<u32> {
        if width <= 0 || height <= 0 {
            return None;
        }
        if self.capture_failed && self.width == width && self.height == height {
            return None;
        }
        if self.framebuffer != 0 && self.width == width && self.height == height {
            return Some(self.framebuffer);
        }
        unsafe {
            self.free_target();
            if self.program == 0 {
                self.program = super::shader::create_program(VERTEX, FRAGMENT);
                gl::GenVertexArrays(1, &mut self.vao);
                gl::UseProgram(self.program);
                for (uniform, unit) in [("u_color", 0), ("u_depth", 1)] {
                    let name = CString::new(uniform).unwrap();
                    gl::Uniform1i(gl::GetUniformLocation(self.program, name.as_ptr()), unit);
                }
                gl::UseProgram(0);
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
                self.capture_failed = true;
                self.width = width;
                self.height = height;
                eprintln!(
                    "Horizon Mirage unavailable: the current framebuffer has no depth buffer"
                );
                return None;
            }
            let packed_depth = depth_bits == 24 && stencil_bits > 0;
            let depth_internal = if packed_depth {
                gl::DEPTH24_STENCIL8
            } else if depth_bits <= 16 {
                gl::DEPTH_COMPONENT16
            } else if depth_bits <= 24 {
                gl::DEPTH_COMPONENT24
            } else {
                gl::DEPTH_COMPONENT32F
            };
            gl::GenFramebuffers(1, &mut self.framebuffer);
            gl::BindFramebuffer(gl::FRAMEBUFFER, self.framebuffer);
            gl::GenTextures(1, &mut self.color);
            gl::BindTexture(gl::TEXTURE_2D, self.color);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                width,
                height,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                ptr::null(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                self.color,
                0,
            );

            gl::GenTextures(1, &mut self.depth);
            gl::BindTexture(gl::TEXTURE_2D, self.depth);
            let (pixel_format, pixel_type, attachment) = if packed_depth {
                (
                    gl::DEPTH_STENCIL,
                    gl::UNSIGNED_INT_24_8,
                    gl::DEPTH_STENCIL_ATTACHMENT,
                )
            } else if depth_internal == gl::DEPTH_COMPONENT32F {
                (gl::DEPTH_COMPONENT, gl::FLOAT, gl::DEPTH_ATTACHMENT)
            } else {
                (gl::DEPTH_COMPONENT, gl::UNSIGNED_INT, gl::DEPTH_ATTACHMENT)
            };
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                depth_internal as i32,
                width,
                height,
                0,
                pixel_format,
                pixel_type,
                ptr::null(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::FramebufferTexture2D(gl::FRAMEBUFFER, attachment, gl::TEXTURE_2D, self.depth, 0);

            let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            if status != gl::FRAMEBUFFER_COMPLETE {
                eprintln!("Horizon Mirage disabled: framebuffer status {status:#x}");
                self.free_target();
                self.capture_failed = true;
                self.width = width;
                self.height = height;
                return None;
            }
            self.width = width;
            self.height = height;
            Some(self.framebuffer)
        }
    }

    pub(super) unsafe fn composite(
        &mut self,
        viewport: (i32, i32, i32, i32),
        pitch: f32,
        time: f32,
        strength: f32,
    ) {
        if self.framebuffer == 0 || viewport.2 <= 0 || viewport.3 <= 0 {
            return;
        }
        unsafe {
            let (x, y, w, h) = viewport;
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.framebuffer);
            for _ in 0..4 {
                if gl::GetError() == gl::NO_ERROR {
                    break;
                }
            }
            gl::BlitFramebuffer(
                x,
                y,
                x + w,
                y + h,
                x,
                y,
                x + w,
                y + h,
                gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT,
                gl::NEAREST,
            );
            let capture_error = gl::GetError();
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
            if capture_error != gl::NO_ERROR {
                self.capture_failed = true;
                eprintln!("Horizon Mirage disabled: GPU scene capture failed ({capture_error:#x})");
                return;
            }
            gl::Viewport(x, y, w, h);
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(x, y, w, h);
            gl::Disable(gl::DEPTH_TEST);
            gl::DepthMask(gl::FALSE);
            gl::Disable(gl::CULL_FACE);
            gl::Disable(gl::BLEND);
            gl::UseProgram(self.program);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.color);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.depth);
            for (name, value) in [
                ("u_time", time),
                ("u_pitch", pitch),
                ("u_strength", strength),
            ] {
                let location =
                    gl::GetUniformLocation(self.program, CString::new(name).unwrap().as_ptr());
                gl::Uniform1f(location, value);
            }
            gl::Uniform2f(
                gl::GetUniformLocation(self.program, c"u_frame_size".as_ptr()),
                self.width as f32,
                self.height as f32,
            );
            gl::Uniform4f(
                gl::GetUniformLocation(self.program, c"u_viewport".as_ptr()),
                x as f32,
                y as f32,
                w as f32,
                h as f32,
            );
            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, 0, 3);
            gl::BindVertexArray(0);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::DEPTH_TEST);
            gl::Enable(gl::CULL_FACE);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        }
    }

    unsafe fn free_target(&mut self) {
        unsafe {
            if self.framebuffer != 0 {
                gl::DeleteFramebuffers(1, &self.framebuffer);
            }
            if self.color != 0 {
                gl::DeleteTextures(1, &self.color);
            }
            if self.depth != 0 {
                gl::DeleteTextures(1, &self.depth);
            }
        }
        self.framebuffer = 0;
        self.color = 0;
        self.depth = 0;
        self.width = 0;
        self.height = 0;
        self.capture_failed = false;
    }

    pub(super) fn free(&mut self) {
        unsafe {
            self.free_target();
            if self.program != 0 {
                gl::DeleteProgram(self.program);
            }
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
            }
            self.program = 0;
            self.vao = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn depth_and_color_shaders_are_bounded_to_viewport() {
        let code = super::FRAGMENT;
        assert!(code.contains("u_depth"));
        assert!(code.contains("clamp(scene_uv"));
        assert!(code.contains("u_viewport.xy"));
        assert!(code.contains("smoothstep(45.0, 275.0"));
        assert!(code.contains("clamp(u_strength, 0.0, 2.0)"));
    }
}
