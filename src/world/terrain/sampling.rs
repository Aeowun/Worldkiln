// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{TerrainData, TerrainState};
use crate::landscape::material_weights::{MaterialWeightPage, PAGE_DIM, PageId};

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainSurfaceSample {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub slope_degrees: f32,
    pub material_id: Option<String>,
    pub source: &'static str,
}

impl TerrainState {
    pub fn sample_surface(&self, x: f32, z: f32) -> Option<TerrainSurfaceSample> {
        if !x.is_finite() || !z.is_finite() {
            return None;
        }
        let active = self.effective()?;
        let y = self.surface_height_at(x, z)?;
        if !y.is_finite() {
            return None;
        }
        let spacing = match &active.data {
            TerrainData::Heightfield(_) => 0.5,
            TerrainData::Voxel(grid) => (grid.spacing_studs * 0.5).clamp(0.25, 2.0),
        };

        let left = self.surface_height_at(x - spacing, z).unwrap_or(y);
        let right = self.surface_height_at(x + spacing, z).unwrap_or(y);
        let back = self.surface_height_at(x, z - spacing).unwrap_or(y);
        let front = self.surface_height_at(x, z + spacing).unwrap_or(y);
        let dx = (if self.surface_height_at(x - spacing, z).is_some() {
            spacing
        } else {
            0.0
        }) + if self.surface_height_at(x + spacing, z).is_some() {
            spacing
        } else {
            0.0
        };
        let dz = (if self.surface_height_at(x, z - spacing).is_some() {
            spacing
        } else {
            0.0
        }) + if self.surface_height_at(x, z + spacing).is_some() {
            spacing
        } else {
            0.0
        };
        let gradient_x = if dx > 0.0 { (right - left) / dx } else { 0.0 };
        let gradient_z = if dz > 0.0 { (front - back) / dz } else { 0.0 };
        let normal = glam::Vec3::new(-gradient_x, 1.0, -gradient_z).normalize_or_zero();
        if !normal.is_finite() || normal.y <= 0.0 {
            return None;
        }

        let material_id = self.paint_material_at(x, z).or_else(|| {
            if self.paint_page_at(x, z).is_some() {
                return None;
            }
            match &active.data {
                TerrainData::Heightfield(_) => None,
                TerrainData::Voxel(grid) => {
                    let step = grid.spacing_studs;
                    let to_index = |world: f32, origin: i32, size: u32| -> Option<u32> {
                        let index = ((world - origin as f32) / step - 0.5).round();
                        (index >= 0.0 && index < size as f32).then_some(index as u32)
                    };
                    let vx = to_index(x, grid.origin[0], grid.size[0])?;
                    let vz = to_index(z, grid.origin[2], grid.size[2])?;
                    let vy = ((y - grid.origin[1] as f32) / step - 0.5).floor() as i32;
                    (0..=2).find_map(|offset| {
                        let iy = vy + 1 - offset;
                        let iy = u32::try_from(iy).ok()?;
                        let voxel = grid.voxel(vx, iy, vz)?;
                        if voxel.is_empty() {
                            return None;
                        }
                        let name = grid.material_name(voxel.material)?;
                        let role = match name {
                            "Surface" => "surface",
                            "Subsurface" => "subsurface",
                            "Rock" => "rock",
                            other => other,
                        };
                        let resolved = self.config.layer_material(role);
                        let name = if resolved.is_empty() { name } else { resolved };
                        (!name.is_empty()).then(|| name.to_owned())
                    })
                }
            }
        });

        Some(TerrainSurfaceSample {
            position: [x, y, z],
            normal: normal.to_array(),
            slope_degrees: normal.y.clamp(-1.0, 1.0).acos().to_degrees(),
            material_id,
            source: if self.runtime.is_some() {
                "runtime"
            } else {
                "authored"
            },
        })
    }

    fn paint_page_at(&self, x: f32, z: f32) -> Option<(&MaterialWeightPage, usize)> {
        let active = self.effective()?;
        let origin = [
            active.request.position[0] as f32,
            active.request.position[2] as f32,
        ];
        const PITCH: f32 = 0.25;
        let span = PITCH * PAGE_DIM as f32;
        let px = ((x - origin[0]) / span).floor();
        let pz = ((z - origin[1]) / span).floor();
        if px.abs() >= i32::MAX as f32 || pz.abs() >= i32::MAX as f32 {
            return None;
        }
        let id = PageId {
            chart: 1,
            x: px as i32,
            y: pz as i32,
        };
        let page = self.paint_weights.pages.get(&id)?;
        let sx = ((x - page.origin[0]) / page.texel_pitch).floor();
        let sz = ((z - page.origin[1]) / page.texel_pitch).floor();
        if !sx.is_finite()
            || !sz.is_finite()
            || sx < 0.0
            || sx >= PAGE_DIM as f32
            || sz < 0.0
            || sz >= PAGE_DIM as f32
        {
            return None;
        }
        let index = sz as usize * PAGE_DIM + sx as usize;
        (page.rgba.len() == PAGE_DIM * PAGE_DIM * 4).then_some((page, index))
    }

    fn paint_layer_id_at(&self, x: f32, z: f32) -> Option<String> {
        let (page, texel) = self.paint_page_at(x, z)?;
        let dynamic_sum: u32 = page
            .layers
            .values()
            .filter_map(|weights| weights.get(texel).map(|w| *w as u32))
            .sum();
        let mut strongest: Option<(&str, u32)> = page
            .layers
            .iter()
            .filter_map(|(id, weights)| weights.get(texel).map(|w| (id.as_str(), *w as u32)))
            .max_by_key(|(_, value)| *value);
        let legacy_channels = ["surface", "subsurface", "rock", "legacy-sand"];
        if let Some((channel, weight)) = page.rgba[texel * 4..texel * 4 + 4]
            .iter()
            .enumerate()
            .map(|(i, w)| (i, (*w as u32 * (255 - dynamic_sum.min(255))) / 255))
            .max_by_key(|(_, weight)| *weight)
        {
            if weight > strongest.map(|(_, w)| w).unwrap_or(0) {
                strongest = Some((legacy_channels[channel], weight));
            }
        }
        let (id, weight) = strongest?;
        if weight == 0 {
            return None;
        }
        Some(self.config.resolved_layer_id(id).to_string())
    }

    fn paint_material_at(&self, x: f32, z: f32) -> Option<String> {
        let id = self.paint_layer_id_at(x, z)?;
        let material = self.config.layer_material(&id);
        (!material.is_empty()).then(|| material.to_string())
    }

    pub fn surface_friction_at(&self, x: f32, z: f32) -> f32 {
        if let Some(layer_id) = self.paint_layer_id_at(x, z) {
            return self.config.layer_friction(&layer_id);
        }
        let fallback = self
            .sample_surface(x, z)
            .and_then(|sample| sample.material_id)
            .and_then(|material| {
                self.config
                    .terrain_material_layers()
                    .iter()
                    .find(|l| l.material == material)
                    .map(|l| l.id.clone())
            });
        self.config
            .layer_friction(fallback.as_deref().unwrap_or("surface"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::terrain::{
        HeightfieldData, TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    };

    fn make_heightfield(heights: Vec<f32>) -> TerrainState {
        let mut terrain = TerrainState::default();
        terrain.authored = Some(TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [3, 10, 3],
            },
            generator: TerrainGeneratorType::Plains,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: [0, 0, 0],
                size: [3, 10, 3],
                heights,
            }),
        });
        terrain
    }

    #[test]
    fn surface_sample_flat_and_missing_are_safe() {
        let terrain = make_heightfield(vec![3.0; 9]);
        let result = terrain.sample_surface(1.5, 1.5).unwrap();
        assert_eq!(result.position, [1.5, 3.0, 1.5]);
        assert_eq!(result.normal, [0.0, 1.0, 0.0]);
        assert_eq!(result.slope_degrees, 0.0);
        assert_eq!(result.material_id, None);
        assert_eq!(result.source, "authored");
        for (x, z) in [
            (f32::NAN, 1.0),
            (0.0, f32::INFINITY),
            (-1.0, 0.0),
            (3.0, 0.0),
        ] {
            assert!(terrain.sample_surface(x, z).is_none());
        }
        assert!(TerrainState::default().sample_surface(1.0, 1.0).is_none());
    }

    #[test]
    fn surface_sample_slope_and_runtime_override() {
        let mut terrain = make_heightfield(vec![1.0, 2.0, 3.0, 1.0, 2.0, 3.0, 1.0, 2.0, 3.0]);
        let sample = terrain.sample_surface(1.5, 1.5).unwrap();
        assert!(sample.slope_degrees > 20.0 && sample.slope_degrees < 70.0);
        assert!(sample.normal[0] < 0.0);
        let mut runtime = terrain.authored.clone().unwrap();
        if let TerrainData::Heightfield(ref mut h) = runtime.data {
            h.heights.fill(8.0);
        }
        terrain.runtime = Some(runtime);
        let sample = terrain.sample_surface(1.5, 1.5).unwrap();
        assert_eq!(sample.source, "runtime");
        assert_eq!(sample.position[1], 8.0);
        assert_eq!(sample.slope_degrees, 0.0);
    }

    #[test]
    fn surface_sample_reports_actual_dominant_paint_material() {
        use crate::world::terrain::TerrainPaintRequest;

        let mut terrain = make_heightfield(vec![3.0; 9]);
        terrain.config.ensure_material_layers();
        terrain
            .config
            .material_layers
            .iter_mut()
            .find(|layer| layer.id == "rock")
            .unwrap()
            .material = "PBR:Rock".into();
        let center = [1.5, 3.0, 1.5];
        let paint = TerrainPaintRequest {
            center,
            radius: 0.75,
            material: "PBR:Rock".to_string(),
            strength: 1.0,
            falloff: Some(0.0),
        };
        assert!(terrain.paint_base_and_record(&paint));
        let sample = terrain.sample_surface(1.5, 1.5).unwrap();
        assert_eq!(sample.source, "authored");
        assert_eq!(sample.material_id.as_deref(), Some("PBR:Rock"));
    }
}
