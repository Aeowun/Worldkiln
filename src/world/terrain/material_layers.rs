// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{TerrainConfig, TerrainState};
use crate::world::cell::DEFAULT_SURFACE_FRICTION;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainMaterialLayer {
    pub id: String,
    pub name: String,
    pub material: String,
    #[serde(default = "default_layer_friction")]
    pub friction: f32,
}
fn default_layer_friction() -> f32 {
    DEFAULT_SURFACE_FRICTION
}
pub(crate) fn default_shore_layer() -> String {
    "subsurface".into()
}
pub(crate) fn legacy_shore_layer() -> String {
    "legacy-sand".into()
}

impl TerrainConfig {
    pub fn terrain_material_layers(&self) -> Vec<TerrainMaterialLayer> {
        if !self.material_layers.is_empty() {
            return self.material_layers.clone();
        }
        let mut layers = vec![
            TerrainMaterialLayer {
                id: "surface".into(),
                name: "Surface".into(),
                material: self.materials.surface.clone(),
                friction: DEFAULT_SURFACE_FRICTION,
            },
            TerrainMaterialLayer {
                id: "subsurface".into(),
                name: "Subsurface".into(),
                material: self.materials.subsurface.clone(),
                friction: DEFAULT_SURFACE_FRICTION,
            },
            TerrainMaterialLayer {
                id: "rock".into(),
                name: "Rock".into(),
                material: self.materials.rock.clone(),
                friction: DEFAULT_SURFACE_FRICTION,
            },
        ];
        if self.shore_material_layer == "legacy-sand" {
            layers.push(TerrainMaterialLayer {
                id: "legacy-sand".into(),
                name: "Sand".into(),
                material: "PBR:Sand".into(),
                friction: DEFAULT_SURFACE_FRICTION,
            });
        }
        layers
    }
    pub fn ensure_material_layers(&mut self) {
        if self.material_layers.is_empty() {
            self.material_layers = self.terrain_material_layers();
        }
    }
    pub fn set_layer_friction(&mut self, id: &str, value: f32) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err("Material layer friction must be between 0 and 100".into());
        }
        self.ensure_material_layers();
        let resolved = self.resolved_layer_id(id).to_owned();
        let layer = self
            .material_layers
            .iter_mut()
            .find(|layer| layer.id == resolved)
            .ok_or_else(|| format!("Unknown material layer: {id}"))?;
        layer.friction = value;
        Ok(())
    }

    pub fn layer_friction(&self, id: &str) -> f32 {
        let resolved = self.resolved_layer_id(id);
        self.terrain_material_layers()
            .iter()
            .find(|layer| layer.id == resolved)
            .map(|layer| {
                if layer.friction.is_finite() {
                    layer.friction.clamp(0.0, 100.0)
                } else {
                    DEFAULT_SURFACE_FRICTION
                }
            })
            .unwrap_or(DEFAULT_SURFACE_FRICTION)
    }

    pub fn layer_material(&self, id: &str) -> &str {
        let id = self.resolved_layer_id(id);
        if !self.material_layers.is_empty() {
            return self
                .material_layers
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.material.as_str())
                .unwrap_or("");
        }
        match id {
            "surface" => &self.materials.surface,
            "subsurface" => &self.materials.subsurface,
            "rock" => &self.materials.rock,
            "legacy-sand" => "PBR:Sand",
            _ => "",
        }
    }
    pub fn resolved_layer_id<'a>(&'a self, id: &'a str) -> &'a str {
        self.material_role_replacements
            .get(id)
            .map(String::as_str)
            .unwrap_or(id)
    }
    pub fn resolved_generation_materials(&self) -> super::TerrainMaterials {
        super::TerrainMaterials {
            surface: self.layer_material("surface").into(),
            subsurface: self.layer_material("subsurface").into(),
            rock: self.layer_material("rock").into(),
            water: self.materials.water.clone(),
        }
    }
    pub fn geometry_material_roles(&self) -> super::TerrainMaterials {
        if self.material_layers.is_empty() {
            return self.materials.clone();
        }
        super::TerrainMaterials {
            surface: "Surface".into(),
            subsurface: "Subsurface".into(),
            rock: "Rock".into(),
            water: self.materials.water.clone(),
        }
    }
    pub fn add_material_layer(&mut self) -> String {
        self.ensure_material_layers();
        let id = format!("{:032x}", rand::random::<u128>());
        self.material_layers.push(TerrainMaterialLayer {
            id: id.clone(),
            name: format!("Layer {}", self.material_layers.len() + 1),
            material: String::new(),
            friction: DEFAULT_SURFACE_FRICTION,
        });
        id
    }
    pub fn move_material_layer(&mut self, id: &str, delta: isize) {
        self.ensure_material_layers();
        if let Some(index) = self.material_layers.iter().position(|l| l.id == id) {
            let target = index
                .saturating_add_signed(delta)
                .min(self.material_layers.len() - 1);
            self.material_layers.swap(index, target);
        }
    }
    pub fn paint_layer_id(&self, reference: &str) -> Option<String> {
        if let Some(id) = reference.strip_prefix("layer:") {
            let id = self.resolved_layer_id(id);
            return self
                .terrain_material_layers()
                .iter()
                .any(|l| l.id == id)
                .then(|| id.to_owned());
        }
        let key = reference.trim_start_matches("PBR:").to_ascii_lowercase();
        let id = match key.as_str() {
            "grass" | "surface" | "grass_1" => Some("surface".into()),
            "subsurface" => Some("subsurface".into()),
            "rock" | "tile_24" => Some("rock".into()),
            "sand" | "beach" => Some("legacy-sand".into()),
            "dirt" => Some("subsurface".into()),
            _ => self
                .terrain_material_layers()
                .iter()
                .find(|l| l.material == reference)
                .map(|l| l.id.clone()),
        };
        id.map(|id| self.resolved_layer_id(&id).to_owned())
    }
    pub fn migrate_material_layers(&mut self) {
        self.ensure_material_layers();
        let mut references: Vec<String> = self
            .native_base_paint_strokes
            .iter()
            .map(|r| r.material.clone())
            .collect();
        for edit in self.generation_layers.iter().flat_map(|l| &l.brush_edits) {
            if let super::LayerBrushEdit::Paint { material, .. } = edit {
                references.push(material.clone());
            }
        }
        for reference in references {
            if reference.starts_with("layer:") {
                continue;
            }
            let id = self
                .paint_layer_id(&reference)
                .unwrap_or_else(|| format!("legacy-{:032x}", rand::random::<u128>()));
            if !self.material_layers.iter().any(|l| l.id == id) {
                self.material_layers.push(TerrainMaterialLayer {
                    id,
                    name: reference.trim_start_matches("PBR:").into(),
                    material: reference,
                    friction: DEFAULT_SURFACE_FRICTION,
                });
            }
        }
        let rewrite: std::collections::HashMap<_, _> = self
            .native_base_paint_strokes
            .iter()
            .map(|r| &r.material)
            .chain(
                self.generation_layers
                    .iter()
                    .flat_map(|l| &l.brush_edits)
                    .filter_map(|e| {
                        if let super::LayerBrushEdit::Paint { material, .. } = e {
                            Some(material)
                        } else {
                            None
                        }
                    }),
            )
            .filter(|reference| {
                !reference.starts_with("layer:")
                    && !matches!(
                        reference
                            .trim_start_matches("PBR:")
                            .to_ascii_lowercase()
                            .as_str(),
                        "grass"
                            | "surface"
                            | "grass_1"
                            | "dirt"
                            | "subsurface"
                            | "rock"
                            | "tile_24"
                            | "sand"
                            | "beach"
                    )
            })
            .filter_map(|reference| {
                self.paint_layer_id(reference)
                    .map(|id| (reference.clone(), format!("layer:{id}")))
            })
            .collect();
        for request in &mut self.native_base_paint_strokes {
            if let Some(id) = rewrite.get(&request.material) {
                request.material = id.clone();
            }
        }
        for edit in self
            .generation_layers
            .iter_mut()
            .flat_map(|l| &mut l.brush_edits)
        {
            if let super::LayerBrushEdit::Paint { material, .. } = edit {
                if let Some(id) = rewrite.get(material) {
                    *material = id.clone();
                }
            }
        }
    }
}

impl TerrainState {
    pub fn remove_material_layer(&mut self, id: &str, replacement: &str) -> Result<(), String> {
        self.config.migrate_material_layers();
        if id == replacement
            || !self
                .config
                .material_layers
                .iter()
                .any(|l| l.id == replacement)
            || !self.config.material_layers.iter().any(|l| l.id == id)
        {
            return Err("Select an existing replacement layer".into());
        }
        if self.config.shore_material_layer == id {
            self.config.shore_material_layer = replacement.into();
        }
        for body in &mut self.config.water_bodies {
            if body.shore_material_layer == id {
                body.shore_material_layer = replacement.into();
            }
        }
        self.config.material_layers.retain(|l| l.id != id);
        for target in self.config.material_role_replacements.values_mut() {
            if target == id {
                *target = replacement.into();
            }
        }
        self.config
            .material_role_replacements
            .insert(id.into(), replacement.into());
        Ok(())
    }
}

#[cfg(test)]
mod unified_terrain_tests {
    use super::*;
    use crate::world::terrain::{
        TerrainData, TerrainGenerationRequest, TerrainPaintRequest, WaterBody, WaterBodyKind,
        storage::TerrainStorage,
    };
    fn state() -> TerrainState {
        let mut state = TerrainState::default();
        state.config.ensure_material_layers();
        state
            .generate_authored(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [64, 40, 64],
            })
            .unwrap();
        state
    }
    fn paint(id: &str, strength: f32) -> TerrainPaintRequest {
        TerrainPaintRequest {
            center: [32.0, 12.0, 32.0],
            radius: 3.0,
            material: format!("layer:{id}"),
            strength,
            falloff: Some(0.4),
        }
    }
    #[test]
    fn unified_terrain_default_layers_and_shore_need_no_assets() {
        let state = state();
        let layers = state.config.terrain_material_layers();
        assert_eq!(
            layers.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
            ["Surface", "Subsurface", "Rock"]
        );
        assert!(layers.iter().all(|l| l.material.is_empty()));
        assert_eq!(state.config.shore_material_layer, "subsurface");
        let TerrainData::Voxel(grid) = &state.authored.as_ref().unwrap().data else {
            panic!()
        };
        assert!(
            !grid
                .materials
                .iter()
                .any(|name| name.eq_ignore_ascii_case("sand"))
        );
        for kind in [WaterBodyKind::Lake, WaterBodyKind::River] {
            assert_eq!(
                WaterBody::new(1, kind, [0.0, 0.0], 0.0).shore_material_layer,
                "subsurface"
            );
        }
    }
    #[test]
    fn unified_terrain_legacy_config_retains_materials_and_sand() {
        let mut value = serde_json::to_value(TerrainConfig::default()).unwrap();
        for key in [
            "material_layers",
            "shore_material_layer",
            "material_role_replacements",
        ] {
            value.as_object_mut().unwrap().remove(key);
        }
        value["materials"]["surface"] = "Material:missing-creator-id".into();
        let mut config: TerrainConfig = serde_json::from_value(value).unwrap();
        config.migrate_material_layers();
        assert_eq!(config.shore_material_layer, "legacy-sand");
        assert_eq!(
            config.layer_material("surface"),
            "Material:missing-creator-id"
        );
        assert_eq!(config.layer_material("legacy-sand"), "PBR:Sand");
        let old: TerrainPaintRequest =
            serde_json::from_str(r#"{"center":[0,0,0],"radius":3,"material":"PBR:Sand"}"#).unwrap();
        assert_eq!(old.strength, 1.0);
        assert_eq!(old.falloff, None);
    }
    #[test]
    fn unified_terrain_custom_material_ids_paint_more_than_six_targets() {
        let mut state = state();
        let geometry = state.authored.clone();
        for i in 0..12 {
            let id = state.config.add_material_layer();
            state.config.material_layers.last_mut().unwrap().material =
                format!("Material:creator-{i}");
            assert!(state.paint_base_and_record(&paint(&id, 0.15)));
        }
        assert_eq!(
            state.authored, geometry,
            "painting must not remesh or mutate density"
        );
        assert!(state.paint_weights.pages.values().all(|p| p.valid()));
        assert!(state.paint_weights.pages.values().any(|p| {
            p.layers
                .values()
                .filter(|w| w.iter().any(|v| *v > 0))
                .count()
                == 12
        }));
        for page in state.paint_weights.pages.values() {
            for (i, _) in page.rgba.chunks_exact(4).enumerate() {
                let x = page.origin[0] + (i % 128) as f32 * 0.25 + 0.125;
                let z = page.origin[1] + (i / 128) as f32 * 0.25 + 0.125;
                if (x - 32.0).hypot(z - 32.0) >= 3.0 {
                    assert!(page.layers.values().all(|w| w[i] == 0));
                }
            }
        }
    }
    #[test]
    fn unified_terrain_rename_reorder_swap_and_missing_references_keep_weights() {
        let mut state = state();
        let id = state.config.add_material_layer();
        assert!(state.paint_base_and_record(&paint(&id, 0.5)));
        let before = state.paint_weights.pages.clone();
        let target = state.config.material_layers.last_mut().unwrap();
        target.name = "Custom coast".into();
        target.material = "Material:unavailable".into();
        state.config.move_material_layer(&id, -3);
        state.config.shore_material_layer = id.clone();
        assert_eq!(state.config.material_layers[0].id, id);
        assert_eq!(state.paint_weights.pages, before);
        state.rebuild_paint_from_layer_strokes();
        assert_eq!(state.paint_weights.pages, before);
        state.config.material_layers.retain(|l| l.id != id);
        state.rebuild_paint_from_layer_strokes();
        assert_eq!(
            state.paint_weights.pages, before,
            "missing target must not erase saved paint"
        );
        assert_eq!(state.config.shore_material_layer, id);
    }
    #[test]
    fn unified_terrain_remove_requires_replacement_and_transfers_shores_and_paint() {
        let mut state = state();
        let id = state.config.add_material_layer();
        assert!(state.paint_base_and_record(&paint(&id, 0.6)));
        state.config.shore_material_layer = id.clone();
        let mut lake = WaterBody::new(1, WaterBodyKind::Lake, [32.0, 32.0], 12.0);
        lake.shore_material_layer = id.clone();
        state.config.water_bodies.push(lake);
        let before = state.paint_weights.pages.clone();
        assert!(state.remove_material_layer(&id, "").is_err());
        assert_eq!(state.paint_weights.pages, before);
        state.remove_material_layer(&id, "subsurface").unwrap();
        assert_eq!(state.config.shore_material_layer, "subsurface");
        assert_eq!(
            state.config.water_bodies[0].shore_material_layer,
            "subsurface"
        );
        assert_eq!(state.paint_weights.pages, before);
        assert_eq!(state.config.resolved_layer_id(&id), "subsurface");
        assert!(
            state.config.native_base_paint_strokes.iter().all(|r| state
                .config
                .paint_layer_id(&r.material)
                .as_deref()
                == Some("subsurface"))
        );
        state.rebuild_paint_from_layer_strokes();
        assert_eq!(state.paint_weights.pages, before);
    }
    #[test]
    fn unified_terrain_save_reopen_restores_targets_shores_and_exact_weights() {
        let mut state = state();
        let id = state.config.add_material_layer();
        state.config.shore_material_layer = id.clone();
        for (index, kind) in [WaterBodyKind::Lake, WaterBodyKind::River]
            .into_iter()
            .enumerate()
        {
            let mut body = WaterBody::new(index as u64 + 1, kind, [32.0, 32.0], 12.0);
            body.shore_material_layer = id.clone();
            state.config.water_bodies.push(body);
        }
        assert!(state.paint_base_and_record(&paint(&id, 0.37)));
        state.config.move_material_layer(&id, -2);
        let root = std::env::temp_dir().join(format!(
            "worldkiln_unified_targets_{}_{:032x}",
            std::process::id(),
            rand::random::<u128>()
        ));
        let storage = TerrainStorage::new(&root);
        storage.save(&state).unwrap();
        let restored = storage.load().unwrap().unwrap();
        assert_eq!(restored.config, state.config);
        assert_eq!(restored.paint_weights.pages, state.paint_weights.pages);
        assert_eq!(restored.authored, state.authored);
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn unified_terrain_fractional_strength_and_falloff_survive_replay() {
        let mut state = state();
        let id = state.config.add_material_layer();
        assert!(state.paint_base_and_record(&paint(&id, 0.23)));
        let before = state.paint_weights.pages.clone();
        state.config.paint_falloff = 0.99;
        state.rebuild_paint_from_layer_strokes();
        assert_eq!(before, state.paint_weights.pages);
        let max = state
            .paint_weights
            .pages
            .values()
            .flat_map(|p| p.layers[&id].iter())
            .copied()
            .max()
            .unwrap();
        assert!((57..=60).contains(&max));
        let mut invalid = paint(&id, f32::NAN);
        assert!(!state.paint_native(&invalid));
        invalid.strength = 1.01;
        assert!(!state.paint_native(&invalid));
    }
    #[test]
    fn unified_terrain_removal_redirects_legacy_roles_without_resurrecting_targets() {
        let mut state = state();
        let mut request = paint("subsurface", 0.42);
        request.material = "PBR:Dirt".into();
        assert!(state.paint_base_and_record(&request));
        let before = state.paint_weights.pages.clone();
        let id = state.config.add_material_layer();
        state.config.material_layers.last_mut().unwrap().material = "Material:custom-soil".into();
        state.remove_material_layer("subsurface", &id).unwrap();
        state.config.migrate_material_layers();
        assert!(
            !state
                .config
                .material_layers
                .iter()
                .any(|l| l.id == "subsurface")
        );
        assert_eq!(
            state.config.layer_material("subsurface"),
            "Material:custom-soil"
        );
        assert_eq!(state.config.paint_layer_id("PBR:Dirt"), Some(id));
        state.rebuild_paint_from_layer_strokes();
        assert_eq!(state.paint_weights.pages, before);
    }
    #[test]
    fn unified_terrain_migrates_creator_paint_to_ids_before_material_swap() {
        let mut state = state();
        let mut request = paint("surface", 0.3);
        request.material = "Material:old-creator-asset".into();
        state.config.native_base_paint_strokes.push(request);
        state.config.migrate_material_layers();
        let reference = state.config.native_base_paint_strokes[0].material.clone();
        let id = reference.strip_prefix("layer:").unwrap();
        state.rebuild_paint_from_layer_strokes();
        let before = state.paint_weights.pages.clone();
        assert!(!before.is_empty());
        state
            .config
            .material_layers
            .iter_mut()
            .find(|l| l.id == id)
            .unwrap()
            .material = "Material:new-creator-asset".into();
        state.rebuild_paint_from_layer_strokes();
        assert_eq!(state.paint_weights.pages, before);
    }
}

#[cfg(test)]
mod friction_tests {
    use super::*;

    #[test]
    fn every_created_material_layer_has_independent_persisted_friction() {
        let mut cfg = TerrainConfig::default();
        let first = cfg.add_material_layer();
        let second = cfg.add_material_layer();
        cfg.material_layers
            .iter_mut()
            .find(|l| l.id == first)
            .unwrap()
            .friction = 0.0;
        cfg.material_layers
            .iter_mut()
            .find(|l| l.id == second)
            .unwrap()
            .friction = 4.0;
        let json = serde_json::to_string(&cfg.material_layers).unwrap();
        let layers: Vec<TerrainMaterialLayer> = serde_json::from_str(&json).unwrap();
        assert_eq!(layers.iter().find(|l| l.id == first).unwrap().friction, 0.0);
        assert_eq!(
            layers.iter().find(|l| l.id == second).unwrap().friction,
            4.0
        );
        let legacy = r#"{"id":"old","name":"Old","material":"PBR:Stone"}"#;
        let old: TerrainMaterialLayer = serde_json::from_str(legacy).unwrap();
        assert_eq!(old.friction, DEFAULT_SURFACE_FRICTION);
    }
}
