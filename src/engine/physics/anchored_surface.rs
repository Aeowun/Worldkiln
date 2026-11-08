// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::Vec3;
use std::collections::{HashMap, HashSet, VecDeque};

const GRID: f32 = 10000.0;
type Key = (i64, i64, i64);
fn key(p: Vec3) -> Key {
    (
        (p.x * GRID).round() as i64,
        (p.y * GRID).round() as i64,
        (p.z * GRID).round() as i64,
    )
}
const STEP: [Vec3; 6] = [
    Vec3::NEG_X,
    Vec3::X,
    Vec3::NEG_Y,
    Vec3::Y,
    Vec3::NEG_Z,
    Vec3::Z,
];

#[derive(Clone, Copy, Debug)]
pub struct AnchoredFace {
    pub normal: Vec3,
    pub min: Vec3,
    pub max: Vec3,
    pub cell_id: u64,
}

#[derive(Debug)]
pub struct AnchoredCollider {
    pub min: Vec3,
    pub max: Vec3,
    pub faces: Vec<AnchoredFace>,
    pub cells: usize,
}

fn make_face(pos: Vec3, direction: usize, cell_id: u64) -> AnchoredFace {
    let (min, max) = match direction {
        0 => (pos, pos + Vec3::new(0.0, 1.0, 1.0)),
        1 => (pos + Vec3::X, pos + Vec3::new(1.0, 1.0, 1.0)),
        2 => (pos, pos + Vec3::new(1.0, 0.0, 1.0)),
        3 => (pos + Vec3::Y, pos + Vec3::new(1.0, 1.0, 1.0)),
        4 => (pos, pos + Vec3::new(1.0, 1.0, 0.0)),
        5 => (pos + Vec3::Z, pos + Vec3::new(1.0, 1.0, 1.0)),
        _ => unreachable!(),
    };
    AnchoredFace {
        normal: STEP[direction],
        min,
        max,
        cell_id,
    }
}
fn face_address(face: &AnchoredFace, direction: usize) -> (i64, i64, i64) {
    match direction {
        0 | 1 => (
            (face.min.x * GRID).round() as i64,
            (face.min.y * GRID).round() as i64,
            (face.min.z * GRID).round() as i64,
        ),
        2 | 3 => (
            (face.min.y * GRID).round() as i64,
            (face.min.x * GRID).round() as i64,
            (face.min.z * GRID).round() as i64,
        ),
        _ => (
            (face.min.z * GRID).round() as i64,
            (face.min.x * GRID).round() as i64,
            (face.min.y * GRID).round() as i64,
        ),
    }
}
fn set_rect(face: &mut AnchoredFace, dir: usize, width: f32, height: f32) {
    match dir {
        0 | 1 => {
            face.max.y = face.min.y + width;
            face.max.z = face.min.z + height;
        }
        2 | 3 => {
            face.max.x = face.min.x + width;
            face.max.z = face.min.z + height;
        }
        _ => {
            face.max.x = face.min.x + width;
            face.max.y = face.min.y + height;
        }
    }
}
fn merge_exposed_faces(faces: Vec<(usize, AnchoredFace)>) -> Vec<AnchoredFace> {
    let mut planes: HashMap<(usize, i64), HashMap<(i64, i64), AnchoredFace>> = HashMap::new();
    for (dir, face) in faces {
        let (plane, u, v) = face_address(&face, dir);
        planes.entry((dir, plane)).or_default().insert((u, v), face);
    }
    let mut result = Vec::new();
    for ((dir, _), mut tiles) in planes {
        while let Some((&(start_u, start_v), _)) = tiles.iter().next() {
            let mut w = 1i64;
            while tiles.contains_key(&(start_u + w * GRID as i64, start_v)) {
                w += 1;
            }
            let mut h = 1i64;
            'extend: loop {
                for u in 0..w {
                    if !tiles.contains_key(&(start_u + u * GRID as i64, start_v + h * GRID as i64))
                    {
                        break 'extend;
                    }
                }
                h += 1;
            }
            let mut merged = tiles.remove(&(start_u, start_v)).expect("face tile exists");
            for v in 0..h {
                for u in 0..w {
                    if u != 0 || v != 0 {
                        tiles.remove(&(start_u + u * GRID as i64, start_v + v * GRID as i64));
                    }
                }
            }
            set_rect(&mut merged, dir, w as f32, h as f32);
            result.push(merged);
        }
    }
    result
}
pub fn build_anchored_colliders(cells: &[(u64, Vec3)]) -> Vec<AnchoredCollider> {
    let mut spatial: HashMap<Key, Vec<usize>> = HashMap::new();
    for (i, &(_, position)) in cells.iter().enumerate() {
        if position.is_finite() {
            spatial.entry(key(position)).or_default().push(i);
        }
    }
    let neighbor = |index: usize, dir: usize| -> Option<usize> {
        let target = cells[index].1 + STEP[dir];
        spatial.get(&key(target)).and_then(|indices| {
            indices
                .iter()
                .copied()
                .find(|&i| (cells[i].1 - target).abs().max_element() < 0.0001)
        })
    };
    let mut visited = vec![false; cells.len()];
    let mut groups = Vec::new();
    for start in 0..cells.len() {
        if visited[start] || !cells[start].1.is_finite() {
            continue;
        }
        let mut pending = VecDeque::from([start]);
        visited[start] = true;
        let mut members = Vec::new();
        while let Some(i) = pending.pop_front() {
            members.push(i);
            for dir in 0..6 {
                if let Some(j) = neighbor(i, dir) {
                    if !visited[j] {
                        visited[j] = true;
                        pending.push_back(j);
                    }
                }
            }
        }
        let set: HashSet<usize> = members.iter().copied().collect();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut faces = Vec::new();
        for &i in &members {
            let (cell_id, pos) = cells[i];
            min = min.min(pos);
            max = max.max(pos + Vec3::ONE);
            for dir in 0..6 {
                if !neighbor(i, dir).is_some_and(|other| set.contains(&other)) {
                    faces.push((dir, make_face(pos, dir, cell_id)));
                }
            }
        }
        groups.push(AnchoredCollider {
            min,
            max,
            faces: merge_exposed_faces(faces),
            cells: members.len(),
        });
    }
    groups
}
