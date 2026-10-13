//! Transient CPU particles. Authored worlds and save files never contain these.
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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EmitterSettings {
    pub preset: EmitterPreset,
    pub enabled: bool,
    pub rate: f32,
    pub scale: f32,
    pub radius: f32,
}

impl Default for EmitterSettings {
    fn default() -> Self {
        Self {
            preset: EmitterPreset::Fire,
            enabled: true,
            rate: 40.0,
            scale: 1.0,
            radius: 0.2,
        }
    }
}

impl EmitterSettings {
    pub fn validate(&self) -> Result<(), String> {
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
}

impl Particle {
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

#[derive(Clone, Debug, Default)]
pub struct ParticleSystem {
    particles: Vec<Particle>,
    emission_credit: HashMap<u64, f32>,
    enabled_overrides: HashMap<u64, bool>,
}

impl ParticleSystem {
    /// A full budget drops new particles without disturbing existing effects.
    /// Return the actual emitted count so scripts can observe budget pressure.
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
            // Uniform directions on a sphere, with bounded randomized speed.
            let z: f32 = rng.gen_range(-1.0..=1.0);
            let angle: f32 = rng.gen_range(0.0..std::f32::consts::TAU);
            let radius = (1.0 - z * z).sqrt();
            let direction = Vec3::new(radius * angle.cos(), z, radius * angle.sin());
            self.particles.push(Particle {
                origin: position,
                velocity: config.velocity + direction * config.spread * rng.gen_range(0.0..=1.0),
                config: config.clone(),
                age: 0.0,
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
    pub fn clear_particles(&mut self) {
        self.particles.clear();
    }
    pub fn clear(&mut self) {
        self.particles.clear();
        self.emission_credit.clear();
        self.enabled_overrides.clear();
    }

    pub fn set_emitter_enabled(&mut self, id: u64, enabled: bool) {
        self.enabled_overrides.insert(id, enabled);
        self.emission_credit.remove(&id);
    }

    pub fn update_emitters(&mut self, dt: f32, emitters: &[(u64, Vec3, EmitterSettings)]) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.update(dt);
        let active: std::collections::HashSet<_> = emitters.iter().map(|(id, _, _)| *id).collect();
        self.emission_credit.retain(|id, _| active.contains(id));
        self.enabled_overrides.retain(|id, _| active.contains(id));
        let mut rng = rand::thread_rng();
        for (id, position, settings) in emitters {
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
            // Cap catch-up after stalls; neither a paused window nor a full budget
            // accumulates an unbounded burst of deferred emission.
            let credit = self.emission_credit.entry(*id).or_default();
            *credit += dt.min(0.25) * settings.rate;
            let count = credit.floor() as usize;
            *credit -= count as f32;
            let count = count.min(MAX_PARTICLES.saturating_sub(self.particles.len()));
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
                    // Distribute births through the frame instead of forming visible rings.
                    if let Some(particle) = self.particles.last_mut() {
                        particle.age = dt.min(0.25) * i as f32 / count.max(1) as f32;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
