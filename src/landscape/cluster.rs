// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::mesh::{IndexedMesh, MeshError};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClusterLimits {
    pub vertices: usize,
    pub triangles: usize,
}
impl Default for ClusterLimits {
    fn default() -> Self {
        Self {
            vertices: 64,
            triangles: 124,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: [f64; 3],
    pub max: [f64; 3],
}
impl Bounds {
    fn for_vertices(mesh: &IndexedMesh, vertices: &[u32]) -> Self {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for &vertex in vertices {
            let p = mesh.vertices[vertex as usize].position;
            for a in 0..3 {
                min[a] = min[a].min(p[a]);
                max[a] = max[a].max(p[a]);
            }
        }
        Self { min, max }
    }
    pub fn union(self, other: Self) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i].min(other.min[i])),
            max: std::array::from_fn(|i| self.max[i].max(other.max[i])),
        }
    }
    pub fn intersects_frustum(self, planes: &[[f64; 4]; 6]) -> bool {
        planes.iter().all(|plane| {
            let furthest: [f64; 3] = std::array::from_fn(|axis| {
                if plane[axis] >= 0.0 {
                    self.max[axis]
                } else {
                    self.min[axis]
                }
            });
            plane[0] * furthest[0] + plane[1] * furthest[1] + plane[2] * furthest[2] + plane[3]
                >= 0.0
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Meshlet {
    pub vertices: Vec<u32>,
    pub triangles: Vec<[u8; 3]>,
    pub source_triangles: Vec<u32>,
    pub bounds: Bounds,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityNode {
    pub bounds: Bounds,
    pub children: Vec<usize>,
    pub meshlet: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledGeometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv: Vec<[f32; 2]>,
    pub partition_origin: [f64; 3],
    pub meshlets: Vec<Meshlet>,
    pub hierarchy: Vec<VisibilityNode>,
    pub root: usize,
}

impl CompiledGeometry {
    pub fn camera_relative_translation(
        &self,
        camera_world: [f64; 3],
    ) -> Result<[f32; 3], MeshError> {
        if camera_world.iter().any(|v| !v.is_finite()) {
            return Err(MeshError::InvalidOperation);
        }
        let offset: [f64; 3] = std::array::from_fn(|i| self.partition_origin[i] - camera_world[i]);
        if offset
            .iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
        {
            return Err(MeshError::InvalidOperation);
        }
        Ok(offset.map(|v| v as f32))
    }

    pub fn visible_meshlets(&self, planes: &[[f64; 4]; 6]) -> Vec<usize> {
        let mut found = Vec::new();
        let mut stack = vec![self.root];
        while let Some(node_index) = stack.pop() {
            let node = &self.hierarchy[node_index];
            if !node.bounds.intersects_frustum(planes) {
                continue;
            }
            if let Some(meshlet) = node.meshlet {
                found.push(meshlet);
            }
            stack.extend(node.children.iter().copied());
        }
        found.sort_unstable();
        found
    }
}

pub fn compile_meshlets(
    mesh: &IndexedMesh,
    partition_origin: [f64; 3],
    limits: ClusterLimits,
) -> Result<CompiledGeometry, MeshError> {
    mesh.validate()?;
    if limits.vertices < 3
        || limits.vertices > u8::MAX as usize
        || limits.triangles == 0
        || limits.triangles > 256
        || partition_origin.iter().any(|v| !v.is_finite())
    {
        return Err(MeshError::InvalidOperation);
    }
    let mut positions = Vec::with_capacity(mesh.vertices.len());
    let mut normals = Vec::with_capacity(mesh.vertices.len());
    let mut uv = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        let local: [f64; 3] = std::array::from_fn(|i| v.position[i] - partition_origin[i]);
        if local
            .iter()
            .any(|value| !value.is_finite() || value.abs() > f32::MAX as f64)
        {
            return Err(MeshError::InvalidOperation);
        }
        positions.push(local.map(|x| x as f32));
        normals.push(v.normal);
        uv.push(v.uv);
    }

    let mut edge_owners: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
    for (index, face) in mesh.triangles.iter().enumerate() {
        for k in 0..3 {
            let u = face[k];
            let v = face[(k + 1) % 3];
            edge_owners
                .entry((u.min(v), u.max(v)))
                .or_default()
                .push(index);
        }
    }
    let mut neighbors = vec![Vec::<usize>::new(); mesh.triangles.len()];
    for owners in edge_owners.values() {
        if owners.len() == 2 {
            neighbors[owners[0]].push(owners[1]);
            neighbors[owners[1]].push(owners[0]);
        }
    }
    for n in &mut neighbors {
        n.sort_unstable();
        n.dedup();
    }

    let mut assigned = vec![false; mesh.triangles.len()];
    let mut meshlets = Vec::new();
    for seed in 0..mesh.triangles.len() {
        if assigned[seed] {
            continue;
        }
        let mut frontier = VecDeque::from([seed]);
        let mut queued = BTreeSet::from([seed]);
        let mut remap: BTreeMap<u32, u8> = BTreeMap::new();
        let mut global_vertices = Vec::new();
        let mut triangles = Vec::new();
        let mut source_triangles = Vec::new();
        while let Some(face_index) = frontier.pop_front() {
            if assigned[face_index] {
                continue;
            }
            let face = mesh.triangles[face_index];
            let missing = face.iter().filter(|v| !remap.contains_key(v)).count();
            if triangles.len() == limits.triangles
                || global_vertices.len() + missing > limits.vertices
            {
                continue;
            }
            let local = face.map(|global| {
                if let Some(&index) = remap.get(&global) {
                    return index;
                }
                let index = global_vertices.len() as u8;
                global_vertices.push(global);
                remap.insert(global, index);
                index
            });
            triangles.push(local);
            source_triangles.push(face_index as u32);
            assigned[face_index] = true;
            for &neighbor in &neighbors[face_index] {
                if !assigned[neighbor] && queued.insert(neighbor) {
                    frontier.push_back(neighbor);
                }
            }
        }
        if triangles.is_empty() {
            return Err(MeshError::InvalidOperation);
        }
        let bounds = Bounds::for_vertices(mesh, &global_vertices);
        meshlets.push(Meshlet {
            vertices: global_vertices,
            triangles,
            source_triangles,
            bounds,
        });
    }

    let mut hierarchy: Vec<VisibilityNode> = meshlets
        .iter()
        .enumerate()
        .map(|(index, meshlet)| VisibilityNode {
            bounds: meshlet.bounds,
            children: Vec::new(),
            meshlet: Some(index),
        })
        .collect();
    let mut current: Vec<usize> = (0..hierarchy.len()).collect();
    while current.len() > 1 {
        let mut parents = Vec::new();
        for group in current.chunks(4) {
            let mut bounds = hierarchy[group[0]].bounds;
            for &index in &group[1..] {
                bounds = bounds.union(hierarchy[index].bounds);
            }
            let index = hierarchy.len();
            hierarchy.push(VisibilityNode {
                bounds,
                children: group.to_vec(),
                meshlet: None,
            });
            parents.push(index);
        }
        current = parents;
    }
    Ok(CompiledGeometry {
        positions,
        normals,
        uv,
        partition_origin,
        meshlets,
        hierarchy,
        root: current[0],
    })
}

pub fn compile_simplified_meshlets(
    source: &IndexedMesh,
    partition_origin: [f64; 3],
    settings: super::lod::SimplificationSettings,
    limits: ClusterLimits,
) -> Result<(CompiledGeometry, super::lod::SimplifiedMesh), MeshError> {
    let reduced = super::lod::simplify_mesh(source, settings)?;
    let compiled = compile_meshlets(&reduced.mesh, partition_origin, limits)?;
    Ok((compiled, reduced))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{Heightmap, Vertex};
    fn landscape() -> IndexedMesh {
        let samples: Vec<f64> = (0..17)
            .flat_map(|z| {
                (0..17).map(move |x| ((x as f64 * 0.4).sin() + (z as f64 * 0.32).cos()) * 5.0)
            })
            .collect();
        Heightmap::new(17, 17, [120_000.0, 0.0, -990_000.0], 2.0, samples)
            .unwrap()
            .mesh()
            .unwrap()
    }
    #[test]
    fn meshlet_compiler_covers_every_triangle_exactly_once() {
        let mesh = landscape();
        let limits = ClusterLimits {
            vertices: 32,
            triangles: 24,
        };
        let compiled = compile_meshlets(&mesh, [0.0; 3], limits).unwrap();
        let mut seen: Vec<usize> = compiled
            .meshlets
            .iter()
            .flat_map(|c| c.source_triangles.iter().map(|&v| v as usize))
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..mesh.triangles.len()).collect::<Vec<_>>());
        assert!(compiled.meshlets.len() > 1);
        for c in &compiled.meshlets {
            assert!(c.vertices.len() <= limits.vertices);
            assert!(c.triangles.len() <= limits.triangles);
            for tri in &c.triangles {
                for &i in tri {
                    assert!((i as usize) < c.vertices.len());
                }
            }
        }
        assert_eq!(compile_meshlets(&mesh, [0.0; 3], limits).unwrap(), compiled);
    }
    #[test]
    fn very_large_world_origin_still_compiles_camera_relative_positions() {
        let mut mesh = IndexedMesh {
            vertices: vec![
                Vertex::new([1e12, 2e12, -3e12], [0.0, 0.0]),
                Vertex::new([1e12 + 1.0, 2e12, -3e12], [1.0, 0.0]),
                Vertex::new([1e12, 2e12, -3e12 + 1.0], [0.0, 1.0]),
            ],
            triangles: vec![[0, 2, 1]],
        };
        mesh.recompute_normals().unwrap();
        let result =
            compile_meshlets(&mesh, [1e12, 2e12, -3e12], ClusterLimits::default()).unwrap();
        assert_eq!(
            result.positions,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
        );
    }
    #[test]
    fn six_plane_frustum_culls_remote_geometry() {
        let compiled = compile_meshlets(&landscape(), [0.0; 3], ClusterLimits::default()).unwrap();
        let around_origin = [
            [1.0, 0.0, 0.0, 10.0],
            [-1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 10.0],
            [0.0, -1.0, 0.0, 10.0],
            [0.0, 0.0, 1.0, 10.0],
            [0.0, 0.0, -1.0, 10.0],
        ];
        assert!(compiled.visible_meshlets(&around_origin).is_empty());
        let everything = [
            [1.0, 0.0, 0.0, 1e15],
            [-1.0, 0.0, 0.0, 1e15],
            [0.0, 1.0, 0.0, 1e15],
            [0.0, -1.0, 0.0, 1e15],
            [0.0, 0.0, 1.0, 1e15],
            [0.0, 0.0, -1.0, 1e15],
        ];
        assert_eq!(
            compiled.visible_meshlets(&everything).len(),
            compiled.meshlets.len()
        );
    }
    #[test]
    fn camera_movement_does_not_modify_partition_geometry() {
        let origin = [120000.0, 0.0, -990000.0];
        let compiled = compile_meshlets(&landscape(), origin, ClusterLimits::default()).unwrap();
        let unchanged_positions = compiled.positions.clone();
        assert_eq!(
            compiled.camera_relative_translation(origin).unwrap(),
            [0.0; 3]
        );
        assert_eq!(
            compiled
                .camera_relative_translation([120010.0, 5.0, -990020.0])
                .unwrap(),
            [-10.0, -5.0, 20.0],
        );
        assert_eq!(compiled.positions, unchanged_positions);
        assert_eq!(compiled.partition_origin, origin);
    }

    #[test]
    fn invalid_cluster_limits_are_rejected() {
        let mesh = landscape();
        assert_eq!(
            compile_meshlets(
                &mesh,
                [0.0; 3],
                ClusterLimits {
                    vertices: 2,
                    triangles: 1
                }
            ),
            Err(MeshError::InvalidOperation)
        );
    }
}
