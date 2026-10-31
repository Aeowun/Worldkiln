// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::World;

impl World {
    pub fn update_particles(&mut self, dt: f32) {
        let mut emitters: Vec<_> = self
            .particle_emitter_ids
            .iter()
            .filter_map(|id| {
                if let Some(source) = self.source(*id) {
                    return Some((
                        *id,
                        self.source_position(*id).unwrap_or(source.position),
                        source.cell.particles.clone(),
                    ));
                }
                let coord = self.resolve_cell_id(*id)?;
                let cell = self.get_effective_cell_by_id(*id)?;
                let offset = self
                    .runtime_state
                    .get(id)
                    .and_then(|state| state.visual_offset)
                    .unwrap_or(glam::Vec3::ZERO);
                Some((
                    *id,
                    glam::Vec3::new(
                        coord.x as f32 + 0.5,
                        coord.y as f32 + 0.5,
                        coord.z as f32 + 0.5,
                    ) + offset,
                    cell.particles.clone(),
                ))
            })
            .collect();
        emitters.sort_by_key(|(id, _, _)| *id);
        self.particles.update_emitters(dt, &emitters);
    }

    pub fn fire_emitter_samples(&self) -> Vec<(u64, glam::Vec3, f32, f32, f32)> {
        use crate::engine::particles::EmitterPreset;
        let mut result = Vec::new();
        for id in &self.particle_emitter_ids {
            let data = if let Some(source) = self.source(*id) {
                Some((
                    self.source_position(*id).unwrap_or(source.position),
                    &source.cell.particles,
                ))
            } else {
                let Some(coord) = self.resolve_cell_id(*id) else {
                    continue;
                };
                let Some(cell) = self.get_effective_cell_by_id(*id) else {
                    continue;
                };
                let offset = self
                    .runtime_state
                    .get(id)
                    .and_then(|state| state.visual_offset)
                    .unwrap_or(glam::Vec3::ZERO);
                let position = glam::Vec3::new(
                    coord.x as f32 + 0.5,
                    coord.y as f32 + 0.5,
                    coord.z as f32 + 0.5,
                ) + offset;
                if cell.particles.effect_asset.is_none()
                    && cell.particles.preset == EmitterPreset::Fire
                    && cell.particles.validate().is_ok()
                    && cell.particles.rate > 0.0
                    && self.particles.emitter_enabled(*id, &cell.particles)
                {
                    result.push((
                        *id,
                        position,
                        cell.particles.scale,
                        cell.particles.rate,
                        cell.particles.radius,
                    ));
                }
                None
            };
            if let Some((position, settings)) = data {
                if settings.effect_asset.is_none()
                    && settings.preset == EmitterPreset::Fire
                    && settings.validate().is_ok()
                    && settings.rate > 0.0
                    && self.particles.emitter_enabled(*id, settings)
                {
                    result.push((
                        *id,
                        position,
                        settings.scale,
                        settings.rate,
                        settings.radius,
                    ));
                }
            }
        }
        result.sort_by_key(|(id, _, _, _, _)| *id);
        result
    }
    pub fn fx_emitter_samples(&self) -> Vec<(String, u64, glam::Vec3, f32, f32, f32)> {
        let mut results = Vec::new();
        for id in &self.particle_emitter_ids {
            let data = if let Some(source) = self.source(*id) {
                Some((
                    self.source_position(*id).unwrap_or(source.position),
                    source.cell.particles.clone(),
                ))
            } else {
                let Some(coord) = self.resolve_cell_id(*id) else {
                    continue;
                };
                let Some(cell) = self.get_effective_cell_by_id(*id) else {
                    continue;
                };
                let offset = self
                    .runtime_state
                    .get(id)
                    .and_then(|state| state.visual_offset)
                    .unwrap_or(glam::Vec3::ZERO);
                Some((
                    glam::Vec3::new(
                        coord.x as f32 + 0.5,
                        coord.y as f32 + 0.5,
                        coord.z as f32 + 0.5,
                    ) + offset,
                    cell.particles.clone(),
                ))
            };
            let Some((pos, settings)) = data else {
                continue;
            };
            let Some(asset) = settings.fx_asset_id() else {
                continue;
            };
            if settings.validate().is_ok()
                && settings.rate > 0.0
                && self.particles.emitter_enabled(*id, &settings)
            {
                results.push((
                    asset.to_string(),
                    *id,
                    pos,
                    settings.scale,
                    settings.rate,
                    settings.radius,
                ));
            }
        }
        results.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CellType;

    #[test]
    fn fire_effect_follows_existing_emitter_preset_and_enable_state() {
        use crate::engine::particles::EmitterPreset;
        let mut world = World::new();
        let position = glam::Vec3::new(-0.5, 0.5, -0.5);
        let id = world.add_source(CellType::ParticleEmitter, position);
        assert_eq!(world.fire_emitter_samples().len(), 1);
        assert_eq!(world.fire_emitter_samples()[0].1, position);
        world.source_mut(id).unwrap().cell.particles.preset = EmitterPreset::Smoke;
        assert!(world.fire_emitter_samples().is_empty());
        world.source_mut(id).unwrap().cell.particles.preset = EmitterPreset::Fire;
        world.particles.set_emitter_enabled(id, false);
        assert!(world.fire_emitter_samples().is_empty());
        world.particles.set_emitter_enabled(id, true);
        assert_eq!(world.fire_emitter_samples().len(), 1);
    }

    #[test]
    fn project_effect_emitter_has_no_legacy_fire_or_cpu_fallback() {
        use crate::engine::particles::EmitterSettings;
        let mut world = World::new();
        let position = glam::Vec3::new(1.25, 3.5, -7.0);
        let source_id = world.add_source(CellType::ParticleEmitter, position);
        let source = world.source_mut(source_id).unwrap();
        source.cell.particles = EmitterSettings::blank_fx_emitter();
        assert!(world.fire_emitter_samples().is_empty());
        assert!(world.fx_emitter_samples().is_empty());
        world
            .source_mut(source_id)
            .unwrap()
            .cell
            .particles
            .effect_asset = Some("Fire/Campfire.efkefc".into());
        assert!(world.fire_emitter_samples().is_empty());
        let sources = world.fx_emitter_samples();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].0, "Fire/Campfire.efkefc");
        assert_eq!(sources[0].1, source_id);
        assert_eq!(sources[0].2, position);
        world.update_particles(1.0);
        assert!(world.particles.particles().is_empty());
        world.particles.set_emitter_enabled(source_id, false);
        assert!(world.fx_emitter_samples().is_empty());
        world.particles.set_emitter_enabled(source_id, true);
        assert_eq!(world.fx_emitter_samples().len(), 1);
    }

    #[test]
    fn particle_emitter_uses_exact_freeform_source_position() {
        let mut world = World::new();
        let position = glam::Vec3::new(-3.75, 4.125, 9.5);
        let id = world.add_source(CellType::ParticleEmitter, position);
        let source = world.source_mut(id).unwrap();
        source.cell.particles.rate = 10.0;
        source.cell.particles.radius = 0.0;

        world.update_particles(0.1);

        assert_eq!(world.particles.particles().len(), 1);
        assert_eq!(world.particles.particles()[0].position(), position);
    }

    #[test]
    fn runtime_particle_emitter_is_registered_and_updates_at_its_center() {
        let mut world = World::new();
        let coord = crate::world::WorldCoord::new(2, 3, 4);
        let id = world.create_runtime_cell(CellType::ParticleEmitter);
        world.move_runtime_cell(id, coord).unwrap();
        let cell = world.runtime_cells.get_mut(&id).unwrap();
        cell.particles.rate = 10.0;
        cell.particles.radius = 0.0;

        world.update_particles(0.1);

        assert_eq!(world.particles.particles().len(), 1);
        assert_eq!(
            world.particles.particles()[0].position(),
            glam::Vec3::new(2.5, 3.5, 4.5)
        );
    }
}
