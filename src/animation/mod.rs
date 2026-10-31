// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub mod asset;
pub mod events;
pub mod playback;
pub mod retarget;
pub mod sampling;

pub use asset::{
    AnimationAsset, AnimationEvent, AnimationSource, EventValue, Interpolation, QuatKey,
    RootMotionSettings, TransformTrack, Vec3Key,
};
pub use events::{
    AnimationEventOccurrence, events_crossed, script_event_arguments,
    script_event_payload_arguments,
};
pub use playback::{AnimationPlaybackUpdate, AnimationPlayer};
pub use sampling::{sample_pose, sample_root_motion_delta, sample_track};
