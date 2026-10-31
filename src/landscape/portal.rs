// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::mesh::{IndexedMesh, MeshError};

fn same_directed_cycle(a: &[u32], b: &[u32]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.is_empty() {
        return true;
    }
    let Some(offset) = b.iter().position(|&id| id == a[0]) else {
        return false;
    };
    a.iter()
        .enumerate()
        .all(|(i, id)| *id == b[(i + offset) % b.len()])
}

fn axis_projection(positions: &[[f64; 3]]) -> Result<Vec<[f64; 2]>, MeshError> {
    let mut normal = [0.0; 3];
    for i in 0..positions.len() {
        let p = positions[i];
        let q = positions[(i + 1) % positions.len()];
        normal[0] += (p[1] - q[1]) * (p[2] + q[2]);
        normal[1] += (p[2] - q[2]) * (p[0] + q[0]);
        normal[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let (axis, max) = normal
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .ok_or(MeshError::InvalidOperation)?;
    if !max.is_finite() || max.abs() < 1e-12 {
        return Err(MeshError::InvalidOperation);
    }
    Ok(positions
        .iter()
        .map(|p| match axis {
            0 => [p[1], p[2]],
            1 => [p[0], p[2]],
            _ => [p[0], p[1]],
        })
        .collect())
}

fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn triangulate_reverse_cap(mesh: &IndexedMesh, rim: &[u32]) -> Result<Vec<[u32; 3]>, MeshError> {
    let points: Vec<_> = rim
        .iter()
        .map(|&id| mesh.vertices[id as usize].position)
        .collect();
    let projected = axis_projection(&points)?;
    let mut area = 0.0;
    for i in 0..projected.len() {
        let p = projected[i];
        let q = projected[(i + 1) % projected.len()];
        area += p[0] * q[1] - q[0] * p[1];
    }
    if area.abs() < 1e-12 || !area.is_finite() {
        return Err(MeshError::InvalidOperation);
    }
    let sign = area.signum();
    let mut polygon: Vec<usize> = (0..rim.len()).collect();
    let mut triangles = Vec::with_capacity(rim.len() - 2);
    while polygon.len() > 3 {
        let n = polygon.len();
        let mut ear = None;
        for i in 0..n {
            let (prev, curr, next) = (polygon[(i + n - 1) % n], polygon[i], polygon[(i + 1) % n]);
            let a = projected[prev];
            let b = projected[curr];
            let c = projected[next];
            if orient(a, b, c) * sign <= 1e-12 {
                continue;
            }
            let contains_other = polygon.iter().any(|&j| {
                if j == prev || j == curr || j == next {
                    return false;
                }
                let p = projected[j];
                let ab = orient(a, b, p) * sign;
                let bc = orient(b, c, p) * sign;
                let ca = orient(c, a, p) * sign;
                ab >= -1e-12 && bc >= -1e-12 && ca >= -1e-12
            });
            if !contains_other {
                ear = Some((i, [rim[prev], rim[next], rim[curr]]));
                break;
            }
        }
        let (index, face) = ear.ok_or(MeshError::InvalidOperation)?;
        triangles.push(face);
        polygon.remove(index);
    }
    triangles.push([rim[polygon[0]], rim[polygon[2]], rim[polygon[1]]]);
    Ok(triangles)
}

impl IndexedMesh {
    pub fn extrude_boundary_path(
        &mut self,
        opening: &[u32],
        offsets: &[[f64; 3]],
        cap_end: bool,
    ) -> Result<Vec<u32>, MeshError> {
        self.validate()?;
        if opening.len() < 3
            || offsets.is_empty()
            || offsets.iter().any(|v| v.iter().any(|c| !c.is_finite()))
            || opening.iter().any(|&id| id as usize >= self.vertices.len())
        {
            return Err(MeshError::InvalidOperation);
        }
        let loops = self.boundary_loops()?;
        if !loops
            .iter()
            .any(|boundary| same_directed_cycle(opening, boundary))
        {
            return Err(MeshError::InvalidOperation);
        }
        let new_vertices = opening
            .len()
            .checked_mul(offsets.len())
            .ok_or(MeshError::TooManyVertices)?;
        if self
            .vertices
            .len()
            .checked_add(new_vertices)
            .is_none_or(|n| n > u32::MAX as usize)
        {
            return Err(MeshError::TooManyVertices);
        }

        let mut candidate = self.clone();
        let mut previous = opening.to_vec();
        let base_positions: Vec<_> = opening
            .iter()
            .map(|&id| self.vertices[id as usize].position)
            .collect();
        for offset in offsets {
            let next: Vec<u32> = opening
                .iter()
                .enumerate()
                .map(|(i, &source_id)| {
                    let mut vertex = self.vertices[source_id as usize].clone();
                    for axis in 0..3 {
                        vertex.position[axis] = base_positions[i][axis] + offset[axis];
                    }
                    let id = candidate.vertices.len() as u32;
                    candidate.vertices.push(vertex);
                    id
                })
                .collect();
            for i in 0..previous.len() {
                let j = (i + 1) % previous.len();
                candidate
                    .triangles
                    .push([previous[j], previous[i], next[i]]);
                candidate.triangles.push([previous[j], next[i], next[j]]);
            }
            previous = next;
        }
        if cap_end {
            let cap = triangulate_reverse_cap(&candidate, &previous)?;
            candidate.triangles.extend(cap);
        }
        candidate.validate()?;
        candidate.recompute_normals()?;
        *self = candidate;
        Ok(previous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::heightmap::Heightmap;

    fn terrain_with_hole() -> (IndexedMesh, Vec<u32>) {
        let h = Heightmap::new(5, 5, [0.0; 3], 1.0, vec![0.0; 25]).unwrap();
        let mut mesh = h.mesh().unwrap();
        mesh.remove_faces(&[10, 11]).unwrap();
        let inner = mesh
            .boundary_loops()
            .unwrap()
            .into_iter()
            .find(|loop_ids| loop_ids.len() == 4)
            .unwrap();
        (mesh, inner)
    }

    #[test]
    fn blind_cave_has_genuine_walls_and_a_closed_end() {
        let (mut mesh, opening) = terrain_with_hole();
        let old = mesh.validate().unwrap();
        assert_eq!(old.boundary_edges, 20);
        let terminal = mesh
            .extrude_boundary_path(&opening, &[[0.0, -2.0, 0.0], [0.0, -4.0, 1.0]], true)
            .unwrap();
        assert_eq!(terminal.len(), 4);
        let updated = mesh.validate().unwrap();
        assert_eq!(updated.boundary_edges, 16);
        assert_eq!(mesh.boundary_loops().unwrap().len(), 1);
        assert!(
            terminal
                .iter()
                .all(|&id| mesh.vertices[id as usize].position[1] == -4.0)
        );
    }

    #[test]
    fn open_tunnel_end_preserves_separate_boundary() {
        let (mut mesh, opening) = terrain_with_hole();
        mesh.extrude_boundary_path(&opening, &[[0.0, -3.0, 0.0]], false)
            .unwrap();
        assert_eq!(mesh.boundary_loops().unwrap().len(), 2);
        assert_eq!(mesh.validate().unwrap().boundary_edges, 20);
    }

    #[test]
    fn rejected_invalid_path_is_atomic() {
        let (mut mesh, opening) = terrain_with_hole();
        let old = mesh.clone();
        assert_eq!(
            mesh.extrude_boundary_path(&opening, &[[f64::NAN, 0.0, 0.0]], true),
            Err(MeshError::InvalidOperation),
        );
        assert_eq!(mesh, old);
        let mut reverse = opening.clone();
        reverse.reverse();
        assert_eq!(
            mesh.extrude_boundary_path(&reverse, &[[0.0, -2.0, 0.0]], true),
            Err(MeshError::InvalidOperation),
        );
        assert_eq!(mesh, old);
    }
}
