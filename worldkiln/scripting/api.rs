use super::value::{HandleKind, Value};
use crate::engine::entity::{EntityId, EntityManager};
use glam::Vec3;

/// Abstraction for engine services exposed to AeoScript.
///
/// This bridge allows the scripting runtime to interact with engine systems
/// without being coupled to the full App or Renderer implementation.
pub trait EngineHost {
    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    fn terrain_generate(&mut self, _position: [i32; 3], _size: [i32; 3]) -> Result<(), String> {
        Err("terrain generation is not available on this host".into())
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:05 -05:00
    fn terrain_set_voxels(
        &mut self,
        _origin: [i32; 3],
        _size: [u32; 3],
        _spacing_studs: f32,
        _materials: Vec<String>,
        _voxels: Vec<(u16, f32)>,
    ) -> Result<(), String> {
        Err("programmatic voxel terrain is not available on this host".into())
    }

    fn particle_burst(
        &mut self,
        _preset: &str,
        _position: Vec3,
        _count: usize,
    ) -> Result<usize, String> {
        Err("particles are not available on this host".into())
    }
    fn particle_set_enabled(&mut self, _id: u64, _enabled: bool) -> Result<(), String> {
        Err("particles are not available on this host".into())
    }
    fn particle_clear(&mut self) -> Result<(), String> {
        Err("particles are not available on this host".into())
    }
    fn particle_count(&self) -> usize {
        0
    }

    fn entity_manager(&self) -> &EntityManager;
    fn get_position(&self, id: u64) -> Option<Vec3>;
    fn set_position(&mut self, id: u64, position: Vec3);

    fn lookup_light(&self, x: i32, y: i32, z: i32) -> Option<u64>;
    fn is_light_enabled(&self, id: u64) -> Option<bool>;
    fn set_light_enabled(&mut self, id: u64, enabled: bool);
    fn is_collision_events_enabled(&self, id: u64) -> bool;

    // Generic object model support
    fn get_all_cells_of_class(&self, class_name: &str) -> Vec<u64>;
    fn find_objects(&self, query: &str) -> Vec<(HandleKind, u64)>;
    fn get_children(&self, kind: HandleKind, id: u64) -> Vec<(HandleKind, u64)>;
    fn get_parent(&self, kind: HandleKind, id: u64) -> Option<(HandleKind, u64)>;
    fn get_cell_object(&self, cell_id: u64) -> Option<(HandleKind, u64)>;

    fn actor_exists(&self, _actor_id: u64) -> bool {
        false
    }

    /// Resolves an Actor-attached Module name to its project-relative script path.
    ///
    /// Hosts that do not expose authored Actor Modules may leave the default.
    fn get_actor_module_path(&self, _actor_id: u64, _module_name: &str) -> Option<String> {
        None
    }

    fn get_property(&self, kind: HandleKind, id: u64, name: &str) -> Result<Option<Value>, String>;
    fn set_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: &str,
        value: Value,
    ) -> Result<bool, String>;
    fn call_method(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: &str,
        args: &[Value],
    ) -> Result<Option<Value>, String>;

    fn set_attribute(&mut self, id: u64, key: String, value: Value) -> Result<(), String>;
    fn remove_attribute(&mut self, id: u64, key: &str) -> Result<(), String>;

    fn cell_exists(&self, id: u64) -> bool;
    fn entity_exists(&self, id: u64) -> bool;

    fn get_script_property(
        &self,
        kind: HandleKind,
        id: u64,
        name: &str,
    ) -> Option<crate::scripting::value::DynamicProperty>;
    fn get_script_property_with_owner(
        &self,
        _kind: HandleKind,
        _id: u64,
        _name: &str,
    ) -> Option<crate::scripting::value::DynamicProperty> {
        None
    }
    fn set_script_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        name: String,
        value: Value,
        owner_script: Option<String>,
    );
    fn remove_dynamic_properties_by_owner(&mut self, _owner_script: &str) {}

    // Audio support
    fn is_audio_playing(&self, _id: u64) -> Option<bool> {
        None
    }
    fn is_audio_paused(&self, _id: u64) -> Option<bool> {
        None
    }
    fn is_audio_looped(&self, _id: u64) -> Option<bool> {
        None
    }
    fn get_audio_volume(&self, _id: u64) -> Option<f32> {
        None
    }
    fn set_audio_playing(&mut self, _id: u64, _playing: bool) {}
    fn set_audio_looped(&mut self, _id: u64, _looped: bool) {}
    fn set_audio_volume(&mut self, _id: u64, _volume: f32) {}
    fn audio_play(&mut self, _id: u64) {}
    fn audio_stop(&mut self, _id: u64) {}
    fn audio_pause(&mut self, _id: u64) {}

    fn create_runtime_cell(&mut self, cell_type: &str) -> Result<(HandleKind, u64), String>;
    fn move_runtime_cell(&mut self, id: u64, x: i32, y: i32, z: i32) -> Result<(), String>;
    fn delete_cell(&mut self, id: u64) -> Result<(), String>;

    // Runtime NPC / Entity spawning & destruction
    fn spawn_entity(
        &mut self,
        _entity_name: &str,
        _position: Vec3,
        _package_name: Option<&str>,
    ) -> Result<u64, String> {
        Err("spawn_entity is not supported on this host".to_string())
    }
    fn spawn_character(
        &mut self,
        entity_name: &str,
        position: Vec3,
        character_package: Option<&str>,
    ) -> Result<u64, String> {
        self.spawn_entity(entity_name, position, character_package)
    }
    fn attach_entity(
        &mut self,
        _child_kind: HandleKind,
        _child_id: u64,
        _child_point: &str,
        _parent_kind: HandleKind,
        _parent_id: u64,
        _parent_point: &str,
    ) -> Result<(), String> {
        Err("attach is not supported on this host".to_string())
    }
    fn detach_entity(&mut self, _child_kind: HandleKind, _child_id: u64) -> Result<bool, String> {
        Err("detach is not supported on this host".to_string())
    }
    fn destroy_entity_runtime(&mut self, _id: u64) {}
    fn drain_pending_destroys(&mut self) -> Vec<u64> {
        Vec::new()
    }
    fn remove_dynamic_properties_for_handle(&mut self, _kind: HandleKind, _id: u64) {}

    fn is_entity_declaration_valid(&self, _entity_name: &str) -> bool {
        false
    }
    fn set_valid_entity_declarations(
        &mut self,
        _decls: std::sync::Arc<std::collections::HashSet<String>>,
    ) {
    }
    fn drain_pending_spawns(&mut self) -> Vec<(String, u64)> {
        Vec::new()
    }

    // Runtime UI support
    fn find_ui_element(&self, _document: Option<&str>, _key: &str) -> Option<u64> {
        None
    }
    fn create_ui_element(&mut self, _element_type: &str) -> Result<u64, String> {
        Err("Runtime UI is not supported by this host".to_string())
    }
    fn delete_ui_element(&mut self, _id: u64) -> Result<(), String> {
        Err("Runtime UI is not supported by this host".to_string())
    }
    fn get_ui_property(&self, _id: u64, _name: &str) -> Result<Option<Value>, String> {
        Ok(None)
    }
    fn set_ui_property(&mut self, _id: u64, _name: &str, _value: Value) -> Result<bool, String> {
        Ok(false)
    }
    fn drain_ui_clicks(&mut self) -> Vec<u64> {
        Vec::new()
    }
    // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:13 -05:00
    fn get_ui_symbolic_event_dispatch(
        &self,
        _id: u64,
        _event: &str,
    ) -> Option<crate::engine::ui::UiSymbolicEventDispatch> {
        None
    }
    fn get_viewport_size(&self) -> [f32; 2] {
        [0.0, 0.0]
    }
    fn set_viewport_size(&mut self, _width: f32, _height: f32) {}
    // Generic Gameplay, Input, Camera, Physics APIs
    fn get_input_move_vector(&self) -> [f32; 2] {
        [0.0, 0.0]
    }
    fn is_input_jump_pressed(&self) -> bool {
        false
    }
    fn get_input_orbit_delta(&self) -> [f32; 2] {
        [0.0, 0.0]
    }
    fn is_input_key_down(&self, _key: &str) -> bool {
        false
    }

    fn get_camera_horizontal_basis(&self) -> ([f32; 3], [f32; 3]) {
        ([0.0, 0.0, -1.0], [1.0, 0.0, 0.0])
    }
    fn set_camera_position(&mut self, _pos: [f32; 3]) {}
    fn set_camera_target(&mut self, _target: [f32; 3]) {}
    fn set_camera_orientation(&mut self, _yaw: f32, _pitch: f32) {}

    // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
    fn get_camera_script(&self) -> String {
        "thirdPerson".to_string()
    }
    fn set_camera_script(&mut self, _name: &str) -> Result<(), String> {
        Err("runtime camera script switching is unavailable on this host".into())
    }

    fn resolve_camera_collision(&self, _target: [f32; 3], desired: [f32; 3]) -> [f32; 3] {
        desired
    }

    fn get_player_actor_id(&self) -> Option<u64> {
        None
    }

    /// Resolves a runtime Entity to its script-facing Actor ID when the
    /// Entity currently represents an Actor.
    fn get_script_actor_id_for_entity(&self, _entity_id: u64) -> Option<u64> {
        None
    }

    fn get_player_position(&self) -> Option<[f32; 3]> {
        None
    }
    fn set_player_horizontal_velocity(&mut self, _vx: f32, _vz: f32) {}
    fn set_player_facing_direction(&mut self, _dx: f32, _dz: f32) {}
    fn select_player_animation(&mut self, _anim: &str) {}
    fn is_player_grounded(&self) -> bool {
        true
    }
    fn apply_player_vertical_impulse(&mut self, _impulse: f32) {}

    // Runtime script management, event firing, and test orchestration
    fn enable_script(&mut self, _path: &str) {}
    fn disable_script(&mut self, _path: &str) {}
    fn is_script_enabled(&self, _path: &str) -> bool {
        true
    }
    fn drain_enabled_scripts(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn drain_disabled_scripts(&mut self) -> Vec<String> {
        Vec::new()
    }
    fn fire_event(&mut self, _event_name: &str, _args: Vec<Value>) {}
    fn drain_pending_events(&mut self) -> Vec<(String, Vec<Value>)> {
        Vec::new()
    }
    fn complete_test(&mut self, _test_name: &str, _passed: bool) {}
    fn is_test_completed(&self, _test_name: &str) -> Option<bool> {
        None
    }
    fn get_test_results(&self) -> (usize, usize, usize) {
        (0, 0, 0)
    }

    // Typed runtime save state. Hosts without an active project deliberately
    // reject persistence rather than writing into authored project content.
    fn save_get(&self, _key: &str) -> Result<Option<Value>, String> {
        Err("save state is not available on this host".to_string())
    }
    fn save_set(&mut self, _key: &str, _value: Value) -> Result<(), String> {
        Err("save state is not available on this host".to_string())
    }
    fn save_has(&self, _key: &str) -> Result<bool, String> {
        Err("save state is not available on this host".to_string())
    }
    fn save_remove(&mut self, _key: &str) -> Result<bool, String> {
        Err("save state is not available on this host".to_string())
    }
    fn save_commit(&mut self) -> Result<(), String> {
        Err("save state is not available on this host".to_string())
    }
}

/// A simple implementation of EngineHost that just wraps an EntityManager.
/// Useful for tests and the initial bridge implementation.
impl EngineHost for EntityManager {
    fn entity_manager(&self) -> &EntityManager {
        self
    }

    fn get_position(&self, id: u64) -> Option<Vec3> {
        EntityManager::get_position(self, EntityId(id))
    }

    fn set_position(&mut self, id: u64, position: Vec3) {
        EntityManager::set_position(self, EntityId(id), position);
    }

    fn lookup_light(&self, _x: i32, _y: i32, _z: i32) -> Option<u64> {
        None
    }

    fn is_light_enabled(&self, _id: u64) -> Option<bool> {
        None
    }

    fn set_light_enabled(&mut self, _id: u64, _enabled: bool) {}

    fn is_collision_events_enabled(&self, _id: u64) -> bool {
        true
    }

    fn get_all_cells_of_class(&self, _class_name: &str) -> Vec<u64> {
        Vec::new()
    }

    fn find_objects(&self, _query: &str) -> Vec<(HandleKind, u64)> {
        Vec::new()
    }

    fn get_children(&self, _kind: HandleKind, _id: u64) -> Vec<(HandleKind, u64)> {
        Vec::new()
    }

    fn get_parent(&self, _kind: HandleKind, _id: u64) -> Option<(HandleKind, u64)> {
        None
    }

    fn get_cell_object(&self, _cell_id: u64) -> Option<(HandleKind, u64)> {
        None
    }

    fn get_property(
        &self,
        _kind: HandleKind,
        _id: u64,
        _name: &str,
    ) -> Result<Option<Value>, String> {
        Ok(None)
    }

    fn set_property(
        &mut self,
        _kind: HandleKind,
        _id: u64,
        _name: &str,
        _value: Value,
    ) -> Result<bool, String> {
        Ok(false)
    }

    fn get_script_property(
        &self,
        _kind: HandleKind,
        _id: u64,
        _name: &str,
    ) -> Option<crate::scripting::value::DynamicProperty> {
        None
    }

    fn set_script_property(
        &mut self,
        _kind: HandleKind,
        _id: u64,
        _name: String,
        _value: Value,
        _owner_script: Option<String>,
    ) {
    }

    fn call_method(
        &mut self,
        _kind: HandleKind,
        _id: u64,
        _name: &str,
        _args: &[Value],
    ) -> Result<Option<Value>, String> {
        Ok(None)
    }

    fn set_attribute(&mut self, _id: u64, _key: String, _value: Value) -> Result<(), String> {
        Ok(())
    }

    fn remove_attribute(&mut self, _id: u64, _key: &str) -> Result<(), String> {
        Ok(())
    }

    fn cell_exists(&self, _id: u64) -> bool {
        false
    }

    fn entity_exists(&self, id: u64) -> bool {
        EntityManager::validate_handle(self, id)
    }

    fn create_runtime_cell(&mut self, _cell_type: &str) -> Result<(HandleKind, u64), String> {
        Err("Runtime cell creation not supported in this host".to_string())
    }

    fn move_runtime_cell(&mut self, _id: u64, _x: i32, _y: i32, _z: i32) -> Result<(), String> {
        Err("Runtime cell movement not supported in this host".to_string())
    }

    fn delete_cell(&mut self, _id: u64) -> Result<(), String> {
        Err("Cell deletion not supported in this host".to_string())
    }
}

/// Context provided by the engine when executing AeoScript host operations.
pub struct HostContext<'a> {
    pub delta_time: f64,
    pub engine: &'a mut dyn EngineHost,
}

/// Dispatches a global host function call.
pub fn call_host_function(
    context: &mut HostContext,
    name: &str,
    arguments: &[Value],
) -> Result<Option<Value>, String> {
    match name {
        "get_entity" => {
            if arguments.len() != 1 {
                return Err("get_entity expects exactly 1 argument (name)".to_string());
            }

            let entity_name = arguments[0].as_string()?;

            if let Some(id) = context.engine.entity_manager().lookup_entity(entity_name) {
                Ok(Some(Value::Handle {
                    kind: HandleKind::Entity,
                    id: id.0,
                }))
            } else {
                Ok(Some(Value::Nil))
            }
        }

        "spawn" => {
            if arguments.len() != 2 && arguments.len() != 3 {
                return Err(
                    "spawn expects 2 or 3 arguments: (entity_name, position [, character_package])"
                        .to_string(),
                );
            }

            let entity_name = arguments[0].as_string()?;
            let position_basket = arguments[1].as_basket()?;
            let borrowed = position_basket.borrow();

            if borrowed.elements.len() != 3 {
                return Err("position must be a basket of 3 numbers [x, y, z]".to_string());
            }

            let x = borrowed.elements[0].as_number()? as f32;
            let y = borrowed.elements[1].as_number()? as f32;
            let z = borrowed.elements[2].as_number()? as f32;
            let position = Vec3::new(x, y, z);

            let package_name = if arguments.len() == 3 {
                Some(arguments[2].as_string()?)
            } else {
                None
            };

            let entity_id = if let Some(package_name) = package_name.as_deref() {
                context
                    .engine
                    .spawn_entity(entity_name, position, Some(package_name))?
            } else {
                context
                    .engine
                    .spawn_character(entity_name, position, None)?
            };

            Ok(Some(Value::Handle {
                kind: HandleKind::Entity,
                id: entity_id,
            }))
        }

        "attach" => {
            if arguments.len() != 4 {
                return Err(
                    "attach expects 4 arguments: (child, child_point, parent, parent_point)"
                        .to_string(),
                );
            }
            let (child_kind, child_id) = arguments[0].as_handle()?;
            let child_point = arguments[1].as_string()?;
            let (parent_kind, parent_id) = arguments[2].as_handle()?;
            let parent_point = arguments[3].as_string()?;

            context.engine.attach_entity(
                child_kind,
                child_id,
                child_point,
                parent_kind,
                parent_id,
                parent_point,
            )?;
            Ok(Some(Value::Bool(true)))
        }

        "detach" => {
            if arguments.len() != 1 {
                return Err("detach expects exactly 1 argument (child)".to_string());
            }
            let (child_kind, child_id) = arguments[0].as_handle()?;
            Ok(Some(Value::Bool(
                context.engine.detach_entity(child_kind, child_id)?,
            )))
        }

        "get_light" => {
            if arguments.len() != 3 {
                return Err("get_light expects exactly 3 arguments (x, y, z)".to_string());
            }

            let x = arguments[0].as_number()? as i32;
            let y = arguments[1].as_number()? as i32;
            let z = arguments[2].as_number()? as i32;

            if let Some(id) = context.engine.lookup_light(x, y, z) {
                Ok(Some(Value::Handle {
                    kind: HandleKind::Light,
                    id,
                }))
            } else {
                Ok(Some(Value::Nil))
            }
        }

        "getAllCellsOfClass" => {
            if arguments.len() != 1 {
                return Err(
                    "getAllCellsOfClass expects exactly 1 argument (class_name)".to_string()
                );
            }

            let class_name = arguments[0].as_string()?;
            let cells = context.engine.get_all_cells_of_class(class_name);

            let handles = cells
                .into_iter()
                .map(|id| Value::Handle {
                    kind: HandleKind::Cell,
                    id,
                })
                .collect();
            Ok(Some(Value::array(handles)))
        }

        "find" => {
            if arguments.len() != 1 {
                return Err("find expects exactly 1 argument (query)".to_string());
            }

            let query = arguments[0].as_string()?;
            let objects = context.engine.find_objects(query);
            let handles = objects
                .into_iter()
                .map(|(kind, id)| Value::Handle { kind, id })
                .collect();
            Ok(Some(Value::array(handles)))
        }

        "enable_script" => {
            if arguments.len() != 1 {
                return Err("enable_script expects 1 argument (path)".to_string());
            }
            let path = arguments[0].as_string()?;
            context.engine.enable_script(path);
            Ok(Some(Value::Nil))
        }

        "disable_script" => {
            if arguments.len() != 1 {
                return Err("disable_script expects 1 argument (path)".to_string());
            }
            let path = arguments[0].as_string()?;
            context.engine.disable_script(path);
            Ok(Some(Value::Nil))
        }

        "is_script_enabled" => {
            if arguments.len() != 1 {
                return Err("is_script_enabled expects 1 argument (path)".to_string());
            }
            let path = arguments[0].as_string()?;
            let enabled = context.engine.is_script_enabled(path);
            Ok(Some(Value::Bool(enabled)))
        }

        "fire_event" => {
            if arguments.is_empty() {
                return Err("fire_event expects at least 1 argument (event_name)".to_string());
            }
            let event_name = arguments[0].as_string()?;
            let event_args = arguments[1..].to_vec();
            context.engine.fire_event(event_name, event_args);
            Ok(Some(Value::Nil))
        }

        "complete_test" => {
            if arguments.len() != 2 {
                return Err("complete_test expects 2 arguments (test_name, passed)".to_string());
            }
            let test_name = arguments[0].as_string()?;
            let passed = arguments[1].as_bool()?;
            context.engine.complete_test(test_name, passed);
            Ok(Some(Value::Nil))
        }

        "is_test_completed" => {
            if arguments.len() != 1 {
                return Err("is_test_completed expects 1 argument (test_name)".to_string());
            }
            let test_name = arguments[0].as_string()?;
            if let Some(_passed) = context.engine.is_test_completed(test_name) {
                Ok(Some(Value::Bool(true)))
            } else {
                Ok(Some(Value::Bool(false)))
            }
        }

        "test_passed" => {
            if arguments.len() != 1 {
                return Err("test_passed expects 1 argument (test_name)".to_string());
            }
            let test_name = arguments[0].as_string()?;
            if let Some(passed) = context.engine.is_test_completed(test_name) {
                Ok(Some(Value::Bool(passed)))
            } else {
                Ok(Some(Value::Nil))
            }
        }

        "test_summary" => {
            let (p, f, t) = context.engine.get_test_results();
            Ok(Some(Value::array(vec![
                Value::Number(p as f64),
                Value::Number(f as f64),
                Value::Number(t as f64),
            ])))
        }

        _ => Ok(None),
    }
}

/// Resolves a property on a host object (e.g. time.delta).
pub fn resolve_host_property(
    context: &mut HostContext,
    object_name: &str,
    property_name: &str,
) -> Result<Option<Value>, String> {
    match object_name {
        "time" => {
            if property_name == "delta" {
                return Ok(Some(Value::Number(context.delta_time)));
            }
        }

        _ => {}
    }

    Ok(None)
}

/// Resolves a property on an engine handle (e.g. entity.position).
pub fn resolve_host_member_property(
    context: &mut HostContext,
    handle_kind: HandleKind,
    handle_id: u64,
    property_name: &str,
) -> Result<Option<Value>, String> {
    if handle_kind == HandleKind::Ui {
        return context.engine.get_ui_property(handle_id, property_name);
    }

    if let Some(value) = context
        .engine
        .get_property(handle_kind, handle_id, property_name)?
    {
        return Ok(Some(value));
    }

    if let Some(prop) = context
        .engine
        .get_script_property(handle_kind, handle_id, property_name)
    {
        return Ok(Some(prop.value));
    }

    match handle_kind {
        HandleKind::Entity => match property_name {
            "id" => Ok(Some(Value::Number(handle_id as f64))),
            "visible" => Ok(Some(Value::Bool(
                context
                    .engine
                    .entity_manager()
                    .is_visible(EntityId(handle_id))
                    .unwrap_or(false),
            ))),
            "enabled" => Ok(Some(Value::Bool(
                context
                    .engine
                    .entity_manager()
                    .is_enabled(EntityId(handle_id))
                    .unwrap_or(false),
            ))),
            "name" => {
                if let Some(name) = context
                    .engine
                    .entity_manager()
                    .get_name(EntityId(handle_id))
                {
                    Ok(Some(Value::String(name.to_string())))
                } else {
                    Ok(Some(Value::Nil))
                }
            }
            "position" => {
                if let Some(pos) = context.engine.get_position(handle_id) {
                    Ok(Some(Value::array(vec![
                        Value::Number(pos.x as f64),
                        Value::Number(pos.y as f64),
                        Value::Number(pos.z as f64),
                    ])))
                } else {
                    Ok(Some(Value::Nil))
                }
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

/// Sets a property on an engine handle (e.g. entity.position = ...).
pub fn set_host_member_property(
    context: &mut HostContext,
    handle_kind: HandleKind,
    handle_id: u64,
    property_name: &str,
    value: Value,
    owner_script: Option<String>,
) -> Result<(), String> {
    if handle_kind == HandleKind::Ui {
        if context
            .engine
            .set_ui_property(handle_id, property_name, value)?
        {
            return Ok(());
        } else {
            return Err(format!("failed to set UI property '{}'", property_name));
        }
    }

    if context
        .engine
        .set_property(handle_kind, handle_id, property_name, value.clone())?
    {
        Ok(())
    } else {
        context.engine.set_script_property(
            handle_kind,
            handle_id,
            property_name.to_string(),
            value,
            owner_script,
        );
        Ok(())
    }
}

/// Dispatches a member function call on an engine handle.
pub fn call_host_member(
    context: &mut HostContext,
    handle_kind: HandleKind,
    handle_id: u64,
    name: &str,
    arguments: &[Value],
) -> Result<Option<Value>, String> {
    if let Some(result) = context
        .engine
        .call_method(handle_kind, handle_id, name, arguments)?
    {
        return Ok(Some(result));
    }

    match name {
        "get_children" => {
            let children = context.engine.get_children(handle_kind, handle_id);
            let handles: Vec<Value> = children
                .into_iter()
                .map(|(kind, id)| Value::Handle { kind, id })
                .collect();
            return Ok(Some(Value::array(handles)));
        }
        "get_parent" => {
            if let Some((kind, id)) = context.engine.get_parent(handle_kind, handle_id) {
                return Ok(Some(Value::Handle { kind, id }));
            } else {
                return Ok(Some(Value::Nil));
            }
        }
        _ => {}
    }

    match handle_kind {
        HandleKind::Cell | HandleKind::Light => match name {
            "getObject" => {
                if !arguments.is_empty() {
                    return Err(format!(
                        "{}.getObject() expects 0 arguments",
                        handle_kind.name()
                    ));
                }
                if let Some((kind, id)) = context.engine.get_cell_object(handle_id) {
                    Ok(Some(Value::Handle { kind, id }))
                } else {
                    Ok(Some(Value::Nil))
                }
            }

            "is_enabled" => {
                if !arguments.is_empty() {
                    return Err(format!(
                        "{}.is_enabled() expects 0 arguments",
                        handle_kind.name()
                    ));
                }

                if let Some(enabled) = context.engine.is_light_enabled(handle_id) {
                    Ok(Some(Value::Bool(enabled)))
                } else {
                    Ok(Some(Value::Nil))
                }
            }

            "set_enabled" => {
                if arguments.len() != 1 {
                    return Err(format!(
                        "{}.set_enabled() expects 1 argument (bool)",
                        handle_kind.name()
                    ));
                }

                let enabled = arguments[0].as_bool()?;
                context.engine.set_light_enabled(handle_id, enabled);

                Ok(Some(Value::Nil))
            }

            _ => Ok(None),
        },

        HandleKind::Entity => match name {
            "is_valid" => {
                if !arguments.is_empty() {
                    return Err("entity.is_valid() expects 0 arguments".to_string());
                }

                Ok(Some(Value::Bool(
                    context.engine.entity_manager().validate_handle(handle_id),
                )))
            }
            // ... (keep existing name, set_position, translate)
            "name" => {
                if !arguments.is_empty() {
                    return Err("entity.name() expects 0 arguments".to_string());
                }

                if let Some(entity_name) = context
                    .engine
                    .entity_manager()
                    .get_name(EntityId(handle_id))
                {
                    Ok(Some(Value::String(entity_name.to_string())))
                } else {
                    Err("entity.name() called on an invalid handle".to_string())
                }
            }

            "set_position" => {
                if arguments.len() != 3 {
                    return Err("entity.set_position() expects 3 arguments (x, y, z)".to_string());
                }

                let x = arguments[0].as_number()? as f32;
                let y = arguments[1].as_number()? as f32;
                let z = arguments[2].as_number()? as f32;

                if context.engine.entity_manager().validate_handle(handle_id) {
                    context.engine.set_position(handle_id, Vec3::new(x, y, z));
                }

                Ok(Some(Value::Nil))
            }

            "translate" => {
                if arguments.len() != 3 {
                    return Err("entity.translate() expects 3 arguments (x, y, z)".to_string());
                }

                let dx = arguments[0].as_number()? as f32;
                let dy = arguments[1].as_number()? as f32;
                let dz = arguments[2].as_number()? as f32;

                if let Some(pos) = context.engine.get_position(handle_id) {
                    let new_pos = pos + Vec3::new(dx, dy, dz);
                    context.engine.set_position(handle_id, new_pos);
                }

                Ok(Some(Value::Nil))
            }

            _ => Ok(None),
        },
        HandleKind::EntityPhysics | HandleKind::EntityAppearance | HandleKind::EntityAnimation => {
            Ok(None)
        }

        HandleKind::Actor | HandleKind::Ui | HandleKind::Sound | HandleKind::Mouse => Ok(None),
    }
}
