// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU8, Ordering},
    mpsc::{self, SyncSender},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    #[default]
    Full,
    Diagnostics,
    Off,
}

impl Tier {
    pub const ALL: [Self; 3] = [Self::Full, Self::Diagnostics, Self::Off];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Diagnostics => "Diagnostics Only",
            Self::Off => "Off",
        }
    }

    const fn as_byte(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Diagnostics => 1,
            Self::Full => 2,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum EventKind {
    EditorLaunched,
    PlayStarted,
    PlayStopped,
    BuildStarted,
    BuildFailed,
    InstallFailed,
    UpdateFailed,
    ScriptFailed,
}

impl EventKind {
    const fn name(self) -> &'static str {
        match self {
            Self::EditorLaunched => "editor_launched",
            Self::PlayStarted => "play_started",
            Self::PlayStopped => "play_stopped",
            Self::BuildStarted => "build_requested",
            Self::BuildFailed => "build_failed",
            Self::InstallFailed => "install_failed",
            Self::UpdateFailed => "update_failed",
            Self::ScriptFailed => "script_failed",
        }
    }

    const fn diagnostic(self) -> bool {
        matches!(
            self,
            Self::BuildFailed | Self::InstallFailed | Self::UpdateFailed | Self::ScriptFailed
        )
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
struct Consent {
    tier: Option<Tier>,
    installation_id: Option<String>,
}

impl Consent {
    fn allows(&self, event: EventKind) -> bool {
        match self.tier {
            Some(Tier::Full) => true,
            Some(Tier::Diagnostics) => event.diagnostic(),
            _ => false,
        }
    }
}

#[derive(Serialize)]
struct Event {
    name: &'static str,
    timestamp_ms: u64,
}

#[derive(Serialize)]
struct Payload {
    schema: u8,
    installation_id: String,
    engine_version: &'static str,
    events: Vec<Event>,
}

struct SenderState {
    tier: Arc<AtomicU8>,
    installation_id: Arc<Mutex<Option<String>>>,
    queue: Option<SyncSender<(EventKind, u64)>>,
}

struct State {
    consent: Consent,
    sender: SenderState,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn settings_path() -> std::path::PathBuf {
    crate::project::project::default_user_data_dir().join("insights_settings.json")
}

fn load_from(path: &std::path::Path) -> Consent {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Consent>(&bytes).ok())
        .unwrap_or_default()
}

fn save_to(path: &std::path::Path, consent: &Consent) -> std::io::Result<()> {
    let content = serde_json::to_vec_pretty(consent)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::project::atomic_file::write_atomically(path, &content)
}

fn fresh_id() -> String {
    let mut data = [0_u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut data);
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn endpoint() -> Option<String> {
    let address = std::env::var("WORLDKILN_INSIGHTS_ENDPOINT").ok()?;
    (address.starts_with("https://") && address.len() < 2048).then_some(address)
}

fn spawn_sender(shared: &SenderState, receiver: mpsc::Receiver<(EventKind, u64)>, url: String) {
    let level = Arc::clone(&shared.tier);
    let identifier = Arc::clone(&shared.installation_id);
    let _ = std::thread::Builder::new()
        .name("worldkiln-insights".into())
        .spawn(move || {
            let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(4))
                .build()
            else {
                return;
            };
            while let Ok(first) = receiver.recv() {
                let mut batch = vec![first];
                while batch.len() < 32 {
                    match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(event) => batch.push(event),
                        Err(_) => break,
                    }
                }
                let current = level.load(Ordering::Acquire);
                if current == 0 {
                    continue;
                }
                let Some(id) = identifier.lock().ok().and_then(|guard| guard.clone()) else {
                    continue;
                };
                let events: Vec<Event> = batch
                    .into_iter()
                    .filter(|(kind, _)| current == 2 || kind.diagnostic())
                    .map(|(kind, timestamp_ms)| Event {
                        name: kind.name(),
                        timestamp_ms,
                    })
                    .collect();
                if events.is_empty() || level.load(Ordering::Acquire) == 0 {
                    continue;
                }
                let payload = Payload {
                    schema: 1,
                    installation_id: id,
                    engine_version: env!("CARGO_PKG_VERSION"),
                    events,
                };
                let _ = client.post(&url).json(&payload).send();
            }
        });
}

impl State {
    fn new() -> Self {
        let mut consent = load_from(&settings_path());
        if consent.tier == Some(Tier::Off) {
            consent.installation_id = None;
        } else if matches!(consent.tier, Some(Tier::Full | Tier::Diagnostics))
            && consent.installation_id.is_none()
        {
            let mut updated = consent.clone();
            updated.installation_id = Some(fresh_id());
            if save_to(&settings_path(), &updated).is_ok() {
                consent = updated;
            } else {
                consent.tier = None;
            }
        }
        let (tx, rx) = mpsc::sync_channel(128);
        let sender = SenderState {
            tier: Arc::new(AtomicU8::new(consent.tier.unwrap_or(Tier::Off).as_byte())),
            installation_id: Arc::new(Mutex::new(consent.installation_id.clone())),
            queue: None,
        };
        let mut result = Self { consent, sender };
        if let Some(url) = endpoint() {
            spawn_sender(&result.sender, rx, url);
            result.sender.queue = Some(tx);
        }
        result
    }
}

fn global() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::new()))
}

pub fn initialize() {
    let _ = global();
}

pub fn selected_tier() -> Option<Tier> {
    global().lock().ok().and_then(|state| state.consent.tier)
}

pub fn support_id() -> Option<String> {
    global()
        .lock()
        .ok()
        .and_then(|state| state.consent.installation_id.clone())
}

pub fn set_tier(tier: Tier) -> std::io::Result<()> {
    let mut state = global()
        .lock()
        .map_err(|_| std::io::Error::other("Insights settings are unavailable"))?;
    let mut updated = state.consent.clone();
    updated.tier = Some(tier);
    if tier == Tier::Off {
        updated.installation_id = None;
    } else if updated.installation_id.is_none() {
        updated.installation_id = Some(fresh_id());
    }
    save_to(&settings_path(), &updated)?;
    state.sender.tier.store(0, Ordering::Release);
    if let Ok(mut id) = state.sender.installation_id.lock() {
        *id = updated.installation_id.clone();
    }
    state.consent = updated;
    state.sender.tier.store(tier.as_byte(), Ordering::Release);
    Ok(())
}

pub fn record(kind: EventKind) {
    let Ok(state) = global().lock() else {
        return;
    };
    if !state.consent.allows(kind) {
        return;
    }
    let Some(sender) = &state.sender.queue else {
        return;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let _ = sender.try_send((kind, now));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_does_not_transmit_until_choice_is_saved() {
        let consent = Consent::default();
        assert_eq!(consent.tier, None);
        assert!(consent.installation_id.is_none());
        assert!(!consent.allows(EventKind::EditorLaunched));
        assert!(!consent.allows(EventKind::BuildFailed));
    }

    #[test]
    fn diagnostics_excludes_usage_and_off_excludes_everything() {
        let diagnostics = Consent {
            tier: Some(Tier::Diagnostics),
            installation_id: Some("test".into()),
        };
        assert!(diagnostics.allows(EventKind::BuildFailed));
        assert!(!diagnostics.allows(EventKind::EditorLaunched));
        assert!(!diagnostics.allows(EventKind::PlayStarted));
        let off = Consent {
            tier: Some(Tier::Off),
            installation_id: None,
        };
        assert!(!off.allows(EventKind::BuildFailed));
    }

    #[test]
    fn consent_roundtrip_does_not_need_project_data() {
        let sample = Consent {
            tier: Some(Tier::Full),
            installation_id: Some("test123".into()),
        };
        let restored: Consent =
            serde_json::from_slice(&serde_json::to_vec(&sample).unwrap()).unwrap();
        assert_eq!(restored.tier, Some(Tier::Full));
        assert_eq!(restored.installation_id.as_deref(), Some("test123"));
        assert_eq!(fresh_id().len(), 32);
    }
}
