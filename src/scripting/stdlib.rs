// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::scripting::api::HostContext;
use crate::scripting::interpreter::Interpreter;
use crate::scripting::value::{HandleKind, MapKey, Value};

const MAX_SCRIPT_BASKET_ELEMENTS: usize = 1_000_000;

fn ray_vec3(value: &Value, label: &str) -> Result<glam::Vec3, String> {
    let basket = value.as_basket()?;
    let borrowed = basket.borrow();
    if borrowed.elements.len() != 3 {
        return Err(format!("{label} must be a 3-number basket"));
    }
    let mut coordinates = [0.0_f32; 3];
    for (i, value) in borrowed.elements.iter().enumerate() {
        let n = value.as_number()?;
        if !n.is_finite() || n.abs() > 1.0e7 {
            return Err(format!("{label} must have finite coordinates"));
        }
        coordinates[i] = n as f32;
    }
    Ok(glam::Vec3::from_array(coordinates))
}

fn ray_query_options(
    ray: &mut crate::engine::physics::raycast::RayQuery,
    options: Option<&Value>,
) -> Result<(), String> {
    use crate::engine::physics::raycast::MAX_QUERY_HITS;
    let Some(options) = options else {
        return Ok(());
    };
    let map = options.as_map()?;
    for (key, value) in map.borrow().iter() {
        let MapKey::String(name) = key else {
            return Err("ray query option names must be strings".into());
        };
        match name.as_str() {
            "actors" => ray.actors = value.as_bool()?,
            "cells" => ray.cells = value.as_bool()?,
            "entities" => ray.entities = value.as_bool()?,
            "terrain" => ray.terrain = value.as_bool()?,
            "max_hits" => {
                let count = value.as_number()?;
                if !count.is_finite()
                    || count.fract() != 0.0
                    || count < 1.0
                    || count > MAX_QUERY_HITS as f64
                {
                    return Err(format!(
                        "max_hits must be an integer from 1 to {MAX_QUERY_HITS}"
                    ));
                }
                ray.max_hits = count as usize;
            }
            "ignore" => {
                let basket = value.as_basket()?;
                for element in &basket.borrow().elements {
                    let (kind, id) = element.as_handle()?;
                    ray.ignore.insert((kind, id));
                }
            }
            _ => return Err(format!("unknown ray query option '{name}'")),
        }
    }
    Ok(())
}

fn ray_hit_value(hit: crate::engine::physics::raycast::RayHit) -> Value {
    let vec = |v: glam::Vec3| {
        Value::array(
            v.to_array()
                .into_iter()
                .map(|n| Value::Number(n as f64))
                .collect(),
        )
    };
    let key = |name: &str| MapKey::String(name.to_string());
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(key("kind"), Value::String(hit.kind.to_string()));
    fields.insert(key("position"), vec(hit.position));
    fields.insert(key("normal"), vec(hit.normal));
    fields.insert(key("distance"), Value::Number(hit.distance as f64));
    fields.insert(
        key("object"),
        hit.handle
            .map_or(Value::Nil, |(kind, id)| Value::Handle { kind, id }),
    );
    Value::map(fields)
}

fn spring_arm_vector(input: &Value, size: usize, label: &str) -> Result<Vec<f32>, String> {
    let basket = input
        .as_basket()
        .map_err(|_| format!("camera.spring_arm {label} must be a {size}-number basket"))?;
    let borrowed = basket.borrow();
    if borrowed.elements.len() != size {
        return Err(format!(
            "camera.spring_arm {label} must be a {size}-number basket"
        ));
    }
    borrowed
        .elements
        .iter()
        .map(|v| {
            let n = v.as_number()?;
            if !n.is_finite() || n.abs() > 1.0e7 {
                return Err(format!(
                    "camera.spring_arm {label} must contain finite numbers"
                ));
            }
            Ok(n as f32)
        })
        .collect()
}
fn spring_arm_settings(
    args: &[Value],
) -> Result<crate::renderer::camera_spring_arm::SpringArmInput, String> {
    use crate::renderer::camera_spring_arm::SpringArmInput;
    if args.len() != 1 {
        return Err("camera.spring_arm expects one settings map".into());
    }
    let options = args[0].as_map()?.borrow().clone();
    let get = |name: &str| options.get(&MapKey::String(name.into()));
    let valid = [
        "target",
        "offset",
        "dt",
        "follow_speed",
        "rotation_speed",
        "recovery_speed",
        "look_ahead",
        "dead_zone",
        "collision",
        "collision_padding",
        "reset",
    ];
    for key in options.keys() {
        let MapKey::String(s) = key else {
            return Err("camera.spring_arm settings keys must be strings".into());
        };
        if !valid.contains(&s.as_str()) {
            return Err(format!("camera.spring_arm unknown setting '{s}'"));
        }
    }
    let mut input = SpringArmInput::default();
    let target = get("target").ok_or("camera.spring_arm needs target")?;
    let offset = get("offset").ok_or("camera.spring_arm needs offset")?;
    let dt = get("dt").ok_or("camera.spring_arm needs dt")?;
    input.target = glam::Vec3::from_slice(&spring_arm_vector(target, 3, "target")?);
    input.offset = glam::Vec3::from_slice(&spring_arm_vector(offset, 3, "offset")?);
    input.delta_time = dt.as_number()? as f32;
    if let Some(value) = get("follow_speed") {
        input.follow_speed = value.as_number()? as f32;
    }
    if let Some(value) = get("rotation_speed") {
        input.rotation_speed = value.as_number()? as f32;
    }
    if let Some(value) = get("recovery_speed") {
        input.recovery_speed = value.as_number()? as f32;
    }
    if let Some(value) = get("look_ahead") {
        input.look_ahead = glam::Vec3::from_slice(&spring_arm_vector(value, 3, "look_ahead")?);
    }
    if let Some(value) = get("dead_zone") {
        let dz = spring_arm_vector(value, 2, "dead_zone")?;
        input.dead_zone = [dz[0], dz[1]];
    }
    for (key, flag) in [
        ("collision", &mut input.collision),
        ("reset", &mut input.reset),
    ] {
        if let Some(v) = get(key) {
            *flag = if let Value::Bool(b) = v {
                *b
            } else {
                return Err(format!("camera.spring_arm {key} must be true or false"));
            };
        }
    }
    if let Some(value) = get("collision_padding") {
        input.collision_padding = value.as_number()? as f32;
    }
    Ok(input)
}

fn tag_name(argument: &Value) -> Result<&str, String> {
    let tag = argument
        .as_string()
        .map_err(|_| "tags: tag must be a string".to_string())?;
    if tag.trim() != tag || tag.is_empty() || tag.len() > 64 || tag.chars().any(char::is_control) {
        return Err(
            "tags: tag must be 1..64 characters without surrounding whitespace or controls".into(),
        );
    }
    Ok(tag)
}

fn terrain_vector(argument: &Value, name: &str) -> Result<[i32; 3], String> {
    let basket = argument
        .as_basket()
        .map_err(|_| format!("terrain.generate {name} must be a 3-number array"))?;
    let values = basket.borrow();
    if values.elements.len() != 3 {
        return Err(format!("terrain.generate {name} must be a 3-number array"));
    }

    let mut result = [0; 3];
    for (index, value) in values.elements.iter().enumerate() {
        let number = value
            .as_number()
            .map_err(|_| format!("terrain.generate {name} must be a 3-number array"))?;
        if !number.is_finite()
            || number.fract() != 0.0
            || number < i32::MIN as f64
            || number > i32::MAX as f64
        {
            return Err(format!(
                "terrain.generate {name}[{index}] must be a finite whole number in the 32-bit integer range"
            ));
        }
        result[index] = number as i32;
    }
    Ok(result)
}

fn terrain_set_voxels_origin(argument: &Value) -> Result<[i32; 3], String> {
    let basket = argument
        .as_basket()
        .map_err(|_| "terrain.set_voxels origin must be a 3-number array".to_string())?;
    let values = basket.borrow();
    if values.elements.len() != 3 {
        return Err("terrain.set_voxels origin must be a 3-number array".to_string());
    }
    let mut result = [0_i32; 3];
    for (index, value) in values.elements.iter().enumerate() {
        let number = value
            .as_number()
            .map_err(|_| "terrain.set_voxels origin must be a 3-number array".to_string())?;
        if !number.is_finite()
            || number.fract() != 0.0
            || number < i32::MIN as f64
            || number > i32::MAX as f64
        {
            return Err(format!(
                "terrain.set_voxels origin[{index}] must be a finite whole number in the 32-bit integer range"
            ));
        }
        result[index] = number as i32;
    }
    Ok(result)
}

fn terrain_voxel_size(argument: &Value) -> Result<[u32; 3], String> {
    let basket = argument
        .as_basket()
        .map_err(|_| "terrain.set_voxels size must be a 3-number array".to_string())?;
    let values = basket.borrow();
    if values.elements.len() != 3 {
        return Err("terrain.set_voxels size must be a 3-number array".to_string());
    }
    let mut result = [0_u32; 3];
    for (index, value) in values.elements.iter().enumerate() {
        let number = value
            .as_number()
            .map_err(|_| "terrain.set_voxels size must be a 3-number array".to_string())?;
        if !number.is_finite() || number.fract() != 0.0 || number <= 0.0 || number > u32::MAX as f64
        {
            return Err(format!(
                "terrain.set_voxels size[{index}] must be a positive finite whole number"
            ));
        }
        result[index] = number as u32;
    }
    Ok(result)
}

fn terrain_material_palette(argument: &Value) -> Result<Vec<String>, String> {
    let basket = argument
        .as_basket()
        .map_err(|_| "terrain.set_voxels materials must be a basket of strings".to_string())?;
    let values = basket.borrow();
    if values.elements.is_empty() || values.elements.len() > u16::MAX as usize + 1 {
        return Err(format!(
            "terrain.set_voxels materials must contain 1..={} entries",
            u16::MAX as usize + 1
        ));
    }
    values
        .elements
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_string()
                .map(str::to_string)
                .map_err(|_| format!("terrain.set_voxels materials[{index}] must be a string"))
        })
        .collect()
}

fn terrain_voxel_values(
    argument: &Value,
    size: [u32; 3],
    palette_len: usize,
) -> Result<Vec<(u16, f32)>, String> {
    let expected = (size[0] as u64)
        .checked_mul(size[1] as u64)
        .and_then(|value| value.checked_mul(size[2] as u64))
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "terrain.set_voxels voxel count overflowed".to_string())?;
    let basket = argument
        .as_basket()
        .map_err(|_| "terrain.set_voxels voxels must be a flat basket".to_string())?;
    let values = basket.borrow();
    if values.elements.len() != expected {
        return Err(format!(
            "terrain.set_voxels expected {expected} voxel entries but received {}",
            values.elements.len()
        ));
    }

    let mut result = Vec::with_capacity(expected);
    for (index, value) in values.elements.iter().enumerate() {
        let pair = value.as_basket().map_err(|_| {
            format!("terrain.set_voxels voxels[{index}] must be [material_index, occupancy]")
        })?;
        let pair = pair.borrow();
        if pair.elements.len() != 2 {
            return Err(format!(
                "terrain.set_voxels voxels[{index}] must be [material_index, occupancy]"
            ));
        }
        let material = pair.elements[0].as_number().map_err(|_| {
            format!("terrain.set_voxels voxels[{index}][0] must be a material index")
        })?;
        if !material.is_finite()
            || material.fract() != 0.0
            || material < 0.0
            || material >= palette_len as f64
            || material > u16::MAX as f64
        {
            return Err(format!(
                "terrain.set_voxels voxels[{index}][0] is outside the material palette"
            ));
        }
        let occupancy = pair.elements[1]
            .as_number()
            .map_err(|_| format!("terrain.set_voxels voxels[{index}][1] must be occupancy 0..1"))?;
        if !occupancy.is_finite() || !(0.0..=1.0).contains(&occupancy) {
            return Err(format!(
                "terrain.set_voxels voxels[{index}][1] must be occupancy 0..1"
            ));
        }
        result.push((material as u16, occupancy as f32));
    }
    Ok(result)
}

pub(crate) fn material_layer_namespace(value: &str) -> Option<(&str, &str)> {
    value.strip_prefix("material-layer:")?.split_once('/')
}

pub fn resolve_stdlib_property(
    object_name: &str,
    property_name: &str,
    host: &mut HostContext,
) -> Result<Option<Value>, String> {
    if object_name == "game" {
        return Ok(host.engine.game_property(property_name));
    }
    if object_name == "proj" && property_name == "sky" {
        return Ok(Some(Value::Namespace("world-sky".into())));
    }
    if object_name == "world-sky" && property_name == "material" {
        return Ok(Some(
            host.engine
                .sky_material_id()
                .map(|id| Value::Namespace(format!("material:{id}")))
                .unwrap_or(Value::Nil),
        ));
    }
    if object_name == "engine" && property_name == "tweens" {
        return Ok(Some(Value::Namespace("engine/tweens".into())));
    }
    if let Some((material, layer)) = material_layer_namespace(object_name) {
        if property_name == "alpha" {
            return Ok(Some(
                host.engine
                    .material_layer_alpha(material, layer)?
                    .map(Value::Number)
                    .unwrap_or(Value::Nil),
            ));
        }
    }
    if object_name == "player" && property_name == "position" {
        if let Some(pos) = host.engine.get_player_position() {
            return Ok(Some(Value::array(vec![
                Value::Number(pos[0] as f64),
                Value::Number(pos[1] as f64),
                Value::Number(pos[2] as f64),
            ])));
        }
    }
    Ok(None)
}

pub fn call_stdlib_function(
    _interpreter: &mut Interpreter,
    _instance: &mut crate::scripting::interpreter::ScriptInstance,
    _scopes: &mut Vec<crate::scripting::value::Scope>,
    object_name: &str,
    method_name: &str,
    args: &[Value],
    _host: &mut HostContext,
) -> Result<Option<Value>, String> {
    #[cfg(feature = "kilngraph")]
    let _kg_host = if crate::kilngraph::enabled() {
        Some(crate::kilngraph::script_scope(
            _instance.script_path.as_deref().unwrap_or("<runtime>"),
            &format!("{}.{}", object_name, method_name),
            _instance.parent_actor_id().unwrap_or(0),
            101,
            0,
        ))
    } else {
        None
    };

    match object_name {
        "game" => {
            if !args.is_empty() {
                return Err(format!("game.{method_name} expects no arguments"));
            }
            match method_name {
                "quit" | "restart" | "pause" | "resume" => {
                    _host.engine.game_command(method_name)?;
                    Ok(Some(Value::Nil))
                }
                _ => Err(format!("unknown function 'game.{method_name}'")),
            }
        }

        "postprocess" => match method_name {
            "passes" => {
                if !args.is_empty() {
                    return Err("postprocess.passes expects no arguments".into());
                }
                let ids = _host.engine.post_process_pass_ids()?;
                Ok(Some(Value::array(
                    ids.into_iter().map(Value::String).collect(),
                )))
            }
            "set_parameter" => {
                if args.len() != 3 {
                    return Err("postprocess.set_parameter expects (pass_id, name, value)".into());
                }
                let pass_id = args[0].as_string()?;
                let name = args[1].as_string()?;
                use crate::project::material::ParameterValue;
                let value = match &args[2] {
                    Value::Number(v) if v.is_finite() => ParameterValue::Scalar(*v as f32),
                    Value::Bool(v) => ParameterValue::Boolean(*v),
                    Value::Array(items) => {
                        let values = items
                            .borrow()
                            .elements
                            .iter()
                            .map(Value::as_number)
                            .collect::<Result<Vec<_>, _>>()?;
                        if values.iter().any(|v| !v.is_finite()) {
                            return Err("postprocess parameters must be finite".into());
                        }
                        match values.as_slice() {
                            [a, b] => ParameterValue::Vec2([*a as f32, *b as f32]),
                            [a, b, c] => ParameterValue::Color3([*a as f32, *b as f32, *c as f32]),
                            [a, b, c, d] => {
                                ParameterValue::Color4([*a as f32, *b as f32, *c as f32, *d as f32])
                            }
                            _ => return Err("postprocess vector must contain 2–4 numbers".into()),
                        }
                    }
                    _ => {
                        return Err(
                            "postprocess parameter expects number, bool, or numeric vector".into(),
                        );
                    }
                };
                _host
                    .engine
                    .post_process_set_parameter(pass_id, name, value)?;
                Ok(Some(Value::Nil))
            }
            "enable" => {
                if args.len() != 2 {
                    return Err("postprocess.enable expects (pass_id, enabled)".into());
                }
                let Value::Bool(enabled) = &args[1] else {
                    return Err("postprocess.enable expects a boolean".into());
                };
                _host
                    .engine
                    .post_process_set_enabled(args[0].as_string()?, *enabled)?;
                Ok(Some(Value::Nil))
            }
            "object_mask" => {
                use crate::project::post_process_pipeline::{
                    PostProcessObject, PostProcessObjectKind,
                };
                if args.len() != 3 {
                    return Err("postprocess.object_mask expects (kind, id, channel)".into());
                }
                let kind = match args[0].as_string()? {
                    "Actor" | "actor" => PostProcessObjectKind::Actor,
                    "Cell" | "cell" => PostProcessObjectKind::Cell,
                    "Character" | "character" => PostProcessObjectKind::Character,
                    _ => {
                        return Err(
                            "postprocess.object_mask kind must be Actor, Cell, or Character".into(),
                        );
                    }
                };
                let id = args[1].as_number()?;
                if !id.is_finite() || id < 0.0 || id > 9007199254740991.0 || id.fract() != 0.0 {
                    return Err("postprocess object id must be a nonnegative exact integer".into());
                }
                let channel = args[2].as_number()?;
                if !channel.is_finite() || !(0.0..=8.0).contains(&channel) || channel.fract() != 0.0
                {
                    return Err(
                        "postprocess object channel must be an integer between 0 and 8".into(),
                    );
                }
                _host.engine.post_process_set_object_channel(
                    PostProcessObject {
                        kind,
                        id: id as u64,
                    },
                    channel as u8,
                )?;
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("Unknown postprocess function: {method_name}")),
        },
        "task" => match method_name {
            "spawn" | "defer" | "delay" => {
                let callable_at = if method_name == "delay" { 1 } else { 0 };
                if args.len() <= callable_at {
                    return Err(format!("task.{method_name} requires a callback function"));
                }
                let delay = if method_name == "delay" {
                    let seconds = args[0].as_number()?;
                    if !seconds.is_finite() || seconds < 0.0 {
                        return Err("task.delay seconds must be finite and non-negative".into());
                    }
                    seconds.max(f64::EPSILON)
                } else if method_name == "defer" {
                    f64::EPSILON
                } else {
                    0.0
                };
                let Value::Function(compiled, names, captures) = &args[callable_at] else {
                    return Err(format!("task.{method_name} callback must be a function"));
                };
                if _interpreter.pending_module_fibers.len() >= 4096 {
                    return Err("task scheduler pending-callback limit (4096) exceeded".into());
                }
                let fiber = _interpreter.start_direct_fiber(
                    _instance.clone(),
                    std::sync::Arc::clone(compiled),
                    names.clone(),
                    args[callable_at + 1..].to_vec(),
                    None,
                    format!("task.{method_name}"),
                    captures.clone(),
                )?;
                let state = if delay > 0.0 {
                    crate::scripting::execution::FiberResult::Yield(
                        crate::scripting::execution::YieldReason::WaitSeconds(delay),
                    )
                } else {
                    crate::scripting::execution::FiberResult::Continue
                };
                _interpreter.pending_module_fibers.push((fiber, state));
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("unknown function 'task.{method_name}'")),
        },
        "engine/tweens" => match method_name {
            "create" => {
                if args.len() != 1 {
                    return Err("engine.tweens.create expects one options map".into());
                }
                let map = args[0].as_map()?;
                let options = map.borrow();
                let get = |name: &str| {
                    options
                        .get(&MapKey::String(name.into()))
                        .cloned()
                        .ok_or_else(|| format!("engine.tweens.create requires {name}"))
                };
                let target = get("target")?;
                let property = get("property")?.as_string()?.to_string();
                let end = get("endValue")?;
                let typed = match end {
                    Value::Number(number) => crate::scripting::tween::TweenValue::Scalar(number),
                    Value::Array(basket) => {
                        let elements = basket.borrow();
                        let values = elements
                            .elements
                            .iter()
                            .map(Value::as_number)
                            .collect::<Result<Vec<_>, _>>()?;
                        match values.as_slice() {
                            [a, b] => crate::scripting::tween::TweenValue::Vec2([*a, *b]),
                            [a, b, c] => crate::scripting::tween::TweenValue::Vec3([*a, *b, *c]),
                            [a, b, c, d] => {
                                crate::scripting::tween::TweenValue::Vec4([*a, *b, *c, *d])
                            }
                            _ => return Err("Tween vector must have 2, 3 or 4 numbers".into()),
                        }
                    }
                    _ => return Err("Tween endValue must be a number or numeric vector".into()),
                };
                let duration = get("time")?.as_number()?;
                let easing = options
                    .get(&MapKey::String("easing".into()))
                    .map(|v| {
                        v.as_string()
                            .and_then(crate::scripting::tween::TweenEasing::parse)
                    })
                    .transpose()?
                    .unwrap_or(crate::scripting::tween::TweenEasing::Linear);
                match target {
                    Value::Handle { kind, id } => _host
                        .engine
                        .tween_handle_create(kind, id, &property, typed, duration, easing)?,
                    Value::Namespace(namespace) => {
                        let (material, layer) = material_layer_namespace(&namespace)
                            .ok_or("Tween target must be a live Material layer or engine handle")?;
                        _host
                            .engine
                            .tween_create(material, layer, &property, typed, duration, easing)?;
                    }
                    _ => {
                        return Err(
                            "Tween target must be a live writable engine handle or Material layer"
                                .into(),
                        );
                    }
                }
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("Unknown tween function: {method_name}")),
        },
        namespace if namespace.starts_with("material:") => match method_name {
            "find_layer" => {
                if args.len() != 1 {
                    return Err("material.find_layer expects one layer name".into());
                }
                let material = namespace.strip_prefix("material:").unwrap_or("");
                let layer = args[0].as_string()?;
                let found = _host.engine.material_layer_alpha(material, layer)?;
                Ok(Some(
                    found
                        .map(|_| Value::Namespace(format!("material-layer:{material}/{layer}")))
                        .unwrap_or(Value::Nil),
                ))
            }
            _ => Err(format!("Unknown Material function: {method_name}")),
        },
        "terrain" => match method_name {
            "sample_surface" => {
                if args.len() != 2 {
                    return Err("terrain.sample_surface expects exactly two numbers (x, z)".into());
                }
                let mut coords = [0.0_f32; 2];
                for (index, arg) in args.iter().enumerate() {
                    let value = arg.as_number().map_err(|_| {
                        "terrain.sample_surface x and z must be finite numbers".to_string()
                    })?;
                    if !value.is_finite() || value.abs() > f32::MAX as f64 {
                        return Err("terrain.sample_surface x and z must be finite numbers within world range".into());
                    }
                    coords[index] = value as f32;
                }
                let Some(sample) = _host.engine.terrain_sample_surface(coords[0], coords[1])?
                else {
                    return Ok(Some(Value::Nil));
                };
                let mut data = std::collections::BTreeMap::new();
                let array = |values: [f32; 3]| {
                    Value::array(
                        values
                            .into_iter()
                            .map(|v| Value::Number(v as f64))
                            .collect(),
                    )
                };
                let key = |name: &str| crate::scripting::value::MapKey::String(name.into());
                data.insert(key("position"), array(sample.position));
                data.insert(key("normal"), array(sample.normal));
                data.insert(
                    key("slope_degrees"),
                    Value::Number(sample.slope_degrees as f64),
                );
                data.insert(
                    key("material_id"),
                    sample.material_id.map(Value::String).unwrap_or(Value::Nil),
                );
                data.insert(key("source"), Value::String(sample.source.to_owned()));
                Ok(Some(Value::map(data)))
            }
            "generate" => {
                if args.len() != 2 {
                    return Err(
                        "terrain.generate expects position and size 3-number arrays".to_string()
                    );
                }
                let position = terrain_vector(&args[0], "position")?;
                let size = terrain_vector(&args[1], "size")?;
                _host.engine.terrain_generate(position, size)?;
                Ok(Some(Value::Nil))
            }
            "set_voxels" => {
                if args.len() != 4 && args.len() != 5 {
                    return Err(
                        "terrain.set_voxels expects origin, size, materials, voxels, and optional spacing_studs"
                            .to_string(),
                    );
                }
                let origin = terrain_set_voxels_origin(&args[0])?;
                let size = terrain_voxel_size(&args[1])?;
                let materials = terrain_material_palette(&args[2])?;
                let voxels = terrain_voxel_values(&args[3], size, materials.len())?;
                let spacing_studs = if let Some(value) = args.get(4) {
                    let value = value.as_number().map_err(|_| {
                        "terrain.set_voxels spacing_studs must be a number".to_string()
                    })? as f32;
                    if !value.is_finite() || value <= 0.0 {
                        return Err(
                            "terrain.set_voxels spacing_studs must be finite and greater than zero"
                                .to_string(),
                        );
                    }
                    value
                } else {
                    crate::world::LEGACY_TERRAIN_SAMPLE_SPACING_STUDS
                };
                _host
                    .engine
                    .terrain_set_voxels(origin, size, spacing_studs, materials, voxels)?;
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("unknown function 'terrain.{}'", method_name)),
        },
        "observe" => match method_name {
            "property" => {
                if args.len() != 3 {
                    return Err("observe.property expects (handle, property_name, callback)".into());
                }
                let (kind, id) = args[0].as_handle().map_err(|_| {
                    "observe.property first argument must be a live engine handle".to_string()
                })?;
                let property = args[1]
                    .as_string()
                    .map_err(|_| "observe.property name must be a string".to_string())?;
                let owner = _instance
                    .script_path
                    .clone()
                    .or_else(|| _interpreter.current_script_path.clone());
                let token = _interpreter.observe_property(
                    kind,
                    id,
                    property,
                    args[2].clone(),
                    owner.as_deref(),
                    _host,
                )?;
                Ok(Some(Value::Number(token as f64)))
            }
            "cancel" => {
                if args.len() != 1 {
                    return Err("observe.cancel expects one subscription ID".into());
                }
                let id = args[0].as_number().map_err(|_| {
                    "observe.cancel expects a positive whole-number subscription ID".to_string()
                })?;
                if !id.is_finite()
                    || id.fract() != 0.0
                    || !(1.0..=(1u64 << 53) as f64).contains(&id)
                {
                    return Err(
                        "observe.cancel expects a positive whole-number subscription ID".into(),
                    );
                }
                Ok(Some(Value::Bool(_interpreter.cancel_observer(id as u64))))
            }
            _ => Err(format!("unknown function 'observe.{method_name}'")),
        },
        "tags" => match method_name {
            "find" => {
                if args.len() != 1 {
                    return Err("tags.find expects one string tag".into());
                }
                let tag = tag_name(&args[0])?;
                let ids = _host.engine.tags_find(tag)?;
                Ok(Some(Value::array(
                    ids.into_iter()
                        .map(|id| Value::Handle {
                            kind: HandleKind::Actor,
                            id,
                        })
                        .collect(),
                )))
            }
            "get" => {
                if args.len() != 1 {
                    return Err("tags.get expects one string tag".into());
                }
                let tag = tag_name(&args[0])?;
                let found = _host.engine.tags_get(tag)?;
                Ok(Some(Value::array(
                    found
                        .into_iter()
                        .map(|(kind, id)| Value::Handle { kind, id })
                        .collect(),
                )))
            }
            "has" | "add" | "remove" => {
                if args.len() != 2 {
                    return Err(format!("tags.{method_name} expects (Actor/Cell, tag)"));
                }
                let (kind, id) = args[0]
                    .as_handle()
                    .map_err(|_| format!("tags.{method_name} expects a live Actor/Cell handle"))?;
                if !matches!(
                    kind,
                    HandleKind::Actor | HandleKind::Cell | HandleKind::Light
                ) {
                    return Err(format!(
                        "tags.{method_name} requires an Actor or Cell handle"
                    ));
                }
                let tag = tag_name(&args[1])?;
                let result = if method_name == "has" {
                    _host.engine.tags_has(kind, id, tag)?
                } else {
                    _host.engine.tags_set(kind, id, tag, method_name == "add")?
                };
                Ok(Some(Value::Bool(result)))
            }
            _ => Err(format!("unknown function 'tags.{method_name}'")),
        },
        "particles" => crate::scripting::particle_api::call(method_name, args, _host),
        "get" => {
            let res = match method_name {
                "mouse" => {
                    if !args.is_empty() {
                        return Err("get.mouse expects no arguments".to_string());
                    }

                    Value::Handle {
                        kind: HandleKind::Mouse,
                        id: 0,
                    }
                }

                _ => return Ok(None),
            };

            return Ok(Some(res));
        }
        "input" => match method_name {
            "get_move_vector" => {
                let mv = _host.engine.get_input_move_vector();
                Ok(Some(Value::array(vec![
                    Value::Number(mv[0] as f64),
                    Value::Number(mv[1] as f64),
                ])))
            }
            "is_jump_pressed" => Ok(Some(Value::Bool(_host.engine.is_input_jump_pressed()))),
            "is_key_down" => {
                if args.len() != 1 {
                    return Err("input.is_key_down expects exactly 1 string".to_string());
                }

                let key = args[0].as_string()?;

                Ok(Some(Value::Bool(_host.engine.is_input_key_down(key))))
            }
            "get_orbit_delta" => {
                let orbit = _host.engine.get_input_orbit_delta();
                Ok(Some(Value::array(vec![
                    Value::Number(orbit[0] as f64),
                    Value::Number(orbit[1] as f64),
                ])))
            }
            _ => Err(format!("unknown function 'input.{}'", method_name)),
        },
        "camera" => match method_name {
            "spring_arm" => {
                let settings = spring_arm_settings(args)?;
                let output = _host.engine.spring_arm(settings)?;
                let vec3 = |v: glam::Vec3| {
                    Value::array(
                        v.to_array()
                            .into_iter()
                            .map(|component| Value::Number(component as f64))
                            .collect(),
                    )
                };
                let key = |name: &str| MapKey::String(name.to_owned());
                let mut result = std::collections::BTreeMap::new();
                result.insert(key("position"), vec3(output.position));
                result.insert(key("target"), vec3(output.target));
                result.insert(key("obstructed"), Value::Bool(output.obstructed));
                Ok(Some(Value::map(result)))
            }
            "get_script" => {
                if !args.is_empty() {
                    return Err("camera.get_script expects no arguments".to_string());
                }
                Ok(Some(Value::String(_host.engine.get_camera_script())))
            }
            "set_script" => {
                if args.len() != 1 {
                    return Err("camera.set_script expects one camera package name".to_string());
                }
                let name = args[0].as_string()?;
                _host.engine.set_camera_script(name)?;
                Ok(Some(Value::Nil))
            }
            "get_horizontal_basis" => {
                let (f, r) = _host.engine.get_camera_horizontal_basis();
                Ok(Some(Value::array(vec![
                    Value::array(vec![
                        Value::Number(f[0] as f64),
                        Value::Number(f[1] as f64),
                        Value::Number(f[2] as f64),
                    ]),
                    Value::array(vec![
                        Value::Number(r[0] as f64),
                        Value::Number(r[1] as f64),
                        Value::Number(r[2] as f64),
                    ]),
                ])))
            }
            "set_position" => {
                if args.len() == 3 {
                    let x = args[0].as_number()? as f32;
                    let y = args[1].as_number()? as f32;
                    let z = args[2].as_number()? as f32;
                    _host.engine.set_camera_position([x, y, z]);
                    Ok(Some(Value::Nil))
                } else if args.len() == 1 {
                    let basket = args[0].as_basket()?;
                    let borrowed = basket.borrow();
                    if borrowed.elements.len() == 3 {
                        let x = borrowed.elements[0].as_number()? as f32;
                        let y = borrowed.elements[1].as_number()? as f32;
                        let z = borrowed.elements[2].as_number()? as f32;
                        _host.engine.set_camera_position([x, y, z]);
                        return Ok(Some(Value::Nil));
                    }
                    Err("camera.set_position expects 3 numbers or a 3D array".to_string())
                } else {
                    Err("camera.set_position expects 3 numbers or a 3D array".to_string())
                }
            }
            "set_target" => {
                if args.len() == 3 {
                    let x = args[0].as_number()? as f32;
                    let y = args[1].as_number()? as f32;
                    let z = args[2].as_number()? as f32;
                    _host.engine.set_camera_target([x, y, z]);
                    Ok(Some(Value::Nil))
                } else if args.len() == 1 {
                    let basket = args[0].as_basket()?;
                    let borrowed = basket.borrow();
                    if borrowed.elements.len() == 3 {
                        let x = borrowed.elements[0].as_number()? as f32;
                        let y = borrowed.elements[1].as_number()? as f32;
                        let z = borrowed.elements[2].as_number()? as f32;
                        _host.engine.set_camera_target([x, y, z]);
                        return Ok(Some(Value::Nil));
                    }
                    Err("camera.set_target expects 3 numbers or a 3D array".to_string())
                } else {
                    Err("camera.set_target expects 3 numbers or a 3D array".to_string())
                }
            }
            "set_orientation" => {
                if args.len() != 2 {
                    return Err("camera.set_orientation expects 2 numbers (yaw, pitch)".to_string());
                }
                let yaw = args[0].as_number()? as f32;
                let pitch = args[1].as_number()? as f32;
                _host.engine.set_camera_orientation(yaw, pitch);
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("unknown function 'camera.{}'", method_name)),
        },
        "physics" => match method_name {
            "raycast" | "raycast_all" | "linecast" | "linecast_all" | "line_of_sight" => {
                use crate::engine::physics::raycast::RayQuery;
                let is_segment = method_name.starts_with("line");
                let expected = if is_segment { 2 } else { 3 };
                if args.len() < expected || args.len() > expected + 1 {
                    return Err(format!(
                        "physics.{method_name} expects {expected} arguments and an optional options map"
                    ));
                }
                let origin = ray_vec3(&args[0], "ray origin")?;
                let other = ray_vec3(
                    &args[1],
                    if is_segment {
                        "segment end"
                    } else {
                        "ray direction"
                    },
                )?;
                let (direction, distance) = if is_segment {
                    let delta = other - origin;
                    let length = delta.length();
                    if length <= 1.0e-6 {
                        return Ok(Some(match method_name {
                            "line_of_sight" => Value::Bool(true),
                            "linecast_all" => Value::array(Vec::new()),
                            _ => Value::Nil,
                        }));
                    }
                    (delta, length)
                } else {
                    let max_distance = args[2].as_number()?;
                    if !max_distance.is_finite()
                        || max_distance < 0.0
                        || max_distance > crate::engine::physics::raycast::MAX_RAY_DISTANCE as f64
                    {
                        return Err("ray distance is outside the supported range".into());
                    }
                    (other, max_distance as f32)
                };
                let mut query = RayQuery::new(origin, direction, distance)?;
                ray_query_options(&mut query, args.get(expected))?;
                if matches!(method_name, "raycast" | "linecast" | "line_of_sight") {
                    query.max_hits = 1;
                }
                let mut hits = _host.engine.ray_query(query)?;
                Ok(Some(match method_name {
                    "line_of_sight" => Value::Bool(hits.is_empty()),
                    "raycast_all" | "linecast_all" => {
                        Value::array(hits.into_iter().map(ray_hit_value).collect())
                    }
                    _ => hits.pop().map_or(Value::Nil, ray_hit_value),
                }))
            }
            "resolve_camera_collision" => {
                if args.len() != 2 {
                    return Err(
                        "physics.resolve_camera_collision expects target and desired arrays"
                            .to_string(),
                    );
                }
                let t_basket = args[0].as_basket()?;
                let d_basket = args[1].as_basket()?;
                let tb = t_basket.borrow();
                let db = d_basket.borrow();
                if tb.elements.len() != 3 || db.elements.len() != 3 {
                    return Err("target and desired must be 3-element arrays".to_string());
                }
                let target = [
                    tb.elements[0].as_number()? as f32,
                    tb.elements[1].as_number()? as f32,
                    tb.elements[2].as_number()? as f32,
                ];
                let desired = [
                    db.elements[0].as_number()? as f32,
                    db.elements[1].as_number()? as f32,
                    db.elements[2].as_number()? as f32,
                ];
                let actual = _host.engine.resolve_camera_collision(target, desired);
                Ok(Some(Value::array(vec![
                    Value::Number(actual[0] as f64),
                    Value::Number(actual[1] as f64),
                    Value::Number(actual[2] as f64),
                ])))
            }
            _ => Err(format!("unknown function 'physics.{}'", method_name)),
        },
        "player" => match method_name {
            "is_valid" => {
                if !args.is_empty() {
                    return Err("player.is_valid expects no arguments".into());
                }
                Ok(Some(Value::Bool(
                    _host.engine.get_player_actor_id().is_some(),
                )))
            }
            "set_velocity" => {
                if args.len() != 3 {
                    return Err("player.set_velocity expects vx, vy, vz".into());
                }
                let velocity = [
                    args[0].as_number()? as f32,
                    args[1].as_number()? as f32,
                    args[2].as_number()? as f32,
                ];
                _host.engine.set_player_velocity(velocity)?;
                Ok(Some(Value::Nil))
            }
            "get_velocity" => {
                if !args.is_empty() {
                    return Err("player.get_velocity expects no arguments".into());
                }
                Ok(Some(
                    _host.engine.get_player_velocity().map_or(Value::Nil, |v| {
                        Value::array(v.iter().map(|n| Value::Number(*n as f64)).collect())
                    }),
                ))
            }
            "set_gravity_scale" => {
                if args.len() != 1 {
                    return Err("player.set_gravity_scale expects one number".into());
                }
                _host
                    .engine
                    .set_player_gravity_scale(args[0].as_number()? as f32)?;
                Ok(Some(Value::Nil))
            }
            "get_gravity_scale" => {
                if !args.is_empty() {
                    return Err("player.get_gravity_scale expects no arguments".into());
                }
                Ok(Some(
                    _host
                        .engine
                        .get_player_gravity_scale()
                        .map_or(Value::Nil, |v| Value::Number(v as f64)),
                ))
            }
            "set_controller_enabled" => {
                if args.len() != 1 {
                    return Err("player.set_controller_enabled expects a boolean".into());
                }
                _host
                    .engine
                    .set_player_controller_enabled(args[0].as_bool()?);
                Ok(Some(Value::Nil))
            }
            "is_controller_enabled" => {
                if !args.is_empty() {
                    return Err("player.is_controller_enabled expects no arguments".into());
                }
                Ok(Some(Value::Bool(
                    _host.engine.is_player_controller_enabled(),
                )))
            }
            "set_horizontal_velocity" => {
                if args.len() != 2 {
                    return Err(
                        "player.set_horizontal_velocity expects 2 numbers (vx, vz)".to_string()
                    );
                }
                let vx = args[0].as_number()? as f32;
                let vz = args[1].as_number()? as f32;
                _host.engine.set_player_horizontal_velocity(vx, vz);
                Ok(Some(Value::Nil))
            }
            "set_facing_direction" => {
                if args.len() != 2 {
                    return Err(
                        "player.set_facing_direction expects 2 numbers (dx, dz)".to_string()
                    );
                }
                let dx = args[0].as_number()? as f32;
                let dz = args[1].as_number()? as f32;
                _host.engine.set_player_facing_direction(dx, dz);
                Ok(Some(Value::Nil))
            }
            "select_animation" => {
                if args.len() != 1 {
                    return Err("player.select_animation expects 1 string".to_string());
                }
                let anim = args[0].as_string()?;
                _host.engine.select_player_animation(anim);
                Ok(Some(Value::Nil))
            }
            "is_grounded" => Ok(Some(Value::Bool(_host.engine.is_player_grounded()))),
            "is_swimming" => {
                if !args.is_empty() {
                    return Err("player.is_swimming expects no arguments".to_string());
                }
                Ok(Some(Value::Bool(_host.engine.is_player_swimming())))
            }
            "apply_vertical_impulse" => {
                if args.len() != 1 {
                    return Err("player.apply_vertical_impulse expects 1 number".to_string());
                }
                let impulse = args[0].as_number()? as f32;
                _host.engine.apply_player_vertical_impulse(impulse);
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("unknown function 'player.{}'", method_name)),
        },
        "save" => match method_name {
            "reset" => {
                match args {
                    [] => _host.engine.save_reset(None)?,
                    [key] => _host.engine.save_reset(Some(key.as_string()?))?,
                    _ => return Err("save.reset expects zero arguments or one string key".into()),
                }
                Ok(Some(Value::Nil))
            }
            "restart" => {
                match args {
                    [] => _host.engine.request_game_restart()?,
                    [key] => {
                        let key = key.as_string()?;
                        _host.engine.save_remove(key)?;
                        _host.engine.save_commit()?;
                    }
                    _ => {
                        return Err(
                            "save.restart expects zero arguments or one string key".to_string()
                        );
                    }
                };
                Ok(Some(Value::Nil))
            }
            "get" => {
                if args.len() != 1 {
                    return Err("save.get expects exactly 1 string key".to_string());
                }
                let key = args[0].as_string()?;
                Ok(Some(_host.engine.save_get(key)?.unwrap_or(Value::Nil)))
            }
            "set" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("save.set expects (key, value) or (key, value, default)".into());
                }
                let key = args[0].as_string()?;
                let default = args.get(2).filter(|v| !matches!(v, Value::Nil)).cloned();
                _host
                    .engine
                    .save_set_with_default(key, args[1].clone(), default)?;
                Ok(Some(Value::Nil))
            }
            "get_default" => {
                if args.len() != 1 {
                    return Err("save.get_default expects exactly one string key".into());
                }
                Ok(Some(
                    _host
                        .engine
                        .save_get_default(args[0].as_string()?)?
                        .unwrap_or(Value::Nil),
                ))
            }
            "has" => {
                if args.len() != 1 {
                    return Err("save.has expects exactly 1 string key".to_string());
                }
                let key = args[0].as_string()?;
                Ok(Some(Value::Bool(_host.engine.save_has(key)?)))
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("save.remove expects exactly 1 string key".to_string());
                }
                let key = args[0].as_string()?;
                Ok(Some(Value::Bool(_host.engine.save_remove(key)?)))
            }
            "commit" => {
                if !args.is_empty() {
                    return Err("save.commit expects no arguments".to_string());
                }
                _host.engine.save_commit()?;
                Ok(Some(Value::Nil))
            }
            _ => Err(format!("unknown function 'save.{}'", method_name)),
        },
        "ui" => match method_name {
            "get_z_index_behavior" => {
                if !args.is_empty() {
                    return Err("get_z_index_behavior expects 0 arguments".into());
                }
                Ok(Some(Value::String(_host.engine.get_ui_z_index_behavior())))
            }
            "set_z_index_behavior" => {
                if args.len() != 1 {
                    return Err("set_z_index_behavior expects a mode".into());
                }
                _host.engine.set_ui_z_index_behavior(args[0].as_string()?)?;
                Ok(Some(Value::Nil))
            }
            "get_display_order" => {
                if !args.is_empty() {
                    return Err("get_display_order expects no arguments".into());
                }
                Ok(Some(Value::Number(
                    _host.engine.get_ui_display_order() as f64
                )))
            }
            "set_display_order" => {
                if args.len() != 1 {
                    return Err("set_display_order expects one number".into());
                }
                let number = args[0].as_number()?;
                if !number.is_finite()
                    || number.fract() != 0.0
                    || number < i32::MIN as f64
                    || number > i32::MAX as f64
                {
                    return Err("display order must be a finite 32-bit integer".into());
                }
                _host.engine.set_ui_display_order(number as i32)?;
                Ok(Some(Value::Nil))
            }
            "find" | "get" => {
                let (document, key) = match args {
                    [key] => (None, key.as_string()?),
                    [document, key] => (Some(document.as_string()?), key.as_string()?),
                    _ => {
                        return Err(format!(
                            "ui.{} expects (key) or (document_key, element_key)",
                            method_name
                        ));
                    }
                };
                match _host.engine.find_ui_element(document, key) {
                    Some(id) => Ok(Some(Value::Handle {
                        kind: HandleKind::Ui,
                        id,
                    })),
                    None if method_name == "find" => Ok(Some(Value::Nil)),
                    None => Err(format!(
                        "UI element '{}' was not found in the loaded document",
                        key
                    )),
                }
            }
            "new" => {
                if args.len() != 1 {
                    return Err("ui.new expects exactly 1 argument (element_type)".to_string());
                }
                let element_type = args[0].as_string()?;
                let id = _host.engine.create_ui_element(element_type)?;
                Ok(Some(Value::Handle {
                    kind: HandleKind::Ui,
                    id,
                }))
            }
            "delete" => {
                if args.len() != 1 {
                    return Err("ui.delete expects exactly 1 argument (handle)".to_string());
                }
                let (kind, id) = args[0].as_handle()?;
                if kind != HandleKind::Ui {
                    return Err(format!(
                        "ui.delete expects a Ui handle, got {}",
                        kind.name()
                    ));
                }
                _host.engine.delete_ui_element(id)?;
                Ok(Some(Value::Nil))
            }
            "get_viewport_size" => {
                if !args.is_empty() {
                    return Err("ui.get_viewport_size expects no arguments".to_string());
                }

                let [width, height] = _host.engine.get_viewport_size();

                Ok(Some(Value::array(vec![
                    Value::Number(width as f64),
                    Value::Number(height as f64),
                ])))
            }
            _ => Err(format!("unknown function 'ui.{}'", method_name)),
        },
        "math" => {
            let res = match method_name {
                "abs" => {
                    if args.len() != 1 {
                        return Err("math.abs expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.abs())
                }
                "min" => {
                    if args.len() != 2 {
                        return Err("math.min expects exactly 2 arguments".to_string());
                    }
                    Value::Number(args[0].as_number()?.min(args[1].as_number()?))
                }
                "max" => {
                    if args.len() != 2 {
                        return Err("math.max expects exactly 2 arguments".to_string());
                    }
                    Value::Number(args[0].as_number()?.max(args[1].as_number()?))
                }
                "floor" => {
                    if args.len() != 1 {
                        return Err("math.floor expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.floor())
                }
                "ceil" => {
                    if args.len() != 1 {
                        return Err("math.ceil expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.ceil())
                }
                "round" => {
                    if args.len() != 1 {
                        return Err("math.round expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.round())
                }
                "sqrt" => {
                    if args.len() != 1 {
                        return Err("math.sqrt expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.sqrt())
                }
                "pow" => {
                    if args.len() != 2 {
                        return Err("math.pow expects exactly 2 arguments".to_string());
                    }
                    Value::Number(args[0].as_number()?.powf(args[1].as_number()?))
                }
                "sin" => {
                    if args.len() != 1 {
                        return Err("math.sin expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.sin())
                }
                "cos" => {
                    if args.len() != 1 {
                        return Err("math.cos expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.cos())
                }
                "tan" => {
                    if args.len() != 1 {
                        return Err("math.tan expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.tan())
                }
                "random" => {
                    use rand::Rng;
                    let mut rng = rand::thread_rng();
                    if args.is_empty() {
                        Value::Number(rng.r#gen::<f64>())
                    } else if args.len() == 2 {
                        let min_value = args[0].as_number()?;
                        let max_value = args[1].as_number()?;
                        const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
                        if !min_value.is_finite()
                            || !max_value.is_finite()
                            || min_value.fract() != 0.0
                            || max_value.fract() != 0.0
                            || min_value.abs() > MAX_SAFE_INTEGER
                            || max_value.abs() > MAX_SAFE_INTEGER
                        {
                            return Err(
                                "math.random bounds must be finite whole numbers in the safe integer range".to_string()
                            );
                        }
                        let min = min_value as i64;
                        let max = max_value as i64;
                        if min > max {
                            return Err("math.random min cannot be greater than max".to_string());
                        }
                        Value::Number(rng.gen_range(min..=max) as f64)
                    } else {
                        return Err("math.random expects 0 or 2 arguments".to_string());
                    }
                }
                "clamp" => {
                    if args.len() != 3 {
                        return Err("math.clamp expects exactly 3 arguments".to_string());
                    }
                    let val = args[0].as_number()?;
                    let min = args[1].as_number()?;
                    let max = args[2].as_number()?;
                    if !val.is_finite() || !min.is_finite() || !max.is_finite() {
                        return Err("math.clamp arguments must be finite numbers".to_string());
                    }
                    if min > max {
                        return Err("math.clamp min cannot be greater than max".to_string());
                    }
                    Value::Number(val.clamp(min, max))
                }
                "lerp" => {
                    if args.len() != 3 {
                        return Err("math.lerp expects exactly 3 arguments".to_string());
                    }
                    let a = args[0].as_number()?;
                    let b = args[1].as_number()?;
                    let t = args[2].as_number()?;
                    if !a.is_finite() || !b.is_finite() || !t.is_finite() {
                        return Err("math.lerp arguments must be finite numbers".to_string());
                    }
                    let result = a + (b - a) * t;
                    if !result.is_finite() {
                        return Err("math.lerp result must be finite".to_string());
                    }
                    Value::Number(result)
                }
                "deg_to_rad" => {
                    if args.len() != 1 {
                        return Err("math.deg_to_rad expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.to_radians())
                }
                "rad_to_deg" => {
                    if args.len() != 1 {
                        return Err("math.rad_to_deg expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_number()?.to_degrees())
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "cell" => {
            let res = match method_name {
                "new" => {
                    if args.len() != 1 {
                        return Err("cell.new expects exactly 1 argument (cell_type)".to_string());
                    }
                    let cell_type = args[0].as_string()?;
                    let (kind, id) = _host.engine.create_runtime_cell(cell_type)?;
                    Value::Handle { kind, id }
                }
                "delete" => {
                    if args.len() != 1 {
                        return Err("cell.delete expects exactly 1 argument (handle)".to_string());
                    }
                    let (kind, id) = args[0].as_handle()?;
                    match kind {
                        HandleKind::Cell | HandleKind::Light => {}
                        _ => {
                            return Err(format!(
                                "cell.delete expects a Cell or Light handle, got {}",
                                kind.name()
                            ));
                        }
                    }
                    _host.engine.delete_cell(id)?;
                    Value::Nil
                }
                "get" => {
                    if args.len() != 1 {
                        return Err("cell.get expects exactly 1 argument (id)".to_string());
                    }
                    let id = args[0].as_number()? as u64;
                    if _host.engine.cell_exists(id) {
                        Value::Handle {
                            kind: HandleKind::Cell,
                            id,
                        }
                    } else {
                        Value::Nil
                    }
                }
                "exists" => {
                    if args.len() != 1 {
                        return Err("cell.exists expects exactly 1 argument (id)".to_string());
                    }
                    let id = args[0].as_number()? as u64;
                    Value::Bool(_host.engine.cell_exists(id))
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "entity" => {
            let res = match method_name {
                "get" => {
                    if args.len() != 1 {
                        return Err("entity.get expects exactly 1 argument (id)".to_string());
                    }
                    let id = args[0].as_number()? as u64;
                    if _host.engine.entity_exists(id) {
                        Value::Handle {
                            kind: HandleKind::Entity,
                            id,
                        }
                    } else {
                        Value::Nil
                    }
                }
                "exists" => {
                    if args.len() != 1 {
                        return Err("entity.exists expects exactly 1 argument (id)".to_string());
                    }
                    let id = args[0].as_number()? as u64;
                    Value::Bool(_host.engine.entity_exists(id))
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "basket" => {
            let res = match method_name {
                "len" => {
                    if args.len() != 1 {
                        return Err("basket.len expects 1 argument".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    Value::Number(basket_val.borrow().elements.len() as f64)
                }
                "insert" => {
                    if args.len() < 2 || args.len() > 3 {
                        return Err("basket.insert expects 2 or 3 arguments".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    let mut borrowed = basket_val.borrow_mut();
                    if borrowed.frozen {
                        return Err("cannot mutate frozen basket".to_string());
                    }
                    if args.len() == 2 {
                        let value = args[1].clone();
                        borrowed.elements.push(value);
                    } else {
                        let pos = args[1].as_index()?;
                        let value = args[2].clone();
                        if pos > borrowed.elements.len() {
                            return Err("basket.insert index out of bounds".to_string());
                        }
                        borrowed.elements.insert(pos, value);
                    }
                    Value::Nil
                }
                "remove" => {
                    if args.len() < 1 || args.len() > 2 {
                        return Err("basket.remove expects 1 or 2 arguments".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    let mut borrowed = basket_val.borrow_mut();
                    if borrowed.frozen {
                        return Err("cannot mutate frozen basket".to_string());
                    }
                    if borrowed.elements.is_empty() {
                        return Err("cannot remove from empty basket".to_string());
                    }
                    if args.len() == 1 {
                        borrowed.elements.pop().unwrap_or(Value::Nil)
                    } else {
                        let pos = args[1].as_index()?;
                        if pos >= borrowed.elements.len() {
                            return Err("basket.remove index out of bounds".to_string());
                        }
                        borrowed.elements.remove(pos)
                    }
                }
                "sort" => {
                    if args.len() < 1 || args.len() > 2 {
                        return Err("basket.sort expects 1 or 2 arguments".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    if args.len() == 2 {
                        return Err("comparator not supported yet".to_string());
                    }
                    let mut borrowed = basket_val.borrow_mut();
                    if borrowed.frozen {
                        return Err("cannot mutate frozen basket".to_string());
                    }

                    let mut err = None;
                    borrowed.elements.sort_by(|a, b| match (a, b) {
                        (Value::Number(na), Value::Number(nb)) => na.total_cmp(nb),
                        (Value::String(sa), Value::String(sb)) => sa.cmp(sb),
                        _ => {
                            if err.is_none() {
                                err = Some(format!(
                                    "cannot compare {} and {}",
                                    a.type_name(),
                                    b.type_name()
                                ));
                            }
                            std::cmp::Ordering::Equal
                        }
                    });
                    if let Some(e) = err {
                        return Err(e);
                    }
                    Value::Nil
                }
                "clear" => {
                    if args.len() != 1 {
                        return Err("basket.clear expects exactly 1 argument".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    let mut borrowed = basket_val.borrow_mut();
                    if borrowed.frozen {
                        return Err("cannot mutate frozen basket".to_string());
                    }
                    borrowed.elements.clear();
                    Value::Nil
                }
                "create" => {
                    if args.len() < 1 || args.len() > 2 {
                        return Err("basket.create expects 1 or 2 arguments".to_string());
                    }
                    let count = args[0].as_index()?;
                    if count > MAX_SCRIPT_BASKET_ELEMENTS {
                        return Err(format!(
                            "basket.create count exceeds the {} element limit",
                            MAX_SCRIPT_BASKET_ELEMENTS
                        ));
                    }
                    let default_value = if args.len() == 2 {
                        args[1].clone()
                    } else {
                        Value::Nil
                    };
                    let mut elements = Vec::with_capacity(count);
                    for _ in 0..count {
                        elements.push(default_value.clone());
                    }
                    Value::array(elements)
                }
                "find" => {
                    if args.len() < 2 || args.len() > 3 {
                        return Err("basket.find expects 2 or 3 arguments".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    let target = &args[1];
                    let init = if args.len() == 3 {
                        args[2].as_index()?
                    } else {
                        0
                    };

                    let borrowed = basket_val.borrow();
                    if init > borrowed.elements.len() {
                        return Err("basket.find init index out of bounds".to_string());
                    }
                    let mut found_idx = -1;
                    for i in init..borrowed.elements.len() {
                        if &borrowed.elements[i] == target {
                            found_idx = i as i32;
                            break;
                        }
                    }
                    if found_idx == -1 {
                        Value::Nil
                    } else {
                        Value::Number(found_idx as f64)
                    }
                }
                "move" => {
                    if args.len() < 4 || args.len() > 5 {
                        return Err("basket.move expects 4 or 5 arguments (src, a, b, t, [dst])"
                            .to_string());
                    }
                    let src_basket = args[0].as_basket()?;
                    let a = args[1].as_index()?;
                    let b = args[2].as_index()?;
                    let t = args[3].as_index()?;

                    let has_dst = args.len() == 5;
                    let dst_basket = if has_dst {
                        args[4].as_basket()?
                    } else {
                        src_basket.clone()
                    };

                    let slice_to_move: Vec<Value> = {
                        let borrowed_src = src_basket.borrow();
                        if a > b || b >= borrowed_src.elements.len() {
                            return Err("invalid source range bounds".to_string());
                        }
                        borrowed_src.elements[a..=b].iter().cloned().collect()
                    };

                    let mut borrowed_dst = dst_basket.borrow_mut();
                    if borrowed_dst.frozen {
                        return Err("cannot mutate frozen destination basket".to_string());
                    }
                    if t > borrowed_dst.elements.len() {
                        return Err("destination index out of bounds".to_string());
                    }

                    if t + slice_to_move.len() > borrowed_dst.elements.len() {
                        borrowed_dst
                            .elements
                            .resize(t + slice_to_move.len(), Value::Nil);
                    }

                    for (i, val) in slice_to_move.into_iter().enumerate() {
                        borrowed_dst.elements[t + i] = val;
                    }

                    Value::Nil
                }
                "concat" => {
                    if args.len() < 1 || args.len() > 4 {
                        return Err("basket.concat expects 1, 2, 3, or 4 arguments".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    let sep = if args.len() >= 2 {
                        args[1].as_string()?
                    } else {
                        ""
                    };
                    let borrowed = basket_val.borrow();
                    if borrowed.elements.is_empty() {
                        return Ok(Some(Value::String("".to_string())));
                    }

                    let i = if args.len() >= 3 {
                        args[2].as_index()?
                    } else {
                        0
                    };
                    let j = if args.len() == 4 {
                        args[3].as_index()?
                    } else {
                        borrowed.elements.len().saturating_sub(1)
                    };

                    if i >= borrowed.elements.len() || j >= borrowed.elements.len() || i > j {
                        return Err("invalid bounds for basket.concat".to_string());
                    }

                    let slice = &borrowed.elements[i..=j];
                    let str_parts: Vec<String> = slice.iter().map(|v| v.display_string()).collect();
                    Value::String(str_parts.join(sep))
                }
                "clone" => {
                    if args.len() != 1 {
                        return Err("basket.clone expects exactly 1 argument".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    Value::array(basket_val.borrow().elements.clone())
                }
                "freeze" => {
                    if args.len() != 1 {
                        return Err("basket.freeze expects exactly 1 argument".to_string());
                    }
                    let basket_val = args[0].as_basket()?;
                    basket_val.borrow_mut().frozen = true;
                    args[0].clone()
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "string" => {
            let res = match method_name {
                "len" => {
                    if args.len() != 1 {
                        return Err("string.len expects exactly 1 argument".to_string());
                    }
                    Value::Number(args[0].as_string()?.chars().count() as f64)
                }
                "lower" => {
                    if args.len() != 1 {
                        return Err("string.lower expects exactly 1 argument".to_string());
                    }
                    Value::String(args[0].as_string()?.to_lowercase())
                }
                "upper" => {
                    if args.len() != 1 {
                        return Err("string.upper expects exactly 1 argument".to_string());
                    }
                    Value::String(args[0].as_string()?.to_uppercase())
                }
                "reverse" => {
                    if args.len() != 1 {
                        return Err("string.reverse expects exactly 1 argument".to_string());
                    }
                    Value::String(args[0].as_string()?.chars().rev().collect())
                }
                "split" => {
                    if args.len() < 1 || args.len() > 2 {
                        return Err("string.split expects 1 or 2 arguments".to_string());
                    }
                    let s = args[0].as_string()?;
                    let sep = if args.len() == 2 {
                        args[1].as_string()?
                    } else {
                        ""
                    };

                    let parts: Vec<Value> = if sep.is_empty() {
                        s.chars().map(|c| Value::String(c.to_string())).collect()
                    } else {
                        s.split(sep)
                            .map(|part| Value::String(part.to_string()))
                            .collect()
                    };
                    Value::array(parts)
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "script" => {
            let res = match method_name {
                "enable" => {
                    if args.len() != 1 {
                        return Err("script.enable expects 1 argument (path)".to_string());
                    }
                    let path = args[0].as_string()?;
                    _host.engine.enable_script(path);
                    Value::Nil
                }
                "disable" => {
                    if args.len() != 1 {
                        return Err("script.disable expects 1 argument (path)".to_string());
                    }
                    let path = args[0].as_string()?;
                    _host.engine.disable_script(path);
                    Value::Nil
                }
                "is_enabled" => {
                    if args.len() != 1 {
                        return Err("script.is_enabled expects 1 argument (path)".to_string());
                    }
                    let path = args[0].as_string()?;
                    let enabled = _host.engine.is_script_enabled(path);
                    Value::Bool(enabled)
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "event" => {
            let res = match method_name {
                "fire" => {
                    if args.is_empty() {
                        return Err(
                            "event.fire expects at least 1 argument (event_name)".to_string()
                        );
                    }
                    let event_name = args[0].as_string()?;
                    let event_args = args[1..].to_vec();
                    _host.engine.fire_event(event_name, event_args);
                    Value::Nil
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        "test" => {
            let res = match method_name {
                "complete" => {
                    if args.len() != 2 {
                        return Err(
                            "test.complete expects 2 arguments (test_name, passed)".to_string()
                        );
                    }
                    let test_name = args[0].as_string()?;
                    let passed = args[1].as_bool()?;
                    _host.engine.complete_test(test_name, passed);
                    Value::Nil
                }
                "is_completed" => {
                    if args.len() != 1 {
                        return Err("test.is_completed expects 1 argument (test_name)".to_string());
                    }
                    let test_name = args[0].as_string()?;
                    if let Some(_passed) = _host.engine.is_test_completed(test_name) {
                        Value::Bool(true)
                    } else {
                        Value::Bool(false)
                    }
                }
                "passed" => {
                    if args.len() != 1 {
                        return Err("test.passed expects 1 argument (test_name)".to_string());
                    }
                    let test_name = args[0].as_string()?;
                    if let Some(passed) = _host.engine.is_test_completed(test_name) {
                        Value::Bool(passed)
                    } else {
                        Value::Nil
                    }
                }
                "summary" => {
                    let (p, f, t) = _host.engine.get_test_results();
                    Value::array(vec![
                        Value::Number(p as f64),
                        Value::Number(f as f64),
                        Value::Number(t as f64),
                    ])
                }
                _ => return Ok(None),
            };
            return Ok(Some(res));
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entity::EntityManager;
    use crate::scripting::api::HostContext;
    use crate::scripting::ast::Program;
    use crate::scripting::interpreter::{Interpreter, ScriptInstance};
    use crate::scripting::source::SourceSpan;
    use crate::scripting::value::Scope;

    fn setup() -> (Interpreter, ScriptInstance, Vec<Scope>, EntityManager) {
        let program = Program {
            span: SourceSpan::new(0, 0),
            declarations: vec![],
            statements: vec![],
        };
        let interpreter = Interpreter::new(program);
        let instance = ScriptInstance::new_empty();
        let scopes = vec![Scope::new()];
        let em = EntityManager::new();
        (interpreter, instance, scopes, em)
    }

    #[test]
    fn basket_create_rejects_unrepresentable_and_excessive_counts() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        for count in [
            Value::Number(1e30),
            Value::Number((MAX_SCRIPT_BASKET_ELEMENTS + 1) as f64),
        ] {
            let error = call_stdlib_function(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                "basket",
                "create",
                &[count],
                &mut host,
            )
            .unwrap_err();
            assert!(
                error.contains("integer") || error.contains("element limit"),
                "{error}"
            );
        }
    }

    #[test]
    fn camera_script_functions_validate_arguments_and_use_engine_host() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };
        let result = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "camera",
            "get_script",
            &[],
            &mut host,
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.as_string().unwrap(), "thirdPerson");

        let wrong_get = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "camera",
            "get_script",
            &[Value::Nil],
            &mut host,
        )
        .unwrap_err();
        assert!(wrong_get.contains("expects no arguments"));

        let wrong_set = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "camera",
            "set_script",
            &[],
            &mut host,
        )
        .unwrap_err();
        assert!(wrong_set.contains("expects one camera package name"));

        let no_support = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "camera",
            "set_script",
            &[Value::String("firstPerson".into())],
            &mut host,
        )
        .unwrap_err();
        assert!(no_support.contains("unavailable on this host"));
    }

    #[test]
    fn terrain_generate_validates_array_shape_and_integer_components() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let shape_error = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "terrain",
            "generate",
            &[
                Value::array(vec![Value::Number(0.0), Value::Number(0.0)]),
                Value::array(vec![Value::Number(1.0); 3]),
            ],
            &mut host,
        )
        .unwrap_err();
        assert!(shape_error.contains("position must be a 3-number array"));

        let integer_error = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "terrain",
            "generate",
            &[
                Value::array(vec![Value::Number(0.0); 3]),
                Value::array(vec![
                    Value::Number(1.5),
                    Value::Number(1.0),
                    Value::Number(1.0),
                ]),
            ],
            &mut host,
        )
        .unwrap_err();
        assert!(integer_error.contains("size[0]") && integer_error.contains("whole number"));
    }

    #[test]
    fn terrain_set_voxels_validates_dense_contract_before_host_call() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let bad_count = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "terrain",
            "set_voxels",
            &[
                Value::array(vec![Value::Number(0.0); 3]),
                Value::array(vec![Value::Number(1.0); 3]),
                Value::array(vec![Value::String("rock".into())]),
                Value::array(vec![]),
            ],
            &mut host,
        )
        .unwrap_err();
        assert!(bad_count.contains("expected 1 voxel entries"));

        let bad_occupancy = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "terrain",
            "set_voxels",
            &[
                Value::array(vec![Value::Number(0.0); 3]),
                Value::array(vec![Value::Number(1.0); 3]),
                Value::array(vec![Value::String("rock".into())]),
                Value::array(vec![Value::array(vec![
                    Value::Number(0.0),
                    Value::Number(1.5),
                ])]),
            ],
            &mut host,
        )
        .unwrap_err();
        assert!(bad_occupancy.contains("occupancy 0..1"));
    }

    #[test]
    fn terrain_generate_rejects_nonfinite_and_out_of_range_numbers() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };
        for (number, component) in [
            (f64::NAN, "position[0]"),
            (f64::INFINITY, "position[0]"),
            (i32::MAX as f64 + 1.0, "position[0]"),
        ] {
            let error = call_stdlib_function(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                "terrain",
                "generate",
                &[
                    Value::array(vec![
                        Value::Number(number),
                        Value::Number(0.0),
                        Value::Number(0.0),
                    ]),
                    Value::array(vec![Value::Number(1.0); 3]),
                ],
                &mut host,
            )
            .unwrap_err();
            assert!(error.contains(component), "{error}");
        }
    }

    #[test]
    fn terrain_set_voxels_rejects_invalid_material_and_spacing() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };
        let voxel_args = |material: f64, spacing: f64| {
            vec![
                Value::array(vec![Value::Number(0.0); 3]),
                Value::array(vec![Value::Number(1.0); 3]),
                Value::array(vec![Value::String("rock".into())]),
                Value::array(vec![Value::array(vec![
                    Value::Number(material),
                    Value::Number(1.0),
                ])]),
                Value::Number(spacing),
            ]
        };
        let invalid_material = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "terrain",
            "set_voxels",
            &voxel_args(1.0, 4.0),
            &mut host,
        )
        .unwrap_err();
        assert!(
            invalid_material.contains("outside the material palette"),
            "{invalid_material}"
        );

        let invalid_spacing = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "terrain",
            "set_voxels",
            &voxel_args(0.0, f64::NAN),
            &mut host,
        )
        .unwrap_err();
        assert!(
            invalid_spacing.contains("spacing_studs must be finite"),
            "{invalid_spacing}"
        );
    }

    #[test]
    fn test_math_random_no_args() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let res = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "math",
            "random",
            &[],
            &mut host,
        )
        .unwrap()
        .unwrap();
        let val = res.as_number().unwrap();
        assert!(val >= 0.0 && val < 1.0);
    }

    #[test]
    fn test_math_random_range() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let res = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "math",
            "random",
            &[Value::Number(1.0), Value::Number(1.0)],
            &mut host,
        )
        .unwrap()
        .unwrap();
        assert_eq!(res.as_number().unwrap(), 1.0);

        for _ in 0..100 {
            let res = call_stdlib_function(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                "math",
                "random",
                &[Value::Number(1.0), Value::Number(3.0)],
                &mut host,
            )
            .unwrap()
            .unwrap();
            let val = res.as_number().unwrap();
            assert!(val == 1.0 || val == 2.0 || val == 3.0);
        }
    }

    #[test]
    fn stdlib_math_rejects_nonfinite_bounds_without_native_panics() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };
        for (namespace, method, args, expected) in [
            (
                "math",
                "clamp",
                vec![
                    Value::Number(1.0),
                    Value::Number(f64::NAN),
                    Value::Number(5.0),
                ],
                "finite",
            ),
            (
                "math",
                "clamp",
                vec![
                    Value::Number(f64::INFINITY),
                    Value::Number(0.0),
                    Value::Number(1.0),
                ],
                "finite",
            ),
            (
                "math",
                "lerp",
                vec![
                    Value::Number(0.0),
                    Value::Number(1.0),
                    Value::Number(f64::NAN),
                ],
                "finite",
            ),
            (
                "math",
                "lerp",
                vec![
                    Value::Number(f64::MAX),
                    Value::Number(-f64::MAX),
                    Value::Number(0.5),
                ],
                "finite",
            ),
            (
                "math",
                "random",
                vec![Value::Number(f64::NAN), Value::Number(3.0)],
                "finite whole",
            ),
            (
                "math",
                "random",
                vec![Value::Number(0.5), Value::Number(3.0)],
                "finite whole",
            ),
            (
                "math",
                "random",
                vec![Value::Number(1.0), Value::Number(f64::INFINITY)],
                "finite whole",
            ),
        ] {
            let result = call_stdlib_function(
                &mut interpreter,
                &mut instance,
                &mut scopes,
                namespace,
                method,
                &args,
                &mut host,
            );
            let error = result.expect_err("invalid stdlib numeric arguments must be rejected");
            assert!(error.contains(expected), "{method}: {error}");
        }
        let valid = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "math",
            "clamp",
            &[Value::Number(4.0), Value::Number(0.0), Value::Number(2.0)],
            &mut host,
        )
        .unwrap()
        .unwrap();
        assert_eq!(valid.as_number().unwrap(), 2.0);
    }

    #[test]
    fn test_math_random_invalid_range() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let res = call_stdlib_function(
            &mut interpreter,
            &mut instance,
            &mut scopes,
            "math",
            "random",
            &[Value::Number(10.0), Value::Number(1.0)],
            &mut host,
        );
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("min cannot be greater than max"));
    }
    #[test]
    fn terrain_sample_surface_validates_contract_before_host() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };
        let call = |args: Vec<Value>,
                    interpreter: &mut Interpreter,
                    instance: &mut ScriptInstance,
                    scopes: &mut Vec<Scope>,
                    host: &mut HostContext| {
            call_stdlib_function(
                interpreter,
                instance,
                scopes,
                "terrain",
                "sample_surface",
                &args,
                host,
            )
        };
        assert!(
            call(
                vec![],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host
            )
            .unwrap_err()
            .contains("exactly two")
        );
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            let err = call(
                vec![Value::Number(bad), Value::Number(0.0)],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host,
            )
            .unwrap_err();
            assert!(err.contains("finite"), "{err}");
        }
        assert!(
            call(
                vec![Value::String("x".into()), Value::Number(0.0)],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host
            )
            .unwrap_err()
            .contains("finite numbers")
        );
        assert!(
            call(
                vec![Value::Number(0.0), Value::Number(0.0)],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host
            )
            .unwrap_err()
            .contains("not available on this host")
        );
    }

    #[test]
    fn actor_tags_and_observe_reject_invalid_script_values() {
        let (mut interpreter, mut instance, mut scopes, mut em) = setup();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };
        let call = |namespace: &str,
                    method: &str,
                    args: Vec<Value>,
                    interpreter: &mut Interpreter,
                    instance: &mut ScriptInstance,
                    scopes: &mut Vec<Scope>,
                    host: &mut HostContext| {
            call_stdlib_function(
                interpreter,
                instance,
                scopes,
                namespace,
                method,
                &args,
                host,
            )
        };
        for tag in ["", " bad", "bad ", "bad\nname", &"x".repeat(65)] {
            assert!(
                call(
                    "tags",
                    "find",
                    vec![Value::String(tag.into())],
                    &mut interpreter,
                    &mut instance,
                    &mut scopes,
                    &mut host
                )
                .unwrap_err()
                .contains("1..64")
            );
        }
        assert!(
            call(
                "tags",
                "has",
                vec![Value::Number(1.0), Value::String("good".into())],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host
            )
            .unwrap_err()
            .contains("Actor")
        );
        assert!(
            call(
                "tags",
                "add",
                vec![
                    Value::Handle {
                        kind: HandleKind::Cell,
                        id: 1
                    },
                    Value::String("good".into())
                ],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host
            )
            .unwrap_err()
            .contains("Actor")
        );
        assert!(
            call(
                "observe",
                "property",
                vec![Value::Nil, Value::String("visible".into()), Value::Nil],
                &mut interpreter,
                &mut instance,
                &mut scopes,
                &mut host
            )
            .unwrap_err()
            .contains("engine handle")
        );
        for id in [f64::NAN, 0.0, f64::INFINITY, 1.5, 9007199254740994.0] {
            assert!(
                call(
                    "observe",
                    "cancel",
                    vec![Value::Number(id)],
                    &mut interpreter,
                    &mut instance,
                    &mut scopes,
                    &mut host
                )
                .unwrap_err()
                .contains("positive whole-number")
            );
        }
    }
}

#[cfg(test)]
mod native_spring_arm_script_contract_tests {
    use super::*;
    fn settings(extras: Vec<(&str, Value)>) -> Vec<Value> {
        let mut map = std::collections::BTreeMap::new();
        map.insert(
            MapKey::String("target".into()),
            Value::array(vec![
                Value::Number(0.0),
                Value::Number(1.0),
                Value::Number(0.0),
            ]),
        );
        map.insert(
            MapKey::String("offset".into()),
            Value::array(vec![
                Value::Number(0.0),
                Value::Number(2.0),
                Value::Number(6.0),
            ]),
        );
        map.insert(MapKey::String("dt".into()), Value::Number(1.0 / 60.0));
        for (k, v) in extras {
            map.insert(MapKey::String(k.into()), v);
        }
        vec![Value::map(map)]
    }
    #[test]
    fn spring_arm_is_configurable_with_default_collision() {
        let default = super::spring_arm_settings(&settings(Vec::new())).unwrap();
        assert!(default.collision);
        assert_eq!(default.dead_zone, [0.0, 0.0]);
        let customized = super::spring_arm_settings(&settings(vec![
            ("collision", Value::Bool(false)),
            ("follow_speed", Value::Number(5.0)),
            (
                "dead_zone",
                Value::array(vec![Value::Number(0.2), Value::Number(0.15)]),
            ),
            (
                "look_ahead",
                Value::array(vec![
                    Value::Number(1.0),
                    Value::Number(0.0),
                    Value::Number(0.0),
                ]),
            ),
        ]))
        .unwrap();
        assert!(!customized.collision);
        assert_eq!(customized.follow_speed, 5.0);
        assert_eq!(customized.dead_zone, [0.2, 0.15]);
        assert_eq!(customized.look_ahead, glam::Vec3::X);
    }
    #[test]
    fn spring_arm_rejects_unknown_options_and_bad_baskets() {
        assert!(
            super::spring_arm_settings(&settings(vec![("collsion", Value::Bool(false))]))
                .unwrap_err()
                .contains("unknown setting")
        );
        assert!(
            super::spring_arm_settings(&settings(vec![("collision", Value::Number(0.0))]))
                .unwrap_err()
                .contains("true or false")
        );
        assert!(
            super::spring_arm_settings(&settings(vec![(
                "look_ahead",
                Value::array(vec![Value::Number(1.0)])
            )]))
            .unwrap_err()
            .contains("3-number")
        );
        let invalid =
            super::spring_arm_settings(&settings(vec![("dt", Value::Number(f64::INFINITY))]))
                .unwrap();
        assert!(
            crate::renderer::camera_spring_arm::SpringArmState::default()
                .update(invalid, [1920.0, 1080.0])
                .is_err()
        );
    }
}
