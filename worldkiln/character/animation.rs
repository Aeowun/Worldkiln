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
        let translation = sample_track(&self.translations, time, Vec3::ZERO, |a, b, t| {
            a.lerp(*b, t)
        });
        let rotation = sample_track(&self.rotations, time, Quat::IDENTITY, |a, b, t| {
            a.slerp(*b, t)
        });
        let scale = sample_track(&self.scales, time, Vec3::ONE, |a, b, t| a.lerp(*b, t));

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
        if time >= window[0].time && time <= window[1].time {
            let duration = window[1].time - window[0].time;
            let t = if duration > 0.0 {
                (time - window[0].time) / duration
            } else {
                0.0
            };
            return lerp_fn(&window[0].value, &window[1].value, t);
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
}

impl AnimationClip {
    pub fn evaluate(&self, time: f32, out_transforms: &mut [Transform]) {
        for track in &self.tracks {
            if track.joint_index < out_transforms.len() {
                out_transforms[track.joint_index] = track.evaluate(time);
            }
        }
    }
}

pub struct CharacterAnimationController {
    skeleton: Skeleton,
    clips: Vec<AnimationClip>,
    clip_a: usize,
    clip_b: usize,
    current_time_a: f32,
    current_time_b: f32,
    playback_rate: f32,
    target_weight: f32,
    blend_weight: f32,
    blend_speed: f32,
    local_transforms_a: Vec<Transform>,
    local_transforms_b: Vec<Transform>,
    blended_transforms: Vec<Transform>,
    action_layers: Vec<ActionLayer>,
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
            }]
        } else {
            package.clips.clone()
        };

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
            target_weight: 0.0,
            blend_weight: 0.0,
            blend_speed: package.blend.blend_speed.max(0.01),
            local_transforms_a: vec![Transform::default(); joint_count],
            local_transforms_b: vec![Transform::default(); joint_count],
            blended_transforms: vec![Transform::default(); joint_count],
            action_layers: Vec::new(),
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
            }],
            clip_a: 0,
            clip_b: 0,
            current_time_a: 0.0,
            current_time_b: 0.0,
            playback_rate: 1.0,
            target_weight: 0.0,
            blend_weight: 0.0,
            blend_speed: 4.0,
            local_transforms_a: Vec::new(),
            local_transforms_b: Vec::new(),
            blended_transforms: Vec::new(),
            action_layers: Vec::new(),
            idle_clip: "Idle".to_string(),
            walk_clip: "Walk".to_string(),
            run_clip: None,
        }
    }

    pub fn select_animation(&mut self, animation: TargetAnimation) {
        match animation {
            TargetAnimation::Idle => {
                let clip = self.idle_clip.clone();
                self.select_clip_name(&clip);
            }
            TargetAnimation::Walk => {
                let clip = self.walk_clip.clone();
                self.select_clip_name(&clip);
            }
            TargetAnimation::Run => {
                let clip = self
                    .run_clip
                    .clone()
                    .unwrap_or_else(|| self.walk_clip.clone());
                self.select_clip_name(&clip);
            }
            TargetAnimation::Named(name) => self.select_clip_name(&name),
        }
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

        // Promote the pose that currently owns the transition to the new
        // source clip. This keeps Walk <-> Run changes on the same blended
        // path as Idle <-> Walk instead of hard-cutting clip_b in place.
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

    pub fn set_playback_rate(&mut self, rate: f32) {
        self.playback_rate = rate;
    }

    /// Starts/restarts an action clip blended over a named joint subtree.
    /// The layer is one-shot and fades back to the locomotion pose on completion.
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
        let scaled_dt = dt * self.playback_rate;

        self.current_time_a =
            advance_clip_time(self.current_time_a, scaled_dt, &self.clips[self.clip_a]);
        self.current_time_b =
            advance_clip_time(self.current_time_b, scaled_dt, &self.clips[self.clip_b]);

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
                },
                Keyframe {
                    time: 0.5,
                    value: Quat::from_axis_angle(axis, 0.8),
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
            },
            AnimationClip {
                name: "walk".into(),
                duration: 1.0,
                looping: true,
                tracks: vec![rotation_track(3, Vec3::X)],
            },
            AnimationClip {
                name: "slash".into(),
                duration: 0.55,
                looping: false,
                tracks: vec![rotation_track(2, Vec3::Z)],
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
            target_weight: 0.0,
            blend_weight: 0.0,
            blend_speed: 4.0,
            local_transforms_a: vec![Transform::default(); 4],
            local_transforms_b: vec![Transform::default(); 4],
            blended_transforms: vec![Transform::default(); 4],
            action_layers: Vec::new(),
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
}
