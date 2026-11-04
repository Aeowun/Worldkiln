// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

use super::{AuthError, HttpDeviceTransport, LinkStatus, MemberDeviceLink, MemberIdentity, DevicePrompt, PollProgress};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use super::publish::PublishRequest;
use std::thread;
use std::time::{Duration, Instant};

enum Command {
    Begin(u64),
    Cancel(u64),
    SignOut(u64),
    SwitchAccount(u64),
    Publish(u64, PublishRequest),
    Shutdown,
}

enum EventKind {
    AwaitingApproval(DevicePrompt),
    SignedIn(MemberIdentity),
    SignedOut,
    Denied,
    Expired,
    Failed(AuthError),
    PublishProgress(u8, String),
    Published(String),
    PublishFailed(String),
}

struct Event {
    revision: u64,
    kind: EventKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MemberDisplay {
    SignedOut,
    Starting,
    AwaitingApproval(DevicePrompt),
    SignedIn(MemberIdentity),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PublishStatus {
    Idle,
    Uploading { percent: u8, phase: String },
    Submitted(String),
    Failed(String),
}

pub(crate) struct MemberClient {
    pub(crate) dialog_open: bool,
    pub(crate) display: MemberDisplay,
    pub(crate) error: Option<&'static str>,
    pub(crate) publishing: PublishStatus,
    publish_cancel: Arc<AtomicBool>,
    revision: u64,
    commands: Option<Sender<Command>>,
    updates: Option<Receiver<Event>>,
}

impl Default for MemberClient {
    fn default() -> Self {
        Self {
            dialog_open: false,
            display: MemberDisplay::SignedOut,
            error: None,
            publishing: PublishStatus::Idle,
            publish_cancel: Arc::new(AtomicBool::new(false)),
            revision: 0,
            commands: None,
            updates: None,
        }
    }
}

impl MemberClient {
    fn ensure_worker(&mut self) -> Result<(), ()> {
        if self.commands.is_some() {
            return Ok(());
        }
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let upload_cancel = Arc::clone(&self.publish_cancel);
        thread::Builder::new()
            .name("worldkiln-member-link".into())
            .spawn(move || run_worker(command_receiver, event_sender, upload_cancel))
            .map_err(|_| ())?;
        self.commands = Some(command_sender);
        self.updates = Some(event_receiver);
        Ok(())
    }

    pub(crate) fn sign_in(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.error = None;
        self.display = MemberDisplay::Starting;
        if self.ensure_worker().is_err()
            || self.commands.as_ref().is_none_or(|sender| sender.send(Command::Begin(self.revision)).is_err())
        {
            self.display = MemberDisplay::SignedOut;
            self.error = Some("Member sign-in is temporarily unavailable.");
            self.commands = None;
            self.updates = None;
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.error = None;
        self.display = MemberDisplay::SignedOut;
        if let Some(sender) = &self.commands {
            let _ = sender.send(Command::Cancel(self.revision));
        }
    }

    pub(crate) fn sign_out(&mut self) {
        self.publish_cancel.store(true, Ordering::Relaxed);
        self.publishing = PublishStatus::Idle;
        self.revision = self.revision.wrapping_add(1);
        self.error = None;
        self.display = MemberDisplay::SignedOut;
        if let Some(sender) = &self.commands {
            if sender.send(Command::SignOut(self.revision)).is_err() {
                self.error = Some("Member sign-out could not be confirmed.");
            }
        }
    }

    pub(crate) fn switch_account(&mut self) {
        self.publish_cancel.store(true, Ordering::Relaxed);
        self.publishing = PublishStatus::Idle;
        self.revision = self.revision.wrapping_add(1);
        self.error = None;
        self.display = MemberDisplay::Starting;
        if let Some(sender) = &self.commands {
            if sender.send(Command::SwitchAccount(self.revision)).is_err() {
                self.display = MemberDisplay::SignedOut;
                self.error = Some("Member sign-out could not be confirmed.");
            }
        }
    }

    pub(crate) fn publish(&mut self, archive: std::path::PathBuf, title: String, version: String, description: String) {
        self.queue_publish(PublishRequest {
            archive,
            built_folder: None,
            title,
            version,
            description,
        });
    }

    pub(crate) fn publish_built_game(
        &mut self,
        folder: std::path::PathBuf,
        title: String,
        version: String,
        description: String,
    ) {
        self.queue_publish(PublishRequest {
            archive: std::path::PathBuf::new(),
            built_folder: Some(folder),
            title,
            version,
            description,
        });
    }

    fn queue_publish(&mut self, job: PublishRequest) {
        if !matches!(self.display, MemberDisplay::SignedIn(_)) {
            self.publishing = PublishStatus::Failed("Sign in to AEOWUN first.".into());
            return;
        }
        if matches!(self.publishing, PublishStatus::Uploading { .. }) {
            return;
        }
        self.publish_cancel.store(false, Ordering::Relaxed);
        if self.commands.as_ref().is_some_and(|tx| tx.send(Command::Publish(self.revision, job)).is_ok()) {
            self.publishing = PublishStatus::Uploading { percent: 0, phase: "Preparing submission".into() };
        } else {
            self.publishing = PublishStatus::Failed("Publishing service is unavailable.".into());
        }
    }

    pub(crate) fn cancel_publish(&mut self) {
        self.publish_cancel.store(true, Ordering::Relaxed);
        // Keep the job active until the worker acknowledges cancellation.
        // A retry must not clear the shared cancellation flag while it is running.
        if let PublishStatus::Uploading { phase, .. } = &mut self.publishing {
            *phase = "Upload cancellation requested.".into();
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.updates {
            while let Ok(event) = receiver.try_recv() {
                if event.revision != self.revision {
                    continue;
                }
                self.error = None;
                match event.kind {
                    EventKind::AwaitingApproval(prompt) => {
                        let url = prompt.verification_uri_complete.clone()
                            .unwrap_or_else(|| prompt.verification_uri.clone());
                        self.display = MemberDisplay::AwaitingApproval(prompt);
                        if self.dialog_open {
                            ctx.open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    EventKind::SignedIn(member) => self.display = MemberDisplay::SignedIn(member),
                    EventKind::SignedOut => self.display = MemberDisplay::SignedOut,
                    EventKind::Denied => {
                        self.display = MemberDisplay::SignedOut;
                        self.error = Some("Authorization declined.");
                    }
                    EventKind::Expired => {
                        self.display = MemberDisplay::SignedOut;
                        self.error = Some("Approval expired. Try again.");
                    }
                    EventKind::PublishProgress(percent, phase) => {
                        self.publishing = PublishStatus::Uploading { percent, phase };
                    }
                    EventKind::Published(id) => self.publishing = PublishStatus::Submitted(id),
                    EventKind::PublishFailed(message) => self.publishing = PublishStatus::Failed(message),
                    EventKind::Failed(reason) => {
                        self.display = MemberDisplay::SignedOut;
                        self.error = Some(match reason {
                            AuthError::RevocationFailed => "Member sign-out could not be confirmed.",
                            AuthError::IdentityUnverified => "Member account could not be verified.",
                            _ => "Member sign-in is temporarily unavailable.",
                        });
                    }
                }
                ctx.request_repaint();
            }
        }

        if matches!(self.display, MemberDisplay::Starting | MemberDisplay::AwaitingApproval(_))
            || matches!(self.publishing, PublishStatus::Uploading { .. }) {
            ctx.request_repaint_after(Duration::from_millis(300));
        }
    }
}

impl Drop for MemberClient {
    fn drop(&mut self) {
        self.publish_cancel.store(true, Ordering::Relaxed);
        if let Some(sender) = &self.commands {
            let _ = sender.send(Command::Shutdown);
        }
    }
}

fn send_event(sender: &Sender<Event>, revision: u64, kind: EventKind) -> bool {
    sender.send(Event { revision, kind }).is_ok()
}

fn run_worker(commands: Receiver<Command>, updates: Sender<Event>, upload_cancel: Arc<AtomicBool>) {
    let mut link = MemberDeviceLink::default();
    let mut revision: u64 = 0;
    let mut transport: Option<HttpDeviceTransport> = None;

    loop {
        match commands.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => {
                let switching = matches!(&command, Command::SwitchAccount(_));
                match command {
                    Command::Shutdown => break,
                    Command::Publish(next_revision, request) => {
                        revision = next_revision;
                        if let Some(token) = link.access_token(Instant::now()) {
                            let result = super::publish::publish_zip(token, request, &upload_cancel, |percent, phase| {
                                let _ = send_event(&updates, revision, EventKind::PublishProgress(percent, phase.into()));
                            });
                            let kind = match result {
                                Ok(id) => EventKind::Published(id),
                                Err(message) => EventKind::PublishFailed(message),
                            };
                            if !send_event(&updates, revision, kind) { break; }
                        } else if !send_event(&updates, revision, EventKind::PublishFailed("Sign in to AEOWUN first.".into())) {
                            break;
                        }
                    }
                    Command::Cancel(next_revision) => {
                        revision = next_revision;
                        if matches!(link.status(Instant::now()), LinkStatus::SignedIn(_)) {
                            let _ = transport.as_ref().map(|api| link.sign_out(api));
                        } else {
                            link.cancel();
                        }
                    }
                    Command::SignOut(next_revision) => {
                        revision = next_revision;
                        let result = transport.as_ref().map_or(Ok(()), |api| link.sign_out(api));
                        if let Err(error) = result {
                            if !send_event(&updates, revision, EventKind::Failed(error)) {
                                break;
                            }
                        } else if !send_event(&updates, revision, EventKind::SignedOut) {
                            break;
                        }
                    }
                    Command::Begin(next_revision) | Command::SwitchAccount(next_revision) => {
                        revision = next_revision;
                        if switching {
                            let result = transport.as_ref().map_or(Ok(()), |api| link.sign_out(api));
                            if let Err(error) = result {
                                if !send_event(&updates, revision, EventKind::Failed(error)) {
                                    break;
                                }
                                continue;
                            }
                        }
                        if transport.is_none() {
                            match HttpDeviceTransport::new() {
                                Ok(api) => transport = Some(api),
                                Err(error) => {
                                    if !send_event(&updates, revision, EventKind::Failed(error)) {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }
                        if let Some(api) = &transport {
                            match link.begin(api, Instant::now()) {
                                Ok(prompt) => {
                                    if !send_event(&updates, revision, EventKind::AwaitingApproval(prompt)) {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    if !send_event(&updates, revision, EventKind::Failed(error)) {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        let now = Instant::now();
        if let Some(api) = &transport {
            match link.status(now) {
                LinkStatus::AwaitingApproval(_) => {
                    if let Ok(command) = commands.try_recv() {
                        match command {
                            Command::Cancel(next_revision) => {
                                revision = next_revision;
                                link.cancel();
                                continue;
                            }
                            other => {
                                match other {
                                    Command::Shutdown => break,
                                    Command::SignOut(next_revision) => {
                                        revision = next_revision;
                                        let _ = link.sign_out(api);
                                        let _ = send_event(&updates, revision, EventKind::SignedOut);
                                        continue;
                                    }
                                    Command::SwitchAccount(next_revision) => {
                                        revision = next_revision;
                                        link.cancel();
                                        match link.begin(api, Instant::now()) {
                                            Ok(prompt) => { let _ = send_event(&updates, revision, EventKind::AwaitingApproval(prompt)); }
                                            Err(error) => { let _ = send_event(&updates, revision, EventKind::Failed(error)); }
                                        }
                                        continue;
                                    }
                                    Command::Begin(next_revision) => {
                                        revision = next_revision;
                                        let _ = send_event(&updates, revision, EventKind::Failed(AuthError::AlreadyInProgress));
                                        continue;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    match link.poll(api, now) {
                        Ok(PollProgress::Linked) => {
                            if let LinkStatus::SignedIn(member) = link.status(Instant::now()) {
                                if !send_event(&updates, revision, EventKind::SignedIn(member)) {
                                    break;
                                }
                            }
                        }
                        Ok(PollProgress::Denied) => {
                            if !send_event(&updates, revision, EventKind::Denied) {
                                break;
                            }
                        }
                        Ok(PollProgress::Expired) => {
                            if !send_event(&updates, revision, EventKind::Expired) {
                                break;
                            }
                        }
                        Err(error) => {
                            link.cancel();
                            if !send_event(&updates, revision, EventKind::Failed(error)) {
                                break;
                            }
                        }
                        Ok(_) => {}
                    }
                }
                LinkStatus::SignedOut => {
                    if matches!(link.state, super::LinkState::Linked(_)) {
                        let _ = link.sign_out(api);
                        if !send_event(&updates, revision, EventKind::Expired) {
                            break;
                        }
                    } else if matches!(link.state, super::LinkState::Pending(_)) {
                        link.cancel();
                        if !send_event(&updates, revision, EventKind::Expired) {
                            break;
                        }
                    }
                }
                LinkStatus::SignedIn(_) => {}
            }
        }
    }

    if let Some(api) = &transport {
        let _ = link.sign_out(api);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_does_not_start_any_authentication() {
        let member = MemberClient::default();
        assert_eq!(member.display, MemberDisplay::SignedOut);
        assert!(member.commands.is_none());
        assert!(member.updates.is_none());
    }

    #[test]
    fn cancelled_upload_cannot_be_restarted_before_worker_finishes() {
        let mut member = MemberClient::default();
        member.display = MemberDisplay::SignedIn(MemberIdentity {
            id: "member".into(), handle: "member".into(), display_name: "Member".into(),
        });
        member.publishing = PublishStatus::Uploading { percent: 10, phase: "Uploading".into() };
        member.cancel_publish();
        member.publish(std::path::PathBuf::from("unused.zip"), "Example".into(), "1".into(), "Example".into());
        assert!(member.publish_cancel.load(Ordering::Relaxed));
        assert!(matches!(member.publishing, PublishStatus::Uploading { .. }));
    }

    #[test]
    fn cancel_ignores_outdated_approval_events() {
        let mut member = MemberClient::default();
        let (tx, rx) = mpsc::channel();
        member.updates = Some(rx);
        member.revision = 4;
        member.display = MemberDisplay::Starting;
        tx.send(Event {
            revision: 3,
            kind: EventKind::AwaitingApproval(DevicePrompt {
                user_code: "ABCD".into(),
                verification_uri: "https://aeowun.com/members/device/".into(),
                verification_uri_complete: None,
            }),
        }).unwrap();
        member.poll(&egui::Context::default());
        assert_eq!(member.display, MemberDisplay::Starting);
    }
}
