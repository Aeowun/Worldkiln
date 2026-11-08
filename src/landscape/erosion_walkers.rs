// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::world::terrain::HeightfieldData;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErosionSettings {
    pub strength: u8,
    pub chunk_size: u32,
    pub walk_randomness: u8,
    pub walker_count: u32,
    pub deposit_mode: bool,
    pub seed: u64,
}
impl Default for ErosionSettings {
    fn default() -> Self {
        Self {
            strength: 45,
            chunk_size: 32,
            walk_randomness: 20,
            walker_count: 64,
            deposit_mode: true,
            seed: 0x574B_4C41_4E44_0130,
        }
    }
}
#[derive(Default, Debug)]
pub struct ErosionBatch {
    pub changed_indices: Vec<usize>,
    pub edited_magnitude: f32,
}
impl ErosionBatch {
    fn changed(&mut self, i: usize, delta: f32) {
        if delta.abs() > 0.000001 {
            if !self.changed_indices.contains(&i) {
                self.changed_indices.push(i);
            }
            self.edited_magnitude += delta.abs();
        }
    }
}
#[derive(Clone, Debug)]
pub enum PgrOutcome {
    Converged,
    Stopped,
    Stalled(String),
}

#[derive(Clone, Copy, Default)]
enum Phase {
    #[default]
    SeekPeak,
    SeekHole,
}
#[derive(Clone, Default)]
struct Walker {
    position: usize,
    peak: usize,
    hole: Option<usize>,
    region: usize,
    carrying: f32,
    phase: Phase,
    route: Vec<usize>,
    deposit_route: Vec<usize>,
    active: bool,
}
#[derive(Default)]
struct Region {
    peaks: BinaryHeap<(u32, usize)>,
    holes: BinaryHeap<(u32, usize)>,
}

pub struct ErosionJob {
    width: usize,
    depth: usize,
    source: Vec<f32>,
    target: Vec<f32>,
    protected: Vec<bool>,
    labels: Vec<usize>,
    regions: Vec<Region>,
    reserved: Vec<bool>,
    settings: ErosionSettings,
    sea: f32,
    height_ceiling: f32,
    terrain_span: f32,
    prepared: bool,
    walkers: Vec<Walker>,
    next_walker: usize,
    rng: u64,
    initial_error: f64,
    remaining_error: f64,
    remaining_samples: usize,
    completed_transfers: u64,
    run_ticks: u64,
    best_error: f64,
    last_improvement_tick: u64,
    outcome: Option<PgrOutcome>,
}
impl ErosionJob {
    pub fn new(
        field: &HeightfieldData,
        sea: f32,
        mut settings: ErosionSettings,
    ) -> Result<Self, String> {
        let width = field.size[0] as usize;
        let depth = field.size[2] as usize;
        let len = width.checked_mul(depth).ok_or("PGR heightfield overflow")?;
        if width < 2
            || depth < 2
            || len > 512 * 512
            || field.heights.len() != len
            || !sea.is_finite()
            || field.heights.iter().any(|v| !v.is_finite())
        {
            return Err("PGR requires a finite heightfield up to 512 x 512 samples".into());
        }
        settings.strength = settings.strength.min(100);
        settings.chunk_size = settings.chunk_size.clamp(8, 128);
        settings.walk_randomness = settings.walk_randomness.min(100);
        settings.walker_count = settings.walker_count.clamp(1, 512);
        let mut land = field
            .heights
            .iter()
            .step_by(3)
            .copied()
            .filter(|v| *v > sea)
            .collect::<Vec<_>>();
        land.sort_by(|a, b| a.total_cmp(b));
        let upper = land
            .get(land.len().saturating_mul(95) / 100)
            .copied()
            .unwrap_or(sea + 1.0);
        let lower = land
            .get(land.len().saturating_mul(5) / 100)
            .copied()
            .unwrap_or(sea);
        let terrain_span = (upper - lower).max(upper - sea).max(1.0);
        let highest = field.heights.iter().copied().fold(sea, f32::max);
        let height_ceiling =
            highest.max(field.origin[1] as f32 + field.size[1].saturating_sub(1) as f32);
        Ok(Self {
            width,
            depth,
            source: field.heights.clone(),
            target: field.heights.clone(),
            protected: vec![true; len],
            labels: vec![0; len],
            regions: vec![Region::default()],
            reserved: vec![false; len],
            settings,
            sea,
            height_ceiling,
            terrain_span,
            prepared: false,
            walkers: vec![Walker::default(); settings.walker_count as usize],
            next_walker: 0,
            rng: settings.seed.max(1),
            initial_error: 0.0,
            remaining_error: 0.0,
            remaining_samples: 0,
            completed_transfers: 0,
            run_ticks: 0,
            best_error: f64::INFINITY,
            last_improvement_tick: 0,
            outcome: (settings.strength == 0).then_some(PgrOutcome::Converged),
        })
    }
    fn random(&mut self) -> u64 {
        let mut s = self.rng;
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        self.rng = s;
        s.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn index(&self, x: usize, z: usize) -> usize {
        z * self.width + x
    }

    fn median_scale(&self, input: &[f32], radius: usize) -> Vec<f32> {
        let mut output = input.to_vec();
        if self.width <= radius * 2 || self.depth <= radius * 2 {
            return output;
        }
        for z in radius..self.depth - radius {
            for x in radius..self.width - radius {
                let i = self.index(x, z);
                if self.source[i] <= self.sea {
                    continue;
                }
                let mut sample = [0.0f32; 9];
                let mut k = 0;
                for dz in [-1isize, 0, 1] {
                    for dx in [-1isize, 0, 1] {
                        let j = self.index(
                            (x as isize + dx * radius as isize) as usize,
                            (z as isize + dz * radius as isize) as usize,
                        );
                        sample[k] = input[j];
                        k += 1;
                    }
                }
                sample.sort_by(|a, b| a.total_cmp(b));
                output[i] = sample[4];
            }
        }
        output
    }
    fn smooth_scale(&self, input: &[f32], radius: usize) -> Vec<f32> {
        let mut output = input.to_vec();
        if self.width <= 2 * radius || self.depth <= 2 * radius {
            return output;
        }
        for z in radius..self.depth - radius {
            for x in radius..self.width - radius {
                let i = self.index(x, z);
                if self.source[i] <= self.sea {
                    continue;
                }
                let center = input[i];
                let span = self.terrain_span;
                let cut = (span * 0.20).max(1.0);
                let mut sum = center * 4.0;
                let mut weight = 4.0;
                for (dx, dz) in [
                    (-1isize, 0isize),
                    (1, 0),
                    (0, -1),
                    (0, 1),
                    (-1, -1),
                    (1, -1),
                    (-1, 1),
                    (1, 1),
                ] {
                    let j = self.index(
                        (x as isize + dx * radius as isize) as usize,
                        (z as isize + dz * radius as isize) as usize,
                    );
                    if self.source[j] <= self.sea {
                        continue;
                    }
                    let distance = (input[j] - center).abs();
                    let w = if dx == 0 || dz == 0 { 1.0 } else { 0.7 };
                    let trust = if distance > cut { cut / distance } else { 1.0 };
                    sum += w * trust * input[j];
                    weight += w * trust;
                }
                output[i] = sum / weight;
            }
        }
        output
    }
    fn ridge_gp(
        &self,
        filtered: &[f32],
        coarse: &[f32],
        i: usize,
        slope_x: f32,
        slope_z: f32,
    ) -> (f32, f32) {
        let length = (slope_x * slope_x + slope_z * slope_z).sqrt();
        let tangent = if length > 1e-6 {
            (-slope_z / length, slope_x / length)
        } else {
            (1.0, 0.0)
        };
        let kernel = |dx: f32, dz: f32| {
            let along = dx * tangent.0 + dz * tangent.1;
            let across = -dx * tangent.1 + dz * tangent.0;
            (-0.5 * (along * along / (5.5 * 5.5) + across * across / (2.4 * 2.4))).exp()
        };
        let offsets = [(-2isize, 0isize), (2, 0), (0, -2), (0, 2)];
        let mut covariance = [0.0f32; 4];
        let mut residual = [0.0f32; 4];
        let mut matrix = [[0.0f32; 4]; 4];
        for a in 0..4 {
            let (dx, dz) = offsets[a];
            let j = (i as isize + dz * self.width as isize + dx) as usize;
            residual[a] = filtered[j] - coarse[j];
            covariance[a] = kernel(dx as f32, dz as f32);
            for b in 0..=a {
                let (ex, ez) = offsets[b];
                matrix[a][b] = kernel((dx - ex) as f32, (dz - ez) as f32);
                if a == b {
                    let observation_variance = (residual[a].abs() / self.terrain_span).min(2.0);
                    matrix[a][b] += 0.14 + observation_variance * 0.4;
                }
            }
        }
        let mut lower = [[0.0f32; 4]; 4];
        for i in 0..4 {
            for j in 0..=i {
                let mut v = matrix[i][j];
                for k in 0..j {
                    v -= lower[i][k] * lower[j][k];
                }
                lower[i][j] = if i == j {
                    v.max(1e-5).sqrt()
                } else {
                    v / lower[j][j]
                };
            }
        }
        let mut posterior = [0.0f32; 4];
        let mut normalized = [0.0f32; 4];
        for i in 0..4 {
            let mut v = residual[i];
            let mut c = covariance[i];
            for j in 0..i {
                v -= lower[i][j] * posterior[j];
                c -= lower[i][j] * normalized[j];
            }
            posterior[i] = v / lower[i][i];
            normalized[i] = c / lower[i][i];
        }
        for i in (0..4).rev() {
            let mut v = posterior[i];
            for j in i + 1..4 {
                v -= lower[j][i] * posterior[j];
            }
            posterior[i] = v / lower[i][i];
        }
        let mean = coarse[i] + (0..4).map(|j| covariance[j] * posterior[j]).sum::<f32>();
        let variance = (1.0 - normalized.iter().map(|v| v * v).sum::<f32>()).clamp(0.0, 1.0);
        (mean, variance)
    }
    fn build_reconstruction(&mut self) {
        let count = self.source.len();
        if self.outcome.is_some() {
            self.prepared = true;
            return;
        }

        let a = self.median_scale(&self.source, 1);
        let b = self.median_scale(&a, 2);
        let c = self.smooth_scale(&b, 3);
        let d = self.smooth_scale(&c, 7);
        let e = self.smooth_scale(&d, 13);
        let span = self.terrain_span;

        let mut reference_noise = Vec::new();
        for z in 2..self.depth.saturating_sub(2) {
            for x in 2..self.width.saturating_sub(2) {
                let i = self.index(x, z);
                if self.source[i] <= self.sea {
                    continue;
                }
                let neighborhood = (self.source[i - 1] - self.source[i + 1]).abs()
                    + (self.source[i - self.width] - self.source[i + self.width]).abs();
                if neighborhood < span * 0.055 {
                    reference_noise.push((self.source[i] - a[i]).abs());
                }
            }
        }
        reference_noise.sort_by(|x, y| x.total_cmp(y));
        let baseline = reference_noise
            .get(reference_noise.len() * 3 / 4)
            .copied()
            .unwrap_or(span * 0.002)
            .max(span * 0.0004);
        let mut mountain = vec![false; count];
        let mut full_model = self.source.clone();

        for z in 15..self.depth.saturating_sub(15) {
            for x in 15..self.width.saturating_sub(15) {
                let i = self.index(x, z);
                if self.source[i] <= self.sea {
                    continue;
                }
                let mut safe = true;
                for (dx, dz) in [(0isize, 0isize), (-3, 0), (3, 0), (0, -3), (0, 3)] {
                    let j = self.index((x as isize + dx) as usize, (z as isize + dz) as usize);
                    if self.source[j] <= self.sea {
                        safe = false;
                        break;
                    }
                }
                if !safe {
                    continue;
                }
                let macro_gradient = (e[i - 7] - e[i + 7]).abs()
                    + (e[i - 7 * self.width] - e[i + 7 * self.width]).abs();
                let crest_gradient = (d[i - 3] - d[i + 3]).abs()
                    + (d[i - 3 * self.width] - d[i + 3 * self.width]).abs();
                let relief = (c[i] - e[i]).abs();
                let broad_relief = macro_gradient * 0.5 + crest_gradient * 0.5 + relief;
                let is_mountain = broad_relief > span * 0.018
                    || (c[i] - self.sea > span * 0.06 && crest_gradient > span * 0.007);
                if !is_mountain {
                    continue;
                }
                let local_band = (self.source[i - 1] - self.source[i + 1]).abs()
                    + (self.source[i - self.width] - self.source[i + self.width]).abs();
                if (self.source[i] - a[i]).abs() < baseline * 1.2 && local_band < span * 0.007 {
                    continue;
                }
                let river_limit = span * 0.025;
                let along_z = (self.source[i - self.width] - self.source[i]).abs()
                    < river_limit * 0.4
                    && (self.source[i + self.width] - self.source[i]).abs() < river_limit * 0.4;
                let along_x = (self.source[i - 1] - self.source[i]).abs() < river_limit * 0.4
                    && (self.source[i + 1] - self.source[i]).abs() < river_limit * 0.4;
                if (along_z
                    && self.source[i - 1] > self.source[i] + river_limit
                    && self.source[i + 1] > self.source[i] + river_limit)
                    || (along_x
                        && self.source[i - self.width] > self.source[i] + river_limit
                        && self.source[i + self.width] > self.source[i] + river_limit)
                {
                    continue;
                }

                let slope_x = (d[i + 1] - d[i - 1]) * 0.5;
                let slope_z = (d[i + self.width] - d[i - self.width]) * 0.5;
                let slope = (slope_x * slope_x + slope_z * slope_z).sqrt();
                let tangent = if slope > baseline {
                    (-slope_z / slope, -0.0, slope_x / slope)
                } else {
                    (1.0, 0.0, 0.0)
                };
                let ridge_alignment = (tangent.0.abs() * (d[i + 1] - d[i - 1]).abs()
                    + tangent.2.abs() * (d[i + self.width] - d[i - self.width]).abs())
                .min(span);

                let broad = d[i] * 0.42 + e[i] * 0.22 + c[i] * 0.36;
                let fine = b[i] * 0.55 + a[i] * 0.45;
                let anomaly = (self.source[i] - fine).abs();
                let outlier_weight = (anomaly / (anomaly + baseline * 2.0)).clamp(0.0, 1.0);
                let ridge_weight =
                    (ridge_alignment / (ridge_alignment + span * 0.02)).clamp(0.0, 0.40);
                full_model[i] =
                    broad * (0.56 + 0.26 * outlier_weight) + fine * (0.44 - 0.26 * outlier_weight);
                let (posterior, variance) = self.ridge_gp(&c, &d, i, slope_x, slope_z);
                full_model[i] = full_model[i] * 0.78
                    + posterior * 0.22 * (1.0 - variance * 0.5)
                    + full_model[i] * 0.22 * variance * 0.5;
                full_model[i] += ridge_weight * (c[i] - e[i]) * 0.20;
                mountain[i] = true;
            }
        }

        for _ in 0..8 {
            let old = mountain.clone();
            for z in 15..self.depth.saturating_sub(15) {
                for x in 15..self.width.saturating_sub(15) {
                    let i = self.index(x, z);
                    if old[i] || self.source[i] <= self.sea {
                        continue;
                    }
                    let mut inland = true;
                    for (dx, dz) in [(0isize, 0isize), (-3, 0), (3, 0), (0, -3), (0, 3)] {
                        let j = self.index((x as isize + dx) as usize, (z as isize + dz) as usize);
                        if self.source[j] <= self.sea {
                            inland = false;
                            break;
                        }
                    }
                    if !inland {
                        continue;
                    }
                    if [i - 1, i + 1, i - self.width, i + self.width]
                        .iter()
                        .any(|&j| old[j])
                    {
                        mountain[i] = true;
                        let robust = b[i] * 0.50 + c[i] * 0.30 + d[i] * 0.20;
                        full_model[i] = robust;
                    }
                }
            }
        }
        let mut interior = mountain.clone();
        for z in 1..self.depth.saturating_sub(1) {
            for x in 1..self.width.saturating_sub(1) {
                let i = self.index(x, z);
                if !mountain[i] {
                    continue;
                }
                let adjacent = [i - 1, i + 1, i - self.width, i + self.width];
                if adjacent.iter().filter(|&&j| mountain[j]).count() < 3 {
                    interior[i] = false;
                }
            }
        }
        let fraction = self.settings.strength as f32 / 100.0;
        let epsilon = (span * 0.000015).max(0.0001);
        for i in 0..count {
            if !interior[i] {
                continue;
            }
            let difference = full_model[i] - self.source[i];
            if difference.abs() < baseline.max(epsilon) * 0.70 {
                continue;
            }
            self.protected[i] = false;
            self.target[i] = (self.source[i] + difference * fraction)
                .clamp(self.sea + 0.0001, self.height_ceiling);
            if !self.settings.deposit_mode {
                self.target[i] = self.target[i].min(self.source[i]);
            }
        }

        let mut labels = vec![0usize; count];
        let mut next_region = 0;
        let mut queue = VecDeque::new();
        for i in 0..count {
            if !interior[i] || labels[i] != 0 {
                continue;
            }
            next_region += 1;
            labels[i] = next_region;
            queue.push_back(i);
            while let Some(j) = queue.pop_front() {
                let x = j % self.width;
                let z = j / self.width;
                for (dx, dz) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                    let nx = x as isize + dx;
                    let nz = z as isize + dz;
                    if nx < 0 || nz < 0 || nx >= self.width as isize || nz >= self.depth as isize {
                        continue;
                    }
                    let k = self.index(nx as usize, nz as usize);
                    if interior[k] && labels[k] == 0 {
                        labels[k] = next_region;
                        queue.push_back(k);
                    }
                }
            }
        }
        self.labels = labels;
        if self.settings.deposit_mode {
            let mut by_region = vec![Vec::<usize>::new(); next_region + 1];
            for i in 0..count {
                if !self.protected[i] && self.labels[i] != 0 {
                    by_region[self.labels[i]].push(i);
                }
            }
            for cells in by_region.iter().skip(1) {
                if cells.is_empty() {
                    continue;
                }
                for _ in 0..12 {
                    let signed = cells
                        .iter()
                        .map(|&i| (self.source[i] - self.target[i]) as f64)
                        .sum::<f64>();
                    if signed.abs() <= self.terrain_span as f64 * 0.000001 {
                        break;
                    }
                    let upward = signed > 0.0;
                    let candidates = cells
                        .iter()
                        .copied()
                        .filter(|&i| {
                            if upward {
                                self.target[i] < self.height_ceiling - 0.0001
                            } else {
                                self.target[i] > self.sea + 0.0002
                            }
                        })
                        .collect::<Vec<_>>();
                    if candidates.is_empty() {
                        break;
                    }
                    let shift = (signed / candidates.len() as f64) as f32;
                    for i in candidates {
                        self.target[i] =
                            (self.target[i] + shift).clamp(self.sea + 0.0001, self.height_ceiling);
                    }
                }
            }
        }
        self.regions = (0..=next_region).map(|_| Region::default()).collect();
        self.prepared = true;
        self.rebuild_heaps(&self.source.clone());
        if self.remaining_samples == 0 {
            self.outcome = Some(PgrOutcome::Converged);
        }
    }
    fn positive(&self, heights: &[f32], i: usize) -> f32 {
        if self.protected[i] {
            return 0.0;
        }
        (heights[i] - self.target[i]).max(0.0)
    }
    fn negative(&self, heights: &[f32], i: usize) -> f32 {
        if self.protected[i] || !self.settings.deposit_mode {
            return 0.0;
        }
        (self.target[i] - heights[i]).max(0.0)
    }
    fn rebuild_heaps(&mut self, heights: &[f32]) {
        self.remaining_error = 0.0;
        self.remaining_samples = 0;
        for region in &mut self.regions {
            region.peaks.clear();
            region.holes.clear();
        }
        let epsilon = self.terrain_span * 0.000015;
        for i in 0..heights.len() {
            let excess = self.positive(heights, i);
            let deficit = self.negative(heights, i);
            let value = excess + deficit;
            if value > epsilon {
                self.remaining_error += value as f64;
                self.remaining_samples += 1;
            }
            if self.reserved[i] {
                continue;
            }
            if excess > epsilon {
                self.regions[self.labels[i]]
                    .peaks
                    .push((excess.to_bits(), i));
            }
            if deficit > epsilon {
                self.regions[self.labels[i]]
                    .holes
                    .push((deficit.to_bits(), i));
            }
        }
        if self.initial_error == 0.0 {
            self.initial_error = self.remaining_error;
        }
    }
    fn peek_available(&mut self, region: usize, hole: bool, heights: &[f32]) -> Option<f32> {
        let epsilon = self.terrain_span * 0.000015;
        let heap = if hole {
            &mut self.regions[region].holes
        } else {
            &mut self.regions[region].peaks
        };
        while let Some((bits, i)) = heap.peek().copied() {
            let value = if hole {
                (self.target[i] - heights[i]).max(0.0)
            } else {
                (heights[i] - self.target[i]).max(0.0)
            };
            if self.reserved[i] || value <= epsilon || bits != value.to_bits() {
                heap.pop();
                if !self.reserved[i] && value > epsilon {
                    heap.push((value.to_bits(), i));
                }
                continue;
            }
            return Some(value);
        }
        None
    }
    fn acquire(&mut self, heights: &[f32], hole: bool) -> Option<(usize, usize)> {
        let epsilon = self.terrain_span * 0.000015;
        let mut best = None;
        let mut amount = epsilon;
        for region_id in 1..self.regions.len() {
            let heap = if hole {
                &mut self.regions[region_id].holes
            } else {
                &mut self.regions[region_id].peaks
            };
            while let Some((bits, i)) = heap.peek().copied() {
                let value = if hole {
                    (self.target[i] - heights[i]).max(0.0)
                } else {
                    (heights[i] - self.target[i]).max(0.0)
                };
                if self.reserved[i] || value <= epsilon || bits != value.to_bits() {
                    heap.pop();
                    if !self.reserved[i] && value > epsilon {
                        heap.push((value.to_bits(), i));
                    }
                } else {
                    if value > amount {
                        amount = value;
                        best = Some((region_id, i));
                    }
                    break;
                }
            }
        }
        best
    }
    fn take(&mut self, region: usize, hole: bool, heights: &[f32]) -> Option<usize> {
        let epsilon = self.terrain_span * 0.000015;
        let heap = if hole {
            &mut self.regions[region].holes
        } else {
            &mut self.regions[region].peaks
        };
        while let Some((bits, i)) = heap.pop() {
            let value = if hole {
                (self.target[i] - heights[i]).max(0.0)
            } else {
                (heights[i] - self.target[i]).max(0.0)
            };
            if self.reserved[i] {
                continue;
            }
            if value <= epsilon {
                continue;
            }
            if bits != value.to_bits() {
                heap.push((value.to_bits(), i));
                continue;
            }
            self.reserved[i] = true;
            return Some(i);
        }
        None
    }
    fn release(&mut self, i: usize, heights: &[f32]) {
        self.reserved[i] = false;
        let region = self.labels[i];
        if region == 0 {
            return;
        }
        let excess = self.positive(heights, i);
        let deficit = self.negative(heights, i);
        let epsilon = self.terrain_span * 0.000015;
        if excess > epsilon {
            self.regions[region].peaks.push((excess.to_bits(), i));
        }
        if deficit > epsilon {
            self.regions[region].holes.push((deficit.to_bits(), i));
        }
    }
    fn set(&mut self, heights: &mut [f32], i: usize, new: f32, batch: &mut ErosionBatch) {
        if self.protected[i] || !new.is_finite() {
            return;
        }
        let before = heights[i];
        let old_error = self.positive(heights, i) + self.negative(heights, i);
        heights[i] = new.clamp(self.sea + 0.0001, self.height_ceiling);
        let new_error = self.positive(heights, i) + self.negative(heights, i);
        self.remaining_error = (self.remaining_error + (new_error - old_error) as f64).max(0.0);
        let epsilon = self.terrain_span * 0.000015;
        if old_error > epsilon && new_error <= epsilon {
            self.remaining_samples = self.remaining_samples.saturating_sub(1);
        } else if old_error <= epsilon && new_error > epsilon {
            self.remaining_samples += 1;
        }
        batch.changed(i, heights[i] - before);
    }
    fn route(&self, start: usize, end: usize, region: usize) -> Option<Vec<usize>> {
        if start == end {
            return Some(Vec::new());
        }
        let mut here = start;
        let mut direct = Vec::new();
        for _ in 0..self.width + self.depth {
            if here == end {
                direct.reverse();
                return Some(direct);
            }
            let (x, z) = (here % self.width, here / self.width);
            let dx = (end % self.width).cmp(&x) as i32;
            let dz = (end / self.width).cmp(&z) as i32;
            let next_x = (x as i32 + dx) as usize;
            let next_z = (z as i32 + dz) as usize;
            let next = self.index(next_x, next_z);
            if self.labels[next] != region
                || (dx != 0
                    && dz != 0
                    && (self.labels[self.index(next_x, z)] != region
                        || self.labels[self.index(x, next_z)] != region))
            {
                break;
            }
            direct.push(next);
            here = next;
        }
        let mut open = BinaryHeap::new();
        let mut parents = std::collections::HashMap::<usize, usize>::new();
        let mut distances = std::collections::HashMap::<usize, u32>::new();
        let (gx, gz) = (end % self.width, end / self.width);
        let heuristic = |i: usize| {
            let dx = (i % self.width).abs_diff(gx) as u32;
            let dz = (i / self.width).abs_diff(gz) as u32;
            10 * dx.max(dz) + 4 * dx.min(dz)
        };
        distances.insert(start, 0);
        open.push((Reverse(heuristic(start)), start));
        let mut examined = 0usize;
        while let Some((Reverse(_), i)) = open.pop() {
            examined += 1;
            if examined > 80_000 {
                return None;
            }
            if i == end {
                let mut route = Vec::new();
                let mut here = end;
                while here != start {
                    route.push(here);
                    here = *parents.get(&here)?;
                }
                return Some(route);
            }
            let old = *distances.get(&i).unwrap_or(&u32::MAX);
            if old == u32::MAX {
                continue;
            }
            let x = i % self.width;
            let z = i / self.width;
            for (dx, dz) in [
                (-1isize, 0isize),
                (1, 0),
                (0, -1),
                (0, 1),
                (-1, -1),
                (1, -1),
                (-1, 1),
                (1, 1),
            ] {
                let nx = x as isize + dx;
                let nz = z as isize + dz;
                if nx < 0 || nz < 0 || nx >= self.width as isize || nz >= self.depth as isize {
                    continue;
                }
                let j = self.index(nx as usize, nz as usize);
                if self.labels[j] != region {
                    continue;
                }
                if dx != 0 && dz != 0 {
                    if self.labels[self.index(nx as usize, z)] != region
                        || self.labels[self.index(x, nz as usize)] != region
                    {
                        continue;
                    }
                }
                let step = if dx == 0 || dz == 0 { 10u32 } else { 14 };
                let hash = (j as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ self.settings.seed;
                let variation = ((hash ^ (hash >> 30)) % 10) as u32
                    * self.settings.walk_randomness as u32
                    / 100;
                let tentative = old.saturating_add(step + variation);
                if tentative < *distances.get(&j).unwrap_or(&u32::MAX) {
                    distances.insert(j, tentative);
                    parents.insert(j, i);
                    open.push((Reverse(tentative.saturating_add(heuristic(j))), j));
                }
            }
        }
        None
    }
    fn spawn(&mut self, slot: usize, heights: &[f32]) -> bool {
        let mut selected = None;
        let mut max = f32::NEG_INFINITY;
        for r in 1..self.regions.len() {
            if self.settings.deposit_mode && self.peek_available(r, true, heights).is_none() {
                continue;
            }
            if let Some(val) = self.peek_available(r, false, heights) {
                if val > max {
                    max = val;
                    selected = Some(r);
                }
            }
        }
        let Some(region) = selected else {
            return false;
        };
        let Some(peak) = self.take(region, false, heights) else {
            return false;
        };
        let hole = if self.settings.deposit_mode {
            self.take(region, true, heights)
        } else {
            None
        };
        if self.settings.deposit_mode && hole.is_none() {
            self.release(peak, heights);
            return false;
        }
        let transfer_path = hole.and_then(|h| self.route(peak, h, region));
        if hole.is_some() && transfer_path.is_none() {
            self.release(peak, heights);
            if let Some(h) = hole {
                self.release(h, heights);
            }
            return false;
        }

        let mut position = peak;
        for _ in 0..4 {
            let x = position % self.width;
            let z = position / self.width;
            let mut neighbors = [usize::MAX; 4];
            let mut n = 0;
            for (dx, dz) in [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)] {
                let nx = x as isize + dx;
                let nz = z as isize + dz;
                if nx < 0 || nz < 0 || nx >= self.width as isize || nz >= self.depth as isize {
                    continue;
                }
                let j = self.index(nx as usize, nz as usize);
                if self.labels[j] == region && j != peak {
                    neighbors[n] = j;
                    n += 1;
                }
            }
            if n == 0 {
                break;
            }
            position = neighbors[self.random() as usize % n];
        }
        let approach = self.route(position, peak, region).unwrap_or_default();
        self.walkers[slot] = Walker {
            position,
            peak,
            hole,
            region,
            carrying: 0.0,
            phase: Phase::SeekPeak,
            route: approach,
            deposit_route: transfer_path.unwrap_or_default(),
            active: true,
        };
        true
    }
    fn refund(&mut self, w: &Walker, heights: &mut [f32], batch: &mut ErosionBatch) {
        if w.carrying <= 0.0 {
            return;
        }
        self.set(heights, w.peak, heights[w.peak] + w.carrying, batch);
    }
    fn finish(&mut self, slot: usize, heights: &[f32]) {
        let w = std::mem::take(&mut self.walkers[slot]);
        self.release(w.peak, heights);
        if let Some(h) = w.hole {
            self.release(h, heights);
        }
        self.completed_transfers += 1;
    }
    fn step(&mut self, slot: usize, heights: &mut [f32], batch: &mut ErosionBatch) {
        if !self.walkers[slot].active {
            return;
        }
        if let Some(next) = self.walkers[slot].route.pop() {
            self.walkers[slot].position = next;
            return;
        }
        let w = self.walkers[slot].clone();
        match w.phase {
            Phase::SeekPeak => {
                if w.position != w.peak {
                    self.finish(slot, heights);
                    return;
                }
                let excess = self.positive(heights, w.peak);
                if excess <= 0.0 {
                    self.finish(slot, heights);
                    return;
                }
                if let Some(h) = w.hole {
                    let deficit = self.negative(heights, h);
                    let delta = excess
                        .min(deficit)
                        .min((self.height_ceiling - heights[h]).max(0.0));
                    if delta <= 0.000001 {
                        self.finish(slot, heights);
                        return;
                    }
                    let route = std::mem::take(&mut self.walkers[slot].deposit_route);
                    let before = heights[w.peak];
                    self.set(heights, w.peak, before - delta, batch);
                    let actual = (before - heights[w.peak]).max(0.0);
                    if actual <= 0.0 {
                        self.finish(slot, heights);
                        return;
                    }
                    self.walkers[slot].carrying = actual;
                    self.walkers[slot].phase = Phase::SeekHole;
                    self.walkers[slot].route = route;
                } else {
                    self.set(heights, w.peak, self.target[w.peak], batch);
                    self.finish(slot, heights);
                }
            }
            Phase::SeekHole => {
                if let Some(h) = w.hole {
                    if w.position == h {
                        let before = heights[h];
                        self.set(heights, h, before + w.carrying, batch);
                        let accepted = (heights[h] - before).max(0.0);
                        if (accepted - w.carrying).abs() > 0.00001 {
                            let unsettled = w.carrying - accepted;
                            self.set(heights, w.peak, heights[w.peak] + unsettled, batch);
                        }
                        self.walkers[slot].carrying = 0.0;
                    } else {
                        self.refund(&w, heights, batch);
                    }
                } else {
                    self.refund(&w, heights, batch);
                }
                self.finish(slot, heights);
            }
        }
    }
    fn audit(&mut self, heights: &[f32]) {
        self.rebuild_heaps(heights);
        if self.remaining_samples == 0 || self.remaining_error < 0.00001 {
            self.outcome = Some(PgrOutcome::Converged);
            return;
        }
        let mut donors = 0.0f64;
        let mut holes = 0.0f64;
        for i in 0..heights.len() {
            donors += self.positive(heights, i) as f64;
            holes += self.negative(heights, i) as f64;
        }
        if self.settings.deposit_mode {
            if donors <= 1e-6 || holes <= 1e-6 {
                self.outcome = Some(PgrOutcome::Stalled(format!(
                    "PGR cannot reach target by conservation: excess {donors:.2}, \
                     unfilled holes {holes:.2} elevation units."
                )));
            } else if self
                .regions
                .iter()
                .skip(1)
                .all(|r| r.peaks.is_empty() || r.holes.is_empty())
            {
                self.outcome = Some(PgrOutcome::Stalled(format!(
                    "PGR has {donors:.2} excess and {holes:.2} deficit, \
                     but no donor/hole pair shares an eligible mountain region."
                )));
            }
        } else if donors <= 1e-6 {
            self.outcome = Some(PgrOutcome::Converged);
        }
    }
    fn update_liveness(&mut self, heights: &[f32], effective_error: f64) {
        self.run_ticks = self.run_ticks.saturating_add(1);
        if self.run_ticks % 1024 != 0 || self.outcome.is_some() {
            return;
        }
        let integral_tol = self.terrain_span as f64 * heights.len() as f64 * 0.00002;
        if effective_error <= integral_tol {
            let max_diff = (0..heights.len())
                .filter(|&i| !self.protected[i])
                .map(|i| (heights[i] - self.target[i]).abs())
                .fold(0.0f32, f32::max);
            if max_diff <= self.terrain_span * 0.0005 {
                self.outcome = Some(PgrOutcome::Converged);
                return;
            }
        }
        let min_change = if self.best_error.is_finite() {
            (self.best_error * 0.01).max(self.terrain_span as f64 * 0.0001)
        } else {
            0.0
        };
        if effective_error < self.best_error - min_change {
            self.best_error = effective_error;
            self.last_improvement_tick = self.run_ticks;
        }
        if self.run_ticks.saturating_sub(self.last_improvement_tick) >= 8192 {
            self.outcome = Some(PgrOutcome::Stalled(format!(
                "PGR stalled: {:.3} height-area residual remains across {} samples;                  no measurable convergence over 8192 solver updates.",
                effective_error, self.remaining_samples
            )));
        }
    }
    pub fn advance(&mut self, heights: &mut [f32], operations: usize) -> ErosionBatch {
        let mut batch = ErosionBatch::default();
        if self.outcome.is_some() || heights.len() != self.source.len() {
            return batch;
        }
        if !self.prepared {
            self.build_reconstruction();
            return batch;
        }
        let n = operations.max(1).min(512);
        let credited_before =
            self.remaining_error + self.walkers.iter().map(|w| w.carrying as f64).sum::<f64>();
        let mut spawned = 0usize;
        let spawn_limit = (self.walkers.len() / 8).clamp(4, 16);
        let mut worked = false;
        for _ in 0..n.min(self.walkers.len()) {
            let slot = self.next_walker % self.walkers.len();
            self.next_walker = self.next_walker.wrapping_add(1);
            if !self.walkers[slot].active {
                if spawned >= spawn_limit {
                    continue;
                }
                if !self.spawn(slot, heights) {
                    continue;
                }
                spawned += 1;
            }
            self.step(slot, heights, &mut batch);
            worked = true;
        }
        if self.walkers.iter().all(|w| !w.active) && !worked {
            self.audit(heights);
            if self.outcome.is_none() {
                self.outcome=Some(PgrOutcome::Stalled(
                    "PGR has remaining whole-map peak/hole error, but cannot form another valid route or transfer.".into()));
            }
        }
        let credited_after =
            self.remaining_error + self.walkers.iter().map(|w| w.carrying as f64).sum::<f64>();
        self.update_liveness(heights, credited_after);
        batch.edited_magnitude = (credited_before - credited_after).max(0.0) as f32;
        batch
    }
    pub fn stop(&mut self, heights: &mut [f32]) -> ErosionBatch {
        let mut batch = ErosionBatch::default();
        if heights.len() != self.source.len() {
            return batch;
        }
        for i in 0..self.walkers.len() {
            let w = std::mem::take(&mut self.walkers[i]);
            if w.carrying > 0.0 {
                self.refund(&w, heights, &mut batch);
            }
            if w.active {
                self.release(w.peak, heights);
                if let Some(h) = w.hole {
                    self.release(h, heights);
                }
            }
        }
        self.rebuild_heaps(heights);
        self.outcome = Some(PgrOutcome::Stopped);
        batch
    }
    pub fn cancel(&mut self, heights: &mut [f32]) -> ErosionBatch {
        let batch = self.stop(heights);
        self.outcome = Some(PgrOutcome::Stalled("Cancelled".into()));
        batch
    }
    pub fn progress(&self) -> f32 {
        if matches!(self.outcome, Some(PgrOutcome::Converged)) {
            return 1.0;
        }
        if !self.prepared || self.initial_error <= 1e-8 {
            return 0.0;
        }
        let carrying = self.walkers.iter().map(|w| w.carrying as f64).sum::<f64>();
        let error = (self.remaining_error + carrying).max(0.0);
        (1.0 - error / self.initial_error).clamp(0.0, 0.9999) as f32
    }
    pub fn is_done(&self) -> bool {
        self.outcome.is_some()
    }
    pub fn is_classifying(&self) -> bool {
        !self.prepared
    }
    pub fn residual_status(&self) -> (usize, f64) {
        let carrying = self.walkers.iter().map(|w| w.carrying as f64).sum::<f64>();
        (self.remaining_samples, self.remaining_error + carrying)
    }
    pub fn outcome(&self) -> Option<&PgrOutcome> {
        self.outcome.as_ref()
    }
    pub fn active_walkers(&self) -> Vec<[u32; 2]> {
        self.walkers
            .iter()
            .filter(|w| w.active)
            .map(|w| {
                [
                    (w.position % self.width) as u32,
                    (w.position / self.width) as u32,
                ]
            })
            .collect()
    }
    pub fn active_chunks(&self) -> Vec<[u32; 2]> {
        let size = crate::world::terrain::terrain_render_chunk_samples(1.0) as usize;
        let mut chunks = Vec::new();
        for w in self.walkers.iter().filter(|w| w.active) {
            let c = [
                (w.position % self.width / size) as u32,
                (w.position / self.width / size) as u32,
            ];
            if !chunks.contains(&c) {
                chunks.push(c);
            }
        }
        chunks
    }
    pub fn inflight_refunds(&self, heights: &[f32]) -> Vec<(u32, f32)> {
        self.walkers
            .iter()
            .filter(|w| w.active && w.carrying > 0.0)
            .map(|w| {
                (
                    w.peak as u32,
                    (heights[w.peak] + w.carrying).clamp(self.sea + 0.0001, self.height_ceiling),
                )
            })
            .collect()
    }
    pub fn settings(&self) -> ErosionSettings {
        self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(data: Vec<f32>, width: u32) -> HeightfieldData {
        HeightfieldData {
            origin: [0, 0, 0],
            size: [width, 100, width],
            heights: data,
        }
    }
    fn run(data: &[f32], settings: ErosionSettings, budget: usize) -> Vec<f32> {
        let f = field(data.to_vec(), (data.len() as f64).sqrt() as u32);
        let mut out = data.to_vec();
        let mut job = ErosionJob::new(&f, 0.0, settings).unwrap();
        for _ in 0..100_000 {
            if job.is_done() {
                return out;
            }
            job.advance(&mut out, budget);
        }
        panic!("PGR did not converge or report a reason for stall");
    }
    fn target_fixture(deposit: bool) -> (ErosionJob, Vec<f32>, usize, usize) {
        let width = 64usize;
        let peak = 32 * width + 24;
        let hole = 32 * width + 40;
        let mut heights = vec![50.0; width * width];
        heights[peak] = 80.0;
        heights[hole] = 20.0;
        let map = field(heights.clone(), width as u32);
        let mut job = ErosionJob::new(
            &map,
            0.0,
            ErosionSettings {
                strength: 100,
                deposit_mode: deposit,
                walker_count: 16,
                ..Default::default()
            },
        )
        .unwrap();
        job.prepared = true;
        job.labels.fill(1);
        job.regions = vec![Region::default(), Region::default()];
        job.protected.fill(true);
        job.protected[peak] = false;
        job.protected[hole] = false;
        job.target[peak] = 60.0;
        job.target[hole] = 40.0;
        job.rebuild_heaps(&heights);
        (job, heights, peak, hole)
    }
    #[test]
    fn deposit_transfers_from_highest_excess_to_actual_hole() {
        let (mut job, mut heights, peak, hole) = target_fixture(true);
        let before = heights.iter().map(|v| *v as f64).sum::<f64>();
        for _ in 0..500 {
            if job.is_done() {
                break;
            }
            job.advance(&mut heights, 64);
        }
        assert!(
            matches!(job.outcome(), Some(PgrOutcome::Converged)),
            "eligible donor and hole must converge: {:?}",
            job.outcome()
        );
        assert!((heights[peak] - 60.0).abs() < 0.001);
        assert!((heights[hole] - 40.0).abs() < 0.001);
        let after = heights.iter().map(|v| *v as f64).sum::<f64>();
        assert!((before - after).abs() < 0.001);
    }
    #[test]
    fn delete_mode_only_removes_excess_never_fills_holes() {
        let (mut job, mut heights, peak, hole) = target_fixture(false);
        for _ in 0..500 {
            if job.is_done() {
                break;
            }
            job.advance(&mut heights, 64);
        }
        assert!(matches!(job.outcome(), Some(PgrOutcome::Converged)));
        assert!((heights[peak] - 60.0).abs() < 0.001);
        assert_eq!(heights[hole], 20.0);
    }

    #[test]
    fn highest_available_target_refreshes_after_reservation() {
        let width = 32usize;
        let a = width * 12 + 11;
        let b = width * 12 + 20;
        let heights = vec![45.0; width * width];
        let field = field(heights.clone(), width as u32);
        let mut job = ErosionJob::new(
            &field,
            0.0,
            ErosionSettings {
                deposit_mode: false,
                walker_count: 2,
                ..Default::default()
            },
        )
        .unwrap();
        job.prepared = true;
        job.protected.fill(true);
        job.labels.fill(1);
        job.regions = vec![Region::default(), Region::default()];
        for &i in &[a, b] {
            job.protected[i] = false;
        }
        job.target[a] = 15.0;
        job.target[b] = 25.0;
        job.rebuild_heaps(&heights);
        assert!(job.spawn(0, &heights));
        assert_eq!(job.walkers[0].peak, a);
        assert!(job.spawn(1, &heights));
        assert_eq!(job.walkers[1].peak, b);
        assert_eq!(job.progress(), 0.0);
        assert!(job.remaining_error > 0.0);
    }

    #[test]
    fn progress_is_integral_of_remaining_target_difference() {
        let (mut job, mut heights, peak, hole) = target_fixture(true);
        assert_eq!(job.progress(), 0.0);
        let before = job.initial_error;
        let mut changes = ErosionBatch::default();
        let reduced = heights[peak] - 5.0;
        job.set(&mut heights, peak, reduced, &mut changes);
        assert!((job.remaining_error - (before - 5.0)).abs() < 0.01);
        assert!(job.progress() > 0.0 && job.progress() < 1.0);
        job.walkers[0].carrying = 5.0;
        assert!(job.progress() <= 0.000001);
        job.walkers[0].carrying = 0.0;
        let raised = heights[hole] + 5.0;
        job.set(&mut heights, hole, raised, &mut changes);
        assert!(job.progress() > 0.0);
    }
    #[test]
    fn zero_strength_never_edits() {
        let a = vec![20.0; 64];
        assert_eq!(
            run(
                &a,
                ErosionSettings {
                    strength: 0,
                    ..Default::default()
                },
                73
            ),
            a
        );
    }
    #[test]
    fn fixed_seed_ignores_frame_batching() {
        let w = 32;
        let a = (0..w * w)
            .map(|i| 20.0 + ((i * 73 % 29) as f32))
            .collect::<Vec<_>>();
        let settings = ErosionSettings {
            walker_count: 8,
            ..Default::default()
        };
        assert_eq!(run(&a, settings, 37), run(&a, settings, 193));
    }
    #[test]
    fn deposit_mode_conserves_terrain_mass() {
        let w = 32;
        let mut a = vec![50.0; w * w];
        for z in 8..24 {
            for x in 8..24 {
                a[z * w + x] = 70.0 + ((x * 13 + z * 7) % 9) as f32;
            }
        }
        let after = run(
            &a,
            ErosionSettings {
                walker_count: 32,
                strength: 100,
                seed: 82,
                ..Default::default()
            },
            113,
        );
        let difference =
            a.iter().map(|v| *v as f64).sum::<f64>() - after.iter().map(|v| *v as f64).sum::<f64>();
        assert!(difference.abs() < 0.03, "mass difference {difference}");
    }
    #[test]
    fn coherent_grassland_is_immutable() {
        let w = 32;
        let a = (0..w * w)
            .map(|i| 50.0 + (i % w) as f32 * 0.01)
            .collect::<Vec<_>>();
        assert_eq!(
            run(
                &a,
                ErosionSettings {
                    strength: 100,
                    walker_count: 32,
                    ..Default::default()
                },
                127
            ),
            a
        );
    }
}
