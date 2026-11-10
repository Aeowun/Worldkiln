// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::net::SocketAddr;
use std::path::PathBuf;

use Worldkiln::network::play_session::NetworkTestRole;

fn main() {
    let mut args = std::env::args().skip(1);
    let role = match args.next().as_deref() {
        Some("play") => {
            let path = args.next().map(PathBuf::from).filter(|p| p.join("world.dat").is_file());
            if args.next().is_some() || path.is_none() {
                eprintln!("Expected play PROJECT_FOLDER");
                std::process::exit(2);
            }
            Worldkiln::game::platform::desktop::run_desktop(path.unwrap(), "Worldkiln".into(), None, None, true);
            return;
        }
        Some("host") => NetworkTestRole::Host,
        Some("join") => NetworkTestRole::Join,
        _ => {
            eprintln!("Usage: worldkiln-network-play.exe host|join IP:PORT PROJECT_FOLDER");
            std::process::exit(2);
        }
    };
    let address: SocketAddr = args.next()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            eprintln!("Expected an IP:PORT, e.g. 127.0.0.1:47001");
            std::process::exit(2);
        });
    let project: PathBuf = args.next()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            eprintln!("Expected the path to a Worldkiln project folder containing world.dat");
            std::process::exit(2);
        });
    if args.next().is_some() || !project.join("world.dat").is_file() {
        eprintln!("Expected an existing Worldkiln project folder containing world.dat");
        std::process::exit(2);
    }
    Worldkiln::game::platform::desktop::run_game_with_multiplayer(
        project,
        "Worldkiln".to_owned(),
        None,
        Some((role, address)),
    );
}
