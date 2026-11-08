// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use fastnoise_lite::{FastNoiseLite, NoiseType};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::types::{
    HeightfieldData, SculptTool, TerrainMaterials, TerrainPaintRequest, TerrainSculptRequest,
};

pub const DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS: f32 = 2.0;
pub const LEGACY_TERRAIN_SAMPLE_SPACING_STUDS: f32 = 4.0;

fn legacy_terrain_sample_spacing_studs() -> f32 {
    LEGACY_TERRAIN_SAMPLE_SPACING_STUDS
}
pub const SMOOTH_VOXEL_OCCUPANCY_MAX: u8 = u8::MAX;
pub const SMOOTH_VOXEL_ISO_LEVEL: f32 = 0.5;
pub(crate) const TERRAIN_FALLBACK_SURFACE_MATERIAL: &str = "__worldkiln_terrain_surface";
pub(crate) const TERRAIN_FALLBACK_SUBSURFACE_MATERIAL: &str = "__worldkiln_terrain_subsurface";
pub(crate) const TERRAIN_FALLBACK_ROCK_MATERIAL: &str = "__worldkiln_terrain_rock";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct SmoothVoxel {
    pub material: u16,
    pub occupancy: u8,
}

impl SmoothVoxel {
    pub fn from_unit_occupancy(material: u16, occupancy: f32) -> Self {
        let normalized = if occupancy.is_finite() {
            occupancy.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self {
            material,
            occupancy: (normalized * SMOOTH_VOXEL_OCCUPANCY_MAX as f32).round() as u8,
        }
    }

    pub fn occupancy_unit(self) -> f32 {
        self.occupancy as f32 / SMOOTH_VOXEL_OCCUPANCY_MAX as f32
    }

    pub fn is_empty(self) -> bool {
        self.occupancy == 0
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SmoothVoxelGrid {
    pub origin: [i32; 3],
    #[serde(default = "legacy_terrain_sample_spacing_studs")]
    pub spacing_studs: f32,
    pub size: [u32; 3],
    pub materials: Vec<String>,
    pub voxels: Vec<SmoothVoxel>,
}

impl SmoothVoxelGrid {
    pub fn empty(
        origin: [i32; 3],
        size: [u32; 3],
        materials: Vec<String>,
    ) -> Result<Self, SmoothVoxelGridError> {
        Self::empty_with_spacing(origin, size, LEGACY_TERRAIN_SAMPLE_SPACING_STUDS, materials)
    }

    pub fn empty_with_spacing(
        origin: [i32; 3],
        size: [u32; 3],
        spacing_studs: f32,
        materials: Vec<String>,
    ) -> Result<Self, SmoothVoxelGridError> {
        let volume = checked_volume(size)?;
        let grid = Self {
            origin,
            spacing_studs,
            size,
            materials,
            voxels: vec![SmoothVoxel::default(); volume],
        };
        grid.validate()?;
        Ok(grid)
    }

    pub fn from_dense(
        origin: [i32; 3],
        size: [u32; 3],
        materials: Vec<String>,
        voxels: Vec<SmoothVoxel>,
    ) -> Result<Self, SmoothVoxelGridError> {
        Self::from_dense_with_spacing(
            origin,
            size,
            LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
            materials,
            voxels,
        )
    }

    pub fn from_dense_with_spacing(
        origin: [i32; 3],
        size: [u32; 3],
        spacing_studs: f32,
        materials: Vec<String>,
        voxels: Vec<SmoothVoxel>,
    ) -> Result<Self, SmoothVoxelGridError> {
        let grid = Self {
            origin,
            spacing_studs,
            size,
            materials,
            voxels,
        };
        grid.validate()?;
        Ok(grid)
    }

    pub fn validate(&self) -> Result<(), SmoothVoxelGridError> {
        if !self.spacing_studs.is_finite() || self.spacing_studs <= 0.0 {
            return Err(SmoothVoxelGridError::InvalidSpacing(self.spacing_studs));
        }
        if self.materials.len() > u16::MAX as usize + 1 {
            return Err(SmoothVoxelGridError::PaletteTooLarge {
                actual: self.materials.len(),
            });
        }
        let expected = checked_volume(self.size)?;
        if self.voxels.len() != expected {
            return Err(SmoothVoxelGridError::VoxelCountMismatch {
                expected,
                actual: self.voxels.len(),
            });
        }
        for voxel in self
            .voxels
            .iter()
            .copied()
            .filter(|voxel| !voxel.is_empty())
        {
            if voxel.material as usize >= self.materials.len() {
                return Err(SmoothVoxelGridError::MaterialIndexOutOfRange {
                    material: voxel.material,
                    palette_len: self.materials.len(),
                });
            }
        }
        Ok(())
    }

    pub fn index(&self, x: u32, y: u32, z: u32) -> Option<usize> {
        if x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
            return None;
        }
        Some((y as usize * self.size[2] as usize + z as usize) * self.size[0] as usize + x as usize)
    }

    pub fn voxel(&self, x: u32, y: u32, z: u32) -> Option<SmoothVoxel> {
        self.index(x, y, z).map(|index| self.voxels[index])
    }

    pub fn set_voxel(&mut self, x: u32, y: u32, z: u32, voxel: SmoothVoxel) -> bool {
        if !voxel.is_empty() && voxel.material as usize >= self.materials.len() {
            return false;
        }
        let Some(index) = self.index(x, y, z) else {
            return false;
        };
        self.voxels[index] = voxel;
        true
    }

    pub fn material_name(&self, material: u16) -> Option<&str> {
        self.materials.get(material as usize).map(String::as_str)
    }

    pub fn voxel_world_min(&self, x: u32, y: u32, z: u32) -> Option<[f32; 3]> {
        self.index(x, y, z)?;
        Some([
            self.origin[0] as f32 + x as f32 * self.spacing_studs,
            self.origin[1] as f32 + y as f32 * self.spacing_studs,
            self.origin[2] as f32 + z as f32 * self.spacing_studs,
        ])
    }

    pub fn filled_voxel_count(&self) -> usize {
        self.voxels.iter().filter(|voxel| !voxel.is_empty()).count()
    }

    pub fn world_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let min = [
            self.origin[0] as f32,
            self.origin[1] as f32,
            self.origin[2] as f32,
        ];
        let max = [
            min[0] + self.size[0] as f32 * self.spacing_studs,
            min[1] + self.size[1] as f32 * self.spacing_studs,
            min[2] + self.size[2] as f32 * self.spacing_studs,
        ];
        (min, max)
    }

    pub fn density_at_world(&self, point: [f32; 3]) -> f32 {
        if point.iter().any(|value| !value.is_finite()) {
            return 0.0;
        }
        let gx = (point[0] - self.origin[0] as f32) / self.spacing_studs - 0.5;
        let gy = (point[1] - self.origin[1] as f32) / self.spacing_studs - 0.5;
        let gz = (point[2] - self.origin[2] as f32) / self.spacing_studs - 0.5;
        let x0 = gx.floor() as i32;
        let y0 = gy.floor() as i32;
        let z0 = gz.floor() as i32;
        let tx = gx - x0 as f32;
        let ty = gy - y0 as f32;
        let tz = gz - z0 as f32;

        let sample = |x: i32, y: i32, z: i32| -> f32 {
            if x < 0 || y < 0 || z < 0 {
                return 0.0;
            }
            self.voxel(x as u32, y as u32, z as u32)
                .map(SmoothVoxel::occupancy_unit)
                .unwrap_or(0.0)
        };
        let x00 = lerp(sample(x0, y0, z0), sample(x0 + 1, y0, z0), tx);
        let x10 = lerp(sample(x0, y0 + 1, z0), sample(x0 + 1, y0 + 1, z0), tx);
        let x01 = lerp(sample(x0, y0, z0 + 1), sample(x0 + 1, y0, z0 + 1), tx);
        let x11 = lerp(
            sample(x0, y0 + 1, z0 + 1),
            sample(x0 + 1, y0 + 1, z0 + 1),
            tx,
        );
        let y0v = lerp(x00, x10, ty);
        let y1v = lerp(x01, x11, ty);
        lerp(y0v, y1v, tz)
    }

    pub fn paint_surface_material(&mut self, request: &TerrainPaintRequest) -> bool {
        if request.radius <= 0.0
            || !request.radius.is_finite()
            || request.material.trim().is_empty()
            || request.center.iter().any(|value| !value.is_finite())
            || self.size[0] == 0
            || self.size[1] == 0
            || self.size[2] == 0
        {
            return false;
        }

        let material_name = request.material.trim();
        let mut material = self
            .materials
            .iter()
            .position(|candidate| candidate == material_name)
            .map(|index| index as u16);
        let (min, _) = self.world_bounds();
        let radius_sq = request.radius * request.radius;
        let max_x = self.size[0] as i32 - 1;
        let max_z = self.size[2] as i32 - 1;
        let local_center_x = (request.center[0] - min[0]) / self.spacing_studs - 0.5;
        let local_center_z = (request.center[2] - min[2]) / self.spacing_studs - 0.5;
        let voxel_radius = request.radius / self.spacing_studs;
        let min_x = ((local_center_x - voxel_radius).floor() as i32).clamp(0, max_x);
        let max_x_brush = ((local_center_x + voxel_radius).ceil() as i32).clamp(0, max_x);
        let min_z = ((local_center_z - voxel_radius).floor() as i32).clamp(0, max_z);
        let max_z_brush = ((local_center_z + voxel_radius).ceil() as i32).clamp(0, max_z);

        let max_y = self.size[1] as i32 - 1;
        let local_center_y = (request.center[1] - min[1]) / self.spacing_studs - 0.5;
        let min_y = ((local_center_y - voxel_radius - 1.0).floor() as i32).clamp(0, max_y);
        let max_y_brush = ((local_center_y + voxel_radius + 1.0).ceil() as i32).clamp(0, max_y);
        let mut changed = false;
        for z in min_z..=max_z_brush {
            for x in min_x..=max_x_brush {
                for y in min_y..=max_y_brush {
                    let (ux, uy, uz) = (x as u32, y as u32, z as u32);
                    let Some(mut voxel) = self.voxel(ux, uy, uz) else {
                        continue;
                    };
                    if voxel.is_empty() {
                        continue;
                    }
                    let distance_sq = (0..3)
                        .map(|axis| {
                            let i = [x, y, z][axis] as f32;
                            let low = min[axis] + i * self.spacing_studs;
                            let high = low + self.spacing_studs;
                            let offset =
                                request.center[axis] - request.center[axis].clamp(low, high);
                            offset * offset
                        })
                        .sum::<f32>();
                    if distance_sq > radius_sq {
                        continue;
                    }
                    let exposed = [
                        (x - 1, y, z),
                        (x + 1, y, z),
                        (x, y - 1, z),
                        (x, y + 1, z),
                        (x, y, z - 1),
                        (x, y, z + 1),
                    ]
                    .into_iter()
                    .any(|(nx, ny, nz)| {
                        nx < 0
                            || ny < 0
                            || nz < 0
                            || nx >= self.size[0] as i32
                            || ny >= self.size[1] as i32
                            || nz >= self.size[2] as i32
                            || self
                                .voxel(nx as u32, ny as u32, nz as u32)
                                .is_none_or(|adjacent| adjacent.is_empty())
                    });
                    if !exposed && voxel.occupancy == SMOOTH_VOXEL_OCCUPANCY_MAX {
                        continue;
                    }
                    let target_material = if let Some(id) = material {
                        id
                    } else {
                        if self.materials.len() > u16::MAX as usize {
                            return changed;
                        }
                        let id = self.materials.len() as u16;
                        self.materials.push(material_name.to_string());
                        material = Some(id);
                        id
                    };
                    if voxel.material != target_material {
                        voxel.material = target_material;
                        self.set_voxel(ux, uy, uz, voxel);
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    pub fn surface_height_at(&self, world_x: f32, world_z: f32) -> Option<f32> {
        if !world_x.is_finite() || !world_z.is_finite() {
            return None;
        }
        let (min, max) = self.world_bounds();
        if world_x < min[0] || world_x > max[0] || world_z < min[2] || world_z > max[2] {
            return None;
        }
        let spacing = self.spacing_studs;
        let gx = (world_x - min[0]) / spacing - 0.5;
        let gz = (world_z - min[2]) / spacing - 0.5;
        let x0 = gx.floor() as i32;
        let z0 = gz.floor() as i32;
        let tx = gx - x0 as f32;
        let tz = gz - z0 as f32;
        let sample = |x: i32, y: u32, z: i32| -> f32 {
            if x < 0 || z < 0 {
                return 0.0;
            }
            self.voxel(x as u32, y, z as u32)
                .map(SmoothVoxel::occupancy_unit)
                .unwrap_or(0.0)
        };
        let mut upper_y = max[1] + spacing * 0.5;
        let mut upper_density = 0.0_f32;
        for y in (0..self.size[1]).rev() {
            let xz0 = lerp(sample(x0, y, z0), sample(x0 + 1, y, z0), tx);
            let xz1 = lerp(sample(x0, y, z0 + 1), sample(x0 + 1, y, z0 + 1), tx);
            let lower_density = lerp(xz0, xz1, tz);
            let lower_y = upper_y - spacing;
            if upper_density < SMOOTH_VOXEL_ISO_LEVEL && lower_density >= SMOOTH_VOXEL_ISO_LEVEL {
                let t = ((SMOOTH_VOXEL_ISO_LEVEL - upper_density)
                    / (lower_density - upper_density))
                    .clamp(0.0, 1.0);
                return Some(upper_y + (lower_y - upper_y) * t);
            }
            upper_density = lower_density;
            upper_y = lower_y;
        }
        None
    }

    pub fn solid_occupies_cell(&self, coord: crate::world::WorldCoord) -> bool {
        const OFFSETS: [f32; 3] = [0.05, 0.5, 0.95];
        for dz in OFFSETS {
            for dy in OFFSETS {
                for dx in OFFSETS {
                    if self.density_at_world([
                        coord.x as f32 + dx,
                        coord.y as f32 + dy,
                        coord.z as f32 + dz,
                    ]) >= SMOOTH_VOXEL_ISO_LEVEL
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn is_columnar_surface(&self) -> bool {
        for z in 0..self.size[2] {
            for x in 0..self.size[0] {
                let mut reached_empty = false;
                for y in 0..self.size[1] {
                    let occupied = self.voxel(x, y, z).is_some_and(|voxel| !voxel.is_empty());
                    if occupied && reached_empty {
                        return false;
                    }
                    if !occupied {
                        reached_empty = true;
                    }
                }
            }
        }
        true
    }

    fn column_surface_height(&self, x: u32, z: u32) -> Option<f32> {
        let spacing = self.spacing_studs;
        let mut upper_y = self.origin[1] as f32 + (self.size[1] as f32 + 0.5) * spacing;
        let mut upper_density = 0.0_f32;
        for y in (0..self.size[1]).rev() {
            let lower_density = self.voxel(x, y, z)?.occupancy_unit();
            if upper_density < SMOOTH_VOXEL_ISO_LEVEL && lower_density >= SMOOTH_VOXEL_ISO_LEVEL {
                let t = (SMOOTH_VOXEL_ISO_LEVEL - upper_density) / (lower_density - upper_density);
                return Some(upper_y - spacing * t);
            }
            upper_y -= spacing;
            upper_density = lower_density;
        }
        None
    }

    pub fn apply_surface_sculpt(
        &mut self,
        request: &TerrainSculptRequest,
        min_y: f32,
        max_y: f32,
    ) -> bool {
        if request.tool == SculptTool::None
            || request.radius <= 0.0
            || !request.radius.is_finite()
            || request.strength <= 0.0
            || !request.strength.is_finite()
            || request.center.iter().any(|value| !value.is_finite())
            || (request.tool.is_flatten()
                && request
                    .flatten_target
                    .is_none_or(|value| !value.is_finite()))
        {
            return false;
        }

        let (min, max) = self.world_bounds();
        let sculpt_min_y = min_y.max(min[1]);
        let sculpt_max_y = max_y.min(max[1]);
        if !sculpt_min_y.is_finite() || !sculpt_max_y.is_finite() || sculpt_min_y > sculpt_max_y {
            return false;
        }
        let sx = self.spacing_studs;
        let local_x = (request.center[0] - min[0]) / sx - 0.5;
        let local_z = (request.center[2] - min[2]) / sx - 0.5;
        let sample_radius = request.radius / sx;
        let max_x_index = self.size[0] as i32 - 1;
        let max_z_index = self.size[2] as i32 - 1;
        if local_x + sample_radius < 0.0
            || local_z + sample_radius < 0.0
            || local_x - sample_radius > max_x_index as f32
            || local_z - sample_radius > max_z_index as f32
        {
            return false;
        }
        let min_x = ((local_x - sample_radius).floor() as i32).clamp(0, max_x_index) as u32;
        let max_x = ((local_x + sample_radius).ceil() as i32).clamp(0, max_x_index) as u32;
        let min_z = ((local_z - sample_radius).floor() as i32).clamp(0, max_z_index) as u32;
        let max_z = ((local_z + sample_radius).ceil() as i32).clamp(0, max_z_index) as u32;
        for z in min_z..=max_z {
            for x in min_x..=max_x {
                let mut reached_empty = false;
                for y in 0..self.size[1] {
                    let occupied = self.voxel(x, y, z).is_some_and(|voxel| !voxel.is_empty());
                    if occupied && reached_empty {
                        return false;
                    }
                    if !occupied {
                        reached_empty = true;
                    }
                }
            }
        }
        let snapshot_min_x = min_x.saturating_sub(1);
        let snapshot_min_z = min_z.saturating_sub(1);
        let snapshot_max_x = max_x.saturating_add(1).min(self.size[0] - 1);
        let snapshot_max_z = max_z.saturating_add(1).min(self.size[2] - 1);
        let snapshot_width = (snapshot_max_x - snapshot_min_x + 1) as usize;
        let snapshot_depth = (snapshot_max_z - snapshot_min_z + 1) as usize;
        let mut surfaces = vec![None; snapshot_width * snapshot_depth];
        for z in snapshot_min_z..=snapshot_max_z {
            for x in snapshot_min_x..=snapshot_max_x {
                let wx = min[0] + (x as f32 + 0.5) * sx;
                let wz = min[2] + (z as f32 + 0.5) * sx;
                let idx =
                    (z - snapshot_min_z) as usize * snapshot_width + (x - snapshot_min_x) as usize;
                surfaces[idx] = self.column_surface_height(x, z);
            }
        }

        let radius_sq = request.radius * request.radius;
        let mut changed = false;
        for z in min_z..=max_z {
            let wz = min[2] + (z as f32 + 0.5) * self.spacing_studs;
            let dz = wz - request.center[2];
            for x in min_x..=max_x {
                let wx = min[0] + (x as f32 + 0.5) * self.spacing_studs;
                let dx = wx - request.center[0];
                let edge_dx = (dx.abs() - sx * 0.5).max(0.0);
                let edge_dz = (dz.abs() - sx * 0.5).max(0.0);
                let coverage_distance_sq = edge_dx * edge_dx + edge_dz * edge_dz;
                if coverage_distance_sq >= radius_sq {
                    continue;
                }
                let dist_sq = dx * dx + dz * dz;
                let influence_distance =
                    (dist_sq.sqrt() - sx * 0.25).max(coverage_distance_sq.sqrt());
                let index =
                    (z - snapshot_min_z) as usize * snapshot_width + (x - snapshot_min_x) as usize;
                let Some(current) = surfaces[index] else {
                    continue;
                };
                let falloff = (1.0 - (influence_distance / request.radius).powi(2)).clamp(0.0, 1.0);
                let target = match request.tool {
                    SculptTool::None => current,
                    SculptTool::Raise => current + request.strength * falloff,
                    SculptTool::Lower => current - request.strength * falloff,
                    SculptTool::Flatten
                    | SculptTool::FlattenRaiseOnly
                    | SculptTool::FlattenCutOnly => {
                        let desired = request.flatten_target.unwrap();
                        if request.tool.allows_flatten_direction(current, desired) {
                            let core = (self.spacing_studs * std::f32::consts::SQRT_2)
                                .min(request.radius * 0.75);
                            let feather_width = (request.radius - core).max(f32::EPSILON);
                            let edge_distance = (influence_distance - core).max(0.0);
                            let flatten_weight =
                                (1.0 - (edge_distance / feather_width).powi(2)).clamp(0.0, 1.0);
                            current
                                + (desired - current) * (request.strength * flatten_weight).min(1.0)
                        } else {
                            current
                        }
                    }
                    SculptTool::Paint => current,
                    SculptTool::Smooth => {
                        let mut sum = 0.0;
                        let mut count = 0.0;
                        for nz in z.saturating_sub(1)..=(z + 1).min(snapshot_max_z) {
                            for nx in x.saturating_sub(1)..=(x + 1).min(snapshot_max_x) {
                                if let Some(height) = surfaces[(nz - snapshot_min_z) as usize
                                    * snapshot_width
                                    + (nx - snapshot_min_x) as usize]
                                {
                                    sum += height;
                                    count += 1.0;
                                }
                            }
                        }
                        if count == 0.0 {
                            current
                        } else {
                            let desired = sum / count;
                            current + (desired - current) * (request.strength * falloff).min(1.0)
                        }
                    }
                }
                .clamp(sculpt_min_y, sculpt_max_y);

                if (target - current).abs() > 0.001 {
                    self.set_column_surface_height(x, z, current, target);
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn refresh_local_slope_materials(
        &mut self,
        center: [f32; 3],
        radius: f32,
        materials: &TerrainMaterials,
        settings: VoxelLayerSettings,
        seed: i32,
        water_level: f32,
    ) {
        if !radius.is_finite() || radius <= 0.0 || self.size.contains(&0) {
            return;
        }
        let (min, _) = self.world_bounds();
        let spacing = self.spacing_studs;
        let cx = (center[0] - min[0]) / spacing - 0.5;
        let cz = (center[2] - min[2]) / spacing - 0.5;
        let r = radius / spacing + 2.0;
        let x0 = ((cx - r).floor() as i32).clamp(0, self.size[0] as i32 - 1) as u32;
        let x1 = ((cx + r).ceil() as i32).clamp(0, self.size[0] as i32 - 1) as u32;
        let z0 = ((cz - r).floor() as i32).clamp(0, self.size[2] as i32 - 1) as u32;
        let z1 = ((cz + r).ceil() as i32).clamp(0, self.size[2] as i32 - 1) as u32;

        let surface_id = self.materials.iter().position(|s| s == &materials.surface);
        let soil_id = self
            .materials
            .iter()
            .position(|s| s == &materials.subsurface);
        let rock_id = self.materials.iter().position(|s| s == &materials.rock);
        let mut noise = FastNoiseLite::with_seed(seed ^ 0x5a17_39c1);
        noise.set_noise_type(Some(NoiseType::OpenSimplex2));
        noise.set_frequency(Some(settings.material_noise_scale));
        let mut slope_noise = FastNoiseLite::with_seed(seed ^ 0x2d41_77ab);
        slope_noise.set_noise_type(Some(NoiseType::OpenSimplex2));
        slope_noise.set_frequency(Some(settings.material_noise_scale * 0.73));
        let width = (x1 - x0 + 1) as usize;
        let depth = (z1 - z0 + 1) as usize;
        let mut heights = vec![None; width * depth];
        for z in z0..=z1 {
            for x in x0..=x1 {
                let wx = min[0] + (x as f32 + 0.5) * spacing;
                let wz = min[2] + (z as f32 + 0.5) * spacing;
                heights[(z - z0) as usize * width + (x - x0) as usize] =
                    self.column_surface_height(x, z);
            }
        }
        let height = |x: u32, z: u32| -> Option<f32> {
            heights[(z - z0) as usize * width + (x - x0) as usize]
        };
        for z in z0..=z1 {
            for x in x0..=x1 {
                let Some(surface) = height(x, z) else {
                    continue;
                };
                let west = height(x.saturating_sub(1).max(x0), z).unwrap_or(surface);
                let east = height((x + 1).min(x1), z).unwrap_or(surface);
                let north = height(x, z.saturating_sub(1).max(z0)).unwrap_or(surface);
                let south = height(x, (z + 1).min(z1)).unwrap_or(surface);
                let dx = (east - west)
                    / ((x + 1).min(x1) - x.saturating_sub(1).max(x0)).max(1) as f32
                    / spacing;
                let dz = (south - north)
                    / ((z + 1).min(z1) - z.saturating_sub(1).max(z0)).max(1) as f32
                    / spacing;
                let slope = (dx * dx + dz * dz).sqrt().atan().to_degrees();
                let wx = min[0] + (x as f32 + 0.5) * spacing;
                let wz = min[2] + (z as f32 + 0.5) * spacing;
                let weight = rock_slope_weight(
                    slope,
                    settings.rock_slope_degrees,
                    settings.rock_slope_blend_degrees,
                );
                let breakup = (slope_noise.get_noise_2d(wx, wz) * 0.5 + 0.5).clamp(0.0, 1.0);
                let steep = weight >= 1.0 || (weight > 0.0 && breakup <= weight);
                let high = settings.rock_altitude_studs.is_some_and(|v| surface >= v);
                let offset = noise.get_noise_2d(wx, wz) * settings.material_noise_strength_studs;
                let band = (settings.surface_depth_studs + offset).max(0.0);
                let soil_band = (settings.subsurface_depth_studs - offset * 0.5).max(0.0);
                for y in 0..self.size[1] {
                    let index = self.index(x, y, z).unwrap();
                    let voxel = self.voxels[index];
                    if voxel.is_empty() {
                        continue;
                    }
                    let material_index = voxel.material as usize;
                    if ![surface_id, soil_id, rock_id]
                        .into_iter()
                        .flatten()
                        .any(|id| id == material_index)
                    {
                        continue;
                    }
                    let top = min[1] + (y + 1) as f32 * spacing;
                    let below = (surface - top).max(0.0);
                    let new_role = if (steep || high) && below <= band + soil_band {
                        rock_id
                    } else if below < band {
                        surface_id
                    } else if below < band + soil_band {
                        soil_id
                    } else {
                        rock_id
                    };
                    if let Some(id) = new_role {
                        self.voxels[index].material = id as u16;
                    }
                }
            }
        }
        let _ = water_level;
    }

    fn set_column_surface_height(&mut self, x: u32, z: u32, old_surface: f32, surface: f32) {
        let previous: Vec<SmoothVoxel> = (0..self.size[1])
            .map(|y| self.voxel(x, y, z).unwrap())
            .collect();
        let upper_occupied = previous.iter().rposition(|voxel| !voxel.is_empty());
        let surface_material = upper_occupied
            .map(|index| previous[index].material)
            .unwrap_or(0);
        let vertical_shift = surface - old_surface;
        let max_y = self.size[1] as i32 - 1;
        for y in 0..self.size[1] {
            let index = self.index(x, y, z).unwrap();
            let bottom = self.origin[1] as f32 + y as f32 * self.spacing_studs;
            let occupancy = ((surface - bottom) / self.spacing_studs).clamp(0.0, 1.0);
            let source_y =
                (((bottom + self.spacing_studs * 0.5 - vertical_shift - self.origin[1] as f32)
                    / self.spacing_studs)
                    .floor() as i32)
                    .clamp(0, max_y) as usize;
            let material = if previous[source_y].is_empty() {
                surface_material
            } else {
                previous[source_y].material
            };
            self.voxels[index] = SmoothVoxel::from_unit_occupancy(material, occupancy);
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct VoxelLayerSettings {
    pub surface_depth_studs: f32,
    pub subsurface_depth_studs: f32,
    pub rock_slope_degrees: f32,
    #[serde(default = "default_rock_slope_blend_degrees")]
    pub rock_slope_blend_degrees: f32,
    #[serde(default = "default_slope_shading_strength")]
    pub slope_shading_strength: f32,
    pub material_noise_scale: f32,
    pub material_noise_strength_studs: f32,
    pub rock_altitude_studs: Option<f32>,
}

fn default_rock_slope_blend_degrees() -> f32 {
    12.0
}

fn default_slope_shading_strength() -> f32 {
    0.18
}

impl Default for VoxelLayerSettings {
    fn default() -> Self {
        Self {
            surface_depth_studs: 4.0,
            subsurface_depth_studs: 12.0,
            rock_slope_degrees: 50.0,
            rock_slope_blend_degrees: default_rock_slope_blend_degrees(),
            slope_shading_strength: default_slope_shading_strength(),
            material_noise_scale: 0.045,
            material_noise_strength_studs: 1.5,
            rock_altitude_studs: None,
        }
    }
}

impl VoxelLayerSettings {
    pub fn validate(self) -> Result<(), SmoothVoxelGridError> {
        if !self.surface_depth_studs.is_finite() || self.surface_depth_studs < 0.0 {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "surface depth must be finite and zero or greater",
            ));
        }
        if !self.subsurface_depth_studs.is_finite() || self.subsurface_depth_studs < 0.0 {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "subsurface depth must be finite and zero or greater",
            ));
        }
        if !self.rock_slope_degrees.is_finite() || !(0.0..=90.0).contains(&self.rock_slope_degrees)
        {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "rock slope must be finite and between 0 and 90 degrees",
            ));
        }
        if !self.rock_slope_blend_degrees.is_finite()
            || !(0.0..=90.0).contains(&self.rock_slope_blend_degrees)
        {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "rock slope blend must be finite and between 0 and 90 degrees",
            ));
        }
        if !self.slope_shading_strength.is_finite()
            || !(0.0..=0.75).contains(&self.slope_shading_strength)
        {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "slope shading strength must be finite and between 0 and 0.75",
            ));
        }
        if !self.material_noise_scale.is_finite() || self.material_noise_scale <= 0.0 {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "material noise scale must be finite and greater than zero",
            ));
        }
        if !self.material_noise_strength_studs.is_finite()
            || self.material_noise_strength_studs < 0.0
        {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "material noise strength must be finite and zero or greater",
            ));
        }
        if self
            .rock_altitude_studs
            .is_some_and(|value| !value.is_finite())
        {
            return Err(SmoothVoxelGridError::InvalidLayerSetting(
                "rock altitude must be finite when configured",
            ));
        }
        Ok(())
    }
}

pub fn voxelize_heightfield_layers(
    heightfield: &HeightfieldData,
    materials: &TerrainMaterials,
    settings: VoxelLayerSettings,
) -> Result<SmoothVoxelGrid, SmoothVoxelGridError> {
    voxelize_heightfield_layers_seeded(heightfield, materials, settings, 0)
}

pub fn voxelize_heightfield_layers_seeded(
    heightfield: &HeightfieldData,
    materials: &TerrainMaterials,
    settings: VoxelLayerSettings,
    seed: i32,
) -> Result<SmoothVoxelGrid, SmoothVoxelGridError> {
    settings.validate()?;
    for (axis, extent) in [
        ("x", heightfield.size[0]),
        ("y", heightfield.size[1]),
        ("z", heightfield.size[2]),
    ] {
        if extent == 0 {
            return Err(SmoothVoxelGridError::ZeroSize { axis });
        }
    }

    let spacing_studs = DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS;
    let cells_per_axis = [
        ((heightfield.size[0] as f32 / spacing_studs).ceil() as u32).max(1),
        ((heightfield.size[1] as f32 / spacing_studs).ceil() as u32).max(1),
        ((heightfield.size[2] as f32 / spacing_studs).ceil() as u32).max(1),
    ];

    let surface_name = if materials.surface.trim().is_empty() {
        TERRAIN_FALLBACK_SURFACE_MATERIAL
    } else {
        materials.surface.as_str()
    };
    let subsurface_name = if materials.subsurface.trim().is_empty() {
        TERRAIN_FALLBACK_SUBSURFACE_MATERIAL
    } else {
        materials.subsurface.as_str()
    };
    let rock_name = if materials.rock.trim().is_empty() {
        TERRAIN_FALLBACK_ROCK_MATERIAL
    } else {
        materials.rock.as_str()
    };

    let mut palette = Vec::new();
    let surface_material = intern_material(&mut palette, surface_name);
    let subsurface_material = intern_material(&mut palette, subsurface_name);
    let rock_material = intern_material(&mut palette, rock_name);
    let mut grid = SmoothVoxelGrid::empty_with_spacing(
        heightfield.origin,
        cells_per_axis,
        spacing_studs,
        palette,
    )?;

    let mut material_noise = FastNoiseLite::with_seed(seed ^ 0x5a17_39c1);
    material_noise.set_noise_type(Some(NoiseType::OpenSimplex2));
    material_noise.set_frequency(Some(settings.material_noise_scale));
    let mut slope_noise = FastNoiseLite::with_seed(seed ^ 0x2d41_77ab);
    slope_noise.set_noise_type(Some(NoiseType::OpenSimplex2));
    slope_noise.set_frequency(Some(settings.material_noise_scale * 0.73));

    let world_max_y = heightfield.origin[1] as f32 + heightfield.size[1] as f32;
    for z in 0..grid.size[2] {
        let world_z = sample_axis(heightfield.origin[2], heightfield.size[2], z, spacing_studs);
        for x in 0..grid.size[0] {
            let world_x = sample_axis(heightfield.origin[0], heightfield.size[0], x, spacing_studs);
            let surface_height = heightfield
                .surface_height_at(world_x, world_z)
                .unwrap_or(heightfield.origin[1] as f32)
                .min(world_max_y);
            let slope = estimate_slope_degrees(heightfield, world_x, world_z, surface_height);
            let slope_rock_weight = rock_slope_weight(
                slope,
                settings.rock_slope_degrees,
                settings.rock_slope_blend_degrees,
            );
            let slope_breakup =
                (slope_noise.get_noise_2d(world_x, world_z) * 0.5 + 0.5).clamp(0.0, 1.0);
            let steep = slope_rock_weight >= 1.0
                || (slope_rock_weight > 0.0 && slope_breakup <= slope_rock_weight);
            let high_rock = settings
                .rock_altitude_studs
                .is_some_and(|threshold| surface_height >= threshold);
            let boundary_noise = material_noise.get_noise_2d(world_x, world_z)
                * settings.material_noise_strength_studs;
            let surface_band = (settings.surface_depth_studs + boundary_noise).max(0.0);
            let subsurface_band = (settings.subsurface_depth_studs - boundary_noise * 0.5).max(0.0);

            for y in 0..grid.size[1] {
                let bottom = heightfield.origin[1] as f32 + y as f32 * spacing_studs;
                let occupancy = ((surface_height - bottom) / spacing_studs).clamp(0.0, 1.0);
                if occupancy <= 0.0 {
                    continue;
                }

                let top = bottom + spacing_studs;
                let depth_below_surface = (surface_height - top).max(0.0);
                let material = if (steep || high_rock)
                    && depth_below_surface <= surface_band + subsurface_band
                {
                    rock_material
                } else if depth_below_surface < surface_band {
                    surface_material
                } else if depth_below_surface < surface_band + subsurface_band {
                    subsurface_material
                } else {
                    rock_material
                };

                grid.set_voxel(
                    x,
                    y,
                    z,
                    SmoothVoxel::from_unit_occupancy(material, occupancy),
                );
            }
        }
    }

    Ok(grid)
}

pub fn voxelize_heightmap_layers(
    origin: [i32; 3],
    size: [u32; 3],
    heights: &[f32],
    materials: &TerrainMaterials,
    settings: VoxelLayerSettings,
    seed: i32,
) -> Result<SmoothVoxelGrid, SmoothVoxelGridError> {
    let expected = size[0] as usize * size[2] as usize;
    if heights.len() != expected {
        return Err(SmoothVoxelGridError::HeightmapValueCountMismatch {
            expected,
            actual: heights.len(),
        });
    }
    if heights.iter().any(|height| !height.is_finite()) {
        return Err(SmoothVoxelGridError::InvalidHeightSample);
    }
    let heightfield = HeightfieldData {
        origin,
        size,
        heights: heights.to_vec(),
    };
    voxelize_heightfield_layers_seeded(&heightfield, materials, settings, seed)
}

pub fn apply_surface_material_map(
    grid: &mut SmoothVoxelGrid,
    map_size: [u32; 2],
    material_indices: &[u16],
) -> Result<(), SmoothVoxelGridError> {
    if map_size != [grid.size[0], grid.size[2]] {
        return Err(SmoothVoxelGridError::MaterialMapSizeMismatch {
            expected: [grid.size[0], grid.size[2]],
            actual: map_size,
        });
    }
    let expected = map_size[0] as usize * map_size[1] as usize;
    if material_indices.len() != expected {
        return Err(SmoothVoxelGridError::MaterialMapValueCountMismatch {
            expected,
            actual: material_indices.len(),
        });
    }
    for (index, &material) in material_indices.iter().enumerate() {
        if material as usize >= grid.materials.len() {
            return Err(SmoothVoxelGridError::MaterialIndexOutOfRange {
                material,
                palette_len: grid.materials.len(),
            });
        }
        let x = index as u32 % map_size[0];
        let z = index as u32 / map_size[0];
        if let Some(y) = (0..grid.size[1])
            .rev()
            .find(|&y| grid.voxel(x, y, z).is_some_and(|voxel| !voxel.is_empty()))
        {
            let voxel_index = grid.index(x, y, z).unwrap();
            grid.voxels[voxel_index].material = material;
        }
    }
    Ok(())
}

pub fn apply_surface_color_map(
    grid: &mut SmoothVoxelGrid,
    map_size: [u32; 2],
    colors: &[[u8; 3]],
    color_to_material: &[([u8; 3], u16)],
) -> Result<(), SmoothVoxelGridError> {
    let expected = map_size[0] as usize * map_size[1] as usize;
    if colors.len() != expected {
        return Err(SmoothVoxelGridError::MaterialMapValueCountMismatch {
            expected,
            actual: colors.len(),
        });
    }
    let mut mapped = Vec::with_capacity(colors.len());
    for &color in colors {
        let material = color_to_material
            .iter()
            .find_map(|(candidate, material)| (*candidate == color).then_some(*material))
            .ok_or(SmoothVoxelGridError::UnmappedMaterialColor(color))?;
        mapped.push(material);
    }
    apply_surface_material_map(grid, map_size, &mapped)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn checked_volume(size: [u32; 3]) -> Result<usize, SmoothVoxelGridError> {
    for (axis, extent) in [("x", size[0]), ("y", size[1]), ("z", size[2])] {
        if extent == 0 {
            return Err(SmoothVoxelGridError::ZeroSize { axis });
        }
    }
    let volume = (size[0] as u64)
        .checked_mul(size[1] as u64)
        .and_then(|value| value.checked_mul(size[2] as u64))
        .ok_or(SmoothVoxelGridError::VolumeOverflow)?;
    usize::try_from(volume).map_err(|_| SmoothVoxelGridError::VolumeOverflow)
}

fn intern_material(palette: &mut Vec<String>, material: &str) -> u16 {
    if let Some(index) = palette.iter().position(|candidate| candidate == material) {
        return index as u16;
    }
    let index = palette.len();
    debug_assert!(index <= u16::MAX as usize);
    palette.push(material.to_string());
    index as u16
}

pub(crate) const BEACH_ABOVE_WATER_STUDS: f32 = 2.0;

pub(crate) fn apply_waterline_beach_material(
    grid: &mut SmoothVoxelGrid,
    water_level: f32,
    beach_material_name: &str,
    surface_material_name: &str,
    subsurface_material_name: &str,
    rock_material_name: &str,
) -> usize {
    if !water_level.is_finite()
        || beach_material_name.trim().is_empty()
        || surface_material_name.trim().is_empty()
        || subsurface_material_name.trim().is_empty()
    {
        return 0;
    }

    let beach_material = intern_material(&mut grid.materials, beach_material_name);
    let surface_material = grid
        .materials
        .iter()
        .position(|name| name == surface_material_name)
        .map(|index| index as u16);
    let subsurface_material = grid
        .materials
        .iter()
        .position(|name| name == subsurface_material_name)
        .map(|index| index as u16);
    let rock_material = grid
        .materials
        .iter()
        .position(|name| name == rock_material_name)
        .map(|index| index as u16);

    let (min, _) = grid.world_bounds();
    let mut changed = 0;

    for z in 0..grid.size[2] {
        for x in 0..grid.size[0] {
            let world_x = min[0] + (x as f32 + 0.5) * grid.spacing_studs;
            let world_z = min[2] + (z as f32 + 0.5) * grid.spacing_studs;
            let Some(surface_height) = grid.surface_height_at(world_x, world_z) else {
                continue;
            };

            if surface_height > water_level + BEACH_ABOVE_WATER_STUDS {
                continue;
            }

            for y in 0..grid.size[1] {
                let Some(mut voxel) = grid.voxel(x, y, z) else {
                    continue;
                };
                if voxel.is_empty() || Some(voxel.material) == rock_material {
                    continue;
                }

                let is_soil_role = Some(voxel.material) == surface_material
                    || Some(voxel.material) == subsurface_material;
                if !is_soil_role || voxel.material == beach_material {
                    continue;
                }

                voxel.material = beach_material;
                if grid.set_voxel(x, y, z, voxel) {
                    changed += 1;
                }
            }
        }
    }

    changed
}

fn sample_axis(origin: i32, extent: u32, sample_index: u32, spacing_studs: f32) -> f32 {
    let min = origin as f32;
    let max = min + extent as f32;
    (min + (sample_index as f32 + 0.5) * spacing_studs).clamp(min, max - 0.001)
}

fn rock_slope_weight(slope_degrees: f32, center_degrees: f32, blend_degrees: f32) -> f32 {
    if blend_degrees <= f32::EPSILON {
        return if slope_degrees >= center_degrees {
            1.0
        } else {
            0.0
        };
    }
    let half = blend_degrees * 0.5;
    let start = (center_degrees - half).clamp(0.0, 90.0);
    let end = (center_degrees + half).clamp(0.0, 90.0);
    if end <= start + f32::EPSILON {
        return if slope_degrees >= center_degrees {
            1.0
        } else {
            0.0
        };
    }
    let t = ((slope_degrees - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn estimate_slope_degrees(
    heightfield: &HeightfieldData,
    world_x: f32,
    world_z: f32,
    center_height: f32,
) -> f32 {
    let min_x = heightfield.origin[0] as f32;
    let max_x = min_x + heightfield.size[0] as f32 - 0.001;
    let min_z = heightfield.origin[2] as f32;
    let max_z = min_z + heightfield.size[2] as f32 - 0.001;
    let west_x = (world_x - 1.0).clamp(min_x, max_x);
    let east_x = (world_x + 1.0).clamp(min_x, max_x);
    let north_z = (world_z - 1.0).clamp(min_z, max_z);
    let south_z = (world_z + 1.0).clamp(min_z, max_z);

    let west = heightfield
        .surface_height_at(west_x, world_z)
        .unwrap_or(center_height);
    let east = heightfield
        .surface_height_at(east_x, world_z)
        .unwrap_or(center_height);
    let north = heightfield
        .surface_height_at(world_x, north_z)
        .unwrap_or(center_height);
    let south = heightfield
        .surface_height_at(world_x, south_z)
        .unwrap_or(center_height);

    let dx = (east - west) / (east_x - west_x).abs().max(0.001);
    let dz = (south - north) / (south_z - north_z).abs().max(0.001);
    (dx.mul_add(dx, dz * dz).sqrt()).atan().to_degrees()
}

#[derive(Clone, Debug, PartialEq)]
pub enum SmoothVoxelGridError {
    InvalidSpacing(f32),
    ZeroSize {
        axis: &'static str,
    },
    VolumeOverflow,
    PaletteTooLarge {
        actual: usize,
    },
    VoxelCountMismatch {
        expected: usize,
        actual: usize,
    },
    MaterialIndexOutOfRange {
        material: u16,
        palette_len: usize,
    },
    HeightmapValueCountMismatch {
        expected: usize,
        actual: usize,
    },
    InvalidHeightSample,
    MaterialMapSizeMismatch {
        expected: [u32; 2],
        actual: [u32; 2],
    },
    MaterialMapValueCountMismatch {
        expected: usize,
        actual: usize,
    },
    UnmappedMaterialColor([u8; 3]),
    InvalidLayerSetting(&'static str),
}

impl fmt::Display for SmoothVoxelGridError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpacing(value) => write!(
                formatter,
                "Terrain sample spacing must be finite and greater than zero; received {value}"
            ),
            Self::ZeroSize { axis } => write!(formatter, "Smooth voxel grid {axis} size is zero"),
            Self::VolumeOverflow => write!(formatter, "Smooth voxel grid volume overflowed"),
            Self::PaletteTooLarge { actual } => write!(
                formatter,
                "Smooth voxel material palette has {actual} entries; maximum is {}",
                u16::MAX as usize + 1
            ),
            Self::VoxelCountMismatch { expected, actual } => write!(
                formatter,
                "Smooth voxel dense array expected {expected} values but received {actual}"
            ),
            Self::MaterialIndexOutOfRange {
                material,
                palette_len,
            } => write!(
                formatter,
                "Smooth voxel material index {material} is outside palette length {palette_len}"
            ),
            Self::HeightmapValueCountMismatch { expected, actual } => write!(
                formatter,
                "Heightmap expected {expected} values but received {actual}"
            ),
            Self::InvalidHeightSample => write!(formatter, "Heightmap contains a non-finite value"),
            Self::MaterialMapSizeMismatch { expected, actual } => write!(
                formatter,
                "Material map size {:?} does not match voxel XZ size {:?}",
                actual, expected
            ),
            Self::MaterialMapValueCountMismatch { expected, actual } => write!(
                formatter,
                "Material map expected {expected} values but received {actual}"
            ),
            Self::UnmappedMaterialColor(color) => write!(
                formatter,
                "Material color [{}, {}, {}] has no palette mapping",
                color[0], color[1], color[2]
            ),
            Self::InvalidLayerSetting(reason) => {
                write!(formatter, "Invalid smooth voxel layer setting: {reason}")
            }
        }
    }
}

impl std::error::Error for SmoothVoxelGridError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_sample_spacing_controls_world_mapping() {
        let grid = SmoothVoxelGrid::from_dense_with_spacing(
            [10, -2, 20],
            [2, 2, 2],
            1.5,
            vec!["rock".into()],
            vec![SmoothVoxel::from_unit_occupancy(0, 1.0); 8],
        )
        .unwrap();

        assert_eq!(grid.spacing_studs, 1.5);
        assert_eq!(grid.voxel_world_min(1, 1, 1), Some([11.5, -0.5, 21.5]));
        assert_eq!(grid.world_bounds(), ([10.0, -2.0, 20.0], [13.0, 1.0, 23.0]));
    }

    #[test]
    fn legacy_dense_constructor_keeps_historical_four_stud_spacing() {
        let grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [1, 1, 1],
            vec!["rock".into()],
            vec![SmoothVoxel::from_unit_occupancy(0, 1.0)],
        )
        .unwrap();

        assert_eq!(grid.spacing_studs, LEGACY_TERRAIN_SAMPLE_SPACING_STUDS);
        assert_eq!(grid.world_bounds().1, [4.0, 4.0, 4.0]);
    }

    #[test]
    fn generated_heightfield_voxelization_uses_explicit_default_spacing() {
        let heightfield = HeightfieldData {
            origin: [0, 0, 0],
            size: [4, 4, 4],
            heights: vec![2.0; 16],
        };
        let grid =
            voxelize_heightfield_layers(&heightfield, &materials(), VoxelLayerSettings::default())
                .unwrap();

        assert_eq!(grid.spacing_studs, DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS);
        assert_eq!(grid.size, [2, 2, 2]);
        assert_eq!(grid.world_bounds().1, [4.0, 4.0, 4.0]);
    }

    #[test]
    fn surface_height_center_step_matches_eighth_step_reference_on_irregular_terrain() {
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [4, 12, 4], 2.0, vec!["rock".into()])
                .unwrap();
        for z in 0..4 {
            for x in 0..4 {
                let top = 3 + (x + 2 * z) % 5;
                for y in 0..=top {
                    grid.set_voxel(
                        x,
                        y,
                        z,
                        SmoothVoxel {
                            occupancy: if y == top { 128 } else { 255 },
                            material: 0,
                        },
                    );
                }
            }
        }
        let reference = |wx: f32, wz: f32| {
            let (min, max) = grid.world_bounds();
            let step = grid.spacing_studs * 0.125;
            let mut upper = max[1] + grid.spacing_studs * 0.5;
            let mut upper_density = grid.density_at_world([wx, upper, wz]);
            let mut lower = upper - step;
            while lower >= min[1] - grid.spacing_studs * 0.5 {
                let lower_density = grid.density_at_world([wx, lower, wz]);
                if upper_density < SMOOTH_VOXEL_ISO_LEVEL && lower_density >= SMOOTH_VOXEL_ISO_LEVEL
                {
                    let (mut inside, mut outside) = (lower, upper);
                    for _ in 0..12 {
                        let mid = (inside + outside) * 0.5;
                        if grid.density_at_world([wx, mid, wz]) >= SMOOTH_VOXEL_ISO_LEVEL {
                            inside = mid;
                        } else {
                            outside = mid;
                        }
                    }
                    return Some((inside + outside) * 0.5);
                }
                upper = lower;
                upper_density = lower_density;
                lower -= step;
            }
            None
        };
        let samples = [0.5, 1.0, 2.5, 3.5, 5.0, 6.8, 7.5];
        for x in samples {
            for z in samples {
                let slow = reference(x, z);
                let fast = grid.surface_height_at(x, z);
                assert_eq!(slow.is_some(), fast.is_some(), "{x}, {z}");
                if let (Some(slow), Some(fast)) = (slow, fast) {
                    assert!(
                        (slow - fast).abs() < 0.002,
                        "fast {fast}, slow {slow} at ({x},{z})"
                    );
                }
            }
        }
    }

    #[test]
    fn sculpted_height_moves_surface_subsurface_and_rock_layers_together() {
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [0, 0, 0],
            [1, 5, 1],
            2.0,
            vec![
                "grass".into(),
                "dirt".into(),
                "rock".into(),
                "custom_paint".into(),
            ],
        )
        .unwrap();
        for (y, material) in [(0, 2), (1, 1), (2, 3)] {
            grid.set_voxel(
                0,
                y,
                0,
                SmoothVoxel {
                    occupancy: 255,
                    material,
                },
            );
        }
        grid.set_column_surface_height(0, 0, 6.0, 8.0);
        assert_eq!(grid.voxel(0, 0, 0).unwrap().material, 2);
        assert_eq!(grid.voxel(0, 1, 0).unwrap().material, 2);
        assert_eq!(grid.voxel(0, 2, 0).unwrap().material, 1);
        assert_eq!(grid.voxel(0, 3, 0).unwrap().material, 3);
        assert_eq!(grid.voxel(0, 3, 0).unwrap().occupancy, 255);
        assert!(grid.voxel(0, 4, 0).unwrap().is_empty());
    }

    #[test]
    fn paint_brush_recolors_exposed_beach_wall_without_painting_buried_voxels() {
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [0, 0, 0],
            [3, 3, 3],
            2.0,
            vec!["sand".into(), "grass".into()],
        )
        .unwrap();
        for z in 0..3 {
            for y in 0..3 {
                for x in 0..3 {
                    grid.set_voxel(
                        x,
                        y,
                        z,
                        SmoothVoxel {
                            occupancy: 255,
                            material: 0,
                        },
                    );
                }
            }
        }
        assert!(grid.paint_surface_material(&TerrainPaintRequest {
            strength: 1.0,
            falloff: None,
            center: [0.0, 3.0, 3.0],
            radius: 0.6,
            material: "grass".into(),
        }));
        assert_eq!(grid.voxel(0, 1, 1).unwrap().material, 1);
        assert_eq!(grid.voxel(0, 2, 1).unwrap().material, 0);
        assert_eq!(grid.voxel(1, 1, 1).unwrap().material, 0);
        assert_eq!(grid.voxel(2, 1, 1).unwrap().material, 0);
        assert_eq!(grid.voxel(0, 1, 1).unwrap().occupancy, 255);
    }

    fn materials() -> TerrainMaterials {
        TerrainMaterials {
            surface: "grass".into(),
            subsurface: "dirt".into(),
            rock: "rock".into(),
            water: "water".into(),
        }
    }

    fn constant_heightfield(size: [u32; 3], height: f32) -> HeightfieldData {
        HeightfieldData {
            origin: [0, 0, 0],
            size,
            heights: vec![height; size[0] as usize * size[2] as usize],
        }
    }

    #[test]
    fn dense_grid_uses_four_stud_xyz_lattice_and_stable_indexing() {
        let mut grid = SmoothVoxelGrid::empty([8, -4, 12], [2, 2, 2], vec!["rock".into()]).unwrap();
        let voxel = SmoothVoxel::from_unit_occupancy(0, 0.5);
        assert!(grid.set_voxel(1, 1, 1, voxel));

        assert_eq!(grid.index(0, 0, 0), Some(0));
        assert_eq!(grid.index(1, 0, 0), Some(1));
        assert_eq!(grid.index(0, 0, 1), Some(2));
        assert_eq!(grid.index(0, 1, 0), Some(4));
        assert_eq!(grid.voxel(1, 1, 1), Some(voxel));
        assert_eq!(grid.voxel_world_min(1, 1, 1), Some([12.0, 0.0, 16.0]));
    }

    #[test]
    fn filled_voxel_requires_existing_palette_material() {
        assert!(
            SmoothVoxelGrid::from_dense(
                [0, 0, 0],
                [1, 1, 1],
                Vec::new(),
                vec![SmoothVoxel::from_unit_occupancy(0, 1.0)],
            )
            .is_err()
        );

        let mut grid = SmoothVoxelGrid::empty([0, 0, 0], [1, 1, 1], Vec::new()).unwrap();
        assert!(!grid.set_voxel(0, 0, 0, SmoothVoxel::from_unit_occupancy(0, 1.0)));
    }

    #[test]
    fn heightfield_bridge_preserves_partial_occupancy() {
        let heightfield = constant_heightfield([8, 8, 8], 5.5);
        let grid =
            voxelize_heightfield_layers(&heightfield, &materials(), VoxelLayerSettings::default())
                .unwrap();

        assert_eq!(grid.size, [4, 4, 4]);
        assert_eq!(grid.world_bounds().1, [8.0, 8.0, 8.0]);
        assert_eq!(grid.voxel(0, 0, 0).unwrap().occupancy, 255);
        assert_eq!(grid.voxel(0, 1, 0).unwrap().occupancy, 255);
        let partial = grid.voxel(0, 2, 0).unwrap();
        assert!((partial.occupancy_unit() - 0.75).abs() < 0.01);
    }

    #[test]
    fn depth_rules_stack_surface_subsurface_and_rock() {
        let heightfield = constant_heightfield([4, 16, 4], 15.0);
        let settings = VoxelLayerSettings {
            surface_depth_studs: 4.0,
            subsurface_depth_studs: 4.0,
            rock_slope_degrees: 90.0,
            material_noise_strength_studs: 0.0,
            ..VoxelLayerSettings::default()
        };
        let grid = voxelize_heightfield_layers(&heightfield, &materials(), settings).unwrap();

        let material_at = |y| {
            let voxel = grid.voxel(0, y, 0).unwrap();
            grid.material_name(voxel.material).unwrap()
        };
        assert_eq!(grid.spacing_studs, DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS);
        assert_eq!(material_at(7), "grass");
        assert_eq!(material_at(6), "grass");
        assert_eq!(material_at(4), "dirt");
        assert_eq!(material_at(2), "rock");
    }

    #[test]
    fn steep_surface_switches_near_surface_layer_to_rock() {
        let size = [8, 16, 4];
        let mut heights = Vec::new();
        for _z in 0..size[2] {
            for x in 0..size[0] {
                heights.push((x as f32 * 2.0 + 1.0).min(15.0));
            }
        }
        let heightfield = HeightfieldData {
            origin: [0, 0, 0],
            size,
            heights,
        };
        let settings = VoxelLayerSettings {
            rock_slope_degrees: 30.0,
            rock_slope_blend_degrees: 0.0,
            ..VoxelLayerSettings::default()
        };
        let grid = voxelize_heightfield_layers(&heightfield, &materials(), settings).unwrap();

        let mut found_rock_surface = false;
        for y in (0..grid.size[1]).rev() {
            let voxel = grid.voxel(0, y, 0).unwrap();
            if !voxel.is_empty() {
                found_rock_surface = grid.material_name(voxel.material) == Some("rock");
                break;
            }
        }
        assert!(found_rock_surface);
    }

    #[test]
    fn slope_rock_transition_blends_around_configured_angle() {
        assert_eq!(rock_slope_weight(39.9, 50.0, 20.0), 0.0);
        assert!((rock_slope_weight(50.0, 50.0, 20.0) - 0.5).abs() < 0.001);
        assert_eq!(rock_slope_weight(60.1, 50.0, 20.0), 1.0);
        assert_eq!(rock_slope_weight(49.9, 50.0, 0.0), 0.0);
        assert_eq!(rock_slope_weight(50.0, 50.0, 0.0), 1.0);
    }

    #[test]
    fn layer_settings_missing_new_slope_fields_enable_procedural_defaults() {
        let mut value = serde_json::to_value(VoxelLayerSettings::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("rock_slope_blend_degrees");
        value
            .as_object_mut()
            .unwrap()
            .remove("slope_shading_strength");
        let decoded: VoxelLayerSettings = serde_json::from_value(value).unwrap();
        assert_eq!(
            decoded.rock_slope_blend_degrees,
            default_rock_slope_blend_degrees()
        );
        assert!(
            (decoded.slope_shading_strength - default_slope_shading_strength()).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn empty_material_slots_keep_surface_subsurface_and_rock_distinct() {
        let heightfield = constant_heightfield([4, 16, 4], 15.0);
        let materials = TerrainMaterials {
            surface: String::new(),
            subsurface: String::new(),
            rock: String::new(),
            water: String::new(),
        };
        let settings = VoxelLayerSettings {
            surface_depth_studs: 4.0,
            subsurface_depth_studs: 4.0,
            rock_slope_degrees: 90.0,
            rock_slope_blend_degrees: 0.0,
            material_noise_strength_studs: 0.0,
            ..VoxelLayerSettings::default()
        };
        let grid = voxelize_heightfield_layers(&heightfield, &materials, settings).unwrap();

        assert_eq!(
            grid.materials,
            vec![
                TERRAIN_FALLBACK_SURFACE_MATERIAL.to_string(),
                TERRAIN_FALLBACK_SUBSURFACE_MATERIAL.to_string(),
                TERRAIN_FALLBACK_ROCK_MATERIAL.to_string(),
            ]
        );
    }

    #[test]
    fn beach_replaces_surface_and_subsurface_underwater_but_preserves_rock() {
        let heightfield = HeightfieldData {
            origin: [0, 0, 0],
            size: [6, 16, 2],
            heights: vec![
                7.0, 9.0, 12.0, 7.0, 9.0, 12.0, 7.0, 9.0, 12.0, 7.0, 9.0, 12.0,
            ],
        };
        let materials = TerrainMaterials {
            surface: "grass_1".into(),
            subsurface: "dirt".into(),
            rock: "tile_24".into(),
            water: "water".into(),
        };
        let mut grid = voxelize_heightfield_layers(
            &heightfield,
            &materials,
            VoxelLayerSettings {
                surface_depth_studs: 2.0,
                subsurface_depth_studs: 2.0,
                rock_slope_degrees: 90.0,
                rock_slope_blend_degrees: 0.0,
                material_noise_strength_studs: 0.0,
                ..VoxelLayerSettings::default()
            },
        )
        .unwrap();

        let rock = grid
            .materials
            .iter()
            .position(|name| name == "tile_24")
            .unwrap() as u16;
        let rock_before: Vec<_> = grid
            .voxels
            .iter()
            .enumerate()
            .filter_map(|(index, voxel)| (voxel.material == rock).then_some(index))
            .collect();

        let changed =
            apply_waterline_beach_material(&mut grid, 8.0, "sand", "grass_1", "dirt", "tile_24");
        assert!(changed > 0);

        let sand = grid
            .materials
            .iter()
            .position(|name| name == "sand")
            .unwrap() as u16;

        for y in 0..grid.size[1] {
            let Some(voxel) = grid.voxel(0, y, 0) else {
                continue;
            };
            if voxel.is_empty() || voxel.material == rock {
                continue;
            }
            assert_eq!(voxel.material, sand);
        }

        let shoreline_has_sand = (0..grid.size[1]).any(|y| {
            grid.voxel(1, y, 0)
                .is_some_and(|voxel| !voxel.is_empty() && voxel.material == sand)
        });
        assert!(shoreline_has_sand);

        let high_ground_has_sand = (0..grid.size[1]).any(|y| {
            grid.voxel(2, y, 0)
                .is_some_and(|voxel| !voxel.is_empty() && voxel.material == sand)
        });
        assert!(!high_ground_has_sand);

        for index in rock_before {
            assert_eq!(grid.voxels[index].material, rock);
        }
    }

    #[test]
    fn terrain_paint_changes_only_top_material_and_preserves_occupancy() {
        let mut grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [3, 2, 3],
            vec!["grass_1".into(), "dirt".into()],
            vec![
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
                SmoothVoxel::from_unit_occupancy(0, 1.0),
            ],
        )
        .unwrap();
        let before: Vec<u8> = grid.voxels.iter().map(|voxel| voxel.occupancy).collect();

        assert!(grid.paint_surface_material(&TerrainPaintRequest {
            strength: 1.0,
            falloff: None,
            center: [6.0, 8.0, 6.0],
            radius: 3.0,
            material: "dirt".into(),
        }));

        assert_eq!(
            before,
            grid.voxels
                .iter()
                .map(|voxel| voxel.occupancy)
                .collect::<Vec<_>>()
        );
        let dirt = grid
            .materials
            .iter()
            .position(|name| name == "dirt")
            .unwrap() as u16;
        let center_top = grid.voxel(1, 1, 1).unwrap();
        let center_bottom = grid.voxel(1, 0, 1).unwrap();
        assert_eq!(center_top.material, dirt);
        assert_eq!(center_bottom.material, 0);
    }

    #[test]
    fn terrain_paint_interns_new_material_once_and_rejects_invalid_brushes() {
        let mut grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [1, 1, 1],
            vec!["grass_1".into()],
            vec![SmoothVoxel::from_unit_occupancy(0, 1.0)],
        )
        .unwrap();

        assert!(!grid.paint_surface_material(&TerrainPaintRequest {
            strength: 1.0,
            falloff: None,
            center: [2.0, 2.0, 2.0],
            radius: 0.0,
            material: "dirt".into(),
        }));
        assert!(!grid.paint_surface_material(&TerrainPaintRequest {
            strength: 1.0,
            falloff: None,
            center: [100.0, 2.0, 100.0],
            radius: 1.0,
            material: "never_added".into(),
        }));
        assert!(!grid.materials.iter().any(|name| name == "never_added"));
        assert!(grid.paint_surface_material(&TerrainPaintRequest {
            strength: 1.0,
            falloff: None,
            center: [2.0, 2.0, 2.0],
            radius: 4.0,
            material: "tile_24".into(),
        }));
        assert_eq!(
            grid.materials
                .iter()
                .filter(|name| name.as_str() == "tile_24")
                .count(),
            1
        );
    }

    #[test]
    fn dense_voxel_contract_round_trips_through_serde() {
        let heightfield = constant_heightfield([8, 12, 8], 9.5);
        let grid =
            voxelize_heightfield_layers(&heightfield, &materials(), VoxelLayerSettings::default())
                .unwrap();
        let encoded = serde_json::to_string(&grid).unwrap();
        let decoded: SmoothVoxelGrid = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, grid);
    }

    #[test]
    fn seeded_material_noise_is_deterministic_and_does_not_change_shape_occupancy() {
        let heightfield = constant_heightfield([16, 20, 16], 15.0);
        let settings = VoxelLayerSettings {
            material_noise_strength_studs: 8.0,
            ..VoxelLayerSettings::default()
        };
        let first =
            voxelize_heightfield_layers_seeded(&heightfield, &materials(), settings, 123).unwrap();
        let second =
            voxelize_heightfield_layers_seeded(&heightfield, &materials(), settings, 123).unwrap();
        let no_material_noise = voxelize_heightfield_layers_seeded(
            &heightfield,
            &materials(),
            VoxelLayerSettings {
                material_noise_strength_studs: 0.0,
                ..settings
            },
            123,
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first
                .voxels
                .iter()
                .map(|voxel| voxel.occupancy)
                .collect::<Vec<_>>(),
            no_material_noise
                .voxels
                .iter()
                .map(|voxel| voxel.occupancy)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn altitude_rule_can_force_near_surface_rock() {
        let heightfield = constant_heightfield([8, 20, 8], 15.0);
        let settings = VoxelLayerSettings {
            rock_altitude_studs: Some(10.0),
            rock_slope_degrees: 90.0,
            material_noise_strength_studs: 0.0,
            ..VoxelLayerSettings::default()
        };
        let grid =
            voxelize_heightfield_layers_seeded(&heightfield, &materials(), settings, 77).unwrap();
        let top = (0..grid.size[1])
            .rev()
            .find_map(|y| {
                let voxel = grid.voxel(0, y, 0)?;
                (!voxel.is_empty()).then_some(voxel)
            })
            .unwrap();
        assert_eq!(grid.material_name(top.material), Some("rock"));
    }

    #[test]
    fn heightmap_and_material_maps_share_the_voxel_contract() {
        let mut grid = voxelize_heightmap_layers(
            [0, 0, 0],
            [8, 16, 8],
            &vec![10.0; 64],
            &materials(),
            VoxelLayerSettings {
                material_noise_strength_studs: 0.0,
                ..VoxelLayerSettings::default()
            },
            5,
        )
        .unwrap();
        assert_eq!(grid.size, [4, 8, 4]);
        assert_eq!(grid.world_bounds().1, [8.0, 16.0, 8.0]);

        let mut material_map = vec![2_u16; 16];
        material_map[1] = 1;
        apply_surface_material_map(&mut grid, [4, 4], &material_map).unwrap();
        let top = |grid: &SmoothVoxelGrid, x, z| {
            (0..grid.size[1])
                .rev()
                .find_map(|y| {
                    let voxel = grid.voxel(x, y, z)?;
                    (!voxel.is_empty()).then_some(voxel.material)
                })
                .unwrap()
        };
        assert_eq!(top(&grid, 0, 0), 2);

        let mut color_map = vec![[255, 0, 0]; 16];
        color_map[1] = [0, 255, 0];
        apply_surface_color_map(
            &mut grid,
            [4, 4],
            &color_map,
            &[([255, 0, 0], 0), ([0, 255, 0], 1), ([0, 0, 255], 2)],
        )
        .unwrap();
        assert_eq!(top(&grid, 0, 0), 0);
        assert_eq!(top(&grid, 1, 0), 1);
    }

    #[test]
    fn smooth_density_surface_queries_and_column_sculpt_share_occupancy() {
        let heightfield = constant_heightfield([16, 24, 16], 12.0);
        let mut grid = voxelize_heightfield_layers(
            &heightfield,
            &materials(),
            VoxelLayerSettings {
                material_noise_strength_studs: 0.0,
                ..VoxelLayerSettings::default()
            },
        )
        .unwrap();
        let before = grid.surface_height_at(8.0, 8.0).unwrap();
        assert!(grid.solid_occupies_cell(crate::world::WorldCoord::new(8, 4, 8)));
        assert!(grid.apply_surface_sculpt(
            &TerrainSculptRequest {
                tool: SculptTool::Raise,
                center: [8.0, before, 8.0],
                radius: 5.0,
                strength: 2.0,
                flatten_target: None,
            },
            0.0,
            23.0,
        ));
        assert!(grid.surface_height_at(8.0, 8.0).unwrap() > before);
    }
}
