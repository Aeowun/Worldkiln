// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionMode {
    #[default]
    Box,
    Auto,
    Disabled,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollisionShape {
    Sphere {
        center: [f32; 3],
        radius: f32,
    },
    Box {
        min: [f32; 3],
        max: [f32; 3],
    },
    Capsule {
        start: [f32; 3],
        end: [f32; 3],
        radius: f32,
    },
    Triangles {
        vertices: Vec<[[f32; 3]; 3]>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColliderInstance {
    pub shapes: Arc<Vec<CollisionShape>>,
    pub rotation: Vec3,
    pub scale: Vec3,
}
#[derive(Clone, Copy, Debug)]
pub struct Contact {
    pub normal: Vec3,
    pub depth: f32,
}

fn vec(v: [f32; 3]) -> Vec3 {
    Vec3::from_array(v)
}
fn p3(v: Vec3) -> [f32; 3] {
    v.to_array()
}
fn orient(rot: Vec3) -> Quat {
    Quat::from_euler(EulerRot::XYZ, rot.x, rot.y, rot.z)
}
fn transform(v: Vec3, instance: &ColliderInstance) -> Vec3 {
    orient(instance.rotation) * (v * instance.scale)
}
fn bounds_of_shape(shape: &CollisionShape, instance: &ColliderInstance) -> (Vec3, Vec3) {
    match shape {
        CollisionShape::Sphere { center, radius } => {
            let c = transform(vec(*center), instance);
            let r = radius.abs() * instance.scale.abs().max_element();
            (c - Vec3::splat(r), c + Vec3::splat(r))
        }
        CollisionShape::Box { min, max } => {
            let lo = vec(*min);
            let hi = vec(*max);
            let mut mn = Vec3::splat(f32::INFINITY);
            let mut mx = Vec3::splat(f32::NEG_INFINITY);
            for x in [lo.x, hi.x] {
                for y in [lo.y, hi.y] {
                    for z in [lo.z, hi.z] {
                        let p = transform(Vec3::new(x, y, z), instance);
                        mn = mn.min(p);
                        mx = mx.max(p);
                    }
                }
            }
            (mn, mx)
        }
        CollisionShape::Capsule { start, end, radius } => {
            let a = transform(vec(*start), instance);
            let b = transform(vec(*end), instance);
            let r = *radius * instance.scale.abs().max_element();
            (a.min(b) - Vec3::splat(r), a.max(b) + Vec3::splat(r))
        }
        CollisionShape::Triangles { vertices } => {
            let mut mn = Vec3::splat(f32::INFINITY);
            let mut mx = Vec3::splat(f32::NEG_INFINITY);
            for tri in vertices {
                for &v in tri {
                    let p = transform(vec(v), instance);
                    mn = mn.min(p);
                    mx = mx.max(p);
                }
            }
            if vertices.is_empty() {
                (Vec3::ZERO, Vec3::ZERO)
            } else {
                (mn, mx)
            }
        }
    }
}
pub fn debug_edges(instance: &ColliderInstance, position: Vec3) -> Vec<(Vec3, Vec3)> {
    let mut segments = Vec::new();
    for shape in instance.shapes.iter() {
        match shape {
            CollisionShape::Sphere { center, radius } => {
                let c = transform(vec(*center), instance) + position;
                let r = radius.abs() * instance.scale.abs().max_element();
                for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Y, Vec3::Z), (Vec3::Z, Vec3::X)] {
                    for i in 0..32 {
                        let t = i as f32 * std::f32::consts::TAU / 32.0;
                        let u = (i + 1) as f32 * std::f32::consts::TAU / 32.0;
                        segments.push((
                            c + r * (a * t.cos() + b * t.sin()),
                            c + r * (a * u.cos() + b * u.sin()),
                        ));
                    }
                }
            }
            CollisionShape::Box { min, max } => {
                let lo = vec(*min);
                let hi = vec(*max);
                let mut v = [Vec3::ZERO; 8];
                for (i, p) in v.iter_mut().enumerate() {
                    *p = transform(
                        Vec3::new(
                            if i & 1 != 0 { hi.x } else { lo.x },
                            if i & 2 != 0 { hi.y } else { lo.y },
                            if i & 4 != 0 { hi.z } else { lo.z },
                        ),
                        instance,
                    ) + position;
                }
                for (a, b) in [
                    (0, 1),
                    (0, 2),
                    (0, 4),
                    (1, 3),
                    (1, 5),
                    (2, 3),
                    (2, 6),
                    (3, 7),
                    (4, 5),
                    (4, 6),
                    (5, 7),
                    (6, 7),
                ] {
                    segments.push((v[a], v[b]));
                }
            }
            CollisionShape::Capsule { start, end, radius } => {
                let from = transform(vec(*start), instance) + position;
                let to = transform(vec(*end), instance) + position;
                let direction = (to - from).normalize_or_zero();
                let side = direction.cross(Vec3::X).try_normalize().unwrap_or(Vec3::Z);
                let up = direction.cross(side);
                let r = *radius * instance.scale.abs().max_element();
                for k in 0..16 {
                    let a = k as f32 * std::f32::consts::TAU / 16.0;
                    let b = (k + 1) as f32 * std::f32::consts::TAU / 16.0;
                    let ring = |theta: f32| side * (theta.cos() * r) + up * (theta.sin() * r);
                    segments.push((from + ring(a), from + ring(b)));
                    segments.push((to + ring(a), to + ring(b)));
                    if k % 4 == 0 {
                        segments.push((from + ring(a), to + ring(a)));
                    }
                }
            }
            CollisionShape::Triangles { vertices } => {
                for tri in vertices
                    .iter()
                    .step_by((vertices.len() / 512).max(1))
                    .take(512)
                {
                    let v = tri.map(|p| transform(vec(p), instance) + position);
                    segments.extend([(v[0], v[1]), (v[1], v[2]), (v[2], v[0])]);
                }
            }
        }
    }
    segments
}

pub fn envelope(instance: &ColliderInstance) -> Option<(Vec3, Vec3)> {
    #[derive(Clone, Copy, Hash, PartialEq, Eq)]
    struct CacheKey {
        geometry: usize,
        rotation: [u32; 3],
        scale: [u32; 3],
    }
    struct CacheEntry {
        geometry: std::sync::Weak<Vec<CollisionShape>>,
        bounds: (Vec3, Vec3),
    }
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashMap<CacheKey, CacheEntry>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let expensive = instance.shapes.iter().any(
        |shape| matches!(shape, CollisionShape::Triangles { vertices } if vertices.len() >= 32),
    );
    let key = CacheKey {
        geometry: Arc::as_ptr(&instance.shapes) as usize,
        rotation: instance.rotation.to_array().map(f32::to_bits),
        scale: instance.scale.to_array().map(f32::to_bits),
    };
    if expensive {
        let cached = CACHE.with(|cache| {
            cache.borrow().get(&key).and_then(|entry| {
                entry
                    .geometry
                    .upgrade()
                    .filter(|shapes| Arc::ptr_eq(shapes, &instance.shapes))
                    .map(|_| entry.bounds)
            })
        });
        if cached.is_some() {
            return cached;
        }
    }
    let mut mn = Vec3::splat(f32::INFINITY);
    let mut mx = Vec3::splat(f32::NEG_INFINITY);
    for shape in instance.shapes.iter() {
        let (low, high) = bounds_of_shape(shape, instance);
        mn = mn.min(low);
        mx = mx.max(high);
    }
    let bounds = mn.is_finite().then_some((mn, mx));
    if expensive {
        if let Some(bounds) = bounds {
            CACHE.with(|cache| {
                let mut entries = cache.borrow_mut();
                if entries.len() >= 512 {
                    entries.retain(|_, entry| entry.geometry.strong_count() > 0);
                    if entries.len() >= 512 {
                        entries.clear();
                    }
                }
                entries.insert(
                    key,
                    CacheEntry {
                        geometry: Arc::downgrade(&instance.shapes),
                        bounds,
                    },
                );
            });
        }
    }
    bounds
}
fn overlap_box(min_a: Vec3, max_a: Vec3, min_b: Vec3, max_b: Vec3) -> Option<Contact> {
    let overlap = max_a.min(max_b) - min_a.max(min_b);
    if overlap.min_element() <= 0.0 {
        return None;
    }
    let c_a = (min_a + max_a) * 0.5;
    let c_b = (min_b + max_b) * 0.5;
    let (axis, depth) = if overlap.x < overlap.y && overlap.x < overlap.z {
        (Vec3::X, overlap.x)
    } else if overlap.y < overlap.z {
        (Vec3::Y, overlap.y)
    } else {
        (Vec3::Z, overlap.z)
    };
    let n = if (c_a - c_b).dot(axis) < 0.0 {
        -axis
    } else {
        axis
    };
    Some(Contact { normal: n, depth })
}
fn segment_closest_on_aabb(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> (Vec3, Vec3) {
    let closest = |t: f32| {
        let p = a.lerp(b, t);
        let q = p.clamp(min, max);
        (p, q, (p - q).length_squared())
    };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..22 {
        let l = lo + (hi - lo) / 3.0;
        let r = hi - (hi - lo) / 3.0;
        if closest(l).2 <= closest(r).2 {
            hi = r
        } else {
            lo = l
        }
    }
    let (p, q, _) = closest((lo + hi) * 0.5);
    (p, q)
}

fn sphere_vs_sphere(a: Vec3, ra: f32, b: Vec3, rb: f32) -> Option<Contact> {
    if !a.is_finite()
        || !b.is_finite()
        || !ra.is_finite()
        || !rb.is_finite()
        || ra <= 0.0
        || rb <= 0.0
    {
        return None;
    }
    let delta = a - b;
    let distance = delta.length();
    let radius = ra + rb;
    if distance >= radius {
        return None;
    }
    Some(Contact {
        normal: if distance > 1e-7 {
            delta / distance
        } else {
            Vec3::X
        },
        depth: radius - distance,
    })
}
fn sphere_vs_box(
    center: Vec3,
    radius: f32,
    shape: &CollisionShape,
    instance: &ColliderInstance,
    pos: Vec3,
) -> Option<Contact> {
    let CollisionShape::Box { min, max } = shape else {
        return None;
    };
    if radius <= 0.0 || !radius.is_finite() || !center.is_finite() {
        return None;
    }
    let q = orient(instance.rotation);
    let x = vec(*min) * instance.scale;
    let y = vec(*max) * instance.scale;
    let lo = x.min(y);
    let hi = x.max(y);
    let local = q.conjugate() * (center - pos);
    let nearest = local.clamp(lo, hi);
    let delta = local - nearest;
    let dist = delta.length();
    if dist >= radius {
        return None;
    }
    let (normal, depth) = if dist > 1e-7 {
        (delta / dist, radius - dist)
    } else {
        let options = [
            (local.x - lo.x, Vec3::NEG_X),
            (hi.x - local.x, Vec3::X),
            (local.y - lo.y, Vec3::NEG_Y),
            (hi.y - local.y, Vec3::Y),
            (local.z - lo.z, Vec3::NEG_Z),
            (hi.z - local.z, Vec3::Z),
        ];
        let (distance, outward) = options.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))?;
        (outward, radius + distance.max(0.0))
    };
    Some(Contact {
        normal: q * normal,
        depth,
    })
}
fn sphere_vs_capsule(
    center: Vec3,
    radius: f32,
    a: Vec3,
    b: Vec3,
    capsule_radius: f32,
) -> Option<Contact> {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-12 {
        ((center - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    sphere_vs_sphere(center, radius, a + ab * t, capsule_radius)
}
fn closest_triangle_point(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
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
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-12 {
        return a;
    }
    let inv = 1.0 / denom;
    a + ab * (vb * inv) + ac * (vc * inv)
}
fn sphere_vs_triangle(center: Vec3, radius: f32, triangle: [Vec3; 3]) -> Option<Contact> {
    if radius <= 0.0 || !radius.is_finite() || !center.is_finite() {
        return None;
    }
    let [a, b, c] = triangle;
    let p = closest_triangle_point(center, a, b, c);
    let delta = center - p;
    let d = delta.length();
    if d >= radius {
        return None;
    }
    let n = if d > 1e-7 {
        delta / d
    } else {
        (b - a).cross(c - a).try_normalize().unwrap_or(Vec3::Y)
    };
    Some(Contact {
        normal: n,
        depth: radius - d,
    })
}
fn sphere_geometry(vertices: &[Vec3], min: Vec3, max: Vec3) -> Option<CollisionShape> {
    let extent = max - min;
    let largest = extent.max_element();
    if vertices.len() < 36
        || !min.is_finite()
        || !max.is_finite()
        || largest <= 0.002
        || extent.min_element() / largest < 0.85
    {
        return None;
    }
    let center = (min + max) * 0.5;
    let r = largest * 0.5;
    let stride = (vertices.len() / 2048).max(1);
    let mut close = 0usize;
    let mut seen = 0usize;
    for vertex in vertices.iter().step_by(stride) {
        if !vertex.is_finite() {
            return None;
        }
        let dist = (*vertex - center).length();
        if dist >= r * 0.82 && dist <= r * 1.08 {
            close += 1
        }
        seen += 1;
    }
    (seen >= 36 && close * 100 >= seen * 72).then_some(CollisionShape::Sphere {
        center: center.to_array(),
        radius: r,
    })
}

fn capsule_vs_box(a: Vec3, b: Vec3, r: f32, min: Vec3, max: Vec3) -> Option<Contact> {
    let (p, q) = segment_closest_on_aabb(a, b, min, max);
    let delta = p - q;
    let dist = delta.length();
    if dist >= r {
        return None;
    }
    let (normal, depth) = if dist > 1e-5 {
        (delta / dist, r - dist)
    } else {
        let face = [
            (p.x - min.x, Vec3::NEG_X),
            (max.x - p.x, Vec3::X),
            (p.y - min.y, Vec3::NEG_Y),
            (max.y - p.y, Vec3::Y),
            (p.z - min.z, Vec3::NEG_Z),
            (max.z - p.z, Vec3::Z),
        ];
        let (margin, n) = face.into_iter().min_by(|x, y| x.0.total_cmp(&y.0))?;
        (n, r + margin.max(0.0))
    };
    Some(Contact { normal, depth })
}
fn line_pair(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> (Vec3, Vec3) {
    let u = b - a;
    let v = d - c;
    let w = a - c;
    let aa = u.dot(u);
    let bb = u.dot(v);
    let cc = v.dot(v);
    let dd = u.dot(w);
    let ee = v.dot(w);
    let denom = aa * cc - bb * bb;
    let mut s = if denom > 1e-8 {
        (bb * ee - cc * dd) / denom
    } else {
        0.0
    };
    s = s.clamp(0.0, 1.0);
    let mut t = if cc > 1e-8 { (bb * s + ee) / cc } else { 0.0 };
    t = t.clamp(0.0, 1.0);
    if aa > 1e-8 {
        s = ((bb * t - dd) / aa).clamp(0.0, 1.0);
    }
    (a + u * s, c + v * t)
}
fn capsule_vs_capsule(a: Vec3, b: Vec3, ra: f32, c: Vec3, d: Vec3, rb: f32) -> Option<Contact> {
    let (p, q) = line_pair(a, b, c, d);
    let delta = p - q;
    let distance = delta.length();
    let radius = ra + rb;
    if distance >= radius {
        return None;
    }
    Some(Contact {
        normal: if distance > 1e-5 {
            delta / distance
        } else {
            Vec3::X
        },
        depth: radius - distance,
    })
}
fn tri_aabb(tri: [Vec3; 3], min: Vec3, max: Vec3) -> Option<Contact> {
    let center = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    let v = [tri[0] - center, tri[1] - center, tri[2] - center];
    let edges = [v[1] - v[0], v[2] - v[1], v[0] - v[2]];
    let mut normal = Vec3::Y;
    let mut min_pen = f32::INFINITY;
    let axes = [
        Vec3::X,
        Vec3::Y,
        Vec3::Z,
        edges[0].cross(edges[1]),
        edges[0].cross(Vec3::X),
        edges[0].cross(Vec3::Y),
        edges[0].cross(Vec3::Z),
        edges[1].cross(Vec3::X),
        edges[1].cross(Vec3::Y),
        edges[1].cross(Vec3::Z),
        edges[2].cross(Vec3::X),
        edges[2].cross(Vec3::Y),
        edges[2].cross(Vec3::Z),
    ];
    for axis in axes {
        let len = axis.length();
        if len <= 1e-7 {
            continue;
        }
        let axis = axis / len;
        let p = [axis.dot(v[0]), axis.dot(v[1]), axis.dot(v[2])];
        let t_min = p[0].min(p[1]).min(p[2]);
        let t_max = p[0].max(p[1]).max(p[2]);
        let radius = half.dot(axis.abs());
        if t_min > radius || t_max < -radius {
            return None;
        }
        let pen = (radius - t_min).min(t_max + radius);
        if pen < min_pen {
            min_pen = pen;
            normal = if (v[0] + v[1] + v[2]).dot(axis) > 0.0 {
                axis
            } else {
                -axis
            };
        }
    }
    (min_pen.is_finite() && min_pen > 0.0).then_some(Contact {
        normal,
        depth: min_pen,
    })
}
fn oriented_box(
    shape: &CollisionShape,
    inst: &ColliderInstance,
    pos: Vec3,
) -> (Vec3, [Vec3; 3], Vec3) {
    let CollisionShape::Box { min, max } = shape else {
        unreachable!()
    };
    let q = orient(inst.rotation);
    let a = vec(*min) * inst.scale;
    let b = vec(*max) * inst.scale;
    (
        q * ((a + b) * 0.5) + pos,
        [q * Vec3::X, q * Vec3::Y, q * Vec3::Z],
        (b - a).abs() * 0.5,
    )
}
fn box_pair(
    a: &CollisionShape,
    ai: &ColliderInstance,
    ap: Vec3,
    b: &CollisionShape,
    bi: &ColliderInstance,
    bp: Vec3,
) -> Option<Contact> {
    let (ac, aa, ah) = oriented_box(a, ai, ap);
    let (bc, ba, bh) = oriented_box(b, bi, bp);
    let delta = ac - bc;
    let mut best = Contact {
        normal: Vec3::X,
        depth: f32::INFINITY,
    };
    let axes = aa.into_iter().chain(ba).chain(
        aa.into_iter()
            .flat_map(|a| ba.into_iter().map(move |b| a.cross(b))),
    );
    for axis in axes {
        let Some(n) = axis.try_normalize() else {
            continue;
        };
        let ar = ah.x * n.dot(aa[0]).abs() + ah.y * n.dot(aa[1]).abs() + ah.z * n.dot(aa[2]).abs();
        let br = bh.x * n.dot(ba[0]).abs() + bh.y * n.dot(ba[1]).abs() + bh.z * n.dot(ba[2]).abs();
        let d = ar + br - delta.dot(n).abs();
        if d <= 0.0 {
            return None;
        }
        if d < best.depth {
            best = Contact {
                normal: if delta.dot(n) >= 0.0 { n } else { -n },
                depth: d,
            };
        }
    }
    Some(best)
}
fn capsule_box(
    a: Vec3,
    b: Vec3,
    r: f32,
    shape: &CollisionShape,
    inst: &ColliderInstance,
    pos: Vec3,
) -> Option<Contact> {
    let CollisionShape::Box { min, max } = shape else {
        unreachable!()
    };
    let q = orient(inst.rotation);
    let x = vec(*min) * inst.scale;
    let y = vec(*max) * inst.scale;
    capsule_vs_box(
        q.conjugate() * (a - pos),
        q.conjugate() * (b - pos),
        r,
        x.min(y),
        x.max(y),
    )
    .map(|h| Contact {
        normal: q * h.normal,
        depth: h.depth,
    })
}
pub(crate) fn against_triangle(
    inst: &ColliderInstance,
    pos: Vec3,
    triangle: [Vec3; 3],
) -> Option<Contact> {
    let mut best = None;
    for shape in inst.shapes.iter() {
        let hit = match shape {
            CollisionShape::Sphere { center, radius } => {
                let c = transform(vec(*center), inst) + pos;
                sphere_vs_triangle(c, radius.abs() * inst.scale.abs().max_element(), triangle)
            }
            CollisionShape::Capsule { start, end, radius } => {
                let a = transform(vec(*start), inst) + pos;
                let b = transform(vec(*end), inst) + pos;
                let r = radius * inst.scale.abs().max_element();
                let (axis, face) =
                    crate::engine::physics::triangle_collider::closest_segment_triangle(
                        a, b, triangle,
                    );
                let delta = axis - face;
                let d = delta.length();
                (d < r).then(|| Contact {
                    normal: if d > 1e-6 {
                        delta / d
                    } else {
                        (triangle[1] - triangle[0])
                            .cross(triangle[2] - triangle[0])
                            .normalize_or_zero()
                    },
                    depth: r - d,
                })
            }
            CollisionShape::Box { min, max } => {
                let q = orient(inst.rotation);
                let x = vec(*min) * inst.scale;
                let y = vec(*max) * inst.scale;
                tri_aabb(
                    triangle.map(|p| q.conjugate() * (p - pos)),
                    x.min(y),
                    x.max(y),
                )
                .map(|h| Contact {
                    normal: -(q * h.normal),
                    depth: h.depth,
                })
            }
            CollisionShape::Triangles { .. } => {
                let (lo, hi) = bounds_of_shape(shape, inst);
                tri_aabb(triangle, lo + pos, hi + pos).map(|h| Contact {
                    normal: -h.normal,
                    depth: h.depth,
                })
            }
        };
        if let Some(h) = hit {
            if best.is_none_or(|b: Contact| h.depth < b.depth) {
                best = Some(h);
            }
        }
    }
    best
}
pub fn against_box(
    instance: &ColliderInstance,
    translation: Vec3,
    min: Vec3,
    max: Vec3,
) -> Option<Contact> {
    against_shapes(instance.shapes.as_slice(), instance, translation, min, max)
}
fn against_shapes(
    shapes: &[CollisionShape],
    instance: &ColliderInstance,
    translation: Vec3,
    min: Vec3,
    max: Vec3,
) -> Option<Contact> {
    let mut best = None;
    for shape in shapes {
        let hit = match shape {
            CollisionShape::Sphere { center, radius } => sphere_vs_box(
                transform(vec(*center), instance) + translation,
                radius.abs() * instance.scale.abs().max_element(),
                &CollisionShape::Box {
                    min: min.to_array(),
                    max: max.to_array(),
                },
                &ColliderInstance {
                    shapes: Arc::new(Vec::new()),
                    rotation: Vec3::ZERO,
                    scale: Vec3::ONE,
                },
                Vec3::ZERO,
            ),
            CollisionShape::Box { .. } => {
                let target = CollisionShape::Box {
                    min: min.to_array(),
                    max: max.to_array(),
                };
                let frame = ColliderInstance {
                    shapes: Arc::new(Vec::new()),
                    rotation: Vec3::ZERO,
                    scale: Vec3::ONE,
                };
                box_pair(shape, instance, translation, &target, &frame, Vec3::ZERO)
            }
            CollisionShape::Capsule { start, end, radius } => {
                let a = transform(vec(*start), instance) + translation;
                let b = transform(vec(*end), instance) + translation;
                let r = *radius * instance.scale.abs().max_element();
                capsule_vs_box(a, b, r, min, max)
            }
            CollisionShape::Triangles { vertices } => {
                let mut best = None;
                for tri in vertices {
                    let points = tri.map(|p| transform(vec(p), instance) + translation);
                    if let Some(contact) = tri_aabb(points, min, max) {
                        if best.is_none_or(|x: Contact| contact.depth < x.depth) {
                            best = Some(contact)
                        }
                    }
                }
                best
            }
        };
        if let Some(contact) = hit {
            if best.is_none_or(|x: Contact| contact.depth < x.depth) {
                best = Some(contact)
            }
        }
    }
    best
}
pub fn contact(
    a: Option<&ColliderInstance>,
    position_a: Vec3,
    min_a: Vec3,
    max_a: Vec3,
    b: Option<&ColliderInstance>,
    position_b: Vec3,
    min_b: Vec3,
    max_b: Vec3,
) -> Option<Contact> {
    match (a, b) {
        (Some(a), Some(b)) => between(a, position_a, b, position_b),
        (Some(a), None) => against_box(a, position_a, min_b, max_b),
        (None, Some(b)) => against_box(b, position_b, min_a, max_a).map(|c| Contact {
            normal: -c.normal,
            depth: c.depth,
        }),
        (None, None) => overlap_box(min_a, max_a, min_b, max_b),
    }
}

fn sweep_character_wall_pass(
    character: &ColliderInstance,
    from: Vec3,
    to: Vec3,
    obstacle: Option<&ColliderInstance>,
    obstacle_position: Vec3,
    obstacle_min: Vec3,
    obstacle_max: Vec3,
    max_walkable_slope_degrees: f32,
) -> Option<(Vec3, Contact)> {
    let mesh = obstacle?;
    if !mesh
        .shapes
        .iter()
        .any(|s| matches!(s, CollisionShape::Triangles { .. }))
    {
        return None;
    }
    let travel = to - from;
    let flat_motion = Vec3::new(travel.x, 0.0, travel.z);
    if !travel.is_finite() || flat_motion.length_squared() <= 1e-10 {
        return None;
    }
    let (local_min, local_max) = envelope(character)?;
    let diameter = (local_max - local_min)
        .x
        .min((local_max - local_min).z)
        .max(0.1);
    let steps = (travel.length() / (diameter * 0.20))
        .ceil()
        .clamp(1.0, 256.0) as usize;
    let rotation = orient(mesh.rotation);
    for step in 1..=steps {
        let probe = from + travel * (step as f32 / steps as f32);
        let low = probe + local_min;
        let high = probe + local_max;
        if high.cmplt(obstacle_min).any() || low.cmpgt(obstacle_max).any() {
            continue;
        }
        let mut blocking: Option<Contact> = None;
        let mut score = f32::NEG_INFINITY;
        for shape in mesh.shapes.iter() {
            let CollisionShape::Triangles { vertices } = shape else {
                continue;
            };
            for tri in vertices {
                let points = tri.map(|v| rotation * (vec(v) * mesh.scale) + obstacle_position);
                let plane = (points[1] - points[0]).cross(points[2] - points[0]);
                let plane_length = plane.length();
                if plane_length <= 1e-8 {
                    continue;
                }
                let plane = plane / plane_length;
                if crate::character::collision::contact_is_walkable(plane, max_walkable_slope_degrees, false)
                    || plane.y < -0.65
                {
                    continue;
                }
                let tri_min = points[0].min(points[1]).min(points[2]);
                let tri_max = points[0].max(points[1]).max(points[2]);
                if tri_max.cmplt(low).any() || tri_min.cmpgt(high).any() {
                    continue;
                }
                if tri_max.y <= low.y + 0.44 && tri_min.y >= low.y - 0.10 {
                    continue;
                }
                let Some(hit) = against_triangle(character, probe, points) else {
                    continue;
                };
                if hit.depth <= 0.00001 {
                    continue;
                }
                let mut wall_normal = Vec3::new(plane.x, 0.0, plane.z).normalize_or_zero();
                if wall_normal.length_squared() < 0.1 {
                    continue;
                }
                if (from - points[0]).dot(plane) < 0.0 {
                    wall_normal = -wall_normal;
                }
                let opposition = -wall_normal.dot(flat_motion);
                if opposition <= 0.00001 {
                    continue;
                }
                if opposition > score {
                    score = opposition;
                    blocking = Some(Contact {
                        normal: wall_normal,
                        depth: hit.depth,
                    });
                }
            }
        }
        let Some(hit) = blocking else { continue };
        let corrected = probe + hit.normal * (hit.depth + 0.002);
        let leftover = to - probe;
        let tangent = leftover - hit.normal * leftover.dot(hit.normal).min(0.0);
        return Some((corrected + tangent, hit));
    }
    None
}

pub(crate) fn sweep_character_wall(
    character: &ColliderInstance,
    from: Vec3,
    to: Vec3,
    obstacle: Option<&ColliderInstance>,
    obstacle_position: Vec3,
    obstacle_min: Vec3,
    obstacle_max: Vec3,
    max_walkable_slope_degrees: f32,
) -> Option<(Vec3, Contact)> {
    let mut destination = to;
    let mut last = None;
    for _ in 0..5 {
        let Some((corrected, contact)) = sweep_character_wall_pass(
            character,
            from,
            destination,
            obstacle,
            obstacle_position,
            obstacle_min,
            obstacle_max,
         max_walkable_slope_degrees) else {
            break;
        };
        last = Some(contact);
        if !corrected.is_finite() {
            break;
        }
        let delta = corrected - destination;
        destination = corrected;
        if delta.length_squared() < 1e-8 {
            break;
        }
    }
    last.map(|contact| (destination, contact))
}

pub(crate) fn walkable_mesh_support(
    character: &ColliderInstance,
    from: Vec3,
    to: Vec3,
    obstacle: Option<&ColliderInstance>,
    obstacle_position: Vec3,
    max_walkable_slope_degrees: f32,
) -> Option<f32> {
    let mesh = obstacle?;
    if !from.is_finite() || !to.is_finite() {
        return None;
    }
    let (min, max) = envelope(character)?;
    let radius = ((max.x - min.x).min(max.z - min.z) * 0.5).max(0.02);
    let previous_foot = from.y + min.y;
    let current_foot = to.y + min.y;
    let mut best: Option<f32> = None;
    let q = orient(mesh.rotation);
    let center = glam::Vec2::new(to.x, to.z);
    for shape in mesh.shapes.iter() {
        let CollisionShape::Triangles { vertices } = shape else {
            continue;
        };
        for tri in vertices.iter() {
            let [a, b, c] = tri.map(|v| q * (vec(v) * mesh.scale) + obstacle_position);
            let normal = (b - a).cross(c - a).normalize_or_zero();
            if !crate::character::collision::contact_is_walkable(normal, max_walkable_slope_degrees, false) {
                continue;
            }
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            if center.x < lo.x - radius
                || center.x > hi.x + radius
                || center.y < lo.z - radius
                || center.y > hi.z + radius
                || lo.y > previous_foot + 0.35
                || hi.y < current_foot - 0.10
            {
                continue;
            }
            let points = [
                glam::Vec2::new(a.x, a.z),
                glam::Vec2::new(b.x, b.z),
                glam::Vec2::new(c.x, c.z),
            ];
            let u = points[1] - points[0];
            let v = points[2] - points[0];
            let w = center - points[0];
            let denominator = u.perp_dot(v);
            if denominator.abs() < 1e-7 {
                continue;
            }
            let s = w.perp_dot(v) / denominator;
            let t = u.perp_dot(w) / denominator;
            let support_y;
            if s >= 0.0 && t >= 0.0 && s + t <= 1.0 {
                support_y = a.y * (1.0 - s - t) + b.y * s + c.y * t;
            } else {
                let mut nearest = f32::INFINITY;
                let mut y = 0.0;
                for (p1, p2, h1, h2) in [
                    (points[0], points[1], a.y, b.y),
                    (points[1], points[2], b.y, c.y),
                    (points[2], points[0], c.y, a.y),
                ] {
                    let seg = p2 - p1;
                    let t = if seg.length_squared() > 1e-8 {
                        ((center - p1).dot(seg) / seg.length_squared()).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let d = (center - p1 - seg * t).length_squared();
                    if d < nearest {
                        nearest = d;
                        y = h1 + (h2 - h1) * t;
                    }
                }
                if nearest > radius * radius {
                    continue;
                }
                support_y = y;
            }
            if support_y > previous_foot + 0.35
                || support_y < current_foot - 0.10
                || current_foot < support_y - 0.35
            {
                continue;
            }
            if best.is_none_or(|height| support_y > height) {
                best = Some(support_y);
            }
        }
    }
    best.map(|y| to.y + (y - current_foot))
}

pub(crate) fn within_resting_contact_skin(
    a: &ColliderInstance,
    pos_a: Vec3,
    b: &ColliderInstance,
    pos_b: Vec3,
    skin: f32,
) -> bool {
    if !skin.is_finite() || !(0.0..=0.05).contains(&skin) {
        return false;
    }
    for sa in a.shapes.iter() {
        for sb in b.shapes.iter() {
            let touching = match (sa, sb) {
                (
                    CollisionShape::Sphere {
                        center: ca,
                        radius: ra,
                    },
                    CollisionShape::Sphere {
                        center: cb,
                        radius: rb,
                    },
                ) => {
                    let a_center = transform(vec(*ca), a) + pos_a;
                    let b_center = transform(vec(*cb), b) + pos_b;
                    let reach = ra.abs() * a.scale.abs().max_element()
                        + rb.abs() * b.scale.abs().max_element()
                        + skin;
                    (a_center - b_center).length_squared() <= reach * reach
                }
                (CollisionShape::Sphere { center, radius }, CollisionShape::Box { .. }) => {
                    sphere_vs_box(
                        transform(vec(*center), a) + pos_a,
                        radius.abs() * a.scale.abs().max_element() + skin,
                        sb,
                        b,
                        pos_b,
                    )
                    .is_some()
                }
                (CollisionShape::Box { .. }, CollisionShape::Sphere { center, radius }) => {
                    sphere_vs_box(
                        transform(vec(*center), b) + pos_b,
                        radius.abs() * b.scale.abs().max_element() + skin,
                        sa,
                        a,
                        pos_a,
                    )
                    .is_some()
                }
                (
                    CollisionShape::Sphere { center, radius },
                    CollisionShape::Capsule {
                        start,
                        end,
                        radius: r,
                    },
                ) => sphere_vs_capsule(
                    transform(vec(*center), a) + pos_a,
                    radius.abs() * a.scale.abs().max_element() + skin,
                    transform(vec(*start), b) + pos_b,
                    transform(vec(*end), b) + pos_b,
                    r.abs() * b.scale.abs().max_element(),
                )
                .is_some(),
                (
                    CollisionShape::Capsule {
                        start,
                        end,
                        radius: r,
                    },
                    CollisionShape::Sphere { center, radius },
                ) => sphere_vs_capsule(
                    transform(vec(*center), b) + pos_b,
                    radius.abs() * b.scale.abs().max_element() + skin,
                    transform(vec(*start), a) + pos_a,
                    transform(vec(*end), a) + pos_a,
                    r.abs() * a.scale.abs().max_element(),
                )
                .is_some(),
                (
                    CollisionShape::Capsule {
                        start: a0,
                        end: a1,
                        radius: ra,
                    },
                    CollisionShape::Capsule {
                        start: b0,
                        end: b1,
                        radius: rb,
                    },
                ) => {
                    let (p, q) = line_pair(
                        transform(vec(*a0), a) + pos_a,
                        transform(vec(*a1), a) + pos_a,
                        transform(vec(*b0), b) + pos_b,
                        transform(vec(*b1), b) + pos_b,
                    );
                    let reach =
                        ra * a.scale.abs().max_element() + rb * b.scale.abs().max_element() + skin;
                    (p - q).length_squared() <= reach * reach
                }
                (CollisionShape::Capsule { start, end, radius }, CollisionShape::Box { .. }) => {
                    let r = radius * a.scale.abs().max_element() + skin;
                    capsule_box(
                        transform(vec(*start), a) + pos_a,
                        transform(vec(*end), a) + pos_a,
                        r,
                        sb,
                        b,
                        pos_b,
                    )
                    .is_some()
                }
                (CollisionShape::Box { .. }, CollisionShape::Capsule { start, end, radius }) => {
                    let r = radius * b.scale.abs().max_element() + skin;
                    capsule_box(
                        transform(vec(*start), b) + pos_b,
                        transform(vec(*end), b) + pos_b,
                        r,
                        sa,
                        a,
                        pos_a,
                    )
                    .is_some()
                }
                (CollisionShape::Box { min, max }, CollisionShape::Box { .. }) => {
                    let offset = Vec3::splat(skin) / a.scale.abs().max(Vec3::splat(1e-6));
                    let expanded = CollisionShape::Box {
                        min: (vec(*min) - offset).to_array(),
                        max: (vec(*max) + offset).to_array(),
                    };
                    box_pair(&expanded, a, pos_a, sb, b, pos_b).is_some()
                }
                _ => false,
            };
            if touching {
                return true;
            }
        }
    }
    false
}

pub fn between(
    a: &ColliderInstance,
    pos_a: Vec3,
    b: &ColliderInstance,
    pos_b: Vec3,
) -> Option<Contact> {
    let mut best = None;
    for sa in a.shapes.iter() {
        for sb in b.shapes.iter() {
            let hit = match (sa, sb) {
                (
                    CollisionShape::Sphere {
                        center: ca,
                        radius: ra,
                    },
                    CollisionShape::Sphere {
                        center: cb,
                        radius: rb,
                    },
                ) => sphere_vs_sphere(
                    transform(vec(*ca), a) + pos_a,
                    ra.abs() * a.scale.abs().max_element(),
                    transform(vec(*cb), b) + pos_b,
                    rb.abs() * b.scale.abs().max_element(),
                ),
                (CollisionShape::Sphere { center, radius }, CollisionShape::Box { .. }) => {
                    sphere_vs_box(
                        transform(vec(*center), a) + pos_a,
                        radius.abs() * a.scale.abs().max_element(),
                        sb,
                        b,
                        pos_b,
                    )
                }
                (CollisionShape::Box { .. }, CollisionShape::Sphere { center, radius }) => {
                    sphere_vs_box(
                        transform(vec(*center), b) + pos_b,
                        radius.abs() * b.scale.abs().max_element(),
                        sa,
                        a,
                        pos_a,
                    )
                    .map(|h| Contact {
                        normal: -h.normal,
                        depth: h.depth,
                    })
                }
                (
                    CollisionShape::Sphere { center, radius },
                    CollisionShape::Capsule {
                        start,
                        end,
                        radius: r,
                    },
                ) => sphere_vs_capsule(
                    transform(vec(*center), a) + pos_a,
                    radius.abs() * a.scale.abs().max_element(),
                    transform(vec(*start), b) + pos_b,
                    transform(vec(*end), b) + pos_b,
                    r.abs() * b.scale.abs().max_element(),
                ),
                (
                    CollisionShape::Capsule {
                        start,
                        end,
                        radius: r,
                    },
                    CollisionShape::Sphere { center, radius },
                ) => sphere_vs_capsule(
                    transform(vec(*center), b) + pos_b,
                    radius.abs() * b.scale.abs().max_element(),
                    transform(vec(*start), a) + pos_a,
                    transform(vec(*end), a) + pos_a,
                    r.abs() * a.scale.abs().max_element(),
                )
                .map(|h| Contact {
                    normal: -h.normal,
                    depth: h.depth,
                }),
                (analytic, CollisionShape::Triangles { vertices })
                    if !matches!(analytic, CollisionShape::Triangles { .. }) =>
                {
                    let body = ColliderInstance {
                        shapes: Arc::new(vec![analytic.clone()]),
                        rotation: a.rotation,
                        scale: a.scale,
                    };
                    vertices
                        .iter()
                        .filter_map(|t| {
                            against_triangle(&body, pos_a, t.map(|v| transform(vec(v), b) + pos_b))
                        })
                        .min_by(|a, b| a.depth.total_cmp(&b.depth))
                }
                (CollisionShape::Triangles { vertices }, analytic)
                    if !matches!(analytic, CollisionShape::Triangles { .. }) =>
                {
                    let body = ColliderInstance {
                        shapes: Arc::new(vec![analytic.clone()]),
                        rotation: b.rotation,
                        scale: b.scale,
                    };
                    vertices
                        .iter()
                        .filter_map(|t| {
                            against_triangle(&body, pos_b, t.map(|v| transform(vec(v), a) + pos_a))
                        })
                        .min_by(|a, b| a.depth.total_cmp(&b.depth))
                        .map(|h| Contact {
                            normal: -h.normal,
                            depth: h.depth,
                        })
                }
                (CollisionShape::Box { .. }, CollisionShape::Box { .. }) => {
                    box_pair(sa, a, pos_a, sb, b, pos_b)
                }
                (CollisionShape::Capsule { start, end, radius }, CollisionShape::Box { .. }) => {
                    capsule_box(
                        transform(vec(*start), a) + pos_a,
                        transform(vec(*end), a) + pos_a,
                        radius * a.scale.abs().max_element(),
                        sb,
                        b,
                        pos_b,
                    )
                }
                (CollisionShape::Box { .. }, CollisionShape::Capsule { start, end, radius }) => {
                    capsule_box(
                        transform(vec(*start), b) + pos_b,
                        transform(vec(*end), b) + pos_b,
                        radius * b.scale.abs().max_element(),
                        sa,
                        a,
                        pos_a,
                    )
                    .map(|h| Contact {
                        normal: -h.normal,
                        depth: h.depth,
                    })
                }
                (
                    CollisionShape::Capsule {
                        start: a0,
                        end: a1,
                        radius: ra,
                    },
                    CollisionShape::Capsule {
                        start: b0,
                        end: b1,
                        radius: rb,
                    },
                ) => capsule_vs_capsule(
                    transform(vec(*a0), a) + pos_a,
                    transform(vec(*a1), a) + pos_a,
                    ra * a.scale.abs().max_element(),
                    transform(vec(*b0), b) + pos_b,
                    transform(vec(*b1), b) + pos_b,
                    rb * b.scale.abs().max_element(),
                ),
                _ => {
                    let (lo_b, hi_b) = bounds_of_shape(sb, b);
                    against_one(sa, a, pos_a, lo_b + pos_b, hi_b + pos_b)
                }
            };
            if let Some(contact) = hit {
                if best.is_none_or(|c: Contact| contact.depth < c.depth) {
                    best = Some(contact)
                }
            }
        }
    }
    best
}
fn against_one(
    shape: &CollisionShape,
    inst: &ColliderInstance,
    pos: Vec3,
    min: Vec3,
    max: Vec3,
) -> Option<Contact> {
    against_shapes(std::slice::from_ref(shape), inst, pos, min, max)
}
pub fn package_shapes(vertices: &[Vec3], min: Vec3, max: Vec3) -> Vec<CollisionShape> {
    if vertices.is_empty() || !min.is_finite() || !max.is_finite() {
        return Vec::new();
    }
    let extent = max - min;
    if let Some(sphere) = sphere_geometry(vertices, min, max) {
        return vec![sphere];
    }
    if extent.min_element() < 0.002 {
        return Vec::new();
    }
    let triangles = vertices.len() / 3;
    if triangles == 0 {
        return Vec::new();
    }
    if triangles > 80_000 {
        return Vec::new();
    }
    let mut parent = (0..triangles).collect::<Vec<_>>();
    fn root(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    fn unite(parent: &mut [usize], a: usize, b: usize) {
        let a = root(parent, a);
        let b = root(parent, b);
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    }
    let epsilon = (extent.max_element() * 0.00001).clamp(0.00001, 0.004);
    let mut shared = std::collections::HashMap::<(i64, i64, i64), usize>::new();
    for (i, points) in vertices[..triangles * 3].chunks_exact(3).enumerate() {
        for p in points {
            if !p.is_finite() {
                return Vec::new();
            }
            let key = (
                (p.x / epsilon).round() as i64,
                (p.y / epsilon).round() as i64,
                (p.z / epsilon).round() as i64,
            );
            if let Some(&other) = shared.get(&key) {
                unite(&mut parent, i, other);
            } else {
                shared.insert(key, i);
            }
        }
    }
    let mut components = std::collections::HashMap::<usize, Vec<usize>>::new();
    for tri in 0..triangles {
        let group = root(&mut parent, tri);
        components.entry(group).or_default().push(tri);
    }
    struct Part {
        indices: Vec<usize>,
        lo: Vec3,
        hi: Vec3,
    }
    let mut parts = Vec::new();
    for (_, indices) in components {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for &tri in &indices {
            for &p in &vertices[tri * 3..tri * 3 + 3] {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        parts.push(Part { indices, lo, hi });
    }
    let foot_tolerance = epsilon * 4.0;
    let mut ground = Vec::new();
    let mut overhead = Vec::new();
    for part in parts {
        if part.lo.y <= min.y + foot_tolerance {
            ground.push(part)
        } else {
            overhead.push(part)
        }
    }
    let supports = ground
        .iter()
        .filter(|p| {
            let d = p.hi - p.lo;
            d.y > d.x.max(d.z)
        })
        .collect::<Vec<_>>();
    let floating_crown = overhead.iter().any(|p| {
        let d = p.hi - p.lo;
        supports.iter().any(|g| {
            let stem = g.hi - g.lo;
            d.x.max(d.z) > stem.x.max(stem.z) && p.lo.y > g.lo.y + foot_tolerance
        })
    });
    if !supports.is_empty() && !overhead.is_empty() && floating_crown {
        let mut shafts = Vec::new();
        for p in supports {
            let vertical = (p.hi.y - p.lo.y).max(epsilon);
            let slices = ((vertical / extent.max_element() * 12.0).round() as usize).clamp(3, 16);
            let mut nodes = Vec::new();
            for index in 0..=slices {
                let y = p.lo.y + vertical * index as f32 / slices as f32;
                let mut crossing = Vec::<Vec3>::new();
                for &tri in &p.indices {
                    let pts = &vertices[tri * 3..tri * 3 + 3];
                    for k in 0..3 {
                        let a = pts[k];
                        let b = pts[(k + 1) % 3];
                        if (a.y <= y && b.y >= y) || (b.y <= y && a.y >= y) {
                            let ratio = if (a.y - b.y).abs() <= epsilon {
                                0.0
                            } else {
                                (y - a.y) / (b.y - a.y)
                            };
                            crossing.push(a.lerp(b, ratio.clamp(0.0, 1.0)));
                        }
                    }
                }
                if crossing.is_empty() {
                    continue;
                }
                let center =
                    crossing.iter().fold(Vec3::ZERO, |a, p| a + *p) / crossing.len() as f32;
                let radius = crossing
                    .iter()
                    .map(|p| Vec3::new(p.x - center.x, 0.0, p.z - center.z).length())
                    .fold(0.0, f32::max)
                    .max(0.01);
                nodes.push((center, radius));
            }
            for pair in nodes.windows(2) {
                let (a, ra) = pair[0];
                let (b, rb) = pair[1];
                if (a - b).length_squared() > epsilon * epsilon {
                    shafts.push(CollisionShape::Capsule {
                        start: p3(a),
                        end: p3(b),
                        radius: ra.max(rb),
                    });
                }
            }
        }
        if !shafts.is_empty() {
            return shafts;
        }
    }
    let data = vertices[..triangles * 3]
        .chunks_exact(3)
        .map(|tri| [p3(tri[0]), p3(tri[1]), p3(tri[2])])
        .collect::<Vec<_>>();
    vec![CollisionShape::Triangles { vertices: data }]
}

pub fn set_actor_package_collision(
    actor: &mut crate::world::Actor,
    package: &crate::engine::entity_package::EntityPackage,
) {
    actor.physics.collision_mode_explicit = true;
    if package.character.is_some() || actor_has_character_capability(actor) {
        actor.physics.collision_mode = CollisionMode::Box;
        return;
    }
    let shapes = package.collision_shapes();
    if shapes.is_empty() {
        actor.physics.collision_mode = if package.triangle_mesh_bounds().is_some() {
            CollisionMode::Disabled
        } else {
            CollisionMode::Box
        };
        return;
    }
    actor.physics.collision_mode = CollisionMode::Auto;
    let collider = ColliderInstance {
        shapes,
        rotation: Vec3::ZERO,
        scale: Vec3::ONE,
    };
    if let Some((lo, hi)) = envelope(&collider) {
        actor.physics.collision_size = (hi - lo).max(Vec3::splat(0.01));
        actor.physics.collision_offset = (lo + hi) * 0.5;
    }
}

pub fn resolved_actor_collision_mode(actor: &crate::world::Actor) -> CollisionMode {
    if actor.physics.collision_mode != CollisionMode::Disabled
        && actor_has_character_capability(actor)
    {
        return CollisionMode::Box;
    }
    if actor.physics.collision_mode != CollisionMode::Box || actor.physics.collision_mode_explicit {
        return actor.physics.collision_mode;
    }
    let implicit = actor
        .package
        .as_deref()
        .and_then(crate::engine::entity_package::get_package)
        .and_then(|p| p.triangle_mesh_bounds())
        .is_some_and(|(lo, hi)| {
            (actor.physics.collision_size - (hi - lo).max(Vec3::splat(0.01)))
                .abs()
                .max_element()
                < 0.0001
                && (actor.physics.collision_offset - (hi + lo) * 0.5)
                    .abs()
                    .max_element()
                    < 0.0001
        });
    if implicit {
        CollisionMode::Auto
    } else {
        CollisionMode::Box
    }
}
fn actor_has_character_capability(actor: &crate::world::Actor) -> bool {
    actor.package.as_deref().is_some_and(|name| {
        crate::character::package::get_package(name).is_some()
            || crate::engine::entity_package::get_package(name)
                .is_some_and(|package| package.character.is_some())
    })
}

pub(crate) fn character_actor_collider(actor: &crate::world::Actor) -> Option<ColliderInstance> {
    if !actor.enabled {
        return None;
    }
    let mut physics = crate::engine::entity::EntityPhysicsState::default();
    let mode = if actor.physics.collision_mode == CollisionMode::Disabled {
        CollisionMode::Disabled
    } else {
        CollisionMode::Box
    };
    configure_runtime_actor_mode(actor, &mut physics, mode);
    physics.collider
}
pub fn configure_runtime_actor(
    actor: &crate::world::Actor,
    physics: &mut crate::engine::entity::EntityPhysicsState,
) {
    configure_runtime_actor_mode(actor, physics, resolved_actor_collision_mode(actor));
}
fn configure_runtime_actor_mode(
    actor: &crate::world::Actor,
    physics: &mut crate::engine::entity::EntityPhysicsState,
    mode: CollisionMode,
) {
    physics.collider = None;
    physics.enabled = actor.physics.enabled;
    physics.collision_enabled &= actor.physics.collision_enabled;
    if mode == CollisionMode::Disabled || !physics.enabled || !physics.collision_enabled {
        physics.collision_enabled = false;
        return;
    }
    let shapes = if mode == CollisionMode::Auto {
        let Some(package) = actor
            .package
            .as_deref()
            .and_then(crate::engine::entity_package::get_package)
        else {
            physics.collision_enabled = false;
            return;
        };
        package.collision_shapes()
    } else {
        let half = actor.physics.collision_size * 0.5;
        let center = actor.physics.collision_offset;
        Arc::new(vec![CollisionShape::Box {
            min: (center - half).to_array(),
            max: (center + half).to_array(),
        }])
    };
    if shapes.is_empty() {
        physics.collision_enabled = false;
        return;
    }
    let instance = ColliderInstance {
        shapes,
        rotation: actor.rotation,
        scale: actor.scale,
    };
    if let Some((lo, hi)) = envelope(&instance) {
        physics.size = (hi - lo).max(Vec3::splat(0.01));
        physics.offset = (lo + hi) * 0.5;
        physics.collider = Some(instance);
    }
}
pub fn preview_actor_collider(actor: &crate::world::Actor) -> Option<ColliderInstance> {
    if !actor.enabled {
        return None;
    }
    let mut physics = crate::engine::entity::EntityPhysicsState::default();
    configure_runtime_actor(actor, &mut physics);
    physics
        .collision_enabled
        .then_some(physics.collider)
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn character_box_uses_authored_dimensions_without_loading_visual_geometry() {
        let mut actor = crate::world::Actor::new(1, "animated body", Vec3::ZERO);
        actor.package = Some("unavailable_visual_mesh".into());
        actor.physics.collision_mode = CollisionMode::Auto;
        actor.physics.collision_size = Vec3::new(0.4, 1.8, 0.6);
        actor.physics.collision_offset = Vec3::new(0.1, 0.9, 0.0);
        actor.scale = Vec3::splat(1.5);
        let body =
            character_actor_collider(&actor).expect("Character Box must not require mesh loading");
        assert_eq!(body.shapes.len(), 1);
        assert!(matches!(body.shapes[0], CollisionShape::Box { .. }));
        let (lo, hi) = envelope(&body).unwrap();
        assert!((hi - lo - Vec3::new(0.6, 2.7, 0.9)).length() < 0.00001);
        assert!(((lo + hi) * 0.5 - actor.physics.collision_offset * 1.5).length() < 0.00001);
        actor.physics.collision_mode = CollisionMode::Disabled;
        assert!(character_actor_collider(&actor).is_none());
        actor.physics.collision_mode = CollisionMode::Auto;
        actor.physics.enabled = false;
        assert!(character_actor_collider(&actor).is_none());
        actor.physics.enabled = true;
        actor.physics.collision_enabled = false;
        assert!(character_actor_collider(&actor).is_none());
        actor.physics.collision_enabled = true;
        actor.enabled = false;
        assert!(character_actor_collider(&actor).is_none());
    }
    #[test]
    fn canopy_is_not_solid_but_trunk_is() {
        fn add_box(verts: &mut Vec<Vec3>, lo: Vec3, hi: Vec3) {
            let p = [
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, lo.z),
                Vec3::new(hi.x, hi.y, lo.z),
                Vec3::new(lo.x, hi.y, lo.z),
                Vec3::new(lo.x, lo.y, hi.z),
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(hi.x, hi.y, hi.z),
                Vec3::new(lo.x, hi.y, hi.z),
            ];
            for [a, b, c, d] in [
                [0, 1, 2, 3],
                [4, 5, 6, 7],
                [0, 4, 7, 3],
                [1, 5, 6, 2],
                [0, 1, 5, 4],
                [3, 2, 6, 7],
            ] {
                verts.extend([p[a], p[b], p[c], p[a], p[c], p[d]]);
            }
        }
        let mut verts = Vec::new();
        add_box(
            &mut verts,
            Vec3::new(-0.12, 0.0, -0.12),
            Vec3::new(0.12, 6.0, 0.12),
        );
        add_box(
            &mut verts,
            Vec3::new(-2.0, 6.0, -2.0),
            Vec3::new(2.0, 10.0, 2.0),
        );
        let shapes = package_shapes(
            &verts,
            Vec3::new(-2.0, 0.0, -2.0),
            Vec3::new(2.0, 10.0, 2.0),
        );
        assert!(
            shapes
                .iter()
                .all(|x| matches!(x, CollisionShape::Capsule { .. }))
        );
        let collider = ColliderInstance {
            shapes: Arc::new(shapes),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        assert!(
            against_box(
                &collider,
                Vec3::ZERO,
                Vec3::new(1.4, 1.0, -0.1),
                Vec3::new(1.7, 2.5, 0.1)
            )
            .is_none()
        );
        assert!(
            against_box(
                &collider,
                Vec3::ZERO,
                Vec3::new(-0.1, 1.0, -0.1),
                Vec3::new(0.1, 2.5, 0.1)
            )
            .is_some()
        );
    }

    #[test]
    fn play_mode_dense_palm_grove_has_trunk_contacts_but_no_canopy_walls() {
        use crate::engine::entity::EntityManager;
        use crate::engine::physics::PhysicsWorld;
        let mut manager = EntityManager::new();
        let trunk = Arc::new(vec![CollisionShape::Capsule {
            start: [0.0, 0.3, 0.0],
            end: [0.0, 5.0, 0.0],
            radius: 0.20,
        }]);
        for z in 0..5 {
            for x in 0..5 {
                let id = manager.create_entity("Palm");
                let entity = manager.get_entity_mut(id).unwrap();
                entity.position = Vec3::new(x as f32 * 3.0, 0.0, z as f32 * 3.0);
                entity.physics.enabled = true;
                entity.physics.anchored = true;
                entity.physics.solid = true;
                entity.physics.collision_enabled = true;
                entity.physics.size = Vec3::new(0.4, 5.1, 0.4);
                entity.physics.offset = Vec3::new(0.0, 2.65, 0.0);
                entity.physics.collider = Some(ColliderInstance {
                    shapes: trunk.clone(),
                    rotation: Vec3::ZERO,
                    scale: Vec3::ONE,
                });
            }
        }
        let player = manager.create_entity("Player");
        {
            let entity = manager.get_entity_mut(player).unwrap();
            entity.position = Vec3::new(1.5, 1.0, 1.5);
            entity.physics.enabled = true;
            entity.physics.anchored = false;
            entity.physics.solid = true;
            entity.physics.collision_enabled = true;
            entity.physics.size = Vec3::new(0.7, 2.0, 0.7);
            entity.physics.offset = Vec3::Y;
        }
        let physics = PhysicsWorld::new();
        let open_path = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(
            open_path.current.is_empty(),
            "tree leaves may overlap but must not become walls"
        );
        manager.set_position(player, Vec3::new(0.0, 1.0, 0.0));
        let trunk_contact = physics.process_entity_contacts_and_collisions(&mut manager, None);
        assert!(
            trunk_contact
                .current
                .iter()
                .any(|pair| pair.first == player || pair.second == player),
            "a solid trunk must still block the player"
        );
    }
}

pub(crate) fn navigation_triangles(instance: &ColliderInstance, position: Vec3) -> Vec<[Vec3; 3]> {
    let mut out = Vec::new();
    for shape in instance.shapes.iter() {
        let first = out.len();
        match shape {
            CollisionShape::Sphere { center, radius } => {
                let c = transform(vec(*center), instance) + position;
                let r = radius.abs() * instance.scale.abs().max_element()
                    / (std::f32::consts::PI / 16.0).cos().powi(2);
                let v = |lat: usize, lon: usize| {
                    let theta = (lat as f32 / 16.0) * std::f32::consts::PI;
                    let phi = (lon as f32 / 32.0) * std::f32::consts::TAU;
                    c + Vec3::new(
                        theta.sin() * phi.cos(),
                        theta.cos(),
                        theta.sin() * phi.sin(),
                    ) * r
                };
                for lat in 0..16 {
                    for lon in 0..32 {
                        let a = v(lat, lon);
                        let b = v(lat, lon + 1);
                        let d = v(lat + 1, lon);
                        let e = v(lat + 1, lon + 1);
                        if lat != 0 {
                            out.push([a, b, d]);
                        }
                        if lat != 15 {
                            out.push([b, e, d]);
                        }
                    }
                }
            }
            CollisionShape::Triangles { vertices } => out.extend(
                vertices
                    .iter()
                    .map(|t| t.map(|p| transform(vec(p), instance) + position)),
            ),
            CollisionShape::Box { min, max } => {
                let lo = vec(*min);
                let hi = vec(*max);
                let v: [Vec3; 8] = std::array::from_fn(|i| {
                    transform(
                        Vec3::new(
                            if i & 1 == 0 { lo.x } else { hi.x },
                            if i & 2 == 0 { lo.y } else { hi.y },
                            if i & 4 == 0 { lo.z } else { hi.z },
                        ),
                        instance,
                    ) + position
                });
                for [a, b, c] in [
                    [0, 4, 6],
                    [0, 6, 2],
                    [1, 3, 7],
                    [1, 7, 5],
                    [0, 1, 5],
                    [0, 5, 4],
                    [2, 6, 7],
                    [2, 7, 3],
                    [0, 2, 3],
                    [0, 3, 1],
                    [4, 5, 7],
                    [4, 7, 6],
                ] {
                    out.push([v[a], v[b], v[c]]);
                }
            }
            CollisionShape::Capsule { start, end, radius } => {
                let a = transform(vec(*start), instance) + position;
                let b = transform(vec(*end), instance) + position;
                let axis = (b - a).try_normalize().unwrap_or(Vec3::Y);
                let side = axis.cross(Vec3::X).try_normalize().unwrap_or(Vec3::Z);
                let up = axis.cross(side);
                let r = radius * instance.scale.abs().max_element()
                    / ((std::f32::consts::PI / 16.0).cos().powi(2));
                let rings: Vec<[Vec3; 16]> = (0..=16)
                    .map(|j| {
                        let angle =
                            -std::f32::consts::FRAC_PI_2 + j as f32 * std::f32::consts::PI / 16.0;
                        let center = if j <= 8 { a } else { b };
                        std::array::from_fn(|i| {
                            let theta = i as f32 * std::f32::consts::TAU / 16.0;
                            center
                                + axis * (angle.sin() * r)
                                + (side * theta.cos() + up * theta.sin()) * (angle.cos() * r)
                        })
                    })
                    .collect();
                let mut rings = rings;
                rings.insert(9, std::array::from_fn(|i| rings[8][i] + (b - a)));
                for pair in rings.windows(2) {
                    for i in 0..16 {
                        let k = (i + 1) % 16;
                        out.push([pair[0][i], pair[1][i], pair[1][k]]);
                        out.push([pair[0][i], pair[1][k], pair[0][k]]);
                    }
                }
                for t in &mut out[first..] {
                    t.swap(1, 2);
                }
            }
        }
        if !matches!(shape, CollisionShape::Capsule { .. })
            && instance.scale.x * instance.scale.y * instance.scale.z < 0.0
        {
            for t in &mut out[first..] {
                t.swap(1, 2);
            }
        }
    }
    out
}

#[cfg(test)]
mod active_geometry_contracts {
    use super::*;
    #[test]
    fn resting_contact_skin_uses_rotated_primitives_not_world_envelopes() {
        let box_shape = CollisionShape::Box {
            min: [-0.5, -0.5, -0.5],
            max: [0.5, 0.5, 0.5],
        };
        let angle = std::f32::consts::FRAC_PI_4;
        let box_a = ColliderInstance {
            shapes: Arc::new(vec![box_shape.clone()]),
            rotation: Vec3::new(0.0, angle, 0.0),
            scale: Vec3::ONE,
        };
        let box_b = box_a.clone();
        let normal = orient(box_a.rotation) * Vec3::X;
        assert!(within_resting_contact_skin(
            &box_a,
            Vec3::ZERO,
            &box_b,
            normal * 1.005,
            0.01,
        ));
        assert!(!within_resting_contact_skin(
            &box_a,
            Vec3::ZERO,
            &box_b,
            normal * 1.05,
            0.01,
        ));
        let tangent = orient(box_a.rotation) * Vec3::Z;
        assert!(!within_resting_contact_skin(
            &box_a,
            Vec3::ZERO,
            &box_b,
            normal * 1.05 + tangent * 0.15,
            0.01,
        ));

        let capsule = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Capsule {
                start: [0.0, 0.25, 0.0],
                end: [0.0, 1.0, 0.0],
                radius: 0.25,
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        assert!(within_resting_contact_skin(
            &capsule,
            Vec3::ZERO,
            &capsule,
            Vec3::new(0.505, 0.0, 0.0),
            0.01,
        ));
        assert!(!within_resting_contact_skin(
            &capsule,
            Vec3::ZERO,
            &capsule,
            Vec3::new(0.54, 0.0, 0.0),
            0.01,
        ));
        assert!(!within_resting_contact_skin(
            &capsule,
            Vec3::ZERO,
            &capsule,
            Vec3::new(0.0, 3.0, 0.0),
            0.01,
        ));
    }

    #[test]
    fn active_geometry_rotated_custom_box_does_not_solidify_its_envelope() {
        let body = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-2.0, 0.0, -0.1],
                max: [2.0, 1.0, 0.1],
            }]),
            rotation: Vec3::new(0.0, std::f32::consts::FRAC_PI_4, 0.0),
            scale: Vec3::ONE,
        };
        let margin = Vec3::splat(0.05);
        let empty = Vec3::new(1.3, 0.5, 1.3);
        let solid = Vec3::new(1.0, 0.5, -1.0);
        assert!(against_box(&body, Vec3::ZERO, empty - margin, empty + margin).is_none());
        assert!(against_box(&body, Vec3::ZERO, solid - margin, solid + margin).is_some());
    }
    #[test]
    fn active_geometry_explicit_custom_box_survives_storage_roundtrip() {
        let mut a = crate::world::Actor::new(81, "custom", Vec3::ZERO);
        a.physics.collision_mode = CollisionMode::Box;
        a.physics.collision_mode_explicit = true;
        let stored = crate::world::storage::StoredActor::from_actor(&a);
        let bytes = serde_json::to_vec(&stored).unwrap();
        let b = serde_json::from_slice::<crate::world::storage::StoredActor>(&bytes)
            .unwrap()
            .into_actor();
        assert!(b.physics.collision_mode_explicit);
        assert_eq!(resolved_actor_collision_mode(&b), CollisionMode::Box);
        assert_eq!(preview_actor_collider(&a), preview_actor_collider(&b));
    }
}

#[cfg(test)]
mod selected_triangle_contracts {
    use super::*;
    #[test]
    fn active_geometry_analytic_contacts_use_triangle_surface_not_its_envelope() {
        let mesh = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Triangles {
                vertices: vec![[[0.0, 0.0, 0.0], [0.0, 4.0, 0.0], [0.0, 0.0, 4.0]]],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        for shape in [
            CollisionShape::Capsule {
                start: [0.0, 0.2, 0.0],
                end: [0.0, 0.6, 0.0],
                radius: 0.2,
            },
            CollisionShape::Box {
                min: [-0.2, 0.0, -0.2],
                max: [0.2, 0.6, 0.2],
            },
        ] {
            let body = ColliderInstance {
                shapes: Arc::new(vec![shape]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            };
            let empty = Vec3::new(0.1, 3.1, 3.1);
            let inside = Vec3::new(0.1, 1.0, 1.0);
            assert!(between(&body, empty, &mesh, Vec3::ZERO).is_none());
            assert!(between(&mesh, Vec3::ZERO, &body, empty).is_none());
            assert!(between(&body, inside, &mesh, Vec3::ZERO).is_some());
            assert!(between(&mesh, Vec3::ZERO, &body, inside).is_some());
        }
    }
}

#[cfg(test)]
mod wall_contact_regression {
    use super::*;
    #[test]
    fn corner_wall_contact_must_not_be_hidden_by_shallow_floor_overlap() {
        let walking_body = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Capsule {
                start: [0.0, 0.4, 0.0],
                end: [0.0, 1.4, 0.0],
                radius: 0.4,
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let triangles = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Triangles {
                vertices: vec![
                    [[-3.0, 0.0, -3.0], [-3.0, 0.0, 3.0], [3.0, 0.0, -3.0]],
                    [[3.0, 0.0, -3.0], [-3.0, 0.0, 3.0], [3.0, 0.0, 3.0]],
                    [[0.0, 0.0, -3.0], [0.0, 3.0, -3.0], [0.0, 0.0, 3.0]],
                    [[0.0, 3.0, -3.0], [0.0, 3.0, 3.0], [0.0, 0.0, 3.0]],
                ],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let impact = between(
            &walking_body,
            Vec3::new(-0.2, -0.02, 0.0),
            &triangles,
            Vec3::ZERO,
        )
        .expect("wall and floor overlap");
        assert!(
            impact.normal.y > 0.7 && impact.depth < 0.03,
            "a wall must not override shallow ship deck support: {impact:?}"
        );
    }
}

#[cfg(test)]
mod swept_wall_regressions {
    use super::*;
    #[test]
    fn crossing_a_thin_mesh_wall_is_blocked_even_if_end_position_has_no_overlap() {
        let character = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Capsule {
                start: [0.0, 0.4, 0.0],
                end: [0.0, 1.4, 0.0],
                radius: 0.4,
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let wall = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Triangles {
                vertices: vec![
                    [[0.0, -1.0, -3.0], [0.0, 3.0, -3.0], [0.0, -1.0, 3.0]],
                    [[0.0, 3.0, -3.0], [0.0, 3.0, 3.0], [0.0, -1.0, 3.0]],
                ],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let from = Vec3::new(-2.0, 0.0, -1.0);
        let to = Vec3::new(2.0, 0.0, 1.0);
        assert!(between(&character, to, &wall, Vec3::ZERO).is_none());
        let (stopped, impact) = sweep_character_wall(
            &character,
            from,
            to,
            Some(&wall),
            Vec3::ZERO,
            Vec3::new(-0.001, -1.0, -3.0),
            Vec3::new(0.001, 3.0, 3.0),
         45.0)
        .expect("wall should intercept swept Character");
        assert!(
            stopped.x <= -0.3999,
            "Character tunneled through an authored wall: {stopped:?}"
        );
        assert!(
            stopped.z > -0.8,
            "Character must retain tangential slide motion: {stopped:?}"
        );
        assert!(impact.normal.x < -0.8);
    }
}

#[cfg(test)]
mod spherical_collision_regressions {
    use super::*;
    fn instance(shape: CollisionShape) -> ColliderInstance {
        ColliderInstance {
            shapes: Arc::new(vec![shape]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }
    fn ball() -> ColliderInstance {
        instance(CollisionShape::Sphere {
            center: [0.0, 0.0, 0.0],
            radius: 0.5,
        })
    }
    #[test]
    fn sphere_fits_high_tessellation_round_source() {
        let mut vertices = Vec::new();
        for j in 0..320 {
            let theta = std::f32::consts::PI * ((j as f32 + 0.5) / 320.0);
            for i in 0..320 {
                let phi = std::f32::consts::TAU * (i as f32 / 320.0);
                vertices.push(Vec3::new(
                    theta.sin() * phi.cos() * 0.5,
                    theta.cos() * 0.5,
                    theta.sin() * phi.sin() * 0.5,
                ));
            }
        }
        let shapes = package_shapes(&vertices, Vec3::splat(-0.5), Vec3::splat(0.5));
        assert!(matches!(shapes.as_slice(), [CollisionShape::Sphere { .. }]));
    }
    #[test]
    fn sheet_is_not_fitted_as_solid_sphere() {
        let points = vec![
            Vec3::new(-0.5, 0.0, -0.5),
            Vec3::new(0.5, 0.0, -0.5),
            Vec3::new(0.0, 0.0, 0.5),
        ];
        assert!(
            package_shapes(
                &points,
                Vec3::new(-0.5, 0.0, -0.5),
                Vec3::new(0.5, 0.0, 0.5)
            )
            .is_empty()
        );
    }
    #[test]
    fn sphere_has_exact_floor_and_box_contacts() {
        let s = ball();
        let box_c = instance(CollisionShape::Box {
            min: [-1.0, -1.0, -1.0],
            max: [1.0, 0.0, 1.0],
        });
        let hit = between(&s, Vec3::new(0.2, 0.4, 0.1), &box_c, Vec3::ZERO)
            .expect("sphere touches floor");
        assert!(hit.normal.y > 0.99);
        assert!((hit.depth - 0.1).abs() < 0.00001);
        assert!(between(&s, Vec3::new(1.6, 0.6, 1.6), &box_c, Vec3::ZERO).is_none());
    }
    #[test]
    fn sphere_sphere_and_capsule_have_normalized_contacts() {
        let s = ball();
        let h = between(&s, Vec3::ZERO, &s, Vec3::new(0.8, 0.0, 0.0)).unwrap();
        assert!(h.normal.x < -0.99 && (h.depth - 0.2).abs() < 1e-5);
        let cap = instance(CollisionShape::Capsule {
            start: [0.0, -1.0, 0.0],
            end: [0.0, 1.0, 0.0],
            radius: 0.25,
        });
        let h = between(&s, Vec3::new(0.7, 0.0, 0.0), &cap, Vec3::ZERO).unwrap();
        assert!(h.normal.x > 0.99 && h.depth > 0.04);
        assert!(between(&s, Vec3::new(2.0, 0.0, 0.0), &cap, Vec3::ZERO).is_none());
    }
    #[test]
    fn sphere_triangle_contact_uses_surface_not_envelope() {
        let surface = instance(CollisionShape::Triangles {
            vertices: vec![[[-2.0, 0.0, -2.0], [2.0, 0.0, -2.0], [0.0, 0.0, 2.0]]],
        });
        assert!(between(&ball(), Vec3::new(0.0, 0.2, 0.0), &surface, Vec3::ZERO).is_some());
        assert!(between(&ball(), Vec3::new(1.9, 0.2, 1.9), &surface, Vec3::ZERO).is_none());
    }
    #[test]
    fn authored_no_collision_remains_disabled() {
        let mut actor = crate::world::Actor::new(81, "disabled", Vec3::ZERO);
        actor.physics.collision_mode = CollisionMode::Disabled;
        actor.physics.collision_mode_explicit = true;
        assert_eq!(
            resolved_actor_collision_mode(&actor),
            CollisionMode::Disabled
        );
    }
}

#[cfg(test)]
mod configurable_mesh_slope_tests {
    use super::*;

    #[test]
    fn imported_triangle_support_honors_project_slope_limit() {
        let player = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-0.2, 0.0, -0.2],
                max: [0.2, 1.8, 0.2],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let ramp = ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Triangles {
                vertices: vec![[[0.0, 0.0, 0.0], [0.0, 0.0, 2.0], [2.0, 1.1547, 0.0]]],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        };
        let at = Vec3::new(0.5, 0.25, 0.5);
        assert!(walkable_mesh_support(&player, at + Vec3::Y * 0.10, at, Some(&ramp), Vec3::ZERO, 15.0).is_none());
        assert!(walkable_mesh_support(&player, at + Vec3::Y * 0.10, at, Some(&ramp), Vec3::ZERO, 40.0).is_some());
    }
}
