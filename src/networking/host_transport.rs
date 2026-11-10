// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::session::SessionMessage;
use super::{SessionConnection, SessionHost};
use std::io;
use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub enum HostEvent {
    Connected {
        player_id: u64,
        outgoing: SyncSender<SessionMessage>,
    },
    Message {
        player_id: u64,
        message: SessionMessage,
    },
    Disconnected {
        player_id: u64,
    },
}

pub struct HostListener {
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for HostListener {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn start(address: SocketAddr, tx: Sender<HostEvent>) -> io::Result<(SocketAddr, HostListener)> {
    let listener = SessionHost::bind(address)?;
    let bound_address = listener.local_addr()?;
    eprintln!("Worldkiln multiplayer listening on {bound_address}");
    let stopped = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stopped);
    let worker = std::thread::Builder::new().name("worldkiln-session-host".into()).spawn(move || {
        let active = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut next_player_id: u64 = 2;
        while !stop_worker.load(Ordering::Acquire) {
            let connection = match listener.accept(std::time::Duration::from_millis(100)) {
                Ok(conn) => conn,
                Err(error) if error.kind() == io::ErrorKind::TimedOut => continue,
                Err(error) => {
                    eprintln!("Network accept error: {error}");
                    continue;
                }
            };
            let capacity = std::env::var("WORLDKILN_MULTIPLAYER_MAX_PLAYERS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(8)
                .clamp(2, 8);
            if active.load(std::sync::atomic::Ordering::Relaxed) >= capacity - 1 {
                let _ = connection.close();
                continue;
            }
            let player_id = next_player_id;
            next_player_id = next_player_id.saturating_add(1);
            active.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let active_count = std::sync::Arc::clone(&active);
            let sender = tx.clone();
            std::thread::spawn(move || {
                let (outgoing, incoming) = std::sync::mpsc::sync_channel(2);
                let _ = sender.send(HostEvent::Connected {
                    player_id,
                    outgoing,
                });
                let _ = serve_player(player_id, connection, incoming, &sender);
                let _ = sender.send(HostEvent::Disconnected { player_id });
                active_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
            });
        }
    })?;
    Ok((bound_address, HostListener { stopped, worker: Some(worker) }))
}

fn serve_player(
    player_id: u64,
    mut connection: SessionConnection,
    replies: Receiver<SessionMessage>,
    events: &Sender<HostEvent>,
) -> io::Result<()> {
    let assigned = replies
        .recv_timeout(std::time::Duration::from_secs(3))
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "player assignment timeout"))?;
    let assigned_character = match &assigned {
        SessionMessage::Assigned {
            player_id: id,
            character_id,
        } if *id == player_id => *character_id,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "wrong player assignment",
            ));
        }
    };
    connection.send_message(&assigned)?;
    loop {
        match connection.receive_message() {
            Ok(Some(message @ SessionMessage::CharacterPose { .. })) => {
                let (character_id, sequence) = match &message {
                    SessionMessage::CharacterPose {
                        character_id,
                        sequence,
                        ..
                    } => (*character_id, *sequence),
                    _ => unreachable!(),
                };
                if character_id != assigned_character {
                    connection.send_message(&SessionMessage::Rejected {
                        sequence,
                        reason: "Character does not belong to this connection".into(),
                    })?;
                    continue;
                }
                if events
                    .send(HostEvent::Message { player_id, message })
                    .is_err()
                {
                    break;
                }
                let answer = replies
                    .recv_timeout(std::time::Duration::from_secs(1))
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "authoritative update timeout")
                    })?;
                connection.send_message(&answer)?;
            }
            Ok(None) => break,
            Ok(Some(_)) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                continue;
            }
            Err(_) => break,
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod tests;
