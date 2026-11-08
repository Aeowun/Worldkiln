// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

const UPDATES_URL: &str = "https://aeowun.com/api/worldkiln/updates";
const MAX_RESPONSE_BYTES: usize = 128 * 1024;
const MAX_NOTES: usize = 40;
const REFRESH_PERIOD: Duration = Duration::from_secs(180);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeveloperNote {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) category: String,
    #[serde(default)]
    pub(crate) link_url: Option<String>,
    #[serde(default)]
    pub(crate) pinned: bool,
    #[serde(default)]
    pub(crate) published_at: Option<String>,
}

#[derive(Deserialize)]
struct UpdatesResponse {
    updates: Vec<DeveloperNote>,
}

#[derive(Deserialize, Serialize)]
struct CachedNotes {
    updated_at: String,
    notes: Vec<DeveloperNote>,
}

struct FetchResult {
    notes: Vec<DeveloperNote>,
    updated_at: String,
}

pub(crate) struct DeveloperNotesFeed {
    pub(crate) notes: Vec<DeveloperNote>,
    pub(crate) last_updated: Option<String>,
    pub(crate) unavailable: bool,
    pending: Option<Receiver<Result<FetchResult, String>>>,
    next_refresh: Instant,
    consecutive_failures: u32,
    was_visible: bool,
}

impl Default for DeveloperNotesFeed {
    fn default() -> Self {
        let cache = load_cached_notes();
        Self {
            notes: cache.as_ref().map_or_else(Vec::new, |saved| saved.notes.clone()),
            last_updated: cache.map(|saved| saved.updated_at),
            unavailable: false,
            pending: None,
            next_refresh: Instant::now(),
            consecutive_failures: 0,
            was_visible: false,
        }
    }
}

impl DeveloperNotesFeed {
    pub(crate) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn update(&mut self, ctx: &egui::Context, visible: bool) {
        if !visible {
            self.was_visible = false;
            return;
        }
        let now = Instant::now();
        if !self.was_visible {
            self.was_visible = true;
            self.next_refresh = now;
        }
        if let Some(receiver) = self.pending.as_ref() {
            match receiver.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(fetched) => {
                            self.notes = fetched.notes;
                            self.last_updated = Some(fetched.updated_at);
                            self.unavailable = false;
                            self.consecutive_failures = 0;
                            self.next_refresh = now + REFRESH_PERIOD;
                        }
                        Err(_) => self.failed(now),
                    }
                    ctx.request_repaint();
                }
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.failed(now);
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none() && now >= self.next_refresh {
            let (sender, receiver) = mpsc::channel();
            match std::thread::Builder::new()
                .name("worldkiln-dev-notes".into())
                .spawn(move || {
                    let _ = sender.send(fetch_notes());
                })
            {
                Ok(_) => self.pending = Some(receiver),
                Err(_) => self.failed(now),
            }
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        } else {
            ctx.request_repaint_after(self.next_refresh.saturating_duration_since(now));
        }
    }

    fn failed(&mut self, now: Instant) {
        self.unavailable = true;
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        let exponent = self.consecutive_failures.saturating_sub(1).min(5);
        let seconds = (10u64 << exponent).min(MAX_BACKOFF.as_secs());
        self.next_refresh = now + Duration::from_secs(seconds);
    }
}

fn sanitize_text(value: &str, max_len: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(max_len)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn approved_link(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    url.host_str()?;
    Some(url.to_string())
}

fn sanitize_notes(notes: Vec<DeveloperNote>) -> Vec<DeveloperNote> {
    notes
        .into_iter()
        .take(MAX_NOTES)
        .filter_map(|mut note| {
            note.id = sanitize_text(&note.id, 128);
            note.title = sanitize_text(&note.title, 180);
            note.summary = sanitize_text(&note.summary, 3200);
            note.category = sanitize_text(&note.category, 80);
            note.published_at = note
                .published_at
                .as_deref()
                .map(|date| sanitize_text(date, 64));
            note.link_url = note.link_url.as_deref().and_then(approved_link);
            if note.id.is_empty() || note.title.is_empty() {
                None
            } else {
                Some(note)
            }
        })
        .collect()
}

fn parse_updates(bytes: &[u8]) -> Result<Vec<DeveloperNote>, String> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err("developer-notes response exceeds size limit".into());
    }
    let payload: UpdatesResponse =
        serde_json::from_slice(bytes).map_err(|_| "invalid developer-notes response")?;
    Ok(sanitize_notes(payload.updates))
}

fn cache_path() -> PathBuf {
    let root = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    root.join("Aeowun")
        .join("Worldkiln")
        .join("cache")
        .join("developer-notes.json")
}

fn load_cached_notes() -> Option<CachedNotes> {
    let path = cache_path();
    if fs::metadata(&path).ok()?.len() > MAX_RESPONSE_BYTES as u64 {
        return None;
    }
    let data = fs::read(path).ok()?;
    let mut saved: CachedNotes = serde_json::from_slice(&data).ok()?;
    saved.updated_at = sanitize_text(&saved.updated_at, 64);
    saved.notes = sanitize_notes(saved.notes);
    Some(saved)
}

fn save_cached_notes(notes: &[DeveloperNote], updated_at: &str) {
    let path = cache_path();
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let saved = CachedNotes {
        updated_at: updated_at.to_owned(),
        notes: notes.to_vec(),
    };
    if let Ok(data) = serde_json::to_vec(&saved) {
        if data.len() <= MAX_RESPONSE_BYTES {
            let _ = fs::write(path, data);
        }
    }
}

fn fetch_notes() -> Result<FetchResult, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Worldkiln-DeveloperNotes")
        .build()
        .map_err(|_| "developer-notes client unavailable")?;
    let response = client
        .get(UPDATES_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|_| "developer-notes service unavailable")?;
    if response.content_length().is_some_and(|size| size > MAX_RESPONSE_BYTES as u64) {
        return Err("developer-notes response exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "unable to read developer-notes response")?;
    let notes = parse_updates(&bytes)?;
    let updated_at = chrono::Utc::now().to_rfc3339();
    save_cached_notes(&notes, &updated_at);
    Ok(FetchResult { notes, updated_at })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_preserve_server_order_and_sanitize_untrusted_fields() {
        let body = br#"{"updates":[{"id":"first","title":"Pinned","summary":"A\u0007B","category":"Development","pinned":true,"linkUrl":"https://github.com/Aeowun/Worldkiln","publishedAt":"2026-11-03T00:00:00Z"},{"id":"second","title":"Other","summary":"Public","pinned":false,"linkUrl":"http://example.com"},{"id":"third","title":"Ignored","summary":"Private link","linkUrl":"https://www.indiedb.com/engines/worldkiln"}]}"#;
        let notes = parse_updates(body).unwrap();
        assert_eq!(notes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), vec!["first", "second", "third"]);
        assert!(notes[0].pinned);
        assert_eq!(notes[0].summary, "AB");
        assert!(notes[0].link_url.is_some());
        assert!(notes[1].link_url.is_none());
        assert!(notes[2].link_url.is_some());
    }

    #[test]
    fn an_empty_successful_response_removes_unpublished_notes() {
        assert!(parse_updates(br#"{"updates":[]}"#).unwrap().is_empty());
        assert!(parse_updates(br#"{"updates":[{"id":"x","title":"Old","summary":""}]}"#).unwrap().len() == 1);
    }

    #[test]
    fn invalid_or_oversized_responses_are_rejected() {
        assert!(parse_updates(br#"{"unexpected":[]}"#).is_err());
        assert!(parse_updates(&vec![b' '; MAX_RESPONSE_BYTES + 1]).is_err());
    }

    #[test]
    fn failed_fetch_retains_cached_notes_and_uses_backoff() {
        let now = Instant::now();
        let mut feed = DeveloperNotesFeed {
            notes: parse_updates(br#"{"updates":[{"id":"cached","title":"Cached","summary":"Offline"}]}"#).unwrap(),
            last_updated: None,
            unavailable: false,
            pending: None,
            next_refresh: now,
            consecutive_failures: 0,
            was_visible: false,
        };
        feed.failed(now);
        assert_eq!(feed.notes[0].title, "Cached");
        assert!(feed.unavailable);
        assert!(feed.next_refresh >= now + Duration::from_secs(10));
    }
}
