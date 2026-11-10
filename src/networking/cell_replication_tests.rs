// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::world::CellType;

#[test]
fn host_created_cell_appears_and_moves_on_client_with_full_visual_state() {
    let mut host = World::new();
    let id = host.create_runtime_cell(CellType::Block);
    let from = WorldCoord::new(3, 4, 5);
    let to = WorldCoord::new(8, 4, 5);
    host.move_runtime_cell(id, from).unwrap();
    {
        let cell = host.runtime_cells.get_mut(&id).unwrap();
        cell.texture = "Block_tx".into();
        cell.color_rgb = Vec3::new(0.5, 0.1, 0.9);
        cell.anchored = false;
        cell.solid = true;
    }
    host.set_visual_offset_runtime(from, Vec3::new(0.2, 0.0, 0.0));
    let mut sender = CellReplicator::default();
    let mut receiver = CellReplicator::default();
    let mut guest = World::new();
    assert!(receiver.apply(&mut guest, sender.snapshot(&host)));
    assert_eq!(receiver.replicated_count(), 1);
    let local = guest.get_effective_cell(from).unwrap();
    assert_eq!(local.color_rgb, Vec3::new(0.5, 0.1, 0.9));
    assert!(!local.anchored);
    assert_eq!(
        guest.runtime_state.get(&local.id).unwrap().visual_offset,
        Some(Vec3::new(0.2, 0.0, 0.0))
    );

    host.move_runtime_cell(id, to).unwrap();
    assert!(receiver.apply(&mut guest, sender.snapshot(&host)));
    assert!(guest.get_effective_cell(from).is_none());
    assert!(guest.get_effective_cell(to).is_some());
    host.delete_cell_runtime(id);
    assert!(receiver.apply(&mut guest, sender.snapshot(&host)));
    assert!(guest.get_effective_cell(to).is_none());
    assert_eq!(receiver.replicated_count(), 0);
}

#[test]
fn authoring_remains_unchanged_and_runtime_cell_overrides_reconcile() {
    let coord = WorldCoord::new(1, 2, 3);
    let mut host = World::new();
    let mut guest = World::new();
    let host_id = host.set_cell(coord, CellType::Block);
    let guest_id = guest.set_cell(coord, CellType::Block);
    assert_eq!(host_id, guest_id);
    host.set_cell_color_runtime(coord, Vec3::new(0.9, 0.1, 0.6));
    host.set_cell_solid_runtime(coord, false);
    let mut sender = CellReplicator::default();
    let mut receiver = CellReplicator::default();
    assert!(receiver.apply(&mut guest, sender.snapshot(&host)));
    let state = guest.runtime_state.get(&guest_id).unwrap();
    assert_eq!(state.color_rgb, Some(Vec3::new(0.9, 0.1, 0.6)));
    assert_eq!(state.solid, Some(false));
    host.runtime_state.remove(&host_id);
    assert!(receiver.apply(&mut guest, sender.snapshot(&host)));
    assert!(!guest.runtime_state.contains_key(&guest_id));
}

#[test]
fn dynamic_cells_follow_server_physics_position_rotation_and_velocity() {
    let coord = WorldCoord::new(2, 1, 1);
    let mut host = World::new();
    let id = host.set_cell(coord, CellType::Block);
    host.set_cell_anchored_runtime(coord, false);
    let mut host_physics = PhysicsWorld::new();
    host_physics.register_from_world(&host);
    host_physics.sync_with_world(&mut host);
    let host_body = host_physics
        .bodies
        .iter_mut()
        .find(|b| b.cell_id == id)
        .unwrap();
    host_body.position = Vec3::new(9.5, 3.1, -2.0);
    host_body.rotation = Quat::from_rotation_y(0.4);
    host_body.velocity = Vec3::new(1.5, 0.0, -0.3);
    host_body.angular_velocity = Vec3::new(0.0, 0.7, 0.0);
    let mut guest = World::new();
    let guest_id = guest.set_cell(coord, CellType::Block);
    assert_eq!(id, guest_id);
    let mut guest_physics = PhysicsWorld::new();
    guest_physics.register_from_world(&guest);
    let mut sender = CellReplicator::default();
    let mut receiver = CellReplicator::default();
    assert!(receiver.apply_with_physics(
        &mut guest,
        &mut guest_physics,
        sender.snapshot_with_physics(&host, &host_physics),
    ));
    let guest_body = guest_physics
        .bodies
        .iter()
        .find(|b| b.cell_id == guest_id)
        .unwrap();
    assert_eq!(guest_body.position, Vec3::new(9.5, 3.1, -2.0));
    assert_eq!(guest_body.velocity, Vec3::new(1.5, 0.0, -0.3));
    assert_eq!(guest_body.angular_velocity, Vec3::new(0.0, 0.7, 0.0));
    assert!((guest_body.rotation.dot(Quat::from_rotation_y(0.4))).abs() > 0.999);
    assert!(guest_body.anchored);
}

#[test]
fn invalid_stale_and_out_of_order_runtime_cell_batches_are_rejected() {
    let mut guest = World::new();
    let mut receiver = CellReplicator::default();
    let batch = CellBatch {
        epoch: 1,
        offset: 0,
        total: 0,
        cells: Vec::new(),
    };
    assert!(receiver.apply(&mut guest, batch.clone()));
    assert!(!receiver.apply(&mut guest, batch));
    let invalid = CellBatch {
        epoch: 2,
        offset: 5,
        total: 1,
        cells: Vec::new(),
    };
    assert!(!receiver.apply(&mut guest, invalid));
}

#[test]
fn batches_handle_many_runtime_cells_without_name_specific_paths() {
    let mut host = World::new();
    let mut guest = World::new();
    for i in 0..19 {
        let id = host.create_runtime_cell(CellType::Block);
        host.move_runtime_cell(id, WorldCoord::new(i, 1, 0))
            .unwrap();
    }
    let mut sender = CellReplicator::default();
    let mut receiver = CellReplicator::default();
    for _ in 0..4 {
        assert!(receiver.apply(&mut guest, sender.snapshot(&host)));
    }
    assert_eq!(receiver.replicated_count(), 19);
    assert_eq!(guest.runtime_cells.len(), 19);
}
