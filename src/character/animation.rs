// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::package::CharacterPackage;
use super::rig::{Skeleton, Transform};
use glam::{Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationState {
    Idle,
    Walk,
    Run,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterAnimation {
    pub current_state: AnimationState,
}

impl Default for CharacterAnimation {
    fn default() -> Self {
        Self {
            current_state: AnimationState::Idle,
        }
    }
}

impl CharacterAnimation {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetAnimation {
    Idle,
    Walk,
    Run,
    Named(String),
}

#[derive(Clone, Debug)]
pub struct Keyframe<T> {
    pub time: f32,
    pub value: T,
    pub step: bool,
}

#[derive(Clone, Debug)]
pub struct TransformTrack {
    pub joint_index: usize,
    pub translations: Vec<Keyframe<Vec3>>,
    pub rotations: Vec<Keyframe<Quat>>,
    pub scales: Vec<Keyframe<Vec3>>,
}

impl TransformTrack {
    pub fn evaluate(&self, time: f32) -> Transform {
        self.evaluate_from(time, Transform::default())
    }

    pub fn evaluate_from(&self, time: f32, rest: Transform) -> Transform {
        let translation = sample_track(&self.translations, time, rest.translation, |a, b, t| {
            a.lerp(*b, t)
        });
        let rotation = sample_track(&self.rotations, time, rest.rotation, |a, b, t| {
            a.slerp(*b, t)
        });
        let scale = sample_track(&self.scales, time, rest.scale, |a, b, t| a.lerp(*b, t));

        Transform {
            translation,
            rotation,
            scale,
        }
    }
}

fn sample_track<T: Clone>(
    keyframes: &[Keyframe<T>],
    time: f32,
    default: T,
    lerp_fn: impl Fn(&T, &T, f32) -> T,
) -> T {
    if keyframes.is_empty() {
        return default;
    }
    if keyframes.len() == 1 || time <= keyframes[0].time {
        return keyframes[0].value.clone();
    }
    if time >= keyframes.last().unwrap().time {
        return keyframes.last().unwrap().value.clone();
    }

    for window in keyframes.windows(2) {
        if time >= window[0].time && time < window[1].time {
            let duration = window[1].time - window[0].time;
            let t = if duration > 0.0 {
                (time - window[0].time) / duration
            } else {
                0.0
            };
            return if window[0].step {
                window[0].value.clone()
            } else {
                lerp_fn(&window[0].value, &window[1].value, t)
            };
        }
    }

    default
}

#[derive(Clone, Debug)]
pub struct AnimationClip {
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    pub tracks: Vec<TransformTrack>,
    pub events: Vec<crate::animation::AnimationEvent>,
}

impl AnimationClip {
    pub fn evaluate(&self, time: f32, out_transforms: &mut [Transform]) {
        for track in &self.tracks {
            if track.joint_index < out_transforms.len() {
                let rest = out_transforms[track.joint_index];
                out_transforms[track.joint_index] = track.evaluate_from(time, rest);
            }
        }
    }
}

fn make_kenney_swim_clips(skeleton: &Skeleton) -> Vec<AnimationClip> {
    if !["arm-left", "arm-right", "leg-left", "leg-right"]
        .iter()
        .all(|name| skeleton.joint_index(name).is_some())
    {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(2);
    for (name, duration, travel) in [("SwimIdle", 1.55_f32, false), ("Swim", 0.78_f32, true)] {
        let mut tracks = Vec::new();
        for (i, joint) in skeleton.joints.iter().enumerate() {
            let joint_name = joint.name.to_ascii_lowercase();
            let is_leg = joint_name == "leg-left" || joint_name == "leg-right";
            let is_arm = joint_name == "arm-left" || joint_name == "arm-right";
            let root = joint_name == "root";
            if !is_leg && !is_arm && !root {
                continue;
            }
            let mut rotations = Vec::with_capacity(5);
            for k in 0..5 {
                let phase = std::f32::consts::TAU * k as f32 / 4.0;
                let flutter = phase.sin() * (if travel { 0.39 } else { 0.13 });
                let rest = joint.local_transform.rotation;
                let rotation = if root {
                    Quat::from_rotation_x(if travel { 1.20 } else { 0.35 }) * rest
                } else if is_leg {
                    let opposite = if joint_name == "leg-left" { 1.0 } else { -1.0 };
                    Quat::from_rotation_x(flutter * opposite) * rest
                } else {
                    let outward = if joint_name == "arm-left" { 1.0 } else { -1.0 };
                    if travel {
                        Quat::from_rotation_x(1.25 + flutter * 0.36)
                            * Quat::from_rotation_z(outward * 0.46)
                            * rest
                    } else {
                        Quat::from_rotation_z(outward * (1.36 + flutter * 0.08)) * rest
                    }
                };
                rotations.push(Keyframe {
                    time: duration * k as f32 / 4.0,
                    value: rotation.normalize(),
                    step: false,
                });
            }
            tracks.push(TransformTrack {
                joint_index: i,
                translations: vec![Keyframe {
                    time: 0.0,
                    value: joint.local_transform.translation,
                    step: false,
                }],
                rotations,
                scales: vec![Keyframe {
                    time: 0.0,
                    value: joint.local_transform.scale,
                    step: false,
                }],
            });
        }
        result.push(AnimationClip {
            name: name.to_string(),
            duration,
            looping: true,
            tracks,
            events: Vec::new(),
        });
    }
    result
}

pub struct CharacterAnimationController {
    skeleton: Skeleton,
    clips: Vec<AnimationClip>,
    clip_a: usize,
    clip_b: usize,
    current_time_a: f32,
    current_time_b: f32,
    playback_rate: f32,
    playing: bool,
    target_weight: f32,
    blend_weight: f32,
    blend_speed: f32,
    local_transforms_a: Vec<Transform>,
    local_transforms_b: Vec<Transform>,
    blended_transforms: Vec<Transform>,
    action_layers: Vec<ActionLayer>,
    pending_events: Vec<crate::animation::AnimationEvent>,
    idle_clip: String,
    walk_clip: String,
    run_clip: Option<String>,
}

#[derive(Clone, Debug)]
struct ActionLayer {
    slot: String,
    clip_index: usize,
    time: f32,
    weight: f32,
    joint_mask: Vec<bool>,
}

impl CharacterAnimationController {
    pub fn new() -> Self {
        if let Some(package) =
            super::package::get_package(super::package::DEFAULT_CHARACTER_PACKAGE)
        {
            return Self::from_package(&package);
        }

        Self::empty()
    }

    pub fn from_package(package: &CharacterPackage) -> Self {
        let joint_count = package.skeleton.joints.len();
        let clips = if package.clips.is_empty() {
            vec![AnimationClip {
                name: package.blend.default_state.clone(),
                duration: 1.0,
                looping: true,
                tracks: Vec::new(),
                events: Vec::new(),
            }]
        } else {
            package.clips.clone()
        };

        let mut clips = clips;
        if package.metadata.mesh_type == "imported_glb"
            && [
                "root",
                "leg-left",
                "leg-right",
                "torso",
                "arm-left",
                "arm-right",
                "head",
            ]
            .iter()
            .all(|bone| package.skeleton.joint_index(bone).is_some())
        {
            for clip in make_kenney_swim_clips(&package.skeleton) {
                if find_clip_index(&clips, &clip.name).is_none() {
                    clips.push(clip);
                }
            }
        }
        let clip_a = find_clip_index(&clips, &package.blend.default_state).unwrap_or(0);
        let clip_b = find_clip_index(&clips, &package.blend.walk_clip).unwrap_or(clip_a);

        Self {
            skeleton: package.skeleton.clone(),
            clips,
            clip_a,
            clip_b,
            current_time_a: 0.0,
            current_time_b: 0.0,
            playback_rate: 1.0,
            playing: true,
            target_weight: 0.0,
            blend_weight: 0.0,
            blend_speed: package.blend.blend_speed.max(0.01),
            local_transforms_a: vec![Transform::default(); joint_count],
            local_transforms_b: vec![Transform::default(); joint_count],
            blended_transforms: vec![Transform::default(); joint_count],
            action_layers: Vec::new(),
            pending_events: Vec::new(),
            idle_clip: package.blend.idle_clip.clone(),
            walk_clip: package.blend.walk_clip.clone(),
            run_clip: package.blend.run_clip.clone(),
        }
    }

    fn empty() -> Self {
        Self {
            skeleton: Skeleton::new(Vec::new()),
            clips: vec![AnimationClip {
                name: "idle".to_string(),
                duration: 1.0,
                looping: true,
                tracks: Vec::new(),
                events: Vec::new(),
            }],
            clip_a: 0,
            clip_b: 0,
            current_time_a: 0.0,
            current_time_b: 0.0,
            playback_rate: 1.0,
            playing: true,
            target_weight: 0.0,
            blend_weight: 0.0,
            blend_speed: 4.0,
            local_transforms_a: Vec::new(),
            local_transforms_b: Vec::new(),
            blended_transforms: Vec::new(),
            action_layers: Vec::new(),
            pending_events: Vec::new(),
            idle_clip: "Idle".to_string(),
            walk_clip: "Walk".to_string(),
            run_clip: None,
        }
    }

    pub fn select_animation(&mut self, animation: TargetAnimation) {
        match animation {
            TargetAnimation::Idle => {
                let clip = self.idle_clip.clone();
                self.select_locomotion_clip_name(&clip);
            }
            TargetAnimation::Walk => {
                let clip = self.walk_clip.clone();
                self.select_locomotion_clip_name(&clip);
            }
            TargetAnimation::Run => {
                let clip = self
                    .run_clip
                    .clone()
                    .unwrap_or_else(|| self.walk_clip.clone());
                self.select_locomotion_clip_name(&clip);
            }
            TargetAnimation::Named(name) => self.select_clip_name(&name),
        }
    }

    fn select_locomotion_clip_name(&mut self, name: &str) {
        if let Some(index) = self.resolve_clip_name(name) {
            self.clips[index].looping = true;
            self.playing = true;
            self.select_clip_name(name);
        }
    }

    pub fn set_actor_clip_time(&mut self, name: &str, time: f32) -> bool {
        let Some(index) = self.resolve_clip_name(name) else {
            return false;
        };
        self.clip_a = index;
        self.clip_b = index;
        self.current_time_a = time;
        self.current_time_b = time;
        self.blend_weight = 0.0;
        self.target_weight = 0.0;
        true
    }

    pub fn select_clip_name(&mut self, name: &str) {
        let Some(index) = self.resolve_clip_name(name) else {
            return;
        };

        let current_target = if self.target_weight >= 0.5 {
            self.clip_b
        } else {
            self.clip_a
        };
        if index == current_target {
            return;
        }

        if self.blend_weight >= 0.5 {
            self.clip_a = self.clip_b;
            self.current_time_a = self.current_time_b;
        }
        self.clip_b = index;
        self.current_time_b = 0.0;
        self.blend_weight = 0.0;
        self.target_weight = 1.0;
    }

    fn resolve_clip_name(&self, name: &str) -> Option<usize> {
        if let Some(index) = find_clip_index(&self.clips, name) {
            return Some(index);
        }

        if name.eq_ignore_ascii_case("idle") {
            return Some(self.clip_a);
        }

        None
    }

    pub fn set_playing(&mut self, value: bool) {
        self.playing = value;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn playback_time(&self) -> f32 {
        if self.target_weight >= 0.5 {
            self.current_time_b
        } else {
            self.current_time_a
        }
    }

    pub fn playback_duration(&self) -> f32 {
        if self.target_weight >= 0.5 {
            self.clips[self.clip_b].duration
        } else {
            self.clips[self.clip_a].duration
        }
    }

    pub fn set_playback_rate(&mut self, rate: f32) {
        self.playback_rate = rate;
    }

    pub fn play_layer(
        &mut self,
        slot: &str,
        clip_name: &str,
        joint_root: &str,
    ) -> Result<(), String> {
        let clip_index = self
            .resolve_clip_name(clip_name)
            .ok_or_else(|| format!("animation clip '{}' was not found", clip_name))?;
        let root_index = self
            .skeleton
            .joint_index(joint_root)
            .ok_or_else(|| format!("animation mask joint '{}' was not found", joint_root))?;
        let mut joint_mask = vec![false; self.skeleton.joints.len()];
        joint_mask[root_index] = true;
        for index in root_index + 1..self.skeleton.joints.len() {
            let mut parent = self.skeleton.joints[index].parent_index;
            while let Some(parent_index) = parent {
                if parent_index == root_index {
                    joint_mask[index] = true;
                    break;
                }
                parent = self.skeleton.joints[parent_index].parent_index;
            }
        }
        self.action_layers
            .retain(|layer| !layer.slot.eq_ignore_ascii_case(slot));
        self.action_layers.push(ActionLayer {
            slot: slot.to_string(),
            clip_index,
            time: 0.0,
            weight: 0.0,
            joint_mask,
        });
        Ok(())
    }

    pub fn update(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        let scaled_dt = dt * self.playback_rate;

        let active_index = if self.blend_weight >= 0.5 {
            self.clip_b
        } else {
            self.clip_a
        };
        let active_previous = if active_index == self.clip_b {
            self.current_time_b
        } else {
            self.current_time_a
        };
        self.pending_events.extend(events_during_advance(
            &self.clips[active_index],
            active_previous,
            scaled_dt,
        ));

        self.current_time_a =
            advance_clip_time(self.current_time_a, scaled_dt, &self.clips[self.clip_a]);
        self.current_time_b =
            advance_clip_time(self.current_time_b, scaled_dt, &self.clips[self.clip_b]);

        let active = if self.target_weight >= 0.5 {
            self.clip_b
        } else {
            self.clip_a
        };
        let clip = &self.clips[active];
        let time = if active == self.clip_b {
            self.current_time_b
        } else {
            self.current_time_a
        };
        if !clip.looping && time >= clip.duration {
            self.playing = false;
        }

        if self.blend_weight < self.target_weight {
            self.blend_weight = (self.blend_weight + self.blend_speed * dt).min(self.target_weight);
        } else if self.blend_weight > self.target_weight {
            self.blend_weight = (self.blend_weight - self.blend_speed * dt).max(self.target_weight);
        }

        for layer in &mut self.action_layers {
            let clip = &self.clips[layer.clip_index];
            layer.time = advance_clip_time(layer.time, scaled_dt, clip);
            let target = if clip.looping || layer.time < clip.duration {
                1.0
            } else {
                0.0
            };
            let fade_step = (dt * self.blend_speed.max(4.0)).max(0.0);
            if layer.weight < target {
                layer.weight = (layer.weight + fade_step).min(target);
            } else {
                layer.weight = (layer.weight - fade_step).max(target);
            }
        }
        self.action_layers.retain(|layer| layer.weight > 0.0);
    }

    pub fn drain_events(&mut self) -> impl Iterator<Item = crate::animation::AnimationEvent> + '_ {
        self.pending_events.drain(..)
    }

    pub fn evaluate_pose(&mut self) -> EvaluatedPose {
        for (i, joint) in self.skeleton.joints.iter().enumerate() {
            self.local_transforms_a[i] = joint.local_transform;
            self.local_transforms_b[i] = joint.local_transform;
        }

        self.clips[self.clip_a].evaluate(self.current_time_a, &mut self.local_transforms_a);
        self.clips[self.clip_b].evaluate(self.current_time_b, &mut self.local_transforms_b);

        for i in 0..self.skeleton.joints.len() {
            self.blended_transforms[i] =
                self.local_transforms_a[i].lerp(&self.local_transforms_b[i], self.blend_weight);
        }

        for layer in &self.action_layers {
            let mut action_pose = self.blended_transforms.clone();
            self.clips[layer.clip_index].evaluate(layer.time, &mut action_pose);
            for (index, masked) in layer.joint_mask.iter().copied().enumerate() {
                if masked {
                    self.blended_transforms[index] = self.blended_transforms[index]
                        .lerp(&action_pose[index], layer.weight.clamp(0.0, 1.0));
                }
            }
        }

        self.skeleton.evaluate_pose(&self.blended_transforms)
    }

    pub fn skeleton(&self) -> &Skeleton {
        &self.skeleton
    }

    pub fn blend_weight(&self) -> f32 {
        self.blend_weight
    }

    pub fn current_clip_name(&self) -> &str {
        if self.target_weight >= 0.5 {
            &self.clips[self.clip_b].name
        } else {
            &self.clips[self.clip_a].name
        }
    }
}

pub use super::rig::EvaluatedPose;

fn find_clip_index(clips: &[AnimationClip], name: &str) -> Option<usize> {
    clips
        .iter()
        .position(|clip| clip.name.eq_ignore_ascii_case(name))
}

fn advance_clip_time(time: f32, dt: f32, clip: &AnimationClip) -> f32 {
    let next = time + dt;
    if clip.looping {
        if clip.duration > 0.0 {
            next % clip.duration
        } else {
            0.0
        }
    } else {
        next.min(clip.duration)
    }
}

fn events_during_advance(
    clip: &AnimationClip,
    previous_local_time: f32,
    delta: f32,
) -> Vec<crate::animation::AnimationEvent> {
    if delta <= 0.0 || !delta.is_finite() || clip.duration <= 0.0 {
        return Vec::new();
    }
    let end = previous_local_time + delta;
    if !clip.looping {
        return clip
            .events
            .iter()
            .filter(|event| {
                event.time > previous_local_time && event.time <= end.min(clip.duration)
            })
            .cloned()
            .collect();
    }
    let loops = (end / clip.duration).floor() as usize;
    let mut result = Vec::new();
    for loop_index in 0..=loops {
        let offset = loop_index as f32 * clip.duration;
        result.extend(
            clip.events
                .iter()
                .filter(|event| {
                    let occurrence = offset + event.time;
                    occurrence > previous_local_time && occurrence <= end
                })
                .cloned(),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::rig::{Joint, Skeleton};
    use glam::Mat4;

    fn controller() -> CharacterAnimationController {
        let skeleton = Skeleton::new(vec![
            Joint {
                name: "Hips".into(),
                parent_index: None,
                local_transform: Transform::default(),
            },
            Joint {
                name: "Spine".into(),
                parent_index: Some(0),
                local_transform: Transform::default(),
            },
            Joint {
                name: "RightArm".into(),
                parent_index: Some(1),
                local_transform: Transform::default(),
            },
            Joint {
                name: "RightLeg".into(),
                parent_index: Some(0),
                local_transform: Transform::default(),
            },
        ]);
        let rotation_track = |joint_index, axis: Vec3| TransformTrack {
            joint_index,
            translations: Vec::new(),
            rotations: vec![
                Keyframe {
                    time: 0.0,
                    value: Quat::IDENTITY,
                    step: false,
                },
                Keyframe {
                    time: 0.5,
                    value: Quat::from_axis_angle(axis, 0.8),
                    step: false,
                },
            ],
            scales: Vec::new(),
        };
        let clips = vec![
            AnimationClip {
                name: "idle".into(),
                duration: 1.0,
                looping: true,
                tracks: Vec::new(),
                events: Vec::new(),
            },
            AnimationClip {
                name: "walk".into(),
                duration: 1.0,
                looping: true,
                tracks: vec![rotation_track(3, Vec3::X)],
                events: Vec::new(),
            },
            AnimationClip {
                name: "slash".into(),
                duration: 0.55,
                looping: false,
                tracks: vec![rotation_track(2, Vec3::Z)],
                events: Vec::new(),
            },
        ];
        CharacterAnimationController {
            skeleton,
            clips,
            clip_a: 0,
            clip_b: 0,
            current_time_a: 0.0,
            current_time_b: 0.0,
            playback_rate: 1.0,
            playing: true,
            target_weight: 0.0,
            blend_weight: 0.0,
            blend_speed: 4.0,
            local_transforms_a: vec![Transform::default(); 4],
            local_transforms_b: vec![Transform::default(); 4],
            blended_transforms: vec![Transform::default(); 4],
            action_layers: Vec::new(),
            pending_events: Vec::new(),
            idle_clip: "idle".into(),
            walk_clip: "walk".into(),
            run_clip: None,
        }
    }

    #[test]
    fn upper_body_action_layers_over_locomotion_without_changing_lower_body() {
        let mut base = controller();
        let mut layered = controller();
        base.select_clip_name("walk");
        layered.select_clip_name("walk");
        layered.play_layer("action", "slash", "Spine").unwrap();
        base.update(0.25);
        layered.update(0.25);
        let base_pose = base.evaluate_pose();
        let layered_pose = layered.evaluate_pose();

        assert_ne!(base_pose.matrices[2], layered_pose.matrices[2]);
        assert_eq!(base_pose.matrices[3], layered_pose.matrices[3]);
        assert_eq!(base_pose.matrices[0], layered_pose.matrices[0]);
    }

    #[test]
    fn one_shot_action_layer_fades_out_after_clip_finishes() {
        let mut layered = controller();
        layered.play_layer("action", "slash", "Spine").unwrap();
        for _ in 0..20 {
            layered.update(0.1);
        }
        assert!(layered.action_layers.is_empty());
        assert_eq!(layered.evaluate_pose().matrices[2], Mat4::IDENTITY);
    }

    #[test]
    fn action_layer_rejects_missing_clip_or_joint_mask() {
        let mut layered = controller();
        assert!(layered.play_layer("action", "missing", "Spine").is_err());
        assert!(layered.play_layer("action", "slash", "Missing").is_err());
    }

    #[test]
    fn character_clip_events_cross_loop_boundaries() {
        let mut controller = controller();
        controller.clips[0]
            .events
            .push(crate::animation::AnimationEvent {
                id: "step".into(),
                time: 0.2,
                name: "footstep".into(),
                payload: std::collections::BTreeMap::new(),
            });
        controller.update(2.3);
        assert_eq!(
            controller
                .drain_events()
                .filter(|event| event.name == "footstep")
                .count(),
            3
        );
    }

    #[test]
    fn rotation_only_track_preserves_the_joint_rest_translation_and_scale() {
        let rest = Transform {
            translation: Vec3::new(2.0, 3.0, 4.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(1.5),
        };
        let clip = AnimationClip {
            name: "turn".into(),
            duration: 1.0,
            looping: false,
            tracks: vec![TransformTrack {
                joint_index: 0,
                translations: Vec::new(),
                rotations: vec![Keyframe {
                    time: 0.0,
                    value: Quat::from_rotation_y(0.5),
                    step: false,
                }],
                scales: Vec::new(),
            }],
            events: Vec::new(),
        };
        let mut pose = vec![rest];

        clip.evaluate(0.0, &mut pose);

        assert_eq!(pose[0].translation, rest.translation);
        assert_eq!(pose[0].scale, rest.scale);
        assert_eq!(pose[0].rotation, Quat::from_rotation_y(0.5));
    }
}

#[cfg(test)]
mod wk_swim_animation_tests {
    use super::super::rig::Joint;
    use super::*;
    #[test]
    fn kenney_abc_shared_joint_schema_has_two_distinct_swim_clips() {
        let names = [
            "root",
            "leg-left",
            "leg-right",
            "torso",
            "arm-left",
            "arm-right",
            "head",
        ];
        let skeleton = Skeleton::new(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| Joint {
                    name: (*n).to_owned(),
                    parent_index: if i == 0 {
                        None
                    } else if i < 4 {
                        Some(0)
                    } else if i == 4 {
                        Some(0)
                    } else {
                        Some(3)
                    },
                    local_transform: Transform::default(),
                })
                .collect(),
        );
        let clips = make_kenney_swim_clips(&skeleton);
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].name, "SwimIdle");
        assert_eq!(clips[1].name, "Swim");
        assert!(clips[0].duration > clips[1].duration);
        let left = skeleton.joint_index("leg-left").unwrap();
        let right = skeleton.joint_index("leg-right").unwrap();
        for clip in clips {
            assert!(clip.looping);
            let mut first = vec![Transform::default(); names.len()];
            let mut second = first.clone();
            clip.evaluate(clip.duration * 0.25, &mut first);
            clip.evaluate(clip.duration * 0.75, &mut second);
            assert_ne!(first[left].rotation, second[left].rotation);
            assert_ne!(first[right].rotation, second[right].rotation);
        }
    }
}
