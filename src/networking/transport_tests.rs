// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::super::SessionConnection;
use super::*;
use std::time::Duration;

fn assigned(player_id: u64) -> SessionMessage {
    SessionMessage::Assigned {
        player_id,
        character_id: player_id,
    }
}
fn pose(id: u64, sequence: u64) -> SessionMessage {
    SessionMessage::CharacterPose {
        request: None,
        character_id: id,
        sequence,
        position: [0.0, 1.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        push: false,
        clip: "Idle".into(),
        clip_time: 0.0,
        clip_playing: true,
        clip_rate: 1.0,
    }
}
fn state() -> SessionMessage {
    SessionMessage::Snapshot {
        update: None,
        players: Vec::new(),
        server_tick: 1,
        characters: Vec::new(),
        actors: None,
        character_batch: None,
        cells: None,
    }
}
fn accept(rx: &Receiver<HostEvent>, expected_id: u64) -> SyncSender<SessionMessage> {
    match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        HostEvent::Connected {
            player_id,
            outgoing,
        } => {
            assert_eq!(player_id, expected_id);
            outgoing.try_send(assigned(player_id)).unwrap();
            outgoing
        }
        _ => panic!("expected peer connection"),
    }
}
#[test]
fn separate_peers_keep_authority_and_survive_disconnect_late_join() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (address, listener) = start("127.0.0.1:0".parse().unwrap(), tx).unwrap();
    let mut a = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
    let sender_a = accept(&rx, 2);
    assert_eq!(a.receive_message().unwrap(), Some(assigned(2)));
    let mut b = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
    let sender_b = accept(&rx, 3);
    assert_eq!(b.receive_message().unwrap(), Some(assigned(3)));
    b.send_message(&pose(2, 1)).unwrap();
    assert!(matches!(
        b.receive_message().unwrap(),
        Some(SessionMessage::Rejected { .. })
    ));
    a.send_message(&pose(2, 1)).unwrap();
    match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        HostEvent::Message {
            player_id: 2,
            message,
        } => assert_eq!(message, pose(2, 1)),
        _ => panic!("wrong owner for first movement"),
    }
    sender_a.try_send(state()).unwrap();
    assert!(matches!(
        a.receive_message().unwrap(),
        Some(SessionMessage::Snapshot { .. })
    ));
    b.send_message(&pose(3, 1)).unwrap();
    match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        HostEvent::Message {
            player_id: 3,
            message,
        } => assert_eq!(message, pose(3, 1)),
        _ => panic!("wrong owner for second movement"),
    }
    sender_b.try_send(state()).unwrap();
    assert!(matches!(
        b.receive_message().unwrap(),
        Some(SessionMessage::Snapshot { .. })
    ));
    a.close().unwrap();
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        HostEvent::Disconnected { player_id: 2 }
    ));
    let mut c = SessionConnection::connect(address, Duration::from_secs(3)).unwrap();
    let sender_c = accept(&rx, 4);
    assert_eq!(c.receive_message().unwrap(), Some(assigned(4)));
    b.send_message(&pose(3, 2)).unwrap();
    match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        HostEvent::Message { player_id: 3, .. } => {}
        _ => panic!("surviving peer lost while third peer joined"),
    }
    sender_b.try_send(state()).unwrap();
    assert!(matches!(
        b.receive_message().unwrap(),
        Some(SessionMessage::Snapshot { .. })
    ));
    c.send_message(&pose(4, 1)).unwrap();
    match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        HostEvent::Message { player_id: 4, .. } => {}
        _ => panic!("late join did not establish ownership"),
    }
    sender_c.try_send(state()).unwrap();
    assert!(matches!(
        c.receive_message().unwrap(),
        Some(SessionMessage::Snapshot { .. })
    ));
    b.close().unwrap();
    c.close().unwrap();
    drop(listener);
    assert!(SessionHost::bind(address).is_ok(), "host listener must release its port");
}
