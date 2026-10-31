// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::asset::{AnimationAsset, Interpolation, QuatKey, TransformTrack, Vec3Key};
use crate::character::rig::{Skeleton, Transform};
use glam::{Quat, Vec3};

pub fn sample_pose(asset: &AnimationAsset, skeleton: &Skeleton, time: f32) -> Vec<Transform> {
    let mut pose: Vec<Transform> = skeleton
        .joints
        .iter()
        .map(|joint| joint.local_transform)
        .collect();
    let time = clip_time(asset, time);
    for track in &asset.tracks {
        let index = skeleton.joints.iter().position(|joint| {
            joint.name.eq_ignore_ascii_case(&track.joint_id)
                || joint.name.eq_ignore_ascii_case(&track.joint_name)
        });
        if let Some(index) = index {
            pose[index] = sample_track(track, time, pose[index]);
        }
    }
    pose
}

pub fn sample_track(track: &TransformTrack, time: f32, rest: Transform) -> Transform {
    Transform {
        translation: sample_vec3(&track.translation, time).unwrap_or(rest.translation),
        rotation: sample_quat(&track.rotation, time).unwrap_or(rest.rotation),
        scale: sample_vec3(&track.scale, time).unwrap_or(rest.scale),
    }
}

pub fn sample_root_motion_delta(
    asset: &AnimationAsset,
    skeleton: &Skeleton,
    from_time: f32,
    to_time: f32,
) -> Transform {
    if !asset.root_motion.enabled {
        return Transform::default();
    }
    let Some(track) = asset.tracks.iter().find(|track| {
        track.joint_id == asset.root_motion.joint_id
            || track
                .joint_name
                .eq_ignore_ascii_case(&asset.root_motion.joint_id)
    }) else {
        return Transform::default();
    };
    let rest = skeleton
        .joints
        .iter()
        .find(|joint| {
            joint.name.eq_ignore_ascii_case(&track.joint_id)
                || joint.name.eq_ignore_ascii_case(&track.joint_name)
        })
        .map(|joint| joint.local_transform)
        .unwrap_or_default();
    let from = sample_track(track, clip_time(asset, from_time), rest);
    let to = sample_track(track, clip_time(asset, to_time), rest);
    let mut translation = to.translation - from.translation;
    if !asset.root_motion.translation_axes[0] {
        translation.x = 0.0;
    }
    if !asset.root_motion.translation_axes[1] {
        translation.y = 0.0;
    }
    if !asset.root_motion.translation_axes[2] {
        translation.z = 0.0;
    }
    let rotation = if asset.root_motion.rotation_y {
        let (from_y, _, _) = from.rotation.to_euler(glam::EulerRot::YXZ);
        let (to_y, _, _) = to.rotation.to_euler(glam::EulerRot::YXZ);
        Quat::from_rotation_y(to_y - from_y)
    } else {
        Quat::IDENTITY
    };
    Transform {
        translation,
        rotation,
        scale: Vec3::ONE,
    }
}

fn clip_time(asset: &AnimationAsset, time: f32) -> f32 {
    if asset.looping && asset.duration > 0.0 {
        time.rem_euclid(asset.duration)
    } else {
        time.clamp(0.0, asset.duration)
    }
}

fn sample_vec3(keys: &[Vec3Key], time: f32) -> Option<Vec3> {
    sample_segment(
        keys,
        time,
        |key| key.time,
        |a, b, amount| {
            let a_value = Vec3::from_array(a.value);
            let b_value = Vec3::from_array(b.value);
            match a.interpolation {
                Interpolation::Step => a_value,
                Interpolation::Linear => a_value.lerp(b_value, amount),
                Interpolation::Cubic => {
                    let duration = b.time - a.time;
                    let out_tangent = Vec3::from_array(a.out_tangent.unwrap_or([0.0; 3]));
                    let in_tangent = Vec3::from_array(b.in_tangent.unwrap_or([0.0; 3]));
                    let t2 = amount * amount;
                    let t3 = t2 * amount;
                    a_value * (2.0 * t3 - 3.0 * t2 + 1.0)
                        + out_tangent * duration * (t3 - 2.0 * t2 + amount)
                        + b_value * (-2.0 * t3 + 3.0 * t2)
                        + in_tangent * duration * (t3 - t2)
                }
            }
        },
    )
}

fn sample_quat(keys: &[QuatKey], time: f32) -> Option<Quat> {
    sample_segment(
        keys,
        time,
        |key| key.time,
        |a, b, amount| {
            let a_value = Quat::from_array(a.value).normalize();
            let b_value = Quat::from_array(b.value).normalize();
            match a.interpolation {
                Interpolation::Step => a_value,
                Interpolation::Linear => a_value.slerp(b_value, amount),
                Interpolation::Cubic => {
                    let smooth = amount * amount * (3.0 - 2.0 * amount);
                    a_value.slerp(b_value, smooth)
                }
            }
        },
    )
}

fn sample_segment<T, V>(
    keys: &[T],
    time: f32,
    key_time: impl Fn(&T) -> f32,
    interpolate: impl Fn(&T, &T, f32) -> V,
) -> Option<V>
where
    V: FromKey<T>,
{
    let first = keys.first()?;
    if keys.len() == 1 || time <= key_time(first) {
        return Some(V::from_key(first));
    }
    let last = keys.last().expect("keys is non-empty");
    if time >= key_time(last) {
        return Some(V::from_key(last));
    }
    for pair in keys.windows(2) {
        let start = key_time(&pair[0]);
        let end = key_time(&pair[1]);
        if time >= start && time <= end {
            let amount = if end > start {
                (time - start) / (end - start)
            } else {
                0.0
            };
            return Some(interpolate(&pair[0], &pair[1], amount));
        }
    }
    Some(V::from_key(last))
}

trait FromKey<T> {
    fn from_key(key: &T) -> Self;
}

impl FromKey<Vec3Key> for Vec3 {
    fn from_key(key: &Vec3Key) -> Self {
        Vec3::from_array(key.value)
    }
}

impl FromKey<QuatKey> for Quat {
    fn from_key(key: &QuatKey) -> Self {
        Quat::from_array(key.value).normalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_track_uses_the_rest_transform_for_unkeyed_channels() {
        let rest = Transform {
            translation: Vec3::new(1.0, 2.0, 3.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(2.0),
        };
        let track = TransformTrack {
            joint_id: "Spine".into(),
            joint_name: "Spine".into(),
            rotation: vec![QuatKey {
                time: 0.0,
                value: Quat::from_rotation_x(1.0).to_array(),
                interpolation: Interpolation::Linear,
            }],
            ..TransformTrack::default()
        };
        let sampled = sample_track(&track, 0.0, rest);
        assert_eq!(sampled.translation, rest.translation);
        assert_eq!(sampled.scale, rest.scale);
        assert_ne!(sampled.rotation, rest.rotation);
    }
}
