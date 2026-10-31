// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::landscape::IndexedMesh;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

const BUCKET: f32 = 8.0;
const MAX_BUCKET_CELLS: i64 = 1024;
const CONTACT_SKIN: f32 = 0.002;

#[derive(Clone, Copy, Debug)]
struct Triangle {
    points: [Vec3; 3],
    plane: Vec3,
    min: Vec3,
    max: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct CapsuleContact {
    pub normal: Vec3,
    pub penetration: f32,
}

#[derive(Clone, Debug, Default)]
pub struct TriangleCollider {
    triangles: Vec<Triangle>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
    oversized: Vec<usize>,
}

fn key(x: f32) -> i32 {
    (x / BUCKET).floor() as i32
}

impl TriangleCollider {
    pub fn new(mesh: &IndexedMesh) -> Self {
        let mut result = Self::default();
        for face in &mesh.triangles {
            let (Some(a), Some(b), Some(c)) = (
                mesh.vertices.get(face[0] as usize),
                mesh.vertices.get(face[1] as usize),
                mesh.vertices.get(face[2] as usize),
            ) else {
                continue;
            };
            let points = [a, b, c].map(|v| {
                Vec3::new(
                    v.position[0] as f32,
                    v.position[1] as f32,
                    v.position[2] as f32,
                )
            });
            if points.iter().any(|v| !v.is_finite()) {
                continue;
            }
            let cross = (points[1] - points[0]).cross(points[2] - points[0]);
            if cross.length_squared() < 1e-12 {
                continue;
            }
            let plane = cross.normalize();
            let min = points[0].min(points[1]).min(points[2]);
            let max = points[0].max(points[1]).max(points[2]);
            let index = result.triangles.len();
            result.triangles.push(Triangle {
                points,
                plane,
                min,
                max,
            });
            let (x0, x1, z0, z1) = (key(min.x), key(max.x), key(min.z), key(max.z));
            let count = (i64::from(x1) - i64::from(x0) + 1)
                .saturating_mul(i64::from(z1) - i64::from(z0) + 1);
            if count > MAX_BUCKET_CELLS {
                result.oversized.push(index);
                continue;
            }
            for x in x0..=x1 {
                for z in z0..=z1 {
                    result.buckets.entry((x, z)).or_default().push(index);
                }
            }
        }
        result
    }

    pub(crate) fn triangle_vertices(&self) -> Vec<[Vec3; 3]> {
        self.triangles.iter().map(|face| face.points).collect()
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    pub fn contacts(&self, foot: Vec3, radius: f32, height: f32) -> Vec<CapsuleContact> {
        if !foot.is_finite()
            || !radius.is_finite()
            || !height.is_finite()
            || radius <= 0.0
            || height <= 0.0
        {
            return Vec::new();
        }
        let min = foot - Vec3::new(radius, radius, radius);
        let max = foot + Vec3::new(radius, height + radius, radius);
        let mut candidates = HashSet::new();
        candidates.extend(self.oversized.iter().copied());
        let (x0, x1, z0, z1) = (key(min.x), key(max.x), key(min.z), key(max.z));
        if (i64::from(x1) - i64::from(x0) + 1).saturating_mul(i64::from(z1) - i64::from(z0) + 1)
            > MAX_BUCKET_CELLS
        {
            candidates.extend(0..self.triangles.len());
        } else {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if let Some(indices) = self.buckets.get(&(x, z)) {
                        candidates.extend(indices.iter().copied());
                    }
                }
            }
        }

        let low = foot + Vec3::Y * radius.min(height * 0.5);
        let high = foot + Vec3::Y * (height - radius.min(height * 0.5));
        let mut ordered: Vec<_> = candidates.into_iter().collect();
        ordered.sort_unstable();
        let mut result = Vec::new();
        for id in ordered {
            let tri = &self.triangles[id];
            if tri.max.x < min.x
                || tri.min.x > max.x
                || tri.max.y < min.y
                || tri.min.y > max.y
                || tri.max.z < min.z
                || tri.min.z > max.z
            {
                continue;
            }
            let (on_axis, on_face) = closest_segment_triangle(low, high, tri.points);
            let delta = on_axis - on_face;
            let distance = delta.length();
            if distance >= radius + CONTACT_SKIN {
                continue;
            }
            let normal = if distance > 1e-5 {
                delta / distance
            } else {
                tri.plane
            };
            result.push(CapsuleContact {
                normal,
                penetration: (radius - distance).max(0.0),
            });
        }
        result.sort_by(|a, b| b.penetration.total_cmp(&a.penetration));
        result
    }

    pub(crate) fn contacts_geometry(
        &self,
        position: Vec3,
        body: &crate::engine::collision_geometry::ColliderInstance,
    ) -> Vec<CapsuleContact> {
        use crate::engine::collision_geometry;
        let Some((a, b)) = collision_geometry::envelope(body) else {
            return Vec::new();
        };
        let min = position + a - Vec3::splat(CONTACT_SKIN);
        let max = position + b + Vec3::splat(CONTACT_SKIN);
        let mut ids = HashSet::new();
        ids.extend(self.oversized.iter().copied());
        let (x0, x1, z0, z1) = (key(min.x), key(max.x), key(min.z), key(max.z));
        if (i64::from(x1) - i64::from(x0) + 1).saturating_mul(i64::from(z1) - i64::from(z0) + 1)
            > MAX_BUCKET_CELLS
        {
            ids.extend(0..self.triangles.len());
        } else {
            for x in x0..=x1 {
                for z in z0..=z1 {
                    if let Some(v) = self.buckets.get(&(x, z)) {
                        ids.extend(v.iter().copied());
                    }
                }
            }
        }
        let mut ids = ids.into_iter().collect::<Vec<_>>();
        ids.sort_unstable();
        ids.into_iter()
            .filter_map(|id| {
                let tri = &self.triangles[id];
                if tri.max.cmplt(min).any() || tri.min.cmpgt(max).any() {
                    return None;
                }
                collision_geometry::against_triangle(body, position, tri.points).map(|c| {
                    CapsuleContact {
                        normal: c.normal,
                        penetration: c.depth,
                    }
                })
            })
            .collect()
    }
    pub(crate) fn sweep_geometry(
        colliders: &[&Self],
        from: Vec3,
        to: Vec3,
        body: &crate::engine::collision_geometry::ColliderInstance,
    ) -> (Vec3, Vec<CapsuleContact>) {
        let motion = to - from;
        if !motion.is_finite() {
            return (from, Vec::new());
        }
        let steps = (motion.length() / 0.1).ceil().clamp(1.0, 256.0) as usize;
        let step = motion / steps as f32;
        let mut position = from;
        let mut touched = Vec::new();
        for _ in 0..steps {
            let mut next = position + step;
            for _ in 0..16 {
                let hit = colliders
                    .iter()
                    .flat_map(|c| c.contacts_geometry(next, body))
                    .filter(|h| h.penetration > 1e-5)
                    .max_by(|a, b| a.penetration.total_cmp(&b.penetration));
                let Some(h) = hit else { break };
                next += if h.normal.y >= std::f32::consts::FRAC_1_SQRT_2 {
                    Vec3::Y * ((h.penetration + CONTACT_SKIN) / h.normal.y)
                } else {
                    h.normal * (h.penetration + CONTACT_SKIN)
                };
                touched.push(h);
            }
            position = next;
        }
        (position, touched)
    }

    pub fn sweep_capsule(
        &self,
        from: Vec3,
        to: Vec3,
        radius: f32,
        height: f32,
    ) -> (Vec3, Vec<CapsuleContact>) {
        Self::sweep_colliders(&[self], from, to, radius, height)
    }

    pub fn sweep_colliders(
        colliders: &[&Self],
        from: Vec3,
        to: Vec3,
        radius: f32,
        height: f32,
    ) -> (Vec3, Vec<CapsuleContact>) {
        let motion = to - from;
        if !motion.is_finite() {
            return (from, Vec::new());
        }
        let steps = (motion.length() / (radius.max(0.05) * 0.35))
            .ceil()
            .clamp(1.0, 256.0) as usize;
        let increment = motion / steps as f32;
        let mut position = from;
        let mut touched = Vec::new();
        for _ in 0..steps {
            let mut next = position + increment;
            for _ in 0..4 {
                let mut contacts = Vec::new();
                for collider in colliders {
                    contacts.extend(collider.contacts(next, radius, height));
                }
                if contacts.is_empty() {
                    break;
                }
                contacts.sort_by(|a, b| b.penetration.total_cmp(&a.penetration));
                let Some(contact) = contacts.into_iter().find(|c| c.penetration > 1e-5) else {
                    break;
                };
                let correction = if contact.normal.y >= std::f32::consts::FRAC_1_SQRT_2 {
                    Vec3::Y * ((contact.penetration + CONTACT_SKIN) / contact.normal.y)
                } else {
                    contact.normal * (contact.penetration + CONTACT_SKIN)
                };
                next += correction;
                touched.push(contact);
            }
            position = next;
        }
        (position, touched)
    }
}

fn closest_point_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc).max(1e-12);
    a + ab * (vb * denom) + ac * (vc * denom)
}

fn closest_segments(p0: Vec3, p1: Vec3, q0: Vec3, q1: Vec3) -> (Vec3, Vec3) {
    let d1 = p1 - p0;
    let d2 = q1 - q0;
    let r = p0 - q0;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);
    if a < 1e-9 && e < 1e-9 {
        return (p0, q0);
    }
    if a < 1e-9 {
        let t = (f / e).clamp(0.0, 1.0);
        return (p0, q0 + d2 * t);
    }
    let c = d1.dot(r);
    if e < 1e-9 {
        let s = (-c / a).clamp(0.0, 1.0);
        return (p0 + d1 * s, q0);
    }
    let b = d1.dot(d2);
    let denom = a * e - b * b;
    let mut s = if denom.abs() > 1e-9 {
        ((b * f - c * e) / denom).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let tnom = b * s + f;
    let t;
    if tnom < 0.0 {
        t = 0.0;
        s = (-c / a).clamp(0.0, 1.0);
    } else if tnom > e {
        t = 1.0;
        s = ((b - c) / a).clamp(0.0, 1.0);
    } else {
        t = tnom / e;
    }
    (p0 + d1 * s, q0 + d2 * t)
}

pub(crate) fn closest_segment_triangle(p0: Vec3, p1: Vec3, tri: [Vec3; 3]) -> (Vec3, Vec3) {
    let mut best = (p0, closest_point_triangle(p0, tri));
    let mut best_d = (best.0 - best.1).length_squared();
    let other = (p1, closest_point_triangle(p1, tri));
    if (other.0 - other.1).length_squared() < best_d {
        best = other;
        best_d = (best.0 - best.1).length_squared();
    }
    let normal = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
    let denom = normal.dot(p1 - p0);
    if denom.abs() > 1e-8 {
        let t = normal.dot(tri[0] - p0) / denom;
        if (0.0..=1.0).contains(&t) {
            let point = p0 + (p1 - p0) * t;
            if (point - closest_point_triangle(point, tri)).length_squared() < 1e-8 {
                return (point, point);
            }
        }
    }
    for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
        let pair = closest_segments(p0, p1, a, b);
        let d = (pair.0 - pair.1).length_squared();
        if d < best_d {
            best = pair;
            best_d = d;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::Vertex;

    fn mesh(points: &[[f32; 3]], triangles: &[[u32; 3]]) -> IndexedMesh {
        IndexedMesh {
            vertices: points
                .iter()
                .map(|p| Vertex::new([p[0] as f64, p[1] as f64, p[2] as f64], [0.0, 0.0]))
                .collect(),
            triangles: triangles.to_vec(),
        }
    }

    #[test]
    fn native_triangle_wall_blocks_fast_capsule_without_reversing_side_motion() {
        let wall = mesh(
            &[
                [0.0, 0.0, -5.0],
                [0.0, 4.0, -5.0],
                [0.0, 0.0, 5.0],
                [0.0, 4.0, 5.0],
            ],
            &[[0, 1, 2], [1, 3, 2]],
        );
        let collider = TriangleCollider::new(&wall);
        assert_eq!(collider.triangle_count(), 2);
        let (stopped, contacts) = collider.sweep_capsule(
            Vec3::new(-3.0, 0.0, -3.0),
            Vec3::new(3.0, 0.0, 3.0),
            0.4,
            1.8,
        );
        assert!(
            stopped.x < 0.0,
            "fast capsule crossed vertical wall: {stopped:?}"
        );
        assert!(stopped.z > -2.9, "slide tangent to wall must survive");
        assert!(!contacts.is_empty());
    }

    #[test]
    fn cave_roof_blocks_upward_capsule_and_never_counts_as_walkable() {
        let cave_roof = mesh(
            &[
                [-3.0, 2.2, -3.0],
                [3.0, 2.2, -3.0],
                [-3.0, 2.2, 3.0],
                [3.0, 2.2, 3.0],
            ],
            &[[0, 1, 2], [1, 3, 2]],
        );
        let collider = TriangleCollider::new(&cave_roof);
        let (stopped, contacts) = collider.sweep_capsule(
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.4,
            1.8,
        );
        assert!(
            stopped.y + 1.8 <= 2.22,
            "cave ceiling must stop upward travel"
        );
        assert!(contacts.iter().any(|contact| contact.normal.y < -0.5));
        assert!(!contacts.iter().any(|contact| contact.normal.y > 0.7));
    }

    #[test]
    fn negative_chunk_coordinates_and_multi_contacts_remain_bounded() {
        let ground = mesh(
            &[
                [-12.0, 0.0, -12.0],
                [-4.0, 0.0, -12.0],
                [-12.0, 0.0, -4.0],
                [-4.0, 0.0, -4.0],
            ],
            &[[0, 2, 1], [1, 2, 3]],
        );
        let collider = TriangleCollider::new(&ground);
        let contacts = collider.contacts(Vec3::new(-8.0, -0.1, -8.0), 0.4, 1.8);
        assert!(!contacts.is_empty());
        assert!(contacts.iter().any(|contact| contact.normal.y > 0.7));
        let (position, _) = collider.sweep_capsule(
            Vec3::new(-8.0, 2.0, -8.0),
            Vec3::new(-8.0, -1.0, -8.0),
            0.4,
            1.8,
        );
        assert!(position.is_finite() && position.y >= -0.01);
    }

    #[test]
    fn repeated_uphill_contacts_preserve_positive_forward_progress() {
        let ramp = mesh(
            &[
                [0.0, 0.0, 0.0],
                [12.0, 5.4, 0.0],
                [0.0, 0.0, 12.0],
                [12.0, 5.4, 12.0],
            ],
            &[[0, 2, 1], [1, 2, 3]],
        );
        let collider = TriangleCollider::new(&ramp);
        let mut foot = Vec3::new(3.5, 3.5 * 0.45, 6.0);
        for _ in 0..80 {
            let next = foot + Vec3::new(2.0 / 60.0, 0.45 * 2.0 / 60.0 - 0.003, 0.0);
            let (position, contacts) = collider.sweep_capsule(foot, next, 0.4, 1.8);
            assert!(
                position.x > foot.x + 0.025,
                "slope correction must not eat forward travel"
            );
            assert!(position.y.is_finite());
            assert!(!contacts.is_empty());
            foot = position;
        }
    }

    #[test]
    fn adjacent_terrain_tiles_keep_ground_contact_across_the_seam() {
        let left = TriangleCollider::new(&mesh(
            &[
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [0.0, 0.0, 4.0],
                [4.0, 0.0, 4.0],
            ],
            &[[0, 2, 1], [1, 2, 3]],
        ));
        let right = TriangleCollider::new(&mesh(
            &[
                [4.0, 0.0, 0.0],
                [8.0, 0.0, 0.0],
                [4.0, 0.0, 4.0],
                [8.0, 0.0, 4.0],
            ],
            &[[0, 2, 1], [1, 2, 3]],
        ));
        let from = Vec3::new(3.75, -0.02, 2.0);
        let to = Vec3::new(4.25, -0.02, 2.0);
        let (position, contacts) =
            TriangleCollider::sweep_colliders(&[&left, &right], from, to, 0.4, 1.8);
        assert!(position.x > 4.24, "crossing the tile edge stopped movement");
        assert!(
            position.y.abs() < 0.05,
            "terrain seam changed ground height"
        );
        assert!(contacts.iter().any(|contact| contact.normal.y > 0.7));
    }

    #[test]
    fn terrain_slit_narrower_than_character_diameter_remains_impassable() {
        let cliff = mesh(
            &[
                [-3.0, -1.0, 0.0],
                [-0.25, -1.0, 0.0],
                [-3.0, 3.0, 0.0],
                [-0.25, 3.0, 0.0],
                [0.25, -1.0, 0.0],
                [3.0, -1.0, 0.0],
                [0.25, 3.0, 0.0],
                [3.0, 3.0, 0.0],
            ],
            &[[0, 2, 1], [1, 2, 3], [4, 6, 5], [5, 6, 7]],
        );
        let collider = TriangleCollider::new(&cliff);
        let from = Vec3::new(0.0, 0.0, -1.5);
        let to = Vec3::new(0.0, 0.0, 1.5);
        let (result, _) = collider.sweep_capsule(from, to, 0.4, 1.8);
        assert!(
            result.z <= -0.1,
            "capsule squeezed through terrain slit narrower than its diameter: {result:?}"
        );
    }

    #[test]
    fn rendered_normals_never_override_collision_triangle_normals() {
        let mut roof = mesh(
            &[[-2.0, 1.0, -2.0], [2.0, 1.0, -2.0], [-2.0, 1.0, 2.0]],
            &[[0, 1, 2]],
        );
        for vertex in &mut roof.vertices {
            vertex.normal = [0.0, 1.0, 0.0];
        }
        let collider = TriangleCollider::new(&roof);
        let contacts = collider.contacts(Vec3::new(0.0, 0.3, 0.0), 0.4, 1.5);
        assert!(!contacts.is_empty());
        assert!(contacts.iter().any(|contact| contact.normal.y < -0.5));
    }
}

#[cfg(test)]
mod selected_geometry_tests {
    use super::*;
    use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
    use std::sync::Arc;
    #[test]
    fn active_geometry_terrain_sweep_uses_scaled_body_offset_not_hidden_foot_capsule() {
        let points = [
            [-5.0, 0.0, -5.0],
            [-5.0, 0.0, 5.0],
            [5.0, 0.0, -5.0],
            [5.0, 0.0, 5.0],
        ];
        let mesh = IndexedMesh {
            vertices: points
                .into_iter()
                .map(|p| crate::landscape::Vertex::new(p.map(|v| v as f64), [0.0, 0.0]))
                .collect(),
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        };
        let ground = TriangleCollider::new(&mesh);
        for shape in [
            CollisionShape::Box {
                min: [-1.0, 1.0, -1.0],
                max: [1.0, 3.0, 1.0],
            },
            CollisionShape::Capsule {
                start: [0.0, 2.0, 0.0],
                end: [0.0, 3.0, 0.0],
                radius: 1.0,
            },
        ] {
            let body = ColliderInstance {
                shapes: Arc::new(vec![shape]),
                rotation: Vec3::ZERO,
                scale: Vec3::splat(0.5),
            };
            let (end, touched) = TriangleCollider::sweep_geometry(
                &[&ground],
                Vec3::new(0.0, 3.0, 0.0),
                Vec3::new(0.0, -3.0, 0.0),
                &body,
            );
            assert!(
                (end.y + 0.5).abs() < 0.005,
                "ground must support selected offset body: {end:?}"
            );
            assert!(!touched.is_empty());
            assert!(touched.iter().any(|c| c.normal.y > 0.9));
        }
    }
}
