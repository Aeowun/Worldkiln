// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
fn state(id: u64) -> ActorState {
    ActorState {
        id,
        name: format!("Object {id}"),
        package: None,
        position: [id as f32, 1.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0; 3],
        velocity: [0.0; 3],
        angular_velocity: [0.0; 3],
        dynamic: true,
        enabled: true,
        visible: true,
        animation_clip: None,
        animation_time: 0.0,
        animation_playing: false,
        animation_rate: 1.0,
    }
}
#[test]
fn rejects_stale_and_malformed_states() {
    let mut actors = ActorSystem::new();
    let mut entities = EntityManager::new();
    let mut receiver = ActorReplicator::default();
    let initial = ActorBatch {
        epoch: 1,
        offset: 0,
        total: 1,
        actors: vec![state(4)],
    };
    assert!(receiver.apply_entities(&mut actors, &mut entities, initial.clone()));
    assert!(!receiver.apply_entities(&mut actors, &mut entities, initial));
    assert_eq!(receiver.replicated_count(), 1);
    assert!(receiver.apply_entities(
        &mut actors,
        &mut entities,
        ActorBatch {
            epoch: 2,
            offset: 0,
            total: 0,
            actors: vec![]
        }
    ));
    assert_eq!(receiver.replicated_count(), 0);
    let mut invalid = state(8);
    invalid.position[0] = f32::NAN;
    assert!(
        !ActorBatch {
            epoch: 3,
            offset: 0,
            total: 1,
            actors: vec![invalid]
        }
        .valid()
    );
}
#[test]
fn replicates_many_actors() {
    let mut host_actors = ActorSystem::new();
    let mut host_entities = EntityManager::new();
    for n in 0..109u64 {
        let id = host_entities.create_entity(&format!("Object {n}"));
        host_actors.register_actor(RuntimeActor {
            entity_id: id,
            authored_actor_id: Some(1000 + n),
            package_name: None,
            initial_position: Vec3::ZERO,
        });
    }
    let mut sender = ActorReplicator::default();
    let mut guest_actors = ActorSystem::new();
    let mut guest_entities = EntityManager::new();
    let mut receiver = ActorReplicator::default();
    for _ in 0..5 {
        let packet = sender.snapshot_entities(&host_actors, &host_entities);
        assert!(packet.valid());
        assert!(packet.actors.len() <= BATCH_LIMIT);
        assert!(receiver.apply_entities(&mut guest_actors, &mut guest_entities, packet));
    }
    assert_eq!(receiver.replicated_count(), 109);
    assert_eq!(guest_actors.actors().len(), 109);
}
#[test]
fn pushes_nearest_dynamic_object_by_physics_not_asset_name() {
    let mut actors = ActorSystem::new();
    let mut entities = EntityManager::new();
    let id = entities.create_entity("Generic object");
    actors.register_actor(RuntimeActor {
        entity_id: id,
        authored_actor_id: Some(777),
        package_name: None,
        initial_position: Vec3::X,
    });
    let entity = entities.get_entity_mut(id).unwrap();
    entity.position = Vec3::X;
    entity.physics.enabled = true;
    entity.physics.anchored = false;
    let replicator = ActorReplicator::default();
    assert_eq!(
        replicator.push_nearest_entity(&actors, &mut entities, 1, Vec3::ZERO),
        Some(777)
    );
    assert!(entities.get_entity(id).unwrap().physics.velocity.x > 0.0);
    assert_eq!(
        replicator.push_nearest_entity(&actors, &mut entities, 0, Vec3::ZERO),
        None
    );
}
