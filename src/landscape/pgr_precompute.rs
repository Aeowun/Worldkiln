// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::erosion_walkers::{ErosionJob, ErosionSettings, PgrOutcome};
use crate::world::terrain::HeightfieldData;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

pub const DEFAULT_REPLAY_BUDGET_BYTES: usize = 128 * 1024 * 1024;
const SOLVER_OPERATIONS_PER_FRAME: usize = 192;

#[derive(Clone, Debug)]
pub struct PgrReplayFrame {
    pub changes: Vec<(u32, f32)>,
    pub walkers: Vec<[u32; 2]>,
    pub chunks: Vec<[u32; 2]>,
    pub stop_refunds: Vec<(u32, f32)>,
    pub progress: f32,
    pub residual: f64,
    pub sound_magnitude: f32,
}

#[derive(Debug)]
pub struct PgrPreparedRun {
    pub origin: [i32; 3],
    pub size: [u32; 3],
    pub before: Vec<f32>,
    pub after: Vec<f32>,
    pub frames: Vec<PgrReplayFrame>,
    pub result: PgrOutcome,
    pub bytes_used: usize,
    pub initial_hash: u64,
    pub final_hash: u64,
}

#[derive(Debug)]
pub struct PgrDirectRun {
    pub origin: [i32; 3],
    pub size: [u32; 3],
    pub before: Vec<f32>,
    pub after: Vec<f32>,
    pub result: PgrOutcome,
    pub initial_hash: u64,
}
impl PgrDirectRun {
    pub fn matches_source(&self, field: &HeightfieldData) -> bool {
        self.origin == field.origin
            && self.size == field.size
            && self.before.len() == field.heights.len()
            && self.initial_hash == height_hash(&field.heights)
            && self
                .before
                .iter()
                .zip(&field.heights)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
}

#[derive(Debug)]
pub enum PgrPrecomputeUpdate {
    Running {
        simulated_frames: usize,
        changed_samples: u64,
        residual: f64,
        solver_progress: f32,
        bytes_used: usize,
    },
    Complete(PgrPreparedRun),
    DirectComplete(PgrDirectRun),
    Failed(String),
    Cancelled,
}

pub struct PgrPrecomputeHandle {
    pub updates: Receiver<PgrPrecomputeUpdate>,
    cancel: Arc<AtomicBool>,
}
impl PgrPrecomputeHandle {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Drop for PgrPrecomputeHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub fn height_hash(heights: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for value in heights {
        for octet in value.to_bits().to_le_bytes() {
            h ^= octet as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

impl PgrPreparedRun {
    pub fn matches_source(&self, field: &HeightfieldData) -> bool {
        self.origin == field.origin
            && self.size == field.size
            && self.initial_hash == height_hash(&field.heights)
            && field.heights.len() == self.before.len()
            && self
                .before
                .iter()
                .zip(&field.heights)
                .all(|(a, b)| a.to_bits() == b.to_bits())
    }
    pub fn verify(&self) -> Result<(), String> {
        let mut replay = self.before.clone();
        for frame in &self.frames {
            for &(index, height) in &frame.changes {
                let value = replay
                    .get_mut(index as usize)
                    .ok_or("PGR replay contains an out-of-range sample index")?;
                *value = height;
            }
        }
        if replay.len() != self.after.len()
            || replay
                .iter()
                .zip(&self.after)
                .any(|(a, b)| a.to_bits() != b.to_bits())
            || height_hash(&replay) != self.final_hash
        {
            return Err("PGR cache does not match the precalculated terrain exactly".into());
        }
        Ok(())
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

pub fn start_precompute(
    field: HeightfieldData,
    sea: f32,
    settings: ErosionSettings,
    budget: usize,
) -> Result<PgrPrecomputeHandle, String> {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::Builder::new()
        .name("pgr-precompute".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compute_recording(&field, sea, settings, budget, &flag, |update| {
                    tx.send(update).is_ok()
                })
            }));
            match result {
                Ok(Ok(Some(run))) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Complete(run));
                }
                Ok(Ok(None)) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Cancelled);
                }
                Ok(Err(error)) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Failed(error));
                }
                Err(_) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Failed(
                        "PGR precalculation failed unexpectedly; original terrain was not changed"
                            .into(),
                    ));
                }
            }
        })
        .map_err(|error| format!("Unable to start PGR precalculation: {error}"))?;
    Ok(PgrPrecomputeHandle {
        updates: rx,
        cancel,
    })
}

pub fn start_direct(
    field: HeightfieldData,
    sea: f32,
    settings: ErosionSettings,
) -> Result<PgrPrecomputeHandle, String> {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    std::thread::Builder::new()
        .name("pgr-denoise".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compute_direct(&field, sea, settings, &flag, |update| {
                    tx.send(update).is_ok()
                })
            }));
            match result {
                Ok(Ok(Some(run))) => {
                    let _ = tx.send(PgrPrecomputeUpdate::DirectComplete(run));
                }
                Ok(Ok(None)) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Cancelled);
                }
                Ok(Err(error)) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Failed(error));
                }
                Err(_) => {
                    let _ = tx.send(PgrPrecomputeUpdate::Failed(
                        "PGR calculation failed unexpectedly; original terrain unchanged".into(),
                    ));
                }
            }
        })
        .map_err(|error| format!("Unable to start PGR: {error}"))?;
    Ok(PgrPrecomputeHandle {
        updates: rx,
        cancel,
    })
}

fn compute_direct<F>(
    field: &HeightfieldData,
    sea: f32,
    settings: ErosionSettings,
    cancelled: &AtomicBool,
    mut on_progress: F,
) -> Result<Option<PgrDirectRun>, String>
where
    F: FnMut(PgrPrecomputeUpdate) -> bool,
{
    let mut solver = ErosionJob::new(field, sea, settings)?;
    let mut working = field.heights.clone();
    let initial_hash = height_hash(&working);
    let mut frames = 0usize;
    let mut changed_samples = 0u64;
    while !solver.is_done() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let batch = solver.advance(&mut working, SOLVER_OPERATIONS_PER_FRAME);
        frames += 1;
        changed_samples += batch.changed_indices.len() as u64;
        if frames % 96 == 0 {
            if !on_progress(PgrPrecomputeUpdate::Running {
                simulated_frames: frames,
                changed_samples,
                residual: solver.residual_status().1,
                solver_progress: solver.progress(),
                bytes_used: working.len() * std::mem::size_of::<f32>() * 2,
            }) {
                return Ok(None);
            }
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let result = solver
        .outcome()
        .cloned()
        .ok_or("PGR finished without an outcome")?;
    for (index, height) in solver.inflight_refunds(&working) {
        let Some(value) = working.get_mut(index as usize) else {
            return Err("PGR settlement returned invalid sample index".into());
        };
        *value = height;
    }
    if working.len() != field.heights.len() || working.iter().any(|h| !h.is_finite()) {
        return Err("PGR returned an invalid heightfield".into());
    }
    Ok(Some(PgrDirectRun {
        origin: field.origin,
        size: field.size,
        before: field.heights.clone(),
        after: working,
        result,
        initial_hash,
    }))
}

fn compute_recording<F>(
    field: &HeightfieldData,
    sea: f32,
    settings: ErosionSettings,
    budget: usize,
    cancelled: &AtomicBool,
    mut on_progress: F,
) -> Result<Option<PgrPreparedRun>, String>
where
    F: FnMut(PgrPrecomputeUpdate) -> bool,
{
    let mut solver = ErosionJob::new(field, sea, settings)?;
    let mut working = field.heights.clone();
    let initial_hash = height_hash(&working);
    let mut frames = Vec::<PgrReplayFrame>::new();
    let mut bytes_used = field.heights.len() * std::mem::size_of::<f32>() * 2;
    let mut edited_samples = 0u64;
    while !solver.is_done() {
        if cancelled.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let batch = solver.advance(&mut working, SOLVER_OPERATIONS_PER_FRAME);
        let changes = batch
            .changed_indices
            .iter()
            .map(|&i| (i as u32, working[i]))
            .collect::<Vec<_>>();
        edited_samples += changes.len() as u64;
        let (.., residual) = solver.residual_status();
        let walkers = solver.active_walkers();
        let chunks = solver.active_chunks();
        let stop_refunds = solver.inflight_refunds(&working);
        let frame = PgrReplayFrame {
            changes,
            walkers,
            chunks,
            stop_refunds,
            progress: solver.progress(),
            residual,
            sound_magnitude: batch.edited_magnitude,
        };
        let increment = std::mem::size_of::<PgrReplayFrame>()
            + frame.changes.len() * std::mem::size_of::<(u32, f32)>()
            + frame.walkers.len() * std::mem::size_of::<[u32; 2]>()
            + frame.chunks.len() * std::mem::size_of::<[u32; 2]>()
            + frame.stop_refunds.len() * std::mem::size_of::<(u32, f32)>();
        bytes_used = bytes_used.saturating_add(increment);
        if bytes_used > budget {
            return Err(format!(
                "PGR replay cache reached its {} MiB memory limit after {} frames; \
                 the original terrain is unchanged",
                budget / (1024 * 1024),
                frames.len()
            ));
        }
        frames.push(frame);
        if frames.len() % 96 == 0 {
            if !on_progress(PgrPrecomputeUpdate::Running {
                simulated_frames: frames.len(),
                changed_samples: edited_samples,
                residual,
                solver_progress: solver.progress(),
                bytes_used,
            }) {
                return Ok(None);
            }
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let result = solver
        .outcome()
        .cloned()
        .ok_or("PGR finished without an outcome")?;
    let settled = solver.inflight_refunds(&working);
    if !settled.is_empty() {
        let changes = settled;
        for &(i, height) in &changes {
            working[i as usize] = height;
        }
        bytes_used += std::mem::size_of::<PgrReplayFrame>()
            + changes.len() * std::mem::size_of::<(u32, f32)>();
        if bytes_used > budget {
            return Err("PGR replay memory limit reached during final settlement".into());
        }
        frames.push(PgrReplayFrame {
            changes,
            walkers: Vec::new(),
            chunks: Vec::new(),
            stop_refunds: Vec::new(),
            progress: solver.progress(),
            residual: solver.residual_status().1,
            sound_magnitude: 0.0,
        });
    }
    let run = PgrPreparedRun {
        origin: field.origin,
        size: field.size,
        before: field.heights.clone(),
        after: working,
        frames,
        result,
        bytes_used,
        initial_hash,
        final_hash: 0,
    };
    let mut run = run;
    run.final_hash = height_hash(&run.after);
    run.verify()?;
    Ok(Some(run))
}

pub struct PgrReplay {
    pub recording: PgrPreparedRun,
    cursor: usize,
}
impl PgrReplay {
    pub fn new(recording: PgrPreparedRun, field: &HeightfieldData) -> Result<Self, String> {
        recording.verify()?;
        if !recording.matches_source(field) {
            return Err("Terrain changed since PGR precalculation; re-run Denoise".into());
        }
        Ok(Self {
            recording,
            cursor: 0,
        })
    }
    pub fn apply_frame(&mut self, heights: &mut [f32]) -> Result<Option<&PgrReplayFrame>, String> {
        if self.cursor == self.recording.frames.len() {
            return Ok(None);
        }
        if heights.len() != self.recording.before.len() {
            return Err("Terrain size changed during replay".into());
        }
        let frame = &self.recording.frames[self.cursor];
        for &(i, v) in &frame.changes {
            heights[i as usize] = v;
        }
        self.cursor += 1;
        Ok(Some(frame))
    }
    pub fn frames_applied(&self) -> usize {
        self.cursor
    }
    pub fn is_finished(&self) -> bool {
        self.cursor == self.recording.frames.len()
    }
    pub fn stop_refunds(&self) -> &[(u32, f32)] {
        self.cursor
            .checked_sub(1)
            .and_then(|i| self.recording.frames.get(i))
            .map(|frame| frame.stop_refunds.as_slice())
            .unwrap_or(&[])
    }
    pub fn verify_result(&self, heights: &[f32]) -> Result<(), String> {
        if !self.is_finished() {
            return Err("PGR replay has not finished".into());
        }
        if height_hash(heights) != self.recording.final_hash
            || heights
                .iter()
                .zip(&self.recording.after)
                .any(|(a, b)| a.to_bits() != b.to_bits())
        {
            return Err("PGR playback heights differ from the precalculated result".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_denoise_runs_complete_solver_without_visual_replay() {
        let n = 48usize;
        let heights = (0..n * n)
            .map(|i| {
                let x = i % n;
                let z = i / n;
                if (9..n - 9).contains(&x) && (9..n - 9).contains(&z) {
                    34.0 + ((x * z * 29) % 19) as f32 * 0.33
                } else {
                    26.0
                }
            })
            .collect::<Vec<f32>>();
        let field = HeightfieldData {
            origin: [0, 0, 0],
            size: [n as u32, 100, n as u32],
            heights,
        };
        for deposit_mode in [false, true] {
            let settings = ErosionSettings {
                deposit_mode,
                walker_count: 8,
                ..Default::default()
            };
            let direct = compute_direct(&field, 0.0, settings, &AtomicBool::new(false), |_| true)
                .unwrap()
                .unwrap();
            let recorded = compute_recording(
                &field,
                0.0,
                settings,
                DEFAULT_REPLAY_BUDGET_BYTES,
                &AtomicBool::new(false),
                |_| true,
            )
            .unwrap()
            .unwrap();
            assert!(direct.matches_source(&field));
            assert_eq!(direct.after, recorded.after);
            assert_eq!(
                std::mem::discriminant(&direct.result),
                std::mem::discriminant(&recorded.result)
            );
            let mut edited = field.clone();
            edited.heights[0] += 1.0;
            assert!(!direct.matches_source(&edited));
        }
        assert!(
            compute_direct(
                &field,
                0.0,
                ErosionSettings::default(),
                &AtomicBool::new(true),
                |_| true
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn bit_exact_replay_matches_offline_heightfield() {
        let n = 48usize;
        let mut h = vec![26.0; n * n];
        for z in 9..n - 9 {
            for x in 9..n - 9 {
                let i = z * n + x;
                h[i] = 34.0 + ((x * z * 29) % 19) as f32 * 0.33;
            }
        }
        let field = HeightfieldData {
            origin: [0, 0, 0],
            size: [n as u32, 100, n as u32],
            heights: h.clone(),
        };
        for deposit in [false, true] {
            let settings = ErosionSettings {
                deposit_mode: deposit,
                walker_count: 8,
                ..Default::default()
            };
            let run = compute_recording(
                &field,
                0.0,
                settings,
                DEFAULT_REPLAY_BUDGET_BYTES,
                &AtomicBool::new(false),
                |_| true,
            )
            .unwrap()
            .unwrap();
            let mut playback = PgrReplay::new(run, &field).unwrap();
            let mut actual = h.clone();
            while playback.apply_frame(&mut actual).unwrap().is_some() {}
            playback.verify_result(&actual).unwrap();
            assert!(playback.is_finished());
        }
    }
    #[test]
    fn cancelled_precompute_never_mutates_its_source() {
        let n = 32usize;
        let mut heights = vec![42.0; n * n];
        heights[16 * n + 15] = 91.0;
        let source = HeightfieldData {
            origin: [0, 0, 0],
            size: [n as u32, 128, n as u32],
            heights: heights.clone(),
        };
        let cancelled = AtomicBool::new(true);
        let response = compute_recording(
            &source,
            0.0,
            ErosionSettings::default(),
            DEFAULT_REPLAY_BUDGET_BYTES,
            &cancelled,
            |_| true,
        )
        .unwrap();
        assert!(response.is_none());
        assert_eq!(source.heights, heights);
    }
    #[test]
    fn bounded_cache_refuses_insufficient_budget_without_source_edits() {
        let n = 32usize;
        let heights = vec![42.0; n * n];
        let source = HeightfieldData {
            origin: [0, 0, 0],
            size: [n as u32, 128, n as u32],
            heights: heights.clone(),
        };
        let result = compute_recording(
            &source,
            0.0,
            ErosionSettings::default(),
            64,
            &AtomicBool::new(false),
            |_| true,
        );
        assert!(result.is_err());
        assert_eq!(source.heights, heights);
    }
    #[test]
    fn source_mismatch_rejected_without_mutating_existing_terrain() {
        let field = HeightfieldData {
            origin: [0, 0, 0],
            size: [8, 16, 8],
            heights: vec![4.0; 64],
        };
        let settings = ErosionSettings {
            strength: 0,
            ..Default::default()
        };
        let run = compute_recording(
            &field,
            0.0,
            settings,
            DEFAULT_REPLAY_BUDGET_BYTES,
            &AtomicBool::new(false),
            |_| true,
        )
        .unwrap()
        .unwrap();
        let mut changed = field.clone();
        changed.heights[4] += 0.25;
        assert!(PgrReplay::new(run, &changed).is_err());
    }
}
