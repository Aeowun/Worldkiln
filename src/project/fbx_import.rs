// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
// FBX decoding is native (ufbx); generated GLB is an internal interchange format.
// This deliberately feeds the existing Worldkiln Actor/Character import and animation
// pipeline instead of introducing a second runtime mesh/skeleton representation.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use ufbx::{self, CoordinateAxis as Axis};

use super::gltf_import::{self, GlbAssetKind, GlbFolderImportResult};

const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 512 * 1024 * 1024;
const MAX_TRIANGLES: usize = 2_000_000;
const MAX_ANIMATION_SAMPLES: usize = 3600;
const MAX_TEXTURE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct FbxInspection {
    pub mesh_names: Vec<String>,
    pub material_count: usize,
    pub joint_names: Vec<String>,
    pub animation_names: Vec<String>,
    pub blend_shape_count: usize,
}

fn scene_from_file(path: &Path) -> Result<ufbx::SceneRoot, String> {
    if !path.is_file()
        || path.is_symlink()
        || !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("fbx"))
    {
        return Err("FBX input must be a regular .fbx file".into());
    }
    let size = path.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > MAX_SOURCE_BYTES {
        return Err("FBX source is empty or exceeds the 512 MiB import limit".into());
    }
    let mut opts = ufbx::LoadOpts::default();
    opts.target_axes = ufbx::CoordinateAxes {
        right: Axis::PositiveX,
        up: Axis::PositiveY,
        front: Axis::PositiveZ,
    };
    opts.target_unit_meters = 1.0;
    opts.space_conversion = ufbx::SpaceConversion::AdjustTransforms;
    opts.geometry_transform_handling = ufbx::GeometryTransformHandling::ModifyGeometry;
    opts.generate_missing_normals = true;
    opts.clean_skin_weights = true;
    opts.load_external_files = false;
    ufbx::load_file(path.to_str().ok_or("FBX path is not valid UTF-8")?, opts)
        .map_err(|e| format!("Cannot decode FBX '{}': {e:?}", path.display()))
}

pub fn inspect_fbx(path: &Path) -> Result<FbxInspection, String> {
    let scene = scene_from_file(path)?;
    let mut meshes = Vec::new();
    for node in &scene.nodes {
        if node.mesh.is_some() {
            meshes.push(node.element.name.to_string());
        }
    }
    if meshes.is_empty() {
        return Err("FBX contains no mesh nodes".into());
    }
    Ok(FbxInspection {
        mesh_names: meshes,
        material_count: scene.materials.len(),
        joint_names: scene
            .bones
            .iter()
            .map(|bone| bone.element.name.to_string())
            .collect(),
        animation_names: scene
            .anim_stacks
            .iter()
            .map(|stack| stack.element.name.to_string())
            .collect(),
        blend_shape_count: scene.blend_shapes.len(),
    })
}

fn f32_checked(value: f64) -> Result<f32, String> {
    let converted = value as f32;
    if !converted.is_finite() {
        return Err("FBX contains a non-finite or out-of-range numeric value".into());
    }
    Ok(converted)
}

fn vec3(v: ufbx::Vec3) -> Result<[f32; 3], String> {
    Ok([f32_checked(v.x)?, f32_checked(v.y)?, f32_checked(v.z)?])
}

fn quat(v: ufbx::Quat) -> Result<[f32; 4], String> {
    let mut q = [
        f32_checked(v.x)?,
        f32_checked(v.y)?,
        f32_checked(v.z)?,
        f32_checked(v.w)?,
    ];
    let sq = q.iter().map(|v| v * v).sum::<f32>();
    if sq <= 1.0e-15 {
        return Err("FBX has a zero-length joint rotation".into());
    }
    let inv = sq.sqrt().recip();
    for c in &mut q {
        *c *= inv;
    }
    Ok(q)
}

fn transform(t: &ufbx::Transform) -> Result<Value, String> {
    Ok(json!({
        "translation": vec3(t.translation)?,
        "rotation": quat(t.rotation)?,
        "scale": vec3(t.scale)?,
    }))
}

fn matrix(m: &ufbx::Matrix) -> Result<[f32; 16], String> {
    Ok([
        f32_checked(m.m00)?,
        f32_checked(m.m10)?,
        f32_checked(m.m20)?,
        0.0,
        f32_checked(m.m01)?,
        f32_checked(m.m11)?,
        f32_checked(m.m21)?,
        0.0,
        f32_checked(m.m02)?,
        f32_checked(m.m12)?,
        f32_checked(m.m22)?,
        0.0,
        f32_checked(m.m03)?,
        f32_checked(m.m13)?,
        f32_checked(m.m23)?,
        1.0,
    ])
}

#[derive(Default)]
struct GltfWriter {
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    images: Vec<Value>,
    textures: Vec<Value>,
    materials: Vec<Value>,
    meshes: Vec<Value>,
    nodes: Vec<Value>,
    skins: Vec<Value>,
    animations: Vec<Value>,
}

impl GltfWriter {
    fn append_view(&mut self, bytes: &[u8], target: Option<u32>) -> Result<usize, String> {
        while !self.binary.len().is_multiple_of(4) {
            self.binary.push(0);
        }
        let start = self.binary.len();
        if start
            .checked_add(bytes.len())
            .is_none_or(|sum| sum > MAX_OUTPUT_BYTES)
        {
            return Err("Converted FBX would exceed the 512 MiB GLB limit".into());
        }
        self.binary.extend_from_slice(bytes);
        let mut view = json!({"buffer": 0, "byteOffset": start, "byteLength": bytes.len()});
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        let index = self.views.len();
        self.views.push(view);
        Ok(index)
    }

    fn accessor_f32(&mut self, values: &[f32], kind: &str, bounds: bool) -> Result<usize, String> {
        let components = match kind {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT4" => 16,
            _ => return Err("Invalid FBX accessor type".into()),
        };
        if values.len() % components != 0 || values.iter().any(|v| !v.is_finite()) {
            return Err("Invalid FBX attribute buffer".into());
        }
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let view = self.append_view(&bytes, Some(34962))?;
        let mut accessor = json!({
            "bufferView": view, "componentType": 5126,
            "count": values.len() / components, "type": kind
        });
        if bounds && !values.is_empty() {
            let mut minimum = vec![f32::INFINITY; components];
            let mut maximum = vec![f32::NEG_INFINITY; components];
            for vertex in values.chunks_exact(components) {
                for i in 0..components {
                    minimum[i] = minimum[i].min(vertex[i]);
                    maximum[i] = maximum[i].max(vertex[i]);
                }
            }
            accessor["min"] = json!(minimum);
            accessor["max"] = json!(maximum);
        }
        let index = self.accessors.len();
        self.accessors.push(accessor);
        Ok(index)
    }

    fn accessor_u16(&mut self, values: &[u16], kind: &str) -> Result<usize, String> {
        let components = if kind == "VEC4" {
            4
        } else {
            return Err("Invalid u16 attribute".into());
        };
        if values.len() % components != 0 {
            return Err("Unaligned joint values".into());
        }
        let mut bytes = Vec::with_capacity(values.len() * 2);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let view = self.append_view(&bytes, Some(34962))?;
        let index = self.accessors.len();
        self.accessors.push(json!({
            "bufferView": view, "componentType": 5123,
            "count": values.len() / components, "type": kind
        }));
        Ok(index)
    }

    fn encode(mut self) -> Result<Vec<u8>, String> {
        while !self.binary.len().is_multiple_of(4) {
            self.binary.push(0);
        }
        let document = json!({
            "asset": {"version": "2.0", "generator": "Worldkiln FBX importer / ufbx"},
            "scene": 0,
            "scenes": [{"nodes": (0..self.nodes.len())
                .filter(|i| self.nodes[*i].get("extras").and_then(|v|v.get("isRoot")).and_then(Value::as_bool).unwrap_or(false))
                .collect::<Vec<_>>() }],
            "nodes": self.nodes.iter_mut().map(|v| {
                if let Some(obj) = v.as_object_mut() { obj.remove("extras"); }
                v.clone()
            }).collect::<Vec<_>>(),
            "meshes": self.meshes,
            "buffers": [{"byteLength": self.binary.len()}],
            "bufferViews": self.views,
            "accessors": self.accessors,
            "materials": self.materials,
            "textures": self.textures,
            "images": self.images,
            "skins": self.skins,
            "animations": self.animations,
        });
        let mut json_bytes = serde_json::to_vec(&document).map_err(|e| e.to_string())?;
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let full_length = 12 + 8 + json_bytes.len() + 8 + self.binary.len();
        let total = u32::try_from(full_length).map_err(|_| "Converted GLB is too large")?;
        let json_len = u32::try_from(json_bytes.len()).map_err(|_| "GLB JSON is too large")?;
        let bin_len = u32::try_from(self.binary.len()).map_err(|_| "GLB binary is too large")?;
        let mut bytes = Vec::with_capacity(full_length);
        bytes.extend_from_slice(b"glTF");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&total.to_le_bytes());
        bytes.extend_from_slice(&json_len.to_le_bytes());
        bytes.extend_from_slice(&0x4e4f534au32.to_le_bytes());
        bytes.extend_from_slice(&json_bytes);
        bytes.extend_from_slice(&bin_len.to_le_bytes());
        bytes.extend_from_slice(&0x004e4942u32.to_le_bytes());
        bytes.extend_from_slice(&self.binary);
        Ok(bytes)
    }
}

fn image_from_texture(source: &Path, texture: &ufbx::Texture) -> Option<Vec<u8>> {
    let embedded = &texture.content;
    let bytes = if !embedded.is_empty() && embedded.len() <= MAX_TEXTURE_BYTES {
        Some(embedded.to_vec())
    } else {
        // FBX exporters often include stale absolute workstation paths. Never follow
        // them; resolve only names within the source folder or its pack parent.
        let stem = [&*texture.relative_filename, &*texture.filename]
            .into_iter()
            .filter_map(|s| s.replace('\\', "/").rsplit('/').next().map(str::to_owned))
            .find(|s| !s.is_empty() && !s.contains(".."))?;
        let root = source.parent()?;
        let mut candidates = Vec::new();
        for parent in [
            Some(root),
            root.parent(),
            root.parent().and_then(Path::parent),
        ]
        .into_iter()
        .flatten()
        {
            candidates.push(parent.join(&stem));
            candidates.push(parent.join("textures").join(&stem));
            candidates.push(parent.join("Textures").join(&stem));
            candidates.push(parent.join("Textures_2k").join(&stem));
            candidates.push(parent.join("Textures_1k").join(&stem));
        }
        candidates.into_iter().find_map(|path| {
            let meta = fs::symlink_metadata(&path).ok()?;
            if !meta.file_type().is_file() || meta.len() as usize > MAX_TEXTURE_BYTES {
                return None;
            }
            fs::read(&path).ok()
        })
    }?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    if decoded.width() > 8192 || decoded.height() > 8192 {
        return None;
    }
    let mut png = Cursor::new(Vec::new());
    decoded.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

fn register_materials(
    writer: &mut GltfWriter,
    scene: &ufbx::Scene,
    source: &Path,
    selected: Option<u32>,
) -> Result<HashMap<u32, usize>, String> {
    let mut used = HashSet::<u32>::new();
    for node in &scene.nodes {
        if selected.is_some_and(|id| id != node.element.typed_id) {
            continue;
        }
        let Some(mesh) = node.mesh.as_ref() else {
            continue;
        };
        for &index in &mesh.face_material {
            if let Some(material) = node
                .materials
                .get(index as usize)
                .or_else(|| mesh.materials.get(index as usize))
            {
                used.insert(material.element.typed_id);
            }
        }
    }
    let mut material_indices = HashMap::new();
    let mut image_lookup = HashMap::<u32, usize>::new();
    for material in &scene.materials {
        if !used.contains(&material.element.typed_id) {
            continue;
        }
        material_indices.insert(material.element.typed_id, writer.materials.len());
        let pbr = &material.pbr;
        let fbx_color = &material.fbx.diffuse_color;
        let color = if pbr.base_color.has_value {
            &pbr.base_color.value_vec4
        } else {
            &fbx_color.value_vec4
        };
        let base = [
            f32_checked(color.x)?,
            f32_checked(color.y)?,
            f32_checked(color.z)?,
            1.0f32,
        ];
        let mut pbr_value = json!({
            "baseColorFactor": base,
            "metallicFactor": if pbr.metalness.has_value {
                f32_checked(pbr.metalness.value_vec4.x)?.clamp(0.0,1.0)
            } else {0.0},
            "roughnessFactor": if pbr.roughness.has_value {
                f32_checked(pbr.roughness.value_vec4.x)?.clamp(0.0,1.0)
            } else {1.0}
        });
        let texture = pbr
            .base_color
            .texture
            .as_ref()
            .or(fbx_color.texture.as_ref());
        if let Some(texture) = texture {
            let texture_id = texture.element.typed_id;
            let texture_index = if let Some(index) = image_lookup.get(&texture_id) {
                Some(*index)
            } else if let Some(data) = image_from_texture(source, texture) {
                let view = writer.append_view(&data, None)?;
                let image_index = writer.images.len();
                writer.images.push(json!({
                    "bufferView":view,"mimeType":"image/png"
                }));
                let index = writer.textures.len();
                writer.textures.push(json!({"source": image_index}));
                image_lookup.insert(texture_id, index);
                Some(index)
            } else {
                None
            };
            if let Some(index) = texture_index {
                pbr_value["baseColorTexture"] = json!({"index":index});
            }
        }
        writer.materials.push(json!({
            "name": material.element.name.to_string(),
            "pbrMetallicRoughness": pbr_value,
            "doubleSided":true
        }));
    }
    Ok(material_indices)
}

fn triangulate(mesh: &ufbx::Mesh, face: ufbx::Face) -> Vec<u32> {
    let mut vertices = Vec::new();
    ufbx::triangulate_face_vec(&mut vertices, mesh, face);
    vertices
}

fn vertex_skin(
    mesh: &ufbx::Mesh,
    control: usize,
    joint_map: &HashMap<u32, u16>,
) -> Result<([u16; 4], [f32; 4]), String> {
    let Some(skin) = mesh.skin_deformers.first() else {
        return Ok(([0; 4], [1.0, 0.0, 0.0, 0.0]));
    };
    let info = skin
        .vertices
        .get(control)
        .ok_or("FBX skin has missing control point")?;
    let begin = info.weight_begin as usize;
    let end = begin
        .checked_add(info.num_weights as usize)
        .ok_or("Invalid skin range")?;
    let mut influences = Vec::new();
    for weight in skin
        .weights
        .get(begin..end)
        .ok_or("Skin weight range is out of bounds")?
    {
        let cluster = skin
            .clusters
            .get(weight.cluster_index as usize)
            .ok_or("FBX skin refers to missing cluster")?;
        let Some(bone) = cluster.bone_node.as_ref() else {
            continue;
        };
        let Some(&joint) = joint_map.get(&bone.element.typed_id) else {
            continue;
        };
        let amount = f32_checked(weight.weight)?;
        if amount > 0.0 {
            influences.push((joint, amount));
        }
    }
    influences.sort_by(|a, b| b.1.total_cmp(&a.1));
    influences.truncate(4);
    let sum: f32 = influences.iter().map(|x| x.1).sum();
    let mut joints = [0; 4];
    let mut weights = [0.0; 4];
    if sum < 1e-7 {
        weights[0] = 1.0;
        return Ok((joints, weights));
    }
    for (i, (joint, weight)) in influences.iter().enumerate() {
        joints[i] = *joint;
        weights[i] = *weight / sum;
    }
    Ok((joints, weights))
}

fn append_mesh(
    writer: &mut GltfWriter,
    node: &ufbx::Node,
    joint_map: &HashMap<u32, u16>,
    name: &str,
) -> Result<usize, String> {
    let mesh = node.mesh.as_ref().ok_or("FBX node has no mesh")?;
    let has_skin = !mesh.skin_deformers.is_empty();
    let mut groups =
        BTreeMap::<usize, (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>, Vec<u16>, Vec<f32>)>::new();
    let mut triangle_count = 0usize;
    for (face_index, face) in mesh.faces.iter().enumerate() {
        if face.num_indices < 3 {
            continue;
        }
        let material_index = mesh.face_material.get(face_index).copied().unwrap_or(0) as usize;
        let parts = groups.entry(material_index).or_default();
        for vertex_index in triangulate(mesh, *face) {
            triangle_count += 1;
            if triangle_count > MAX_TRIANGLES * 3 {
                return Err("FBX mesh exceeds the safe triangle limit".into());
            }
            let idx = vertex_index as usize;
            let position = mesh
                .vertex_position
                .indices
                .get(idx)
                .map(|_| ufbx::get_vertex_vec3(&mesh.vertex_position, idx))
                .ok_or("FBX polygon has invalid position index")?;
            let normal = if mesh.vertex_normal.exists {
                ufbx::get_vertex_vec3(&mesh.vertex_normal, idx)
            } else {
                ufbx::Vec3 {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                }
            };
            let uv = if mesh.vertex_uv.exists {
                ufbx::get_vertex_vec2(&mesh.vertex_uv, idx)
            } else {
                ufbx::Vec2 { x: 0.0, y: 0.0 }
            };
            parts.0.extend_from_slice(&vec3(position)?);
            parts.1.extend_from_slice(&vec3(normal)?);
            parts
                .2
                .extend_from_slice(&[f32_checked(uv.x)?, 1.0 - f32_checked(uv.y)?]);
            if mesh.vertex_color.exists {
                let color = ufbx::get_vertex_vec4(&mesh.vertex_color, idx);
                parts.3.extend_from_slice(&[
                    f32_checked(color.x)?,
                    f32_checked(color.y)?,
                    f32_checked(color.z)?,
                    f32_checked(color.w)?,
                ]);
            } else {
                parts.3.extend_from_slice(&[1.0, 1.0, 1.0, 1.0]);
            }
            if has_skin {
                let control = *mesh
                    .vertex_indices
                    .get(idx)
                    .ok_or("Missing FBX vertex index")? as usize;
                let (joints, weights) = vertex_skin(mesh, control, joint_map)?;
                parts.4.extend_from_slice(&joints);
                parts.5.extend_from_slice(&weights);
            }
        }
    }
    if groups.is_empty() {
        return Err(format!("FBX mesh '{name}' contains no triangles"));
    }
    let mut primitives = Vec::new();
    for (mat, (positions, normals, uvs, colors, joints, weights)) in groups {
        let mut attrs = json!({
            "POSITION":writer.accessor_f32(&positions,"VEC3",true)?,
            "NORMAL":writer.accessor_f32(&normals,"VEC3",false)?,
            "TEXCOORD_0":writer.accessor_f32(&uvs,"VEC2",false)?,
            "COLOR_0":writer.accessor_f32(&colors,"VEC4",false)?,
        });
        if has_skin {
            attrs["JOINTS_0"] = json!(writer.accessor_u16(&joints, "VEC4")?);
            attrs["WEIGHTS_0"] = json!(writer.accessor_f32(&weights, "VEC4", false)?);
        }
        let mut primitive = json!({"attributes":attrs,"mode":4});
        if let Some(material) = node.materials.get(mat).or_else(|| mesh.materials.get(mat)) {
            primitive["material"] = json!(material.element.typed_id);
        }
        primitives.push(primitive);
    }
    let index = writer.meshes.len();
    writer
        .meshes
        .push(json!({"name":name,"primitives":primitives}));
    Ok(index)
}

fn build_document(
    scene: &ufbx::Scene,
    source: &Path,
    selected: Option<u32>,
) -> Result<Vec<u8>, String> {
    let mut writer = GltfWriter::default();
    let material_indices = register_materials(&mut writer, scene, source, selected)?;
    let mut node_ids = HashMap::new();
    for (index, node) in scene.nodes.iter().enumerate() {
        node_ids.insert(node.element.typed_id, index);
    }
    let mut mesh_names = HashSet::new();
    let mut skinned_nodes = Vec::new();
    for node in &scene.nodes {
        let mut item = transform(&node.local_transform)?;
        let parent = node.parent.as_ref().map(|p| p.element.typed_id);
        item["name"] = json!(if node.element.name.is_empty() {
            format!("Node_{}", node.element.typed_id)
        } else {
            node.element.name.to_string()
        });
        item["extras"] =
            json!({"isRoot":parent.is_none() || parent.is_some_and(|p|!node_ids.contains_key(&p))});
        if let Some(mesh) = node.mesh.as_ref() {
            if selected.is_none() || selected == Some(node.element.typed_id) {
                let base = if node.element.name.is_empty() {
                    format!("Mesh_{}", node.element.typed_id)
                } else {
                    node.element.name.to_string()
                };
                let mut name = base.clone();
                let mut suffix = 2;
                while !mesh_names.insert(name.to_ascii_lowercase()) {
                    name = format!("{base}_{suffix}");
                    suffix += 1;
                }
                if !mesh.skin_deformers.is_empty() {
                    skinned_nodes.push(node.element.typed_id);
                }
                // Meshes are appended after the shared joint map is available.
                item["mesh"] = json!(null);
                item["name"] = json!(name);
            }
        }
        writer.nodes.push(item);
    }
    for (index, node) in scene.nodes.iter().enumerate() {
        let children = node
            .children
            .iter()
            .filter_map(|child| node_ids.get(&child.element.typed_id).copied())
            .collect::<Vec<_>>();
        if !children.is_empty() {
            writer.nodes[index]["children"] = json!(children);
        }
    }
    let mut joint_map = HashMap::<u32, u16>::new();
    let mut joint_nodes = Vec::new();
    let mut matrices = Vec::new();
    if !skinned_nodes.is_empty() {
        for node in &scene.nodes {
            if !skinned_nodes.contains(&node.element.typed_id) {
                continue;
            }
            let Some(mesh) = node.mesh.as_ref() else {
                continue;
            };
            for skin in &mesh.skin_deformers {
                for cluster in &skin.clusters {
                    let Some(bone) = cluster.bone_node.as_ref() else {
                        continue;
                    };
                    if joint_map.contains_key(&bone.element.typed_id) {
                        continue;
                    }
                    let idx = u16::try_from(joint_map.len())
                        .map_err(|_| "FBX rig exceeds 65,535 joints")?;
                    let node_index = *node_ids
                        .get(&bone.element.typed_id)
                        .ok_or("FBX bone is missing from scene hierarchy")?;
                    joint_map.insert(bone.element.typed_id, idx);
                    joint_nodes.push(node_index);
                    matrices.extend_from_slice(&matrix(&cluster.geometry_to_bone)?);
                }
            }
        }
        if joint_map.is_empty() {
            return Err("FBX skin has no connected bone nodes".into());
        }
    }
    for node in &scene.nodes {
        if let Some(mesh) = node.mesh.as_ref() {
            if selected.is_some_and(|id| id != node.element.typed_id) {
                continue;
            }
            let node_index = node_ids[&node.element.typed_id];
            let name = writer.nodes[node_index]["name"]
                .as_str()
                .unwrap_or("Imported")
                .to_string();
            let mesh_index = append_mesh(&mut writer, node, &joint_map, &name)?;
            writer.nodes[node_index]["mesh"] = json!(mesh_index);
            if !mesh.skin_deformers.is_empty() {
                let inverse = writer.accessor_f32(&matrices, "MAT4", false)?;
                let skin_idx = writer.skins.len();
                writer
                    .skins
                    .push(json!({"joints":joint_nodes,"inverseBindMatrices":inverse}));
                writer.nodes[node_index]["skin"] = json!(skin_idx);
            }
        }
    }
    if selected.is_none() {
        for stack in &scene.anim_stacks {
            let begin = stack.time_begin;
            let duration = (stack.time_end - begin).max(0.0);
            if !duration.is_finite() || duration > 120.0 {
                return Err(format!(
                    "FBX animation '{}' has unsupported duration",
                    stack.element.name
                ));
            }
            if duration < 1.0e-5 {
                continue;
            }
            let fps = scene.settings.frames_per_second.clamp(1.0, 60.0);
            let samples = ((duration * fps).ceil() as usize + 1).clamp(2, MAX_ANIMATION_SAMPLES);
            let mut sample_times = Vec::new();
            for sample in 0..samples {
                sample_times.push(f32_checked(
                    duration * sample as f64 / (samples - 1) as f64,
                )?);
            }
            let t_accessor = writer.accessor_f32(&sample_times, "SCALAR", true)?;
            let mut channels = Vec::new();
            let mut samplers = Vec::new();
            for node in &scene.nodes {
                if node.is_root {
                    continue;
                }
                // Each joint receives the full sampled local TRS. This retains FBX
                // animation layers, constraints and rotation order as evaluated by ufbx.
                if !joint_map.is_empty() && !joint_map.contains_key(&node.element.typed_id) {
                    continue;
                }
                let mut translations = Vec::new();
                let mut rotations = Vec::new();
                let mut scales = Vec::new();
                for sample in &sample_times {
                    let evaluated =
                        ufbx::evaluate_transform(&stack.anim, node, begin + *sample as f64);
                    translations.extend_from_slice(&vec3(evaluated.translation)?);
                    rotations.extend_from_slice(&quat(evaluated.rotation)?);
                    scales.extend_from_slice(&vec3(evaluated.scale)?);
                }
                let node_index = node_ids[&node.element.typed_id];
                for (path, values, kind) in [
                    ("translation", translations, "VEC3"),
                    ("rotation", rotations, "VEC4"),
                    ("scale", scales, "VEC3"),
                ] {
                    let out = writer.accessor_f32(&values, kind, false)?;
                    let sampler = samplers.len();
                    samplers
                        .push(json!({"input":t_accessor,"output":out,"interpolation":"LINEAR"}));
                    channels
                        .push(json!({"sampler":sampler,"target":{"node":node_index,"path":path}}));
                }
            }
            if !channels.is_empty() {
                writer.animations.push(json!({
                    "name":if stack.element.name.is_empty(){"Animation".to_owned()}else{stack.element.name.to_string()},
                    "channels":channels,"samplers":samplers
                }));
            }
        }
    }
    // JSON material IDs correspond to the ordered scene.materials list.
    let ids = material_indices;
    for mesh in &mut writer.meshes {
        if let Some(prims) = mesh["primitives"].as_array_mut() {
            for prim in prims {
                if let Some(value) = prim["material"].as_u64() {
                    if let Some(&index) = ids.get(&(value as u32)) {
                        prim["material"] = json!(index);
                    } else {
                        prim.as_object_mut().map(|obj| obj.remove("material"));
                    }
                }
            }
        }
    }
    writer.encode()
}

fn safe_name(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        out.push(if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            ch
        } else {
            '_'
        });
    }
    let value = out.trim_matches('_');
    if value.is_empty() {
        "Imported".to_owned()
    } else {
        value.to_owned()
    }
}

struct ConversionDirectory(PathBuf);
impl Drop for ConversionDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn temp_dir() -> Result<ConversionDirectory, String> {
    for _ in 0..16 {
        let path =
            std::env::temp_dir().join(format!("worldkiln-fbx-{:032x}", rand::random::<u128>()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(ConversionDirectory(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Could not reserve FBX conversion workspace".into())
}

/// Import FBX as native Worldkiln Actor/Character packages through the existing
/// GLB runtime representation. Static FBX scenes expose each mesh node separately.
pub fn import_fbx(project: &Path, source: &Path) -> Result<Vec<GlbFolderImportResult>, String> {
    use super::asset_registry::AssetRegistry;

    let scene = scene_from_file(source)?;
    let base = safe_name(source.file_stem().and_then(|x| x.to_str()).unwrap_or("FBX"));
    let nodes = scene
        .nodes
        .iter()
        .filter(|n| n.mesh.is_some())
        .collect::<Vec<_>>();
    if nodes.is_empty() {
        return Err("FBX contains no mesh nodes".into());
    }
    let is_rigged = nodes.iter().any(|n| {
        n.mesh
            .as_ref()
            .is_some_and(|mesh| !mesh.skin_deformers.is_empty())
    }) || !scene.anim_stacks.is_empty();
    let conversion = temp_dir()?;
    let mut planned: Vec<(String, PathBuf, GlbAssetKind)> = Vec::new();

    if is_rigged {
        let path = conversion.0.join(format!("{base}.glb"));
        fs::write(&path, build_document(&scene, source, None)?).map_err(|e| e.to_string())?;
        planned.push((base, path, GlbAssetKind::RiggedCharacter));
    } else {
        let mut named = HashSet::new();
        for node in &nodes {
            let stem = safe_name(if node.element.name.is_empty() {
                "Mesh"
            } else {
                &node.element.name
            });
            let name = safe_name(&format!("{base}_{stem}"));
            let mut unique = name.clone();
            let mut serial = 2;
            while !named.insert(unique.to_ascii_lowercase()) {
                unique = format!("{name}_{serial}");
                serial += 1;
            }
            let path = conversion.0.join(format!("{unique}.glb"));
            fs::write(
                &path,
                build_document(&scene, source, Some(node.element.typed_id))?,
            )
            .map_err(|e| e.to_string())?;
            planned.push((unique, path, GlbAssetKind::StaticMesh));
        }
    }

    // Complete all conversion and format validation before modifying the project.
    // Existing packages must not be replaced by a fresh FBX import.
    let destination = project.join(".assets/entities");
    for (name, path, expected) in &planned {
        if destination.join(name).exists() {
            return Err(format!(
                "Actor '{name}' already exists; FBX import does not overwrite creators' assets"
            ));
        }
        let summary = gltf_import::inspect_glb(path)?;
        if summary.kind != *expected {
            return Err(format!(
                "FBX conversion of '{name}' lost mesh or animation information"
            ));
        }
    }
    let baseline = AssetRegistry::load(project).or_else(|_| AssetRegistry::reconcile(project))?;
    let new_paths = planned
        .iter()
        .map(|(name, _, _)| format!(".assets/entities/{name}"))
        .collect::<Vec<_>>();
    let mut results = Vec::new();
    let operation = (|| -> Result<(), String> {
        for (name, path, kind) in &planned {
            let imported = match kind {
                GlbAssetKind::StaticMesh => GlbFolderImportResult::Static(
                    gltf_import::import_static_glb_named(project, path, name)?,
                ),
                GlbAssetKind::RiggedCharacter => GlbFolderImportResult::Character(
                    gltf_import::import_character_glb_named(project, path, Some(name))?,
                ),
            };
            results.push(imported);
        }
        Ok(())
    })();
    if let Err(error) = operation {
        return match AssetRegistry::rollback_new_imports(project, &baseline, &new_paths) {
            Ok(()) => Err(format!("FBX import rolled back: {error}")),
            Err(rollback) => Err(format!(
                "{error}; FBX import rollback needs attention: {rollback}"
            )),
        };
    }
    Ok(results)
}

/// Import every FBX in a directory as one rollback-safe operation. Unrelated
/// files are not imported, and links are not traversed.
pub fn import_fbx_folder(
    project: &Path,
    folder: &Path,
) -> Result<Vec<GlbFolderImportResult>, String> {
    use super::asset_registry::AssetRegistry;
    if !folder.is_dir() || folder.is_symlink() {
        return Err("FBX asset pack must be a normal folder".into());
    }
    let mut pending = vec![(folder.to_path_buf(), 0usize)];
    let mut sources = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        if depth > 24 || sources.len() > 2000 {
            return Err("FBX pack exceeds safe import limits".into());
        }
        for item in fs::read_dir(directory).map_err(|e| e.to_string())? {
            let path = item.map_err(|e| e.to_string())?.path();
            let ty = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if ty.file_type().is_symlink() {
                continue;
            }
            if ty.is_dir() {
                pending.push((path, depth + 1));
            } else if ty.is_file()
                && path
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("fbx"))
            {
                sources.push(path);
            }
        }
    }
    sources.sort();
    if sources.is_empty() {
        return Err("FBX folder contains no .fbx meshes".into());
    }
    let baseline = AssetRegistry::load(project).or_else(|_| AssetRegistry::reconcile(project))?;
    let mut results = Vec::new();
    let mut created_paths = Vec::new();
    for source in sources {
        match import_fbx(project, &source) {
            Ok(imported) => {
                for item in &imported {
                    match item {
                        GlbFolderImportResult::Static(result) => {
                            created_paths.extend(
                                result
                                    .packages
                                    .iter()
                                    .map(|p| format!(".assets/entities/{}", p.package_name)),
                            );
                        }
                        GlbFolderImportResult::Character(result) => {
                            created_paths.push(format!(".assets/entities/{}", result.package_name));
                        }
                    }
                }
                results.extend(imported);
            }
            Err(error) => {
                return match AssetRegistry::rollback_new_imports(project, &baseline, &created_paths)
                {
                    Ok(()) => Err(format!("FBX pack import rolled back: {error}")),
                    Err(rollback) => Err(format!(
                        "{error}; FBX pack rollback needs attention: {rollback}"
                    )),
                };
            }
        }
    }
    Ok(results)
}
