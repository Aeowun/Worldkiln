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
                // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
                // Runtime sources are ID-addressed so Light/ParticleEmitter
                // effects may intentionally occupy the same center coordinate.
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
        // Stable ordering makes budget pressure deterministic across loads.
        emitters.sort_by_key(|(id, _, _)| *id);
        self.particles.update_emitters(dt, &emitters);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::CellType;

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

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
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
