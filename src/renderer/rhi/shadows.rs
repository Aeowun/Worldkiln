// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::landscape::Bounds;
use glam::{DMat4, DVec3, DVec4};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowCamera {
    pub camera_to_world: DMat4,
    pub vertical_fov_radians: f64,
    pub aspect_ratio: f64,
    pub near: f64,
    pub far: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CascadedShadowSettings {
    pub count: usize,
    pub resolution: u32,
    pub distribution: f64,
    pub caster_padding: f64,
    pub transition_fraction: f64,
}

impl Default for CascadedShadowSettings {
    fn default() -> Self {
        Self {
            count: 4,
            resolution: 2048,
            distribution: 0.7,
            caster_padding: 60.0,
            transition_fraction: 0.1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowCascade {
    pub split_near: f64,
    pub split_far: f64,
    pub world_to_shadow_clip: DMat4,
    pub world_center: DVec3,
    pub texel_world_size: f64,
    pub shadow_world_extent: f64,
    pub receiver_corners: [DVec3; 8],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShadowCascades {
    pub cascades: Vec<ShadowCascade>,
    pub transition_fraction: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShadowError {
    InvalidCamera,
    InvalidSettings,
    InvalidLight,
}

fn receiver_corners(camera: ShadowCamera, start: f64, end: f64) -> [DVec3; 8] {
    let tan_half_y = (camera.vertical_fov_radians * 0.5).tan();
    std::array::from_fn(|index| {
        let depth = if index < 4 { start } else { end };
        let x = if index & 1 == 0 { -1.0 } else { 1.0 };
        let y = if index & 2 == 0 { -1.0 } else { 1.0 };
        let point = DVec3::new(
            x * depth * tan_half_y * camera.aspect_ratio,
            y * depth * tan_half_y,
            -depth,
        );
        camera.camera_to_world.transform_point3(point)
    })
}

fn valid_camera(c: ShadowCamera) -> bool {
    c.vertical_fov_radians.is_finite()
        && c.vertical_fov_radians > 0.01
        && c.vertical_fov_radians < std::f64::consts::PI - 0.01
        && c.aspect_ratio.is_finite()
        && c.aspect_ratio > 0.0
        && c.near.is_finite()
        && c.near > 0.0
        && c.far.is_finite()
        && c.far > c.near
        && c.camera_to_world
            .to_cols_array()
            .iter()
            .all(|v| v.is_finite())
        && c.camera_to_world.determinant().abs() > 1e-8
}

fn cascade_matrix(
    corners: &[DVec3; 8],
    light_rotation: DMat4,
    resolution: u32,
    caster_padding: f64,
) -> (DMat4, DVec3, f64, f64) {
    let center = corners.iter().copied().sum::<DVec3>() / 8.0;
    let radius = corners
        .iter()
        .map(|corner| (*corner - center).length())
        .fold(0.0_f64, f64::max);
    let extent = (radius * 16.0).ceil().max(1.0) / 16.0;
    let texel = (extent * 2.0) / resolution as f64;
    let light_center = light_rotation.transform_point3(center);
    let snapped_x = (light_center.x / texel).floor() * texel;
    let snapped_y = (light_center.y / texel).floor() * texel;
    let (mut min_z, mut max_z) = (f64::INFINITY, f64::NEG_INFINITY);
    for corner in corners {
        let z = light_rotation.transform_point3(*corner).z;
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    min_z -= caster_padding;
    max_z += caster_padding;
    let inv_range = 1.0 / (min_z - max_z);
    let projection = DMat4::from_cols(
        DVec4::new(1.0 / extent, 0.0, 0.0, 0.0),
        DVec4::new(0.0, 1.0 / extent, 0.0, 0.0),
        DVec4::new(0.0, 0.0, inv_range, 0.0),
        DVec4::new(
            -snapped_x / extent,
            -snapped_y / extent,
            -max_z * inv_range,
            1.0,
        ),
    );
    (projection * light_rotation, center, texel, extent)
}

pub fn build_cascaded_shadows(
    camera: ShadowCamera,
    sun_direction_to_sun: DVec3,
    settings: CascadedShadowSettings,
) -> Result<ShadowCascades, ShadowError> {
    if !valid_camera(camera) {
        return Err(ShadowError::InvalidCamera);
    }
    if settings.count == 0
        || settings.count > 8
        || settings.resolution < 128
        || settings.resolution > 16384
        || !settings.distribution.is_finite()
        || !(0.0..=1.0).contains(&settings.distribution)
        || !settings.transition_fraction.is_finite()
        || !(0.0..=0.4).contains(&settings.transition_fraction)
        || !settings.caster_padding.is_finite()
        || settings.caster_padding < 0.0
    {
        return Err(ShadowError::InvalidSettings);
    }
    if !sun_direction_to_sun.is_finite() || sun_direction_to_sun.length_squared() < 1e-12 {
        return Err(ShadowError::InvalidLight);
    }
    let sun = sun_direction_to_sun.normalize();
    let up = if sun.dot(DVec3::Y).abs() > 0.95 {
        DVec3::X
    } else {
        DVec3::Y
    };
    let rotation = DMat4::look_at_rh(DVec3::ZERO, -sun, up);
    let mut result = Vec::with_capacity(settings.count);
    let mut last = camera.near;
    for i in 1..=settings.count {
        let fraction = i as f64 / settings.count as f64;
        let linear = camera.near + (camera.far - camera.near) * fraction;
        let logarithmic = camera.near * (camera.far / camera.near).powf(fraction);
        let far = if i == settings.count {
            camera.far
        } else {
            linear * (1.0 - settings.distribution) + logarithmic * settings.distribution
        };
        let corners = receiver_corners(camera, last, far);
        let (matrix, center, texel, extent) = cascade_matrix(
            &corners,
            rotation,
            settings.resolution,
            settings.caster_padding,
        );
        result.push(ShadowCascade {
            split_near: last,
            split_far: far,
            world_to_shadow_clip: matrix,
            world_center: center,
            texel_world_size: texel,
            shadow_world_extent: extent,
            receiver_corners: corners,
        });
        last = far;
    }
    Ok(ShadowCascades {
        cascades: result,
        transition_fraction: settings.transition_fraction,
    })
}

impl ShadowCascades {
    pub fn sample_cascades(&self, camera_depth: f64) -> Option<(usize, Option<usize>, f32)> {
        if !camera_depth.is_finite() {
            return None;
        }
        for (index, cascade) in self.cascades.iter().enumerate() {
            if camera_depth < cascade.split_near {
                return None;
            }
            if camera_depth <= cascade.split_far {
                let band = (cascade.split_far - cascade.split_near) * self.transition_fraction;
                if index + 1 < self.cascades.len()
                    && band > 0.0
                    && camera_depth > cascade.split_far - band
                {
                    return Some((
                        index,
                        Some(index + 1),
                        ((camera_depth - (cascade.split_far - band)) / band) as f32,
                    ));
                }
                return Some((index, None, 0.0));
            }
        }
        None
    }
}

impl ShadowCascade {
    pub fn intersects_world_aabb(&self, bounds: Bounds) -> bool {
        let m = self.world_to_shadow_clip.to_cols_array();
        let row = |r: usize| -> [f64; 4] { [m[r], m[r + 4], m[r + 8], m[r + 12]] };
        let a = row(0);
        let b = row(1);
        let c = row(2);
        let w = row(3);
        let planes = [
            std::array::from_fn(|i| w[i] + a[i]),
            std::array::from_fn(|i| w[i] - a[i]),
            std::array::from_fn(|i| w[i] + b[i]),
            std::array::from_fn(|i| w[i] - b[i]),
            c,
            std::array::from_fn(|i| w[i] - c[i]),
        ];
        planes.iter().all(|p| {
            let furthest: [f64; 3] = std::array::from_fn(|i| {
                if p[i] >= 0.0 {
                    bounds.max[i]
                } else {
                    bounds.min[i]
                }
            });
            p[0] * furthest[0] + p[1] * furthest[1] + p[2] * furthest[2] + p[3] >= 0.0
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn camera() -> ShadowCamera {
        ShadowCamera {
            camera_to_world: DMat4::IDENTITY,
            vertical_fov_radians: 1.1,
            aspect_ratio: 16.0 / 9.0,
            near: 0.3,
            far: 1500.0,
        }
    }

    #[test]
    fn frustum_corners_fit_each_shadow_cascade() {
        let cascades = build_cascaded_shadows(
            camera(),
            DVec3::new(0.3, 1.0, 0.1),
            CascadedShadowSettings::default(),
        )
        .unwrap();
        assert_eq!(cascades.cascades.len(), 4);
        let mut near = 0.3;
        for c in &cascades.cascades {
            assert_eq!(c.split_near, near);
            assert!(c.split_far > near);
            for corner in &c.receiver_corners {
                let p = c.world_to_shadow_clip.project_point3(*corner);
                assert!(p.x.abs() <= 1.0000001, "x {p:?}");
                assert!(p.y.abs() <= 1.0000001, "y {p:?}");
                assert!((-1e-7..=1.0000001).contains(&p.z), "z {p:?}");
            }
            near = c.split_far;
        }
        assert_eq!(near, 1500.0);
    }

    #[test]
    fn subtexel_camera_movement_does_not_move_shadow_projection() {
        let s = CascadedShadowSettings::default();
        let sun = DVec3::new(0.2, 0.9, -0.3);
        let first = build_cascaded_shadows(camera(), sun, s).unwrap();
        let mut moved = camera();
        moved.camera_to_world = DMat4::from_translation(DVec3::new(0.00001, 0.0, 0.0));
        let second = build_cascaded_shadows(moved, sun, s).unwrap();
        for (a, b) in first.cascades.iter().zip(&second.cascades) {
            let aa = a.world_to_shadow_clip.to_cols_array();
            let bb = b.world_to_shadow_clip.to_cols_array();
            for i in [0, 4, 8, 12, 1, 5, 9, 13] {
                assert!(
                    (aa[i] - bb[i]).abs() < 1e-10,
                    "unstable {i}: {} vs {}",
                    aa[i],
                    bb[i]
                );
            }
        }
    }

    #[test]
    fn light_crossfade_and_bounds_are_correct() {
        let cs =
            build_cascaded_shadows(camera(), DVec3::Y, CascadedShadowSettings::default()).unwrap();
        assert_eq!(cs.sample_cascades(0.01), None);
        assert_eq!(cs.sample_cascades(1501.0), None);
        let first = cs.cascades[0];
        let depth = first.split_far - (first.split_far - first.split_near) * 0.04;
        let (a, b, weight) = cs.sample_cascades(depth).unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, Some(1));
        assert!(weight > 0.5 && weight < 0.7);
        let p = first.world_center;
        let inside = Bounds {
            min: [p.x - 0.5, p.y - 0.5, p.z - 0.5],
            max: [p.x + 0.5, p.y + 0.5, p.z + 0.5],
        };
        assert!(first.intersects_world_aabb(inside));
        assert!(!first.intersects_world_aabb(Bounds {
            min: [1e6, 1e6, 1e6],
            max: [1e6 + 1.0, 1e6 + 1.0, 1e6 + 1.0],
        }));
    }

    #[test]
    fn invalid_inputs_rejected_while_vertical_sun_is_stable() {
        let mut c = camera();
        c.near = 0.0;
        assert_eq!(
            build_cascaded_shadows(c, DVec3::Y, CascadedShadowSettings::default()),
            Err(ShadowError::InvalidCamera)
        );
        assert_eq!(
            build_cascaded_shadows(camera(), DVec3::ZERO, CascadedShadowSettings::default()),
            Err(ShadowError::InvalidLight)
        );
        let mut bad = CascadedShadowSettings::default();
        bad.resolution = 0;
        assert_eq!(
            build_cascaded_shadows(camera(), DVec3::Y, bad),
            Err(ShadowError::InvalidSettings)
        );
        assert!(
            build_cascaded_shadows(camera(), DVec3::Y, CascadedShadowSettings::default()).is_ok()
        );
    }
}
