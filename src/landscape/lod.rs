// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::mesh::{IndexedMesh, MeshError};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimplificationSettings {
    pub target_triangles: usize,
    pub maximum_error: f64,
    pub feature_angle_degrees: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SimplifiedMesh {
    pub mesh: IndexedMesh,
    pub collapsed_edges: usize,
    pub max_quadric_error: f64,
}

#[derive(Clone, Copy, Default, Debug)]
struct Quadric([f64; 10]);

impl Quadric {
    fn add_plane(&mut self, p: [f64; 4]) {
        let mut idx = 0;
        for i in 0..4 {
            for j in i..4 {
                self.0[idx] += p[i] * p[j];
                idx += 1;
            }
        }
    }
    fn combined(self, other: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] + other.0[i]))
    }
    fn eval(self, v: [f64; 3]) -> f64 {
        let p = [v[0], v[1], v[2], 1.0];
        let mut cost = 0.0;
        let mut k = 0;
        for i in 0..4 {
            for j in i..4 {
                cost += self.0[k] * p[i] * p[j] * if i == j { 1.0 } else { 2.0 };
                k += 1;
            }
        }
        cost.max(0.0)
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn plane(points: [[f64; 3]; 3]) -> Option<([f64; 4], [f64; 3])> {
    let raw = cross(sub(points[1], points[0]), sub(points[2], points[0]));
    let norm = dot(raw, raw).sqrt();
    if !norm.is_finite() || norm < 1e-12 {
        return None;
    }
    let n = raw.map(|c| c / norm);
    Some(([n[0], n[1], n[2], -dot(n, points[0])], n))
}

fn find_features(
    mesh: &IndexedMesh,
    cos_limit: f64,
) -> (
    BTreeSet<u32>,
    BTreeMap<(u32, u32), Vec<usize>>,
    Vec<[f64; 3]>,
) {
    let mut edges = BTreeMap::<(u32, u32), Vec<usize>>::new();
    let mut face_normals = Vec::with_capacity(mesh.triangles.len());
    for (i, tri) in mesh.triangles.iter().enumerate() {
        let p = tri.map(|v| mesh.vertices[v as usize].position);
        face_normals.push(plane(p).unwrap().1);
        for k in 0..3 {
            let (a, b) = (tri[k], tri[(k + 1) % 3]);
            edges.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut frozen = BTreeSet::<u32>::new();
    for (&(a, b), face_ids) in &edges {
        if face_ids.len() == 1
            || face_ids.len() == 2
                && dot(face_normals[face_ids[0]], face_normals[face_ids[1]]) < cos_limit
        {
            frozen.insert(a);
            frozen.insert(b);
        }
    }
    (frozen, edges, face_normals)
}

fn quadrics(mesh: &IndexedMesh) -> Vec<Quadric> {
    let mut qs = vec![Quadric::default(); mesh.vertices.len()];
    for tri in &mesh.triangles {
        if let Some((coefficients, _)) = plane(tri.map(|i| mesh.vertices[i as usize].position)) {
            for &id in tri {
                qs[id as usize].add_plane(coefficients);
            }
        }
    }
    qs
}

fn collapsed_candidate(
    original: &IndexedMesh,
    a: u32,
    b: u32,
    replacement: [f64; 3],
    old_normals: &[[f64; 3]],
) -> Option<IndexedMesh> {
    let mut next = original.clone();
    next.vertices[a as usize].position = replacement;
    next.vertices[a as usize].uv = std::array::from_fn(|i| {
        (original.vertices[a as usize].uv[i] + original.vertices[b as usize].uv[i]) * 0.5
    });
    let mut changed = Vec::with_capacity(next.triangles.len());
    for (index, old) in original.triangles.iter().enumerate() {
        let converted = old.map(|v| if v == b { a } else { v });
        if converted[0] == converted[1]
            || converted[1] == converted[2]
            || converted[2] == converted[0]
        {
            continue;
        }
        if old.contains(&a) || old.contains(&b) {
            let points = converted.map(|i| next.vertices[i as usize].position);
            let (_, normal) = plane(points)?;
            if dot(normal, old_normals[index]) < 0.5 {
                return None;
            }
        }
        changed.push(converted);
    }
    if changed.len() >= original.triangles.len() || changed.is_empty() {
        return None;
    }
    next.triangles = changed;
    if next.validate().is_err() {
        return None;
    }
    Some(next)
}

fn compact(mesh: &mut IndexedMesh) {
    let mut referenced = BTreeSet::<u32>::new();
    for tri in &mesh.triangles {
        referenced.extend(tri);
    }
    let mut remap = BTreeMap::new();
    let mut vertices = Vec::with_capacity(referenced.len());
    for i in referenced {
        remap.insert(i, vertices.len() as u32);
        vertices.push(mesh.vertices[i as usize].clone());
    }
    for triangle in &mut mesh.triangles {
        for v in triangle {
            *v = remap[v];
        }
    }
    mesh.vertices = vertices;
}

pub fn simplify_mesh(
    original: &IndexedMesh,
    settings: SimplificationSettings,
) -> Result<SimplifiedMesh, MeshError> {
    original.validate()?;
    if settings.target_triangles == 0
        || !settings.maximum_error.is_finite()
        || settings.maximum_error < 0.0
        || !settings.feature_angle_degrees.is_finite()
        || !(0.0..=180.0).contains(&settings.feature_angle_degrees)
    {
        return Err(MeshError::InvalidOperation);
    }
    let mut mesh = original.clone();
    let mut collapsed_edges = 0;
    let mut max_quadric_error: f64 = 0.0;
    let cos_limit = settings.feature_angle_degrees.to_radians().cos();
    if mesh.triangles.len() > 16_384 {
        return Err(MeshError::InvalidOperation);
    }
    while mesh.triangles.len() > settings.target_triangles {
        let (frozen, edges, normals) = find_features(&mesh, cos_limit);
        let qs = quadrics(&mesh);
        let mut ranked = Vec::new();
        for (&(a, b), owners) in &edges {
            if owners.len() != 2 || frozen.contains(&a) || frozen.contains(&b) {
                continue;
            }
            let p = mesh.vertices[a as usize].position;
            let q = mesh.vertices[b as usize].position;
            let quadric = qs[a as usize].combined(qs[b as usize]);
            for (option, candidate) in [p, std::array::from_fn(|i| (p[i] + q[i]) * 0.5), q]
                .into_iter()
                .enumerate()
            {
                let squared = quadric.eval(candidate);
                let distance = (squared / 2.0).sqrt();
                if distance <= settings.maximum_error {
                    ranked.push((distance, a, b, option, candidate));
                }
            }
        }
        ranked.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(left.3.cmp(&right.3))
        });
        let mut accepted = None;
        for (error, a, b, _, position) in ranked {
            if let Some(next) = collapsed_candidate(&mesh, a, b, position, &normals) {
                accepted = Some((next, error));
                break;
            }
        }
        let Some((next, error)) = accepted else {
            break;
        };
        mesh = next;
        collapsed_edges += 1;
        max_quadric_error = max_quadric_error.max(error);
    }
    compact(&mut mesh);
    mesh.recompute_normals()?;
    mesh.validate()?;
    Ok(SimplifiedMesh {
        mesh,
        collapsed_edges,
        max_quadric_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::Heightmap;

    fn plane_grid(n: usize) -> IndexedMesh {
        Heightmap::new(
            n,
            n,
            [0.0; 3],
            1.0,
            (0..n * n)
                .map(|i| (i % n) as f64 * 0.2 + (i / n) as f64 * 0.1)
                .collect(),
        )
        .unwrap()
        .mesh()
        .unwrap()
    }
    fn opts(target: usize) -> SimplificationSettings {
        SimplificationSettings {
            target_triangles: target,
            maximum_error: 0.1,
            feature_angle_degrees: 35.0,
        }
    }
    #[test]
    fn simplification_reduces_polygon_count_without_changing_boundaries() {
        let source = plane_grid(13);
        let boundaries: Vec<_> = source.boundary_loops().unwrap()[0]
            .iter()
            .map(|&id| source.vertices[id as usize].position)
            .collect();
        let lod = simplify_mesh(&source, opts(90)).unwrap();
        assert!(lod.mesh.triangles.len() < source.triangles.len());
        assert!(lod.mesh.triangles.len() >= 90);
        assert!(lod.collapsed_edges > 10);
        assert!(lod.max_quadric_error < 1e-6);
        let new_boundaries: Vec<_> = lod.mesh.boundary_loops().unwrap()[0]
            .iter()
            .map(|&id| lod.mesh.vertices[id as usize].position)
            .collect();
        assert_eq!(new_boundaries, boundaries);
        assert_eq!(
            lod.mesh.validate().unwrap().boundary_edges,
            source.validate().unwrap().boundary_edges
        );
    }

    #[test]
    fn preserves_real_cave_portal_and_overhanging_tunnel() {
        let mut source = plane_grid(11);
        let mid = (5 * 10 + 5) * 2;
        source.remove_faces(&[mid, mid + 1]).unwrap();
        let mouth = source
            .boundary_loops()
            .unwrap()
            .into_iter()
            .find(|ring| ring.len() == 4)
            .unwrap();
        source
            .extrude_boundary_path(&mouth, &[[0.0, -3.0, 0.0], [1.5, -6.0, 0.0]], true)
            .unwrap();
        let source_boundary = source.validate().unwrap().boundary_edges;
        let original_faces = source.triangles.len();
        let lod = simplify_mesh(&source, opts(100)).unwrap();
        assert!(lod.mesh.triangles.len() < original_faces);
        assert_eq!(lod.mesh.validate().unwrap().boundary_edges, source_boundary);
        assert_eq!(lod.mesh.boundary_loops().unwrap().len(), 1);
        assert!(lod.mesh.vertices.iter().any(|v| v.position[1] < -4.0));
    }

    #[test]
    fn zero_error_preserves_nonplanar_features() {
        let n = 9;
        let data: Vec<_> = (0..n * n)
            .map(|i| {
                let x = i % n;
                let z = i / n;
                (x as f64 * 0.7).sin() * (z as f64 * 0.4).cos() * 5.0
            })
            .collect();
        let source = Heightmap::new(n, n, [0.0; 3], 1.0, data)
            .unwrap()
            .mesh()
            .unwrap();
        let lod = simplify_mesh(
            &source,
            SimplificationSettings {
                target_triangles: 8,
                maximum_error: 0.0,
                feature_angle_degrees: 35.0,
            },
        )
        .unwrap();
        assert_eq!(lod.mesh.triangles.len(), source.triangles.len());
    }

    #[test]
    fn invalid_budget_is_rejected_without_modification() {
        let source = plane_grid(5);
        let pristine = source.clone();
        assert_eq!(
            simplify_mesh(&source, opts(0)),
            Err(MeshError::InvalidOperation)
        );
        assert_eq!(source, pristine);
    }
}
