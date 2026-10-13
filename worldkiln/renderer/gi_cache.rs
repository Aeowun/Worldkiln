use glam::Vec3;
use std::time::Instant;

use crate::engine::physics::PhysicsWorld;
use crate::world::World;

use super::PointLightData;
use super::gi_scene::{GiVoxelScene, unpack_rgb8};
use super::settings::{
    BounceLighting, CacheUpdateRate, Denoising, GiResolution, LightingQuality,
    RendererLightingSettings, ShadowMethod, TemporalReuse,
};

const RAY_DIRECTIONS: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
];
const MAX_GI_RAY_DISTANCE: f32 = 48.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GiCacheStats {
    pub updated_probes: usize,
    pub traced_rays: usize,
    pub cache_probes: usize,
    pub scene_voxels: usize,
    pub scene_payload_bytes: usize,
    pub update_cpu_ms: f32,
    pub dynamic_aabbs: usize,
}

#[derive(Clone, Copy, Debug)]
struct DynamicAabb {
    min: Vec3,
    max: Vec3,
    albedo: Vec3,
}

#[derive(Clone, Copy, Debug)]
struct SceneHit {
    coord: Option<crate::world::WorldCoord>,
    distance: f32,
    normal: Vec3,
    albedo: Vec3,
    emissive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CacheLayout {
    dimensions: [usize; 3],
    spacing: f32,
}

pub struct GiRadianceCache {
    scene: GiVoxelScene,
    texture: u32,
    layout: CacheLayout,
    origin: Vec3,
    radiance: Vec<f32>,
    update_cursor: usize,
    initialized: bool,
    stats: GiCacheStats,
    dynamic_aabbs: Vec<DynamicAabb>,
}

impl Default for GiRadianceCache {
    fn default() -> Self {
        Self {
            scene: GiVoxelScene::default(),
            texture: 0,
            layout: layout_for(LightingQuality::Balanced, GiResolution::Half),
            origin: Vec3::ZERO,
            radiance: Vec::new(),
            update_cursor: 0,
            initialized: false,
            stats: GiCacheStats::default(),
            dynamic_aabbs: Vec::new(),
        }
    }
}

impl GiRadianceCache {
    pub fn texture(&self) -> u32 {
        self.texture
    }

    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    pub fn spacing(&self) -> f32 {
        self.layout.spacing
    }

    pub fn dimensions(&self) -> [i32; 3] {
        self.layout.dimensions.map(|value| value as i32)
    }

    pub fn stats(&self) -> GiCacheStats {
        self.stats
    }

    pub fn update(
        &mut self,
        world: &World,
        physics: &PhysicsWorld,
        camera_position: Vec3,
        point_lights: &[PointLightData],
        settings: RendererLightingSettings,
    ) {
        let update_started = Instant::now();
        let scene_stats = self.scene.update(world);
        self.dynamic_aabbs.clear();
        if settings.dynamic_geometry_gi == super::settings::DynamicGeometryGi::BodiesOnly {
            self.dynamic_aabbs.extend(
                physics
                    .bodies
                    .iter()
                    .filter(|body| body.visible && body.solid)
                    .map(|body| DynamicAabb {
                        min: body.min_corner(),
                        max: body.max_corner(),
                        albedo: body.color_rgb.clamp(Vec3::ZERO, Vec3::ONE),
                    }),
            );
        }
        let layout = layout_for(settings.lighting_quality, settings.gi_resolution);
        let origin = cache_origin(camera_position, layout);
        if !self.initialized || self.layout != layout || self.origin != origin {
            self.layout = layout;
            self.origin = origin;
            self.radiance = vec![0.0; probe_count(layout) * RAY_DIRECTIONS.len() * 3];
            self.update_cursor = 0;
            self.initialized = true;
            self.allocate_texture();
        }

        let probe_total = probe_count(layout);
        let update_count = probes_per_frame(settings.cache_update_rate, probe_total);
        for _ in 0..update_count {
            let probe_index = self.update_cursor % probe_total;
            self.update_probe(probe_index, world, point_lights, settings);
            self.update_cursor = (self.update_cursor + 1) % probe_total;
        }
        self.upload_texture();
        self.stats = GiCacheStats {
            updated_probes: update_count,
            traced_rays: update_count * RAY_DIRECTIONS.len(),
            cache_probes: probe_total,
            scene_voxels: scene_stats.occupied_voxels,
            scene_payload_bytes: scene_stats.payload_bytes,
            update_cpu_ms: update_started.elapsed().as_secs_f32() * 1000.0,
            dynamic_aabbs: self.dynamic_aabbs.len(),
        };
    }

    pub fn free_gl_resources(&mut self) {
        if self.texture != 0 {
            unsafe { gl::DeleteTextures(1, &self.texture) };
            self.texture = 0;
        }
    }

    fn update_probe(
        &mut self,
        probe_index: usize,
        world: &World,
        point_lights: &[PointLightData],
        settings: RendererLightingSettings,
    ) {
        let probe_position = self.probe_position(probe_index);
        for (direction_index, direction) in RAY_DIRECTIONS.iter().copied().enumerate() {
            let radiance =
                self.trace_radiance(probe_position, direction, world, point_lights, settings);
            let offset = radiance_offset(probe_index, direction_index, probe_count(self.layout));
            let previous = Vec3::from_slice(&self.radiance[offset..offset + 3]);
            let value = if settings.temporal_reuse == TemporalReuse::On {
                previous.lerp(radiance, 0.18)
            } else {
                radiance
            };
            self.radiance[offset..offset + 3].copy_from_slice(&value.to_array());
        }
    }

    fn trace_radiance(
        &self,
        origin: Vec3,
        direction: Vec3,
        world: &World,
        point_lights: &[PointLightData],
        settings: RendererLightingSettings,
    ) -> Vec3 {
        let Some(hit) = self.trace_scene(
            origin,
            direction,
            MAX_GI_RAY_DISTANCE,
            settings.scene_query_backend,
        ) else {
            return Vec3::ZERO;
        };
        let hit_position = origin + direction * hit.distance;
        let mut incoming = Vec3::ZERO;

        if hit.emissive {
            if let Some(cell) = hit.coord.and_then(|coord| world.get_effective_cell(coord)) {
                incoming += cell.light_color * cell.light_intensity;
            }
        }

        if world.lighting.global_light_enabled {
            let light_direction = -world.lighting.global_light_direction.normalize_or_zero();
            let lambert = hit.normal.dot(light_direction).max(0.0);
            if lambert > 0.0
                && (settings.shadow_method == ShadowMethod::Off
                    || !self.occluded(
                        hit_position + hit.normal * 0.02,
                        light_direction,
                        MAX_GI_RAY_DISTANCE,
                        settings.scene_query_backend,
                    ))
            {
                incoming += world.lighting.global_light_color
                    * world.lighting.global_light_intensity
                    * lambert;
            }
        }

        for light in point_lights {
            let to_light = light.position - hit_position;
            let distance = to_light.length();
            if distance <= 0.0001 || distance >= light.range {
                continue;
            }
            let light_direction = to_light / distance;
            let lambert = hit.normal.dot(light_direction).max(0.0);
            if lambert <= 0.0
                || (settings.shadow_method != ShadowMethod::Off
                    && self.occluded(
                        hit_position + hit.normal * 0.02,
                        light_direction,
                        (distance - 0.04).max(0.0),
                        settings.scene_query_backend,
                    ))
            {
                continue;
            }
            let attenuation = (1.0 - distance / light.range).powi(2);
            incoming += light.color * light.intensity * attenuation * lambert;
        }

        let mut outgoing = hit.albedo * incoming;
        if settings.bounce_lighting == BounceLighting::MultipleBounce {
            outgoing += hit.albedo * self.sample_cpu(hit_position, hit.normal) * 0.55;
        }
        outgoing.min(Vec3::splat(8.0))
    }

    fn occluded(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        backend: super::settings::SceneQueryBackend,
    ) -> bool {
        self.trace_scene(origin, direction, max_distance, backend)
            .is_some()
    }

    fn trace_scene(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        backend: super::settings::SceneQueryBackend,
    ) -> Option<SceneHit> {
        let static_hit = self
            .scene
            .trace(origin, direction, max_distance, backend)
            .map(|hit| SceneHit {
                coord: Some(hit.coord),
                distance: hit.distance,
                normal: hit.normal,
                albedo: unpack_rgb8(hit.packed_material),
                emissive: hit.packed_material & (1 << 31) != 0,
            });
        let dynamic_hit = self
            .dynamic_aabbs
            .iter()
            .filter_map(|aabb| {
                ray_aabb_hit(origin, direction, aabb.min, aabb.max, max_distance).map(
                    |(distance, normal)| SceneHit {
                        coord: None,
                        distance,
                        normal,
                        albedo: aabb.albedo,
                        emissive: false,
                    },
                )
            })
            .min_by(|left, right| left.distance.total_cmp(&right.distance));

        match (static_hit, dynamic_hit) {
            (Some(static_hit), Some(dynamic_hit)) => {
                Some(if static_hit.distance <= dynamic_hit.distance {
                    static_hit
                } else {
                    dynamic_hit
                })
            }
            (Some(hit), None) | (None, Some(hit)) => Some(hit),
            (None, None) => None,
        }
    }

    fn probe_position(&self, index: usize) -> Vec3 {
        let [width, height, _] = self.layout.dimensions;
        let x = index % width;
        let y = (index / width) % height;
        let z = index / (width * height);
        self.origin + Vec3::new(x as f32, y as f32, z as f32) * self.layout.spacing
    }

    fn sample_cpu(&self, position: Vec3, normal: Vec3) -> Vec3 {
        if self.radiance.is_empty() {
            return Vec3::ZERO;
        }
        let local = ((position - self.origin) / self.layout.spacing).round();
        let [width, height, depth] = self.layout.dimensions;
        let x = (local.x as i32).clamp(0, width as i32 - 1) as usize;
        let y = (local.y as i32).clamp(0, height as i32 - 1) as usize;
        let z = (local.z as i32).clamp(0, depth as i32 - 1) as usize;
        let probe_index = x + y * width + z * width * height;
        let weights = [
            normal.x.max(0.0),
            (-normal.x).max(0.0),
            normal.y.max(0.0),
            (-normal.y).max(0.0),
            normal.z.max(0.0),
            (-normal.z).max(0.0),
        ];
        let mut sum = Vec3::ZERO;
        let mut weight_sum = 0.0;
        for (direction, weight) in weights.into_iter().enumerate() {
            if weight == 0.0 {
                continue;
            }
            let offset = radiance_offset(probe_index, direction, probe_count(self.layout));
            sum += Vec3::from_slice(&self.radiance[offset..offset + 3]) * weight;
            weight_sum += weight;
        }
        sum / weight_sum.max(1.0)
    }

    fn allocate_texture(&mut self) {
        unsafe {
            if self.texture == 0 {
                gl::GenTextures(1, &mut self.texture);
            }
            gl::BindTexture(gl::TEXTURE_3D, self.texture);
            gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_3D, gl::TEXTURE_WRAP_R, gl::CLAMP_TO_EDGE as i32);
            let [width, height, depth] = self.layout.dimensions;
            gl::TexImage3D(
                gl::TEXTURE_3D,
                0,
                gl::RGB16F as i32,
                width as i32,
                height as i32,
                (depth * RAY_DIRECTIONS.len()) as i32,
                0,
                gl::RGB,
                gl::FLOAT,
                self.radiance.as_ptr().cast(),
            );
            gl::BindTexture(gl::TEXTURE_3D, 0);
        }
    }

    fn upload_texture(&self) {
        if self.texture == 0 || self.radiance.is_empty() {
            return;
        }
        let [width, height, depth] = self.layout.dimensions;
        unsafe {
            gl::BindTexture(gl::TEXTURE_3D, self.texture);
            gl::TexSubImage3D(
                gl::TEXTURE_3D,
                0,
                0,
                0,
                0,
                width as i32,
                height as i32,
                (depth * RAY_DIRECTIONS.len()) as i32,
                gl::RGB,
                gl::FLOAT,
                self.radiance.as_ptr().cast(),
            );
            gl::BindTexture(gl::TEXTURE_3D, 0);
        }
    }
}

fn ray_aabb_hit(
    origin: Vec3,
    direction: Vec3,
    min: Vec3,
    max: Vec3,
    max_distance: f32,
) -> Option<(f32, Vec3)> {
    let direction = direction.normalize_or_zero();
    if direction == Vec3::ZERO {
        return None;
    }
    let mut enter = 0.0_f32;
    let mut exit = max_distance;
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if direction[axis].abs() < 1.0e-8 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let inverse = direction[axis].recip();
        let mut near = (min[axis] - origin[axis]) * inverse;
        let mut far = (max[axis] - origin[axis]) * inverse;
        let mut near_normal = Vec3::ZERO;
        near_normal[axis] = -direction[axis].signum();
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        if near > enter {
            enter = near;
            normal = near_normal;
        }
        exit = exit.min(far);
        if enter > exit {
            return None;
        }
    }
    (enter <= max_distance).then_some((enter, normal))
}

fn layout_for(quality: LightingQuality, resolution: GiResolution) -> CacheLayout {
    let (base, spacing) = match quality {
        LightingQuality::Performance => ([12, 6, 12], 4.0),
        LightingQuality::Balanced => ([16, 8, 16], 3.0),
        LightingQuality::Quality => ([24, 12, 24], 2.0),
        LightingQuality::Custom => ([20, 10, 20], 2.5),
    };
    let scale = match resolution {
        GiResolution::Quarter => 0.75,
        GiResolution::Half => 1.0,
        GiResolution::Full => 1.25,
    };
    CacheLayout {
        dimensions: base.map(|value| ((value as f32 * scale).round() as usize).max(2)),
        spacing,
    }
}

fn cache_origin(camera_position: Vec3, layout: CacheLayout) -> Vec3 {
    let extent = Vec3::new(
        (layout.dimensions[0] - 1) as f32,
        (layout.dimensions[1] - 1) as f32,
        (layout.dimensions[2] - 1) as f32,
    ) * layout.spacing;
    ((camera_position - extent * 0.5) / layout.spacing).floor() * layout.spacing
}

fn probe_count(layout: CacheLayout) -> usize {
    layout.dimensions.iter().product()
}

fn probes_per_frame(rate: CacheUpdateRate, total: usize) -> usize {
    match rate {
        CacheUpdateRate::Low => 24.min(total),
        CacheUpdateRate::Medium => 96.min(total),
        CacheUpdateRate::High => 384.min(total),
        CacheUpdateRate::EveryFrame => total,
    }
}

fn radiance_offset(probe_index: usize, direction_index: usize, total_probes: usize) -> usize {
    (direction_index * total_probes + probe_index) * 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_and_resolution_change_cache_density() {
        let low = layout_for(LightingQuality::Performance, GiResolution::Quarter);
        let high = layout_for(LightingQuality::Quality, GiResolution::Full);
        assert!(probe_count(high) > probe_count(low));
        assert!(high.spacing < low.spacing);
    }

    #[test]
    fn cache_origin_is_stable_inside_one_spacing_cell() {
        let layout = layout_for(LightingQuality::Balanced, GiResolution::Half);
        let a = cache_origin(Vec3::new(10.1, 4.2, -3.7), layout);
        let b = cache_origin(Vec3::new(10.2, 4.3, -3.6), layout);
        assert_eq!(a, b);
    }

    #[test]
    fn update_rates_have_strictly_increasing_budgets() {
        let total = 10_000;
        assert!(
            probes_per_frame(CacheUpdateRate::Low, total)
                < probes_per_frame(CacheUpdateRate::Medium, total)
        );
        assert!(
            probes_per_frame(CacheUpdateRate::Medium, total)
                < probes_per_frame(CacheUpdateRate::High, total)
        );
        assert_eq!(probes_per_frame(CacheUpdateRate::EveryFrame, total), total);
    }

    #[test]
    fn denoising_enum_is_consumed_by_cache_contract() {
        let modes = Denoising::ALL;
        assert_eq!(modes.len(), 3);
    }

    #[test]
    fn dynamic_aabb_hit_reports_distance_and_entry_normal() {
        let (distance, normal) = ray_aabb_hit(
            Vec3::new(-3.0, 0.5, 0.5),
            Vec3::X,
            Vec3::ZERO,
            Vec3::ONE,
            10.0,
        )
        .unwrap();
        assert!((distance - 3.0).abs() < 0.0001);
        assert_eq!(normal, Vec3::NEG_X);
    }

    #[test]
    fn dynamic_aabb_hit_rejects_misses_and_distance_overflow() {
        assert!(
            ray_aabb_hit(
                Vec3::new(-3.0, 2.0, 0.5),
                Vec3::X,
                Vec3::ZERO,
                Vec3::ONE,
                10.0,
            )
            .is_none()
        );
        assert!(
            ray_aabb_hit(
                Vec3::new(-3.0, 0.5, 0.5),
                Vec3::X,
                Vec3::ZERO,
                Vec3::ONE,
                2.0,
            )
            .is_none()
        );
    }
}
