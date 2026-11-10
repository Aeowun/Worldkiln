// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
#[path = "cell_replication.rs"]
pub mod cell_replication;
#[path = "character_replication.rs"]
pub mod character_replication;
#[path = "host_transport.rs"]
pub mod host_transport;
#[path = "invite.rs"]
pub mod invite;
#[path = "ownership.rs"]
pub mod ownership;
#[path = "physics_authority.rs"]
pub mod physics_authority;
#[path = "play_session.rs"]
pub mod play_session;
#[path = "replication.rs"]
pub mod replication;
#[path = "session.rs"]
pub mod session;

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

const MAGIC: [u8; 4] = *b"WKN1";
const PROTOCOL_VERSION: u16 = 1;
const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const ACK: u8 = 3;
const CLOSE: u8 = 4;
const FRAME_LEN: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(3);

fn bad_frame(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn send(stream: &mut TcpStream, kind: u8, nonce: u64) -> io::Result<()> {
    let mut frame = [0u8; FRAME_LEN];
    frame[..4].copy_from_slice(&MAGIC);
    frame[4..6].copy_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    frame[6] = kind;
    frame[8..].copy_from_slice(&nonce.to_be_bytes());
    stream.write_all(&frame)
}

fn receive(stream: &mut TcpStream, expected_kind: u8) -> io::Result<u64> {
    let mut frame = [0u8; FRAME_LEN];
    stream.read_exact(&mut frame)?;
    if frame[..4] != MAGIC {
        return Err(bad_frame("invalid Worldkiln network handshake"));
    }
    if u16::from_be_bytes([frame[4], frame[5]]) != PROTOCOL_VERSION {
        return Err(bad_frame("incompatible Worldkiln network protocol"));
    }
    if frame[6] != expected_kind || frame[7] != 0 {
        return Err(bad_frame("unexpected Worldkiln network message"));
    }
    Ok(u64::from_be_bytes(frame[8..].try_into().unwrap()))
}

fn configure(stream: &TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))
}

pub struct SessionHost {
    listener: TcpListener,
}

impl SessionHost {
    pub fn bind(address: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn accept(&self, timeout: Duration) -> io::Result<SessionConnection> {
        let started = Instant::now();
        loop {
            match self.listener.accept() {
                Ok((mut stream, peer)) => {
                    if !peer.ip().is_loopback() {
                        let _ = stream.shutdown(Shutdown::Both);
                        return Err(io::Error::new(io::ErrorKind::PermissionDenied,
                            "Unauthenticated direct TCP peers are restricted to loopback; use an authenticated invite for remote multiplayer"));
                    }
                    stream.set_nonblocking(false)?;
                    configure(&stream)?;
                    let nonce = receive(&mut stream, HELLO)?;
                    if nonce == 0 {
                        return Err(bad_frame("invalid session handshake nonce"));
                    }
                    send(&mut stream, WELCOME, nonce)?;
                    if receive(&mut stream, ACK)? != nonce {
                        return Err(bad_frame("mismatched session handshake acknowledgement"));
                    }
                    return Ok(SessionConnection { stream, nonce });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    if started.elapsed() >= timeout {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "no peer connected before the session timeout",
                        ));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

pub struct SessionConnection {
    stream: TcpStream,
    nonce: u64,
}

impl SessionConnection {
    pub fn connect(address: SocketAddr, timeout: Duration) -> io::Result<Self> {
        if !address.ip().is_loopback() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied,
                "Unauthenticated direct TCP connections are restricted to loopback; use a session invite for remote multiplayer"));
        }
        if timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "connection timeout must be positive",
            ));
        }
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        configure(&stream)?;
        let nonce = rand::random::<u64>() | 1;
        send(&mut stream, HELLO, nonce)?;
        if receive(&mut stream, WELCOME)? != nonce {
            return Err(bad_frame("mismatched session handshake response"));
        }
        send(&mut stream, ACK, nonce)?;
        Ok(Self { stream, nonce })
    }

    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    pub fn close(mut self) -> io::Result<()> {
        let result = send(&mut self.stream, CLOSE, self.nonce);
        let _ = self.stream.shutdown(Shutdown::Both);
        result
    }

    pub fn wait_for_close(mut self) -> io::Result<()> {
        if receive(&mut self.stream, CLOSE)? != self.nonce {
            return Err(bad_frame("mismatched session close acknowledgement"));
        }
        self.stream.shutdown(Shutdown::Both)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn localhost() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    #[test]
    fn multiple_connections_disconnect_without_stale_sessions() {
        let host = SessionHost::bind(localhost()).unwrap();
        let address = host.local_addr().unwrap();
        let server = thread::spawn(move || {
            for _ in 0..5 {
                host.accept(Duration::from_secs(2))
                    .unwrap()
                    .wait_for_close()
                    .unwrap();
            }
        });
        for _ in 0..5 {
            SessionConnection::connect(address, Duration::from_secs(2))
                .unwrap()
                .close()
                .unwrap();
        }
        server.join().unwrap();
    }

    #[test]
    fn idle_server_returns_a_bounded_timeout() {
        let host = SessionHost::bind(localhost()).unwrap();
        let err = match host.accept(Duration::from_millis(35)) {
            Ok(_) => panic!("unexpected client"),
            Err(error) => error,
        };
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn rejects_incompatible_protocol() {
        let host = SessionHost::bind(localhost()).unwrap();
        let addr = host.local_addr().unwrap();
        let server = thread::spawn(move || match host.accept(Duration::from_secs(2)) {
            Ok(_) => panic!("invalid client accepted"),
            Err(error) => error.kind(),
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        let mut frame = [0u8; FRAME_LEN];
        frame[..4].copy_from_slice(&MAGIC);
        frame[4..6].copy_from_slice(&(PROTOCOL_VERSION + 1).to_be_bytes());
        frame[6] = HELLO;
        frame[8..].copy_from_slice(&1u64.to_be_bytes());
        stream.write_all(&frame).unwrap();
        assert_eq!(server.join().unwrap(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_incomplete_handshake() {
        let host = SessionHost::bind(localhost()).unwrap();
        let addr = host.local_addr().unwrap();
        let server = thread::spawn(move || match host.accept(Duration::from_secs(2)) {
            Ok(_) => panic!("invalid client accepted"),
            Err(error) => error.kind(),
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        stream.write_all(&MAGIC).unwrap();
        stream.shutdown(Shutdown::Write).unwrap();
        assert_eq!(server.join().unwrap(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn port_conflicts_are_reported() {
        let host = SessionHost::bind(localhost()).unwrap();
        let err = match SessionHost::bind(host.local_addr().unwrap()) {
            Ok(_) => panic!("duplicate listener was bound"),
            Err(error) => error,
        };
        assert_eq!(err.kind(), io::ErrorKind::AddrInUse);
    }
}
