// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

fn strip_unlit_extension(doc: &mut serde_json::Value) {
    for key in ["extensionsRequired", "extensionsUsed"] {
        if let Some(exts) = doc.get_mut(key).and_then(serde_json::Value::as_array_mut) {
            exts.retain(|ext| ext.as_str() != Some("KHR_materials_unlit"));
        }
    }
    if let Some(materials) = doc
        .get_mut("materials")
        .and_then(serde_json::Value::as_array_mut)
    {
        for material in materials {
            if let Some(ext) = material
                .get_mut("extensions")
                .and_then(serde_json::Value::as_object_mut)
            {
                ext.remove("KHR_materials_unlit");
            }
        }
    }
}

fn append_view(
    doc: &mut serde_json::Value,
    bin: &mut Vec<u8>,
    bytes: &[u8],
) -> Result<usize, String> {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let offset = bin.len();
    bin.extend_from_slice(bytes);
    let views = doc["bufferViews"]
        .as_array_mut()
        .ok_or("Missing bufferViews")?;
    let index = views.len();
    views.push(serde_json::json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()}));
    Ok(index)
}

pub(super) fn convert_node_animation(source: &Path) -> Result<Vec<u8>, String> {
    let bytes = fs::read(source).map_err(|e| format!("Cannot read node-animated GLB: {e}"))?;
    if bytes.len() < 28
        || &bytes[..4] != b"glTF"
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 2
        || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len()
    {
        return Err("Invalid glTF 2.0 GLB".into());
    }
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json_end = 20usize.checked_add(json_len).ok_or("Invalid GLB length")?;
    if json_end + 8 > bytes.len() || &bytes[16..20] != b"JSON" {
        return Err("GLB JSON/BIN missing".into());
    }
    let bin_len = u32::from_le_bytes(bytes[json_end..json_end + 4].try_into().unwrap()) as usize;
    if &bytes[json_end + 4..json_end + 8] != b"BIN\0" || json_end + 8 + bin_len != bytes.len() {
        return Err("Node-animated GLB requires embedded binary data".into());
    }
    let mut doc: serde_json::Value =
        serde_json::from_slice(&bytes[20..json_end]).map_err(|e| e.to_string())?;
    strip_unlit_extension(&mut doc);
    let mut bin = bytes[json_end + 8..].to_vec();
    let original_meshes = doc["meshes"].as_array().ok_or("Missing meshes")?.clone();
    let original_nodes = doc["nodes"].as_array().ok_or("Missing nodes")?.clone();
    if original_nodes.is_empty()
        || original_nodes.len() > u16::MAX as usize
        || doc["skins"]
            .as_array()
            .is_some_and(|skins| !skins.is_empty())
    {
        return Err("Invalid node-animation Character graph".into());
    }
    if doc["animations"].as_array().is_none_or(Vec::is_empty) {
        return Err("Node-animated Characters require animation clips".into());
    }
    let mut names = HashSet::new();
    for i in 0..original_nodes.len() {
        let name = doc["nodes"][i]["name"].as_str().unwrap_or("");
        let base = if name.trim().is_empty() {
            format!("Node_{i}")
        } else {
            name.to_owned()
        };
        let unique = if names.insert(base.clone()) {
            base
        } else {
            format!("{base}_Node_{i}")
        };
        doc["nodes"][i]["name"] = serde_json::json!(unique);
    }
    let mut meshes = Vec::new();
    for i in 0..original_nodes.len() {
        let Some(original) = original_nodes[i]["mesh"].as_u64().map(|n| n as usize) else {
            continue;
        };
        let mut mesh = original_meshes
            .get(original)
            .ok_or("Invalid mesh node")?
            .clone();
        let count = mesh["primitives"]
            .as_array()
            .ok_or("Missing primitives")?
            .len();
        for p in 0..count {
            let position = mesh["primitives"][p]["attributes"]["POSITION"]
                .as_u64()
                .ok_or("Missing POSITION")? as usize;
            let vertices = doc["accessors"][position]["count"]
                .as_u64()
                .ok_or("Invalid vertex count")? as usize;
            if vertices == 0 || vertices > 1_000_000 {
                return Err("Node mesh exceeds vertex limit".into());
            }
            let mut joints = Vec::with_capacity(vertices * 8);
            let mut weights = Vec::with_capacity(vertices * 16);
            for _ in 0..vertices {
                for n in [i as u16, 0, 0, 0] {
                    joints.extend_from_slice(&n.to_le_bytes());
                }
                for n in [1f32, 0., 0., 0.] {
                    weights.extend_from_slice(&n.to_le_bytes());
                }
            }
            let jv = append_view(&mut doc, &mut bin, &joints)?;
            let wv = append_view(&mut doc, &mut bin, &weights)?;
            let accessors = doc["accessors"].as_array_mut().ok_or("Missing accessors")?;
            let ji = accessors.len();
            accessors.push(serde_json::json!({"bufferView":jv,"componentType":5123,"count":vertices,"type":"VEC4"}));
            let wi = accessors.len();
            accessors.push(serde_json::json!({"bufferView":wv,"componentType":5126,"count":vertices,"type":"VEC4"}));
            mesh["primitives"][p]["attributes"]["JOINTS_0"] = serde_json::json!(ji);
            mesh["primitives"][p]["attributes"]["WEIGHTS_0"] = serde_json::json!(wi);
        }
        mesh["name"] = serde_json::json!(format!(
            "{}_node_{i}",
            mesh["name"].as_str().unwrap_or("Mesh")
        ));
        doc["nodes"][i]["mesh"] = serde_json::json!(meshes.len());
        doc["nodes"][i]["skin"] = serde_json::json!(0);
        meshes.push(mesh);
    }
    if meshes.is_empty() {
        return Err("No mesh nodes in Character".into());
    }
    doc["meshes"] = serde_json::json!(meshes);
    doc["skins"] = serde_json::json!([{"joints":(0..original_nodes.len()).collect::<Vec<_>>()}]);
    let image_count = doc["images"].as_array().map_or(0, Vec::len);
    for i in 0..image_count {
        let Some(uri) = doc["images"][i]["uri"].as_str() else {
            continue;
        };
        if uri.starts_with("data:") {
            continue;
        }
        let relative = Path::new(uri);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(format!("Unsafe Character image URI: {uri}"));
        }
        let base = source
            .parent()
            .ok_or("Invalid Character source directory")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let path = base
            .join(relative)
            .canonicalize()
            .map_err(|e| format!("Character texture '{uri}' is missing: {e}"))?;
        if !path.starts_with(&base) || !path.is_file() {
            return Err("Character image escapes source folder".into());
        }
        let mime = match path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => return Err("Unsupported Character texture format".into()),
        };
        let image = fs::read(path).map_err(|e| e.to_string())?;
        if image.len() > 64 * 1024 * 1024 {
            return Err("Character texture exceeds 64MB".into());
        }
        let view = append_view(&mut doc, &mut bin, &image)?;
        doc["images"][i]
            .as_object_mut()
            .ok_or("Invalid Character image")?
            .remove("uri");
        doc["images"][i]["bufferView"] = serde_json::json!(view);
        doc["images"][i]["mimeType"] = serde_json::json!(mime);
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    doc["buffers"][0]["byteLength"] = serde_json::json!(bin.len());
    let mut json = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 20usize
        .checked_add(json.len())
        .and_then(|v| v.checked_add(8 + bin.len()))
        .ok_or("GLB size overflow")?;
    if total > 512 * 1024 * 1024 || total > u32::MAX as usize {
        return Err("Converted Character GLB exceeds limit".into());
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend(json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend(bin);
    Ok(out)
}

pub(super) fn import_node_character(
    project: &Path,
    source: &Path,
    incoming_name: Option<&str>,
) -> Result<super::CharacterGlbImportResult, String> {
    let bytes = convert_node_animation(source)?;
    let dir: PathBuf =
        std::env::temp_dir().join(format!("wk-node-animation-{:032x}", rand::random::<u128>()));
    fs::create_dir(&dir).map_err(|e| format!("Cannot stage converted Character: {e}"))?;
    let output = dir.join(
        source
            .file_name()
            .ok_or("Character source filename missing")?,
    );
    let result = (|| {
        fs::write(&output, &bytes).map_err(|e| format!("Cannot write converted Character: {e}"))?;
        let name = incoming_name.or_else(|| source.file_stem().and_then(|stem| stem.to_str()));
        super::import_character_glb_named(project, &output, name)
    })();
    let _ = fs::remove_dir_all(dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_optional_gltf_extensions_do_not_become_null() {
        let mut doc = serde_json::json!({
            "asset":{"version":"2.0"},
            "extensionsUsed":["KHR_materials_unlit","KHR_texture_transform"],
            "materials":[
                {"name":"no-extensions"},
                {"extensions":{"KHR_materials_unlit":{},"KHR_texture_transform":{}}}
            ]
        });
        strip_unlit_extension(&mut doc);
        assert!(doc.get("extensionsRequired").is_none());
        assert!(doc["materials"][0].get("extensions").is_none());
        assert_eq!(
            doc["extensionsUsed"],
            serde_json::json!(["KHR_texture_transform"])
        );
        assert!(
            doc["materials"][1]["extensions"]
                .get("KHR_materials_unlit")
                .is_none()
        );
        assert!(
            doc["materials"][1]["extensions"]
                .get("KHR_texture_transform")
                .is_some()
        );
    }
}
