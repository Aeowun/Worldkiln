// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_EVENTS: usize = 50_000;
const MAX_DEEP_SAMPLES: usize = 10_000;
const MAX_SLOW_EPISODES: usize = 64;
const MAX_FRAME_CONTEXT_EVENTS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CaptureConfig {
    pub detailed: bool,
    pub pre_trigger_frames: usize,
    pub post_trigger_frames: u8,
    pub threshold_ms: u32,
    pub cpu: bool,
    pub gpu: bool,
    pub scripts: bool,
    pub resources: bool,
    pub script_spikes: bool,
    pub navigation_trace: bool,
    pub script_spike_threshold_ms: u32,
}
impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            detailed: false,
            pre_trigger_frames: 2,
            post_trigger_frames: 2,
            threshold_ms: 33,
            cpu: true,
            gpu: true,
            scripts: true,
            resources: true,
            script_spikes: true,
            navigation_trace: true,
            script_spike_threshold_ms: 20,
        }
    }
}
#[derive(Clone, Debug)]
pub struct DiagnosticNode {
    pub label: String,
    pub calls: u64,
    pub total: Duration,
    pub max: Duration,
}

#[derive(Clone, Debug)]
pub struct DiagnosticEvent {
    pub at: Duration,
    pub label: String,
    pub duration: Duration,
}

#[derive(Clone, Debug, Default)]
pub struct ResourceSnapshot {
    pub physics_entities: usize,
    pub active_characters: usize,
    pub dynamic_bodies: usize,
    pub active_dynamic_bodies: usize,
    pub sleeping_dynamic_bodies: usize,
    pub broadphase_cells: usize,
    pub broadphase_references: usize,
    pub anchored_colliders: usize,
    pub static_collider_chunks: usize,
    pub noncolliding_physics_entities: usize,
    pub character_packages: Vec<(String, usize)>,
}
#[derive(Clone, Debug)]
struct FrameContext {
    index: u64,
    events: Vec<TimedEvent>,
}
#[derive(Clone, Debug)]
struct SlowEpisode {
    trigger_frame: u64,
    at: Duration,
    duration: Duration,
    frames: Vec<FrameContext>,
    post_remaining: u8,
    resources: Option<ResourceSnapshot>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureState {
    Ready,
    Running,
    Paused,
    Stopped,
}

#[derive(Clone, Debug)]
pub struct CaptureSummary {
    pub state: CaptureState,
    pub active_duration: Duration,
    pub paused_duration: Duration,
    pub event_count: usize,
    pub dropped_events: u64,
    pub slow_frame_triggers: u64,
    pub slow_episodes_retained: usize,
    pub slow_episodes_evicted: u64,
    pub recording_overhead_us: u128,
    pub script_spike_samples: usize,
    pub navigation_call_samples: usize,
    pub dropped_deep_samples: u64,
    pub longest_script_slice: Option<(String, Duration)>,
    pub longest_navigation_call: Option<(String, String, Duration)>,
    pub script_nodes: Vec<DiagnosticNode>,
    pub navigation_nodes: Vec<DiagnosticNode>,
    pub engine_nodes: Vec<DiagnosticNode>,
    pub recent_diagnostics: Vec<DiagnosticEvent>,
    pub average_fps: Option<f64>,
    pub fps_1pct_low: Option<f64>,
    pub p95_frame_time: Option<Duration>,
    pub p99_frame_time: Option<Duration>,
}

#[derive(Clone, Debug)]
pub struct TimedEvent {
    pub at: Duration,
    pub category: &'static str,
    pub operation: &'static str,
    pub duration: Duration,
}

#[derive(Clone, Debug)]
pub struct OperationTiming {
    pub category: &'static str,
    pub operation: &'static str,
    pub samples: usize,
    pub inclusive_total_ns: u128,
    pub average_ns: f64,
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub max: Duration,
}

#[derive(Clone)]
pub struct TestRunRecorder {
    state: CaptureState,
    display_frozen: Option<Box<CaptureSummary>>,
    display_capture: Option<Arc<super::development_capture::Capture>>,
    capture_samples: Arc<VecDeque<super::development_capture::Sample>>,
    capture_dropped: u64,
    pinned_samples: Arc<BTreeMap<u64, super::development_capture::Sample>>,
    pinned_dropped: u64,
    next_sample: u64,
    pub conditions: super::development_capture::Conditions,
    active_since: Option<Instant>,
    paused_since: Option<Instant>,
    last_frame_start: Option<Instant>,
    active: Duration,
    paused: Duration,
    started_at: Option<SystemTime>,
    stopped_at: Option<SystemTime>,
    events: Arc<VecDeque<TimedEvent>>,
    script_timings: BTreeMap<String, ScriptTiming>,
    script_spikes: Vec<DeepSample>,
    navigation_calls: Vec<DeepSample>,
    dropped_deep_samples: u64,
    config: CaptureConfig,
    completed_frames: u64,
    frame_context: Vec<TimedEvent>,
    recent_frames: VecDeque<FrameContext>,
    pending_slow: Option<SlowEpisode>,
    slow_episodes: VecDeque<SlowEpisode>,
    slow_frame_triggers: u64,
    slow_episodes_evicted: u64,
    frame_context_dropped: u64,
    dropped: u64,
    recording_overhead_ns: u128,
    unsaved: bool,
    last_saved_directory: Option<PathBuf>,
    generation: u64,
}

#[derive(Clone, Default)]
struct ScriptTiming {
    calls: u64,
    total: Duration,
    max: Duration,
}

#[derive(Clone, Debug)]
struct DeepSample {
    at: Duration,
    script: String,
    operation: &'static str,
    duration: Duration,
}

#[derive(Default)]
struct NavigationTrace {
    enabled: bool,
    script: String,
    samples: Vec<NavigationTimedSample>,
}

pub(crate) struct NavigationTimedSample {
    pub script: String,
    pub operation: &'static str,
    pub elapsed: Duration,
    pub ended: Instant,
}

thread_local! {
    static NAVIGATION_TRACE: RefCell<NavigationTrace> = RefCell::new(NavigationTrace::default());
}

pub(crate) fn start_navigation_trace(enabled: bool) {
    NAVIGATION_TRACE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.enabled = enabled;
        state.script.clear();
        state.samples.clear();
    });
}

pub(crate) fn trace_script_context(path: Option<&str>) {
    NAVIGATION_TRACE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.enabled {
            state.script = path.unwrap_or("<runtime>").to_owned();
        }
    });
}

pub(crate) fn timed_navigation<T>(operation: &'static str, action: impl FnOnce() -> T) -> T {
    let enabled = NAVIGATION_TRACE.with(|cell| cell.borrow().enabled);
    if !enabled {
        return action();
    }
    let _span = super::development_capture::span("navigation", operation, false);
    let started = Instant::now();
    let result = action();
    let ended = Instant::now();
    NAVIGATION_TRACE.with(|cell| {
        let mut state = cell.borrow_mut();
        if state.samples.len() < MAX_DEEP_SAMPLES {
            let script = state.script.clone();
            state.samples.push(NavigationTimedSample {
                script,
                operation,
                elapsed: ended.duration_since(started),
                ended,
            });
        }
    });
    result
}

pub(crate) fn finish_navigation_trace() -> Vec<NavigationTimedSample> {
    NAVIGATION_TRACE.with(|cell| {
        let mut state = cell.borrow_mut();
        state.enabled = false;
        state.script.clear();
        std::mem::take(&mut state.samples)
    })
}

pub struct CaptureSaveTask {
    generation: u64,
    worker: std::thread::JoinHandle<io::Result<PathBuf>>,
}

impl CaptureSaveTask {
    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub fn finish(self, recorder: &mut TestRunRecorder) -> io::Result<PathBuf> {
        let result = self
            .worker
            .join()
            .map_err(|_| io::Error::other("development report writer panicked"))?;
        if result.is_ok()
            && recorder.generation == self.generation
            && recorder.state == CaptureState::Stopped
        {
            recorder.unsaved = false;
            recorder.last_saved_directory = result.as_ref().ok().cloned();
        }
        result
    }
}

impl Default for TestRunRecorder {
    fn default() -> Self {
        Self {
            state: CaptureState::Ready,
            display_frozen: None,
            display_capture: None,
            capture_samples: Arc::new(VecDeque::new()),
            capture_dropped: 0,
            pinned_samples: Arc::new(BTreeMap::new()),
            pinned_dropped: 0,
            next_sample: 0,
            conditions: Default::default(),
            active_since: None,
            paused_since: None,
            last_frame_start: None,
            active: Duration::ZERO,
            paused: Duration::ZERO,
            started_at: None,
            stopped_at: None,
            events: Arc::new(VecDeque::new()),
            script_timings: BTreeMap::new(),
            script_spikes: Vec::new(),
            navigation_calls: Vec::new(),
            dropped_deep_samples: 0,
            config: CaptureConfig::default(),
            completed_frames: 0,
            frame_context: Vec::new(),
            recent_frames: VecDeque::new(),
            pending_slow: None,
            slow_episodes: VecDeque::new(),
            slow_frame_triggers: 0,
            slow_episodes_evicted: 0,
            frame_context_dropped: 0,
            dropped: 0,
            recording_overhead_ns: 0,
            unsaved: false,
            last_saved_directory: None,
            generation: 0,
        }
    }
}

impl TestRunRecorder {
    pub fn state(&self) -> CaptureState {
        self.state
    }
    pub fn config(&self) -> CaptureConfig {
        self.config
    }
    pub fn set_config(&mut self, config: CaptureConfig) -> bool {
        if matches!(self.state, CaptureState::Running | CaptureState::Paused) {
            return false;
        }
        self.config = CaptureConfig {
            pre_trigger_frames: config.pre_trigger_frames.min(120),
            post_trigger_frames: config.post_trigger_frames.min(120),
            threshold_ms: config.threshold_ms.clamp(1, 1000),
            script_spike_threshold_ms: config.script_spike_threshold_ms.clamp(1, 1000),
            ..config
        };
        true
    }
    fn permits(&self, category: &str, operation: &str) -> bool {
        if category == "render" && operation == "frame" {
            return true;
        }
        if category == "gpu" || operation.ends_with("_gpu") {
            return self.config.gpu;
        }
        if category == "script" {
            return self.config.scripts;
        }
        if category == "resources" {
            return self.config.resources;
        }
        self.config.cpu
    }
    pub fn note_trigger_resources(&mut self, frame: u64, counts: ResourceSnapshot) {
        if !self.config.resources {
            return;
        }
        if let Some(pending) = &mut self.pending_slow {
            if pending.trigger_frame == frame {
                pending.resources = Some(counts);
            }
        }
    }
    pub fn slow_episode_count(&self) -> usize {
        self.slow_episodes.len() + usize::from(self.pending_slow.is_some())
    }
    fn retain_slow_episode(&mut self, episode: SlowEpisode) {
        if self.slow_episodes.len() >= MAX_SLOW_EPISODES {
            self.slow_episodes.pop_front();
            self.slow_episodes_evicted += 1;
        }
        self.slow_episodes.push_back(episode);
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn events(&self) -> &VecDeque<TimedEvent> {
        &self.events
    }
    pub fn dropped_events(&self) -> u64 {
        self.dropped
    }
    pub fn has_unsaved_capture(&self) -> bool {
        self.unsaved
    }
    pub fn last_saved_directory(&self) -> Option<&Path> {
        self.last_saved_directory.as_deref()
    }
    pub fn display_paused(&self) -> bool {
        self.display_frozen.is_some()
    }
    pub fn pause_display(&mut self) {
        if self.state == CaptureState::Running && self.display_frozen.is_none() {
            let mut summary = self.summary();
            summary.state = CaptureState::Paused;
            self.display_capture = Some(Arc::new(self.capture()));
            self.display_frozen = Some(Box::new(summary));
        }
    }
    pub fn resume_display(&mut self) {
        self.display_frozen = None;
        self.display_capture = None;
    }
    pub fn presentation_capture(&self) -> Arc<super::development_capture::Capture> {
        self.display_capture
            .clone()
            .unwrap_or_else(|| Arc::new(self.capture()))
    }
    pub fn summary(&self) -> CaptureSummary {
        if let Some(frozen) = &self.display_frozen {
            return *frozen.clone();
        }
        CaptureSummary {
            state: self.state,
            active_duration: self.active_time(),
            paused_duration: self.paused_time(),
            event_count: self.events.len(),
            dropped_events: self.dropped,
            slow_frame_triggers: self.slow_frame_triggers,
            slow_episodes_retained: self.slow_episode_count(),
            slow_episodes_evicted: self.slow_episodes_evicted,
            recording_overhead_us: self.recording_overhead_ns / 1_000,
            script_spike_samples: self.script_spikes.len(),
            navigation_call_samples: self.navigation_calls.len(),
            dropped_deep_samples: self.dropped_deep_samples,
            longest_script_slice: self
                .script_spikes
                .iter()
                .max_by_key(|s| s.duration)
                .map(|s| (s.script.clone(), s.duration)),
            longest_navigation_call: self
                .navigation_calls
                .iter()
                .max_by_key(|s| s.duration)
                .map(|s| (s.script.clone(), s.operation.to_owned(), s.duration)),
            script_nodes: self.live_script_nodes(),
            navigation_nodes: self.live_navigation_nodes(),
            engine_nodes: self.live_engine_nodes(),
            recent_diagnostics: self.live_recent_diagnostics(),
            average_fps: self.average_fps(),
            fps_1pct_low: self.fps_1pct_low(),
            p95_frame_time: self.captured_frame_time_percentile(95),
            p99_frame_time: self.captured_frame_time_percentile(99),
        }
    }
    fn ranked(mut rows: Vec<DiagnosticNode>, limit: usize) -> Vec<DiagnosticNode> {
        rows.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.label.cmp(&b.label)));
        rows.truncate(limit);
        rows
    }

    fn live_script_nodes(&self) -> Vec<DiagnosticNode> {
        Self::ranked(
            self.script_timings
                .iter()
                .map(|(name, timing)| DiagnosticNode {
                    label: name.clone(),
                    calls: timing.calls,
                    total: timing.total,
                    max: timing.max,
                })
                .collect(),
            10,
        )
    }

    fn live_navigation_nodes(&self) -> Vec<DiagnosticNode> {
        let mut groups: BTreeMap<String, (u64, Duration, Duration)> = BTreeMap::new();
        for sample in &self.navigation_calls {
            let key = format!("{} / {}", sample.script, sample.operation);
            let stat = groups.entry(key).or_default();
            stat.0 += 1;
            stat.1 += sample.duration;
            stat.2 = stat.2.max(sample.duration);
        }
        Self::ranked(
            groups
                .into_iter()
                .map(|(label, (calls, total, max))| DiagnosticNode {
                    label,
                    calls,
                    total,
                    max,
                })
                .collect(),
            10,
        )
    }

    fn live_engine_nodes(&self) -> Vec<DiagnosticNode> {
        let since = self.active_time().saturating_sub(Duration::from_secs(5));
        let mut groups: BTreeMap<String, (u64, Duration, Duration)> = BTreeMap::new();
        for sample in self.events.iter().rev() {
            if sample.at < since {
                break;
            }
            if sample.category == "render" && sample.operation == "frame" {
                continue;
            }
            let key = format!("{}/{}", sample.category, sample.operation);
            let stat = groups.entry(key).or_default();
            stat.0 += 1;
            stat.1 += sample.duration;
            stat.2 = stat.2.max(sample.duration);
        }
        Self::ranked(
            groups
                .into_iter()
                .map(|(label, (calls, total, max))| DiagnosticNode {
                    label,
                    calls,
                    total,
                    max,
                })
                .collect(),
            8,
        )
    }

    fn live_recent_diagnostics(&self) -> Vec<DiagnosticEvent> {
        let mut items: Vec<DiagnosticEvent> = self
            .script_spikes
            .iter()
            .rev()
            .take(12)
            .map(|s| DiagnosticEvent {
                at: s.at,
                label: format!("Script {}", s.script),
                duration: s.duration,
            })
            .chain(
                self.navigation_calls
                    .iter()
                    .rev()
                    .take(16)
                    .filter(|s| s.duration >= Duration::from_millis(5))
                    .map(|s| DiagnosticEvent {
                        at: s.at,
                        label: format!("{} / {}", s.script, s.operation),
                        duration: s.duration,
                    }),
            )
            .chain(
                self.events
                    .iter()
                    .rev()
                    .filter(|s| {
                        s.category == "render"
                            && s.operation == "frame"
                            && s.duration >= Duration::from_millis(self.config.threshold_ms as u64)
                    })
                    .take(12)
                    .map(|s| DiagnosticEvent {
                        at: s.at,
                        label: "Slow frame".to_owned(),
                        duration: s.duration,
                    }),
            )
            .collect();
        items.sort_by(|a, b| b.at.cmp(&a.at));
        items.truncate(16);
        items
    }

    pub fn log_directory(project_dir: &Path) -> PathBuf {
        project_dir
            .join(".worldkiln")
            .join("development")
            .join("logs")
    }
    pub fn captured_frame_time_percentile(&self, percentile: usize) -> Option<Duration> {
        if percentile > 100 {
            return None;
        }
        let mut frames: Vec<_> = self
            .events
            .iter()
            .filter(|event| event.category == "render" && event.operation == "frame")
            .map(|event| event.duration)
            .collect();
        if frames.is_empty() {
            return None;
        }
        frames.sort_unstable();
        let index = ((frames.len() - 1) * percentile + 50) / 100;
        Some(frames[index])
    }
    pub fn average_fps(&self) -> Option<f64> {
        let mut total_ns: u128 = 0;
        let mut count: u128 = 0;
        for event in self.events.iter() {
            if event.category == "render" && event.operation == "frame" {
                total_ns += event.duration.as_nanos();
                count += 1;
            }
        }
        if count == 0 || total_ns == 0 {
            return None;
        }
        Some(count as f64 * 1_000_000_000.0 / total_ns as f64)
    }
    pub fn fps_1pct_low(&self) -> Option<f64> {
        let mut frames: Vec<u128> = self
            .events
            .iter()
            .filter(|event| event.category == "render" && event.operation == "frame")
            .map(|event| event.duration.as_nanos())
            .collect();
        if frames.is_empty() {
            return None;
        }
        frames.sort_unstable_by(|a, b| b.cmp(a));
        let count = frames.len().div_ceil(100);
        let slow_total: u128 = frames.iter().take(count).sum();
        if slow_total == 0 {
            return None;
        }
        Some(count as f64 * 1_000_000_000.0 / slow_total as f64)
    }
    pub fn operation_timings(&self) -> Vec<OperationTiming> {
        let mut groups: BTreeMap<(&'static str, &'static str), Vec<Duration>> = BTreeMap::new();
        for event in self.events.iter() {
            if event.category == "render" && event.operation == "frame" {
                continue;
            }
            groups
                .entry((event.category, event.operation))
                .or_default()
                .push(event.duration);
        }
        let mut timings: Vec<_> = groups
            .into_iter()
            .map(|((category, operation), mut values)| {
                values.sort_unstable();
                let samples = values.len();
                let inclusive_total_ns = values
                    .iter()
                    .fold(0_u128, |acc, d| acc.saturating_add(d.as_nanos()));
                let rank_index =
                    |percentile: usize| (samples * percentile).div_ceil(100).saturating_sub(1);
                OperationTiming {
                    category,
                    operation,
                    samples,
                    inclusive_total_ns,
                    average_ns: inclusive_total_ns as f64 / samples as f64,
                    p50: values[rank_index(50)],
                    p95: values[rank_index(95)],
                    p99: values[rank_index(99)],
                    max: values[samples - 1],
                }
            })
            .collect();
        timings.sort_by(|a, b| {
            b.inclusive_total_ns
                .cmp(&a.inclusive_total_ns)
                .then_with(|| a.category.cmp(b.category))
                .then_with(|| a.operation.cmp(b.operation))
        });
        timings
    }

    pub fn slowest_operations(&self, limit: usize) -> Vec<&TimedEvent> {
        let mut operations: Vec<_> = self
            .events
            .iter()
            .filter(|e| !(e.category == "render" && e.operation == "frame"))
            .collect();
        operations.sort_unstable_by(|a, b| {
            b.duration
                .cmp(&a.duration)
                .then_with(|| a.at.cmp(&b.at))
                .then_with(|| a.category.cmp(b.category))
                .then_with(|| a.operation.cmp(b.operation))
        });
        operations.truncate(limit);
        operations
    }
    pub fn active_time(&self) -> Duration {
        self.active + self.active_since.map_or(Duration::ZERO, |t| t.elapsed())
    }
    pub fn paused_time(&self) -> Duration {
        self.paused + self.paused_since.map_or(Duration::ZERO, |t| t.elapsed())
    }
    pub fn start(&mut self, discard_unsaved: bool) -> bool {
        match self.state {
            CaptureState::Running => {
                self.resume_display();
                true
            }
            CaptureState::Paused => {
                if let Some(since) = self.paused_since.take() {
                    self.paused += since.elapsed();
                }
                self.active_since = Some(Instant::now());
                self.last_frame_start = None;
                self.state = CaptureState::Running;
                true
            }
            CaptureState::Ready | CaptureState::Stopped => {
                if self.unsaved && !discard_unsaved {
                    return false;
                }
                if Arc::strong_count(&self.events) > 1 {
                    self.events = Arc::new(VecDeque::with_capacity(MAX_EVENTS));
                } else {
                    let events = Arc::get_mut(&mut self.events).expect("exclusive event buffer");
                    events.clear();
                    if events.capacity() < MAX_EVENTS {
                        events.reserve(MAX_EVENTS);
                    }
                }
                self.capture_samples = Arc::new(VecDeque::new());
                self.pinned_samples = Arc::new(BTreeMap::new());
                self.pinned_dropped = 0;
                self.capture_dropped = 0;
                self.next_sample = 0;
                self.resume_display();
                self.script_timings.clear();
                self.script_spikes.clear();
                self.navigation_calls.clear();
                self.dropped_deep_samples = 0;
                self.completed_frames = 0;
                self.frame_context.clear();
                self.recent_frames.clear();
                self.pending_slow = None;
                self.slow_episodes.clear();
                self.slow_frame_triggers = 0;
                self.slow_episodes_evicted = 0;
                self.frame_context_dropped = 0;
                self.dropped = 0;
                self.recording_overhead_ns = 0;
                self.active = Duration::ZERO;
                self.paused = Duration::ZERO;
                self.paused_since = None;
                self.started_at = Some(SystemTime::now());
                self.stopped_at = None;
                self.active_since = Some(Instant::now());
                self.last_frame_start = None;
                self.state = CaptureState::Running;
                self.unsaved = true;
                self.generation = self.generation.saturating_add(1);
                true
            }
        }
    }
    fn freeze(&mut self) {
        self.last_frame_start = None;
        if let Some(pending) = self.pending_slow.take() {
            self.retain_slow_episode(pending);
        }
        self.recent_frames.clear();
        self.frame_context.clear();
        if let Some(since) = self.active_since.take() {
            self.active += since.elapsed();
        }
    }
    pub fn pause(&mut self) {
        if self.state == CaptureState::Running {
            self.freeze();
            self.paused_since = Some(Instant::now());
            self.state = CaptureState::Paused;
        }
    }
    pub fn stop(&mut self) {
        self.resume_display();
        if self.state == CaptureState::Running {
            self.freeze();
        }
        if matches!(self.state, CaptureState::Running | CaptureState::Paused) {
            if let Some(since) = self.paused_since.take() {
                self.paused += since.elapsed();
            }
            self.stopped_at = Some(SystemTime::now());
            self.state = CaptureState::Stopped;
        }
    }
    pub fn frame_boundary(&mut self) -> Option<u64> {
        if self.state != CaptureState::Running {
            return None;
        }
        let now = Instant::now();
        self.last_frame_start
            .replace(now)
            .and_then(|previous| self.complete_frame(now.duration_since(previous)))
    }
    fn complete_frame(&mut self, duration: Duration) -> Option<u64> {
        self.record("render", "frame", duration);
        self.completed_frames += 1;
        let index = self.completed_frames;
        let frame = FrameContext {
            index,
            events: std::mem::take(&mut self.frame_context),
        };
        let slow = duration > Duration::from_millis(self.config.threshold_ms as u64);
        if slow {
            self.slow_frame_triggers += 1;
        }
        if slow || self.pending_slow.is_some() {
            let first = index.saturating_sub(self.config.pre_trigger_frames as u64);
            for sample in self
                .capture_samples
                .iter()
                .filter(|s| (first..=index).contains(&s.frame))
            {
                if self.pinned_samples.contains_key(&sample.id) {
                    continue;
                }
                if self.pinned_samples.len() < 10_000 {
                    Arc::make_mut(&mut self.pinned_samples).insert(sample.id, sample.clone());
                } else {
                    self.pinned_dropped += 1;
                }
            }
        }
        let triggered = if let Some(active) = &mut self.pending_slow {
            active.frames.push(frame.clone());
            active.post_remaining = active.post_remaining.saturating_sub(1);
            if active.post_remaining == 0 {
                let finished = self.pending_slow.take().expect("capture is active");
                self.retain_slow_episode(finished);
            }
            None
        } else if slow {
            let mut frames = self.recent_frames.iter().cloned().collect::<Vec<_>>();
            frames.push(frame.clone());
            self.pending_slow = Some(SlowEpisode {
                trigger_frame: index,
                at: self.active_time(),
                duration,
                frames,
                post_remaining: self.config.post_trigger_frames,
                resources: None,
            });
            Some(index)
        } else {
            None
        };
        if self
            .pending_slow
            .as_ref()
            .is_some_and(|e| e.post_remaining == 0)
        {
            let finished = self.pending_slow.take().expect("zero post history");
            self.retain_slow_episode(finished);
        }
        self.recent_frames.push_back(frame);
        if self.recent_frames.len() > self.config.pre_trigger_frames {
            self.recent_frames.pop_front();
        }
        triggered
    }
    pub fn record(&mut self, category: &'static str, operation: &'static str, duration: Duration) {
        if self.state != CaptureState::Running || !self.permits(category, operation) {
            return;
        }
        let recording_started = Instant::now();
        let at = self.active_time();
        let events = Arc::make_mut(&mut self.events);
        if events.len() == MAX_EVENTS {
            events.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.next_sample = self.next_sample.saturating_add(1);
        let sample = super::development_capture::Sample {
            id: self.next_sample,
            parent: None,
            cause: None,
            frame: self.completed_frames + 1,
            thread: format!("{:?}", std::thread::current().id()),
            at_ns: super::development_capture::ns(at.saturating_sub(duration)),
            duration_ns: super::development_capture::ns(duration),
            self_ns: None,
            category: category.into(),
            operation: operation.into(),
            script: None,
            entity: None,
            asset: None,
            source: None,
            diagnostic: None,
        };
        events.push_back(TimedEvent {
            at,
            category,
            operation,
            duration,
        });
        if self.frame_context.len() < MAX_FRAME_CONTEXT_EVENTS {
            self.frame_context.push(TimedEvent {
                at,
                category,
                operation,
                duration,
            });
        } else {
            self.frame_context_dropped = self.frame_context_dropped.saturating_add(1);
        }
        self.push_capture_sample(sample);
        self.recording_overhead_ns = self
            .recording_overhead_ns
            .saturating_add(recording_started.elapsed().as_nanos());
    }
    pub(crate) fn note_missing_samples(&mut self, count: u64) {
        if self.state == CaptureState::Running {
            self.capture_dropped += count;
        }
    }
    pub(crate) fn next_frame(&self) -> u64 {
        self.completed_frames + 1
    }
    pub(crate) fn record_render_samples(
        &mut self,
        samples: Vec<(u64, &'static str, Duration, Instant, bool)>,
        dropped: u64,
    ) {
        if self.state != CaptureState::Running {
            return;
        }
        self.capture_dropped += dropped;
        for (frame, operation, duration, started, gpu) in samples {
            if (gpu && !self.config.gpu) || (!gpu && !self.config.cpu) {
                continue;
            }
            self.next_sample += 1;
            self.push_capture_sample(super::development_capture::Sample {
                id: self.next_sample,
                parent: None,
                cause: None,
                frame,
                thread: if gpu {
                    "GPU".into()
                } else {
                    format!("{:?}", std::thread::current().id())
                },
                at_ns: super::development_capture::ns(
                    self.active_time().saturating_sub(started.elapsed()),
                ),
                duration_ns: super::development_capture::ns(duration),
                self_ns: None,
                category: if gpu { "gpu".into() } else { "render".into() },
                operation: operation.into(),
                script: None,
                entity: None,
                asset: None,
                source: None,
                diagnostic: None,
            });
        }
    }
    fn push_capture_sample(&mut self, sample: super::development_capture::Sample) {
        if self.capture_samples.len() >= super::development_capture::SAMPLE_LIMIT {
            Arc::make_mut(&mut self.capture_samples).pop_front();
            self.capture_dropped += 1;
        }
        Arc::make_mut(&mut self.capture_samples).push_back(sample);
    }
    pub(crate) fn collect_spans(
        &mut self,
        samples: Vec<(super::development_capture::Sample, Instant)>,
        dropped: u64,
    ) {
        if self.state != CaptureState::Running {
            return;
        }
        self.capture_dropped += dropped;
        for (mut sample, started) in samples {
            if !self.permits(&sample.category, &sample.operation)
                || (sample.category == "callback" && !self.config.scripts)
                || (sample.category == "navigation" && !self.config.navigation_trace)
            {
                continue;
            }
            sample.id |= 1 << 63;
            sample.parent = sample.parent.map(|p| p | (1 << 63));
            if sample.id & (1 << 62) == 0 {
                sample.frame = self.completed_frames + 1;
            }
            sample.at_ns = super::development_capture::ns(
                self.active_time().saturating_sub(started.elapsed()),
            );
            sample.script = sample.script.as_deref().map(Self::script_label);
            self.push_capture_sample(sample);
        }
    }
    pub fn capture(&self) -> super::development_capture::Capture {
        let mut conditions = self.conditions.clone();
        conditions.engine = env!("CARGO_PKG_VERSION").into();
        conditions.configuration = serde_json::to_value(self.config).unwrap_or_default();
        let gpu = self
            .capture_samples
            .iter()
            .any(|s| s.category == "gpu" || s.operation.ends_with("_gpu"));
        let mut warnings=vec!["Unattributed: operations without captured parents have unknown self time. Inclusive totals overlap; do not sum them.".into(),
            "Not captured: allocator memory totals, uninstrumented lock waits and background work; uncovered frame time remains unexplained. In-flight jobs at Stop can be absent; pending GPU samples are counted as missing.".into(),
            "Source locations identify the callback entry when available; nested Module statement locations are not sampled. GPU durations use their original frame ID; CPU issue offsets are not calibrated GPU timestamps.".into()];
        if !gpu {
            warnings.push(
                if self.config.gpu {
                    "GPU timing Unavailable (or queries still pending)"
                } else {
                    "GPU timing Not captured"
                }
                .into(),
            );
        }
        warnings.push(format!("Recorder insertion cost: {} us; includes buffer insertion, excludes timers and workspace aggregation.",self.recording_overhead_ns/1000));
        let mut samples: Vec<_> = self.pinned_samples.values().cloned().collect();
        let mut omitted = 0;
        for sample in self.capture_samples.iter().rev() {
            if self.pinned_samples.contains_key(&sample.id) {
                continue;
            }
            if samples.len() < super::development_capture::SAMPLE_LIMIT {
                samples.push(sample.clone());
            } else {
                omitted += 1;
            }
        }
        samples.sort_by_key(|s| (s.frame, s.at_ns, s.id));
        if self.pinned_dropped > 0 {
            warnings.push(format!(
                "Hitch detail buffer full: {} events could not be pinned",
                self.pinned_dropped
            ));
        }
        super::development_capture::Capture {
            conditions,
            samples,
            dropped: self.capture_dropped
                + self.dropped_deep_samples
                + self.frame_context_dropped
                + self.pinned_dropped
                + omitted,
            warnings,
            ..Default::default()
        }
    }

    pub fn record_script(&mut self, script_path: &str, duration: Duration) {
        if self.state != CaptureState::Running || !self.config.scripts {
            return;
        }
        let normalized = script_path.replace('\\', "/");
        let label = normalized
            .rsplit_once("scripts/")
            .map(|(_, name)| format!("scripts/{name}"))
            .unwrap_or_else(|| {
                if normalized.starts_with("scripts/") {
                    normalized.clone()
                } else {
                    "<runtime>".to_owned()
                }
            });
        if label.len() > 180 {
            return;
        }
        if self.script_timings.len() >= 4096 && !self.script_timings.contains_key(&label) {
            self.dropped_deep_samples += 1;
            return;
        }
        let stat = self.script_timings.entry(label.clone()).or_default();
        stat.calls += 1;
        stat.total += duration;
        stat.max = stat.max.max(duration);
        if self.config.script_spikes
            && duration >= Duration::from_millis(self.config.script_spike_threshold_ms as u64)
        {
            let at = self.active_time();
            if self.script_spikes.len() < MAX_DEEP_SAMPLES {
                self.script_spikes.push(DeepSample {
                    at,
                    script: label,
                    operation: "script_fiber",
                    duration,
                });
            } else {
                self.dropped_deep_samples += 1;
            }
        }
    }

    pub(crate) fn record_navigation_traces(&mut self, samples: Vec<NavigationTimedSample>) {
        if self.state != CaptureState::Running || !self.config.navigation_trace {
            return;
        }
        for sample in samples {
            self.record("navigation", sample.operation, sample.elapsed);
            let at = self.active_time().saturating_sub(sample.ended.elapsed());
            if self.navigation_calls.len() < MAX_DEEP_SAMPLES {
                let label = Self::script_label(&sample.script);
                self.navigation_calls.push(DeepSample {
                    at,
                    script: label,
                    operation: sample.operation,
                    duration: sample.elapsed,
                });
            } else {
                self.dropped_deep_samples += 1;
            }
        }
    }

    fn script_label(path: &str) -> String {
        let normalized = path.replace('\\', "/");
        normalized
            .rsplit_once("scripts/")
            .map(|(_, name)| format!("scripts/{name}"))
            .unwrap_or_else(|| {
                if normalized.starts_with("scripts/") {
                    normalized
                } else {
                    "<runtime>".to_owned()
                }
            })
    }

    pub fn save_async(&self, project_dir: PathBuf) -> io::Result<CaptureSaveTask> {
        if self.state != CaptureState::Stopped {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stop capture before saving",
            ));
        }
        let mut snapshot = self.clone();
        let worker = std::thread::Builder::new()
            .name("worldkiln-development-save".into())
            .spawn(move || snapshot.save(&project_dir))?;
        Ok(CaptureSaveTask {
            generation: self.generation,
            worker,
        })
    }
    pub fn save(&mut self, project_dir: &Path) -> io::Result<PathBuf> {
        if self.state != CaptureState::Stopped {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stop capture before saving",
            ));
        }
        let root = Self::log_directory(project_dir);
        fs::create_dir_all(&root)?;
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        let mut output = None;
        for index in 0..1000 {
            let path = root.join(format!(
                "test-run-{}-{}-{index}",
                time.as_secs(),
                self.generation
            ));
            match fs::create_dir(&path) {
                Ok(()) => {
                    output = Some(path);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        let path = output.ok_or_else(|| {
            io::Error::new(io::ErrorKind::AlreadyExists, "no free session directory")
        })?;
        let written = (|| -> io::Result<()> {
            let events = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("events.csv"))?;
            let mut writer = io::BufWriter::new(events);
            writeln!(writer, "at_us,category,operation,duration_us")?;
            for event in self.events.iter() {
                writeln!(
                    writer,
                    "{},{},{},{}",
                    event.at.as_micros(),
                    csv_escape(event.category),
                    csv_escape(event.operation),
                    event.duration.as_micros()
                )?;
            }
            writer.flush()?;
            writer.get_ref().sync_all()?;
            let script_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("scripts.csv"))?;
            let mut script_writer = io::BufWriter::new(script_file);
            writeln!(script_writer, "script,calls,total_us,average_us,max_us")?;
            let mut ranked: Vec<_> = self.script_timings.iter().collect();
            ranked.sort_by(|a, b| b.1.total.cmp(&a.1.total));
            for (name, stat) in ranked {
                writeln!(
                    script_writer,
                    "{},{},{},{:.3},{}",
                    csv_escape(name),
                    stat.calls,
                    stat.total.as_micros(),
                    stat.total.as_secs_f64() * 1_000_000.0 / stat.calls as f64,
                    stat.max.as_micros()
                )?;
            }
            script_writer.flush()?;
            script_writer.get_ref().sync_all()?;

            for (filename, samples) in [
                ("script_spikes.csv", &self.script_spikes),
                ("navigation_calls.csv", &self.navigation_calls),
            ] {
                let file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path.join(filename))?;
                let mut writer = io::BufWriter::new(file);
                writeln!(writer, "at_us,script,operation,duration_us")?;
                for sample in samples {
                    writeln!(
                        writer,
                        "{},{},{},{}",
                        sample.at.as_micros(),
                        csv_escape(&sample.script),
                        csv_escape(sample.operation),
                        sample.duration.as_micros()
                    )?;
                }
                writer.flush()?;
            }
            let errors = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("errors.csv"))?;
            let mut errors_writer = io::BufWriter::new(errors);
            writeln!(errors_writer, "at_us,category,operation")?;
            for event in self.events.iter() {
                if event.operation.contains("failed") || event.operation.contains("error") {
                    writeln!(
                        errors_writer,
                        "{},{},{}",
                        event.at.as_micros(),
                        csv_escape(event.category),
                        csv_escape(event.operation)
                    )?;
                }
            }
            errors_writer.flush()?;
            errors_writer.get_ref().sync_all()?;

            let operation_timings = self.operation_timings();
            let operation_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("operations.csv"))?;
            let mut operation_writer = io::BufWriter::new(operation_file);
            writeln!(
                operation_writer,
                "rank,category,operation,samples,inclusive_total_us,average_us,p95_us,max_us,p50_us,p99_us"
            )?;
            for (index, stat) in operation_timings.iter().enumerate() {
                writeln!(
                    operation_writer,
                    "{},{},{},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
                    index + 1,
                    csv_escape(stat.category),
                    csv_escape(stat.operation),
                    stat.samples,
                    stat.inclusive_total_ns as f64 / 1_000.0,
                    stat.average_ns / 1_000.0,
                    stat.p95.as_secs_f64() * 1_000_000.0,
                    stat.max.as_secs_f64() * 1_000_000.0,
                    stat.p50.as_secs_f64() * 1_000_000.0,
                    stat.p99.as_secs_f64() * 1_000_000.0
                )?;
            }
            operation_writer.flush()?;
            operation_writer.get_ref().sync_all()?;

            let slowest_samples = self.slowest_operations(100);
            let spikes_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("slowest_events.csv"))?;
            let mut spikes_writer = io::BufWriter::new(spikes_file);
            writeln!(spikes_writer, "rank,at_us,category,operation,duration_us")?;
            for (index, event) in slowest_samples.iter().enumerate() {
                writeln!(
                    spikes_writer,
                    "{},{},{},{},{}",
                    index + 1,
                    event.at.as_micros(),
                    csv_escape(event.category),
                    csv_escape(event.operation),
                    event.duration.as_micros()
                )?;
            }
            spikes_writer.flush()?;
            spikes_writer.get_ref().sync_all()?;

            let slow_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("slow_frames.csv"))?;
            let mut slow_writer = io::BufWriter::new(slow_file);
            writeln!(
                slow_writer,
                "trigger_frame,at_us,frame_duration_us,context_frames,physics_entities,active_characters"
            )?;
            let details_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("slow_frame_events.csv"))?;
            let mut details_writer = io::BufWriter::new(details_file);
            writeln!(
                details_writer,
                "trigger_frame,context_frame,at_us,category,operation,duration_us"
            )?;
            for episode in &self.slow_episodes {
                writeln!(
                    slow_writer,
                    "{},{},{},{},{},{}",
                    episode.trigger_frame,
                    episode.at.as_micros(),
                    episode.duration.as_micros(),
                    episode.frames.len(),
                    episode
                        .resources
                        .as_ref()
                        .map(|r| r.physics_entities.to_string())
                        .unwrap_or_default(),
                    episode
                        .resources
                        .as_ref()
                        .map(|r| r.active_characters.to_string())
                        .unwrap_or_default()
                )?;
                for frame in &episode.frames {
                    for event in &frame.events {
                        writeln!(
                            details_writer,
                            "{},{},{},{},{},{}",
                            episode.trigger_frame,
                            frame.index,
                            event.at.as_micros(),
                            csv_escape(event.category),
                            csv_escape(event.operation),
                            event.duration.as_micros()
                        )?;
                    }
                }
            }
            slow_writer.flush()?;
            details_writer.flush()?;
            let packages_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("slow_frame_resources.csv"))?;
            let mut packages_writer = io::BufWriter::new(packages_file);
            writeln!(packages_writer, "trigger_frame,package,count")?;
            for episode in &self.slow_episodes {
                if let Some(resources) = &episode.resources {
                    for (package, count) in &resources.character_packages {
                        writeln!(
                            packages_writer,
                            "{},{},{}",
                            episode.trigger_frame,
                            csv_escape(package),
                            count
                        )?;
                    }
                }
            }
            packages_writer.flush()?;
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("slow_frame_physics.csv"))?;
            let mut writer = io::BufWriter::new(file);
            writeln!(
                writer,
                "trigger_frame,physics_entities,characters,dynamic_bodies,active_dynamic_bodies,sleeping_dynamic_bodies,broadphase_cells,broadphase_references,anchored_colliders,static_collider_chunks,noncolliding_physics_entities"
            )?;
            for episode in &self.slow_episodes {
                if let Some(p) = &episode.resources {
                    writeln!(
                        writer,
                        "{},{},{},{},{},{},{},{},{},{},{}",
                        episode.trigger_frame,
                        p.physics_entities,
                        p.active_characters,
                        p.dynamic_bodies,
                        p.active_dynamic_bodies,
                        p.sleeping_dynamic_bodies,
                        p.broadphase_cells,
                        p.broadphase_references,
                        p.anchored_colliders,
                        p.static_collider_chunks,
                        p.noncolliding_physics_entities
                    )?;
                }
            }
            writer.flush()?;

            let top_operations: Vec<_> = operation_timings
                .iter()
                .take(20)
                .enumerate()
                .map(|(index, stat)| {
                    serde_json::json!({
                        "rank": index + 1,
                        "category": stat.category,
                        "operation": stat.operation,
                        "samples": stat.samples,
                        "inclusive_total_us": stat.inclusive_total_ns as f64 / 1_000.0,
                        "average_us": stat.average_ns / 1_000.0,
                        "p50_us": stat.p50.as_secs_f64() * 1_000_000.0,
                        "p95_us": stat.p95.as_secs_f64() * 1_000_000.0,
                        "p99_us": stat.p99.as_secs_f64() * 1_000_000.0,
                        "max_us": stat.max.as_secs_f64() * 1_000_000.0,
                    })
                })
                .collect();
            let summary = self.summary();
            let report = serde_json::json!({
                "schema_version": 2,
                "capture_mode": "automatic",
                "slow_frame_threshold_ms": self.config.threshold_ms,
                "capture_categories": self.config,
                "slow_frame_triggers": self.slow_frame_triggers,
                "script_spike_samples": self.script_spikes.len(),
                "navigation_call_samples": self.navigation_calls.len(),
                "discarded_deep_samples": self.dropped_deep_samples,
                "slow_episodes_retained": self.slow_episodes.len(),
                "slow_episodes_evicted": self.slow_episodes_evicted,
                "slow_frame_context_events_dropped": self.frame_context_dropped,
                "slow_frame_note": "Trigger compares render-start intervals; inclusive context events can overlap. Captures include up to two preceding and two following frames; consecutive slow frames may share one episode. Eviction counters report incomplete captures.",
                "engine_version": env!("CARGO_PKG_VERSION"),
                "project_path": project_dir.to_string_lossy(),
                "host_os": std::env::consts::OS,
                "host_architecture": std::env::consts::ARCH,
                "recording_duration_us": summary.active_duration.as_micros(),
                "paused_duration_us": summary.paused_duration.as_micros(),
                "started_at_unix_ms": self.started_at.and_then(|v| v.duration_since(UNIX_EPOCH).ok()).map(|v| v.as_millis()),
                "stopped_at_unix_ms": self.stopped_at.and_then(|v| v.duration_since(UNIX_EPOCH).ok()).map(|v| v.as_millis()),
                "event_count": summary.event_count,
                "discarded_oldest_events": summary.dropped_events,
                "estimated_capture_recording_cpu_us": summary.recording_overhead_us,
                "operation_statistics_note": "Retained non-frame samples; inclusive spans can overlap; totals are not additive frame costs.",
                "operation_statistics_count": operation_timings.len(),
                "slowest_event_samples_count": slowest_samples.len(),
                "most_expensive_operations": top_operations,
                "average_fps": summary.average_fps,
                "fps_1pct_low": summary.fps_1pct_low,
                "frame_p95_us": summary.p95_frame_time.map(|d| d.as_micros()),
                "frame_p99_us": summary.p99_frame_time.map(|d| d.as_micros()),
                "gpu_timing_available": self.capture_samples.iter().any(|e|e.category=="gpu"||e.operation.ends_with("_gpu")),
                "gpu_timing_note": "GPU samples are asynchronous and limited to instrumented passes; no full-frame GPU timing is inferred."
            });
            let capture = self.capture();
            capture.write_attribution(&path)?;
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("capture.json"))?;
            let mut writer = io::BufWriter::new(file);
            serde_json::to_writer(&mut writer, &capture).map_err(io::Error::other)?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
            let mut readable = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("report.txt"))?;
            readable.write_all(capture.report().as_bytes())?;
            readable.sync_all()?;
            let summary_file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join("summary.json"))?;
            let mut summary_writer = io::BufWriter::new(summary_file);
            serde_json::to_writer_pretty(&mut summary_writer, &report).map_err(io::Error::other)?;
            summary_writer.flush()?;
            summary_writer.get_ref().sync_all()?;
            Ok(())
        })();
        if let Err(error) = written {
            let _ = fs::remove_dir_all(&path);
            return Err(error);
        }
        self.unsaved = false;
        self.last_saved_directory = Some(path.clone());
        Ok(path)
    }
}
fn csv_escape(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_pause_freezes_summary_while_recording_continues() {
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.record("ui", "before", Duration::from_millis(1));
        recorder.pause_display();
        let frozen = recorder.summary();
        let generation = recorder.generation();
        recorder.record("ui", "during", Duration::from_millis(2));
        assert_eq!(recorder.state(), CaptureState::Running);
        assert_eq!(recorder.summary().state, CaptureState::Paused);
        assert_eq!(recorder.summary().event_count, frozen.event_count);
        assert_eq!(recorder.events().len(), 2);
        recorder.start(false);
        assert_eq!(recorder.generation(), generation);
        assert_eq!(recorder.summary().event_count, 2);
        recorder.pause_display();
        recorder.stop();
        assert!(!recorder.display_paused());
        assert_eq!(recorder.summary().state, CaptureState::Stopped);
    }
    #[test]
    fn saved_capture_reopens_with_unknown_optional_fields_and_legacy_reports() {
        let root = std::env::temp_dir().join(format!(
            "wk-capture-roundtrip-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.record("physics", "step", Duration::from_millis(4));
        recorder.complete_frame(Duration::from_millis(55));
        recorder.stop();
        let directory = recorder.save(&root).unwrap();
        let path = directory.join("capture.json");
        let mut json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        json["future_optional_metadata"] = serde_json::json!({"supported":true});
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        let capture = crate::engine::development_capture::Capture::read(&path).unwrap();
        assert!(
            capture
                .samples
                .iter()
                .any(|s| s.operation == "step" && s.self_ns.is_none())
        );
        assert!(directory.join("events.csv").is_file());
        assert!(directory.join("summary.json").is_file());
        assert!(directory.join("report.txt").is_file());
        assert!(capture.report().contains("unexplained"));
        json["format_version"] = serde_json::json!(999);
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(crate::engine::development_capture::Capture::read(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn asynchronous_gpu_samples_keep_original_frame_identity() {
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.complete_frame(Duration::from_millis(16));
        recorder.complete_frame(Duration::from_millis(17));
        recorder.record_render_samples(
            vec![(1, "shadow", Duration::from_millis(3), Instant::now(), true)],
            0,
        );
        let capture = recorder.capture();
        let sample = capture
            .samples
            .iter()
            .find(|s| s.category == "gpu")
            .unwrap();
        assert_eq!(sample.frame, 1);
        assert_eq!(sample.duration_ns, 3_000_000);
        assert!(sample.self_ns.is_none());
        recorder.stop();
        recorder.set_config(CaptureConfig {
            gpu: false,
            ..Default::default()
        });
        recorder.start(true);
        recorder.record_render_samples(
            vec![(1, "shadow", Duration::from_millis(3), Instant::now(), true)],
            9,
        );
        assert!(recorder.capture().samples.is_empty());
    }
    #[test]
    fn configured_hitch_history_supports_immediate_and_extended_capture() {
        let mut recorder = TestRunRecorder::default();
        recorder.set_config(CaptureConfig {
            pre_trigger_frames: 3,
            post_trigger_frames: 0,
            ..Default::default()
        });
        recorder.start(false);
        for _ in 0..3 {
            recorder.complete_frame(Duration::from_millis(10));
        }
        recorder.complete_frame(Duration::from_millis(70));
        assert_eq!(recorder.slow_episodes.len(), 1);
        assert_eq!(recorder.slow_episodes[0].frames.len(), 4);
        assert!(recorder.pending_slow.is_none());
    }

    #[test]
    fn deep_diagnostics_attribute_native_navigation_to_invoking_script() {
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        start_navigation_trace(true);
        trace_script_context(Some("scripts/gameplay/enemies/ai_controller.aeo"));
        timed_navigation("navmesh_build", || {
            std::thread::sleep(Duration::from_millis(2));
        });
        timed_navigation("navmesh_path_query", || {
            std::thread::sleep(Duration::from_millis(1));
        });
        let samples = finish_navigation_trace();
        assert_eq!(samples.len(), 2);
        assert!(
            samples
                .iter()
                .all(|s| s.script == "scripts/gameplay/enemies/ai_controller.aeo")
        );
        recorder.record_navigation_traces(samples);
        recorder.record_script(
            "scripts/gameplay/enemies/ai_controller.aeo",
            Duration::from_millis(1200),
        );
        assert_eq!(recorder.navigation_calls.len(), 2);
        assert_eq!(recorder.script_spikes.len(), 1);
        assert_eq!(
            recorder.script_spikes[0].duration,
            Duration::from_millis(1200)
        );
        assert!(
            recorder
                .events()
                .iter()
                .any(|event| event.operation == "navmesh_build" && event.category == "navigation")
        );
        recorder.stop();
        let root =
            std::env::temp_dir().join(format!("worldkiln-stall-diag-{}", std::process::id(),));
        let directory = recorder.save(&root).unwrap();
        let nav = fs::read_to_string(directory.join("navigation_calls.csv")).unwrap();
        let scripts = fs::read_to_string(directory.join("script_spikes.csv")).unwrap();
        assert!(nav.contains("\"scripts/gameplay/enemies/ai_controller.aeo\",\"navmesh_build\","));
        assert!(
            nav.contains("\"scripts/gameplay/enemies/ai_controller.aeo\",\"navmesh_path_query\",")
        );
        assert!(
            scripts.contains(
                "\"scripts/gameplay/enemies/ai_controller.aeo\",\"script_fiber\",1200000"
            )
        );
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("summary.json")).unwrap()).unwrap();
        assert_eq!(report["navigation_call_samples"], 2);
        assert_eq!(report["script_spike_samples"], 1);
        assert_eq!(report["discarded_deep_samples"], 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deep_trace_can_be_disabled_and_script_threshold_is_respected() {
        let mut recorder = TestRunRecorder::default();
        recorder.set_config(CaptureConfig {
            script_spike_threshold_ms: 50,
            navigation_trace: false,
            ..CaptureConfig::default()
        });
        assert!(recorder.start(false));
        start_navigation_trace(false);
        trace_script_context(Some("scripts/ai.aeo"));
        timed_navigation("navmesh_build", || ());
        assert!(finish_navigation_trace().is_empty());
        recorder.record_script("scripts/ai.aeo", Duration::from_millis(40));
        recorder.record_script("scripts/ai.aeo", Duration::from_millis(51));
        assert_eq!(recorder.script_spikes.len(), 1);
        assert_eq!(recorder.script_timings["scripts/ai.aeo"].calls, 2);
        recorder.stop();
    }

    #[test]
    fn profiler_categories_can_be_disabled_and_do_not_change_frame_detection() {
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.set_config(CaptureConfig {
            cpu: false,
            gpu: false,
            scripts: false,
            resources: false,
            threshold_ms: 33,
            script_spikes: false,
            navigation_trace: false,
            script_spike_threshold_ms: 20,
            ..CaptureConfig::default()
        }));
        assert!(recorder.start(false));
        assert!(!recorder.set_config(CaptureConfig::default()));
        recorder.record("physics", "contacts_cpu", Duration::from_millis(10));
        recorder.record("fx", "native_effekseer_draw_gpu", Duration::from_millis(10));
        recorder.record("script", "script_update_cpu", Duration::from_millis(10));
        recorder.record("resources", "snapshot", Duration::from_millis(10));
        recorder.record_script("scripts/systems/ghosts.aeo", Duration::from_millis(20));
        assert!(recorder.events().is_empty());
        assert!(recorder.script_timings.is_empty());
        assert_eq!(recorder.complete_frame(Duration::from_millis(34)), Some(1));
        assert_eq!(recorder.events().len(), 1);
        recorder.note_trigger_resources(
            1,
            ResourceSnapshot {
                physics_entities: 5,
                ..Default::default()
            },
        );
        recorder.stop();
        assert_eq!(recorder.slow_episodes.len(), 1);
        assert!(recorder.slow_episodes[0].resources.is_none());
        assert!(recorder.set_config(CaptureConfig::default()));
    }

    #[test]
    fn slow_frame_context_survives_general_event_ring_eviction() {
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        recorder.record("physics", "pre_trigger", Duration::from_micros(12));
        recorder.complete_frame(Duration::from_millis(50));
        for _ in 0..MAX_EVENTS + 5 {
            recorder.record("physics", "ordinary", Duration::from_micros(1));
        }
        recorder.complete_frame(Duration::from_millis(15));
        recorder.stop();
        assert!(recorder.dropped_events() > 0);
        assert_eq!(recorder.slow_episodes.len(), 1);
        assert!(
            recorder.slow_episodes[0].frames[0]
                .events
                .iter()
                .any(|event| event.operation == "pre_trigger")
        );
        assert!(recorder.frame_context_dropped > 0);
    }

    #[test]
    fn session_transitions_and_overwrite_guard() {
        let mut recorder = TestRunRecorder::default();
        assert_eq!(recorder.events.capacity(), 0);
        assert!(recorder.start(false));
        assert!(recorder.events.capacity() >= MAX_EVENTS);
        recorder.record("ui", "open_panel", Duration::from_millis(3));
        recorder.pause();
        assert_eq!(recorder.state(), CaptureState::Paused);
        let paused_time = recorder.active_time();
        std::thread::sleep(Duration::from_millis(2));
        assert_eq!(recorder.active_time(), paused_time);
        recorder.record("ui", "ignored", Duration::ZERO);
        assert!(recorder.paused_time() >= Duration::from_millis(2));
        assert!(recorder.start(false));
        assert_eq!(recorder.events().len(), 1);
        recorder.stop();
        assert!(!recorder.start(false));
        assert!(recorder.start(true));
        assert_eq!(recorder.generation(), 2);
        assert!(recorder.events().is_empty());
    }
    #[test]
    fn frame_boundary_does_not_count_paused_gap() {
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.frame_boundary();
        recorder.frame_boundary();
        assert_eq!(recorder.events().len(), 1);
        recorder.pause();
        recorder.frame_boundary();
        recorder.start(false);
        recorder.frame_boundary();
        assert_eq!(recorder.events().len(), 1);
        recorder.frame_boundary();
        assert_eq!(recorder.events().len(), 2);
    }
    #[test]
    fn frame_percentiles_ignore_other_events() {
        let mut recorder = TestRunRecorder::default();
        assert_eq!(recorder.captured_frame_time_percentile(95), None);
        recorder.start(false);
        recorder.record("ui", "button", Duration::from_secs(3));
        for millis in [10, 20, 30, 40, 50] {
            recorder.record("render", "frame", Duration::from_millis(millis));
        }
        assert_eq!(
            recorder.captured_frame_time_percentile(0),
            Some(Duration::from_millis(10))
        );
        assert_eq!(
            recorder.captured_frame_time_percentile(95),
            Some(Duration::from_millis(50))
        );
        assert_eq!(recorder.captured_frame_time_percentile(101), None);
        let summary = recorder.summary();
        assert_eq!(summary.state, CaptureState::Running);
        assert_eq!(summary.event_count, 6);
        assert_eq!(summary.p95_frame_time, Some(Duration::from_millis(50)));
        assert_eq!(summary.p99_frame_time, Some(Duration::from_millis(50)));
        assert!((summary.fps_1pct_low.unwrap() - 20.0).abs() < 0.01);
        assert!((recorder.average_fps().unwrap() - (1000.0 / 30.0)).abs() < 0.01);
        assert_eq!(recorder.slowest_operations(1)[0].operation, "button");
    }
    #[test]
    fn operation_timings_rank_retained_samples_and_exclude_frame_intervals() {
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.operation_timings().is_empty());
        recorder.start(false);
        for ms in [10, 20, 30] {
            recorder.record("ui", "properties_panel", Duration::from_millis(ms));
        }
        recorder.record("script", "update_cpu", Duration::from_millis(100));
        recorder.record("render", "frame", Duration::from_secs(1));
        recorder.pause();
        recorder.record("ui", "properties_panel", Duration::from_secs(9));
        let stats = recorder.operation_timings();
        assert_eq!(stats.len(), 2);
        assert_eq!(
            (stats[0].category, stats[0].operation),
            ("script", "update_cpu")
        );
        assert_eq!(stats[0].samples, 1);
        assert_eq!(stats[0].inclusive_total_ns, 100_000_000);
        assert_eq!(
            (stats[1].category, stats[1].operation),
            ("ui", "properties_panel")
        );
        assert_eq!(stats[1].samples, 3);
        assert_eq!(stats[1].inclusive_total_ns, 60_000_000);
        assert_eq!(stats[1].average_ns, 20_000_000.0);
        assert_eq!(stats[1].p50, Duration::from_millis(20));
        assert_eq!(stats[1].p95, Duration::from_millis(30));
        assert_eq!(stats[1].p99, Duration::from_millis(30));
        assert_eq!(stats[1].max, Duration::from_millis(30));
    }

    #[test]
    fn operation_percentiles_use_nearest_rank_without_interpolating_spikes() {
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        for millis in 1..=20 {
            recorder.record("ui", "frame_update", Duration::from_millis(millis));
        }
        let row = recorder.operation_timings().remove(0);
        assert_eq!(row.samples, 20);
        assert_eq!(row.p50, Duration::from_millis(10));
        assert_eq!(row.p95, Duration::from_millis(19));
        assert_eq!(row.p99, Duration::from_millis(20));
        assert_eq!(row.max, Duration::from_millis(20));
    }

    #[test]
    fn operation_rank_ties_are_stable_and_bounded_overflow_is_disclosed() {
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.record("ui", "zeta", Duration::from_micros(200));
        recorder.record("ui", "alpha", Duration::from_micros(200));
        let rows = recorder.operation_timings();
        assert_eq!(
            rows.iter().map(|r| r.operation).collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        for _ in 0..MAX_EVENTS {
            recorder.record("ui", "alpha", Duration::from_nanos(1));
        }
        assert_eq!(recorder.dropped_events(), 2);
        let rows = recorder.operation_timings();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].samples, MAX_EVENTS);
    }

    #[test]
    fn bounded_capture_reports_dropped_events() {
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        for index in 0..MAX_EVENTS + 7 {
            recorder.record("ui", "operation", Duration::from_micros(index as u64));
        }
        assert_eq!(recorder.events().len(), MAX_EVENTS);
        assert_eq!(recorder.dropped_events(), 7);
        assert_eq!(
            recorder.events().front().unwrap().duration,
            Duration::from_micros(7)
        );
        assert_eq!(
            recorder.events().back().unwrap().duration,
            Duration::from_micros((MAX_EVENTS + 6) as u64)
        );
        recorder.stop();
        assert!(recorder.start(true));
        assert_eq!(recorder.dropped_events(), 0);
        assert!(recorder.events().is_empty());
    }
    #[test]
    fn rapid_pause_resume_stop_preserves_session_and_clean_frame_boundaries() {
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        for _ in 0..100 {
            recorder.frame_boundary();
            recorder.pause();
            let prior = recorder.active_time();
            recorder.pause();
            assert_eq!(recorder.active_time(), prior);
            recorder.frame_boundary();
            assert!(recorder.start(false));
            recorder.frame_boundary();
        }
        assert_eq!(recorder.events.len(), 99);
        recorder.frame_boundary();
        assert_eq!(recorder.events.len(), 100);
        recorder.stop();
        recorder.stop();
        assert_eq!(recorder.state(), CaptureState::Stopped);
        assert!(!recorder.start(false));
    }
    #[test]
    fn background_save_uses_snapshot_and_respects_new_generation() {
        let folder = std::env::temp_dir().join(format!(
            "worldkiln-test-run-async-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        assert!(recorder.save_async(folder.clone()).is_err());
        recorder.record("render", "frame", Duration::from_millis(20));
        recorder.stop();
        let task = recorder.save_async(folder.clone()).unwrap();
        assert!(recorder.has_unsaved_capture());
        let path = task.finish(&mut recorder).unwrap();
        assert!(path.join("summary.json").exists());
        assert_eq!(recorder.last_saved_directory(), Some(path.as_path()));
        assert!(!recorder.has_unsaved_capture());
        assert!(recorder.start(false));
        recorder.stop();
        let stale = recorder.save_async(folder.clone()).unwrap();
        assert!(recorder.start(true));
        recorder.stop();
        stale.finish(&mut recorder).unwrap();
        assert!(recorder.has_unsaved_capture());
        assert_eq!(recorder.last_saved_directory(), Some(path.as_path()));
        let _ = fs::remove_dir_all(folder);
    }
    #[test]
    fn overlapping_background_save_keeps_old_events_when_new_session_begins() {
        let folder = std::env::temp_dir().join(format!(
            "wk-dev004-snapshot-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.record("ui", "old_project_panel", Duration::from_millis(12));
        recorder.stop();
        let old_task = recorder.save_async(folder.clone()).unwrap();
        assert!(recorder.start(true));
        recorder.record("ui", "new_project_panel", Duration::from_millis(25));
        recorder.stop();
        let old_path = old_task.finish(&mut recorder).unwrap();
        let old_csv = fs::read_to_string(old_path.join("events.csv")).unwrap();
        assert!(old_csv.contains("old_project_panel"));
        assert!(!old_csv.contains("new_project_panel"));
        assert_eq!(recorder.events().len(), 1);
        assert_eq!(
            recorder.events().front().unwrap().operation,
            "new_project_panel"
        );
        assert!(recorder.has_unsaved_capture());
        let new_path = recorder.save(&folder).unwrap();
        assert_ne!(old_path, new_path);
        let new_csv = fs::read_to_string(new_path.join("events.csv")).unwrap();
        assert!(new_csv.contains("new_project_panel"));
        assert!(!new_csv.contains("old_project_panel"));
        assert!(old_path.join("operations.csv").is_file());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn failed_save_preserves_unsaved_capture() {
        let folder = std::env::temp_dir().join(format!(
            "worldkiln-test-run-failed-save-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&folder, b"not a directory").unwrap();
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        recorder.record("ui", "edit", Duration::from_millis(2));
        recorder.stop();
        assert!(recorder.save(&folder).is_err());
        assert!(recorder.has_unsaved_capture());
        assert_eq!(recorder.events().len(), 1);
        fs::remove_file(folder).unwrap();
    }
    #[test]
    fn save_rejects_active_capture_and_escapes_event_fields() {
        let folder = std::env::temp_dir().join(format!(
            "worldkiln-test-run-escape-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = TestRunRecorder::default();
        recorder.start(false);
        assert_eq!(
            recorder.save(&folder).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(!folder.exists());
        recorder.record("ui,buttons", "clicked \"Save\"", Duration::from_millis(4));
        recorder.record(
            "project",
            "project_save_failed_cpu",
            Duration::from_millis(5),
        );
        recorder.stop();
        let directory = recorder.save(&folder).unwrap();
        let csv = fs::read_to_string(directory.join("events.csv")).unwrap();
        let summary: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("summary.json")).unwrap()).unwrap();
        assert_eq!(summary["event_count"], 2);
        assert_eq!(summary["operation_statistics_count"], 2);
        assert_eq!(
            summary["most_expensive_operations"][0]["operation"],
            "project_save_failed_cpu"
        );
        assert_eq!(
            summary["most_expensive_operations"][0]["inclusive_total_us"],
            5_000.0
        );
        assert_eq!(summary["most_expensive_operations"][1]["samples"], 1);
        let operations = fs::read_to_string(directory.join("operations.csv")).unwrap();
        assert!(operations.starts_with("rank,category,operation,samples,inclusive_total_us"));
        assert!(operations.contains("1,\"project\",\"project_save_failed_cpu\",1,5000.000"));
        assert!(operations.contains("\"ui,buttons\",\"clicked \"\"Save\"\"\""));
        assert!(
            operations
                .lines()
                .next()
                .unwrap()
                .ends_with(",p50_us,p99_us")
        );
        assert_eq!(summary["most_expensive_operations"][0]["p50_us"], 5_000.0);
        assert_eq!(summary["most_expensive_operations"][0]["p99_us"], 5_000.0);
        let spikes = fs::read_to_string(directory.join("slowest_events.csv")).unwrap();
        assert_eq!(spikes.lines().count(), 3);
        assert!(
            spikes
                .lines()
                .nth(1)
                .unwrap()
                .contains("\"project\",\"project_save_failed_cpu\",5000")
        );
        assert!(
            spikes
                .lines()
                .nth(2)
                .unwrap()
                .contains("\"ui,buttons\",\"clicked \"\"Save\"\"\",4000")
        );
        assert_eq!(summary["slowest_event_samples_count"], 2);
        let errors = fs::read_to_string(directory.join("errors.csv")).unwrap();
        assert!(errors.contains("project_save_failed_cpu"));
        assert!(!errors.contains("clicked"));
        assert!(summary["started_at_unix_ms"].is_number());
        assert!(summary["stopped_at_unix_ms"].is_number());
        assert!(summary["paused_duration_us"].is_number());
        assert!(csv.contains("\"ui,buttons\",\"clicked \"\"Save\"\"\""));
        let _ = fs::remove_dir_all(folder);
    }
    #[test]
    #[ignore = "manual microbenchmark; no pass/fail timing threshold"]
    fn benchmark_recording_overhead() {
        let mut off = TestRunRecorder::default();
        let disabled_started = Instant::now();
        for _ in 0..100_000 {
            off.record("render", "frame", Duration::from_millis(16));
        }
        let disabled = disabled_started.elapsed();
        let mut on = TestRunRecorder::default();
        on.start(false);
        let enabled_started = Instant::now();
        for _ in 0..100_000 {
            on.record("render", "frame", Duration::from_millis(16));
        }
        let enabled = enabled_started.elapsed();
        println!(
            "test_run 100000 events: disabled={disabled:?}; enabled={enabled:?}; estimated_recorder_cpu={:?}; retained={}; dropped={}",
            Duration::from_nanos((on.recording_overhead_ns.min(u64::MAX as u128)) as u64),
            on.events().len(),
            on.dropped_events()
        );
    }
    #[test]
    #[ignore = "manual report-writer benchmark; no fixed timing threshold"]
    fn benchmark_large_capture_report_save() {
        let root = std::env::temp_dir().join(format!(
            "wk-dev004-report-bench-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        for index in 0..MAX_EVENTS {
            let op = match index % 4 {
                0 => "properties_panel",
                1 => "asset_manager_browser",
                2 => "script_update_cpu",
                _ => "physics_step_cpu",
            };
            recorder.record("ui", op, Duration::from_micros((index % 900 + 100) as u64));
        }
        recorder.stop();
        let snapshot_started = Instant::now();
        let task = recorder.save_async(root.clone()).unwrap();
        let snapshot_time = snapshot_started.elapsed();
        let complete_started = Instant::now();
        let report = task.finish(&mut recorder).unwrap();
        let write_time = complete_started.elapsed();
        let ops = fs::metadata(report.join("operations.csv")).unwrap().len();
        let spikes = fs::read_to_string(report.join("slowest_events.csv")).unwrap();
        assert_eq!(spikes.lines().count(), 101);
        assert_eq!(
            recorder
                .operation_timings()
                .iter()
                .map(|s| s.samples)
                .sum::<usize>(),
            MAX_EVENTS
        );
        println!(
            "DEV004 50000 events: snapshot/start={} ms, async finish={} ms, operations.csv={} bytes",
            snapshot_time.as_secs_f64() * 1000.0,
            write_time.as_secs_f64() * 1000.0,
            ops
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stopped_capture_saves_and_preserves_prior_logs() {
        let folder = std::env::temp_dir().join(format!(
            "worldkiln-test-run-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        recorder.record("render", "frame", Duration::from_millis(16));
        recorder.stop();
        let first = recorder.save(&folder).unwrap();
        assert!(first.exists());
        assert_eq!(recorder.last_saved_directory(), Some(first.as_path()));
        let operations = fs::read_to_string(first.join("operations.csv")).unwrap();
        assert_eq!(
            operations.lines().count(),
            1,
            "frame-only session has no ranked operations"
        );
        assert_eq!(
            fs::read_to_string(first.join("slowest_events.csv"))
                .unwrap()
                .lines()
                .count(),
            1
        );
        let report: serde_json::Value =
            serde_json::from_slice(&fs::read(first.join("summary.json")).unwrap()).unwrap();
        assert_eq!(report["operation_statistics_count"], 0);
        assert_eq!(
            report["most_expensive_operations"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(!recorder.has_unsaved_capture());
        assert!(recorder.start(false));
        recorder.stop();
        let second = recorder.save(&folder).unwrap();
        assert_ne!(first, second);
        assert!(first.exists());
        let _ = fs::remove_dir_all(folder);
    }
}
