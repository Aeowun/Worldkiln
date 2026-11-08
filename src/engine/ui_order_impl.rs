// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
pub fn sort_ui_ids(
    mut ids: Vec<u32>,
    nodes: &HashMap<u32, (Option<u32>, f32)>,
    behavior: UiZIndexBehavior,
    legacy_z: impl Fn(u32) -> f32,
) -> Vec<u32> {
    fn ancestors(nodes: &HashMap<u32, (Option<u32>, f32)>, id: u32) -> Vec<(f32, u32)> {
        let mut path = Vec::new();
        let mut seen = HashSet::new();
        let mut current = Some(id);
        while let Some(id) = current {
            if !seen.insert(id) { break; }
            let Some(&(parent, z)) = nodes.get(&id) else { break; };
            path.push((z, id));
            current = parent;
        }
        path.reverse();
        path
    }
    ids.sort_by(|a, b| match behavior {
        UiZIndexBehavior::Legacy => legacy_z(*a).partial_cmp(&legacy_z(*b))
            .unwrap_or(Ordering::Equal).then_with(|| a.cmp(b)),
        UiZIndexBehavior::Global => {
            let az = nodes.get(a).map(|x| x.1).unwrap_or(0.0);
            let bz = nodes.get(b).map(|x| x.1).unwrap_or(0.0);
            az.partial_cmp(&bz).unwrap_or(Ordering::Equal).then_with(|| a.cmp(b))
        }
        UiZIndexBehavior::Sibling => {
            let left = ancestors(nodes, *a);
            let right = ancestors(nodes, *b);
            for (x, y) in left.iter().zip(&right) {
                let cmp = x.0.partial_cmp(&y.0).unwrap_or(Ordering::Equal)
                    .then_with(|| x.1.cmp(&y.1));
                if cmp != Ordering::Equal { return cmp; }
            }
            left.len().cmp(&right.len())
        }
    });
    ids
}

impl RuntimeUi {
    pub fn paint_order(&self, viewport: [f32; 2]) -> Vec<u32> {
        let ids = self.elements.keys().copied()
            .filter(|id| self.resolve_layout_in(*id as u64, viewport)
                .is_ok_and(|layout| layout.visible))
            .collect();
        let nodes: HashMap<u32, (Option<u32>, f32)> = self.elements.iter()
            .map(|(id, value)| (*id, (value.common().parent, value.common().z_index)))
            .collect();
        sort_ui_ids(ids, &nodes, self.z_index_behavior, |id|
            self.resolve_layout_in(id as u64, viewport)
                .map(|layout| layout.z_index).unwrap_or(0.0))
    }
}

#[cfg(test)]
mod z_index_behavior_tests {
    use super::*;

    #[test]
    fn global_z_overrides_insertion_and_parenting() {
        let mut ui = RuntimeUi::new();
        let menu = ui.allocate("Panel").unwrap();
        let black = ui.allocate("Panel").unwrap();
        ui.set_property(menu, "z_index", Value::Number(300.0)).unwrap();
        ui.set_property(black, "z_index", Value::Number(200.0)).unwrap();
        ui.z_index_behavior = UiZIndexBehavior::Global;
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![black as u32, menu as u32]);
        ui.set_property(black, "z_index", Value::Number(1000.0)).unwrap();
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![menu as u32, black as u32]);
        ui.set_property(black, "visible", Value::Bool(false)).unwrap();
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![menu as u32]);
        ui.set_property(black, "visible", Value::Bool(true)).unwrap();
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![menu as u32, black as u32]);
    }

    #[test]
    fn siblings_keep_descendants_inside_parent_group() {
        let mut ui = RuntimeUi::new();
        let bottom = ui.allocate("Panel").unwrap();
        let top = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Text").unwrap();
        ui.set_property(bottom, "z_index", Value::Number(10.0)).unwrap();
        ui.set_property(top, "z_index", Value::Number(20.0)).unwrap();
        ui.set_property(child, "z_index", Value::Number(1000.0)).unwrap();
        ui.set_property(child, "parent", Value::Number(bottom as f64)).unwrap();
        ui.z_index_behavior = UiZIndexBehavior::Sibling;
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![bottom as u32, child as u32, top as u32]);
        ui.z_index_behavior = UiZIndexBehavior::Global;
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![bottom as u32, top as u32, child as u32]);
    }

    #[test]
    fn child_text_in_a_button_paints_above_button_even_when_local_z_is_lower() {
        let mut ui = RuntimeUi::new();
        let button = ui.allocate("Button").unwrap();
        let label = ui.allocate("Text").unwrap();
        let overlay = ui.allocate("Panel").unwrap();
        ui.set_property(button, "z_index", Value::Number(20.0)).unwrap();
        ui.set_property(label, "parent", Value::Number(button as f64)).unwrap();
        ui.set_property(label, "z_index", Value::Number(-10.0)).unwrap();
        ui.set_property(overlay, "z_index", Value::Number(10.0)).unwrap();
        ui.z_index_behavior = UiZIndexBehavior::Sibling;
        assert_eq!(ui.paint_order([900.0, 600.0]),
            vec![overlay as u32, button as u32, label as u32]);
    }

    #[test]
    fn hidden_parent_hides_children_without_changing_stack() {
        let mut ui = RuntimeUi::new();
        let root = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Button").unwrap();
        ui.set_property(child, "parent", Value::Number(root as f64)).unwrap();
        ui.z_index_behavior = UiZIndexBehavior::Sibling;
        assert_eq!(ui.paint_order([900.0, 600.0]), vec![root as u32, child as u32]);
        ui.set_property(root, "visible", Value::Bool(false)).unwrap();
        assert!(ui.paint_order([900.0, 600.0]).is_empty());
    }
}
