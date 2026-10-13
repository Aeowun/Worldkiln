// Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:00 -05:00
// Opt-in Editor CPU/GPU timing. GPU queries are read only after completion.
use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::time::Instant;

const LABELS: [&str; 9] = [
    "setup/cache",
    "shadow",
    "main setup",
    "Actors",
    "static cells",
    "opaque Terrain",
    "characters",
    "water",
    "editor overlays",
];
const POINTS: usize = 10;
const SLOTS: usize = 5;
const WINDOW: usize = 120;
#[derive(Default)]
struct GpuSlot {
    ids: [u32; POINTS],
    pending: bool,
}
#[derive(Default, Clone)]
pub(super) struct SceneCounters {
    pub(super) actors: u64,
    pub(super) by_package: BTreeMap<String, u64>,
    pub(super) terrain: super::terrain::TerrainDrawStats,
    pub(super) water: super::terrain::TerrainDrawStats,
}
pub(super) struct EditorProfiler {
    gpu_available: bool,
    slots: [GpuSlot; SLOTS],
    next: usize,
    active: Option<usize>,
    last: Instant,
    cpu_frame: [f64; 9],
    cpu: VecDeque<[f64; 9]>,
    gpu: VecDeque<[f64; 9]>,
    host: VecDeque<[f64; 5]>,
    counts: SceneCounters,
    frames: u64,
    report: Option<String>,
    hover_cpu_ms: f64,
    hover_calls: u64,
}
fn mean(values: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = values.fold((0.0, 0_usize), |(s, n), x| (s + x, n + 1));
    if n == 0 { 0.0 } else { sum / n as f64 }
}
fn p95(values: impl Iterator<Item = f64>) -> f64 {
    let mut values: Vec<f64> = values.filter(|x| x.is_finite()).collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let i = ((values.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    values[i.min(values.len() - 1)]
}
impl EditorProfiler {
    pub(super) fn new() -> Option<Self> {
        if std::env::var("AEO_EDITOR_PROFILE").as_deref() != Ok("1") {
            return None;
        }
        let gpu_available = gl::QueryCounter::is_loaded()
            && gl::GenQueries::is_loaded()
            && gl::GetQueryObjectiv::is_loaded()
            && gl::GetQueryObjectui64v::is_loaded();
        let mut result = Self {
            gpu_available,
            slots: std::array::from_fn(|_| GpuSlot::default()),
            next: 0,
            active: None,
            last: Instant::now(),
            cpu_frame: [0.0; 9],
            cpu: VecDeque::new(),
            gpu: VecDeque::new(),
            host: VecDeque::new(),
            counts: SceneCounters::default(),
            frames: 0,
            report: None,
            hover_cpu_ms: 0.0,
            hover_calls: 0,
        };
        if gpu_available {
            for slot in &mut result.slots {
                unsafe {
                    gl::GenQueries(POINTS as i32, slot.ids.as_mut_ptr());
                }
            }
        }
        eprintln!(
            "WO46 Editor profiler enabled; asynchronous GPU={gpu_available}. Log: {}",
            Self::log_path().display()
        );
        Some(result)
    }
    pub(super) fn log_path() -> std::path::PathBuf {
        std::env::temp_dir().join("Worldkiln_WO46_editor_profile.log")
    }
    fn poll(&mut self) {
        if !self.gpu_available {
            return;
        }
        for slot in &mut self.slots {
            if !slot.pending {
                continue;
            }
            let mut ready = 0;
            unsafe {
                gl::GetQueryObjectiv(slot.ids[POINTS - 1], gl::QUERY_RESULT_AVAILABLE, &mut ready);
            }
            if ready == 0 {
                continue;
            }
            let mut stamps = [0_u64; POINTS];
            unsafe {
                for (dst, id) in stamps.iter_mut().zip(slot.ids) {
                    gl::GetQueryObjectui64v(id, gl::QUERY_RESULT, dst);
                }
            }
            let mut durations = [0.0; 9];
            for i in 0..9 {
                durations[i] = stamps[i + 1].saturating_sub(stamps[i]) as f64 / 1_000_000.0;
            }
            slot.pending = false;
            self.gpu.push_back(durations);
            if self.gpu.len() > WINDOW {
                self.gpu.pop_front();
            }
        }
    }
    pub(super) fn begin(&mut self) {
        self.poll();
        self.last = Instant::now();
        self.cpu_frame = [0.0; 9];
        self.active = None;
        if self.gpu_available {
            let i = self.next;
            self.next = (i + 1) % SLOTS;
            if !self.slots[i].pending {
                self.active = Some(i);
                unsafe {
                    gl::QueryCounter(self.slots[i].ids[0], gl::TIMESTAMP);
                }
            }
        }
    }
    pub(super) fn mark(&mut self, boundary: usize) {
        debug_assert!((1..POINTS).contains(&boundary));
        let now = Instant::now();
        self.cpu_frame[boundary - 1] = (now - self.last).as_secs_f64() * 1000.0;
        self.last = now;
        if let Some(i) = self.active {
            unsafe {
                gl::QueryCounter(self.slots[i].ids[boundary], gl::TIMESTAMP);
            }
        }
    }
    pub(super) fn finish(&mut self, counts: SceneCounters) {
        self.mark(9);
        if let Some(i) = self.active.take() {
            self.slots[i].pending = true;
        }
        self.cpu.push_back(self.cpu_frame);
        if self.cpu.len() > WINDOW {
            self.cpu.pop_front();
        }
        self.counts = counts;
    }
    // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:03 -05:00
    pub(super) fn hover_call(&mut self, millis: f64) {
        self.hover_cpu_ms += millis;
        self.hover_calls += 1;
    }

    pub(super) fn host_frame(&mut self, frame: [f64; 5]) {
        self.host.push_back(frame);
        if self.host.len() > WINDOW {
            self.host.pop_front();
        }
        self.frames += 1;
        if self.frames % WINDOW as u64 != 0 {
            return;
        }
        let avg = |i: usize| mean(self.host.iter().map(|s| s[i]));
        let total = mean(self.host.iter().map(|s| s.iter().sum()));
        let p = p95(self.host.iter().map(|s| s.iter().sum()));
        let mut out = format!(
            "WO46 frame wall avg {total:.2} ms p95 {p:.2} ms (~{fps:.1} FPS)\nCPU host: update {u:.2} render {r:.2} egui layout {l:.2} egui paint {e:.2} swap {s:.2} ms\n",
            fps = if total > 0.0 { 1000.0 / total } else { 0.0 },
            u = avg(0),
            r = avg(1),
            l = avg(2),
            e = avg(3),
            s = avg(4)
        );
        out.push_str("CPU render: ");
        for (i, label) in LABELS.iter().enumerate() {
            out.push_str(&format!(
                "{label} {:.2} | ",
                mean(self.cpu.iter().map(|s| s[i]))
            ));
        }
        out.push_str("\nGPU render (delayed): ");
        if self.gpu.is_empty() {
            out.push_str("unavailable/pending");
        } else {
            for (i, label) in LABELS.iter().enumerate() {
                out.push_str(&format!(
                    "{label} {:.2} | ",
                    mean(self.gpu.iter().map(|s| s[i]))
                ));
            }
        }
        out.push_str(&format!(
            "\nEditor pointer picking {:.2}ms/frame, {:.2} calls/frame (only when invoked)",
            self.hover_cpu_ms / WINDOW as f64,
            self.hover_calls as f64 / WINDOW as f64
        ));
        self.hover_cpu_ms = 0.0;
        self.hover_calls = 0;
        out.push_str(&format!("\nActors {} | Terrain draws {} tris {} visible/culled chunks {}/{} | water draws {} tris {} | delayed GPU samples {}",
            self.counts.actors,self.counts.terrain.draws,self.counts.terrain.triangles,
            self.counts.terrain.chunks_drawn,self.counts.terrain.chunks_culled,
            self.counts.water.draws,self.counts.water.triangles,self.gpu.len()));
        // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
        out.push_str(&format!(
            "\nTerrain CPU breakdown (last frame): texture resolve {:.3} ms, glDrawArrays wall {:.3} ms; remaining Terrain CPU time includes uniforms/state/driver waits",
            self.counts.terrain.texture_lookup_ns as f64 / 1_000_000.0,
            self.counts.terrain.draw_submission_ns as f64 / 1_000_000.0
        ));
        let mut packages: Vec<_> = self.counts.by_package.iter().collect();
        packages.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        for (name, n) in packages.into_iter().take(5) {
            out.push_str(&format!("\nActor package {name}: {n}"));
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(Self::log_path())
        {
            let _ = writeln!(file, "{out}\n");
        }
        eprintln!("{out}");
        self.report = Some(out);
    }
    pub(super) fn summary(&self) -> Option<String> {
        self.report.clone()
    }
}
impl Drop for EditorProfiler {
    fn drop(&mut self) {
        if self.gpu_available {
            for slot in &self.slots {
                unsafe {
                    gl::DeleteQueries(POINTS as i32, slot.ids.as_ptr());
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregation_handles_empty_and_spikes() {
        assert_eq!(mean([].into_iter()), 0.0);
        assert_eq!(p95([].into_iter()), 0.0);
        assert_eq!(mean([2.0, 4.0, 6.0].into_iter()), 4.0);
        assert_eq!(p95((1..=100).map(|n| n as f64)), 95.0);
    }
    #[test]
    fn stage_names_are_unique_and_match_gpu_boundaries() {
        let unique: std::collections::BTreeSet<_> = LABELS.into_iter().collect();
        assert_eq!(unique.len(), POINTS - 1);
        assert_eq!(LABELS[3], "Actors");
        assert_eq!(LABELS[7], "water");
    }
    #[test]
    fn gpu_slots_begin_unblocked_and_can_represent_pending_queries() {
        let mut slot = GpuSlot::default();
        assert!(!slot.pending);
        slot.pending = true;
        assert!(slot.pending);
    }
}
