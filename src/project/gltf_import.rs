// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::{Mat4, Vec2, Vec3, Vec4};
use gltf::buffer;
use gltf::mesh::Mode;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[path = "node_animation_import.rs"]
mod node_animation_import;

const MAX_GLB_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlbAssetKind {
    StaticMesh,
    RiggedCharacter,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbMaterialSummary {
    pub name: String,
    pub base_color_factor: [f32; 4],
    pub has_base_color_texture: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbMeshSummary {
    pub name: String,
    pub primitive_count: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub material_indices: Vec<Option<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbAssetSummary {
    pub kind: GlbAssetKind,
    pub meshes: Vec<GlbMeshSummary>,
    pub materials: Vec<GlbMaterialSummary>,
    pub skin_joint_counts: Vec<usize>,
    pub animation_names: Vec<String>,
    pub embedded_image_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlbStaticVertex {
    pub source_color: [f32; 4],
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coord: Vec2,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbStaticPrimitive {
    pub base_color_factor: [f32; 4],
    pub material_name: String,
    pub material_index: Option<usize>,
    pub base_color_texture_index: Option<usize>,
    pub vertices: Vec<GlbStaticVertex>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbStaticMesh {
    pub name: String,
    pub primitives: Vec<GlbStaticPrimitive>,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedActorPackage {
    pub package_name: String,
    pub source_mesh_name: String,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
    pub collision_size: Vec3,
    pub collision_offset: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StaticGlbImportResult {
    pub source_file_name: String,
    pub packages: Vec<ImportedActorPackage>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlbSkinnedVertex {
    pub source_color: [f32; 4],
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coord: Vec2,
    pub color: [f32; 4],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbSkinnedPrimitive {
    pub base_color_texture_index: Option<usize>,
    pub material_name: String,
    pub vertices: Vec<GlbSkinnedVertex>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbSkinnedMesh {
    pub name: String,
    pub joint_names: Vec<String>,
    pub inverse_bind_matrices: Vec<Mat4>,
    pub primitives: Vec<GlbSkinnedPrimitive>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterGlbImportResult {
    pub package_name: String,
    pub source_file_name: String,
    pub mesh_names: Vec<String>,
    pub joint_count: usize,
    pub clip_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GlbFolderImportResult {
    Static(StaticGlbImportResult),
    Character(CharacterGlbImportResult),
}

#[derive(Default)]
struct ImportedTrack {
    translation: Vec<serde_json::Value>,
    rotation: Vec<serde_json::Value>,
    scale: Vec<serde_json::Value>,
}

pub fn inspect_glb(path: &Path) -> Result<GlbAssetSummary, String> {
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
    {
        return Err("Select a binary glTF 2.0 file with the .glb extension.".to_string());
    }

    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not read GLB metadata '{}': {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("Selected GLB is not a file: {}", path.display()));
    }
    if metadata.len() > MAX_GLB_BYTES {
        return Err(format!(
            "GLB is too large to import safely ({} bytes; limit is {} bytes).",
            metadata.len(),
            MAX_GLB_BYTES
        ));
    }

    let bytes = fs::read(path)
        .map_err(|error| format!("Could not read GLB '{}': {error}", path.display()))?;
    inspect_glb_bytes(&bytes, path.parent())
        .map_err(|error| format!("Invalid GLB '{}': {error}", path.display()))
}

pub fn load_static_glb_mesh(path: &Path, mesh_name: &str) -> Result<GlbStaticMesh, String> {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("asset", "decode_static_mesh", false)
            .map(|span| span.asset(path.as_ref()));
    load_static_glb_mesh_at(path, mesh_name, None)
}

fn glb_world_node_transforms(document: &gltf::Document) -> HashMap<usize, Mat4> {
    fn walk(node: gltf::Node<'_>, parent: Mat4, output: &mut HashMap<usize, Mat4>) {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        output.insert(node.index(), world);
        for child in node.children() {
            walk(child, world, output);
        }
    }
    let mut result = HashMap::new();
    if let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        for node in scene.nodes() {
            walk(node, Mat4::IDENTITY, &mut result);
        }
    } else {
        for node in document.nodes() {
            result.insert(
                node.index(),
                Mat4::from_cols_array_2d(&node.transform().matrix()),
            );
        }
    }
    result
}

fn load_static_glb_scene(path: &Path) -> Result<GlbStaticMesh, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let gltf = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
    let transforms = glb_world_node_transforms(&gltf.document);
    let origin = gltf
        .document
        .nodes()
        .filter(|node| node.mesh().is_some())
        .find_map(|node| transforms.get(&node.index()).map(|m| m.w_axis.truncate()))
        .unwrap_or(Vec3::ZERO);
    let mut combined = GlbStaticMesh {
        name: "__worldkiln_scene__".to_owned(),
        primitives: Vec::new(),
        bounds_min: Vec3::splat(f32::INFINITY),
        bounds_max: Vec3::splat(f32::NEG_INFINITY),
    };
    for mesh in gltf.document.meshes() {
        if mesh
            .name()
            .is_some_and(|name| collision_mesh_target(name).is_some())
        {
            continue;
        }
        if !gltf.document.nodes().any(|node| {
            node.mesh().is_some_and(|m| m.index() == mesh.index())
                && transforms.contains_key(&node.index())
        }) {
            continue;
        }
        let part = load_static_glb_mesh_document(
            &gltf,
            path,
            mesh.name().unwrap_or("Unnamed"),
            Some(mesh.index()),
            Some(origin),
            &transforms,
        )?;
        combined.bounds_min = combined.bounds_min.min(part.bounds_min);
        combined.bounds_max = combined.bounds_max.max(part.bounds_max);
        combined.primitives.extend(part.primitives);
    }
    if combined.primitives.is_empty() {
        return Err("GLB has no renderable meshes for one Actor".into());
    }
    Ok(combined)
}

pub fn load_static_glb_mesh_at(
    path: &Path,
    mesh_name: &str,
    mesh_index: Option<usize>,
) -> Result<GlbStaticMesh, String> {
    if mesh_name == "__worldkiln_scene__" {
        return load_static_glb_scene(path);
    }
    let bytes =
        fs::read(path).map_err(|e| format!("Could not read GLB '{}': {e}", path.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes)
        .map_err(|e| format!("Invalid GLB '{}': {e}", path.display()))?;
    let transforms = glb_world_node_transforms(&gltf.document);
    load_static_glb_mesh_document(&gltf, path, mesh_name, mesh_index, None, &transforms)
}

fn load_static_glb_mesh_document(
    gltf: &gltf::Gltf,
    path: &Path,
    mesh_name: &str,
    mesh_index: Option<usize>,
    scene_origin: Option<Vec3>,
    world_nodes: &HashMap<usize, Mat4>,
) -> Result<GlbStaticMesh, String> {
    let blob = gltf.blob.as_deref().ok_or_else(|| {
        format!(
            "Invalid GLB '{}': missing embedded binary buffer",
            path.display()
        )
    })?;
    let mesh = gltf
        .document
        .meshes()
        .find(|mesh| {
            mesh_index.map_or_else(
                || mesh.name().is_some_and(|name| name == mesh_name),
                |index| mesh.index() == index,
            )
        })
        .ok_or_else(|| {
            format!("GLB does not contain mesh '{mesh_name}' at index {mesh_index:?}")
        })?;

    let referencing_nodes = gltf
        .document
        .nodes()
        .filter(|node| {
            node.mesh()
                .is_some_and(|node_mesh| node_mesh.index() == mesh.index())
                && world_nodes.contains_key(&node.index())
        })
        .collect::<Vec<_>>();
    if referencing_nodes.len() > 128 {
        return Err(format!(
            "mesh '{mesh_name}' exceeds the 128 source-instance import limit"
        ));
    }
    if referencing_nodes.iter().any(|node| node.skin().is_some()) {
        return Err(format!(
            "mesh '{mesh_name}' is skinned; use the bind-pose loader"
        ));
    }
    let mut transforms = referencing_nodes
        .iter()
        .filter_map(|node| world_nodes.get(&node.index()).copied())
        .collect::<Vec<_>>();
    let origin = scene_origin
        .map(|p| p.extend(1.0))
        .or_else(|| transforms.first().map(|matrix| matrix.w_axis))
        .unwrap_or(glam::Vec4::W);
    for matrix in &mut transforms {
        if !matrix.is_finite() || matrix.determinant().abs() <= f32::EPSILON {
            return Err(format!(
                "mesh '{mesh_name}' uses a non-finite or non-invertible source node transform"
            ));
        }
        matrix.w_axis -= origin - glam::Vec4::W;
    }
    let transform = transforms.first().copied().unwrap_or(Mat4::IDENTITY);
    let normal_transform = transform.inverse().transpose();
    let extra_instances = transforms
        .iter()
        .skip(1)
        .map(|matrix| *matrix * transform.inverse())
        .collect::<Vec<_>>();

    let mut bounds_min = Vec3::splat(f32::INFINITY);
    let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
    let mut primitives = Vec::new();

    for primitive in mesh.primitives() {
        let primitive_index = primitive.index();
        if primitive.mode() != Mode::Triangles {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} uses {:?}; only triangle primitives are supported",
                primitive.mode()
            ));
        }
        let reader = primitive.reader(|source_buffer| match source_buffer.source() {
            buffer::Source::Bin if source_buffer.index() == 0 => Some(blob),
            _ => None,
        });

        let positions = reader
            .read_positions()
            .ok_or_else(|| {
                format!("mesh '{mesh_name}' primitive {primitive_index} has no POSITION attribute")
            })?
            .map(Vec3::from_array)
            .collect::<Vec<_>>();
        let normals = reader
            .read_normals()
            .map(|normals| normals.map(Vec3::from_array).collect::<Vec<_>>());
        let tex_coords = reader
            .read_tex_coords(0)
            .map(|coords| coords.into_f32().map(Vec2::from_array).collect::<Vec<_>>());
        let colors = reader
            .read_colors(0)
            .map(|colors| colors.into_rgba_f32().collect::<Vec<_>>());
        let indices = reader
            .read_indices()
            .map(|values| {
                values
                    .into_u32()
                    .map(|index| index as usize)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| (0..positions.len()).collect::<Vec<_>>());

        if indices.is_empty() || !indices.len().is_multiple_of(3) {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} has {} indices; triangle indices must be a non-zero multiple of three",
                indices.len()
            ));
        }
        if normals
            .as_ref()
            .is_some_and(|normals| normals.len() != positions.len())
            || tex_coords
                .as_ref()
                .is_some_and(|coords| coords.len() != positions.len())
            || colors
                .as_ref()
                .is_some_and(|colors| colors.len() != positions.len())
        {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} has mismatched vertex attribute counts"
            ));
        }

        let material = primitive.material();
        let material_index = material.index();
        let base_color_texture_index = material
            .pbr_metallic_roughness()
            .base_color_texture()
            .map(|texture| texture.texture().source().index());
        let material_name = material.name().map(str::to_string).unwrap_or_else(|| {
            material_index
                .map(|index| format!("Material {index}"))
                .unwrap_or_else(|| "Default Material".to_string())
        });
        let base_color = Vec4::from_array(material.pbr_metallic_roughness().base_color_factor());
        let mut vertices = Vec::with_capacity(indices.len());

        for triangle in indices.chunks_exact(3) {
            let triangle_positions = triangle
                .iter()
                .map(|index| {
                    positions.get(*index).copied().ok_or_else(|| {
                        format!(
                            "mesh '{mesh_name}' primitive {primitive_index} index {index} exceeds its {} vertices",
                            positions.len()
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let fallback_normal = (triangle_positions[1] - triangle_positions[0])
                .cross(triangle_positions[2] - triangle_positions[0])
                .try_normalize()
                .unwrap_or(Vec3::Y);

            for (corner, index) in triangle.iter().copied().enumerate() {
                let source_position = triangle_positions[corner];
                let position = transform.transform_point3(source_position);
                let source_normal = normals
                    .as_ref()
                    .and_then(|normals| normals.get(index))
                    .copied()
                    .unwrap_or(fallback_normal);
                let normal = normal_transform
                    .transform_vector3(source_normal)
                    .try_normalize()
                    .ok_or_else(|| {
                        format!(
                            "mesh '{mesh_name}' primitive {primitive_index} contains an invalid normal"
                        )
                    })?;
                let tex_coord = tex_coords
                    .as_ref()
                    .and_then(|coords| coords.get(index))
                    .copied()
                    .unwrap_or(Vec2::ZERO);
                let vertex_color = colors
                    .as_ref()
                    .and_then(|colors| colors.get(index))
                    .copied()
                    .map(Vec4::from_array)
                    .unwrap_or(Vec4::ONE);
                let color = (vertex_color * base_color).to_array();

                if !position.is_finite()
                    || !normal.is_finite()
                    || !tex_coord.is_finite()
                    || !color.iter().all(|value| value.is_finite())
                {
                    return Err(format!(
                        "mesh '{mesh_name}' primitive {primitive_index} contains non-finite translated vertex data"
                    ));
                }

                bounds_min = bounds_min.min(position);
                bounds_max = bounds_max.max(position);
                vertices.push(GlbStaticVertex {
                    source_color: vertex_color.to_array(),
                    position,
                    normal,
                    tex_coord,
                    color,
                });
            }
        }

        if vertices.len().saturating_mul(extra_instances.len() + 1) > 3_000_000 {
            return Err(format!(
                "mesh '{mesh_name}' has too many expanded instance vertices"
            ));
        }
        let original = GlbStaticPrimitive {
            base_color_factor: base_color.to_array(),
            material_name,
            material_index,
            base_color_texture_index,
            vertices,
        };
        for matrix in &extra_instances {
            let mut duplicate = original.clone();
            let normal_matrix = matrix.inverse().transpose();
            for vertex in &mut duplicate.vertices {
                vertex.position = matrix.transform_point3(vertex.position);
                vertex.normal = normal_matrix
                    .transform_vector3(vertex.normal)
                    .try_normalize()
                    .ok_or_else(|| {
                        format!("mesh '{mesh_name}' contains invalid instance normals")
                    })?;
                if !vertex.position.is_finite() || !vertex.normal.is_finite() {
                    return Err(format!(
                        "mesh '{mesh_name}' contains invalid instance geometry"
                    ));
                }
                bounds_min = bounds_min.min(vertex.position);
                bounds_max = bounds_max.max(vertex.position);
            }
            primitives.push(duplicate);
        }
        primitives.push(original);
    }

    Ok(GlbStaticMesh {
        name: mesh_name.to_string(),
        primitives,
        bounds_min,
        bounds_max,
    })
}

pub fn load_skinned_glb_mesh(path: &Path, mesh_name: &str) -> Result<GlbSkinnedMesh, String> {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("asset", "decode_skinned_mesh", false)
            .map(|span| span.asset(path.as_ref()));
    let summary = inspect_glb(path)?;
    if summary.kind != GlbAssetKind::RiggedCharacter {
        return Err(format!(
            "GLB '{}' has no skin and must be imported as a static Actor",
            path.display()
        ));
    }

    let bytes = fs::read(path)
        .map_err(|error| format!("Could not read GLB '{}': {error}", path.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes)
        .map_err(|error| format!("Invalid GLB '{}': {error}", path.display()))?;
    let blob = gltf.blob.as_deref().ok_or_else(|| {
        format!(
            "Invalid GLB '{}': missing embedded binary buffer",
            path.display()
        )
    })?;
    let mesh = gltf
        .document
        .meshes()
        .find(|mesh| mesh.name().is_some_and(|name| name == mesh_name))
        .ok_or_else(|| format!("GLB does not contain a mesh named '{mesh_name}'"))?;
    let nodes = gltf
        .document
        .nodes()
        .filter(|node| {
            node.mesh()
                .is_some_and(|node_mesh| node_mesh.index() == mesh.index())
        })
        .collect::<Vec<_>>();
    if nodes.len() != 1 {
        return Err(format!(
            "skinned mesh '{mesh_name}' must be referenced by exactly one source node (found {})",
            nodes.len()
        ));
    }
    let skin = nodes[0]
        .skin()
        .ok_or_else(|| format!("mesh '{mesh_name}' is not associated with a skin"))?;
    let joint_names = skin
        .joints()
        .map(|joint| {
            joint
                .name()
                .map(str::to_string)
                .ok_or_else(|| format!("skin {} contains an unnamed joint", skin.index()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let skin_reader = skin.reader(|source_buffer| match source_buffer.source() {
        buffer::Source::Bin if source_buffer.index() == 0 => Some(blob),
        _ => None,
    });
    let inverse_bind_matrices = skin_reader
        .read_inverse_bind_matrices()
        .map(|matrices| {
            matrices
                .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![Mat4::IDENTITY; joint_names.len()]);
    if inverse_bind_matrices.len() != joint_names.len()
        || inverse_bind_matrices
            .iter()
            .any(|matrix| !matrix.is_finite())
    {
        return Err(format!(
            "skin {} inverse-bind count/data does not match its {} joints",
            skin.index(),
            joint_names.len()
        ));
    }

    let mut primitives = Vec::new();
    for primitive in mesh.primitives() {
        let primitive_index = primitive.index();
        if primitive.mode() != Mode::Triangles {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} is not a triangle list"
            ));
        }
        let reader = primitive.reader(|source_buffer| match source_buffer.source() {
            buffer::Source::Bin if source_buffer.index() == 0 => Some(blob),
            _ => None,
        });
        let positions = reader
            .read_positions()
            .ok_or_else(|| {
                format!("mesh '{mesh_name}' primitive {primitive_index} has no POSITION")
            })?
            .map(Vec3::from_array)
            .collect::<Vec<_>>();
        let normals = reader
            .read_normals()
            .ok_or_else(|| format!("mesh '{mesh_name}' primitive {primitive_index} has no NORMAL"))?
            .map(Vec3::from_array)
            .collect::<Vec<_>>();
        let joints = reader
            .read_joints(0)
            .ok_or_else(|| {
                format!("mesh '{mesh_name}' primitive {primitive_index} has no JOINTS_0")
            })?
            .into_u16()
            .collect::<Vec<_>>();
        let weights = reader
            .read_weights(0)
            .ok_or_else(|| {
                format!("mesh '{mesh_name}' primitive {primitive_index} has no WEIGHTS_0")
            })?
            .into_f32()
            .collect::<Vec<_>>();
        let tex_coords = reader
            .read_tex_coords(0)
            .map(|values| values.into_f32().map(Vec2::from_array).collect::<Vec<_>>());
        let colors = reader
            .read_colors(0)
            .map(|values| values.into_rgba_f32().collect::<Vec<_>>());
        if normals.len() != positions.len()
            || joints.len() != positions.len()
            || weights.len() != positions.len()
            || tex_coords
                .as_ref()
                .is_some_and(|values| values.len() != positions.len())
            || colors
                .as_ref()
                .is_some_and(|values| values.len() != positions.len())
        {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} has mismatched skinned vertex attributes"
            ));
        }
        let indices = reader
            .read_indices()
            .map(|values| {
                values
                    .into_u32()
                    .map(|index| index as usize)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| (0..positions.len()).collect::<Vec<_>>());
        if indices.is_empty() || !indices.len().is_multiple_of(3) {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} has an invalid triangle index count"
            ));
        }
        let material = primitive.material();
        let material_name = material.name().map(str::to_string).unwrap_or_else(|| {
            material
                .index()
                .map(|index| format!("Material {index}"))
                .unwrap_or_else(|| "Default Material".to_string())
        });
        let base_color = Vec4::from_array(material.pbr_metallic_roughness().base_color_factor());
        let mut vertices = Vec::with_capacity(indices.len());
        for index in indices {
            if index >= positions.len() {
                return Err(format!(
                    "mesh '{mesh_name}' primitive {primitive_index} index {index} exceeds its {} vertices",
                    positions.len()
                ));
            }
            if joints[index]
                .iter()
                .any(|joint| *joint as usize >= joint_names.len())
            {
                return Err(format!(
                    "mesh '{mesh_name}' primitive {primitive_index} references a joint outside skin {}",
                    skin.index()
                ));
            }
            let weight_sum = weights[index].iter().sum::<f32>();
            if !weight_sum.is_finite() || weight_sum <= f32::EPSILON {
                return Err(format!(
                    "mesh '{mesh_name}' primitive {primitive_index} contains invalid skin weights"
                ));
            }
            let normalized_weights = weights[index].map(|weight| weight / weight_sum);
            let vertex_color = colors
                .as_ref()
                .map(|values| Vec4::from_array(values[index]))
                .unwrap_or(Vec4::ONE);
            let color = (vertex_color * base_color).to_array();
            let tex_coord = tex_coords
                .as_ref()
                .map(|values| values[index])
                .unwrap_or(Vec2::ZERO);
            if !positions[index].is_finite()
                || !normals[index].is_finite()
                || !tex_coord.is_finite()
                || !color.iter().all(|value| value.is_finite())
            {
                return Err(format!(
                    "mesh '{mesh_name}' primitive {primitive_index} contains non-finite vertex data"
                ));
            }
            vertices.push(GlbSkinnedVertex {
                source_color: vertex_color.to_array(),
                position: positions[index],
                normal: normals[index],
                tex_coord,
                color,
                joints: joints[index],
                weights: normalized_weights,
            });
        }
        primitives.push(GlbSkinnedPrimitive {
            base_color_texture_index: material
                .pbr_metallic_roughness()
                .base_color_texture()
                .map(|t| t.texture().source().index()),
            material_name,
            vertices,
        });
    }

    Ok(GlbSkinnedMesh {
        name: mesh_name.to_string(),
        joint_names,
        inverse_bind_matrices,
        primitives,
    })
}

fn safe_external_image_path(source: &Path, uri: &str) -> Result<PathBuf, String> {
    if uri.contains(['?', '#']) || uri.starts_with("data:") {
        return Err(format!(
            "image URI '{uri}' is not a safe relative file path"
        ));
    }
    let relative = Path::new(uri);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "image URI '{uri}' must stay inside the GLB source folder"
        ));
    }

    let root = source
        .parent()
        .ok_or_else(|| format!("GLB '{}' has no source folder", source.display()))?
        .canonicalize()
        .map_err(|error| format!("Could not resolve GLB source folder: {error}"))?;
    let candidate = root.join(relative).canonicalize().map_err(|error| {
        format!("Could not resolve external image '{uri}' beside the GLB: {error}")
    })?;
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Err(format!(
            "external image '{uri}' must be a file inside the GLB source folder"
        ));
    }
    Ok(candidate)
}

fn stage_glb_relative_images(source: &Path, package_dir: &Path) -> Result<(), String> {
    let bytes = fs::read(source).map_err(|e| format!("Cannot read GLB dependencies: {e}"))?;
    let document = gltf::Gltf::from_slice(&bytes)
        .map_err(|e| format!("Cannot inspect GLB dependencies: {e}"))?;
    for image in document.document.images() {
        let gltf::image::Source::Uri { uri, .. } = image.source() else {
            continue;
        };
        if uri.starts_with("data:") {
            continue;
        }
        let original = safe_external_image_path(source, uri)?;
        let staged = package_dir.join(Path::new(uri));
        if let Some(parent) = staged.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Cannot stage GLB texture directory: {e}"))?;
        }
        fs::copy(&original, &staged)
            .map_err(|e| format!("Cannot preserve GLB image dependency '{uri}': {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod relative_image_staging_regression {
    use super::*;

    #[test]
    fn actor_package_preserves_relative_glb_texture_uri() {
        let root = std::env::temp_dir().join(format!(
            "wk-glb-relative-images-{:032x}",
            rand::random::<u128>()
        ));
        let source_dir = root.join("Models/GLB Format");
        let source = source_dir.join("skater.glb");
        let texture = source_dir.join("Textures/colormap.png");
        let package = root.join("package");
        fs::create_dir_all(texture.parent().unwrap()).unwrap();
        fs::create_dir_all(&package).unwrap();
        fs::write(&texture, b"fixture image").unwrap();
        let mut json = br#"{"asset":{"version":"2.0"},"images":[{"uri":"Textures/colormap.png"}],"buffers":[{"byteLength":4}]}"#.to_vec();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let length = 12 + 8 + json.len() + 8 + 4;
        let mut glb = Vec::new();
        glb.extend_from_slice(&0x46546c67u32.to_le_bytes());
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(length as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x4e4f534au32.to_le_bytes());
        glb.extend_from_slice(&json);
        glb.extend_from_slice(&4u32.to_le_bytes());
        glb.extend_from_slice(&0x004e4942u32.to_le_bytes());
        glb.extend_from_slice(&[0, 0, 0, 0]);
        fs::write(&source, &glb).unwrap();
        fs::copy(&source, package.join("source.glb")).unwrap();
        stage_glb_relative_images(&source, &package).unwrap();
        assert_eq!(
            fs::read(package.join("Textures/colormap.png")).unwrap(),
            b"fixture image"
        );
        assert_eq!(
            safe_external_image_path(&package.join("source.glb"), "Textures/colormap.png").unwrap(),
            fs::canonicalize(package.join("Textures/colormap.png")).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }
}

fn stage_glb_images(source: &Path, package_dir: &Path) -> Result<usize, String> {
    let bytes = fs::read(source)
        .map_err(|error| format!("Could not read GLB '{}': {error}", source.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes)
        .map_err(|error| format!("Invalid GLB '{}': {error}", source.display()))?;
    let blob = gltf.blob.as_deref();
    let images = gltf.document.images().collect::<Vec<_>>();
    if images.is_empty() {
        return Ok(0);
    }

    let texture_dir = package_dir.join("textures");
    fs::create_dir_all(&texture_dir).map_err(|error| {
        format!(
            "Failed to create imported texture directory '{}': {error}",
            texture_dir.display()
        )
    })?;

    for source_image in images {
        let encoded = match source_image.source() {
            gltf::image::Source::View { view, .. } => {
                if !matches!(view.buffer().source(), buffer::Source::Bin)
                    || view.buffer().index() != 0
                {
                    return Err(format!(
                        "image {} does not use the GLB binary buffer",
                        source_image.index()
                    ));
                }
                let blob = blob.ok_or_else(|| "GLB has no embedded binary buffer".to_string())?;
                let start = view.offset();
                let end = start.checked_add(view.length()).ok_or_else(|| {
                    format!("image {} buffer range overflowed", source_image.index())
                })?;
                blob.get(start..end)
                    .ok_or_else(|| {
                        format!(
                            "image {} exceeds the GLB binary buffer",
                            source_image.index()
                        )
                    })?
                    .to_vec()
            }
            gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => {
                return Err(
                    "data-URI images are not supported; embed the image in the GLB binary chunk or keep a relative PNG beside the GLB"
                        .to_string(),
                );
            }
            gltf::image::Source::Uri { uri, .. } => {
                let path = safe_external_image_path(source, uri)?;
                fs::read(&path).map_err(|error| {
                    format!(
                        "Could not read external image '{}': {error}",
                        path.display()
                    )
                })?
            }
        };

        let decoded = image::load_from_memory(&encoded).map_err(|error| {
            format!(
                "Could not decode GLB image {} as a supported PNG: {error}",
                source_image.index()
            )
        })?;
        let destination = texture_dir.join(format!("image_{}.png", source_image.index()));
        decoded.save(&destination).map_err(|error| {
            format!(
                "Could not write imported texture '{}': {error}",
                destination.display()
            )
        })?;
    }

    Ok(gltf.document.images().count())
}

fn publish_glb_import_or_rollback(project_path: &Path, names: &[String]) -> Result<(), String> {
    if let Err(error) = crate::project::asset_registry::AssetRegistry::reconcile(project_path) {
        for name in names {
            let _ = fs::remove_dir_all(project_path.join(".assets/entities").join(name));
        }
        crate::engine::entity_package::reload_packages(Some(project_path));
        return Err(format!(
            "Actor files were restored after registry publication failed: {error}"
        ));
    }
    Ok(())
}

pub fn import_static_glb(
    project_path: &Path,
    source: &Path,
) -> Result<StaticGlbImportResult, String> {
    let namespace = safe_package_name(
        source
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Actor"),
        0,
    );
    let destination = project_path.join(".assets/entities").join(&namespace);
    if destination.is_dir() {
        return replace_packages(project_path, &[destination], || {
            import_static_glb_with_package_prefix(project_path, source, None)
        });
    }
    let result = import_static_glb_with_package_prefix(project_path, source, None)?;
    let names = result
        .packages
        .iter()
        .map(|p| p.package_name.clone())
        .collect::<Vec<_>>();
    publish_glb_import_or_rollback(project_path, &names)?;
    Ok(result)
}

pub fn import_static_glb_named(
    project_path: &Path,
    source: &Path,
    namespace: &str,
) -> Result<StaticGlbImportResult, String> {
    if namespace.trim().is_empty() {
        return Err("Incoming package name cannot be empty.".into());
    }
    let namespace = safe_package_name(namespace, 0);
    let destination = project_path.join(".assets/entities").join(&namespace);
    if destination.is_dir() {
        return replace_packages(project_path, &[destination], || {
            import_static_glb_with_package_prefix(project_path, source, Some(&namespace))
        });
    }
    let result = import_static_glb_with_package_prefix(project_path, source, Some(&namespace))?;
    let names = result
        .packages
        .iter()
        .map(|p| p.package_name.clone())
        .collect::<Vec<_>>();
    publish_glb_import_or_rollback(project_path, &names)?;
    Ok(result)
}

fn unique_mesh_package_name(base: String, seen: &mut HashSet<String>) -> String {
    let mut candidate = base.clone();
    let mut suffix = 2usize;
    while !seen.insert(candidate.to_ascii_lowercase()) {
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
    candidate
}

fn import_static_glb_with_package_prefix(
    project_path: &Path,
    source: &Path,
    package_prefix: Option<&str>,
) -> Result<StaticGlbImportResult, String> {
    let summary = inspect_glb(source)?;
    if summary.kind != GlbAssetKind::StaticMesh {
        return Err("Rigged GLB content must be imported as an Actor source package.".to_string());
    }

    let source_file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "Selected GLB has no valid source filename.".to_string())?
        .to_string();
    let name = safe_package_name(
        package_prefix.unwrap_or_else(|| {
            source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Actor")
        }),
        0,
    );
    let planned = vec![(name, load_static_glb_scene(source)?, usize::MAX)];
    if planned.is_empty() {
        return Err("No renderable mesh in GLB".into());
    }

    let entities_root = project_path.join(".assets").join("entities");
    for (package_name, _, _) in &planned {
        let destination = entities_root.join(package_name);
        if destination
            .try_exists()
            .map_err(|error| format!("Failed to inspect '{}': {error}", destination.display()))?
        {
            return Err(format!(
                "An Actor package named '{package_name}' already exists in this project; import will not overwrite or merge it."
            ));
        }
    }

    fs::create_dir_all(&entities_root).map_err(|error| {
        format!(
            "Failed to create project Actor asset directory '{}': {error}",
            entities_root.display()
        )
    })?;

    let mut staged = Vec::with_capacity(planned.len());
    let mut committed = Vec::with_capacity(planned.len());
    let import_result = (|| {
        for (index, (package_name, mesh, mesh_index)) in planned.iter().enumerate() {
            let temp = unique_import_temp(&entities_root, package_name, index)?;
            fs::create_dir(&temp).map_err(|error| {
                format!(
                    "Failed to create temporary Actor import '{}': {error}",
                    temp.display()
                )
            })?;
            staged.push(temp.clone());

            let copied_glb = temp.join("source.glb");
            fs::copy(source, &copied_glb).map_err(|error| {
                format!(
                    "Failed to copy GLB into project package '{}': {error}",
                    copied_glb.display()
                )
            })?;
            stage_glb_images(source, &temp)?;
            stage_glb_relative_images(source, &temp)?;

            let collider_shapes = authored_glb_collider_shapes(source, &mesh.name)?;
            let package_json = serde_json::json!({
                "name": package_name,
                "version": 1,
                "description": format!("Imported from {source_file_name} mesh {}", mesh.name),
                "appearance": {
                    "nodes": [{ "name": "Root" }],
                    "imported_glb": {
                        "source": "source.glb",
                        "mesh": mesh.name,
                        "mesh_index": if *mesh_index == usize::MAX { None } else { Some(*mesh_index) },
                    }
                },
                "physics": {
                    "enabled": true,
                    "solid": true,
                    "anchored": true,
                    "collision_enabled": true,
                    "mass": 1.0,
                    "collision_shapes": collider_shapes,
                }
            });
            write_json(&temp.join("package.json"), &package_json)?;

            let import_json = serde_json::json!({
                "format": "glb",
                "source_file_name": source_file_name,
                "source_mesh_name": mesh.name,
                "bounds_min": mesh.bounds_min.to_array(),
                "bounds_max": mesh.bounds_max.to_array(),
            });
            write_json(&temp.join("import.json"), &import_json)?;

            crate::engine::entity_package::load_package(&temp).map_err(|error| {
                format!(
                    "Generated Actor package '{}' failed validation: {error}",
                    package_name
                )
            })?;
        }

        for ((package_name, _, _), temp) in planned.iter().zip(staged.iter()) {
            let destination = entities_root.join(package_name);
            fs::rename(temp, &destination).map_err(|error| {
                format!(
                    "Failed to publish imported Actor package '{}': {error}",
                    destination.display()
                )
            })?;
            committed.push(destination);
        }

        crate::engine::entity_package::reload_packages(Some(project_path));
        for (package_name, _, _) in &planned {
            if crate::engine::entity_package::get_package(package_name).is_none() {
                return Err(format!(
                    "Imported Actor package '{package_name}' did not resolve after catalog refresh."
                ));
            }
        }

        Ok(())
    })();

    if let Err(error) = import_result {
        for path in staged.iter().chain(committed.iter()) {
            if path.exists() {
                let _ = fs::remove_dir_all(path);
            }
        }
        crate::engine::entity_package::reload_packages(Some(project_path));
        return Err(error);
    }

    let packages = planned
        .into_iter()
        .map(|(package_name, mesh, _)| {
            let collision_size = (mesh.bounds_max - mesh.bounds_min).max(Vec3::splat(0.01));
            ImportedActorPackage {
                package_name,
                source_mesh_name: mesh.name,
                bounds_min: mesh.bounds_min,
                bounds_max: mesh.bounds_max,
                collision_size,
                collision_offset: (mesh.bounds_min + mesh.bounds_max) * 0.5,
            }
        })
        .collect();

    Ok(StaticGlbImportResult {
        source_file_name,
        packages,
    })
}

fn planned_single_glb_destinations(
    project_path: &Path,
    source: &Path,
) -> Result<(GlbAssetKind, Vec<PathBuf>), String> {
    let summary = inspect_glb(source)?;
    match summary.kind {
        GlbAssetKind::StaticMesh => {
            let name = safe_package_name(
                source
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("Actor"),
                0,
            );
            Ok((
                GlbAssetKind::StaticMesh,
                vec![project_path.join(".assets/entities").join(name)],
            ))
        }
        GlbAssetKind::RiggedCharacter => {
            let name = if summary.skin_joint_counts.is_empty() {
                safe_package_name(
                    source
                        .file_stem()
                        .and_then(|name| name.to_str())
                        .unwrap_or("AnimatedCharacter"),
                    0,
                )
            } else {
                summary
                    .meshes
                    .iter()
                    .find(|mesh| load_skinned_glb_mesh(source, &mesh.name).is_ok())
                    .map(|mesh| safe_package_name(&mesh.name, 0))
                    .ok_or("Rigged GLB has no importable skinned mesh")?
            };
            Ok((
                GlbAssetKind::RiggedCharacter,
                vec![project_path.join(".assets/entities").join(name)],
            ))
        }
    }
}

fn replace_packages<T>(
    project_path: &Path,
    destinations: &[PathBuf],
    import: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut backups = Vec::<(PathBuf, PathBuf)>::new();
    for (i, dest) in destinations.iter().enumerate() {
        if !dest.exists() {
            continue;
        }
        let parent = dest.parent().ok_or("Invalid Actor/Character destination")?;
        let name = dest
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid Actor/Character package name")?;
        let backup = unique_import_temp(parent, name, i)?;
        if let Err(error) = fs::rename(dest, &backup) {
            for (original, staged) in backups.iter().rev() {
                let _ = fs::rename(staged, original);
            }
            return Err(format!(
                "Cannot back up existing package '{}': {error}",
                dest.display()
            ));
        }
        backups.push((dest.clone(), backup));
    }
    let result = import().and_then(|value| {
        crate::project::asset_registry::AssetRegistry::reconcile(project_path)?;
        crate::project::asset_registry::AssetRegistry::notify_content_changed(project_path)?;
        Ok(value)
    });
    match result {
        Ok(value) => {
            for (_, backup) in backups {
                let _ = fs::remove_dir_all(backup);
            }
            Ok(value)
        }
        Err(error) => {
            for path in destinations {
                if path.is_dir() {
                    let _ = fs::remove_dir_all(path);
                }
            }
            let mut restore_errors = Vec::new();
            for (original, backup) in backups.into_iter().rev() {
                if let Err(e) = fs::rename(&backup, &original) {
                    restore_errors.push(format!("{}: {e}", original.display()));
                }
            }
            crate::engine::entity_package::reload_packages(Some(project_path));
            crate::character::package::reload_packages(Some(project_path));
            if restore_errors.is_empty() {
                Err(format!("{error}; original packages were restored"))
            } else {
                Err(format!(
                    "{error}; package restore failed: {}",
                    restore_errors.join("; ")
                ))
            }
        }
    }
}

pub fn replace_registered_actor(
    project: &Path,
    id: &str,
    source: &Path,
) -> Result<crate::project::asset_registry::AssetOperationResult, String> {
    use crate::project::asset_registry::{AssetOperationResult, AssetRegistry, AssetType};
    if !source.is_file()
        || source.is_symlink()
        || !source
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("glb"))
    {
        return Err("Actor replacement must be a regular GLB file".into());
    }
    let registry = AssetRegistry::load(project)?;
    let record = registry.find(id).ok_or("Unknown Actor AssetID")?;
    if record.kind != AssetType::Actor {
        return Err("Selected asset is not an Actor".into());
    }
    let target = project.join(&record.path);
    let package_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid registered Actor package")?
        .to_owned();
    if record.path != format!(".assets/entities/{package_name}") || !target.is_dir() {
        return Err("Actor does not occupy an approved project package location".into());
    }
    let summary = inspect_glb(source)?;
    if summary.kind == GlbAssetKind::StaticMesh
        && registry
            .users_of(id)
            .iter()
            .any(|user| user.kind == AssetType::Character)
    {
        return Err("This Actor supports a Character. Replace it with another rigged GLB".into());
    }
    replace_packages(project, &[target], || match summary.kind {
        GlbAssetKind::StaticMesh => {
            import_static_glb_with_package_prefix(project, source, Some(&package_name)).map(|_| ())
        }
        GlbAssetKind::RiggedCharacter => {
            import_character_glb_named_inner(project, source, Some(&package_name)).map(|_| ())
        }
    })?;
    let current = AssetRegistry::load(project)?;
    if current.find(id).is_none() {
        return Err("Actor AssetID was not preserved".into());
    }
    Ok(AssetOperationResult {
        asset_id: id.to_owned(),
        revision: current.revision,
        removed: false,
        affected_ids: vec![id.to_owned()],
    })
}

pub fn replace_single_glb(
    project_path: &Path,
    source: &Path,
) -> Result<GlbFolderImportResult, String> {
    let (kind, destinations) = planned_single_glb_destinations(project_path, source)?;
    replace_packages(project_path, &destinations, || match kind {
        GlbAssetKind::StaticMesh => {
            import_static_glb(project_path, source).map(GlbFolderImportResult::Static)
        }
        GlbAssetKind::RiggedCharacter => {
            import_character_glb(project_path, source).map(GlbFolderImportResult::Character)
        }
    })
}

pub fn replace_glb_folder(
    project_path: &Path,
    folder: &Path,
) -> Result<GlbFolderImportResult, String> {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
        if depth > 24 || out.len() > 2000 {
            return Err("GLB package directory exceeds the safe import scan limit".into());
        }
        for item in fs::read_dir(dir).map_err(|e| format!("Cannot inspect GLB package: {e}"))? {
            let item = item.map_err(|e| format!("Cannot read GLB package entry: {e}"))?;
            let kind = item.file_type().map_err(|e| e.to_string())?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                collect(&item.path(), out, depth + 1)?;
            } else if kind.is_file()
                && item
                    .path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
            {
                out.push(item.path());
            }
        }
        Ok(())
    }
    if !folder.is_dir() || folder.is_symlink() {
        return Err("GLB asset pack must be a normal folder.".into());
    }
    let mut sources = Vec::new();
    collect(folder, &mut sources, 0)?;
    sources.sort();
    if sources.is_empty() {
        return Err("GLB package contains no .glb files".into());
    }
    let mut destinations = Vec::new();
    let mut seen_names = HashSet::new();
    for source in &sources {
        inspect_glb(source)?;
        let name = safe_package_name(
            source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Actor"),
            0,
        );
        if !seen_names.insert(name.to_ascii_lowercase()) {
            return Err(format!("GLB names conflict at '{name}'"));
        }
        destinations.push(project_path.join(".assets/entities").join(name));
    }
    replace_packages(project_path, &destinations, || {
        import_glb_folder(project_path, folder)
    })
}

pub fn import_static_glb_folder(
    project_path: &Path,
    folder: &Path,
) -> Result<StaticGlbImportResult, String> {
    import_static_glb_folder_named(project_path, folder, None)
}

pub fn import_static_glb_folder_named(
    project_path: &Path,
    folder: &Path,
    namespace: Option<&str>,
) -> Result<StaticGlbImportResult, String> {
    import_static_glb_folder_with_progress(project_path, folder, namespace, None)
}

fn import_static_glb_folder_with_progress(
    project_path: &Path,
    folder: &Path,
    namespace: Option<&str>,
    progress: Option<&std::sync::atomic::AtomicUsize>,
) -> Result<StaticGlbImportResult, String> {
    if namespace.is_some_and(|value| value.trim().is_empty()) {
        return Err("Incoming package name cannot be empty.".into());
    }
    fn collect_glbs(folder: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(folder).map_err(|error| {
            format!(
                "Could not read asset-pack folder '{}': {error}",
                folder.display()
            )
        })? {
            let path = entry
                .map_err(|error| format!("Could not read asset-pack entry: {error}"))?
                .path();
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                collect_glbs(&path, output)?;
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
            {
                output.push(path);
            }
        }
        Ok(())
    }

    if !folder.is_dir() {
        return Err(format!(
            "Selected asset-pack path is not a folder: {}",
            folder.display()
        ));
    }
    let mut sources = Vec::new();
    collect_glbs(folder, &mut sources)?;
    sources.sort();
    if let Some(progress) = progress {
        progress
            .compare_exchange(
                0,
                sources.len() << 32,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            )
            .ok();
    }
    if sources.is_empty() {
        return Err("Selected asset-pack folder contains no .glb files.".to_string());
    }

    let entities_root = project_path.join(".assets").join("entities");
    let mut planned_names = HashSet::new();
    for source in &sources {
        let summary = inspect_glb(source)?;
        if let Some(progress) = progress {
            progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if summary.kind != GlbAssetKind::StaticMesh {
            return Err(format!(
                "Asset-pack GLB '{}' is rigged; folder import currently accepts static Actor packs only.",
                source.display()
            ));
        }
        let prefix = safe_package_name(
            source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Imported"),
            0,
        );
        let prefix = namespace
            .map(|group| format!("{}_{}", safe_package_name(group, 0), prefix))
            .unwrap_or(prefix);
        if !planned_names.insert(prefix.to_ascii_lowercase()) {
            return Err(format!("Imported GLB names conflict at '{prefix}'"));
        }
        if entities_root.join(&prefix).exists() {
            return Err(format!(
                "An Actor named '{prefix}' already exists in this project"
            ));
        }
    }

    let mut imported_packages = Vec::new();
    for source in &sources {
        let prefix = safe_package_name(
            source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Imported"),
            0,
        );
        let prefix = namespace
            .map(|group| format!("{}_{}", safe_package_name(group, 0), prefix))
            .unwrap_or(prefix);
        match import_static_glb_with_package_prefix(project_path, source, Some(&prefix)) {
            Ok(imported) => {
                imported_packages.extend(imported.packages);
                if let Some(progress) = progress {
                    progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Err(error) => {
                for package in &imported_packages {
                    let _ = fs::remove_dir_all(entities_root.join(&package.package_name));
                }
                crate::engine::entity_package::reload_packages(Some(project_path));
                return Err(error);
            }
        }
    }

    crate::project::asset_registry::AssetRegistry::reconcile(project_path)?;
    Ok(StaticGlbImportResult {
        source_file_name: folder
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("asset pack")
            .to_string(),
        packages: imported_packages,
    })
}

pub fn import_glb_folder(
    project_path: &Path,
    folder: &Path,
) -> Result<GlbFolderImportResult, String> {
    import_glb_folder_named(project_path, folder, None)
}

pub fn import_glb_folder_named(
    project_path: &Path,
    folder: &Path,
    namespace: Option<&str>,
) -> Result<GlbFolderImportResult, String> {
    import_glb_folder_with_progress(project_path, folder, namespace, None)
}

pub fn import_glb_folder_with_progress(
    project_path: &Path,
    folder: &Path,
    namespace: Option<&str>,
    progress: Option<&std::sync::atomic::AtomicUsize>,
) -> Result<GlbFolderImportResult, String> {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("asset", "import_background_job", false);
    if namespace.is_some_and(|value| value.trim().is_empty()) {
        return Err("Incoming package name cannot be empty.".into());
    }
    fn collect(folder: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(folder).map_err(|error| {
            format!(
                "Could not read 3D asset folder '{}': {error}",
                folder.display()
            )
        })? {
            let path = entry
                .map_err(|error| format!("Could not read 3D asset folder entry: {error}"))?
                .path();
            if path.is_symlink() {
                continue;
            }
            if path.is_dir() {
                collect(&path, output)?;
            } else if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
            {
                output.push(path);
            }
        }
        Ok(())
    }

    if !folder.is_dir() {
        return Err(format!(
            "Selected 3D asset path is not a folder: {}",
            folder.display()
        ));
    }
    let mut sources = Vec::new();
    collect(folder, &mut sources)?;
    sources.sort();
    if let Some(progress) = progress {
        progress.store(sources.len() << 32, std::sync::atomic::Ordering::Relaxed);
    }
    if sources.is_empty() {
        return Err("Selected 3D asset folder contains no .glb files.".to_string());
    }

    let mut plan = Vec::new();
    let mut names = HashSet::new();
    let actor_root = project_path.join(".assets/entities");
    for source in &sources {
        let summary = inspect_glb(source)?;
        let stem = source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Actor");
        let name = safe_package_name(
            &namespace
                .map(|prefix| format!("{}_{}", safe_package_name(prefix, 0), stem))
                .unwrap_or_else(|| stem.to_owned()),
            0,
        );
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "More than one GLB normalizes to Actor name '{name}'"
            ));
        }
        plan.push((source, summary, name));
    }
    let destinations = plan
        .iter()
        .map(|(_, _, name)| actor_root.join(name))
        .collect::<Vec<_>>();
    if destinations.iter().any(|destination| destination.exists()) {
        return replace_packages(project_path, &destinations, || {
            import_glb_folder_with_progress(project_path, folder, namespace, progress)
        });
    }
    let mut created = Vec::new();
    let mut actors = Vec::new();
    let mut only_character = None;
    for (index, (source, summary, name)) in plan.iter().enumerate() {
        let imported = match summary.kind {
            GlbAssetKind::StaticMesh => {
                import_static_glb_with_package_prefix(project_path, source, Some(name))
                    .map(|result| result.packages)
            }
            GlbAssetKind::RiggedCharacter => {
                import_character_glb_named_inner(project_path, source, Some(name)).map(|result| {
                    if plan.len() == 1 {
                        only_character = Some(result.clone());
                    }
                    let min = summary
                        .meshes
                        .iter()
                        .fold(Vec3::splat(f32::INFINITY), |bounds, mesh| {
                            bounds.min(mesh.bounds_min)
                        });
                    let max = summary
                        .meshes
                        .iter()
                        .fold(Vec3::splat(f32::NEG_INFINITY), |bounds, mesh| {
                            bounds.max(mesh.bounds_max)
                        });
                    let size = (max - min).max(Vec3::splat(0.01));
                    vec![ImportedActorPackage {
                        package_name: result.package_name,
                        source_mesh_name: result.mesh_names.first().cloned().unwrap_or_default(),
                        bounds_min: min,
                        bounds_max: max,
                        collision_size: size,
                        collision_offset: (max + min) * 0.5,
                    }]
                })
            }
        };
        match imported {
            Ok(packages) => {
                created.extend(packages.iter().map(|p| p.package_name.clone()));
                actors.extend(packages);
            }
            Err(error) => {
                for name in &created {
                    let _ = fs::remove_dir_all(actor_root.join(name));
                }
                crate::engine::entity_package::reload_packages(Some(project_path));
                return Err(error);
            }
        }
        if let Some(progress) = progress {
            progress.store(
                ((index + 1) * 100 / plan.len()).min(100),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }
    publish_glb_import_or_rollback(project_path, &created)?;
    if let Some(character) = only_character {
        return Ok(GlbFolderImportResult::Character(character));
    }
    Ok(GlbFolderImportResult::Static(StaticGlbImportResult {
        source_file_name: folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Imported")
            .to_owned(),
        packages: actors,
    }))
}

pub fn import_character_glb(
    project_path: &Path,
    source: &Path,
) -> Result<CharacterGlbImportResult, String> {
    import_character_glb_named(project_path, source, None)
}

pub fn import_character_glb_named(
    project_path: &Path,
    source: &Path,
    incoming_name: Option<&str>,
) -> Result<CharacterGlbImportResult, String> {
    let destination = if let Some(name) = incoming_name {
        project_path
            .join(".assets/entities")
            .join(safe_package_name(name, 0))
    } else {
        planned_single_glb_destinations(project_path, source)?
            .1
            .into_iter()
            .next()
            .ok_or("No planned Actor package")?
    };
    if destination.is_dir() {
        return replace_packages(project_path, &[destination], || {
            import_character_glb_named_inner(project_path, source, incoming_name)
        });
    }
    let result = import_character_glb_named_inner(project_path, source, incoming_name)?;
    publish_glb_import_or_rollback(project_path, &[result.package_name.clone()])?;
    Ok(result)
}
fn import_character_glb_named_inner(
    project_path: &Path,
    source: &Path,
    incoming_name: Option<&str>,
) -> Result<CharacterGlbImportResult, String> {
    if incoming_name.is_some_and(|name| name.trim().is_empty()) {
        return Err("Incoming package name cannot be empty.".into());
    }
    let summary = inspect_glb(source)?;
    if summary.kind != GlbAssetKind::RiggedCharacter {
        return Err("Static GLB content must be imported as Actor packages.".to_string());
    }
    if summary.skin_joint_counts.is_empty() {
        return node_animation_import::import_node_character(project_path, source, incoming_name);
    }
    let source_file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "Selected GLB has no valid source filename.".to_string())?
        .to_string();
    let bytes = fs::read(source)
        .map_err(|error| format!("Could not read GLB '{}': {error}", source.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes)
        .map_err(|error| format!("Invalid GLB '{}': {error}", source.display()))?;
    let blob = gltf
        .blob
        .as_deref()
        .ok_or_else(|| "Rigged GLB has no embedded binary buffer".to_string())?;

    let mut skinned_meshes = Vec::new();
    let mut static_meshes = Vec::new();
    for mesh in &summary.meshes {
        if load_skinned_glb_mesh(source, &mesh.name).is_ok() {
            skinned_meshes.push(mesh.name.clone());
        } else if load_static_glb_mesh_at(source, &mesh.name, None).is_ok() {
            static_meshes.push(mesh.name.clone());
        } else {
            return Err(format!(
                "GLB mesh '{}' could not be imported; refusing a partial Actor",
                mesh.name
            ));
        }
    }
    if skinned_meshes.is_empty() {
        return Err("Rigged GLB contains no mesh node associated with a skin.".to_string());
    }
    let package_name = safe_package_name(incoming_name.unwrap_or(&skinned_meshes[0]), 0);
    let actors_root = project_path.join(".assets").join("entities");
    let destination = actors_root.join(&package_name);
    if destination
        .try_exists()
        .map_err(|error| format!("Failed to inspect '{}': {error}", destination.display()))?
    {
        return Err(format!(
            "An Actor package named '{package_name}' already exists in this project; import will not overwrite or merge it."
        ));
    }
    let nodes = gltf.document.nodes().collect::<Vec<_>>();
    let mut parents = vec![None; nodes.len()];
    for node in &nodes {
        for child in node.children() {
            if parents[child.index()].replace(node.index()).is_some() {
                return Err(format!(
                    "node '{}' has more than one parent",
                    child.name().unwrap_or("unnamed")
                ));
            }
        }
    }
    let local_matrices = nodes
        .iter()
        .map(|node| Mat4::from_cols_array_2d(&node.transform().matrix()))
        .collect::<Vec<_>>();
    let skin = gltf
        .document
        .skins()
        .next()
        .ok_or_else(|| "Rigged GLB contains no skin".to_string())?;
    let source_joint_indices = skin.joints().map(|joint| joint.index()).collect::<Vec<_>>();
    let source_joint_set = source_joint_indices.iter().copied().collect::<HashSet<_>>();
    let mut ordered_joint_indices = source_joint_indices.clone();
    ordered_joint_indices.sort_by_key(|node_index| {
        let mut depth = 0usize;
        let mut parent = parents[*node_index];
        while let Some(parent_index) = parent {
            if source_joint_set.contains(&parent_index) {
                depth += 1;
            }
            parent = parents[parent_index];
        }
        depth
    });
    let skeleton_index_by_node = ordered_joint_indices
        .iter()
        .enumerate()
        .map(|(index, node)| (*node, index))
        .collect::<HashMap<_, _>>();

    let global_matrix = |node_index: usize| {
        let mut chain = vec![node_index];
        let mut parent = parents[node_index];
        while let Some(parent_index) = parent {
            chain.push(parent_index);
            parent = parents[parent_index];
        }
        chain
            .into_iter()
            .rev()
            .fold(Mat4::IDENTITY, |matrix, index| {
                matrix * local_matrices[index]
            })
    };
    let mut rig_joints = Vec::new();
    let mut bind_transforms = HashMap::new();
    for node_index in &ordered_joint_indices {
        let node = &nodes[*node_index];
        let name = node
            .name()
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| format!("joint node {node_index} has no stable name"))?;
        let parent_joint =
            parents[*node_index].and_then(|parent| skeleton_index_by_node.get(&parent).copied());
        let matrix = if parent_joint.is_some() {
            local_matrices[*node_index]
        } else {
            global_matrix(*node_index)
        };
        let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
        if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
            return Err(format!(
                "joint '{name}' contains a non-finite bind transform"
            ));
        }
        bind_transforms.insert(*node_index, (translation, rotation, scale));
        rig_joints.push(serde_json::json!({
            "name": name,
            "parent": parent_joint,
            "translation": translation.to_array(),
            "rotation": rotation.to_array(),
            "scale": scale.to_array(),
        }));
    }

    let right_hand_joint = ["Fist.R", "Hand.R", "Wrist.R", "RightHand"]
        .into_iter()
        .find_map(|candidate| {
            ordered_joint_indices.iter().find_map(|node_index| {
                nodes[*node_index]
                    .name()
                    .filter(|name| name.eq_ignore_ascii_case(candidate))
                    .map(str::to_string)
            })
        });

    let mut seen_clips = HashSet::new();
    let mut animations_json = Vec::new();
    let mut clip_names = Vec::new();
    for animation in gltf.document.animations() {
        let source_name = animation
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Animation {}", animation.index()));
        let clip_name = source_name
            .rsplit('|')
            .next()
            .unwrap_or(&source_name)
            .trim()
            .to_string();
        if clip_name.is_empty() || !seen_clips.insert(clip_name.to_ascii_lowercase()) {
            continue;
        }
        let mut tracks: HashMap<usize, ImportedTrack> = HashMap::new();
        let mut duration = 0.0f32;
        for channel in animation.channels() {
            let stepped = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Linear => false,
                gltf::animation::Interpolation::Step => true,
                gltf::animation::Interpolation::CubicSpline => {
                    return Err(format!(
                        "animation '{source_name}' uses CUBICSPLINE interpolation, which requires tangent-aware sampling"
                    ));
                }
            };
            let node_index = channel.target().node().index();
            if !source_joint_set.contains(&node_index) {
                continue;
            }
            let reader = channel.reader(|source_buffer| match source_buffer.source() {
                buffer::Source::Bin if source_buffer.index() == 0 => Some(blob),
                _ => None,
            });
            let times = reader
                .read_inputs()
                .ok_or_else(|| format!("animation '{source_name}' channel has no input times"))?
                .collect::<Vec<_>>();
            if times.iter().any(|time| !time.is_finite()) {
                return Err(format!(
                    "animation '{source_name}' contains non-finite key times"
                ));
            }
            duration = duration.max(times.last().copied().unwrap_or(0.0));
            let outputs = reader
                .read_outputs()
                .ok_or_else(|| format!("animation '{source_name}' channel has no outputs"))?;
            let track = tracks.entry(node_index).or_default();
            match outputs {
                gltf::animation::util::ReadOutputs::Translations(values) => {
                    let values = values.collect::<Vec<_>>();
                    if values.len() != times.len() {
                        return Err(format!(
                            "animation '{source_name}' translation key count mismatch"
                        ));
                    }
                    for (time, value) in times.iter().copied().zip(values) {
                        if value.iter().any(|component| !component.is_finite()) {
                            return Err(format!(
                                "animation '{source_name}' has non-finite translation keys"
                            ));
                        }
                        track.translation.push(
                            serde_json::json!({"time": time, "value": value, "step": stepped}),
                        );
                    }
                }
                gltf::animation::util::ReadOutputs::Rotations(values) => {
                    let values = values.into_f32().collect::<Vec<_>>();
                    if values.len() != times.len() {
                        return Err(format!(
                            "animation '{source_name}' rotation key count mismatch"
                        ));
                    }
                    for (time, value) in times.iter().copied().zip(values) {
                        if value.iter().any(|component| !component.is_finite()) {
                            return Err(format!(
                                "animation '{source_name}' has non-finite rotation keys"
                            ));
                        }
                        track.rotation.push(
                            serde_json::json!({"time": time, "value": value, "step": stepped}),
                        );
                    }
                }
                gltf::animation::util::ReadOutputs::Scales(values) => {
                    let values = values.collect::<Vec<_>>();
                    if values.len() != times.len() {
                        return Err(format!(
                            "animation '{source_name}' scale key count mismatch"
                        ));
                    }
                    for (time, value) in times.iter().copied().zip(values) {
                        if value.iter().any(|component| !component.is_finite()) {
                            return Err(format!(
                                "animation '{source_name}' has non-finite scale keys"
                            ));
                        }
                        track.scale.push(
                            serde_json::json!({"time": time, "value": value, "step": stepped}),
                        );
                    }
                }
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    return Err(format!(
                        "animation '{source_name}' uses unsupported morph weights"
                    ));
                }
            }
        }
        let mut track_json = Vec::new();
        for node_index in ordered_joint_indices.iter().copied() {
            let Some(mut track) = tracks.remove(&node_index) else {
                continue;
            };
            let (translation, rotation, scale) = bind_transforms[&node_index];
            if track.translation.is_empty() {
                track
                    .translation
                    .push(serde_json::json!({"time": 0.0, "value": translation.to_array()}));
            }
            if track.rotation.is_empty() {
                track
                    .rotation
                    .push(serde_json::json!({"time": 0.0, "value": rotation.to_array()}));
            }
            if track.scale.is_empty() {
                track
                    .scale
                    .push(serde_json::json!({"time": 0.0, "value": scale.to_array()}));
            }
            track_json.push(serde_json::json!({
                "joint": nodes[node_index].name().unwrap_or("Joint"),
                "translation": track.translation,
                "rotation": track.rotation,
                "scale": track.scale,
            }));
        }
        let looping = matches!(
            clip_name.to_ascii_lowercase().as_str(),
            "idle" | "walk" | "run" | "attacking_idle"
        );
        animations_json.push(serde_json::json!({
            "name": clip_name,
            "duration": duration.max(0.0001),
            "loop": looping,
            "tracks": track_json,
        }));
        clip_names.push(clip_name);
    }

    let preferred_clip = |label: &str| {
        clip_names
            .iter()
            .find(|clip| clip.eq_ignore_ascii_case(label))
            .map(String::as_str)
    };
    let default_clip = preferred_clip("Idle")
        .or_else(|| clip_names.first().map(String::as_str))
        .unwrap_or("");
    let idle_clip = preferred_clip("Idle").unwrap_or(default_clip);
    let walk_clip = preferred_clip("Walk").unwrap_or(idle_clip);
    let run_clip = preferred_clip("Run").unwrap_or(walk_clip);

    fs::create_dir_all(&actors_root).map_err(|error| {
        format!(
            "Failed to create Actor asset directory '{}': {error}",
            actors_root.display()
        )
    })?;
    let temp = unique_import_temp(&actors_root, &package_name, 0)?;
    fs::create_dir(&temp)
        .map_err(|error| format!("Failed to create temporary Character import: {error}"))?;
    let result = (|| {
        fs::copy(source, temp.join("source.glb"))
            .map_err(|error| format!("Failed to copy GLB into Actor package: {error}"))?;
        stage_glb_images(source, &temp)?;
        stage_glb_relative_images(source, &temp)?;
        let attachment_rotations = right_hand_joint
            .as_deref()
            .filter(|joint| joint.eq_ignore_ascii_case("Fist.R"))
            .map(|_| {
                serde_json::json!({
                    "RightHand": [-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2]
                })
            })
            .unwrap_or_else(|| serde_json::json!({}));
        write_json(
            &temp.join("character-package.json"),
            &serde_json::json!({
                "name": package_name,
                "mesh_type": "imported_glb",
                "default_animation": default_clip,
                "attachment_points": right_hand_joint
                    .as_ref()
                    .map(|joint| serde_json::json!({ "RightHand": joint }))
                    .unwrap_or_else(|| serde_json::json!({})),
                "attachment_rotations": attachment_rotations
            }),
        )?;
        let materials = summary
            .materials
            .iter()
            .map(|material| {
                (
                    material.name.clone(),
                    serde_json::json!(material.base_color_factor),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        write_json(
            &temp.join("appearance.json"),
            &serde_json::json!({"materials": materials}),
        )?;
        write_json(
            &temp.join("collision.json"),
            &serde_json::json!({"radius": 0.5, "height": 2.0}),
        )?;
        write_json(
            &temp.join("rig.json"),
            &serde_json::json!({"joints": rig_joints}),
        )?;
        write_json(
            &temp.join("animation.json"),
            &serde_json::json!({"animations": animations_json}),
        )?;
        write_json(
            &temp.join("blend.json"),
            &serde_json::json!({
                "default_state": default_clip,
                "blend_speed": 4.0,
                "idle_clip": idle_clip,
                "walk_clip": walk_clip,
                "run_clip": run_clip,
                "run_threshold": 5.0
            }),
        )?;
        let geometry_parts = skinned_meshes
            .iter()
            .map(|mesh| {
                serde_json::json!({
                    "type": "imported_glb", "source": "source.glb", "mesh": mesh
                })
            })
            .collect::<Vec<_>>();
        write_json(
            &temp.join("geometry.json"),
            &serde_json::json!({"parts": geometry_parts}),
        )?;
        write_json(
            &temp.join("import.json"),
            &serde_json::json!({
                "format": "glb",
                "source_file_name": source_file_name,
                "meshes": summary.meshes.iter().map(|mesh|&mesh.name).collect::<Vec<_>>(),
                "joint_count": ordered_joint_indices.len(),
                "clips": clip_names,
            }),
        )?;

        let metadata_path = temp.join("package.json");
        let character_metadata_path = temp.join("character-package.json");
        fs::copy(&character_metadata_path, &metadata_path)
            .map_err(|error| format!("Could not validate rigged Actor metadata: {error}"))?;
        let package = crate::character::package::load_package(&temp)
            .map_err(|error| format!("Generated rigged Actor source failed validation: {error}"))?;
        let character =
            crate::character::Character::from_package(1, Vec3::ZERO, std::sync::Arc::new(package));
        let vertices = crate::character::generate_character_mesh(&character);
        if vertices.is_empty() {
            return Err("Generated Character package produced no bind-pose geometry".to_string());
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for vertex in vertices.chunks_exact(10) {
            let position = Vec3::new(vertex[0], vertex[1], vertex[2]);
            min = min.min(position);
            max = max.max(position);
        }
        let size = max - min;
        if !size.is_finite() || size.y <= 0.0 {
            return Err("Generated Character bind-pose bounds are invalid".to_string());
        }
        let radius = (size.x.max(size.z) * 0.5).max(0.05);
        write_json(
            &temp.join("collision.json"),
            &serde_json::json!({
                "radius": radius,
                "height": size.y.max(radius * 2.0),
            }),
        )?;
        crate::character::package::load_package(&temp).map_err(|error| {
            format!("Generated rigged Actor source failed final validation: {error}")
        })?;
        fs::remove_file(&metadata_path)
            .map_err(|error| format!("Could not publish rigged Actor metadata: {error}"))?;
        write_json(
            &metadata_path,
            &serde_json::json!({
                "name": package_name,
                "version": 1,
                "description": format!("Rigged Actor source imported from {source_file_name}"),
                "appearance": {
                    "nodes": [{ "name": "Root" }],
                    "imported_glbs": skinned_meshes.iter().map(|mesh| serde_json::json!({
                        "source": "source.glb", "mesh": mesh, "skinned_bind_pose": true
                    })).chain(static_meshes.iter().map(|mesh|serde_json::json!({
                        "source": "source.glb", "mesh": mesh, "skinned_bind_pose": false
                    }))).collect::<Vec<_>>()
                },
                "physics": { "enabled": true, "solid": true, "anchored": true, "collision_enabled": true, "mass": 1.0 }
            }),
        )?;
        crate::engine::entity_package::load_package(&temp)
            .map_err(|error| format!("Generated Actor package failed validation: {error}"))?;
        fs::rename(&temp, &destination).map_err(|error| {
            format!(
                "Failed to publish Actor package '{}': {error}",
                destination.display()
            )
        })?;
        crate::engine::entity_package::reload_packages(Some(project_path));
        if crate::engine::entity_package::get_package(&package_name).is_none() {
            return Err(format!(
                "Imported Actor package '{package_name}' did not resolve after refresh"
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temp);
        let _ = fs::remove_dir_all(&destination);
        crate::engine::entity_package::reload_packages(Some(project_path));
        return Err(error);
    }

    Ok(CharacterGlbImportResult {
        package_name,
        source_file_name,
        mesh_names: skinned_meshes,
        joint_count: ordered_joint_indices.len(),
        clip_names,
    })
}

fn character_combat_script(package_name: &str, clip_names: &[String]) -> Option<String> {
    let has_clip = |required: &str| {
        clip_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(required))
    };
    let attack_body = if has_clip("Dagger_Attack") && has_clip("Dagger_Attack2") {
        r#"    if math.random(1, 2) == 1 {
        player.animation.play_layer("attack", "Dagger_Attack", "Torso")
    } else {
        player.animation.play_layer("attack", "Dagger_Attack2", "Torso")
    }"#
    } else if has_clip("Sword_Slash") {
        r#"    player.animation.play_layer("attack", "Sword_Slash", "Torso")"#
    } else {
        return None;
    };

    const TEMPLATE: &str = r#"// Generated by Worldkiln's GLB Character importer.
// Profile-global attack behavior. Locomotion belongs to the separately
// imported project controller package, while sword pickup/attachment belongs
// to the authored sword Actor that owns that interaction.

// The engine publishes physical keyboard transitions as the established
// unscoped `keypress` event. Keep this handler on that exact contract so one
// non-repeat F-down transition produces one attack action.
on.keypress(key, down) {
    if key != "F" || down == false {
        return
    }
    if player == nil || player.package != "__PACKAGE__" {
        return
    }
__ATTACK__
}
"#;
    Some(
        TEMPLATE
            .replace("__PACKAGE__", package_name)
            .replace("__ATTACK__", attack_body),
    )
}

fn imported_character_controller_script() -> &'static str {
    r#"// Generated by Worldkiln's GLB Character importer.
// This project controller owns locomotion only. The built-in Hero controller
// remains unchanged; sword attachment and attacks live in UserData/scripts.
entity ImportedCharacterController {
player = nil
camera = nil
input = nil
speed = 5.0
sprint_multiplier = 1.8
jump_impulse = 6.5

fn constructor(player_object, camera_object, input_system) {
    player = player_object
    camera = camera_object
    input = input_system
}

fn update(dt) {
    if player == nil { return }

    const move = input.get_move_vector()
    const basis = camera.get_horizontal_basis()
    const forward = basis[0]
    const right = basis[1]
    const move_x = forward[0] * move[1] + right[0] * move[0]
    const move_z = forward[2] * move[1] + right[2] * move[0]
    const length = math.sqrt(move_x * move_x + move_z * move_z)

    if length > 0.001 {
        const dir_x = move_x / length
        const dir_z = move_z / length
        const sprinting = input.is_key_down("ShiftLeft") || input.is_key_down("ShiftRight")
        move_speed = speed
        if sprinting { move_speed = speed * sprint_multiplier }

        player.set_horizontal_velocity(dir_x * move_speed, dir_z * move_speed)
        player.set_facing_direction(dir_x, dir_z)
        if sprinting {
            player.select_animation("Run")
        } else {
            player.select_animation("Walk")
        }
    } else {
        player.set_horizontal_velocity(0.0, 0.0)
        player.select_animation("Idle")
    }

    if input.is_jump_pressed() && player.is_grounded() {
        player.apply_vertical_impulse(jump_impulse)
    }
}
}
"#
}

fn safe_package_name(mesh_name: &str, mesh_index: usize) -> String {
    let mut output = String::new();
    let mut previous_separator = false;
    for character in mesh_name.trim().chars() {
        let mapped = if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            previous_separator = false;
            Some(character)
        } else if !previous_separator {
            previous_separator = true;
            Some('_')
        } else {
            None
        };
        if let Some(character) = mapped {
            output.push(character);
        }
    }
    let output = output.trim_matches(['.', ' ', '_']).to_string();
    let mut output = if output.is_empty() {
        format!("ImportedMesh_{}", mesh_index + 1)
    } else {
        output
    };
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if reserved
        .iter()
        .any(|reserved| output.eq_ignore_ascii_case(reserved))
    {
        output.insert(0, '_');
    }
    output
}

fn unique_import_temp(
    root: &Path,
    package_name: &str,
    index: usize,
) -> Result<std::path::PathBuf, String> {
    for attempt in 0..10_000u32 {
        let candidate = root.join(format!(
            ".{package_name}.worldkiln-glb-import-{}-{index}-{attempt}",
            std::process::id()
        ));
        if !candidate
            .try_exists()
            .map_err(|error| format!("Failed to inspect '{}': {error}", candidate.display()))?
        {
            return Ok(candidate);
        }
    }
    Err(format!(
        "Could not allocate a temporary import directory for '{package_name}'."
    ))
}

fn collision_mesh_target(name: &str) -> Option<&str> {
    let lower = name.to_ascii_lowercase();
    for prefix in ["ucx_", "ubx_", "col_", "collider_", "collision_"] {
        if lower.starts_with(prefix) {
            return Some(&name[prefix.len()..]);
        }
    }
    None
}
fn authored_glb_collider_shapes(
    source: &Path,
    visual_name: &str,
) -> Result<Vec<crate::engine::collision_geometry::CollisionShape>, String> {
    use crate::engine::collision_geometry::CollisionShape;
    let bytes = fs::read(source).map_err(|e| e.to_string())?;
    let gltf = gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
    let blob = gltf.blob.as_deref().ok_or("GLB lacks binary buffer")?;
    let mut triangles = Vec::<[[f32; 3]; 3]>::new();
    let wanted = visual_name.to_ascii_lowercase();
    for mesh in gltf.document.meshes() {
        let Some(mesh_name) = mesh.name() else {
            continue;
        };
        let Some(target) = collision_mesh_target(mesh_name) else {
            continue;
        };
        let target = target.to_ascii_lowercase();
        if wanted != "__worldkiln_scene__"
            && target != wanted
            && !target.strip_prefix(&wanted).is_some_and(|tail| {
                tail.starts_with('_') && tail[1..].chars().all(|c| c.is_ascii_digit())
            })
        {
            continue;
        }
        let source_node = gltf
            .document
            .nodes()
            .find(|n| n.mesh().is_some_and(|m| m.index() == mesh.index()));
        let transform = source_node
            .map(|n| Mat4::from_cols_array_2d(&n.transform().matrix()))
            .unwrap_or(Mat4::IDENTITY);
        let visual_node = gltf.document.nodes().find(|n| {
            n.mesh().is_some_and(|m| {
                if wanted == "__worldkiln_scene__" {
                    m.name()
                        .is_none_or(|name| collision_mesh_target(name).is_none())
                } else {
                    m.name() == Some(visual_name)
                }
            })
        });
        let visual_origin = visual_node
            .map(|n| {
                Mat4::from_cols_array_2d(&n.transform().matrix())
                    .w_axis
                    .truncate()
            })
            .unwrap_or(Vec3::ZERO);
        for primitive in mesh.primitives() {
            if primitive.mode() != Mode::Triangles {
                return Err(format!("Authored collider {mesh_name} must use triangles"));
            }
            let reader = primitive.reader(|source_buffer| match source_buffer.source() {
                buffer::Source::Bin if source_buffer.index() == 0 => Some(blob),
                _ => None,
            });
            let points = reader
                .read_positions()
                .ok_or_else(|| format!("Authored collider {mesh_name} needs positions"))?
                .map(|v| transform.transform_point3(Vec3::from_array(v)) - visual_origin)
                .collect::<Vec<_>>();
            let indices = reader
                .read_indices()
                .ok_or_else(|| format!("Authored collider {mesh_name} needs triangle indices"))?
                .into_u32()
                .map(|v| v as usize)
                .collect::<Vec<_>>();
            if indices.len() % 3 != 0 {
                return Err(format!("Collider {mesh_name} invalid indices"));
            }
            if triangles.len() + indices.len() / 3 > 20_000 {
                return Err("Imported collision mesh exceeds 20,000 triangles".into());
            }
            for tri in indices.chunks_exact(3) {
                let points = tri
                    .iter()
                    .map(|&i| {
                        points
                            .get(i)
                            .copied()
                            .ok_or_else(|| format!("Collider {mesh_name} invalid index {i}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if points.iter().any(|p| !p.is_finite()) {
                    return Err(format!("Collider {mesh_name} has non-finite vertices"));
                }
                triangles.push(
                    points
                        .iter()
                        .map(|p| p.to_array())
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap(),
                );
            }
        }
    }
    if triangles.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![CollisionShape::Triangles {
            vertices: triangles,
        }])
    }
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to encode '{}': {error}", path.display()))?;
    fs::write(path, bytes).map_err(|error| format!("Failed to write '{}': {error}", path.display()))
}

fn inspect_glb_bytes(bytes: &[u8], source_root: Option<&Path>) -> Result<GlbAssetSummary, String> {
    if !bytes.starts_with(b"glTF") {
        return Err("the file is not a binary glTF container".to_string());
    }

    let gltf = match gltf::Gltf::from_slice(bytes) {
        Ok(gltf) => gltf,
        Err(error) if error.to_string().contains("KHR_materials_unlit") => {
            gltf::Gltf::from_slice_without_validation(bytes)
                .map_err(|error| format!("glTF 2.0 parse failed: {error}"))?
        }
        Err(error) => return Err(format!("glTF 2.0 validation failed: {error}")),
    };
    let blob = gltf
        .blob
        .as_deref()
        .ok_or_else(|| "the GLB does not contain an embedded binary buffer".to_string())?;

    let required_extensions = gltf
        .document
        .extensions_required()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if required_extensions
        .iter()
        .any(|ext| ext != "KHR_materials_unlit")
    {
        return Err(format!(
            "required glTF extensions are not supported in this import slice: {}",
            required_extensions.join(", ")
        ));
    }

    for source_buffer in gltf.document.buffers() {
        match source_buffer.source() {
            buffer::Source::Bin if source_buffer.index() == 0 => {}
            buffer::Source::Bin => {
                return Err(format!(
                    "buffer {} cannot be resolved from the single GLB binary chunk",
                    source_buffer.index()
                ));
            }
            buffer::Source::Uri(uri) => {
                return Err(format!(
                    "external buffer URI '{uri}' is unsupported; embed all buffers in the GLB"
                ));
            }
        }
    }

    for node in gltf.document.nodes() {
        if node
            .transform()
            .matrix()
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err(format!(
                "node '{}' contains a non-finite transform",
                node.name().unwrap_or("unnamed")
            ));
        }
    }

    let mut embedded_image_count = 0;
    for image in gltf.document.images() {
        match image.source() {
            gltf::image::Source::View { .. } => embedded_image_count += 1,
            gltf::image::Source::Uri { uri, .. } if uri.starts_with("data:") => {
                embedded_image_count += 1;
            }
            gltf::image::Source::Uri { uri, .. } => {
                let Some(source_root) = source_root else {
                    return Err(format!(
                        "external image URI '{uri}' requires a GLB source folder"
                    ));
                };
                let synthetic_source = source_root.join("source.glb");
                safe_external_image_path(&synthetic_source, uri)?;
            }
        }
    }

    let materials = gltf
        .document
        .materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            GlbMaterialSummary {
                name: material
                    .name()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("Material {}", material.index().unwrap_or(0))),
                base_color_factor: pbr.base_color_factor(),
                has_base_color_texture: pbr.base_color_texture().is_some(),
            }
        })
        .collect::<Vec<_>>();

    let mut meshes = Vec::new();
    for mesh in gltf.document.meshes() {
        let mut primitive_count = 0;
        let mut vertex_count = 0;
        let mut index_count = 0;
        let mut bounds_min = Vec3::splat(f32::INFINITY);
        let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
        let mut material_indices = Vec::new();

        for primitive in mesh.primitives() {
            let primitive_index = primitive.index();
            if primitive.mode() != Mode::Triangles {
                return Err(format!(
                    "mesh '{}' primitive {primitive_index} uses {:?}; only triangle primitives are supported",
                    mesh.name().unwrap_or("unnamed"),
                    primitive.mode()
                ));
            }

            let reader = primitive.reader(|source_buffer| match source_buffer.source() {
                buffer::Source::Bin if source_buffer.index() == 0 => Some(blob),
                _ => None,
            });
            let positions = reader.read_positions().ok_or_else(|| {
                format!(
                    "mesh '{}' primitive {primitive_index} has no POSITION attribute",
                    mesh.name().unwrap_or("unnamed")
                )
            })?;

            let mut primitive_vertex_count = 0usize;
            for position in positions {
                let position = Vec3::from_array(position);
                if !position.is_finite() {
                    return Err(format!(
                        "mesh '{}' primitive {primitive_index} contains a non-finite vertex position",
                        mesh.name().unwrap_or("unnamed")
                    ));
                }
                primitive_vertex_count += 1;
                bounds_min = bounds_min.min(position);
                bounds_max = bounds_max.max(position);
            }
            if primitive_vertex_count == 0 {
                return Err(format!(
                    "mesh '{}' primitive {primitive_index} contains no vertices",
                    mesh.name().unwrap_or("unnamed")
                ));
            }

            let mut primitive_index_count = 0usize;
            if let Some(indices) = reader.read_indices() {
                for index in indices.into_u32() {
                    if index as usize >= primitive_vertex_count {
                        return Err(format!(
                            "mesh '{}' primitive {primitive_index} index {index} exceeds its {} vertices",
                            mesh.name().unwrap_or("unnamed"),
                            primitive_vertex_count
                        ));
                    }
                    primitive_index_count += 1;
                }
            } else {
                primitive_index_count = primitive_vertex_count;
            }
            if primitive_index_count == 0 || !primitive_index_count.is_multiple_of(3) {
                return Err(format!(
                    "mesh '{}' primitive {primitive_index} has {primitive_index_count} indices; triangle indices must be a non-zero multiple of three",
                    mesh.name().unwrap_or("unnamed")
                ));
            }

            primitive_count += 1;
            vertex_count += primitive_vertex_count;
            index_count += primitive_index_count;
            material_indices.push(primitive.material().index());
        }

        if primitive_count == 0 {
            return Err(format!(
                "mesh '{}' contains no primitives",
                mesh.name().unwrap_or("unnamed")
            ));
        }

        meshes.push(GlbMeshSummary {
            name: mesh
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("Mesh {}", mesh.index())),
            primitive_count,
            vertex_count,
            index_count,
            bounds_min,
            bounds_max,
            material_indices,
        });
    }

    if meshes.is_empty() {
        return Err("the GLB contains no mesh geometry".to_string());
    }

    let skin_joint_counts = gltf
        .document
        .skins()
        .map(|skin| skin.joints().count())
        .collect::<Vec<_>>();
    if skin_joint_counts
        .iter()
        .any(|joint_count| *joint_count == 0)
    {
        return Err("the GLB contains a skin with no joints".to_string());
    }

    let animation_names = gltf
        .document
        .animations()
        .map(|animation| {
            animation
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("Animation {}", animation.index()))
        })
        .collect::<Vec<_>>();

    Ok(GlbAssetSummary {
        kind: if skin_joint_counts.is_empty() && animation_names.is_empty() {
            GlbAssetKind::StaticMesh
        } else {
            GlbAssetKind::RiggedCharacter
        },
        meshes,
        materials,
        skin_joint_counts,
        animation_names,
        embedded_image_count,
    })
}

#[cfg(test)]
mod khr_materials_unlit_tests {
    use super::*;

    fn require_unlit(source: &Path, destination: &Path) {
        let data = fs::read(source).unwrap();
        assert_eq!(&data[0..4], b"glTF");
        let json_size = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        let mut document: serde_json::Value =
            serde_json::from_slice(&data[20..20 + json_size]).unwrap();
        document["extensionsUsed"] = serde_json::json!(["KHR_materials_unlit"]);
        document["extensionsRequired"] = serde_json::json!(["KHR_materials_unlit"]);
        let material = document["materials"]
            .as_array_mut()
            .unwrap()
            .first_mut()
            .unwrap();
        material
            .as_object_mut()
            .unwrap()
            .entry("extensions")
            .or_insert_with(|| serde_json::json!({}))["KHR_materials_unlit"] =
            serde_json::json!({});

        let mut json = serde_json::to_vec(&document).unwrap();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let remaining_chunks = &data[20 + json_size..];
        let total = 12 + 8 + json.len() + remaining_chunks.len();
        let mut output = Vec::with_capacity(total);
        output.extend_from_slice(b"glTF");
        output.extend_from_slice(&2u32.to_le_bytes());
        output.extend_from_slice(&(total as u32).to_le_bytes());
        output.extend_from_slice(&(json.len() as u32).to_le_bytes());
        output.extend_from_slice(b"JSON");
        output.extend_from_slice(&json);
        output.extend_from_slice(remaining_chunks);
        fs::write(destination, output).unwrap();
    }
}

#[cfg(test)]
mod step_animation_external_tests {
    use super::*;
}

#[cfg(test)]
mod external_animated_character_fixture_tests {
    use super::*;
    use std::sync::Arc;
}

#[cfg(test)]
mod wk_asset_002_glb_collision_tests {
    use super::*;

    fn test_root(tag: &str) -> PathBuf {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-projects")
            .join(format!(
                "WorldkilnAsset002_{tag}_{}_{}",
                std::process::id(),
                tick
            ))
    }

    #[test]
    fn failed_replace_restores_original_package() {
        let root = test_root("rollback");
        let destination = root.join(".assets/entities/Pirate");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("original.flag"), b"original").unwrap();
        let result: Result<(), String> = replace_packages(&root, &[destination.clone()], || {
            fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
            fs::write(destination.join("incoming.flag"), b"incoming").map_err(|e| e.to_string())?;
            Err("injected failure".into())
        });
        assert!(result.unwrap_err().contains("restored"));
        assert_eq!(
            fs::read(destination.join("original.flag")).unwrap(),
            b"original"
        );
        assert!(!destination.join("incoming.flag").exists());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod nonindexed_triangle_ship_contract {
    use super::*;
}
