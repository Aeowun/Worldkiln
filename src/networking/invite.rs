// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde_json::Value as Json;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Error as WsError, Message, WebSocket, connect};

use super::play_session::{MultiplayerTestSession, NetworkTestRole};

const DEFAULT_RELAY: &str = "https://worldkiln-session-relay.aeowun-game-submissions.workers.dev";

pub struct InviteBridge {
    cancelled: Arc<AtomicBool>,
}

impl Drop for InviteBridge {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub struct PreparedInvite {
    pub session: MultiplayerTestSession,
    pub role: NetworkTestRole,
    pub link: String,
    pub bridge: InviteBridge,
}

fn relay_origin() -> Result<String, String> {
    let value = std::env::var("WORLDKILN_RELAY_URL").unwrap_or_else(|_| DEFAULT_RELAY.into());
    let url = reqwest::Url::parse(&value).map_err(|_| "Invalid relay address")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
    {
        return Err("Relay endpoint must be an HTTPS origin".into());
    }
    Ok(url.origin().ascii_serialization())
}

fn ws_origin(relay: &str) -> String {
    relay.replacen("https://", "wss://", 1)
}

fn socket_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> io::Result<()> {
    let timeout = Some(Duration::from_millis(10));
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(timeout),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(timeout),
        _ => Ok(()),
    }
}

fn ws_read(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<Option<Message>, String> {
    match socket.read() {
        Ok(message) => Ok(Some(message)),
        Err(WsError::Io(error))
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(None)
        }
        Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
            Err("Relay connection closed".into())
        }
        Err(error) => Err(format!("Relay transport: {error}")),
    }
}

fn tunnel(
    mut socket: WebSocket<MaybeTlsStream<TcpStream>>,
    mut tcp: TcpStream,
    stop: Arc<AtomicBool>,
) {
    let _ = socket_timeout(&mut socket);
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(10)));
    let _ = tcp.set_nodelay(true);
    let mut buffer = [0u8; 16 * 1024];
    while !stop.load(Ordering::Relaxed) {
        match tcp.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => {
                if socket
                    .send(Message::Binary(buffer[..length].to_vec().into()))
                    .is_err()
                {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut
                        | io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
        match ws_read(&mut socket) {
            Ok(Some(Message::Binary(bytes))) => {
                if tcp.write_all(&bytes).is_err() {
                    break;
                }
            }
            Ok(Some(Message::Close(_))) | Err(_) => break,
            Ok(_) => {}
        }
    }
    let _ = socket.close(None);
    let _ = tcp.shutdown(std::net::Shutdown::Both);
}

fn code_from_link(input: &str, relay: &str) -> Result<String, String> {
    let input = input.trim();
    let code = if input.len() == 32 && input.chars().all(|c| c.is_ascii_hexdigit()) {
        input.to_owned()
    } else {
        let url = reqwest::Url::parse(input).map_err(|_| "Paste a valid session invite link")?;
        if url.origin().ascii_serialization() != relay || url.path_segments().is_none() {
            return Err("Invitation belongs to a different relay".into());
        }
        let segments: Vec<_> = url.path_segments().unwrap().collect();
        if segments.len() != 2 || segments[0] != "join" {
            return Err("Invalid session invite link".into());
        }
        segments[1].to_owned()
    };
    if code.len() != 32 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid session invitation".into());
    }
    Ok(code.to_ascii_lowercase())
}

pub fn host_invite(max_players: usize) -> Result<PreparedInvite, String> {
    let relay = relay_origin()?;
    let session =
        MultiplayerTestSession::start(NetworkTestRole::Host, "127.0.0.1:0".parse().unwrap())
            .map_err(|error| format!("Cannot start game host: {error}"))?;
    let body = serde_json::json!({"game": "worldkiln-v1", "max_players": max_players.clamp(2, 8)});
    let data: Json = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| e.to_string())?
        .post(format!("{relay}/v1/sessions"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|error| format!("Cannot contact online relay: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Online relay rejected host: {error}"))?
        .text()
        .map_err(|error| format!("Invalid relay response: {error}"))
        .and_then(|content| {
            serde_json::from_str(&content)
                .map_err(|error| format!("Invalid relay payload: {error}"))
        })?;
    let code = data["code"]
        .as_str()
        .ok_or("Relay did not issue an invite code")?;
    let secret = data["secret"]
        .as_str()
        .ok_or("Relay did not issue a host credential")?;
    let link = data["url"]
        .as_str()
        .ok_or("Relay did not issue a link")?
        .to_owned();
    let url = format!(
        "{}/v1/sessions/{code}/host?secret={secret}",
        ws_origin(&relay)
    );
    let (mut admin, _) =
        connect(url).map_err(|error| format!("Cannot connect host to relay: {error}"))?;
    socket_timeout(&mut admin).map_err(|error| error.to_string())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    let wss = ws_origin(&relay);
    let code = code.to_owned();
    let secret = secret.to_owned();
    let server_address = session.bound_address;
    thread::Builder::new()
        .name("worldkiln-online-host".into())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match ws_read(&mut admin) {
                    Ok(Some(Message::Text(text))) => {
                        let Ok(event) = serde_json::from_str::<Json>(text.as_str()) else {
                            continue;
                        };
                        if event["kind"] != "guest_joined" {
                            continue;
                        }
                        let Some(id) = event["id"].as_str().filter(|id| {
                            id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())
                        }) else {
                            continue;
                        };
                        let url =
                            format!("{wss}/v1/sessions/{code}/peer?peer={id}&secret={secret}");
                        let stop_peer = stop.clone();
                        thread::spawn(move || {
                            if let Ok((ws, _)) = connect(url) {
                                if let Ok(tcp) = TcpStream::connect_timeout(
                                    &server_address,
                                    Duration::from_secs(3),
                                ) {
                                    tunnel(ws, tcp, stop_peer);
                                }
                            }
                        });
                    }
                    Ok(Some(Message::Close(_))) | Err(_) => break,
                    _ => {}
                }
            }
            let _ = admin.close(None);
        })
        .map_err(|error| error.to_string())?;
    Ok(PreparedInvite {
        session,
        role: NetworkTestRole::Host,
        link,
        bridge: InviteBridge { cancelled },
    })
}

pub fn join_invite(link: String) -> Result<PreparedInvite, String> {
    let relay = relay_origin()?;
    let code = code_from_link(&link, &relay)?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("Cannot prepare local connection: {error}"))?;
    let local_addr: SocketAddr = listener.local_addr().map_err(|error| error.to_string())?;
    let url = format!("{}/v1/sessions/{code}/guest", ws_origin(&relay));
    let (ws, _) = connect(url).map_err(|error| format!("Cannot reach online game: {error}"))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let stop = cancelled.clone();
    thread::Builder::new()
        .name("worldkiln-online-guest".into())
        .spawn(move || {
            if let Ok((tcp, _)) = listener.accept() {
                tunnel(ws, tcp, stop);
            }
        })
        .map_err(|error| error.to_string())?;
    let session = MultiplayerTestSession::start(NetworkTestRole::Join, local_addr)
        .map_err(|error| format!("Cannot join online game: {error}"))?;
    Ok(PreparedInvite {
        session,
        role: NetworkTestRole::Join,
        link: format!("{relay}/join/{code}"),
        bridge: InviteBridge { cancelled },
    })
}
