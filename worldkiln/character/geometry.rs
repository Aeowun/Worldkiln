use super::character::AppearanceCustomization;
use super::package::{GeometryPart, GeometryPrimitive, SkinnedVertex};
use super::rig::EvaluatedPose;
use glam::Vec3;

pub fn generate_mesh_from_parts(
    parts: &[GeometryPart],
    pose: &EvaluatedPose,
    appearance: &AppearanceCustomization,
    has_gun: bool,
) -> Vec<f32> {
    let mut vertices = Vec::new();

    for part in parts {
        if !part_visible(part, appearance, has_gun) {
            continue;
        }

        let color = appearance.material(&part.material);

        match &part.primitive {
            GeometryPrimitive::SkinnedTriangleMesh {
                vertices: skinned_vertices,
                inverse_bind_matrices,
            } => add_skinned_triangle_mesh(
                &mut vertices,
                pose,
                skinned_vertices,
                inverse_bind_matrices,
            ),
            GeometryPrimitive::Cube { min, max } => {
                add_cube(&mut vertices, pose, part.joint_index, *min, *max, color);
            }
            GeometryPrimitive::Cylinder {
                radius_x,
                radius_z,
                min_y,
                max_y,
                segments,
            } => {
                add_cylinder(
                    &mut vertices,
                    pose,
                    part.joint_index,
                    *radius_x,
                    *radius_z,
                    *min_y,
                    *max_y,
                    *segments,
                    color,
                );
            }
            GeometryPrimitive::Ellipsoid {
                center,
                radius_x,
                radius_y,
                radius_z,
                segments,
                rings,
            } => {
                add_ellipsoid(
                    &mut vertices,
                    pose,
                    part.joint_index,
                    *center,
                    *radius_x,
                    *radius_y,
                    *radius_z,
                    *segments,
                    *rings,
                    color,
                );
            }
            GeometryPrimitive::TaperedFlame {
                radius_bottom,
                radius_top,
                min_y,
                max_y,
                segments,
            } => {
                add_tapered_flame(
                    &mut vertices,
                    pose,
                    part.joint_index,
                    *radius_bottom,
                    *radius_top,
                    *min_y,
                    *max_y,
                    *segments,
                    color,
                );
            }
        }
    }

    vertices
}

fn add_skinned_triangle_mesh(
    output: &mut Vec<f32>,
    pose: &EvaluatedPose,
    vertices: &[SkinnedVertex],
    inverse_bind_matrices: &[glam::Mat4],
) {
    for vertex in vertices {
        let mut position = Vec3::ZERO;
        let mut normal = Vec3::ZERO;
        for influence in 0..4 {
            let weight = vertex.weights[influence];
            if weight <= 0.0 {
                continue;
            }
            let joint = vertex.joints[influence];
            let (Some(pose_matrix), Some(inverse_bind)) =
                (pose.matrices.get(joint), inverse_bind_matrices.get(joint))
            else {
                continue;
            };
            let skin_matrix = *pose_matrix * *inverse_bind;
            position += skin_matrix.transform_point3(vertex.position) * weight;
            normal += skin_matrix.transform_vector3(vertex.normal) * weight;
        }
        let normal = normal.try_normalize().unwrap_or(Vec3::Y);
        push_vertex(output, position, normal, vertex.color);
    }
}

pub const DEBUGGING_MODE: bool = true;

fn part_visible(part: &GeometryPart, appearance: &AppearanceCustomization, has_gun: bool) -> bool {
    match part.when.as_deref() {
        None => true,
        Some("show_accessory") => appearance.show_accessory,
        Some("!show_accessory") => !appearance.show_accessory,
        Some("show_accent") => appearance.show_accent,
        Some("!show_accent") => !appearance.show_accent,
        Some("has_gun") => has_gun,
        Some("!has_gun") => !has_gun,
        Some("debugging_mode") => DEBUGGING_MODE,
        Some("!debugging_mode") => !DEBUGGING_MODE,
        Some(_) => true,
    }
}

fn add_cube(
    vertices: &mut Vec<f32>,
    pose: &EvaluatedPose,
    joint_idx: usize,
    min: Vec3,
    max: Vec3,
    color: [f32; 4],
) {
    if joint_idx >= pose.matrices.len() {
        return;
    }

    let mat = pose.matrices[joint_idx];

    let faces = [
        (
            [min.x, min.y, max.z],
            [max.x, min.y, max.z],
            [max.x, max.y, max.z],
            [min.x, max.y, max.z],
            [0.0, 0.0, 1.0],
        ),
        (
            [max.x, min.y, min.z],
            [min.x, min.y, min.z],
            [min.x, max.y, min.z],
            [max.x, max.y, min.z],
            [0.0, 0.0, -1.0],
        ),
        (
            [min.x, min.y, min.z],
            [min.x, min.y, max.z],
            [min.x, max.y, max.z],
            [min.x, max.y, min.z],
            [-1.0, 0.0, 0.0],
        ),
        (
            [max.x, min.y, max.z],
            [max.x, min.y, min.z],
            [max.x, max.y, min.z],
            [max.x, max.y, max.z],
            [1.0, 0.0, 0.0],
        ),
        (
            [min.x, max.y, max.z],
            [max.x, max.y, max.z],
            [max.x, max.y, min.z],
            [min.x, max.y, min.z],
            [0.0, 1.0, 0.0],
        ),
        (
            [min.x, min.y, min.z],
            [max.x, min.y, min.z],
            [max.x, min.y, max.z],
            [min.x, min.y, max.z],
            [0.0, -1.0, 0.0],
        ),
    ];

    for (v1, v2, v3, v4, normal) in faces {
        let n = mat.transform_vector3(Vec3::from_slice(&normal)).normalize();
        let points = [v1, v2, v3, v4].map(|p| mat.transform_point3(Vec3::from_slice(&p)));

        push_vertex(vertices, points[0], n, color);
        push_vertex(vertices, points[1], n, color);
        push_vertex(vertices, points[2], n, color);

        push_vertex(vertices, points[0], n, color);
        push_vertex(vertices, points[2], n, color);
        push_vertex(vertices, points[3], n, color);
    }
}

fn add_cylinder(
    vertices: &mut Vec<f32>,
    pose: &EvaluatedPose,
    joint_idx: usize,
    radius_x: f32,
    radius_z: f32,
    min_y: f32,
    max_y: f32,
    segments: usize,
    color: [f32; 4],
) {
    if joint_idx >= pose.matrices.len() || segments < 3 {
        return;
    }

    let mat = pose.matrices[joint_idx];
    let radius_x = radius_x.max(0.0001);
    let radius_z = radius_z.max(0.0001);

    let mut bottom = Vec::with_capacity(segments);
    let mut top = Vec::with_capacity(segments);

    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let x = angle.cos();
        let z = angle.sin();
        bottom.push(Vec3::new(x * radius_x, min_y, z * radius_z));
        top.push(Vec3::new(x * radius_x, max_y, z * radius_z));
    }

    for i in 0..segments {
        let next = (i + 1) % segments;
        let b0 = bottom[i];
        let b1 = bottom[next];
        let t1 = top[next];
        let t0 = top[i];

        let local_normal = Vec3::new(b0.x / radius_x, 0.0, b0.z / radius_z).normalize();
        let normal = mat.transform_vector3(local_normal).normalize();

        let p0 = mat.transform_point3(b0);
        let p1 = mat.transform_point3(b1);
        let p2 = mat.transform_point3(t1);
        let p3 = mat.transform_point3(t0);

        push_vertex(vertices, p0, normal, color);
        push_vertex(vertices, p2, normal, color);
        push_vertex(vertices, p1, normal, color);

        push_vertex(vertices, p0, normal, color);
        push_vertex(vertices, p3, normal, color);
        push_vertex(vertices, p2, normal, color);
    }

    {
        let normal = mat.transform_vector3(Vec3::Y).normalize();
        let center = mat.transform_point3(Vec3::new(0.0, max_y, 0.0));

        for i in 0..segments {
            let next = (i + 1) % segments;
            let p1 = mat.transform_point3(top[i]);
            let p2 = mat.transform_point3(top[next]);
            push_vertex(vertices, center, normal, color);
            push_vertex(vertices, p2, normal, color);
            push_vertex(vertices, p1, normal, color);
        }
    }

    {
        let normal = mat.transform_vector3(-Vec3::Y).normalize();
        let center = mat.transform_point3(Vec3::new(0.0, min_y, 0.0));

        for i in 0..segments {
            let next = (i + 1) % segments;
            let p1 = mat.transform_point3(bottom[i]);
            let p2 = mat.transform_point3(bottom[next]);
            push_vertex(vertices, center, normal, color);
            push_vertex(vertices, p1, normal, color);
            push_vertex(vertices, p2, normal, color);
        }
    }
}

fn add_ellipsoid(
    vertices: &mut Vec<f32>,
    pose: &EvaluatedPose,
    joint_idx: usize,
    center: Vec3,
    radius_x: f32,
    radius_y: f32,
    radius_z: f32,
    segments: usize,
    rings: usize,
    color: [f32; 4],
) {
    if joint_idx >= pose.matrices.len() || segments < 3 || rings < 2 {
        return;
    }

    let mat = pose.matrices[joint_idx];

    for ring in 0..rings {
        let phi0 =
            -std::f32::consts::FRAC_PI_2 + (ring as f32 / rings as f32) * std::f32::consts::PI;
        let phi1 = -std::f32::consts::FRAC_PI_2
            + ((ring + 1) as f32 / rings as f32) * std::f32::consts::PI;

        let y0 = phi0.sin();
        let y1 = phi1.sin();
        let r0 = phi0.cos();
        let r1 = phi1.cos();

        for segment in 0..segments {
            let a0 = (segment as f32 / segments as f32) * std::f32::consts::TAU;
            let a1 = ((segment + 1) as f32 / segments as f32) * std::f32::consts::TAU;

            let p00 = Vec3::new(
                center.x + a0.cos() * radius_x * r0,
                center.y + y0 * radius_y,
                center.z + a0.sin() * radius_z * r0,
            );
            let p01 = Vec3::new(
                center.x + a1.cos() * radius_x * r0,
                center.y + y0 * radius_y,
                center.z + a1.sin() * radius_z * r0,
            );
            let p10 = Vec3::new(
                center.x + a0.cos() * radius_x * r1,
                center.y + y1 * radius_y,
                center.z + a0.sin() * radius_z * r1,
            );
            let p11 = Vec3::new(
                center.x + a1.cos() * radius_x * r1,
                center.y + y1 * radius_y,
                center.z + a1.sin() * radius_z * r1,
            );

            let n00 = Vec3::new(a0.cos() * r0, y0, a0.sin() * r0).normalize();
            let n01 = Vec3::new(a1.cos() * r0, y0, a1.sin() * r0).normalize();
            let n10 = Vec3::new(a0.cos() * r1, y1, a0.sin() * r1).normalize();
            let n11 = Vec3::new(a1.cos() * r1, y1, a1.sin() * r1).normalize();

            let tp00 = mat.transform_point3(p00);
            let tp01 = mat.transform_point3(p01);
            let tp10 = mat.transform_point3(p10);
            let tp11 = mat.transform_point3(p11);

            let tn00 = mat.transform_vector3(n00).normalize();
            let tn01 = mat.transform_vector3(n01).normalize();
            let tn10 = mat.transform_vector3(n10).normalize();
            let tn11 = mat.transform_vector3(n11).normalize();

            push_vertex(vertices, tp00, tn00, color);
            push_vertex(vertices, tp10, tn10, color);
            push_vertex(vertices, tp11, tn11, color);

            push_vertex(vertices, tp00, tn00, color);
            push_vertex(vertices, tp11, tn11, color);
            push_vertex(vertices, tp01, tn01, color);
        }
    }
}

fn add_tapered_flame(
    vertices: &mut Vec<f32>,
    pose: &EvaluatedPose,
    joint_idx: usize,
    radius_bottom: f32,
    radius_top: f32,
    min_y: f32,
    max_y: f32,
    segments: usize,
    color: [f32; 4],
) {
    if joint_idx >= pose.matrices.len() || segments < 3 {
        return;
    }

    let mat = pose.matrices[joint_idx];

    let mut bottom = Vec::with_capacity(segments);
    let mut top = Vec::with_capacity(segments);

    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let x = angle.cos();
        let z = angle.sin();
        bottom.push(Vec3::new(x * radius_bottom, min_y, z * radius_bottom));
        top.push(Vec3::new(x * radius_top, max_y, z * radius_top));
    }

    let height = (max_y - min_y).max(0.0001);
    let slope = (radius_top - radius_bottom) / height;

    for i in 0..segments {
        let next = (i + 1) % segments;
        let angle = ((i as f32 + 0.5) / segments as f32) * std::f32::consts::TAU;
        let radial = Vec3::new(angle.cos(), 0.0, angle.sin());
        let local_normal = Vec3::new(radial.x, -slope, radial.z).normalize();
        let normal = mat.transform_vector3(local_normal).normalize();

        let p0 = mat.transform_point3(bottom[i]);
        let p1 = mat.transform_point3(bottom[next]);
        let p2 = mat.transform_point3(top[next]);
        let p3 = mat.transform_point3(top[i]);

        push_vertex(vertices, p0, normal, color);
        push_vertex(vertices, p2, normal, color);
        push_vertex(vertices, p1, normal, color);

        push_vertex(vertices, p0, normal, color);
        push_vertex(vertices, p3, normal, color);
        push_vertex(vertices, p2, normal, color);
    }

    {
        let normal = mat.transform_vector3(Vec3::Y).normalize();
        let center = mat.transform_point3(Vec3::new(0.0, max_y, 0.0));

        for i in 0..segments {
            let next = (i + 1) % segments;
            let p1 = mat.transform_point3(top[i]);
            let p2 = mat.transform_point3(top[next]);
            push_vertex(vertices, center, normal, color);
            push_vertex(vertices, p2, normal, color);
            push_vertex(vertices, p1, normal, color);
        }
    }

    if radius_bottom > 0.001 {
        let normal = mat.transform_vector3(-Vec3::Y).normalize();
        let center = mat.transform_point3(Vec3::new(0.0, min_y, 0.0));

        for i in 0..segments {
            let next = (i + 1) % segments;
            let p1 = mat.transform_point3(bottom[i]);
            let p2 = mat.transform_point3(bottom[next]);
            push_vertex(vertices, center, normal, color);
            push_vertex(vertices, p1, normal, color);
            push_vertex(vertices, p2, normal, color);
        }
    }
}

fn push_vertex(buffer: &mut Vec<f32>, pos: Vec3, normal: Vec3, color: [f32; 4]) {
    buffer.extend_from_slice(&[
        pos.x, pos.y, pos.z, normal.x, normal.y, normal.z, color[0], color[1], color[2], color[3],
    ]);
}
