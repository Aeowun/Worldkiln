// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::animation::sampling::sample_pose;
use crate::animation::{AnimationAsset, Interpolation, QuatKey, TransformTrack, Vec3Key};
use crate::character::rig::{Joint, Skeleton, Transform};
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootMotion {
    InPlace,
    Preserve,
}

#[derive(Clone, Debug)]
pub struct RetargetSettings {
    pub root_motion: RootMotion,
    pub frames_per_second: f32,
    pub source_to_target_orientation: Quat,
    pub translation_scale: f32,
    pub looping: Option<bool>,
    pub clip_looping: BTreeMap<String, bool>,
}
impl Default for RetargetSettings {
    fn default() -> Self {
        Self {
            root_motion: RootMotion::InPlace,
            frames_per_second: 30.0,
            source_to_target_orientation: Quat::IDENTITY,
            translation_scale: 1.0,
            looping: None,
            clip_looping: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClipInfo {
    pub name: String,
    pub duration: f32,
    pub channel_count: usize,
}

#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub skins: Vec<(usize, String, usize)>,
    pub clips: Vec<ClipInfo>,
}

#[derive(Clone, Debug)]
pub struct MappingReport {
    pub pairs: Vec<(usize, usize)>,
    pub unmapped_source: Vec<String>,
    pub unmapped_target: Vec<String>,
    pub ambiguous: Vec<String>,
}
impl MappingReport {
    pub fn mapped(&self) -> usize {
        self.pairs.len()
    }
}

pub struct SourceLibrary {
    pub skeleton: Skeleton,
    pub clips: Vec<AnimationAsset>,
    pub warnings: Vec<String>,
}

fn checked_glb(path: &Path) -> Result<gltf::Gltf, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Reading animation GLB: {e}"))?;
    parse_glb(&bytes)
}
fn parse_glb(bytes: &[u8]) -> Result<gltf::Gltf, String> {
    gltf::Gltf::from_slice(bytes).map_err(|e| format!("Invalid animation GLB: {e}"))
}
const MAX_ARCHIVE_GLB_BYTES: u64 = 256 * 1024 * 1024;
fn archive_entry(path: &Path, entry: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Opening animation archive: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP archive: {e}"))?;
    let mut member = zip
        .by_name(entry)
        .map_err(|e| format!("Archive entry not found: {e}"))?;
    if !member.name().to_ascii_lowercase().ends_with(".glb") || member.is_dir() {
        return Err("Selected archive entry must be a GLB animation file".into());
    }
    if member.size() > MAX_ARCHIVE_GLB_BYTES {
        return Err("Animation GLB archive entry exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    member
        .take(MAX_ARCHIVE_GLB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Reading compressed animation GLB: {e}"))?;
    if bytes.len() as u64 > MAX_ARCHIVE_GLB_BYTES {
        return Err("Animation GLB archive entry exceeds size limit".into());
    }
    Ok(bytes)
}
pub fn inspect_archive(path: &Path) -> Result<Vec<(String, SourceInfo)>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Opening animation archive: {e}"))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP archive: {e}"))?;
    if zip.len() > 4096 {
        return Err("Animation archive contains too many entries".into());
    }
    let mut sources = Vec::new();
    for index in 0..zip.len() {
        let member = zip.by_index(index).map_err(|e| format!("ZIP entry: {e}"))?;
        if member.is_dir() || !member.name().to_ascii_lowercase().ends_with(".glb") {
            continue;
        }
        let entry_name = member.name().to_string();
        drop(member);
        let bytes = archive_entry(path, &entry_name)?;
        sources.push((entry_name, inspect_gltf(&parse_glb(&bytes)?)));
    }
    if sources.is_empty() {
        return Err("No GLB files found in animation archive".into());
    }
    Ok(sources)
}

pub fn inspect(path: &Path) -> Result<SourceInfo, String> {
    let gltf = checked_glb(path)?;
    Ok(inspect_gltf(&gltf))
}
fn inspect_gltf(gltf: &gltf::Gltf) -> SourceInfo {
    let mut skins = gltf
        .skins()
        .map(|s| {
            (
                s.index(),
                s.name().unwrap_or("Unnamed skin").to_string(),
                s.joints().count(),
            )
        })
        .collect::<Vec<_>>();
    if skins.is_empty() && gltf.animations().any(|a| a.channels().next().is_some()) {
        let animated = gltf
            .animations()
            .flat_map(|a| a.channels().map(|ch| ch.target().node().index()))
            .collect::<HashSet<_>>();
        skins.push((0, "Animated Node Hierarchy".into(), animated.len()));
    }
    SourceInfo {
        skins,
        clips: gltf
            .animations()
            .map(|a| {
                let duration = a
                    .channels()
                    .flat_map(|ch| {
                        ch.sampler().input().max().and_then(|max| {
                            max.as_array()
                                .and_then(|arr| arr.first())
                                .and_then(|v| v.as_f64())
                        })
                    })
                    .fold(0.0f32, |v, n| v.max(n as f32));
                ClipInfo {
                    name: a.name().unwrap_or("Unnamed clip").to_string(),
                    duration,
                    channel_count: a.channels().count(),
                }
            })
            .collect(),
    }
}

fn local(node: gltf::Node<'_>) -> Transform {
    let (t, r, s) = node.transform().decomposed();
    Transform {
        translation: Vec3::from_array(t),
        rotation: Quat::from_array(r),
        scale: Vec3::from_array(s),
    }
}

pub fn load_glb(
    path: &Path,
    skin_index: usize,
    selected: &[String],
) -> Result<SourceLibrary, String> {
    load_gltf(checked_glb(path)?, skin_index, selected)
}
pub fn load_zip_glb(
    archive: &Path,
    entry: &str,
    skin_index: usize,
    selected: &[String],
) -> Result<SourceLibrary, String> {
    load_gltf(
        parse_glb(&archive_entry(archive, entry)?)?,
        skin_index,
        selected,
    )
}
fn load_gltf(
    gltf: gltf::Gltf,
    skin_index: usize,
    selected: &[String],
) -> Result<SourceLibrary, String> {
    let has_skins = gltf.skins().next().is_some();
    let skin = gltf.skins().find(|s| s.index() == skin_index);
    if skin.is_none() && (has_skins || skin_index != 0) {
        return Err(format!("GLB skin {skin_index} does not exist"));
    }
    let nodes = gltf.nodes().collect::<Vec<_>>();
    let mut parents = vec![None; nodes.len()];
    for node in &nodes {
        for child in node.children() {
            if parents[child.index()].replace(node.index()).is_some() {
                return Err(format!("Node {} has more than one parent", child.index()));
            }
        }
    }
    let joint_nodes = if let Some(skin) = skin {
        skin.joints().map(|j| j.index()).collect::<Vec<_>>()
    } else {
        let mut nodes_in_scope = HashSet::new();
        for animation in gltf.animations() {
            let name = animation
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("Clip_{}", animation.index()));
            if !selected.is_empty() && !selected.iter().any(|item| item == &name) {
                continue;
            }
            for channel in animation.channels() {
                let mut cursor = Some(channel.target().node().index());
                while let Some(id) = cursor {
                    if !nodes_in_scope.insert(id) {
                        break;
                    }
                    cursor = parents[id];
                }
            }
        }
        nodes_in_scope.into_iter().collect::<Vec<_>>()
    };
    if joint_nodes.is_empty() {
        return Err("Selected GLB has no skin joints or animated nodes".into());
    }
    let selected_nodes = joint_nodes.iter().copied().collect::<HashSet<_>>();
    if selected_nodes.len() != joint_nodes.len() {
        return Err("Duplicate GLB skin joint".into());
    }
    let mut depths = HashMap::<usize, usize>::new();
    for &joint in &joint_nodes {
        let mut cursor = parents[joint];
        let mut depth = 0;
        let mut seen = HashSet::new();
        while let Some(parent) = cursor {
            if !seen.insert(parent) {
                return Err("Cycle in GLB node hierarchy".into());
            }
            depth += 1;
            cursor = parents[parent];
        }
        depths.insert(joint, depth);
    }
    let mut ordered = joint_nodes;
    ordered.sort_by_key(|node| (depths[node], *node));
    let indices = ordered
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect::<HashMap<_, _>>();
    let mut skeleton_joints = Vec::new();
    let mut leading_nonjoint_transforms = Vec::<Mat4>::new();
    let mut used_names = HashSet::new();
    for &joint_node in &ordered {
        let mut chain = vec![joint_node];
        let mut cursor = parents[joint_node];
        while let Some(id) = cursor {
            if selected_nodes.contains(&id) {
                break;
            }
            chain.push(id);
            cursor = parents[id];
        }
        chain.reverse();
        let prefix = chain
            .iter()
            .take(chain.len().saturating_sub(1))
            .fold(Mat4::IDENTITY, |acc, &i| {
                acc * local(nodes[i].clone()).to_matrix()
            });
        leading_nonjoint_transforms.push(prefix);
        let local_matrix = prefix * local(nodes[joint_node].clone()).to_matrix();
        let (scale, rotation, translation) = local_matrix.to_scale_rotation_translation();
        if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
            return Err("Source rig includes invalid bind transforms".into());
        }
        let name = nodes[joint_node].name().unwrap_or("").trim();
        let name = if name.is_empty() {
            format!("Joint_{joint_node}")
        } else {
            name.to_owned()
        };
        if !used_names.insert(name.to_ascii_lowercase()) {
            return Err(format!(
                "Source rig contains duplicate joint name: {name}; rename bones before importing"
            ));
        }
        skeleton_joints.push(Joint {
            name,
            parent_index: cursor.and_then(|id| indices.get(&id).copied()),
            local_transform: Transform {
                translation,
                rotation,
                scale,
            },
        });
    }
    let skeleton = Skeleton::new(skeleton_joints);
    let blob = gltf
        .blob
        .as_deref()
        .ok_or("Animation GLB has no embedded binary buffer")?;
    let chosen = selected.iter().collect::<HashSet<_>>();
    if chosen.len() != selected.len() {
        return Err("Selected clip names must be unique".into());
    }
    let mut warnings = Vec::new();
    let mut clips = Vec::new();
    let mut found = HashSet::new();
    for animation in gltf.animations() {
        let name = animation
            .name()
            .map(str::to_string)
            .unwrap_or_else(|| format!("Clip_{}", animation.index()));
        if !chosen.is_empty() && !chosen.contains(&name) {
            continue;
        }
        if !found.insert(name.clone()) {
            return Err(format!("Duplicate source clip name: {name}"));
        }
        let mut asset = AnimationAsset::new(name.clone(), "source-rig");
        asset.source.imported_clip = Some(name);
        asset.looping = false;
        let mut tracks = BTreeMap::<usize, TransformTrack>::new();
        let mut duration = 0.0f32;
        for channel in animation.channels() {
            let node = channel.target().node().index();
            let Some(&idx) = indices.get(&node) else {
                warnings.push(format!(
                    "Clip {} has non-skin node animation; that channel is ignored",
                    asset.name
                ));
                continue;
            };
            let interpolation = match channel.sampler().interpolation() {
                gltf::animation::Interpolation::Linear => Interpolation::Linear,
                gltf::animation::Interpolation::Step => Interpolation::Step,
                gltf::animation::Interpolation::CubicSpline => {
                    return Err(format!(
                        "Clip {} uses cubic-spline interpolation; conversion requires tangent-aware sampling",
                        asset.name
                    ));
                }
            };
            let reader = channel.reader(|buffer| {
                if buffer.index() == 0 {
                    Some(blob)
                } else {
                    None
                }
            });
            let times = reader
                .read_inputs()
                .ok_or("Animation channel has no key times")?
                .collect::<Vec<_>>();
            if times.is_empty()
                || times.iter().any(|v| !v.is_finite() || *v < 0.0)
                || times.windows(2).any(|w| w[0] >= w[1])
            {
                return Err(format!(
                    "Clip {} has invalid animation key times",
                    asset.name
                ));
            }
            duration = duration.max(*times.last().unwrap());
            let joint_name = skeleton.joints[idx].name.clone();
            let track = tracks.entry(idx).or_insert_with(|| TransformTrack {
                joint_id: joint_name.clone(),
                joint_name,
                ..Default::default()
            });
            let values = reader
                .read_outputs()
                .ok_or("Animation channel has no key values")?;
            match values {
                gltf::animation::util::ReadOutputs::Translations(v) => {
                    if !track.translation.is_empty() {
                        return Err("Duplicate translation track".into());
                    }
                    let vs = v.collect::<Vec<_>>();
                    if vs.len() != times.len() || vs.iter().flatten().any(|f| !f.is_finite()) {
                        return Err("Invalid translation keys".into());
                    }
                    track.translation = times
                        .iter()
                        .zip(vs)
                        .map(|(&time, value)| Vec3Key {
                            time,
                            value: leading_nonjoint_transforms[idx]
                                .transform_point3(Vec3::from_array(value))
                                .to_array(),
                            interpolation,
                            in_tangent: None,
                            out_tangent: None,
                        })
                        .collect();
                }
                gltf::animation::util::ReadOutputs::Rotations(v) => {
                    if !track.rotation.is_empty() {
                        return Err("Duplicate rotation track".into());
                    }
                    let vs = v.into_f32().collect::<Vec<_>>();
                    if vs.len() != times.len()
                        || vs.iter().any(|r| {
                            !Quat::from_array(*r).is_finite()
                                || Quat::from_array(*r).length_squared() < 1e-8
                        })
                    {
                        return Err("Invalid rotation keys".into());
                    }
                    let prefix_rotation = leading_nonjoint_transforms[idx]
                        .to_scale_rotation_translation()
                        .1;
                    track.rotation = times
                        .iter()
                        .zip(vs)
                        .map(|(&time, value)| QuatKey {
                            time,
                            value: (prefix_rotation * Quat::from_array(value))
                                .normalize()
                                .to_array(),
                            interpolation,
                        })
                        .collect();
                }
                gltf::animation::util::ReadOutputs::Scales(v) => {
                    if !track.scale.is_empty() {
                        return Err("Duplicate scale track".into());
                    }
                    let vs = v.collect::<Vec<_>>();
                    if vs.len() != times.len() || vs.iter().flatten().any(|f| !f.is_finite()) {
                        return Err("Invalid scale keys".into());
                    }
                    let prefix_scale = leading_nonjoint_transforms[idx]
                        .to_scale_rotation_translation()
                        .0;
                    track.scale = times
                        .iter()
                        .zip(vs)
                        .map(|(&time, value)| Vec3Key {
                            time,
                            value: (prefix_scale * Vec3::from_array(value)).to_array(),
                            interpolation,
                            in_tangent: None,
                            out_tangent: None,
                        })
                        .collect();
                }
                gltf::animation::util::ReadOutputs::MorphTargetWeights(_) => {
                    warnings.push(format!(
                        "Clip {} has morph-target animation not supported by skeletal retargeting",
                        asset.name
                    ));
                }
            }
        }
        asset.duration = duration.max(0.001);
        asset.tracks = tracks.into_values().collect();
        asset.validate()?;
        clips.push(asset);
    }
    if !chosen.is_empty() {
        let missing = chosen
            .into_iter()
            .filter(|n| !found.contains(*n))
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(format!("Selected clips unavailable: {missing:?}"));
        }
    }
    if clips.is_empty() {
        return Err("GLB has no selected usable animations".into());
    }
    Ok(SourceLibrary {
        skeleton,
        clips,
        warnings,
    })
}

fn normalized(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let mut tokens = Vec::new();
    let mut word = String::new();
    for c in lower.chars() {
        if c.is_ascii_alphanumeric() {
            word.push(c);
        } else if !word.is_empty() {
            tokens.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    while tokens.first().is_some_and(|s| {
        matches!(
            s.as_str(),
            "cc" | "base" | "mixamorig" | "armature" | "bone" | "def" | "bip" | "bip001"
        )
    }) {
        tokens.remove(0);
    }
    if tokens
        .last()
        .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()))
        && (tokens.len() >= 3
            || tokens
                .get(tokens.len().saturating_sub(2))
                .is_some_and(|s| s.chars().last().is_some_and(|c| c.is_ascii_digit())))
    {
        tokens.pop();
    }
    tokens.join("")
}
fn role(name: &str) -> Option<(String, u8)> {
    let mut n = normalized(name);
    let side = if n.starts_with("left") {
        n = n[4..].to_owned();
        1
    } else if n.starts_with("right") {
        n = n[5..].to_owned();
        2
    } else if n.ends_with('l')
        && [
            "arm", "upperarm", "lowerarm", "forearm", "hand", "wrist", "clavicle", "shoulder",
            "thigh", "upperleg", "lowerleg", "calf", "shin", "foot", "toe", "ankle", "thumb",
            "index", "middle", "pinky", "ring",
        ]
        .iter()
        .any(|part| {
            &n[..n.len() - 1] == *part
                || (n[..n.len() - 1].starts_with(part)
                    && n[part.len()..n.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_digit()))
        })
    {
        n.pop();
        1
    } else if n.ends_with('r')
        && [
            "arm", "upperarm", "lowerarm", "forearm", "hand", "wrist", "clavicle", "shoulder",
            "thigh", "upperleg", "lowerleg", "calf", "shin", "foot", "toe", "ankle", "thumb",
            "index", "middle", "pinky", "ring",
        ]
        .iter()
        .any(|part| {
            &n[..n.len() - 1] == *part
                || (n[..n.len() - 1].starts_with(part)
                    && n[part.len()..n.len() - 1]
                        .chars()
                        .all(|c| c.is_ascii_digit()))
        })
    {
        n.pop();
        2
    } else if n.starts_with('l')
        && [
            "upperarm", "lowerarm", "forearm", "arm", "thigh", "upperleg", "lowerleg", "calf",
            "shin", "hand", "foot", "toe", "clavicle", "shoulder", "wrist", "ankle",
        ]
        .iter()
        .any(|part| n[1..].starts_with(part))
    {
        n.remove(0);
        1
    } else if n.starts_with('r')
        && [
            "upperarm", "lowerarm", "forearm", "arm", "thigh", "upperleg", "lowerleg", "calf",
            "shin", "hand", "foot", "toe", "clavicle", "shoulder", "wrist", "ankle",
        ]
        .iter()
        .any(|part| n[1..].starts_with(part))
    {
        n.remove(0);
        2
    } else {
        0
    };
    let aliases = [
        ("hip", "pelvis"),
        ("hips", "pelvis"),
        ("root", "root"),
        ("collar", "clavicle"),
        ("shoulder", "clavicle"),
        ("upperarm", "upperarm"),
        ("arm", "upperarm"),
        ("forearm", "lowerarm"),
        ("lowerarm", "lowerarm"),
        ("upperleg", "upperleg"),
        ("thigh", "upperleg"),
        ("calf", "lowerleg"),
        ("shin", "lowerleg"),
        ("lowerleg", "lowerleg"),
        ("ankle", "foot"),
        ("wrist", "hand"),
        ("toe", "toe"),
        ("spine", "spine"),
        ("chest", "chest"),
        ("neck", "neck"),
        ("head", "head"),
        ("hand", "hand"),
        ("foot", "foot"),
        ("pelvis", "pelvis"),
        ("thumb", "thumb"),
        ("index", "index"),
        ("middle", "middle"),
        ("ring", "ring"),
        ("pinky", "pinky"),
        ("ball", "toe"),
    ];
    for (alias, canonical) in aliases {
        if n == alias
            || n.starts_with(alias) && n[alias.len()..].chars().all(|c| c.is_ascii_digit())
        {
            let suffix = &n[alias.len()..];
            let number = if suffix.is_empty() {
                String::new()
            } else {
                suffix
                    .parse::<usize>()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|_| suffix.to_owned())
            };
            return Some((format!("{canonical}{number}"), side));
        }
    }
    None
}

pub fn map_joints(
    source: &Skeleton,
    target: &Skeleton,
    manual: &BTreeMap<String, String>,
) -> Result<MappingReport, String> {
    let mut pairs = Vec::<(usize, usize)>::new();
    let mut source_used = HashSet::new();
    let mut target_used = HashSet::new();
    for (source_name, target_name) in manual {
        let s = source
            .joint_index(source_name)
            .ok_or_else(|| format!("Unknown source joint: {source_name}"))?;
        if target_name.is_empty() {
            if !source_used.insert(s) {
                return Err("Duplicate ignored source joint".into());
            }
            continue;
        }
        let t = target
            .joint_index(target_name)
            .ok_or_else(|| format!("Unknown target joint: {target_name}"))?;
        if !source_used.insert(s) || !target_used.insert(t) {
            return Err("Manual joint map must be one-to-one".into());
        }
        pairs.push((s, t));
    }
    let mut ambiguous = Vec::new();
    for (s, joint) in source.joints.iter().enumerate() {
        if source_used.contains(&s) {
            continue;
        }
        let simple = normalized(&joint.name);
        let source_role = role(&joint.name);
        let scored = target
            .joints
            .iter()
            .enumerate()
            .filter(|(t, _)| !target_used.contains(t))
            .filter_map(|(t, destination)| {
                let dst = normalized(&destination.name);
                let destination_role = role(&destination.name);
                let score = if simple == dst {
                    100
                } else if source_role.is_some() && source_role == destination_role {
                    80
                } else if dst.starts_with(&simple)
                    && !simple.is_empty()
                    && dst[simple.len()..].chars().all(|c| c.is_ascii_digit())
                    && !dst[simple.len()..].is_empty()
                {
                    65
                } else if source_role
                    .as_ref()
                    .zip(destination_role.as_ref())
                    .is_some_and(|((a, side_a), (b, side_b))| {
                        side_a == side_b
                            && a.trim_end_matches(|c: char| c.is_ascii_digit())
                                == b.trim_end_matches(|c: char| c.is_ascii_digit())
                    })
                {
                    50
                } else {
                    0
                };
                (score > 0).then_some((t, score))
            })
            .collect::<Vec<_>>();
        let best = scored.iter().map(|x| x.1).max().unwrap_or(0);
        let candidates = scored
            .into_iter()
            .filter(|x| x.1 == best)
            .collect::<Vec<_>>();
        if candidates.len() == 1 {
            let t = candidates[0].0;
            source_used.insert(s);
            target_used.insert(t);
            pairs.push((s, t));
        } else if candidates.len() > 1 {
            ambiguous.push(format!(
                "{} matches multiple destination joints",
                joint.name
            ));
        }
    }
    if !pairs.is_empty() {
        let source_roots = source
            .joints
            .iter()
            .enumerate()
            .filter(|(_, j)| j.parent_index.is_none())
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let target_roots = target
            .joints
            .iter()
            .enumerate()
            .filter(|(_, j)| j.parent_index.is_none())
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if source_roots.len() == 1 && target_roots.len() == 1 {
            let s = source_roots[0];
            let t = target_roots[0];
            if !source_used.contains(&s) && !target_used.contains(&t) {
                source_used.insert(s);
                target_used.insert(t);
                pairs.push((s, t));
            }
        }
    }
    pairs.sort_unstable();
    Ok(MappingReport {
        unmapped_source: source
            .joints
            .iter()
            .enumerate()
            .filter(|(i, _)| !source_used.contains(i))
            .map(|(_, j)| j.name.clone())
            .collect(),
        unmapped_target: target
            .joints
            .iter()
            .enumerate()
            .filter(|(i, _)| !target_used.contains(i))
            .map(|(_, j)| j.name.clone())
            .collect(),
        pairs,
        ambiguous,
    })
}

fn body_up_axis(skeleton: &Skeleton) -> Option<Vec3> {
    let mut pelvis = None;
    let mut head = None;
    for (index, joint) in skeleton.joints.iter().enumerate() {
        let semantic = role(&joint.name).map(|(label, _)| label);
        if semantic
            .as_deref()
            .is_some_and(|label| label.starts_with("head"))
        {
            head.get_or_insert(index);
        } else if semantic
            .as_deref()
            .is_some_and(|label| label.starts_with("pelvis"))
        {
            pelvis.get_or_insert(index);
        }
    }
    let (Some(pelvis), Some(head)) = (pelvis, head) else {
        return None;
    };
    let rest = skeleton
        .joints
        .iter()
        .map(|joint| joint.local_transform)
        .collect::<Vec<_>>();
    let global = skeleton.evaluate_pose(&rest);
    let direction = (global.matrices[head].transform_point3(Vec3::ZERO)
        - global.matrices[pelvis].transform_point3(Vec3::ZERO))
    .normalize_or_zero();
    (direction.is_finite() && direction.length_squared() > 0.5).then_some(direction)
}
fn inferred_rig_alignment(source: &Skeleton, target: &Skeleton) -> Quat {
    match (body_up_axis(source), body_up_axis(target)) {
        (Some(from), Some(to)) => Quat::from_rotation_arc(from, to),
        _ => Quat::IDENTITY,
    }
}

fn approx_height(skeleton: &Skeleton) -> f32 {
    if skeleton.joints.is_empty() {
        return 1.0;
    }
    let pose = skeleton
        .joints
        .iter()
        .map(|j| j.local_transform)
        .collect::<Vec<_>>();
    let eval = skeleton.evaluate_pose(&pose);
    let (mut min, mut max) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for mat in &eval.matrices {
        let point = mat.w_axis.truncate();
        min = min.min(point);
        max = max.max(point);
    }
    (max - min).length().max(0.01)
}

pub fn retarget_clip(
    source: &Skeleton,
    target: &Skeleton,
    clip: &AnimationAsset,
    map: &MappingReport,
    settings: &RetargetSettings,
    target_id: &str,
) -> Result<AnimationAsset, String> {
    if !settings.frames_per_second.is_finite()
        || !(1.0..=120.0).contains(&settings.frames_per_second)
    {
        return Err("Retarget sampling FPS must be between 1 and 120".into());
    }
    if source.joints.is_empty() || target.joints.is_empty() || map.pairs.is_empty() {
        return Err("Retargeting needs nonempty source, destination and joint mapping".into());
    }
    if !settings.source_to_target_orientation.is_finite()
        || settings.source_to_target_orientation.length_squared() < 1e-8
        || !settings.translation_scale.is_finite()
        || !(0.001..=1000.0).contains(&settings.translation_scale)
    {
        return Err("Retarget orientation or translation scale is invalid".into());
    }
    let alignment = (settings.source_to_target_orientation.normalize()
        * inferred_rig_alignment(source, target))
    .normalize();
    clip.validate()?;
    let frames = ((clip.duration * settings.frames_per_second).ceil() as usize).saturating_add(1);
    if frames > 21601 {
        return Err(format!(
            "Clip requires {frames} sampled frames at {} FPS (limit 21601). Choose a lower sampling rate or split the source clip.",
            settings.frames_per_second
        ));
    }
    let source_rest = source
        .joints
        .iter()
        .map(|j| j.local_transform)
        .collect::<Vec<_>>();
    let target_rest = target
        .joints
        .iter()
        .map(|j| j.local_transform)
        .collect::<Vec<_>>();
    let src_global = source.evaluate_pose(&source_rest);
    let dst_global = target.evaluate_pose(&target_rest);
    let src_quats = src_global
        .matrices
        .iter()
        .map(|m| m.to_scale_rotation_translation().1)
        .collect::<Vec<_>>();
    let dst_quats = dst_global
        .matrices
        .iter()
        .map(|m| m.to_scale_rotation_translation().1)
        .collect::<Vec<_>>();
    let mut target_to_source = vec![None; target.joints.len()];
    for &(s, t) in &map.pairs {
        if s >= source.joints.len() || t >= target.joints.len() || target_to_source[t].is_some() {
            return Err("Invalid or duplicate retarget joint mapping".into());
        }
        target_to_source[t] = Some(s);
    }
    let height_scale = ((approx_height(target) / approx_height(source))
        * settings.translation_scale)
        .clamp(0.001, 1000.0);
    let mut result = AnimationAsset::new(clip.name.clone(), target_id);
    result.source.imported_clip = Some(clip.name.clone());
    result.frames_per_second = settings.frames_per_second;
    result.looping = settings
        .clip_looping
        .get(&clip.name)
        .copied()
        .or(settings.looping)
        .unwrap_or(clip.looping);
    result.duration = clip.duration;
    let mut tracks = target
        .joints
        .iter()
        .map(|j| TransformTrack {
            joint_id: j.name.clone(),
            joint_name: j.name.clone(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    for frame in 0..frames {
        let time = (frame as f32 / settings.frames_per_second).min(clip.duration);
        let src_local = sample_pose(clip, source, time);
        let src_anim = source.evaluate_pose(&src_local);
        let mut dst_rot_global = vec![Quat::IDENTITY; target.joints.len()];
        for (t, joint) in target.joints.iter().enumerate() {
            let rotation = if let Some(s) = target_to_source[t] {
                let current = src_anim.matrices[s].to_scale_rotation_translation().1;
                let source_delta = current * src_quats[s].inverse();
                let delta = alignment * source_delta * alignment.inverse();
                let desired = (delta * dst_quats[t]).normalize();
                if let Some(parent) = joint.parent_index {
                    (dst_rot_global[parent].inverse() * desired).normalize()
                } else {
                    desired
                }
            } else {
                joint.local_transform.rotation
            };
            if !rotation.is_finite() || rotation.length_squared() < 1e-8 {
                return Err(format!(
                    "Retarget produced an invalid rotation for joint {}",
                    joint.name
                ));
            }
            dst_rot_global[t] = if let Some(parent) = joint.parent_index {
                (dst_rot_global[parent] * rotation).normalize()
            } else {
                rotation
            };
            if target_to_source[t].is_some() {
                tracks[t].rotation.push(QuatKey {
                    time,
                    value: rotation.to_array(),
                    interpolation: Interpolation::Linear,
                });
            }
        }
        for (t, joint) in target.joints.iter().enumerate() {
            let Some(s) = target_to_source[t] else {
                continue;
            };
            if !(joint.parent_index.is_none()
                || role(&joint.name).is_some_and(|(r, _)| r == "pelvis"))
            {
                continue;
            }
            let delta = src_local[s].translation - source_rest[s].translation;
            let src_parent_rot = source.joints[s]
                .parent_index
                .map(|i| src_quats[i])
                .unwrap_or(Quat::IDENTITY);
            let dst_parent_rot = joint
                .parent_index
                .map(|i| dst_quats[i])
                .unwrap_or(Quat::IDENTITY);
            let mut movement =
                dst_parent_rot.inverse() * alignment * src_parent_rot * delta * height_scale;
            if joint.parent_index.is_none() && settings.root_motion == RootMotion::InPlace {
                movement.x = 0.0;
                movement.z = 0.0;
            }
            let value = (joint.local_transform.translation + movement).to_array();
            if !value.iter().all(|v| v.is_finite()) {
                return Err("Retarget produced invalid position keys".into());
            }
            tracks[t].translation.push(Vec3Key {
                time,
                value,
                interpolation: Interpolation::Linear,
                in_tangent: None,
                out_tangent: None,
            });
        }
    }
    result.tracks = tracks
        .into_iter()
        .enumerate()
        .filter(|(i, _)| target_to_source[*i].is_some())
        .map(|(_, mut track)| {
            if track.rotation.len() > 1 {
                let baseline = Quat::from_array(track.rotation[0].value);
                if track
                    .rotation
                    .iter()
                    .all(|k| baseline.dot(Quat::from_array(k.value)).abs() > 0.999999)
                {
                    track.rotation.truncate(1);
                }
            }
            if track.translation.len() > 1 {
                let baseline = Vec3::from_array(track.translation[0].value);
                if track
                    .translation
                    .iter()
                    .all(|k| (Vec3::from_array(k.value) - baseline).length_squared() < 1e-9)
                {
                    track.translation.truncate(1);
                }
            }
            track
        })
        .filter(|track| !track.rotation.is_empty() || !track.translation.is_empty())
        .collect();
    result.normalize();
    result.validate()?;
    Ok(result)
}

pub fn import_clips(
    project: &Path,
    target_asset_id: &str,
    source_glb: &Path,
    skin: usize,
    names: &[String],
    manual: &BTreeMap<String, String>,
    settings: &RetargetSettings,
) -> Result<(Vec<String>, MappingReport, Vec<String>), String> {
    if names.is_empty() {
        return Err("Select at least one animation clip".into());
    }
    let library = load_glb(source_glb, skin, names)?;
    import_library(project, target_asset_id, library, manual, settings)
}

pub fn import_archive_clips(
    project: &Path,
    target_asset_id: &str,
    archive: &Path,
    entry: &str,
    skin: usize,
    names: &[String],
    manual: &BTreeMap<String, String>,
    settings: &RetargetSettings,
) -> Result<(Vec<String>, MappingReport, Vec<String>), String> {
    if names.is_empty() {
        return Err("Select at least one animation clip".into());
    }
    let library = load_zip_glb(archive, entry, skin, names)?;
    import_library(project, target_asset_id, library, manual, settings)
}
fn same_animation_motion(a: &AnimationAsset, b: &AnimationAsset) -> bool {
    if a.source.rig_asset_id != b.source.rig_asset_id {
        return false;
    }
    let normalize = |original: &AnimationAsset| {
        let mut value = original.clone();
        value.id.clear();
        value.name.clear();
        value.source.imported_clip = None;
        value.looping = false;
        value
    };
    normalize(a) == normalize(b)
}

fn import_library(
    project: &Path,
    target_asset_id: &str,
    library: SourceLibrary,
    manual: &BTreeMap<String, String>,
    settings: &RetargetSettings,
) -> Result<(Vec<String>, MappingReport, Vec<String>), String> {
    let registry = crate::project::asset_registry::AssetRegistry::load(project)?;
    let rig = registry
        .find(target_asset_id)
        .ok_or("Target rig AssetID is unavailable")?;
    if !matches!(
        rig.kind,
        crate::project::asset_registry::AssetType::Actor
            | crate::project::asset_registry::AssetType::Character
    ) {
        return Err("Target must be an Actor or Character rig".into());
    }
    let target = crate::character::package::load_package(&project.join(&rig.path))?.skeleton;
    let mapping = map_joints(&library.skeleton, &target, manual)?;
    if mapping.pairs.is_empty() {
        return Err("No compatible joints mapped; provide manual mappings".into());
    }
    let mut warnings = library.warnings.clone();
    if !mapping.unmapped_source.is_empty() {
        warnings.push(format!(
            "{} source joints have no destination mapping; they remain excluded",
            mapping.unmapped_source.len()
        ));
    }
    if !mapping.unmapped_target.is_empty() {
        warnings.push(format!(
            "{} destination joints retain their authored rest-pose behavior",
            mapping.unmapped_target.len()
        ));
    }
    for ambiguity in &mapping.ambiguous {
        warnings.push(format!("Unresolved bone mapping: {ambiguity}"));
    }
    let mut documents = Vec::new();
    for clip in &library.clips {
        documents.push(retarget_clip(
            &library.skeleton,
            &target,
            clip,
            &mapping,
            settings,
            target_asset_id,
        )?);
    }
    let mut existing = registry
        .by_type(crate::project::asset_registry::AssetType::Animation)
        .filter(|record| record.dependencies.iter().any(|dep| dep == target_asset_id))
        .filter_map(|record| {
            let path = project
                .join(&record.path)
                .join(crate::animation::asset::ANIMATION_FILE);
            AnimationAsset::load(&path)
                .ok()
                .map(|clip| (clip, record.id.clone()))
        })
        .collect::<Vec<_>>();
    let mut saved_ids = Vec::new();
    let mut new_documents = Vec::new();
    for document in documents {
        if let Some((prior, id)) = existing
            .iter_mut()
            .find(|(old, _)| same_animation_motion(old, &document))
        {
            if prior.looping != document.looping {
                prior.looping = document.looping;
                prior.save_authoritative(project)?;
            }
            warnings.push(format!(
                "{} already exists for this rig; reusing its animation asset",
                document.name
            ));
            saved_ids.push(id.clone());
        } else {
            let path = crate::animation::asset::animation_asset_path(project, &document.id)?;
            if path.exists() {
                return Err("Generated Animation AssetID already exists".into());
            }
            existing.push((document.clone(), document.id.clone()));
            saved_ids.push(document.id.clone());
            new_documents.push(document);
        }
    }
    for asset in &new_documents {
        asset.save_authoritative(project)?;
    }
    crate::project::asset_registry::AssetRegistry::reconcile(project)?;
    Ok((saved_ids, mapping, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn joint(name: &str, parent: Option<usize>, pos: [f32; 3], rot: Quat) -> Joint {
        Joint {
            name: name.into(),
            parent_index: parent,
            local_transform: Transform {
                translation: Vec3::from_array(pos),
                rotation: rot,
                scale: Vec3::ONE,
            },
        }
    }
    #[test]
    fn auto_mapping_independent_of_exporter_prefix_and_joint_order() {
        let source = Skeleton::new(vec![
            joint("mixamorig:Hips", None, [0.0, 1.0, 0.0], Quat::IDENTITY),
            joint(
                "mixamorig:LeftArm",
                Some(0),
                [-0.4, 0.5, 0.0],
                Quat::IDENTITY,
            ),
            joint(
                "mixamorig:RightArm",
                Some(0),
                [0.4, 0.5, 0.0],
                Quat::IDENTITY,
            ),
        ]);
        let destination = Skeleton::new(vec![
            joint("pelvis", None, [0.0, 2.0, 0.0], Quat::IDENTITY),
            joint("upperarm_r", Some(0), [0.8, 1.0, 0.0], Quat::IDENTITY),
            joint("upperarm_l", Some(0), [-0.8, 1.0, 0.0], Quat::IDENTITY),
        ]);
        let mapping = map_joints(&source, &destination, &BTreeMap::new()).unwrap();
        assert_eq!(mapping.mapped(), 3);
        assert!(mapping.pairs.contains(&(1, 2)));
        assert!(mapping.pairs.contains(&(2, 1)));
    }
    #[test]
    fn rest_pose_transfer_preserves_target_proportions_and_unmapped_joints() {
        let source = Skeleton::new(vec![
            joint("root", None, [0.0, 0.0, 0.0], Quat::IDENTITY),
            joint("leftarm", Some(0), [1.0, 0.0, 0.0], Quat::IDENTITY),
        ]);
        let dst = Skeleton::new(vec![
            joint("root", None, [0.0, 0.0, 0.0], Quat::IDENTITY),
            joint(
                "upperarm_l",
                Some(0),
                [2.0, 0.0, 0.0],
                Quat::from_rotation_z(0.4),
            ),
            joint("unmapped", Some(1), [0.0, 3.0, 0.0], Quat::IDENTITY),
        ]);
        let map = map_joints(&source, &dst, &BTreeMap::new()).unwrap();
        let mut clip = AnimationAsset::new("wave", "source");
        clip.tracks.push(TransformTrack {
            joint_id: "leftarm".into(),
            joint_name: "leftarm".into(),
            rotation: vec![
                QuatKey {
                    time: 0.0,
                    value: Quat::IDENTITY.to_array(),
                    interpolation: Interpolation::Linear,
                },
                QuatKey {
                    time: 1.0,
                    value: Quat::from_rotation_z(0.7).to_array(),
                    interpolation: Interpolation::Linear,
                },
            ],
            ..Default::default()
        });
        let retargeted = retarget_clip(
            &source,
            &dst,
            &clip,
            &map,
            &RetargetSettings::default(),
            "target",
        )
        .unwrap();
        let rest = sample_pose(&retargeted, &dst, 0.0);
        assert!((rest[1].rotation.dot(dst.joints[1].local_transform.rotation)).abs() > 0.99999);
        assert_eq!(
            rest[1].translation,
            dst.joints[1].local_transform.translation
        );
        assert_eq!(rest[2], dst.joints[2].local_transform);
        let animated = sample_pose(&retargeted, &dst, 0.5);
        assert_ne!(animated[1].rotation, rest[1].rotation);
    }
    #[test]
    fn mismatched_rest_up_axes_transfer_motion_in_target_space() {
        let source = Skeleton::new(vec![
            joint("Hips", None, [0.0, 0.0, 0.0], Quat::IDENTITY),
            joint("Head", Some(0), [0.0, 1.0, 0.0], Quat::IDENTITY),
        ]);
        let target = Skeleton::new(vec![
            joint("pelvis", None, [0.0, 0.0, 0.0], Quat::IDENTITY),
            joint("head", Some(0), [0.0, 0.0, 1.0], Quat::IDENTITY),
        ]);
        let mapping = map_joints(&source, &target, &BTreeMap::new()).unwrap();
        let mut clip = AnimationAsset::new("Forward", "source");
        clip.duration = 1.0;
        clip.tracks.push(TransformTrack {
            joint_id: "Hips".into(),
            joint_name: "Hips".into(),
            translation: vec![
                Vec3Key {
                    time: 0.0,
                    value: [0.0, 0.0, 0.0],
                    interpolation: Interpolation::Linear,
                    in_tangent: None,
                    out_tangent: None,
                },
                Vec3Key {
                    time: 1.0,
                    value: [0.0, 1.0, 0.0],
                    interpolation: Interpolation::Linear,
                    in_tangent: None,
                    out_tangent: None,
                },
            ],
            ..Default::default()
        });
        let settings = RetargetSettings {
            root_motion: RootMotion::Preserve,
            ..Default::default()
        };
        let output = retarget_clip(&source, &target, &clip, &mapping, &settings, "target").unwrap();
        let pose = sample_pose(&output, &target, 0.8);
        assert!(
            pose[0].translation.z > 0.7,
            "Root motion must follow the destination skeleton's axis"
        );
        assert!(pose[0].translation.y.abs() < 0.05);
        assert!((target.joints[1].local_transform.translation - Vec3::Z).length() < 0.0001);
    }

    #[test]
    fn per_clip_loop_flags_are_creator_selected_not_inferred_from_names() {
        let source = Skeleton::new(vec![joint("root", None, [0.0; 3], Quat::IDENTITY)]);
        let target = source.clone();
        let mapping = map_joints(&source, &target, &BTreeMap::new()).unwrap();
        let mut first = AnimationAsset::new("Movement_Loop", "source");
        first.looping = false;
        let default_clip = retarget_clip(
            &source,
            &target,
            &first,
            &mapping,
            &RetargetSettings::default(),
            "target",
        )
        .unwrap();
        assert!(!default_clip.looping);
        let mut settings = RetargetSettings::default();
        settings.clip_looping.insert(first.name.clone(), true);
        let custom_clip =
            retarget_clip(&source, &target, &first, &mapping, &settings, "target").unwrap();
        assert!(custom_clip.looping);
        let mut unrelated = AnimationAsset::new("Attack", "source");
        unrelated.looping = false;
        let other_clip =
            retarget_clip(&source, &target, &unrelated, &mapping, &settings, "target").unwrap();
        assert!(!other_clip.looping);
    }

    #[test]
    fn in_place_and_preserve_root_motion_behave_differently() {
        let source = Skeleton::new(vec![
            joint("root", None, [0.0, 0.0, 0.0], Quat::IDENTITY),
            joint("child", Some(0), [0.0, 1.0, 0.0], Quat::IDENTITY),
        ]);
        let target = Skeleton::new(vec![
            joint("root", None, [0.0, 0.0, 0.0], Quat::IDENTITY),
            joint("child", Some(0), [0.0, 2.0, 0.0], Quat::IDENTITY),
        ]);
        let mapping = map_joints(&source, &target, &BTreeMap::new()).unwrap();
        let mut clip = AnimationAsset::new("Travel", "source");
        clip.tracks.push(TransformTrack {
            joint_id: "root".into(),
            joint_name: "root".into(),
            translation: vec![
                Vec3Key {
                    time: 0.0,
                    value: [0.0; 3],
                    interpolation: Interpolation::Linear,
                    in_tangent: None,
                    out_tangent: None,
                },
                Vec3Key {
                    time: 1.0,
                    value: [1.0, 0.0, 0.0],
                    interpolation: Interpolation::Linear,
                    in_tangent: None,
                    out_tangent: None,
                },
            ],
            ..Default::default()
        });
        let in_place = retarget_clip(
            &source,
            &target,
            &clip,
            &mapping,
            &RetargetSettings::default(),
            "target",
        )
        .unwrap();
        let preserve = retarget_clip(
            &source,
            &target,
            &clip,
            &mapping,
            &RetargetSettings {
                root_motion: RootMotion::Preserve,
                ..Default::default()
            },
            "target",
        )
        .unwrap();
        assert!(sample_pose(&in_place, &target, 0.9)[0].translation.x.abs() < 0.0001);
        assert!(sample_pose(&preserve, &target, 0.9)[0].translation.x > 1.0);
    }
    #[test]
    fn ambiguous_auto_mapping_does_not_guess() {
        let source = Skeleton::new(vec![joint("LeftArm", None, [0.0; 3], Quat::IDENTITY)]);
        let target = Skeleton::new(vec![
            joint("upperarm_l", None, [0.0; 3], Quat::IDENTITY),
            joint("L_UpperArm", None, [0.0; 3], Quat::IDENTITY),
        ]);
        let mapping = map_joints(&source, &target, &BTreeMap::new()).unwrap();
        assert_eq!(mapping.mapped(), 0);
        assert_eq!(mapping.ambiguous.len(), 1);
    }
    #[test]
    fn unsided_anatomical_names_are_not_mistaken_for_side_suffixes() {
        assert_eq!(role("shoulder"), Some(("clavicle".into(), 0)));
        assert_eq!(role("collar"), Some(("clavicle".into(), 0)));
        assert_eq!(role("upperarm_r"), Some(("upperarm".into(), 2)));
        assert_eq!(role("upperarm_l"), Some(("upperarm".into(), 1)));
        assert_eq!(role("R_UpperArm"), Some(("upperarm".into(), 2)));
        assert_eq!(role("LeftArm"), Some(("upperarm".into(), 1)));
        assert_eq!(role("tail"), None);
    }

    #[test]
    fn identical_animation_data_is_scoped_to_target_rig_not_name() {
        let a = AnimationAsset::new("Walk_Loop", "ActorA");
        let mut b = a.clone();
        b.id = "renamed-asset-id".into();
        b.name = "Anything".into();
        b.source.imported_clip = Some("RenamedSource".into());
        b.looping = !a.looping;
        assert!(same_animation_motion(&a, &b));
        b.source.rig_asset_id = "ActorB".into();
        assert!(!same_animation_motion(&a, &b));
    }

    #[test]
    fn unknown_bones_remain_unmapped_manual_names_are_strict() {
        let a = Skeleton::new(vec![joint("bone_123", None, [0.0; 3], Quat::IDENTITY)]);
        let b = Skeleton::new(vec![joint("strange_99", None, [0.0; 3], Quat::IDENTITY)]);
        assert_eq!(map_joints(&a, &b, &BTreeMap::new()).unwrap().mapped(), 0);
        assert!(
            map_joints(
                &a,
                &b,
                &BTreeMap::from([("nonexistent".into(), "strange_99".into())])
            )
            .is_err()
        );
        assert_eq!(
            map_joints(
                &a,
                &b,
                &BTreeMap::from([("bone_123".into(), "strange_99".into())])
            )
            .unwrap()
            .mapped(),
            1
        );
    }
}
