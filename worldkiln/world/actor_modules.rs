use crate::scripting::binding::ActorScriptBinding;
use crate::world::world::World;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorModuleRef {
    pub actor_id: u64,
    pub script_path: String,
    pub module_name: String,
    pub enabled: bool,
}

impl ActorModuleRef {
    fn from_binding(binding: &ActorScriptBinding) -> Self {
        Self {
            actor_id: binding.actor_id,
            script_path: binding.normalized_script_path(),
            module_name: binding.module_name(),
            enabled: binding.enabled,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ActorModuleIndex {
    by_actor: HashMap<u64, Vec<ActorModuleRef>>,
    by_actor_name: HashMap<(u64, String), Vec<ActorModuleRef>>,
    by_path: HashMap<String, Vec<u64>>,
}

impl ActorModuleIndex {
    pub fn rebuild(
        &mut self,
        actors: &HashMap<u64, crate::world::Actor>,
        bindings: &[ActorScriptBinding],
    ) {
        self.by_actor.clear();
        self.by_actor_name.clear();
        self.by_path.clear();

        for binding in bindings {
            if !actors.contains_key(&binding.actor_id) {
                continue;
            }

            let module = ActorModuleRef::from_binding(binding);

            self.by_actor
                .entry(binding.actor_id)
                .or_default()
                .push(module.clone());

            self.by_actor_name
                .entry((binding.actor_id, module.module_name.to_lowercase()))
                .or_default()
                .push(module.clone());

            self.by_path
                .entry(module.script_path.clone())
                .or_default()
                .push(binding.actor_id);
        }

        for modules in self.by_actor.values_mut() {
            modules.sort_by(|a, b| {
                a.script_path
                    .cmp(&b.script_path)
                    .then_with(|| a.module_name.cmp(&b.module_name))
            });
        }

        for actors in self.by_path.values_mut() {
            actors.sort_unstable();
            actors.dedup();
        }
    }

    pub fn actor_modules(&self, actor_id: u64) -> &[ActorModuleRef] {
        self.by_actor
            .get(&actor_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn actor_module(&self, actor_id: u64, module_name: &str) -> Option<ActorModuleRef> {
        let normalized_name = module_name
            .trim()
            .strip_suffix(".aeo")
            .unwrap_or(module_name.trim())
            .to_lowercase();

        self.by_actor_name
            .get(&(actor_id, normalized_name))
            .and_then(|modules| modules.first())
            .cloned()
    }

    pub fn actor_modules_named(&self, actor_id: u64, module_name: &str) -> Vec<ActorModuleRef> {
        let normalized_name = module_name
            .trim()
            .strip_suffix(".aeo")
            .unwrap_or(module_name.trim())
            .to_lowercase();

        self.by_actor_name
            .get(&(actor_id, normalized_name))
            .cloned()
            .unwrap_or_default()
    }

    pub fn actors_with_module_path(&self, script_path: &str) -> &[u64] {
        let normalized = script_path.replace('\\', "/");

        self.by_path
            .get(&normalized)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

impl World {
    pub fn rebuild_actor_module_index(&mut self) {
        self.actor_module_index
            .rebuild(&self.actors, &self.actor_script_bindings);
    }

    pub fn actor_modules(&self, actor_id: u64) -> &[ActorModuleRef] {
        self.actor_module_index.actor_modules(actor_id)
    }

    pub fn actor_module(&self, actor_id: u64, module_name: &str) -> Option<ActorModuleRef> {
        self.actor_module_index.actor_module(actor_id, module_name)
    }

    pub fn actor_modules_named(&self, actor_id: u64, module_name: &str) -> Vec<ActorModuleRef> {
        self.actor_module_index
            .actor_modules_named(actor_id, module_name)
    }

    pub fn actors_with_module_path(&self, script_path: &str) -> &[u64] {
        self.actor_module_index.actors_with_module_path(script_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn indexes_multiple_modules_per_actor() {
        let mut world = World::new();

        let actor_id = world.add_actor("Door", Vec3::ZERO);

        world.actor_script_bindings.push(ActorScriptBinding::new(
            actor_id,
            "scripts/Dungeon/doors.aeo",
        ));

        world.actor_script_bindings.push(ActorScriptBinding::new(
            actor_id,
            "scripts/Dungeon/combat.aeo",
        ));

        world.rebuild_actor_module_index();

        let modules = world.actor_modules(actor_id);

        assert_eq!(modules.len(), 2);
        assert_eq!(modules[0].module_name, "combat");
        assert_eq!(modules[1].module_name, "doors");
    }

    #[test]
    fn indexes_module_by_scoped_name() {
        let mut world = World::new();

        let actor_id = world.add_actor("Door", Vec3::ZERO);

        world.actor_script_bindings.push(ActorScriptBinding::new(
            actor_id,
            "scripts/Dungeon/doors.aeo",
        ));

        world.rebuild_actor_module_index();

        let module = world
            .actor_module(actor_id, "doors")
            .expect("Module should resolve");

        assert_eq!(module.script_path, "scripts/Dungeon/doors.aeo");
    }

    #[test]
    fn same_module_name_on_different_actors_stays_scoped() {
        let mut world = World::new();

        let first = world.add_actor("DoorA", Vec3::ZERO);
        let second = world.add_actor("DoorB", Vec3::ZERO);

        world
            .actor_script_bindings
            .push(ActorScriptBinding::new(first, "scripts/A/combat.aeo"));

        world
            .actor_script_bindings
            .push(ActorScriptBinding::new(second, "scripts/B/combat.aeo"));

        world.rebuild_actor_module_index();

        assert_eq!(
            world.actor_module(first, "combat").unwrap().script_path,
            "scripts/A/combat.aeo"
        );

        assert_eq!(
            world.actor_module(second, "combat").unwrap().script_path,
            "scripts/B/combat.aeo"
        );
    }

    #[test]
    fn path_index_returns_all_attached_actors() {
        let mut world = World::new();

        let first = world.add_actor("DoorA", Vec3::ZERO);
        let second = world.add_actor("DoorB", Vec3::ZERO);

        world
            .actor_script_bindings
            .push(ActorScriptBinding::new(first, "scripts/Dungeon/doors.aeo"));

        world.actor_script_bindings.push(ActorScriptBinding::new(
            second,
            r"scripts\Dungeon\doors.aeo",
        ));

        world.rebuild_actor_module_index();

        assert_eq!(
            world.actors_with_module_path("scripts/Dungeon/doors.aeo"),
            &[first, second]
        );
    }

    #[test]
    fn rebuild_drops_bindings_for_missing_actors() {
        let mut world = World::new();

        world
            .actor_script_bindings
            .push(ActorScriptBinding::new(999_999, "scripts/missing.aeo"));

        world.rebuild_actor_module_index();

        assert!(world.actor_modules(999_999).is_empty());
        assert!(
            world
                .actors_with_module_path("scripts/missing.aeo")
                .is_empty()
        );
    }
}
