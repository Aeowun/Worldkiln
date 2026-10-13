use glam::Vec3;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::engine::entity_package::{
    EntityAnimationClip, EntityAnimationDefinition, EntityAppearanceDefinition,
    EntityPhysicsDefaults, get_package,
};
use crate::world::cell::AttributeValue;

/// Unique runtime identifier for an Entity during a single Play session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u64);

/// Normalized contact pair between two Entities during a Play session.
///
/// `first` is guaranteed to be ordered before `second`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityContactPair {
    pub first: EntityId,
    pub second: EntityId,
}

impl EntityContactPair {
    pub fn new(a: EntityId, b: EntityId) -> Self {
        if a.0 < b.0 {
            Self {
                first: a,
                second: b,
            }
        } else {
            Self {
                first: b,
                second: a,
            }
        }
    }

    pub fn contains(&self, entity_id: EntityId) -> bool {
        self.first == entity_id || self.second == entity_id
    }

    pub fn other(&self, entity_id: EntityId) -> Option<EntityId> {
        if self.first == entity_id {
            Some(self.second)
        } else if self.second == entity_id {
            Some(self.first)
        } else {
            None
        }
    }
}

/// Result of evaluating Entity contact transitions for one physics tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityContactResult {
    pub current: HashSet<EntityContactPair>,
    pub entered: HashSet<EntityContactPair>,
    pub exited: HashSet<EntityContactPair>,
}

impl EntityContactResult {
    pub fn clear(&mut self) {
        self.current.clear();
        self.entered.clear();
        self.exited.clear();
    }
}

/// Generic runtime physics state attached to every Entity.
///
/// Physics is intentionally disabled by default so introducing the generic
/// Entity physics capability does not change the behavior of existing
/// CharacterSystem-driven entities or legacy runtime Entities.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityPhysicsState {
    pub enabled: bool,
    pub solid: bool,
    pub anchored: bool,
    pub collision_enabled: bool,

    pub mass: f32,
    pub velocity: Vec3,
    pub accumulated_force: Vec3,

    pub gravity_scale: f32,
    pub size: Vec3,
    pub offset: Vec3,

    pub grounded: bool,
}

impl Default for EntityPhysicsState {
    fn default() -> Self {
        Self {
            enabled: false,
            solid: true,
            anchored: true,
            collision_enabled: true,

            mass: 1.0,
            velocity: Vec3::ZERO,
            accumulated_force: Vec3::ZERO,

            gravity_scale: 1.0,
            size: Vec3::ONE,
            offset: Vec3::ZERO,

            grounded: false,
        }
    }
}

impl EntityPhysicsState {
    pub fn apply_defaults(&mut self, defaults: &EntityPhysicsDefaults) {
        self.enabled = defaults.enabled;
        self.solid = defaults.solid;
        self.anchored = defaults.anchored;
        self.collision_enabled = defaults.collision_enabled;
        self.mass = defaults.mass;

        self.velocity = Vec3::ZERO;
        self.accumulated_force = Vec3::ZERO;
        self.offset = Vec3::ZERO;
        self.grounded = false;
    }

    pub fn set_mass(&mut self, mass: f32) -> Result<(), String> {
        if !mass.is_finite() || mass <= 0.0 {
            return Err("Entity physics mass must be finite and greater than zero".to_string());
        }

        self.mass = mass;
        Ok(())
    }

    pub fn set_gravity_scale(&mut self, gravity_scale: f32) -> Result<(), String> {
        if !gravity_scale.is_finite() {
            return Err("Entity physics gravity_scale must be finite".to_string());
        }

        self.gravity_scale = gravity_scale;
        Ok(())
    }

    pub fn set_size(&mut self, size: Vec3) -> Result<(), String> {
        if !size.is_finite() || size.x <= 0.0 || size.y <= 0.0 || size.z <= 0.0 {
            return Err(
                "Entity physics size must contain finite positive x, y, and z values".to_string(),
            );
        }

        self.size = size;
        Ok(())
    }

    pub fn set_offset(&mut self, offset: Vec3) -> Result<(), String> {
        if !offset.is_finite() {
            return Err("Entity physics offset must contain finite values".to_string());
        }

        self.offset = offset;
        Ok(())
    }

    pub fn collider_center(&self, position: Vec3) -> Vec3 {
        position + self.offset
    }

    pub fn set_velocity(&mut self, velocity: Vec3) {
        self.velocity = velocity;
        self.grounded = false;
    }

    pub fn apply_force(&mut self, force: Vec3) {
        if force.is_finite() {
            self.accumulated_force += force;
        }
    }

    pub fn apply_impulse(&mut self, impulse: Vec3) {
        if impulse.is_finite() {
            self.velocity += impulse / self.mass;
            self.grounded = false;
        }
    }

    pub fn stop(&mut self) {
        self.velocity = Vec3::ZERO;
        self.accumulated_force = Vec3::ZERO;
    }

    pub fn wake(&mut self) {
        self.grounded = false;
    }

    pub fn half_extents(&self) -> Vec3 {
        self.size * 0.5
    }
}

/// Runtime appearance overrides for a package-backed Entity.
///
/// The authored package remains the source of truth. These maps contain only
/// transient Play-mode overrides and are discarded with the EntityManager.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntityAppearanceState {
    pub material_overrides: HashMap<String, [f32; 4]>,
    pub node_visibility: HashMap<String, bool>,
}

impl EntityAppearanceState {
    pub fn reset(&mut self) {
        self.material_overrides.clear();
        self.node_visibility.clear();
    }

    pub fn set_material(&mut self, name: impl Into<String>, color: [f32; 4]) -> bool {
        if !color.iter().all(|value| value.is_finite()) {
            return false;
        }

        self.material_overrides.insert(name.into(), color);
        true
    }

    pub fn set_node_visible(&mut self, name: impl Into<String>, visible: bool) {
        self.node_visibility.insert(name.into(), visible);
    }
}

/// Runtime animation state for a package-backed Entity.
///
/// The animation definition remains owned by EntityPackage. This struct owns
/// only transient selection and playback state.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityAnimationState {
    pub current_clip: Option<String>,
    pub time: f32,
    pub playing: bool,
    pub playback_rate: f32,
}

impl Default for EntityAnimationState {
    fn default() -> Self {
        Self {
            current_clip: None,
            time: 0.0,
            playing: true,
            playback_rate: 1.0,
        }
    }
}

impl EntityAnimationState {
    fn from_definition(definition: &EntityAnimationDefinition) -> Self {
        let current_clip = definition
            .default_clip
            .clone()
            .or_else(|| definition.clips.first().map(|clip| clip.name.clone()));

        Self {
            current_clip,
            time: 0.0,
            playing: true,
            playback_rate: 1.0,
        }
    }

    pub fn current_clip_definition<'a>(
        &'a self,
        definition: &'a EntityAnimationDefinition,
    ) -> Option<&'a EntityAnimationClip> {
        let clip_name = self.current_clip.as_deref()?;

        definition
            .clips
            .iter()
            .find(|clip| clip.name.eq_ignore_ascii_case(clip_name))
    }

    pub fn reset(&mut self) {
        self.time = 0.0;
    }

    pub fn advance(&mut self, definition: &EntityAnimationDefinition, dt: f32) {
        if !self.playing || !dt.is_finite() || dt <= 0.0 {
            return;
        }

        let Some(clip) = self.current_clip_definition(definition) else {
            return;
        };

        let duration = clip.duration.max(0.0001);
        let scaled_dt = dt * self.playback_rate;

        if !scaled_dt.is_finite() {
            return;
        }

        if clip.looping {
            self.time = (self.time + scaled_dt).rem_euclid(duration);
        } else {
            self.time = (self.time + scaled_dt).clamp(0.0, duration);

            if self.time >= duration {
                self.playing = false;
            }
        }
    }

    fn select_clip(&mut self, definition: &EntityAnimationDefinition, name: &str) -> bool {
        let Some(clip) = definition
            .clips
            .iter()
            .find(|clip| clip.name.eq_ignore_ascii_case(name))
        else {
            return false;
        };

        self.current_clip = Some(clip.name.clone());
        self.time = 0.0;
        self.playing = true;
        true
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    pub fn set_playback_rate(&mut self, rate: f32) -> bool {
        if !rate.is_finite() || rate < 0.0 {
            return false;
        }

        self.playback_rate = rate;
        true
    }
}

/// Complete generic runtime state for an Entity.
///
/// This is intentionally independent of CharacterSystem. A Character may
/// provide additional capabilities, but every package-backed runtime Entity
/// can own generic appearance and animation state here.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeEntity {
    pub id: EntityId,
    pub name: String,

    pub position: Vec3,
    pub rotation: Vec3,
    pub scale: Vec3,

    pub visible: bool,
    pub enabled: bool,

    pub package_name: Option<String>,
    pub appearance: EntityAppearanceState,
    pub animation: EntityAnimationState,

    pub attributes: BTreeMap<String, AttributeValue>,

    pub physics: EntityPhysicsState,
}

impl RuntimeEntity {
    pub fn new(id: EntityId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),

            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,

            visible: true,
            enabled: true,

            package_name: None,
            appearance: EntityAppearanceState::default(),
            animation: EntityAnimationState::default(),

            attributes: BTreeMap::new(),

            physics: EntityPhysicsState::default(),
        }
    }
}

/// Registry for runtime Entity identity.
///
/// This registry is valid only for the duration of a Play session. It is
/// intentionally discarded when returning to the Editor.
pub struct EntityManager {
    entities: HashMap<EntityId, RuntimeEntity>,
    name_to_id: HashMap<String, EntityId>,
    next_id: u64,

    pub previous_contacts: HashSet<EntityContactPair>,
    pub last_contact_result: EntityContactResult,
}

impl EntityManager {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            name_to_id: HashMap::new(),
            next_id: 1,

            previous_contacts: HashSet::new(),
            last_contact_result: EntityContactResult::default(),
        }
    }

    /// Creates a new runtime Entity and returns its ID.
    pub fn create_entity(&mut self, name: &str) -> EntityId {
        let id = EntityId(self.next_id);
        self.next_id += 1;

        let entity = RuntimeEntity::new(id, name);

        self.entities.insert(id, entity);
        self.name_to_id.insert(name.to_string(), id);

        id
    }

    pub fn lookup_entity(&self, name: &str) -> Option<EntityId> {
        self.name_to_id.get(name).copied()
    }

    pub fn get_entity(&self, id: EntityId) -> Option<&RuntimeEntity> {
        self.entities.get(&id)
    }

    pub fn get_entity_mut(&mut self, id: EntityId) -> Option<&mut RuntimeEntity> {
        self.entities.get_mut(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RuntimeEntity> {
        self.entities.values()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut RuntimeEntity> {
        self.entities.values_mut()
    }

    pub fn validate_handle(&self, id: u64) -> bool {
        self.entities.contains_key(&EntityId(id))
    }

    pub fn get_name(&self, id: EntityId) -> Option<&str> {
        self.entities.get(&id).map(|entity| entity.name.as_str())
    }

    pub fn get_position(&self, id: EntityId) -> Option<Vec3> {
        self.entities.get(&id).map(|entity| entity.position)
    }

    pub fn set_position(&mut self, id: EntityId, position: Vec3) {
        if let Some(entity) = self.entities.get_mut(&id) {
            entity.position = position;
        }
    }

    pub fn get_rotation(&self, id: EntityId) -> Option<Vec3> {
        self.entities.get(&id).map(|entity| entity.rotation)
    }

    pub fn set_rotation(&mut self, id: EntityId, rotation: Vec3) {
        if let Some(entity) = self.entities.get_mut(&id) {
            entity.rotation = rotation;
        }
    }

    pub fn get_scale(&self, id: EntityId) -> Option<Vec3> {
        self.entities.get(&id).map(|entity| entity.scale)
    }

    pub fn set_scale(&mut self, id: EntityId, scale: Vec3) {
        if let Some(entity) = self.entities.get_mut(&id) {
            entity.scale = scale;
        }
    }

    pub fn is_visible(&self, id: EntityId) -> Option<bool> {
        self.entities.get(&id).map(|entity| entity.visible)
    }

    pub fn set_visible(&mut self, id: EntityId, visible: bool) {
        if let Some(entity) = self.entities.get_mut(&id) {
            entity.visible = visible;
        }
    }

    pub fn is_enabled(&self, id: EntityId) -> Option<bool> {
        self.entities.get(&id).map(|entity| entity.enabled)
    }

    pub fn set_enabled(&mut self, id: EntityId, enabled: bool) {
        if let Some(entity) = self.entities.get_mut(&id) {
            entity.enabled = enabled;
        }
    }

    pub fn get_physics(&self, id: EntityId) -> Option<&EntityPhysicsState> {
        self.entities.get(&id).map(|entity| &entity.physics)
    }

    pub fn get_physics_mut(&mut self, id: EntityId) -> Option<&mut EntityPhysicsState> {
        self.entities.get_mut(&id).map(|entity| &mut entity.physics)
    }

    pub fn apply_physics_defaults(
        &mut self,
        id: EntityId,
        defaults: &EntityPhysicsDefaults,
    ) -> bool {
        let Some(physics) = self.get_physics_mut(id) else {
            return false;
        };

        physics.apply_defaults(defaults);
        true
    }

    /// Binds a generic Entity to an EntityPackage and initializes all
    /// package-defined runtime capabilities.
    pub fn apply_package(&mut self, id: EntityId, package_name: &str) -> bool {
        let Some(package) = get_package(package_name) else {
            return false;
        };

        let Some(entity) = self.get_entity_mut(id) else {
            return false;
        };

        entity.package_name = Some(package_name.to_string());
        entity.appearance.reset();
        entity.animation = EntityAnimationState::from_definition(&package.animation);
        entity.physics.apply_defaults(&package.physics);

        true
    }

    /// Compatibility entry point used by the existing runtime actor bootstrap.
    ///
    /// It now binds the entire generic package, not only the physics defaults,
    /// so existing package-backed Entity creation automatically gains the
    /// generic appearance and animation capabilities.
    pub fn apply_package_physics(&mut self, id: EntityId, package_name: &str) -> bool {
        self.apply_package(id, package_name)
    }

    pub fn get_appearance(&self, id: EntityId) -> Option<&EntityAppearanceState> {
        self.entities.get(&id).map(|entity| &entity.appearance)
    }

    pub fn get_appearance_mut(&mut self, id: EntityId) -> Option<&mut EntityAppearanceState> {
        self.entities
            .get_mut(&id)
            .map(|entity| &mut entity.appearance)
    }

    pub fn set_appearance_material(
        &mut self,
        id: EntityId,
        name: impl Into<String>,
        color: [f32; 4],
    ) -> bool {
        let Some(appearance) = self.get_appearance_mut(id) else {
            return false;
        };

        appearance.set_material(name, color)
    }

    pub fn set_appearance_node_visible(
        &mut self,
        id: EntityId,
        name: impl Into<String>,
        visible: bool,
    ) -> bool {
        let Some(appearance) = self.get_appearance_mut(id) else {
            return false;
        };

        appearance.set_node_visible(name, visible);
        true
    }

    pub fn clear_appearance_overrides(&mut self, id: EntityId) -> bool {
        let Some(appearance) = self.get_appearance_mut(id) else {
            return false;
        };

        appearance.reset();
        true
    }

    pub fn get_animation(&self, id: EntityId) -> Option<&EntityAnimationState> {
        self.entities.get(&id).map(|entity| &entity.animation)
    }

    pub fn get_animation_mut(&mut self, id: EntityId) -> Option<&mut EntityAnimationState> {
        self.entities
            .get_mut(&id)
            .map(|entity| &mut entity.animation)
    }

    pub fn set_animation(&mut self, id: EntityId, clip_name: &str) -> bool {
        let package_name = self
            .entities
            .get(&id)
            .and_then(|entity| entity.package_name.clone());

        let Some(package_name) = package_name else {
            return false;
        };

        let Some(package) = get_package(&package_name) else {
            return false;
        };

        let Some(entity) = self.entities.get_mut(&id) else {
            return false;
        };

        entity.animation.select_clip(&package.animation, clip_name)
    }

    pub fn set_animation_playing(&mut self, id: EntityId, playing: bool) -> bool {
        let Some(animation) = self.get_animation_mut(id) else {
            return false;
        };

        animation.set_playing(playing);
        true
    }

    pub fn set_animation_playback_rate(&mut self, id: EntityId, rate: f32) -> bool {
        let Some(animation) = self.get_animation_mut(id) else {
            return false;
        };

        animation.set_playback_rate(rate)
    }

    pub fn reset_animation(&mut self, id: EntityId) -> bool {
        let Some(animation) = self.get_animation_mut(id) else {
            return false;
        };

        animation.reset();
        true
    }

    /// Advances all package-backed Entity animations in the fixed simulation
    /// timestep. This updates runtime state only; authored package data is
    /// never modified.
    pub fn update_animations(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }

        for entity in self.entities.values_mut() {
            let Some(package_name) = entity.package_name.as_deref() else {
                continue;
            };

            let Some(package) = get_package(package_name) else {
                continue;
            };

            entity.animation.advance(&package.animation, dt);
        }
    }

    pub fn set_physics_velocity(&mut self, id: EntityId, velocity: Vec3) -> bool {
        let Some(physics) = self.get_physics_mut(id) else {
            return false;
        };

        physics.set_velocity(velocity);
        true
    }

    pub fn apply_physics_force(&mut self, id: EntityId, force: Vec3) -> bool {
        let Some(physics) = self.get_physics_mut(id) else {
            return false;
        };

        physics.apply_force(force);
        true
    }

    pub fn apply_physics_impulse(&mut self, id: EntityId, impulse: Vec3) -> bool {
        let Some(physics) = self.get_physics_mut(id) else {
            return false;
        };

        physics.apply_impulse(impulse);
        true
    }

    pub fn stop_physics(&mut self, id: EntityId) -> bool {
        let Some(physics) = self.get_physics_mut(id) else {
            return false;
        };

        physics.stop();
        true
    }

    pub fn wake_physics(&mut self, id: EntityId) -> bool {
        let Some(physics) = self.get_physics_mut(id) else {
            return false;
        };

        physics.wake();
        true
    }

    pub fn get_attributes(&self, id: EntityId) -> Option<&BTreeMap<String, AttributeValue>> {
        self.entities.get(&id).map(|entity| &entity.attributes)
    }

    pub fn get_attributes_mut(
        &mut self,
        id: EntityId,
    ) -> Option<&mut BTreeMap<String, AttributeValue>> {
        self.entities
            .get_mut(&id)
            .map(|entity| &mut entity.attributes)
    }

    pub fn set_attribute(
        &mut self,
        id: EntityId,
        name: impl Into<String>,
        value: AttributeValue,
    ) -> bool {
        let Some(entity) = self.entities.get_mut(&id) else {
            return false;
        };

        entity.attributes.insert(name.into(), value);
        true
    }

    pub fn remove_attribute(&mut self, id: EntityId, name: &str) -> Option<AttributeValue> {
        self.entities
            .get_mut(&id)
            .and_then(|entity| entity.attributes.remove(name))
    }

    pub fn remove_entity(&mut self, id: EntityId) {
        let Some(entity) = self.entities.remove(&id) else {
            return;
        };

        if self.name_to_id.get(&entity.name) == Some(&id) {
            self.name_to_id.remove(&entity.name);

            if let Some(replacement) = self
                .entities
                .values()
                .filter(|candidate| candidate.name == entity.name)
                .max_by_key(|candidate| candidate.id)
            {
                self.name_to_id.insert(entity.name.clone(), replacement.id);
            }
        }

        self.previous_contacts.retain(|pair| !pair.contains(id));
        self.last_contact_result
            .current
            .retain(|pair| !pair.contains(id));
        self.last_contact_result
            .entered
            .retain(|pair| !pair.contains(id));
        self.last_contact_result
            .exited
            .retain(|pair| !pair.contains(id));
    }

    pub fn clear(&mut self) {
        self.entities.clear();
        self.name_to_id.clear();
        self.next_id = 1;
        self.previous_contacts.clear();
        self.last_contact_result.clear();
    }
}

impl Default for EntityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_starts_empty() {
        let manager = EntityManager::new();
        assert!(manager.entities.is_empty());
    }

    #[test]
    fn creating_entity_produces_unique_id() {
        let mut manager = EntityManager::new();
        let id1 = manager.create_entity("Entity1");
        let id2 = manager.create_entity("Entity2");
        assert_ne!(id1, id2);
    }

    #[test]
    fn new_entity_has_generic_defaults() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Test");
        let entity = manager.get_entity(id).unwrap();

        assert_eq!(entity.position, Vec3::ZERO);
        assert_eq!(entity.rotation, Vec3::ZERO);
        assert_eq!(entity.scale, Vec3::ONE);
        assert!(entity.visible);
        assert!(entity.enabled);
        assert!(entity.attributes.is_empty());
        assert!(entity.package_name.is_none());
        assert!(entity.appearance.material_overrides.is_empty());
        assert!(entity.appearance.node_visibility.is_empty());
        assert!(entity.animation.current_clip.is_none());
        assert_eq!(entity.animation.time, 0.0);
        assert!(entity.animation.playing);
        assert_eq!(entity.animation.playback_rate, 1.0);

        assert!(!entity.physics.enabled);
        assert!(entity.physics.solid);
        assert!(entity.physics.anchored);
        assert!(entity.physics.collision_enabled);
        assert_eq!(entity.physics.mass, 1.0);
        assert_eq!(entity.physics.velocity, Vec3::ZERO);
        assert_eq!(entity.physics.gravity_scale, 1.0);
        assert_eq!(entity.physics.size, Vec3::ONE);
        assert!(!entity.physics.grounded);
    }

    #[test]
    fn entity_can_be_looked_up_by_name() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Test");
        assert_eq!(manager.lookup_entity("Test"), Some(id));
    }

    #[test]
    fn valid_handle_validates_successfully() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Test");
        assert!(manager.validate_handle(id.0));
    }

    #[test]
    fn removing_entity_invalidates_handle() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Test");
        manager.remove_entity(id);
        assert!(!manager.validate_handle(id.0));
        assert_eq!(manager.lookup_entity("Test"), None);
    }

    #[test]
    fn duplicate_identity_behavior() {
        let mut manager = EntityManager::new();
        let id1 = manager.create_entity("Duplicate");
        let id2 = manager.create_entity("Duplicate");

        assert_ne!(id1, id2);
        assert_eq!(manager.lookup_entity("Duplicate"), Some(id2));

        manager.remove_entity(id2);

        assert_eq!(manager.lookup_entity("Duplicate"), Some(id1));
        assert!(manager.validate_handle(id1.0));
        assert!(!manager.validate_handle(id2.0));
    }

    #[test]
    fn transform_state_is_independent() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Transform");

        let position = Vec3::new(1.0, 2.0, 3.0);
        let rotation = Vec3::new(0.1, 0.2, 0.3);
        let scale = Vec3::new(2.0, 3.0, 4.0);

        manager.set_position(id, position);
        manager.set_rotation(id, rotation);
        manager.set_scale(id, scale);

        assert_eq!(manager.get_position(id), Some(position));
        assert_eq!(manager.get_rotation(id), Some(rotation));
        assert_eq!(manager.get_scale(id), Some(scale));
    }

    #[test]
    fn visibility_and_enabled_state_are_generic() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Generic");

        manager.set_visible(id, false);
        manager.set_enabled(id, false);

        assert_eq!(manager.is_visible(id), Some(false));
        assert_eq!(manager.is_enabled(id), Some(false));
    }

    #[test]
    fn entity_attributes_are_independent_of_character_state() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Door");

        assert!(manager.set_attribute(id, "state", AttributeValue::String("closed".to_string())));
        assert!(manager.set_attribute(id, "locked", AttributeValue::Bool(true)));

        assert_eq!(
            manager
                .get_attributes(id)
                .and_then(|attributes| attributes.get("state")),
            Some(&AttributeValue::String("closed".to_string()))
        );

        assert_eq!(
            manager
                .get_attributes(id)
                .and_then(|attributes| attributes.get("locked")),
            Some(&AttributeValue::Bool(true))
        );

        assert_eq!(
            manager.remove_attribute(id, "locked"),
            Some(AttributeValue::Bool(true))
        );
    }

    #[test]
    fn physics_defaults_can_be_applied() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("PhysicsBody");

        let defaults = EntityPhysicsDefaults {
            enabled: true,
            solid: true,
            anchored: false,
            collision_enabled: true,
            mass: 4.0,
        };

        assert!(manager.apply_physics_defaults(id, &defaults));

        let physics = manager.get_physics(id).unwrap();

        assert!(physics.enabled);
        assert!(physics.solid);
        assert!(!physics.anchored);
        assert!(physics.collision_enabled);
        assert_eq!(physics.mass, 4.0);
        assert_eq!(physics.velocity, Vec3::ZERO);
        assert_eq!(physics.accumulated_force, Vec3::ZERO);
    }

    #[test]
    fn physics_force_and_impulse_are_distinct() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("PhysicsBody");

        manager.get_physics_mut(id).unwrap().enabled = true;

        assert!(manager.apply_physics_force(id, Vec3::new(4.0, 0.0, 0.0)));
        assert_eq!(
            manager.get_physics(id).unwrap().accumulated_force,
            Vec3::new(4.0, 0.0, 0.0)
        );

        assert!(manager.apply_physics_impulse(id, Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(
            manager.get_physics(id).unwrap().velocity,
            Vec3::new(2.0, 0.0, 0.0)
        );
    }

    #[test]
    fn physics_mass_and_size_validate() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("PhysicsBody");

        let physics = manager.get_physics_mut(id).unwrap();

        assert!(physics.set_mass(2.0).is_ok());
        assert_eq!(physics.mass, 2.0);

        assert!(physics.set_mass(0.0).is_err());
        assert!(physics.set_mass(f32::NAN).is_err());

        assert!(physics.set_size(Vec3::new(1.0, 2.0, 3.0)).is_ok());
        assert_eq!(physics.size, Vec3::new(1.0, 2.0, 3.0));
        assert!(physics.set_size(Vec3::ZERO).is_err());
    }

    #[test]
    fn generic_state_is_removed_with_entity() {
        let mut manager = EntityManager::new();
        let id = manager.create_entity("Test");

        manager.set_position(id, Vec3::new(5.0, 6.0, 7.0));
        manager.set_rotation(id, Vec3::new(0.5, 1.0, 1.5));
        manager.set_scale(id, Vec3::new(2.0, 2.0, 2.0));
        manager.set_visible(id, false);
        manager.set_enabled(id, false);
        manager.get_physics_mut(id).unwrap().enabled = true;
        manager.set_attribute(id, "health", AttributeValue::Number(100.0));

        manager.remove_entity(id);

        assert!(manager.get_entity(id).is_none());
        assert!(!manager.validate_handle(id.0));
        assert_eq!(manager.get_position(id), None);
        assert_eq!(manager.get_rotation(id), None);
        assert_eq!(manager.get_scale(id), None);
        assert_eq!(manager.is_visible(id), None);
        assert_eq!(manager.is_enabled(id), None);
        assert_eq!(manager.get_attributes(id), None);
        assert_eq!(manager.get_physics(id), None);
    }

    #[test]
    fn contact_pair_normalization() {
        let id1 = EntityId(10);
        let id2 = EntityId(20);

        let pair1 = EntityContactPair::new(id1, id2);
        let pair2 = EntityContactPair::new(id2, id1);

        assert_eq!(pair1, pair2);
        assert_eq!(pair1.first, id1);
        assert_eq!(pair1.second, id2);
        assert!(pair1.contains(id1));
        assert!(pair1.contains(id2));
        assert_eq!(pair1.other(id1), Some(id2));
        assert_eq!(pair1.other(id2), Some(id1));
    }

    #[test]
    fn contact_state_clears_with_manager() {
        let mut manager = EntityManager::new();
        let e1 = manager.create_entity("A");
        let e2 = manager.create_entity("B");
        let pair = EntityContactPair::new(e1, e2);

        manager.previous_contacts.insert(pair);
        manager.last_contact_result.current.insert(pair);

        manager.clear();

        assert!(manager.previous_contacts.is_empty());
        assert!(manager.last_contact_result.current.is_empty());
    }
}
