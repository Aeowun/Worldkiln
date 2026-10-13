use super::{
    api::HostContext,
    value::{HandleKind, Value},
};
use glam::Vec3;

pub(super) fn call(
    method: &str,
    args: &[Value],
    host: &mut HostContext,
) -> Result<Option<Value>, String> {
    let result = match method {
        "burst" => {
            if !(2..=3).contains(&args.len()) {
                return Err("particles.burst expects (preset, [x, y, z] [, count])".into());
            }
            let preset = args[0]
                .as_string()
                .map_err(|_| "particles.burst: preset must be a string")?;
            // Validate before crossing the host boundary, including in headless hosts.
            crate::engine::particles::BurstConfig::preset(preset)?;
            let basket = args[1]
                .as_basket()
                .map_err(|_| "particles.burst: position must be [x, y, z]")?;
            let basket = basket.borrow();
            if basket.elements.len() != 3 {
                return Err("particles.burst: position must be [x, y, z]".into());
            }
            let mut components = [0.0f32; 3];
            for (component, value) in components.iter_mut().zip(&basket.elements) {
                *component = value
                    .as_number()
                    .map_err(|_| "particles.burst: position must contain finite numbers")?
                    as f32;
                if !component.is_finite() {
                    return Err("particles.burst: position must contain finite numbers".into());
                }
            }
            let count = if let Some(value) = args.get(2) {
                value
                    .as_number()
                    .map_err(|_| "particles.burst: count must be an integer in 0..=1024")?
            } else {
                24.0
            };
            if !count.is_finite()
                || count.fract() != 0.0
                || !(0.0..=crate::engine::particles::MAX_BURST as f64).contains(&count)
            {
                return Err("particles.burst: count must be an integer in 0..=1024".into());
            }
            Value::Number(host.engine.particle_burst(
                preset,
                Vec3::from_array(components),
                count as usize,
            )? as f64)
        }
        "set_enabled" => {
            if args.len() != 2 {
                return Err("particles.set_enabled expects (ParticleEmitter Cell, bool)".into());
            }
            let (kind, id) = args[0]
                .as_handle()
                .map_err(|_| "particles.set_enabled: expected a ParticleEmitter Cell handle")?;
            if kind != HandleKind::Cell {
                return Err(format!(
                    "particles.set_enabled: expected a ParticleEmitter Cell handle, got {}",
                    kind.name()
                ));
            }
            let Value::Bool(enabled) = args[1] else {
                return Err("particles.set_enabled: enabled must be bool".into());
            };
            host.engine.particle_set_enabled(id, enabled)?;
            Value::Nil
        }
        "clear" | "count" => {
            if !args.is_empty() {
                return Err(format!("particles.{method} expects no arguments"));
            }
            if method == "clear" {
                host.engine.particle_clear()?;
                Value::Nil
            } else {
                Value::Number(host.engine.particle_count() as f64)
            }
        }
        _ => return Err(format!("unknown function 'particles.{method}'")),
    };
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn particle_api_rejects_malformed_data_before_host_mutation() {
        let mut manager = crate::engine::entity::EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut manager,
        };
        for count in [-1.0, 1.5, 1025.0, f64::NAN, f64::INFINITY] {
            let args = [
                Value::String("fire".into()),
                Value::array(vec![Value::Number(0.0); 3]),
                Value::Number(count),
            ];
            assert!(
                call("burst", &args, &mut host)
                    .unwrap_err()
                    .contains("count")
            );
        }
        let args = [
            Value::String("smoke".into()),
            Value::array(vec![Value::Number(f64::INFINITY); 3]),
        ];
        assert!(
            call("burst", &args, &mut host)
                .unwrap_err()
                .contains("position")
        );
        let args = [
            Value::Handle {
                kind: HandleKind::Actor,
                id: 1,
            },
            Value::Bool(false),
        ];
        assert!(
            call("set_enabled", &args, &mut host)
                .unwrap_err()
                .contains("got Actor")
        );
        assert!(
            call("clear", &[Value::Nil], &mut host)
                .unwrap_err()
                .contains("no arguments")
        );
    }
}
