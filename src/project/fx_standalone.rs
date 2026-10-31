// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::fx_authoring::{EffectDocument, EffectNode};
use super::fx_export::{EditorElement, EfkEfc, TemplateNodePatch, rebind_textures};
use std::collections::HashSet;

const ARCHETYPE: &[u8] = include_bytes!("../../assets/effekseer/fire.efkefc");
const HEADER_BYTES: usize = 298;
const SPRITE_BEGIN: usize = 1298;
const SPRITE_END: usize = 2270;
const MAX_SPRITES: usize = 64;
const SIZE_CURVE_SAMPLES: usize = 101;

const SCALE_START: usize = 480;
const SCALE_END: usize = 584;
const COLOR_START: usize = 912;
const COLOR_END: usize = 920;

const SPAWN_BEGIN: usize = 584;
const SPAWN_END: usize = 616;
const MOTION_START: usize = 180;
const ACCEL_START: usize = 204;
const LFF_TYPE: usize = 232;
const LFF_POWER: usize = 236;
fn encoded_spawn(node: &EffectNode) -> Result<Vec<u8>, String> {
    use super::fx_authoring::SpawnShape;
    let mut out = Vec::with_capacity(80);
    let mut i32val = |value: i32| out.extend_from_slice(&value.to_le_bytes());
    i32val(0);
    i32val(match node.spawn_shape {
        SpawnShape::Point => 0,
        SpawnShape::Sphere => 1,
        SpawnShape::Circle => 3,
        SpawnShape::Line => 4,
    });
    drop(i32val);
    let mut float = |value: f32| out.extend_from_slice(&value.to_le_bytes());
    let r = node.spawn_radius;
    match node.spawn_shape {
        SpawnShape::Point => {
            for _ in 0..6 {
                float(0.0);
            }
        }
        SpawnShape::Sphere => {
            for value in [
                r,
                r,
                std::f32::consts::TAU,
                0.0,
                std::f32::consts::FRAC_PI_2,
                -std::f32::consts::FRAC_PI_2,
            ] {
                float(value);
            }
        }
        SpawnShape::Circle => {
            drop(float);
            out.extend_from_slice(&16_i32.to_le_bytes());
            let mut float = |value: f32| out.extend_from_slice(&value.to_le_bytes());
            for value in [r, r, 0.0, 0.0, std::f32::consts::TAU, std::f32::consts::TAU] {
                float(value);
            }
            drop(float);
            out.extend_from_slice(&0_i32.to_le_bytes());
            out.extend_from_slice(&1_i32.to_le_bytes());
            out.extend_from_slice(&0_f32.to_le_bytes());
            out.extend_from_slice(&0_f32.to_le_bytes());
        }
        SpawnShape::Line => {
            drop(float);
            out.extend_from_slice(&16_i32.to_le_bytes());
            let mut float = |value: f32| out.extend_from_slice(&value.to_le_bytes());
            for value in [
                -r, 0.0, 0.0, -r, 0.0, 0.0, r, 0.0, 0.0, r, 0.0, 0.0, 0.0, 0.0,
            ] {
                float(value);
            }
            drop(float);
            out.extend_from_slice(&0_i32.to_le_bytes());
        }
    }
    Ok(out)
}
fn shape_delta(node: &EffectNode) -> Result<usize, String> {
    encoded_spawn(node)?
        .len()
        .checked_sub(SPAWN_END - SPAWN_BEGIN)
        .ok_or_else(|| "Short spawn record".to_string())
}
fn encoded_motion_base(base: &[u8], node: &EffectNode) -> Result<Vec<u8>, String> {
    if base.len() != SCALE_START - 12
        || base.get(MOTION_START - 12..MOTION_START - 12 + 24) != Some(&[0u8; 24][..])
        || base.get(LFF_TYPE - 12..LFF_TYPE - 8) != Some(&0_i32.to_le_bytes()[..])
        || base.get(LFF_POWER - 12..LFF_POWER - 8) != Some(&1_f32.to_le_bytes()[..])
    {
        return Err("Pinned Fire motion and local force field changed".into());
    }
    let mut output = base.to_vec();
    for (offset, vector) in [
        (MOTION_START, node.velocity),
        (ACCEL_START, node.acceleration),
    ] {
        for component in 0..3 {
            for base in [0_usize, 12] {
                let position = offset - 12 + base + component * 4;
                output[position..position + 4].copy_from_slice(&vector[component].to_le_bytes());
            }
        }
    }
    if node.drag > 0.0 {
        output[LFF_TYPE - 12..LFF_TYPE - 8].copy_from_slice(&7_i32.to_le_bytes());
        output[LFF_POWER - 12..LFF_POWER - 8].copy_from_slice(&node.drag.to_le_bytes());
    }
    Ok(output)
}
struct PinnedFireSprite<'a> {
    initial: &'a [u8],
    properties_before_scale: &'a [u8],
    fixed_scale: &'a [u8],
    properties_before_color: &'a [u8],
    fixed_color: &'a [u8],
    trailing: &'a [u8],
}

impl<'a> PinnedFireSprite<'a> {
    fn decode(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() != SPRITE_END - SPRITE_BEGIN
            || bytes.get(0..4) != Some(&2_i32.to_le_bytes()[..])
            || bytes.get(12..16) != Some(&100_i32.to_le_bytes()[..])
            || bytes.get(SCALE_START..SCALE_START + 8) != Some(&[1, 0, 0, 0, 96, 0, 0, 0][..])
            || bytes.get(SPAWN_BEGIN..SPAWN_END) != Some(&[0_u8; 32][..])
            || bytes.get(COLOR_START - 12..COLOR_END)
                != Some(
                    &[
                        2, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255,
                    ][..],
                )
        {
            return Err("Unsupported pinned Effekseer sprite record".into());
        }
        Ok(Self {
            initial: &bytes[..8],
            properties_before_scale: &bytes[12..SCALE_START],
            fixed_scale: &bytes[SCALE_START..SCALE_END],
            properties_before_color: &bytes[SCALE_END..COLOR_START],
            fixed_color: &bytes[COLOR_START..COLOR_END],
            trailing: &bytes[COLOR_END..],
        })
    }

    fn encode(&self, node: &EffectNode, render_index: usize) -> Result<Vec<u8>, String> {
        let scale = if node.size.is_empty() {
            self.fixed_scale.to_vec()
        } else {
            encoded_size_curve(&node.size)?
        };
        let color = if !has_rgba_keys(node) {
            self.fixed_color.to_vec()
        } else {
            encoded_rgba_curves(node)?
        };
        let motion = encoded_motion_base(self.properties_before_scale, node)?;
        let spawn = encoded_spawn(node)?;
        let mut post_scale = Vec::with_capacity(self.properties_before_color.len() + spawn.len());
        post_scale.extend_from_slice(&spawn);
        post_scale.extend_from_slice(&self.properties_before_color[SPAWN_END - SCALE_END..]);
        let size = self.initial.len()
            + 4
            + motion.len()
            + scale.len()
            + post_scale.len()
            + color.len()
            + self.trailing.len();
        let mut output = Vec::with_capacity(size);
        output.extend_from_slice(self.initial);
        output.extend_from_slice(&(render_index as i32).to_le_bytes());
        output.extend_from_slice(&motion);
        output.extend_from_slice(&scale);
        output.extend_from_slice(&post_scale);
        output.extend_from_slice(&color);
        output.extend_from_slice(self.trailing);
        if output.len() != size {
            return Err("AeoFX sprite serialization size mismatch".into());
        }
        Ok(output)
    }
}

fn fresh_runtime_header(archetype: &[u8], count: usize) -> Result<Vec<u8>, String> {
    if archetype.len() < HEADER_BYTES
        || count == 0
        || count > MAX_SPRITES
        || archetype.get(..4) != Some(&b"SKFE"[..])
        || archetype.get(4..8) != Some(&1810_u32.to_le_bytes()[..])
        || archetype.get(258..262) != Some(&6_i32.to_le_bytes()[..])
        || archetype.get(294..298) != Some(&5_i32.to_le_bytes()[..])
    {
        return Err("Unsupported pinned Effekseer runtime header".into());
    }
    let mut output = Vec::with_capacity(HEADER_BYTES + count * 3000);
    output.extend_from_slice(&archetype[..258]);
    output.extend_from_slice(&(count as i32).to_le_bytes());
    output.extend_from_slice(&archetype[262..294]);
    output.extend_from_slice(&(count as i32).to_le_bytes());
    Ok(output)
}

fn encoded_size_curve(keys: &[super::fx_authoring::CurveKey]) -> Result<Vec<u8>, String> {
    if keys.is_empty()
        || keys
            .iter()
            .any(|key| !key.value.is_finite() || !(0.0..=1000.0).contains(&key.value))
    {
        return Err("Invalid size curve".into());
    }
    let mut body = Vec::with_capacity(440);
    body.extend_from_slice(&1_i32.to_le_bytes());
    for value in [0_i32, 0] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_f32, 0_f32] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0_i32, 100, 1, SIZE_CURVE_SAMPLES as i32] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    let mut segment = 0usize;
    for frame in 0..SIZE_CURVE_SAMPLES {
        let t = frame as f32 / 100.0;
        while segment + 1 < keys.len() && keys[segment + 1].time < t {
            segment += 1;
        }
        let value = if t <= keys[0].time {
            keys[0].value
        } else if segment + 1 == keys.len() {
            keys[segment].value
        } else {
            let a = &keys[segment];
            let b = &keys[segment + 1];
            let weight = ((t - a.time) / (b.time - a.time)).clamp(0.0, 1.0);
            a.value + (b.value - a.value) * weight
        };
        body.extend_from_slice(&value.to_le_bytes());
    }
    let mut result = Vec::with_capacity(8 + body.len());
    result.extend_from_slice(&6_i32.to_le_bytes());
    result.extend_from_slice(&(body.len() as i32).to_le_bytes());
    result.extend_from_slice(&body);
    Ok(result)
}

fn has_rgba_keys(node: &EffectNode) -> bool {
    !node.opacity.is_empty()
        || !node.color_r.is_empty()
        || !node.color_g.is_empty()
        || !node.color_b.is_empty()
}
fn encoded_rgba_curves(node: &EffectNode) -> Result<Vec<u8>, String> {
    let curves = [&node.color_r, &node.color_g, &node.color_b, &node.opacity];
    if curves.iter().any(|curve| {
        curve
            .iter()
            .any(|key| !key.value.is_finite() || !(0.0..=1.0).contains(&key.value))
    }) {
        return Err("RGB and opacity values must be normalized".into());
    }
    let mut body = Vec::with_capacity(4 + 4 * 436);
    body.extend_from_slice(&3_i32.to_le_bytes());
    body.extend_from_slice(&1_i32.to_le_bytes());
    for keys in curves {
        for number in [0_i32, 0] {
            body.extend_from_slice(&number.to_le_bytes());
        }
        for number in [0_f32, 0_f32] {
            body.extend_from_slice(&number.to_le_bytes());
        }
        for number in [0_i32, 100, 1, SIZE_CURVE_SAMPLES as i32] {
            body.extend_from_slice(&number.to_le_bytes());
        }
        let mut segment = 0usize;
        for frame in 0..SIZE_CURVE_SAMPLES {
            let t = frame as f32 / 100.0;
            let value = if keys.is_empty() {
                1.0
            } else {
                while segment + 1 < keys.len() && keys[segment + 1].time < t {
                    segment += 1;
                }
                if t <= keys[0].time {
                    keys[0].value
                } else if segment + 1 == keys.len() {
                    keys[segment].value
                } else {
                    let a = &keys[segment];
                    let b = &keys[segment + 1];
                    let ratio = ((t - a.time) / (b.time - a.time)).clamp(0.0, 1.0);
                    a.value + (b.value - a.value) * ratio
                }
            };
            body.extend_from_slice(&(value * 255.0).to_le_bytes());
        }
    }
    Ok(body)
}
pub struct AeoFxCompiler;

impl AeoFxCompiler {
    pub fn export(document: &EffectDocument) -> Result<Vec<u8>, String> {
        if document
            .roots
            .iter()
            .any(|n| n.kind == super::fx_authoring::EmitterKind::Ring)
        {
            compile_sprites_with_ring(document)
        } else if {
            let mut pending: Vec<_> = document.roots.iter().collect();
            let mut has = false;
            while let Some(n) = pending.pop() {
                if matches!(
                    n.kind,
                    super::fx_authoring::EmitterKind::Ribbon
                        | super::fx_authoring::EmitterKind::Track
                ) {
                    has = true;
                }
                pending.extend(n.children.iter());
            }
            has
        } {
            compile_sprites_with_trails(document)
        } else {
            compile_standalone_sprites(document)
        }
    }
}

fn flatten_authored_nodes<'a>(
    nodes: &'a [EffectNode],
    parent: Option<u64>,
    flat: &mut Vec<(&'a EffectNode, Option<u64>)>,
) {
    for node in nodes {
        flat.push((node, parent));
        flatten_authored_nodes(&node.children, Some(node.id), flat);
    }
}

pub fn compile_standalone_sprites(document: &EffectDocument) -> Result<Vec<u8>, String> {
    document.validate()?;
    let mut flat = Vec::new();
    flatten_authored_nodes(&document.roots, None, &mut flat);
    if flat.is_empty() || flat.len() > MAX_SPRITES {
        return Err("Standalone effects require 1 to 64 sprite roots".into());
    }
    if flat
        .iter()
        .any(|(node, _)| node.kind != super::fx_authoring::EmitterKind::Sprite)
    {
        return Err("Sprite-only compiler cannot erase a non-Sprite renderer".into());
    }
    let mut seen = HashSet::new();
    let mut shared_texture: Option<&str> = None;
    for (node, _parent) in &flat {
        if !seen.insert(node.name.as_str())
            || node.name != node.name.trim()
            || node.name.chars().any(|c| c.is_control())
        {
            return Err("Unsafe or duplicated standalone sprite name".into());
        }
        if !node.enabled {
            return Err("Disabled sprite runtime serialization not yet supported".into());
        }
        let texture = node.texture.as_deref().unwrap_or("Texture/fire_tex.png");
        if !super::fx_authoring::valid_resource_path(texture) {
            return Err("Invalid project-owned effect texture path".into());
        }
        if shared_texture.is_some_and(|existing| existing != texture) {
            return Err("Independent sprite textures require a resource-index writer".into());
        }
        shared_texture = Some(texture);
        if node.emission_rate.is_some_and(|rate| rate <= 0.0) {
            return Err("Standalone sprite emission rate must be positive".into());
        }
    }

    let starter = EfkEfc::parse(ARCHETYPE)?;
    let archetype = starter
        .chunks
        .iter()
        .find(|c| &c.tag == b"BIN_")
        .ok_or("Bundled effect runtime missing")?
        .bytes
        .as_slice();
    if archetype.len() != 5672
        || archetype.get(258..262) != Some(&6_i32.to_le_bytes()[..])
        || archetype.get(294..298) != Some(&5_i32.to_le_bytes()[..])
        || archetype.get(SPRITE_BEGIN + 12..SPRITE_BEGIN + 16) != Some(&100_i32.to_le_bytes()[..])
    {
        return Err("Pinned sprite layout changed; cannot safely author effect".into());
    }
    let sprite = PinnedFireSprite::decode(
        archetype
            .get(SPRITE_BEGIN..SPRITE_END)
            .ok_or("Bundled Fire sprite archetype is truncated")?,
    )?;
    let mut native = fresh_runtime_header(archetype, flat.len())?;
    for (index, (node, _parent)) in flat.iter().enumerate() {
        native.extend_from_slice(&sprite.encode(node, index)?);
    }
    let mut compiled = starter.clone();
    compiled
        .chunks
        .iter_mut()
        .find(|chunk| &chunk.tag == b"BIN_")
        .ok_or("Compiled runtime destination missing")?
        .bytes = native;

    let mut tree = starter.editor_tree()?;
    let project = tree
        .iter_mut()
        .find(|e| e.name == "EffekseerProject")
        .ok_or("Bundled editor root missing")?;
    let group = project
        .child_mut("Root")
        .and_then(|r| r.child_mut("Children"))
        .ok_or("Bundled emitter list missing")?;
    let archetype_node = group
        .children
        .iter()
        .filter(|n| n.name == "Node")
        .nth(1)
        .ok_or("Bundled Fire sprite metadata missing")?
        .clone();
    group.children.clear();
    group.has_children = true;
    for (node, parent) in &flat {
        let mut emitter: EditorElement = archetype_node.clone();
        emitter
            .child_mut("Name")
            .ok_or("Sprite name field missing")?
            .value = Some(node.name.clone());
        emitter.children.push(EditorElement {
            name: "AeoFXStableId".into(),
            value: Some(node.id.to_string()),
            has_children: false,
            children: vec![],
        });
        if let Some(parent_id) = parent {
            emitter.children.push(EditorElement {
                name: "AeoFXParentId".into(),
                value: Some(parent_id.to_string()),
                has_children: false,
                children: vec![],
            });
        }
        let mut scalar = |name: &str, value: String| {
            emitter.children.push(EditorElement {
                name: name.into(),
                value: Some(value),
                has_children: false,
                children: vec![],
            });
        };
        if node.spawn_shape != super::fx_authoring::SpawnShape::Point {
            scalar("AeoFXSpawnShape", format!("{:?}", node.spawn_shape));
        }
        if node.spawn_radius != 0.0 {
            scalar("AeoFXSpawnRadius", node.spawn_radius.to_string());
        }
        for (prefix, vector) in [
            ("Velocity", node.velocity),
            ("Acceleration", node.acceleration),
        ] {
            for (axis, value) in ["X", "Y", "Z"].iter().zip(vector) {
                if value != 0.0 {
                    scalar(&format!("AeoFX{prefix}{axis}"), value.to_string());
                }
            }
        }
        if node.drag != 0.0 {
            scalar("AeoFXDrag", node.drag.to_string());
        }
        if !node.size.is_empty() {
            let mut points = Vec::with_capacity(node.size.len());
            for point in &node.size {
                points.push(EditorElement {
                    name: "Key".into(),
                    value: None,
                    has_children: true,
                    children: vec![
                        EditorElement {
                            name: "Time".into(),
                            value: Some(point.time.to_string()),
                            has_children: false,
                            children: vec![],
                        },
                        EditorElement {
                            name: "Value".into(),
                            value: Some(point.value.to_string()),
                            has_children: false,
                            children: vec![],
                        },
                    ],
                });
            }
            emitter.children.push(EditorElement {
                name: "AeoFXSizeCurve".into(),
                value: None,
                has_children: true,
                children: points,
            });
        }
        if !node.opacity.is_empty() {
            let mut points = Vec::with_capacity(node.opacity.len());
            for point in &node.opacity {
                points.push(EditorElement {
                    name: "Key".into(),
                    value: None,
                    has_children: true,
                    children: vec![
                        EditorElement {
                            name: "Time".into(),
                            value: Some(point.time.to_string()),
                            has_children: false,
                            children: vec![],
                        },
                        EditorElement {
                            name: "Value".into(),
                            value: Some(point.value.to_string()),
                            has_children: false,
                            children: vec![],
                        },
                    ],
                });
            }
            emitter.children.push(EditorElement {
                name: "AeoFXOpacityCurve".into(),
                value: None,
                has_children: true,
                children: points,
            });
        }
        for (name, keys) in [
            ("AeoFXRedCurve", &node.color_r),
            ("AeoFXGreenCurve", &node.color_g),
            ("AeoFXBlueCurve", &node.color_b),
        ] {
            if keys.is_empty() {
                continue;
            }
            let points = keys
                .iter()
                .map(|point| EditorElement {
                    name: "Key".into(),
                    value: None,
                    has_children: true,
                    children: vec![
                        EditorElement {
                            name: "Time".into(),
                            value: Some(point.time.to_string()),
                            has_children: false,
                            children: vec![],
                        },
                        EditorElement {
                            name: "Value".into(),
                            value: Some(point.value.to_string()),
                            has_children: false,
                            children: vec![],
                        },
                    ],
                })
                .collect();
            emitter.children.push(EditorElement {
                name: name.into(),
                value: None,
                has_children: true,
                children: points,
            });
        }
        group.children.push(emitter);
    }
    compiled = compiled.with_editor_tree(&tree)?;

    let patches: Vec<TemplateNodePatch> = flat
        .iter()
        .map(|(node, _)| TemplateNodePatch {
            lifetime_frames: node
                .lifetime_seconds
                .map(|seconds| (seconds * 60.0).round().max(1.0) as i32),
            generation_interval_frames: node.emission_rate.map(|per_second| 60.0 / per_second),
        })
        .collect();
    let compiled = compiled.with_template_particle_settings(&patches)?;
    let requested = shared_texture.unwrap_or("Texture/fire_tex.png");
    let compiled = if requested != "Texture/fire_tex.png" {
        rebind_textures(&compiled, &[("Texture/fire_tex.png", requested)])?
    } else {
        compiled
    };
    let encoded = compiled.encode()?;
    let reopened = EfkEfc::parse(&encoded)?;
    let rebuilt = EffectDocument::from_effekseer_template(&document.id, &document.name, &reopened)?;
    if rebuilt.roots.len() != document.roots.len()
        || !tree_ids_match(&document.roots, &rebuilt.roots)
        || rebuilt
            .roots
            .iter()
            .zip(&document.roots)
            .any(|(actual, expected)| {
                actual.id != expected.id
                    || actual.name != expected.name
                    || actual.texture.as_deref()
                        != Some(
                            expected
                                .texture
                                .as_deref()
                                .unwrap_or("Texture/fire_tex.png"),
                        )
                    || actual.size != expected.size
                    || actual.opacity != expected.opacity
                    || actual.color_r != expected.color_r
                    || actual.color_g != expected.color_g
                    || actual.color_b != expected.color_b
                    || actual.spawn_shape != expected.spawn_shape
                    || (actual.spawn_radius - expected.spawn_radius).abs() > 0.0001
                    || actual.velocity != expected.velocity
                    || actual.acceleration != expected.acceleration
                    || (actual.drag - expected.drag).abs() > 0.0001
                    || expected.emission_rate.is_some_and(|rate| {
                        actual
                            .emission_rate
                            .is_none_or(|got| (rate - got).abs() > 0.001)
                    })
                    || expected.lifetime_seconds.is_some_and(|seconds| {
                        actual
                            .lifetime_seconds
                            .is_none_or(|got| (seconds - got).abs() > 0.017)
                    })
            })
    {
        return Err("Standalone sprite effect failed parameter roundtrip".into());
    }
    verify_native_size_curves(&reopened, document)?;
    Ok(encoded)
}

fn tree_ids_match(expected: &[EffectNode], actual: &[EffectNode]) -> bool {
    expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(a, b)| {
            a.id == b.id && a.name == b.name && tree_ids_match(&a.children, &b.children)
        })
}
fn verify_native_size_curves(effect: &EfkEfc, document: &EffectDocument) -> Result<(), String> {
    let bytes = &effect
        .chunks
        .iter()
        .find(|c| &c.tag == b"BIN_")
        .ok_or("Missing native AeoFX runtime")?
        .bytes;
    let texture = document.roots[0]
        .texture
        .as_deref()
        .unwrap_or("Texture/fire_tex.png");
    let delta = (texture.encode_utf16().count() as isize
        - "Texture/fire_tex.png".encode_utf16().count() as isize)
        * 2;
    let mut start = HEADER_BYTES
        .checked_add_signed(delta)
        .ok_or("AeoFX texture header size overflow")?;
    let mut flat = Vec::new();
    flatten_authored_nodes(&document.roots, None, &mut flat);
    if start < 4 || bytes.get(start - 4..start) != Some(&(flat.len() as i32).to_le_bytes()[..]) {
        return Err("AeoFX runtime header count disagrees with sprite list".into());
    }
    for (node, _) in flat {
        let expected = if node.size.is_empty() {
            None
        } else {
            Some(encoded_size_curve(&node.size)?)
        };
        let mut length = SPRITE_END - SPRITE_BEGIN;
        let mut scale_expansion = 0usize;
        if let Some(curve) = expected {
            let begin = start.checked_add(480).ok_or("AeoFX runtime overflow")?;
            let end = begin
                .checked_add(curve.len())
                .ok_or("AeoFX runtime overflow")?;
            if bytes.get(begin..end) != Some(curve.as_slice()) {
                return Err("AeoFX native size curve differs from authored keys".into());
            }
            scale_expansion = curve.len() - 104;
            length += scale_expansion;
        }
        let native_shape = encoded_spawn(node)?;
        let shape_offset = start + SPAWN_BEGIN + scale_expansion;
        if bytes.get(shape_offset..shape_offset + native_shape.len())
            != Some(native_shape.as_slice())
        {
            return Err("AeoFX native spawn shape differs from authored settings".into());
        }
        let shape_expansion = shape_delta(node)?;
        length += shape_expansion;
        if has_rgba_keys(node) {
            let color = encoded_rgba_curves(node)?;
            let begin = start + COLOR_START + scale_expansion + shape_expansion;
            let end = begin
                .checked_add(color.len())
                .ok_or("AeoFX color overflow")?;
            if bytes.get(begin..end) != Some(color.as_slice()) {
                return Err("AeoFX native opacity curve differs from authored keys".into());
            }
            length += color.len() - 8;
        }
        start = start
            .checked_add(length)
            .ok_or("AeoFX native node size overflow")?;
        if start > bytes.len() {
            return Err("AeoFX runtime is truncated".into());
        }
    }
    if start != bytes.len() {
        return Err("AeoFX runtime node boundaries do not match".into());
    }
    Ok(())
}

pub fn compile_sprites_with_ring(document: &EffectDocument) -> Result<Vec<u8>, String> {
    use super::fx_authoring::{EmitterKind, SpawnShape};
    document.validate()?;
    let Some((last, _)) = document.roots.split_last() else {
        return Err("Ring effect requires Sprite nodes".into());
    };
    if last.kind != EmitterKind::Ring || !last.children.is_empty() {
        return Err("Native Ring must be the final root emitter".into());
    }
    let mut sprites = document.clone();
    sprites.roots.pop();
    let mut flat = Vec::new();
    flatten_authored_nodes(&sprites.roots, None, &mut flat);
    if flat.is_empty()
        || flat.len() >= MAX_SPRITES
        || flat
            .iter()
            .any(|(node, _)| node.kind != EmitterKind::Sprite)
    {
        return Err("Mixed native authoring supports 1-63 Sprites and one Ring".into());
    }
    if !last.enabled
        || last.texture.is_some()
        || last.spawn_shape != SpawnShape::Point
        || last.spawn_radius != 0.0
        || last.velocity != [0.0; 3]
        || last.acceleration != [0.0; 3]
        || last.drag != 0.0
        || !last.size.is_empty()
        || !last.opacity.is_empty()
        || !last.color_r.is_empty()
        || !last.color_g.is_empty()
        || !last.color_b.is_empty()
    {
        return Err("This Ring property has no verified native writer".into());
    }
    if flat.iter().any(|(node, _)| node.name == last.name) {
        return Err("Duplicate mixed emitter name".into());
    }
    let life = (last.lifetime_seconds.ok_or("Ring lifetime missing")? * 60.0).round();
    if !life.is_finite() || life < 1.0 || life > 60_000.0 {
        return Err("Ring lifetime cannot be represented natively".into());
    }
    let ring_bytes = super::fx_export::compile_native_ring_emitter(
        &last.name,
        life as i32,
        last.emission_rate.ok_or("Ring emission rate missing")?,
    )?;
    let ring = EfkEfc::parse(&ring_bytes)?;
    let ring_native = ring
        .chunks
        .iter()
        .find(|c| &c.tag == b"BIN_")
        .ok_or("Ring template missing native runtime")?
        .bytes
        .as_slice();
    const RING_PREFIX: usize = 104;
    if ring_native.len() != 844
        || ring_native.get(100..104) != Some(&1_i32.to_le_bytes()[..])
        || ring_native.get(RING_PREFIX..RING_PREFIX + 4) != Some(&4_i32.to_le_bytes()[..])
        || ring_native.get(RING_PREFIX + 8..RING_PREFIX + 12) != Some(&0_i32.to_le_bytes()[..])
    {
        return Err("Pinned Effekseer 1.80.7 Ring node layout changed".into());
    }
    let mut ring_tree = ring.editor_tree()?;
    let ring_node = ring_tree
        .iter_mut()
        .find(|e| e.name == "EffekseerProject")
        .and_then(|project| project.child_mut("Root"))
        .and_then(|root| root.child_mut("Children"))
        .and_then(|children| children.children.iter().find(|e| e.name == "Node"))
        .ok_or("Pinned Ring editor metadata missing")?
        .clone();
    if ring_node
        .child("DrawingValues")
        .and_then(|d| d.child_value("Type"))
        != Some("4")
    {
        return Err("Native Ring editor type mismatch".into());
    }

    let sprite_bytes = compile_standalone_sprites(&sprites)?;
    let mut mixed = EfkEfc::parse(&sprite_bytes)?;
    let native = mixed
        .chunks
        .iter_mut()
        .find(|c| &c.tag == b"BIN_")
        .ok_or("Sprite native runtime missing")?;
    let texture = sprites
        .roots
        .first()
        .and_then(|n| n.texture.as_deref())
        .unwrap_or("Texture/fire_tex.png");
    let delta = (texture.encode_utf16().count() as isize
        - "Texture/fire_tex.png".encode_utf16().count() as isize)
        * 2;
    let start = HEADER_BYTES
        .checked_add_signed(delta)
        .ok_or("Unsafe native sprite resource header")?;
    if start < 40
        || native.bytes.len() < start
        || flat.len() + 1 > MAX_SPRITES
        || native.bytes.get(start - 40..start - 36) != Some(&(flat.len() as i32).to_le_bytes()[..])
        || native.bytes.get(start - 4..start) != Some(&(flat.len() as i32).to_le_bytes()[..])
    {
        return Err("Sprite resource header changed; Ring append refused".into());
    }
    let old_runtime = native.bytes.clone();
    let combined = (flat.len() + 1) as i32;
    native.bytes[start - 40..start - 36].copy_from_slice(&combined.to_le_bytes());
    native.bytes[start - 4..start].copy_from_slice(&combined.to_le_bytes());
    let mut record = ring_native[RING_PREFIX..].to_vec();
    record[8..12].copy_from_slice(&(flat.len() as i32).to_le_bytes());
    native.bytes.extend_from_slice(&record);
    if native.bytes[start..old_runtime.len()] != old_runtime[start..] {
        return Err("Mixed native writer changed an existing Sprite record".into());
    }
    let mut editor = mixed.editor_tree()?;
    let group = editor
        .iter_mut()
        .find(|e| e.name == "EffekseerProject")
        .and_then(|e| e.child_mut("Root"))
        .and_then(|e| e.child_mut("Children"))
        .ok_or("Sprite editor hierarchy missing")?;
    let mut ring_node = ring_node;
    ring_node.children.push(EditorElement {
        name: "AeoFXStableId".into(),
        value: Some(last.id.to_string()),
        children: Vec::new(),
        has_children: false,
    });
    ring_node.children.push(EditorElement {
        name: "AeoFXRenderer".into(),
        value: Some("Ring".into()),
        children: Vec::new(),
        has_children: false,
    });
    group.children.push(ring_node);
    let mixed = mixed.with_editor_tree(&editor)?;
    let bytes = mixed.encode()?;
    let reopened = EfkEfc::parse(&bytes)?;
    let rebuilt = EffectDocument::from_effekseer_template(&document.id, &document.name, &reopened)?;
    if rebuilt.roots != document.roots {
        return Err("Sprite/Ring document settings changed during native roundtrip".into());
    }
    Ok(bytes)
}

fn compile_sprites_with_trails(doc: &EffectDocument) -> Result<Vec<u8>, String> {
    use super::fx_authoring::{EmitterKind, SpawnShape};
    doc.validate()?;
    if doc.roots.iter().any(|n| n.kind != EmitterKind::Sprite) {
        return Err("Ribbon/Track native authoring requires Sprite roots (not Ring)".into());
    }
    let mut cloned = doc.clone();
    fn strip(nodes: &mut [EffectNode]) -> Result<(), String> {
        for node in nodes {
            node.children
                .retain(|n| n.kind == super::fx_authoring::EmitterKind::Sprite);
            strip(&mut node.children)?;
        }
        Ok(())
    }
    strip(&mut cloned.roots)?;
    let mut flat = Vec::new();
    flatten_authored_nodes(&cloned.roots, None, &mut flat);
    let Some(&(parent, _)) = flat.last() else {
        return Err("A parent Sprite is required".into());
    };
    if flat.len() > MAX_SPRITES - 2 {
        return Err("Too many Sprites for trail emitters".into());
    }
    let trail_nodes = doc
        .node(parent.id)
        .ok_or("Native Sprite parent missing")?
        .children
        .iter()
        .filter(|n| n.kind != EmitterKind::Sprite)
        .collect::<Vec<_>>();
    if trail_nodes.is_empty() || trail_nodes.len() > 2 {
        return Err("Only one Ribbon and one Track child are currently supported".into());
    }
    fn check_advanced(nodes: &[EffectNode], parent: u64, found: &mut usize) -> Result<(), String> {
        for node in nodes {
            if node.kind != EmitterKind::Sprite {
                *found += 1;
                if !matches!(node.kind, EmitterKind::Ribbon | EmitterKind::Track) {
                    return Err("Unknown native advanced emitter".into());
                }
            }
            if node.id != parent && node.children.iter().any(|c| c.kind != EmitterKind::Sprite) {
                return Err("Ribbon/Track parent must be the final serialized Sprite".into());
            }
            check_advanced(&node.children, parent, found)?;
        }
        Ok(())
    }
    let mut found = 0;
    check_advanced(&doc.roots, parent.id, &mut found)?;
    if found != trail_nodes.len() {
        return Err("Unsupported extra native trail emitter".into());
    }
    let order = trail_nodes.iter().map(|n| n.kind).collect::<Vec<_>>();
    if !(order.as_slice() == [EmitterKind::Ribbon]
        || order.as_slice() == [EmitterKind::Track]
        || order.as_slice() == [EmitterKind::Ribbon, EmitterKind::Track])
    {
        return Err("Checked Ribbon/Track order must be Ribbon before Track".into());
    }
    for (index, node) in trail_nodes.iter().enumerate() {
        if !node.children.is_empty()
            || !node.enabled
            || node.texture.is_some()
            || !node.size.is_empty()
            || !node.opacity.is_empty()
            || !node.color_r.is_empty()
            || !node.color_g.is_empty()
            || !node.color_b.is_empty()
            || node.spawn_shape != SpawnShape::Point
            || node.spawn_radius != 0.0
            || node.velocity != [0.0; 3]
            || node.acceleration != [0.0; 3]
            || node.drag != 0.0
            || node.emission_rate.is_none()
            || node.lifetime_seconds.is_none()
        {
            return Err(format!(
                "Unverified {} property in child {}",
                node.kind.label(),
                index
            ));
        }
    }
    let source = include_bytes!("../../assets/effekseer/archetypes/trail.efkefc");
    let reference = EfkEfc::parse(source)?;
    let source_native = reference
        .chunks
        .iter()
        .find(|c| &c.tag == b"BIN_")
        .ok_or("Pinned native Trail chunk missing")?
        .bytes
        .as_slice();
    if source_native.len() != 2268
        || source_native.get(816..820) != Some(&2_i32.to_le_bytes()[..])
        || source_native.get(820..824) != Some(&3_i32.to_le_bytes()[..])
        || source_native.get(1520..1524) != Some(&6_i32.to_le_bytes()[..])
    {
        return Err("Pinned Ribbon/Track native node boundaries changed".into());
    }
    let editor = reference.editor_tree()?;
    let source_parent = editor
        .iter()
        .find(|n| n.name == "EffekseerProject")
        .and_then(|n| n.child("Root"))
        .and_then(|n| n.child("Children"))
        .and_then(|n| n.children.iter().find(|x| x.name == "Node"))
        .ok_or("Pinned Trail editor parent missing")?;
    let kids = source_parent
        .child("Children")
        .ok_or("Pinned Trail editor children missing")?;
    let mut checked = Vec::new();
    for child in &kids.children {
        if child.name != "Node" {
            continue;
        }
        let code = child
            .child("DrawingValues")
            .and_then(|d| d.child_value("Type"));
        if !matches!(code, Some("3") | Some("6")) {
            return Err("Pinned Trail renderer mismatch".into());
        }
        checked.push((code.unwrap(), child.clone()));
    }
    if checked.len() != 2 || checked[0].0 != "3" || checked[1].0 != "6" {
        return Err("Pinned Trail editor order mismatch".into());
    }
    let bytes = compile_standalone_sprites(&cloned)?;
    let mut effect = EfkEfc::parse(&bytes)?;
    let native = effect
        .chunks
        .iter_mut()
        .find(|c| &c.tag == b"BIN_")
        .ok_or("Native Sprite runtime missing")?;
    let texture = cloned
        .roots
        .first()
        .and_then(|n| n.texture.as_deref())
        .unwrap_or("Texture/fire_tex.png");
    let delta = (texture.encode_utf16().count() as isize
        - "Texture/fire_tex.png".encode_utf16().count() as isize)
        * 2;
    let start = HEADER_BYTES
        .checked_add_signed(delta)
        .ok_or("Unsafe native trail resource header")?;
    let last = native
        .bytes
        .len()
        .checked_sub(4)
        .ok_or("Sprite runtime missing")?;
    if native.bytes.get(start - 40..start - 36) != Some(&(flat.len() as i32).to_le_bytes()[..])
        || native.bytes.get(start - 4..start) != Some(&(flat.len() as i32).to_le_bytes()[..])
        || native.bytes.get(last..last + 4) != Some(&0_i32.to_le_bytes()[..])
    {
        return Err("Sprite runtime cannot safely accept native children".into());
    }
    let baseline = native.bytes.clone();
    native.bytes[start - 40..start - 36]
        .copy_from_slice(&((flat.len() + trail_nodes.len()) as i32).to_le_bytes());
    native.bytes[last..last + 4].copy_from_slice(&(trail_nodes.len() as i32).to_le_bytes());
    for (index, node) in trail_nodes.iter().enumerate() {
        let (begin, end) = match node.kind {
            EmitterKind::Ribbon => (820, 1520),
            EmitterKind::Track => (1520, 2268),
            _ => return Err("Unknown trail type".into()),
        };
        let mut record = source_native[begin..end].to_vec();
        record[8..12].copy_from_slice(&((flat.len() + index) as i32).to_le_bytes());
        if record.get(76..84) != Some(&[100_i32.to_le_bytes(), 100_i32.to_le_bytes()].concat()[..])
            || record.get(84..92) != Some(&[2_f32.to_le_bytes(), 2_f32.to_le_bytes()].concat()[..])
        {
            return Err("Pinned native Trail Life/Generation offsets changed".into());
        }
        let frames = (node.lifetime_seconds.ok_or("Trail life missing")? * 60.0).round();
        let rate = node.emission_rate.ok_or("Trail rate missing")?;
        if !frames.is_finite()
            || frames < 1.0
            || frames > 60_000.0
            || !rate.is_finite()
            || !(0.01..=100_000.0).contains(&rate)
        {
            return Err("Invalid native Trail lifetime or emission rate".into());
        }
        let interval = 60.0 / rate;
        if !interval.is_finite() || interval <= 0.0 || interval > 1_000_000.0 {
            return Err("Trail generation interval outside supported native range".into());
        }
        record[76..80].copy_from_slice(&(frames as i32).to_le_bytes());
        record[80..84].copy_from_slice(&(frames as i32).to_le_bytes());
        record[84..88].copy_from_slice(&interval.to_le_bytes());
        record[88..92].copy_from_slice(&interval.to_le_bytes());
        native.bytes.extend_from_slice(&record);
    }
    if native.bytes[start..last] != baseline[start..last] {
        return Err("Existing Sprite runtime unexpectedly changed".into());
    }
    let mut tree = effect.editor_tree()?;
    let group = tree
        .iter_mut()
        .find(|n| n.name == "EffekseerProject")
        .and_then(|n| n.child_mut("Root"))
        .and_then(|n| n.child_mut("Children"))
        .ok_or("Sprite editor root missing")?;
    let parent_node = group
        .children
        .iter_mut()
        .filter(|n| n.name == "Node")
        .last()
        .ok_or("Last native Sprite metadata missing")?;
    if !parent_node.children.iter().any(|n| n.name == "Children") {
        parent_node.children.push(EditorElement {
            name: "Children".into(),
            value: None,
            children: Vec::new(),
            has_children: true,
        });
    }
    let editable = parent_node
        .child_mut("Children")
        .ok_or("Native Sprite metadata children missing")?;
    editable.has_children = true;
    for node in trail_nodes {
        let template = checked
            .iter()
            .find(|(ty, _)| match node.kind {
                EmitterKind::Ribbon => *ty == "3",
                EmitterKind::Track => *ty == "6",
                _ => false,
            })
            .ok_or("Missing typed Trail editor node")?;
        let mut child = template.1.clone();
        child.child_mut("Name").ok_or("Missing Trail name")?.value = Some(node.name.clone());
        let common = child
            .child_mut("CommonValues")
            .ok_or("Trail CommonValues missing")?;
        if common.child("Life").is_none() {
            common.children.push(EditorElement {
                name: "Life".into(),
                value: None,
                has_children: true,
                children: ["Center", "Max", "Min"]
                    .iter()
                    .map(|name| EditorElement {
                        name: (*name).into(),
                        value: Some("100".into()),
                        children: Vec::new(),
                        has_children: false,
                    })
                    .collect(),
            });
        }
        let life = (node
            .lifetime_seconds
            .ok_or("Native trail lifetime missing")?
            * 60.0)
            .round();
        let rate = node.emission_rate.ok_or("Native trail rate missing")?;
        let interval = 60.0 / rate;
        for (path, value) in [
            (false, (life as i32).to_string()),
            (true, interval.to_string()),
        ] {
            let group = if path {
                common
                    .child_mut("Generation")
                    .and_then(|g| g.child_mut("GenerationTime"))
                    .ok_or("Missing Trail GenerationTime")?
            } else {
                common.child_mut("Life").ok_or("Missing Trail Life")?
            };
            for name in ["Center", "Max", "Min"] {
                group
                    .child_mut(name)
                    .ok_or("Missing native Trail numeric field")?
                    .value = Some(value.clone());
            }
        }
        child.children.push(EditorElement {
            name: "AeoFXStableId".into(),
            value: Some(node.id.to_string()),
            children: Vec::new(),
            has_children: false,
        });
        child.children.push(EditorElement {
            name: "AeoFXRenderer".into(),
            value: Some(node.kind.label().into()),
            children: Vec::new(),
            has_children: false,
        });
        editable.children.push(child);
    }
    let effect = effect.with_editor_tree(&tree)?;
    let serialized = effect.encode()?;
    let readback = EfkEfc::parse(&serialized)?;
    let recovered = EffectDocument::from_effekseer_template(&doc.id, &doc.name, &readback)?;
    if recovered.roots != doc.roots {
        return Err(format!(
            "Trail readback mismatch: expected {:?}, got {:?}",
            doc.roots, recovered.roots
        ));
    }
    Ok(serialized)
}

#[cfg(test)]
mod tests {
    use super::super::fx_authoring::EffectNode;
    use super::*;
    fn source(names: &[&str]) -> EffectDocument {
        let mut doc = EffectDocument::new("new.fx", "Standalone");
        for (index, name) in names.iter().enumerate() {
            doc.roots.push(EffectNode {
                id: index as u64 + 1,
                kind: super::super::fx_authoring::EmitterKind::Sprite,
                name: (*name).into(),
                enabled: true,
                emission_rate: Some(20.0 + index as f32 * 10.0),
                lifetime_seconds: Some(1.0 + index as f32),
                texture: Some("Texture/fire_tex.png".into()),
                color_r: Vec::new(),
                color_g: Vec::new(),
                color_b: Vec::new(),
                spawn_shape: crate::project::fx_authoring::SpawnShape::Point,
                spawn_radius: 0.0,
                velocity: [0.0; 3],
                acceleration: [0.0; 3],
                drag: 0.0,
                size: Vec::new(),
                opacity: Vec::new(),
                children: Vec::new(),
            });
        }
        doc
    }
    #[test]
    fn native_ribbon_track_child_compiler_preserves_parent_sprite() {
        use super::super::fx_authoring::EmitterKind;
        for children in [
            vec![EmitterKind::Ribbon],
            vec![EmitterKind::Track],
            vec![EmitterKind::Ribbon, EmitterKind::Track],
        ] {
            let mut doc = source(&["Parent Sprite"]);
            doc.roots[0].size = vec![
                super::super::fx_authoring::CurveKey {
                    time: 0.0,
                    value: 0.5,
                },
                super::super::fx_authoring::CurveKey {
                    time: 1.0,
                    value: 1.5,
                },
            ];
            let original = doc.roots[0].clone();
            let base = AeoFxCompiler::export(&doc).unwrap();
            for (i, kind) in children.iter().enumerate() {
                let mut child = EffectNode::new(i as u64 + 2, format!("{} {}", kind.label(), i));
                child.kind = *kind;
                child.emission_rate = Some(30.0);
                child.lifetime_seconds = Some(100.0 / 60.0);
                doc.roots[0].children.push(child);
            }
            let data = AeoFxCompiler::export(&doc).unwrap();
            let native = EfkEfc::parse(&data).unwrap();
            let round =
                EffectDocument::from_effekseer_template(&doc.id, &doc.name, &native).unwrap();
            assert_eq!(round.roots, doc.roots);
            assert_eq!(round.roots[0].name, original.name);
            assert_eq!(round.roots[0].size, original.size);
            assert_eq!(AeoFxCompiler::export(&round).unwrap(), data);
            let base_native = EfkEfc::parse(&base).unwrap();
            let old = base_native
                .chunks
                .iter()
                .find(|x| &x.tag == b"BIN_")
                .unwrap()
                .bytes
                .as_slice();
            let new = native
                .chunks
                .iter()
                .find(|x| &x.tag == b"BIN_")
                .unwrap()
                .bytes
                .as_slice();
            let start = HEADER_BYTES;
            let tail = old.len() - 4;
            assert_eq!(&old[start..tail], &new[start..tail]);
            assert_eq!(
                new.len() - old.len(),
                children
                    .iter()
                    .map(|k| if *k == EmitterKind::Ribbon { 700 } else { 748 })
                    .sum::<usize>()
            );
            let mut modified = doc.clone();
            modified.roots[0].children[0].emission_rate = Some(55.0);
            modified.roots[0].children[0].lifetime_seconds = Some(3.0);
            let modified_native = AeoFxCompiler::export(&modified).unwrap();
            let restored = EffectDocument::from_effekseer_template(
                &modified.id,
                &modified.name,
                &EfkEfc::parse(&modified_native).unwrap(),
            )
            .unwrap();
            assert_eq!(restored.roots, modified.roots);
            assert_ne!(modified_native, data);
            assert_eq!(AeoFxCompiler::export(&doc).unwrap(), data);
        }
    }

    #[test]
    fn additive_ring_preserves_multiple_sprites_and_nested_hierarchy() {
        use super::super::fx_authoring::{CurveKey, EmitterKind, SpawnShape};
        let mut doc = source(&["Flames", "Smoke", "Embers"]);
        for sprite in &mut doc.roots {
            sprite.texture = Some("Texture/custom.png".into());
        }
        doc.roots[0].size = vec![
            CurveKey {
                time: 0.0,
                value: 0.3,
            },
            CurveKey {
                time: 1.0,
                value: 1.5,
            },
        ];
        doc.roots[1].opacity = vec![
            CurveKey {
                time: 0.0,
                value: 1.0,
            },
            CurveKey {
                time: 1.0,
                value: 0.1,
            },
        ];
        doc.roots[2].spawn_shape = SpawnShape::Sphere;
        doc.roots[2].spawn_radius = 1.3;
        doc.roots[2].velocity = [0.5, 1.0, 0.3];
        doc.roots[2].drag = 0.25;
        doc.reparent_node(3, Some(1)).unwrap();
        let originals = doc.roots.clone();
        let sprite_only = AeoFxCompiler::export(&doc).unwrap();
        let mut ring = EffectNode::new(4, "Magic Ring");
        ring.kind = EmitterKind::Ring;
        ring.emission_rate = Some(12.0);
        ring.lifetime_seconds = Some(2.5);
        doc.roots.push(ring);
        let mixed = AeoFxCompiler::export(&doc).unwrap();
        let first = EfkEfc::parse(&sprite_only).unwrap();
        let second = EfkEfc::parse(&mixed).unwrap();
        let first_native = &first
            .chunks
            .iter()
            .find(|c| &c.tag == b"BIN_")
            .unwrap()
            .bytes;
        let second_native = &second
            .chunks
            .iter()
            .find(|c| &c.tag == b"BIN_")
            .unwrap()
            .bytes;
        let path = "Texture/custom.png";
        let delta = (path.encode_utf16().count() as isize
            - "Texture/fire_tex.png".encode_utf16().count() as isize)
            * 2;
        let begin = HEADER_BYTES.checked_add_signed(delta).unwrap();
        assert_eq!(
            &second_native[begin..first_native.len()],
            &first_native[begin..]
        );
        assert_eq!(second_native.len() - first_native.len(), 740);
        let back = EffectDocument::from_effekseer_template(&doc.id, &doc.name, &second).unwrap();
        assert_eq!(&back.roots[..back.roots.len() - 1], originals.as_slice());
        assert_eq!(back.roots.last(), doc.roots.last());
        assert_eq!(AeoFxCompiler::export(&back).unwrap(), mixed);
        let mut invalid = doc.clone();
        invalid.roots.last_mut().unwrap().size.push(CurveKey {
            time: 0.0,
            value: 1.0,
        });
        assert!(AeoFxCompiler::export(&invalid).is_err());
        assert_eq!(AeoFxCompiler::export(&doc).unwrap(), mixed);
    }

    #[test]
    fn native_spawn_shape_velocity_acceleration_and_drag_roundtrip() {
        use super::super::fx_authoring::SpawnShape;
        for (shape, kind, expected_extra) in [
            (SpawnShape::Point, 0_i32, 0_usize),
            (SpawnShape::Circle, 3_i32, 20_usize),
            (SpawnShape::Sphere, 1_i32, 0_usize),
            (SpawnShape::Line, 4_i32, 40_usize),
        ] {
            let mut doc = source(&["Particle"]);
            let velocity = [1.0, 2.0, -3.0];
            let acceleration = [-0.5, 0.25, 0.75];
            doc.set_spawn_motion(1, shape, 2.25, velocity, acceleration, 0.6)
                .unwrap();
            let data = AeoFxCompiler::export(&doc).unwrap();
            let effect = EfkEfc::parse(&data).unwrap();
            let loaded =
                EffectDocument::from_effekseer_template(&doc.id, &doc.name, &effect).unwrap();
            assert_eq!(loaded, doc);
            assert_eq!(data, AeoFxCompiler::export(&loaded).unwrap());
            let bin = &effect
                .chunks
                .iter()
                .find(|c| &c.tag == b"BIN_")
                .unwrap()
                .bytes;
            assert_eq!(
                bin.len(),
                HEADER_BYTES + SPRITE_END - SPRITE_BEGIN + expected_extra
            );
            let first = HEADER_BYTES;
            let kind_offset = first + SPAWN_BEGIN + 4;
            assert_eq!(
                i32::from_le_bytes(bin[kind_offset..kind_offset + 4].try_into().unwrap()),
                kind
            );
            for (offset, values) in [(MOTION_START, velocity), (ACCEL_START, acceleration)] {
                for (i, value) in values.iter().enumerate() {
                    for shift in [0, 12] {
                        let offset = first + offset + shift + i * 4;
                        assert_eq!(
                            f32::from_le_bytes(bin[offset..offset + 4].try_into().unwrap()),
                            *value
                        );
                    }
                }
            }
            assert_eq!(
                i32::from_le_bytes(
                    bin[first + LFF_TYPE..first + LFF_TYPE + 4]
                        .try_into()
                        .unwrap()
                ),
                7
            );
            assert_eq!(
                f32::from_le_bytes(
                    bin[first + LFF_POWER..first + LFF_POWER + 4]
                        .try_into()
                        .unwrap()
                ),
                0.6
            );
        }
    }

    #[test]
    fn native_rgba_lifetime_curves_roundtrip_and_modify_rgb_samples() {
        let mut doc = source(&["Color"]);
        use super::super::fx_authoring::{CurveChannel, CurveKey};
        let red = vec![
            CurveKey {
                time: 0.0,
                value: 1.0,
            },
            CurveKey {
                time: 1.0,
                value: 0.0,
            },
        ];
        let green = vec![
            CurveKey {
                time: 0.0,
                value: 0.2,
            },
            CurveKey {
                time: 1.0,
                value: 0.8,
            },
        ];
        doc.set_curve(1, CurveChannel::Red, red.clone()).unwrap();
        doc.set_curve(1, CurveChannel::Green, green.clone())
            .unwrap();
        doc.set_curve(
            1,
            CurveChannel::Opacity,
            vec![
                CurveKey {
                    time: 0.0,
                    value: 1.0,
                },
                CurveKey {
                    time: 1.0,
                    value: 0.0,
                },
            ],
        )
        .unwrap();
        let encoded = AeoFxCompiler::export(&doc).unwrap();
        let reopened = EfkEfc::parse(&encoded).unwrap();
        let restored =
            EffectDocument::from_effekseer_template(&doc.id, &doc.name, &reopened).unwrap();
        assert_eq!(restored, doc);
        assert_eq!(AeoFxCompiler::export(&restored).unwrap(), encoded);
        let bytes = &reopened
            .chunks
            .iter()
            .find(|c| &c.tag == b"BIN_")
            .unwrap()
            .bytes;
        let color_offset = HEADER_BYTES + COLOR_START;
        assert_eq!(&bytes[color_offset..color_offset + 4], &3_i32.to_le_bytes());
        let red_samples = color_offset + 8 + 32;
        assert_eq!(
            f32::from_le_bytes(bytes[red_samples..red_samples + 4].try_into().unwrap()),
            255.0
        );
        assert_eq!(
            f32::from_le_bytes(
                bytes[red_samples + 400..red_samples + 404]
                    .try_into()
                    .unwrap()
            ),
            0.0
        );
        let prior = doc.clone();
        assert!(
            doc.set_curve(
                1,
                CurveChannel::Red,
                vec![CurveKey {
                    time: 0.0,
                    value: 1.2
                }]
            )
            .is_err()
        );
        assert_eq!(doc, prior);
    }

    #[test]
    fn authored_parent_groups_and_stable_ids_roundtrip_in_native_sprite_effect() {
        let mut doc = source(&["Flames", "Smoke", "Sparks"]);
        doc.roots[0].id = 10;
        doc.roots[1].id = 20;
        doc.roots[2].id = 30;
        doc.reparent_node(30, Some(10)).unwrap();
        doc.roots[0].children[0].size = vec![
            super::super::fx_authoring::CurveKey {
                time: 0.0,
                value: 0.5,
            },
            super::super::fx_authoring::CurveKey {
                time: 1.0,
                value: 2.0,
            },
        ];
        let data = AeoFxCompiler::export(&doc).unwrap();
        let opened = EfkEfc::parse(&data).unwrap();
        let restored =
            EffectDocument::from_effekseer_template(&doc.id, &doc.name, &opened).unwrap();
        assert_eq!(restored, doc, "IDs and editable grouping must survive");
        assert_eq!(AeoFxCompiler::export(&restored).unwrap(), data);
        let removed = doc.remove_node(20).unwrap();
        assert_eq!(removed.id, 20);
        assert_eq!(doc.roots[0].children[0].id, 30);
        let smaller = AeoFxCompiler::export(&doc).unwrap();
        let reopened = EffectDocument::from_effekseer_template(
            &doc.id,
            &doc.name,
            &EfkEfc::parse(&smaller).unwrap(),
        )
        .unwrap();
        assert_eq!(reopened, doc);
        let before = doc.clone();
        assert!(doc.reparent_node(10, Some(30)).is_err());
        assert_eq!(doc, before);
    }

    #[test]
    fn checked_sprite_record_reencodes_the_official_archetype() {
        let asset = EfkEfc::parse(ARCHETYPE).unwrap();
        let bytes = &asset
            .chunks
            .iter()
            .find(|c| &c.tag == b"BIN_")
            .unwrap()
            .bytes;
        let fixture = &bytes[SPRITE_BEGIN..SPRITE_END];
        let sprite = PinnedFireSprite::decode(fixture).unwrap();
        let node = source(&["Fire"]).roots.remove(0);
        assert_eq!(sprite.encode(&node, 1).unwrap(), fixture);
        let mut broken = fixture.to_vec();
        broken[SCALE_START] ^= 1;
        assert!(PinnedFireSprite::decode(&broken).is_err());
        broken = fixture.to_vec();
        broken[COLOR_START - 12] ^= 1;
        assert!(PinnedFireSprite::decode(&broken).is_err());
        assert!(PinnedFireSprite::decode(&fixture[..fixture.len() - 1]).is_err());
    }

    #[test]
    fn standalone_sprite_from_new_document_roundtrips() {
        for count in [1, 3, 32] {
            let names: Vec<_> = (0..count).map(|index| format!("Sprite-{index}")).collect();
            let doc = source(&names.iter().map(String::as_str).collect::<Vec<_>>());
            let encoded = compile_standalone_sprites(&doc).unwrap();
            let reopened = EfkEfc::parse(&encoded).unwrap();
            let recovered =
                EffectDocument::from_effekseer_template("new.fx", "Standalone", &reopened).unwrap();
            assert_eq!(recovered.roots.len(), count);
            assert_eq!(compile_standalone_sprites(&doc).unwrap(), encoded);
            for (actual, authored) in recovered.roots.iter().zip(&doc.roots) {
                assert_eq!(actual.name, authored.name);
                assert_eq!(actual.emission_rate, authored.emission_rate);
                assert!(
                    (actual.lifetime_seconds.unwrap() - authored.lifetime_seconds.unwrap()).abs()
                        < 0.017
                );
            }
        }
    }
    #[test]
    fn aeofx_size_curve_roundtrip_and_native_mismatch_rejected() {
        use super::super::fx_authoring::CurveKey;
        let mut document = source(&["Spark", "Mist"]);
        document.roots[0].size = vec![
            CurveKey {
                time: 0.0,
                value: 2.0,
            },
            CurveKey {
                time: 0.5,
                value: 0.8,
            },
            CurveKey {
                time: 1.0,
                value: 0.2,
            },
        ];
        document.roots[0].opacity = vec![
            CurveKey {
                time: 0.0,
                value: 1.0,
            },
            CurveKey {
                time: 0.5,
                value: 0.7,
            },
            CurveKey {
                time: 1.0,
                value: 0.0,
            },
        ];
        let exported = AeoFxCompiler::export(&document).unwrap();
        let decoded = EfkEfc::parse(&exported).unwrap();
        let reopened =
            EffectDocument::from_effekseer_template(&document.id, &document.name, &decoded)
                .unwrap();
        assert_eq!(reopened.roots[0].size, document.roots[0].size);
        assert_eq!(reopened.roots[0].opacity, document.roots[0].opacity);
        assert!(reopened.roots[1].size.is_empty());
        assert_eq!(exported, AeoFxCompiler::export(&reopened).unwrap());

        let mut corrupted = decoded.clone();
        let runtime = &mut corrupted
            .chunks
            .iter_mut()
            .find(|chunk| &chunk.tag == b"BIN_")
            .unwrap()
            .bytes;
        let sample_offset = HEADER_BYTES + 480 + 8 + 4 * 8;
        runtime[sample_offset] ^= 0x10;
        assert!(verify_native_size_curves(&corrupted, &document).is_err());

        let mut color_corrupted = decoded.clone();
        let color_runtime = &mut color_corrupted
            .chunks
            .iter_mut()
            .find(|chunk| &chunk.tag == b"BIN_")
            .unwrap()
            .bytes;
        let scale_extension = encoded_size_curve(&document.roots[0].size).unwrap().len() - 104;
        let color_sample = HEADER_BYTES + 912 + scale_extension + 40;
        color_runtime[color_sample] ^= 0x10;
        assert!(verify_native_size_curves(&color_corrupted, &document).is_err());
    }

    #[test]
    fn shorter_shared_texture_preserves_curves_and_node_boundaries() {
        use super::super::fx_authoring::CurveKey;
        let mut doc = source(&["First", "Second"]);
        for node in &mut doc.roots {
            node.texture = Some("Texture/x.png".into());
            node.size = vec![
                CurveKey {
                    time: 0.0,
                    value: 1.0,
                },
                CurveKey {
                    time: 1.0,
                    value: 0.4,
                },
            ];
            node.opacity = vec![
                CurveKey {
                    time: 0.0,
                    value: 1.0,
                },
                CurveKey {
                    time: 1.0,
                    value: 0.0,
                },
            ];
        }
        let bytes = AeoFxCompiler::export(&doc).unwrap();
        let decoded = EfkEfc::parse(&bytes).unwrap();
        let rebuilt =
            EffectDocument::from_effekseer_template(&doc.id, &doc.name, &decoded).unwrap();
        assert_eq!(rebuilt.roots[0].texture, Some("Texture/x.png".into()));
        assert_eq!(rebuilt.roots[1].size, doc.roots[1].size);
        assert_eq!(rebuilt.roots[1].opacity, doc.roots[1].opacity);
        assert_eq!(AeoFxCompiler::export(&rebuilt).unwrap(), bytes);
    }

    #[test]
    fn custom_project_texture_is_rebound_across_runtime_and_editable_data() {
        let mut document = source(&["Flame A", "Flame B"]);
        for node in &mut document.roots {
            node.texture = Some("Texture/creator-flames.png".into());
        }
        let encoded = AeoFxCompiler::export(&document).unwrap();
        let reopened = EfkEfc::parse(&encoded).unwrap();
        let restored =
            EffectDocument::from_effekseer_template(&document.id, &document.name, &reopened)
                .unwrap();
        assert!(
            restored
                .roots
                .iter()
                .all(|node| node.texture.as_deref() == Some("Texture/creator-flames.png"))
        );
        assert_eq!(AeoFxCompiler::export(&restored).unwrap(), encoded);
        assert!(
            reopened
                .editor_data()
                .unwrap()
                .windows(18)
                .any(|w| w == b"creator-flames.png")
        );
        document.roots[1].texture = Some("Texture/other.png".into());
        assert!(AeoFxCompiler::export(&document).is_err());
        document.roots[1].texture = Some("../unsafe.png".into());
        assert!(AeoFxCompiler::export(&document).is_err());
    }

    #[test]
    fn standalone_rejects_unsupported_parameters_without_modifying_archetype() {
        let mut doc = source(&["Sprite A"]);
        doc.roots[0]
            .opacity
            .push(super::super::fx_authoring::CurveKey {
                time: 0.0,
                value: 1.5,
            });
        assert!(compile_standalone_sprites(&doc).is_err());
        doc.roots[0].opacity.clear();
        doc.roots[0].texture = Some("../../bad.png".into());
        assert!(compile_standalone_sprites(&doc).is_err());
        doc.roots[0].texture = Some("Texture/fire_tex.png".into());
        doc.roots[0].id = 99;
        let renamed = compile_standalone_sprites(&doc).unwrap();
        let recovered = EffectDocument::from_effekseer_template(
            &doc.id,
            &doc.name,
            &EfkEfc::parse(&renamed).unwrap(),
        )
        .unwrap();
        assert_eq!(recovered.roots[0].id, 99);
        doc.roots[0].id = 1;
        doc.roots[0]
            .children
            .push(source(&["Child"]).roots.remove(0));
        assert!(compile_standalone_sprites(&doc).is_err());
        assert_eq!(
            EfkEfc::parse(ARCHETYPE).unwrap().encode().unwrap(),
            ARCHETYPE
        );
    }
}
