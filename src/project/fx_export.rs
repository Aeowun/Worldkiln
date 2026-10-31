// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use flate2::read::ZlibDecoder;
use std::collections::HashSet;
use std::io::Read;

const MAX_EFFECT_BYTES: usize = 64 * 1024 * 1024;
const MAX_EDITOR_BYTES: u64 = 16 * 1024 * 1024;
pub const RUNTIME_VERSION_1800: u32 = 1810;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub tag: [u8; 4],
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EfkEfc {
    pub version: u32,
    pub chunks: Vec<Chunk>,
}

impl EfkEfc {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 8
            || bytes.len() > MAX_EFFECT_BYTES
            || bytes.get(0..4) != Some(&b"EFKE"[..])
        {
            return Err("Invalid Effekseer effect header or size".into());
        }
        let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if version != 0 {
            return Err("Unsupported EFKE container version".into());
        }
        let mut offset = 8usize;
        let mut chunks = Vec::new();
        let mut tags = HashSet::new();
        while offset < bytes.len() {
            if bytes.len() - offset < 8 || chunks.len() >= 64 {
                return Err("Truncated or excessive effect chunks".into());
            }
            let tag: [u8; 4] = bytes[offset..offset + 4].try_into().unwrap();
            let size =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            offset += 8;
            let end = offset.checked_add(size).ok_or("Effect chunk overflow")?;
            if end > bytes.len() || !tags.insert(tag) {
                return Err("Out-of-range or duplicate effect chunk".into());
            }
            chunks.push(Chunk {
                tag,
                bytes: bytes[offset..end].to_vec(),
            });
            offset = end;
        }
        let find = |name: &[u8; 4]| {
            chunks
                .iter()
                .find(|c| &c.tag == name)
                .map(|c| c.bytes.as_slice())
        };
        let info = find(b"INFO").ok_or("Missing Effekseer dependency info")?;
        let editor = find(b"EDIT").ok_or("Missing Effekseer editable document")?;
        let runtime = find(b"BIN_").ok_or("Missing Effekseer runtime binary")?;
        if info.len() < 4 || runtime.len() < 8 || runtime.get(0..4) != Some(&b"SKFE"[..]) {
            return Err("Invalid Effekseer runtime payload".into());
        }
        let info_version = u32::from_le_bytes(info[0..4].try_into().unwrap());
        let runtime_version = u32::from_le_bytes(runtime[4..8].try_into().unwrap());
        if info_version != RUNTIME_VERSION_1800 || runtime_version != RUNTIME_VERSION_1800 {
            return Err("Unsupported Effekseer runtime version".into());
        }
        Self::unpack_editor(editor)?;
        Ok(Self { version, chunks })
    }

    pub fn unpack_editor(editor: &[u8]) -> Result<Vec<u8>, String> {
        let mut decoder = ZlibDecoder::new(editor);
        let mut output = Vec::new();
        decoder
            .by_ref()
            .take(MAX_EDITOR_BYTES + 1)
            .read_to_end(&mut output)
            .map_err(|e| format!("Invalid compressed effect editor data: {e}"))?;
        if output.len() as u64 > MAX_EDITOR_BYTES || output.len() < 4 {
            return Err("Invalid size of effect editor data".into());
        }
        Ok(output)
    }

    pub fn editor_data(&self) -> Result<Vec<u8>, String> {
        let chunk = self
            .chunks
            .iter()
            .find(|c| &c.tag == b"EDIT")
            .ok_or("Missing editable effect payload")?;
        Self::unpack_editor(&chunk.bytes)
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let mut output = Vec::new();
        output.extend_from_slice(b"EFKE");
        output.extend_from_slice(&self.version.to_le_bytes());
        for chunk in &self.chunks {
            let size = u32::try_from(chunk.bytes.len()).map_err(|_| "Effect chunk too large")?;
            output.extend_from_slice(&chunk.tag);
            output.extend_from_slice(&size.to_le_bytes());
            output.extend_from_slice(&chunk.bytes);
            if output.len() > MAX_EFFECT_BYTES {
                return Err("Effect file exceeds safe limit".into());
            }
        }
        Self::parse(&output)?;
        Ok(output)
    }
}

pub fn compile_native_ring_emitter(
    name: &str,
    lifetime_frames: i32,
    emission_rate: f32,
) -> Result<Vec<u8>, String> {
    if name.is_empty()
        || name.len() > 128
        || name.trim() != name
        || name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
    {
        return Err("Unsafe native ring emitter name".into());
    }
    if !(1..=60_000).contains(&lifetime_frames)
        || !emission_rate.is_finite()
        || !(0.01..=100_000.0).contains(&emission_rate)
    {
        return Err("Invalid native ring lifetime or emission rate".into());
    }
    let reference = include_bytes!("../../assets/effekseer/archetypes/ring.efkefc");
    let effect = EfkEfc::parse(reference)?;
    let mut tree = effect.editor_tree()?;
    let project = tree
        .iter_mut()
        .find(|n| n.name == "EffekseerProject")
        .ok_or("Missing checked native ring document")?;
    let children = project
        .child_mut("Root")
        .and_then(|root| root.child_mut("Children"))
        .ok_or("Missing native ring emitter group")?;
    let nodes: Vec<_> = children
        .children
        .iter_mut()
        .filter(|node| node.name == "Node")
        .collect();
    if nodes.len() != 1 {
        return Err("Pinned native ring archetype must have exactly one emitter".into());
    }
    let node = &mut *nodes.into_iter().next().unwrap();
    if node
        .child("DrawingValues")
        .and_then(|draw| draw.child_value("Type"))
        != Some("4")
    {
        return Err("Pinned native ring renderer type mismatch".into());
    }
    let label = node
        .child_mut("Name")
        .ok_or("Missing native ring node name")?;
    label.value = Some(name.into());
    let renamed = effect.with_editor_tree(&tree)?;
    let interval = 60.0 / emission_rate;
    let patch = TemplateNodePatch {
        lifetime_frames: Some(lifetime_frames),
        generation_interval_frames: Some(interval),
    };
    let compiled = renamed.with_template_particle_settings(&[patch])?;
    let output = compiled.encode()?;
    let readback = EfkEfc::parse(&output)?;
    let editor = readback.editor_tree()?;
    let emitted = editor
        .iter()
        .find(|n| n.name == "EffekseerProject")
        .and_then(|root| root.child("Root"))
        .and_then(|root| root.child("Children"))
        .and_then(|group| group.children.iter().find(|node| node.name == "Node"))
        .ok_or("Compiled ring emitter missing from editor metadata")?;
    if emitted.child_value("Name") != Some(name)
        || emitted
            .child("DrawingValues")
            .and_then(|d| d.child_value("Type"))
            != Some("4")
    {
        return Err("Ring native authoring round-trip does not match requested renderer".into());
    }
    let common = emitted
        .child("CommonValues")
        .ok_or("Ring common values missing")?;
    let life = common
        .child("Life")
        .and_then(|value| value.child_value("Max"))
        .and_then(|value| value.parse::<i32>().ok());
    let generation = common
        .child("Generation")
        .and_then(|generation| generation.child("GenerationTime"))
        .and_then(|time| time.child_value("Max"))
        .and_then(|value| value.parse::<f32>().ok());
    if life != Some(lifetime_frames)
        || !generation.is_some_and(|n| (n - interval).abs() < (interval * 0.00001).max(0.00001))
    {
        return Err(
            "Compiled ring native lifetime or generation differs from authored values".into(),
        );
    }
    Ok(output)
}

pub fn compile_native_sprite_ring(
    sprite_name: &str,
    sprite_life_frames: i32,
    sprite_rate: f32,
    ring_name: &str,
    ring_life_frames: i32,
    ring_rate: f32,
) -> Result<Vec<u8>, String> {
    fn validated(name: &str, life: i32, rate: f32) -> Result<(), String> {
        if name.is_empty()
            || name.len() > 128
            || name.trim() != name
            || name
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'))
            || !(1..=60_000).contains(&life)
            || !rate.is_finite()
            || !(0.01..=100_000.0).contains(&rate)
        {
            return Err("Invalid native mixed emitter name/lifetime/rate".into());
        }
        Ok(())
    }
    validated(sprite_name, sprite_life_frames, sprite_rate)?;
    validated(ring_name, ring_life_frames, ring_rate)?;
    if sprite_name == ring_name {
        return Err("Mixed emitter names must be unique".into());
    }
    let fixture = include_bytes!("../../assets/effekseer/archetypes/sprite_ring.efkefc");
    let original = EfkEfc::parse(fixture)?;
    let mut tree = original.editor_tree()?;
    let root = tree
        .iter_mut()
        .find(|node| node.name == "EffekseerProject")
        .and_then(|p| p.child_mut("Root"))
        .and_then(|r| r.child_mut("Children"))
        .ok_or("Pinned mixed emitter root missing")?;
    let items: Vec<_> = root
        .children
        .iter()
        .filter(|node| node.name == "Node")
        .collect();
    let [sprite, ring] = items.as_slice() else {
        return Err("Mixed archetype has unsupported node count".into());
    };
    if !matches!(
        sprite
            .child("DrawingValues")
            .and_then(|n| n.child_value("Type")),
        None | Some("2")
    ) || ring
        .child("DrawingValues")
        .and_then(|n| n.child_value("Type"))
        != Some("4")
    {
        return Err("Mixed native renderer types changed".into());
    }
    for (index, name) in [sprite_name, ring_name].iter().enumerate() {
        let target = root
            .children
            .iter_mut()
            .filter(|node| node.name == "Node")
            .nth(index)
            .ok_or("Mixed emitter disappeared")?;
        target
            .child_mut("Name")
            .ok_or("Missing emitter name")?
            .value = Some((*name).to_owned());
    }
    let renamed = original.with_editor_tree(&tree)?;
    let requested = [
        TemplateNodePatch {
            lifetime_frames: Some(sprite_life_frames),
            generation_interval_frames: Some(60.0 / sprite_rate),
        },
        TemplateNodePatch {
            lifetime_frames: Some(ring_life_frames),
            generation_interval_frames: Some(60.0 / ring_rate),
        },
    ];
    let result = renamed.with_template_particle_settings(&requested)?;
    let bytes = result.encode()?;
    let decoded = EfkEfc::parse(&bytes)?;
    if decoded.chunks.iter().map(|c| c.tag).collect::<Vec<_>>()
        != original.chunks.iter().map(|c| c.tag).collect::<Vec<_>>()
    {
        return Err("Mixed effect native container changed format".into());
    }
    let view = decoded.editor_tree()?;
    let roots = view
        .iter()
        .find(|node| node.name == "EffekseerProject")
        .and_then(|p| p.child("Root"))
        .and_then(|r| r.child("Children"))
        .ok_or("Mixed effect lost hierarchy")?;
    let nodes: Vec<_> = roots
        .children
        .iter()
        .filter(|node| node.name == "Node")
        .collect();
    if nodes.len() != 2 {
        return Err("Mixed effect lost an emitter".into());
    }
    for (index, (name, ty, frames, rate)) in [
        (sprite_name, "2", sprite_life_frames, sprite_rate),
        (ring_name, "4", ring_life_frames, ring_rate),
    ]
    .iter()
    .enumerate()
    {
        let node = nodes[index];
        let saved_type = node
            .child("DrawingValues")
            .and_then(|d| d.child_value("Type"));
        if node.child_value("Name") != Some(*name)
            || (if *ty == "2" {
                !matches!(saved_type, None | Some("2"))
            } else {
                saved_type != Some(*ty)
            })
        {
            return Err("Mixed emitter type/name round-trip failed".into());
        }
        let common = node
            .child("CommonValues")
            .ok_or("Missing mixed emitter settings")?;
        let got_life = common
            .child("Life")
            .and_then(|s| s.child_value("Max"))
            .and_then(|v| v.parse::<i32>().ok());
        let interval = common
            .child("Generation")
            .and_then(|v| v.child("GenerationTime"))
            .and_then(|v| v.child_value("Max"))
            .and_then(|v| v.parse::<f32>().ok());
        let expected = 60.0 / *rate;
        if got_life != Some(*frames)
            || !interval.is_some_and(|v| (v - expected).abs() < (expected * 0.00001).max(0.0001))
        {
            return Err("Mixed emitter rate/lifetime round-trip failed".into());
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn official_fire() -> &'static [u8] {
        include_bytes!("../../assets/effekseer/fire.efkefc")
    }






    #[cfg(windows)]

    #[test]
    fn editor_tree_rewrites_metadata_without_touching_runtime_or_resources() {
        let source = EfkEfc::parse(official_fire()).unwrap();
        let mut tree = source.editor_tree().unwrap();
        let fire_nodes = tree
            .iter_mut()
            .find(|e| e.name == "EffekseerProject")
            .unwrap()
            .child_mut("Root")
            .unwrap()
            .child_mut("Children")
            .unwrap();
        let fire = fire_nodes
            .children
            .iter_mut()
            .filter(|e| e.name == "Node")
            .nth(1)
            .unwrap();
        let life = fire
            .child_mut("CommonValues")
            .unwrap()
            .child_mut("Life")
            .unwrap();
        life.child_mut("Center").unwrap().value = Some("90".into());
        life.child_mut("Max").unwrap().value = Some("90".into());
        life.child_mut("Min").unwrap().value = Some("90".into());
        let changed = source.with_editor_tree(&tree).unwrap();
        assert_eq!(changed.editor_tree().unwrap(), tree);
        assert_eq!(
            changed
                .chunks
                .iter()
                .find(|c| &c.tag == b"BIN_")
                .unwrap()
                .bytes,
            source
                .chunks
                .iter()
                .find(|c| &c.tag == b"BIN_")
                .unwrap()
                .bytes
        );
        assert_eq!(
            changed
                .chunks
                .iter()
                .find(|c| &c.tag == b"INFO")
                .unwrap()
                .bytes,
            source
                .chunks
                .iter()
                .find(|c| &c.tag == b"INFO")
                .unwrap()
                .bytes
        );
    }


    #[cfg(windows)]

    #[test]
    fn several_creator_sprite_layers_are_bounded_and_roundtrip() {
        let source = EfkEfc::parse(official_fire()).unwrap();
        let names = ["Creator A", "Creator B", "Creator C", "Creator D"];
        let generated = source.append_builtin_fire_sprites(&names).unwrap();
        let doc = super::super::fx_authoring::EffectDocument::from_effekseer_template(
            "created", "Created", &generated,
        )
        .unwrap();
        assert_eq!(doc.roots.len(), 9);
        for (index, name) in names.iter().enumerate() {
            assert_eq!(&doc.roots[index + 5].name, name);
        }
        let bin = &generated
            .chunks
            .iter()
            .find(|c| &c.tag == b"BIN_")
            .unwrap()
            .bytes;
        assert_eq!(bin.len(), 5672 + 972 * names.len());
        assert_eq!(i32::from_le_bytes(bin[258..262].try_into().unwrap()), 10);
        assert_eq!(i32::from_le_bytes(bin[294..298].try_into().unwrap()), 9);
        assert_eq!(
            EfkEfc::parse(&generated.encode().unwrap()).unwrap(),
            generated
        );
        assert!(
            source
                .append_builtin_fire_sprites(&["Repeat", "Repeat"])
                .is_err()
        );
        assert!(source.append_builtin_fire_sprites(&["Fire"]).is_err());
        assert!(source.append_builtin_fire_sprites(&[]).is_err());
        assert!(
            source
                .append_builtin_fire_sprites(&["unsafe/name"])
                .is_err()
        );
        assert_eq!(source.encode().unwrap(), official_fire());
    }

    #[test]
    fn runtime_template_mismatch_fails_without_patching_binary() {
        let source = EfkEfc::parse(official_fire()).unwrap();
        let mut tree = source.editor_tree().unwrap();
        let root = tree
            .iter_mut()
            .find(|e| e.name == "EffekseerProject")
            .unwrap();
        let nodes = root
            .child_mut("Root")
            .unwrap()
            .child_mut("Children")
            .unwrap();
        let smoke = nodes
            .children
            .iter_mut()
            .find(|e| e.name == "Node")
            .unwrap();
        let life = smoke
            .child_mut("CommonValues")
            .unwrap()
            .child_mut("Life")
            .unwrap();
        life.child_mut("Max").unwrap().value = Some("999".into());
        let incompatible = source.with_editor_tree(&tree).unwrap();
        let mut patches = vec![TemplateNodePatch::default(); 6];
        patches[1].lifetime_frames = Some(180);
        assert!(
            incompatible
                .with_template_particle_settings(&patches)
                .is_err()
        );
        assert_eq!(source.encode().unwrap(), official_fire());
    }

    #[test]
    fn texture_rebinding_rejects_overlapping_mappings_without_mutation() {
        let source = EfkEfc::parse(official_fire()).unwrap();
        assert!(
            rebind_textures(
                &source,
                &[
                    ("Texture/fire_tex.png", "Texture/smoke_tex.png"),
                    ("Texture/smoke_tex.png", "Texture/fire_tex.png"),
                ]
            )
            .is_err()
        );
        assert_eq!(rebind_textures(&source, &[]).unwrap(), source);
        assert_eq!(source.encode().unwrap(), official_fire());
    }

    #[test]
    fn reject_invalid_malformed_or_incompatible_efke() {
        let bytes = official_fire();
        let mut bad = bytes.to_vec();
        bad[0] = 0;
        assert!(EfkEfc::parse(&bad).is_err());
        let mut bad = bytes.to_vec();
        bad[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(EfkEfc::parse(&bad).is_err());
        let mut bad = bytes.to_vec();
        bad[4..8].copy_from_slice(&1u32.to_le_bytes());
        assert!(EfkEfc::parse(&bad).is_err());
        assert!(EfkEfc::parse(&bytes[..bytes.len() - 1]).is_err());
    }
}

pub fn rebind_textures(source: &EfkEfc, assignments: &[(&str, &str)]) -> Result<EfkEfc, String> {
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    if assignments.len() > 32 {
        return Err("Too many texture assignments".into());
    }
    for &(old, new) in assignments {
        if old != new
            && assignments
                .iter()
                .any(|&(other, _)| other != old && other == new)
        {
            return Err(
                "Overlapping effect texture mappings require simultaneous replacement".into(),
            );
        }
    }
    if assignments.is_empty() {
        return Ok(source.clone());
    }
    let mut output = source.clone();
    let edit = output
        .chunks
        .iter()
        .position(|c| &c.tag == b"EDIT")
        .ok_or("Missing editor data")?;
    let info = output
        .chunks
        .iter()
        .position(|c| &c.tag == b"INFO")
        .ok_or("Missing resource info")?;
    let compiled = output
        .chunks
        .iter()
        .position(|c| &c.tag == b"BIN_")
        .ok_or("Missing runtime data")?;
    let mut editor = EfkEfc::unpack_editor(&output.chunks[edit].bytes)?;
    let mut sources = HashSet::new();
    let mut targets = HashSet::new();

    for &(old, new) in assignments {
        if !super::fx_authoring::valid_resource_path(old)
            || !super::fx_authoring::valid_resource_path(new)
            || !sources.insert(old)
            || !targets.insert(new)
        {
            return Err("Unsafe or duplicated effect texture mapping".into());
        }
        if old == new {
            continue;
        }
        fn wide_entry(path: &str) -> Vec<u8> {
            let utf16: Vec<u16> = path.encode_utf16().collect();
            let mut result = Vec::new();
            result.extend_from_slice(&((utf16.len() + 1) as u32).to_le_bytes());
            for item in utf16 {
                result.extend_from_slice(&item.to_le_bytes());
            }
            result.extend_from_slice(&[0, 0]);
            result
        }
        let original = wide_entry(old);
        let replacement = wide_entry(new);
        for &index in &[info, compiled] {
            replace_exactly_once(&mut output.chunks[index].bytes, &original, &replacement)?;
        }
        let mut original_editor = Vec::new();
        original_editor.extend_from_slice(&(old.len() as u16).to_le_bytes());
        original_editor.extend_from_slice(old.as_bytes());
        let mut new_editor = Vec::new();
        new_editor.extend_from_slice(&(new.len() as u16).to_le_bytes());
        new_editor.extend_from_slice(new.as_bytes());
        replace_exactly_once(&mut editor, &original_editor, &new_editor)?;
    }
    if !assignments.is_empty() {
        let mut compress = ZlibEncoder::new(Vec::new(), Compression::default());
        compress.write_all(&editor).map_err(|e| e.to_string())?;
        output.chunks[edit].bytes = compress.finish().map_err(|e| e.to_string())?;
    }
    EfkEfc::parse(&output.encode()?)
}

fn replace_exactly_once(data: &mut Vec<u8>, old: &[u8], new: &[u8]) -> Result<(), String> {
    if old.is_empty() {
        return Err("Empty replacement".into());
    }
    let Some(position) = data.windows(old.len()).position(|w| w == old) else {
        return Err("Texture reference is absent in effect data".into());
    };
    if data[position + old.len()..]
        .windows(old.len())
        .any(|w| w == old)
    {
        return Err("Ambiguous effect texture reference".into());
    }
    data.splice(position..position + old.len(), new.iter().copied());
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditorElement {
    pub name: String,
    pub value: Option<String>,
    pub children: Vec<EditorElement>,
    pub has_children: bool,
}

impl EditorElement {
    pub fn child(&self, name: &str) -> Option<&Self> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn child_value(&self, name: &str) -> Option<&str> {
        self.child(name)?.value.as_deref()
    }
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Self> {
        self.children.iter_mut().find(|c| c.name == name)
    }
}

struct EditorCursor<'a> {
    raw: &'a [u8],
    position: usize,
    elements: usize,
}

impl EditorCursor<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        let end = self
            .position
            .checked_add(count)
            .ok_or("Editor offset overflow")?;
        if end > self.raw.len() {
            return Err("Truncated editor document".into());
        }
        let section = &self.raw[self.position..end];
        self.position = end;
        Ok(section)
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn dictionary(&mut self) -> Result<Vec<String>, String> {
        let count = self.u16()? as usize;
        if count > 4096 {
            return Err("Oversized editor dictionary".into());
        }
        let mut names = vec![String::new(); count];
        let mut seen = HashSet::new();
        for _ in 0..count {
            let length = self.u16()? as usize;
            if length > 4096 {
                return Err("Oversized editor token".into());
            }
            let token = std::str::from_utf8(self.take(length)?)
                .map_err(|_| "Invalid editor text")?
                .to_string();
            let index = self.u16()? as usize;
            if index >= count || !seen.insert(index) {
                return Err("Invalid editor dictionary index".into());
            }
            names[index] = token;
        }
        Ok(names)
    }
    fn elements(
        &mut self,
        keys: &[String],
        values: &[String],
        depth: usize,
    ) -> Result<Vec<EditorElement>, String> {
        if depth > 64 {
            return Err("Effect nesting exceeds safe depth".into());
        }
        let count = self.u16()? as usize;
        if self.elements.checked_add(count).is_none_or(|n| n > 65536) {
            return Err("Too many editor elements".into());
        }
        self.elements += count;
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            let key = self.u16()? as usize;
            let name = keys.get(key).ok_or("Unknown editor element name")?.clone();
            let value = if self.u32()? == 1 {
                let index = self.u16()? as usize;
                Some(
                    values
                        .get(index)
                        .ok_or("Unknown editor element value")?
                        .clone(),
                )
            } else {
                None
            };
            let has_children = match self.u32()? {
                0 => false,
                1 => true,
                _ => return Err("Invalid editor child-list flag".into()),
            };
            let children = if has_children {
                self.elements(keys, values, depth + 1)?
            } else {
                vec![]
            };
            result.push(EditorElement {
                name,
                value,
                children,
                has_children,
            });
        }
        Ok(result)
    }
}

impl EfkEfc {
    pub fn editor_tree(&self) -> Result<Vec<EditorElement>, String> {
        let raw = self.editor_data()?;
        let mut reader = EditorCursor {
            raw: &raw,
            position: 0,
            elements: 0,
        };
        let keys = reader.dictionary()?;
        let values = reader.dictionary()?;
        let roots = reader.elements(&keys, &values, 0)?;
        if reader.position != raw.len() {
            return Err("Trailing editor bytes".into());
        }
        Ok(roots)
    }
}

impl EfkEfc {
    pub fn with_editor_tree(&self, tree: &[EditorElement]) -> Result<Self, String> {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::collections::{BTreeMap, BTreeSet};
        use std::io::Write;

        fn collect(
            elements: &[EditorElement],
            depth: usize,
            total: &mut usize,
            names: &mut BTreeSet<String>,
            values: &mut BTreeSet<String>,
        ) -> Result<(), String> {
            if depth > 64 || elements.len() > u16::MAX as usize {
                return Err("Unsupported Effekseer editor tree depth or width".into());
            }
            *total = total
                .checked_add(elements.len())
                .ok_or("Editor element overflow")?;
            if *total > 65_536 {
                return Err("Too many effect editor elements".into());
            }
            for element in elements {
                if !element.has_children && !element.children.is_empty() {
                    return Err("Invalid editor child-list state".into());
                }
                names.insert(element.name.clone());
                if let Some(value) = &element.value {
                    values.insert(value.clone());
                }
                collect(&element.children, depth + 1, total, names, values)?;
            }
            Ok(())
        }
        let (mut names, mut values) = (BTreeSet::new(), BTreeSet::new());
        collect(tree, 0, &mut 0usize, &mut names, &mut values)?;
        if names.len() > 4096 || values.len() > 4096 {
            return Err("Effect editor dictionary limit exceeded".into());
        }
        fn dictionary(
            out: &mut Vec<u8>,
            words: BTreeSet<String>,
        ) -> Result<BTreeMap<String, u16>, String> {
            out.extend_from_slice(&(words.len() as u16).to_le_bytes());
            let mut indexes = BTreeMap::new();
            for (index, word) in words.into_iter().enumerate() {
                if word.len() > 4096 || word.len() > u16::MAX as usize {
                    return Err("Effect editor token too large".into());
                }
                out.extend_from_slice(&(word.len() as u16).to_le_bytes());
                out.extend_from_slice(word.as_bytes());
                out.extend_from_slice(&(index as u16).to_le_bytes());
                indexes.insert(word, index as u16);
            }
            Ok(indexes)
        }
        fn encode_elements(
            out: &mut Vec<u8>,
            elements: &[EditorElement],
            names: &BTreeMap<String, u16>,
            values: &BTreeMap<String, u16>,
        ) -> Result<(), String> {
            out.extend_from_slice(&(elements.len() as u16).to_le_bytes());
            for element in elements {
                out.extend_from_slice(&names[&element.name].to_le_bytes());
                if let Some(value) = &element.value {
                    out.extend_from_slice(&1_u32.to_le_bytes());
                    out.extend_from_slice(&values[value].to_le_bytes());
                } else {
                    out.extend_from_slice(&0_u32.to_le_bytes());
                }
                out.extend_from_slice(&(element.has_children as u32).to_le_bytes());
                if element.has_children {
                    encode_elements(out, &element.children, names, values)?;
                }
                if out.len() > MAX_EDITOR_BYTES as usize {
                    return Err("Effect editor data exceeds safe limit".into());
                }
            }
            Ok(())
        }
        let mut bytes = Vec::new();
        let name_index = dictionary(&mut bytes, std::mem::take(&mut names))?;
        let value_index = dictionary(&mut bytes, std::mem::take(&mut values))?;
        encode_elements(&mut bytes, tree, &name_index, &value_index)?;
        if bytes.len() > MAX_EDITOR_BYTES as usize {
            return Err("Effect editor data exceeds safe limit".into());
        }
        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::default());
        compressed.write_all(&bytes).map_err(|e| e.to_string())?;
        let mut result = self.clone();
        let edit = result
            .chunks
            .iter_mut()
            .find(|c| &c.tag == b"EDIT")
            .ok_or("Missing effect editor chunk")?;
        edit.bytes = compressed.finish().map_err(|e| e.to_string())?;
        let parsed = Self::parse(&result.encode()?)?;
        if parsed.editor_tree()? != tree {
            return Err("Effect editor document failed round-trip validation".into());
        }
        Ok(parsed)
    }
}

impl EfkEfc {
    pub fn append_builtin_fire_sprite(&self, name: &str) -> Result<Self, String> {
        self.append_builtin_fire_sprites(&[name])
    }

    pub fn append_builtin_fire_sprites(&self, names: &[&str]) -> Result<Self, String> {
        if names.is_empty() || names.len() > 64 {
            return Err("Invalid new emitter count".into());
        }
        let mut distinct = HashSet::new();
        for &name in names {
            if name.trim().is_empty()
                || name.len() > 128
                || name != name.trim()
                || name
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\'))
                || !distinct.insert(name)
            {
                return Err("Invalid or duplicate new emitter name".into());
            }
        }
        let original = include_bytes!("../../assets/effekseer/fire.efkefc");
        if self.encode()?.as_slice() != original {
            return Err("Emitter cloning requires the verified bundled Fire starter".into());
        }
        let mut updated = self.clone();
        let runtime = updated
            .chunks
            .iter_mut()
            .find(|c| &c.tag == b"BIN_")
            .ok_or("Missing compiled Effekseer runtime")?;
        let bytes = &mut runtime.bytes;
        if bytes.len() != 5672
            || bytes.get(258..262) != Some(&6_i32.to_le_bytes()[..])
            || bytes.get(294..298) != Some(&5_i32.to_le_bytes()[..])
            || bytes.get(1298..1302) != Some(&2_i32.to_le_bytes()[..])
            || bytes.get(1310..1314) != Some(&100_i32.to_le_bytes()[..])
            || bytes.get(2270..2274) != Some(&2_i32.to_le_bytes()[..])
        {
            return Err("Bundled Fire runtime layout does not match pinned serializer".into());
        }
        let original_node = bytes[1298..2270].to_vec();
        for index in 0..names.len() {
            let mut clone = original_node.clone();
            clone[8..12].copy_from_slice(&(6_i32 + index as i32).to_le_bytes());
            bytes.extend_from_slice(&clone);
        }
        bytes[258..262].copy_from_slice(&(6_i32 + names.len() as i32).to_le_bytes());
        bytes[294..298].copy_from_slice(&(5_i32 + names.len() as i32).to_le_bytes());

        let mut tree = updated.editor_tree()?;
        let root = tree
            .iter_mut()
            .find(|e| e.name == "EffekseerProject")
            .ok_or("Missing editable Effekseer root")?;
        let group = root
            .child_mut("Root")
            .and_then(|r| r.child_mut("Children"))
            .ok_or("Missing editable Fire emitter hierarchy")?;
        for &name in names {
            if group
                .children
                .iter()
                .filter(|e| e.name == "Node")
                .any(|e| e.child_value("Name") == Some(name))
            {
                return Err("An emitter with that name already exists".into());
            }
        }
        let fire = group
            .children
            .iter()
            .filter(|e| e.name == "Node")
            .nth(1)
            .ok_or("Missing Fire sprite template")?
            .clone();
        for &name in names {
            let mut emitter = fire.clone();
            let title = emitter
                .child_mut("Name")
                .ok_or("Missing editable emitter name")?;
            title.value = Some(name.into());
            group.children.push(emitter);
        }
        updated.with_editor_tree(&tree)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TemplateNodePatch {
    pub lifetime_frames: Option<i32>,
    pub generation_interval_frames: Option<f32>,
}

impl EfkEfc {
    pub fn with_template_node_patches(
        &self,
        patches: &[TemplateNodePatch],
    ) -> Result<Self, String> {
        if patches.len() > super::fx_authoring::MAX_NODES {
            return Err("Too many runtime emitter patches".into());
        }
        if patches.iter().all(|p| *p == TemplateNodePatch::default()) {
            return Ok(self.clone());
        }
        let mut updated = self.clone();
        let binary = updated
            .chunks
            .iter_mut()
            .find(|c| &c.tag == b"BIN_")
            .ok_or("Effekseer runtime chunk missing")?;
        if binary.bytes.get(..8).is_none_or(|x| {
            x.get(..4) != Some(&b"SKFE"[..])
                || u32::from_le_bytes(x[4..8].try_into().unwrap()) != RUNTIME_VERSION_1800
        }) {
            return Err("Unsupported Effekseer runtime for parameter editing".into());
        }
        let bytes = &mut binary.bytes;
        fn int_at(raw: &[u8], at: usize) -> i32 {
            i32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
        }
        fn float_at(raw: &[u8], at: usize) -> f32 {
            f32::from_le_bytes(raw[at..at + 4].try_into().unwrap())
        }
        let mut positions = Vec::new();
        for offset in 12..bytes.len().saturating_sub(104) {
            if bytes[offset] != 100 || bytes[offset + 1..offset + 4] != [0, 0, 0] {
                continue;
            }
            let node_type = int_at(bytes, offset - 12);
            let visible = int_at(bytes, offset - 8);
            let order = int_at(bytes, offset - 4);
            if !(0..=5).contains(&node_type)
                || !(0..=1).contains(&visible)
                || order < -1
                || order > patches.len() as i32
            {
                continue;
            }
            let refs = (0..9).map(|n| int_at(bytes, offset + 4 + n * 4));
            if !refs.clone().all(|index| (-1..=1_000_000).contains(&index)) {
                continue;
            }
            let max_gen = int_at(bytes, offset + 40);
            let life_max = int_at(bytes, offset + 64);
            let life_min = int_at(bytes, offset + 68);
            let time_max = float_at(bytes, offset + 72);
            let time_min = float_at(bytes, offset + 76);
            if max_gen < 0
                || life_max <= 0
                || life_min <= 0
                || life_max > 1_000_000
                || life_min > 1_000_000
                || !time_max.is_finite()
                || !time_min.is_finite()
                || time_max <= 0.0
                || time_min <= 0.0
                || time_max > 1_000_000.0
                || time_min > 1_000_000.0
            {
                continue;
            }
            positions.push(offset);
        }
        if positions.len() != patches.len() {
            return Err("Unsupported or ambiguous Effekseer node layout".into());
        }
        let tree = self.editor_tree()?;
        fn collect_originals(
            elements: &[EditorElement],
            out: &mut Vec<(Option<i32>, Option<i32>, Option<f32>, Option<f32>)>,
        ) {
            for node in elements.iter().filter(|e| e.name == "Node") {
                let common = node.child("CommonValues");
                let life = common.and_then(|c| c.child("Life"));
                let generation = common
                    .and_then(|c| c.child("Generation"))
                    .and_then(|c| c.child("GenerationTime"));
                out.push((
                    life.and_then(|c| c.child_value("Max"))
                        .and_then(|v| v.parse().ok()),
                    life.and_then(|c| c.child_value("Min"))
                        .and_then(|v| v.parse().ok()),
                    generation
                        .and_then(|c| c.child_value("Max"))
                        .and_then(|v| v.parse().ok()),
                    generation
                        .and_then(|c| c.child_value("Min"))
                        .and_then(|v| v.parse().ok()),
                ));
                if let Some(children) = node.child("Children") {
                    collect_originals(&children.children, out);
                }
            }
        }
        let mut expected = Vec::new();
        let project = tree
            .iter()
            .find(|e| e.name == "EffekseerProject")
            .ok_or("Effekseer metadata hierarchy missing")?;
        let group = project
            .child("Root")
            .and_then(|r| r.child("Children"))
            .ok_or("Effekseer metadata emitter list missing")?;
        collect_originals(&group.children, &mut expected);
        if expected.len() != positions.len() {
            return Err("Effekseer binary/editor emitter count mismatch".into());
        }
        for (&position, &(life_max, life_min, generation_max, generation_min)) in
            positions.iter().zip(&expected)
        {
            if life_max.is_some_and(|n| n != int_at(bytes, position + 64))
                || life_min.is_some_and(|n| n != int_at(bytes, position + 68))
                || generation_max.is_some_and(|n| {
                    (n - float_at(bytes, position + 72)).abs() > n.abs().max(1.0) * 0.00001
                })
                || generation_min.is_some_and(|n| {
                    (n - float_at(bytes, position + 76)).abs() > n.abs().max(1.0) * 0.00001
                })
            {
                return Err("Effekseer runtime and editable parameters disagree".into());
            }
        }
        for (&position, patch) in positions.iter().zip(patches) {
            if let Some(lifetime) = patch.lifetime_frames {
                if !(1..=60_000).contains(&lifetime)
                    || int_at(bytes, position + 8) != -1
                    || int_at(bytes, position + 12) != -1
                {
                    return Err("Unsupported dynamic or out-of-range particle lifetime".into());
                }
                bytes[position + 64..position + 68].copy_from_slice(&lifetime.to_le_bytes());
                bytes[position + 68..position + 72].copy_from_slice(&lifetime.to_le_bytes());
            }
            if let Some(interval) = patch.generation_interval_frames {
                if !interval.is_finite()
                    || !(0.000001..=1_000_000.0).contains(&interval)
                    || int_at(bytes, position + 16) != -1
                    || int_at(bytes, position + 20) != -1
                {
                    return Err("Unsupported dynamic or out-of-range emission interval".into());
                }
                bytes[position + 72..position + 76].copy_from_slice(&interval.to_le_bytes());
                bytes[position + 76..position + 80].copy_from_slice(&interval.to_le_bytes());
            }
        }
        Self::parse(&updated.encode()?)
    }
}

impl EfkEfc {
    pub fn with_template_particle_settings(
        &self,
        patches: &[TemplateNodePatch],
    ) -> Result<Self, String> {
        if patches.iter().all(|p| *p == TemplateNodePatch::default()) {
            return Ok(self.clone());
        }
        let runtime = self.with_template_node_patches(patches)?;
        let mut tree = self.editor_tree()?;
        fn set_triplet(
            common: &mut EditorElement,
            segments: &[&str],
            value: String,
        ) -> Result<(), String> {
            let mut current = common;
            for &segment in segments {
                current.has_children = true;
                if !current.children.iter().any(|child| child.name == segment) {
                    current.children.push(EditorElement {
                        name: segment.into(),
                        value: None,
                        children: Vec::new(),
                        has_children: true,
                    });
                }
                current = current
                    .child_mut(segment)
                    .ok_or("Missing editable effect field")?;
            }
            current.has_children = true;
            for name in ["Center", "Max", "Min"] {
                if !current.children.iter().any(|child| child.name == name) {
                    current.children.push(EditorElement {
                        name: name.into(),
                        value: Some(value.clone()),
                        children: Vec::new(),
                        has_children: false,
                    });
                }
                let value_node = current
                    .child_mut(name)
                    .ok_or("Missing editable effect numeric value")?;
                value_node.value = Some(value.clone());
            }
            Ok(())
        }
        fn update(
            group: &mut EditorElement,
            patches: &[TemplateNodePatch],
            index: &mut usize,
        ) -> Result<(), String> {
            for node in group.children.iter_mut().filter(|x| x.name == "Node") {
                let patch = *patches.get(*index).ok_or("Extra editor effect node")?;
                *index += 1;
                if patch != TemplateNodePatch::default() {
                    let common = node
                        .child_mut("CommonValues")
                        .ok_or("Missing emitter common settings")?;
                    if let Some(frames) = patch.lifetime_frames {
                        set_triplet(common, &["Life"], frames.to_string())?;
                    }
                    if let Some(interval) = patch.generation_interval_frames {
                        set_triplet(
                            common,
                            &["Generation", "GenerationTime"],
                            interval.to_string(),
                        )?;
                    }
                }
                if let Some(children) = node.child_mut("Children") {
                    update(children, patches, index)?;
                }
            }
            Ok(())
        }
        let roots = tree
            .iter_mut()
            .find(|e| e.name == "EffekseerProject")
            .ok_or("Missing Effekseer editor document root")?;
        let group = roots
            .child_mut("Root")
            .and_then(|root| root.child_mut("Children"))
            .ok_or("Missing template emitter hierarchy")?;
        let mut index = 0;
        update(group, patches, &mut index)?;
        if index != patches.len() {
            return Err("Editor/runtime node count mismatch".into());
        }
        runtime.with_editor_tree(&tree)
    }
}
