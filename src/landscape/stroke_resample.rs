// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::paint_mask::CircularPaintBrush;
#[derive(Clone, Debug, Default)]
pub struct StrokeResampler {
    last: Option<[f32; 3]>,
    distance_since_dab: f32,
}
impl StrokeResampler {
    pub fn begin(&mut self, at: [f32; 3]) -> bool {
        if !at.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.last = Some(at);
        self.distance_since_dab = 0.0;
        true
    }
    pub fn sample(&mut self, to: [f32; 3], radius: f32, mut emit: impl FnMut([f32; 3])) -> usize {
        let Some(from) = self.last else { return 0 };
        if !to.iter().all(|v| v.is_finite()) || !radius.is_finite() || radius <= 0.0 {
            return 0;
        }
        let distance = (to[0] - from[0]).hypot(to[2] - from[2]);
        if distance <= 1e-6 {
            self.last = Some(to);
            return 0;
        }
        let spacing = (radius * 0.25).max(0.015);
        let mut cursor = (spacing - self.distance_since_dab).max(0.0);
        let mut count = 0;
        while cursor <= distance && count < 4096 {
            let t = cursor / distance;
            emit(std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t));
            count += 1;
            cursor += spacing;
        }
        self.distance_since_dab = if count == 4096 {
            0.0
        } else {
            (spacing - (cursor - distance)).rem_euclid(spacing)
        };
        self.last = Some(to);
        count
    }
    pub fn end(&mut self) {
        self.last = None;
        self.distance_since_dab = 0.0;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rapid_pointer_motion_has_no_gaps() {
        let mut r = StrokeResampler::default();
        assert!(r.begin([2.0, 0.0, 8.0]));
        let mut points = vec![[2.0, 0.0, 8.0]];
        let count = r.sample([26.0, 0.0, 8.0], 1.0, |p| points.push(p));
        assert_eq!(count, 96);
        assert_eq!(points.last().unwrap()[0], 26.0);
        for pair in points.windows(2) {
            assert!((pair[1][0] - pair[0][0]).abs() <= 0.25 + 1e-5);
        }
        for x in 2..=26 {
            assert!(
                points.iter().any(|p| {
                    CircularPaintBrush {
                        center: *p,
                        radius: 1.0,
                        edge_softness: 0.15,
                    }
                    .weight_at([x as f32, 0.0, 8.0])
                        > 0.0
                }),
                "missed x={x}"
            );
        }
    }
    #[test]
    fn frame_boundaries_do_not_reset_brush_spacing() {
        let mut r = StrokeResampler::default();
        r.begin([0.0, 0.0, 0.0]);
        let mut dabs = Vec::new();
        r.sample([0.14, 0.0, 0.0], 1.0, |p| dabs.push(p));
        r.sample([0.28, 0.0, 0.0], 1.0, |p| dabs.push(p));
        r.sample([0.72, 0.0, 0.0], 1.0, |p| dabs.push(p));
        assert_eq!(dabs.len(), 2);
        assert!((dabs[0][0] - 0.25).abs() < 1e-5);
        assert!((dabs[1][0] - 0.5).abs() < 1e-5);
    }
    #[test]
    fn inactive_and_invalid_events_produce_no_work() {
        let mut r = StrokeResampler::default();
        assert_eq!(r.sample([8.0, 0.0, 0.0], 1.0, |_| panic!()), 0);
        assert!(!r.begin([f32::NAN, 0.0, 0.0]));
        r.begin([0.0, 0.0, 0.0]);
        assert_eq!(r.sample([f32::NAN, 0.0, 0.0], 1.0, |_| panic!()), 0);
        r.end();
        assert_eq!(r.sample([10.0, 0.0, 0.0], 1.0, |_| panic!()), 0);
    }
}
