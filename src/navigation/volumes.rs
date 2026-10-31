// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NavMeshBoundsVolume {
    pub id: u64,
    pub center: [f32; 3],
    pub size: [f32; 3],
    #[serde(default = "default_true")]
    pub enabled: bool,
}
fn default_true() -> bool {
    true
}
impl NavMeshBoundsVolume {
    pub fn new(id: u64, center: [f32; 3]) -> Self {
        Self {
            id,
            center,
            size: [32.0, 24.0, 32.0],
            enabled: true,
        }
    }
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        if self
            .center
            .iter()
            .chain(self.size.iter())
            .any(|x| !x.is_finite())
            || self.size.iter().any(|x| *x <= 0.0)
        {
            return None;
        }
        Some((
            std::array::from_fn(|i| self.center[i] - self.size[i] * 0.5),
            std::array::from_fn(|i| self.center[i] + self.size[i] * 0.5),
        ))
    }
    pub(crate) fn contains(&self, p: [f32; 3]) -> bool {
        self.bounds()
            .is_some_and(|(lo, hi)| (0..3).all(|i| p[i] >= lo[i] && p[i] < hi[i]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshGenerationSettings {
    pub max_slope_degrees: f32,
    pub max_step_height: f32,
    pub max_water_depth: f32,
    #[serde(default)]
    pub global_navigation: bool,
}
impl Default for NavMeshGenerationSettings {
    fn default() -> Self {
        Self {
            max_slope_degrees: 35.0,
            max_step_height: 1.0,
            max_water_depth: 0.0,
            global_navigation: false,
        }
    }
}
impl NavMeshGenerationSettings {
    pub(super) fn validate(self) -> Result<Self, NavigationBuildError> {
        if !self.max_slope_degrees.is_finite()
            || !(0.0..89.0).contains(&self.max_slope_degrees)
            || !self.max_step_height.is_finite()
            || self.max_step_height < 0.0
            || !self.max_water_depth.is_finite()
            || self.max_water_depth < 0.0
        {
            return Err(NavigationBuildError::InvalidAgent(
                "invalid slope, step height, or water depth",
            ));
        }
        Ok(self)
    }
}
impl NavigationGrid {
    pub fn build_from_volumes(
        world: &World,
        agent: NavigationAgent,
        volumes: &[NavMeshBoundsVolume],
        settings: NavMeshGenerationSettings,
    ) -> Result<Self, NavigationBuildError> {
        let agent = agent.validate()?;
        let settings = settings.validate()?;
        let active: Vec<_> = volumes.iter().filter(|v| v.enabled).collect();
        if active.is_empty() {
            return Err(NavigationBuildError::InvalidAgent(
                "add an enabled bounds volume",
            ));
        }
        if active.iter().any(|v| v.bounds().is_none()) {
            return Err(NavigationBuildError::InvalidAgent("invalid bounds volume"));
        }
        let mut columns = BTreeSet::new();
        for volume in &active {
            let (min, max) = volume.bounds().expect("validated");
            let [x0, z0, x1, z1] = [
                (min[0] - 0.5).ceil() as i64,
                (min[2] - 0.5).ceil() as i64,
                (max[0] - 0.5).ceil() as i64,
                (max[2] - 0.5).ceil() as i64,
            ];
            if x0 < i32::MIN as i64
                || z0 < i32::MIN as i64
                || x1 >= i32::MAX as i64
                || z1 >= i32::MAX as i64
            {
                return Err(NavigationBuildError::BoundsOverflow);
            }
            let area = ((x1 - x0).max(0) as u64).saturating_mul((z1 - z0).max(0) as u64);
            if area > DEFAULT_MAX_EXTRACTION_CELLS as u64 {
                return Err(NavigationBuildError::ExtractionLimitExceeded {
                    candidates: area.min(usize::MAX as u64) as usize,
                    limit: DEFAULT_MAX_EXTRACTION_CELLS,
                });
            }
            for z in z0..z1 {
                for x in x0..x1 {
                    columns.insert((x as i32, z as i32));
                    if columns.len() > DEFAULT_MAX_EXTRACTION_CELLS {
                        return Err(NavigationBuildError::ExtractionLimitExceeded {
                            candidates: columns.len(),
                            limit: DEFAULT_MAX_EXTRACTION_CELLS,
                        });
                    }
                }
            }
        }
        let mut candidates = BTreeSet::new();
        let mut surface_heights = BTreeMap::new();
        for (x, z) in columns {
            let (wx, wz) = (x as f32 + 0.5, z as f32 + 0.5);
            if let Some(height) = world.terrain.surface_height_at(wx, wz) {
                if height.is_finite()
                    && height >= i32::MIN as f32
                    && height < i32::MAX as f32
                    && active.iter().any(|v| v.contains([wx, height, wz]))
                {
                    let pos = WorldCoord::new(x, height.floor() as i32, z);
                    candidates.insert(pos);
                    surface_heights.insert(pos, height);
                }
            }
        }
        for support in world.iter_active_effective_coords() {
            if !world.is_cell_solid(support) {
                continue;
            }
            let Some(y) = support.y.checked_add(1) else {
                continue;
            };
            for dz in agent.footprint_min()..=agent.footprint_max() {
                for dx in agent.footprint_min()..=agent.footprint_max() {
                    let (Some(x), Some(z)) = (support.x.checked_sub(dx), support.z.checked_sub(dz))
                    else {
                        continue;
                    };
                    if active
                        .iter()
                        .any(|v| v.contains([x as f32 + 0.5, y as f32, z as f32 + 0.5]))
                    {
                        candidates.insert(WorldCoord::new(x, y, z));
                        if candidates.len() > DEFAULT_MAX_EXTRACTION_CELLS {
                            return Err(NavigationBuildError::ExtractionLimitExceeded {
                                candidates: candidates.len(),
                                limit: DEFAULT_MAX_EXTRACTION_CELLS,
                            });
                        }
                    }
                }
            }
        }
        let Some(first) = candidates.first().copied() else {
            return Err(NavigationBuildError::NoSolidSurface);
        };
        let (mut min, mut max) = (first, first);
        let water_samples = world
            .terrain
            .config
            .water_bodies
            .iter()
            .filter(|body| body.visible)
            .map(|body| (body, body.spline_samples()))
            .collect::<Vec<_>>();
        let mut walkable = BTreeSet::new();
        let mut blocked = BTreeMap::new();
        for pos in candidates {
            min.x = min.x.min(pos.x);
            min.y = min.y.min(pos.y);
            min.z = min.z.min(pos.z);
            max.x = max.x.max(pos.x);
            max.y = max.y.max(pos.y);
            max.z = max.z.max(pos.z);
            let reason = if let Some(&h) = surface_heights.get(&pos) {
                classify_terrain_cached(world, agent, settings, pos, h, &water_samples)
            } else {
                classify(world, agent, pos).1
            };
            if let Some(reason) = reason {
                blocked.insert(pos, reason);
            } else {
                walkable.insert(pos);
            }
        }
        let polygon = polygon::PolygonNavigation::build(
            world,
            agent,
            volumes,
            settings,
            agent.width_cells as f32 * 0.5 - 0.1,
            agent.height_cells as f32 - 0.2,
        )?;
        Ok(Self {
            polygon,
            agent,
            bounds: NavigationBounds { min, max },
            walkable,
            blocked,
            terrain_heights: surface_heights,
            terrain_settings: Some(settings),
        })
    }
}
#[cfg(test)]
fn classify_terrain(
    world: &World,
    agent: NavigationAgent,
    settings: NavMeshGenerationSettings,
    pos: WorldCoord,
    h: f32,
) -> Option<BlockedReason> {
    let samples = world
        .terrain
        .config
        .water_bodies
        .iter()
        .filter(|body| body.visible)
        .map(|body| (body, body.spline_samples()))
        .collect::<Vec<_>>();
    classify_terrain_cached(world, agent, settings, pos, h, &samples)
}
fn classify_terrain_cached(
    world: &World,
    agent: NavigationAgent,
    settings: NavMeshGenerationSettings,
    pos: WorldCoord,
    h: f32,
    water_samples: &[(&crate::world::terrain::WaterBody, Vec<([f32; 2], f32, f32)>)],
) -> Option<BlockedReason> {
    if world.terrain.config.ocean_enabled
        && world.terrain.config.plains.water_level > h + settings.max_water_depth + 0.001
    {
        return Some(BlockedReason::Water);
    }
    let sample_x = pos.x as f32 + 0.5;
    let sample_z = pos.z as f32 + 0.5;
    if water_samples.iter().any(|(body, samples)| {
        body.vertical_bounds_at_samples(sample_x, sample_z, h, samples)
            .is_some_and(|(bottom, top)| top - bottom > settings.max_water_depth + 0.001)
    }) {
        return Some(BlockedReason::Water);
    }
    for dz in agent.footprint_min()..=agent.footprint_max() {
        for dx in agent.footprint_min()..=agent.footprint_max() {
            let (Some(x), Some(z)) = (pos.x.checked_add(dx), pos.z.checked_add(dz)) else {
                return Some(BlockedReason::InsufficientClearance);
            };
            let Some(floor) = world
                .terrain
                .surface_height_at(x as f32 + 0.5, z as f32 + 0.5)
            else {
                return Some(BlockedReason::UnsupportedFootprint);
            };
            if (floor - h).abs() > settings.max_step_height + 0.001 {
                return Some(BlockedReason::UnsupportedFootprint);
            }
            let bottom = h.floor() as i32;
            let top = (h + agent.height_cells as f32).ceil() as i32;
            for y in bottom..top {
                if world.is_cell_solid(WorldCoord::new(x, y, z)) {
                    return Some(BlockedReason::InsufficientClearance);
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::TerrainGenerationRequest;
    fn terrain() -> World {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 4.0;
        world.terrain.config.plains.height_variation = 0.0;
        world.terrain.config.plains.water_level = 2.0;
        world
            .terrain
            .generate_authored(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [12, 12, 12],
            })
            .unwrap();
        world
    }
    #[test]
    fn navigation_blocks_local_water_volume_only_inside_contour() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut world = terrain();
        world.terrain.config.ocean_enabled = false;
        let mut lake = WaterBody::new(7, WaterBodyKind::Lake, [6.0, 6.0], 7.5);
        lake.depth = 6.0;
        for point in &mut lake.points {
            for axis in 0..2 {
                point.position[axis] = 6.0 + (point.position[axis] - 6.0) * 0.25;
            }
        }
        world.terrain.config.water_bodies.push(lake);
        let agent = NavigationAgent::default();
        let settings = NavMeshGenerationSettings::default();
        assert_eq!(
            classify_terrain(&world, agent, settings, WorldCoord::new(6, 4, 6), 4.0),
            Some(BlockedReason::Water)
        );
        assert_ne!(
            classify_terrain(&world, agent, settings, WorldCoord::new(0, 4, 0), 4.0),
            Some(BlockedReason::Water)
        );
        world.terrain.config.water_bodies[0].visible = false;
        assert_ne!(
            classify_terrain(&world, agent, settings, WorldCoord::new(6, 4, 6), 4.0),
            Some(BlockedReason::Water)
        );
    }

    #[test]
    fn volume_generates_from_terrain_without_cells() {
        let world = terrain();
        let grid = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[NavMeshBoundsVolume::new(1, [6.0, 5.0, 6.0])],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        assert!(grid.walkable_positions().len() > 0);
        let a = grid.walkable_positions().next().unwrap();
        assert!(grid.is_walkable(a));
    }
    #[test]
    fn submerged_terrain_is_blocked() {
        let mut world = terrain();
        world.terrain.config.plains.water_level = 7.0;
        let grid = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[NavMeshBoundsVolume::new(1, [6.0, 5.0, 6.0])],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        assert!(grid.walkable_positions().len() == 0);
        assert!(
            grid.blocked
                .values()
                .all(|reason| *reason == BlockedReason::Water)
        );
    }
    #[test]
    fn bounds_union_excludes_terrain_outside_each_box() {
        let world = terrain();
        let volume = NavMeshBoundsVolume {
            id: 91,
            center: [4.0, 5.0, 4.0],
            size: [4.0, 12.0, 4.0],
            enabled: true,
        };
        let grid = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[volume],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        assert!(grid.walkable_positions().len() > 0);
        assert!(
            grid.walkable_positions()
                .all(|p| (2..6).contains(&p.x) && (2..6).contains(&p.z))
        );
    }

    #[test]
    fn solid_cell_over_terrain_blocks_clearance() {
        let mut world = terrain();
        let volume = NavMeshBoundsVolume::new(11, [6.0, 5.0, 6.0]);
        let before = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[volume.clone()],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        let position = before.walkable_positions().next().unwrap();
        world.set_cell(position, crate::world::CellType::Block);
        let after = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[volume],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        assert!(!after.is_walkable(position));
        assert_eq!(
            after.blocked.get(&position),
            Some(&BlockedReason::InsufficientClearance)
        );
    }

    #[test]
    fn old_project_settings_without_global_navigation_default_to_bounded_mode() {
        let old = r#"{"max_slope_degrees":35.0,"max_step_height":1.0,"max_water_depth":0.0}"#;
        let parsed: NavMeshGenerationSettings = serde_json::from_str(old).unwrap();
        assert!(!parsed.global_navigation);
        let enabled = NavMeshGenerationSettings {
            global_navigation: true,
            ..parsed
        };
        let roundtrip: NavMeshGenerationSettings =
            serde_json::from_str(&serde_json::to_string(&enabled).unwrap()).unwrap();
        assert!(roundtrip.global_navigation);
    }

    #[test]
    fn save_reload_preserves_multiple_bounds_volumes() {
        use crate::world::persistence::{load_world, save_world};
        let base = std::env::temp_dir().join(format!(
            "worldkiln_navmesh_volume_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("world.dat");
        let mut world = World::new();
        world.navmesh_volumes = vec![
            NavMeshBoundsVolume::new(1, [0.0, 4.0, -12.0]),
            NavMeshBoundsVolume {
                id: 2,
                center: [10.5, 3.0, 14.0],
                size: [70.0, 12.0, 17.0],
                enabled: false,
            },
        ];
        world.navmesh_settings = NavMeshGenerationSettings {
            max_slope_degrees: 38.0,
            max_step_height: 0.6,
            max_water_depth: 0.3,
            global_navigation: true,
        };
        save_world(&world, &path).unwrap();
        let mut loaded = World::new();
        load_world(&mut loaded, &path).unwrap();
        assert_eq!(loaded.navmesh_volumes, world.navmesh_volumes);
        assert_eq!(loaded.navmesh_settings, world.navmesh_settings);
        std::fs::remove_dir_all(base).ok();
    }

    #[test]
    fn terrain_connections_reject_steep_slopes_and_large_steps() {
        let mut world = World::new();
        world.terrain.config.ocean_enabled = false;
        world.terrain.authored = Some(crate::world::TerrainResult {
            request: crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 32, 12],
            },
            generator: crate::world::TerrainGeneratorType::Plains,
            generator_implementation_version: 1,
            data: crate::world::TerrainData::Heightfield(crate::world::HeightfieldData {
                origin: [0, 0, 0],
                size: [16, 32, 12],
                heights: (0..12)
                    .flat_map(|_| (0..16).map(|x| 4.0 + x as f32 * 0.75))
                    .collect(),
            }),
        });
        world.navmesh_settings.max_slope_degrees = 20.0;
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        assert!(
            grid.find_path(
                WorldCoord::new(3, 6, 5),
                WorldCoord::new(12, 13, 5),
                PathQueryOptions::default()
            )
            .is_err(),
            "35+ degree terrain must not connect when agent maximum is 20 degrees"
        );
        let mut world = World::new();
        for x in 0..16 {
            for z in 0..12 {
                world.set_cell(WorldCoord::new(x, 0, z), crate::world::CellType::Block);
            }
        }
        let mut step = crate::world::Actor::new(71, "half-stud step", Vec3::new(12.0, 1.25, 6.0));
        step.physics.collision_size = Vec3::new(8.0, 0.5, 12.0);
        world.actors.insert(step.id, step);
        world.navmesh_settings.max_slope_degrees = 60.0;
        world.navmesh_settings.max_step_height = 0.1;
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        let (a, b) = (WorldCoord::new(3, 1, 5), WorldCoord::new(12, 1, 5));
        assert!(
            grid.find_path(a, b, PathQueryOptions::default()).is_err(),
            "half-stud change must not connect when max step is 0.1"
        );
        world.navmesh_settings.max_step_height = 1.0;
        let grid = NavigationGrid::build(&world, NavigationAgent::default()).unwrap();
        let path = grid.find_path(a, b, PathQueryOptions::default()).unwrap();
        assert!(grid.find_path(a, b, PathQueryOptions::default()).is_ok());
        assert!(
            grid.route_waypoints(&path)
                .iter()
                .any(|p| (p.y - 1.5).abs() < 0.001),
            "route must use the true elevated collision surface"
        );
    }

    #[test]
    fn overlapping_volumes_deduplicate() {
        let world = terrain();
        let volume = NavMeshBoundsVolume::new(1, [6.0, 5.0, 6.0]);
        let a = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[volume.clone()],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        let b = NavigationGrid::build_from_volumes(
            &world,
            NavigationAgent::default(),
            &[volume.clone(), volume],
            NavMeshGenerationSettings::default(),
        )
        .unwrap();
        assert_eq!(a.walkable, b.walkable);
    }
}
