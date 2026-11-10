// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use super::App;
use crate::editor::editor::multiplayer_ui::MultiplayerAction;

#[derive(Clone)]
pub(super) struct LaunchSettings {
    pub action: MultiplayerAction,
    pub players: usize,
    pub arrange: bool,
    pub logs: bool,
    pub address: String,
    pub port: u16,
    pub max_players: usize,
}

pub(super) struct PendingMultiplayerLaunch {
    pub receiver: mpsc::Receiver<LaunchMessage>,
    pub cancelled: Arc<AtomicBool>,
    pub source: PathBuf,
}

impl Drop for PendingMultiplayerLaunch {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

pub(super) enum LaunchMessage {
    Status(String),
    Finished(Result<MultiplayerSession, String>),
}

struct SnapshotContents {
    world: crate::world::World,
    hierarchy: crate::project::ProjectHierarchy,
    editor_state: crate::project::EditorProjectState,
    ui: crate::engine::ui_authoring::AuthoredUiWorkspace,
    scripts: Vec<(PathBuf, String)>,
}

pub(super) struct MultiplayerSession {
    pub processes: Vec<Child>,
    pub exited: BTreeSet<usize>,
    pub source: PathBuf,
    pub root: PathBuf,
    pub target: usize,
    pub ready: usize,
    pub host_port: u16,
    pub deadline: Instant,
    pub host_listening: bool,
    pub options: LaunchSettings,
    pub runtime: PathBuf,
}

impl Drop for MultiplayerSession {
    fn drop(&mut self) {
        for child in &mut self.processes {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        let count = if self.options.action == MultiplayerAction::Test {
            self.options.players.clamp(2, 8)
        } else {
            1
        };
        for index in 0..count {
            let _ = fs::remove_dir_all(self.root.join(format!("player-{}", index + 1)));
        }
    }
}

fn copy_project(source: &Path, destination: &Path, cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Multiplayer launch cancelled",
        ));
    }
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" || name == "target" {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Project contains a symbolic link; cannot isolate it safely",
            ));
        }
        let target = destination.join(&name);
        if kind.is_dir() {
            copy_project(&entry.path(), &target, cancelled)?;
        } else if kind.is_file() {
            if cancelled.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Multiplayer launch cancelled",
                ));
            }
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn runtime_binary() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("Cannot locate Editor runtime: {error}"))
}

fn launch(
    executable: &Path,
    kind: &str,
    address: &str,
    project: &Path,
    log_root: &Path,
    slot: usize,
    participants: usize,
    settings: &LaunchSettings,
) -> Result<Child, String> {
    let name = format!("player-{}", slot + 1);
    let mut command = Command::new(executable);
    command.env(
        "WORLDKILN_RUNTIME_SAVE_ROOT",
        project.join(".multiplayer-saves"),
    );
    command
        .arg("--worldkiln-multiplayer-instance")
        .arg(kind)
        .arg(address)
        .arg(project);
    if kind == "host" {
        command.env(
            "WORLDKILN_MULTIPLAYER_MAX_PLAYERS",
            settings.max_players.to_string(),
        );
    }
    if settings.arrange {
        command.env(
            "WORLDKILN_MULTIPLAYER_WINDOW",
            format!("{slot}/{participants}"),
        );
    }
    if settings.logs || kind == "host" {
        fs::create_dir_all(log_root).map_err(|error| error.to_string())?;
        let stdout = File::create(log_root.join(format!("{name}.stdout.log")))
            .map_err(|error| format!("Cannot create game log: {error}"))?;
        let stderr = File::create(log_root.join(format!("{name}.stderr.log")))
            .map_err(|error| format!("Cannot create game log: {error}"))?;
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
    } else {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    }
    command
        .spawn()
        .map_err(|error| format!("Cannot start {name}: {error}"))
}

fn prepare_multiplayer(
    source: PathBuf,
    snapshot: SnapshotContents,
    settings: LaunchSettings,
    cancelled: Arc<AtomicBool>,
    notify: &mpsc::Sender<LaunchMessage>,
) -> Result<MultiplayerSession, String> {
    let runtime = runtime_binary()?;
    let players = if settings.action == MultiplayerAction::Test {
        settings.players.clamp(2, 8)
    } else {
        1
    };
    let address = if settings.action == MultiplayerAction::Join {
        settings.address.clone()
    } else {
        format!("127.0.0.1:{}", settings.port)
    };
    let remote: SocketAddr = address
        .parse()
        .map_err(|_| "Enter a valid numeric IP address and port".to_owned())?;
    let root = std::env::temp_dir().join(format!(
        "worldkiln-multiplayer-{:032x}",
        rand::random::<u128>()
    ));
    fs::create_dir(&root).map_err(|error| format!("Cannot create isolated session: {error}"))?;
    let mut guard = LaunchGuard {
        root: root.clone(),
        processes: Vec::new(),
        committed: false,
    };
    let check_cancel = || -> Result<(), String> {
        if cancelled.load(Ordering::Relaxed) {
            Err("Multiplayer launch cancelled".into())
        } else {
            Ok(())
        }
    };
    let _ = notify.send(LaunchMessage::Status(
        "Copying project into isolated test folders".into(),
    ));
    let first = root.join("player-1");
    copy_project(&source, &first, &cancelled)
        .map_err(|error| format!("Cannot isolate project: {error}"))?;
    check_cancel()?;
    let _ = notify.send(LaunchMessage::Status(
        "Preparing unsaved world, UI and scripts".into(),
    ));
    crate::world::persistence::save_world(&snapshot.world, &first.join("world.dat"))
        .map_err(|error| format!("Cannot capture unsaved world: {error}"))?;
    crate::project::save_project_metadata(&first, &snapshot.hierarchy)
        .map_err(|error| format!("Cannot capture project structure: {error}"))?;
    snapshot
        .ui
        .save_to_project(&first)
        .map_err(|error| format!("Cannot capture unsaved UI: {error}"))?;
    crate::project::save_editor_state(&first, &snapshot.editor_state)
        .map_err(|error| format!("Cannot capture editor state: {error}"))?;
    for (relative, contents) in &snapshot.scripts {
        check_cancel()?;
        let path = first.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, contents).map_err(|error| {
            format!(
                "Cannot capture unsaved script {}: {error}",
                relative.display()
            )
        })?;
    }
    for index in 1..players {
        check_cancel()?;
        let _ = notify.send(LaunchMessage::Status(format!(
            "Preparing player {} of {}",
            index + 1,
            players
        )));
        let destination = root.join(format!("player-{}", index + 1));
        copy_project(&first, &destination, &cancelled)
            .map_err(|error| format!("Cannot prepare player {}: {error}", index + 1))?;
    }
    check_cancel()?;
    let kind = if settings.action == MultiplayerAction::Join {
        "join"
    } else {
        "host"
    };
    let host_address = if kind == "host" {
        format!("0.0.0.0:{}", remote.port())
    } else {
        address.clone()
    };
    let _ = notify.send(LaunchMessage::Status("Launching first game window".into()));
    guard.processes.push(launch(
        &runtime,
        kind,
        &host_address,
        &first,
        &root.join("logs"),
        0,
        players,
        &settings,
    )?);
    if players > 1 {
        let deadline = Instant::now() + Duration::from_secs(30);
        let log_file = root.join("logs/player-1.stderr.log");
        let _ = notify.send(LaunchMessage::Status(
            "Waiting for host to begin listening".into(),
        ));
        loop {
            check_cancel()?;
            if guard.processes[0]
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                return Err("Host exited before multiplayer clients could connect".into());
            }
            if fs::read_to_string(&log_file)
                .is_ok_and(|content| content.contains("Worldkiln multiplayer listening on"))
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Host did not begin listening before the connection timeout".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        for index in 1..players {
            check_cancel()?;
            let _ = notify.send(LaunchMessage::Status(format!(
                "Launching player {} of {}",
                index + 1,
                players
            )));
            let project = root.join(format!("player-{}", index + 1));
            guard.processes.push(launch(
                &runtime,
                "join",
                &format!("127.0.0.1:{}", remote.port()),
                &project,
                &root.join("logs"),
                index,
                players,
                &settings,
            )?);
        }
    }
    check_cancel()?;
    let processes = std::mem::take(&mut guard.processes);
    guard.committed = true;
    Ok(MultiplayerSession {
        processes,
        exited: BTreeSet::new(),
        source,
        root,
        target: players,
        ready: players,
        host_port: remote.port(),
        deadline: Instant::now(),
        host_listening: true,
        options: settings,
        runtime,
    })
}

struct LaunchGuard {
    root: PathBuf,
    processes: Vec<Child>,
    committed: bool,
}

impl Drop for LaunchGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for child in &mut self.processes {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stop_session(session: Option<MultiplayerSession>) {
    if let Some(session) = session {
        std::thread::spawn(move || drop(session));
    }
}

impl App {
    fn capture_multiplayer_snapshot(&mut self, source: &Path) -> Result<SnapshotContents, String> {
        self.editor.synchronize_project_cell_nodes(&self.world);
        self.editor.sync_active_ui_document();
        let mut scripts = Vec::new();
        for (path, document) in &self.editor.script_editor.open_documents {
            if !document.dirty {
                continue;
            }
            let relative = path.strip_prefix(source).map_err(|_| {
                format!(
                    "Unsaved script is outside current project: {}",
                    path.display()
                )
            })?;
            scripts.push((relative.to_path_buf(), document.source.clone()));
        }
        Ok(SnapshotContents {
            world: self.world.clone(),
            hierarchy: self.editor.project_hierarchy.clone(),
            editor_state: self.editor.project_state(source),
            ui: self.editor.ui_workspace.clone(),
            scripts,
        })
    }

    fn start_multiplayer(
        &mut self,
        mut settings: LaunchSettings,
        previous: Option<MultiplayerSession>,
    ) -> Result<(), String> {
        if settings.action == MultiplayerAction::Test {
            settings.max_players = settings.players.clamp(2, 8);
        }
        if self.multiplayer_session.is_some() || self.multiplayer_pending.is_some() {
            return Err("Stop the current multiplayer session before starting another".into());
        }
        if self.project_loading() || self.project_load_failed {
            return Err("Wait for the current project to finish loading before testing".into());
        }
        let source = self
            .project_manager
            .current_project
            .clone()
            .ok_or("Open a project before starting multiplayer")?;
        let address = if settings.action == MultiplayerAction::Join {
            settings.address.clone()
        } else {
            format!("127.0.0.1:{}", settings.port)
        };
        let _: SocketAddr = address
            .parse()
            .map_err(|_| "Enter a valid numeric IP address and port".to_string())?;
        let snapshot = self.capture_multiplayer_snapshot(&source)?;
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let snapshot_source = source.clone();
        std::thread::Builder::new()
            .name("worldkiln-multiplayer-launch".into())
            .spawn(move || {
                drop(previous);
                let result =
                    prepare_multiplayer(source, snapshot, settings, worker_cancel.clone(), &sender);
                if worker_cancel.load(Ordering::Relaxed) {
                    drop(result);
                } else {
                    let _ = sender.send(LaunchMessage::Finished(result));
                }
            })
            .map_err(|error| format!("Cannot start multiplayer background worker: {error}"))?;
        self.multiplayer_pending = Some(PendingMultiplayerLaunch {
            receiver,
            cancelled,
            source: snapshot_source,
        });
        self.editor.multiplayer_ui.status = "Preparing multiplayer session in background".into();
        self.editor.multiplayer_ui.running = true;
        self.editor.multiplayer_ui.logs_path = None;
        Ok(())
    }

    pub(super) fn update_multiplayer_ui(&mut self) {
        let mut completed = None;
        if let Some(pending) = &self.multiplayer_pending {
            if self.project_manager.current_project.as_ref() != Some(&pending.source) {
                pending.cancelled.store(true, Ordering::Relaxed);
            }
            loop {
                match pending.receiver.try_recv() {
                    Ok(LaunchMessage::Status(status)) => self.editor.multiplayer_ui.status = status,
                    Ok(LaunchMessage::Finished(result)) => {
                        completed = Some(result);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        completed = Some(Err("Multiplayer launch stopped unexpectedly".into()));
                        break;
                    }
                }
            }
        }
        if let Some(result) = completed {
            let pending = self.multiplayer_pending.take();
            let cancelled = pending
                .as_ref()
                .is_some_and(|job| job.cancelled.load(Ordering::Relaxed));
            match result {
                Ok(session) if !cancelled => {
                    self.editor.multiplayer_ui.logs_path =
                        session.options.logs.then(|| session.root.join("logs"));
                    self.editor.multiplayer_ui.status =
                        format!("Running ? {} players launched", session.ready);
                    self.multiplayer_session = Some(session);
                }
                Ok(session) => stop_session(Some(session)),
                Err(error) => self.editor.multiplayer_ui.status = error,
            }
        }
        if let Some(session) = &mut self.multiplayer_session {
            let mut failed = None;
            for (i, process) in session.processes.iter_mut().enumerate() {
                if session.exited.contains(&i) {
                    continue;
                }
                match process.try_wait() {
                    Ok(Some(exit)) => {
                        session.exited.insert(i);
                        failed = Some(format!("Player {} exited: {}", i + 1, exit));
                        if i == 0 {
                            session.target = session.ready;
                        }
                    }
                    Err(error) => {
                        failed = Some(format!("Cannot inspect player {}: {error}", i + 1));
                        break;
                    }
                    _ => {}
                }
            }
            if let Some(reason) = failed {
                self.editor.multiplayer_ui.status = reason;
            }
            if session.exited.len() == session.processes.len()
                || self.project_manager.current_project.as_ref() != Some(&session.source)
            {
                stop_session(self.multiplayer_session.take());
            }
        }
        let ui = &mut self.editor.multiplayer_ui;
        ui.running = self.multiplayer_session.is_some() || self.multiplayer_pending.is_some();
        ui.active_players = self
            .multiplayer_session
            .as_ref()
            .map_or(0, |session| session.processes.len() - session.exited.len());
        if let Some(session) = self.multiplayer_session.as_ref() {
            if ui.status.is_empty() || ui.status.starts_with("Running") {
                let address = if session.options.action == MultiplayerAction::Join {
                    session.options.address.clone()
                } else {
                    format!("127.0.0.1:{}", session.host_port)
                };
                ui.status = format!(
                    "Running — launched {} / {} | {}",
                    session.ready, session.target, address
                );
            }
            ui.logs_path = session.options.logs.then(|| session.root.join("logs"));
        }
        let Some(action) = ui.action.take() else {
            return;
        };
        match action {
            MultiplayerAction::ViewLogs => {
                if let Some(path) = &ui.logs_path {
                    let opener = if cfg!(windows) {
                        "explorer.exe"
                    } else {
                        "xdg-open"
                    };
                    let _ = Command::new(opener).arg(path).spawn();
                }
            }
            MultiplayerAction::Stop => {
                if let Some(pending) = self.multiplayer_pending.take() {
                    pending.cancelled.store(true, Ordering::Relaxed);
                }
                stop_session(self.multiplayer_session.take());
                self.editor.multiplayer_ui.running = false;
                self.editor.multiplayer_ui.status = "Stopped".into();
            }
            MultiplayerAction::Restart => {
                let settings = self
                    .multiplayer_session
                    .as_ref()
                    .map(|session| session.options.clone());
                if let Some(pending) = self.multiplayer_pending.take() {
                    pending.cancelled.store(true, Ordering::Relaxed);
                }
                let Some(settings) = settings else {
                    self.editor.multiplayer_ui.status =
                        "Stop the preparing session before restarting".into();
                    return;
                };
                let previous = self.multiplayer_session.take();
                if let Err(error) = self.start_multiplayer(settings, previous) {
                    self.editor.multiplayer_ui.status = error;
                }
            }
            other => {
                let ui = &self.editor.multiplayer_ui;
                let settings = LaunchSettings {
                    action: other,
                    players: ui.players,
                    arrange: ui.arrange_windows,
                    logs: ui.capture_logs,
                    port: ui.port,
                    max_players: ui.player_limit,
                    address: ui.join_address.clone(),
                };
                if let Err(error) = self.start_multiplayer(settings, None) {
                    self.editor.multiplayer_ui.status = error;
                }
            }
        }
    }
}
