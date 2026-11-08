// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use egui::{Color32, Pos2, Rect, Shape, Ui};
use glam::{Mat3, Quat, Vec3};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

const MAX_CACHED: usize = 112;

struct Face {
    points: [Vec3; 3],
    rgb: [u8; 3],
}
struct Model {
    faces: Vec<Face>,
    center: Vec3,
    orientation: Mat3,
    projections: Mutex<VecDeque<(i16, Arc<Projection>)>>,
}
struct Projection {
    mesh: egui::Mesh,
    min: glam::Vec2,
    max: glam::Vec2,
}

enum ResultState {
    Loading,
    Ready(Arc<Model>),
    Failed,
}
struct Cache {
    models: HashMap<PathBuf, ResultState>,
    order: VecDeque<PathBuf>,
    sender: mpsc::SyncSender<(PathBuf, egui::Context)>,
}
static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| {
        let (sender, receiver) = mpsc::sync_channel::<(PathBuf, egui::Context)>(48);
        std::thread::spawn(move || {
            while let Ok((path, context)) = receiver.recv() {
                let loaded = read_model(&path);
                if let Some(cache) = CACHE.get() {
                    if let Ok(mut guard) = cache.lock() {
                        if guard.models.contains_key(&path) {
                            guard.models.insert(
                                path,
                                loaded.map_or(ResultState::Failed, |mesh| {
                                    ResultState::Ready(Arc::new(mesh))
                                }),
                            );
                        }
                    }
                }
                context.request_repaint();
            }
        });
        Mutex::new(Cache {
            models: HashMap::new(),
            order: VecDeque::new(),
            sender,
        })
    })
}

pub(crate) fn invalidate_project(project: &Path) {
    if let Some(cache) = CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.models.retain(|path, _| !path.starts_with(project));
            guard.order.retain(|path| !path.starts_with(project));
        }
    }
}

fn visible_model(ui: &Ui, package: &Path) -> Option<Arc<Model>> {
    let mut guard = cache().lock().ok()?;
    match guard.models.get(package) {
        Some(ResultState::Ready(model)) => return Some(model.clone()),
        Some(ResultState::Loading | ResultState::Failed) => return None,
        None => {}
    }
    let task = (package.to_owned(), ui.ctx().clone());
    if guard.sender.try_send(task).is_err() {
        return None;
    }
    if guard.order.len() >= MAX_CACHED {
        if let Some(old) = guard.order.pop_front() {
            guard.models.remove(&old);
        }
    }
    guard.order.push_back(package.to_owned());
    guard
        .models
        .insert(package.to_owned(), ResultState::Loading);
    None
}

pub(crate) fn gltf_atlas_texel(image: &image::RgbaImage, uv: glam::Vec2) -> image::Rgba<u8> {
    let x = ((uv.x.rem_euclid(1.0) * image.width() as f32) as u32).min(image.width() - 1);
    let y = ((uv.y.rem_euclid(1.0) * image.height() as f32) as u32).min(image.height() - 1);
    *image.get_pixel(x, y)
}

pub(crate) fn orientation_for_mesh_bounds(min: Vec3, max: Vec3) -> Mat3 {
    let extent = max - min;
    if !extent.is_finite() || extent.min_element() < 0.0 {
        return Mat3::IDENTITY;
    }
    if extent.z > extent.y * 1.35 && extent.z > extent.x * 1.25 {
        let direction = if max.z.abs() >= min.z.abs() {
            Vec3::Z
        } else {
            -Vec3::Z
        };
        return Mat3::from_quat(Quat::from_rotation_arc(direction, Vec3::Y));
    }
    if extent.x > extent.y * 1.35 && extent.x > extent.z * 1.25 {
        let direction = if max.x.abs() >= min.x.abs() {
            Vec3::X
        } else {
            -Vec3::X
        };
        return Mat3::from_quat(Quat::from_rotation_arc(direction, Vec3::Y));
    }
    Mat3::IDENTITY
}

fn upright_preview_orientation(dir: &Path) -> Mat3 {
    let Ok(package) = crate::character::package::load_package(dir) else {
        return Mat3::IDENTITY;
    };
    orientation_for_skeleton(&package.skeleton)
}
pub(crate) fn orientation_for_skeleton(skeleton: &crate::character::rig::Skeleton) -> Mat3 {
    let rest = skeleton
        .joints
        .iter()
        .map(|joint| joint.local_transform)
        .collect::<Vec<_>>();
    let global = skeleton.evaluate_pose(&rest);
    let find = |patterns: &[&str]| {
        skeleton
            .joints
            .iter()
            .enumerate()
            .find(|(_, joint)| {
                let name = joint.name.to_ascii_lowercase();
                patterns.iter().any(|label| name.contains(label))
            })
            .map(|(index, _)| global.matrices[index].transform_point3(Vec3::ZERO))
    };
    let Some(head) = find(&["head"]) else {
        return Mat3::IDENTITY;
    };
    let Some(torso) = find(&["pelvis", "hips", "hip_", "spine"]) else {
        return Mat3::IDENTITY;
    };
    let direction = (head - torso).normalize_or_zero();
    if !direction.is_finite() || direction.length_squared() < 0.5 || direction.dot(Vec3::Y) > 0.5 {
        return Mat3::IDENTITY;
    }
    Mat3::from_quat(Quat::from_rotation_arc(direction, Vec3::Y))
}

fn read_model(dir: &Path) -> Option<Model> {
    if let Ok(bytes) = std::fs::read(dir.join("package.json"))
        && let Ok(asset) =
            serde_json::from_slice::<crate::project::character_asset::CharacterAsset>(&bytes)
        && !asset.source_actor.is_empty()
    {
        let project = dir.parent()?.parent()?.parent()?;
        let source = crate::project::asset_registry::AssetRegistry::actor_directory(
            project,
            &asset.source_actor,
        )
        .ok()?;
        return read_model(&source);
    }
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("import.json")).ok()?).ok()?;
    if let Some(meshes) = json.get("meshes").and_then(serde_json::Value::as_array) {
        let mut faces = Vec::new();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let names = meshes
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        let selected = names
            .into_iter()
            .filter_map(|name| {
                crate::project::load_skinned_glb_mesh(&dir.join("source.glb"), name).ok()
            })
            .collect::<Vec<_>>();
        let mut texture_cache = HashMap::<usize, Option<image::RgbaImage>>::new();
        for mesh in &selected {
            for prim in &mesh.primitives {
                let texture = prim.base_color_texture_index.and_then(|i| {
                    texture_cache
                        .entry(i)
                        .or_insert_with(|| {
                            image::open(dir.join("textures").join(format!("image_{i}.png")))
                                .ok()
                                .map(|image| image.to_rgba8())
                        })
                        .as_ref()
                });
                for tri in prim.vertices.chunks_exact(3) {
                    {
                        let mut color = [0u8; 3];
                        let uv = tri.iter().map(|v| v.tex_coord).sum::<glam::Vec2>() / 3.0;
                        let texel = texture.map(|img| gltf_atlas_texel(img, uv));
                        for k in 0..3 {
                            let base = tri.iter().map(|v| v.color[k]).sum::<f32>() / 3.0;
                            color[k] = (base.clamp(0.0, 1.0)
                                * texel.map_or(255.0, |rgba| rgba[k] as f32))
                            .clamp(0.0, 255.0) as u8;
                        }
                        let points = [tri[0].position, tri[1].position, tri[2].position];
                        for point in points {
                            min = min.min(point);
                            max = max.max(point);
                        }
                        faces.push(Face { points, rgb: color });
                    }
                }
            }
        }
        if faces.is_empty() {
            return None;
        }
        let model = Model {
            faces,
            center: (min + max) * 0.5,
            orientation: orientation_for_mesh_bounds(min, max),
            projections: Mutex::new(VecDeque::new()),
        };
        let _ = cached_projection(&model, 0.65);
        return Some(model);
    }
    let name = json.get("source_mesh_name")?.as_str()?;
    let index = json
        .get("source_mesh_index")
        .or_else(|| json.get("mesh_index"))
        .and_then(serde_json::Value::as_u64)
        .map(|i| i as usize);
    let mesh =
        crate::project::load_static_glb_mesh_at(&dir.join("source.glb"), name, index).ok()?;
    let mut faces = Vec::new();
    let mut texture_cache = HashMap::<usize, Option<image::RgbaImage>>::new();
    for primitive in &mesh.primitives {
        let texture = primitive.base_color_texture_index.and_then(|index| {
            texture_cache
                .entry(index)
                .or_insert_with(|| {
                    image::open(dir.join("textures").join(format!("image_{index}.png")))
                        .ok()
                        .map(|image| image.to_rgba8())
                })
                .as_ref()
        });
        for triangle in primitive.vertices.chunks_exact(3) {
            {
                let mut rgb = [0u8; 3];
                let avg = triangle
                    .iter()
                    .map(|v| v.color)
                    .fold([0.0f32; 3], |mut a, c| {
                        for k in 0..3 {
                            a[k] += c[k] / 3.0;
                        }
                        a
                    });
                let texel = texture.map(|img| {
                    let uv = triangle.iter().map(|v| v.tex_coord).sum::<glam::Vec2>() / 3.0;
                    gltf_atlas_texel(img, uv)
                });
                for k in 0..3 {
                    let base = avg[k].clamp(0.0, 1.0);
                    rgb[k] =
                        (base * texel.map_or(255.0, |rgba| rgba[k] as f32)).clamp(0.0, 255.0) as u8;
                }
                faces.push(Face {
                    points: [
                        triangle[0].position,
                        triangle[1].position,
                        triangle[2].position,
                    ],
                    rgb,
                });
            }
        }
    }
    if faces.is_empty() {
        return None;
    }
    let model = Model {
        faces,
        center: (mesh.bounds_min + mesh.bounds_max) * 0.5,
        orientation: Mat3::IDENTITY,
        projections: Mutex::new(VecDeque::new()),
    };
    let _ = cached_projection(&model, 0.65);
    Some(model)
}

fn prepare_projection(model: &Model, yaw: f32) -> Projection {
    let rotation = Mat3::from_rotation_x(-0.32) * Mat3::from_rotation_y(yaw);
    let mut triangles = Vec::with_capacity(model.faces.len());
    let mut min = glam::Vec2::splat(f32::INFINITY);
    let mut max = glam::Vec2::splat(f32::NEG_INFINITY);
    let light = Vec3::new(0.4, 0.8, 0.5).normalize();
    for face in &model.faces {
        let p = face
            .points
            .map(|p| rotation * model.orientation * (p - model.center));
        for corner in &p {
            min = min.min(corner.truncate());
            max = max.max(corner.truncate());
        }
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        if n.length_squared() < f32::EPSILON {
            continue;
        }
        let shade = 0.53 + 0.42 * n.dot(light).abs();
        let color = Color32::from_rgb(
            (face.rgb[0] as f32 * shade).min(255.0) as u8,
            (face.rgb[1] as f32 * shade).min(255.0) as u8,
            (face.rgb[2] as f32 * shade).min(255.0) as u8,
        );
        triangles.push((p, color));
    }
    triangles.sort_by(|(a, _), (b, _)| {
        let az = a.iter().map(|p| p.z).sum::<f32>();
        let bz = b.iter().map(|p| p.z).sum::<f32>();
        az.total_cmp(&bz)
    });
    let mut mesh = egui::Mesh::default();
    for (triangle, color) in triangles {
        let start = mesh.vertices.len() as u32;
        for corner in triangle {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: Pos2::new(corner.x, corner.y),
                uv: egui::epaint::WHITE_UV,
                color,
            });
        }
        mesh.add_triangle(start, start + 1, start + 2);
    }
    Projection { mesh, min, max }
}

fn cached_projection(model: &Model, yaw: f32) -> Option<Arc<Projection>> {
    let angular_resolution = if model.faces.len() > 2000 { 6.0 } else { 10.0 };
    let bucket = (yaw * angular_resolution).round() as i16;
    let mut cached = model.projections.lock().ok()?;
    if let Some((_, geometry)) = cached.iter().find(|(angle, _)| *angle == bucket) {
        return Some(geometry.clone());
    }
    let geometry = Arc::new(prepare_projection(
        model,
        bucket as f32 / angular_resolution,
    ));
    let max_cached_angles = if model.faces.len() > 2000 { 4 } else { 12 };
    if cached.len() >= max_cached_angles {
        cached.pop_front();
    }
    cached.push_back((bucket, geometry.clone()));
    Some(geometry)
}

pub fn paint_model(ui: &Ui, bounds: Rect, package: &Path) -> bool {
    if bounds.width() < 12.0 || bounds.height() < 12.0 || !ui.is_rect_visible(bounds) {
        return false;
    }
    let Some(model) = visible_model(ui, package) else {
        return false;
    };
    let yaw = if ui.rect_contains_pointer(bounds) {
        ui.input(|i| i.pointer.hover_pos())
            .map(|pointer| {
                let offset = (pointer.x - bounds.center().x) / bounds.width().max(1.0);
                0.65 + offset.clamp(-0.5, 0.5) * 1.5
            })
            .unwrap_or(0.65)
    } else {
        0.65
    };
    let Some(projection) = cached_projection(&model, yaw) else {
        return false;
    };
    if projection.mesh.vertices.is_empty() {
        return false;
    }
    let span = (projection.max - projection.min).max(glam::Vec2::splat(0.0001));
    let zoom = (bounds.width() * 0.78 / span.x).min(bounds.height() * 0.78 / span.y);
    let offset = (projection.min + projection.max) * 0.5;
    let center = bounds.center();
    let mut mesh = projection.mesh.clone();
    for vertex in &mut mesh.vertices {
        vertex.pos = Pos2::new(
            center.x + (vertex.pos.x - offset.x) * zoom,
            center.y - (vertex.pos.y - offset.y) * zoom,
        );
    }
    ui.painter().rect_filled(
        bounds,
        2.0,
        ui.visuals()
            .widgets
            .noninteractive
            .weak_bg_fill
            .gamma_multiply(0.48),
    );
    ui.painter().add(Shape::mesh(mesh));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wo_asm_001_gltf_atlas_sampling_keeps_upper_left_uv_origin() {
        let mut image = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(5, 6, image::Rgba([230, 144, 100, 255]));
        let sampled = gltf_atlas_texel(&image, glam::Vec2::new(0.68, 0.81));
        assert_eq!(sampled.0, [230, 144, 100, 255]);
        assert_eq!(
            gltf_atlas_texel(&image, glam::Vec2::new(1.68, -0.19)).0,
            sampled.0
        );
        assert_eq!(
            gltf_atlas_texel(&image, glam::Vec2::new(0.68, 0.18)).0,
            [0, 0, 0, 255]
        );
    }

    fn enlarged_glb_fixture(source: &Path, destination: &Path, repeats: usize) {
        let data = std::fs::read(source).unwrap();
        assert_eq!(&data[0..4], b"glTF");
        let original_json_size = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let mut document: serde_json::Value =
            serde_json::from_slice(&data[20..20 + original_json_size]).unwrap();
        let primitives = document["meshes"][0]["primitives"]
            .as_array()
            .unwrap()
            .clone();
        document["meshes"][0]["primitives"] = serde_json::Value::Array(
            (0..repeats)
                .flat_map(|_| primitives.iter().cloned())
                .collect(),
        );
        let mut json = serde_json::to_vec(&document).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let bin_chunk = &data[20 + original_json_size..];
        let total_length = 12 + 8 + json.len() + bin_chunk.len();
        let mut output = Vec::with_capacity(total_length);
        output.extend_from_slice(b"glTF");
        output.extend_from_slice(&2u32.to_le_bytes());
        output.extend_from_slice(&(total_length as u32).to_le_bytes());
        output.extend_from_slice(&(json.len() as u32).to_le_bytes());
        output.extend_from_slice(b"JSON");
        output.extend_from_slice(&json);
        output.extend_from_slice(bin_chunk);
        std::fs::write(destination, output).unwrap();
    }

    #[test]
    fn mesh_basis_handles_z_up_and_preserves_y_up_without_editing_geometry() {
        let z_up =
            orientation_for_mesh_bounds(Vec3::new(-1.0, -0.5, 0.0), Vec3::new(1.0, 0.5, 3.0));
        assert!((z_up * Vec3::Z - Vec3::Y).length() < 0.001);
        assert_eq!(
            orientation_for_mesh_bounds(Vec3::new(-0.5, 0.0, -0.4), Vec3::new(0.5, 3.0, 0.4)),
            Mat3::IDENTITY
        );
    }

    #[test]
    fn rig_preview_corrects_prone_bind_axis_without_mutating_skeleton() {
        use crate::character::rig::{Joint, Skeleton, Transform};
        use glam::Quat;
        let make = |translation: Vec3| {
            Skeleton::new(vec![
                Joint {
                    name: "Hips".into(),
                    parent_index: None,
                    local_transform: Transform {
                        translation: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                        scale: Vec3::ONE,
                    },
                },
                Joint {
                    name: "Head".into(),
                    parent_index: Some(0),
                    local_transform: Transform {
                        translation,
                        rotation: Quat::IDENTITY,
                        scale: Vec3::ONE,
                    },
                },
            ])
        };
        let prone = make(Vec3::Z * 2.0);
        let upright = orientation_for_skeleton(&prone);
        assert!((upright * Vec3::Z - Vec3::Y).length() < 0.001);
        let original = prone.joints[1].local_transform.translation;
        assert_eq!(original, Vec3::Z * 2.0);
        let standing = make(Vec3::Y * 2.0);
        assert_eq!(orientation_for_skeleton(&standing), Mat3::IDENTITY);
    }

    #[test]
    fn preview_is_non_destructive_for_missing_packages() {
        let ctx = egui::Context::default();
        ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(!paint_model(
                    ui,
                    Rect::from_min_size(Pos2::ZERO, egui::vec2(120.0, 100.0)),
                    Path::new("missing-preview-package")
                ));
            });
        });
    }
}
