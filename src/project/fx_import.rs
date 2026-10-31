// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::fx_authoring::EffectDocument;
use super::fx_export::{EfkEfc, compile_native_ring_emitter};

#[derive(Clone, Debug)]
pub enum EffectCompatibility {
    NativeAeoFx,
    NativeRing,
    ImportedFire {
        original: EfkEfc,
        writable_rate: Vec<u64>,
        writable_lifetime: Vec<u64>,
    },
}

impl EffectCompatibility {
    pub fn imported(&self) -> bool {
        matches!(self, Self::ImportedFire { .. })
    }
    pub fn can_edit_rate(&self, id: u64) -> bool {
        match self {
            Self::NativeAeoFx | Self::NativeRing => true,
            Self::ImportedFire { writable_rate, .. } => writable_rate.contains(&id),
        }
    }
    pub fn can_edit_lifetime(&self, id: u64) -> bool {
        match self {
            Self::NativeAeoFx | Self::NativeRing => true,
            Self::ImportedFire {
                writable_lifetime, ..
            } => writable_lifetime.contains(&id),
        }
    }
    pub fn compile(&self, edited: &EffectDocument) -> Result<Vec<u8>, String> {
        match self {
            Self::NativeAeoFx => super::fx_standalone::AeoFxCompiler::export(edited),
            Self::NativeRing => compile_ring_document(edited),
            Self::ImportedFire { original, .. } => {
                edited.compile_template_texture_variant(original)
            }
        }
    }
}

pub fn compile_ring_document(doc: &EffectDocument) -> Result<Vec<u8>, String> {
    use super::fx_authoring::SpawnShape;
    doc.validate()?;
    let [node] = doc.roots.as_slice() else {
        return Err("Ring authoring supports exactly one root emitter".into());
    };
    if node.id != 1
        || !node.enabled
        || !node.children.is_empty()
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
    {
        return Err("Unsupported mixed or custom Ring properties".into());
    }
    let frames = (node.lifetime_seconds.ok_or("Ring lifetime missing")? * 60.0).round();
    if !frames.is_finite() || frames < 1.0 || frames > 60_000.0 {
        return Err("Ring lifetime is outside native bounds".into());
    }
    let rate = node.emission_rate.ok_or("Ring emission missing")?;
    compile_native_ring_emitter(&node.name, frames as i32, rate)
}

fn inspect_ring(
    bytes: &[u8],
    effect: &EfkEfc,
) -> Result<(EffectCompatibility, EffectDocument), String> {
    let tree = effect.editor_tree()?;
    let group = tree
        .iter()
        .find(|node| node.name == "EffekseerProject")
        .and_then(|root| root.child("Root"))
        .and_then(|root| root.child("Children"))
        .ok_or("Ring emitter hierarchy missing")?;
    let nodes = group
        .children
        .iter()
        .filter(|node| node.name == "Node")
        .collect::<Vec<_>>();
    let [node] = nodes.as_slice() else {
        return Err("Unsupported multi-node advanced effect".into());
    };
    if node
        .child("DrawingValues")
        .and_then(|d| d.child_value("Type"))
        != Some("4")
    {
        return Err("Unsupported advanced renderer".into());
    }
    let name = node
        .child_value("Name")
        .ok_or("Ring emitter name missing")?;
    let values = node
        .child("CommonValues")
        .ok_or("Ring native settings missing")?;
    let frames = values
        .child("Life")
        .and_then(|v| v.child_value("Max"))
        .ok_or("Ring lifetime missing")?
        .parse::<i32>()
        .map_err(|_| "Invalid Ring lifetime")?;
    let interval = values
        .child("Generation")
        .and_then(|v| v.child("GenerationTime"))
        .and_then(|v| v.child_value("Max"))
        .ok_or("Ring generation missing")?
        .parse::<f32>()
        .map_err(|_| "Invalid Ring interval")?;
    if !interval.is_finite() || interval <= 0.0 {
        return Err("Ring generation interval invalid".into());
    }
    let rate = 60.0 / interval;
    let expected = compile_native_ring_emitter(name, frames, rate)?;
    if expected != bytes {
        return Err("Unsupported Ring layout or settings: imported effect stays unchanged".into());
    }
    let mut doc = EffectDocument::new("ring", "Ring");
    let mut emitter = super::fx_authoring::EffectNode::new(1, name);
    emitter.kind = super::fx_authoring::EmitterKind::Ring;
    emitter.emission_rate = Some(rate);
    emitter.lifetime_seconds = Some(frames as f32 / 60.0);
    doc.roots.push(emitter);
    Ok((EffectCompatibility::NativeRing, doc))
}

pub fn inspect(bytes: &[u8]) -> Result<(EffectCompatibility, EffectDocument), String> {
    let effect = EfkEfc::parse(bytes)?;
    if let Ok((mode, doc)) = inspect_ring(bytes, &effect) {
        return Ok((mode, doc));
    }
    let tree = effect.editor_tree()?;
    let mut pending: Vec<_> = tree.iter().collect();
    let mut authored = false;
    while let Some(node) = pending.pop() {
        if node.name == "AeoFXStableId" {
            authored = true;
            break;
        }
        pending.extend(node.children.iter());
    }
    let doc = EffectDocument::from_effekseer_template("imported", "Imported Effect", &effect)?;
    if authored {
        let recompiled = super::fx_standalone::AeoFxCompiler::export(&doc)
            .map_err(|e| format!("Unrecognized AeoFX authoring layout: {e}"))?;
        let mut pending: Vec<_> = doc.roots.iter().collect();
        let mut advanced = false;
        while let Some(n) = pending.pop() {
            advanced |= n.kind != super::fx_authoring::EmitterKind::Sprite;
            pending.extend(n.children.iter());
        }
        if advanced && recompiled.as_slice() != bytes {
            return Err("Native mixed Effect failed exact preserve-all compatibility check".into());
        }
        return Ok((EffectCompatibility::NativeAeoFx, doc));
    }
    let verified = include_bytes!("../../assets/effekseer/fire.efkefc");
    let source = EfkEfc::parse(verified)?;
    let reproducible = doc
        .compile_template_texture_variant(&source)
        .map(|compiled| compiled == bytes)
        .unwrap_or(false);
    if !reproducible {
        return Err("This imported Effekseer effect is playable, but its authoring layout has not been verified for editing. The original remains unchanged.".into());
    }
    let mut ids = Vec::new();
    fn collect(nodes: &[super::fx_authoring::EffectNode], ids: &mut Vec<u64>) {
        for node in nodes {
            ids.push(node.id);
            collect(&node.children, ids);
        }
    }
    collect(&doc.roots, &mut ids);
    let mut writable_rate = Vec::new();
    let mut writable_lifetime = Vec::new();
    for id in ids {
        let baseline = doc.node(id).ok_or("Invalid imported node ID")?;
        let rate = if baseline.emission_rate == Some(19.0) {
            21.0
        } else {
            19.0
        };
        let seconds = if baseline.lifetime_seconds == Some(3.0) {
            4.0
        } else {
            3.0
        };
        let check = |lifetime: bool| -> bool {
            let mut attempt = doc.clone();
            let set = if lifetime {
                attempt.set_lifetime_seconds(id, seconds)
            } else {
                attempt.set_emission_rate(id, rate)
            };
            if set.is_err() {
                return false;
            }
            let Ok(encoded) = attempt.compile_template_texture_variant(&effect) else {
                return false;
            };
            let Ok(parsed) = EfkEfc::parse(&encoded) else {
                return false;
            };
            let Ok(reopened) =
                EffectDocument::from_effekseer_template("imported", "Imported Effect", &parsed)
            else {
                return false;
            };
            let Some(node) = reopened.node(id) else {
                return false;
            };
            if lifetime {
                node.lifetime_seconds
                    .is_some_and(|got| (got - seconds).abs() < 0.02)
            } else {
                node.emission_rate
                    .is_some_and(|got| (got - rate).abs() < 0.02)
            }
        };
        if check(false) {
            writable_rate.push(id);
        }
        if check(true) {
            writable_lifetime.push(id);
        }
    }
    let no_op = doc.compile_template_texture_variant(&effect)?;
    if no_op != bytes {
        return Err("Imported Fire no-op writer changed the creator file".into());
    }
    Ok((
        EffectCompatibility::ImportedFire {
            original: effect,
            writable_rate,
            writable_lifetime,
        },
        doc,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;


}
