// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::Mat4;

#[derive(Default)]
pub(super) struct ContactFoamDepth {
    texture: u32,
    fbo: u32,
    width: i32,
    height: i32,
    supported: bool,
    failed: bool,
    origin: (i32, i32),
    projection: [f32; 2],
}

impl ContactFoamDepth {
    pub(super) fn no_gl() -> Self {
        Self::default()
    }

    pub(super) fn capture(
        &mut self,
        viewport: (i32, i32, i32, i32),
        projection: Mat4,
        water_scene_available: bool,
    ) -> bool {
        let (x, y, w, h) = viewport;
        self.supported = false;
        if !water_scene_available || self.failed || w <= 0 || h <= 0 {
            return false;
        }
        unsafe {
            if self.texture == 0 || self.width != w || self.height != h {
                self.free();
                gl::GenTextures(1, &mut self.texture);
                gl::ActiveTexture(gl::TEXTURE8);
                gl::BindTexture(gl::TEXTURE_2D, self.texture);
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::DEPTH_COMPONENT24 as i32,
                    w,
                    h,
                    0,
                    gl::DEPTH_COMPONENT,
                    gl::UNSIGNED_INT,
                    std::ptr::null(),
                );
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_COMPARE_MODE, gl::NONE as i32);
                gl::GenFramebuffers(1, &mut self.fbo);
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.fbo);
                gl::FramebufferTexture2D(
                    gl::FRAMEBUFFER,
                    gl::DEPTH_ATTACHMENT,
                    gl::TEXTURE_2D,
                    self.texture,
                    0,
                );
                gl::DrawBuffer(gl::NONE);
                gl::ReadBuffer(gl::NONE);
                let status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);
                gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
                gl::BindTexture(gl::TEXTURE_2D, 0);
                gl::ActiveTexture(gl::TEXTURE0);
                if status != gl::FRAMEBUFFER_COMPLETE {
                    self.free();
                    self.failed = true;
                    eprintln!("[WATER CONTACT FOAM] Unsupported depth framebuffer: {status:#x}");
                    return false;
                }
                self.width = w;
                self.height = h;
            }
            let scissor_was_enabled = gl::IsEnabled(gl::SCISSOR_TEST) == gl::TRUE;
            if scissor_was_enabled {
                gl::Disable(gl::SCISSOR_TEST);
            }
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.fbo);
            gl::BlitFramebuffer(
                x,
                y,
                x + w,
                y + h,
                0,
                0,
                w,
                h,
                gl::DEPTH_BUFFER_BIT,
                gl::NEAREST,
            );
            gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, 0);
            gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
            if scissor_was_enabled {
                gl::Enable(gl::SCISSOR_TEST);
            }
        }
        self.origin = (x, y);
        self.projection = [projection.z_axis.z, projection.w_axis.z];
        self.supported = true;
        true
    }

    pub(super) fn bind(&self, program: u32) {
        unsafe {
            gl::Uniform1i(
                super::resources::get_uniform_location(program, "u_object_foam_enabled"),
                i32::from(self.supported && self.texture != 0),
            );
            if !self.supported {
                return;
            }
            gl::Uniform1i(
                super::resources::get_uniform_location(program, "u_object_foam_depth"),
                8,
            );
            gl::Uniform4f(
                super::resources::get_uniform_location(program, "u_object_foam_viewport"),
                self.origin.0 as f32,
                self.origin.1 as f32,
                1.0 / (self.width.max(1) as f32),
                1.0 / (self.height.max(1) as f32),
            );
            gl::Uniform2f(
                super::resources::get_uniform_location(program, "u_object_foam_projection"),
                self.projection[0],
                self.projection[1],
            );
            gl::ActiveTexture(gl::TEXTURE8);
            gl::BindTexture(gl::TEXTURE_2D, self.texture);
            gl::ActiveTexture(gl::TEXTURE0);
        }
    }

    pub(super) fn free(&mut self) {
        unsafe {
            if self.fbo != 0 {
                gl::DeleteFramebuffers(1, &self.fbo);
            }
            if self.texture != 0 {
                gl::DeleteTextures(1, &self.texture);
            }
        }
        self.fbo = 0;
        self.texture = 0;
        self.width = 0;
        self.height = 0;
        self.supported = false;
    }
}

pub(super) const OBJECT_CONTACT_FOAM_GLSL: &str = r#"
uniform sampler2D u_object_foam_depth;
uniform bool u_object_foam_enabled;
uniform vec4 u_object_foam_viewport; // origin.xy, inverse-size.zw
uniform vec2 u_object_foam_projection; // projection[2][2], projection[3][2]
float wk_object_contact_eye_depth(float depth) {
    float divisor = depth*2.0-1.0+u_object_foam_projection.x;
    return abs(u_object_foam_projection.y) / max(abs(divisor),0.000001);
}
float wk_object_contact_foam() {
    if(!u_object_foam_enabled || u_horizon_pass || v_swash>0.5)
        return 0.0;
    vec2 uv=(gl_FragCoord.xy-u_object_foam_viewport.xy)*
        u_object_foam_viewport.zw;
    if(any(lessThanEqual(uv,vec2(0.0))) ||
       any(greaterThanEqual(uv,vec2(1.0)))) return 0.0;
    float water_depth=wk_object_contact_eye_depth(gl_FragCoord.z);
    float scene=texture(u_object_foam_depth,uv).r;
    float contact=0.0;
    if(scene<0.99999) {
        float separation=wk_object_contact_eye_depth(scene)-water_depth;
        // Positive: the terrain/Cell/Actor is behind the visible water
        // surface. Use actual eye-space distance, independent of camera height.
        contact=1.0-smoothstep(0.08,1.20,max(separation,0.0));
        contact*=step(-0.025,separation);
    }
    // Vertical walls at the waterline are just OUTSIDE the water fragment.
    // Only query four adjacent pixels if the central water pixel is not
    // already touching the object. Bounded independent of object count.
    if(contact<0.05) {
        vec2 du=vec2(u_object_foam_viewport.z,0.0)*2.0;
        vec2 dv=vec2(0.0,u_object_foam_viewport.w)*2.0;
        for(int i=0;i<4;++i) {
            vec2 offset=i==0?du:i==1?-du:i==2?dv:-dv;
            vec2 sample_uv=clamp(uv+offset,vec2(0.001),vec2(0.999));
            float near_z=texture(u_object_foam_depth,sample_uv).r;
            if(near_z>=0.99999) continue;
            float delta=wk_object_contact_eye_depth(near_z)-water_depth;
            // A nearer opaque wall at an adjacent pixel can intersect the
            // water surface. Never extend foam onto the object's own pixels.
            float edge=(1.0-smoothstep(0.08,0.85,abs(delta)))*
                step(delta,0.0);
            contact=max(contact,edge*0.80);
        }
    }
    if(contact<0.015) return 0.0;
    // Fine local texture breaks up the rim without making disconnected
    // triangles or projecting foam onto the beach.
    // Smooth, animated noise replaces pixelated square contact cells.
    // Both foam controls affect object and shore contact, not only waves.
    vec2 grain=v_world_pos.xz*max(u_foam_scale,0.25)*1.35
        +vec2(0.13,-0.09)*u_time*u_wave_speed;
    float noise=foam_noise(grain+u_wave_seed*0.0137);
    float breakup=smoothstep(0.22,0.80,noise);
    return clamp(contact*mix(0.32,1.0,breakup)*u_foam_amount,0.0,1.0);
}
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn blank_scene_skips_depth_capture_and_invalidates_previous_support() {
        let mut depth = super::ContactFoamDepth::no_gl();
        depth.supported = true;
        depth.texture = 42;
        assert!(!depth.capture((0, 0, 1912, 589), glam::Mat4::IDENTITY, false));
        assert!(!depth.supported);
        assert_eq!(depth.texture, 42);
    }

    #[test]
    fn independent_contact_is_depth_based_and_bounded() {
        let glsl = super::OBJECT_CONTACT_FOAM_GLSL;
        assert!(glsl.contains("gl_FragCoord.z"));
        assert!(glsl.contains("u_object_foam_depth"));
        assert!(glsl.contains("for(int i=0;i<4;++i)"));
        assert!(!glsl.contains("u_underwater_surface_y"));
        assert!(!glsl.contains("u_camera_position"));
        assert!(!glsl.contains("u_ocean_contact_atlas"));
        assert!(!glsl.contains("v_foam"));
    }
}
