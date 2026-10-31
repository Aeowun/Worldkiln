// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::asset::{AnimationAsset, AnimationEvent};
use crate::scripting::value::{BasketData, MapKey, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationEventOccurrence<'a> {
    pub event: &'a AnimationEvent,
    pub playback_time: f64,
}

pub fn events_crossed(
    asset: &AnimationAsset,
    previous_time: f64,
    current_time: f64,
) -> Vec<AnimationEventOccurrence<'_>> {
    if current_time <= previous_time || asset.duration <= 0.0 {
        return Vec::new();
    }
    let duration = f64::from(asset.duration);
    let mut occurrences = Vec::new();
    if asset.looping {
        let first_loop = (previous_time / duration).floor() as i64;
        let last_loop = (current_time / duration).floor() as i64;
        for loop_index in first_loop..=last_loop {
            let loop_start = loop_index as f64 * duration;
            for event in &asset.events {
                let occurrence = loop_start + f64::from(event.time);
                if occurrence > previous_time && occurrence <= current_time {
                    occurrences.push(AnimationEventOccurrence {
                        event,
                        playback_time: occurrence,
                    });
                }
            }
        }
    } else {
        for event in &asset.events {
            let occurrence = f64::from(event.time);
            if occurrence > previous_time && occurrence <= current_time.min(duration) {
                occurrences.push(AnimationEventOccurrence {
                    event,
                    playback_time: occurrence,
                });
            }
        }
    }
    occurrences.sort_by(|a, b| a.playback_time.total_cmp(&b.playback_time));
    occurrences
}

pub fn script_event_arguments(event: &AnimationEvent) -> Vec<Value> {
    script_event_payload_arguments(&event.payload)
}

pub fn script_event_payload_arguments(
    event_payload: &std::collections::BTreeMap<String, super::asset::EventValue>,
) -> Vec<Value> {
    let payload = event_payload
        .iter()
        .map(|(key, value)| {
            let value = match value {
                super::asset::EventValue::Bool(value) => Value::Bool(*value),
                super::asset::EventValue::Number(value) => Value::Number(*value),
                super::asset::EventValue::Text(value) => Value::String(value.clone()),
                super::asset::EventValue::Vec3(value) => {
                    Value::Array(Arc::new(RefCell::new(BasketData {
                        elements: value
                            .iter()
                            .map(|component| Value::Number(f64::from(*component)))
                            .collect(),
                        frozen: true,
                    })))
                }
            };
            (MapKey::String(key.clone()), value)
        })
        .collect::<BTreeMap<_, _>>();
    vec![Value::Map(Arc::new(RefCell::new(payload)))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::asset::{AnimationEvent, AnimationSource};
    use std::collections::BTreeMap;

    fn asset() -> AnimationAsset {
        AnimationAsset {
            version: 1,
            id: "clip".into(),
            name: "Footsteps".into(),
            source: AnimationSource {
                rig_asset_id: "rig".into(),
                imported_clip: None,
            },
            duration: 1.0,
            frames_per_second: 30.0,
            looping: true,
            root_motion: Default::default(),
            tracks: Vec::new(),
            events: vec![AnimationEvent {
                id: "step".into(),
                time: 0.25,
                name: "footstep".into(),
                payload: BTreeMap::new(),
            }],
        }
    }

    #[test]
    fn looping_event_traversal_keeps_every_crossed_occurrence() {
        let asset = asset();
        let occurrences = events_crossed(&asset, 0.1, 2.4);
        assert_eq!(occurrences.len(), 3);
        assert_eq!(occurrences[0].playback_time, 0.25);
        assert_eq!(occurrences[1].playback_time, 1.25);
        assert_eq!(occurrences[2].playback_time, 2.25);
    }

    #[test]
    fn script_payload_conversion_preserves_authored_types() {
        let payload = BTreeMap::from([
            (
                "enabled".into(),
                super::super::asset::EventValue::Bool(true),
            ),
            ("power".into(), super::super::asset::EventValue::Number(2.5)),
            (
                "label".into(),
                super::super::asset::EventValue::Text("hit".into()),
            ),
            (
                "offset".into(),
                super::super::asset::EventValue::Vec3([1.0, 2.0, 3.0]),
            ),
        ]);
        let arguments = script_event_payload_arguments(&payload);
        assert_eq!(arguments.len(), 1);
        let Value::Map(map) = &arguments[0] else {
            panic!("animation payload must be one map argument");
        };
        let map = map.borrow();
        assert_eq!(
            map.get(&MapKey::String("enabled".into())),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            map.get(&MapKey::String("power".into())),
            Some(&Value::Number(2.5))
        );
        assert_eq!(
            map.get(&MapKey::String("label".into())),
            Some(&Value::String("hit".into()))
        );
        let Some(Value::Array(offset)) = map.get(&MapKey::String("offset".into())) else {
            panic!("Vec3 payload must be a basket");
        };
        let offset = offset.borrow();
        assert!(offset.frozen);
        assert_eq!(
            offset.elements,
            vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]
        );
    }
}
