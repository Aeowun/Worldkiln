// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{CachedNavigationGrid, CharacterSystem};
use crate::engine::entity::EntityId;
use crate::navigation::{NavigationBuildError, PathFailure};
use crate::world::{World, WorldCoord};
use glam::Vec3;

impl CharacterSystem {
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
        crate::engine::development_test_run::timed_navigation("find_path_total", || {
            self.find_path_for_entity_with_stop_range(entity_id, world, target, 0.0)
        })
    }

    pub(crate) fn find_path_for_entity_with_stop_range(
        &self,
        entity_id: EntityId,
        world: &World,
        target: Vec3,
        stop_distance: f32,
    ) -> Result<Vec<Vec3>, NavigationPathQueryError> {
        let character = self
            .get_character_for_entity(entity_id)
            .ok_or(NavigationPathQueryError::NotACharacter)?;

        let bottom_offset = character.navigation_bottom_offset();
        let start_position = character.transform.position + Vec3::Y * bottom_offset;
        let start = world_position_to_navigation_cell(start_position);
        let goal = world_position_to_navigation_cell(target);
        if !world.navmesh_settings.global_navigation {
            let active = world
                .navmesh_volumes
                .iter()
                .filter(|volume| volume.enabled && volume.bounds().is_some())
                .collect::<Vec<_>>();
            if active.is_empty() {
                return Err(NavigationPathQueryError::NavigationUnavailable);
            }
            let inside = |position: Vec3| {
                active
                    .iter()
                    .any(|volume| volume.contains([position.x, position.y, position.z]))
            };
            if !inside(start_position) {
                return Err(NavigationPathQueryError::Path(PathFailure::InvalidStart {
                    position: start,
                }));
            }
            if !inside(target) {
                return Err(NavigationPathQueryError::Path(PathFailure::InvalidTarget {
                    position: goal,
                }));
            }
        }
        let policy = navigation_policy(world);
        let mut cache = self.navigation_cache.borrow_mut();
        if cache.as_ref().is_none_or(|c| c.policy != policy) {
            *cache = Some(CachedNavigationGrid {
                policy,
                world_revision: world.render_revision(),
                terrain_revision: world.terrain.render_revision(),
                profiles: Default::default(),
            });
        }
        let cache = cache.as_mut().unwrap();
        let profile = super::navigation_build::profile(
            character.collision.radius,
            character.collision.height,
        )
        .map_err(NavigationPathQueryError::Build)?;
        if !self
            .navigation_builds
            .borrow_mut()
            .request(world, cache, profile)
            .map_err(NavigationPathQueryError::Build)?
        {
            return Err(NavigationPathQueryError::Pending);
        }
        cache.world_revision = world.render_revision();
        cache.terrain_revision = world.terrain.render_revision();
        let grid = cache
            .profiles
            .get(&profile)
            .ok_or(NavigationPathQueryError::Pending)?;
        let path =
            crate::engine::development_test_run::timed_navigation("navmesh_path_query", || {
                if stop_distance > 0.0 {
                    grid.find_continuous_path_in_range(start_position, target, stop_distance)
                } else {
                    grid.find_continuous_path(start_position, target)
                }
            })
            .map_err(NavigationPathQueryError::Path)?;
        Ok(
            crate::engine::development_test_run::timed_navigation(
                "navmesh_route_waypoints",
                || grid.route_waypoints(&path),
            )
            .into_iter()
            .skip(1)
            .map(|p| p - Vec3::Y * bottom_offset)
            .collect(),
        )
    }
}

pub(super) fn navigation_policy(world: &World) -> String {
    format!(
        "{}:{:?}:{:?}:{}",
        world.navigation_world_id,
        world.navmesh_volumes,
        world.navmesh_settings,
        world.max_walkable_slope_degrees
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NavigationPathQueryError {
    NotACharacter,
    Pending,
    NavigationUnavailable,
    Build(NavigationBuildError),
    Path(PathFailure),
}

impl NavigationPathQueryError {
    pub(crate) fn reason_code(&self) -> &'static str {
        match self {
            Self::NotACharacter => "not_a_character",
            Self::NavigationUnavailable | Self::Pending => "navigation_unavailable",
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

fn world_position_to_navigation_cell(position: Vec3) -> WorldCoord {
    WorldCoord::new(
        position.x.floor() as i32,
        position.y.floor() as i32,
        position.z.floor() as i32,
    )
}

#[cfg(test)]
fn prepared_test_path(
    system: &mut CharacterSystem,
    entity_id: EntityId,
    world: &World,
    target: Vec3,
) -> Result<Vec<Vec3>, NavigationPathQueryError> {
    for _ in 0..4096 {
        match system.find_path_for_entity_detailed(entity_id, world, target) {
            Err(NavigationPathQueryError::Pending) => {
                system.tick_navigation_builds(world);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            result => return result,
        }
    }
    Err(NavigationPathQueryError::Pending)
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
    fn project_requires_bounds_by_default_and_never_auto_generates_global_nav() {
        use crate::navigation::NavMeshBoundsVolume;
        let mut world = World::new();
        for x in 0..=5 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        let mut system = CharacterSystem::new();
        let id = EntityId(181);
        let character = system.spawn_character(Vec3::new(0.5, 1.0, 0.5), None);
        system.associate_entity(id, character);

        assert!(!world.navmesh_settings.global_navigation);
        assert_eq!(
            prepared_test_path(&mut system, id, &world, Vec3::new(2.5, 1.0, 0.5)),
            Err(NavigationPathQueryError::NavigationUnavailable)
        );
        assert!(system.navigation_cache.borrow().is_none());

        let mut volume = NavMeshBoundsVolume::new(9, [1.5, 1.5, 0.5]);
        volume.size = [3.0, 3.0, 1.0];
        world.navmesh_volumes.push(volume);
        assert!(
            prepared_test_path(&mut system, id, &world, Vec3::new(2.5, 1.0, 0.5))
                .is_ok()
        );
        assert_eq!(
            prepared_test_path(&mut system, id, &world, Vec3::new(5.5, 1.0, 0.5)),
            Err(NavigationPathQueryError::Path(PathFailure::InvalidTarget {
                position: WorldCoord::new(5, 1, 0)
            }))
        );
        let outside = EntityId(182);
        let outside_character = system.spawn_character(Vec3::new(5.5, 1.0, 0.5), None);
        system.associate_entity(outside, outside_character);
        assert_eq!(
            prepared_test_path(&mut system, outside, &world, Vec3::new(2.5, 1.0, 0.5)),
            Err(NavigationPathQueryError::Path(PathFailure::InvalidStart {
                position: WorldCoord::new(5, 1, 0)
            }))
        );

        world.navmesh_volumes[0].enabled = false;
        assert_eq!(
            prepared_test_path(&mut system, id, &world, Vec3::new(2.5, 1.0, 0.5)),
            Err(NavigationPathQueryError::NavigationUnavailable)
        );

        world.navmesh_settings.global_navigation = true;
        world.bump_render_revision();
        assert!(
            prepared_test_path(&mut system, id, &world, Vec3::new(5.5, 1.0, 0.5))
                .is_ok()
        );
    }

    #[test]
    fn path_queries_return_cell_centers_and_refresh_after_world_changes() {
        let mut world = World::new();
        world.navmesh_settings.global_navigation = true;
        for x in 0..=2 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }

        let mut system = CharacterSystem::new();
        let entity_id = EntityId(77);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 0.5), None);
        system.associate_entity(entity_id, character_id);

        let path = prepared_test_path(&mut system, entity_id, &world, Vec3::new(2.5, 1.0, 0.5))
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
            prepared_test_path(&mut system, entity_id, &world, Vec3::new(2.5, 1.0, 0.5))
                .is_err()
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

    #[test]
    fn terrain_volume_paths_use_effective_surface_and_refresh_when_water_changes() {
        let mut world = World::new();
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
        let path = prepared_test_path(&mut system, entity_id, &world, Vec3::new(9.5, 4.0, 9.5))
            .expect("flat dry terrain in a bounds volume must have a route");
        let end = *path.last().expect("path must end at a waypoint");
        assert_eq!((end.x, end.z), (9.5, 9.5));
        assert!(end.y > 0.0 && end.y < 12.0);

        let previous_nav_revision = world.navigation_revision();
        world.terrain.config.plains.water_level = 8.0;
        assert_ne!(world.navigation_revision(), previous_nav_revision);
        assert!(
            prepared_test_path(&mut system, entity_id, &world, Vec3::new(9.5, 4.0, 9.5))
                .is_err(),
            "raising water must invalidate cached walkable terrain even without a voxel change"
        );
    }

    #[test]
    fn detailed_queries_preserve_script_facing_failure_reasons() {
        let mut world = World::new();
        world.navmesh_settings.global_navigation = true;
        for x in 0..=2 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        world.set_cell(WorldCoord::new(2, 1, 0), CellType::Block);
        world.set_cell(WorldCoord::new(2, 2, 0), CellType::Block);

        let mut system = CharacterSystem::new();
        let entity_id = EntityId(78);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 0.5), None);
        system.associate_entity(entity_id, character_id);

        let blocked = prepared_test_path(&mut system, entity_id, &world, Vec3::new(2.5, 1.0, 0.5))
            .unwrap_err();
        assert_eq!(blocked.reason_code(), "target_blocked");

        let invalid = prepared_test_path(&mut system, entity_id, &world, Vec3::new(99.5, 1.0, 99.5))
            .unwrap_err();
        assert_eq!(invalid.reason_code(), "invalid_target");

        let empty_world = World::new();
        let unavailable = prepared_test_path(&mut system, entity_id, &empty_world, Vec3::ZERO)
            .unwrap_err();
        assert_eq!(unavailable.reason_code(), "navigation_unavailable");
    }
}

#[cfg(test)]
mod polygon_cache_tests {
    use super::*;
    #[test]
    fn heterogeneous_characters_share_profile_bakes_and_render_only_changes_do_not_rebuild() {
        let mut world = World::new();
        world.navmesh_settings.global_navigation = true;
        for x in 0..12 {
            for z in 0..12 {
                world.set_cell(WorldCoord::new(x, 0, z), crate::world::CellType::Block);
            }
        }
        let mut system = CharacterSystem::new();
        for (id, radius) in [(EntityId(701), 0.4), (EntityId(702), 0.3)] {
            let c = system.spawn_character(Vec3::new(3.5, 1.0, 3.5), None);
            system.associate_entity(id, c);
            system
                .get_character_for_entity_mut(id)
                .unwrap()
                .collision
                .radius = radius;
        }
        for id in [EntityId(701), EntityId(702)] {
            assert!(
                prepared_test_path(&mut system, id, &world, Vec3::new(8.5, 1.0, 8.5))
                    .is_ok()
            );
        }
        let builds = system
            .navigation_cache
            .borrow()
            .as_ref()
            .unwrap()
            .profiles
            .values()
            .map(|g| g.statistics().tiles_built)
            .collect::<Vec<_>>();
        world.bump_render_revision();
        for _ in 0..10 {
            for id in [EntityId(701), EntityId(702)] {
                assert!(
                    prepared_test_path(&mut system, id, &world, Vec3::new(8.5, 1.0, 8.5))
                        .is_ok()
                );
            }
        }
        let cache = system.navigation_cache.borrow();
        let cache = cache.as_ref().unwrap();
        assert_eq!(cache.profiles.len(), 2);
        assert_eq!(
            cache
                .profiles
                .values()
                .map(|g| g.statistics().tiles_built)
                .collect::<Vec<_>>(),
            builds
        );
    }
}
