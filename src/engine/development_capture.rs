// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub const FORMAT_VERSION: u32 = 1;
pub const SAMPLE_LIMIT: usize = 50_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sample {
    pub id: u64,
    pub parent: Option<u64>,
    #[serde(default)]
    pub cause: Option<u64>,
    pub frame: u64,
    pub thread: String,
    pub at_ns: u64,
    pub duration_ns: u64,
    pub self_ns: Option<u64>,
    pub category: String,
    pub operation: String,
    pub script: Option<String>,
    pub entity: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Conditions {
    pub engine: String,
    pub build: String,
    pub hardware: BTreeMap<String, String>,
    pub settings: BTreeMap<String, String>,
    pub configuration: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capture {
    pub format_version: u32,
    pub conditions: Conditions,
    pub samples: Vec<Sample>,
    pub dropped: u64,
    pub warnings: Vec<String>,
}
impl Default for Capture {
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            conditions: Conditions::default(),
            samples: Vec::new(),
            dropped: 0,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    Live,
    SelectedFrame,
    SelectedRange,
    WholeCapture,
}
impl Scope {
    pub const ALL: [Self; 4] = [
        Self::Live,
        Self::SelectedFrame,
        Self::SelectedRange,
        Self::WholeCapture,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "Live",
            Self::SelectedFrame => "Selected Frame",
            Self::SelectedRange => "Selected Range",
            Self::WholeCapture => "Whole Capture",
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub scope: Scope,
    pub first: Option<u64>,
    pub last: Option<u64>,
}
impl Selection {
    pub fn contains(&self, frame: u64, latest: u64) -> bool {
        match self.scope {
            Scope::WholeCapture => true,
            Scope::Live => frame >= latest.saturating_sub(120),
            Scope::SelectedFrame => self.first == Some(frame),
            Scope::SelectedRange => self
                .first
                .zip(self.last)
                .is_some_and(|(a, b)| (a.min(b)..=a.max(b)).contains(&frame)),
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub label: String,
    pub category: String,
    pub calls: u64,
    pub total_ns: u64,
    pub self_ns: Option<u64>,
    pub worst_ns: u64,
    pub sample: Option<Sample>,
}
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub frames: Vec<Sample>,
    pub rows: Vec<Row>,
    pub timeline: Vec<Sample>,
    pub hitches: Vec<Sample>,
    pub current: [Option<f64>; 4],
    pub trends: [Vec<f32>; 4],
    pub dropped: u64,
    pub warnings: Vec<String>,
    pub latest: u64,
    pub aggregation: String,
}
impl Capture {
    pub(crate) fn write_attribution(&self, directory: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("diagnostic_events.csv"))?;
        let mut writer = std::io::BufWriter::new(file);
        writeln!(
            writer,
            "sample_id,parent_id,frame,at_us,category,operation,inclusive_us,self_us,runtime_entity,script,source,diagnostic_json"
        )?;
        let by_id: BTreeMap<_, _> = self.samples.iter().map(|s| (s.id, s)).collect();
        let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        for sample in &self.samples {
            let mut ancestor = sample;
            for _ in 0..64 {
                if ancestor.entity.is_some() {
                    break;
                }
                let Some(parent) = ancestor.parent.and_then(|id| by_id.get(&id)) else {
                    break;
                };
                ancestor = parent;
            }
            if sample.diagnostic.is_none() && ancestor.entity.is_none() {
                continue;
            }
            writeln!(
                writer,
                "{},{},{},{:.3},{},{},{:.3},{},{},{},{},{}",
                sample.id,
                sample.parent.map(|id| id.to_string()).unwrap_or_default(),
                sample.frame,
                sample.at_ns as f64 / 1000.0,
                quote(&sample.category),
                quote(&sample.operation),
                sample.duration_ns as f64 / 1000.0,
                sample
                    .self_ns
                    .map(|n| format!("{:.3}", n as f64 / 1000.0))
                    .unwrap_or_default(),
                quote(ancestor.entity.as_deref().unwrap_or("")),
                quote(sample.script.as_deref().unwrap_or("")),
                quote(sample.source.as_deref().unwrap_or("")),
                quote(
                    &sample
                        .diagnostic
                        .as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_default()
                )
            )?;
        }
        writer.flush()?;
        writer.get_ref().sync_all()
    }
    pub fn read(path: &std::path::Path) -> std::io::Result<Self> {
        if std::fs::metadata(path)?.len() > 64 * 1024 * 1024 {
            return Err(std::io::Error::other("Capture exceeds 64 MiB"));
        }
        let capture: Self =
            serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))
                .map_err(std::io::Error::other)?;
        if capture.format_version != FORMAT_VERSION {
            return Err(std::io::Error::other("Unsupported capture format version"));
        }
        if capture.samples.len() > SAMPLE_LIMIT {
            return Err(std::io::Error::other("Capture exceeds sample limit"));
        }
        let mut ids = std::collections::HashSet::new();
        if capture.samples.iter().any(|s| {
            !ids.insert(s.id)
                || s.at_ns.checked_add(s.duration_ns).is_none()
                || s.self_ns.is_some_and(|v| v > s.duration_ns)
        }) {
            return Err(std::io::Error::other(
                "Invalid capture sample identity or interval",
            ));
        }
        Ok(capture)
    }
    pub fn mismatches(&self, other: &Self) -> Vec<String> {
        let mut result = Vec::new();
        for (name, a, b) in [
            (
                "Engine version",
                serde_json::json!(self.conditions.engine),
                serde_json::json!(other.conditions.engine),
            ),
            (
                "Build",
                serde_json::json!(self.conditions.build),
                serde_json::json!(other.conditions.build),
            ),
            (
                "Hardware",
                serde_json::json!(self.conditions.hardware),
                serde_json::json!(other.conditions.hardware),
            ),
            (
                "Project settings",
                serde_json::json!(self.conditions.settings),
                serde_json::json!(other.conditions.settings),
            ),
            (
                "Capture configuration",
                self.conditions.configuration.clone(),
                other.conditions.configuration.clone(),
            ),
        ] {
            if a != b {
                result.push(format!("{name} differs"));
            }
        }
        if self.conditions.hardware.is_empty()
            || self.conditions.settings.is_empty()
            || self.conditions.build.is_empty()
        {
            result.push("Conditions incomplete; controlled comparison is unconfirmed".into());
        }
        result
    }
    pub fn snapshot(&self, selection: &Selection) -> Snapshot {
        let latest = self.samples.iter().map(|s| s.frame).max().unwrap_or(0);
        let mut result = Snapshot {
            dropped: self.dropped,
            warnings: self.warnings.clone(),
            latest,
            ..Snapshot::default()
        };
        result.frames = self
            .samples
            .iter()
            .filter(|s| s.category == "render" && s.operation == "frame")
            .rev()
            .take(240)
            .cloned()
            .collect();
        result.frames.reverse();
        let threshold = self
            .conditions
            .configuration
            .get("threshold_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(33)
            .saturating_mul(1_000_000);
        result.hitches = self
            .samples
            .iter()
            .filter(|s| {
                s.category == "render" && s.operation == "frame" && s.duration_ns > threshold
            })
            .cloned()
            .collect();
        result
            .hitches
            .sort_by(|a, b| b.duration_ns.cmp(&a.duration_ns));
        result.hitches.truncate(64);
        let mut groups: BTreeMap<(&str, &str, Option<&str>, Option<&str>), Row> = BTreeMap::new();
        for s in self
            .samples
            .iter()
            .filter(|s| selection.contains(s.frame, latest))
        {
            if s.operation == "frame" && s.category == "render" {
                continue;
            }
            let key = (
                s.category.as_str(),
                s.operation.as_str(),
                s.script.as_deref(),
                s.entity.as_deref(),
            );
            let r = groups.entry(key).or_insert_with(|| Row {
                label: format!(
                    "{}{}",
                    s.script
                        .as_ref()
                        .map(|p| format!("{p} / "))
                        .unwrap_or_default(),
                    s.operation
                ),
                category: s.category.clone(),
                self_ns: s.self_ns.map(|_| 0),
                ..Row::default()
            });
            r.calls += 1;
            r.total_ns = r.total_ns.saturating_add(s.duration_ns);
            r.self_ns = r.self_ns.zip(s.self_ns).map(|(a, b)| a.saturating_add(b));
            if r.sample.is_none() || s.duration_ns > r.worst_ns {
                r.worst_ns = s.duration_ns;
                r.sample = Some(s.clone());
            }
        }
        result.rows = groups.into_values().collect();
        result.rows.sort_by(|a, b| {
            b.total_ns
                .cmp(&a.total_ns)
                .then_with(|| a.label.cmp(&b.label))
        });
        result.rows.truncate(200);
        result.timeline = self
            .samples
            .iter()
            .rev()
            .filter(|s| selection.contains(s.frame, latest))
            .take(500)
            .cloned()
            .collect();
        result.timeline.sort_by_key(|s| (s.frame, s.at_ns, s.id));
        let trend_first = result
            .frames
            .iter()
            .map(|s| s.frame)
            .min()
            .unwrap_or(u64::MAX);
        let mut totals: BTreeMap<u64, [Option<u64>; 3]> = BTreeMap::new();
        for sample in self.samples.iter().filter(|s| {
            s.frame >= trend_first
                || (selection.scope != Scope::Live && selection.contains(s.frame, latest))
        }) {
            let slot = if sample.category == "render" && sample.operation == "editor_render_cpu" {
                Some(0)
            } else if sample.category == "gpu" {
                Some(1)
            } else if sample.category == "callback" {
                Some(2)
            } else {
                None
            };
            if let Some(i) = slot {
                let entry = totals.entry(sample.frame).or_default();
                entry[i] = Some(entry[i].unwrap_or(0).saturating_add(sample.duration_ns));
            }
        }
        for f in &result.frames {
            result.trends[0].push(f.duration_ns as f32 / 1e6);
            for i in 0..3 {
                result.trends[i + 1].push(
                    totals
                        .get(&f.frame)
                        .and_then(|v| v[i])
                        .map_or(f32::NAN, |v| v as f32 / 1e6),
                );
            }
        }
        for i in 0..4 {
            result.current[i] = result.trends[i]
                .last()
                .copied()
                .filter(|v| v.is_finite())
                .map(f64::from);
        }
        result.aggregation = "Current completed frame; measured operations only".into();
        if selection.scope != Scope::Live {
            let mut sums = [0_u128; 4];
            let mut counts = [0_u64; 4];
            for f in self.samples.iter().filter(|s| {
                s.category == "render"
                    && s.operation == "frame"
                    && selection.contains(s.frame, latest)
            }) {
                sums[0] += f.duration_ns as u128;
                counts[0] += 1;
                for i in 0..3 {
                    if let Some(value) = totals.get(&f.frame).and_then(|v| v[i]) {
                        sums[i + 1] += value as u128;
                        counts[i + 1] += 1;
                    }
                }
            }
            for i in 0..4 {
                result.current[i] = if counts[i] > 0 {
                    Some(sums[i] as f64 / counts[i] as f64 / 1e6)
                } else {
                    None
                };
            }
            result.aggregation = "Average per measured frame in selected scope (ms)".into();
        }
        result
    }
    pub fn inspect(&self, selection: &Selection, selected: Option<&Sample>) -> Snapshot {
        let mut snapshot = self.snapshot(selection);
        if let Some(selected) = selected {
            snapshot.timeline = self
                .samples
                .iter()
                .filter(|s| s.frame == selected.frame)
                .take(2048)
                .cloned()
                .collect();
            snapshot.timeline.sort_by_key(|s| (s.at_ns, s.id));
            let mut next = selected.cause.or(selected.parent);
            for _ in 0..32 {
                let Some(id) = next else {
                    break;
                };
                let Some(caller) = self.samples.iter().find(|s| s.id == id) else {
                    break;
                };
                if !snapshot.timeline.iter().any(|s| s.id == id) {
                    snapshot.timeline.push(caller.clone());
                }
                next = caller.cause.or(caller.parent);
            }
            for child in self
                .samples
                .iter()
                .filter(|s| s.cause == Some(selected.id))
                .take(256)
            {
                if !snapshot.timeline.iter().any(|s| s.id == child.id) {
                    snapshot.timeline.push(child.clone());
                }
            }
        }
        snapshot
    }
    pub fn report(&self) -> String {
        let mut frames: Vec<u64> = self
            .samples
            .iter()
            .filter(|s| s.category == "render" && s.operation == "frame")
            .map(|s| s.duration_ns)
            .collect();
        frames.sort_unstable();
        let p = |percent: usize| {
            frames
                .get(frames.len().saturating_sub(1) * percent / 100)
                .map_or("Unavailable".into(), |n| {
                    format!("{:.3} ms", *n as f64 / 1e6)
                })
        };
        let mut report = format!(
            "Worldkiln diagnostics\nEngine: {}\nFrame P50: {}\nFrame P95: {}\nFrame P99: {}\nDropped samples: {}\nTimes are inclusive wall time; nested rows must not be added. Unmeasured time is unexplained.\n",
            self.conditions.engine,
            p(50),
            p(95),
            p(99),
            self.dropped
        );
        for warning in &self.warnings {
            report.push_str(&format!("Warning: {warning}\n"));
        }
        let threshold = self
            .conditions
            .configuration
            .get("threshold_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(33)
            .saturating_mul(1_000_000);
        for s in self
            .samples
            .iter()
            .filter(|s| {
                s.category == "render" && s.operation == "frame" && s.duration_ns > threshold
            })
            .take(64)
        {
            report.push_str(&format!(
                "Hitch: frame {} | {:.3} ms\n",
                s.frame,
                s.duration_ns as f64 / 1e6
            ));
        }
        for r in self
            .snapshot(&Selection {
                scope: Scope::WholeCapture,
                ..Selection::default()
            })
            .rows
            .iter()
            .take(30)
        {
            report.push_str(&format!(
                "Measured: {} | {} calls | total {:.3} ms | worst {:.3} ms\n",
                r.label,
                r.calls,
                r.total_ns as f64 / 1e6,
                r.worst_ns as f64 / 1e6
            ));
        }
        report
    }
}

struct OpenSpan {
    sample: Sample,
    start: Instant,
    children: Duration,
}
#[derive(Default)]
struct Trace {
    enabled: bool,
    detailed: bool,
    script: Option<String>,
    entity: Option<String>,
    source: Option<String>,
    next: u64,
    stack: Vec<OpenSpan>,
    done: Vec<(Sample, Instant)>,
    dropped: u64,
}
thread_local! { static TRACE:RefCell<Trace>=RefCell::new(Trace::default()); }
pub(crate) fn begin(detailed: bool) {
    TRACE.with(|v| {
        let mut t = v.borrow_mut();
        t.enabled = true;
        t.detailed = detailed;
        t.done.clear();
        t.stack.clear();
        t.dropped = 0;
    });
}
pub(crate) fn context(script: Option<&str>, entity: Option<&str>, source: Option<String>) {
    TRACE.with(|v| {
        let mut t = v.borrow_mut();
        if t.enabled {
            t.script = script.map(|s| s.chars().take(512).collect());
            t.entity = entity.map(|s| s.chars().take(192).collect());
            t.source = source.map(|s| s.chars().take(256).collect());
        }
    });
}
pub(crate) struct Span {
    active: bool,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
pub(crate) fn span(category: &str, operation: &str, detailed_only: bool) -> Span {
    let active = TRACE.with(|v| {
        let mut t = v.borrow_mut();
        if !t.enabled || (detailed_only && !t.detailed) {
            return false;
        }
        if t.done.len() + t.stack.len() >= 10_000 {
            t.dropped += 1;
            return false;
        }
        t.next = t.next.saturating_add(1);
        let sample = Sample {
            id: t.next,
            parent: t.stack.last().map(|s| s.sample.id),
            cause: None,
            frame: 0,
            thread: format!("{:?}", std::thread::current().id()),
            at_ns: 0,
            duration_ns: 0,
            self_ns: None,
            category: category.into(),
            operation: operation.chars().take(192).collect(),
            script: t.script.clone(),
            entity: t.entity.clone(),
            asset: None,
            source: t.source.clone(),
            diagnostic: None,
        };
        t.stack.push(OpenSpan {
            sample,
            start: Instant::now(),
            children: Duration::ZERO,
        });
        true
    });
    Span {
        active,
        _thread: std::marker::PhantomData,
    }
}
impl Span {
    pub(crate) fn target(&self, entity: impl FnOnce() -> String) {
        if self.active {
            TRACE.with(|v| {
                if let Some(span) = v.borrow_mut().stack.last_mut() {
                    span.sample.entity = Some(entity());
                }
            });
        }
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        TRACE.with(|v| {
            let mut t = v.borrow_mut();
            if let Some(mut s) = t.stack.pop() {
                let end = Instant::now();
                let duration = end.saturating_duration_since(s.start);
                if let Some(parent) = t.stack.last_mut() {
                    parent.children += duration;
                }
                s.sample.duration_ns = ns(duration);
                s.sample.self_ns = Some(ns(duration.saturating_sub(s.children)));
                t.done.push((s.sample, s.start));
                if t.stack.is_empty() {
                    t.script = None;
                    t.entity = None;
                    t.source = None;
                }
            }
        });
    }
}
pub(crate) fn finish() -> (Vec<(Sample, Instant)>, u64) {
    TRACE.with(|v| {
        let mut t = v.borrow_mut();
        t.enabled = false;
        t.script = None;
        t.entity = None;
        t.source = None;
        (std::mem::take(&mut t.done), t.dropped)
    })
}
pub(crate) fn ns(v: Duration) -> u64 {
    v.as_nanos().min(u64::MAX as u128) as u64
}

use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
static ENGINE_GENERATION: AtomicU64 = AtomicU64::new(0);
static ENGINE_DETAIL: AtomicBool = AtomicBool::new(false);
static ENGINE_FRAME: AtomicU64 = AtomicU64::new(0);
static ENGINE_NEXT: AtomicU64 = AtomicU64::new(0);
static ENGINE_DROPPED: AtomicU64 = AtomicU64::new(0);
static ENGINE_QUEUE: Mutex<Vec<(Sample, Instant)>> = Mutex::new(Vec::new());
thread_local! {static ENGINE_STACK:RefCell<Vec<(u64,Duration)>>=RefCell::new(Vec::new());}
pub(crate) fn engine_session(generation: u64, detailed: bool) {
    ENGINE_GENERATION.store(0, Ordering::Release);
    if let Ok(mut queue) = ENGINE_QUEUE.lock() {
        queue.clear();
    }
    ENGINE_DROPPED.store(0, Ordering::Relaxed);
    ENGINE_DETAIL.store(detailed, Ordering::Relaxed);
    ENGINE_GENERATION.store(generation, Ordering::Release);
}
pub(crate) fn engine_frame(frame: u64) {
    ENGINE_FRAME.store(frame, Ordering::Relaxed);
}
pub(crate) fn drain_engine() -> (Vec<(Sample, Instant)>, u64) {
    let samples = ENGINE_QUEUE
        .try_lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default();
    (samples, ENGINE_DROPPED.swap(0, Ordering::Relaxed))
}
pub(crate) struct Operation {
    local: Option<Span>,
    sample: Option<Sample>,
    start: Instant,
    generation: u64,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub(crate) fn detailed_active() -> bool {
    ENGINE_GENERATION.load(Ordering::Acquire) != 0 && ENGINE_DETAIL.load(Ordering::Relaxed)
}

pub(crate) fn measured(
    category: &str,
    name: &str,
    started: Instant,
    elapsed: Duration,
    entity: Option<u64>,
    facts: serde_json::Value,
) {
    if !detailed_active() {
        return;
    }
    let sample = Sample {
        id: (1 << 62) | ENGINE_NEXT.fetch_add(1, Ordering::Relaxed),
        parent: None,
        cause: None,
        frame: ENGINE_FRAME.load(Ordering::Relaxed),
        thread: format!("{:?}", std::thread::current().id()),
        at_ns: 0,
        duration_ns: ns(elapsed),
        self_ns: None,
        category: category.into(),
        operation: name.into(),
        script: None,
        entity: entity.map(|id| id.to_string()),
        asset: None,
        source: None,
        diagnostic: Some(facts),
    };
    if let Ok(mut queue) = ENGINE_QUEUE.try_lock() {
        if queue.len() < 10_000 {
            queue.push((sample, started));
            return;
        }
    }
    ENGINE_DROPPED.fetch_add(1, Ordering::Relaxed);
}

pub(crate) struct PairTimings {
    enabled: bool,
    rows: Vec<(Duration, Instant, u64, u64, &'static str)>,
    pub visits: u64,
}
impl Default for PairTimings {
    fn default() -> Self {
        Self {
            enabled: detailed_active(),
            rows: Vec::new(),
            visits: 0,
        }
    }
}
pub(crate) struct PairTimer<'a> {
    owner: &'a mut PairTimings,
    start: Option<Instant>,
    first: u64,
    second: u64,
    kind: &'static str,
}
impl PairTimings {
    pub(crate) fn measure(&mut self, first: u64, second: u64, kind: &'static str) -> PairTimer<'_> {
        let start = self.enabled.then(Instant::now);
        self.visits += u64::from(start.is_some());
        PairTimer {
            owner: self,
            start,
            first,
            second,
            kind,
        }
    }
    pub(crate) fn emit(self, describe: impl Fn(u64, u64) -> serde_json::Value) {
        for (elapsed, start, first, second, kind) in self.rows {
            measured(
                "physics",
                kind,
                start,
                elapsed,
                Some(first),
                describe(first, second),
            );
        }
    }
}
impl Drop for PairTimer<'_> {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        let row = (start.elapsed(), start, self.first, self.second, self.kind);
        if self.owner.rows.len() < 8 {
            self.owner.rows.push(row);
        } else if let Some((index, smallest)) =
            self.owner.rows.iter().enumerate().min_by_key(|(_, r)| r.0)
        {
            if row.0 > smallest.0 {
                self.owner.rows[index] = row;
            }
        }
    }
}
pub(crate) fn operation(category: &str, name: &str, detailed_only: bool) -> Option<Operation> {
    let generation = ENGINE_GENERATION.load(Ordering::Acquire);
    if generation == 0 {
        return None;
    }
    if detailed_only && !ENGINE_DETAIL.load(Ordering::Relaxed) {
        return None;
    }
    let start = Instant::now();
    if TRACE.with(|v| v.borrow().enabled) {
        return Some(Operation {
            local: Some(span(category, name, detailed_only)),
            sample: None,
            start,
            generation,
            _thread: std::marker::PhantomData,
        });
    }
    let id = (1 << 62) | ENGINE_NEXT.fetch_add(1, Ordering::Relaxed);
    let parent = ENGINE_STACK.with(|v| {
        let mut stack = v.borrow_mut();
        let parent = stack.last().map(|(id, _)| *id);
        stack.push((id, Duration::ZERO));
        parent
    });
    Some(Operation {
        local: None,
        start,
        generation,
        _thread: std::marker::PhantomData,
        sample: Some(Sample {
            id,
            parent,
            cause: None,
            frame: ENGINE_FRAME.load(Ordering::Relaxed),
            thread: format!("{:?}", std::thread::current().id()),
            at_ns: 0,
            duration_ns: 0,
            self_ns: None,
            category: category.into(),
            operation: name.into(),
            script: None,
            entity: None,
            asset: None,
            source: None,
            diagnostic: None,
        }),
    })
}
impl Operation {
    pub(crate) fn character_identity(self, character: u64, entity: Option<u64>) -> Self {
        let span = self.diagnostic(serde_json::json!({"character_id": character}));
        match entity {
            Some(id) => span.entity(id),
            None => span,
        }
    }
    pub(crate) fn diagnostic(mut self, facts: serde_json::Value) -> Self {
        if let Some(sample) = &mut self.sample {
            sample.diagnostic = Some(facts);
        } else if self.local.as_ref().is_some_and(|span| span.active) {
            TRACE.with(|v| {
                if let Some(span) = v.borrow_mut().stack.last_mut() {
                    span.sample.diagnostic = Some(facts);
                }
            });
        }
        self
    }
    pub(crate) fn caused_by(mut self, correlation: Option<&Correlation>) -> Self {
        if let Some(c) = correlation.filter(|c| c.generation == self.generation) {
            if let Some(s) = &mut self.sample {
                c.apply(s);
            } else if self.local.as_ref().is_some_and(|s| s.active) {
                TRACE.with(|v| {
                    if let Some(s) = v.borrow_mut().stack.last_mut() {
                        c.apply(&mut s.sample);
                    }
                });
            }
        }
        self
    }
    pub(crate) fn entity(mut self, id: u64) -> Self {
        let identity = id.to_string();
        if let Some(sample) = &mut self.sample {
            sample.entity = Some(identity);
        } else if self.local.is_some() {
            TRACE.with(|v| {
                if let Some(span) = v.borrow_mut().stack.last_mut() {
                    span.sample.entity = Some(identity);
                }
            });
        }
        self
    }
    pub(crate) fn asset(mut self, path: &std::path::Path) -> Self {
        let identity = path.file_name().map(|p| p.to_string_lossy().into_owned());
        if let Some(sample) = &mut self.sample {
            sample.asset = identity;
        } else if self.local.is_some() {
            TRACE.with(|v| {
                if let Some(span) = v.borrow_mut().stack.last_mut() {
                    span.sample.asset = identity;
                }
            });
        }
        self
    }
}

#[derive(Clone)]
pub(crate) struct Correlation {
    generation: u64,
    request: u64,
    script: Option<String>,
    entity: Option<String>,
    source: Option<String>,
}
impl Correlation {
    fn apply(&self, s: &mut Sample) {
        s.cause = Some(self.request);
        s.script = self.script.clone();
        s.entity = self.entity.clone();
        s.source = self.source.clone();
    }
}
pub(crate) fn correlation() -> Option<Correlation> {
    let generation = ENGINE_GENERATION.load(Ordering::Acquire);
    if generation == 0 {
        return None;
    }
    TRACE.with(|v| {
        let t = v.borrow();
        t.stack.last().map(|s| Correlation {
            generation,
            request: s.sample.id | (1 << 63),
            script: s.sample.script.clone(),
            entity: s.sample.entity.clone(),
            source: s.sample.source.clone(),
        })
    })
}
impl Drop for Operation {
    fn drop(&mut self) {
        drop(self.local.take());
        let Some(mut sample) = self.sample.take() else {
            return;
        };
        let elapsed = self.start.elapsed();
        let children = ENGINE_STACK.with(|v| {
            let mut stack = v.borrow_mut();
            let children = stack.pop().map_or(Duration::ZERO, |(_, children)| children);
            if let Some((_, children)) = stack.last_mut() {
                *children += elapsed;
            }
            children
        });
        if ENGINE_GENERATION.load(Ordering::Acquire) != self.generation {
            return;
        }
        sample.duration_ns = ns(elapsed);
        sample.self_ns = Some(ns(elapsed.saturating_sub(children)));
        if let Ok(mut queue) = ENGINE_QUEUE.try_lock() {
            if queue.len() < 10_000 {
                queue.push((sample, self.start));
                return;
            }
        }
        ENGINE_DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_attribution_roundtrip_and_csv_inherit_entity() {
        let mut capture = Capture::default();
        let parent = Sample {
            id: 1,
            parent: None,
            cause: None,
            frame: 3,
            thread: "test".into(),
            at_ns: 0,
            duration_ns: 2000,
            self_ns: Some(1000),
            category: "character".into(),
            operation: "owner".into(),
            script: None,
            entity: Some("42".into()),
            asset: None,
            source: None,
            diagnostic: Some(serde_json::json!({"name": "Actor, \"quoted\""})),
        };
        let mut child = parent.clone();
        child.id = 2;
        child.parent = Some(1);
        child.entity = None;
        child.operation = "movement".into();
        child.duration_ns = 1000;
        child.self_ns = None;
        child.diagnostic = None;
        capture.samples = vec![parent, child];
        let encoded = serde_json::to_value(&capture).unwrap();
        assert!(encoded["samples"][1].get("diagnostic").is_none());
        let decoded: Capture = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            decoded.samples[0].diagnostic.as_ref().unwrap()["name"],
            "Actor, \"quoted\""
        );
        let directory = std::env::temp_dir().join(format!(
            "worldkiln-diagnostic-attribution-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let result = capture.write_attribution(&directory);
        let output = std::fs::read_to_string(directory.join("diagnostic_events.csv"));
        std::fs::remove_file(directory.join("diagnostic_events.csv")).unwrap();
        std::fs::remove_dir(&directory).unwrap();
        result.unwrap();
        let output = output.unwrap();
        assert!(output.contains("2,1,3,0.000,\"character\",\"movement\",1.000,,\"42\""));
        assert!(output.contains("\"\"name\"\""));
    }

    #[test]
    fn diagnostic_attribution_pair_retention_is_bounded_and_disabled_is_empty() {
        let mut disabled = PairTimings {
            enabled: false,
            rows: Vec::new(),
            visits: 0,
        };
        drop(disabled.measure(1, 2, "pair"));
        assert_eq!(disabled.visits, 0);
        assert!(disabled.rows.is_empty());
        let mut enabled = PairTimings {
            enabled: true,
            rows: Vec::new(),
            visits: 0,
        };
        for second in 0..100 {
            drop(enabled.measure(1, second, "pair"));
        }
        assert_eq!(enabled.visits, 100);
        assert_eq!(enabled.rows.len(), 8);
    }
    #[test]
    #[ignore = "process-wide recorder integration; run alone"]
    fn asynchronous_request_links_worker_without_subtracting_worker_time() {
        engine_session(91, true);
        engine_frame(7);
        begin(true);
        context(
            Some("scripts/request.aeo"),
            Some("Actor:42"),
            Some("on_update".into()),
        );
        let request = span("native", "find_path", false);
        let token = correlation().expect("active request");
        drop(request);
        let (callers, _) = finish();
        let request = &callers[0].0;
        let request_id = request.id | (1 << 63);
        let original_self = request.self_ns;
        engine_frame(9);
        std::thread::spawn(move || {
            let _build = operation("navigation", "navmesh_build_worker", false)
                .unwrap()
                .caused_by(Some(&token));
            std::thread::sleep(Duration::from_millis(3));
        })
        .join()
        .unwrap();
        let (samples, dropped) = drain_engine();
        engine_session(0, false);
        assert_eq!(dropped, 0);
        let worker = &samples[0].0;
        assert_eq!(worker.cause, Some(request_id));
        assert_eq!(worker.parent, None);
        assert_eq!(worker.frame, 9);
        assert_eq!(worker.script.as_deref(), Some("scripts/request.aeo"));
        assert_eq!(worker.entity.as_deref(), Some("Actor:42"));
        assert_ne!(worker.thread, request.thread);
        assert!(worker.duration_ns >= 3_000_000);
        assert_eq!(request.self_ns, original_self);
        assert_eq!(request.self_ns, Some(request.duration_ns));
    }
    #[test]
    fn nested_native_spans_preserve_request_and_exclusive_time() {
        begin(true);
        context(
            Some("scripts/a.aeo"),
            Some("Entity:42"),
            Some("on_update".into()),
        );
        {
            let _callback = span("callback", "on_update", false);
            {
                let _native = span("native", "find_path", false);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let (samples, dropped) = finish();
        assert_eq!(dropped, 0);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].0.parent, Some(samples[1].0.id));
        assert_eq!(samples[0].0.script.as_deref(), Some("scripts/a.aeo"));
        assert_eq!(
            samples[1].0.self_ns.unwrap() + samples[0].0.duration_ns,
            samples[1].0.duration_ns
        );
        assert!(!span("native", "disabled", false).active);
    }
    #[test]
    fn trace_is_bounded_and_detailed_is_opt_in() {
        begin(false);
        drop(span("native", "detail", true));
        assert!(finish().0.is_empty());
        begin(true);
        for _ in 0..10_020 {
            drop(span("native", "test", true));
        }
        let (samples, dropped) = finish();
        assert_eq!(samples.len(), 10_000);
        assert_eq!(dropped, 20);
    }
    #[test]
    fn scope_ranges_are_shared_and_handle_reverse_selection() {
        let s = Selection {
            scope: Scope::SelectedRange,
            first: Some(8),
            last: Some(3),
        };
        assert!(s.contains(5, 50));
        assert!(!s.contains(9, 50));
        assert!(
            !Selection {
                scope: Scope::SelectedFrame,
                ..Selection::default()
            }
            .contains(0, 0)
        );
    }
    #[test]
    fn snapshot_preserves_scoped_averages_trends_and_missing_gpu_samples() {
        let mut capture = Capture::default();
        for frame in 1..=300 {
            let mut add = |category: &str, operation: &str, ms: u64| {
                capture.samples.push(Sample {
                    id: capture.samples.len() as u64 + 1,
                    parent: None,
                    cause: None,
                    frame,
                    thread: "test".into(),
                    at_ns: frame * 16_000_000,
                    duration_ns: ms * 1_000_000,
                    self_ns: None,
                    category: category.into(),
                    operation: operation.into(),
                    script: None,
                    entity: None,
                    asset: None,
                    source: None,
                    diagnostic: None,
                });
            };
            add("render", "frame", 16);
            add(
                "render",
                "editor_render_cpu",
                if frame == 1 { 9 } else { 1 },
            );
            if frame < 300 {
                add("gpu", "water", 2);
                add("gpu", "main setup", 3);
            }
            add("callback", "on_update", 4);
        }
        let live = capture.snapshot(&Selection::default());
        assert_eq!(live.current, [Some(16.0), Some(1.0), None, Some(4.0)]);
        assert_eq!(live.frames.len(), 240);
        assert_eq!(live.trends[1], vec![1.0; 240]);
        assert!(live.trends[2].last().unwrap().is_nan());
        assert!(live.trends[2][..239].iter().all(|v| *v == 5.0));
        assert_eq!(
            live.rows
                .iter()
                .find(|r| r.label == "on_update")
                .unwrap()
                .calls,
            121
        );
        let whole = capture.snapshot(&Selection {
            scope: Scope::WholeCapture,
            ..Selection::default()
        });
        assert!((whole.current[1].unwrap() - 308.0 / 300.0).abs() < 1e-12);
        assert_eq!(whole.current[2], Some(5.0));
        let range = capture.snapshot(&Selection {
            scope: Scope::SelectedRange,
            first: Some(2),
            last: Some(1),
        });
        assert_eq!(range.current, [Some(16.0), Some(5.0), Some(5.0), Some(4.0)]);
        assert_eq!(range.trends[1], live.trends[1]);
        let selected = capture.snapshot(&Selection {
            scope: Scope::SelectedFrame,
            first: Some(1),
            last: None,
        });
        assert_eq!(selected.current[1], Some(9.0));
    }

    #[test]
    fn comparisons_disclose_mismatched_and_incomplete_conditions() {
        let mut first = Capture::default();
        assert!(!first.mismatches(&first).is_empty());
        first.conditions.build = "build-a".into();
        first
            .conditions
            .hardware
            .insert("gpu".into(), "device".into());
        first
            .conditions
            .settings
            .insert("scene".into(), "conditions".into());
        assert!(first.mismatches(&first).is_empty());
        let mut second = first.clone();
        second.conditions.configuration = serde_json::json!({"detailed":true});
        assert_eq!(
            first.mismatches(&second),
            vec!["Capture configuration differs"]
        );
    }
}

#[cfg(test)]
mod saved_capture_benchmark {
    use super::*;

    #[test]
    #[ignore = "manual saved-capture benchmark; WORLDKILN_CAPTURE_BENCH_PATH required"]
    fn benchmark_saved_capture_workspace() {
        let path = std::env::var("WORLDKILN_CAPTURE_BENCH_PATH").unwrap();
        let capture = Capture::read(std::path::Path::new(&path)).unwrap();
        for scope in [Scope::Live, Scope::WholeCapture] {
            let selection = Selection {
                scope,
                ..Selection::default()
            };
            let started = Instant::now();
            for _ in 0..30 {
                std::hint::black_box(capture.inspect(&selection, None));
            }
            println!(
                "saved capture {scope:?}: {:.3} ms/inspect; {} samples",
                started.elapsed().as_secs_f64() * 1000.0 / 30.0,
                capture.samples.len()
            );
        }
    }
}
