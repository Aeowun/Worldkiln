// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use glam::{Quat, Vec2, Vec3};
use winit::keyboard::KeyCode;

use super::cell_replication::{CellBatch, CellReplicator};
use super::character_replication::{CharacterBatch, CharacterReplicator};
use super::host_transport::{self, HostEvent};
use super::replication::{ActorBatch, ActorReplicator};
use super::session::{CharacterSnapshot, SessionMessage};
use super::{SessionConnection, SessionHost};
use crate::character::animation::TargetAnimation;
use crate::game::GameRuntime;
use crate::scripting::network::{NetworkRole, ScriptPacket};
use crate::scripting::value::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkTestRole {
    Host,
    Join,
}

enum Incoming {
    Assigned {
        player_id: u64,
        character_id: u64,
    },
    State {
        update: Option<ScriptPacket>,
        players: Vec<u64>,
        characters: Vec<CharacterSnapshot>,
        actors: Option<ActorBatch>,
        character_batch: Option<CharacterBatch>,
        cells: Option<CellBatch>,
    },
    Disconnected,
    Rejected(String),
}

enum Outgoing {
    Input(SessionMessage),
}

struct HostPeer {
    character_id: u64,
    outgoing: SyncSender<SessionMessage>,
    last_sequence: u64,
    last_pose: Instant,
    delivered: BTreeMap<String, u64>,
    actor_replication: ActorReplicator,
    character_replication: CharacterReplicator,
    cell_replication: CellReplicator,
}

fn pose_is_plausible(previous: Vec3, next: Vec3, seconds: f32) -> bool {
    previous.is_finite()
        && next.is_finite()
        && next.distance(previous) <= seconds.clamp(0.0, 0.5) * 28.0
}

fn host_pose_respects_collision(game: &mut GameRuntime, character: u64, candidate: Vec3) -> bool {
    let Some(remote) = game.character_system.get_active_characters_mut().find(|c| c.id == character) else {
        return false;
    };
    if !remote.collision_enabled || !remote.collision_solid {
        return true;
    }
    let previous = remote.transform.position;
    let original_movement = remote.movement.clone();
    remote.transform.position = candidate;
    let mut contacted = std::collections::HashSet::new();
    crate::character::collision::resolve_static_voxel_collisions(
        remote,
        &game.physics_world,
        &mut contacted,
        previous,
        game.world.max_walkable_slope_degrees,
    );
    let static_resolved = remote.transform.position;
    let terrain_resolved = crate::character::collider::sweep_terrain(
        &game.physics_world,
        &game.world,
        remote,
        previous,
        static_resolved,
    ).map(|(position, _)| position);
    remote.transform.position = previous;
    remote.movement = original_movement;
    terrain_resolved.is_some_and(|position| position.distance(candidate) <= 0.05)
}

fn peer_offset(player_id: u64) -> Vec3 {
    let angle = (player_id.saturating_sub(2) as f32) * 2.4;
    Vec3::new(angle.cos() * 2.0, 0.0, angle.sin() * 2.0)
}

#[cfg(test)]
mod movement_tests {
    use super::*;

    #[test]
    fn guest_motion_accepts_normal_speed_and_rejects_teleportation() {
        let previous = Vec3::new(-8.0, 1.0, 9.0);
        assert!(pose_is_plausible(
            previous,
            previous + Vec3::X * 0.17,
            1.0 / 30.0
        ));
        assert!(!pose_is_plausible(
            previous,
            previous + Vec3::X * 12.0,
            1.0 / 30.0
        ));
        assert!(!pose_is_plausible(
            previous,
            Vec3::splat(f32::NAN),
            1.0 / 30.0
        ));
    }
}

pub struct MultiplayerTestSession {
    pub(crate) role: NetworkTestRole,
    incoming: Receiver<Incoming>,
    host_events: Receiver<HostEvent>,
    host_listener: Option<host_transport::HostListener>,
    peers: HashMap<u64, HostPeer>,
    outgoing: SyncSender<Outgoing>,
    player_id: u64,
    character_id: u64,
    character_replication: CharacterReplicator,
    cell_replication: CellReplicator,
    next_sequence: u64,
    previous_input: Instant,
    last_diagnostics: Instant,
    connected: bool,
    actor_replication: ActorReplicator,
    pub test_controls: bool,
    pub bound_address: SocketAddr,
    push_was_down: bool,
    pending_push: bool,
    host_lost: bool,
}

impl MultiplayerTestSession {
    pub fn start(role: NetworkTestRole, address: SocketAddr) -> io::Result<Self> {
        let (incoming_tx, incoming) = mpsc::channel();
        let (host_events_tx, host_events) = mpsc::channel();
        let (outgoing, outgoing_rx) = mpsc::sync_channel(4);
        let mut bound_address = address;
        let mut host_listener = None;
        match role {
            NetworkTestRole::Host => {
                let (bound, listener) = host_transport::start(address, host_events_tx)?;
                bound_address = bound;
                host_listener = Some(listener);
            }
            NetworkTestRole::Join => {
                let mut conn = SessionConnection::connect(address, Duration::from_secs(3))?;
                eprintln!("Worldkiln multiplayer test connected to {address}");
                thread::spawn(move || {
                    match conn.receive_message() {
                        Ok(Some(SessionMessage::Assigned {
                            player_id,
                            character_id,
                        })) => {
                            let _ = incoming_tx.send(Incoming::Assigned {
                                player_id,
                                character_id,
                            });
                        }
                        _ => {
                            let _ = incoming_tx.send(Incoming::Disconnected);
                            return;
                        }
                    }
                    loop {
                        let input = match outgoing_rx.recv_timeout(Duration::from_secs(3)) {
                            Ok(Outgoing::Input(input)) => input,
                            Ok(_) => continue,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(_) => break,
                        };
                        if conn.send_message(&input).is_err() {
                            break;
                        }
                        match conn.receive_message() {
                            Ok(Some(SessionMessage::Snapshot {
                                update,
                                players,
                                characters,
                                actors,
                                character_batch,
                                cells,
                                ..
                            })) => {
                                let _ = incoming_tx.send(Incoming::State {
                                    update,
                                    players,
                                    characters,
                                    actors,
                                    character_batch,
                                    cells,
                                });
                            }
                            Ok(Some(SessionMessage::Rejected { reason, .. })) => {
                                let _ = incoming_tx.send(Incoming::Rejected(reason));
                            }
                            _ => break,
                        }
                    }
                    let _ = incoming_tx.send(Incoming::Disconnected);
                });
            }
        }
        Ok(Self {
            role,
            incoming,
            host_events,
            host_listener,
            peers: HashMap::new(),
            outgoing,
            player_id: 0,
            character_id: 0,
            character_replication: CharacterReplicator::default(),
            cell_replication: CellReplicator::default(),
            next_sequence: 0,
            previous_input: Instant::now(),
            last_diagnostics: Instant::now(),
            connected: false,
            actor_replication: ActorReplicator::default(),
            bound_address,
            test_controls: false,
            push_was_down: false,
            pending_push: false,
            host_lost: false,
        })
    }

    fn owners(&self, game: &GameRuntime) -> Vec<(u64, u64)> {
        let mut owners = Vec::with_capacity(1 + self.peers.len());
        if let Some(player) = game.character_system.get_active_player() {
            owners.push((1, player.id));
        }
        owners.extend(
            self.peers
                .iter()
                .map(|(&network_id, peer)| (network_id, peer.character_id)),
        );
        owners
    }

    fn snapshot_for(&mut self, player_id: u64, game: &GameRuntime) -> Option<SessionMessage> {
        let owners = self.owners(game);
        let peer = self.peers.get_mut(&player_id)?;
        let io = game.script_network.io.as_ref()?.borrow();
        let update = io
            .states
            .values()
            .filter(|(rev, packet)| peer.delivered.get(&packet.topic).copied().unwrap_or(0) < *rev)
            .min_by_key(|(rev, _)| rev)
            .map(|(rev, packet)| {
                peer.delivered.insert(packet.topic.clone(), *rev);
                packet.clone()
            });
        Some(SessionMessage::Snapshot {
            update,
            players: owners.iter().map(|(owner, _)| *owner).collect(),
            server_tick: peer.last_sequence,
            actors: Some(peer.actor_replication.snapshot(game)),
            character_batch: Some(
                peer.character_replication
                    .snapshot(&game.character_system, &owners),
            ),
            cells: Some(
                peer.cell_replication
                    .snapshot_with_physics(&game.world, &game.physics_world),
            ),
            characters: Vec::new(),
        })
    }

    fn handle_host_event(&mut self, game: &mut GameRuntime, event: HostEvent) {
        match event {
            HostEvent::Connected {
                player_id,
                outgoing,
            } => {
                let Some(local) = game.character_system.get_active_player() else {
                    return;
                };
                let pos = local.transform.position + peer_offset(player_id);
                let package = local.package_name.clone();
                let id = game.character_system.spawn_character(pos, Some(&package));
                let entity = game
                    .entity_manager
                    .create_entity(&format!("Peer {player_id}"));
                game.character_system.associate_entity(entity, id);
                game.character_system.set_entity_kinematic(entity, true);
                if let Some(io) = &game.script_network.io {
                    io.borrow_mut().players.insert(player_id, Some(entity.0));
                }
                game.script_pending_events.push((
                    "network_player_joined".into(),
                    vec![Value::Number(player_id as f64)],
                ));
                self.peers.insert(
                    player_id,
                    HostPeer {
                        delivered: Default::default(),
                        character_id: id,
                        outgoing: outgoing.clone(),
                        last_sequence: 0,
                        last_pose: Instant::now(),
                        actor_replication: ActorReplicator::default(),
                        character_replication: CharacterReplicator::default(),
                        cell_replication: CellReplicator::default(),
                    },
                );
                let _ = outgoing.try_send(SessionMessage::Assigned {
                    player_id,
                    character_id: player_id,
                });
                self.connected = true;
                eprintln!("MP-03 peer joined player={player_id} Character={id}");
            }
            HostEvent::Message { player_id, message } => {
                self.handle_host_message(game, player_id, message);
            }
            HostEvent::Disconnected { player_id } => {
                if let Some(peer) = self.peers.remove(&player_id) {
                    if let Some(entity) = game
                        .character_system
                        .get_entity_for_character(peer.character_id)
                    {
                        game.character_system.destroy_entity(entity);
                        game.entity_manager.remove_entity(entity);
                    }
                    if let Some(io) = &game.script_network.io {
                        io.borrow_mut().players.remove(&player_id);
                    }
                    game.script_pending_events.push((
                        "network_player_left".into(),
                        vec![Value::Number(player_id as f64)],
                    ));
                    eprintln!("MP-03 peer disconnected player={player_id}");
                }
                self.connected = !self.peers.is_empty();
            }
        }
    }

    fn handle_host_message(
        &mut self,
        game: &mut GameRuntime,
        player_id: u64,
        message: SessionMessage,
    ) {
        let SessionMessage::CharacterPose {
            request,
            character_id,
            sequence,
            position,
            rotation,
            push,
            clip,
            clip_time,
            clip_playing,
            clip_rate,
        } = message
        else {
            return;
        };
        let candidate = Vec3::from_array(position);
        let Some(peer) = self.peers.get_mut(&player_id) else {
            return;
        };
        let elapsed = peer.last_pose.elapsed().as_secs_f32();
        let valid = character_id == player_id
            && sequence > peer.last_sequence
            && game
                .character_system
                .get_active_characters()
                .find(|c| c.id == peer.character_id)
                .is_some_and(|c| pose_is_plausible(c.transform.position, candidate, elapsed))
            && host_pose_respects_collision(game, peer.character_id, candidate);
        if !valid {
            let pose = game
                .character_system
                .get_active_characters()
                .find(|c| c.id == peer.character_id)
                .map(|c| CharacterSnapshot {
                    character_id: player_id,
                    owner: player_id,
                    position: c.transform.position.to_array(),
                    rotation: c.transform.rotation.to_array(),
                });
            if let Some(pose) = pose {
                peer.last_sequence = peer.last_sequence.max(sequence);
                if let Some(mut snapshot) = self.snapshot_for(player_id, game) {
                    if let SessionMessage::Snapshot { characters, .. } = &mut snapshot {
                        characters.push(pose);
                    }
                    let _ = self.peers[&player_id].outgoing.try_send(snapshot);
                }
                return;
            }
            let _ = peer.outgoing.try_send(SessionMessage::Rejected {
                sequence,
                reason: "Invalid ownership, sequence or movement bounds".into(),
            });
            return;
        }
        peer.last_sequence = sequence;
        peer.last_pose = Instant::now();
        if let Some(request) = request {
            let mut args = request.arguments();
            args.insert(0, Value::Number(player_id as f64));
            game.script_pending_events.push(("network_request".into(), args));
        }
        let character = peer.character_id;
        if self.test_controls && push {
            self.actor_replication
                .push_nearest(game, player_id, candidate);
        }
        let previous_position = game
            .character_system
            .get_active_characters()
            .find(|c| c.id == character)
            .map(|c| c.transform.position)
            .unwrap_or(candidate);
        if let Some(remote) = game
            .character_system
            .get_active_characters_mut()
            .find(|c| c.id == character)
        {
            let previous = remote.transform.position;
            remote.transform.position = candidate;
            remote.transform.rotation = Quat::from_array(rotation);
            let forward = remote.transform.rotation * Vec3::Z;
            remote.facing = Vec2::new(forward.x, forward.z).normalize_or_zero();
            remote.movement.velocity = (candidate - previous) / elapsed.max(0.01);
            remote.animation_controller.sync_network_playback(
                &clip,
                clip_time,
                clip_playing,
                clip_rate,
            );
            remote.animation_override = Some(TargetAnimation::Named(clip));
        }
        // Resolve remote-player contact on the authoritative host, not on replica clients.
        // Only actual movement toward a solid host Character can displace the host.
        let remote_contact = game
            .character_system
            .get_active_characters()
            .find(|c| c.id == character)
            .filter(|c| c.collision_enabled && c.collision_solid)
            .map(|c| (c.transform.position, c.collision.radius, c.collision.height));
        if let Some((remote_pos, remote_radius, remote_height)) = remote_contact {
            if let Some(host) = game.character_system.get_active_player_mut() {
                if host.id != character && host.collision_enabled && host.collision_solid {
                    let vertical = (host.transform.position.y - remote_pos.y).abs();
                    if vertical < host.collision.height.min(remote_height) {
                        let between = Vec2::new(
                            host.transform.position.x - remote_pos.x,
                            host.transform.position.z - remote_pos.z,
                        );
                        let distance = between.length();
                        let separation = host.collision.radius + remote_radius;
                        let motion = Vec2::new(
                            candidate.x - previous_position.x,
                            candidate.z - previous_position.z,
                        );
                        let direction = if distance > 0.0001 {
                            between / distance
                        } else {
                            motion.normalize_or_zero()
                        };
                        let advance = motion.dot(direction).max(0.0);
                        if distance < separation && advance > 0.0 {
                            let displacement =
                                direction * (separation - distance).min(advance).min(0.35);
                            host.transform.position.x += displacement.x;
                            host.transform.position.z += displacement.y;
                        }
                    }
                }
            }
        }
        if let Some(answer) = self.snapshot_for(player_id, game) {
            if let Some(peer) = self.peers.get(&player_id) {
                let _ = peer.outgoing.try_send(answer);
            }
        }
    }

    pub fn host_lost(&self) -> bool {
        self.role == NetworkTestRole::Join && self.host_lost
    }

    pub fn recover_offline(&mut self, game: &mut GameRuntime) {
        self.actor_replication.restore_guest_simulation(game);
        self.cell_replication
            .restore_guest_simulation(&mut game.physics_world);
        self.character_replication
            .release_guest_replicas(&mut game.character_system);
    }

    pub fn tick(&mut self, game: &mut GameRuntime, _frame_time: f32) {
        if self.role == NetworkTestRole::Join && !self.host_lost {
            self.actor_replication.disable_guest_simulation(game);
        }
        let push_down = self.test_controls && game.keys_down.contains(&KeyCode::KeyG);
        if push_down && !self.push_was_down {
            if self.role == NetworkTestRole::Host {
                if let Some(player) = game
                    .character_system
                    .get_active_player()
                    .map(|p| p.transform.position)
                {
                    self.actor_replication.push_nearest(game, 1, player);
                }
            } else {
                self.pending_push = true;
            }
        }
        self.push_was_down = push_down;
        if self.last_diagnostics.elapsed() >= Duration::from_secs(3) {
            self.last_diagnostics = Instant::now();
            let player = game
                .character_system
                .get_active_player()
                .map(|c| (c.id, c.transform.position, c.movement.velocity));
            eprintln!(
                "MP-03 DIAG role={:?} connected={} player={:?} peers={} camera_position={:?} camera_target={:?}",
                self.role,
                self.connected,
                player,
                self.peers.len(),
                game.gameplay_camera.current_position,
                game.gameplay_camera.current_target
            );
        }
        while let Ok(event) = self.host_events.try_recv() {
            if self.role == NetworkTestRole::Host {
                self.handle_host_event(game, event);
            }
        }
        while let Ok(event) = self.incoming.try_recv() {
            match event {
                Incoming::Assigned {
                    player_id,
                    character_id,
                } => {
                    if !self.connected {
                        if let Some(player) = game.character_system.get_active_player_mut() {
                            player.transform.position += peer_offset(player_id);
                            player.movement.velocity = Vec3::ZERO;
                        }
                    }
                    self.player_id = player_id;
                    self.character_id = character_id;
                    self.connected = true;
                    game.script_network.player_id = Some(player_id);
                    game.script_network.connected = true;
                }
                Incoming::Disconnected => {
                    self.connected = false;
                    if self.role == NetworkTestRole::Join {
                        self.host_lost = true;
                        eprintln!("Multiplayer host disconnected; restoring local gameplay");
                        break;
                    }
                }
                Incoming::Rejected(reason) => eprintln!("Multiplayer input rejected: {reason}"),
                Incoming::State {
                    update,
                    players,
                    characters,
                    actors,
                    character_batch,
                    cells,
                } if self.role == NetworkTestRole::Join => {
                    if let Some(io) = &game.script_network.io {
                        io.borrow_mut().players =
                            players.into_iter().map(|id| (id, None)).collect();
                    }
                    if let Some(update) = update {
                        game.script_pending_events
                            .push(("network_state".into(), update.arguments()));
                    }
                    if let Some(correction) = characters.iter().find(|c| c.owner == self.player_id)
                    {
                        if let Some(local) = game.character_system.get_active_player_mut() {
                            local.transform.position = Vec3::from_array(correction.position);
                            local.movement.velocity = Vec3::ZERO;
                        }
                    }
                    if let Some(batch) = cells {
                        self.cell_replication.apply_with_physics(
                            &mut game.world,
                            &mut game.physics_world,
                            batch,
                        );
                    }
                    if let Some(batch) = actors {
                        self.actor_replication.apply(game, batch);
                    }
                    if let Some(batch) = character_batch {
                        self.character_replication.apply(
                            &mut game.character_system,
                            self.player_id,
                            batch,
                        );
                    }
                }
                _ => {}
            }
        }

        if self.role == NetworkTestRole::Join
            && !self.host_lost
            && self.connected
            && self.character_id != 0
            && self.previous_input.elapsed() >= Duration::from_millis(33)
        {
            self.previous_input = Instant::now();
            self.next_sequence += 1;
            if let Some(player) = game.character_system.get_active_player() {
                let request = game
                    .script_network
                    .io
                    .as_ref()
                    .and_then(|io| io.borrow().requests.front().cloned());
                let submitted = self
                    .outgoing
                    .try_send(Outgoing::Input(SessionMessage::CharacterPose {
                        request,
                        character_id: self.character_id,
                        sequence: self.next_sequence,
                        position: player.transform.position.to_array(),
                        rotation: player.transform.rotation.to_array(),
                        push: self.pending_push,
                        clip: player.animation_controller.current_clip_name().to_owned(),
                        clip_time: player.animation_controller.playback_time(),
                        clip_playing: player.animation_controller.is_playing(),
                        clip_rate: player.animation_controller.playback_rate(),
                    }))
                    .is_ok();
                if submitted {
                    if let Some(io) = &game.script_network.io {
                        io.borrow_mut().requests.pop_front();
                    }
                    self.pending_push = false;
                }
            }
        }
    }
}

impl GameRuntime {
    pub fn start_session(&mut self, role: NetworkTestRole, address: SocketAddr) -> io::Result<()> {
        let session = MultiplayerTestSession::start(role, address)?;
        self.activate_session(role, session);
        Ok(())
    }

    fn activate_session(&mut self, role: NetworkTestRole, session: MultiplayerTestSession) {
        let io = self.script_network.io.clone().unwrap_or_default();
        let mut data = io.borrow_mut();
        data.requests.clear();
        data.states.clear();
        data.players.clear();
        data.error.clear();
        let mut advertised = session.bound_address;
        if role == NetworkTestRole::Host && advertised.ip().is_unspecified() {
            if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
                if socket.connect("192.0.2.1:9").is_ok() {
                    if let Ok(local) = socket.local_addr() {
                        advertised.set_ip(local.ip());
                    }
                }
            }
        }
        data.address = advertised.to_string();
        self.script_network = match role {
            NetworkTestRole::Host => {
                data.players
                    .insert(1, self.entity_manager.lookup_entity("Player").map(|e| e.0));
                crate::scripting::network::NetworkScriptContext::host()
            }
            NetworkTestRole::Join => {
                crate::scripting::network::NetworkScriptContext::client(None, false)
            }
        };
        drop(data);
        self.script_network.io = Some(io);
        self.multiplayer = Some(session);
        self.script_pending_events
            .push(("network_started".into(), Vec::new()));
    }

    pub(crate) fn update_network(&mut self, dt: f32) {
        let finished = self
            .invite_task
            .as_ref()
            .and_then(|task| match task.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Invite connection stopped unexpectedly".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = finished {
            self.invite_task = None;
            match result {
                Ok(ready) => {
                    let link = ready.link.clone();
                    let hosting = ready.role == NetworkTestRole::Host;
                    self.activate_session(ready.role, ready.session);
                    self.invite_bridge = Some(ready.bridge);
                    if let Some(io) = &self.script_network.io {
                        io.borrow_mut().invite_link = link.clone();
                    }
                    if hosting {
                        self.script_pending_events
                            .push(("network_invite_ready".into(), vec![Value::String(link)]));
                    }
                }
                Err(error) => {
                    if let Some(io) = &self.script_network.io {
                        io.borrow_mut().error = error.clone();
                    }
                    self.script_pending_events
                        .push(("network_join_failed".into(), vec![Value::String(error)]));
                }
            }
        }
        let (invite, leave) = if let Some(io) = &self.script_network.io {
            let mut io = io.borrow_mut();
            (
                io.invite_request.take(),
                std::mem::take(&mut io.leave_requested),
            )
        } else {
            (None, false)
        };
        if leave {
            self.invite_task = None;
            self.invite_bridge = None;
            if let Some(mut session) = self.multiplayer.take() {
                if session.role == NetworkTestRole::Join {
                    session.recover_offline(self);
                }
            }
            self.script_network.role = NetworkRole::Solo;
            self.script_network.player_id = None;
            self.script_network.connected = false;
            if let Some(io) = &self.script_network.io {
                let mut io = io.borrow_mut();
                io.invite_link.clear();
                io.players.clear();
                io.requests.clear();
                io.states.clear();
            }
            self.script_pending_events
                .push(("network_disconnected".into(), Vec::new()));
        }
        if let Some(request) = invite {
            if self.multiplayer.is_none() && self.invite_task.is_none() {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                std::thread::spawn(move || {
                    let result = match request {
                        crate::scripting::network::InviteRequest::Host { max_players } => {
                            super::invite::host_invite(max_players)
                        }
                        crate::scripting::network::InviteRequest::Join { link } => {
                            super::invite::join_invite(link)
                        }
                    };
                    let _ = tx.send(result);
                });
                self.invite_task = Some(rx);
            } else if let Some(io) = &self.script_network.io {
                io.borrow_mut().error = "Already in or preparing a multiplayer session".into();
            }
        }
        let command = self
            .script_network
            .io
            .as_ref()
            .and_then(|io| io.borrow_mut().start.take());
        if let Some((role, address)) = command {
            let role = if role == NetworkRole::Host {
                NetworkTestRole::Host
            } else {
                NetworkTestRole::Join
            };
            if let Err(error) = self.start_session(role, address) {
                if let Some(io) = &self.script_network.io {
                    io.borrow_mut().error = error.to_string();
                }
            }
        }
        if let Some(mut session) = self.multiplayer.take() {
            session.tick(self, dt);
            if session.host_lost() {
                self.invite_bridge = None;
                session.recover_offline(self);
                self.script_network.role = NetworkRole::Solo;
                self.script_network.player_id = None;
                self.script_network.connected = false;
                if let Some(io) = &self.script_network.io {
                    let mut io = io.borrow_mut();
                    io.players.clear();
                    io.requests.clear();
                    io.states.clear();
                    io.start = None;
                    io.error = "Connection to host closed".into();
                }
                self.script_pending_events
                    .push(("network_disconnected".into(), Vec::new()));
            } else {
                self.multiplayer = Some(session);
            }
        }
    }
}
