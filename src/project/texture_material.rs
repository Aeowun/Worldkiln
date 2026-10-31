// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::material::{self, MaterialDefinition, MaterialDocument, MaterialOutput};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn texture_path(project: &Path, identity: &str) -> Result<std::path::PathBuf, String> {
    if identity.starts_with("pack:") {
        return super::asset_packs::resource_path(project, identity);
    }
    let name = identity.strip_suffix(".png").unwrap_or(identity);
    let relative = format!(".assets/textures/{name}.png");
    super::asset_packs::safe_relative(&relative)?;
    let path = project.join(relative);
    for ancestor in path.ancestors() {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() {
                return Err("Linked Texture path".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Reparse Texture path".into());
                }
            }
        }
    }
    Ok(path)
}

pub fn from_texture(project: &Path, name: &str, texture: &str) -> Result<MaterialDocument, String> {
    from_maps(
        project,
        name,
        BTreeMap::from([("BaseColor".into(), texture.into())]),
    )
}

pub fn from_maps(
    project: &Path,
    name: &str,
    maps: BTreeMap<String, String>,
) -> Result<MaterialDocument, String> {
    if !maps.contains_key("BaseColor") {
        return Err("Material requires a BaseColor texture".into());
    }
    let mut dimensions = None;
    for (kind, identity) in &maps {
        if !["BaseColor", "NormalGL", "Roughness", "AO", "Height", "ARM"].contains(&kind.as_str()) {
            return Err(format!("Unsupported PBR map {kind}"));
        }
        let path = texture_path(project, identity)?;
        let reader = image::ImageReader::open(&path)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        if reader.format() != Some(image::ImageFormat::Png) {
            return Err("Material maps must be validated PNG Texture assets".into());
        }
        let size = reader.into_dimensions().map_err(|e| e.to_string())?;
        if size.0 == 0
            || size.1 == 0
            || size.0 > 16384
            || size.1 > 16384
            || dimensions.is_some_and(|existing| existing != size)
        {
            return Err("PBR map dimensions must match and remain within limits".into());
        }
        dimensions = Some(size);
    }
    let mut document = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::SurfacePbr {
            base_color: [1.0; 4],
            roughness: 0.8,
            metallic: 0.0,
        },
    });
    document.textures = maps;
    material::save(project, name, &document, None)?;
    Ok(document)
}

pub fn recognized_sets(names: &[String]) -> Vec<BTreeMap<String, String>> {
    let mut groups: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for name in names {
        let stem = name.strip_suffix(".png").unwrap_or(name);
        for kind in ["BaseColor", "NormalGL", "Roughness", "AO", "Height", "ARM"] {
            if let Some(prefix) = stem.strip_suffix(&format!("_{kind}")) {
                groups
                    .entry(prefix.into())
                    .or_default()
                    .insert(kind.into(), name.clone());
                break;
            }
        }
    }
    groups
        .into_values()
        .filter(|maps| {
            ["BaseColor", "NormalGL", "Roughness", "AO"]
                .iter()
                .all(|kind| maps.contains_key(*kind))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
}
