// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::{Vec3, Vec4};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MAX_PARTICLES: usize = 4096;
pub const MAX_BURST: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmitterPreset {
    Fire,
    Smoke,
    Sparks,
    Magic,
}

impl EmitterPreset {
    pub const ALL: [Self; 4] = [Self::Fire, Self::Smoke, Self::Sparks, Self::Magic];
    pub fn name(self) -> &'static str {
        match self {
            Self::Fire => "fire",
            Self::Smoke => "smoke",
            Self::Sparks => "sparks",
            Self::Magic => "magic",
        }
    }
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Fire => "Fire",
            Self::Smoke => "Smoke",
            Self::Sparks => "Sparks",
            Self::Magic => "Magic",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterSettings {
    #[serde(default)]
    pub effect_asset: Option<String>,
    pub preset: EmitterPreset,
    pub enabled: bool,
    pub rate: f32,
    pub scale: f32,
    pub radius: f32,
}

impl Default for EmitterSettings {
    fn default() -> Self {
        Self {
            effect_asset: None,
            preset: EmitterPreset::Fire,
            enabled: true,
            rate: 40.0,
            scale: 1.0,
            radius: 0.2,
        }
    }
}

impl EmitterSettings {
    pub fn blank_fx_emitter() -> Self {
        Self {
            effect_asset: Some(String::new()),
            ..Self::default()
        }
    }

    pub fn fx_asset_id(&self) -> Option<&str> {
        self.effect_asset.as_deref().filter(|id| !id.is_empty())
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(asset) = &self.effect_asset {
            if !asset.is_empty() && !crate::project::fx_assets::valid_asset_id(asset) {
                return Err("Invalid FX asset reference".into());
            }
        }
        for (name, value, min, max) in [
            ("rate", self.rate, 0.0, 1000.0),
            ("scale", self.scale, 0.01, 20.0),
            ("radius", self.radius, 0.0, 20.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!(
                    "Particle emitter {name} must be finite and in {min}..={max}"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct BurstConfig {
    pub lifetime: f32,
    pub velocity: Vec3,
    pub spread: f32,
    pub gravity: Vec3,
    pub size: f32,
    pub end_size: f32,
    pub color: Vec4,
    pub end_color: Vec4,
}

impl BurstConfig {
    pub fn preset(name: &str) -> Result<Self, String> {
        match name {
            "fire" => Ok(Self {
                lifetime: 0.85,
                velocity: Vec3::Y * 1.5,
                spread: 0.35,
                gravity: Vec3::Y * 0.5,
                size: 0.55,
                end_size: 0.08,
                color: Vec4::new(1.0, 0.85, 0.15, 0.9),
                end_color: Vec4::new(0.9, 0.08, 0.01, 0.0),
            }),
            "sparks" => Ok(Self {
                lifetime: 0.7,
                velocity: Vec3::Y * 2.0,
                spread: 3.0,
                gravity: Vec3::new(0.0, -6.0, 0.0),
                size: 0.12,
                end_size: 0.02,
                color: Vec4::new(1.0, 0.8, 0.2, 1.0),
                end_color: Vec4::new(1.0, 0.2, 0.0, 0.0),
            }),
            "smoke" => Ok(Self {
                lifetime: 2.0,
                velocity: Vec3::Y * 0.8,
                spread: 0.4,
                gravity: Vec3::ZERO,
                size: 0.3,
                end_size: 1.2,
                color: Vec4::new(0.55, 0.55, 0.6, 0.5),
                end_color: Vec4::new(0.3, 0.3, 0.35, 0.0),
            }),
            "magic" => Ok(Self {
                lifetime: 1.2,
                velocity: Vec3::Y * 0.5,
                spread: 1.5,
                gravity: Vec3::ZERO,
                size: 0.18,
                end_size: 0.0,
                color: Vec4::new(0.3, 0.5, 1.0, 1.0),
                end_color: Vec4::new(0.8, 0.2, 1.0, 0.0),
            }),
            _ => Err(format!(
                "particles.burst: unknown preset '{name}' (fire, smoke, sparks, magic)"
            )),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, value, min, max) in [
            ("lifetime", self.lifetime, 0.01, 60.0),
            ("spread", self.spread, 0.0, 1000.0),
            ("size", self.size, 0.0, 100.0),
            ("end_size", self.end_size, 0.0, 100.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!(
                    "particles.burst: {name} must be finite and in {min}..={max}"
                ));
            }
        }
        for (name, vector) in [("velocity", self.velocity), ("gravity", self.gravity)] {
            if !vector.is_finite() || vector.abs().max_element() > 1000.0 {
                return Err(format!(
                    "particles.burst: {name} must contain finite numbers in -1000..=1000"
                ));
            }
        }
        for (name, color) in [("color", self.color), ("end_color", self.end_color)] {
            if !color.is_finite() || color.min_element() < 0.0 || color.max_element() > 1.0 {
                return Err(format!(
                    "particles.burst: {name} must contain RGBA numbers in 0..=1"
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Particle {
    origin: Vec3,
    velocity: Vec3,
    config: BurstConfig,
    age: f32,
    authored_fire: bool,
}

impl Particle {
    pub fn is_authored_fire(&self) -> bool {
        self.authored_fire
    }
    pub fn position(&self) -> Vec3 {
        self.origin + self.velocity * self.age + self.config.gravity * (0.5 * self.age * self.age)
    }

    pub fn size(&self) -> f32 {
        self.config.size + (self.config.end_size - self.config.size) * self.progress()
    }

    pub fn color(&self) -> Vec4 {
        self.config
            .color
            .lerp(self.config.end_color, self.progress())
    }

    fn progress(&self) -> f32 {
        (self.age / self.config.lifetime).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParticleTickStats {
    pub input_emitters: usize,
    pub eligible_cpu_emitters: usize,
    pub requested_particles: usize,
    pub spawned_particles: usize,
    pub budget_dropped_particles: usize,
    pub live_particles: usize,
    pub particle_capacity: usize,
    pub particle_buffer_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ParticleSystem {
    particles: Vec<Particle>,
    emission_credit: HashMap<u64, f32>,
    enabled_overrides: HashMap<u64, bool>,
    last_tick_stats: ParticleTickStats,
}

impl ParticleSystem {
    pub fn burst(
        &mut self,
        position: Vec3,
        count: usize,
        config: BurstConfig,
    ) -> Result<usize, String> {
        config.validate()?;
        if !position.is_finite() {
            return Err("particles.burst: position must contain finite numbers".into());
        }
        if count > MAX_BURST {
            return Err(format!("particles.burst: count must be in 0..={MAX_BURST}"));
        }
        let count = count.min(MAX_PARTICLES.saturating_sub(self.particles.len()));
        let mut rng = rand::thread_rng();
        for _ in 0..count {
            let z: f32 = rng.gen_range(-1.0..=1.0);
            let angle: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
            let radius = (1.0 - z * z).sqrt();
            let direction = Vec3::new(radius * angle.cos(), z, radius * angle.sin());
            self.particles.push(Particle {
                origin: position,
                velocity: config.velocity + direction * config.spread * rng.gen_range(0.0..=1.0),
                config: config.clone(),
                age: 0.0,
                authored_fire: false,
            });
        }
        Ok(count)
    }

    pub fn update(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        for particle in &mut self.particles {
            particle.age += dt;
        }
        self.particles
            .retain(|particle| particle.age < particle.config.lifetime);
    }

    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }
    pub fn last_tick_stats(&self) -> ParticleTickStats {
        self.last_tick_stats
    }
    pub fn clear_particles(&mut self) {
        self.particles.clear();
    }
    pub fn clear(&mut self) {
        self.particles.clear();
        self.emission_credit.clear();
        self.enabled_overrides.clear();
        self.last_tick_stats = ParticleTickStats::default();
    }

    pub fn set_emitter_enabled(&mut self, id: u64, enabled: bool) {
        self.enabled_overrides.insert(id, enabled);
        self.emission_credit.remove(&id);
    }

    pub fn emitter_enabled(&self, id: u64, settings: &EmitterSettings) -> bool {
        self.enabled_overrides
            .get(&id)
            .copied()
            .unwrap_or(settings.enabled)
    }

    pub fn update_emitters(&mut self, dt: f32, emitters: &[(u64, Vec3, EmitterSettings)]) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let mut stats = ParticleTickStats {
            input_emitters: emitters.len(),
            ..ParticleTickStats::default()
        };
        self.update(dt);
        let active: std::collections::HashSet<_> = emitters.iter().map(|(id, _, _)| *id).collect();
        self.emission_credit.retain(|id, _| active.contains(id));
        self.enabled_overrides.retain(|id, _| active.contains(id));
        let mut rng = rand::thread_rng();
        for (id, position, settings) in emitters {
            if settings.effect_asset.is_some() {
                continue;
            }
            if settings.validate().is_err()
                || !self
                    .enabled_overrides
                    .get(id)
                    .copied()
                    .unwrap_or(settings.enabled)
            {
                self.emission_credit.remove(id);
                continue;
            }
            stats.eligible_cpu_emitters += 1;
            let credit = self.emission_credit.entry(*id).or_default();
            *credit += dt.min(0.25) * settings.rate;
            let requested = credit.floor() as usize;
            *credit -= requested as f32;
            stats.requested_particles = stats.requested_particles.saturating_add(requested);
            let count = requested.min(MAX_PARTICLES.saturating_sub(self.particles.len()));
            stats.budget_dropped_particles = stats
                .budget_dropped_particles
                .saturating_add(requested.saturating_sub(count));
            for i in 0..count {
                let mut config = BurstConfig::preset(settings.preset.name()).unwrap();
                config.size *= settings.scale;
                config.end_size *= settings.scale;
                config.velocity *= settings.scale;
                config.spread *= settings.scale;
                config.gravity *= settings.scale;
                let angle = rng.gen_range(0.0..std::f32::consts::TAU);
                let radius = rng.gen_range(0.0f32..=1.0).sqrt() * settings.radius;
                let origin = *position + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
                if self.burst(origin, 1, config).is_ok() {
                    stats.spawned_particles += 1;
                    if let Some(particle) = self.particles.last_mut() {
                        particle.authored_fire = settings.preset == EmitterPreset::Fire;
                        particle.age = dt.min(0.25) * i as f32 / count.max(1) as f32;
                    }
                }
            }
        }
        stats.live_particles = self.particles.len();
        stats.particle_capacity = self.particles.capacity();
        stats.particle_buffer_bytes = stats
            .particle_capacity
            .saturating_mul(std::mem::size_of::<Particle>());
        self.last_tick_stats = stats;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emitter_budget_counters_exclude_native_fx_and_rejected_emitters() {
        let mut system = ParticleSystem::default();
        let heavy = EmitterSettings {
            rate: 1000.0,
            ..EmitterSettings::default()
        };
        let inputs: Vec<_> = (0..50_u64)
            .map(|id| (id, Vec3::ZERO, heavy.clone()))
            .collect();
        system.update_emitters(0.25, &inputs);
        let first = system.last_tick_stats();
        assert_eq!(first.input_emitters, 50);
        assert_eq!(first.eligible_cpu_emitters, 50);
        assert_eq!(first.requested_particles, 12_500);
        assert_eq!(first.spawned_particles, MAX_PARTICLES);
        assert_eq!(first.budget_dropped_particles, 12_500 - MAX_PARTICLES);
        assert_eq!(first.live_particles, MAX_PARTICLES);
        assert!(first.particle_capacity >= MAX_PARTICLES);
        assert_eq!(
            first.particle_buffer_bytes,
            first.particle_capacity * std::mem::size_of::<Particle>()
        );

        let mut native = heavy.clone();
        native.effect_asset = Some("Effects/MyEffect.efkefc".into());
        let mut disabled = heavy.clone();
        disabled.enabled = false;
        let invalid = EmitterSettings {
            rate: f32::NAN,
            ..heavy.clone()
        };
        system.update_emitters(
            0.25,
            &[
                (100, Vec3::ZERO, native),
                (101, Vec3::ZERO, disabled),
                (102, Vec3::ZERO, invalid),
            ],
        );
        let second = system.last_tick_stats();
        assert_eq!(second.input_emitters, 3);
        assert_eq!(second.eligible_cpu_emitters, 0);
        assert_eq!(second.requested_particles, 0);
        assert_eq!(second.spawned_particles, 0);
        assert_eq!(second.budget_dropped_particles, 0);
        assert_eq!(second.live_particles, MAX_PARTICLES);
        system.clear();
        assert_eq!(system.last_tick_stats(), ParticleTickStats::default());
    }

    #[test]
    fn preset_budget_stats_are_bounded_at_two_hundred_emitters() {
        let emitters: Vec<_> = (0..200)
            .map(|id| {
                (
                    id,
                    Vec3::ZERO,
                    EmitterSettings {
                        rate: 1000.0,
                        ..EmitterSettings::default()
                    },
                )
            })
            .collect();
        let mut system = ParticleSystem::default();
        system.update_emitters(0.25, &emitters);
        let stats = system.last_tick_stats();
        assert_eq!(stats.input_emitters, 200);
        assert_eq!(stats.eligible_cpu_emitters, 200);
        assert_eq!(stats.requested_particles, 50_000);
        assert_eq!(stats.spawned_particles, MAX_PARTICLES);
        assert_eq!(stats.budget_dropped_particles, 50_000 - MAX_PARTICLES);
        assert_eq!(system.particles().len(), MAX_PARTICLES);
        system.update_emitters(0.01, &emitters);
        assert!(system.particles().len() <= MAX_PARTICLES);
        assert_eq!(system.last_tick_stats().input_emitters, 200);
    }

    #[test]
    #[ignore = "manual CPU emitter profiling; hardware and renderer are not controlled"]
    fn benchmark_cpu_preset_emitter_counts() {
        use std::time::{Duration, Instant};
        for emitter_count in [1_usize, 50, 200] {
            let emitters: Vec<_> = (0..emitter_count as u64)
                .map(|id| {
                    (
                        id,
                        Vec3::ZERO,
                        EmitterSettings {
                            rate: 40.0,
                            ..EmitterSettings::default()
                        },
                    )
                })
                .collect();
            let mut system = ParticleSystem::default();
            let mut samples = Vec::with_capacity(120);
            let mut peak_particles = 0;
            let mut peak_bytes = 0;
            for frame in 0..140 {
                let started = Instant::now();
                system.update_emitters(1.0 / 60.0, &emitters);
                let elapsed = started.elapsed();
                let stats = system.last_tick_stats();
                peak_particles = peak_particles.max(stats.live_particles);
                peak_bytes = peak_bytes.max(stats.particle_buffer_bytes);
                if frame >= 20 {
                    samples.push(elapsed);
                }
            }
            samples.sort_unstable();
            let percentile = |p: usize| samples[(samples.len() * p).div_ceil(100) - 1];
            let to_ms = |d: Duration| d.as_secs_f64() * 1_000.0;
            println!(
                "WK-FX-EDITOR-005 CPU preset emitters={emitter_count} frames={} p50_ms={:.4} p95_ms={:.4} max_ms={:.4} peak_particles={} peak_buffer_bytes={} capacity_limit={}",
                samples.len(),
                to_ms(percentile(50)),
                to_ms(percentile(95)),
                to_ms(*samples.last().unwrap()),
                peak_particles,
                peak_bytes,
                MAX_PARTICLES
            );
            assert!(peak_particles <= MAX_PARTICLES);
        }
    }

    #[test]
    fn particles_follow_ballistics_fade_and_expire() {
        let mut system = ParticleSystem::default();
        let mut config = BurstConfig::preset("sparks").unwrap();
        config.spread = 0.0;
        config.lifetime = 1.0;
        system.burst(Vec3::ZERO, 1, config.clone()).unwrap();
        system.update(0.5);
        let particle = &system.particles()[0];
        assert!(particle.position().abs_diff_eq(Vec3::Y * 0.25, 1e-6));
        assert!((particle.size() - 0.07).abs() < 1e-6);
        assert!(
            particle
                .color()
                .abs_diff_eq(config.color.lerp(config.end_color, 0.5), 1e-6)
        );
        system.update(0.5);
        assert!(system.particles().is_empty());
    }

    #[test]
    fn particle_budget_and_invalid_inputs_do_not_disrupt_live_effects() {
        let mut system = ParticleSystem::default();
        let config = BurstConfig::preset("magic").unwrap();
        for _ in 0..4 {
            assert_eq!(
                system.burst(Vec3::ZERO, MAX_BURST, config.clone()).unwrap(),
                MAX_BURST
            );
        }
        assert_eq!(system.burst(Vec3::ZERO, 10, config.clone()).unwrap(), 0);
        assert!(system.burst(Vec3::NAN, 1, config.clone()).is_err());
        assert!(system.burst(Vec3::ZERO, MAX_BURST + 1, config).is_err());
        system.update(f32::NAN);
        system.update(-1.0);
        assert_eq!(system.particles().len(), MAX_PARTICLES);
        system.clear();
        assert!(system.particles().is_empty());
    }

    #[test]
    fn invalid_config_is_rejected_before_emitting() {
        for name in ["sparks", "smoke", "magic"] {
            BurstConfig::preset(name).unwrap().validate().unwrap();
        }
        let mut config = BurstConfig::preset("smoke").unwrap();
        config.lifetime = 0.0;
        let mut system = ParticleSystem::default();
        assert!(
            system
                .burst(Vec3::ZERO, 3, config)
                .unwrap_err()
                .contains("lifetime")
        );
        assert!(system.particles().is_empty());
    }

    #[test]
    fn ballistic_motion_is_independent_of_frame_partition() {
        let mut a = ParticleSystem::default();
        let mut config = BurstConfig::preset("sparks").unwrap();
        config.spread = 0.0;
        a.burst(Vec3::ZERO, 1, config).unwrap();
        let mut b = a.clone();
        a.update(0.4);
        for _ in 0..4 {
            b.update(0.1);
        }
        assert!(
            a.particles()[0]
                .position()
                .abs_diff_eq(b.particles()[0].position(), 1e-6)
        );
    }
}

#[cfg(test)]
mod fx_reference_tests {
    use super::*;
    #[test]
    fn emitter_fx_reference_round_trip_and_legacy_default() {
        let legacy = r#"{"preset":"smoke","enabled":true,"rate":40.0,"scale":1.0,"radius":0.2}"#;
        let parsed: EmitterSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.effect_asset, None);
        assert_eq!(parsed.preset, EmitterPreset::Smoke);
        let mut emitter = EmitterSettings::blank_fx_emitter();
        assert_eq!(emitter.effect_asset.as_deref(), Some(""));
        emitter.effect_asset = Some("Fire/Campfire.efkefc".into());
        let saved = serde_json::to_string(&emitter).unwrap();
        let reopened: EmitterSettings = serde_json::from_str(&saved).unwrap();
        assert_eq!(reopened.effect_asset, emitter.effect_asset);
        assert!(reopened.validate().is_ok());
        emitter.effect_asset = Some("../Unsafe.efkefc".into());
        assert!(emitter.validate().is_err());
    }
}
