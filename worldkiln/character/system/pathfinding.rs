use super::{CachedNavigationGrid, CharacterSystem};
use crate::engine::entity::EntityId;
use crate::navigation::{
    NavigationAgent, NavigationBuildError, NavigationGrid, PathFailure, PathQueryOptions,
};
use crate::world::{World, WorldCoord};
use glam::Vec3;

impl CharacterSystem {
    /// Returns a navigation path from the character to the target position.
    ///
    /// The result contains world-space voxel-center waypoints.
    /// AeoScript is responsible for deciding how to follow the path.
    pub fn find_path_for_entity(
        &self,
        entity_id: EntityId,
        world: &World,
        target: Vec3,
    ) -> Option<Vec<Vec3>> {
        self.find_path_for_entity_detailed(entity_id, world, target)
            .ok()
    }

    pub(crate) fn find_path_for_entity_detailed(
        &self,
        entity_id: EntityId,
        world: &World,
        target: Vec3,
    ) -> Result<Vec<Vec3>, NavigationPathQueryError> {
        let character = self
            .get_character_for_entity(entity_id)
            .ok_or(NavigationPathQueryError::NotACharacter)?;

        let start = world_position_to_navigation_cell(character.transform.position);
        let goal = world_position_to_navigation_cell(target);
        let agent = NavigationAgent::default();
        // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
        let revision = world.render_revision();
        let terrain_revision = world.terrain.render_revision()
            ^ (world.terrain.config.plains.water_level.to_bits() as u64).rotate_left(13);

        let cache_is_current = self
            .navigation_cache
            .borrow()
            .as_ref()
            .is_some_and(|cache| {
                cache.world_revision == revision
                    && cache.terrain_revision == terrain_revision
                    && cache.agent == agent
            });
        if !cache_is_current {
            // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
            let grid = if world.navmesh_volumes.is_empty() {
                NavigationGrid::build(world, agent)
            } else {
                NavigationGrid::build_from_volumes(
                    world,
                    agent,
                    &world.navmesh_volumes,
                    world.navmesh_settings,
                )
            }
            .map_err(NavigationPathQueryError::Build)?;
            *self.navigation_cache.borrow_mut() = Some(CachedNavigationGrid {
                world_revision: revision,
                terrain_revision,
                agent,
                grid,
            });
        }

        let cache = self.navigation_cache.borrow();
        let grid = &cache
            .as_ref()
            .ok_or(NavigationPathQueryError::NavigationUnavailable)?
            .grid;
        let start = grid.project_endpoint(start);
        let goal = grid.project_endpoint(goal);
        let path = grid
            .find_path(start, goal, PathQueryOptions::default())
            .map_err(NavigationPathQueryError::Path)?;
        Ok(grid.route_waypoints(&path).into_iter().skip(1).collect())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NavigationPathQueryError {
    NotACharacter,
    NavigationUnavailable,
    Build(NavigationBuildError),
    Path(PathFailure),
}

impl NavigationPathQueryError {
    pub(crate) fn reason_code(&self) -> &'static str {
        match self {
            Self::NotACharacter => "not_a_character",
            Self::NavigationUnavailable => "navigation_unavailable",
            Self::Build(_) => "navigation_unavailable",
            Self::Path(PathFailure::InvalidStart { .. }) => "invalid_start",
            Self::Path(PathFailure::InvalidTarget { .. }) => "invalid_target",
            Self::Path(PathFailure::StartBlocked { .. }) => "start_blocked",
            Self::Path(PathFailure::TargetBlocked { .. }) => "target_blocked",
            Self::Path(PathFailure::Unreachable) => "unreachable",
            Self::Path(PathFailure::SearchLimitExceeded { .. }) => "search_limit_exceeded",
        }
    }
}

/// Maps a continuous world position to the voxel cell containing it.
///
/// Horizontal voxel cells occupy `[coordinate, coordinate + 1)`, so floor is
/// correct for both positive and negative coordinates. Character Y is its
/// bottom position and follows the same convention at step elevations.
fn world_position_to_navigation_cell(position: Vec3) -> WorldCoord {
    WorldCoord::new(
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CellType;

    #[test]
    fn world_positions_map_to_the_containing_cell_on_both_sides_of_zero() {
        assert_eq!(
            world_position_to_navigation_cell(Vec3::new(4.5, 1.0, 8.99)),
            WorldCoord::new(4, 1, 8)
        );
        assert_eq!(
            world_position_to_navigation_cell(Vec3::new(-4.5, 1.0, -8.01)),
            WorldCoord::new(-5, 1, -9)
        );
    }

    #[test]
    fn path_queries_return_cell_centers_and_refresh_after_world_changes() {
        let mut world = World::new();
        for x in 0..=2 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }

        let mut system = CharacterSystem::new();
        let entity_id = EntityId(77);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 0.5), None);
        system.associate_entity(entity_id, character_id);

        let path = system
            .find_path_for_entity(entity_id, &world, Vec3::new(2.5, 1.0, 0.5))
            .unwrap();
        assert_eq!(path.last(), Some(&Vec3::new(2.5, 1.0, 0.5)));
        assert_eq!(
            system
                .navigation_cache
                .borrow()
                .as_ref()
                .unwrap()
                .world_revision,
            world.render_revision()
        );

        world.set_cell(WorldCoord::new(2, 1, 0), CellType::Block);
        world.set_cell(WorldCoord::new(2, 2, 0), CellType::Block);
        assert!(
            system
                .find_path_for_entity(entity_id, &world, Vec3::new(2.5, 1.0, 0.5))
                .is_none()
        );
        assert_eq!(
            system
                .navigation_cache
                .borrow()
                .as_ref()
                .unwrap()
                .world_revision,
            world.render_revision()
        );
    }

    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09
    #[test]
    fn terrain_volume_paths_use_effective_surface_and_refresh_when_water_changes() {
        let mut world = World::new();
        world.terrain.config.mode = crate::world::TerrainMode::Smooth;
        world.terrain.config.plains.base_height = 4.0;
        world.terrain.config.plains.height_variation = 0.0;
        world.terrain.config.plains.water_level = 2.0;
        world
            .terrain
            .generate_authored(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [12, 12, 12],
            })
            .unwrap();
        world
            .navmesh_volumes
            .push(crate::navigation::NavMeshBoundsVolume::new(
                1,
                [6.0, 5.0, 6.0],
            ));
        let entity_id = EntityId(413);
        let mut system = CharacterSystem::new();
        let character_id = system.spawn_character(Vec3::new(2.5, 4.0, 2.5), None);
        system.associate_entity(entity_id, character_id);
        let path = system
            .find_path_for_entity_detailed(entity_id, &world, Vec3::new(9.5, 4.0, 9.5))
            .expect("flat dry terrain in a bounds volume must have a route");
        let end = *path.last().expect("path must end at a waypoint");
        assert_eq!((end.x, end.z), (9.5, 9.5));
        assert!(end.y > 0.0 && end.y < 12.0);

        let previous_nav_revision = world.navigation_revision();
        world.terrain.config.plains.water_level = 8.0;
        assert_ne!(world.navigation_revision(), previous_nav_revision);
        assert!(
            system
                .find_path_for_entity(entity_id, &world, Vec3::new(9.5, 4.0, 9.5))
                .is_none(),
            "raising water must invalidate cached walkable terrain even without a voxel change"
        );
    }

    #[test]
    fn detailed_queries_preserve_script_facing_failure_reasons() {
        let mut world = World::new();
        for x in 0..=2 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        world.set_cell(WorldCoord::new(2, 1, 0), CellType::Block);
        world.set_cell(WorldCoord::new(2, 2, 0), CellType::Block);

        let mut system = CharacterSystem::new();
        let entity_id = EntityId(78);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 0.5), None);
        system.associate_entity(entity_id, character_id);

        let blocked = system
            .find_path_for_entity_detailed(entity_id, &world, Vec3::new(2.5, 1.0, 0.5))
            .unwrap_err();
        assert_eq!(blocked.reason_code(), "target_blocked");

        let invalid = system
            .find_path_for_entity_detailed(entity_id, &world, Vec3::new(99.5, 1.0, 99.5))
            .unwrap_err();
        assert_eq!(invalid.reason_code(), "invalid_target");

        let empty_world = World::new();
        let unavailable = system
            .find_path_for_entity_detailed(entity_id, &empty_world, Vec3::ZERO)
            .unwrap_err();
        assert_eq!(unavailable.reason_code(), "navigation_unavailable");
    }
}
