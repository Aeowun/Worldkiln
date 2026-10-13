use glam::{EulerRot, Mat4, Quat, Vec3};
use std::path::PathBuf;

use crate::engine::entity::RuntimeEntity;
use crate::engine::entity_package::{
    EntityAnimationClip, EntityGeometryPart, EntityGeometryPrimitive, EntityPackage,
    EntityQuaternionKeyframe, EntityTransformTrack, EntityVec3Keyframe, get_package,
};
use crate::world::cell::AttributeValue;

pub const ENTITY_VERTEX_FLOATS: usize = 10;
pub const TEXTURED_ENTITY_VERTEX_FLOATS: usize = 12;

#[derive(Clone, Debug, PartialEq)]
pub struct StaticEntityMeshBatch {
    pub texture: Option<PathBuf>,
    pub vertices: Vec<f32>,
}

#[derive(Clone, Copy)]
struct ResolvedNode {
    matrix: Mat4,
    visible: bool,
}

/// Generates the complete runtime mesh for a package-backed generic Entity.
///
/// The authored EntityPackage supplies geometry/material definitions while
/// RuntimeEntity supplies transient visibility, material, attribute and
/// animation state.
pub fn generate_entity_mesh(entity: &RuntimeEntity) -> Vec<f32> {
    let Some(package_name) = entity.package_name.as_deref() else {
        return Vec::new();
    };

    let Some(package) = get_package(package_name) else {
        return Vec::new();
    };

    generate_entity_mesh_from_package(entity, &package)
}

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
pub fn package_geometry_is_static(package: &EntityPackage) -> bool {
    package.animation.clips.is_empty()
        && package
            .appearance
            .geometry
            .iter()
            .all(|part| part.when.is_none())
}

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
pub fn entity_uses_static_package_geometry(
    entity: &RuntimeEntity,
    package: &EntityPackage,
) -> bool {
    package_geometry_is_static(package)
        && entity.appearance.material_overrides.is_empty()
        && entity.appearance.node_visibility.is_empty()
}

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
pub fn static_entity_world_bounds(entity: &RuntimeEntity) -> Option<(Vec3, Vec3)> {
    let package_name = entity.package_name.as_deref()?;
    let package = get_package(package_name)?;
    if !entity_uses_static_package_geometry(entity, &package) {
        return None;
    }

    let (local_min, local_max) = package.triangle_mesh_bounds()?;
    let rotation = Quat::from_euler(
        EulerRot::XYZ,
        entity.rotation.x,
        entity.rotation.y,
        entity.rotation.z,
    );
    let model = Mat4::from_scale_rotation_translation(entity.scale, rotation, entity.position);
    Some(transform_local_bounds(local_min, local_max, model))
}

fn transform_local_bounds(min: Vec3, max: Vec3, model: Mat4) -> (Vec3, Vec3) {
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ];

    let mut world_min = Vec3::splat(f32::INFINITY);
    let mut world_max = Vec3::splat(f32::NEG_INFINITY);
    for corner in corners {
        let transformed = model.transform_point3(corner);
        world_min = world_min.min(transformed);
        world_max = world_max.max(transformed);
    }
    (world_min, world_max)
}

/// Testable package-explicit mesh generation entry point.
pub fn generate_entity_mesh_from_package(
    entity: &RuntimeEntity,
    package: &EntityPackage,
) -> Vec<f32> {
    if !entity.visible || !entity.enabled {
        return Vec::new();
    }

    if package.appearance.geometry.is_empty() {
        return Vec::new();
    }

    let nodes = resolve_nodes(entity, package);
    let mut vertices = Vec::new();

    for part in &package.appearance.geometry {
        if !when_matches(entity, part.when.as_deref()) {
            continue;
        }

        let node_name = part.node.to_ascii_lowercase();

        let Some(node_index) = package
            .appearance
            .nodes
            .iter()
            .position(|node| node.name.eq_ignore_ascii_case(&node_name))
        else {
            continue;
        };

        let node = nodes[node_index];

        if !node.visible {
            continue;
        }

        let color = material_color(entity, package, &part.material);

        append_geometry(&mut vertices, part, &node.matrix, color);
    }

    vertices
}

/// Builds package-stable batches for the immutable Entity GPU cache.
///
/// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
/// Imported GLB UVs and package-local base-color images stay attached to the
/// geometry batch. Native geometry receives a zero UV and follows the existing
/// vertex-color path when no texture is present.
pub fn generate_static_entity_mesh_batches(
    entity: &RuntimeEntity,
    package: &EntityPackage,
) -> Vec<StaticEntityMeshBatch> {
    if !entity.visible || !entity.enabled || !entity_uses_static_package_geometry(entity, package) {
        return Vec::new();
    }

    let nodes = resolve_nodes(entity, package);
    let mut batches = Vec::new();
    for part in &package.appearance.geometry {
        if !when_matches(entity, part.when.as_deref()) {
            continue;
        }
        let Some(node_index) = package
            .appearance
            .nodes
            .iter()
            .position(|node| node.name.eq_ignore_ascii_case(&part.node))
        else {
            continue;
        };
        let node = nodes[node_index];
        if !node.visible {
            continue;
        }

        let color = material_color(entity, package, &part.material);
        let mut vertices = Vec::new();
        match &part.primitive {
            EntityGeometryPrimitive::TriangleMesh {
                vertices: mesh_vertices,
            } => {
                let normal_matrix = node.matrix.inverse().transpose();
                for vertex in mesh_vertices {
                    let position = node.matrix.transform_point3(vertex.position);
                    let normal = normal_matrix
                        .transform_vector3(vertex.normal)
                        .normalize_or_zero();
                    let vertex_color = [
                        vertex.color[0] * color[0],
                        vertex.color[1] * color[1],
                        vertex.color[2] * color[2],
                        vertex.color[3] * color[3],
                    ];
                    vertices.extend_from_slice(&[
                        position.x,
                        position.y,
                        position.z,
                        normal.x,
                        normal.y,
                        normal.z,
                        vertex_color[0],
                        vertex_color[1],
                        vertex_color[2],
                        vertex_color[3],
                        vertex.tex_coord.x,
                        // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 15:59 -05:00
                        // glTF UVs use the upper-left image origin. Worldkiln uploads
                        // decoded image rows without reversing them, so preserving V
                        // maps those UVs correctly; flipping here sampled the black
                        // half of palette textures such as Kenney's colormap.png.
                        vertex.tex_coord.y,
                    ]);
                }
            }
            _ => {
                let mut untextured = Vec::new();
                append_geometry(&mut untextured, part, &node.matrix, color);
                for vertex in untextured.chunks_exact(ENTITY_VERTEX_FLOATS) {
                    vertices.extend_from_slice(vertex);
                    vertices.extend_from_slice(&[0.0, 0.0]);
                }
            }
        }

        if !vertices.is_empty() {
            batches.push(StaticEntityMeshBatch {
                texture: part.base_color_texture.clone(),
                vertices,
            });
        }
    }
    batches
}

fn resolve_nodes(entity: &RuntimeEntity, package: &EntityPackage) -> Vec<ResolvedNode> {
    let node_count = package.appearance.nodes.len();
    let mut cache = vec![None; node_count];
    let mut visiting = vec![false; node_count];

    for index in 0..node_count {
        resolve_node(index, entity, package, &mut cache, &mut visiting);
    }

    cache
        .into_iter()
        .map(|node| {
            node.unwrap_or(ResolvedNode {
                matrix: Mat4::IDENTITY,
                visible: false,
            })
        })
        .collect()
}

fn resolve_node(
    index: usize,
    entity: &RuntimeEntity,
    package: &EntityPackage,
    cache: &mut [Option<ResolvedNode>],
    visiting: &mut [bool],
) -> ResolvedNode {
    if let Some(node) = cache[index] {
        return node;
    }

    let definition = &package.appearance.nodes[index];
    let local = evaluate_local_transform(entity, package, definition.name.as_str());

    if visiting[index] {
        let node = ResolvedNode {
            matrix: local.to_matrix(),
            visible: node_is_visible(entity, &definition.name),
        };
        cache[index] = Some(node);
        return node;
    }

    visiting[index] = true;

    let parent_pose = definition.parent.as_deref().and_then(|parent_name| {
        package
            .appearance
            .nodes
            .iter()
            .position(|node| node.name.eq_ignore_ascii_case(parent_name))
            .map(|parent_index| resolve_node(parent_index, entity, package, cache, visiting))
    });

    visiting[index] = false;

    let matrix = match parent_pose {
        Some(parent) => parent.matrix * local.to_matrix(),
        None => local.to_matrix(),
    };

    let visible = node_is_visible(entity, &definition.name)
        && parent_pose.map(|parent| parent.visible).unwrap_or(true);

    let resolved = ResolvedNode { matrix, visible };
    cache[index] = Some(resolved);
    resolved
}

fn evaluate_local_transform(
    entity: &RuntimeEntity,
    package: &EntityPackage,
    node_name: &str,
) -> LocalTransform {
    let Some(node) = package
        .appearance
        .nodes
        .iter()
        .find(|node| node.name.eq_ignore_ascii_case(node_name))
    else {
        return LocalTransform::default();
    };

    let mut translation = node.translation;
    let mut rotation = quat_from_array(node.rotation);
    let mut scale = node.scale;

    let Some(clip_name) = entity.animation.current_clip.as_deref() else {
        return LocalTransform {
            translation,
            rotation,
            scale,
        };
    };

    let Some(clip) = package.animation_named(clip_name) else {
        return LocalTransform {
            translation,
            rotation,
            scale,
        };
    };

    let Some(track) = clip
        .tracks
        .iter()
        .find(|track| track.node.eq_ignore_ascii_case(node_name))
    else {
        return LocalTransform {
            translation,
            rotation,
            scale,
        };
    };

    translation = sample_vec3(&track.translation, entity.animation.time, translation);
    rotation = sample_quat(&track.rotation, entity.animation.time, rotation);
    scale = sample_vec3(&track.scale, entity.animation.time, scale);

    LocalTransform {
        translation,
        rotation,
        scale,
    }
}

#[derive(Clone, Copy)]
struct LocalTransform {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl Default for LocalTransform {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

impl LocalTransform {
    fn to_matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

fn node_is_visible(entity: &RuntimeEntity, name: &str) -> bool {
    entity
        .appearance
        .node_visibility
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, visible)| *visible)
        .unwrap_or(true)
}

fn material_color(entity: &RuntimeEntity, package: &EntityPackage, name: &str) -> [f32; 4] {
    if let Some((_, color)) = entity
        .appearance
        .material_overrides
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
    {
        return *color;
    }

    if let Some((_, color)) = package
        .appearance
        .materials
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
    {
        return *color;
    }

    [1.0, 0.0, 1.0, 1.0]
}

fn when_matches(entity: &RuntimeEntity, expression: Option<&str>) -> bool {
    let Some(expression) = expression else {
        return true;
    };

    let expression = expression.trim();

    if expression.is_empty() {
        return true;
    }

    let (negated, condition) = if let Some(rest) = expression.strip_prefix('!') {
        (true, rest.trim())
    } else {
        (false, expression)
    };

    let lower = condition.to_ascii_lowercase();

    let result = match lower.as_str() {
        "visible" => entity.visible,
        "enabled" => entity.enabled,
        "grounded" => entity.physics.grounded,
        "not_grounded" => !entity.physics.grounded,
        "moving" => entity.physics.velocity.length_squared() > 0.0001,
        "not_moving" => entity.physics.velocity.length_squared() <= 0.0001,
        "anchored" => entity.physics.anchored,
        "dynamic" => !entity.physics.anchored,
        _ if lower.starts_with("attribute:") => {
            let name = condition["attribute:".len()..].trim();
            attribute_truthy(entity, name)
        }
        _ => true,
    };

    if negated { !result } else { result }
}

fn attribute_truthy(entity: &RuntimeEntity, name: &str) -> bool {
    let Some(value) = entity.attributes.get(name) else {
        return false;
    };

    match value {
        AttributeValue::Bool(value) => *value,
        AttributeValue::Number(value) => *value != 0.0,
        AttributeValue::String(value) => !value.is_empty(),
    }
}

fn sample_vec3(keyframes: &[EntityVec3Keyframe], time: f32, default: Vec3) -> Vec3 {
    if keyframes.is_empty() {
        return default;
    }

    if keyframes.len() == 1 || time <= keyframes[0].time {
        return Vec3::from_array(keyframes[0].value);
    }

    if let Some(last) = keyframes.last() {
        if time >= last.time {
            return Vec3::from_array(last.value);
        }
    }

    for pair in keyframes.windows(2) {
        let left = pair[0];
        let right = pair[1];

        if time >= left.time && time <= right.time {
            let duration = right.time - left.time;
            let blend = if duration > 0.0 {
                (time - left.time) / duration
            } else {
                0.0
            };

            return Vec3::from_array(left.value).lerp(Vec3::from_array(right.value), blend);
        }
    }

    default
}

fn sample_quat(keyframes: &[EntityQuaternionKeyframe], time: f32, default: Quat) -> Quat {
    if keyframes.is_empty() {
        return default;
    }

    if keyframes.len() == 1 || time <= keyframes[0].time {
        return quaternion_from_keyframe(&keyframes[0]);
    }

    if let Some(last) = keyframes.last() {
        if time >= last.time {
            return quaternion_from_keyframe(last);
        }
    }

    for pair in keyframes.windows(2) {
        let left = &pair[0];
        let right = &pair[1];

        if time >= left.time && time <= right.time {
            let duration = right.time - left.time;
            let blend = if duration > 0.0 {
                (time - left.time) / duration
            } else {
                0.0
            };

            return quaternion_from_keyframe(left)
                .slerp(quaternion_from_keyframe(right), blend)
                .normalize();
        }
    }

    default
}

fn quaternion_from_keyframe(keyframe: &EntityQuaternionKeyframe) -> Quat {
    if let Some(value) = keyframe.value {
        return quat_from_array(value);
    }

    if let Some(euler) = keyframe.euler {
        return Quat::from_euler(EulerRot::XYZ, euler[0], euler[1], euler[2]).normalize();
    }

    Quat::IDENTITY
}

fn quat_from_array(value: [f32; 4]) -> Quat {
    Quat::from_xyzw(value[0], value[1], value[2], value[3]).normalize()
}

fn append_geometry(
    vertices: &mut Vec<f32>,
    part: &EntityGeometryPart,
    matrix: &Mat4,
    color: [f32; 4],
) {
    match &part.primitive {
        EntityGeometryPrimitive::TriangleMesh {
            vertices: mesh_vertices,
        } => {
            let normal_matrix = matrix.inverse().transpose();
            for vertex in mesh_vertices {
                let transformed_position = matrix.transform_point3(vertex.position);
                let transformed_normal = normal_matrix
                    .transform_vector3(vertex.normal)
                    .normalize_or_zero();
                let vertex_color = [
                    vertex.color[0] * color[0],
                    vertex.color[1] * color[1],
                    vertex.color[2] * color[2],
                    vertex.color[3] * color[3],
                ];
                push_vertex(
                    vertices,
                    transformed_position,
                    transformed_normal,
                    vertex_color,
                );
            }
        }

        EntityGeometryPrimitive::Cube { min, max } => {
            add_cube(vertices, matrix, *min, *max, color);
        }

        EntityGeometryPrimitive::Sphere {
            center,
            radius,
            segments,
            rings,
        } => {
            add_ellipsoid(
                vertices, matrix, *center, *radius, *radius, *radius, *segments, *rings, color,
            );
        }

        EntityGeometryPrimitive::Cylinder {
            radius_x,
            radius_z,
            min_y,
            max_y,
            segments,
        } => {
            add_cylinder(
                vertices, matrix, *radius_x, *radius_z, *min_y, *max_y, *segments, color,
            );
        }

        EntityGeometryPrimitive::Ellipsoid {
            center,
            radius_x,
            radius_y,
            radius_z,
            segments,
            rings,
        } => {
            add_ellipsoid(
                vertices, matrix, *center, *radius_x, *radius_y, *radius_z, *segments, *rings,
                color,
            );
        }

        EntityGeometryPrimitive::TaperedFlame {
            radius_bottom,
            radius_top,
            min_y,
            max_y,
            segments,
        } => {
            add_tapered_flame(
                vertices,
                matrix,
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

fn add_cube(vertices: &mut Vec<f32>, matrix: &Mat4, min: Vec3, max: Vec3, color: [f32; 4]) {
    let faces = [
        (
            [min.x, min.y, min.z],
            [min.x, max.y, min.z],
            [max.x, max.y, min.z],
            [max.x, min.y, min.z],
            Vec3::NEG_Z,
        ),
        (
            [max.x, min.y, max.z],
            [max.x, max.y, max.z],
            [min.x, max.y, max.z],
            [min.x, min.y, max.z],
            Vec3::Z,
        ),
        (
            [min.x, min.y, max.z],
            [min.x, max.y, max.z],
            [min.x, max.y, min.z],
            [min.x, min.y, min.z],
            Vec3::NEG_X,
        ),
        (
            [max.x, min.y, min.z],
            [max.x, max.y, min.z],
            [max.x, max.y, max.z],
            [max.x, min.y, max.z],
            Vec3::X,
        ),
        (
            [min.x, max.y, min.z],
            [min.x, max.y, max.z],
            [max.x, max.y, max.z],
            [max.x, max.y, min.z],
            Vec3::Y,
        ),
        (
            [min.x, min.y, max.z],
            [min.x, min.y, min.z],
            [max.x, min.y, min.z],
            [max.x, min.y, max.z],
            Vec3::NEG_Y,
        ),
    ];

    for (p0, p1, p2, p3, normal) in faces {
        let transformed_normal = matrix.transform_vector3(normal).normalize();
        let points = [p0, p1, p2, p3].map(|point| matrix.transform_point3(Vec3::from_array(point)));

        push_vertex(vertices, points[0], transformed_normal, color);
        push_vertex(vertices, points[1], transformed_normal, color);
        push_vertex(vertices, points[2], transformed_normal, color);

        push_vertex(vertices, points[0], transformed_normal, color);
        push_vertex(vertices, points[2], transformed_normal, color);
        push_vertex(vertices, points[3], transformed_normal, color);
    }
}

fn add_cylinder(
    vertices: &mut Vec<f32>,
    matrix: &Mat4,
    radius_x: f32,
    radius_z: f32,
    min_y: f32,
    max_y: f32,
    segments: usize,
    color: [f32; 4],
) {
    if segments < 3 {
        return;
    }

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

        let p0 = bottom[i];
        let p1 = bottom[next];
        let p2 = top[next];
        let p3 = top[i];

        let radial = Vec3::new(p0.x / radius_x, 0.0, p0.z / radius_z).normalize();

        let normal = matrix.transform_vector3(radial).normalize();

        let tp0 = matrix.transform_point3(p0);
        let tp1 = matrix.transform_point3(p1);
        let tp2 = matrix.transform_point3(p2);
        let tp3 = matrix.transform_point3(p3);

        push_vertex(vertices, tp0, normal, color);
        push_vertex(vertices, tp2, normal, color);
        push_vertex(vertices, tp1, normal, color);

        push_vertex(vertices, tp0, normal, color);
        push_vertex(vertices, tp3, normal, color);
        push_vertex(vertices, tp2, normal, color);
    }

    let top_normal = matrix.transform_vector3(Vec3::Y).normalize();
    let top_center = matrix.transform_point3(Vec3::new(0.0, max_y, 0.0));

    for i in 0..segments {
        let next = (i + 1) % segments;
        let p0 = matrix.transform_point3(top[i]);
        let p1 = matrix.transform_point3(top[next]);

        push_vertex(vertices, top_center, top_normal, color);
        push_vertex(vertices, p1, top_normal, color);
        push_vertex(vertices, p0, top_normal, color);
    }

    let bottom_normal = matrix.transform_vector3(Vec3::NEG_Y).normalize();
    let bottom_center = matrix.transform_point3(Vec3::new(0.0, min_y, 0.0));

    for i in 0..segments {
        let next = (i + 1) % segments;
        let p0 = matrix.transform_point3(bottom[i]);
        let p1 = matrix.transform_point3(bottom[next]);

        push_vertex(vertices, bottom_center, bottom_normal, color);
        push_vertex(vertices, p0, bottom_normal, color);
        push_vertex(vertices, p1, bottom_normal, color);
    }
}

fn add_ellipsoid(
    vertices: &mut Vec<f32>,
    matrix: &Mat4,
    center: Vec3,
    radius_x: f32,
    radius_y: f32,
    radius_z: f32,
    segments: usize,
    rings: usize,
    color: [f32; 4],
) {
    if segments < 3 || rings < 2 {
        return;
    }

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

            let tp00 = matrix.transform_point3(p00);
            let tp01 = matrix.transform_point3(p01);
            let tp10 = matrix.transform_point3(p10);
            let tp11 = matrix.transform_point3(p11);

            let tn00 = matrix.transform_vector3(n00).normalize();
            let tn01 = matrix.transform_vector3(n01).normalize();
            let tn10 = matrix.transform_vector3(n10).normalize();
            let tn11 = matrix.transform_vector3(n11).normalize();

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
    matrix: &Mat4,
    radius_bottom: f32,
    radius_top: f32,
    min_y: f32,
    max_y: f32,
    segments: usize,
    color: [f32; 4],
) {
    if segments < 3 {
        return;
    }

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
        let normal = matrix.transform_vector3(local_normal).normalize();

        let p0 = matrix.transform_point3(bottom[i]);
        let p1 = matrix.transform_point3(bottom[next]);
        let p2 = matrix.transform_point3(top[next]);
        let p3 = matrix.transform_point3(top[i]);

        push_vertex(vertices, p0, normal, color);
        push_vertex(vertices, p2, normal, color);
        push_vertex(vertices, p1, normal, color);

        push_vertex(vertices, p0, normal, color);
        push_vertex(vertices, p3, normal, color);
        push_vertex(vertices, p2, normal, color);
    }

    let top_normal = matrix.transform_vector3(Vec3::Y).normalize();
    let top_center = matrix.transform_point3(Vec3::new(0.0, max_y, 0.0));

    for i in 0..segments {
        let next = (i + 1) % segments;
        let p0 = matrix.transform_point3(top[i]);
        let p1 = matrix.transform_point3(top[next]);

        push_vertex(vertices, top_center, top_normal, color);
        push_vertex(vertices, p1, top_normal, color);
        push_vertex(vertices, p0, top_normal, color);
    }

    if radius_bottom > 0.001 {
        let bottom_normal = matrix.transform_vector3(Vec3::NEG_Y).normalize();
        let bottom_center = matrix.transform_point3(Vec3::new(0.0, min_y, 0.0));

        for i in 0..segments {
            let next = (i + 1) % segments;
            let p0 = matrix.transform_point3(bottom[i]);
            let p1 = matrix.transform_point3(bottom[next]);

            push_vertex(vertices, bottom_center, bottom_normal, color);
            push_vertex(vertices, p0, bottom_normal, color);
            push_vertex(vertices, p1, bottom_normal, color);
        }
    }
}

fn push_vertex(buffer: &mut Vec<f32>, position: Vec3, normal: Vec3, color: [f32; 4]) {
    buffer.extend_from_slice(&[
        position.x, position.y, position.z, normal.x, normal.y, normal.z, color[0], color[1],
        color[2], color[3],
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entity::{
        EntityAnimationState, EntityAppearanceState, EntityId, EntityPhysicsState,
    };
    use crate::engine::entity_package::{
        EntityAnimationDefinition, EntityAppearanceDefinition, EntityMeshVertex, EntityPackage,
        EntityPackageMetadata, EntityPhysicsDefaults,
    };
    use glam::Vec2;
    use std::collections::HashMap;

    fn test_package() -> EntityPackage {
        let mut materials = HashMap::new();
        materials.insert("body".to_string(), [0.2, 0.4, 0.8, 1.0]);

        EntityPackage {
            metadata: EntityPackageMetadata {
                name: "Test".to_string(),
                version: 1,
                description: None,
            },
            appearance: EntityAppearanceDefinition {
                materials,
                nodes: vec![crate::engine::entity_package::EntityNodeDefinition {
                    name: "root".to_string(),
                    parent: None,
                    translation: Vec3::ZERO,
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    scale: Vec3::ONE,
                }],
                geometry: vec![EntityGeometryPart {
                    node: "root".to_string(),
                    material: "body".to_string(),
                    when: None,
                    base_color_texture: None,
                    primitive: EntityGeometryPrimitive::Cube {
                        min: Vec3::new(-0.5, -0.5, -0.5),
                        max: Vec3::new(0.5, 0.5, 0.5),
                    },
                }],
            },
            physics: EntityPhysicsDefaults::default(),
            animation: EntityAnimationDefinition::default(),
            character: None,
            triangle_mesh_bounds_cache: std::sync::OnceLock::new(),
        }
    }

    #[test]
    fn generic_entity_mesh_generates_vertices() {
        let package = test_package();

        let entity = RuntimeEntity {
            id: EntityId(1),
            name: "Test".to_string(),
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
            visible: true,
            enabled: true,
            package_name: None,
            appearance: EntityAppearanceState::default(),
            animation: EntityAnimationState::default(),
            attributes: std::collections::BTreeMap::new(),
            physics: EntityPhysicsState::default(),
        };

        let vertices = generate_entity_mesh_from_package(&entity, &package);

        assert_eq!(vertices.len() % ENTITY_VERTEX_FLOATS, 0);
        assert!(!vertices.is_empty());
    }

    #[test]
    fn immutable_entity_package_geometry_is_cacheable_until_runtime_overrides_exist() {
        let package = test_package();
        let mut entity = RuntimeEntity::new(EntityId(7), "Static");

        assert!(package_geometry_is_static(&package));
        assert!(entity_uses_static_package_geometry(&entity, &package));

        entity
            .appearance
            .material_overrides
            .insert("body".to_string(), [1.0, 0.0, 0.0, 1.0]);
        assert!(!entity_uses_static_package_geometry(&entity, &package));
    }

    #[test]
    fn imported_static_mesh_preserves_gltf_texture_coordinates() {
        let mut package = test_package();
        let expected = [
            Vec2::new(0.125, 0.75),
            Vec2::new(0.5, 0.625),
            Vec2::new(0.9, 0.8),
        ];
        package.appearance.geometry[0] = EntityGeometryPart {
            node: "root".to_string(),
            material: "body".to_string(),
            when: None,
            base_color_texture: Some(PathBuf::from("textures/colormap.png")),
            primitive: EntityGeometryPrimitive::TriangleMesh {
                vertices: expected
                    .iter()
                    .enumerate()
                    .map(|(index, tex_coord)| EntityMeshVertex {
                        position: Vec3::new(index as f32, 0.0, 0.0),
                        normal: Vec3::Y,
                        tex_coord: *tex_coord,
                        color: [1.0; 4],
                    })
                    .collect(),
            },
        };
        let entity = RuntimeEntity::new(EntityId(8), "Imported");

        let batches = generate_static_entity_mesh_batches(&entity, &package);

        assert_eq!(batches.len(), 1);
        for (vertex, expected_uv) in batches[0]
            .vertices
            .chunks_exact(TEXTURED_ENTITY_VERTEX_FLOATS)
            .zip(expected)
        {
            assert_eq!([vertex[10], vertex[11]], expected_uv.to_array());
        }
    }
}
