// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{App, EditorMode, View};
use crate::project::ProjectHierarchy;
use crate::world::World;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    hash::Hasher,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::{Duration, Instant, SystemTime},
};

type Fingerprints = BTreeMap<PathBuf, u64>;
type Stamps = BTreeMap<PathBuf, (u64, SystemTime)>;

pub(super) struct LiveProjectSync {
    root: PathBuf,
    base: BTreeMap<String, u64>,
    sender: mpsc::Sender<Fingerprints>,
    receiver: mpsc::Receiver<Result<Scan, String>>,
    in_flight: bool,
    last_scan: Instant,
    scripts_seen: Fingerprints,
    reported_messages: BTreeSet<String>,
}
#[derive(Clone)]
struct Candidate {
    world: World,
    hierarchy: ProjectHierarchy,
    disk: Fingerprints,
    stamps: Stamps,
}
struct Scan {
    scripts: Option<BTreeMap<PathBuf, Arc<str>>>,
    scene: Result<Option<Candidate>, String>,
}

fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    h.write(bytes);
    h.finish()
}
fn encoded<T: serde::Serialize>(value: &T) -> u64 {
    fingerprint(&serde_json::to_vec(value).expect("authored scene values are serializable"))
}
fn scene_path(path: &Path) -> bool {
    path == Path::new("world.dat")
        || path == Path::new("project.json")
        || path.starts_with("chunks")
}
fn scene_disk(all: &Fingerprints) -> Fingerprints {
    all.iter()
        .filter(|(p, _)| scene_path(p))
        .map(|(p, h)| (p.clone(), *h))
        .collect()
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>, scripts: bool) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            continue;
        }
        let resolved = std::fs::canonicalize(&path).map_err(|e| e.to_string())?;
        if !resolved.starts_with(root) {
            continue;
        }
        if ty.is_dir() {
            walk(root, &path, out, scripts)?;
        } else if ty.is_file()
            && (!scripts
                || path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("aeo")))
        {
            out.push(path);
        }
    }
    Ok(())
}
fn stamps(root: &Path, include_scripts: bool) -> Result<Stamps, String> {
    let canonical = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
    let mut paths = vec![root.join("world.dat"), root.join("project.json")];
    walk(&canonical, &root.join("chunks"), &mut paths, false)?;
    if include_scripts {
        for dir in ["scripts", "playerscripts", ".assets"] {
            walk(&canonical, &root.join(dir), &mut paths, true)?;
        }
    }
    let mut out = BTreeMap::new();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        let m = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        out.insert(
            path.strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_path_buf(),
            (m.len(), m.modified().map_err(|e| e.to_string())?),
        );
    }
    Ok(out)
}
fn read_scene(root: &Path, disk: Fingerprints, before: Stamps) -> Result<Candidate, String> {
    let text = std::fs::read_to_string(root.join("world.dat")).map_err(|e| e.to_string())?;
    for required in [
        "WORLD_FORMAT ",
        "NEXT_CELL_ID ",
        "NEXT_ACTOR_ID ",
        "SELECTED_CAMERA ",
        "CHUNK_MANIFEST 1",
    ] {
        if !text.lines().any(|l| l.starts_with(required)) {
            return Err("Outside scene write is incomplete; keeping the current scene".into());
        }
    }
    if !text.ends_with('\n') {
        return Err("Outside scene write is incomplete; keeping the current scene".into());
    }
    let mut actor_ids = BTreeSet::new();
    for line in text.lines().filter(|l| l.starts_with("ACTOR ")) {
        let p: Vec<_> = line.split_whitespace().collect();
        if p.len() < 20
            || p[1]
                .parse::<u64>()
                .ok()
                .is_none_or(|id| !actor_ids.insert(id))
            || (3..12)
                .chain(std::iter::once(19))
                .any(|i| p[i].parse::<f32>().ok().is_none_or(|v| !v.is_finite()))
        {
            return Err(
                "Outside scene contains an invalid Actor; keeping the current scene".into(),
            );
        }
        if [12, 13, 15, 16, 17, 18]
            .into_iter()
            .any(|i| p[i].parse::<bool>().is_err())
        {
            return Err("Outside scene contains invalid Actor flags".into());
        }
    }
    let mut world = World::new();
    crate::world::persistence::load::load_scene_without_terrain(
        &mut world,
        &root.join("world.dat"),
    )
    .map_err(|e| e.to_string())?;
    let hierarchy = crate::project::load_project_metadata(root).map_err(|e| e.to_string())?;
    hierarchy.validate()?;
    let after = stamps(root, false)?;
    if before != after {
        return Err("Outside scene files are still changing; refresh deferred".into());
    }
    Ok(Candidate {
        world,
        hierarchy,
        disk,
        stamps: after,
    })
}

fn global_signatures(w: &World) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    let mut add = |key: &str, value: String| {
        m.insert(format!("g:{key}"), fingerprint(value.as_bytes()));
    };
    add(
        "physics",
        format!(
            "{:?}",
            (
                w.gravity,
                w.max_walkable_slope_degrees,
                w.character_walk_speed
            )
        ),
    );
    add(
        "lighting",
        format!(
            "{:?}",
            (
                w.lighting.shadows_enabled,
                w.lighting.global_light_enabled,
                w.lighting.global_light_direction,
                w.lighting.global_light_color,
                w.lighting.global_light_intensity,
                w.lighting.ambient_intensity
            )
        ),
    );
    add(
        "sky",
        format!(
            "{:?}",
            (
                w.sky.enabled,
                &w.sky.texture,
                &w.sky.material_asset,
                w.sky.atmosphere_enabled,
                w.sky.fog_start_studs,
                w.sky.fog_end_studs,
                w.sky.fog_color,
                w.sky.horizon_strength,
                w.sky.horizon_mirage_enabled,
                w.sky.horizon_mirage_strength
            )
        ),
    );
    add("render", format!("{:?}", w.render_distance));
    add(
        "player",
        format!(
            "{:?}",
            (
                &w.selected_character,
                w.player_character_scale,
                &w.selected_controller,
                &w.selected_camera,
                w.cursor_visible,
                w.screen_locked
            )
        ),
    );
    add(
        "nav",
        serde_json::to_string(&(&w.navmesh_settings, &w.navmesh_volumes)).unwrap(),
    );
    add(
        "bindings",
        serde_json::to_string(&w.script_bindings).unwrap(),
    );
    add(
        "actor_bindings",
        serde_json::to_string(&w.actor_script_bindings).unwrap(),
    );
    add(
        "disabled",
        serde_json::to_string(&w.disabled_scripts).unwrap(),
    );
    m
}
fn signatures(w: &World, h: &ProjectHierarchy) -> BTreeMap<String, u64> {
    use crate::world::storage::{StoredActor, StoredCell, StoredSource};
    let mut out = global_signatures(w);
    for (&id, a) in &w.actors {
        out.insert(format!("a:{id}"), encoded(&StoredActor::from_actor(a)));
    }
    for (&id, s) in &w.sources {
        out.insert(format!("s:{id}"), encoded(&StoredSource::from_source(s)));
    }
    for (&coord, c) in &w.cells {
        out.insert(
            format!("c:{}", c.id),
            encoded(&([coord.x, coord.y, coord.z], StoredCell::from_cell(coord, c))),
        );
    }
    for (&id, n) in &h.nodes {
        out.insert(format!("n:{id}"), encoded(n));
    }
    out
}
fn apply_globals(local: &mut World, remote: &World, changed: &BTreeSet<String>) {
    for key in changed {
        match key.as_str() {
            "g:physics" => {
                local.gravity = remote.gravity;
                local.max_walkable_slope_degrees = remote.max_walkable_slope_degrees;
                local.character_walk_speed = remote.character_walk_speed;
            }
            "g:lighting" => local.lighting = remote.lighting.clone(),
            "g:sky" => local.sky = remote.sky.clone(),
            "g:render" => local.render_distance = remote.render_distance.clone(),
            "g:player" => {
                local.selected_character = remote.selected_character.clone();
                local.player_character_scale = remote.player_character_scale;
                local.selected_controller = remote.selected_controller.clone();
                local.selected_camera = remote.selected_camera.clone();
                local.cursor_visible = remote.cursor_visible;
                local.screen_locked = remote.screen_locked;
            }
            "g:nav" => {
                local.navmesh_settings = remote.navmesh_settings.clone();
                local.navmesh_volumes = remote.navmesh_volumes.clone();
            }
            "g:bindings" => local.script_bindings = remote.script_bindings.clone(),
            "g:actor_bindings" => {
                local.actor_script_bindings = remote.actor_script_bindings.clone()
            }
            "g:disabled" => local.disabled_scripts = remote.disabled_scripts.clone(),
            _ => {}
        }
    }
}

impl LiveProjectSync {
    fn new(root: PathBuf, base: BTreeMap<String, u64>) -> Self {
        let (sender, requests) = mpsc::channel::<Fingerprints>();
        let (results, receiver) = mpsc::channel();
        let worker_root = root.clone();
        std::thread::spawn(move || {
            let mut previous = None;
            let mut cache: BTreeMap<PathBuf, ((u64, SystemTime), u64, Option<Arc<str>>)> =
                BTreeMap::new();
            let mut prepared_scene: Option<(Fingerprints, Result<Candidate, String>)> = None;
            while let Ok(expected) = requests.recv() {
                let result = (|| {
                    let observed = stamps(&worker_root, true)?;
                    if previous.as_ref() != Some(&observed) {
                        previous = Some(observed);
                        return Ok(None);
                    }
                    let mut disk = BTreeMap::new();
                    let mut scripts = BTreeMap::new();
                    for (path, stamp) in &observed {
                        let item = if let Some(old) = cache.get(path).filter(|old| old.0 == *stamp)
                        {
                            old.clone()
                        } else {
                            let bytes =
                                std::fs::read(worker_root.join(path)).map_err(|e| e.to_string())?;
                            let hash = fingerprint(&bytes);
                            let source = if scene_path(path) {
                                None
                            } else {
                                Some(Arc::<str>::from(
                                    String::from_utf8(bytes).map_err(|e| e.to_string())?,
                                ))
                            };
                            (*stamp, hash, source)
                        };
                        if scene_path(path) {
                            disk.insert(path.clone(), item.1);
                        } else if let Some(source) = &item.2 {
                            scripts.insert(worker_root.join(path), source.clone());
                        }
                        cache.insert(path.clone(), item);
                    }
                    cache.retain(|p, _| observed.contains_key(p));
                    if stamps(&worker_root, true)? != observed {
                        previous = None;
                        return Ok(None);
                    }
                    let scene = if disk != expected {
                        if prepared_scene.as_ref().is_none_or(|(old, _)| old != &disk) {
                            let scene_stamps = observed
                                .into_iter()
                                .filter(|(p, _)| scene_path(p))
                                .collect();
                            let candidate = read_scene(&worker_root, disk.clone(), scene_stamps);
                            prepared_scene = Some((disk.clone(), candidate));
                        }
                        prepared_scene.as_ref().unwrap().1.clone().map(Some)
                    } else {
                        Ok(None)
                    };
                    Ok(Some(Scan {
                        scripts: Some(scripts),
                        scene,
                    }))
                })();
                let message = match result {
                    Ok(Some(scan)) => Ok(scan),
                    Ok(None) => Ok(Scan {
                        scripts: None,
                        scene: Ok(None),
                    }),
                    Err(e) => Err(e),
                };
                if results.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            root,
            base,
            sender,
            receiver,
            in_flight: false,
            last_scan: Instant::now() - Duration::from_secs(2),
            scripts_seen: BTreeMap::new(),
            reported_messages: BTreeSet::new(),
        }
    }
}

impl App {
    pub(super) fn reset_live_project_sync(&mut self) {
        self.live_project_sync = self.project_manager.current_project.clone().map(|root| {
            let disk_h = crate::project::load_project_metadata(&root).unwrap_or_default();
            LiveProjectSync::new(root, signatures(&self.world, &disk_h))
        });
    }
    fn refresh_scripts_from_scan(
        &mut self,
        sync: &mut LiveProjectSync,
        scripts: BTreeMap<PathBuf, Arc<str>>,
    ) {
        for (path, doc) in &mut self.editor.script_editor.open_documents {
            if !path.starts_with(&sync.root) {
                continue;
            }
            if !scripts.contains_key(path) && !path.exists() {
                let message = format!(
                    "External script removed: {}; open text preserved",
                    path.display()
                );
                if sync.reported_messages.insert(message.clone()) {
                    self.editor.script_editor.output_log.push(message);
                }
            }
            if let Some(source) = scripts.get(path) {
                if source.as_ref() != doc.original_source {
                    if let Err(e) = doc.accept_external_source(source) {
                        let message = format!("External script refresh: {}: {e}", path.display());
                        if sync.reported_messages.insert(message.clone()) {
                            self.editor.script_editor.output_log.push(message);
                        }
                    }
                }
            }
        }
        let seen = scripts
            .iter()
            .map(|(p, s)| (p.clone(), fingerprint(s.as_bytes())))
            .collect();
        if sync.scripts_seen != seen {
            self.editor
                .script_editor
                .refresh_scripts(&Some(sync.root.clone()));
            self.editor.reconcile_filesystem_scripts(&sync.root);
            sync.scripts_seen = seen;
        }
    }

    fn apply_external_scene(
        &mut self,
        sync: &mut LiveProjectSync,
        candidate: &Candidate,
    ) -> Result<(), String> {
        if stamps(&sync.root, false)? != candidate.stamps {
            return Err("Outside scene changed again; refresh deferred".into());
        }
        let local = signatures(&self.world, &self.editor.project_hierarchy);
        let remote = signatures(&candidate.world, &candidate.hierarchy);
        let mut changed = BTreeSet::new();
        for key in sync
            .base
            .keys()
            .chain(remote.keys())
            .collect::<BTreeSet<_>>()
        {
            if remote.get(key) == sync.base.get(key) {
                continue;
            }
            if local.get(key) != sync.base.get(key) && local.get(key) != remote.get(key) {
                let message = format!(
                    "External scene conflict at {key}; keeping Worldkiln's unsaved changes"
                );
                if sync.reported_messages.insert(message.clone()) {
                    self.editor.script_editor.push_output(message);
                }
                continue;
            }
            if local.get(key) != remote.get(key) {
                changed.insert(key.clone());
            }
        }
        let changed_ids = |prefix: &str| {
            changed
                .iter()
                .filter_map(|k| k.strip_prefix(prefix)?.parse::<u64>().ok())
                .collect::<BTreeSet<_>>()
        };
        let actor_ids = changed_ids("a:");
        let cell_ids = changed_ids("c:");
        let source_ids = changed_ids("s:");
        let node_ids = changed_ids("n:");
        let mut cells = if cell_ids.is_empty() {
            HashMap::new()
        } else {
            self.world.cells.clone()
        };
        cells.retain(|_, c| !cell_ids.contains(&c.id));
        for (&coord, c) in &candidate.world.cells {
            if cell_ids.contains(&c.id) {
                if cells.get(&coord).is_some_and(|old| old.id != c.id) {
                    return Err(format!("External Cell destination conflicts at {coord:?}"));
                }
                cells.insert(coord, c.clone());
            }
        }
        let mut entity_ids = BTreeSet::new();
        let merged_cells = if cell_ids.is_empty() {
            &self.world.cells
        } else {
            &cells
        };
        for cell in merged_cells.values() {
            if !entity_ids.insert(cell.id) {
                return Err("Outside scene has duplicate Cell IDs".into());
            }
        }
        for (&id, _) in self
            .world
            .sources
            .iter()
            .filter(|(id, _)| !source_ids.contains(id))
            .chain(
                candidate
                    .world
                    .sources
                    .iter()
                    .filter(|(id, _)| source_ids.contains(id)),
            )
        {
            if !entity_ids.insert(id) {
                return Err("Outside scene conflicts with a Cell/Source ID".into());
            }
        }
        let mut hierarchy = self.editor.project_hierarchy.clone();
        for id in node_ids {
            if let Some(node) = candidate.hierarchy.nodes.get(&id) {
                hierarchy.nodes.insert(id, node.clone());
            } else {
                hierarchy.nodes.remove(&id);
            }
        }
        hierarchy.next_node_id = hierarchy.next_node_id.max(candidate.hierarchy.next_node_id);
        hierarchy.validate()?;
        if !changed.is_empty() {
            let snapshot = self.editor.create_snapshot_without_terrain(&self.world);
            self.editor.history.push_undo_entry(snapshot);
            self.editor.history.redo_stack.clear();
            if !cell_ids.is_empty() {
                self.world.swap_authored_cells_for_history(cells);
            }
            for id in actor_ids {
                if let Some(a) = candidate.world.actors.get(&id) {
                    self.world.actors.insert(id, a.clone());
                } else {
                    self.world.actors.remove(&id);
                }
            }
            self.world.invalidate_actor_tag_index();
            for id in source_ids {
                if let Some(s) = candidate.world.sources.get(&id) {
                    self.world.insert_source(s.clone());
                } else {
                    self.world.remove_source(id);
                }
            }
            apply_globals(&mut self.world, &candidate.world, &changed);
            self.world.next_actor_id = self.world.next_actor_id.max(candidate.world.next_actor_id);
            self.world.next_cell_id = self.world.next_cell_id.max(candidate.world.next_cell_id);
            self.world.rebuild_actor_module_index();
            self.world.rebuild_light_and_marker_indices();
            self.world.bump_render_revision();
            self.editor.project_hierarchy = hierarchy;
            self.editor.project_actor_sync_revision = None;
            self.editor.project_cell_sync_revision = None;
            self.editor.synchronize_project_actor_nodes(&self.world);
            self.editor.synchronize_project_cell_nodes(&self.world);
            self.editor
                .selected_coords
                .retain(|c| self.world.get(*c).is_some());
            if self
                .editor
                .selected_coord
                .is_some_and(|c| self.world.get(c).is_none())
            {
                self.editor.selected_coord = self.editor.selected_coords.last().copied();
            }
            if self
                .editor
                .selected_actor_id
                .is_some_and(|id| !self.world.actors.contains_key(&id))
            {
                self.editor.selected_actor_id = None;
            }
            if self
                .editor
                .selected_source_id
                .is_some_and(|id| self.world.source(id).is_none())
            {
                self.editor.selected_source_id = None;
            }
            self.editor.clear_hover();
            self.editor.editor_max_distance_chunks = self.world.render_distance.editor_chunks;
            self.editor.play_max_distance_chunks = self.world.render_distance.play_chunks;
        }
        sync.base = remote;
        if let Some(baseline) = self.project_disk_snapshot.as_mut() {
            baseline.retain(|p, _| !scene_path(p));
            baseline.extend(candidate.disk.clone());
        }
        Ok(())
    }

    pub(super) fn poll_live_project_sync(&mut self) {
        if self.view != View::Editor
            || self.editor.mode != EditorMode::Editor
            || self.project_loading()
            || self.project_load_failed
            || self.is_left_mouse_down
            || self.editor.pending_inspector_history.is_some()
        {
            return;
        }
        let Some(root) = self.project_manager.current_project.clone() else {
            return;
        };
        if self
            .live_project_sync
            .as_ref()
            .is_none_or(|s| s.root != root)
        {
            self.reset_live_project_sync();
        }
        let Some(mut sync) = self.live_project_sync.take() else {
            return;
        };
        if let Ok(response) = sync.receiver.try_recv() {
            sync.in_flight = false;
            let result = response.and_then(|scan| {
                if let Some(scripts) = scan.scripts {
                    self.refresh_scripts_from_scan(&mut sync, scripts);
                }
                if let Some(candidate) = scan.scene? {
                    self.apply_external_scene(&mut sync, &candidate)?;
                }
                Ok(())
            });
            if let Err(error) = result {
                if sync.reported_messages.insert(error.clone()) {
                    self.editor.script_editor.push_output(error);
                }
            }
        }
        if !sync.in_flight && sync.last_scan.elapsed() >= Duration::from_millis(500) {
            if let Some(baseline) = self.project_disk_snapshot.as_ref() {
                if sync.sender.send(scene_disk(baseline)).is_ok() {
                    sync.in_flight = true;
                    sync.last_scan = Instant::now();
                }
            }
        }
        self.live_project_sync = Some(sync);
    }

    pub(super) fn refresh_external_scene_before_save(&mut self) -> Result<(), String> {
        if self.editor.mode != EditorMode::Editor {
            return Ok(());
        }
        let Some(mut sync) = self.live_project_sync.take() else {
            return Ok(());
        };
        let result = (|| {
            let observed = stamps(&sync.root, false)?;
            let mut disk = BTreeMap::new();
            for path in observed.keys() {
                disk.insert(
                    path.clone(),
                    fingerprint(&std::fs::read(sync.root.join(path)).map_err(|e| e.to_string())?),
                );
            }
            let expected = self
                .project_disk_snapshot
                .as_ref()
                .map(scene_disk)
                .unwrap_or_default();
            if disk != expected {
                let candidate = read_scene(&sync.root, disk, observed)?;
                self.apply_external_scene(&mut sync, &candidate)?;
            }
            Ok(())
        })();
        self.live_project_sync = Some(sync);
        result
    }
}
