// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::engine::collision_geometry::{self, ColliderInstance};
use crate::engine::physics::{
    anchored_surface::build_anchored_colliders, terrain_collision_cache::collision_source_tile,
};
use crate::world::TerrainData;

pub(crate) fn actor_colliders(
    world: &World,
) -> std::sync::Arc<BTreeMap<u64, (Vec3, ColliderInstance)>> {
    if let Some(snapshot) = world
        .navigation_runtime_geometry
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        return snapshot.clone();
    }
    std::sync::Arc::new(
        world
            .actors
            .values()
            .filter(|a| {
                a.enabled
                    && a.physics.enabled
                    && a.physics.solid
                    && a.physics.collision_enabled
                    && a.physics.anchored
            })
            .filter_map(|a| {
                collision_geometry::preview_actor_collider(a).map(|c| (a.id, (a.position, c)))
            })
            .collect(),
    )
}

pub(super) fn terrain_layout(world: &World) -> Option<(f32, u32, u32, f32, f32, u32)> {
    let r = world.terrain.effective()?;
    let (spacing, cols, rows, x, z) = match &r.data {
        TerrainData::Voxel(g) => (
            g.spacing_studs,
            g.size[0],
            g.size[2],
            r.request.position[0] as f32,
            r.request.position[2] as f32,
        ),
        TerrainData::Heightfield(g) => (
            1.0,
            g.size[0],
            g.size[2],
            g.origin[0] as f32 + 0.5,
            g.origin[2] as f32 + 0.5,
        ),
    };
    Some((
        spacing,
        cols,
        rows,
        x,
        z,
        crate::world::terrain::terrain_render_chunk_samples(spacing),
    ))
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CellSources {
    positions: BTreeMap<(i32, i32, i32), std::sync::Arc<BTreeMap<WorldCoord, (u64, Vec3)>>>,
    buckets: BTreeMap<(i32, i32), std::sync::Arc<BTreeSet<WorldCoord>>>,
}
impl CellSources {
    pub fn new(world: &World) -> Self {
        let mut result = Self::default();
        for p in world.iter_active_effective_coords() {
            result.update(world, p);
        }
        result
    }
    fn keys(p: Vec3) -> impl Iterator<Item = (i32, i32)> {
        let x0 = (p.x / 16.0).floor() as i32;
        let x1 = ((p.x + 1.0) / 16.0).floor() as i32;
        let z0 = (p.z / 16.0).floor() as i32;
        let z1 = ((p.z + 1.0) / 16.0).floor() as i32;
        (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
    }
    fn chunk(p: WorldCoord) -> (i32, i32, i32) {
        (p.x.div_euclid(16), p.y.div_euclid(16), p.z.div_euclid(16))
    }
    fn get(&self, p: WorldCoord) -> Option<(u64, Vec3)> {
        self.positions.get(&Self::chunk(p))?.get(&p).copied()
    }
    pub fn position(&self, p: WorldCoord) -> Option<Vec3> {
        self.get(p).map(|(_, p)| p)
    }
    pub fn update(&mut self, world: &World, p: WorldCoord) -> (Option<Vec3>, Option<Vec3>) {
        let old = self
            .positions
            .get_mut(&Self::chunk(p))
            .and_then(|chunk| std::sync::Arc::make_mut(chunk).remove(&p))
            .map(|(_, v)| v);
        if let Some(old) = old {
            for key in Self::keys(old) {
                if let Some(bucket) = self.buckets.get_mut(&key) {
                    std::sync::Arc::make_mut(bucket).remove(&p);
                }
            }
        }
        let current = world
            .get_effective_cell(p)
            .filter(|_| {
                world.is_cell_solid(p)
                    && world.is_cell_anchored(p)
                    && world
                        .get_effective_cell(p)
                        .is_some_and(|c| c.cell_type != crate::world::CellType::Light)
            })
            .map(|c| {
                (
                    c.id,
                    Vec3::new(p.x as f32, p.y as f32, p.z as f32) + world.get_visual_offset(p),
                )
            });
        if let Some((id, v)) = current {
            if v.is_finite() {
                std::sync::Arc::make_mut(self.positions.entry(Self::chunk(p)).or_default())
                    .insert(p, (id, v));
                for key in Self::keys(v) {
                    std::sync::Arc::make_mut(self.buckets.entry(key).or_default()).insert(p);
                }
            }
        }
        (old, current.map(|(_, v)| v))
    }
    pub fn estimated_bytes(&self) -> usize {
        self.positions.values().map(|c| c.len() * 80).sum::<usize>()
            + self.buckets.values().map(|b| b.len() * 40).sum::<usize>()
    }
    pub fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.positions
            .values()
            .flat_map(|c| c.values().map(|(_, p)| *p))
    }
    fn query(&self, lo: Vec3, hi: Vec3) -> Vec<(u64, Vec3)> {
        let mut candidates = BTreeSet::new();
        for x in (lo.x / 16.0).floor() as i32..=(hi.x / 16.0).floor() as i32 {
            for z in (lo.z / 16.0).floor() as i32..=(hi.z / 16.0).floor() as i32 {
                if let Some(bucket) = self.buckets.get(&(x, z)) {
                    candidates.extend(bucket.iter().copied());
                }
            }
        }
        candidates
            .into_iter()
            .filter_map(|p| self.get(p))
            .filter(|(_, p)| !(*p + Vec3::ONE).cmplt(lo).any() && !p.cmpgt(hi).any())
            .collect()
    }
}

pub(crate) struct CellSourceCapture {
    chunks: Vec<crate::world::ChunkCoord>,
    coords: Vec<WorldCoord>,
    sources: CellSources,
}
impl CellSourceCapture {
    pub fn new(world: &World) -> Self {
        Self {
            chunks: world.spatial_index.chunks.keys().copied().collect(),
            coords: Vec::new(),
            sources: CellSources::default(),
        }
    }
    pub fn from_previous(world: &World, revision: u64, sources: &CellSources) -> Self {
        let current = world.navigation_collision_revision.get();
        let journal = world.navigation_collision_changes.borrow();
        let complete = revision <= current
            && (revision == current || journal.front().is_some_and(|(r, _)| *r <= revision + 1))
            && journal
                .iter()
                .filter(|(r, _)| *r > revision)
                .all(|(_, p)| p.is_some());
        if complete {
            Self {
                chunks: Vec::new(),
                sources: sources.clone(),
                coords: journal
                    .iter()
                    .filter(|(r, _)| *r > revision)
                    .filter_map(|(_, p)| *p)
                    .collect(),
            }
        } else {
            Self::new(world)
        }
    }
    pub fn advance(&mut self, world: &World, deadline: std::time::Instant) -> bool {
        loop {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            if let Some(coord) = self.coords.pop() {
                self.sources.update(world, coord);
            } else if let Some(chunk) = self.chunks.pop() {
                if let Some(coords) = world.get_resident_coords_in_chunk(chunk) {
                    self.coords.extend(coords.iter().copied());
                }
            } else {
                return true;
            }
        }
    }
    pub fn finish(self) -> std::sync::Arc<CellSources> {
        std::sync::Arc::new(self.sources)
    }
}

#[derive(Default)]
pub(crate) struct NavigationTerrainSources {
    revision: Option<(u64, u64)>,
    tiles: BTreeMap<[u32; 2], std::sync::Arc<crate::landscape::IndexedMesh>>,
}
impl NavigationTerrainSources {
    fn get(
        &mut self,
        world: &World,
        key: [u32; 2],
    ) -> Option<std::sync::Arc<crate::landscape::IndexedMesh>> {
        let revision = (world.navigation_world_id, world.terrain.render_revision());
        if self.revision != Some(revision) {
            self.tiles.clear();
            self.revision = Some(revision);
        }
        if let Some(tile) = self.tiles.get(&key) {
            return Some(tile.clone());
        }
        let tile = std::sync::Arc::new(collision_source_tile(world, key)?);
        self.tiles.insert(key, tile.clone());
        Some(tile)
    }
}
pub(super) fn triangles(
    world: &World,
    lo: Vec3,
    hi: Vec3,
    actors: &BTreeMap<u64, (Vec3, ColliderInstance)>,
    sources: &CellSources,
) -> Vec<[Vec3; 3]> {
    triangles_cached(world, lo, hi, actors, sources, None)
}
pub(super) fn triangles_cached(
    world: &World,
    lo: Vec3,
    hi: Vec3,
    actors: &BTreeMap<u64, (Vec3, ColliderInstance)>,
    sources: &CellSources,
    mut terrain: Option<&mut NavigationTerrainSources>,
) -> Vec<[Vec3; 3]> {
    let mut out = Vec::new();
    let cells = sources.query(lo - Vec3::ONE, hi + Vec3::ONE);
    for group in build_anchored_colliders(&cells) {
        for f in group.faces {
            let (u, v) = if f.normal.x != 0.0 {
                (Vec3::Y, Vec3::Z)
            } else if f.normal.y != 0.0 {
                (Vec3::X, Vec3::Z)
            } else {
                (Vec3::X, Vec3::Y)
            };
            let d = f.max - f.min;
            let b = f.min + u * d.dot(u);
            let c = f.min + v * d.dot(v);
            let mut t = [[f.min, b, f.max], [f.min, f.max, c]];
            if u.cross(v).dot(f.normal) < 0.0 {
                for p in &mut t {
                    p.swap(1, 2);
                }
            }
            out.extend(t);
        }
    }
    for (pos, collider) in actors.values() {
        if let Some((min, max)) = collision_geometry::envelope(collider) {
            if (max + *pos).cmplt(lo).any() || (min + *pos).cmpgt(hi).any() {
                continue;
            }
            out.extend(collision_geometry::navigation_triangles(collider, *pos));
        }
    }
    if let Some((spacing, cols, rows, x, z, size)) = terrain_layout(world) {
        let extent = spacing * size as f32;
        let a = ((lo.x - x) / extent).floor().max(0.0) as u32;
        let b = ((hi.x - x) / extent).floor().max(0.0) as u32;
        let c = ((lo.z - z) / extent).floor().max(0.0) as u32;
        let d = ((hi.z - z) / extent).floor().max(0.0) as u32;
        for tx in a..=b.min(cols.div_ceil(size).saturating_sub(1)) {
            for tz in c..=d.min(rows.div_ceil(size).saturating_sub(1)) {
                let mesh = if let Some(cache) = terrain.as_deref_mut() {
                    cache.get(world, [tx, tz])
                } else {
                    collision_source_tile(world, [tx, tz]).map(std::sync::Arc::new)
                };
                if let Some(mesh) = mesh {
                    for t in &mesh.triangles {
                        let p = t.map(|i| {
                            Vec3::from_array(mesh.vertices[i as usize].position.map(|v| v as f32))
                        });
                        if p.iter()
                            .fold(Vec3::splat(f32::NEG_INFINITY), |a, p| a.max(*p))
                            .cmplt(lo)
                            .any()
                            || p.iter()
                                .fold(Vec3::splat(f32::INFINITY), |a, p| a.min(*p))
                                .cmpgt(hi)
                                .any()
                        {
                            continue;
                        }
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

pub(crate) type RuntimeCollisionSnapshot = std::sync::Arc<
    std::sync::Mutex<Option<std::sync::Arc<BTreeMap<u64, (Vec3, ColliderInstance)>>>>,
>;
pub(crate) struct RuntimeCollisionContext {
    snapshot: RuntimeCollisionSnapshot,
    previous: Option<std::sync::Arc<BTreeMap<u64, (Vec3, ColliderInstance)>>>,
}
impl Drop for RuntimeCollisionContext {
    fn drop(&mut self) {
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = self.previous.take();
    }
}
pub(crate) fn runtime_collision_context(
    world: &World,
    entities: &crate::engine::entity::EntityManager,
    characters: &crate::character::system::CharacterSystem,
) -> RuntimeCollisionContext {
    use crate::engine::collision_geometry::CollisionShape;
    let geometry = std::sync::Arc::new(
        entities
            .iter()
            .filter(|e| {
                e.enabled
                    && e.physics.enabled
                    && e.physics.anchored
                    && e.physics.solid
                    && e.physics.collision_enabled
                    && characters.get_character_for_entity(e.id).is_none()
            })
            .map(|e| {
                let c = e
                    .physics
                    .collider
                    .clone()
                    .unwrap_or_else(|| ColliderInstance {
                        shapes: std::sync::Arc::new(vec![CollisionShape::Box {
                            min: (e.physics.offset - e.physics.size * 0.5).to_array(),
                            max: (e.physics.offset + e.physics.size * 0.5).to_array(),
                        }]),
                        rotation: Vec3::ZERO,
                        scale: Vec3::ONE,
                    });
                (e.id.0, (e.position, c))
            })
            .collect(),
    );
    let snapshot = world.navigation_runtime_geometry.clone();
    let previous = snapshot
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(geometry);
    RuntimeCollisionContext { snapshot, previous }
}

#[cfg(test)]
mod collision_context_tests {
    use super::*;
    use crate::character::system::CharacterSystem;
    use crate::engine::entity::EntityManager;
    #[test]
    fn runtime_static_collision_toggles_rebuild_local_tiles_and_characters_are_excluded() {
        let mut world = World::new();
        for x in 0..16 {
            for z in 0..12 {
                world.set_cell(WorldCoord::new(x, 0, z), crate::world::CellType::Block);
            }
        }
        let mut entities = EntityManager::new();
        let mut characters = CharacterSystem::new();
        let wall = entities.create_entity("door");
        {
            let e = entities.get_entity_mut(wall).unwrap();
            e.position = Vec3::new(8.0, 2.5, 6.0);
            e.physics.enabled = true;
            e.physics.anchored = true;
            e.physics.size = Vec3::new(1.0, 3.0, 12.0);
        }
        let ghost = entities.create_entity("character");
        let c = characters.spawn_character(Vec3::new(3.5, 1.0, 5.5), None);
        characters.associate_entity(ghost, c);
        entities.get_entity_mut(ghost).unwrap().physics.enabled = true;
        let mut nav = {
            let _context = runtime_collision_context(&world, &entities, &characters);
            assert_eq!(
                actor_colliders(&world).len(),
                1,
                "Characters must not be baked as static blockers"
            );
            NavigationGrid::build(&world, NavigationAgent::default()).unwrap()
        };
        assert!(world.navigation_runtime_geometry.lock().unwrap().is_none());
        assert!(
            nav.find_path(
                WorldCoord::new(3, 1, 5),
                WorldCoord::new(12, 1, 5),
                Default::default()
            )
            .is_err()
        );
        let built = nav.statistics().tiles_built;
        entities
            .get_entity_mut(wall)
            .unwrap()
            .physics
            .collision_enabled = false;
        {
            let _context = runtime_collision_context(&world, &entities, &characters);
            nav.refresh_collision(&world).unwrap();
        }
        assert_eq!(nav.statistics().tiles_built - built, 1);
        assert!(
            nav.find_path(
                WorldCoord::new(3, 1, 5),
                WorldCoord::new(12, 1, 5),
                Default::default()
            )
            .is_ok()
        );
    }
}
