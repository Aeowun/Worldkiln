use glam::Vec3;

use super::{Cell, CellType};

#[derive(Clone, Debug, PartialEq)]
pub struct AuthoredSource {
    pub position: Vec3,
    pub cell: Cell,
}

impl AuthoredSource {
    pub fn new(id: u64, source_type: CellType, position: Vec3) -> Self {
        assert!(Self::supports(source_type));
        let mut cell = match source_type {
            CellType::Light => Cell::new_light(),
            CellType::AudioEmitter => Cell::new_audio_emitter(),
            CellType::ParticleEmitter => {
                Cell::new_particle_emitter(crate::engine::particles::EmitterPreset::Fire)
            }
            _ => unreachable!(),
        };
        cell.id = id;
        Self { position, cell }
    }

    pub fn supports(cell_type: CellType) -> bool {
        matches!(
            cell_type,
            CellType::Light | CellType::AudioEmitter | CellType::ParticleEmitter
        )
    }

    pub fn source_type(&self) -> CellType {
        self.cell.cell_type
    }
}
