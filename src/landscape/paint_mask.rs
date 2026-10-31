// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircularPaintBrush {
    pub center: [f32; 3],
    pub radius: f32,
    pub edge_softness: f32,
}
impl CircularPaintBrush {
    pub fn validated(self) -> Option<Self> {
        if self.center.iter().all(|v| v.is_finite())
            && self.radius.is_finite()
            && self.radius > 0.0
            && self.edge_softness.is_finite()
            && (0.0..=1.0).contains(&self.edge_softness)
        {
            Some(self)
        } else {
            None
        }
    }
    pub fn weight_at(&self, point: [f32; 3]) -> f32 {
        if point.iter().any(|v| !v.is_finite()) {
            return 0.0;
        }
        let distance =
            ((point[0] - self.center[0]).powi(2) + (point[2] - self.center[2]).powi(2)).sqrt();
        if distance >= self.radius {
            return 0.0;
        }
        let inner = self.radius * (1.0 - self.edge_softness);
        if distance <= inner || self.edge_softness == 0.0 {
            return 1.0;
        }
        let t = ((self.radius - distance) / (self.radius - inner)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }
    pub fn overlaps_triangle_xz(&self, tri: [[f32; 3]; 3]) -> bool {
        if self.validated().is_none() || tri.iter().flatten().any(|v| !v.is_finite()) {
            return false;
        }
        let p = [self.center[0], self.center[2]];
        let a = [tri[0][0], tri[0][2]];
        let b = [tri[1][0], tri[1][2]];
        let c = [tri[2][0], tri[2][2]];
        let edge = |u: [f32; 2], v: [f32; 2], q: [f32; 2]| -> f32 {
            (v[0] - u[0]) * (q[1] - u[1]) - (v[1] - u[1]) * (q[0] - u[0])
        };
        let ab = edge(a, b, p);
        let bc = edge(b, c, p);
        let ca = edge(c, a, p);
        if (ab >= 0.0 && bc >= 0.0 && ca >= 0.0) || (ab <= 0.0 && bc <= 0.0 && ca <= 0.0) {
            return true;
        }
        let dist2 = |u: [f32; 2], v: [f32; 2]| {
            let dx = v[0] - u[0];
            let dz = v[1] - u[1];
            let len = dx * dx + dz * dz;
            let t = if len > 0.0 {
                ((p[0] - u[0]) * dx + (p[1] - u[1]) * dz) / len
            } else {
                0.0
            }
            .clamp(0.0, 1.0);
            (p[0] - u[0] - dx * t).powi(2) + (p[1] - u[1] - dz * t).powi(2)
        };
        [dist2(a, b), dist2(b, c), dist2(c, a)]
            .into_iter()
            .any(|d| d <= self.radius * self.radius)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pixel_accurate_circle_does_not_paint_bounding_square_corners() {
        let brush = CircularPaintBrush {
            center: [0.0, 0.0, 0.0],
            radius: 4.0,
            edge_softness: 0.1,
        };
        assert_eq!(brush.weight_at([4.0, 0.0, 4.0]), 0.0);
        assert_eq!(brush.weight_at([0.0, 0.0, 0.0]), 1.0);
        assert!(brush.weight_at([3.8, 0.0, 0.0]) > 0.0);
        assert_eq!(brush.weight_at([4.1, 0.0, 0.0]), 0.0);
    }
    #[test]
    fn thin_triangle_crossing_circle_counts_even_if_no_vertex_inside() {
        let brush = CircularPaintBrush {
            center: [0.0; 3],
            radius: 1.0,
            edge_softness: 0.0,
        };
        assert!(brush.overlaps_triangle_xz([[-3.0, 0.0, -0.1], [3.0, 0.0, -0.1], [0.0, 0.0, 2.0]]));
        assert!(!brush.overlaps_triangle_xz([[3.0, 0.0, 3.0], [4.0, 0.0, 3.0], [3.0, 0.0, 4.0]]));
    }
    #[test]
    fn soft_edges_fall_off_smoothly_and_invalid_brush_rejected() {
        let brush = CircularPaintBrush {
            center: [0.0; 3],
            radius: 10.0,
            edge_softness: 0.2,
        };
        assert!(brush.weight_at([8.5, 0.0, 0.0]) > brush.weight_at([9.5, 0.0, 0.0]));
        assert!(
            CircularPaintBrush {
                radius: f32::NAN,
                ..brush
            }
            .validated()
            .is_none()
        );
    }
}
