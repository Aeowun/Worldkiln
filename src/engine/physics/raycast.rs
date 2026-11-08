// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::actor::ActorSystem;
use crate::character::CharacterSystem;
use crate::engine::collision_geometry::{self, ColliderInstance, CollisionShape};
use crate::engine::entity::{EntityId, EntityManager};
use crate::scripting::value::HandleKind;
use crate::world::{
    HeightfieldData, SMOOTH_VOXEL_ISO_LEVEL, SmoothVoxelGrid, TerrainData, World, WorldCoord,
};
use glam::{EulerRot, Quat, Vec3};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainSurfaceHit {
    pub coord: WorldCoord,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

fn terrain_ray_aabb_distance(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let mut entry = 0.0_f32;
    let mut exit = MAX_RAY_DISTANCE;
    for (position, component, lower, upper) in [
        (origin.x, direction.x, min.x, max.x),
        (origin.y, direction.y, min.y, max.y),
        (origin.z, direction.z, min.z, max.z),
    ] {
        if component.abs() <= f32::EPSILON {
            if position < lower || position > upper {
                return None;
            }
            continue;
        }
        let mut near = (lower - position) / component;
        let mut far = (upper - position) / component;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        entry = entry.max(near);
        exit = exit.min(far);
        if entry > exit {
            return None;
        }
    }
    Some(entry)
}

pub fn raycast_terrain_from_ray_with_distance(
    near: Vec3,
    dir: Vec3,
    world: &World,
    max_distance: f32,
) -> Option<TerrainSurfaceHit> {
    if !near.is_finite() || !dir.is_finite() || !max_distance.is_finite() || max_distance < 0.0 {
        return None;
    }
    let result = world.terrain.effective()?;
    match &result.data {
        TerrainData::Heightfield(heightfield) => {
            raycast_heightfield_terrain(near, dir, heightfield, max_distance)
        }
        TerrainData::Voxel(grid) => raycast_voxel_terrain(near, dir, grid, max_distance),
    }
}

fn raycast_heightfield_terrain(
    near: Vec3,
    dir: Vec3,
    heightfield: &HeightfieldData,
    max_distance: f32,
) -> Option<TerrainSurfaceHit> {
    let min_x = heightfield.origin[0] as f32;
    let max_x = (heightfield.origin[0] + heightfield.size[0] as i32) as f32;
    let min_z = heightfield.origin[2] as f32;
    let max_z = (heightfield.origin[2] + heightfield.size[2] as i32) as f32;
    let min_y = heightfield.origin[1] as f32;
    let max_y = (heightfield.origin[1] + heightfield.size[1] as i32) as f32;

    let t_bounds = terrain_ray_aabb_distance(
        near,
        dir,
        Vec3::new(min_x, min_y, min_z),
        Vec3::new(max_x, max_y, max_z),
    )?;

    let step_size = 0.25;
    let max_distance = max_distance.min(t_bounds + 300.0);
    let mut t = t_bounds;
    let mut prev_t = t;
    let mut hit_t = None;

    while t <= max_distance {
        let p = near + dir * t;
        if p.x >= min_x && p.x <= max_x && p.z >= min_z && p.z <= max_z {
            if let Some(surf_y) = heightfield.surface_height_at(p.x, p.z) {
                if p.y <= surf_y {
                    let mut low = prev_t;
                    let mut high = t;
                    for _ in 0..10 {
                        let mid = (low + high) * 0.5;
                        let mid_p = near + dir * mid;
                        let mid_surf = heightfield
                            .surface_height_at(mid_p.x, mid_p.z)
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
        .surface_height_at(point.x + sample_eps, point.z)
        .unwrap_or(point.y);
    let h_nx = heightfield
        .surface_height_at(point.x - sample_eps, point.z)
        .unwrap_or(point.y);
    let h_pz = heightfield
        .surface_height_at(point.x, point.z + sample_eps)
        .unwrap_or(point.y);
    let h_nz = heightfield
        .surface_height_at(point.x, point.z - sample_eps)
        .unwrap_or(point.y);

    let normal_x = -(h_px - h_nx) / (2.0 * sample_eps);
    let normal_z = -(h_pz - h_nz) / (2.0 * sample_eps);
    let normal = Vec3::new(normal_x, 1.0, normal_z).normalize_or_zero();

    Some(TerrainSurfaceHit {
        coord,
        point,
        normal,
        distance: t_hit,
    })
}

fn raycast_voxel_terrain(
    near: Vec3,
    dir: Vec3,
    grid: &SmoothVoxelGrid,
    max_distance: f32,
) -> Option<TerrainSurfaceHit> {
    let (min, max) = grid.world_bounds();
    let min = Vec3::from_array(min);
    let max = Vec3::from_array(max);
    let entry = terrain_ray_aabb_distance(near, dir, min, max)?;
    let step = (grid.spacing_studs * 0.0625).clamp(0.03125, 0.25);
    let safe_step = |density: f32| {
        let distance = (density - SMOOTH_VOXEL_ISO_LEVEL).abs();
        (distance * grid.spacing_studs / 1.732_051 * 0.9)
            .max(step)
            .min(grid.spacing_studs)
    };
    let mut previous_t = (entry - step * 2.0).max(0.0);
    let mut previous_density = grid.density_at_world((near + dir * previous_t).to_array());
    let max_distance = max_distance.min(entry + (max - min).length() + 2.0);
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
            return Some(TerrainSurfaceHit {
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
        t += safe_step(density);
    }
    None
}

pub const MAX_RAY_DISTANCE: f32 = 10_000.0;
pub const MAX_QUERY_HITS: usize = 256;

#[derive(Clone, Debug)]
pub struct RayQuery {
    pub origin: Vec3,
    pub direction: Vec3,
    pub max_distance: f32,
    pub actors: bool,
    pub cells: bool,
    pub entities: bool,
    pub terrain: bool,
    pub ignore: HashSet<(HandleKind, u64)>,
    pub max_hits: usize,
}

impl RayQuery {
    pub fn new(origin: Vec3, direction: Vec3, max_distance: f32) -> Result<Self, String> {
        if !origin.is_finite() || !direction.is_finite() || direction.length_squared() < 1.0e-12 {
            return Err("ray query requires finite origin and nonzero direction".into());
        }
        if !max_distance.is_finite() || !(0.0..=MAX_RAY_DISTANCE).contains(&max_distance) {
            return Err(format!(
                "ray query distance must be within 0..={MAX_RAY_DISTANCE}"
            ));
        }
        Ok(Self {
            origin,
            direction: direction.normalize(),
            max_distance,
            actors: true,
            cells: true,
            entities: true,
            terrain: true,
            ignore: HashSet::new(),
            max_hits: MAX_QUERY_HITS,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    pub kind: &'static str,
    pub handle: Option<(HandleKind, u64)>,
    pub position: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

fn hit(
    ray: &RayQuery,
    distance: f32,
    normal: Vec3,
    kind: &'static str,
    handle: Option<(HandleKind, u64)>,
) -> RayHit {
    RayHit {
        kind,
        handle,
        position: ray.origin + ray.direction * distance,
        normal,
        distance,
    }
}

fn aabb(ray: &RayQuery, min: Vec3, max: Vec3) -> Option<(f32, Vec3)> {
    if !min.is_finite() || !max.is_finite() || min.cmpgt(max).any() {
        return None;
    }
    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    let mut face = -ray.direction;
    for axis in 0..3 {
        let o = ray.origin[axis];
        let d = ray.direction[axis];
        if d.abs() < 1.0e-8 {
            if o < min[axis] || o > max[axis] {
                return None;
            }
            continue;
        }
        let mut t1 = (min[axis] - o) / d;
        let mut t2 = (max[axis] - o) / d;
        let mut normal = Vec3::ZERO;
        normal[axis] = if d > 0.0 { -1.0 } else { 1.0 };
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        if t1 > entry {
            entry = t1;
            face = normal;
        }
        exit = exit.min(t2);
        if entry > exit {
            return None;
        }
    }
    if exit < 0.0 {
        return None;
    }
    let t = entry.max(0.0);
    (t <= ray.max_distance).then_some((t, if entry < 0.0 { -ray.direction } else { face }))
}

fn triangle(ray: &RayQuery, vertices: [Vec3; 3]) -> Option<(f32, Vec3)> {
    let [a, b, c] = vertices;
    let e1 = b - a;
    let e2 = c - a;
    let p = ray.direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1.0e-8 {
        return None;
    }
    let inv = det.recip();
    let tvec = ray.origin - a;
    let u = tvec.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = ray.direction.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    if t < 0.0 || t > ray.max_distance {
        return None;
    }
    let n = e1.cross(e2).normalize_or_zero();
    if n.length_squared() < 0.5 {
        return None;
    }
    Some((t, if n.dot(ray.direction) > 0.0 { -n } else { n }))
}

fn sphere(ray: &RayQuery, center: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    let oc = ray.origin - center;
    let b = oc.dot(ray.direction);
    let c = oc.length_squared() - radius * radius;
    if c <= 0.0 {
        return Some((0.0, -ray.direction));
    }
    let discriminant = b * b - c;
    if discriminant < 0.0 {
        return None;
    }
    let t = -b - discriminant.sqrt();
    if t < 0.0 || t > ray.max_distance {
        return None;
    }
    Some((
        t,
        (ray.origin + ray.direction * t - center).normalize_or_zero(),
    ))
}

fn capsule(ray: &RayQuery, start: Vec3, end: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let ba = end - start;
    let oa = ray.origin - start;
    let baba = ba.length_squared();
    if baba < 1.0e-10 {
        return sphere(ray, start, radius);
    }
    let closest = start + ba * (oa.dot(ba) / baba).clamp(0.0, 1.0);
    if ray.origin.distance_squared(closest) <= radius * radius {
        return Some((0.0, -ray.direction));
    }
    let bard = ba.dot(ray.direction);
    let baoa = ba.dot(oa);
    let rdoa = ray.direction.dot(oa);
    let a = baba - bard * bard;
    let b = baba * rdoa - baoa * bard;
    let c = baba * oa.length_squared() - baoa * baoa - radius * radius * baba;
    let mut best = None;
    if a > 1.0e-10 {
        let disc = b * b - a * c;
        if disc >= 0.0 {
            let t = (-b - disc.sqrt()) / a;
            let y = baoa + t * bard;
            if t >= 0.0 && t <= ray.max_distance && y > 0.0 && y < baba {
                let p = ray.origin + ray.direction * t;
                let n = (p - (start + ba * (y / baba))).normalize_or_zero();
                best = Some((t, n));
            }
        }
    }
    for endcap in [start, end] {
        if let Some(candidate) = sphere(ray, endcap, radius) {
            if best.is_none_or(|h: (f32, Vec3)| candidate.0 < h.0) {
                best = Some(candidate);
            }
        }
    }
    best
}

pub fn collider(
    ray: &RayQuery,
    collider: &ColliderInstance,
    position: Vec3,
) -> Option<(f32, Vec3)> {
    let (low, high) = collision_geometry::envelope(collider)?;
    aabb(ray, position + low, position + high)?;
    let rotation = Quat::from_euler(
        EulerRot::XYZ,
        collider.rotation.x,
        collider.rotation.y,
        collider.rotation.z,
    );
    let mut best: Option<(f32, Vec3)> = None;
    for shape in collider.shapes.iter() {
        let candidate = match shape {
            CollisionShape::Sphere { center, radius } => sphere(
                ray,
                position + rotation * (Vec3::from_array(*center) * collider.scale),
                radius.abs() * collider.scale.abs().max_element(),
            ),
            CollisionShape::Box { min, max } => {
                if collider.scale.abs().min_element() < 1.0e-8 {
                    continue;
                }
                let local_origin = rotation.conjugate() * (ray.origin - position) / collider.scale;
                let local_direction = rotation.conjugate() * ray.direction / collider.scale;
                let local_ray = RayQuery {
                    origin: local_origin,
                    direction: local_direction,
                    ..ray.clone()
                };
                aabb(&local_ray, Vec3::from_array(*min), Vec3::from_array(*max)).map(|(t, n)| {
                    let transformed = (rotation * (n / collider.scale)).normalize_or_zero();
                    (
                        t,
                        if transformed.dot(ray.direction) > 0.0 {
                            -transformed
                        } else {
                            transformed
                        },
                    )
                })
            }
            CollisionShape::Capsule { start, end, radius } => {
                let a = position + rotation * (Vec3::from_array(*start) * collider.scale);
                let b = position + rotation * (Vec3::from_array(*end) * collider.scale);
                capsule(ray, a, b, *radius * collider.scale.abs().max_element())
            }
            CollisionShape::Triangles { vertices } => {
                let mut nearest = None;
                for tri in vertices {
                    let points =
                        tri.map(|v| position + rotation * (Vec3::from_array(v) * collider.scale));
                    if let Some(candidate) = triangle(ray, points) {
                        if nearest.is_none_or(|h: (f32, Vec3)| candidate.0 < h.0) {
                            nearest = Some(candidate);
                        }
                    }
                }
                nearest
            }
        };
        if let Some(candidate) = candidate {
            if best.is_none_or(|h| candidate.0 < h.0) {
                best = Some(candidate);
            }
        }
    }
    best
}

pub fn query(
    world: &World,
    entities: &EntityManager,
    actors: Option<&ActorSystem>,
    characters: Option<&CharacterSystem>,
    ray: &RayQuery,
) -> Result<Vec<RayHit>, String> {
    if !ray.origin.is_finite()
        || !ray.direction.is_finite()
        || (ray.direction.length_squared() - 1.0).abs() > 1.0e-3
        || !ray.max_distance.is_finite()
        || !(0.0..=MAX_RAY_DISTANCE).contains(&ray.max_distance)
        || ray.max_hits == 0
        || ray.max_hits > MAX_QUERY_HITS
    {
        return Err("invalid ray query settings".into());
    }
    let mut hits = Vec::new();
    if ray.cells {
        let mut seen_cells = HashSet::new();
        for coord in world.iter_active_effective_coords() {
            let Some(cell) = world.get_effective_cell(coord) else {
                continue;
            };
            let key = (HandleKind::Cell, cell.id);
            if !seen_cells.insert(cell.id)
                || ray.ignore.contains(&key)
                || !world.is_cell_solid(coord)
            {
                continue;
            }
            let min = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                + world.get_visual_offset(coord);
            if let Some((distance, normal)) = aabb(ray, min, min + Vec3::ONE) {
                hits.push(hit(ray, distance, normal, "Cell", Some(key)));
            }
        }
        for source in world.iter_sources() {
            let cell = &source.cell;
            let key = (HandleKind::Cell, cell.id);
            if !seen_cells.insert(cell.id)
                || ray.ignore.contains(&key)
                || !cell.solid
                || world.get_effective_cell_by_id(cell.id).is_none()
            {
                continue;
            }
            let min = world.source_position(cell.id).unwrap_or(source.position);
            if let Some((distance, normal)) = aabb(ray, min, min + Vec3::ONE) {
                hits.push(hit(ray, distance, normal, "Cell", Some(key)));
            }
        }
    }
    if ray.actors || ray.entities {
        for entity in entities.iter() {
            if !entity.enabled
                || !entity.physics.enabled
                || !entity.physics.collision_enabled
                || !entity.physics.solid
            {
                continue;
            }
            let actor_id = actors.and_then(|system| system.script_actor_id_for_entity(entity.id));
            let key = if let Some(id) = actor_id {
                if !ray.actors {
                    continue;
                }
                (HandleKind::Actor, id)
            } else {
                if !ray.entities {
                    continue;
                }
                (HandleKind::Entity, entity.id.0)
            };
            if ray.ignore.contains(&key) || ray.ignore.contains(&(HandleKind::Entity, entity.id.0))
            {
                continue;
            }
            let Some(shape) = entity.physics.collider.as_ref() else {
                continue;
            };
            let position = characters
                .and_then(|system| system.get_character_for_entity(EntityId(entity.id.0)))
                .map_or(entity.position, |character| character.transform.position);
            if let Some((distance, normal)) = collider(ray, shape, position) {
                let kind = if actor_id.is_some() {
                    "Actor"
                } else {
                    "Entity"
                };
                hits.push(hit(ray, distance, normal, kind, Some(key)));
            }
        }
    }
    if ray.terrain {
        if let Some(surface) = raycast_terrain_from_ray_with_distance(
            ray.origin,
            ray.direction,
            world,
            ray.max_distance,
        ) {
            if surface.distance <= ray.max_distance {
                hits.push(hit(ray, surface.distance, surface.normal, "Terrain", None));
            }
        }
    }
    hits.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then_with(|| a.kind.cmp(b.kind))
            .then_with(|| a.handle.cmp(&b.handle))
    });
    hits.truncate(ray.max_hits);
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn ray_box_hits_exact_nearest_face_and_start_inside() {
        let box_shape = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-1.0; 3],
                max: [1.0; 3],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let cast = RayQuery::new(Vec3::new(0.0, 0.0, -5.0), Vec3::Z, 10.0).unwrap();
        let (t, n) = collider(&cast, &box_shape, Vec3::ZERO).unwrap();
        assert!((t - 4.0).abs() < 0.0001);
        assert_eq!(n, Vec3::NEG_Z);
        let inside = RayQuery::new(Vec3::ZERO, Vec3::Z, 10.0).unwrap();
        assert_eq!(collider(&inside, &box_shape, Vec3::ZERO).unwrap().0, 0.0);
    }

    #[test]
    fn rotated_collider_does_not_report_envelope_corner() {
        let shape = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-1.0, -0.2, -0.2],
                max: [1.0, 0.2, 0.2],
            }]),
            rotation: Vec3::new(0.0, std::f32::consts::FRAC_PI_4, 0.0),
            scale: Vec3::ONE,
        };
        let misses = RayQuery::new(Vec3::new(0.8, 0.0, 2.0), Vec3::NEG_Z, 1.7).unwrap();
        assert!(collider(&misses, &shape, Vec3::ZERO).is_none());
    }

    #[test]
    fn capsule_and_triangles_produce_surface_normals() {
        let ray = RayQuery::new(Vec3::new(0.0, 0.0, -4.0), Vec3::Z, 10.0).unwrap();
        let capsule_shape = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Capsule {
                start: [0.0, -1.0, 0.0],
                end: [0.0, 1.0, 0.0],
                radius: 0.5,
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        assert!((collider(&ray, &capsule_shape, Vec3::ZERO).unwrap().0 - 3.5).abs() < 0.001);
        let mesh = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Triangles {
                vertices: vec![[[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]]],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        assert!((collider(&ray, &mesh, Vec3::ZERO).unwrap().0 - 4.0).abs() < 0.001);
    }
}
