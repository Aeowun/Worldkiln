use crate::editor::GridPlane;
use crate::renderer::camera::CameraController;
use crate::world::{
    CellType, HeightfieldData, SMOOTH_VOXEL_ISO_LEVEL, SmoothVoxelGrid, TerrainData, TerrainMode,
    World, WorldCoord,
};
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceHit {
    pub coord: WorldCoord,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

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

/// Returns the nearest authored scene object under the cursor.
///
/// Actors and Cells share the same camera ray. A Cell that contains an Actor
/// does not automatically win merely because the voxel DDA entered that Cell
/// first. When the nearest Actor intersects the front-most Cell, the Actor is
/// treated as the editor hit.
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

/// Returns whether any authored Actor visually occupies the given voxel.
///
/// This is used by Build so voxel geometry cannot be created directly through
/// an existing Actor.
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

/// Returns the exact nearest voxel surface under the cursor.
///
/// Direct manipulation uses the floating-point hit rather than projecting
/// authored objects onto the editor grid plane.
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

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
/// Returns the nearest point where the camera ray hits effective authored Terrain.
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

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
/// Returns the Terrain surface only when it is the nearest authored surface
/// under the cursor. Terrain remains a native Terrain hit; authored Cells,
/// Actors, and freeform sources merely occlude a farther Terrain hit.
pub fn raycast_sculpt_terrain_surface(
    mx: f32,
    my: f32,
    width: f32,
    height: f32,
    camera: &CameraController,
    world: &World,
) -> Option<SurfaceHit> {
    let terrain_hit = raycast_terrain_surface(mx, my, width, height, camera, world)?;
    let scene_hit = raycast_scene(mx, my, width, height, camera, world, true);
    sculpt_terrain_hit_if_nearest(terrain_hit, scene_hit)
}

fn sculpt_terrain_hit_if_nearest(
    terrain_hit: SurfaceHit,
    scene_hit: Option<SceneHit>,
) -> Option<SurfaceHit> {
    const DISTANCE_EPSILON: f32 = 0.001;

    if scene_hit
        .is_some_and(|hit| scene_hit_distance(hit) <= terrain_hit.distance + DISTANCE_EPSILON)
    {
        None
    } else {
        Some(terrain_hit)
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:00 -05:00
pub fn raycast_terrain_from_ray(near: Vec3, dir: Vec3, world: &World) -> Option<SurfaceHit> {
    let result = world.terrain.effective()?;
    match &result.data {
        TerrainData::Heightfield(heightfield) => {
            raycast_heightfield_terrain(near, dir, heightfield, world.terrain.config.mode)
        }
        TerrainData::Voxel(grid) => raycast_voxel_terrain(near, dir, grid),
    }
}

fn raycast_heightfield_terrain(
    near: Vec3,
    dir: Vec3,
    heightfield: &HeightfieldData,
    mode: TerrainMode,
) -> Option<SurfaceHit> {
    let min_x = heightfield.origin[0] as f32;
    let max_x = (heightfield.origin[0] + heightfield.size[0] as i32) as f32;
    let min_z = heightfield.origin[2] as f32;
    let max_z = (heightfield.origin[2] + heightfield.size[2] as i32) as f32;
    let min_y = heightfield.origin[1] as f32;
    let max_y = (heightfield.origin[1] + heightfield.size[1] as i32) as f32;

    let t_bounds = ray_aabb_distance(
        near,
        dir,
        Vec3::new(min_x, min_y, min_z),
        Vec3::new(max_x, max_y, max_z),
    )?;

    let step_size = 0.25;
    let max_distance = RAYCAST_MAX_DISTANCE.min(t_bounds + 300.0);
    let mut t = t_bounds;
    let mut prev_t = t;
    let mut hit_t = None;

    while t <= max_distance {
        let p = near + dir * t;
        if p.x >= min_x && p.x <= max_x && p.z >= min_z && p.z <= max_z {
            if let Some(surf_y) = heightfield.surface_height_at(p.x, p.z, mode) {
                if p.y <= surf_y {
                    let mut low = prev_t;
                    let mut high = t;
                    for _ in 0..10 {
                        let mid = (low + high) * 0.5;
                        let mid_p = near + dir * mid;
                        let mid_surf = heightfield
                            .surface_height_at(mid_p.x, mid_p.z, mode)
                            .unwrap_or(min_y);
                        if mid_p.y <= mid_surf {
                            high = mid;
                        } else {
                            low = mid;
                        }
                    }
                    hit_t = Some((low + high) * 0.5);
                    break;
                }
            }
        } else if t > t_bounds
            && (p.x < min_x - 1.0 || p.x > max_x + 1.0 || p.z < min_z - 1.0 || p.z > max_z + 1.0)
        {
            break;
        }
        prev_t = t;
        t += step_size;
    }

    let t_hit = hit_t?;
    let point = near + dir * t_hit;
    let coord = WorldCoord::new(
        point.x.floor() as i32,
        point.y.floor() as i32,
        point.z.floor() as i32,
    );

    let sample_eps = 0.1;
    let h_px = heightfield
        .surface_height_at(point.x + sample_eps, point.z, mode)
        .unwrap_or(point.y);
    let h_nx = heightfield
        .surface_height_at(point.x - sample_eps, point.z, mode)
        .unwrap_or(point.y);
    let h_pz = heightfield
        .surface_height_at(point.x, point.z + sample_eps, mode)
        .unwrap_or(point.y);
    let h_nz = heightfield
        .surface_height_at(point.x, point.z - sample_eps, mode)
        .unwrap_or(point.y);

    let normal_x = -(h_px - h_nx) / (2.0 * sample_eps);
    let normal_z = -(h_pz - h_nz) / (2.0 * sample_eps);
    let normal = Vec3::new(normal_x, 1.0, normal_z).normalize_or_zero();

    Some(SurfaceHit {
        coord,
        point,
        normal,
        distance: t_hit,
    })
}

fn raycast_voxel_terrain(near: Vec3, dir: Vec3, grid: &SmoothVoxelGrid) -> Option<SurfaceHit> {
    let (min, max) = grid.world_bounds();
    let min = Vec3::from_array(min);
    let max = Vec3::from_array(max);
    let entry = ray_aabb_distance(near, dir, min, max)?;
    let step = (grid.spacing_studs * 0.0625).clamp(0.03125, 0.25);
    let mut previous_t = (entry - step * 2.0).max(0.0);
    let mut previous_density = grid.density_at_world((near + dir * previous_t).to_array());
    let max_distance = RAYCAST_MAX_DISTANCE.min(entry + (max - min).length() + 2.0);
    let mut t = entry;

    while t <= max_distance {
        let point = near + dir * t;
        let density = grid.density_at_world(point.to_array());
        if previous_density < SMOOTH_VOXEL_ISO_LEVEL && density >= SMOOTH_VOXEL_ISO_LEVEL {
            let mut outside_t = previous_t;
            let mut inside_t = t;
            for _ in 0..12 {
                let mid = (outside_t + inside_t) * 0.5;
                let mid_density = grid.density_at_world((near + dir * mid).to_array());
                if mid_density >= SMOOTH_VOXEL_ISO_LEVEL {
                    inside_t = mid;
                } else {
                    outside_t = mid;
                }
            }
            let hit_t = (outside_t + inside_t) * 0.5;
            let hit = near + dir * hit_t;
            let eps = (grid.spacing_studs * 0.025).clamp(0.01, 0.1);
            let gradient = Vec3::new(
                grid.density_at_world((hit + Vec3::X * eps).to_array())
                    - grid.density_at_world((hit - Vec3::X * eps).to_array()),
                grid.density_at_world((hit + Vec3::Y * eps).to_array())
                    - grid.density_at_world((hit - Vec3::Y * eps).to_array()),
                grid.density_at_world((hit + Vec3::Z * eps).to_array())
                    - grid.density_at_world((hit - Vec3::Z * eps).to_array()),
            );
            let mut normal = (-gradient).normalize_or_zero();
            if normal.length_squared() <= f32::EPSILON {
                normal = -dir;
            }
            return Some(SurfaceHit {
                coord: WorldCoord::new(
                    hit.x.floor() as i32,
                    hit.y.floor() as i32,
                    hit.z.floor() as i32,
                ),
                point: hit,
                normal,
                distance: hit_t,
            });
        }
        if point.x < min.x - grid.spacing_studs
            || point.x > max.x + grid.spacing_studs
            || point.y < min.y - grid.spacing_studs
            || point.y > max.y + grid.spacing_studs
            || point.z < min.z - grid.spacing_studs
            || point.z > max.z + grid.spacing_studs
        {
            if t > entry {
                break;
            }
        }
        previous_t = t;
        previous_density = density;
        t += step;
    }
    None
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

fn raycast_actors_from_ray(near: Vec3, dir: Vec3, world: &World) -> Option<SceneHit> {
    let mut closest: Option<(u64, f32)> = None;

    for actor in world.actors.values() {
        if !actor.visible || !actor.enabled {
            continue;
        }

        let (min, max) = actor_bounds(actor);

        let Some(distance) = ray_aabb_distance(near, dir, min, max) else {
            continue;
        };

        match closest {
            Some((_, best_distance)) if distance >= best_distance => {}

            _ => {
                closest = Some((actor.id, distance));
            }
        }
    }

    closest.map(|(actor_id, distance)| SceneHit::Actor { actor_id, distance })
}

/// Builds a world-space AABB for the actual rendered CharacterPackage
/// geometry when possible.
///
/// Physics collision is only used as a fallback. This matters for packages
/// such as the switch, where the rendered lever extends beyond its collision
/// height.
pub(crate) fn actor_bounds(actor: &crate::world::Actor) -> (Vec3, Vec3) {
    if let Some(package_name) = actor.package.as_deref() {
        if let Some(package) = crate::engine::entity_package::get_package(package_name)
            && let Some((local_min, local_max)) = package.triangle_mesh_bounds()
        {
            return transform_local_bounds(
                local_min,
                local_max,
                actor.position,
                actor.rotation,
                actor.scale,
            );
        }
        if let Some(package) = crate::character::package::get_package(package_name) {
            let character =
                crate::character::Character::from_package(actor.id, actor.position, package);

            let vertices = crate::character::generate_character_mesh(&character);

            if let Some((local_min, local_max)) = local_mesh_bounds(&vertices) {
                return transform_local_bounds(
                    local_min,
                    local_max,
                    actor.position,
                    actor.rotation,
                    actor.scale,
                );
            }
        }
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

/// Returns the normalized world-space ray used by editor picking.
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

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
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

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
    #[test]
    fn terrain_surface_picking_works_in_block_mode() {
        let mut world = World::new();
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 32, 16],
            })
            .unwrap();
        world
            .terrain
            .rebuild_representation(crate::world::TerrainMode::Block);

        let hit = raycast_terrain_from_ray(Vec3::new(8.0, 50.0, 8.0), -Vec3::Y, &world)
            .expect("block Terrain should remain directly pickable");

        assert!(hit.point.is_finite());
        assert!(hit.distance > 0.0);
        assert!((0..16).contains(&hit.coord.x));
        assert!((0..16).contains(&hit.coord.z));
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:20 -05:00
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

    #[test]
    fn sculpt_terrain_hit_yields_to_nearer_authored_scene_geometry() {
        let terrain_hit = SurfaceHit {
            coord: WorldCoord::new(4, 5, 6),
            point: Vec3::new(4.5, 5.0, 6.5),
            normal: Vec3::Y,
            distance: 10.0,
        };

        for scene_hit in [
            SceneHit::Cell {
                coord: WorldCoord::new(4, 7, 6),
                normal: -Vec3::Z,
                distance: 5.0,
            },
            SceneHit::Actor {
                actor_id: 7,
                distance: 5.0,
            },
            SceneHit::Source {
                source_id: 8,
                distance: 5.0,
            },
        ] {
            assert!(sculpt_terrain_hit_if_nearest(terrain_hit, Some(scene_hit)).is_none());
        }

        let terrain_in_front = sculpt_terrain_hit_if_nearest(
            terrain_hit,
            Some(SceneHit::Cell {
                coord: WorldCoord::new(4, 2, 6),
                normal: Vec3::Y,
                distance: 15.0,
            }),
        );
        assert_eq!(terrain_in_front, Some(terrain_hit));
        assert_eq!(
            sculpt_terrain_hit_if_nearest(terrain_hit, None),
            Some(terrain_hit)
        );
    }
}
