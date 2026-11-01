// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

pub(crate) fn mesh_bottom_y(
    mesh_bounds: Option<(glam::Vec3, glam::Vec3)>,
    scale: f32,
    rotation: glam::Vec3,
) -> f32 {
    mesh_bottom_y_scaled(mesh_bounds, glam::Vec3::splat(scale), rotation)
}

fn mesh_bottom_y_scaled(
    mesh_bounds: Option<(glam::Vec3, glam::Vec3)>,
    scale: glam::Vec3,
    rotation: glam::Vec3,
) -> f32 {
    let Some((min, max)) = mesh_bounds else {
        return 0.0;
    };
    let orientation =
        glam::Quat::from_euler(glam::EulerRot::XYZ, rotation.x, rotation.y, rotation.z);
    let mut bottom = f32::INFINITY;
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                bottom = bottom.min((orientation * (glam::Vec3::new(x, y, z) * scale)).y);
            }
        }
    }
    bottom
}

pub(crate) fn surface_position(
    mut surface: glam::Vec3,
    mesh_bounds: Option<(glam::Vec3, glam::Vec3)>,
    scale: glam::Vec3,
    rotation: glam::Vec3,
) -> glam::Vec3 {
    surface.y -= mesh_bottom_y_scaled(mesh_bounds, scale, rotation);
    surface
}

pub(crate) fn surface_placement_y(
    surface_y: f32,
    mesh_bounds: Option<(glam::Vec3, glam::Vec3)>,
    scale: f32,
    rotation: glam::Vec3,
    y_offset: f32,
) -> f32 {
    surface_y - mesh_bottom_y(mesh_bounds, scale, rotation) + y_offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn surface_placement_accounts_for_mesh_origin() {
        for (minimum_y, maximum_y, expected_y) in [
            (-0.5, 0.5, 10.5),
            (-0.2, 0.8, 10.2),
            (-0.125, 0.375, 10.125),
            (0.0, 1.0, 10.0),
            (0.25, 1.0, 9.75),
        ] {
            let bounds = Some((
                Vec3::new(-1.0, minimum_y, -1.0),
                Vec3::new(1.0, maximum_y, 1.0),
            ));
            let placed_y = surface_placement_y(10.0, bounds, 1.0, Vec3::ZERO, 0.0);
            assert!((placed_y - expected_y).abs() < 0.0001);
            assert!((placed_y + minimum_y - 10.0).abs() < 0.0001);
        }
        assert_eq!(surface_placement_y(10.0, None, 1.0, Vec3::ZERO, 0.0), 10.0);
    }

    #[test]
    fn surface_placement_retains_build_scale_rotation_and_offset() {
        let bounds = Some((Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 2.0, 1.0)));
        let rotation = Vec3::new(30.0_f32.to_radians(), 0.0, 0.0);
        assert!((mesh_bottom_y(bounds, 2.0, rotation) + 1.0).abs() < 0.0001);
        assert!((surface_placement_y(10.0, bounds, 2.0, rotation, 0.3) - 11.3).abs() < 0.0001);
        assert!((surface_placement_y(10.0, bounds, 2.0, rotation, -0.3) - 10.7).abs() < 0.0001);
    }
}
