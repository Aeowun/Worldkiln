use crate::scripting::api::HostContext;
use crate::scripting::interpreter::Interpreter;
use crate::scripting::value::{HandleKind, Value};

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
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

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:10 -05:00
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

pub fn resolve_stdlib_property(
    object_name: &str,
    property_name: &str,
    host: &mut HostContext,
) -> Result<Option<Value>, String> {
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
    match object_name {
        // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
        "terrain" => match method_name {
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
            // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:10 -05:00
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
            // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
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
            "get" => {
                if args.len() != 1 {
                    return Err("save.get expects exactly 1 string key".to_string());
                }
                let key = args[0].as_string()?;
                Ok(Some(_host.engine.save_get(key)?.unwrap_or(Value::Nil)))
            }
            "set" => {
                if args.len() != 2 {
                    return Err("save.set expects a string key and a typed value".to_string());
                }
                let key = args[0].as_string()?;
                _host.engine.save_set(key, args[1].clone())?;
                Ok(Some(Value::Nil))
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
                        // Worldkiln 0.8.5 | WO MAT-017 | 2026-10-10 16:04 -05:00
                        // AeoScript numbers are f64: reject fractional/nonfinite
                        // ranges instead of silently truncating or saturating.
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
                    // Worldkiln 0.8.5 | WO MAT-017 | 2026-10-10 16:04 -05:00
                    // f64::clamp panics on NaN bounds; reject all nonfinite
                    // operands with a script error before invoking it.
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
                    // Worldkiln 0.8.5 | WO MAT-017 | 2026-10-10 16:04 -05:00
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

    // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
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

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
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

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:10 -05:00
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

        // [1, 1] -> 1
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

        // [1, 3] -> 1, 2, or 3
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

    // Worldkiln 0.8.5 | WO MAT-017 | 2026-10-10 16:04 -05:00
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
}
