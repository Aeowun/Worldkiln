// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::types::TerrainWaterSettings;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaterBodyKind {
    Lake,
    River,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterControlPoint {
    pub position: [f32; 2],
    pub width: f32,
    #[serde(default)]
    pub height_offset: f32,
    #[serde(default)]
    pub tangent: [f32; 2],
    #[serde(default)]
    pub flow_angle: f32,
    #[serde(default = "default_flow_speed")]
    pub flow_speed: f32,
    #[serde(default)]
    pub channel_depth: Option<f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WaterBody {
    #[serde(default = "super::material_layers::default_shore_layer")]
    pub shore_material_layer: String,
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: WaterBodyKind,
    #[serde(default = "enabled")]
    pub visible: bool,
    pub elevation: f32,
    pub points: Vec<WaterControlPoint>,
    #[serde(default)]
    pub appearance: Option<TerrainWaterSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_asset: Option<String>,
    #[serde(default = "default_depth")]
    pub depth: f32,
    #[serde(default = "default_bank_width")]
    pub bank_width: f32,
}
fn enabled() -> bool {
    true
}
fn default_flow_speed() -> f32 {
    1.0
}
fn default_depth() -> f32 {
    24.0
}
fn default_bank_width() -> f32 {
    3.0
}

impl WaterBody {
    pub fn new(id: u64, kind: WaterBodyKind, center: [f32; 2], elevation: f32) -> Self {
        let point = |x, z, width| WaterControlPoint {
            position: [x, z],
            width,
            height_offset: 0.0,
            tangent: [0.0, 0.0],
            flow_angle: 0.0,
            flow_speed: default_flow_speed(),
            channel_depth: None,
        };
        let [x, z] = center;
        let points = match kind {
            WaterBodyKind::Lake => vec![
                point(x - 8.0, z - 8.0, 1.0),
                point(x + 8.0, z - 8.0, 1.0),
                point(x + 8.0, z + 8.0, 1.0),
                point(x - 8.0, z + 8.0, 1.0),
            ],
            WaterBodyKind::River => vec![point(x - 12.0, z, 6.0), point(x + 12.0, z, 6.0)],
        };
        Self {
            shore_material_layer: super::material_layers::default_shore_layer(),
            id,
            name: None,
            kind,
            visible: true,
            elevation,
            points,
            appearance: Some(TerrainWaterSettings::default()),
            material_asset: None,
            depth: default_depth(),
            bank_width: default_bank_width(),
        }
    }

    pub fn assign_material(
        &mut self,
        catalog: &crate::project::material::MaterialCatalog,
        id: &str,
    ) -> Result<(), String> {
        let look = catalog
            .water(id)
            .ok_or("Material is missing, invalid, or does not have Water output")?;
        self.material_asset = Some(id.to_owned());
        self.appearance = Some(look);
        Ok(())
    }

    pub fn resolved_appearance(
        &self,
        catalog: &crate::project::material::MaterialCatalog,
        global: &TerrainWaterSettings,
    ) -> TerrainWaterSettings {
        self.material_asset
            .as_deref()
            .and_then(|id| catalog.last_known_water(id))
            .unwrap_or(*self.appearance_or(global))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id == 0 {
            return Err("Water body ID must be nonzero");
        }
        if !self.depth.is_finite()
            || !(0.1..=10000.0).contains(&self.depth)
            || !self.bank_width.is_finite()
            || !(0.0..=1000.0).contains(&self.bank_width)
            || self
                .appearance
                .as_ref()
                .is_some_and(|look| look.validate().is_err())
        {
            return Err("Water body depth, bank width or appearance invalid");
        }
        if !self.elevation.is_finite() {
            return Err("Water level must be finite");
        }
        if self.points.len()
            < match self.kind {
                WaterBodyKind::Lake => 3,
                WaterBodyKind::River => 2,
            }
            || self.points.len() > 128
        {
            return Err("Water body has an invalid number of points");
        }
        if self.points.iter().any(|p| {
            p.position.iter().any(|v| !v.is_finite())
                || !p.width.is_finite()
                || !(0.1..=4096.0).contains(&p.width)
                || !p.height_offset.is_finite()
                || p.height_offset.abs() > 10000.0
                || p.tangent
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 10000.0)
                || !p.flow_angle.is_finite()
                || !p.flow_speed.is_finite()
                || !(0.0..=40.0).contains(&p.flow_speed)
                || p.channel_depth
                    .is_some_and(|depth| !depth.is_finite() || !(0.1..=10000.0).contains(&depth))
        }) {
            return Err("Water point position/width must be finite and width positive");
        }
        let area = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let next = self.points[(i + 1) % self.points.len()].position;
                p.position[0] * next[1] - next[0] * p.position[1]
            })
            .sum::<f32>()
            .abs();
        if self.kind == WaterBodyKind::Lake && area < 0.001 {
            return Err("Lake polygon must enclose an area");
        }
        if self.kind == WaterBodyKind::River
            && self
                .points
                .windows(2)
                .all(|w| distance_sq(w[0].position, w[1].position) < 0.000001)
        {
            return Err("River must have a nonzero segment");
        }
        Ok(())
    }

    pub fn surface_y_at(&self, x: f32, z: f32) -> f32 {
        let samples = self.spline_samples();
        self.surface_y_at_samples(x, z, &samples)
    }
    pub fn surface_y_at_samples(&self, x: f32, z: f32, samples: &[([f32; 2], f32, f32)]) -> f32 {
        if self.kind == WaterBodyKind::Lake {
            return self.elevation;
        }
        let mut selected = (f32::MAX, 0.0);
        for pair in samples.windows(2) {
            let (distance, t) = segment_distance_sq([x, z], pair[0].0, pair[1].0);
            if distance < selected.0 {
                selected = (distance, pair[0].2 + (pair[1].2 - pair[0].2) * t);
            }
        }
        self.elevation + selected.1
    }

    pub fn maximum_surface_y(&self) -> f32 {
        self.elevation
            + if self.kind == WaterBodyKind::River {
                self.points
                    .iter()
                    .map(|p| p.height_offset)
                    .fold(0.0_f32, f32::max)
            } else {
                0.0
            }
    }

    pub fn sample_segment(&self, index: usize, t: f32) -> Option<WaterControlPoint> {
        let n = self.points.len();
        if n < 2 || index >= n || (self.kind == WaterBodyKind::River && index + 1 >= n) {
            return None;
        }
        let a = self.points[index];
        let b = self.points[(index + 1) % n];
        let t = t.clamp(0.0, 1.0);
        let u = 1.0 - t;
        let p0 = a.position;
        let p1 = [p0[0] + a.tangent[0], p0[1] + a.tangent[1]];
        let p3 = b.position;
        let p2 = [p3[0] - b.tangent[0], p3[1] - b.tangent[1]];
        let mut new = a;
        new.position = [
            u * u * u * p0[0]
                + 3.0 * u * u * t * p1[0]
                + 3.0 * u * t * t * p2[0]
                + t * t * t * p3[0],
            u * u * u * p0[1]
                + 3.0 * u * u * t * p1[1]
                + 3.0 * u * t * t * p2[1]
                + t * t * t * p3[1],
        ];
        new.width = a.width + (b.width - a.width) * t;
        new.height_offset = a.height_offset + (b.height_offset - a.height_offset) * t;
        new.tangent = [
            (a.tangent[0] + b.tangent[0]) * 0.5,
            (a.tangent[1] + b.tangent[1]) * 0.5,
        ];
        Some(new)
    }

    pub fn spline_samples(&self) -> Vec<([f32; 2], f32, f32)> {
        let closed = self.kind == WaterBodyKind::Lake;
        let n = self.points.len();
        if n < 2 {
            return Vec::new();
        }
        let count = if closed { n } else { n - 1 };
        let mut samples = Vec::with_capacity(count * 8 + 1);
        for i in 0..count {
            let a = self.points[i];
            let b = self.points[(i + 1) % n];
            let p0 = a.position;
            let p1 = [p0[0] + a.tangent[0], p0[1] + a.tangent[1]];
            let p3 = b.position;
            let p2 = [p3[0] - b.tangent[0], p3[1] - b.tangent[1]];
            for step in 0..8 {
                let t = step as f32 / 8.0;
                let u = 1.0 - t;
                let point = [
                    u * u * u * p0[0]
                        + 3.0 * u * u * t * p1[0]
                        + 3.0 * u * t * t * p2[0]
                        + t * t * t * p3[0],
                    u * u * u * p0[1]
                        + 3.0 * u * u * t * p1[1]
                        + 3.0 * u * t * t * p2[1]
                        + t * t * t * p3[1],
                ];
                samples.push((
                    point,
                    a.width + (b.width - a.width) * t,
                    a.height_offset + (b.height_offset - a.height_offset) * t,
                ));
            }
        }
        if !closed {
            let p = self.points[n - 1];
            samples.push((p.position, p.width, p.height_offset));
        }
        samples
    }

    pub fn vertical_bounds_at(&self, x: f32, z: f32, terrain_floor: f32) -> Option<(f32, f32)> {
        let samples = self.spline_samples();
        self.vertical_bounds_at_samples(x, z, terrain_floor, &samples)
    }
    pub fn vertical_bounds_at_samples(
        &self,
        x: f32,
        z: f32,
        terrain_floor: f32,
        samples: &[([f32; 2], f32, f32)],
    ) -> Option<(f32, f32)> {
        if !self.visible
            || !terrain_floor.is_finite()
            || self.signed_coverage_samples(x, z, samples) <= 0.0
        {
            return None;
        }
        let top = self.surface_y_at_samples(x, z, samples);
        let depth = self.channel_depth_at_samples(x, z, samples);
        let bottom = (top - depth).max(terrain_floor);
        (top > bottom + 0.001).then_some((bottom, top))
    }
    pub fn channel_depth_at_samples(
        &self,
        x: f32,
        z: f32,
        samples: &[([f32; 2], f32, f32)],
    ) -> f32 {
        if self.kind == WaterBodyKind::Lake || samples.len() < 2 {
            return self.depth;
        }
        let mut best = (f32::MAX, self.depth);
        for (i, pair) in samples.windows(2).enumerate() {
            let (distance, t) = segment_distance_sq([x, z], pair[0].0, pair[1].0);
            if distance < best.0 {
                let segment = (i / 8).min(self.points.len() - 2);
                let interpolation = ((i % 8) as f32 + t) / 8.0;
                let a = self.points[segment].channel_depth.unwrap_or(self.depth);
                let b = self.points[segment + 1].channel_depth.unwrap_or(self.depth);
                best = (distance, a + (b - a) * interpolation);
            }
        }
        best.1
    }
    pub fn channel_depth_at(&self, x: f32, z: f32) -> f32 {
        let samples = self.spline_samples();
        self.channel_depth_at_samples(x, z, &samples)
    }

    pub fn contains_volume_point(&self, point: [f32; 3], terrain_floor: f32) -> bool {
        self.vertical_bounds_at(point[0], point[2], terrain_floor)
            .is_some_and(|(bottom, top)| point[1] >= bottom && point[1] < top)
    }
    pub fn water_depth_at(&self, x: f32, z: f32, terrain_floor: f32) -> f32 {
        self.vertical_bounds_at(x, z, terrain_floor)
            .map_or(0.0, |(bottom, top)| top - bottom)
    }
    pub fn bank_proximity_at(&self, x: f32, z: f32) -> Option<f32> {
        let distance = self.signed_coverage(x, z);
        (distance >= -self.bank_width && distance <= self.bank_width).then_some(distance)
    }
    pub fn appearance_or<'a>(
        &'a self,
        global: &'a TerrainWaterSettings,
    ) -> &'a TerrainWaterSettings {
        self.appearance.as_ref().unwrap_or(global)
    }
    pub fn flow_at(&self, x: f32, z: f32) -> [f32; 2] {
        let samples = self.spline_samples();
        self.flow_at_samples(x, z, &samples)
    }
    pub fn flow_at_samples(&self, x: f32, z: f32, samples: &[([f32; 2], f32, f32)]) -> [f32; 2] {
        if self.kind != WaterBodyKind::River || samples.len() < 2 {
            return [0.0, 0.0];
        }
        let mut closest = (f32::MAX, [0.0, 0.0]);
        for (i, pair) in samples.windows(2).enumerate() {
            let (distance, t) = segment_distance_sq([x, z], pair[0].0, pair[1].0);
            if distance < closest.0 {
                let dx = pair[1].0[0] - pair[0].0[0];
                let dz = pair[1].0[1] - pair[0].0[1];
                let base = dz.atan2(dx);
                let segment = (i / 8).min(self.points.len() - 2);
                let local_t = ((i % 8) as f32 + t) / 8.0;
                let a = self.points[segment];
                let b = self.points[segment + 1];
                let offset = a.flow_angle + (b.flow_angle - a.flow_angle) * local_t;
                let speed = a.flow_speed + (b.flow_speed - a.flow_speed) * local_t;
                let angle = base + offset;
                closest = (distance, [angle.cos() * speed, angle.sin() * speed]);
            }
        }
        closest.1
    }

    pub fn bounds(&self) -> ([f32; 2], [f32; 2]) {
        let padding = if self.kind == WaterBodyKind::River {
            self.points
                .iter()
                .map(|p| p.width * 0.5)
                .fold(0.0_f32, f32::max)
        } else {
            0.0
        };
        let mut min = [f32::MAX; 2];
        let mut max = [f32::MIN; 2];
        for (position, _, _) in self.spline_samples() {
            for i in 0..2 {
                min[i] = min[i].min(position[i] - padding);
                max[i] = max[i].max(position[i] + padding);
            }
        }
        (min, max)
    }

    pub fn signed_coverage(&self, x: f32, z: f32) -> f32 {
        let samples = self.spline_samples();
        self.signed_coverage_samples(x, z, &samples)
    }
    pub fn signed_coverage_samples(&self, x: f32, z: f32, samples: &[([f32; 2], f32, f32)]) -> f32 {
        let p = [x, z];
        if samples.len() < 2 {
            return -f32::MAX;
        }
        match self.kind {
            WaterBodyKind::Lake => {
                let mut inside = false;
                let mut min_dist_sq = f32::MAX;
                for i in 0..samples.len() {
                    let a = samples[i].0;
                    let b = samples[(i + 1) % samples.len()].0;
                    min_dist_sq = min_dist_sq.min(segment_distance_sq(p, a, b).0);
                    if (a[1] > z) != (b[1] > z)
                        && x < (b[0] - a[0]) * (z - a[1]) / (b[1] - a[1]) + a[0]
                    {
                        inside = !inside;
                    }
                }
                if inside {
                    min_dist_sq.sqrt()
                } else {
                    -min_dist_sq.sqrt()
                }
            }
            WaterBodyKind::River => {
                let mut signed = -f32::MAX;
                for pair in samples.windows(2) {
                    let (dist_sq, t) = segment_distance_sq(p, pair[0].0, pair[1].0);
                    let width = pair[0].1 + (pair[1].1 - pair[0].1) * t;
                    signed = signed.max(width * 0.5 - dist_sq.sqrt());
                }
                signed
            }
        }
    }
}

fn distance_sq(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}
fn segment_distance_sq(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> (f32, f32) {
    let dx = b[0] - a[0];
    let dz = b[1] - a[1];
    let len_sq = dx * dx + dz * dz;
    let t = if len_sq > 0.000001 {
        ((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / len_sq
    } else {
        0.0
    }
    .clamp(0.0, 1.0);
    let q = [a[0] + dx * t, a[1] + dz * t];
    (distance_sq(p, q), t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lake_and_river_have_continuous_bounded_signed_shorelines() {
        let lake = WaterBody::new(1, WaterBodyKind::Lake, [30.0, 40.0], 10.0);
        assert!(lake.signed_coverage(30.0, 40.0) > 0.0);
        assert!(lake.signed_coverage(60.0, 40.0) < 0.0);
        assert!(lake.signed_coverage(38.0, 40.0).abs() < 0.0001);
        let river = WaterBody::new(2, WaterBodyKind::River, [30.0, 40.0], 10.0);
        assert!(river.signed_coverage(30.0, 40.0) > 0.0);
        assert!(river.signed_coverage(30.0, 46.0) < 0.0);
        assert!(river.signed_coverage(30.0, 43.0).abs() < 0.0001);
        assert!(lake.validate().is_ok());
        assert!(river.validate().is_ok());
    }
    #[test]
    fn river_height_interpolates_but_lake_surface_is_level() {
        let mut river = WaterBody::new(3, WaterBodyKind::River, [0.0, 0.0], 10.0);
        river.points[0].height_offset = -2.0;
        river.points[1].height_offset = 4.0;
        assert_eq!(river.surface_y_at(-12.0, 0.0), 8.0);
        assert_eq!(river.surface_y_at(12.0, 0.0), 14.0);
        assert!((river.surface_y_at(0.0, 0.0) - 11.0).abs() < 0.0001);
        let mut lake = WaterBody::new(4, WaterBodyKind::Lake, [0.0, 0.0], 10.0);
        lake.points[0].height_offset = 500.0;
        assert_eq!(lake.surface_y_at(-8.0, -8.0), 10.0);
        let old = serde_json::json!({
            "id":3,"kind":"River","visible":true,"elevation":10.0,
            "points":[
                {"position":[-12.0,0.0],"width":6.0},
                {"position":[12.0,0.0],"width":6.0}
            ]
        });
        let restored: WaterBody = serde_json::from_value(old).unwrap();
        assert!(restored.points.iter().all(|p| p.height_offset == 0.0));
        assert!(restored.points.iter().all(|p| p.tangent == [0.0, 0.0]));
    }
    #[test]
    fn spline_tangent_changes_real_lake_and_river_boundary() {
        let mut lake = WaterBody::new(9, WaterBodyKind::Lake, [0.0, 0.0], 4.0);
        let prior = lake.signed_coverage(0.0, -10.0);
        lake.points[0].tangent = [0.0, -9.0];
        lake.points[1].tangent = [0.0, 9.0];
        assert_ne!(prior, lake.signed_coverage(0.0, -10.0));
        assert!(lake.validate().is_ok());
        let mut river = WaterBody::new(10, WaterBodyKind::River, [0.0, 0.0], 4.0);
        let old = river.signed_coverage(0.0, 9.0);
        river.points[0].tangent = [0.0, 13.0];
        river.points[1].tangent = [0.0, -13.0];
        assert!(river.signed_coverage(0.0, 9.0) > old);
    }
    #[test]
    fn curved_river_points_persist_height_tangent_and_flow_direction() {
        let mut river = WaterBody::new(100, WaterBodyKind::River, [0.0, 0.0], 12.0);
        river.points[0].height_offset = 3.5;
        river.points[1].height_offset = -2.0;
        river.points[0].tangent = [0.5, 4.25];
        river.points[1].tangent = [0.5, -4.25];
        river.points[0].flow_angle = 1.25;
        let serialized = serde_json::to_string(&river).unwrap();
        let reopened: WaterBody = serde_json::from_str(&serialized).unwrap();
        assert_eq!(river, reopened);
        assert!(reopened.validate().is_ok());
        assert!(reopened.spline_samples().len() > reopened.points.len());
        let mut bad = reopened.clone();
        bad.points[0].flow_angle = f32::NAN;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn river_channel_depth_follows_independent_control_points_and_old_projects() {
        let mut river = WaterBody::new(71, WaterBodyKind::River, [0.0, 0.0], 12.0);
        river.depth = 8.0;
        assert_eq!(river.channel_depth_at(-12.0, 0.0), 8.0);
        river.points[0].channel_depth = Some(2.0);
        river.points[1].channel_depth = Some(14.0);
        assert!((river.channel_depth_at(0.0, 0.0) - 8.0).abs() < 0.001);
        assert_eq!(
            river.vertical_bounds_at(-12.0, 0.0, 0.0),
            Some((10.0, 12.0))
        );
        assert_eq!(river.vertical_bounds_at(12.0, 0.0, 0.0), Some((0.0, 12.0)));
        let json = serde_json::to_string(&river).unwrap();
        let again: WaterBody = serde_json::from_str(&json).unwrap();
        assert_eq!(again, river);
        let mut invalid = river;
        invalid.points[0].channel_depth = Some(f32::NAN);
        assert!(invalid.validate().is_err());
        let legacy = WaterBody::new(72, WaterBodyKind::Lake, [0.0, 0.0], 8.0);
        assert!(legacy.points.iter().all(|p| p.channel_depth.is_none()));
    }

    #[test]
    fn three_dimensional_water_volume_tracks_floor_and_contour() {
        let mut lake = WaterBody::new(7, WaterBodyKind::Lake, [0.0, 0.0], 12.0);
        lake.depth = 6.0;
        assert_eq!(lake.vertical_bounds_at(0.0, 0.0, 3.0), Some((6.0, 12.0)));
        assert_eq!(lake.vertical_bounds_at(0.0, 0.0, 9.0), Some((9.0, 12.0)));
        assert!(lake.contains_volume_point([0.0, 10.0, 0.0], 9.0));
        assert!(!lake.contains_volume_point([0.0, 8.0, 0.0], 9.0));
        assert!(!lake.contains_volume_point([80.0, 10.0, 0.0], 0.0));
        assert_eq!(lake.water_depth_at(0.0, 0.0, 13.0), 0.0);
        assert!(lake.bank_proximity_at(8.0, 0.0).is_some());
    }
    #[test]
    fn separate_water_body_appearances_and_flow_are_independent() {
        let mut a = WaterBody::new(2, WaterBodyKind::River, [0.0, 0.0], 5.0);
        let mut b = WaterBody::new(3, WaterBodyKind::River, [0.0, 0.0], 5.0);
        a.appearance.as_mut().unwrap().wave_height = 0.1;
        b.appearance.as_mut().unwrap().wave_height = 2.0;
        a.points[0].flow_speed = 2.0;
        b.points[0].flow_speed = 12.0;
        assert_ne!(a.appearance, b.appearance);
        assert_ne!(a.flow_at(-12.0, 0.0), b.flow_at(-12.0, 0.0));
        let restored: WaterBody =
            serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(restored, a);
        let mut old = serde_json::to_value(&a).unwrap();
        for key in ["appearance", "depth", "bank_width"] {
            old.as_object_mut().unwrap().remove(key);
        }
        for point in old["points"].as_array_mut().unwrap() {
            point.as_object_mut().unwrap().remove("flow_speed");
        }
        let old: WaterBody = serde_json::from_value(old).unwrap();
        assert_eq!(old.depth, 24.0);
        assert!(old.appearance.is_none());
        assert_eq!(old.points[0].flow_speed, 1.0);
    }

    #[test]
    fn water_bodies_roundtrip_without_assigned_names() {
        let mut river = WaterBody::new(55, WaterBodyKind::River, [0.0, 0.0], 9.0);
        river.points[1].width = 18.0;
        let copy: WaterBody =
            serde_json::from_str(&serde_json::to_string(&river).unwrap()).unwrap();
        assert_eq!(river, copy);
        assert_eq!(copy.name, None);
        let mut bad = copy.clone();
        bad.points[0].width = f32::NAN;
        assert!(bad.validate().is_err());
    }
}
