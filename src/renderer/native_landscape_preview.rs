// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::terrain::{TerrainMaterialSlot, TerrainMesh, TerrainMeshBatch};
use crate::landscape::{IndexedMesh, legacy_import::import_legacy_surface_tile_with_validity};
use crate::world::{TerrainConfig, TerrainResult};

pub(super) fn enabled() -> bool {
    true
}

pub(super) fn triangulated_vertex_stream(mesh: &IndexedMesh) -> Vec<f32> {
    let mut data = Vec::with_capacity(mesh.triangles.len() * 3 * 12);
    for face in &mesh.triangles {
        for &vertex_index in face {
            let vertex = &mesh.vertices[vertex_index as usize];
            data.extend_from_slice(&[
                vertex.position[0] as f32,
                vertex.position[1] as f32,
                vertex.position[2] as f32,
                vertex.normal[0],
                vertex.normal[1],
                vertex.normal[2],
                1.0,
                1.0,
                1.0,
                1.0,
                vertex.uv[0] * 0.125,
                vertex.uv[1] * 0.125,
            ]);
        }
    }
    data
}

pub(super) fn native_surface_for_legacy_chunk(
    result: &TerrainResult,
    config: &TerrainConfig,
    x_start: u32,
    x_end: u32,
    z_start: u32,
    z_end: u32,
) -> Option<TerrainMesh> {
    let (source, occupied) = import_legacy_surface_tile_with_validity(
        result,
        x_start as usize,
        x_end as usize,
        z_start as usize,
        z_end as usize,
    )
    .ok()?;
    if x_end <= x_start || z_end <= z_start {
        return None;
    }
    let polygon_mesh = source
        .tile_mesh(
            1,
            1,
            (x_end - x_start) as usize,
            (z_end - z_start) as usize,
            1,
        )
        .ok()?;
    let vertex_cols = (x_end - x_start) as usize + 1;
    let sample_stride = (x_end - x_start) as usize + 3;
    let vertex_occupied: Vec<bool> = (0..polygon_mesh.vertices.len())
        .map(|index| {
            let x = index % vertex_cols;
            let z = index / vertex_cols;
            occupied[(z + 1) * sample_stride + x + 1]
        })
        .collect();
    let batches = match &result.data {
        crate::world::TerrainData::Voxel(grid) => {
            let palette: Vec<_> = grid
                .materials
                .iter()
                .map(|name| {
                    if name == &config.materials.subsurface && !name.is_empty() {
                        1
                    } else if name == &config.materials.rock && !name.is_empty() {
                        2
                    } else {
                        super::terrain::terrain_palette_layer(name)
                    }
                })
                .collect();
            let weights_by_vertex: Vec<_> = polygon_mesh
                .vertices
                .iter()
                .map(|point| {
                    let position = glam::Vec3::new(
                        point.position[0] as f32,
                        point.position[1] as f32,
                        point.position[2] as f32,
                    );
                    super::terrain::voxel_layers_and_normal_with_palette(
                        grid,
                        position,
                        0,
                        glam::Vec3::Y,
                        Some(&palette),
                    )
                    .0
                })
                .collect();
            let mut vertices = Vec::with_capacity(polygon_mesh.triangles.len() * 3 * 12);
            for face in &polygon_mesh.triangles {
                if !face.iter().all(|&i| vertex_occupied[i as usize]) {
                    continue;
                }
                for &index in face {
                    let point = &polygon_mesh.vertices[index as usize];
                    let position = glam::Vec3::new(
                        point.position[0] as f32,
                        point.position[1] as f32,
                        point.position[2] as f32,
                    );
                    let weights = weights_by_vertex[index as usize];
                    vertices.extend_from_slice(&[
                        position.x,
                        position.y,
                        position.z,
                        point.normal[0],
                        point.normal[1],
                        point.normal[2],
                        weights[0],
                        weights[1],
                        weights[2],
                        weights[3],
                        weights[4],
                        weights[5],
                    ]);
                }
            }
            vec![TerrainMeshBatch {
                material: TerrainMaterialSlot::Palette(0),
                vertices,
                water_body_id: None,
            }]
        }
        crate::world::TerrainData::Heightfield(_) => {
            let mut valid = polygon_mesh.clone();
            valid
                .triangles
                .retain(|face| face.iter().all(|&i| vertex_occupied[i as usize]));
            vec![TerrainMeshBatch {
                material: TerrainMaterialSlot::Surface,
                vertices: triangulated_vertex_stream(&valid),
                water_body_id: None,
            }]
        }
    };
    Some(TerrainMesh {
        source_signature: 0,
        batches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{Heightmap, Vertex};
    #[test]
    fn native_surface_stream_preserves_real_triangle_normals_and_positions() {
        let mut mesh = IndexedMesh {
            vertices: vec![
                Vertex::new([0.0, 0.0, 0.0], [0.0, 0.0]),
                Vertex::new([2.0, 1.0, 0.0], [2.0, 0.0]),
                Vertex::new([0.0, 0.0, 2.0], [0.0, 2.0]),
            ],
            triangles: vec![[0, 2, 1]],
        };
        mesh.recompute_normals().unwrap();
        let stream = triangulated_vertex_stream(&mesh);
        assert_eq!(stream.len(), 36);
        assert_eq!(&stream[..3], &[0.0, 0.0, 0.0]);
        assert!(stream[4] > 0.8);
        assert_eq!(stream[10], 0.0);
        assert_eq!(stream[11], 0.0);
    }
    #[test]
    fn authored_paint_reaches_native_surface_material_batches_without_restart() {
        use crate::world::{TerrainData, TerrainGenerationRequest, TerrainPaintRequest, World};
        let mut world = World::new();
        world.terrain.config.plains.base_height = 8.0;
        world.terrain.config.plains.height_variation = 0.0;
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 16, 16],
            })
            .unwrap();
        let original = world.terrain.effective().unwrap();
        let sample_size = match &original.data {
            TerrainData::Voxel(grid) => grid.size[0],
            _ => panic!("expected migration fixture"),
        };
        let before = native_surface_for_legacy_chunk(
            original,
            &world.terrain.config,
            0,
            sample_size,
            0,
            sample_size,
        )
        .unwrap();
        let revision = world.terrain.render_revision();
        assert!(world.paint_authored_terrain(&TerrainPaintRequest {
            strength: 1.0,
            falloff: None,
            center: [6.0, 8.0, 6.0],
            radius: 4.0,
            material: "PBR:Rock".into(),
        }));
        assert!(world.terrain.render_revision() > revision);
        let changed = world.terrain.effective().unwrap();
        let after = native_surface_for_legacy_chunk(
            changed,
            &world.terrain.config,
            0,
            sample_size,
            0,
            sample_size,
        )
        .unwrap();
        assert_eq!(before.batches.len(), 1);
        assert_eq!(after.batches.len(), 1);
        let rock_sum = |mesh: &TerrainMesh| -> f32 {
            mesh.batches
                .iter()
                .flat_map(|batch| batch.vertices.chunks_exact(12))
                .map(|vertex| vertex[8])
                .sum::<f32>()
        };
        assert!(
            rock_sum(&after) > rock_sum(&before),
            "painting rock must increase the actual native GPU Rock weight"
        );
    }
    #[test]
    fn actual_native_terrain_batches_preserve_layer_weights_not_white_palette() {
        use crate::world::{TerrainData, TerrainGenerationRequest, World};
        let mut world = World::new();
        world.terrain.config.plains.base_height = 9.0;
        world.terrain.config.plains.height_variation = 1.0;
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 24, 16],
            })
            .unwrap();
        let result = world.terrain.effective().unwrap();
        let TerrainData::Voxel(grid) = &result.data else {
            panic!("voxel terrain expected")
        };
        let mesh = native_surface_for_legacy_chunk(
            result,
            &world.terrain.config,
            0,
            grid.size[0],
            0,
            grid.size[2],
        )
        .unwrap();
        let mut vertices = 0;
        for batch in mesh.batches {
            for attributes in batch.vertices.chunks_exact(12) {
                let primary = &attributes[6..10];
                let secondary = &attributes[10..12];
                let sum = primary.iter().chain(secondary.iter()).sum::<f32>();
                assert!(
                    (sum - 1.0).abs() < 0.001,
                    "active native stream must carry normalized six-way weights"
                );
                vertices += 1;
            }
        }
        assert!(vertices > 0);
    }

    #[test]
    fn heightmap_native_mesh_stream_is_fully_polygonal() {
        let heightmap = Heightmap::new(4, 4, [0.0, 0.0, 0.0], 1.0, vec![2.0; 16]).unwrap();
        let polygon = heightmap.mesh().unwrap();
        assert_eq!(triangulated_vertex_stream(&polygon).len(), 18 * 3 * 12);
    }
}
