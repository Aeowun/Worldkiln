// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::geometry;
use super::*;
use crate::engine::collision_geometry::{ColliderInstance, envelope};
use glam::Vec2;
use rerecast::{
    AreaType, BuildContoursFlags, DetailNavmesh, HeightfieldBuilder, PolygonNavmesh, TriMesh,
};
use std::sync::Arc;
const TILE: f32 = 16.0;
const CS: f32 = 0.03125;
const CH: f32 = 0.03125;
type Key = (i32, i32);
#[derive(Clone, Debug)]
struct Poly {
    source: Arc<Vec<[Vec3; 3]>>,
    vertices: Vec<Vec3>,
    detail: Vec<[Vec3; 3]>,
    neighbors: Vec<u16>,
}
#[derive(Clone, Debug, Default)]
struct Tile {
    polys: Vec<Poly>,
}
#[derive(Clone, Copy, Debug)]
struct Link {
    to: usize,
    left: Vec3,
    right: Vec3,
}
#[derive(Clone, Debug, Default)]
struct QueryData {
    polys: Vec<Poly>,
    links: Vec<Vec<Link>>,
    tiles: BTreeMap<Key, Vec<usize>>,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct NavigationStatistics {
    pub tiles: usize,
    pub polygons: usize,
    pub estimated_bytes: usize,
    pub tiles_built: u64,
    pub last_build_micros: u128,
}
#[derive(Clone, Debug)]
pub(super) struct PolygonNavigation {
    tiles: BTreeMap<Key, Arc<Tile>>,
    data: Arc<QueryData>,
    volumes: Vec<NavMeshBoundsVolume>,
    settings: NavMeshGenerationSettings,
    agent: NavigationAgent,
    radius: f32,
    height: f32,
    min: Vec3,
    max: Vec3,
    cells: Arc<geometry::CellSources>,
    deferred: bool,
    refresh_sources: Option<Arc<geometry::CellSources>>,
    pending: BTreeSet<Key>,
    captured: BTreeMap<Key, Vec<[Vec3; 3]>>,
    actors: Arc<BTreeMap<u64, (Vec3, ColliderInstance)>>,
    cell_revision: u64,
    terrain_revision: u64,
    full_terrain: u64,
    world_id: u64,
    water_key: String,
    pub stats: NavigationStatistics,
}
pub(super) struct BakeWater {
    has_terrain: bool,
    ocean_enabled: bool,
    level: f32,
    bodies: Vec<crate::world::terrain::WaterBody>,
}
impl BakeWater {
    pub(super) fn new(world: &World) -> Self {
        Self {
            has_terrain: world.terrain.effective().is_some(),
            ocean_enabled: world.terrain.config.ocean_enabled,
            level: world.terrain.config.plains.water_level,
            bodies: world
                .terrain
                .config
                .water_bodies
                .iter()
                .filter(|b| b.visible)
                .cloned()
                .collect(),
        }
    }
}
impl PolygonNavigation {
    pub fn build(
        world: &World,
        agent: NavigationAgent,
        volumes: &[NavMeshBoundsVolume],
        settings: NavMeshGenerationSettings,
        radius: f32,
        height: f32,
    ) -> Result<Self, NavigationBuildError> {
        let mut nav = Self::prepare(
            world,
            agent,
            volumes,
            settings,
            radius,
            height,
            Arc::new(geometry::CellSources::new(world)),
        )?;
        nav.deferred = false;
        let keys = std::mem::take(&mut nav.pending);
        nav.rebuild(world, &keys)?;
        Ok(nav)
    }
    pub(super) fn prepare(
        world: &World,
        agent: NavigationAgent,
        volumes: &[NavMeshBoundsVolume],
        settings: NavMeshGenerationSettings,
        radius: f32,
        height: f32,
        cells: Arc<geometry::CellSources>,
    ) -> Result<Self, NavigationBuildError> {
        agent.validate()?;
        settings.validate()?;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let volumes: Vec<_> = volumes.iter().filter(|v| v.enabled).cloned().collect();
        if !settings.global_navigation {
            if volumes.is_empty() {
                return Err(NavigationBuildError::NoSolidSurface);
            }
            for v in &volumes {
                let (a, b) = v.bounds().ok_or(NavigationBuildError::BoundsOverflow)?;
                min = min.min(a.into());
                max = max.max(b.into());
            }
        } else {
            for p in cells.positions() {
                min = min.min(p);
                max = max.max(p + Vec3::ONE);
            }
            if let Some(r) = world.terrain.effective() {
                let p = Vec3::from_array(r.request.position.map(|v| v as f32));
                min = min.min(p);
                max = max.max(p + Vec3::from_array(r.request.size.map(|v| v as f32)));
            }
            for (pos, c) in geometry::actor_colliders(world).values() {
                if let Some((a, b)) = envelope(c) {
                    min = min.min(*pos + a);
                    max = max.max(*pos + b);
                }
            }
            if !min.is_finite() {
                return Err(NavigationBuildError::EmptyWorld);
            }
            min.y -= height;
            max.y += height;
        }
        if !radius.is_finite()
            || radius <= 0.0
            || !height.is_finite()
            || height <= 0.0
            || max.y - min.y > CH * 65000.0
        {
            return Err(NavigationBuildError::InvalidAgent(
                "unsupported capsule or vertical extent",
            ));
        }
        let mut nav = Self {
            tiles: BTreeMap::new(),
            data: Arc::default(),
            volumes,
            settings,
            agent,
            radius,
            height,
            min,
            max,
            cells,
            deferred: true,
            refresh_sources: None,
            pending: BTreeSet::new(),
            captured: BTreeMap::new(),
            actors: geometry::actor_colliders(world),
            cell_revision: world.navigation_collision_revision.get(),
            terrain_revision: world.terrain.render_revision(),
            full_terrain: world.terrain.full_render_revision(),
            world_id: world.navigation_world_id,
            water_key: Self::water_key(world),
            stats: NavigationStatistics::default(),
        };
        let keys = nav.keys()?;
        nav.rebuild(world, &keys)?;
        Ok(nav)
    }
    pub fn bounds(&self) -> NavigationBounds {
        NavigationBounds {
            min: WorldCoord::new(
                self.min.x.floor() as i32,
                self.min.y.floor() as i32,
                self.min.z.floor() as i32,
            ),
            max: WorldCoord::new(
                self.max.x.ceil() as i32,
                self.max.y.ceil() as i32,
                self.max.z.ceil() as i32,
            ),
        }
    }
    pub fn debug_samples(
        &self,
        limit: usize,
    ) -> Result<
        (
            NavigationBounds,
            BTreeSet<WorldCoord>,
            BTreeMap<WorldCoord, f32>,
        ),
        NavigationBuildError,
    > {
        let mut points = BTreeSet::new();
        let mut heights = BTreeMap::new();
        for p in &self.data.polys {
            let lo = p
                .vertices
                .iter()
                .copied()
                .fold(Vec3::splat(f32::INFINITY), Vec3::min);
            let hi = p
                .vertices
                .iter()
                .copied()
                .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
            for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
                for z in lo.z.floor() as i32..=hi.z.ceil() as i32 {
                    let sample = Vec3::new(x as f32 + 0.5, 0.0, z as f32 + 0.5);
                    if let Some(y) = surface_height(p, sample) {
                        let point = WorldCoord::new(x, y.floor() as i32, z);
                        points.insert(point);
                        heights.insert(point, y);
                        if points.len() > limit {
                            return Err(NavigationBuildError::ExtractionLimitExceeded {
                                candidates: points.len(),
                                limit,
                            });
                        }
                    }
                }
            }
        }
        let min = WorldCoord::new(
            self.min.x.floor() as i32,
            self.min.y.floor() as i32,
            self.min.z.floor() as i32,
        );
        let max = WorldCoord::new(
            self.max.x.ceil() as i32,
            self.max.y.ceil() as i32,
            self.max.z.ceil() as i32,
        );
        Ok((NavigationBounds { min, max }, points, heights))
    }
    fn keys(&self) -> Result<BTreeSet<Key>, NavigationBuildError> {
        let mut keys = BTreeSet::new();
        let regions: Vec<_> = if self.settings.global_navigation {
            vec![(self.min, self.max)]
        } else {
            self.volumes
                .iter()
                .filter_map(|v| v.bounds())
                .map(|(a, b)| (a.into(), b.into()))
                .collect()
        };
        for (a, b) in regions {
            let (x0, z0, x1, z1) = (
                (a.x / TILE).floor() as i32,
                (a.z / TILE).floor() as i32,
                (b.x / TILE).ceil() as i32,
                (b.z / TILE).ceil() as i32,
            );
            let count =
                (i64::from(x1) - i64::from(x0)).saturating_mul(i64::from(z1) - i64::from(z0));
            if count > 4096 || count < 0 {
                return Err(NavigationBuildError::ExtractionLimitExceeded {
                    candidates: count.max(0) as usize,
                    limit: 4096,
                });
            }
            for x in x0..x1 {
                for z in z0..z1 {
                    keys.insert((x, z));
                }
            }
            if keys.len() > 4096 {
                return Err(NavigationBuildError::ExtractionLimitExceeded {
                    candidates: keys.len(),
                    limit: 4096,
                });
            }
        }
        Ok(keys)
    }
    pub(super) fn water_key(world: &World) -> String {
        use std::hash::Hasher;
        let mut key = std::collections::hash_map::DefaultHasher::new();
        key.write_u8(world.terrain.config.ocean_enabled as u8);
        key.write_u32(world.terrain.config.plains.water_level.to_bits());
        for body in world
            .terrain
            .config
            .water_bodies
            .iter()
            .filter(|b| b.visible)
        {
            key.write_u64(body.id);
            key.write_u8(matches!(body.kind, crate::world::terrain::WaterBodyKind::River) as u8);
            key.write_u32(body.elevation.to_bits());
            key.write_u32(body.depth.to_bits());
            for p in &body.points {
                for v in [
                    p.position[0],
                    p.position[1],
                    p.width,
                    p.height_offset,
                    p.tangent[0],
                    p.tangent[1],
                    p.channel_depth.unwrap_or(body.depth),
                ] {
                    key.write_u32(v.to_bits());
                }
            }
        }
        key.finish().to_string()
    }
    pub fn refresh(&mut self, world: &World) -> Result<(), NavigationBuildError> {
        let actors = geometry::actor_colliders(world);
        let water = Self::water_key(world);
        if world.navigation_world_id == self.world_id
            && actors == self.actors
            && water == self.water_key
            && world.navigation_collision_revision.get() == self.cell_revision
            && world.terrain.render_revision() == self.terrain_revision
        {
            return Ok(());
        }
        let previous_bounds = (self.min, self.max);
        let previous_cells = (world.navigation_collision_revision.get() != self.cell_revision
            || world.navigation_world_id != self.world_id)
            .then(|| self.cells.clone());
        let mut dirty = BTreeSet::new();
        let mut all = world.navigation_world_id != self.world_id
            || water != self.water_key
            || world.terrain.full_render_revision() != self.full_terrain;
        if world.navigation_collision_revision.get() != self.cell_revision {
            let journal = world.navigation_collision_changes.borrow();
            if self.cell_revision > world.navigation_collision_revision.get()
                || journal
                    .front()
                    .is_none_or(|(r, _)| *r > self.cell_revision + 1)
            {
                all = true;
            }
            for (_, p) in journal.iter().filter(|(r, _)| *r > self.cell_revision) {
                if let Some(p) = p {
                    let (old, new) = if let Some(sources) = &self.refresh_sources {
                        (self.cells.position(*p), sources.position(*p))
                    } else {
                        Arc::make_mut(&mut self.cells).update(world, *p)
                    };
                    if old != new {
                        for v in old.into_iter().chain(new) {
                            Self::dirty_region(&mut dirty, v, v + Vec3::ONE, self.radius);
                        }
                    }
                } else {
                    all = true;
                }
            }
        }
        for (&id, (pos, c)) in self.actors.iter().chain(actors.iter()) {
            if self.actors.get(&id) == actors.get(&id) {
                continue;
            }
            if let Some((a, b)) = envelope(c) {
                Self::dirty_region(&mut dirty, *pos + a, *pos + b, self.radius);
            }
        }
        if world.terrain.render_revision() != self.terrain_revision {
            match (
                world
                    .terrain
                    .render_dirty_chunks_since(self.terrain_revision),
                geometry::terrain_layout(world),
            ) {
                (Some(keys), Some((spacing, _, _, x, z, size))) => {
                    let extent = spacing * size as f32;
                    for [a, b] in keys {
                        let lo =
                            Vec3::new(x + a as f32 * extent, self.min.y, z + b as f32 * extent);
                        Self::dirty_region(
                            &mut dirty,
                            lo,
                            lo + Vec3::new(extent, self.max.y - self.min.y, extent),
                            self.radius,
                        );
                    }
                }
                _ => all = true,
            }
        }
        if let Some(sources) = self.refresh_sources.take() {
            self.cells = sources;
        } else if all
            && (world.navigation_collision_revision.get() != self.cell_revision
                || world.navigation_world_id != self.world_id)
        {
            self.cells = Arc::new(geometry::CellSources::new(world));
        }
        if self.settings.global_navigation {
            let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
            for p in self.cells.positions() {
                min = min.min(p);
                max = max.max(p + Vec3::ONE);
            }
            if let Some(r) = world.terrain.effective() {
                let p = Vec3::from_array(r.request.position.map(|v| v as f32));
                min = min.min(p);
                max = max.max(p + Vec3::from_array(r.request.size.map(|v| v as f32)));
            }
            for (pos, c) in actors.values() {
                if let Some((a, b)) = envelope(c) {
                    min = min.min(*pos + a);
                    max = max.max(*pos + b);
                }
            }
            if min.is_finite() {
                min.y -= self.height;
                max.y += self.height;
                self.min = min;
                self.max = max;
            }
        }
        let keys = match self.keys() {
            Ok(keys) => keys,
            Err(e) => {
                if let Some(cells) = previous_cells {
                    self.cells = cells;
                }
                self.min = previous_bounds.0;
                self.max = previous_bounds.1;
                return Err(e);
            }
        };
        dirty.extend(keys.iter().copied().filter(|k| !self.tiles.contains_key(k)));
        if all {
            dirty = keys.clone();
        } else {
            dirty.retain(|k| keys.contains(k));
        }
        let old = std::mem::replace(&mut self.actors, actors);
        if let Err(e) = self.rebuild(world, &dirty) {
            self.actors = old;
            if let Some(cells) = previous_cells {
                self.cells = cells;
            }
            self.min = previous_bounds.0;
            self.max = previous_bounds.1;
            return Err(e);
        }
        self.cell_revision = world.navigation_collision_revision.get();
        self.terrain_revision = world.terrain.render_revision();
        self.full_terrain = world.terrain.full_render_revision();
        self.world_id = world.navigation_world_id;
        self.water_key = water;
        Ok(())
    }
    fn dirty_region(out: &mut BTreeSet<Key>, lo: Vec3, hi: Vec3, r: f32) {
        let r = r + CS * 4.0;
        for x in ((lo.x - r) / TILE).floor() as i32..=((hi.x + r) / TILE).floor() as i32 {
            for z in ((lo.z - r) / TILE).floor() as i32..=((hi.z + r) / TILE).floor() as i32 {
                out.insert((x, z));
            }
        }
    }
    fn rebuild(&mut self, world: &World, keys: &BTreeSet<Key>) -> Result<(), NavigationBuildError> {
        if self.deferred {
            self.pending.extend(keys.iter().copied());
            return Ok(());
        }
        let all_keys = self.keys()?;
        if keys.is_empty() && self.tiles.keys().all(|k| all_keys.contains(k)) {
            return Ok(());
        }
        let time = std::time::Instant::now();
        let mut tiles = self.tiles.clone();
        tiles.retain(|k, _| all_keys.contains(k));
        let water = BakeWater::new(world);
        for &key in keys {
            let (lo, hi) = self.tile_bounds(key);
            let triangles = geometry::triangles(world, lo, hi, &self.actors, &self.cells);
            tiles.insert(key, Arc::new(self.bake(triangles, &water, key)?));
        }
        self.publish_tiles(tiles, keys.len(), time);
        Ok(())
    }
    fn publish_tiles(
        &mut self,
        tiles: BTreeMap<Key, Arc<Tile>>,
        count: usize,
        time: std::time::Instant,
    ) {
        let data = Arc::new(self.connect(&tiles));
        self.stats.tiles = tiles.len();
        self.stats.polygons = data.polys.len();
        self.stats.estimated_bytes = self.cells.estimated_bytes()
            + tiles
                .values()
                .filter_map(|t| t.polys.first())
                .map(|p| p.source.capacity() * 36)
                .sum::<usize>()
            + data
                .polys
                .iter()
                .map(|p| {
                    std::mem::size_of::<Poly>()
                        + p.vertices.capacity() * 12
                        + p.detail.capacity() * 36
                        + p.neighbors.capacity() * 2
                })
                .sum::<usize>()
                * 2
            + data
                .links
                .iter()
                .map(|p| p.capacity() * std::mem::size_of::<Link>())
                .sum::<usize>();
        self.stats.tiles_built += count as u64;
        self.stats.last_build_micros = time.elapsed().as_micros();
        self.tiles = tiles;
        self.data = data;
    }
    pub(super) fn defer_refresh(
        &mut self,
        world: &World,
        sources: Arc<geometry::CellSources>,
    ) -> Result<(), NavigationBuildError> {
        self.deferred = true;
        if world.navigation_collision_revision.get() != self.cell_revision {
            self.refresh_sources = Some(sources);
        }
        self.refresh(world)
    }
    pub(super) fn capture_next(
        &mut self,
        world: &World,
        terrain: &mut geometry::NavigationTerrainSources,
    ) -> bool {
        let Some(key) = self.pending.pop_first() else {
            return true;
        };
        let (lo, hi) = self.tile_bounds(key);
        self.captured.insert(
            key,
            geometry::triangles_cached(world, lo, hi, &self.actors, &self.cells, Some(terrain)),
        );
        self.pending.is_empty()
    }
    pub(super) fn finish_background(
        &mut self,
        water: &BakeWater,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<(), NavigationBuildError> {
        let time = std::time::Instant::now();
        let keys = self.keys()?;
        let mut tiles = self.tiles.clone();
        tiles.retain(|k, _| keys.contains(k));
        let captured = std::mem::take(&mut self.captured);
        let count = captured.len();
        for (key, triangles) in captured {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(NavigationBuildError::InvalidAgent(
                    "navigation build superseded",
                ));
            }
            tiles.insert(key, Arc::new(self.bake(triangles, water, key)?));
        }
        self.publish_tiles(tiles, count, time);
        self.deferred = false;
        Ok(())
    }
    fn tile_bounds(&self, (x, z): Key) -> (Vec3, Vec3) {
        let border = (self.radius / CS).ceil() as u16 + 3;
        let pad = border as f32 * CS;
        let lo = Vec3::new(
            x as f32 * TILE - pad,
            (self.min.y / CH).floor() * CH - CH,
            z as f32 * TILE - pad,
        );
        let hi = Vec3::new(
            (x + 1) as f32 * TILE + pad,
            (self.max.y / CH).ceil() * CH + CH,
            (z + 1) as f32 * TILE + pad,
        );
        (lo, hi)
    }
    fn bake(
        &self,
        triangles: Vec<[Vec3; 3]>,
        water: &BakeWater,
        (x, z): Key,
    ) -> Result<Tile, NavigationBuildError> {
        fn failure<T>(_: T) -> NavigationBuildError {
            NavigationBuildError::InvalidAgent("Recast mesh generation failed")
        }
        let border = (self.radius / CS).ceil() as u16 + 3;
        let (lo, hi) = self.tile_bounds((x, z));
        if triangles.is_empty() {
            return Ok(Tile::default());
        }
        let mut mesh = TriMesh::default();
        let source = Arc::new(
            triangles
                .iter()
                .copied()
                .filter(|t| {
                    (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero().y
                        >= self.settings.max_slope_degrees.to_radians().cos()
                })
                .collect::<Vec<_>>(),
        );
        let origin = Vec3::new(x as f32 * TILE, lo.y, z as f32 * TILE);
        let local_lo = lo - origin;
        let local_hi = hi - origin;
        for t in triangles {
            for t in clip_triangle(t.map(|p| p - origin), local_lo, local_hi) {
                let i = mesh.vertices.len() as u32;
                for v in t {
                    mesh.vertices.push(v.to_array().into());
                }
                mesh.indices.push([i, i + 1, i + 2].into());
                mesh.area_types.push(AreaType::NOT_WALKABLE);
            }
        }
        mesh.mark_walkable_triangles(self.settings.max_slope_degrees.to_radians());
        let mut field = HeightfieldBuilder {
            aabb: rerecast::Aabb3d::new(
                ((local_lo + local_hi) * 0.5).to_array(),
                ((local_hi - local_lo) * 0.5).to_array(),
            ),
            cell_size: CS,
            cell_height: CH,
        }
        .build()
        .map_err(failure)?;
        let climb = (self
            .settings
            .max_step_height
            .min(self.agent.max_step_up as f32)
            .min(self.agent.max_step_down as f32)
            / CH)
            .floor() as u16;
        let height = (self.height / CH).ceil() as u16;
        field.rasterize_triangles(&mesh, climb).map_err(failure)?;
        field.filter_low_hanging_walkable_obstacles(climb);
        field.filter_walkable_low_height_spans(height);
        let mut compact = field.into_compact(height, climb).map_err(failure)?;
        let water_bodies: Vec<_> = water
            .bodies
            .iter()
            .filter(|b| b.visible)
            .map(|b| (b, b.spline_samples()))
            .collect();
        for z in 0..compact.height {
            for x in 0..compact.width {
                let wx = lo.x + x as f32 * CS;
                let wz = lo.z + z as f32 * CS;
                for i in compact.cell_at(x, z).index_range() {
                    let y = lo.y + compact.spans[i].y as f32 * CH;
                    let inside = self.settings.global_navigation
                        || covered_by_bounds(&self.volumes, wx, wz, y);
                    let wet = (water.has_terrain
                        && water.ocean_enabled
                        && water.level > y + self.settings.max_water_depth + 0.001)
                        || water_bodies.iter().any(|(b, s)| {
                            b.vertical_bounds_at_samples(wx + CS * 0.5, wz + CS * 0.5, y, s)
                                .is_some_and(|(a, b)| b - a > self.settings.max_water_depth + 0.001)
                        });
                    if !inside || wet {
                        compact.areas[i] = AreaType::NOT_WALKABLE;
                    }
                }
            }
        }
        erode_physics_footprint(&mut compact, self.radius / CS);
        compact.build_distance_field();
        compact.build_regions(border, 0, 0).map_err(failure)?;
        let poly = compact
            .build_contours(0.0, 64, BuildContoursFlags::empty())
            .into_polygon_mesh(6)
            .map_err(failure)?;
        if poly.polygon_count() == 0 {
            return Ok(Tile::default());
        }
        let detail = DetailNavmesh::new(&poly, &compact, 0.5, CH * 0.5).map_err(failure)?;
        let mut polys = Vec::new();
        let n = poly.max_vertices_per_polygon as usize;
        for (i, indices) in poly.polygons().enumerate() {
            let vertices = indices
                .map(|j| {
                    let v = poly.vertices[j as usize];
                    origin
                        + Vec3::new(
                            poly.aabb.min.x + v.x as f32 * CS,
                            poly.aabb.min.y + v.y as f32 * CH,
                            poly.aabb.min.z + v.z as f32 * CS,
                        )
                })
                .collect::<Vec<_>>();
            let sub = &detail.meshes[i];
            let triangles = detail.triangles[sub.base_triangle_index as usize..]
                [..sub.triangle_count as usize]
                .iter()
                .map(|t| {
                    t.map(|j| {
                        let v = detail.vertices[sub.base_vertex_index as usize + j as usize];
                        origin + Vec3::from_array(v.to_array())
                    })
                })
                .collect();
            let neighbors = poly.polygon_neighbors[i * n..i * n + vertices.len()].to_vec();
            polys.push(Poly {
                source: source.clone(),
                vertices,
                detail: triangles,
                neighbors,
            });
        }
        Ok(Tile { polys })
    }
    fn connect(&self, tiles: &BTreeMap<Key, Arc<Tile>>) -> QueryData {
        let mut data = QueryData::default();
        let mut seams: BTreeMap<(u8, i32, i32), Vec<(usize, Vec3, Vec3)>> = BTreeMap::new();
        for (&key, tile) in tiles {
            let base = data.polys.len();
            let mut ids = Vec::new();
            for (local, p) in tile.polys.iter().enumerate() {
                let id = base + local;
                ids.push(id);
                let mut links = Vec::new();
                for (i, &neighbor) in p.neighbors.iter().enumerate() {
                    let a = p.vertices[i];
                    let b = p.vertices[(i + 1) % p.vertices.len()];
                    if neighbor != PolygonNavmesh::NO_CONNECTION && neighbor & 0x8000 == 0 {
                        links.push(Link {
                            to: base + neighbor as usize,
                            left: a,
                            right: b,
                        });
                    } else {
                        for (axis, value, edge) in [(0, a.x, b.x), (1, a.z, b.z)] {
                            if (value - edge).abs() < 1e-4
                                && ((value / TILE).round() * TILE - value).abs() < 1e-4
                            {
                                seams
                                    .entry((
                                        axis,
                                        (value / TILE).round() as i32,
                                        if axis == 0 { key.1 } else { key.0 },
                                    ))
                                    .or_default()
                                    .push((id, a, b));
                            }
                        }
                    }
                }
                data.polys.push(p.clone());
                data.links.push(links);
            }
            data.tiles.insert(key, ids);
        }
        for ((axis, _, _), edges) in seams {
            for i in 0..edges.len() {
                for j in i + 1..edges.len() {
                    let (a, p, q) = edges[i];
                    let (b, r, s) = edges[j];
                    if a == b || (q - p).dot(s - r) >= 0.0 {
                        continue;
                    }
                    let component = |v: Vec3| if axis == 0 { v.z } else { v.x };
                    let (p0, p1, r0, r1) = (component(p), component(q), component(r), component(s));
                    let (low, high) = (p0.min(p1).max(r0.min(r1)), p0.max(p1).min(r0.max(r1)));
                    if high - low <= 1e-4 {
                        continue;
                    }
                    let point = |v: Vec3, w: Vec3, t: f32| {
                        v.lerp(w, (t - component(v)) / (component(w) - component(v)))
                    };
                    let (l, h) = (point(p, q, low), point(p, q, high));
                    let limit = self
                        .settings
                        .max_step_height
                        .min(self.agent.max_step_up as f32)
                        .min(self.agent.max_step_down as f32)
                        + 1e-4;
                    if (l.y - point(r, s, low).y).abs() > limit
                        || (h.y - point(r, s, high).y).abs() > limit
                    {
                        continue;
                    }
                    let (left, right) = if p0 < p1 { (l, h) } else { (h, l) };
                    data.links[a].push(Link { to: b, left, right });
                    data.links[b].push(Link {
                        to: a,
                        left: right,
                        right: left,
                    });
                }
            }
        }
        data
    }
    fn sample(&self, p: Vec3) -> Option<(usize, Vec3)> {
        if !p.is_finite() {
            return None;
        }
        let tile = ((p.x / TILE).floor() as i32, (p.z / TILE).floor() as i32);
        let mut best = None;
        let mut distance = 1.05_f32;
        for dx in -1..=1 {
            for dz in -1..=1 {
                if let Some(ids) = self.data.tiles.get(&(tile.0 + dx, tile.1 + dz)) {
                    for &id in ids {
                        if let Some(y) = surface_height(&self.data.polys[id], p) {
                            let d = (y - p.y).abs();
                            if d < distance {
                                distance = d;
                                best = Some((id, Vec3::new(p.x, y, p.z)));
                            }
                        }
                    }
                }
            }
        }
        #[cfg(test)]
        if best.is_none() && std::env::var_os("WORLDKILN_NAV_DIAGNOSTICS").is_some() {
            eprintln!(
                "Projection failure at {p:?}, {} polygons: {:?}",
                self.data.polys.len(),
                self.data
                    .polys
                    .iter()
                    .map(|p| (&p.vertices, &p.detail))
                    .collect::<Vec<_>>()
            );
        }
        best
    }
    pub fn find_path(
        &self,
        start: Vec3,
        target: Vec3,
        options: PathQueryOptions,
    ) -> Result<NavigationPath, PathFailure> {
        self.find_path_in_range(start, target, options, 0.0)
    }
    pub fn find_path_in_range(
        &self,
        start: Vec3,
        target: Vec3,
        options: PathQueryOptions,
        stop_distance: f32,
    ) -> Result<NavigationPath, PathFailure> {
        let cell =
            |p: Vec3| WorldCoord::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
        for (p, first) in [(start, true), (target, false)] {
            let outside = if self.settings.global_navigation {
                p.x < self.min.x
                    || p.x >= self.max.x
                    || p.z < self.min.z
                    || p.z >= self.max.z
                    || p.y < self.min.y
                    || p.y > self.max.y
            } else {
                !self.volumes.iter().any(|v| v.contains(p.to_array()))
            };
            if outside {
                return Err(if first {
                    PathFailure::InvalidStart { position: cell(p) }
                } else {
                    PathFailure::InvalidTarget { position: cell(p) }
                });
            }
        }
        let (a, start) = self.sample(start).ok_or(PathFailure::StartBlocked {
            position: cell(start),
            reason: BlockedReason::InsufficientClearance,
        })?;
        let goals: BTreeMap<usize, Vec3> = if stop_distance > 0.0 {
            self.data
                .tiles
                .iter()
                .filter(|((x, z), _)| {
                    ((*x + 1) as f32 * TILE) >= target.x - stop_distance
                        && (*x as f32 * TILE) <= target.x + stop_distance
                        && ((*z + 1) as f32 * TILE) >= target.z - stop_distance
                        && (*z as f32 * TILE) <= target.z + stop_distance
                })
                .flat_map(|(_, ids)| ids.iter().copied())
                .filter_map(|id| {
                    let p = closest_surface_point(&self.data.polys[id], target)?;
                    ((p.y - target.y).abs() < 1.05
                        && Vec2::new(p.x - target.x, p.z - target.z).length() <= stop_distance)
                        .then_some((id, p))
                })
                .collect()
        } else {
            self.sample(target).into_iter().collect()
        };
        if goals.is_empty() {
            return Err(PathFailure::TargetBlocked {
                position: cell(target),
                reason: BlockedReason::InsufficientClearance,
            });
        }
        if options.max_visited_nodes == 0 && !goals.contains_key(&a) {
            return Err(PathFailure::SearchLimitExceeded {
                visited: 0,
                limit: 0,
            });
        }
        let center = |i: usize| {
            self.data.polys[i].vertices.iter().copied().sum::<Vec3>()
                / self.data.polys[i].vertices.len() as f32
        };
        let goal_centers = goals.keys().map(|&id| center(id)).collect::<Vec<_>>();
        let mut open = BinaryHeap::new();
        let mut cost = BTreeMap::new();
        let mut from: BTreeMap<usize, (usize, Link)> = BTreeMap::new();
        let mut closed = BTreeSet::new();
        open.push(Reverse((0_u64, a)));
        cost.insert(a, 0.0_f32);
        while let Some(Reverse((_, id))) = open.pop() {
            if !closed.insert(id) {
                continue;
            }
            if closed.len() > options.max_visited_nodes && !goals.contains_key(&a) {
                return Err(PathFailure::SearchLimitExceeded {
                    visited: closed.len(),
                    limit: options.max_visited_nodes,
                });
            }
            if let Some(&target) = goals.get(&id) {
                let mut corridor = vec![id];
                let mut portals = Vec::new();
                let mut cursor = id;
                while cursor != a {
                    let (prev, link) = from[&cursor];
                    portals.push((link.left, link.right));
                    cursor = prev;
                    corridor.push(cursor);
                }
                portals.reverse();
                corridor.reverse();
                let points = funnel(start, target, &portals);
                let points = refine_heights(&self.data.polys, &corridor, &points);
                let length = points.windows(2).map(|p| p[0].distance(p[1])).sum::<f32>();
                return Ok(NavigationPath {
                    nodes: points.iter().copied().map(cell).collect(),
                    total_cost: (length * 10.0).round() as u32,
                    visited_nodes: closed.len(),
                    polygon_points: Some(points),
                });
            }
            for link in &self.data.links[id] {
                if closed.contains(&link.to) {
                    continue;
                }
                let next = cost[&id] + center(id).distance(center(link.to));
                if cost.get(&link.to).is_some_and(|c| *c <= next) {
                    continue;
                }
                cost.insert(link.to, next);
                from.insert(link.to, (id, *link));
                open.push(Reverse((
                    ((next
                        + goal_centers
                            .iter()
                            .map(|goal| center(link.to).distance(*goal))
                            .fold(f32::INFINITY, f32::min))
                        * 10000.0) as u64,
                    link.to,
                )));
            }
        }
        Err(PathFailure::Unreachable)
    }
}
fn area(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    (b.z - a.z) * (c.x - a.x) - (b.x - a.x) * (c.z - a.z)
}
fn same(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < 1e-5 && (a.z - b.z).abs() < 1e-5
}
fn funnel(start: Vec3, target: Vec3, portals: &[(Vec3, Vec3)]) -> Vec<Vec3> {
    let mut portals = portals.to_vec();
    portals.push((target, target));
    let (mut apex, mut left, mut right) = (start, start, start);
    let (mut li, mut ri) = (0, 0);
    let mut points = vec![start];
    let mut i = 0;
    while i < portals.len() {
        let (l, r) = portals[i];
        if area(apex, right, r) <= 0.0 {
            if same(apex, right) || area(apex, left, r) > 0.0 {
                right = r;
                ri = i;
            } else {
                points.push(left);
                apex = left;
                right = apex;
                left = apex;
                i = li + 1;
                ri = li;
                continue;
            }
        }
        if area(apex, left, l) >= 0.0 {
            if same(apex, left) || area(apex, right, l) < 0.0 {
                left = l;
                li = i;
            } else {
                points.push(right);
                apex = right;
                left = apex;
                right = apex;
                i = ri + 1;
                li = ri;
                continue;
            }
        }
        i += 1;
    }
    if !same(*points.last().unwrap(), target) {
        points.push(target);
    }
    points
}
fn triangle_height([a, b, c]: [Vec3; 3], p: Vec3) -> Option<f32> {
    let den = area(a, b, c);
    if den.abs() < 1e-10 {
        return None;
    }
    let u = area(p, b, c) / den;
    let v = area(a, p, c) / den;
    let w = 1.0 - u - v;
    (u >= -1e-4 && v >= -1e-4 && w >= -1e-4).then_some(u * a.y + v * b.y + w * c.y)
}
fn closest_surface_point(poly: &Poly, target: Vec3) -> Option<Vec3> {
    if let Some(y) = surface_height(poly, target) {
        return Some(Vec3::new(target.x, y, target.z));
    }
    let t = Vec2::new(target.x, target.z);
    poly.vertices
        .iter()
        .zip(poly.vertices.iter().cycle().skip(1))
        .take(poly.vertices.len())
        .filter_map(|(a, b)| {
            let av = Vec2::new(a.x, a.z);
            let delta = Vec2::new(b.x - a.x, b.z - a.z);
            let u = if delta.length_squared() > 0.0 {
                ((t - av).dot(delta) / delta.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let p = a.lerp(*b, u);
            surface_height(poly, p).map(|y| Vec3::new(p.x, y, p.z))
        })
        .min_by(|a, b| {
            Vec2::new(a.x - target.x, a.z - target.z)
                .length_squared()
                .total_cmp(&Vec2::new(b.x - target.x, b.z - target.z).length_squared())
        })
}
fn surface_height(poly: &Poly, p: Vec3) -> Option<f32> {
    let approximate = poly.detail.iter().find_map(|t| triangle_height(*t, p))?;
    poly.source
        .iter()
        .filter_map(|t| triangle_height(*t, p))
        .filter(|y| (*y - approximate).abs() <= CH * 4.0)
        .min_by(|a, b| (a - approximate).abs().total_cmp(&(b - approximate).abs()))
}
fn refine_heights(polys: &[Poly], corridor: &[usize], points: &[Vec3]) -> Vec<Vec3> {
    let mut out = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mut cuts = vec![0.0, 1.0];
        let cross = |u: Vec3, v: Vec3| u.x * v.z - u.z * v.x;
        for &id in corridor {
            for t in &polys[id].detail {
                for j in 0..3 {
                    let (c, d) = (t[j], t[(j + 1) % 3]);
                    let den = cross(b - a, d - c);
                    if den.abs() < 1e-7 {
                        continue;
                    }
                    let x = cross(c - a, d - c) / den;
                    let y = cross(c - a, b - a) / den;
                    if x > 1e-5 && x < 1.0 - 1e-5 && (0.0..=1.0).contains(&y) {
                        cuts.push(x);
                    }
                }
            }
        }
        cuts.sort_by(f32::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
        for x in cuts {
            let mut p = a.lerp(b, x);
            if let Some(y) = corridor
                .iter()
                .filter_map(|id| surface_height(&polys[*id], p))
                .min_by(|u, v| (u - p.y).abs().total_cmp(&(v - p.y).abs()))
            {
                p.y = y;
            }
            if out
                .last()
                .is_some_and(|last: &Vec3| last.distance_squared(p) < 1e-9)
            {
                continue;
            }
            while out.len() >= 2 {
                let prev = out[out.len() - 1];
                let d = prev - out[out.len() - 2];
                let next = p - prev;
                if d.cross(next).length() <= 1e-4 * (d.length() + next.length())
                    && d.dot(next) >= 0.0
                {
                    out.pop();
                } else {
                    break;
                }
            }
            out.push(p);
        }
    }
    if points.len() == 1 {
        out.push(points[0]);
    }
    out
}

fn erode_physics_footprint(field: &mut rerecast::CompactHeightfield, radius: f32) {
    const NONE: u32 = u32::MAX;
    let count = field.spans.len();
    let mut neighbors = vec![[NONE; 8]; count];
    for z in 0..field.height {
        for x in 0..field.width {
            for i in field.cell_at(x, z).index_range() {
                if !field.areas[i].is_walkable() {
                    continue;
                }
                for d in 0..4 {
                    if let Some(c) = field.spans[i].con(d) {
                        let next = field.con_indices(x as i32, z as i32, d, c).2;
                        if field.areas[next].is_walkable() {
                            neighbors[i][d as usize] = next as u32;
                        }
                    }
                }
            }
        }
    }
    for i in 0..count {
        if !field.areas[i].is_walkable() {
            continue;
        }
        for d in 0..4 {
            let e = (d + 1) % 4;
            let (a, b) = (neighbors[i][d], neighbors[i][e]);
            if a == NONE || b == NONE {
                continue;
            }
            let (via_a, via_b) = (neighbors[a as usize][e], neighbors[b as usize][d]);
            if via_a != NONE && via_a == via_b {
                neighbors[i][4 + d] = via_a;
            }
        }
    }
    let required = (radius.ceil() as u16).saturating_add(1);
    let mut distance = vec![u16::MAX; count];
    let mut queue = std::collections::VecDeque::new();
    for i in 0..count {
        if field.areas[i].is_walkable() && neighbors[i].contains(&NONE) {
            distance[i] = 1;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        if distance[i] >= required {
            continue;
        }
        field.areas[i] = AreaType::NOT_WALKABLE;
        let next_distance = distance[i] + 1;
        for &j in &neighbors[i] {
            if j == NONE {
                continue;
            }
            let j = j as usize;
            if distance[j] > next_distance {
                distance[j] = next_distance;
                queue.push_back(j);
            }
        }
    }
}

fn clip_triangle(t: [Vec3; 3], lo: Vec3, hi: Vec3) -> Vec<[Vec3; 3]> {
    let mut points = t.to_vec();
    for (axis, bound, lower) in [
        (0, lo.x, true),
        (0, hi.x, false),
        (1, lo.y, true),
        (1, hi.y, false),
        (2, lo.z, true),
        (2, hi.z, false),
    ] {
        let input = std::mem::take(&mut points);
        if input.len() < 3 {
            return Vec::new();
        }
        let mut previous = *input.last().unwrap();
        let inside = |p: Vec3| {
            if lower {
                p[axis] >= bound
            } else {
                p[axis] <= bound
            }
        };
        for current in input {
            if inside(previous) != inside(current) {
                let mut p = previous.lerp(
                    current,
                    (bound - previous[axis]) / (current[axis] - previous[axis]),
                );
                p[axis] = bound;
                points.push(p);
            }
            if inside(current) {
                points.push(current);
            }
            previous = current;
        }
        points.dedup_by(|a, b| a.distance_squared(*b) < 1e-12);
        if points.len() > 1 && points[0].distance_squared(*points.last().unwrap()) < 1e-12 {
            points.pop();
        }
    }
    if points.len() < 3 {
        return Vec::new();
    }
    (1..points.len() - 1)
        .map(|i| [points[0], points[i], points[i + 1]])
        .filter(|t| (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-12)
        .collect()
}

fn covered_by_bounds(volumes: &[NavMeshBoundsVolume], x: f32, z: f32, y: f32) -> bool {
    let mut candidates = Vec::new();
    for v in volumes {
        if let Some((a, b)) = v.bounds() {
            if y < a[1] || y >= b[1] || b[0] <= x || a[0] >= x + CS || b[2] <= z || a[2] >= z + CS {
                continue;
            }
            if a[0] <= x && b[0] >= x + CS && a[2] <= z && b[2] >= z + CS {
                return true;
            }
            candidates.push((a, b));
        }
    }
    if candidates.is_empty() {
        return false;
    }
    let mut cuts = vec![x, x + CS];
    for (a, b) in &candidates {
        cuts.push(a[0].clamp(x, x + CS));
        cuts.push(b[0].clamp(x, x + CS));
    }
    cuts.sort_by(f32::total_cmp);
    cuts.dedup();
    for pair in cuts.windows(2) {
        let mid = (pair[0] + pair[1]) * 0.5;
        let mut intervals = candidates
            .iter()
            .filter(|(a, b)| a[0] <= mid && b[0] >= mid)
            .map(|(a, b)| (a[2], b[2]))
            .collect::<Vec<_>>();
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut end = z;
        for (lo, hi) in intervals {
            if lo > end {
                break;
            }
            end = end.max(hi);
        }
        if end < z + CS {
            return false;
        }
    }
    true
}
