// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f64; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

impl Vertex {
    pub fn new(position: [f64; 3], uv: [f32; 2]) -> Self {
        Self {
            position,
            normal: [0.0, 1.0, 0.0],
            uv,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexedMesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<[u32; 3]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    EmptyMesh,
    NonFiniteVertex(usize),
    InvalidIndex(usize),
    DegenerateTriangle(usize),
    NonManifoldEdge(u32, u32),
    InconsistentWinding(u32, u32),
    EdgeNotFound(u32, u32),
    InvalidOperation,
    TooManyVertices,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopologyReport {
    pub vertices: usize,
    pub triangles: usize,
    pub edges: usize,
    pub boundary_edges: usize,
}

impl TopologyReport {
    pub fn is_closed(&self) -> bool {
        self.boundary_edges == 0
    }
}

fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn length(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

impl IndexedMesh {
    pub fn validate(&self) -> Result<TopologyReport, MeshError> {
        if self.vertices.is_empty() || self.triangles.is_empty() {
            return Err(MeshError::EmptyMesh);
        }
        for (i, vertex) in self.vertices.iter().enumerate() {
            if !vertex.position.iter().all(|v| v.is_finite())
                || !vertex.normal.iter().all(|v| v.is_finite())
                || !vertex.uv.iter().all(|v| v.is_finite())
            {
                return Err(MeshError::NonFiniteVertex(i));
            }
        }
        let mut edges: BTreeMap<(u32, u32), (usize, bool)> = BTreeMap::new();
        for (face_index, face) in self.triangles.iter().enumerate() {
            if face.iter().any(|&i| i as usize >= self.vertices.len()) {
                return Err(MeshError::InvalidIndex(face_index));
            }
            if face[0] == face[1] || face[1] == face[2] || face[2] == face[0] {
                return Err(MeshError::DegenerateTriangle(face_index));
            }
            let a = self.vertices[face[0] as usize].position;
            let b = self.vertices[face[1] as usize].position;
            let c = self.vertices[face[2] as usize].position;
            let area_twice = length(cross(subtract(b, a), subtract(c, a)));
            if !area_twice.is_finite() || area_twice <= 1e-12 {
                return Err(MeshError::DegenerateTriangle(face_index));
            }
            for edge in 0..3 {
                let u = face[edge];
                let v = face[(edge + 1) % 3];
                let key = (u.min(v), u.max(v));
                let orientation = u < v;
                match edges.get_mut(&key) {
                    None => {
                        edges.insert(key, (1, orientation));
                    }
                    Some((count, previous_orientation)) => {
                        if *count >= 2 {
                            return Err(MeshError::NonManifoldEdge(key.0, key.1));
                        }
                        if *previous_orientation == orientation {
                            return Err(MeshError::InconsistentWinding(key.0, key.1));
                        }
                        *count += 1;
                    }
                }
            }
        }
        Ok(TopologyReport {
            vertices: self.vertices.len(),
            triangles: self.triangles.len(),
            edges: edges.len(),
            boundary_edges: edges.values().filter(|(n, _)| *n == 1).count(),
        })
    }

    pub fn recompute_normals(&mut self) -> Result<(), MeshError> {
        self.validate()?;
        let mut sums = vec![[0.0_f64; 3]; self.vertices.len()];
        for face in &self.triangles {
            let [a, b, c] = *face;
            let p = self.vertices[a as usize].position;
            let q = self.vertices[b as usize].position;
            let r = self.vertices[c as usize].position;
            let n = cross(subtract(q, p), subtract(r, p));
            for idx in [a, b, c] {
                let sum = &mut sums[idx as usize];
                for axis in 0..3 {
                    sum[axis] += n[axis];
                }
            }
        }
        for (vertex, normal) in self.vertices.iter_mut().zip(sums) {
            let magnitude = length(normal);
            vertex.normal = if magnitude > 1e-12 {
                [
                    (normal[0] / magnitude) as f32,
                    (normal[1] / magnitude) as f32,
                    (normal[2] / magnitude) as f32,
                ]
            } else {
                [0.0, 1.0, 0.0]
            };
        }
        Ok(())
    }

    pub fn split_edge(&mut self, a: u32, b: u32) -> Result<u32, MeshError> {
        self.validate()?;
        if a == b || a as usize >= self.vertices.len() || b as usize >= self.vertices.len() {
            return Err(MeshError::InvalidOperation);
        }
        if self.vertices.len() >= u32::MAX as usize {
            return Err(MeshError::TooManyVertices);
        }
        let mut matching = Vec::new();
        for (index, face) in self.triangles.iter().enumerate() {
            for corner in 0..3 {
                let u = face[corner];
                let v = face[(corner + 1) % 3];
                if (u == a && v == b) || (u == b && v == a) {
                    matching.push((index, corner));
                    break;
                }
            }
        }
        if matching.is_empty() {
            return Err(MeshError::EdgeNotFound(a, b));
        }
        let va = &self.vertices[a as usize];
        let vb = &self.vertices[b as usize];
        let mut middle = Vertex::new(
            std::array::from_fn(|i| (va.position[i] + vb.position[i]) * 0.5),
            std::array::from_fn(|i| (va.uv[i] + vb.uv[i]) * 0.5),
        );
        middle.normal = std::array::from_fn(|i| (va.normal[i] + vb.normal[i]) * 0.5);
        let new_index = self.vertices.len() as u32;
        self.vertices.push(middle);
        let mut additions = Vec::with_capacity(matching.len());
        for (index, corner) in matching {
            let face = self.triangles[index];
            let u = face[corner];
            let v = face[(corner + 1) % 3];
            let opposite = face[(corner + 2) % 3];
            self.triangles[index] = [u, new_index, opposite];
            additions.push([new_index, v, opposite]);
        }
        self.triangles.extend(additions);
        self.recompute_normals()?;
        Ok(new_index)
    }

    pub fn extrude_face(
        &mut self,
        face_index: usize,
        distance: f64,
    ) -> Result<[u32; 3], MeshError> {
        self.validate()?;
        if !distance.is_finite() || distance.abs() <= 1e-9 || face_index >= self.triangles.len() {
            return Err(MeshError::InvalidOperation);
        }
        if self.vertices.len() > (u32::MAX as usize).saturating_sub(3) {
            return Err(MeshError::TooManyVertices);
        }
        let [a, b, c] = self.triangles[face_index];
        let p = self.vertices[a as usize].position;
        let q = self.vertices[b as usize].position;
        let r = self.vertices[c as usize].position;
        let n = cross(subtract(q, p), subtract(r, p));
        let magnitude = length(n);
        if magnitude <= 1e-12 {
            return Err(MeshError::InvalidOperation);
        }
        let first = self.vertices.len() as u32;
        for old in [a, b, c] {
            let mut v = self.vertices[old as usize].clone();
            for axis in 0..3 {
                v.position[axis] += n[axis] / magnitude * distance;
            }
            self.vertices.push(v);
        }
        let cap = [first, first + 1, first + 2];
        self.triangles[face_index] = cap;
        for (u, v, new_u, new_v) in [
            (a, b, cap[0], cap[1]),
            (b, c, cap[1], cap[2]),
            (c, a, cap[2], cap[0]),
        ] {
            self.triangles.push([u, v, new_v]);
            self.triangles.push([u, new_v, new_u]);
        }
        self.recompute_normals()?;
        Ok(cap)
    }

    pub fn remove_faces(&mut self, faces: &[usize]) -> Result<(), MeshError> {
        self.validate()?;
        let selected: BTreeSet<_> = faces.iter().copied().collect();
        if selected.is_empty()
            || selected.iter().any(|&f| f >= self.triangles.len())
            || selected.len() == self.triangles.len()
        {
            return Err(MeshError::InvalidOperation);
        }
        let mut result = self.clone();
        result.triangles = self
            .triangles
            .iter()
            .enumerate()
            .filter_map(|(index, face)| (!selected.contains(&index)).then_some(*face))
            .collect();
        result.validate()?;
        *self = result;
        self.recompute_normals()
    }

    pub fn boundary_loops(&self) -> Result<Vec<Vec<u32>>, MeshError> {
        self.validate()?;
        let mut oriented: BTreeMap<(u32, u32), (usize, (u32, u32))> = BTreeMap::new();
        for face in &self.triangles {
            for corner in 0..3 {
                let u = face[corner];
                let v = face[(corner + 1) % 3];
                let key = (u.min(v), u.max(v));
                let entry = oriented.entry(key).or_insert((0, (u, v)));
                entry.0 += 1;
            }
        }
        let mut next = BTreeMap::<u32, u32>::new();
        let mut incoming = BTreeSet::<u32>::new();
        for (count, (from, to)) in oriented.values() {
            if *count == 1 {
                if next.insert(*from, *to).is_some() || !incoming.insert(*to) {
                    return Err(MeshError::InvalidOperation);
                }
            }
        }
        if next.len() != incoming.len() || next.keys().any(|v| !incoming.contains(v)) {
            return Err(MeshError::InvalidOperation);
        }
        let mut loops = Vec::new();
        while let Some((&start, _)) = next.first_key_value() {
            let mut vertices = Vec::new();
            let mut current = start;
            loop {
                vertices.push(current);
                let following = next.remove(&current).ok_or(MeshError::InvalidOperation)?;
                if following == start {
                    break;
                }
                if vertices.len() > self.vertices.len() {
                    return Err(MeshError::InvalidOperation);
                }
                current = following;
            }
            if vertices.len() < 3 {
                return Err(MeshError::InvalidOperation);
            }
            loops.push(vertices);
        }
        Ok(loops)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> IndexedMesh {
        let mut mesh = IndexedMesh {
            vertices: vec![
                Vertex::new([0.0, 0.0, 0.0], [0.0, 0.0]),
                Vertex::new([1.0, 0.0, 0.0], [1.0, 0.0]),
                Vertex::new([0.0, 0.0, 1.0], [0.0, 1.0]),
                Vertex::new([1.0, 0.0, 1.0], [1.0, 1.0]),
            ],
            triangles: vec![[0, 2, 1], [1, 2, 3]],
        };
        mesh.recompute_normals().unwrap();
        mesh
    }

    #[test]
    fn indexed_sheet_reports_its_outer_boundary() {
        let mesh = quad();
        let report = mesh.validate().unwrap();
        assert_eq!(
            (report.vertices, report.triangles, report.boundary_edges),
            (4, 2, 4)
        );
        assert_eq!(mesh.boundary_loops().unwrap().len(), 1);
        assert!(mesh.vertices.iter().all(|v| v.normal[1] > 0.99));
    }

    #[test]
    fn edge_split_preserves_orientation_and_connectivity() {
        let mut mesh = quad();
        let mid = mesh.split_edge(1, 2).unwrap();
        assert_eq!(mid, 4);
        assert_eq!(mesh.validate().unwrap().triangles, 4);
        assert_eq!(mesh.validate().unwrap().boundary_edges, 4);
        assert_eq!(mesh.boundary_loops().unwrap().len(), 1);
    }

    #[test]
    fn face_extrusion_produces_real_polygon_walls() {
        let mut mesh = quad();
        let cap = mesh.extrude_face(0, 3.0).unwrap();
        assert_eq!(mesh.validate().unwrap().triangles, 8);
        assert_eq!(mesh.validate().unwrap().boundary_edges, 4);
        assert!(
            cap.iter()
                .all(|&i| mesh.vertices[i as usize].position[1] > 2.99)
        );
    }

    #[test]
    fn removing_faces_creates_topological_opening() {
        let mut mesh = quad();
        mesh.remove_faces(&[0]).unwrap();
        assert_eq!(mesh.validate().unwrap().triangles, 1);
        assert_eq!(mesh.validate().unwrap().boundary_edges, 3);
    }

    #[test]
    fn winding_and_nonmanifold_edges_are_rejected() {
        let mut mesh = quad();
        mesh.triangles[1] = [1, 3, 2];
        assert_eq!(mesh.validate(), Err(MeshError::InconsistentWinding(1, 2)));
        mesh.triangles[1] = [1, 2, 3];
        mesh.triangles.push([1, 3, 2]);
        assert!(matches!(
            mesh.validate(),
            Err(MeshError::NonManifoldEdge(1, 2))
        ));
    }

    #[test]
    fn closed_tetrahedron_is_manifold_without_boundaries() {
        let mut mesh = IndexedMesh {
            vertices: vec![
                Vertex::new([0.0, 0.0, 0.0], [0.0, 0.0]),
                Vertex::new([1.0, 0.0, 0.0], [1.0, 0.0]),
                Vertex::new([0.0, 0.0, 1.0], [0.0, 1.0]),
                Vertex::new([0.0, 1.0, 0.0], [1.0, 1.0]),
            ],
            triangles: vec![[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]],
        };
        let topology = mesh.validate().unwrap();
        assert!(topology.is_closed());
        assert_eq!(topology.edges, 6);
        assert!(mesh.boundary_loops().unwrap().is_empty());
        mesh.recompute_normals().unwrap();
    }

    #[test]
    fn invalid_mutation_does_not_change_mesh() {
        let mut mesh = quad();
        let before = mesh.clone();
        assert_eq!(mesh.split_edge(0, 3), Err(MeshError::EdgeNotFound(0, 3)));
        assert_eq!(mesh, before);
        assert_eq!(
            mesh.extrude_face(0, f64::NAN),
            Err(MeshError::InvalidOperation)
        );
        assert_eq!(mesh, before);
    }
}
