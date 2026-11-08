// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::renderer::resources::get_uniform_location;
use crate::world::World;
use crate::world::terrain::{WaterBody, WaterBodyKind};
use glam::Vec3;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub(crate) const BODY_LIMIT: usize = 8;
pub(crate) const ATLAS_DIM: usize = 128;
pub(crate) const VOLUME_RAY_LIMIT: f32 = 72.0;
pub(crate) const ATLAS_TEXTURE_UNIT: u32 = 13;

#[derive(Clone, Copy, Debug, Default)]
struct VolumeDescriptor {
    id: u64,
    rect: [f32; 4],
    fog: [f32; 4],
    settings: [f32; 4],
    waves: [f32; 4],
}
#[derive(Clone, Copy, Debug)]
struct WaterVolumeUniforms {
    sampler: i32,
    count: i32,
    rect: [i32; BODY_LIMIT],
    fog: [i32; BODY_LIMIT],
    settings: [i32; BODY_LIMIT],
    waves: [i32; BODY_LIMIT],
    time: i32,
}
impl WaterVolumeUniforms {
    fn new(program: u32) -> Self {
        Self {
            sampler: get_uniform_location(program, "u_local_water_atlas"),
            count: get_uniform_location(program, "u_local_water_count"),
            rect: std::array::from_fn(|i| {
                get_uniform_location(program, &format!("u_local_water_rect[{i}]"))
            }),
            fog: std::array::from_fn(|i| {
                get_uniform_location(program, &format!("u_local_water_fog[{i}]"))
            }),
            settings: std::array::from_fn(|i| {
                get_uniform_location(program, &format!("u_local_water_params[{i}]"))
            }),
            waves: std::array::from_fn(|i| {
                get_uniform_location(program, &format!("u_local_water_waves[{i}]"))
            }),
            time: get_uniform_location(program, "u_local_water_time"),
        }
    }
}
pub(crate) struct WaterVolumeGpu {
    tex: u32,
    geometry_key: u64,
    ready: usize,
    time: f32,
    descriptors: Vec<VolumeDescriptor>,
    grid: Option<WaterVolumeUniforms>,
    sky: Option<WaterVolumeUniforms>,
}
impl WaterVolumeGpu {
    pub(crate) fn new(grid_program: u32, sky_program: u32) -> Self {
        Self {
            tex: 0,
            geometry_key: 0,
            ready: 0,
            time: 0.0,
            descriptors: Vec::new(),
            grid: (grid_program != 0).then(|| WaterVolumeUniforms::new(grid_program)),
            sky: (sky_program != 0).then(|| WaterVolumeUniforms::new(sky_program)),
        }
    }
    pub(crate) fn no_gl() -> Self {
        Self {
            tex: 0,
            geometry_key: 0,
            ready: 0,
            time: 0.0,
            descriptors: Vec::new(),
            grid: None,
            sky: None,
        }
    }
    pub(crate) fn sync(
        &mut self,
        world: &World,
        camera: Vec3,
        time: f32,
        materials: &crate::project::material::MaterialCatalog,
    ) {
        self.time = time;
        let selected = nearby_bodies(world, camera);
        let key = geometry_key(world, &selected);
        if selected.is_empty() {
            self.descriptors.clear();
            self.ready = 0;
            self.geometry_key = key;
            return;
        }
        if self.tex == 0 {
            unsafe {
                gl::GenTextures(1, &mut self.tex);
                gl::ActiveTexture(gl::TEXTURE0 + ATLAS_TEXTURE_UNIT);
                gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.tex);
                gl::TexParameteri(
                    gl::TEXTURE_2D_ARRAY,
                    gl::TEXTURE_MIN_FILTER,
                    gl::LINEAR as i32,
                );
                gl::TexParameteri(
                    gl::TEXTURE_2D_ARRAY,
                    gl::TEXTURE_MAG_FILTER,
                    gl::LINEAR as i32,
                );
                gl::TexParameteri(
                    gl::TEXTURE_2D_ARRAY,
                    gl::TEXTURE_WRAP_S,
                    gl::CLAMP_TO_EDGE as i32,
                );
                gl::TexParameteri(
                    gl::TEXTURE_2D_ARRAY,
                    gl::TEXTURE_WRAP_T,
                    gl::CLAMP_TO_EDGE as i32,
                );
                gl::TexImage3D(
                    gl::TEXTURE_2D_ARRAY,
                    0,
                    gl::RGBA32F as i32,
                    ATLAS_DIM as i32,
                    ATLAS_DIM as i32,
                    BODY_LIMIT as i32,
                    0,
                    gl::RGBA,
                    gl::FLOAT,
                    std::ptr::null(),
                );
                gl::BindTexture(gl::TEXTURE_2D_ARRAY, 0);
                gl::ActiveTexture(gl::TEXTURE0);
            }
            self.geometry_key = 0;
        }
        if self.geometry_key != key {
            self.geometry_key = key;
            self.ready = 0;
        }
        if self.ready < selected.len() {
            let i = self.ready;
            let pixels = volume_pixels(world, selected[i], ATLAS_DIM);
            unsafe {
                gl::ActiveTexture(gl::TEXTURE0 + ATLAS_TEXTURE_UNIT);
                gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.tex);
                gl::TexSubImage3D(
                    gl::TEXTURE_2D_ARRAY,
                    0,
                    0,
                    0,
                    i as i32,
                    ATLAS_DIM as i32,
                    ATLAS_DIM as i32,
                    1,
                    gl::RGBA,
                    gl::FLOAT,
                    pixels.as_ptr() as *const _,
                );
                gl::BindTexture(gl::TEXTURE_2D_ARRAY, 0);
                gl::ActiveTexture(gl::TEXTURE0);
            }
            self.ready += 1;
        }
        self.descriptors = selected
            .iter()
            .take(self.ready)
            .map(|body| {
                descriptor_with_material(
                    body,
                    &world.terrain.config.resolved_water(materials),
                    materials,
                )
            })
            .collect();
    }
    pub(crate) fn bind_grid(&self) {
        if let Some(locations) = self.grid {
            unsafe {
                self.bind(locations);
            }
        }
    }
    pub(crate) fn bind_sky(&self) {
        if let Some(locations) = self.sky {
            unsafe {
                self.bind(locations);
            }
        }
    }
    unsafe fn bind(&self, u: WaterVolumeUniforms) {
        unsafe {
            gl::Uniform1i(u.count, self.descriptors.len() as i32);
            gl::Uniform1i(u.sampler, ATLAS_TEXTURE_UNIT as i32);
            gl::Uniform1f(u.time, self.time);
            for (i, body) in self.descriptors.iter().enumerate() {
                gl::Uniform4fv(u.rect[i], 1, body.rect.as_ptr());
                gl::Uniform4fv(u.fog[i], 1, body.fog.as_ptr());
                gl::Uniform4fv(u.settings[i], 1, body.settings.as_ptr());
                gl::Uniform4fv(u.waves[i], 1, body.waves.as_ptr());
            }
            gl::ActiveTexture(gl::TEXTURE0 + ATLAS_TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.tex);
            gl::ActiveTexture(gl::TEXTURE0);
        }
    }
    pub(crate) fn free(&mut self) {
        if self.tex != 0 {
            unsafe {
                gl::DeleteTextures(1, &self.tex);
            }
            self.tex = 0;
        }
    }
}
#[cfg(test)]
fn descriptor(
    body: &WaterBody,
    global: &crate::world::terrain::TerrainWaterSettings,
) -> VolumeDescriptor {
    descriptor_settings(body, *body.appearance_or(global))
}
fn descriptor_with_material(
    body: &WaterBody,
    global: &crate::world::terrain::TerrainWaterSettings,
    materials: &crate::project::material::MaterialCatalog,
) -> VolumeDescriptor {
    descriptor_settings(body, body.resolved_appearance(materials, global))
}
fn descriptor_settings(
    body: &WaterBody,
    settings: crate::world::terrain::TerrainWaterSettings,
) -> VolumeDescriptor {
    let (min, max) = body.bounds();
    VolumeDescriptor {
        id: body.id,
        rect: [
            min[0],
            min[1],
            1.0 / (max[0] - min[0]).max(0.01),
            1.0 / (max[1] - min[1]).max(0.01),
        ],
        fog: [
            settings.underwater_fog_color[0],
            settings.underwater_fog_color[1],
            settings.underwater_fog_color[2],
            if settings.underwater_effects_enabled {
                settings.underwater_fog_density
            } else {
                0.0
            },
        ],
        settings: [
            settings.underwater_depth_fog,
            settings.underwater_depth_fog_limit,
            if settings.underwater_effects_enabled {
                settings.underwater_fog_limit
            } else {
                0.0
            },
            if settings.underwater_effects_enabled {
                settings.underwater_sky_tint
            } else {
                0.0
            },
        ],
        waves: [
            settings.wave_height,
            settings.wave_speed,
            settings.wave_length,
            settings.wave_direction_degrees.to_radians(),
        ],
    }
}
fn nearby_bodies(world: &World, camera: Vec3) -> Vec<&WaterBody> {
    let mut result = world
        .terrain
        .config
        .water_bodies
        .iter()
        .filter(|body| body.visible && body.validate().is_ok())
        .filter_map(|body| {
            let (min, max) = body.bounds();
            let dx = (min[0] - camera.x).max(0.0).max(camera.x - max[0]);
            let dz = (min[1] - camera.z).max(0.0).max(camera.z - max[1]);
            let dy = (body.elevation - camera.y).abs();
            let dist = (dx * dx + dz * dz).sqrt();
            (dist <= VOLUME_RAY_LIMIT && dy <= VOLUME_RAY_LIMIT + body.depth)
                .then_some((dist + dy * 0.05, body))
        })
        .collect::<Vec<_>>();
    result.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)));
    result
        .into_iter()
        .take(BODY_LIMIT)
        .map(|(_, body)| body)
        .collect()
}
fn geometry_key(world: &World, selected: &[&WaterBody]) -> u64 {
    let mut h = DefaultHasher::new();
    world.terrain.render_revision().hash(&mut h);
    selected.len().hash(&mut h);
    for b in selected {
        b.id.hash(&mut h);
        b.visible.hash(&mut h);
        (matches!(b.kind, WaterBodyKind::River) as u8).hash(&mut h);
        b.elevation.to_bits().hash(&mut h);
        b.depth.to_bits().hash(&mut h);
        for p in &b.points {
            for value in p.position {
                value.to_bits().hash(&mut h);
            }
            for value in p.tangent {
                value.to_bits().hash(&mut h);
            }
            p.width.to_bits().hash(&mut h);
            p.height_offset.to_bits().hash(&mut h);
            p.channel_depth.map(f32::to_bits).hash(&mut h);
        }
    }
    h.finish()
}
fn volume_pixels(world: &World, body: &WaterBody, dim: usize) -> Vec<f32> {
    let (min, max) = body.bounds();
    let samples = body.spline_samples();
    let width = (max[0] - min[0]).max(0.01);
    let height = (max[1] - min[1]).max(0.01);
    let mut pixels = vec![0.0_f32; dim * dim * 4];
    for z in 0..dim {
        for x in 0..dim {
            let wx = min[0] + (x as f32 + 0.5) / dim as f32 * width;
            let wz = min[1] + (z as f32 + 0.5) / dim as f32 * height;
            let sd = body.signed_coverage_samples(wx, wz, &samples);
            let i = (z * dim + x) * 4;
            pixels[i + 2] = sd;
            if sd <= 0.0 {
                continue;
            }
            let Some(floor) = world.terrain.surface_height_at(wx, wz) else {
                continue;
            };
            let Some((bottom, top)) = body.vertical_bounds_at_samples(wx, wz, floor, &samples)
            else {
                continue;
            };
            pixels[i] = top;
            pixels[i + 1] = bottom;
            pixels[i + 3] = 1.0;
        }
    }
    pixels
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::terrain::{TerrainGenerationRequest, WaterBodyKind};
    #[test]
    fn atlas_masks_spline_outside_and_terrain_floor() {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 3.0;
        world.terrain.config.plains.height_variation = 0.0;
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [32, 16, 32],
            })
            .unwrap();
        let mut lake = WaterBody::new(1, WaterBodyKind::Lake, [16.0, 16.0], 9.0);
        lake.depth = 4.0;
        lake.points[0].position = [11.0, 11.0];
        let pixels = volume_pixels(&world, &lake, 64);
        let center = ((32 * 64 + 32) * 4) as usize;
        assert!(pixels[center + 3] > 0.5);
        assert!((pixels[center] - 9.0).abs() < 0.01);
        assert!(pixels[center + 1] >= 5.0);
        assert_eq!(pixels[3], 0.0, "outside a lake has no volume");
    }
    #[test]
    fn geometry_cache_is_independent_of_color_and_fog() {
        let mut world = World::new();
        let mut lake = WaterBody::new(1, WaterBodyKind::Lake, [0.0, 0.0], 9.0);
        world.terrain.config.water_bodies.push(lake.clone());
        let first = geometry_key(&world, &[&lake]);
        lake.appearance.as_mut().unwrap().underwater_fog_density = 0.4;
        assert_eq!(first, geometry_key(&world, &[&lake]));
        lake.points[0].position[0] -= 2.0;
        assert_ne!(first, geometry_key(&world, &[&lake]));
    }
    #[test]
    fn nearest_visible_bodies_selected_deterministically() {
        let mut world = World::new();
        for i in 0..10 {
            let mut b = WaterBody::new(
                (i + 1) as u64,
                WaterBodyKind::Lake,
                [i as f32 * 10.0, 0.0],
                10.0,
            );
            b.appearance.as_mut().unwrap().underwater_fog_color = [i as f32 / 7.0, 0.0, 0.0];
            world.terrain.config.water_bodies.push(b);
        }
        let near = nearby_bodies(&world, Vec3::new(0.0, 10.0, 0.0));
        assert_eq!(near.len(), BODY_LIMIT);
        assert_eq!(near[0].id, 1);
    }
    #[test]
    fn shader_contract_applies_same_local_volume_to_sky_and_objects() {
        let grid = super::super::GRID_FRAGMENT_SHADER;
        let sky = super::super::sky::SKY_FRAGMENT_SHADER;
        for source in [grid, sky] {
            assert!(source.contains("sampler2DArray u_local_water_atlas"));
            assert!(source.contains("uniform vec4 u_local_water_rect"));
            assert!(source.contains("uniform vec4 u_local_water_waves"));
            assert!(source.contains("wk_volume_segment(camera,target,i)"));
            assert!(source.contains("p.y>=column.g && p.y<surface"));
            assert!(source.contains("wk_apply_local_water("));
        }
        assert!(grid.contains("v_world_pos,false"));
        assert!(sky.contains("direction*72.0,true"));
    }
    #[test]
    fn separate_body_postprocess_settings_reach_distinct_uniforms() {
        let mut a = WaterBody::new(1, WaterBodyKind::Lake, [0.0, 0.0], 10.0);
        let mut b = WaterBody::new(2, WaterBodyKind::River, [0.0, 0.0], 10.0);
        let global = crate::world::terrain::TerrainWaterSettings::default();
        a.appearance.as_mut().unwrap().underwater_fog_color = [0.7, 0.1, 0.2];
        b.appearance.as_mut().unwrap().underwater_fog_color = [0.1, 0.7, 0.2];
        a.appearance.as_mut().unwrap().wave_height = 0.15;
        b.appearance.as_mut().unwrap().wave_height = 2.5;
        let left = descriptor(&a, &global);
        let right = descriptor(&b, &global);
        assert_ne!(left.fog, right.fog);
        assert_ne!(left.waves, right.waves);
        b.appearance.as_mut().unwrap().underwater_effects_enabled = false;
        let disabled = descriptor(&b, &global);
        assert_eq!(disabled.settings[2], 0.0);
        assert_eq!(disabled.settings[3], 0.0);
    }
    #[test]
    fn bounded_ray_partial_waterline_and_shore_exclusion() {
        let mut lake = WaterBody::new(12, WaterBodyKind::Lake, [0.0, 0.0], 10.0);
        lake.depth = 5.0;
        let count = |from: [f32; 3], to: [f32; 3]| {
            let mut count = 0usize;
            for i in 0..100 {
                let t = (i as f32 + 0.5) / 100.0;
                let p = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                    from[2] + (to[2] - from[2]) * t,
                ];
                if lake.contains_volume_point(p, 3.0) {
                    count += 1;
                }
            }
            count
        };
        assert_eq!(count([0.0, 12.0, 0.0], [0.0, 11.0, 0.0]), 0);
        assert_eq!(count([0.0, 8.0, 0.0], [0.0, 7.0, 0.0]), 100);
        assert!(count([0.0, 12.0, 0.0], [0.0, 8.0, 0.0]) >= 45);
        assert!(count([0.0, 12.0, 0.0], [0.0, 8.0, 0.0]) <= 55);
        assert_eq!(count([15.0, 8.0, 0.0], [20.0, 8.0, 0.0]), 0);
        assert_eq!(count([0.0, 4.0, 0.0], [0.0, 3.0, 0.0]), 0);
        assert!(count([-12.0, 8.0, 0.0], [0.0, 8.0, 0.0]) > 0);
    }
}

#[cfg(test)]
mod water_material_volume_tests {
    use super::*;
    use crate::project::material::*;
    #[test]
    fn water_material_surface_and_underwater_share_live_resolution_without_atlas_rebuild() {
        let mut look = crate::world::terrain::TerrainWaterSettings::default();
        look.wave_height = 1.8;
        look.underwater_fog_density = 0.24;
        let doc = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(look),
        });
        let root = std::env::temp_dir().join(&doc.id);
        std::fs::create_dir_all(&root).unwrap();
        save(&root, "water", &doc, None).unwrap();
        let catalog = MaterialCatalog::load(&root);
        let world = World::new();
        let mut body = WaterBody::new(1, WaterBodyKind::Lake, [0.0, 0.0], 10.0);
        let old_key = geometry_key(&world, &[&body]);
        body.assign_material(&catalog, &doc.id).unwrap();
        let descriptor = descriptor_with_material(&body, &world.terrain.config.water, &catalog);
        let surface = body.resolved_appearance(&catalog, &world.terrain.config.water);
        assert_eq!(descriptor.waves[0], surface.wave_height);
        assert_eq!(descriptor.fog[3], surface.underwater_fog_density);
        assert_eq!(old_key, geometry_key(&world, &[&body]));
        std::fs::remove_dir_all(root).unwrap();
    }
}
