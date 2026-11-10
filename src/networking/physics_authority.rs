// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::engine::physics::rapier_backend::{BodyShape, RigidSolver};

const ACTOR_ID: u64 = 1;
const SERVER_DT: f64 = 1.0 / 60.0;
const SERVER_GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);
const MAX_FRAME_SECONDS: f64 = 0.25;
const MAX_IMPULSE: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharedActorSnapshot {
    pub tick: u64,
    pub actor_id: u64,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub linear_velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub sleeping: bool,
}

impl SharedActorSnapshot {
    pub fn is_valid(&self) -> bool {
        self.actor_id != 0
            && self
                .position
                .iter()
                .chain(&self.linear_velocity)
                .chain(&self.angular_velocity)
                .all(|v| v.is_finite())
            && self.rotation.iter().all(|v| v.is_finite())
            && (0.98..=1.02).contains(&self.rotation.iter().map(|v| v * v).sum::<f32>())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlayerContact {
    pub player_id: u64,
    pub impulse: Vec3,
    pub contact_offset: Vec3,
}

pub struct SharedActorAuthority {
    rapier: RigidSolver,
    tick: u64,
    accumulator: f64,
    pending: BTreeMap<u64, Vec<PlayerContact>>,
}

impl SharedActorAuthority {
    pub fn new() -> Self {
        let mut rapier = RigidSolver::new(SERVER_GRAVITY, SERVER_DT as f32);
        assert!(rapier.add_static(
            BodyShape::Box {
                half_extents: Vec3::new(10.0, 0.5, 10.0)
            },
            Vec3::new(0.0, -0.5, 0.0),
            0.7,
        ));
        assert!(rapier.add_dynamic(
            ACTOR_ID,
            BodyShape::Box {
                half_extents: Vec3::splat(0.5)
            },
            Vec3::new(0.0, 1.5, 0.0),
            1.0,
            0.7,
            Vec3::ZERO,
            Quat::IDENTITY,
            true,
        ));
        Self {
            rapier,
            tick: 0,
            accumulator: 0.0,
            pending: BTreeMap::new(),
        }
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn submit_contact(
        &mut self,
        tick: u64,
        contact: PlayerContact,
    ) -> Result<(), &'static str> {
        if tick < self.tick + 1 || tick > self.tick + 120 {
            return Err("contact tick outside authoritative window");
        }
        if !(1..=2).contains(&contact.player_id)
            || !contact.impulse.is_finite()
            || !contact.contact_offset.is_finite()
            || contact.impulse.length() > MAX_IMPULSE
            || contact.contact_offset.length() > 2.0
        {
            return Err("invalid authoritative contact");
        }
        self.pending.entry(tick).or_default().push(contact);
        Ok(())
    }

    pub fn advance(&mut self, elapsed_seconds: f64) -> Result<u32, &'static str> {
        if !elapsed_seconds.is_finite() || !(0.0..=MAX_FRAME_SECONDS).contains(&elapsed_seconds) {
            return Err("invalid or excessive frame interval");
        }
        self.accumulator += elapsed_seconds;
        let mut steps = 0;
        while self.accumulator + 1e-10 >= SERVER_DT {
            self.tick += 1;
            self.accumulator -= SERVER_DT;
            let mut inputs = self.pending.remove(&self.tick).unwrap_or_default();
            inputs.sort_by_key(|input| input.player_id);
            if !inputs.is_empty() {
                let state = self
                    .rapier
                    .state(ACTOR_ID)
                    .ok_or("missing authoritative actor")?;
                let force =
                    inputs.iter().map(|input| input.impulse).sum::<Vec3>() / SERVER_DT as f32;
                let torque = inputs
                    .iter()
                    .map(|input| input.contact_offset.cross(input.impulse))
                    .sum::<Vec3>()
                    / SERVER_DT as f32;
                self.rapier.apply_motion(
                    ACTOR_ID,
                    state.translation,
                    state.rotation,
                    state.linear_velocity,
                    state.angular_velocity,
                    force,
                    torque,
                    Vec3::ZERO,
                    Some(state),
                );
            }
            self.rapier.step(SERVER_GRAVITY);
            steps += 1;
        }
        Ok(steps)
    }

    pub fn snapshot(&self) -> SharedActorSnapshot {
        let state = self
            .rapier
            .state(ACTOR_ID)
            .expect("authoritative actor exists");
        SharedActorSnapshot {
            tick: self.tick,
            actor_id: ACTOR_ID,
            position: state.translation.to_array(),
            rotation: state.rotation.to_array(),
            linear_velocity: state.linear_velocity.to_array(),
            angular_velocity: state.angular_velocity.to_array(),
            sleeping: state.sleeping,
        }
    }
}

#[derive(Default)]
pub struct SharedActorReplica {
    current: Option<SharedActorSnapshot>,
}

impl SharedActorReplica {
    pub fn apply(&mut self, state: SharedActorSnapshot) -> bool {
        if !state.is_valid()
            || self.current.is_some_and(|previous| {
                state.tick <= previous.tick || state.actor_id != previous.actor_id
            })
        {
            return false;
        }
        self.current = Some(state);
        true
    }

    pub fn state(&self) -> Option<SharedActorSnapshot> {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulate(render_hz: u32) -> SharedActorSnapshot {
        let mut authority = SharedActorAuthority::new();
        let mut scheduled = [false; 2];
        for _ in 0..render_hz * 10 {
            if authority.tick() >= 120 && !scheduled[0] {
                authority
                    .submit_contact(
                        authority.tick() + 1,
                        PlayerContact {
                            player_id: 1,
                            impulse: Vec3::new(3.0, 0.0, 0.0),
                            contact_offset: Vec3::new(0.0, 0.0, 0.4),
                        },
                    )
                    .unwrap();
                scheduled[0] = true;
            }
            if authority.tick() >= 240 && !scheduled[1] {
                let target = authority.tick() + 1;
                authority
                    .submit_contact(
                        target,
                        PlayerContact {
                            player_id: 2,
                            impulse: Vec3::new(-2.0, 0.0, 0.0),
                            contact_offset: Vec3::new(0.0, 0.0, -0.3),
                        },
                    )
                    .unwrap();
                authority
                    .submit_contact(
                        target,
                        PlayerContact {
                            player_id: 1,
                            impulse: Vec3::new(1.0, 0.0, 0.0),
                            contact_offset: Vec3::new(0.0, 0.0, 0.2),
                        },
                    )
                    .unwrap();
                scheduled[1] = true;
            }
            authority.advance(1.0 / render_hz as f64).unwrap();
        }
        assert!(scheduled.into_iter().all(|x| x));
        authority.snapshot()
    }

    #[test]
    fn poc01_authoritative_rapier_actor_ignores_30_and_144_fps_render_cadence() {
        let slow = simulate(30);
        let fast = simulate(144);
        assert_eq!(slow.tick, 600);
        assert_eq!(fast.tick, 600);
        for (a, b) in slow.position.iter().zip(fast.position) {
            assert!((*a - b).abs() < 0.00001);
        }
        for (a, b) in slow.rotation.iter().zip(fast.rotation) {
            assert!((*a - b).abs() < 0.00001);
        }
        for (a, b) in slow.linear_velocity.iter().zip(fast.linear_velocity) {
            assert!((*a - b).abs() < 0.00001);
        }
        for (a, b) in slow.angular_velocity.iter().zip(fast.angular_velocity) {
            assert!((*a - b).abs() < 0.00001);
        }
    }

    #[test]
    fn newer_authoritative_snapshots_win_over_duplicates_and_reordered_updates() {
        let mut server = SharedActorAuthority::new();
        server.advance(1.0 / 30.0).unwrap();
        let old = server.snapshot();
        server.advance(1.0 / 30.0).unwrap();
        let latest = server.snapshot();
        let mut client = SharedActorReplica::default();
        assert!(client.apply(old));
        assert!(client.apply(latest));
        assert!(!client.apply(old));
        assert!(!client.apply(latest));
        assert_eq!(client.state(), Some(latest));
    }

    #[test]
    fn rejects_unowned_or_impossible_contacts_and_bad_frame_times() {
        let mut server = SharedActorAuthority::new();
        for player_id in [0, 3] {
            assert!(
                server
                    .submit_contact(
                        1,
                        PlayerContact {
                            player_id,
                            impulse: Vec3::X,
                            contact_offset: Vec3::ZERO,
                        }
                    )
                    .is_err()
            );
        }
        assert!(
            server
                .submit_contact(
                    1,
                    PlayerContact {
                        player_id: 1,
                        impulse: Vec3::splat(f32::INFINITY),
                        contact_offset: Vec3::ZERO,
                    }
                )
                .is_err()
        );
        assert!(server.advance(f64::NAN).is_err());
        assert_eq!(server.tick(), 0);
    }
}
