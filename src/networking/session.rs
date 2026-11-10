// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

use super::cell_replication::CellBatch;
use super::character_replication::CharacterBatch;
use super::physics_authority::SharedActorSnapshot;
use super::replication::ActorBatch;
use super::{MAGIC, PROTOCOL_VERSION, SessionConnection, bad_frame};
use crate::scripting::network::ScriptPacket;

const DATA: u8 = 5;
const MAX_PAYLOAD: usize = 128 * 1024;
const MAX_CHARACTERS: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterSnapshot {
    pub character_id: u64,
    pub owner: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SessionMessage {
    Assigned {
        player_id: u64,
        character_id: u64,
    },
    MovementIntent {
        character_id: u64,
        sequence: u64,
        axes: [f32; 2],
        jump: bool,
    },
    CharacterPose {
        #[serde(default)]
        request: Option<ScriptPacket>,
        character_id: u64,
        sequence: u64,
        position: [f32; 3],
        rotation: [f32; 4],
        push: bool,
        clip: String,
        clip_time: f32,
        clip_playing: bool,
        clip_rate: f32,
    },
    Snapshot {
        #[serde(default)]
        update: Option<ScriptPacket>,
        #[serde(default)]
        players: Vec<u64>,
        server_tick: u64,
        characters: Vec<CharacterSnapshot>,
        actors: Option<ActorBatch>,
        character_batch: Option<CharacterBatch>,
        cells: Option<CellBatch>,
    },
    PhysicsSnapshot {
        state: SharedActorSnapshot,
    },
    Rejected {
        sequence: u64,
        reason: String,
    },
}

fn valid_pose(position: &[f32; 3], rotation: &[f32; 4]) -> bool {
    position.iter().all(|value| value.is_finite())
        && rotation.iter().all(|value| value.is_finite())
        && (0.98..=1.02).contains(&rotation.iter().map(|value| value * value).sum::<f32>())
}

impl SessionMessage {
    fn validate(&self) -> io::Result<()> {
        match self {
            Self::Assigned {
                player_id,
                character_id,
            } if *player_id != 0 && *character_id != 0 => {}
            Self::MovementIntent {
                character_id, axes, ..
            } if *character_id != 0
                && axes
                    .iter()
                    .all(|axis| axis.is_finite() && axis.abs() <= 1.0) => {}
            Self::CharacterPose {
                request,
                character_id,
                position,
                rotation,
                clip,
                clip_time,
                clip_rate,
                ..
            } if request.as_ref().is_none_or(ScriptPacket::valid)
                && *character_id != 0
                && valid_pose(position, rotation)
                && !clip.is_empty()
                && clip.len() <= 160
                && clip_time.is_finite()
                && *clip_time >= 0.0
                && clip_rate.is_finite()
                && (0.0..=8.0).contains(clip_rate) => {}
            Self::Snapshot {
                update,
                players,
                characters,
                actors,
                character_batch,
                cells,
                ..
            } if update.as_ref().is_none_or(ScriptPacket::valid)
                && players.len() <= MAX_CHARACTERS
                && actors.as_ref().is_none_or(ActorBatch::valid)
                && character_batch.as_ref().is_none_or(CharacterBatch::valid)
                && cells.as_ref().is_none_or(CellBatch::valid)
                && characters.len() <= MAX_CHARACTERS
                && characters.iter().all(|character| {
                    character.character_id != 0
                        && character.owner != 0
                        && valid_pose(&character.position, &character.rotation)
                }) => {}
            Self::PhysicsSnapshot { state } if state.is_valid() => {}
            Self::Rejected { reason, .. } if !reason.is_empty() && reason.len() <= 256 => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid network session message",
                ));
            }
        }
        Ok(())
    }
}

impl SessionConnection {
    pub fn send_message(&mut self, message: &SessionMessage) -> io::Result<()> {
        message.validate()?;
        let payload = serde_json::to_vec(message)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD {
            return Err(bad_frame("network message exceeds payload limit"));
        }
        super::send(&mut self.stream, DATA, self.nonce)?;
        self.stream
            .write_all(&(payload.len() as u32).to_be_bytes())?;
        self.stream.write_all(&payload)
    }

    pub fn receive_message(&mut self) -> io::Result<Option<SessionMessage>> {
        let mut header = [0u8; 16];
        self.stream.read_exact(&mut header)?;
        if header[..4] != MAGIC
            || header[4..6] != PROTOCOL_VERSION.to_be_bytes()
            || header[7] != 0
            || header[8..] != self.nonce.to_be_bytes()
        {
            return Err(bad_frame("invalid network session frame"));
        }
        if header[6] == super::CLOSE {
            return Ok(None);
        }
        if header[6] != DATA {
            return Err(bad_frame("unexpected network session frame type"));
        }
        let mut length = [0u8; 4];
        self.stream.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > MAX_PAYLOAD {
            return Err(bad_frame("network message exceeds payload limit"));
        }
        let mut buffer = vec![0; length];
        self.stream.read_exact(&mut buffer)?;
        let message: SessionMessage = serde_json::from_slice(&buffer)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        message.validate()?;
        Ok(Some(message))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SessionHost, ownership::CharacterOwnership};
    use super::*;
    use std::net::SocketAddr;
    use std::thread;
    use std::time::Duration;

    fn connect_pair() -> (SessionHost, SocketAddr) {
        let host = SessionHost::bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = host.local_addr().unwrap();
        (host, address)
    }

    #[test]
    fn two_clients_receive_authoritative_character_states() {
        let (host, address) = connect_pair();
        let server = thread::spawn(move || {
            let mut first = host.accept(Duration::from_secs(3)).unwrap();
            let mut second = host.accept(Duration::from_secs(3)).unwrap();
            let mut ownership = CharacterOwnership::default();
            let first_character = ownership.join(1).unwrap();
            let second_character = ownership.join(2).unwrap();
            first
                .send_message(&SessionMessage::Assigned {
                    player_id: 1,
                    character_id: first_character,
                })
                .unwrap();
            second
                .send_message(&SessionMessage::Assigned {
                    player_id: 2,
                    character_id: second_character,
                })
                .unwrap();
            let first_input = first.receive_message().unwrap().unwrap();
            let second_input = second.receive_message().unwrap().unwrap();
            assert_eq!(
                first_input,
                SessionMessage::MovementIntent {
                    character_id: first_character,
                    sequence: 1,
                    axes: [1.0, 0.0],
                    jump: false
                }
            );
            assert_eq!(
                second_input,
                SessionMessage::MovementIntent {
                    character_id: second_character,
                    sequence: 1,
                    axes: [0.0, 1.0],
                    jump: false
                }
            );
            assert_eq!(
                ownership.move_owned(1, first_character, [1.0, 0.0, 0.0]),
                Ok([1.0, 0.0, 0.0])
            );
            assert_eq!(
                ownership.move_owned(2, second_character, [0.0, 0.0, 1.0]),
                Ok([0.0, 0.0, 1.0])
            );
            let snapshot = SessionMessage::Snapshot {
                update: None,
                players: Vec::new(),
                server_tick: 1,
                actors: None,
                character_batch: None,
                cells: None,
                characters: vec![
                    CharacterSnapshot {
                        character_id: first_character,
                        owner: 1,
                        position: ownership.character(first_character).unwrap().position,
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    },
                    CharacterSnapshot {
                        character_id: second_character,
                        owner: 2,
                        position: ownership.character(second_character).unwrap().position,
                        rotation: [0.0, 0.0, 0.0, 1.0],
                    },
                ],
            };
            first.send_message(&snapshot).unwrap();
            second.send_message(&snapshot).unwrap();
            assert_eq!(first.receive_message().unwrap(), None);
            assert_eq!(second.receive_message().unwrap(), None);
            ownership.disconnect(1);
            ownership.disconnect(2);
            assert_eq!(ownership.count(), 0);
            snapshot
        });
        let mut first = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
        let mut second = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
        let first_id = match first.receive_message().unwrap().unwrap() {
            SessionMessage::Assigned {
                player_id: 1,
                character_id,
            } => character_id,
            other => panic!("unexpected assignment: {other:?}"),
        };
        let second_id = match second.receive_message().unwrap().unwrap() {
            SessionMessage::Assigned {
                player_id: 2,
                character_id,
            } => character_id,
            other => panic!("unexpected assignment: {other:?}"),
        };
        assert_ne!(first_id, second_id);
        first
            .send_message(&SessionMessage::MovementIntent {
                character_id: first_id,
                sequence: 1,
                axes: [1.0, 0.0],
                jump: false,
            })
            .unwrap();
        second
            .send_message(&SessionMessage::MovementIntent {
                character_id: second_id,
                sequence: 1,
                axes: [0.0, 1.0],
                jump: false,
            })
            .unwrap();
        let first_state = first.receive_message().unwrap().unwrap();
        let second_state = second.receive_message().unwrap().unwrap();
        assert_eq!(first_state, second_state);
        assert_eq!(
            first_state,
            match first_state.clone() {
                SessionMessage::Snapshot {
                    server_tick: 1,
                    characters,
                    actors: None,
                    character_batch: None,
                    cells: None,
                    ..
                } if characters.len() == 2 => SessionMessage::Snapshot {
                    server_tick: 1,
                    characters,
                    actors: None,
                    character_batch: None,
                    cells: None,
                    update: None,
                    players: Vec::new()
                },
                other => panic!("unexpected state: {other:?}"),
            }
        );
        first.close().unwrap();
        second.close().unwrap();
        assert_eq!(server.join().unwrap(), first_state);
    }

    #[test]
    fn rejects_non_finite_and_out_of_range_inputs() {
        let (host, address) = connect_pair();
        let server = thread::spawn(move || {
            host.accept(Duration::from_secs(3))
                .unwrap()
                .wait_for_close()
                .unwrap()
        });
        let mut client = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
        for axes in [[f32::NAN, 0.0], [f32::INFINITY, 0.0], [1.1, 0.0]] {
            assert_eq!(
                client
                    .send_message(&SessionMessage::MovementIntent {
                        character_id: 1,
                        sequence: 1,
                        axes,
                        jump: false
                    })
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
        client.close().unwrap();
        server.join().unwrap();
    }

    #[test]
    fn character_pose_preserves_facing_and_rejects_malformed_transforms() {
        let pose = SessionMessage::CharacterPose {
            request: None,
            character_id: 2,
            sequence: 7,
            position: [3.0, 1.0, -4.0],
            rotation: [0.0, 0.70710677, 0.0, 0.70710677],
            push: false,
            clip: "Idle".into(),
            clip_time: 0.0,
            clip_playing: true,
            clip_rate: 1.0,
        };
        assert!(pose.validate().is_ok());
        let encoded = serde_json::to_vec(&pose).unwrap();
        let decoded: SessionMessage = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(pose, decoded);
        let bad_rotation = SessionMessage::CharacterPose {
            request: None,
            character_id: 2,
            sequence: 8,
            position: [3.0, 1.0, -4.0],
            rotation: [0.0; 4],
            push: false,
            clip: "Idle".into(),
            clip_time: 0.0,
            clip_playing: true,
            clip_rate: 1.0,
        };
        assert!(bad_rotation.validate().is_err());
        let bad_position = SessionMessage::CharacterPose {
            request: None,
            character_id: 2,
            sequence: 9,
            position: [f32::NAN, 1.0, -4.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            push: false,
            clip: "Idle".into(),
            clip_time: 0.0,
            clip_playing: true,
            clip_rate: 1.0,
        };
        assert!(bad_position.validate().is_err());
    }

    #[test]
    fn authoritative_rapier_snapshot_crosses_a_network_session() {
        use super::super::physics_authority::{SharedActorAuthority, SharedActorReplica};
        let (host, address) = connect_pair();
        let server = thread::spawn(move || {
            let mut connection = host.accept(Duration::from_secs(3)).unwrap();
            let mut authority = SharedActorAuthority::new();
            authority.advance(1.0 / 30.0).unwrap();
            let state = authority.snapshot();
            connection
                .send_message(&SessionMessage::PhysicsSnapshot { state })
                .unwrap();
            connection.wait_for_close().unwrap();
            state
        });
        let mut client = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
        let state = match client.receive_message().unwrap().unwrap() {
            SessionMessage::PhysicsSnapshot { state } => state,
            message => panic!("unexpected physics message: {message:?}"),
        };
        assert_eq!(state.tick, 2);
        let mut replica = SharedActorReplica::default();
        assert!(replica.apply(state));
        assert!(!replica.apply(state));
        client.close().unwrap();
        assert_eq!(server.join().unwrap(), replica.state().unwrap());
    }

    #[test]
    fn rejects_oversized_payload_before_allocation() {
        let (host, address) = connect_pair();
        let server = thread::spawn(move || {
            let mut connection = host.accept(Duration::from_secs(3)).unwrap();
            assert_eq!(
                connection.receive_message().unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
        });
        let mut client = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
        super::super::send(&mut client.stream, DATA, client.nonce).unwrap();
        client
            .stream
            .write_all(&((MAX_PAYLOAD + 1) as u32).to_be_bytes())
            .unwrap();
        server.join().unwrap();
    }
}
