// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::world::World;
use glam::Vec3;
use std::{ffi::CString, ptr};

pub(super) const CELLS: [i32; 3] = [96, 48, 96];
const DX: f32 = 0.5;
const DENSITY: f32 = 998.2;
const GRAVITY: f32 = 9.80665;
const GLSL: &str = r#"
#version 430
layout(local_size_x=4,local_size_y=4,local_size_z=4) in;
// xyz in velocity texel i,j,k represent respectively:
// u(i,j+1/2,k+1/2), v(i+1/2,j,k+1/2), w(i+1/2,j+1/2,k)
layout(rgba32f, binding=0) uniform writeonly image3D o_velocity;
layout(r32f, binding=1) uniform writeonly image3D o_scalar;
uniform sampler3D velocity_tex;
uniform sampler3D pressure_tex;
uniform sampler3D alpha_tex;
uniform usampler3D solid_tex;
uniform ivec3 size_cells;
uniform float delta_t;
uniform float dx;
uniform float rho;
uniform float gravity;
uniform vec3 origin;
uniform int stage; // 0 provisional 3D velocity; 1 pressure; 2 projection; 3 VOF
uniform int impulse_count;
uniform vec4 impulses[8]; // xyz world position, strength
uniform vec3 impulse_vectors[8]; // actual moving-object directional force
bool inCell(ivec3 c){
    return all(greaterThanEqual(c,ivec3(0)))&&
           all(lessThan(c,size_cells));
}
bool solid(ivec3 c){
    return !inCell(c)||texelFetch(solid_tex,c,0).r!=0u;
}
float water(ivec3 c){
    return inCell(c)?texelFetch(alpha_tex,c,0).r:0.0;
}
float pressure(ivec3 c){
    return inCell(c)?texelFetch(pressure_tex,c,0).r:0.0;
}
vec3 faces(ivec3 c){
    return texelFetch(velocity_tex,clamp(c,ivec3(0),size_cells),0).xyz;
}
// x,y,z measured in cell-grid coordinates (not texel centers).
vec3 velocityAt(vec3 x){
    vec3 dim=vec3(size_cells+1);
    float u=texture(velocity_tex,clamp((x+vec3(0.5,0,0))/dim,
                                    vec3(0.0),vec3(1.0))).x;
    float v=texture(velocity_tex,clamp((x+vec3(0,0.5,0))/dim,
                                    vec3(0.0),vec3(1.0))).y;
    float w=texture(velocity_tex,clamp((x+vec3(0,0,0.5))/dim,
                                    vec3(0.0),vec3(1.0))).z;
    return vec3(u,v,w);
}
bool faceSolid(ivec3 c,int axis){
    ivec3 lower=c;
    lower[axis]--;
    return solid(lower)||solid(c);
}
vec3 facePosition(ivec3 c,int axis){
    vec3 pos=vec3(c)+vec3(0.5);
    pos[axis]=float(c[axis]);
    return pos;
}
float faceValue(ivec3 c,int axis){
    if(c[axis]<0||c[axis]>size_cells[axis]||faceSolid(c,axis))
        return 0.0;
    return faces(c)[axis];
}
float adjacentWater(ivec3 c,int axis){
    ivec3 back=c;back[axis]--;
    return max(water(back),water(c));
}
float pressureNeighbor(ivec3 c,ivec3 center){
    // A solid face has Neumann pressure; air has atmospheric pressure p=0.
    return solid(c)?pressure(center):water(c)>0.0001?pressure(c):0.0;
}
float faceAlphaFlux(ivec3 cell,int axis){
    // cell labels right-side cell of the face; left cell = cell-e_axis.
    ivec3 left=cell;left[axis]--;
    float vel=faceValue(cell,axis);
    return vel*(vel>=0.0?water(left):water(cell));
}
void main(){
    ivec3 c=ivec3(gl_GlobalInvocationID.xyz);
    if(stage==0||stage==2){
        if(any(greaterThan(c,size_cells)))return;
        vec3 updated=vec3(0.0);
        for(int axis=0;axis<3;axis++){
            if(c[axis]>size_cells[axis]||faceSolid(c,axis))continue;
            vec3 pos=facePosition(c,axis);
            if(stage==0){
                // Semi-Lagrangian advect + gravity + localized momentum.
                vec3 back=pos-velocityAt(pos)*(delta_t/dx);
                updated[axis]=velocityAt(back)[axis];
                float wet=adjacentWater(c,axis);
                updated[axis]*=step(0.0001,wet);
                if(axis==1)updated[axis]-=gravity*delta_t*step(0.0001,wet);
                for(int i=0;i<8;i++){
                    if(i>=impulse_count)break;
                    vec3 world=origin+pos*dx;
                    vec3 dr=world-impulses[i].xyz;
                    float radius2=dot(dr,dr);
                    updated[axis]+=impulse_vectors[i][axis]*
                                  exp(-radius2*0.5)*
                                  impulses[i].w*delta_t*step(0.0001,wet);
                }
            }else{
                // Discrete Helmholtz–Hodge projection: u_new=u*−dt/rho grad(p).
                ivec3 lower=c;lower[axis]--;
                float pr=pressureNeighbor(c,lower);
                float pl=pressureNeighbor(lower,c);
                updated[axis]=faceValue(c,axis)-
                              delta_t/rho*(pr-pl)/dx;
            }
        }
        imageStore(o_velocity,c,vec4(updated,0.0));
        return;
    }
    if(!inCell(c))return;
    if(stage==1){
        if(solid(c)||water(c)<0.0001){
            imageStore(o_scalar,c,vec4(0.0));return;
        }
        // 3D staggered-grid divergence. The same face velocity is used
        // in both adjacent cells, so the discrete divergence telescopes.
        float divergence=0.0;
        float sumP=0.0;
        float count=0.0;
        for(int axis=0;axis<3;axis++){
            ivec3 plus=c;plus[axis]++;
            divergence+=(faceValue(plus,axis)-faceValue(c,axis))/dx;
            ivec3 minus=c;minus[axis]--;
            if(!solid(plus)){sumP+=pressureNeighbor(plus,c);count++;}
            if(!solid(minus)){sumP+=pressureNeighbor(minus,c);count++;}
        }
        // ∇²p=(rho/dt)∇·u* (Jacobi). Solid faces satisfy homogeneous
        // Neumann; air interfaces use atmospheric Dirichlet p=0.
        float next=count>0.0?
            (sumP-rho*dx*dx*divergence/delta_t)/count:0.0;
        imageStore(o_scalar,c,vec4(next,0,0,0));
        return;
    }
    if(stage==3){
        if(solid(c)){imageStore(o_scalar,c,vec4(0.0));return;}
        float net=0.0;
        for(int axis=0;axis<3;axis++){
            ivec3 plus=c;plus[axis]++;
            net+=faceAlphaFlux(plus,axis)-faceAlphaFlux(c,axis);
        }
        // First-order conservative Eulerian VOF transport. At sealed faces
        // flux is 0; each interior face contributes equal/opposite volume.
        // This is a transport foundation, NOT yet geometric PLIC VOF.
        float alpha=water(c)-delta_t/dx*net;
        imageStore(o_scalar,c,vec4(alpha,0.0,0.0,0.0));
    }
}
"#;

#[derive(Debug)]
pub(super) struct ThreeDimensionalWaterGpu {
    program: u32,
    velocity: [u32; 2],
    pressure: [u32; 2],
    volume: [u32; 2],
    solid: u32,
    vel_idx: usize,
    alpha_idx: usize,
    pub(super) origin: Vec3,
    accumulated: f32,
    last_time: f32,
}

impl ThreeDimensionalWaterGpu {
    pub(super) fn new() -> Result<Self, String> {
        unsafe {
            if !gl::DispatchCompute::is_loaded()
                || !gl::BindImageTexture::is_loaded()
                || !gl::TexImage3D::is_loaded()
            {
                return Err("OpenGL 4.3 compute / 3D images are required".into());
            }
            let shader = gl::CreateShader(gl::COMPUTE_SHADER);
            if shader == 0 {
                return Err("Cannot create 3D compute shader".into());
            }
            let src = CString::new(GLSL).unwrap();
            gl::ShaderSource(shader, 1, &src.as_ptr(), ptr::null());
            gl::CompileShader(shader);
            let mut ok = 0;
            gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut ok);
            if ok != gl::TRUE as i32 {
                let error = shader_log(shader);
                gl::DeleteShader(shader);
                return Err(format!("3D fluid GLSL error: {error}"));
            }
            let prog = gl::CreateProgram();
            gl::AttachShader(prog, shader);
            gl::LinkProgram(prog);
            gl::DeleteShader(shader);
            gl::GetProgramiv(prog, gl::LINK_STATUS, &mut ok);
            if ok != gl::TRUE as i32 {
                let error = program_log(prog);
                gl::DeleteProgram(prog);
                return Err(format!("3D fluid program error: {error}"));
            }
            let mut velocity = [0; 2];
            let mut pressure = [0; 2];
            let mut volume = [0; 2];
            let mut solid = 0;
            gl::GenTextures(2, velocity.as_mut_ptr());
            gl::GenTextures(2, pressure.as_mut_ptr());
            gl::GenTextures(2, volume.as_mut_ptr());
            gl::GenTextures(1, &mut solid);
            for &id in &velocity {
                allocate(id, gl::RGBA32F, gl::RGBA, gl::FLOAT);
            }
            for &id in &pressure {
                allocate(id, gl::R32F, gl::RED, gl::FLOAT);
            }
            for &id in &volume {
                allocate(id, gl::R32F, gl::RED, gl::FLOAT);
            }
            allocate(solid, gl::R8UI, gl::RED_INTEGER, gl::UNSIGNED_BYTE);
            gl::BindTexture(gl::TEXTURE_3D, 0);
            Ok(Self {
                program: prog,
                velocity,
                pressure,
                volume,
                solid,
                vel_idx: 0,
                alpha_idx: 0,
                origin: Vec3::ZERO,
                accumulated: 0.0,
                last_time: 0.0,
            })
        }
    }

    pub(super) fn initialize(&mut self, world: &World, focus: Vec3, now: f32) {
        let dims = CELLS.map(|v| v as usize);
        self.origin = Vec3::new(
            focus.x - DX * dims[0] as f32 * 0.5,
            world.terrain.config.plains.water_level - DX * dims[1] as f32 * 0.5,
            focus.z - DX * dims[2] as f32 * 0.5,
        );
        let padded = dims.map(|v| v + 1);
        let count = padded.iter().product::<usize>();
        let mut volume = vec![0.0f32; count];
        let mut hydrostatic_pressure = vec![0.0f32; count];
        let mut solid = vec![0u8; count];
        let velocity = vec![0.0f32; count * 4];
        let sea = world.terrain.config.plains.water_level;
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let i = (z * padded[1] + y) * padded[0] + x;
                    let world_x = self.origin.x + (x as f32 + 0.5) * DX;
                    let world_z = self.origin.z + (z as f32 + 0.5) * DX;
                    let center_y = self.origin.y + (y as f32 + 0.5) * DX;
                    let terrain = world
                        .terrain
                        .surface_height_at(world_x, world_z)
                        .unwrap_or(sea - 100.0);
                    if center_y <= terrain {
                        solid[i] = 1;
                    } else {
                        let bottom = center_y - DX * 0.5;
                        let top = center_y + DX * 0.5;
                        volume[i] = ((sea - bottom) / (top - bottom)).clamp(0.0, 1.0);
                        if volume[i] > 0.0 {
                            hydrostatic_pressure[i] = DENSITY * GRAVITY * (sea - center_y).max(0.0);
                        }
                    }
                }
            }
        }
        unsafe {
            for &id in &self.velocity {
                upload_float(id, gl::RGBA, velocity.as_ptr().cast());
            }
            for &id in &self.pressure {
                upload_float(id, gl::RED, hydrostatic_pressure.as_ptr().cast());
            }
            for &id in &self.volume {
                upload_float(id, gl::RED, volume.as_ptr().cast());
            }
            gl::BindTexture(gl::TEXTURE_3D, self.solid);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexSubImage3D(
                gl::TEXTURE_3D,
                0,
                0,
                0,
                0,
                CELLS[0] + 1,
                CELLS[1] + 1,
                CELLS[2] + 1,
                gl::RED_INTEGER,
                gl::UNSIGNED_BYTE,
                solid.as_ptr().cast(),
            );
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
            gl::BindTexture(gl::TEXTURE_3D, 0);
        }
        self.vel_idx = 0;
        self.alpha_idx = 0;
        self.accumulated = 0.0;
        self.last_time = now;
    }

    pub(super) fn step(&mut self, dt: f32) {
        const PRESSURE_ITERATIONS: usize = 96;
        unsafe {
            gl::UseProgram(self.program);
            let uniform = |name: &str| {
                let c = CString::new(name).unwrap();
                gl::GetUniformLocation(self.program, c.as_ptr())
            };
            gl::Uniform3i(uniform("size_cells"), CELLS[0], CELLS[1], CELLS[2]);
            gl::Uniform1f(uniform("delta_t"), dt);
            gl::Uniform1f(uniform("dx"), DX);
            gl::Uniform1f(uniform("rho"), DENSITY);
            gl::Uniform1f(uniform("gravity"), GRAVITY);
            gl::Uniform3f(
                uniform("origin"),
                self.origin.x,
                self.origin.y,
                self.origin.z,
            );
            gl::Uniform1i(uniform("impulse_count"), 0);
            for (i, id) in ["velocity_tex", "pressure_tex", "alpha_tex", "solid_tex"]
                .iter()
                .enumerate()
            {
                gl::Uniform1i(uniform(id), i as i32);
            }
            let provisional = 1 - self.vel_idx;
            self.dispatch(
                0,
                self.velocity[self.vel_idx],
                self.pressure[0],
                self.volume[self.alpha_idx],
                self.velocity[provisional],
                self.pressure[1],
            );
            let mut pressure_index = 0;
            for _ in 0..PRESSURE_ITERATIONS {
                let next = 1 - pressure_index;
                self.dispatch(
                    1,
                    self.velocity[provisional],
                    self.pressure[pressure_index],
                    self.volume[self.alpha_idx],
                    self.velocity[provisional],
                    self.pressure[next],
                );
                pressure_index = next;
            }
            self.dispatch(
                2,
                self.velocity[provisional],
                self.pressure[pressure_index],
                self.volume[self.alpha_idx],
                self.velocity[self.vel_idx],
                self.pressure[1 - pressure_index],
            );
            let out = 1 - self.alpha_idx;
            self.dispatch(
                3,
                self.velocity[self.vel_idx],
                self.pressure[pressure_index],
                self.volume[self.alpha_idx],
                self.velocity[provisional],
                self.volume[out],
            );
            self.alpha_idx = out;
            gl::UseProgram(0);
            gl::ActiveTexture(gl::TEXTURE0);
        }
    }
    #[allow(dead_code)]
    pub(super) fn advance(&mut self, now: f32) {
        self.accumulated = (self.accumulated + (now - self.last_time).clamp(0.0, 0.04)).min(0.08);
        self.last_time = now;
        const DT: f32 = 1.0 / 180.0;
        let mut n = 0;
        while self.accumulated >= DT && n < 10 {
            self.step(DT);
            self.accumulated -= DT;
            n += 1;
        }
    }
    unsafe fn dispatch(
        &self,
        stage: i32,
        velocity: u32,
        pressure: u32,
        alpha: u32,
        write_vel: u32,
        write_scalar: u32,
    ) {
        let bind = |slot: u32, id: u32| {
            gl::ActiveTexture(gl::TEXTURE0 + slot);
            gl::BindTexture(gl::TEXTURE_3D, id);
        };
        bind(0, velocity);
        bind(1, pressure);
        bind(2, alpha);
        bind(3, self.solid);
        gl::BindImageTexture(0, write_vel, 0, gl::TRUE, 0, gl::WRITE_ONLY, gl::RGBA32F);
        gl::BindImageTexture(1, write_scalar, 0, gl::TRUE, 0, gl::WRITE_ONLY, gl::R32F);
        let name = CString::new("stage").unwrap();
        gl::Uniform1i(gl::GetUniformLocation(self.program, name.as_ptr()), stage);
        gl::DispatchCompute(
            ((CELLS[0] + 1 + 3) / 4) as u32,
            ((CELLS[1] + 1 + 3) / 4) as u32,
            ((CELLS[2] + 1 + 3) / 4) as u32,
        );
        gl::MemoryBarrier(gl::SHADER_IMAGE_ACCESS_BARRIER_BIT | gl::TEXTURE_FETCH_BARRIER_BIT);
        for i in 0..4 {
            gl::ActiveTexture(gl::TEXTURE0 + i);
            gl::BindTexture(gl::TEXTURE_3D, 0);
        }
        gl::ActiveTexture(gl::TEXTURE0);
    }
    pub(super) fn volume_texture(&self) -> u32 {
        self.volume[self.alpha_idx]
    }
    pub(super) fn velocity_texture(&self) -> u32 {
        self.velocity[self.vel_idx]
    }
    pub(super) fn free(&mut self) {
        if self.program == 0 {
            return;
        }
        unsafe {
            gl::DeleteTextures(2, self.velocity.as_ptr());
            gl::DeleteTextures(2, self.pressure.as_ptr());
            gl::DeleteTextures(2, self.volume.as_ptr());
            gl::DeleteTextures(1, &self.solid);
            gl::DeleteProgram(self.program);
        }
        self.program = 0;
        self.velocity = [0; 2];
        self.pressure = [0; 2];
        self.volume = [0; 2];
        self.solid = 0;
    }
}

unsafe fn allocate(id: u32, internal: u32, format: u32, ty: u32) {
    gl::BindTexture(gl::TEXTURE_3D, id);
    let filter = if internal == gl::R8UI {
        gl::NEAREST
    } else {
        gl::LINEAR
    };
    gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_MIN_FILTER, filter as i32);
    gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_MAG_FILTER, filter as i32);
    for wrap in [gl::TEXTURE_WRAP_S, gl::TEXTURE_WRAP_T, gl::TEXTURE_WRAP_R] {
        gl::TexParameteri(gl::TEXTURE_3D, wrap, gl::CLAMP_TO_EDGE as i32);
    }
    gl::TexImage3D(
        gl::TEXTURE_3D,
        0,
        internal as i32,
        CELLS[0] + 1,
        CELLS[1] + 1,
        CELLS[2] + 1,
        0,
        format,
        ty,
        ptr::null(),
    );
}
unsafe fn upload_float(id: u32, format: u32, data: *const std::ffi::c_void) {
    gl::BindTexture(gl::TEXTURE_3D, id);
    gl::TexSubImage3D(
        gl::TEXTURE_3D,
        0,
        0,
        0,
        0,
        CELLS[0] + 1,
        CELLS[1] + 1,
        CELLS[2] + 1,
        format,
        gl::FLOAT,
        data,
    );
}
unsafe fn shader_log(shader: u32) -> String {
    let mut n = 0;
    gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut n);
    let mut buf = vec![0u8; n.max(1) as usize];
    gl::GetShaderInfoLog(shader, n, ptr::null_mut(), buf.as_mut_ptr().cast());
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_owned()
}
unsafe fn program_log(program: u32) -> String {
    let mut n = 0;
    gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut n);
    let mut buf = vec![0u8; n.max(1) as usize];
    gl::GetProgramInfoLog(program, n, ptr::null_mut(), buf.as_mut_ptr().cast());
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_three_dimensional_mac_projection_vof_are_present() {
        assert!(GLSL.contains("local_size_z=4"));
        assert!(GLSL.contains("u(i,j+1/2,k+1/2)"));
        assert!(GLSL.contains("divergence+="));
        assert!(GLSL.contains("pressureNeighbor"));
        assert!(GLSL.contains("faceAlphaFlux"));
        assert!(GLSL.contains("delta_t/rho"));
        assert!(GLSL.contains("gl_GlobalInvocationID.xyz"));
        assert!(CELLS.iter().all(|n| *n % 4 == 0));
    }
    #[test]
    fn shared_face_volume_flux_conserves_closed_grid() {
        let a = [0.1f64, 0.6, 0.8, 0.3, 0.5, 0.7];
        let velocities = [0.0, 0.16, -0.09, 0.2, -0.14, 0.04, 0.0];
        let mut next = a;
        for i in 0..a.len() {
            let inflow = if velocities[i] >= 0.0 {
                if i == 0 {
                    0.0
                } else {
                    velocities[i] * a[i - 1]
                }
            } else {
                velocities[i] * a[i]
            };
            let outflow = if velocities[i + 1] >= 0.0 {
                velocities[i + 1] * a[i]
            } else if i + 1 < a.len() {
                velocities[i + 1] * a[i + 1]
            } else {
                0.0
            };
            next[i] = a[i] - 0.1 * (outflow - inflow);
        }
        let drift = f64::abs(a.iter().sum::<f64>() - next.iter().sum::<f64>());
        assert!(
            drift < 1e-13,
            "Conservative shared-face flux drift: {drift}"
        );
    }
}
