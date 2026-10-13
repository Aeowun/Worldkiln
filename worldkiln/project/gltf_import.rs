use glam::{Mat4, Vec2, Vec3, Vec4};
use gltf::buffer;
use gltf::mesh::Mode;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coord: Vec2,
    pub color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbStaticPrimitive {
    pub material_name: String,
    pub material_index: Option<usize>,
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    // Keep the source image identity beside UVs so package loading can bind
    // the project-owned extracted base-color texture without reparsing it.
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
    pub position: Vec3,
    pub normal: Vec3,
    pub tex_coord: Vec2,
    pub color: [f32; 4],
    pub joints: [u16; 4],
    pub weights: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlbSkinnedPrimitive {
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
    pub controller_package_name: String,
    pub global_script_path: Option<String>,
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

/// Validates and summarizes a binary glTF before an importer is allowed to
/// create project-owned files. This function is intentionally read-only so a
/// malformed or unsupported source cannot partially mutate the asset catalog.
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

/// Loads one named static mesh into a renderer-neutral triangle stream. Source
/// node rotation and scale are baked into the returned local Actor geometry so
/// the imported package no longer depends on the original scene hierarchy.
/// Scene-layout translation is intentionally removed: an Actor's authored
/// position owns placement after import, rather than retaining the mesh's
/// staging position in a Blender/glTF asset sheet.
pub fn load_static_glb_mesh(path: &Path, mesh_name: &str) -> Result<GlbStaticMesh, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("Could not read GLB '{}': {error}", path.display()))?;
    let gltf = gltf::Gltf::from_slice(&bytes)
        .map_err(|error| format!("Invalid GLB '{}': {error}", path.display()))?;
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
    // Package-owned source.glb may retain its original relative image URI,
    // while import has normalized that image into package textures/. Geometry
    // loading therefore validates mesh/skin data without reopening the old
    // external path; texture binding resolves the staged package PNG.
    if gltf.document.skins().next().is_some() {
        return Err(format!(
            "GLB '{}' contains skins and must be imported as a Character, not a static Actor",
            path.display()
        ));
    }
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

    let referencing_nodes = gltf
        .document
        .nodes()
        .filter(|node| {
            node.mesh()
                .is_some_and(|node_mesh| node_mesh.index() == mesh.index())
        })
        .collect::<Vec<_>>();
    if referencing_nodes.len() > 1 {
        return Err(format!(
            "mesh '{mesh_name}' is instanced by {} source nodes; importing ambiguous instances as one Actor is not supported",
            referencing_nodes.len()
        ));
    }

    let mut transform = referencing_nodes
        .first()
        .map(|node| Mat4::from_cols_array_2d(&node.transform().matrix()))
        .unwrap_or(Mat4::IDENTITY);
    if !transform.is_finite() || transform.determinant().abs() <= f32::EPSILON {
        return Err(format!(
            "mesh '{mesh_name}' uses a non-finite or non-invertible source node transform"
        ));
    }
    transform.w_axis = glam::Vec4::W;
    let normal_transform = transform.inverse().transpose();

    let mut bounds_min = Vec3::splat(f32::INFINITY);
    let mut bounds_max = Vec3::splat(f32::NEG_INFINITY);
    let mut primitives = Vec::new();

    for primitive in mesh.primitives() {
        let primitive_index = primitive.index();
        if primitive.mode() != Mode::Triangles {
            return Err(format!(
                "mesh '{mesh_name}' primitive {primitive_index} uses {:?}; only indexed triangles are supported",
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
            .ok_or_else(|| {
                format!("mesh '{mesh_name}' primitive {primitive_index} is not indexed")
            })?
            .into_u32()
            .map(|index| index as usize)
            .collect::<Vec<_>>();

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
                // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
                // Real asset packs may retain zero-area helper triangles. A
                // stable fallback normal keeps the valid indexed primitive
                // importable without producing NaNs or discarding the mesh.
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
                    position,
                    normal,
                    tex_coord,
                    color,
                });
            }
        }

        primitives.push(GlbStaticPrimitive {
            material_name,
            material_index,
            base_color_texture_index,
            vertices,
        });
    }

    Ok(GlbStaticMesh {
        name: mesh_name.to_string(),
        primitives,
        bounds_min,
        bounds_max,
    })
}

/// Loads one skinned mesh while preserving its source skin joint order. The
/// Character-package loader maps these stable joint names into its Skeleton.
pub fn load_skinned_glb_mesh(path: &Path, mesh_name: &str) -> Result<GlbSkinnedMesh, String> {
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
            .ok_or_else(|| {
                format!("mesh '{mesh_name}' primitive {primitive_index} is not indexed")
            })?
            .into_u32()
            .map(|index| index as usize)
            .collect::<Vec<_>>();
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
                position: positions[index],
                normal: normals[index],
                tex_coord,
                color,
                joints: joints[index],
                weights: normalized_weights,
            });
        }
        primitives.push(GlbSkinnedPrimitive {
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

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
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

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
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

/// Imports every named mesh from a static GLB as a self-contained project
/// EntityPackage. All source validation and destination checks happen before
/// catalog mutation, and staged directories are rolled back together on error.
pub fn import_static_glb(
    project_path: &Path,
    source: &Path,
) -> Result<StaticGlbImportResult, String> {
    import_static_glb_with_package_prefix(project_path, source, None)
}

fn import_static_glb_with_package_prefix(
    project_path: &Path,
    source: &Path,
    package_prefix: Option<&str>,
) -> Result<StaticGlbImportResult, String> {
    let summary = inspect_glb(source)?;
    if summary.kind != GlbAssetKind::StaticMesh {
        return Err("Rigged GLB content must be imported as a Character package.".to_string());
    }

    let source_file_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| "Selected GLB has no valid source filename.".to_string())?
        .to_string();
    let mut package_names = HashSet::new();
    let mut planned = Vec::with_capacity(summary.meshes.len());
    for (index, mesh_summary) in summary.meshes.iter().enumerate() {
        let mesh_package_name = safe_package_name(&mesh_summary.name, index);
        let mut package_name = package_prefix
            .map(|prefix| {
                if mesh_package_name.eq_ignore_ascii_case(prefix) {
                    prefix.to_string()
                } else {
                    format!("{prefix}_{mesh_package_name}")
                }
            })
            .unwrap_or(mesh_package_name);
        if !package_names.insert(package_name.to_ascii_lowercase()) {
            if package_prefix.is_some() {
                package_name = format!("{package_name}_{}", index + 1);
                if !package_names.insert(package_name.to_ascii_lowercase()) {
                    return Err(format!(
                        "GLB mesh names collide after project-safe normalization at '{package_name}'."
                    ));
                }
            } else {
                return Err(format!(
                    "GLB mesh names collide after project-safe normalization at '{package_name}'."
                ));
            }
        }
        let mesh = load_static_glb_mesh(source, &mesh_summary.name)?;
        planned.push((package_name, mesh));
    }

    let entities_root = project_path.join(".assets").join("entities");
    for (package_name, _) in &planned {
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
        for (index, (package_name, mesh)) in planned.iter().enumerate() {
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

            let package_json = serde_json::json!({
                "name": package_name,
                "version": 1,
                "description": format!("Imported from {source_file_name} mesh {}", mesh.name),
                "appearance": {
                    "nodes": [{ "name": "Root" }],
                    "imported_glb": {
                        "source": "source.glb",
                        "mesh": mesh.name,
                    }
                },
                "physics": {
                    "enabled": true,
                    "solid": true,
                    "anchored": true,
                    "collision_enabled": true,
                    "mass": 1.0,
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

        for ((package_name, _), temp) in planned.iter().zip(staged.iter()) {
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
        for (package_name, _) in &planned {
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
        .map(|(package_name, mesh)| {
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

/// Imports every static GLB beneath a selected asset-pack folder. File stems
/// namespace secondary mesh names so shared names such as `Group` or `paddles`
/// remain deterministic project packages rather than colliding across files.
pub fn import_static_glb_folder(
    project_path: &Path,
    folder: &Path,
) -> Result<StaticGlbImportResult, String> {
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
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
    if sources.is_empty() {
        return Err("Selected asset-pack folder contains no .glb files.".to_string());
    }

    // Validate the entire pack and every destination identity before writing
    // the first package, preserving the single-file import failure contract.
    let entities_root = project_path.join(".assets").join("entities");
    let mut planned_names = HashSet::new();
    for source in &sources {
        let summary = inspect_glb(source)?;
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
        let mut source_names = HashSet::new();
        for (index, mesh) in summary.meshes.iter().enumerate() {
            let mesh_name = safe_package_name(&mesh.name, index);
            let mut package_name = if mesh_name.eq_ignore_ascii_case(&prefix) {
                prefix.clone()
            } else {
                format!("{prefix}_{mesh_name}")
            };
            if !source_names.insert(package_name.to_ascii_lowercase()) {
                package_name = format!("{package_name}_{}", index + 1);
                if !source_names.insert(package_name.to_ascii_lowercase()) {
                    return Err(format!(
                        "Asset-pack mesh names collide after project-safe normalization at '{package_name}'."
                    ));
                }
            }
            if !planned_names.insert(package_name.to_ascii_lowercase()) {
                return Err(format!(
                    "Asset-pack mesh names collide after project-safe normalization at '{package_name}'."
                ));
            }
            if entities_root.join(&package_name).exists() {
                return Err(format!(
                    "An Actor package named '{package_name}' already exists in this project; folder import will not partially overwrite the pack."
                ));
            }
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
        match import_static_glb_with_package_prefix(project_path, source, Some(&prefix)) {
            Ok(imported) => imported_packages.extend(imported.packages),
            Err(error) => {
                for package in &imported_packages {
                    let _ = fs::remove_dir_all(entities_root.join(&package.package_name));
                }
                crate::engine::entity_package::reload_packages(Some(project_path));
                return Err(error);
            }
        }
    }

    Ok(StaticGlbImportResult {
        source_file_name: folder
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("asset pack")
            .to_string(),
        packages: imported_packages,
    })
}

/// Classifies a selected folder as either one rigged Character GLB or a pack
/// of static Actor GLBs, then routes through the same transactional importers
/// used by the single-file workflow.
pub fn import_glb_folder(
    project_path: &Path,
    folder: &Path,
) -> Result<GlbFolderImportResult, String> {
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:59 -05:00
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
    if sources.is_empty() {
        return Err("Selected 3D asset folder contains no .glb files.".to_string());
    }

    let summaries = sources
        .iter()
        .map(|source| inspect_glb(source).map(|summary| (source, summary)))
        .collect::<Result<Vec<_>, _>>()?;
    let rigged = summaries
        .iter()
        .filter(|(_, summary)| summary.kind == GlbAssetKind::RiggedCharacter)
        .collect::<Vec<_>>();
    if rigged.is_empty() {
        return import_static_glb_folder(project_path, folder).map(GlbFolderImportResult::Static);
    }
    if sources.len() == 1 && rigged.len() == 1 {
        return import_character_glb(project_path, sources[0].as_path())
            .map(GlbFolderImportResult::Character);
    }

    Err(format!(
        "3D asset folder mixes {} rigged Character GLB(s) with {} total GLB files; import a Character folder containing one GLB or a static-only pack.",
        rigged.len(),
        sources.len()
    ))
}

/// Translates one rigged GLB into a self-contained project CharacterPackage.
/// The copied GLB remains the canonical project-owned vertex/skin payload;
/// rig and normalized animation metadata are generated during import.
pub fn import_character_glb(
    project_path: &Path,
    source: &Path,
) -> Result<CharacterGlbImportResult, String> {
    let summary = inspect_glb(source)?;
    if summary.kind != GlbAssetKind::RiggedCharacter {
        return Err("Static GLB content must be imported as Actor packages.".to_string());
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
    for mesh in &summary.meshes {
        if load_skinned_glb_mesh(source, &mesh.name).is_ok() {
            skinned_meshes.push(mesh.name.clone());
        }
    }
    if skinned_meshes.is_empty() {
        return Err("Rigged GLB contains no mesh node associated with a skin.".to_string());
    }
    let package_name = safe_package_name(&skinned_meshes[0], 0);
    let characters_root = project_path.join(".assets").join("characters");
    let destination = characters_root.join(&package_name);
    let controller_package_name = format!("{package_name}_Controller");
    let controllers_root = project_path.join(".assets").join("controllers");
    let controller_destination = controllers_root.join(&controller_package_name);
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
    // Projects live directly beneath the profile UserData directory. Combat
    // Modules are profile-owned so controller packages remain locomotion-only.
    let profile_scripts_root = project_path
        .parent()
        .ok_or_else(|| "Project has no UserData parent directory".to_string())?
        .join("scripts");
    let global_script_name = format!("{}_combat.aeo", package_name.to_ascii_lowercase());
    let global_script_destination = profile_scripts_root.join(&global_script_name);
    if destination
        .try_exists()
        .map_err(|error| format!("Failed to inspect '{}': {error}", destination.display()))?
    {
        return Err(format!(
            "A Character package named '{package_name}' already exists in this project; import will not overwrite or merge it."
        ));
    }
    if controller_destination.try_exists().map_err(|error| {
        format!(
            "Failed to inspect '{}': {error}",
            controller_destination.display()
        )
    })? {
        return Err(format!(
            "A controller package named '{controller_package_name}' already exists in this project; import will not overwrite or merge it."
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

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    // Attachment aliases are package metadata, not a Rogue-only runtime
    // special case. Prefer the supplied fixture's fist, then common wrist/hand
    // names used by compatible third-party rigs such as Adventurer.
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
            if channel.sampler().interpolation() != gltf::animation::Interpolation::Linear {
                return Err(format!(
                    "animation '{source_name}' uses {:?} interpolation; this import slice requires LINEAR",
                    channel.sampler().interpolation()
                ));
            }
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
                        track
                            .translation
                            .push(serde_json::json!({"time": time, "value": value}));
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
                        track
                            .rotation
                            .push(serde_json::json!({"time": time, "value": value}));
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
                        track
                            .scale
                            .push(serde_json::json!({"time": time, "value": value}));
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

    for required in ["Idle", "Walk", "Run"] {
        if !clip_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(required))
        {
            return Err(format!("rigged GLB is missing required clip '{required}'"));
        }
    }

    let combat_source = right_hand_joint
        .as_ref()
        .and_then(|_| character_combat_script(&package_name, &clip_names));
    if combat_source.is_some()
        && global_script_destination.try_exists().map_err(|error| {
            format!(
                "Failed to inspect global combat Module '{}': {error}",
                global_script_destination.display()
            )
        })?
    {
        return Err(format!(
            "A global combat Module named '{global_script_name}' already exists in UserData/scripts; import will not overwrite or merge it."
        ));
    }

    fs::create_dir_all(&characters_root).map_err(|error| {
        format!(
            "Failed to create Character asset directory '{}': {error}",
            characters_root.display()
        )
    })?;
    fs::create_dir_all(&controllers_root).map_err(|error| {
        format!(
            "Failed to create controller asset directory '{}': {error}",
            controllers_root.display()
        )
    })?;
    fs::create_dir_all(&profile_scripts_root).map_err(|error| {
        format!(
            "Failed to create profile Modules directory '{}': {error}",
            profile_scripts_root.display()
        )
    })?;
    let temp = unique_import_temp(&characters_root, &package_name, 0)?;
    let controller_temp = unique_import_temp(&controllers_root, &package_name, 1)?;
    let global_script_temp = unique_import_temp(&profile_scripts_root, &package_name, 2)?;
    fs::create_dir(&temp)
        .map_err(|error| format!("Failed to create temporary Character import: {error}"))?;
    fs::create_dir(&controller_temp)
        .map_err(|error| format!("Failed to create temporary controller import: {error}"))?;
    let result = (|| {
        let controller_source = imported_character_controller_script();
        let tokens = crate::scripting::lexer::Lexer::new(controller_source)
            .tokenize()
            .map_err(|error| {
                format!(
                    "Generated controller lexer error at {}:{}: {}",
                    error.line, error.column, error.message
                )
            })?;
        crate::scripting::parser::Parser::new(tokens)
            .parse()
            .map_err(|error| {
                format!(
                    "Generated controller parser error at {}:{}: {}",
                    error.line, error.column, error.message
                )
            })?;
        fs::write(
            controller_temp.join("characterController.aeo"),
            controller_source,
        )
        .map_err(|error| format!("Failed to stage imported Character controller: {error}"))?;
        write_json(
            &controller_temp.join("package.json"),
            &serde_json::json!({
                "name": controller_package_name,
                "type": "third_person"
            }),
        )?;

        if let Some(combat_source) = combat_source.as_deref() {
            let tokens = crate::scripting::lexer::Lexer::new(combat_source)
                .tokenize()
                .map_err(|error| {
                    format!(
                        "Generated global combat Module lexer error at {}:{}: {}",
                        error.line, error.column, error.message
                    )
                })?;
            crate::scripting::parser::Parser::new(tokens)
                .parse()
                .map_err(|error| {
                    format!(
                        "Generated global combat Module parser error at {}:{}: {}",
                        error.line, error.column, error.message
                    )
                })?;
            fs::write(&global_script_temp, combat_source)
                .map_err(|error| format!("Failed to stage global combat Module: {error}"))?;
        }
        fs::copy(source, temp.join("source.glb"))
            .map_err(|error| format!("Failed to copy GLB into Character package: {error}"))?;
        let attachment_rotations = right_hand_joint
            .as_deref()
            // Quaternius Fist bones point props sideways relative to the
            // Worldkiln Grip frame. Store that compatibility in the imported
            // package rather than special-casing AttachmentSystem.
            .filter(|joint| joint.eq_ignore_ascii_case("Fist.R"))
            .map(|_| {
                serde_json::json!({
                    "RightHand": [-std::f32::consts::FRAC_1_SQRT_2, 0.0, 0.0, std::f32::consts::FRAC_1_SQRT_2]
                })
            })
            .unwrap_or_else(|| serde_json::json!({}));
        write_json(
            &temp.join("package.json"),
            &serde_json::json!({
                "name": package_name,
                "mesh_type": "imported_glb",
                "default_animation": "Idle",
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
                "default_state": "Idle",
                "blend_speed": 4.0,
                "idle_clip": "Idle",
                "walk_clip": "Walk",
                "run_clip": "Run",
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
                "meshes": skinned_meshes,
                "joint_count": ordered_joint_indices.len(),
                "clips": clip_names,
            }),
        )?;

        let package = crate::character::package::load_package(&temp)
            .map_err(|error| format!("Generated Character package failed validation: {error}"))?;
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
            format!("Generated Character package failed final validation: {error}")
        })?;
        fs::rename(&temp, &destination).map_err(|error| {
            format!(
                "Failed to publish Character package '{}': {error}",
                destination.display()
            )
        })?;
        fs::rename(&controller_temp, &controller_destination).map_err(|error| {
            format!(
                "Failed to publish controller package '{}': {error}",
                controller_destination.display()
            )
        })?;
        if combat_source.is_some() {
            fs::rename(&global_script_temp, &global_script_destination).map_err(|error| {
                format!(
                    "Failed to publish global combat Module '{}': {error}",
                    global_script_destination.display()
                )
            })?;
        }
        crate::character::package::reload_packages(Some(project_path));
        if crate::character::package::get_package(&package_name).is_none() {
            return Err(format!(
                "Imported Character package '{package_name}' did not resolve after refresh"
            ));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temp);
        let _ = fs::remove_dir_all(&destination);
        let _ = fs::remove_dir_all(&controller_temp);
        let _ = fs::remove_dir_all(&controller_destination);
        let _ = fs::remove_file(&global_script_temp);
        if combat_source.is_some() {
            let _ = fs::remove_file(&global_script_destination);
        }
        crate::character::package::reload_packages(Some(project_path));
        return Err(error);
    }

    Ok(CharacterGlbImportResult {
        package_name,
        source_file_name,
        mesh_names: skinned_meshes,
        joint_count: ordered_joint_indices.len(),
        clip_names,
        controller_package_name,
        global_script_path: combat_source.map(|_| format!("scripts/{global_script_name}")),
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
// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
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
// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
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

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("Failed to encode '{}': {error}", path.display()))?;
    fs::write(path, bytes).map_err(|error| format!("Failed to write '{}': {error}", path.display()))
}

fn inspect_glb_bytes(bytes: &[u8], source_root: Option<&Path>) -> Result<GlbAssetSummary, String> {
    if !bytes.starts_with(b"glTF") {
        return Err("the file is not a binary glTF container".to_string());
    }

    let gltf = gltf::Gltf::from_slice(bytes)
        .map_err(|error| format!("glTF 2.0 validation failed: {error}"))?;
    let blob = gltf
        .blob
        .as_deref()
        .ok_or_else(|| "the GLB does not contain an embedded binary buffer".to_string())?;

    let required_extensions = gltf
        .document
        .extensions_required()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !required_extensions.is_empty() {
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
                // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
                // A pack may share a PNG beside several GLBs. Accept only a
                // resolvable, confined relative file; package staging copies
                // it inward so reopening never depends on this source path.
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
                    "mesh '{}' primitive {primitive_index} uses {:?}; only indexed triangles are supported",
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

            let indices = reader.read_indices().ok_or_else(|| {
                format!(
                    "mesh '{}' primitive {primitive_index} is not indexed",
                    mesh.name().unwrap_or("unnamed")
                )
            })?;
            let mut primitive_index_count = 0usize;
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
        kind: if skin_joint_counts.is_empty() {
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

// Worldkiln 0.8.5 | WO MAT-011 | 2026-10-10 14:50 -05:00
// Explicit opt-in test for the user's real animated GLB. Fixture path is an
// environment input, never a hard-coded dependency in normal engine tests.
#[cfg(test)]
mod external_animated_character_fixture_tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    #[ignore = "requires WORLDKILN_ANIMATED_GLB_DIR on the actual Windows disk"]
    fn imported_animated_character_preserves_clips_and_generates_pose() {
        let folder = std::env::var_os("WORLDKILN_ANIMATED_GLB_DIR")
            .expect("set WORLDKILN_ANIMATED_GLB_DIR to the animated GLB folder");
        let folder = PathBuf::from(folder);
        let source = folder.join("model.glb");
        let summary = inspect_glb(&source).expect("the source must be a valid rigged GLB");
        assert_eq!(summary.kind, GlbAssetKind::RiggedCharacter);
        assert_eq!(summary.animation_names.len(), 24);
        assert!(!summary.skin_joint_counts.is_empty());

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "WorldkilnAnimatedCharacterFixture_{}_{}",
            std::process::id(),
            unique
        ));
        let project = root.join("FixtureProject");
        fs::create_dir_all(&project).expect("create isolated project");
        let result = (|| {
            let imported = import_glb_folder(&project, &folder)
                .expect("the real animated GLB folder must import");
            let GlbFolderImportResult::Character(imported) = imported else {
                panic!("a rigged GLB must import as a Character");
            };
            assert!(imported.joint_count > 0);
            for name in ["Idle", "Walk", "Run", "Dagger_Attack", "Death"] {
                assert!(
                    imported
                        .clip_names
                        .iter()
                        .any(|clip| clip.eq_ignore_ascii_case(name)),
                    "missing imported clip {name}"
                );
            }
            let package_dir = project
                .join(".assets")
                .join("characters")
                .join(&imported.package_name);
            let package = Arc::new(
                crate::character::package::load_package(&package_dir)
                    .expect("imported package must load"),
            );
            assert!(package.skeleton.joints.len() > 0);
            let character =
                crate::character::Character::from_package(1, Vec3::ZERO, package.clone());
            let mesh = crate::character::generate_character_mesh(&character);
            assert!(
                !mesh.is_empty(),
                "imported Character must have drawable geometry"
            );
            assert!(mesh.iter().all(|value| value.is_finite()));
            assert!(package.clip_named("Walk").is_some());
            // Worldkiln 0.8.5 | WO MAT-011 | 2026-10-10 14:52 -05:00
            // Drive imported joint poses through Walk and Run; import alone is insufficient.
            let mut animator =
                crate::character::CharacterAnimationController::from_package(&package);
            let idle = animator.evaluate_pose();
            animator.select_animation(crate::character::TargetAnimation::Walk);
            animator.update(0.25);
            let walk = animator.evaluate_pose();
            assert_eq!(walk.matrices.len(), imported.joint_count);
            assert!(walk.matrices.iter().all(|matrix| matrix.is_finite()));
            assert!(
                walk.matrices.iter().zip(&idle.matrices).any(|(a, b)| {
                    a.to_cols_array()
                        .iter()
                        .zip(b.to_cols_array().iter())
                        .any(|(x, y)| (x - y).abs() > 0.00001)
                }),
                "Walk animation must move at least one joint"
            );
            animator.select_animation(crate::character::TargetAnimation::Run);
            animator.update(0.25);
            let run = animator.evaluate_pose();
            assert!(run.matrices.iter().all(|matrix| matrix.is_finite()));
            assert_eq!(animator.current_clip_name(), "Run");
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:15 -05:00
            // An explicit opt-in materializes the verified GLB as the engine's
            // real default package. Never overwrite an existing package.
            if std::env::var_os("WORLDKILN_INSTALL_DEFAULT_CHARACTER").is_some() {
                assert_eq!(imported.package_name, "Rogue");
                let destination = crate::project::engine_assets_dir()
                    .join("characters")
                    .join("Rogue");
                assert!(
                    !destination.exists(),
                    "refusing to overwrite existing Rogue package"
                );
                fs::create_dir_all(destination.parent().unwrap())
                    .expect("create engine character asset directory");
                fs::copy(
                    folder.join("LICENSE-AND-ATTRIBUTION.txt"),
                    package_dir.join("LICENSE-AND-ATTRIBUTION.txt"),
                )
                .expect("preserve GLB attribution");
                fs::copy(
                    folder.join("poly-pizza-metadata.json"),
                    package_dir.join("poly-pizza-metadata.json"),
                )
                .expect("preserve original asset metadata");
                fs::rename(&package_dir, &destination)
                    .expect("publish fully validated Rogue package");
                println!(
                    "Installed default Character package: {}",
                    destination.display()
                );
            }
            println!(
                "Fixture imported: {} joints, {} unique clips, {} mesh floats",
                imported.joint_count,
                imported.clip_names.len(),
                mesh.len()
            );
        })();
        fs::remove_dir_all(&root).expect("remove isolated test project");
        result
    }
}
