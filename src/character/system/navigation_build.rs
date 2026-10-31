// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::CachedNavigationGrid;
use crate::engine::collision_geometry::ColliderInstance;
use crate::navigation::{
    self, CellSourceCapture, CellSources, NavigationAgent, NavigationBuildCapture,
    NavigationBuildError, NavigationGrid,
};
use crate::world::World;
use glam::Vec3;
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};
use std::time::{Duration, Instant};

pub(super) type Profile = (u32, u32);

pub(super) fn profile(radius: f32, height: f32) -> Result<Profile, NavigationBuildError> {
    if !radius.is_finite()
        || !height.is_finite()
        || radius <= 0.0
        || height <= 0.0
        || radius > 1000.0
        || height > 2000.0
    {
        return Err(NavigationBuildError::InvalidAgent(
            "unsupported navigation capsule",
        ));
    }
    Ok(((radius * 32.0).ceil() as u32, (height * 32.0).ceil() as u32))
}

#[derive(Clone, PartialEq)]
struct Revision {
    world_id: u64,
    policy: String,
    cells: u64,
    terrain: u64,
    water: String,
    actors: Arc<BTreeMap<u64, (Vec3, ColliderInstance)>>,
}
impl Revision {
    fn new(world: &World, policy: &str) -> Self {
        Self {
            world_id: world.navigation_world_id,
            policy: policy.to_owned(),
            cells: world.navigation_collision_revision.get(),
            terrain: world.terrain.render_revision(),
            water: navigation::navigation_water_key(world),
            actors: navigation::navigation_actor_colliders(world),
        }
    }
}

enum Job {
    Queued(Option<NavigationGrid>),
    Capturing(NavigationBuildCapture),
}
struct Worker {
    revision: Arc<Revision>,
    profile: Profile,
    cancelled: Arc<AtomicBool>,
    result: Receiver<Result<NavigationGrid, NavigationBuildError>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
pub(crate) struct NavigationBuildService {
    revision: Option<Arc<Revision>>,
    ready: BTreeMap<Profile, Arc<Revision>>,
    jobs: BTreeMap<Profile, Job>,
    failures: BTreeMap<Profile, NavigationBuildError>,
    diagnostic_requests: BTreeMap<Profile, crate::engine::development_capture::Correlation>,
    sources: Option<(u64, Arc<CellSources>)>,
    source_capture: Option<CellSourceCapture>,
    terrain_sources: navigation::NavigationTerrainSources,
    worker: Option<Worker>,
}

impl NavigationBuildService {
    pub(super) fn has_work(&self) -> bool {
        self.worker.is_some() || !self.jobs.is_empty()
    }

    fn synchronize(&mut self, world: &World, cache: &CachedNavigationGrid) {
        let revision = Revision::new(world, &cache.policy);
        if self.revision.as_deref() == Some(&revision) {
            return;
        }
        let same_world = self
            .revision
            .as_ref()
            .is_some_and(|r| r.world_id == revision.world_id);
        let same_cells = self
            .revision
            .as_ref()
            .is_some_and(|r| r.world_id == revision.world_id && r.cells == revision.cells);
        self.source_capture = if same_cells {
            self.source_capture.take()
        } else if !same_world {
            self.sources = None;
            None
        } else if let Some((old_revision, sources)) = &self.sources
            && *old_revision != revision.cells
        {
            Some(CellSourceCapture::from_previous(
                world,
                *old_revision,
                sources,
            ))
        } else {
            None
        };
        if self.source_capture.is_some() {
            self.sources = None;
        }
        self.ready.clear();
        self.jobs.clear();
        self.diagnostic_requests.clear();
        self.failures.clear();
        if let Some(worker) = &self.worker {
            worker.cancelled.store(true, Ordering::Relaxed);
        }
        self.revision = Some(Arc::new(revision));
    }

    pub(super) fn request(
        &mut self,
        world: &World,
        cache: &mut CachedNavigationGrid,
        profile: Profile,
    ) -> Result<bool, NavigationBuildError> {
        self.synchronize(world, cache);
        if self.ready.get(&profile) == self.revision.as_ref()
            && cache.profiles.contains_key(&profile)
        {
            return Ok(true);
        }
        if let Some(error) = self.failures.get(&profile) {
            return Err(error.clone());
        }
        let running = self
            .worker
            .as_ref()
            .is_some_and(|w| w.profile == profile && Some(&w.revision) == self.revision.as_ref());
        if !self.diagnostic_requests.contains_key(&profile) {
            if let Some(request) = crate::engine::development_capture::correlation() {
                self.diagnostic_requests.insert(profile, request);
            }
        }
        if !running && !self.jobs.contains_key(&profile) {
            self.jobs
                .insert(profile, Job::Queued(cache.profiles.remove(&profile)));
        }
        Ok(false)
    }

    pub(super) fn tick(&mut self, world: &World, cache: &mut CachedNavigationGrid) {
        self.synchronize(world, cache);
        let revision = self.revision.as_ref().unwrap().clone();
        if let Some(worker) = &self.worker {
            let completed = match worker.result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err(NavigationBuildError::InvalidAgent(
                    "navigation worker stopped",
                ))),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = completed {
                let worker = self.worker.take().unwrap();
                if worker.revision == revision && !worker.cancelled.load(Ordering::Relaxed) {
                    self.diagnostic_requests.remove(&worker.profile);
                    match result {
                        Ok(grid) => {
                            cache.profiles.insert(worker.profile, grid);
                            self.ready.insert(worker.profile, revision.clone());
                        }
                        Err(error) => {
                            self.failures.insert(worker.profile, error);
                        }
                    }
                }
            }
        }
        if self.worker.is_some() {
            return;
        }
        let Some((&profile, _)) = self.jobs.first_key_value() else {
            return;
        };
        let diagnostic_request = self.diagnostic_requests.get(&profile).cloned();
        let _diagnostic_capture = crate::engine::development_capture::operation(
            "navigation",
            "navmesh_collision_capture",
            false,
        )
        .map(|span| span.caused_by(diagnostic_request.as_ref()));
        let deadline = Instant::now() + Duration::from_millis(1);
        let job = self.jobs.remove(&profile).unwrap();
        let mut capture = match job {
            Job::Capturing(capture) => capture,
            Job::Queued(previous) => {
                if self.sources.is_none() {
                    let source = self
                        .source_capture
                        .get_or_insert_with(|| CellSourceCapture::new(world));
                    if !source.advance(world, deadline) {
                        self.jobs.insert(profile, Job::Queued(previous));
                        return;
                    }
                    self.sources =
                        Some((revision.cells, self.source_capture.take().unwrap().finish()));
                    self.jobs.insert(profile, Job::Queued(previous));
                    return;
                }
                let prepared = if let Some(grid) = previous {
                    grid.prepare_collision_refresh(world, self.sources.as_ref().unwrap().1.clone())
                } else {
                    let radius = profile.0 as f32 / 32.0;
                    let height = profile.1 as f32 / 32.0;
                    let agent = NavigationAgent {
                        width_cells: (radius * 2.0).ceil().max(1.0) as u32,
                        height_cells: height.ceil().max(1.0) as u32,
                        ..NavigationAgent::default()
                    };
                    NavigationGrid::prepare_character_build(
                        world,
                        agent,
                        radius,
                        height,
                        self.sources.as_ref().unwrap().1.clone(),
                    )
                };
                match prepared {
                    Ok(capture) => {
                        self.jobs.insert(profile, Job::Capturing(capture));
                        return;
                    }
                    Err(error) => {
                        self.failures.insert(profile, error);
                        return;
                    }
                }
            }
        };
        if !capture.advance(world, &mut self.terrain_sources) {
            self.jobs.insert(profile, Job::Capturing(capture));
            return;
        }
        let (sender, result) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancellation = cancelled.clone();
        match std::thread::Builder::new()
            .name("navigation-build".into())
            .spawn(move || {
                let _diagnostic_build = crate::engine::development_capture::operation(
                    "navigation",
                    "navmesh_build_worker",
                    false,
                )
                .map(|span| span.caused_by(diagnostic_request.as_ref()));
                let built = capture.finish(&cancellation);
                let _ = sender.send(built);
            }) {
            Ok(_) => {
                self.worker = Some(Worker {
                    revision,
                    profile,
                    cancelled,
                    result,
                })
            }
            Err(_) => {
                self.failures.insert(
                    profile,
                    NavigationBuildError::InvalidAgent("cannot start navigation worker"),
                );
            }
        }
    }
}
