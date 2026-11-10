// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::io::{self, Write};
use std::net::SocketAddr;
use std::process::ExitCode;
use std::time::Duration;
use Worldkiln::network::{SessionConnection, SessionHost};

fn run() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "expected host or join")
    })?;
    let addr: SocketAddr = args
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected IP:port"))?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid IP:port"))?;
    let count: usize = args
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected session count"))?
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid session count"))?;
    if args.next().is_some() || !(1..=100).contains(&count) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected exactly three arguments and a session count from 1 to 100",
        ));
    }

    match mode.as_str() {
        "host" => {
            let host = SessionHost::bind(addr)?;
            println!("LISTENING {}", host.local_addr()?);
            io::stdout().flush()?;
            for round in 1..=count {
                let connection = host.accept(Duration::from_secs(8))?;
                println!("CONNECTED {round} {}", connection.peer_addr()?);
                connection.wait_for_close()?;
                println!("DISCONNECTED {round}");
                io::stdout().flush()?;
            }
            println!("HOST_DONE {count}");
        }
        "join" => {
            for round in 1..=count {
                let connection = SessionConnection::connect(addr, Duration::from_secs(3))?;
                println!("JOINED {round} {}", connection.peer_addr()?);
                connection.close()?;
                println!("LEFT {round}");
                io::stdout().flush()?;
            }
            println!("CLIENT_DONE {count}");
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected host or join",
            ));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("NETWORK_TEST_ERROR: {error}");
            ExitCode::FAILURE
        }
    }
}
