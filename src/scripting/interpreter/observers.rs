// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::Interpreter;
use crate::scripting::api::HostContext;
use crate::scripting::value::{HandleKind, Value};

const MAX_OBSERVERS: usize = 1024;

#[derive(Clone, Debug)]
pub(super) struct PropertyObserver {
    pub kind: HandleKind,
    pub id: u64,
    pub property: String,
    pub callback: Value,
    pub previous: Value,
    pub owner_script: Option<String>,
}

fn observable_is_alive(kind: HandleKind, id: u64, host: &mut HostContext<'_>) -> bool {
    match kind {
        HandleKind::Actor => host.engine.actor_exists(id),
        HandleKind::Cell | HandleKind::Light => host.engine.cell_exists(id),
        HandleKind::Entity => host.engine.entity_exists(id),
        _ => false,
    }
}

fn read_observable(
    kind: HandleKind,
    id: u64,
    property: &str,
    host: &mut HostContext<'_>,
) -> Result<Option<Value>, String> {
    if !observable_is_alive(kind, id, host) {
        return Ok(None);
    }
    if let Some(value) = host.engine.get_property(kind, id, property)? {
        return Ok(Some(value));
    }
    Ok(host
        .engine
        .get_script_property(kind, id, property)
        .map(|prop| prop.value))
}

fn snapshot_value(value: &Value, depth: usize) -> Option<Value> {
    if depth > 6 {
        return None;
    }
    match value {
        Value::Array(items) => {
            let contents = items.try_borrow().ok()?;
            if contents.elements.len() > 128 {
                return None;
            }
            let copied = contents
                .elements
                .iter()
                .map(|item| snapshot_value(item, depth + 1))
                .collect::<Option<Vec<_>>>()?;
            Some(Value::array(copied))
        }
        Value::Map(items) => {
            let contents = items.try_borrow().ok()?;
            if contents.len() > 128 {
                return None;
            }
            let copied = contents
                .iter()
                .map(|(key, value)| Some((key.clone(), snapshot_value(value, depth + 1)?)))
                .collect::<Option<std::collections::BTreeMap<_, _>>>()?;
            Some(Value::map(copied))
        }
        _ => Some(value.clone()),
    }
}

fn canonical_owner(path: &str) -> String {
    let normal = path.replace('\\', "/");
    if normal.starts_with("scripts/")
        || normal.starts_with("playerscripts/")
        || normal.starts_with("controllers/")
        || normal.starts_with("cameras/")
    {
        normal
    } else {
        format!("scripts/{normal}")
    }
}

impl Interpreter {
    pub(crate) fn observe_property(
        &mut self,
        kind: HandleKind,
        id: u64,
        property: &str,
        callback: Value,
        owner: Option<&str>,
        host: &mut HostContext<'_>,
    ) -> Result<u64, String> {
        if !matches!(
            kind,
            HandleKind::Actor | HandleKind::Cell | HandleKind::Entity | HandleKind::Light
        ) {
            return Err("observe.property requires an Actor, Cell, Light, or Entity handle".into());
        }
        if property.is_empty() || property.len() > 64 || property.chars().any(char::is_control) {
            return Err("observe.property name must have 1..64 characters without controls".into());
        }
        if !matches!(callback, Value::Function(..)) {
            return Err("observe.property callback must be a function".into());
        }
        if self.property_observers.len() >= MAX_OBSERVERS {
            return Err("observe.property exceeded the 1024 active subscriptions limit".into());
        }
        let initial = read_observable(kind, id, property, host)?.ok_or_else(|| {
            format!("observe.property cannot read '{property}' on missing or unsupported handle")
        })?;
        let initial = snapshot_value(&initial, 0)
            .ok_or("observe.property cannot watch oversized or cyclic collections")?;
        let token = self.next_observer_id;
        self.next_observer_id = self
            .next_observer_id
            .checked_add(1)
            .ok_or("observe.property subscription ID exhausted")?;
        self.property_observers.insert(
            token,
            PropertyObserver {
                kind,
                id,
                property: property.to_owned(),
                callback,
                previous: initial,
                owner_script: owner.map(canonical_owner),
            },
        );
        Ok(token)
    }

    pub(crate) fn cancel_observer(&mut self, token: u64) -> bool {
        self.property_observers.remove(&token).is_some()
    }

    pub(crate) fn clear_observers(&mut self) {
        self.property_observers.clear();
    }

    pub(crate) fn cancel_observers_for_script(&mut self, script: &str) {
        let owner = canonical_owner(script);
        self.property_observers
            .retain(|_, observer| observer.owner_script.as_deref() != Some(owner.as_str()));
    }

    pub(crate) fn poll_observers(
        &mut self,
        host: &mut HostContext<'_>,
    ) -> Vec<(Value, Value, Value)> {
        let mut callbacks = Vec::new();
        let mut stale = Vec::new();
        for (token, watch) in self.property_observers.iter_mut() {
            match read_observable(watch.kind, watch.id, &watch.property, host) {
                Ok(Some(current)) => {
                    let Some(current) = snapshot_value(&current, 0) else {
                        stale.push(*token);
                        continue;
                    };
                    if current != watch.previous {
                        let old = std::mem::replace(&mut watch.previous, current.clone());
                        callbacks.push((watch.callback.clone(), old, current));
                    }
                }
                Ok(None) | Err(_) => stale.push(*token),
            }
        }
        for token in stale {
            self.property_observers.remove(&token);
        }
        callbacks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entity::EntityManager;
    use crate::scripting::ast::Program;
    use crate::scripting::source::SourceSpan;

    #[test]
    fn invalid_observers_cannot_register_without_a_live_engine_handle() {
        let program = Program {
            span: SourceSpan::new(0, 0),
            declarations: vec![],
            statements: vec![],
        };
        let mut interpreter = Interpreter::new(program);
        let mut host_impl = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut host_impl,
        };
        let err = interpreter
            .observe_property(HandleKind::Actor, 1, "visible", Value::Nil, None, &mut host)
            .unwrap_err();
        assert!(err.contains("callback"));
        assert!(!interpreter.cancel_observer(123));
        assert!(interpreter.poll_observers(&mut host).is_empty());
    }

    #[test]
    fn observer_tracks_changes_and_drops_deleted_actors() {
        use crate::scripting::ast::Declaration;
        use crate::scripting::interpreter::interpreter_compile::compile_function;
        use crate::scripting::lexer::Lexer;
        use crate::scripting::parser::Parser;
        use crate::scripting::scene::tests::test_host;
        use std::sync::Arc;

        let source = "fn on_changed(before, after) { }";
        let parsed = Parser::new(Lexer::new(source).tokenize().unwrap())
            .parse()
            .unwrap();
        let callback = match &parsed.declarations[0] {
            Declaration::Function(declaration) => Value::Function(
                Arc::new(compile_function(declaration)),
                vec!["before".into(), "after".into()],
                vec![],
            ),
            _ => panic!("Expected a callback function"),
        };
        let mut interpreter = Interpreter::new(parsed);
        let mut engine = test_host();
        let id = engine.world.add_actor("Crate", glam::Vec3::ZERO);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut engine,
        };
        let token = interpreter
            .observe_property(
                HandleKind::Actor,
                id,
                "visible",
                callback,
                Some("scripts/watcher.aeo"),
                &mut host,
            )
            .unwrap();
        assert_eq!(token, 1);
        assert_eq!(interpreter.poll_observers(&mut host).len(), 0);
        host.engine
            .set_property(HandleKind::Actor, id, "visible", Value::Bool(false))
            .unwrap();
        let events = interpreter.poll_observers(&mut host);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].1, Value::Bool(true));
        assert_eq!(events[0].2, Value::Bool(false));
        assert!(interpreter.poll_observers(&mut host).is_empty());
        interpreter.cancel_observers_for_script("scripts/watcher.aeo");
        assert!(!interpreter.cancel_observer(token));

        let second = interpreter
            .observe_property(
                HandleKind::Actor,
                id,
                "visible",
                events[0].0.clone(),
                Some("scripts/watcher.aeo"),
                &mut host,
            )
            .unwrap();
        assert_eq!(second, 2);
        host.engine
            .set_property(HandleKind::Actor, id, "visible", Value::Bool(true))
            .unwrap();
        drop(host);
        engine.world.remove_actor(id);
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut engine,
        };
        assert!(interpreter.poll_observers(&mut host).is_empty());
        assert!(!interpreter.cancel_observer(second));
    }

    #[test]
    fn observer_snapshots_in_place_basket_and_map_updates() {
        use crate::scripting::value::MapKey;
        let live = Value::array(vec![Value::Number(1.0)]);
        let before = snapshot_value(&live, 0).unwrap();
        live.as_basket().unwrap().borrow_mut().elements[0] = Value::Number(2.0);
        assert_ne!(before, live);
        assert_eq!(
            before.as_basket().unwrap().borrow().elements[0],
            Value::Number(1.0)
        );

        let mut entries = std::collections::BTreeMap::new();
        entries.insert(MapKey::String("score".into()), live.clone());
        let map = Value::map(entries);
        let old_map = snapshot_value(&map, 0).unwrap();
        live.as_basket().unwrap().borrow_mut().elements[0] = Value::Number(3.0);
        assert_ne!(old_map, map);
        assert!(snapshot_value(&Value::array(vec![Value::Nil; 129]), 0).is_none());
    }
}
