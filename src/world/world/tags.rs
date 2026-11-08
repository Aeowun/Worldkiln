// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::World;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct ActorTagIndex {
    members: HashMap<String, BTreeSet<u64>>,
    snapshots: HashMap<String, Arc<[u64]>>,
}

impl World {
    pub fn actor_tags(&self, id: u64) -> Option<&BTreeSet<String>> {
        let authored = self.actors.get(&id)?;
        Some(self.runtime_actor_tags.get(&id).unwrap_or(&authored.tags))
    }

    pub fn has_actor_tag(&self, id: u64, tag: &str) -> Option<bool> {
        self.actor_tags(id).map(|tags| tags.contains(tag))
    }

    pub fn actor_tag_snapshot(&self, tag: &str) -> Arc<[u64]> {
        let mut cached = self.actor_tag_index.borrow_mut();
        if cached.is_none() {
            let mut index = ActorTagIndex::default();
            for (id, actor) in &self.actors {
                for label in self.runtime_actor_tags.get(id).unwrap_or(&actor.tags) {
                    index.members.entry(label.clone()).or_default().insert(*id);
                }
            }
            *cached = Some(index);
        }

        let index = cached.as_mut().expect("tag index initialized");
        if let Some(snapshot) = index.snapshots.get(tag) {
            return Arc::clone(snapshot);
        }
        let snapshot: Arc<[u64]> = index
            .members
            .get(tag)
            .map(|ids| ids.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default()
            .into();
        index
            .snapshots
            .insert(tag.to_owned(), Arc::clone(&snapshot));
        snapshot
    }

    pub fn actor_ids_with_tag(&self, tag: &str) -> Vec<u64> {
        self.actor_tag_snapshot(tag).iter().copied().collect()
    }

    pub(crate) fn invalidate_actor_tag_index(&self) {
        *self.actor_tag_index.borrow_mut() = None;
    }

    fn update_actor_tag_index(&self, id: u64, tag: &str, enabled: bool) {
        let mut cached = self.actor_tag_index.borrow_mut();
        let Some(index) = cached.as_mut() else { return };
        let changed = if enabled {
            index.members.entry(tag.to_owned()).or_default().insert(id)
        } else if let Some(ids) = index.members.get_mut(tag) {
            let changed = ids.remove(&id);
            if ids.is_empty() {
                index.members.remove(tag);
            }
            changed
        } else {
            false
        };
        if changed {
            index.snapshots.remove(tag);
        }
    }

    pub(crate) fn add_actor_to_tag_index(&self, id: u64, tags: &BTreeSet<String>) {
        for tag in tags {
            self.update_actor_tag_index(id, tag, true);
        }
    }

    pub(crate) fn remove_actor_from_tag_index(&self, id: u64, tags: &BTreeSet<String>) {
        for tag in tags {
            self.update_actor_tag_index(id, tag, false);
        }
    }

    pub fn cell_tags(&self, id: u64) -> Option<&BTreeSet<String>> {
        let cell = self.get_effective_cell_by_id(id)?;
        Some(
            self.runtime_state
                .get(&id)
                .and_then(|state| state.tag_overrides.as_ref())
                .unwrap_or(&cell.tags),
        )
    }

    pub fn has_cell_tag(&self, id: u64, tag: &str) -> Option<bool> {
        self.cell_tags(id).map(|tags| tags.contains(tag))
    }

    pub fn cell_ids_with_tag(&self, tag: &str) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .cells
            .values()
            .chain(self.sources.values().map(|source| &source.cell))
            .chain(self.runtime_cells.values())
            .filter(|cell| self.has_cell_tag(cell.id, tag) == Some(true))
            .map(|cell| cell.id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub fn set_runtime_cell_tag(&mut self, id: u64, tag: &str, enabled: bool) -> Option<bool> {
        let authored = self.get_effective_cell_by_id(id)?.tags.clone();
        let tags = self
            .runtime_state
            .entry(id)
            .or_default()
            .tag_overrides
            .get_or_insert(authored);
        Some(if enabled {
            tags.insert(tag.to_owned())
        } else {
            tags.remove(tag)
        })
    }

    pub fn set_runtime_actor_tag(&mut self, id: u64, tag: &str, enabled: bool) -> Option<bool> {
        let original = self.actors.get(&id)?.tags.clone();
        let tags = self.runtime_actor_tags.entry(id).or_insert(original);
        let changed = if enabled {
            tags.insert(tag.to_owned())
        } else {
            tags.remove(tag)
        };
        if changed {
            self.update_actor_tag_index(id, tag, enabled);
        }
        Some(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_are_deterministic_and_runtime_changes_do_not_edit_authored_tags() {
        let mut world = World::new();
        let first = world.add_actor("First", glam::Vec3::ZERO);
        let second = world.add_actor("Second", glam::Vec3::ONE);
        world
            .actors
            .get_mut(&first)
            .unwrap()
            .tags
            .insert("collectible".into());
        assert_eq!(world.actor_ids_with_tag("collectible"), vec![first]);
        assert_eq!(
            world.set_runtime_actor_tag(second, "collectible", true),
            Some(true)
        );
        assert_eq!(
            world.set_runtime_actor_tag(second, "collectible", true),
            Some(false)
        );
        assert_eq!(world.actor_ids_with_tag("collectible"), vec![first, second]);
        assert!(world.actors[&second].tags.is_empty());
        assert_eq!(
            world.set_runtime_actor_tag(first, "collectible", false),
            Some(true)
        );
        assert_eq!(world.actor_ids_with_tag("collectible"), vec![second]);
        world.clear_runtime_state();
        assert_eq!(world.actor_ids_with_tag("collectible"), vec![first]);
        assert_eq!(world.has_actor_tag(u64::MAX, "collectible"), None);
    }

    #[test]
    fn cell_tags_cover_authored_sources_and_runtime_overrides_without_writing() {
        use crate::world::{cell::CellType, coordinate::WorldCoord};
        let mut world = World::new();
        let block = world.set_cell(WorldCoord::new(3, 4, 5), CellType::Block);
        world
            .get_mut(WorldCoord::new(3, 4, 5))
            .unwrap()
            .tags
            .insert("puzzle".into());
        let mut source = crate::world::cell::Cell::new_light();
        source.id = 10_000_201;
        source.tags.insert("puzzle".into());
        let source_id = source.id;
        world.insert_source(crate::world::AuthoredSource {
            cell: source,
            position: glam::Vec3::new(10.0, 3.0, 9.0),
        });
        assert_eq!(world.cell_ids_with_tag("puzzle"), {
            let mut ids = vec![block, source_id];
            ids.sort_unstable();
            ids
        });
        assert_eq!(world.has_cell_tag(block, "puzzle"), Some(true));
        assert_eq!(
            world.set_runtime_cell_tag(block, "puzzle", false),
            Some(true)
        );
        assert_eq!(world.has_cell_tag(block, "puzzle"), Some(false));
        assert!(
            world
                .get_effective_cell_by_id(block)
                .unwrap()
                .tags
                .contains("puzzle")
        );
        let source_cell = world.get_effective_cell_by_id(source_id).unwrap();
        let stored =
            crate::world::storage::StoredSource::from_source(&crate::world::AuthoredSource {
                cell: source_cell.clone(),
                position: glam::Vec3::ZERO,
            });
        let decoded = serde_json::from_str::<crate::world::storage::StoredSource>(
            &serde_json::to_string(&stored).unwrap(),
        )
        .unwrap();
        assert!(decoded.into_source().cell.tags.contains("puzzle"));
        world.clear_runtime_state();
        assert_eq!(world.has_cell_tag(block, "puzzle"), Some(true));
    }

    #[test]
    fn deleting_actor_discards_runtime_tags() {
        let mut world = World::new();
        let id = world.add_actor("Temporary", glam::Vec3::ZERO);
        world.set_runtime_actor_tag(id, "door", true);
        assert!(world.remove_actor(id).is_some());
        assert_eq!(world.has_actor_tag(id, "door"), None);
        assert!(!world.runtime_actor_tags.contains_key(&id));
    }

    #[test]
    fn actor_membership_snapshots_are_reused_and_invalidated_per_tag() {
        let mut world = World::new();
        let first = world.add_actor("First", glam::Vec3::ZERO);
        world
            .actors
            .get_mut(&first)
            .unwrap()
            .tags
            .insert("interactive".into());
        let first_view = world.actor_tag_snapshot("interactive");
        let same_view = world.actor_tag_snapshot("interactive");
        assert!(Arc::ptr_eq(&first_view, &same_view));

        let other = world.actor_tag_snapshot("decor");
        let second = world.add_actor("Second", glam::Vec3::ONE);
        assert_eq!(
            world.set_runtime_actor_tag(second, "interactive", true),
            Some(true)
        );
        let changed = world.actor_tag_snapshot("interactive");
        assert!(!Arc::ptr_eq(&first_view, &changed));
        assert_eq!(&*changed, &[first, second]);
        assert!(Arc::ptr_eq(&other, &world.actor_tag_snapshot("decor")));
        assert_eq!(&*first_view, &[first], "immutable old view must stay valid");
        assert_eq!(
            world.set_runtime_actor_tag(second, "interactive", true),
            Some(false)
        );
        assert!(Arc::ptr_eq(
            &changed,
            &world.actor_tag_snapshot("interactive")
        ));

        let copied = world.actors.get(&first).unwrap().clone();
        let pasted = world.paste_actor(&copied, &[], glam::Vec3::new(3.0, 0.0, 0.0));
        assert!(!Arc::ptr_eq(
            &changed,
            &world.actor_tag_snapshot("interactive")
        ));
        assert!(Arc::ptr_eq(&other, &world.actor_tag_snapshot("decor")));

        world.remove_actor(pasted);
        world.remove_actor(second);
        assert_eq!(world.actor_ids_with_tag("interactive"), vec![first]);
        world.clear_runtime_state();
        assert_eq!(world.actor_ids_with_tag("interactive"), vec![first]);
        world.remove_actor(first);
        assert!(world.actor_ids_with_tag("interactive").is_empty());
    }

    #[test]
    fn actor_tag_index_tracks_runtime_membership_without_rescanning_actors() {
        let mut world = World::new();
        let tree = world.add_actor("Tree", glam::Vec3::ZERO);
        let sign = world.add_actor("Sign", glam::Vec3::ONE);
        world
            .actors
            .get_mut(&sign)
            .unwrap()
            .tags
            .insert("interactable".into());

        assert_eq!(world.actor_ids_with_tag("interactable"), vec![sign]);
        assert!(world.actor_tag_index.borrow().is_some());

        assert_eq!(
            world.set_runtime_actor_tag(tree, "interactable", true),
            Some(true)
        );
        assert_eq!(world.actor_ids_with_tag("interactable"), vec![tree, sign]);

        assert_eq!(
            world.set_runtime_actor_tag(sign, "interactable", false),
            Some(true)
        );
        assert_eq!(world.actor_ids_with_tag("interactable"), vec![tree]);

        world.remove_actor(tree);
        assert!(world.actor_ids_with_tag("interactable").is_empty());
    }
}
