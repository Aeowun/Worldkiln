// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::editor::GridPlane;
use crate::renderer::camera::CameraController;
use crate::world::{CellType, World, WorldCoord};
use glam::{EulerRot, Quat, Vec3};

const PICK_FOV: f32 = 60.0_f32.to_radians();
const PICK_NEAR: f32 = 0.1;
const PICK_FAR: f32 = 1000.0;
const RAYCAST_MAX_DISTANCE: f32 = 200.0;

const ACTOR_FALLBACK_RADIUS: f32 = 0.5;
const ACTOR_FALLBACK_HEIGHT: f32 = 1.5;
const ACTOR_PICK_PADDING: f32 = 0.03;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SceneHit {
    Cell {
        coord: WorldCoord,
        normal: Vec3,
        distance: f32,
    },
    Actor {
        actor_id: u64,
        distance: f32,
    },
    Source {
        source_id: u64,
        distance: f32,
    },
}

pub use crate::engine::physics::raycast::TerrainSurfaceHit as SurfaceHit;

pub fn update_hover(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    anchor: WorldCoord,
) -> Option<WorldCoord> {
    let (near, dir) = screen_ray(mx, my, width, height, camera)?;

    let plane = super::grid::select_grid_plane(camera.get_position(), camera.target);

    let (normal, offset) = match plane {
        GridPlane::Xz => (Vec3::Y, anchor.y as f32),
        GridPlane::Yz => (Vec3::X, anchor.x as f32),
        GridPlane::Xy => (Vec3::Z, anchor.z as f32),
    };

    let denominator = dir.dot(normal);

    if denominator.abs() <= f32::EPSILON {
        return None;
    }

    let distance = (offset - near.dot(normal)) / denominator;

    if distance < 0.0 {
        return None;
    }

    let hit = near + dir * distance;

    let hit = match plane {
        GridPlane::Xz => Vec3::new(hit.x, offset, hit.z),
        GridPlane::Yz => Vec3::new(offset, hit.y, hit.z),
        GridPlane::Xy => Vec3::new(hit.x, hit.y, offset),
    };

    Some(WorldCoord::new(
        hit.x.floor() as i32,
        hit.y.floor() as i32,
        hit.z.floor() as i32,
    ))
}

pub fn raycast_scene(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
    include_markers: bool,
) -> Option<SceneHit> {
    let (near, dir) = screen_ray(mx, my, width, height, camera)?;

    let cell_hit = raycast_world(mx, my, width, height, camera, world, include_markers).and_then(
        |(coord, normal)| {
            ray_aabb_distance(
                near,
                dir,
                Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32),
                Vec3::new(
                    coord.x as f32 + 1.0,
                    coord.y as f32 + 1.0,
                    coord.z as f32 + 1.0,
                ),
            )
            .map(|distance| (coord, normal, distance))
        },
    );

    let mut hits = Vec::new();
    if let Some((coord, normal, distance)) = cell_hit {
        hits.push(SceneHit::Cell {
            coord,
            normal,
            distance,
        });
    }
    if let Some(actor) = raycast_actors_from_ray(near, dir, world) {
        hits.push(actor);
    }
    if include_markers {
        if let Some(source) = raycast_sources_from_ray(near, dir, world) {
            hits.push(source);
        }
    }
    hits.into_iter().min_by(|left, right| {
        scene_hit_distance(*left)
            .partial_cmp(&scene_hit_distance(*right))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn scene_hit_distance(hit: SceneHit) -> f32 {
    match hit {
        SceneHit::Cell { distance, .. }
        | SceneHit::Actor { distance, .. }
        | SceneHit::Source { distance, .. } => distance,
    }
}

fn raycast_sources_from_ray(near: Vec3, dir: Vec3, world: &World) -> Option<SceneHit> {
    world
        .iter_sources()
        .filter_map(|source| {
            let half_extent = Vec3::splat(0.35);
            let position = world
                .source_position(source.cell.id)
                .unwrap_or(source.position);
            ray_aabb_distance(near, dir, position - half_extent, position + half_extent)
                .map(|distance| (source.cell.id, distance))
        })
        .min_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(source_id, distance)| SceneHit::Source {
            source_id,
            distance,
        })
}

pub fn actor_intersects_cell(world: &World, coord: WorldCoord) -> bool {
    let cell_min = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32);

    let cell_max = cell_min + Vec3::ONE;

    world.actors.values().any(|actor| {
        if !actor.visible || !actor.enabled {
            return false;
        }

        let (actor_min, actor_max) = actor_bounds(actor);

        aabb_overlaps(actor_min, actor_max, cell_min, cell_max)
    })
}

pub fn raycast_world(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
    include_markers: bool,
) -> Option<(WorldCoord, Vec3)> {
    raycast_world_hit(mx, my, width, height, camera, world, include_markers)
        .map(|(coord, normal, _distance)| (coord, normal))
}

pub fn raycast_world_surface(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
    include_markers: bool,
) -> Option<SurfaceHit> {
    let (near, dir) = screen_ray(mx, my, width, height, camera)?;
    let (coord, normal, distance) =
        raycast_world_hit(mx, my, width, height, camera, world, include_markers)?;

    Some(SurfaceHit {
        coord,
        point: near + dir * distance,
        normal,
        distance,
    })
}

pub fn raycast_terrain_surface(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
) -> Option<SurfaceHit> {
    let (near, dir) = screen_ray(mx, my, width, height, camera)?;
    raycast_terrain_from_ray(near, dir, world)
}

pub fn raycast_sculpt_terrain_surface(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
) -> Option<SurfaceHit> {
    raycast_terrain_surface(mx, my, width, height, camera, world)
}

pub fn raycast_terrain_from_ray(near: Vec3, dir: Vec3, world: &World) -> Option<SurfaceHit> {
    raycast_terrain_from_ray_with_distance(near, dir, world, RAYCAST_MAX_DISTANCE)
}

pub fn raycast_terrain_from_ray_with_distance(
    near: Vec3,
    dir: Vec3,
    world: &World,
    max_distance: f32,
) -> Option<SurfaceHit> {
    crate::engine::physics::raycast::raycast_terrain_from_ray_with_distance(
        near,
        dir,
        world,
        max_distance,
    )
}

fn raycast_world_hit(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
    include_markers: bool,
) -> Option<(WorldCoord, Vec3, f32)> {
    let (near, dir) = screen_ray(mx, my, width, height, camera)?;

    let mut map_pos = WorldCoord::new(
        near.x.floor() as i32,
        near.y.floor() as i32,
        near.z.floor() as i32,
    );

    let delta_dist = Vec3::new(
        reciprocal_abs(dir.x),
        reciprocal_abs(dir.y),
        reciprocal_abs(dir.z),
    );

    let step = WorldCoord::new(
        if dir.x < 0.0 { -1 } else { 1 },
        if dir.y < 0.0 { -1 } else { 1 },
        if dir.z < 0.0 { -1 } else { 1 },
    );

    let mut side_dist = Vec3::new(
        initial_side_distance(near.x, map_pos.x, dir.x, delta_dist.x),
        initial_side_distance(near.y, map_pos.y, dir.y, delta_dist.y),
        initial_side_distance(near.z, map_pos.z, dir.z, delta_dist.z),
    );

    let mut distance: f32 = 0.0;
    let mut hit_normal = Vec3::ZERO;

    while distance < RAYCAST_MAX_DISTANCE {
        if let Some(cell) = world.get(map_pos) {
            let eligible = match cell.cell_type {
                CellType::Block | CellType::SpawnPoint => true,
                CellType::Light | CellType::AudioEmitter | CellType::ParticleEmitter => {
                    include_markers
                }
                _ => false,
            };

            if eligible {
                let selectable = world.is_cell_visible(map_pos) || include_markers;

                if selectable {
                    return Some((map_pos, hit_normal, distance.max(0.0)));
                }
            }
        }

        if side_dist.x < side_dist.y && side_dist.x < side_dist.z {
            distance = side_dist.x;
            side_dist.x += delta_dist.x;
            map_pos.x += step.x;

            hit_normal = Vec3::new(-step.x as f32, 0.0, 0.0);
        } else if side_dist.y < side_dist.z {
            distance = side_dist.y;
            side_dist.y += delta_dist.y;
            map_pos.y += step.y;

            hit_normal = Vec3::new(0.0, -step.y as f32, 0.0);
        } else {
            distance = side_dist.z;
            side_dist.z += delta_dist.z;
            map_pos.z += step.z;

            hit_normal = Vec3::new(0.0, 0.0, -step.z as f32);
        }
    }

    None
}

fn actor_render_meshes(actor: &crate::world::Actor) -> Vec<(Vec<f32>, usize)> {
    let Some(name) = actor.package.as_deref() else {
        return Vec::new();
    };
    if let Some(package) = crate::character::package::get_package(name) {
        let mut character =
            crate::character::Character::from_package(actor.id, Vec3::ZERO, package);
        crate::character::character::set_model_material(&mut character, &actor.attributes);
        return crate::character::character::generate_character_mesh_batches(&character)
            .into_iter()
            .map(|batch| {
                (
                    batch.vertices,
                    crate::engine::entity_visual::TEXTURED_ENTITY_VERTEX_FLOATS,
                )
            })
            .collect();
    }
    if let Some(package) = crate::engine::entity_package::get_package(name) {
        let mut entity = crate::engine::entity::RuntimeEntity::new(
            crate::engine::entity::EntityId(actor.id),
            &actor.identity,
        );
        entity.visible = actor.visible;
        entity.enabled = actor.enabled;
        entity.package_name = Some(name.to_owned());
        entity.attributes = actor.attributes.clone();
        return vec![(
            crate::engine::entity_visual::generate_entity_mesh_from_package(&entity, &package),
            crate::engine::entity_visual::ENTITY_VERTEX_FLOATS,
        )];
    }
    Vec::new()
}

fn ray_triangle_distance(origin: Vec3, direction: Vec3, v: [Vec3; 3]) -> Option<f32> {
    let e1 = v[1] - v[0];
    let e2 = v[2] - v[0];
    let p = direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = det.recip();
    let tvec = origin - v[0];
    let u = tvec.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = direction.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t.is_finite() && t >= 0.0 && t <= RAYCAST_MAX_DISTANCE).then_some(t)
}

fn actor_rendered_distance(
    near: Vec3,
    direction: Vec3,
    actor: &crate::world::Actor,
) -> Option<f32> {
    let rot = Quat::from_euler(
        EulerRot::XYZ,
        actor.rotation.x,
        actor.rotation.y,
        actor.rotation.z,
    );
    let model = glam::Mat4::from_scale_rotation_translation(actor.scale, rot, actor.position);
    if model.determinant().abs() < 1e-10 || !model.is_finite() {
        return None;
    }
    let inverse = model.inverse();
    let origin = inverse.transform_point3(near);
    let local_direction = inverse.transform_vector3(direction);
    let mut best: Option<f32> = None;
    for (mesh, stride) in actor_render_meshes(actor) {
        for triangle in mesh.chunks_exact(stride * 3) {
            let vertex = |i: usize| Vec3::new(triangle[i], triangle[i + 1], triangle[i + 2]);
            let vertices = [vertex(0), vertex(stride), vertex(stride * 2)];
            if let Some(distance) = ray_triangle_distance(origin, local_direction, vertices)
                && best.is_none_or(|prior| distance < prior)
            {
                best = Some(distance);
            }
        }
    }
    best
}

fn actor_rendered_local_bounds(actor: &crate::world::Actor) -> Option<(Vec3, Vec3)> {
    use std::hash::{Hash, Hasher};
    thread_local! {
        static BOUNDS: std::cell::RefCell<
            std::collections::HashMap<(u64, usize, u64), Option<(Vec3, Vec3)>>
        > = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let package_key = actor
        .package
        .as_deref()
        .and_then(|name| {
            crate::engine::entity_package::get_package(name)
                .map(|package| std::sync::Arc::as_ptr(&package) as usize)
                .or_else(|| {
                    crate::character::package::get_package(name)
                        .map(|package| std::sync::Arc::as_ptr(&package) as usize)
                })
        })
        .unwrap_or(0);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    actor.identity.hash(&mut hasher);
    actor.visible.hash(&mut hasher);
    actor.enabled.hash(&mut hasher);
    format!("{:?}", actor.attributes).hash(&mut hasher);
    let key = (actor.id, package_key, hasher.finish());
    if let Some(bounds) = BOUNDS.with(|cache| cache.borrow().get(&key).copied()) {
        return bounds;
    }

    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for (mesh, stride) in actor_render_meshes(actor) {
        for vertex in mesh.chunks_exact(stride) {
            let point = Vec3::new(vertex[0], vertex[1], vertex[2]);
            if point.is_finite() {
                min = min.min(point);
                max = max.max(point);
            }
        }
    }
    let bounds = if min.is_finite() && max.is_finite() {
        Some((min, max))
    } else {
        None
    };
    BOUNDS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() > 65536 {
            cache.clear();
        }
        cache.insert(key, bounds);
    });
    bounds
}

pub(crate) fn actor_visual_local_center(actor: &crate::world::Actor) -> Vec3 {
    actor_rendered_local_bounds(actor)
        .map(|(min, max)| (min + max) * 0.5)
        .unwrap_or(Vec3::ZERO)
}

pub(crate) fn actor_visual_world_pivot(
    position: Vec3,
    rotation: Vec3,
    scale: Vec3,
    local_center: Vec3,
) -> Vec3 {
    let q = Quat::from_euler(EulerRot::XYZ, rotation.x, rotation.y, rotation.z);
    position + q * (scale * local_center)
}

fn raycast_actors_from_ray(near: Vec3, dir: Vec3, world: &World) -> Option<SceneHit> {
    let mut closest: Option<(u64, f32)> = None;
    for actor in world.actors.values() {
        if !actor.visible || !actor.enabled {
            continue;
        }
        let (min, max) = actor_bounds(actor);
        if ray_aabb_distance(near, dir, min, max).is_none() {
            continue;
        }
        let Some(distance) = actor_rendered_distance(near, dir, actor) else {
            continue;
        };
        if closest.is_none_or(|(_, prior)| distance < prior) {
            closest = Some((actor.id, distance));
        }
    }
    closest.map(|(actor_id, distance)| SceneHit::Actor { actor_id, distance })
}

pub(crate) fn actor_bounds(actor: &crate::world::Actor) -> (Vec3, Vec3) {
    if let Some((local_min, local_max)) = actor_rendered_local_bounds(actor) {
        return transform_local_bounds(
            local_min,
            local_max,
            actor.position,
            actor.rotation,
            actor.scale,
        );
    }

    let (base_radius, base_height) = actor
        .package
        .as_deref()
        .and_then(crate::character::package::get_package)
        .map(|package| (package.collision.radius, package.collision.height))
        .unwrap_or((ACTOR_FALLBACK_RADIUS, ACTOR_FALLBACK_HEIGHT));

    let scale_x = actor.scale.x.abs().max(0.001);
    let scale_y = actor.scale.y.abs().max(0.001);
    let scale_z = actor.scale.z.abs().max(0.001);

    let radius_x = base_radius.max(0.05) * scale_x + ACTOR_PICK_PADDING;

    let radius_z = base_radius.max(0.05) * scale_z + ACTOR_PICK_PADDING;

    let height = base_height.max(0.05) * scale_y + ACTOR_PICK_PADDING * 2.0;

    (
        Vec3::new(
            actor.position.x - radius_x,
            actor.position.y - ACTOR_PICK_PADDING,
            actor.position.z - radius_z,
        ),
        Vec3::new(
            actor.position.x + radius_x,
            actor.position.y + height,
            actor.position.z + radius_z,
        ),
    )
}

fn local_mesh_bounds(vertices: &[f32]) -> Option<(Vec3, Vec3)> {
    const VERTEX_FLOATS: usize = 10;

    if vertices.len() < VERTEX_FLOATS {
        return None;
    }

    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut found = false;

    for vertex in vertices.chunks_exact(VERTEX_FLOATS) {
        let position = Vec3::new(vertex[0], vertex[1], vertex[2]);

        if !position.is_finite() {
            continue;
        }

        min = min.min(position);
        max = max.max(position);
        found = true;
    }

    if found { Some((min, max)) } else { None }
}

fn transform_local_bounds(
    min: Vec3,
    max: Vec3,
    position: Vec3,
    rotation: Vec3,
    scale: Vec3,
) -> (Vec3, Vec3) {
    let rotation = Quat::from_euler(EulerRot::XYZ, rotation.x, rotation.y, rotation.z);

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

    let mut world_min = Vec3::splat(f32::MAX);
    let mut world_max = Vec3::splat(f32::MIN);

    for corner in corners {
        let scaled = corner * scale;

        let transformed = position + rotation * scaled;

        world_min = world_min.min(transformed);
        world_max = world_max.max(transformed);
    }

    world_min -= Vec3::splat(ACTOR_PICK_PADDING);
    world_max += Vec3::splat(ACTOR_PICK_PADDING);

    (world_min, world_max)
}

fn aabb_overlaps(min_a: Vec3, max_a: Vec3, min_b: Vec3, max_b: Vec3) -> bool {
    min_a.x < max_b.x
        && max_a.x > min_b.x
        && min_a.y < max_b.y
        && max_a.y > min_b.y
        && min_a.z < max_b.z
        && max_a.z > min_b.z
}

fn ray_aabb_distance(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut t_min: f32 = 0.0;
    let mut t_max: f32 = RAYCAST_MAX_DISTANCE;

    let axes = [
        (origin.x, direction.x, min.x, max.x),
        (origin.y, direction.y, min.y, max.y),
        (origin.z, direction.z, min.z, max.z),
    ];

    for (ray_origin, ray_direction, box_min, box_max) in axes {
        if ray_direction.abs() <= f32::EPSILON {
            if ray_origin < box_min || ray_origin > box_max {
                return None;
            }

            continue;
        }

        let inverse_direction = 1.0 / ray_direction;

        let mut t1 = (box_min - ray_origin) * inverse_direction;

        let mut t2 = (box_max - ray_origin) * inverse_direction;

        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }

        t_min = t_min.max(t1);
        t_max = t_max.min(t2);

        if t_min > t_max {
            return None;
        }
    }

    if t_max < 0.0 {
        return None;
    }

    Some(t_min.max(0.0))
}

pub(crate) fn screen_ray(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
) -> Option<(Vec3, Vec3)> {
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let view = camera.get_view_matrix();

    let projection =
        glam::camera::rh::proj::opengl::perspective(PICK_FOV, width / height, PICK_NEAR, PICK_FAR);

    let normalized_x = (2.0 * mx) / width - 1.0;

    let normalized_y = 1.0 - (2.0 * my) / height;

    let inverse_view_projection = (projection * view).inverse();

    let near = inverse_view_projection.project_point3(Vec3::new(normalized_x, normalized_y, -1.0));

    let far = inverse_view_projection.project_point3(Vec3::new(normalized_x, normalized_y, 1.0));

    let direction = far - near;

    if direction.length_squared() <= f32::EPSILON {
        return None;
    }

    Some((near, direction.normalize()))
}

fn reciprocal_abs(value: f32) -> f32 {
    if value.abs() <= f32::EPSILON {
        f32::INFINITY
    } else {
        1.0 / value.abs()
    }
}

fn initial_side_distance(
    coordinate: f32,
    cell_coordinate: i32,
    direction: f32,
    delta_distance: f32,
) -> f32 {
    if direction < 0.0 {
        (coordinate - cell_coordinate as f32) * delta_distance
    } else {
        (cell_coordinate as f32 + 1.0 - coordinate) * delta_distance
    }
}

#[cfg(test)]
mod surface_hit_tests {
    use super::*;

    #[test]
    fn rendered_actor_faces_ignore_empty_bounds_even_when_collision_disabled() {
        let near = Vec3::new(0.0, 0.0, 0.0);
        let ray = Vec3::Z;
        let missed_triangle = [
            Vec3::new(4.0, -1.0, 2.0),
            Vec3::new(6.0, -1.0, 2.0),
            Vec3::new(5.0, 1.0, 2.0),
        ];
        assert!(
            ray_aabb_distance(
                near,
                ray,
                Vec3::new(-5.0, -5.0, 1.0),
                Vec3::new(6.0, 5.0, 3.0)
            )
            .is_some()
        );
        assert!(ray_triangle_distance(near, ray, missed_triangle).is_none());
        let target = [
            Vec3::new(-1.0, -1.0, 4.0),
            Vec3::new(1.0, -1.0, 4.0),
            Vec3::new(0.0, 1.0, 4.0),
        ];
        assert!((ray_triangle_distance(near, ray, target).unwrap() - 4.0).abs() < 1e-5);
    }

    #[test]
    fn voxel_surface_hit_preserves_floating_point_contact() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, crate::world::CellType::Block);
        let mut camera = CameraController::new();
        camera.target = Vec3::splat(0.5);

        let hit = raycast_world_surface(400.0, 300.0, 800.0, 600.0, &camera, &world, false)
            .expect("center ray should hit the authored voxel surface");

        assert_eq!(hit.coord, coord);
        assert!(hit.point.is_finite());
        assert!(hit.point.cmpge(Vec3::ZERO).all());
        assert!(hit.point.cmple(Vec3::ONE).all());
        assert!(
            [hit.point.x, hit.point.y, hit.point.z]
                .iter()
                .any(|value| value.fract().abs() > 0.0001),
            "surface contact was unexpectedly quantized: {:?}",
            hit.point
        );
    }

    #[test]
    fn source_marker_picking_uses_exact_freeform_position() {
        let mut world = World::new();
        let position = Vec3::new(2.5, 1.2, 55.0);
        let id = world.add_source(crate::world::CellType::AudioEmitter, position);

        let hit = raycast_sources_from_ray(
            Vec3::new(position.x, position.y, position.z - 10.0),
            Vec3::Z,
            &world,
        );

        assert!(matches!(hit, Some(SceneHit::Source { source_id, .. }) if source_id == id));
    }

    #[test]
    fn terrain_surface_picking_hits_generated_terrain() {
        let mut world = World::new();
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 32, 16],
            })
            .unwrap();

        let hit = raycast_terrain_from_ray(Vec3::new(8.0, 50.0, 8.0), -Vec3::Y, &world)
            .expect("ray pointing straight down should hit generated terrain");

        assert!(hit.point.is_finite());
        assert!(hit.distance > 0.0);
        assert!((0..16).contains(&hit.coord.x));
        assert!((0..16).contains(&hit.coord.z));
    }

    #[test]
    fn voxel_terrain_picking_hits_true_volumetric_side_surface() {
        let mut world = World::new();
        world
            .set_runtime_voxel_terrain(
                [0, 0, 0],
                [1, 1, 1],
                vec!["rock".into()],
                vec![crate::world::SmoothVoxel::from_unit_occupancy(0, 1.0)],
            )
            .unwrap();

        let hit = raycast_terrain_from_ray(Vec3::new(-10.0, 2.0, 2.0), Vec3::X, &world)
            .expect("side ray should hit the voxel occupancy contour");
        assert!(
            hit.point.x.abs() < 0.05,
            "unexpected side hit: {:?}",
            hit.point
        );
        assert!(
            hit.normal.x < -0.9,
            "unexpected side normal: {:?}",
            hit.normal
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneGrabKind {
    Slide,
    Normal,
}

#[derive(Clone, Copy, Debug)]
pub struct PlaneDrag {
    pub plane: GridPlane,
    pub kind: PlaneGrabKind,
    pub start_anchor: WorldCoord,
    start_mouse: glam::Vec2,
    start_hit: Vec3,
    normal_screen: glam::Vec2,
    normal_pixels_per_unit: f32,
}

fn plane_normal(plane: GridPlane) -> Vec3 {
    match plane {
        GridPlane::Xz => Vec3::Y,
        GridPlane::Yz => Vec3::X,
        GridPlane::Xy => Vec3::Z,
    }
}

fn ray_plane_hit(
    mouse: glam::Vec2,
    size: glam::Vec2,
    camera: &CameraController,
    plane: GridPlane,
    anchor: Vec3,
) -> Option<Vec3> {
    let (origin, dir) = screen_ray(mouse.x, mouse.y, size.x, size.y, camera)?;
    let normal = plane_normal(plane);
    let denominator = dir.dot(normal);
    if denominator.abs() < 0.00001 {
        return None;
    }
    let t = (anchor - origin).dot(normal) / denominator;
    (t >= 0.0 && t.is_finite()).then_some(origin + dir * t)
}

fn project_handle(
    camera: &CameraController,
    point: Vec3,
    viewport: glam::Vec2,
) -> Option<glam::Vec2> {
    if viewport.x <= 0.0 || viewport.y <= 0.0 {
        return None;
    }
    let projection = glam::camera::rh::proj::opengl::perspective(
        PICK_FOV,
        viewport.x / viewport.y,
        PICK_NEAR,
        PICK_FAR,
    );
    let clip = projection * camera.get_view_matrix() * point.extend(1.0);
    if clip.w <= 0.0 || !clip.is_finite() {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if !ndc.is_finite() {
        return None;
    }
    Some(glam::Vec2::new(
        (ndc.x + 1.0) * viewport.x * 0.5,
        (1.0 - ndc.y) * viewport.y * 0.5,
    ))
}

pub fn plane_handle_positions(
    camera: &CameraController,
    anchor: WorldCoord,
    plane: GridPlane,
    size: glam::Vec2,
    ppp: f32,
) -> Option<(glam::Vec2, glam::Vec2, glam::Vec2, f32)> {
    let origin = Vec3::new(anchor.x as f32, anchor.y as f32, anchor.z as f32);
    let globe = project_handle(camera, origin, size)?;
    let projected_normal = project_handle(camera, origin + plane_normal(plane), size)
        .map(|tip| tip - globe)
        .unwrap_or(glam::Vec2::ZERO);
    let direction = if projected_normal.length_squared() > 16.0 {
        projected_normal.normalize()
    } else {
        glam::Vec2::new(0.0, -1.0)
    };
    let scale = if projected_normal.length() > 4.0 {
        projected_normal.length()
    } else {
        24.0 * ppp.max(0.5)
    };
    let tip = globe + direction * 30.0 * ppp.max(0.5);
    Some((globe, tip, direction, scale))
}

pub fn plane_handle_hit(
    mouse: glam::Vec2,
    globe: glam::Vec2,
    tip: glam::Vec2,
    ppp: f32,
) -> Option<PlaneGrabKind> {
    let radius = 13.0 * ppp.max(0.5);
    if mouse.distance(tip) <= radius {
        Some(PlaneGrabKind::Normal)
    } else if mouse.distance(globe) <= radius {
        Some(PlaneGrabKind::Slide)
    } else {
        None
    }
}

pub fn begin_plane_drag(
    mouse: glam::Vec2,
    size: glam::Vec2,
    ppp: f32,
    camera: &CameraController,
    anchor: WorldCoord,
) -> Option<PlaneDrag> {
    let plane = super::grid::select_grid_plane(camera.get_position(), camera.target);
    let (center, tip, direction, pixel_unit) =
        plane_handle_positions(camera, anchor, plane, size, ppp)?;
    let kind = plane_handle_hit(mouse, center, tip, ppp)?;
    let point = Vec3::new(anchor.x as f32, anchor.y as f32, anchor.z as f32);
    let hit = if kind == PlaneGrabKind::Slide {
        ray_plane_hit(mouse, size, camera, plane, point)?
    } else {
        point
    };
    Some(PlaneDrag {
        plane,
        kind,
        start_anchor: anchor,
        start_mouse: mouse,
        start_hit: hit,
        normal_screen: direction,
        normal_pixels_per_unit: pixel_unit.max(1.0),
    })
}

pub fn update_plane_drag(
    drag: &PlaneDrag,
    mouse: glam::Vec2,
    size: glam::Vec2,
    camera: &CameraController,
) -> Option<WorldCoord> {
    let start = drag.start_anchor;
    let mut point = Vec3::new(start.x as f32, start.y as f32, start.z as f32);
    match drag.kind {
        PlaneGrabKind::Slide => {
            let hit = ray_plane_hit(mouse, size, camera, drag.plane, point)?;
            let normal = plane_normal(drag.plane);
            let delta = hit - drag.start_hit;
            point += delta - normal * delta.dot(normal);
        }
        PlaneGrabKind::Normal => {
            let delta_pixels = (mouse - drag.start_mouse).dot(drag.normal_screen);
            let units = (delta_pixels / drag.normal_pixels_per_unit).round();
            point += plane_normal(drag.plane) * units;
        }
    }
    if !point.is_finite() {
        return None;
    }
    Some(WorldCoord::new(
        point.x.round() as i32,
        point.y.round() as i32,
        point.z.round() as i32,
    ))
}

#[cfg(test)]
mod build_plane_grab_tests {
    use super::*;
    #[test]
    fn planar_projection_never_changes_locked_normal() {
        let start = WorldCoord::new(2, 3, 4);
        let drag = PlaneDrag {
            plane: GridPlane::Xz,
            kind: PlaneGrabKind::Normal,
            start_anchor: start,
            start_mouse: glam::Vec2::ZERO,
            start_hit: Vec3::ZERO,
            normal_screen: glam::Vec2::new(0.0, -1.0),
            normal_pixels_per_unit: 20.0,
        };
        let camera = CameraController::new();
        let result = update_plane_drag(
            &drag,
            glam::Vec2::new(0.0, -40.0),
            glam::Vec2::new(1280.0, 720.0),
            &camera,
        )
        .unwrap();
        assert_eq!(result, WorldCoord::new(2, 5, 4));
    }
    #[test]
    fn plane_handle_hit_prioritizes_tip() {
        let c = glam::Vec2::new(10.0, 10.0);
        let tip = glam::Vec2::new(10.0, -20.0);
        assert_eq!(
            plane_handle_hit(tip, c, tip, 1.0),
            Some(PlaneGrabKind::Normal)
        );
        assert_eq!(plane_handle_hit(c, c, tip, 1.0), Some(PlaneGrabKind::Slide));
        assert_eq!(
            plane_handle_hit(glam::Vec2::new(90.0, 90.0), c, tip, 1.0),
            None
        );
    }
}

#[cfg(test)]
mod build_plane_axis_regressions {
    use super::*;
    #[test]
    fn build_plane_slides_on_all_camera_selected_axes_without_normal_drift() {
        let size = glam::Vec2::new(1280.0, 720.0);
        let anchor = WorldCoord::new(0, 0, 0);
        for (yaw, pitch, expected_plane, target) in [
            (
                0.0,
                80.0_f32.to_radians(),
                GridPlane::Xz,
                Vec3::new(2.0, 0.0, 1.0),
            ),
            (
                90.0_f32.to_radians(),
                0.0,
                GridPlane::Yz,
                Vec3::new(0.0, 1.0, 2.0),
            ),
            (0.0, 0.0, GridPlane::Xy, Vec3::new(1.0, 2.0, 0.0)),
        ] {
            let mut camera = CameraController::new();
            camera.yaw = yaw;
            camera.pitch = pitch;
            assert_eq!(
                super::super::grid::select_grid_plane(camera.get_position(), camera.target),
                expected_plane
            );
            let start = project_handle(&camera, Vec3::ZERO, size).unwrap();
            let end = project_handle(&camera, target, size).unwrap();
            let drag = begin_plane_drag(start, size, 1.0, &camera, anchor).unwrap();
            assert_eq!(drag.kind, PlaneGrabKind::Slide);
            assert_eq!(drag.plane, expected_plane);
            let actual = update_plane_drag(&drag, end, size, &camera).unwrap();
            assert_eq!(
                actual,
                WorldCoord::new(target.x as i32, target.y as i32, target.z as i32)
            );
            assert_eq!(update_plane_drag(&drag, start, size, &camera), Some(anchor));
        }
    }
    #[test]
    fn depth_handle_keeps_only_normal_axis_and_uses_frozen_plane() {
        let camera = CameraController::new();
        let size = glam::Vec2::new(1280.0, 720.0);
        let anchor = WorldCoord::new(5, -2, 9);
        let plane = super::super::grid::select_grid_plane(camera.get_position(), camera.target);
        let (_, tip, direction, scale) =
            plane_handle_positions(&camera, anchor, plane, size, 1.0).unwrap();
        let drag = begin_plane_drag(tip, size, 1.0, &camera, anchor).unwrap();
        assert_eq!(drag.kind, PlaneGrabKind::Normal);
        assert_eq!(drag.plane, plane);
        let moved = update_plane_drag(&drag, tip + direction * scale * 3.0, size, &camera).unwrap();
        let offset = Vec3::new(
            (moved.x - anchor.x) as f32,
            (moved.y - anchor.y) as f32,
            (moved.z - anchor.z) as f32,
        );
        assert_eq!(offset, plane_normal(plane) * 3.0);
    }
}

#[cfg(test)]
mod generated_layer_brush_targeting_regression {
    use super::*;
    #[test]
    fn generated_plains_has_brush_hit_two_studs_above_surface() {
        use crate::world::terrain::TerrainGenerationRequest;
        let mut world = World::new();
        let id = world
            .terrain
            .add_generation_layer(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [24, 40, 24],
            })
            .unwrap();
        {
            let layer = world
                .terrain
                .config
                .generation_layers
                .iter_mut()
                .find(|l| l.id == id)
                .unwrap();
            layer.settings.base_height = 12.0;
            layer.settings.relief = 0.0;
            layer.settings.water_level = 0.0;
        }
        world.terrain.generate_selected_layer(id).unwrap();
        let y = world
            .terrain
            .surface_height_at(12.5, 12.5)
            .expect("generated surface");
        let near = Vec3::new(12.5, y + 2.0, 12.5);
        let hit = raycast_terrain_from_ray(near, Vec3::NEG_Y, &world);
        println!(
            "GENERATED_LAYER_BRUSH_HIT near={:?} ground_y={} hit={:?}",
            near, y, hit
        );
        assert!(
            hit.is_some(),
            "2-stud sculpt ray must hit generated voxel terrain"
        );
        let hit = hit.unwrap();
        assert!(
            (hit.point.y - y).abs() < 2.5,
            "brush hit must match visible geometry"
        );
    }
}
