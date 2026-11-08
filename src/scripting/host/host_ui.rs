// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::scripting::value::Value;

use super::ScriptHostBridge;

impl<'a> ScriptHostBridge<'a> {
    pub fn create_ui_element(&mut self, element_type: &str) -> Result<u64, String> {
        self.runtime_ui.allocate(element_type)
    }

    pub fn delete_ui_element(&mut self, id: u64) -> Result<(), String> {
        self.runtime_ui.delete(id)
    }

    pub fn get_ui_property(&self, id: u64, name: &str) -> Result<Option<Value>, String> {
        self.runtime_ui.get_property(id, name)
    }

    pub fn set_ui_property(&mut self, id: u64, name: &str, value: Value) -> Result<bool, String> {
        self.world
            .runtime_property_tweens
            .cancel(&(crate::scripting::value::HandleKind::Ui, id), name);
        self.runtime_ui.set_property(id, name, value)
    }

    pub fn drain_ui_clicks(&mut self) -> Vec<u64> {
        self.runtime_ui.drain_pending_clicks()
    }

    pub fn get_ui_symbolic_event_dispatch(
        &self,
        id: u64,
        event: &str,
    ) -> Option<crate::engine::ui::UiSymbolicEventDispatch> {
        self.runtime_ui.symbolic_event_dispatch(id, event)
    }
}
