// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::renderer::Renderer;
use crate::renderer::mesh::upload_block_vertices_3d;
use crate::renderer::resources::get_uniform_location;
use crate::renderer::shader::create_program;
use crate::renderer::terrain::UnderwaterState;
use crate::world::World;
use glam::{Mat4, Vec3};

pub struct SkyRenderer {
    pub(crate) program: u32,
    pub(crate) vao: u32,
    pub(crate) vbo: u32,
    pub(crate) view_projection_location: i32,
    pub(crate) model_location: i32,
    pub(crate) sky_stop_count_location: i32,
    pub(crate) sky_positions_location: i32,
    pub(crate) sky_colors_location: i32,
    pub(crate) sky_smooth_location: i32,
    pub(crate) underwater_enabled_location: i32,
    pub(crate) underwater_color_location: i32,
    pub(crate) underwater_blend_location: i32,
    pub(crate) underwater_camera_y_location: i32,
    pub(crate) local_water_camera_xz_location: i32,
    pub(crate) underwater_surface_y_location: i32,
    pub(crate) atmosphere_enabled_location: i32,
    pub(crate) fog_color_location: i32,
    pub(crate) horizon_strength_location: i32,
}

impl SkyRenderer {
    pub fn new() -> Self {
        let program = create_program(SKY_VERTEX_SHADER, SKY_FRAGMENT_SHADER);

        let mut vertices = Vec::new();

        add_face(
            &mut vertices,
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
        );
        add_face(
            &mut vertices,
            [-0.5, -0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
        );
        add_face(
            &mut vertices,
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        );
        add_face(
            &mut vertices,
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
        );
        add_face(
            &mut vertices,
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        );
        add_face(
            &mut vertices,
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
        );

        let (vao, vbo, _) = upload_block_vertices_3d(&vertices);

        Self {
            program,
            vao,
            vbo,
            view_projection_location: get_uniform_location(program, "u_view_projection"),
            model_location: get_uniform_location(program, "u_model"),
            sky_stop_count_location: get_uniform_location(program, "u_sky_stop_count"),
            sky_positions_location: get_uniform_location(program, "u_sky_positions[0]"),
            sky_colors_location: get_uniform_location(program, "u_sky_colors[0]"),
            sky_smooth_location: get_uniform_location(program, "u_sky_smooth"),
            underwater_enabled_location: get_uniform_location(program, "u_underwater_enabled"),
            underwater_color_location: get_uniform_location(program, "u_underwater_color"),
            underwater_blend_location: get_uniform_location(program, "u_underwater_blend"),
            underwater_camera_y_location: get_uniform_location(program, "u_underwater_camera_y"),
            local_water_camera_xz_location: get_uniform_location(
                program,
                "u_local_water_camera_xz",
            ),
            underwater_surface_y_location: get_uniform_location(program, "u_underwater_surface_y"),
            atmosphere_enabled_location: get_uniform_location(program, "u_atmosphere_enabled"),
            fog_color_location: get_uniform_location(program, "u_fog_color"),
            horizon_strength_location: get_uniform_location(program, "u_horizon_strength"),
        }
    }

    pub fn render(
        &self,
        renderer: &Renderer,
        world: &World,
        camera_pos: Vec3,
        view_projection: Mat4,
        underwater: Option<UnderwaterState>,
    ) {
        *renderer.active_sky_material.borrow_mut() = world.sky.material_asset.clone();
        if !world.sky.enabled {
            return;
        }

        unsafe {
            gl::Disable(gl::CULL_FACE);
            gl::DepthMask(gl::FALSE);

            gl::UseProgram(self.program);
            renderer.water_volume_mask.borrow().bind_sky();
            gl::UniformMatrix4fv(
                self.view_projection_location,
                1,
                gl::FALSE,
                view_projection.to_cols_array().as_ptr(),
            );

            let model = Mat4::from_translation(camera_pos);
            gl::UniformMatrix4fv(
                self.model_location,
                1,
                gl::FALSE,
                model.to_cols_array().as_ptr(),
            );

            gl::BindVertexArray(self.vao);
            let catalog = renderer.material_snapshot();
            let authored = world
                .sky
                .material_asset
                .as_deref()
                .and_then(|id| catalog.sky(id));
            let sky = authored
                .as_ref()
                .unwrap_or_else(|| crate::project::sky_material::default_sky());
            let mut positions = [0.0_f32; 16];
            let mut colors = [0.0_f32; 48];
            for (index, stop) in sky.stops.iter().take(16).enumerate() {
                positions[index] = stop.position;
                colors[index * 3..index * 3 + 3].copy_from_slice(&stop.color);
            }
            gl::Uniform1i(self.sky_stop_count_location, sky.stops.len().min(16) as i32);
            gl::Uniform1fv(self.sky_positions_location, 16, positions.as_ptr());
            gl::Uniform3fv(self.sky_colors_location, 16, colors.as_ptr());
            gl::Uniform1i(
                self.sky_smooth_location,
                i32::from(
                    sky.interpolation
                        == crate::project::sky_material::SkyInterpolation::SmoothLinear,
                ),
            );
            let overlay = world
                .sky
                .material_asset
                .as_deref()
                .and_then(|id| renderer.material_overlay_texture(id));
            gl::Uniform1i(
                get_uniform_location(self.program, "u_stars_layer_enabled"),
                i32::from(overlay.is_some()),
            );
            gl::Uniform1i(
                get_uniform_location(self.program, "u_stars_layer_texture"),
                14,
            );
            if let Some(texture) = overlay {
                gl::ActiveTexture(gl::TEXTURE14);
                gl::BindTexture(gl::TEXTURE_2D, texture);
                gl::ActiveTexture(gl::TEXTURE0);
            }
            gl::Uniform2f(
                self.local_water_camera_xz_location,
                camera_pos.x,
                camera_pos.z,
            );
            let cache = renderer.terrain_cache.borrow();
            let mask = cache.as_ref().filter(|c| c.ocean_contact_texture != 0);
            let mapping = mask.map_or([0.0; 4], |c| c.ocean_contact_mapping);
            gl::Uniform4f(
                get_uniform_location(self.program, "u_ocean_contact_mapping"),
                mapping[0],
                mapping[1],
                mapping[2],
                mapping[3],
            );
            gl::Uniform1i(
                get_uniform_location(self.program, "u_ocean_contact_atlas"),
                13,
            );
            gl::ActiveTexture(gl::TEXTURE13);
            gl::BindTexture(gl::TEXTURE_2D, mask.map_or(0, |c| c.ocean_contact_texture));
            gl::ActiveTexture(gl::TEXTURE0);
            let waves = world.terrain.config.resolved_water(&catalog);
            gl::Uniform4f(
                get_uniform_location(self.program, "u_ocean_wave"),
                waves.wave_height.max(0.0),
                waves.wave_speed,
                waves.wave_length,
                renderer.water_started.elapsed().as_secs_f32(),
            );
            gl::Uniform1f(
                get_uniform_location(self.program, "u_ocean_direction"),
                waves.wave_direction_degrees.to_radians(),
            );
            gl::Uniform1f(
                get_uniform_location(self.program, "u_ocean_seed"),
                world.terrain.config.plains.seed.rem_euclid(65536) as f32,
            );

            let underwater_enabled = underwater.is_some_and(|state| state.enabled);
            gl::Uniform1i(
                get_uniform_location(self.program, "u_ocean_optics_mode"),
                world.terrain.config.ocean_optics_mode.shader_id(),
            );
            gl::Uniform1i(
                self.underwater_enabled_location,
                i32::from(underwater_enabled),
            );
            if let Some(state) = underwater {
                gl::Uniform3f(
                    self.underwater_color_location,
                    state.color.x,
                    state.color.y,
                    state.color.z,
                );
                gl::Uniform1f(self.underwater_blend_location, state.sky_tint);
                gl::Uniform1f(self.underwater_camera_y_location, camera_pos.y);
                gl::Uniform1f(self.underwater_surface_y_location, state.surface_y);
            } else {
                gl::Uniform3f(self.underwater_color_location, 0.0, 0.0, 0.0);
                gl::Uniform1f(self.underwater_blend_location, 0.0);
                gl::Uniform1f(self.underwater_camera_y_location, camera_pos.y);
                gl::Uniform1f(self.underwater_surface_y_location, 0.0);
            }

            let atmosphere_enabled = world.sky.atmosphere_enabled;
            let fog_color = world.sky.fog_color.clamp(Vec3::ZERO, Vec3::ONE);
            gl::Uniform1i(
                self.atmosphere_enabled_location,
                i32::from(atmosphere_enabled),
            );
            gl::Uniform3f(
                self.fog_color_location,
                fog_color.x,
                fog_color.y,
                fog_color.z,
            );
            gl::Uniform1f(
                self.horizon_strength_location,
                world.sky.horizon_strength.clamp(0.0, 1.0),
            );

            gl::DrawArrays(gl::TRIANGLES, 0, 36);

            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::CULL_FACE);
        }
    }
}

fn add_face(vertices: &mut Vec<f32>, v1: [f32; 3], v2: [f32; 3], v3: [f32; 3], v4: [f32; 3]) {
    let normal = [0.0, 0.0, 0.0];
    let color = [1.0, 1.0, 1.0, 1.0];
    let dummy_uv = [0.0, 0.0];

    vertices.extend_from_slice(&v1);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v2);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v3);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);

    vertices.extend_from_slice(&v1);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v3);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v4);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
}

const SKY_VERTEX_SHADER: &str = r#"
#version 330 core
layout (location = 0) in vec3 a_position;

uniform mat4 u_view_projection;
uniform mat4 u_model;

out vec3 v_pos;

void main() {
    v_pos = a_position;
    gl_Position = u_view_projection * u_model * vec4(a_position, 1.0);
}
"#;

pub(super) const SKY_FRAGMENT_SHADER: &str = r#"
#version 330 core
in vec3 v_pos;
uniform int u_sky_stop_count;
uniform float u_sky_positions[16];
uniform vec3 u_sky_colors[16];
uniform bool u_sky_smooth;
uniform bool u_stars_layer_enabled;
uniform sampler2D u_stars_layer_texture;
vec3 wk_sky_gradient(float height) {
    float h = clamp(height, 0.0, 1.0);
    vec3 result = u_sky_colors[0];
    for (int i = 1; i < 16; ++i) {
        if (i >= u_sky_stop_count) break;
        if (h <= u_sky_positions[i]) {
            float span = max(u_sky_positions[i] - u_sky_positions[i - 1], 0.00001);
            float t = clamp((h - u_sky_positions[i - 1]) / span, 0.0, 1.0);
            if (u_sky_smooth) t = t * t * (3.0 - 2.0 * t);
            return mix(u_sky_colors[i - 1], u_sky_colors[i], t);
        }
        result = u_sky_colors[i];
    }
    return result;
}
uniform bool u_underwater_enabled;
uniform int u_ocean_optics_mode; // 0 Off, 1 Height, 2 Ray
uniform vec3 u_underwater_color;
uniform float u_underwater_blend;
uniform float u_underwater_camera_y;
uniform vec2 u_local_water_camera_xz;
uniform float u_underwater_surface_y;
uniform sampler2D u_ocean_contact_atlas;
uniform vec4 u_ocean_contact_mapping;
uniform vec4 u_ocean_wave;
uniform float u_ocean_direction;
uniform float u_ocean_seed;
uniform bool u_atmosphere_enabled;
uniform vec3 u_fog_color;
uniform float u_horizon_strength;
out vec4 FragColor;


// WK-WATER-013 | 2026-10-12 — camera-ray length inside local Lake/River
// volume. GPU atlas contains XZ spline signed distance + top/bottom planes.
#define WK_LOCAL_WATER_BODIES 8
uniform sampler2DArray u_local_water_atlas;
uniform int u_local_water_count;
uniform vec4 u_local_water_rect[WK_LOCAL_WATER_BODIES];
uniform vec4 u_local_water_fog[WK_LOCAL_WATER_BODIES];
uniform vec4 u_local_water_params[WK_LOCAL_WATER_BODIES];
uniform vec4 u_local_water_waves[WK_LOCAL_WATER_BODIES];
uniform float u_local_water_time;
// Approximate the displaced crest at the pixel's water-column position,
// using the SAME dominant components, phase and wave material as the
// actual rendered water mesh. Compute only near the surface.
float wk_local_wave_crest(vec2 p,int i,float depth) {
    vec4 wave=u_local_water_waves[i];
    float c=cos(wave.w),s=sin(wave.w);
    vec2 uv=vec2(c*p.x-s*p.y,s*p.x+c*p.y)*
        (50.0/max(wave.z,1.0));
    float t=u_local_water_time*wave.y;
    float w=sin(dot(uv,vec2(0.115,0.072))+t*0.90)*0.18
          +sin(dot(uv,vec2(-0.083,0.145))+t*1.27+1.7)*0.09
          +sin(dot(uv,vec2(0.215,-0.047))+t*0.54+3.1)*0.045;
    float normalized_depth=clamp(depth/20.0,0.0,1.0);
    float shore=mix(0.08,1.0,smoothstep(0.03,0.48,normalized_depth));
    return w*wave.x*shore;
}


vec2 wk_volume_segment(vec3 ray_a,vec3 ray_b,int i){
    vec4 rect=u_local_water_rect[i];
    vec2 lo=rect.xy;
    vec2 hi=rect.xy+1.0/rect.zw;
    vec2 origin_xz=ray_a.xz;
    vec2 delta=ray_b.xz-origin_xz;
    float enter=0.0,exit=1.0;
    for(int axis=0;axis<2;++axis){
        if(abs(delta[axis])<0.00001){
            if(origin_xz[axis]<lo[axis] || origin_xz[axis]>hi[axis]){
                return vec2(0.0,0.0);
            }
        }else{
            float a=(lo[axis]-origin_xz[axis])/delta[axis];
            float b=(hi[axis]-origin_xz[axis])/delta[axis];
            enter=max(enter,min(a,b));
            exit=min(exit,max(a,b));
        }
    }
    if(exit<=enter){return vec2(0.0,0.0);}
    float ray_length=length(ray_b-ray_a);
    if(ray_length<0.0001){return vec2(0.0,0.0);}
    float wet_length=0.0;
    float maximum_depth=0.0;
    // Constrained to the body's own footprint; only ray intersections with
    // the spline footprint AND water top/floor count. Fixed bounded budget.
    for(int step=0;step<32;++step){
        float t=mix(enter,exit,(float(step)+0.5)/32.0);
        vec3 p=mix(ray_a,ray_b,t);
        vec2 uv=(p.xz-rect.xy)*rect.zw;
        vec4 column=texture(u_local_water_atlas,vec3(uv,float(i)));
        if(column.a<=0.02 || column.b<=0.0){continue;}
        float edge=clamp(column.b*1.5,0.0,1.0)*clamp(column.a*3.0,0.0,1.0);
        float surface=column.r;
        if(abs(p.y-surface)<1.6 && u_local_water_waves[i].x>0.0){
            surface+=wk_local_wave_crest(p.xz,i,surface-column.g);
        }
        if(p.y>=column.g && p.y<surface){
            wet_length+=ray_length*(exit-enter)/32.0*edge;
            maximum_depth=max(maximum_depth,surface-p.y);
        }
    }
    return vec2(wet_length,maximum_depth);
}
vec4 wk_apply_local_water(vec3 original,vec3 camera,vec3 target,bool is_sky){
    vec3 final_color=original;
    float strongest=0.0;
    float any_volume=0.0;
    for(int i=0;i<WK_LOCAL_WATER_BODIES;++i){
        if(i>=u_local_water_count)break;
        vec4 fog=u_local_water_fog[i];
        vec4 params=u_local_water_params[i];
        vec2 wet=wk_volume_segment(camera,target,i);
        if(wet.x<=0.0)continue;
        any_volume=1.0;
        float fog_amount=1.0-exp(-max(fog.a,0.0)*wet.x);
        float deep=clamp(wet.y*max(params.x,0.0),0.0,params.y);
        float blend=clamp(fog_amount+deep,0.0,params.z);
        if(is_sky){blend=max(blend,params.w*clamp(wet.x/12.0,0.0,1.0));}
        // Deterministic priority for overlapping bodies: strongest optical
        // contribution wins, independent of iteration order.
        if(blend>strongest){
            strongest=blend;
            final_color=mix(original,fog.rgb,blend);
        }
    }
    return vec4(final_color,any_volume);
}

float wk_sky_seed_hash(float x) {
    return fract(sin(x*17.197+13.123)*43758.5453);
}
float wk_sky_ocean_three_bands(vec2 p,float t) {
    float k=6.28318530718/max(u_ocean_wave.z,1.0);
    float seed=wk_sky_seed_hash(u_ocean_seed+0.123);
    float a=-dot(p,vec2(cos(u_ocean_direction),
                          sin(u_ocean_direction)))*k+
            t*u_ocean_wave.y;
    float angle2=u_ocean_direction+0.43+seed*0.47;
    float angle3=u_ocean_direction-0.91+
        wk_sky_seed_hash(u_ocean_seed+7.0)*0.39;
    float b=-dot(p,vec2(cos(angle2),sin(angle2)))*k*1.71+
        t*u_ocean_wave.y*1.18+seed*6.28318530718;
    float c=-dot(p,vec2(cos(angle3),sin(angle3)))*k*0.58+
        t*u_ocean_wave.y*0.74+
        wk_sky_seed_hash(u_ocean_seed+5.0)*6.28318530718;
    return sin(a)*0.72+sin(b)*0.19+sin(c)*0.09;
}

void main() {
    vec3 direction_sky = normalize(v_pos);
    vec4 sky = vec4(wk_sky_gradient(max(direction_sky.y, 0.0)), 1.0);
    if (u_atmosphere_enabled) {
        vec3 direction = normalize(v_pos);
        float horizon = 1.0 - clamp(abs(direction.y), 0.0, 1.0);
        horizon = horizon * horizon;
        float atmosphere = clamp(horizon * u_horizon_strength, 0.0, 1.0);
        sky.rgb = mix(sky.rgb, u_fog_color, atmosphere);
    }
    if (u_stars_layer_enabled) {
        vec3 dir=normalize(v_pos);
        vec2 uv=vec2(atan(dir.z,dir.x)/6.28318530718+0.5,
            asin(clamp(dir.y,-1.0,1.0))/3.14159265359+0.5);
        vec4 overlay=texture(u_stars_layer_texture,uv);
        sky.rgb=mix(sky.rgb,overlay.rgb,overlay.a);
    }
    vec3 local_water_air_color=sky.rgb;
    // Height mode uses a single dynamic surface-height/shoreline test for
    // each sky direction, rather than 16 ocean ray-march samples.
    if (u_ocean_optics_mode==1 && u_underwater_enabled &&
        u_ocean_contact_mapping.z>0.0) {
        vec3 cam=vec3(u_local_water_camera_xz.x,
            u_underwater_camera_y,u_local_water_camera_xz.y);
        vec3 dir=normalize(v_pos);
        const float sky_segment=72.0;
        vec3 target=cam+dir*sky_segment;
        float crossing=abs(target.y-cam.y)<0.00001?0.5:
            clamp((u_underwater_surface_y-cam.y)/(target.y-cam.y),0.0,1.0);
        vec3 point=mix(cam,target,crossing);
        vec2 uv=(point.xz-u_ocean_contact_mapping.xy)*
            u_ocean_contact_mapping.zw;
        if (all(greaterThanEqual(uv,vec2(0.0))) &&
            all(lessThanEqual(uv,vec2(1.0))) &&
            texture(u_ocean_contact_atlas,uv).r>0.5) {
            float surface=u_underwater_surface_y+
                wk_sky_ocean_three_bands(point.xz,u_ocean_wave.w)*
                max(u_ocean_wave.x,0.0);
            crossing=abs(target.y-cam.y)<0.00001?0.5:
                clamp((surface-cam.y)/(target.y-cam.y),0.0,1.0);
            float submerged_fraction=abs(target.y-cam.y)<0.00001?
                (cam.y<surface?1.0:0.0):
                (cam.y<surface?crossing:1.0-crossing);
            vec3 scatter=mix(u_underwater_color,sky.rgb,0.02);
            sky.rgb=mix(sky.rgb,scatter,
                clamp(submerged_fraction*u_underwater_blend,0.0,1.0));
        }
    }
    // WK-WAVES-005: pixel-specific sky rays, never camera-wide water tint.
    if (u_ocean_optics_mode==2 && u_underwater_enabled &&
        u_ocean_contact_mapping.z > 0.0) {
        vec3 camera=vec3(u_local_water_camera_xz.x,
                         u_underwater_camera_y,u_local_water_camera_xz.y);
        vec3 direction=normalize(v_pos);
        float immersed=0.0;
        const float sky_segment=72.0;
        for(int i=0;i<16;++i){
            vec3 point=camera+direction*((float(i)+0.5)/16.0)*sky_segment;
            vec2 uv=(point.xz-u_ocean_contact_mapping.xy)*
                u_ocean_contact_mapping.zw;
            if(all(greaterThanEqual(uv,vec2(0.0))) &&
               all(lessThanEqual(uv,vec2(1.0))) &&
               texture(u_ocean_contact_atlas,uv).r>0.5){
                float height=u_underwater_surface_y+
                    wk_sky_ocean_three_bands(point.xz,u_ocean_wave.w)*
                    max(u_ocean_wave.x,0.0);
                immersed+=smoothstep(-0.025,0.025,height-point.y)/16.0;
            }
        }
        vec3 underwater_sky_scatter=mix(
            u_underwater_color,sky.rgb,0.02);
        sky.rgb=mix(sky.rgb,underwater_sky_scatter,
            clamp(immersed*u_underwater_blend,0.0,1.0));
    }
    // Sky rays use the same bounded local water-volume atlas. Rays that
    // do not intersect water preserve their original atmosphere/gradient.
    vec3 direction=normalize(v_pos);
    vec3 camera=vec3(0.0,u_underwater_camera_y,0.0);
    // Local bodies use actual world XZ, unlike the legacy ocean-only mask.
    camera.xz=u_local_water_camera_xz;
    vec4 local=wk_apply_local_water(
        local_water_air_color,camera,camera+direction*72.0,true);
    sky.rgb=mix(sky.rgb,local.rgb,local.a);

    FragColor = sky;
}
"#;

#[cfg(test)]
mod underwater_sky_mask_tests {
    #[test]
    fn stars_sky_overlay_is_optional_and_direction_anchored() {
        let shader = super::SKY_FRAGMENT_SHADER;
        assert!(shader.contains("if (u_stars_layer_enabled)"));
        assert!(shader.contains("atan(dir.z,dir.x)"));
        assert!(shader.contains("mix(sky.rgb,overlay.rgb,overlay.a)"));
    }
    #[test]
    fn ocean_sky_tint_requires_camera_below_waterline() {
        let shader = super::SKY_FRAGMENT_SHADER;
        assert!(shader.contains("texture(u_ocean_contact_atlas,uv)"));
        assert!(shader.contains("wk_sky_ocean_three_bands(point.xz,u_ocean_wave.w)"));
        assert!(shader.contains("u_ocean_seed"));
        assert!(!shader.contains("if (cam_y < water_y) {"));
        assert!(shader.contains("wk_apply_local_water("));
    }
}
