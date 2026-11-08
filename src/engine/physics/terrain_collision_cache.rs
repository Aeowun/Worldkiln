// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::physics_world::PhysicsWorld;
use super::triangle_collider::{CapsuleContact, TriangleCollider};
use crate::world::terrain::terrain_render_chunk_samples;
use crate::world::{TerrainData, World};
use glam::Vec3;
use std::sync::Arc;

pub(super) struct CachedTerrainTile {
    full_revision: u64,
    local_revision: u64,
    collider: Arc<TriangleCollider>,
}

impl PhysicsWorld {
    pub fn sweep_terrain(
        &self,
        world: &World,
        from: Vec3,
        to: Vec3,
        radius: f32,
        height: f32,
    ) -> Option<(Vec3, Vec<CapsuleContact>)> {
        self.sweep_terrain_impl(world, from, to, radius, height, None)
    }
    pub(crate) fn sweep_terrain_geometry(
        &self,
        world: &World,
        from: Vec3,
        to: Vec3,
        radius: f32,
        height: f32,
        body: &crate::engine::collision_geometry::ColliderInstance,
    ) -> Option<(Vec3, Vec<CapsuleContact>)> {
        self.sweep_terrain_impl(world, from, to, radius, height, Some(body))
    }
    fn sweep_terrain_impl(
        &self,
        world: &World,
        from: Vec3,
        to: Vec3,
        radius: f32,
        height: f32,
        body: Option<&crate::engine::collision_geometry::ColliderInstance>,
    ) -> Option<(Vec3, Vec<CapsuleContact>)> {
        if let Some(native) = self.native_terrain_collider() {
            return Some(if let Some(body) = body {
                TriangleCollider::sweep_geometry(&[native], from, to, body)
            } else {
                native.sweep_capsule(from, to, radius, height)
            });
        }
        let result = world.terrain.effective()?;
        let (spacing, columns, rows, origin_x, origin_z) = match &result.data {
            TerrainData::Voxel(grid) => (
                grid.spacing_studs,
                grid.size[0],
                grid.size[2],
                result.request.position[0] as f32,
                result.request.position[2] as f32,
            ),
            TerrainData::Heightfield(field) => (
                1.0,
                field.size[0],
                field.size[2],
                field.origin[0] as f32 + 0.5,
                field.origin[2] as f32 + 0.5,
            ),
        };
        if !spacing.is_finite()
            || spacing <= 0.0
            || columns < 2
            || rows < 2
            || !from.is_finite()
            || !to.is_finite()
        {
            return None;
        }
        let size = terrain_render_chunk_samples(spacing);
        let extent = size as f32 * spacing;
        let tile_count_x = columns.div_ceil(size);
        let tile_count_z = rows.div_ceil(size);
        let margin = radius.abs().max(0.05) + spacing;
        let min_x = from.x.min(to.x) - margin;
        let max_x = from.x.max(to.x) + margin;
        let min_z = from.z.min(to.z) - margin;
        let max_z = from.z.max(to.z) + margin;
        let x0 = ((min_x - origin_x) / extent).floor().max(0.0) as u32;
        let x1 = ((max_x - origin_x) / extent).floor().max(0.0) as u32;
        let z0 = ((min_z - origin_z) / extent).floor().max(0.0) as u32;
        let z1 = ((max_z - origin_z) / extent).floor().max(0.0) as u32;
        if x0 >= tile_count_x || z0 >= tile_count_z {
            return Some((to, Vec::new()));
        }
        let x1 = x1.min(tile_count_x - 1);
        let z1 = z1.min(tile_count_z - 1);
        if (u64::from(x1 - x0) + 1) * (u64::from(z1 - z0) + 1) > 256 {
            return None;
        }
        let full_rev = world.terrain.full_render_revision();
        let mut tiles = Vec::new();
        for x in x0..=x1 {
            for z in z0..=z1 {
                let key = [x, z];
                let local_rev = world.terrain.render_chunk_revision(key);
                let current = {
                    let cache = self.terrain_tiles.borrow();
                    cache.get(&key).and_then(|entry| {
                        (entry.full_revision == full_rev && entry.local_revision == local_rev)
                            .then(|| entry.collider.clone())
                    })
                };
                if let Some(current) = current {
                    tiles.push(current);
                    continue;
                }
                let tile = collision_source_tile(world, key)?;
                let collider = Arc::new(TriangleCollider::new(&tile));
                {
                    let mut cache = self.terrain_tiles.borrow_mut();
                    if cache.len() >= 64 && !cache.contains_key(&key) {
                        if let Some(evict) = cache.keys().copied().find(|old| *old != key) {
                            cache.remove(&evict);
                        }
                    }
                    cache.insert(
                        key,
                        CachedTerrainTile {
                            full_revision: full_rev,
                            local_revision: local_rev,
                            collider: collider.clone(),
                        },
                    );
                }
                tiles.push(collider);
            }
        }
        let refs = tiles
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<&TriangleCollider>>();
        Some(if let Some(body) = body {
            TriangleCollider::sweep_geometry(&refs, from, to, body)
        } else {
            TriangleCollider::sweep_colliders(&refs, from, to, radius, height)
        })
    }
}

fn heightfield_render_tile(
    field: &crate::world::HeightfieldData,
    x0: usize,
    x1: usize,
    z0: usize,
    z1: usize,
) -> Option<crate::landscape::IndexedMesh> {
    use crate::landscape::{IndexedMesh, Vertex};
    if x1 < x0 || z1 < z0 || x1 >= field.size[0] as usize || z1 >= field.size[2] as usize {
        return None;
    }
    let width = x1 - x0 + 1;
    let mut vertices = Vec::new();
    for z in z0..=z1 {
        for x in x0..=x1 {
            vertices.push(Vertex::new(
                [
                    field.origin[0] as f64 + x as f64 + 0.5,
                    field.height(x as u32, z as u32)? as f64,
                    field.origin[2] as f64 + z as f64 + 0.5,
                ],
                [x as f32, z as f32],
            ));
        }
    }
    let mut triangles = Vec::new();
    for z in 0..(z1 - z0) {
        for x in 0..(x1 - x0) {
            let a = (z * width + x) as u32;
            let b = a + 1;
            let c = a + width as u32;
            let d = c + 1;
            triangles.push([a, d, b]);
            triangles.push([a, c, d]);
        }
    }
    Some(IndexedMesh {
        vertices,
        triangles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::terrain::{
        HeightfieldData, PLAINS_GENERATOR_IMPLEMENTATION_VERSION, TerrainData,
        TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    };

    #[test]
    fn native_tiles_cache_until_their_own_sculpt_revision_changes() {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 8.0;
        world.terrain.config.plains.height_variation = 0.0;
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [144, 24, 80],
            })
            .unwrap();
        assert!(matches!(
            world.terrain.effective().unwrap().data,
            TerrainData::Voxel(_)
        ));
        let physics = PhysicsWorld::new();
        let first = Vec3::new(12.0, 14.0, 12.0);
        let dest = Vec3::new(12.0, 4.0, 12.0);
        let result = physics
            .sweep_terrain(&world, first, dest, 0.35, 1.8)
            .expect("native terrain must be available during Play");
        assert!(
            result.0.y > dest.y + 0.5,
            "the native terrain must physically stop falling actors: {:?}",
            result.0
        );
        assert!(
            !result.1.is_empty(),
            "the native terrain must supply triangle contacts"
        );
        let first_colliders = physics.terrain_tiles.borrow();
        assert!(!first_colliders.is_empty());
        let tile_key = *first_colliders.keys().next().unwrap();
        let original = first_colliders[&tile_key].collider.clone();
        drop(first_colliders);
        physics
            .sweep_terrain(&world, first, dest, 0.35, 1.8)
            .unwrap();
        assert!(
            Arc::ptr_eq(
                &original,
                &physics.terrain_tiles.borrow()[&tile_key].collider
            ),
            "unchanged terrain must reuse its collider"
        );

        world
            .terrain
            .mark_authored_brush_dirty([12.0, 8.0, 12.0], 3.0);
        physics
            .sweep_terrain(&world, first, dest, 0.35, 1.8)
            .unwrap();
        assert!(
            !Arc::ptr_eq(
                &original,
                &physics.terrain_tiles.borrow()[&tile_key].collider
            ),
            "the affected sculpt tile must refresh without rebuilding the world"
        );
    }

    fn vertical_wall() -> crate::landscape::IndexedMesh {
        use crate::landscape::{IndexedMesh, Vertex};
        let vertices = [
            [0.0, 0.0, -8.0],
            [0.0, 5.0, -8.0],
            [0.0, 0.0, 8.0],
            [0.0, 5.0, 8.0],
        ]
        .into_iter()
        .map(|p| Vertex::new(p, [0.0, 0.0]))
        .collect();
        IndexedMesh {
            vertices,
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        }
    }

    #[test]
    fn generic_actor_slides_sideways_against_published_terrain_mesh() {
        use crate::engine::entity::EntityManager;
        use std::collections::HashSet;
        let mut physics = PhysicsWorld::new();
        let mesh = vertical_wall();
        physics.set_native_terrain_mesh(Some(&mesh));
        let mut entities = EntityManager::new();
        let id = entities.create_entity("Collider Test");
        entities.set_position(id, Vec3::new(-2.0, 0.0, -1.0));
        let state = entities.get_physics_mut(id).unwrap();
        state.enabled = true;
        state.anchored = false;
        state.solid = true;
        state.collision_enabled = true;
        state.size = Vec3::new(0.8, 1.8, 0.8);
        state.velocity = Vec3::new(6.0, 0.0, 4.0);
        physics.step_entities_excluding_with_terrain(
            &mut entities,
            Vec3::ZERO,
            0.6,
            &HashSet::new(),
            Some(&World::new()),
        );
        let pos = entities.get_position(id).unwrap();
        assert!(pos.x < -0.2, "Actor crossed native cliff: {pos:?}");
        assert!(
            pos.z > -0.5,
            "side travel must survive wall contact: {pos:?}"
        );
        assert!(entities.get_physics_mut(id).unwrap().velocity.x.abs() < 0.01);
    }

    #[test]
    fn character_system_uses_published_native_terrain_mesh() {
        use crate::character::CharacterSystem;
        let mut world = World::new();
        world.gravity = Vec3::ZERO;
        let mut physics = PhysicsWorld::new();
        physics.set_native_terrain_mesh(Some(&vertical_wall()));
        let mut characters = CharacterSystem::new();
        let id = characters.spawn_character(Vec3::new(-2.0, 0.0, -1.0), None);
        characters
            .characters
            .get_mut(&id)
            .unwrap()
            .movement
            .velocity = Vec3::new(6.0, 0.0, 4.0);
        characters.update_scripted(&world, &mut physics, 0.6);
        let current = characters.characters.get(&id).unwrap();
        assert!(
            current.transform.position.x < -0.2,
            "Character crossed native cliff: {:?}",
            current.transform.position
        );
        assert!(
            current.transform.position.z > -0.5,
            "Character must keep moving along steep terrain"
        );
    }

    #[test]
    fn surface_collision_updates_without_rebuilding_unchanged_tiles() {
        let mut world = World::new();
        world.terrain.authored = Some(TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [12, 12, 12],
            },
            generator: TerrainGeneratorType::Plains,
            generator_implementation_version: PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: [0, 0, 0],
                size: [12, 12, 12],
                heights: vec![0.0; 144],
            }),
        });
        let physics = PhysicsWorld::new();
        let (position, contacts) = physics
            .sweep_terrain(
                &world,
                Vec3::new(4.0, 1.0, 4.0),
                Vec3::new(4.0, -1.0, 4.0),
                0.3,
                1.8,
            )
            .unwrap();
        assert!(
            position.y >= -0.01,
            "historic terrain must stop falling actors"
        );
        assert!(contacts.iter().any(|c| c.normal.y > 0.7));
    }
}

pub(crate) fn collision_source_tile(
    world: &World,
    [x, z]: [u32; 2],
) -> Option<crate::landscape::IndexedMesh> {
    let result = world.terrain.effective()?;
    let (spacing, columns, rows) = match &result.data {
        TerrainData::Voxel(g) => (g.spacing_studs, g.size[0], g.size[2]),
        TerrainData::Heightfield(g) => (1.0, g.size[0], g.size[2]),
    };
    let size = terrain_render_chunk_samples(spacing);
    let (sx, sz) = (x.checked_mul(size)? as usize, z.checked_mul(size)? as usize);
    let (ex, ez) = (
        ((x + 1) * size).min(columns) as usize,
        ((z + 1) * size).min(rows) as usize,
    );
    match &result.data {
        TerrainData::Voxel(_) => {
            crate::landscape::legacy_import::import_legacy_surface_tile(result, sx, ex, sz, ez)
                .ok()?
                .tile_mesh(1, 1, ex - sx, ez - sz, 1)
                .ok()
        }
        TerrainData::Heightfield(g) => heightfield_render_tile(
            g,
            sx,
            ex.min(columns as usize - 1),
            sz,
            ez.min(rows as usize - 1),
        ),
    }
}
