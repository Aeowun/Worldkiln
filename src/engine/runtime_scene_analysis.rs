// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::actor::ActorSystem;
use crate::character::CharacterSystem;
use crate::engine::entity::{EntityId, EntityManager, RuntimeEntity};
use crate::world::World;

#[derive(Clone, Serialize, PartialEq)]
struct SceneObject {
    key: String,
    source: &'static str,
    public_actor_id: Option<u64>,
    authored_actor_id: Option<u64>,
    entity_id: Option<u64>,
    character_id: Option<u64>,
    name: String,
    package: Option<String>,
    status: String,
    activity: String,
    position: [f32; 3],
    velocity: [f32; 3],
    visible: bool,
    enabled: bool,
    physics_enabled: bool,
    anchored: bool,
    collidable: bool,
    grounded: bool,
    is_player: bool,
    navigating: bool,
    animation: Option<String>,
    bone_count: Option<usize>,
    render_candidate: bool,
    tags: Vec<String>,
    scripts: Vec<String>,
}

fn v3(v: glam::Vec3) -> [f32; 3] {
    let f = |x: f32| {
        if x.is_finite() {
            (x * 100.0).round() / 100.0
        } else {
            0.0
        }
    };
    [f(v.x), f(v.y), f(v.z)]
}

fn entity_fields(row: &mut SceneObject, entity: &RuntimeEntity, characters: &CharacterSystem) {
    row.entity_id = Some(entity.id.0);
    row.position = v3(entity.position);
    row.velocity = v3(entity.physics.velocity);
    row.visible = entity.visible;
    row.enabled = entity.enabled;
    row.physics_enabled = entity.physics.enabled;
    row.anchored = entity.physics.anchored;
    row.collidable = entity.physics.collision_enabled;
    row.grounded = entity.physics.grounded;
    row.render_candidate = row.enabled && row.visible;
    if entity.package_name.is_some() {
        row.package = entity.package_name.clone();
    }
    row.animation = entity.animation.current_clip.clone();
    row.navigating = characters.is_entity_navigating(entity.id);
    if let Some(character) = characters.get_character_for_entity(entity.id) {
        row.character_id = Some(character.id);
        row.position = v3(character.transform.position);
        row.velocity = v3(character.movement.velocity);
        row.grounded = character.movement.is_grounded;
        row.animation = Some(
            character
                .animation_controller
                .current_clip_name()
                .to_owned(),
        );
        row.bone_count = Some(character.package.skeleton.joints.len());
        row.is_player = characters
            .get_active_player()
            .is_some_and(|player| player.id == character.id);
        row.activity = if row.navigating {
            "navigating".to_owned()
        } else if character.movement.is_swimming {
            "swimming".to_owned()
        } else {
            format!("{:?}", character.movement.state).to_lowercase()
        };
    } else if row.navigating {
        row.activity = "navigating".to_owned();
    } else if entity.animation.playing && entity.animation.current_clip.is_some() {
        row.activity = "animating".to_owned();
    } else if entity.physics.velocity.length_squared() > 0.01 {
        row.activity = "moving".to_owned();
    } else {
        row.activity = "idle".to_owned();
    }
    if !row.enabled {
        row.status = "disabled".to_owned();
        row.activity = "disabled".to_owned();
    } else if !row.visible {
        row.status = "hidden".to_owned();
    } else {
        row.status = "active".to_owned();
    }
}

fn empty_row(key: String, source: &'static str, name: String) -> SceneObject {
    SceneObject {
        key,
        source,
        public_actor_id: None,
        authored_actor_id: None,
        entity_id: None,
        character_id: None,
        name,
        package: None,
        status: "not_spawned".to_owned(),
        activity: "not_spawned".to_owned(),
        position: [0.0; 3],
        velocity: [0.0; 3],
        visible: false,
        enabled: false,
        physics_enabled: false,
        anchored: false,
        collidable: false,
        grounded: false,
        is_player: false,
        navigating: false,
        animation: None,
        bone_count: None,
        render_candidate: false,
        tags: Vec::new(),
        scripts: Vec::new(),
    }
}

fn collect(
    world: &World,
    actors: &ActorSystem,
    entities: &EntityManager,
    characters: &CharacterSystem,
) -> Vec<SceneObject> {
    let mut result = Vec::with_capacity(world.actors.len() + actors.actors().len() + 8);
    let mut accounted_entities = BTreeSet::<EntityId>::new();
    let mut accounted_characters = BTreeSet::<u64>::new();

    for (id, authored) in &world.actors {
        let mut row = empty_row(
            format!("actor:{id}"),
            "authored_actor",
            authored.identity.clone(),
        );
        row.authored_actor_id = Some(*id);
        row.public_actor_id = Some(*id);
        row.package = authored.package.clone();
        row.position = v3(authored.position);
        row.visible = authored.visible;
        row.enabled = authored.enabled;
        row.physics_enabled = authored.physics.enabled;
        row.anchored = authored.physics.anchored;
        row.collidable = authored.physics.collision_enabled;
        row.tags = world
            .actor_tags(*id)
            .map(|tags| tags.iter().cloned().collect())
            .unwrap_or_default();
        row.scripts = world
            .actor_modules(*id)
            .iter()
            .filter(|module| module.enabled)
            .map(|module| module.script_path.clone())
            .collect();
        if let Some(entity_id) = actors.entity_for_authored_actor(*id) {
            if let Some(entity) = entities.get_entity(entity_id) {
                entity_fields(&mut row, entity, characters);
                accounted_entities.insert(entity_id);
                if let Some(character_id) = row.character_id {
                    accounted_characters.insert(character_id);
                }
            } else {
                row.status = "missing_entity".to_owned();
                row.activity = "missing_entity".to_owned();
            }
        }
        result.push(row);
    }

    for (entity_id, actor) in actors.actors() {
        if accounted_entities.contains(entity_id) {
            continue;
        }
        let id = actors.script_actor_id_for_entity(*entity_id);
        let name = entities
            .get_entity(*entity_id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "missing_runtime_entity".into());
        let mut row = empty_row(
            format!("actor:{}", id.unwrap_or(entity_id.0)),
            "runtime_actor",
            name,
        );
        row.public_actor_id = id;
        row.authored_actor_id = actor.authored_actor_id;
        row.package = actor.package_name.clone();
        row.position = v3(actor.initial_position);
        if let Some(entity) = entities.get_entity(*entity_id) {
            entity_fields(&mut row, entity, characters);
            accounted_entities.insert(*entity_id);
            if let Some(character_id) = row.character_id {
                accounted_characters.insert(character_id);
            }
        }
        result.push(row);
    }

    for entity in entities.iter() {
        if accounted_entities.contains(&entity.id) {
            continue;
        }
        let mut row = empty_row(
            format!("entity:{}", entity.id.0),
            "runtime_entity",
            entity.name.clone(),
        );
        entity_fields(&mut row, entity, characters);
        if let Some(character_id) = row.character_id {
            accounted_characters.insert(character_id);
        }
        result.push(row);
    }
    for character in characters.get_active_characters() {
        if accounted_characters.contains(&character.id) {
            continue;
        }
        let mut row = empty_row(
            format!("character:{}", character.id),
            "unlinked_character",
            character.package_name.clone(),
        );
        row.character_id = Some(character.id);
        row.package = Some(character.package_name.clone());
        row.position = v3(character.transform.position);
        row.velocity = v3(character.movement.velocity);
        row.enabled = true;
        row.visible = true;
        row.render_candidate = true;
        row.status = "unlinked_character".to_owned();
        row.activity = format!("{:?}", character.movement.state).to_lowercase();
        row.animation = Some(
            character
                .animation_controller
                .current_clip_name()
                .to_owned(),
        );
        row.bone_count = Some(character.package.skeleton.joints.len());
        result.push(row);
    }
    result.sort_by(|a, b| a.key.cmp(&b.key));
    result
}

enum Message {
    Snapshot {
        elapsed_ms: u64,
        objects: Vec<SceneObject>,
    },
}

pub(crate) struct SceneAnalysis {
    sender: Option<SyncSender<Message>>,
    worker: Option<JoinHandle<()>>,
    started: Instant,
    sampled: Instant,
    interval: Duration,
    output: Option<PathBuf>,
    dropped: u64,
}

impl Default for SceneAnalysis {
    fn default() -> Self {
        Self {
            sender: None,
            worker: None,
            started: Instant::now(),
            sampled: Instant::now(),
            interval: Duration::from_secs(1),
            output: None,
            dropped: 0,
        }
    }
}

impl SceneAnalysis {
    pub(crate) fn start(
        &mut self,
        project: &Path,
        development_build: bool,
    ) -> std::io::Result<Option<&Path>> {
        self.stop();
        let mode = std::env::var("WORLDKILN_SCENE_ANALYSIS").unwrap_or_default();
        if mode == "0" || (!development_build && mode != "1") {
            return Ok(None);
        }
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let dir = project
            .join(".worldkiln")
            .join("development")
            .join("logs")
            .join(format!("scene-analysis-{ms}"));
        fs::create_dir_all(&dir)?;
        let interval = std::env::var("WORLDKILN_SCENE_ANALYSIS_INTERVAL_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1000)
            .clamp(250, 30_000);
        fs::write(
            dir.join("README.txt"),
            "Worldkiln Play Mode Scene Analysis\n\
             initial.json: all authored and runtime Actors at first Play sample.\n\
             latest.json: complete latest sampled inventory (atomic replace).\n\
             changes.jsonl: spawn/despawn/state changes keyed to Actor ID.\n\
             summary.json: counts and elapsed capture time.\n\
             Data is sampled every configured interval. This is state telemetry,\n\
             not proof a GPU draw succeeded. FPS measurements remain separate.\n\
             Set WORLDKILN_SCENE_ANALYSIS=0 to disable or\n\
             WORLDKILN_SCENE_ANALYSIS_INTERVAL_MS=1000 to change sample period.\n",
        )?;
        let (tx, rx) = mpsc::sync_channel(8);
        let worker_path = dir.clone();
        let worker = std::thread::Builder::new()
            .name("worldkiln-scene-analysis".into())
            .spawn(move || write_loop(&worker_path, rx))?;
        self.sender = Some(tx);
        self.worker = Some(worker);
        self.started = Instant::now();
        self.sampled = self.started - Duration::from_secs(60);
        self.interval = Duration::from_millis(interval);
        self.output = Some(dir);
        self.dropped = 0;
        Ok(self.output.as_deref())
    }

    pub(crate) fn sample(
        &mut self,
        world: &World,
        actors: &ActorSystem,
        entities: &EntityManager,
        characters: &CharacterSystem,
    ) {
        let Some(tx) = &self.sender else {
            return;
        };
        let now = Instant::now();
        if now.duration_since(self.sampled) < self.interval {
            return;
        }
        self.sampled = now;
        let objects = collect(world, actors, entities, characters);
        if tx
            .try_send(Message::Snapshot {
                elapsed_ms: now.duration_since(self.started).as_millis() as u64,
                objects,
            })
            .is_err()
        {
            self.dropped += 1;
        }
    }

    pub(crate) fn stop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        if self.dropped > 0 {
            eprintln!(
                "[Worldkiln Scene Analysis] {} snapshots dropped because writer could not keep up",
                self.dropped
            );
        }
        self.output = None;
        self.dropped = 0;
    }
}

fn write_loop(dir: &Path, rx: Receiver<Message>) {
    let events = match File::create(dir.join("changes.jsonl")) {
        Ok(file) => file,
        Err(err) => {
            eprintln!("[Worldkiln Scene Analysis] {err}");
            return;
        }
    };
    let mut events = BufWriter::new(events);
    let mut previous: BTreeMap<String, SceneObject> = BTreeMap::new();
    let mut first = true;
    let mut snapshots = 0u64;
    let mut changes = 0u64;
    let mut elapsed = 0u64;
    while let Ok(Message::Snapshot {
        elapsed_ms,
        objects,
    }) = rx.recv()
    {
        elapsed = elapsed_ms;
        snapshots += 1;
        let current: BTreeMap<String, SceneObject> = objects
            .iter()
            .map(|row| (row.key.clone(), row.clone()))
            .collect();
        if first {
            if let Err(err) = write_json_atomic(&dir.join("initial.json"), &objects) {
                eprintln!("[Worldkiln Scene Analysis] initial snapshot error: {err}");
            }
            first = false;
        }
        for (key, row) in &current {
            let event = match previous.get(key) {
                None => "present",
                Some(old) if old != row => "changed",
                Some(_) => continue,
            };
            if serde_json::to_writer(
                &mut events,
                &json!({"at_ms":elapsed_ms,"event":event,"actor":row}),
            )
            .is_ok()
            {
                let _ = events.write_all(b"\n");
                changes += 1;
            }
        }
        for (key, old) in &previous {
            if !current.contains_key(key) {
                if serde_json::to_writer(
                    &mut events,
                    &json!({"at_ms":elapsed_ms,"event":"removed","actor":old}),
                )
                .is_ok()
                {
                    let _ = events.write_all(b"\n");
                    changes += 1;
                }
            }
        }
        let _ = events.flush();
        if let Err(err) = write_json_atomic(&dir.join("latest.json"), &objects) {
            eprintln!("[Worldkiln Scene Analysis] latest snapshot error: {err}");
        }
        let _ = write_json_atomic(
            &dir.join("summary.json"),
            &json!({
                "elapsed_ms": elapsed, "samples": snapshots, "changes": changes,
                "objects": current.len(),
                "authored_actors": current.values().filter(|v| v.source == "authored_actor").count(),
                "active_actors": current.values().filter(|v| v.enabled && v.entity_id.is_some()).count(),
                "visible_characters": current.values().filter(|v| v.render_candidate && v.character_id.is_some()).count(),
                "not_spawned": current.values().filter(|v| v.status == "not_spawned").count(),
                "note": "render_candidate is eligibility, not proof of GPU drawing"
            }),
        );
        previous = current;
    }
    let _ = events.flush();
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let temp = path.with_extension("writing");
    {
        let file = File::create(&temp)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, value).map_err(std::io::Error::other)?;
        writer.flush()?;
    }
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)
}
