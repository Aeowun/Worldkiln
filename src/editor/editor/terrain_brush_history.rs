// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{Editor, HistorySnapshot, TerrainHistorySnapshot};
use crate::world::{
    TerrainData, TerrainResult, World,
    terrain::{
        LayerBrushEdit, SculptTool, SmoothVoxel, TerrainState, terrain_render_chunk_samples,
    },
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(super) struct BrushRegionSnapshot {
    layer_id: Option<u64>,
    journal_len: usize,
    edits: Vec<LayerBrushEdit>,
    origin: [i32; 3],
    size: [u32; 3],
    spacing: f32,
    heightfield: bool,
    heights: HashMap<u32, f32>,
    columns: HashMap<(u32, u32), Vec<SmoothVoxel>>,
    chunks: HashSet<[u32; 2]>,
}

impl BrushRegionSnapshot {
    fn result<'a>(&self, terrain: &'a TerrainState) -> Option<&'a TerrainResult> {
        if self.layer_id.is_some() {
            terrain.layered_authored.as_ref()
        } else {
            terrain.authored.as_ref()
        }
    }

    fn matches(&self, result: &TerrainResult) -> bool {
        match &result.data {
            TerrainData::Heightfield(f) => {
                self.heightfield && f.origin == self.origin && f.size == self.size
            }
            TerrainData::Voxel(g) => {
                !self.heightfield
                    && g.origin == self.origin
                    && g.size == self.size
                    && g.spacing_studs == self.spacing
            }
        }
    }

    pub(super) fn capture(&mut self, terrain: &TerrainState, center: [f32; 3], radius: f32) {
        if !radius.is_finite() || radius <= 0.0 || center.iter().any(|n| !n.is_finite()) {
            return;
        }
        let Some(result) = self.result(terrain).filter(|r| self.matches(r)) else {
            return;
        };
        let cx = (center[0] - self.origin[0] as f32) / self.spacing - 0.5;
        let cz = (center[2] - self.origin[2] as f32) / self.spacing - 0.5;
        let r = radius / self.spacing + 2.0;
        let x0 = ((cx - r).floor() as i32).clamp(0, self.size[0] as i32 - 1) as u32;
        let x1 = ((cx + r).ceil() as i32).clamp(0, self.size[0] as i32 - 1) as u32;
        let z0 = ((cz - r).floor() as i32).clamp(0, self.size[2] as i32 - 1) as u32;
        let z1 = ((cz + r).ceil() as i32).clamp(0, self.size[2] as i32 - 1) as u32;
        let chunk = terrain_render_chunk_samples(self.spacing);
        for z in z0..=z1 {
            for x in x0..=x1 {
                match &result.data {
                    TerrainData::Heightfield(f) => {
                        let i = z * self.size[0] + x;
                        self.heights.entry(i).or_insert(f.heights[i as usize]);
                    }
                    TerrainData::Voxel(g) => {
                        self.columns.entry((x, z)).or_insert_with(|| {
                            (0..g.size[1]).map(|y| g.voxel(x, y, z).unwrap()).collect()
                        });
                    }
                }
                for tz in z.saturating_sub(1) / chunk..=(z + 1).min(self.size[2] - 1) / chunk {
                    for tx in x.saturating_sub(1) / chunk..=(x + 1).min(self.size[0] - 1) / chunk {
                        self.chunks.insert([tx, tz]);
                    }
                }
            }
        }
    }

    pub(super) fn counterpart(&self, terrain: &TerrainState) -> Self {
        let mut inverse = self.clone();
        let Some(result) = self.result(terrain).filter(|r| self.matches(r)) else {
            return inverse;
        };
        match &result.data {
            TerrainData::Heightfield(f) => {
                for (&i, value) in &mut inverse.heights {
                    *value = f.heights[i as usize];
                }
            }
            TerrainData::Voxel(g) => {
                for (&(x, z), column) in &mut inverse.columns {
                    for (y, value) in column.iter_mut().enumerate() {
                        *value = g.voxel(x, y as u32, z).unwrap();
                    }
                }
            }
        }
        if let Some(id) = self.layer_id {
            inverse.edits = terrain
                .config
                .generation_layers
                .iter()
                .find(|l| l.id == id)
                .and_then(|l| l.brush_edits.get(self.journal_len..))
                .unwrap_or_default()
                .to_vec();
        }
        inverse
    }

    pub(super) fn apply(self, terrain: &mut TerrainState) {
        if !self.result(terrain).is_some_and(|r| self.matches(r)) {
            return;
        }
        if let Some(id) = self.layer_id {
            let Some(layer) = terrain
                .config
                .generation_layers
                .iter_mut()
                .find(|l| l.id == id)
            else {
                return;
            };
            layer.brush_edits.truncate(self.journal_len);
            layer.brush_edits.extend(self.edits);
        }
        let result = if self.layer_id.is_some() {
            terrain.layered_authored.as_mut()
        } else {
            terrain.authored.as_mut()
        }
        .unwrap();
        match &mut result.data {
            TerrainData::Heightfield(f) => {
                for (i, height) in self.heights {
                    f.heights[i as usize] = height;
                }
            }
            TerrainData::Voxel(g) => {
                for ((x, z), column) in self.columns {
                    for (y, value) in column.into_iter().enumerate() {
                        g.set_voxel(x, y as u32, z, value);
                    }
                }
            }
        }
        terrain.mark_render_chunks_dirty(self.chunks.into_iter().collect());
    }
}

impl Editor {
    pub(super) fn local_brush_snapshot(
        &self,
        world: &World,
        layer_id: Option<u64>,
    ) -> Option<HistorySnapshot> {
        if matches!(self.sculpt_tool, SculptTool::None | SculptTool::Paint) {
            return None;
        }
        if layer_id.is_some() && world.terrain.brush_recompose_required() {
            return None;
        }
        let result = if layer_id.is_some() {
            world.terrain.layered_authored.as_ref()
        } else {
            world.terrain.authored.as_ref()
        }?;
        let (origin, size, spacing, heightfield) = match &result.data {
            TerrainData::Heightfield(f) => (f.origin, f.size, 1.0, true),
            TerrainData::Voxel(_) if layer_id.is_none() => return None,
            TerrainData::Voxel(g) => (g.origin, g.size, g.spacing_studs, false),
        };
        if size.contains(&0) {
            return None;
        }
        let journal_len = if let Some(id) = layer_id {
            world
                .terrain
                .config
                .generation_layers
                .iter()
                .find(|l| l.id == id && l.visible && !l.locked && l.applied.is_some())?
                .brush_edits
                .len()
        } else {
            0
        };
        Some(
            self.blank_history_with_terrain(TerrainHistorySnapshot::BrushRegion(
                BrushRegionSnapshot {
                    layer_id,
                    journal_len,
                    edits: Vec::new(),
                    origin,
                    size,
                    spacing,
                    heightfield,
                    heights: HashMap::new(),
                    columns: HashMap::new(),
                    chunks: HashSet::new(),
                },
            )),
        )
    }
}
