// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::world::{World, WorldCoord};
use glam::Vec3;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
mod geometry;
mod polygon;
mod volumes;
pub(crate) use geometry::{
    CellSourceCapture, CellSources, NavigationTerrainSources,
    actor_colliders as navigation_actor_colliders,
};
pub(crate) use geometry::{RuntimeCollisionSnapshot, runtime_collision_context};
pub use polygon::NavigationStatistics;
pub(crate) fn navigation_water_key(world: &World) -> String {
    polygon::PolygonNavigation::water_key(world)
}

pub(crate) struct NavigationBuildCapture {
    grid: NavigationGrid,
    water: polygon::BakeWater,
}
impl NavigationBuildCapture {
    pub(crate) fn advance(
        &mut self,
        world: &World,
        terrain: &mut NavigationTerrainSources,
    ) -> bool {
        self.grid.polygon.capture_next(world, terrain)
    }
    pub(crate) fn finish(
        mut self,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<NavigationGrid, NavigationBuildError> {
        self.grid
            .polygon
            .finish_background(&self.water, cancelled)?;
        self.grid.bounds = self.grid.polygon.bounds();
        Ok(self.grid)
    }
}
pub use volumes::{NavMeshBoundsVolume, NavMeshGenerationSettings};

const DEFAULT_MAX_EXTRACTION_CELLS: usize = 2_000_000;
const DEFAULT_MAX_VISITED_NODES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavigationAgent {
    pub width_cells: u32,
    pub height_cells: u32,
    pub max_step_up: u32,
    pub max_step_down: u32,
}

impl Default for NavigationAgent {
    fn default() -> Self {
        Self {
            width_cells: 1,
            height_cells: 2,
            max_step_up: 1,
            max_step_down: 1,
        }
    }
}

impl NavigationAgent {
    pub fn validate(self) -> Result<Self, NavigationBuildError> {
        if self.width_cells == 0 {
            return Err(NavigationBuildError::InvalidAgent(
                "width_cells must be at least 1",
            ));
        }
        if self.height_cells == 0 {
            return Err(NavigationBuildError::InvalidAgent(
                "height_cells must be at least 1",
            ));
        }
        if self.width_cells > i32::MAX as u32
            || self.height_cells > i32::MAX as u32
            || self.max_step_up > i32::MAX as u32
            || self.max_step_down > i32::MAX as u32
        {
            return Err(NavigationBuildError::InvalidAgent(
                "agent dimensions exceed supported voxel coordinates",
            ));
        }
        Ok(self)
    }

    fn footprint_min(self) -> i32 {
        -((self.width_cells as i32 - 1) / 2)
    }

    fn footprint_max(self) -> i32 {
        self.width_cells as i32 / 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavigationBounds {
    pub min: WorldCoord,
    pub max: WorldCoord,
}

impl NavigationBounds {
    pub fn contains(self, position: WorldCoord) -> bool {
        position.x >= self.min.x
            && position.x <= self.max.x
            && position.y >= self.min.y
            && position.y <= self.max.y
            && position.z >= self.min.z
            && position.z <= self.max.z
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedReason {
    UnsupportedFootprint,
    InsufficientClearance,
    Water,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationBuildError {
    EmptyWorld,
    NoSolidSurface,
    InvalidAgent(&'static str),
    BoundsOverflow,
    ExtractionLimitExceeded { candidates: usize, limit: usize },
}

impl std::fmt::Display for NavigationBuildError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyWorld => write!(formatter, "the world contains no active voxels"),
            Self::NoSolidSurface => {
                write!(formatter, "the world contains no solid navigation surface")
            }
            Self::InvalidAgent(reason) => write!(formatter, "invalid navigation agent: {reason}"),
            Self::BoundsOverflow => {
                write!(formatter, "navigation bounds exceed supported coordinates")
            }
            Self::ExtractionLimitExceeded { candidates, limit } => write!(
                formatter,
                "navigation extraction requires {candidates} candidates (limit {limit})"
            ),
        }
    }
}

impl std::error::Error for NavigationBuildError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathQueryOptions {
    pub max_visited_nodes: usize,
}

impl Default for PathQueryOptions {
    fn default() -> Self {
        Self {
            max_visited_nodes: DEFAULT_MAX_VISITED_NODES,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathFailure {
    InvalidStart {
        position: WorldCoord,
    },
    InvalidTarget {
        position: WorldCoord,
    },
    StartBlocked {
        position: WorldCoord,
        reason: BlockedReason,
    },
    TargetBlocked {
        position: WorldCoord,
        reason: BlockedReason,
    },
    Unreachable,
    SearchLimitExceeded {
        visited: usize,
        limit: usize,
    },
}

impl std::fmt::Display for PathFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStart { position } => write!(
                formatter,
                "start {position:?} is outside the navigation snapshot"
            ),
            Self::InvalidTarget { position } => write!(
                formatter,
                "target {position:?} is outside the navigation snapshot"
            ),
            Self::StartBlocked { position, reason } => {
                write!(formatter, "start {position:?} is blocked: {reason:?}")
            }
            Self::TargetBlocked { position, reason } => {
                write!(formatter, "target {position:?} is blocked: {reason:?}")
            }
            Self::Unreachable => write!(formatter, "target is unreachable from the start"),
            Self::SearchLimitExceeded { visited, limit } => write!(
                formatter,
                "path search visited {visited} nodes (limit {limit})"
            ),
        }
    }
}

impl std::error::Error for PathFailure {}

#[derive(Clone, Debug, PartialEq)]
pub struct NavigationPath {
    pub nodes: Vec<WorldCoord>,
    pub total_cost: u32,
    pub visited_nodes: usize,
    polygon_points: Option<Vec<Vec3>>,
}

impl NavigationPath {
    pub fn world_points(&self) -> Vec<Vec3> {
        self.nodes
            .iter()
            .map(|node| Vec3::new(node.x as f32 + 0.5, node.y as f32, node.z as f32 + 0.5))
            .collect()
    }

    pub fn simplified_nodes(&self) -> Vec<WorldCoord> {
        if self.nodes.len() <= 2 {
            return self.nodes.clone();
        }

        let mut simplified = Vec::with_capacity(self.nodes.len());
        simplified.push(self.nodes[0]);

        for index in 1..self.nodes.len() - 1 {
            let previous = self.nodes[index - 1];
            let current = self.nodes[index];
            let next = self.nodes[index + 1];
            let incoming = (
                current.x - previous.x,
                current.y - previous.y,
                current.z - previous.z,
            );
            let outgoing = (next.x - current.x, next.y - current.y, next.z - current.z);

            if incoming != outgoing {
                simplified.push(current);
            }
        }

        simplified.push(*self.nodes.last().expect("path has at least three nodes"));
        simplified
    }

    pub fn waypoints(&self) -> Vec<Vec3> {
        if let Some(points) = &self.polygon_points {
            return points.clone();
        }
        self.simplified_nodes()
            .into_iter()
            .map(|node| Vec3::new(node.x as f32 + 0.5, node.y as f32, node.z as f32 + 0.5))
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct NavigationGrid {
    polygon: polygon::PolygonNavigation,
    agent: NavigationAgent,
    bounds: NavigationBounds,
    walkable: BTreeSet<WorldCoord>,
    blocked: BTreeMap<WorldCoord, BlockedReason>,
    terrain_heights: BTreeMap<WorldCoord, f32>,
    terrain_settings: Option<NavMeshGenerationSettings>,
}

impl NavigationGrid {
    pub fn build_for_project(
        world: &World,
        agent: NavigationAgent,
    ) -> Result<Self, NavigationBuildError> {
        if world.navmesh_settings.global_navigation {
            Self::build(world, agent)
        } else {
            Self::build_from_volumes(world, agent, &world.navmesh_volumes, world.navmesh_settings)
        }
    }

    pub fn build(world: &World, agent: NavigationAgent) -> Result<Self, NavigationBuildError> {
        Self::build_with_limit(world, agent, DEFAULT_MAX_EXTRACTION_CELLS)
    }

    pub fn build_with_limit(
        world: &World,
        agent: NavigationAgent,
        max_candidates: usize,
    ) -> Result<Self, NavigationBuildError> {
        let agent = agent.validate()?;
        let mut source_cells = 0_usize;
        let mut candidates = BTreeSet::new();
        for support in world.iter_active_effective_coords() {
            source_cells += 1;
            if !world.is_cell_solid(support) {
                continue;
            }
            let y = support
                .y
                .checked_add(1)
                .ok_or(NavigationBuildError::BoundsOverflow)?;
            for dz in agent.footprint_min()..=agent.footprint_max() {
                for dx in agent.footprint_min()..=agent.footprint_max() {
                    let x = support
                        .x
                        .checked_sub(dx)
                        .ok_or(NavigationBuildError::BoundsOverflow)?;
                    let z = support
                        .z
                        .checked_sub(dz)
                        .ok_or(NavigationBuildError::BoundsOverflow)?;
                    candidates.insert(WorldCoord::new(x, y, z));
                    if candidates.len() > max_candidates {
                        return Err(NavigationBuildError::ExtractionLimitExceeded {
                            candidates: candidates.len(),
                            limit: max_candidates,
                        });
                    }
                }
            }
        }
        if source_cells == 0
            && (world.terrain.effective().is_some() || !geometry::actor_colliders(world).is_empty())
        {
            let mut settings = world.navmesh_settings;
            settings.global_navigation = true;
            let polygon = polygon::PolygonNavigation::build(
                world,
                agent,
                &[],
                settings,
                agent.width_cells as f32 * 0.5 - 0.1,
                agent.height_cells as f32 - 0.2,
            )?;
            let (bounds, walkable, terrain_heights) = polygon.debug_samples(max_candidates)?;
            return Ok(Self {
                polygon,
                agent,
                bounds,
                walkable,
                blocked: BTreeMap::new(),
                terrain_heights,
                terrain_settings: Some(settings),
            });
        }
        if source_cells == 0 {
            return Err(NavigationBuildError::EmptyWorld);
        }
        let Some(first) = candidates.first().copied() else {
            return Err(NavigationBuildError::NoSolidSurface);
        };
        let mut min = first;
        let mut max = first;
        for candidate in &candidates {
            min.x = min.x.min(candidate.x);
            min.y = min.y.min(candidate.y);
            min.z = min.z.min(candidate.z);
            max.x = max.x.max(candidate.x);
            max.y = max.y.max(candidate.y);
            max.z = max.z.max(candidate.z);
        }
        let bounds = NavigationBounds { min, max };

        let mut walkable = BTreeSet::new();
        let mut blocked = BTreeMap::new();
        for position in candidates {
            let (_, classification) = classify(world, agent, position);
            match classification {
                None => {
                    walkable.insert(position);
                }
                Some(reason) => {
                    blocked.insert(position, reason);
                }
            }
        }

        let mut settings = world.navmesh_settings;
        settings.global_navigation = true;
        let polygon = polygon::PolygonNavigation::build(
            world,
            agent,
            &[],
            settings,
            agent.width_cells as f32 * 0.5 - 0.1,
            agent.height_cells as f32 - 0.2,
        )?;
        Ok(Self {
            polygon,
            agent,
            bounds,
            walkable,
            blocked,
            terrain_heights: BTreeMap::new(),
            terrain_settings: None,
        })
    }

    pub fn route_waypoints(&self, path: &NavigationPath) -> Vec<Vec3> {
        if let Some(points) = &path.polygon_points {
            return points.clone();
        }
        path.simplified_nodes()
            .into_iter()
            .map(|node| {
                Vec3::new(
                    node.x as f32 + 0.5,
                    self.terrain_heights
                        .get(&node)
                        .copied()
                        .unwrap_or(node.y as f32),
                    node.z as f32 + 0.5,
                )
            })
            .collect()
    }

    pub fn project_endpoint(&self, requested: WorldCoord) -> WorldCoord {
        requested
    }

    pub fn statistics(&self) -> NavigationStatistics {
        self.polygon.stats
    }
    pub fn refresh_collision(&mut self, world: &World) -> Result<(), NavigationBuildError> {
        self.polygon.refresh(world)
    }
    pub(crate) fn find_continuous_path(
        &self,
        start: Vec3,
        target: Vec3,
    ) -> Result<NavigationPath, PathFailure> {
        self.polygon
            .find_path(start, target, PathQueryOptions::default())
    }
    pub(crate) fn find_continuous_path_in_range(
        &self,
        start: Vec3,
        target: Vec3,
        stop_distance: f32,
    ) -> Result<NavigationPath, PathFailure> {
        self.polygon
            .find_path_in_range(start, target, PathQueryOptions::default(), stop_distance)
    }
    pub(crate) fn build_for_character(
        world: &World,
        agent: NavigationAgent,
        radius: f32,
        height: f32,
    ) -> Result<Self, NavigationBuildError> {
        let mut settings = world.navmesh_settings.validate()?;
        settings.max_slope_degrees = settings
            .max_slope_degrees
            .min(world.max_walkable_slope_degrees);
        let polygon = polygon::PolygonNavigation::build(
            world,
            agent,
            &world.navmesh_volumes,
            settings,
            radius,
            height,
        )?;
        let bounds = polygon.bounds();
        Ok(Self {
            polygon,
            agent,
            bounds,
            walkable: BTreeSet::new(),
            blocked: BTreeMap::new(),
            terrain_heights: BTreeMap::new(),
            terrain_settings: Some(settings),
        })
    }

    pub(crate) fn prepare_character_build(
        world: &World,
        agent: NavigationAgent,
        radius: f32,
        height: f32,
        cells: std::sync::Arc<CellSources>,
    ) -> Result<NavigationBuildCapture, NavigationBuildError> {
        let mut settings = world.navmesh_settings.validate()?;
        settings.max_slope_degrees = settings
            .max_slope_degrees
            .min(world.max_walkable_slope_degrees);
        let polygon = polygon::PolygonNavigation::prepare(
            world,
            agent,
            &world.navmesh_volumes,
            settings,
            radius,
            height,
            cells,
        )?;
        let bounds = polygon.bounds();
        let grid = Self {
            polygon,
            agent,
            bounds,
            walkable: BTreeSet::new(),
            blocked: BTreeMap::new(),
            terrain_heights: BTreeMap::new(),
            terrain_settings: Some(settings),
        };
        Ok(NavigationBuildCapture {
            grid,
            water: polygon::BakeWater::new(world),
        })
    }
    pub(crate) fn prepare_collision_refresh(
        mut self,
        world: &World,
        sources: std::sync::Arc<CellSources>,
    ) -> Result<NavigationBuildCapture, NavigationBuildError> {
        self.polygon.defer_refresh(world, sources)?;
        Ok(NavigationBuildCapture {
            grid: self,
            water: polygon::BakeWater::new(world),
        })
    }

    pub fn debug_surface_height(&self, position: WorldCoord) -> f32 {
        self.terrain_heights
            .get(&position)
            .copied()
            .unwrap_or(position.y as f32)
    }

    pub fn agent(&self) -> NavigationAgent {
        self.agent
    }

    pub fn bounds(&self) -> NavigationBounds {
        self.bounds
    }

    pub fn walkable_positions(&self) -> impl ExactSizeIterator<Item = WorldCoord> + '_ {
        self.walkable.iter().copied()
    }

    pub fn blocked_positions(
        &self,
    ) -> impl ExactSizeIterator<Item = (WorldCoord, BlockedReason)> + '_ {
        self.blocked
            .iter()
            .map(|(position, reason)| (*position, *reason))
    }

    pub fn is_walkable(&self, position: WorldCoord) -> bool {
        self.walkable.contains(&position)
    }

    pub fn find_path(
        &self,
        start: WorldCoord,
        target: WorldCoord,
        options: PathQueryOptions,
    ) -> Result<NavigationPath, PathFailure> {
        self.polygon.find_path(
            Vec3::new(start.x as f32 + 0.5, start.y as f32, start.z as f32 + 0.5),
            Vec3::new(
                target.x as f32 + 0.5,
                target.y as f32,
                target.z as f32 + 0.5,
            ),
            options,
        )
    }
}

fn classify(
    world: &World,
    agent: NavigationAgent,
    position: WorldCoord,
) -> (bool, Option<BlockedReason>) {
    let mut any_support = false;
    let mut all_supported = true;
    let mut clearance = true;
    for dz in agent.footprint_min()..=agent.footprint_max() {
        for dx in agent.footprint_min()..=agent.footprint_max() {
            let Some(x) = position.x.checked_add(dx) else {
                return (true, Some(BlockedReason::InsufficientClearance));
            };
            let Some(z) = position.z.checked_add(dz) else {
                return (true, Some(BlockedReason::InsufficientClearance));
            };
            let Some(floor_y) = position.y.checked_sub(1) else {
                return (true, Some(BlockedReason::UnsupportedFootprint));
            };
            let support = world.is_cell_solid(WorldCoord::new(x, floor_y, z));
            any_support |= support;
            all_supported &= support;
            for height in 0..agent.height_cells {
                let Some(y) = position.y.checked_add(height as i32) else {
                    clearance = false;
                    continue;
                };
                clearance &= !world.is_cell_solid(WorldCoord::new(x, y, z));
            }
        }
    }
    if !all_supported {
        (any_support, Some(BlockedReason::UnsupportedFootprint))
    } else if !clearance {
        (true, Some(BlockedReason::InsufficientClearance))
    } else {
        (true, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CellType;

    fn floor(
        world: &mut World,
        x: std::ops::RangeInclusive<i32>,
        z: std::ops::RangeInclusive<i32>,
    ) {
        for z in z {
            for x in x.clone() {
                world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
            }
        }
    }

    #[test]
    fn open_area_path_has_no_manhattan_bend() {
        let mut world = World::new();
        floor(&mut world, 0..=8, 0..=8);
        let nav = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        let start = WorldCoord::new(1, 1, 1);
        let goal = WorldCoord::new(7, 1, 7);
        let path = nav
            .find_path(start, goal, PathQueryOptions::default())
            .unwrap();
        let points = path.waypoints();
        assert_eq!(
            points.len(),
            2,
            "open collision-free plane must travel directly, not along orthogonal grid segments: {points:?}"
        );
    }

    #[test]
    fn finds_a_deterministic_path_across_flat_ground() {
        let mut world = World::new();
        floor(&mut world, 0..=3, 0..=2);
        world.set_cell(WorldCoord::new(1, 1, 1), CellType::Block);
        world.set_cell(WorldCoord::new(1, 2, 1), CellType::Block);
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        let start = WorldCoord::new(0, 1, 1);
        let target = WorldCoord::new(3, 1, 1);
        let first = grid
            .find_path(start, target, PathQueryOptions::default())
            .unwrap();
        let second = grid
            .find_path(start, target, PathQueryOptions::default())
            .unwrap();
        assert_eq!(first.nodes, second.nodes);
        assert_eq!(first.nodes.first(), Some(&start));
        assert_eq!(first.nodes.last(), Some(&target));
        assert!(!first.nodes.contains(&WorldCoord::new(1, 1, 1)));
    }

    #[test]
    fn simplifies_straight_runs_without_cutting_route_corners() {
        let path = NavigationPath {
            nodes: vec![
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(1, 1, 0),
                WorldCoord::new(2, 1, 0),
                WorldCoord::new(2, 1, 1),
                WorldCoord::new(2, 1, 2),
                WorldCoord::new(3, 1, 2),
            ],
            total_cost: 50,
            visited_nodes: 6,
            polygon_points: None,
        };

        assert_eq!(
            path.simplified_nodes(),
            vec![
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(2, 1, 0),
                WorldCoord::new(2, 1, 2),
                WorldCoord::new(3, 1, 2),
            ]
        );
        assert_eq!(
            path.waypoints(),
            vec![
                Vec3::new(0.5, 1.0, 0.5),
                Vec3::new(2.5, 1.0, 0.5),
                Vec3::new(2.5, 1.0, 2.5),
                Vec3::new(3.5, 1.0, 2.5),
            ]
        );
    }

    #[test]
    fn simplification_preserves_elevation_transitions() {
        let path = NavigationPath {
            nodes: vec![
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(1, 1, 0),
                WorldCoord::new(2, 2, 0),
                WorldCoord::new(3, 2, 0),
            ],
            total_cost: 31,
            visited_nodes: 4,
            polygon_points: None,
        };

        assert_eq!(path.simplified_nodes(), path.nodes);
    }

    #[test]
    fn applies_step_up_and_down_constraints() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);
        world.set_cell(WorldCoord::new(1, 1, 0), CellType::Block);
        let can_step = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        assert!(
            can_step
                .find_path(
                    WorldCoord::new(0, 1, 0),
                    WorldCoord::new(1, 2, 0),
                    PathQueryOptions::default()
                )
                .is_ok()
        );
        let cannot_step = NavigationGrid::build(
            &world,
            NavigationAgent {
                max_step_up: 0,
                ..NavigationAgent::default()
            },
        )
        .unwrap();
        assert_eq!(
            cannot_step.find_path(
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(1, 2, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::Unreachable)
        );

        let cannot_drop = NavigationGrid::build(
            &world,
            NavigationAgent {
                max_step_down: 0,
                ..NavigationAgent::default()
            },
        )
        .unwrap();
        assert_eq!(
            cannot_drop.find_path(
                WorldCoord::new(1, 2, 0),
                WorldCoord::new(0, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::Unreachable)
        );
    }

    #[test]
    fn rejects_insufficient_head_clearance() {
        let mut world = World::new();
        floor(&mut world, 0..=2, 0..=0);
        world.set_cell(WorldCoord::new(1, 2, 0), CellType::Block);
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        assert_eq!(
            grid.find_path(
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(1, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::TargetBlocked {
                position: WorldCoord::new(1, 1, 0),
                reason: BlockedReason::InsufficientClearance
            })
        );
    }

    #[test]
    fn rejects_a_wide_agent_on_a_narrow_floor() {
        let mut world = World::new();
        floor(&mut world, 0..=2, 0..=0);
        let grid = NavigationGrid::build(
            &world,
            NavigationAgent {
                width_cells: 2,
                ..NavigationAgent::default()
            },
        )
        .unwrap();
        assert!(!grid.is_walkable(WorldCoord::new(1, 1, 0)));
    }

    #[test]
    fn distinguishes_invalid_blocked_and_unreachable_targets() {
        let mut world = World::new();
        floor(&mut world, 0..=4, 0..=0);
        world.set_cell(WorldCoord::new(2, 1, 0), CellType::Block);
        world.set_cell(WorldCoord::new(2, 2, 0), CellType::Block);
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        assert!(matches!(
            grid.find_path(
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(99, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::InvalidTarget { .. })
        ));
        assert!(matches!(
            grid.find_path(
                WorldCoord::new(-99, 1, 0),
                WorldCoord::new(4, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::InvalidStart { .. })
        ));
        assert!(matches!(
            grid.find_path(
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(2, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::TargetBlocked { .. })
        ));
        assert!(matches!(
            grid.find_path(
                WorldCoord::new(2, 1, 0),
                WorldCoord::new(4, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::StartBlocked { .. })
        ));
        assert_eq!(
            grid.find_path(
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(4, 1, 0),
                PathQueryOptions::default()
            ),
            Err(PathFailure::Unreachable)
        );
    }

    #[test]
    fn reports_search_limits() {
        let mut world = World::new();
        floor(&mut world, 0..=10, 0..=0);
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        assert!(matches!(
            grid.find_path(
                WorldCoord::new(0, 1, 0),
                WorldCoord::new(10, 1, 0),
                PathQueryOptions {
                    max_visited_nodes: 1
                }
            ),
            Err(PathFailure::SearchLimitExceeded { .. })
        ));
    }

    #[test]
    fn exposes_walkable_and_blocked_debug_data_in_sorted_order() {
        let mut world = World::new();
        floor(&mut world, 0..=1, 0..=0);
        world.set_cell(WorldCoord::new(1, 2, 0), CellType::Block);
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        assert_eq!(
            grid.walkable_positions().collect::<Vec<_>>(),
            vec![WorldCoord::new(0, 1, 0), WorldCoord::new(1, 3, 0)]
        );
        assert_eq!(
            grid.blocked_positions().collect::<Vec<_>>(),
            vec![(
                WorldCoord::new(1, 1, 0),
                BlockedReason::InsufficientClearance
            )]
        );
    }

    #[test]
    fn validates_empty_world_agent_and_extraction_limit() {
        assert_eq!(
            NavigationGrid::build(&World::new(), NavigationAgent::default()).unwrap_err(),
            NavigationBuildError::EmptyWorld
        );
        let mut world = World::new();
        floor(&mut world, 0..=1, 0..=1);
        assert!(matches!(
            NavigationGrid::build(
                &world,
                NavigationAgent {
                    width_cells: 0,
                    ..NavigationAgent::default()
                }
            ),
            Err(NavigationBuildError::InvalidAgent(_))
        ));
        assert!(matches!(
            NavigationGrid::build_with_limit(&world, NavigationAgent::default(), 1),
            Err(NavigationBuildError::ExtractionLimitExceeded { .. })
        ));
    }
}
