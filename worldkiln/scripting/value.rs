use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Kinds of opaque engine handles.
///
/// Handles are intentionally lightweight. The scripting runtime does not own
/// the underlying engine object; it only carries an identifier that the host
/// runtime can resolve later.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HandleKind {
    Actor,
    Entity,
    EntityPhysics,
    EntityAppearance,
    EntityAnimation,
    Light,
    Cell,
    Ui,
    Sound,
    Mouse,
}

impl HandleKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Actor => "Actor",
            Self::Entity => "Entity",
            Self::EntityPhysics => "EntityPhysics",
            Self::EntityAppearance => "EntityAppearance",
            Self::EntityAnimation => "EntityAnimation",
            Self::Light => "Light",
            Self::Cell => "Cell",
            Self::Ui => "Ui",
            Self::Sound => "Sound",
            Self::Mouse => "Mouse",
        }
    }
}

/// A dynamic property assigned to an engine handle in script.
#[derive(Clone, Debug, PartialEq)]
pub struct DynamicProperty {
    pub value: Value,
    pub owner_script: Option<String>,
}

/// A wrapper for f64 that implements Ord and Hash using total_cmp.
#[derive(Clone, Copy, Debug)]
pub struct OrderedFloat(pub f64);

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == std::cmp::Ordering::Equal
    }
}

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.0.total_cmp(&other.0))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for OrderedFloat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.0.to_bits());
    }
}

/// A key in an AeoScript map.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKey {
    Number(OrderedFloat),
    String(String),
}

impl std::fmt::Display for MapKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => {
                if n.0.fract() == 0.0 {
                    write!(f, "{:.0}", n.0)
                } else {
                    write!(f, "{}", n.0)
                }
            }
            Self::String(s) => write!(f, "{}", s),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BasketData {
    pub elements: Vec<Value>,
    pub frozen: bool,
}

/// Values that can exist inside the AeoScript runtime.
#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    Bool(bool),
    String(String),
    Nil,
    Array(Arc<RefCell<BasketData>>),
    Map(Arc<RefCell<BTreeMap<MapKey, Value>>>),

    /// Opaque engine object reference.
    ///
    /// The scripting VM never receives a pointer or a Rust object directly.
    Handle {
        kind: HandleKind,
        id: u64,
    },

    /// A reference to a function.
    /// Stores the compiled instructions, the names of its parameters, and captured lexical scopes.
    Function(
        Arc<crate::scripting::ast::CompiledFunction>,
        Vec<String>,
        Vec<Scope>,
    ),

    /// Represents a built-in library namespace (e.g. math, cell).
    Namespace(String),

    /// Reference to a script-defined object instance (`entity`).
    Object(Arc<RefCell<crate::scripting::interpreter::ScriptInstance>>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Number(_) => "number",
            Self::Bool(_) => "bool",
            Self::String(_) => "string",
            Self::Nil => "nil",
            Self::Array(_) => "basket",
            Self::Map(_) => "map",
            Self::Handle { .. } => "handle",
            Self::Function(..) => "function",
            Self::Namespace(_) => "namespace",
            Self::Object(_) => "object",
        }
    }

    /// AeoScript conditions are strict for booleans but nullable values are
    /// naturally useful in gameplay code (`if light { }` becomes true for a valid handle and false for nil).
    pub fn is_truthy(&self) -> Result<bool, String> {
        match self {
            Self::Bool(value) => Ok(*value),
            Self::Nil => Ok(false),
            Self::Handle { .. } => Ok(true),
            Self::Object(_) => Ok(true),
            other => Err(format!(
                "expected bool or optional value in condition, got {}",
                other.type_name()
            )),
        }
    }

    pub fn as_number(&self) -> Result<f64, String> {
        match self {
            Self::Number(value) => Ok(*value),
            other => Err(format!("expected number, got {}", other.type_name())),
        }
    }

    pub fn as_index(&self) -> Result<usize, String> {
        let number = self.as_number()?;
        if number.fract() != 0.0 || number < 0.0 {
            return Err("array index must be a non-negative integer.".to_string());
        }
        Ok(number as usize)
    }

    pub fn as_bool(&self) -> Result<bool, String> {
        match self {
            Self::Bool(value) => Ok(*value),
            other => Err(format!("expected bool, got {}", other.type_name())),
        }
    }

    pub fn as_string(&self) -> Result<&str, String> {
        match self {
            Self::String(value) => Ok(value.as_str()),
            other => Err(format!("expected string, got {}", other.type_name())),
        }
    }

    pub fn as_basket(&self) -> Result<Arc<RefCell<BasketData>>, String> {
        match self {
            Self::Array(value) => Ok(value.clone()),
            other => Err(format!("expected basket, got {}", other.type_name())),
        }
    }

    pub fn as_map(&self) -> Result<Arc<RefCell<BTreeMap<MapKey, Value>>>, String> {
        match self {
            Self::Map(value) => Ok(value.clone()),
            other => Err(format!("expected map, got {}", other.type_name())),
        }
    }

    pub fn as_handle(&self) -> Result<(HandleKind, u64), String> {
        match self {
            Self::Handle { kind, id } => Ok((*kind, *id)),
            other => Err(format!("expected engine handle, got {}", other.type_name())),
        }
    }

    pub fn as_map_key(&self) -> Result<MapKey, String> {
        match self {
            Self::Number(n) => {
                if n.is_nan() {
                    return Err("NaN cannot be used as a map key".to_string());
                }
                let normalized = if *n == 0.0 { 0.0 } else { *n };
                Ok(MapKey::Number(OrderedFloat(normalized)))
            }
            Self::String(s) => Ok(MapKey::String(s.clone())),
            other => Err(format!(
                "{} cannot be used as a map key (expected string or number)",
                other.type_name()
            )),
        }
    }

    pub fn array(values: Vec<Value>) -> Self {
        Self::Array(Arc::new(RefCell::new(BasketData {
            elements: values,
            frozen: false,
        })))
    }

    pub fn map(values: BTreeMap<MapKey, Value>) -> Self {
        Self::Map(Arc::new(RefCell::new(values)))
    }

    pub fn display_string(&self) -> String {
        match self {
            Self::Number(value) => {
                if value.fract() == 0.0 {
                    format!("{:.0}", value)
                } else {
                    value.to_string()
                }
            }

            Self::Bool(value) => value.to_string(),

            Self::String(value) => value.clone(),

            Self::Nil => "nil".to_string(),

            Self::Array(values) => {
                let borrowed = values.borrow();
                let items = borrowed
                    .elements
                    .iter()
                    .map(Value::display_string)
                    .collect::<Vec<_>>()
                    .join(", ");

                format!("[{}]", items)
            }

            Self::Map(values) => {
                let borrowed = values.borrow();
                let items = borrowed
                    .iter()
                    .map(|(key, value)| format!("{}: {}", key, value.display_string()))
                    .collect::<Vec<_>>()
                    .join(", ");

                format!("{{{}}}", items)
            }

            Self::Handle { kind, id } => {
                format!("{}#{}", kind.name(), id)
            }

            Self::Function(..) => "function".to_string(),

            Self::Namespace(name) => format!("namespace:{}", name),

            Self::Object(inst) => format!("object:{}", inst.borrow().entity_name()),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Nil, Self::Nil) => true,
            (Self::Array(a), Self::Array(b)) => {
                if Arc::ptr_eq(a, b) {
                    return true;
                }
                *a.borrow() == *b.borrow()
            }
            (Self::Map(a), Self::Map(b)) => {
                if Arc::ptr_eq(a, b) {
                    return true;
                }
                *a.borrow() == *b.borrow()
            }

            (
                Self::Handle {
                    kind: left_kind,
                    id: left_id,
                },
                Self::Handle {
                    kind: right_kind,
                    id: right_id,
                },
            ) => left_kind == right_kind && left_id == right_id,

            (
                Self::Function(a_compiled, a_params, a_caps),
                Self::Function(b_compiled, b_params, b_caps),
            ) => Arc::ptr_eq(a_compiled, b_compiled) && a_params == b_params && a_caps == b_caps,

            (Self::Namespace(a), Self::Namespace(b)) => a == b,
            (Self::Object(a), Self::Object(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Value {}

impl std::fmt::Display for Value {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.display_string())
    }
}

/// A single lexical scope.
#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub(crate) values: Arc<RefCell<BTreeMap<String, Value>>>,
    pub(crate) constants: Arc<RefCell<BTreeMap<String, ()>>>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            values: Arc::new(RefCell::new(BTreeMap::new())),
            constants: Arc::new(RefCell::new(BTreeMap::new())),
        }
    }

    pub fn declare(
        &self,
        name: impl Into<String>,
        value: Value,
        is_const: bool,
    ) -> Result<(), String> {
        let name = name.into();
        let mut values = self.values.borrow_mut();

        if values.contains_key(&name) {
            return Err(format!(
                "variable '{}' is already declared in this scope",
                name
            ));
        }

        values.insert(name.clone(), value);

        if is_const {
            self.constants.borrow_mut().insert(name, ());
        }

        Ok(())
    }

    pub fn set_or_declare(&self, name: String, value: Value) -> Result<(), String> {
        self.values.borrow_mut().insert(name, value);
        Ok(())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.values.borrow().contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        self.values.borrow().get(name).cloned()
    }

    pub fn set(&self, name: &str, value: Value) -> Result<(), String> {
        if self.constants.borrow().contains_key(name) {
            return Err(format!("cannot assign to const variable '{}'", name));
        }

        let mut values = self.values.borrow_mut();
        match values.get_mut(name) {
            Some(existing) => {
                *existing = value;
                Ok(())
            }

            None => Err(format!("variable '{}' does not exist in this scope", name)),
        }
    }

    pub fn is_const(&self, name: &str) -> bool {
        self.constants.borrow().contains_key(name)
    }
}

impl PartialEq for Scope {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.values, &other.values)
    }
}

impl Eq for Scope {}

#[cfg(test)]
mod tests {
    use super::{HandleKind, MapKey, OrderedFloat, Scope, Value};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    #[test]
    fn value_types_are_reported() {
        assert_eq!(Value::Number(10.0).type_name(), "number");

        assert_eq!(Value::Bool(true).type_name(), "bool");

        assert_eq!(Value::String("hello".to_string()).type_name(), "string");

        assert_eq!(Value::Nil.type_name(), "nil");

        assert_eq!(Value::array(vec![Value::Number(1.0)]).type_name(), "basket");

        assert_eq!(
            Value::Map(Arc::new(RefCell::new(BTreeMap::new()))).type_name(),
            "map"
        );

        assert_eq!(
            Value::Handle {
                kind: HandleKind::Light,
                id: 7,
            }
            .type_name(),
            "handle"
        );
    }

    #[test]
    fn handle_values_display_cleanly() {
        let value = Value::Handle {
            kind: HandleKind::Light,
            id: 42,
        };

        assert_eq!(value.display_string(), "Light#42");

        assert_eq!(
            value.as_handle().expect("handle should resolve"),
            (HandleKind::Light, 42)
        );
    }

    #[test]
    fn nil_is_falsey() {
        assert_eq!(Value::Nil.is_truthy().expect("nil should be valid"), false);
    }

    #[test]
    fn handles_are_truthy() {
        let value = Value::Handle {
            kind: HandleKind::Entity,
            id: 1,
        };

        assert_eq!(value.is_truthy().expect("handle should be valid"), true);
    }

    #[test]
    fn non_boolean_non_optional_values_are_rejected_as_conditions() {
        assert!(Value::Number(1.0).is_truthy().is_err());

        assert!(Value::String("yes".to_string()).is_truthy().is_err());
    }

    #[test]
    fn arrays_and_maps_display() {
        let array = Value::array(vec![
            Value::Number(1.0),
            Value::Bool(true),
            Value::String("x".to_string()),
        ]);

        assert_eq!(array.display_string(), "[1, true, x]");

        let mut map = BTreeMap::new();

        map.insert(MapKey::String("hp".to_string()), Value::Number(100.0));

        map.insert(MapKey::String("alive".to_string()), Value::Bool(true));

        let map = Value::Map(Arc::new(RefCell::new(map)));

        assert_eq!(map.display_string(), "{alive: true, hp: 100}");
    }

    #[test]
    fn scope_declares_and_updates_variables() {
        let mut scope = Scope::new();

        scope
            .declare("value", Value::Number(10.0), false)
            .expect("variable should declare");

        assert!(scope.contains("value"));

        assert_eq!(scope.get("value"), Some(Value::Number(10.0)));

        scope
            .set("value", Value::Number(15.0))
            .expect("variable should update");

        assert_eq!(scope.get("value"), Some(Value::Number(15.0)));
    }

    #[test]
    fn const_variables_cannot_change() {
        let mut scope = Scope::new();

        scope
            .declare("limit", Value::Number(10.0), true)
            .expect("const should declare");

        let result = scope.set("limit", Value::Number(20.0));

        assert!(result.is_err());
        assert!(scope.is_const("limit"));

        assert_eq!(scope.get("limit"), Some(Value::Number(10.0)));
    }

    #[test]
    fn duplicate_declarations_are_rejected() {
        let mut scope = Scope::new();

        scope
            .declare("value", Value::Number(10.0), false)
            .expect("first declaration should work");

        let result = scope.declare("value", Value::Number(20.0), false);

        assert!(result.is_err());
    }

    #[test]
    fn map_keys_distinct() {
        let mut map = BTreeMap::new();
        map.insert(
            MapKey::Number(OrderedFloat(1.0)),
            Value::String("number".to_string()),
        );
        map.insert(
            MapKey::String("1".to_string()),
            Value::String("string".to_string()),
        );

        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get(&MapKey::Number(OrderedFloat(1.0)))
                .unwrap()
                .as_string()
                .unwrap(),
            "number"
        );
    }

    #[test]
    fn map_key_normalizes_zeros_and_rejects_nan() {
        let positive_zero = Value::Number(0.0).as_map_key().unwrap();
        let negative_zero = Value::Number(-0.0).as_map_key().unwrap();
        assert_eq!(positive_zero, negative_zero);

        let nan_key = Value::Number(f64::NAN).as_map_key();
        assert!(nan_key.is_err());
    }

    #[test]
    fn test_numeric_equality() {
        assert_eq!(Value::Number(0.0), Value::Number(-0.0));
        assert_eq!(Value::Number(1.0), Value::Number(1.0));
        assert_ne!(Value::Number(1.0), Value::Number(2.0));
    }

    #[test]
    fn entity_capability_handles_have_distinct_kinds() {
        let physics = Value::Handle {
            kind: HandleKind::EntityPhysics,
            id: 7,
        };

        let appearance = Value::Handle {
            kind: HandleKind::EntityAppearance,
            id: 7,
        };

        let animation = Value::Handle {
            kind: HandleKind::EntityAnimation,
            id: 7,
        };

        assert_ne!(physics, appearance);
        assert_ne!(physics, animation);
        assert_ne!(appearance, animation);

        assert_eq!(physics.display_string(), "EntityPhysics#7");
        assert_eq!(appearance.display_string(), "EntityAppearance#7");
        assert_eq!(animation.display_string(), "EntityAnimation#7");
    }
}
