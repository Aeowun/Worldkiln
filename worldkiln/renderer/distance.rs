//! Rendering scope only: meshes stay resident and simulation is unaffected.
use crate::world::{CHUNK_SIZE, ChunkCoord};
use glam::Vec3;

pub fn within_chunk_radius(chunk: ChunkCoord, focus: Vec3, radius: i32) -> bool {
    // Zero means Unlimited for legacy projects. Y is intentionally ignored.
    if radius <= 0 {
        return true;
    }
    let x = (focus.x / CHUNK_SIZE as f32).floor() as i64;
    let z = (focus.z / CHUNK_SIZE as f32).floor() as i64;
    (i64::from(chunk.x) - x).abs() <= i64::from(radius)
        && (i64::from(chunk.z) - z).abs() <= i64::from(radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unlimited_accepts_any_column() {
        assert!(within_chunk_radius(
            ChunkCoord::new(999, -999, -999),
            Vec3::ZERO,
            0
        ));
    }
    #[test]
    fn inclusive_horizontal_boundary_ignores_height() {
        for y in [-100, 0, 100] {
            assert!(within_chunk_radius(
                ChunkCoord::new(-4, y, 4),
                Vec3::ZERO,
                4
            ));
            assert!(!within_chunk_radius(
                ChunkCoord::new(5, y, 0),
                Vec3::ZERO,
                4
            ));
            assert!(!within_chunk_radius(
                ChunkCoord::new(0, y, -5),
                Vec3::ZERO,
                4
            ));
        }
    }
    #[test]
    fn negative_focus_and_chunk_boundaries_use_floor() {
        let focus = Vec3::new(-0.01, 500.0, -16.01);
        assert!(within_chunk_radius(ChunkCoord::new(-2, 0, -3), focus, 1));
        assert!(!within_chunk_radius(ChunkCoord::new(1, 0, -2), focus, 1));
        assert!(within_chunk_radius(
            ChunkCoord::new(2, 0, 0),
            Vec3::new(16.0, 0.0, 0.0),
            1
        ));
        assert!(!within_chunk_radius(
            ChunkCoord::new(2, 0, 0),
            Vec3::new(15.99, 0.0, 0.0),
            1
        ));
    }
}
