// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SkyInterpolation {
    Linear,
    SmoothLinear,
}
impl SkyInterpolation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::SmoothLinear => "SmoothLinear",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkyStop {
    pub position: f32,
    pub color: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SkyGradient {
    pub stops: Vec<SkyStop>,
    pub interpolation: SkyInterpolation,
}

impl Default for SkyGradient {
    fn default() -> Self {
        Self {
            stops: vec![
                SkyStop {
                    position: 0.0,
                    color: [0.94, 0.49, 0.29],
                },
                SkyStop {
                    position: 0.24,
                    color: [0.94, 0.69, 0.54],
                },
                SkyStop {
                    position: 0.51,
                    color: [0.78, 0.55, 0.67],
                },
                SkyStop {
                    position: 0.76,
                    color: [0.39, 0.38, 0.61],
                },
                SkyStop {
                    position: 1.0,
                    color: [0.13, 0.16, 0.30],
                },
            ],
            interpolation: SkyInterpolation::SmoothLinear,
        }
    }
}

impl SkyGradient {
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=16).contains(&self.stops.len()) {
            return Err("Sky Material requires 2 to 16 gradient stops".into());
        }
        let mut previous = -1.0_f32;
        for stop in &self.stops {
            if !stop.position.is_finite()
                || !(0.0..=1.0).contains(&stop.position)
                || stop.position <= previous
                || stop
                    .color
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            {
                return Err("Sky gradient stops must be ordered with finite positions/colors between 0 and 1".into());
            }
            previous = stop.position;
        }
        if self.stops[0].position != 0.0 || self.stops.last().unwrap().position != 1.0 {
            return Err("Sky gradient requires horizon (0%) and zenith (100%) stops".into());
        }
        Ok(())
    }
    pub fn sample(&self, height: f32) -> [f32; 3] {
        let h = height.clamp(0.0, 1.0);
        let Some(first) = self.stops.first() else {
            return [0.0; 3];
        };
        if h <= first.position {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if h <= b.position {
                let mut t =
                    ((h - a.position) / (b.position - a.position).max(0.00001)).clamp(0.0, 1.0);
                if self.interpolation == SkyInterpolation::SmoothLinear {
                    t = t * t * (3.0 - 2.0 * t);
                }
                return std::array::from_fn(|channel| {
                    a.color[channel] + (b.color[channel] - a.color[channel]) * t
                });
            }
        }
        self.stops
            .last()
            .map(|stop| stop.color)
            .unwrap_or(first.color)
    }

    pub fn sample_direction(&self, y: f32) -> [f32; 3] {
        self.sample(y.max(0.0))
    }
}

pub const DEFAULT_SKY_ID: &str = "WorldkilnDefaultSky";

pub fn default_sky() -> &'static SkyGradient {
    static DEFAULT: std::sync::LazyLock<SkyGradient> =
        std::sync::LazyLock::new(SkyGradient::default);
    &DEFAULT
}
