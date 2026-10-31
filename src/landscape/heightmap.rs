// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::mesh::{IndexedMesh, MeshError, Vertex};

#[derive(Clone, Debug, PartialEq)]
pub enum HeightmapError {
    InvalidDimensions,
    InvalidSpacing,
    InvalidSamples,
    InvalidBounds,
    Mesh(MeshError),
}

impl From<MeshError> for HeightmapError {
    fn from(value: MeshError) -> Self {
        Self::Mesh(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heightmap {
    pub columns: usize,
    pub rows: usize,
    pub origin: [f64; 3],
    pub spacing: f64,
    pub heights: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceLod {
    pub sample_step: usize,
    pub max_vertical_error: f64,
    pub mesh: IndexedMesh,
}

impl Heightmap {
    pub fn new(
        columns: usize,
        rows: usize,
        origin: [f64; 3],
        spacing: f64,
        heights: Vec<f64>,
    ) -> Result<Self, HeightmapError> {
        if columns < 2 || rows < 2 || columns.checked_mul(rows) != Some(heights.len()) {
            return Err(HeightmapError::InvalidDimensions);
        }
        if !spacing.is_finite() || spacing <= 0.0 || !origin.iter().all(|v| v.is_finite()) {
            return Err(HeightmapError::InvalidSpacing);
        }
        if !heights.iter().all(|v| v.is_finite()) {
            return Err(HeightmapError::InvalidSamples);
        }
        Ok(Self {
            columns,
            rows,
            origin,
            spacing,
            heights,
        })
    }

    pub fn height(&self, x: usize, z: usize) -> f64 {
        self.heights[z * self.columns + x]
    }

    fn derivative(&self, x: usize, z: usize, x_axis: bool) -> f64 {
        let (current, max) = if x_axis {
            (x, self.columns)
        } else {
            (z, self.rows)
        };
        let lower = current.saturating_sub(1);
        let upper = (current + 1).min(max - 1);
        if upper == lower {
            return 0.0;
        }
        let low_height = if x_axis {
            self.height(lower, z)
        } else {
            self.height(x, lower)
        };
        let high_height = if x_axis {
            self.height(upper, z)
        } else {
            self.height(x, upper)
        };
        (high_height - low_height) / ((upper - lower) as f64 * self.spacing)
    }

    fn vertex(&self, x: usize, z: usize) -> Vertex {
        let mut vertex = Vertex::new(
            [
                self.origin[0] + x as f64 * self.spacing,
                self.origin[1] + self.height(x, z),
                self.origin[2] + z as f64 * self.spacing,
            ],
            [x as f32, z as f32],
        );
        let nx = -self.derivative(x, z, true);
        let nz = -self.derivative(x, z, false);
        let magnitude = (nx * nx + 1.0 + nz * nz).sqrt();
        vertex.normal = [
            (nx / magnitude) as f32,
            (1.0 / magnitude) as f32,
            (nz / magnitude) as f32,
        ];
        vertex
    }

    pub fn mesh(&self) -> Result<IndexedMesh, HeightmapError> {
        self.tile_mesh(0, 0, self.columns - 1, self.rows - 1, 1)
    }

    pub fn tile_mesh(
        &self,
        start_x: usize,
        start_z: usize,
        quads_x: usize,
        quads_z: usize,
        step: usize,
    ) -> Result<IndexedMesh, HeightmapError> {
        if step == 0
            || quads_x == 0
            || quads_z == 0
            || start_x
                .checked_add(quads_x)
                .is_none_or(|end| end >= self.columns)
            || start_z
                .checked_add(quads_z)
                .is_none_or(|end| end >= self.rows)
        {
            return Err(HeightmapError::InvalidBounds);
        }
        let xs = sample_coordinates(start_x, start_x + quads_x, step);
        let zs = sample_coordinates(start_z, start_z + quads_z, step);
        let capacity = xs
            .len()
            .checked_mul(zs.len())
            .ok_or(HeightmapError::InvalidDimensions)?;
        if capacity > u32::MAX as usize {
            return Err(HeightmapError::InvalidDimensions);
        }
        let mut vertices = Vec::with_capacity(capacity);
        for &z in &zs {
            for &x in &xs {
                vertices.push(self.vertex(x, z));
            }
        }
        let mut triangles = Vec::with_capacity((xs.len() - 1) * (zs.len() - 1) * 2);
        for z in 0..zs.len() - 1 {
            for x in 0..xs.len() - 1 {
                let a = (z * xs.len() + x) as u32;
                let b = a + 1;
                let c = a + xs.len() as u32;
                let d = c + 1;
                triangles.push([a, c, b]);
                triangles.push([b, c, d]);
            }
        }
        let mesh = IndexedMesh {
            vertices,
            triangles,
        };
        mesh.validate()?;
        Ok(mesh)
    }

    pub fn tile_lod(
        &self,
        start_x: usize,
        start_z: usize,
        quads_x: usize,
        quads_z: usize,
        step: usize,
    ) -> Result<SurfaceLod, HeightmapError> {
        let mesh = self.tile_mesh(start_x, start_z, quads_x, quads_z, step)?;
        let xs = sample_coordinates(start_x, start_x + quads_x, step);
        let zs = sample_coordinates(start_z, start_z + quads_z, step);
        let mut error: f64 = 0.0;
        for zi in 0..zs.len() - 1 {
            for xi in 0..xs.len() - 1 {
                let (x0, x1) = (xs[xi], xs[xi + 1]);
                let (z0, z1) = (zs[zi], zs[zi + 1]);
                for z in z0..=z1 {
                    for x in x0..=x1 {
                        let u = (x - x0) as f64 / (x1 - x0) as f64;
                        let v = (z - z0) as f64 / (z1 - z0) as f64;
                        let interpolated = if u + v <= 1.0 {
                            (1.0 - u - v) * self.height(x0, z0)
                                + u * self.height(x1, z0)
                                + v * self.height(x0, z1)
                        } else {
                            (u + v - 1.0) * self.height(x1, z1)
                                + (1.0 - v) * self.height(x1, z0)
                                + (1.0 - u) * self.height(x0, z1)
                        };
                        error = error.max((self.height(x, z) - interpolated).abs());
                    }
                }
            }
        }
        Ok(SurfaceLod {
            sample_step: step,
            max_vertical_error: error,
            mesh,
        })
    }
}

fn sample_coordinates(start: usize, end: usize, stride: usize) -> Vec<usize> {
    let mut indices = vec![start];
    let mut current = start;
    while current < end {
        current = current.saturating_add(stride).min(end);
        indices.push(current);
    }
    indices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_grid_is_indexed_and_upward_facing() {
        let data = Heightmap::new(3, 3, [1000.0, 0.0, -250.0], 2.0, vec![4.0; 9]).unwrap();
        let mesh = data.mesh().unwrap();
        assert_eq!((mesh.vertices.len(), mesh.triangles.len()), (9, 8));
        assert_eq!(mesh.validate().unwrap().boundary_edges, 8);
        assert!(mesh.vertices.iter().all(|v| v.normal[1] > 0.99));
        assert_eq!(mesh.vertices[0].position, [1000.0, 4.0, -250.0]);
    }

    #[test]
    fn neighboring_tiles_share_exact_boundary_positions_and_normals() {
        let heights: Vec<f64> = (0..5)
            .flat_map(|z| (0..5).map(move |x| (x * x + z) as f64))
            .collect();
        let source = Heightmap::new(5, 5, [0.0; 3], 1.0, heights).unwrap();
        let a = source.tile_mesh(0, 0, 2, 4, 1).unwrap();
        let b = source.tile_mesh(2, 0, 2, 4, 1).unwrap();
        for z in 0..5 {
            assert_eq!(a.vertices[z * 3 + 2].position, b.vertices[z * 3].position);
            assert_eq!(a.vertices[z * 3 + 2].normal, b.vertices[z * 3].normal);
        }
    }

    #[test]
    fn geometric_lod_error_is_zero_for_a_plane_and_positive_for_peak() {
        let flat = Heightmap::new(
            5,
            5,
            [0.0; 3],
            1.0,
            (0..25).map(|i| (i % 5 + i / 5) as f64).collect(),
        )
        .unwrap();
        assert!(flat.tile_lod(0, 0, 4, 4, 2).unwrap().max_vertical_error < 1e-10);
        let mut peak = flat.clone();
        peak.heights[2 + 2 * 5] += 12.0;
        assert!(peak.tile_lod(0, 0, 4, 4, 4).unwrap().max_vertical_error >= 12.0);
    }

    #[test]
    fn malformed_heights_and_overflowing_bounds_rejected() {
        assert!(Heightmap::new(2, 2, [0.0; 3], 0.0, vec![0.0; 4]).is_err());
        assert!(Heightmap::new(2, 2, [0.0; 3], 1.0, vec![f64::NAN; 4]).is_err());
        let data = Heightmap::new(2, 2, [0.0; 3], 1.0, vec![0.0; 4]).unwrap();
        assert!(data.tile_mesh(usize::MAX, 0, 1, 1, 1).is_err());
    }
}
