// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TweenValue {
    Scalar(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
}
impl TweenValue {
    pub fn valid(self) -> bool {
        match self {
            Self::Scalar(x) => x.is_finite(),
            Self::Vec2(x) => x.iter().all(|v| v.is_finite()),
            Self::Vec3(x) => x.iter().all(|v| v.is_finite()),
            Self::Vec4(x) => x.iter().all(|v| v.is_finite()),
        }
    }
    pub fn interpolate(self, other: Self, t: f64) -> Result<Self, String> {
        let t = t.clamp(0.0, 1.0);
        let f = |a: f64, b: f64| a + (b - a) * t;
        let value = match (self, other) {
            (Self::Scalar(a), Self::Scalar(b)) => Self::Scalar(f(a, b)),
            (Self::Vec2(a), Self::Vec2(b)) => Self::Vec2(std::array::from_fn(|i| f(a[i], b[i]))),
            (Self::Vec3(a), Self::Vec3(b)) => Self::Vec3(std::array::from_fn(|i| f(a[i], b[i]))),
            (Self::Vec4(a), Self::Vec4(b)) => Self::Vec4(std::array::from_fn(|i| f(a[i], b[i]))),
            _ => return Err("Tween property and target value have different types".into()),
        };
        if !value.valid() {
            return Err("Tween value must be finite".into());
        }
        Ok(value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TweenEasing {
    Linear,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubicIn,
    CubicOut,
    CubicInOut,
    SineIn,
    SineOut,
    SineInOut,
}
impl TweenEasing {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "linear" => Ok(Self::Linear),
            "quadIn" => Ok(Self::QuadIn),
            "quadOut" => Ok(Self::QuadOut),
            "quadInOut" => Ok(Self::QuadInOut),
            "cubicIn" => Ok(Self::CubicIn),
            "cubicOut" => Ok(Self::CubicOut),
            "cubicInOut" => Ok(Self::CubicInOut),
            "sineIn" => Ok(Self::SineIn),
            "sineOut" => Ok(Self::SineOut),
            "sineInOut" => Ok(Self::SineInOut),
            _ => Err(format!("Unsupported tween easing {s}")),
        }
    }
    fn apply(self, t: f64) -> f64 {
        match self {
            Self::Linear => t,
            Self::QuadIn => t * t,
            Self::QuadOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::QuadInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Self::CubicIn => t * t * t,
            Self::CubicOut => 1.0 - (1.0 - t).powi(3),
            Self::CubicInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Self::SineIn => 1.0 - (t * std::f64::consts::FRAC_PI_2).cos(),
            Self::SineOut => (t * std::f64::consts::FRAC_PI_2).sin(),
            Self::SineInOut => -((std::f64::consts::PI * t).cos() - 1.0) / 2.0,
        }
    }
}
pub trait TweenBinding {
    type Target: Clone + Ord;
    fn read(&self, target: &Self::Target, property: &str) -> Result<TweenValue, String>;
    fn validate(
        &self,
        target: &Self::Target,
        property: &str,
        value: TweenValue,
    ) -> Result<(), String> {
        self.read(target, property)?.interpolate(value, 0.0)?;
        Ok(())
    }
    fn write(
        &mut self,
        target: &Self::Target,
        property: &str,
        value: TweenValue,
    ) -> Result<(), String>;
    fn interpolate(
        &self,
        _target: &Self::Target,
        _property: &str,
        start: TweenValue,
        end: TweenValue,
        factor: f64,
    ) -> Result<TweenValue, String> {
        start.interpolate(end, factor)
    }
}
#[derive(Clone, Debug)]
struct Tween<T> {
    target: T,
    property: String,
    start: TweenValue,
    end: TweenValue,
    time: f64,
    duration: f64,
    easing: TweenEasing,
}
#[derive(Clone, Debug)]
pub struct TweenScheduler<T: Clone + Ord> {
    active: BTreeMap<(T, String), Tween<T>>,
}
impl<T: Clone + Ord> Default for TweenScheduler<T> {
    fn default() -> Self {
        Self {
            active: BTreeMap::new(),
        }
    }
}
impl<T: Clone + Ord> TweenScheduler<T> {
    pub fn len(&self) -> usize {
        self.active.len()
    }
    pub fn cancel(&mut self, target: &T, property: &str) -> bool {
        self.active
            .remove(&(target.clone(), property.into()))
            .is_some()
    }
    pub fn create<B: TweenBinding<Target = T>>(
        &mut self,
        binding: &mut B,
        target: T,
        property: String,
        end: TweenValue,
        duration: f64,
        easing: TweenEasing,
    ) -> Result<(), String> {
        if !duration.is_finite() || duration < 0.0 {
            return Err("Tween duration must be finite and nonnegative".into());
        }
        if !end.valid() {
            return Err("Tween end value must be finite".into());
        }
        let start = binding.read(&target, &property)?;
        binding.validate(&target, &property, end)?;
        if duration == 0.0 {
            binding.write(&target, &property, end)?;
            self.cancel(&target, &property);
            return Ok(());
        }
        self.active.remove(&(target.clone(), property.clone()));
        self.active.insert(
            (target.clone(), property.clone()),
            Tween {
                target,
                property,
                start,
                end,
                time: 0.0,
                duration,
                easing,
            },
        );
        Ok(())
    }
    pub fn tick<B: TweenBinding<Target = T>>(&mut self, binding: &mut B, dt: f64) -> Vec<String> {
        if !dt.is_finite() || dt < 0.0 {
            return vec!["Tween dt must be finite and nonnegative".into()];
        }
        let mut failures = Vec::new();
        self.active.retain(|_, t| {
            t.time = (t.time + dt).min(t.duration);
            let progress = (t.time / t.duration).clamp(0.0, 1.0);
            let result = binding
                .interpolate(
                    &t.target,
                    &t.property,
                    t.start,
                    t.end,
                    t.easing.apply(progress),
                )
                .and_then(|v| binding.write(&t.target, &t.property, v));
            if let Err(err) = result {
                failures.push(err);
                return false;
            }
            progress < 1.0
        });
        failures
    }
}

fn slerp_euler_radians(start: [f64; 3], end: [f64; 3], t: f64) -> [f64; 3] {
    use glam::{EulerRot, Quat};
    let first = Quat::from_euler(
        EulerRot::XYZ,
        start[0] as f32,
        start[1] as f32,
        start[2] as f32,
    );
    let last = Quat::from_euler(EulerRot::XYZ, end[0] as f32, end[1] as f32, end[2] as f32);
    let (x, y, z) = first.slerp(last, t as f32).to_euler(EulerRot::XYZ);
    [x as f64, y as f64, z as f64]
}

pub struct HandleTweenBinding<'a> {
    pub host: &'a mut dyn crate::scripting::api::EngineHost,
}
impl TweenValue {
    pub fn from_script_value(value: &crate::scripting::value::Value) -> Result<Self, String> {
        use crate::scripting::value::Value;
        let result = match value {
            Value::Number(v) => Self::Scalar(*v),
            Value::Array(values) => {
                let borrowed = values.borrow();
                let numbers = borrowed
                    .elements
                    .iter()
                    .map(Value::as_number)
                    .collect::<Result<Vec<_>, _>>()?;
                match numbers.as_slice() {
                    [a, b] => Self::Vec2([*a, *b]),
                    [a, b, c] => Self::Vec3([*a, *b, *c]),
                    [a, b, c, d] => Self::Vec4([*a, *b, *c, *d]),
                    _ => return Err("Tween vectors must contain 2, 3 or 4 numbers".into()),
                }
            }
            _ => {
                return Err(format!(
                    "Tween property must be a number or numeric vector, got {}",
                    value.type_name()
                ));
            }
        };
        if !result.valid() {
            return Err("Tween value must be finite".into());
        }
        Ok(result)
    }
    pub fn as_script_value(self) -> crate::scripting::value::Value {
        use crate::scripting::value::Value;
        match self {
            Self::Scalar(x) => Value::Number(x),
            Self::Vec2(v) => Value::array(v.into_iter().map(Value::Number).collect()),
            Self::Vec3(v) => Value::array(v.into_iter().map(Value::Number).collect()),
            Self::Vec4(v) => Value::array(v.into_iter().map(Value::Number).collect()),
        }
    }
}
impl TweenBinding for HandleTweenBinding<'_> {
    type Target = (crate::scripting::value::HandleKind, u64);
    fn interpolate(
        &self,
        target: &Self::Target,
        property: &str,
        start: TweenValue,
        end: TweenValue,
        t: f64,
    ) -> Result<TweenValue, String> {
        if target.0 == crate::scripting::value::HandleKind::Actor && property == "rotation" {
            if let (TweenValue::Vec3(a), TweenValue::Vec3(b)) = (start, end) {
                if t <= 0.0 {
                    return Ok(start);
                }
                if t >= 1.0 {
                    return Ok(end);
                }
                return Ok(TweenValue::Vec3(slerp_euler_radians(a, b, t)));
            }
            return Err("Actor.rotation tween expects three Euler radians".into());
        }
        start.interpolate(end, t)
    }
    fn read(&self, target: &Self::Target, property: &str) -> Result<TweenValue, String> {
        if let Some(key) = property.strip_prefix("attributes.") {
            if key.is_empty() {
                return Err("Tween attribute key cannot be empty".into());
            }
            let value = self
                .host
                .get_handle_attribute(target.0, target.1, key)?
                .ok_or_else(|| format!("Missing numeric attribute {key}"))?;
            return TweenValue::from_script_value(&value);
        }
        let value = if target.0 == crate::scripting::value::HandleKind::Ui {
            self.host.get_ui_property(target.1, property)?
        } else {
            self.host.get_property(target.0, target.1, property)?
        }
        .ok_or_else(|| {
            format!(
                "{}.{} is unavailable on live target {}",
                target.0.name(),
                property,
                target.1
            )
        })?;
        TweenValue::from_script_value(&value)
    }
    fn write(
        &mut self,
        target: &Self::Target,
        property: &str,
        value: TweenValue,
    ) -> Result<(), String> {
        if let Some(key) = property.strip_prefix("attributes.") {
            if key.is_empty() {
                return Err("Tween attribute key cannot be empty".into());
            }
            let TweenValue::Scalar(n) = value else {
                return Err("Native attributes support primitive numbers, not vectors".into());
            };
            return self.host.set_handle_attribute(
                target.0,
                target.1,
                key.to_owned(),
                crate::scripting::value::Value::Number(n),
            );
        }
        let success = if target.0 == crate::scripting::value::HandleKind::Ui {
            self.host
                .set_ui_property(target.1, property, value.as_script_value())?
        } else {
            self.host
                .set_property(target.0, target.1, property, value.as_script_value())?
        };
        if !success {
            return Err(format!(
                "{}.{} is not a writable tween target",
                target.0.name(),
                property
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Store(BTreeMap<(u64, String), TweenValue>);
    impl TweenBinding for Store {
        type Target = u64;
        fn read(&self, t: &u64, p: &str) -> Result<TweenValue, String> {
            self.0
                .get(&(*t, p.into()))
                .copied()
                .ok_or_else(|| "Missing property".into())
        }
        fn validate(&self, t: &u64, p: &str, v: TweenValue) -> Result<(), String> {
            self.read(t, p)?.interpolate(v, 0.0)?;
            if p == "alpha" && !matches!(v,TweenValue::Scalar(x) if (0.0..=1.0).contains(&x)) {
                return Err("Alpha out of range".into());
            }
            Ok(())
        }
        fn write(&mut self, t: &u64, p: &str, v: TweenValue) -> Result<(), String> {
            self.read(t, p)?.interpolate(v, 0.0)?;
            if p == "alpha" && !matches!(v,TweenValue::Scalar(x) if (0.0..=1.0).contains(&x)) {
                return Err("Alpha out of range".into());
            }
            self.0.insert((*t, p.into()), v);
            Ok(())
        }
    }
    #[test]
    fn interpolates_scalar_and_vector() {
        let mut b = Store::default();
        b.0.insert((1, "alpha".into()), TweenValue::Scalar(0.0));
        b.0.insert((1, "position".into()), TweenValue::Vec3([0.0; 3]));
        let mut s = TweenScheduler::default();
        s.create(
            &mut b,
            1,
            "alpha".into(),
            TweenValue::Scalar(1.0),
            2.0,
            TweenEasing::QuadOut,
        )
        .unwrap();
        s.create(
            &mut b,
            1,
            "position".into(),
            TweenValue::Vec3([10.0, 20.0, 30.0]),
            2.0,
            TweenEasing::Linear,
        )
        .unwrap();
        assert!(s.tick(&mut b, 1.0).is_empty());
        assert_eq!(b.read(&1, "alpha").unwrap(), TweenValue::Scalar(0.75));
        assert_eq!(
            b.read(&1, "position").unwrap(),
            TweenValue::Vec3([5.0, 10.0, 15.0])
        );
        assert!(s.tick(&mut b, 1.0).is_empty());
        assert_eq!(s.len(), 0);
    }
    #[test]
    fn invalid_target_keeps_existing_tween() {
        let mut b = Store::default();
        b.0.insert((1, "alpha".into()), TweenValue::Scalar(0.0));
        let mut s = TweenScheduler::default();
        s.create(
            &mut b,
            1,
            "alpha".into(),
            TweenValue::Scalar(1.0),
            1.0,
            TweenEasing::Linear,
        )
        .unwrap();
        assert!(
            s.create(
                &mut b,
                1,
                "alpha".into(),
                TweenValue::Vec3([1.0; 3]),
                1.0,
                TweenEasing::Linear
            )
            .is_err()
        );
        assert!(
            s.create(
                &mut b,
                1,
                "alpha".into(),
                TweenValue::Scalar(f64::NAN),
                1.0,
                TweenEasing::Linear
            )
            .is_err()
        );
        assert!(
            s.create(
                &mut b,
                1,
                "alpha".into(),
                TweenValue::Scalar(2.0),
                1.0,
                TweenEasing::Linear
            )
            .is_err()
        );
        assert_eq!(s.len(), 1);
    }
    #[test]
    fn missing_binding_cancels_only_failed_tween() {
        let mut b = Store::default();
        for id in [1, 2] {
            b.0.insert((id, "alpha".into()), TweenValue::Scalar(0.0));
        }
        let mut s = TweenScheduler::default();
        for id in [1, 2] {
            s.create(
                &mut b,
                id,
                "alpha".into(),
                TweenValue::Scalar(1.0),
                1.0,
                TweenEasing::Linear,
            )
            .unwrap();
        }
        b.0.remove(&(1, "alpha".into()));
        assert_eq!(s.tick(&mut b, 0.5).len(), 1);
        assert_eq!(s.len(), 1);
    }
}

#[cfg(test)]
mod universal_tween_math_regressions {
    use super::*;
    #[test]
    fn shortest_path_rotation_across_zero_degrees() {
        let a = [0.0, 350_f64.to_radians(), 0.0];
        let b = [0.0, 10_f64.to_radians(), 0.0];
        let mid = slerp_euler_radians(a, b, 0.5);
        assert!(
            mid[1].sin().abs() < 0.002,
            "orientation midpoint: {:?}",
            mid
        );
        assert!(mid[1].cos() > 0.99, "midpoint must face zero degrees");
    }
    #[test]
    fn every_easing_has_exact_endpoints_and_is_monotonic() {
        for name in [
            "linear",
            "quadIn",
            "quadOut",
            "quadInOut",
            "cubicIn",
            "cubicOut",
            "cubicInOut",
            "sineIn",
            "sineOut",
            "sineInOut",
        ] {
            let easing = TweenEasing::parse(name).unwrap();
            assert!(easing.apply(0.0).abs() < 1e-12, "{name} first");
            assert!((easing.apply(1.0) - 1.0).abs() < 1e-12, "{name} last");
            let mut before = 0.0;
            for n in 1..=100 {
                let v = easing.apply(n as f64 / 100.0);
                assert!(
                    (0.0..=1.0).contains(&v) && v >= before - 1e-12,
                    "{name} non-monotonic at {n}: {v}"
                );
                before = v;
            }
        }
    }
}
