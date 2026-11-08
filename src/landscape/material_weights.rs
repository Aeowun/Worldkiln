// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::paint_mask::CircularPaintBrush;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
pub const PAGE_DIM: usize = 128;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PageId {
    pub chart: u64,
    pub x: i32,
    pub y: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirtyRect {
    pub x0: u16,
    pub y0: u16,
    pub x1: u16,
    pub y1: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MaterialWeightPage {
    pub id: PageId,
    pub origin: [f32; 2],
    pub texel_pitch: f32,
    pub rgba: Vec<u8>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, Vec<u8>>,
}
impl MaterialWeightPage {
    pub fn new(id: PageId, origin: [f32; 2], pitch: f32) -> Option<Self> {
        if !pitch.is_finite() || pitch <= 0.0 || origin.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut rgba = vec![0; PAGE_DIM * PAGE_DIM * 4];
        for px in rgba.chunks_exact_mut(4) {
            px[0] = 255
        }
        Some(Self {
            id,
            origin,
            texel_pitch: pitch,
            rgba,
            layers: BTreeMap::new(),
        })
    }
    pub fn valid(&self) -> bool {
        self.layers.values().all(|w| w.len() == PAGE_DIM * PAGE_DIM)
            && (0..PAGE_DIM * PAGE_DIM)
                .all(|i| self.layers.values().map(|w| w[i] as u32).sum::<u32>() <= 255)
            && self.rgba.len() == PAGE_DIM * PAGE_DIM * 4
            && self.texel_pitch.is_finite()
            && self.texel_pitch > 0.0
            && self.origin.iter().all(|v| v.is_finite())
            && self
                .rgba
                .chunks_exact(4)
                .all(|p| p.iter().map(|v| *v as u32).sum::<u32>() == 255)
    }
    fn paint(
        &mut self,
        brush: &CircularPaintBrush,
        channel: usize,
        strength: f32,
    ) -> Option<(DirtyRect, Vec<(usize, [u8; 4])>)> {
        if self.rgba.len() != PAGE_DIM * PAGE_DIM * 4
            || !self.texel_pitch.is_finite()
            || self.texel_pitch <= 0.0
            || !self.origin.iter().all(|v| v.is_finite())
            || brush.validated().is_none()
            || channel >= 4
            || !strength.is_finite()
            || strength <= 0.0
            || strength > 1.0
        {
            return None;
        }
        let radius = brush.radius;
        let cx = brush.center[0];
        let cy = brush.center[2];
        let lo_x = (((cx - radius - self.origin[0]) / self.texel_pitch) - 0.5).ceil() as i32;
        let hi_x = (((cx + radius - self.origin[0]) / self.texel_pitch) - 0.5).floor() as i32;
        let lo_y = (((cy - radius - self.origin[1]) / self.texel_pitch) - 0.5).ceil() as i32;
        let hi_y = (((cy + radius - self.origin[1]) / self.texel_pitch) - 0.5).floor() as i32;
        let (xmin, xmax) = (lo_x.max(0), hi_x.min(PAGE_DIM as i32 - 1));
        let (ymin, ymax) = (lo_y.max(0), hi_y.min(PAGE_DIM as i32 - 1));
        if xmin > xmax || ymin > ymax {
            return None;
        }
        let mut dirty = DirtyRect {
            x0: u16::MAX,
            y0: u16::MAX,
            x1: 0,
            y1: 0,
        };
        let mut changed = Vec::new();
        for y in ymin..=ymax {
            for x in xmin..=xmax {
                let wx = self.origin[0] + (x as f32 + 0.5) * self.texel_pitch;
                let wy = self.origin[1] + (y as f32 + 0.5) * self.texel_pitch;
                let coverage = brush.weight_at([wx, 0.0, wy]) * strength;
                if coverage <= 0.0 {
                    continue;
                }
                let texel = y as usize * PAGE_DIM + x as usize;
                let mut dynamic_changed = false;
                for weights in self.layers.values_mut() {
                    let old = weights[texel];
                    weights[texel] = (old as f32 * (1.0 - coverage)).round() as u8;
                    dynamic_changed |= old != weights[texel];
                }
                let offset = texel * 4;
                let original: [u8; 4] = self.rgba[offset..offset + 4].try_into().unwrap();
                let target = original[channel] as f32 / 255.0;
                let next = target + (1.0 - target) * coverage;
                if (next - target) < 1.0 / 510.0 && !dynamic_changed {
                    continue;
                }
                let mut weights = original.map(|v| v as f32 / 255.0);
                let rest = (1.0 - next).max(0.0);
                let other_sum = weights
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != channel)
                    .map(|(_, v)| v)
                    .sum::<f32>();
                for i in 0..4 {
                    if i != channel {
                        weights[i] = if other_sum > 0.0 {
                            weights[i] / other_sum * rest
                        } else {
                            if i == 0 { rest } else { 0.0 }
                        };
                    }
                }
                weights[channel] = next;
                let mut encoded = weights.map(|v| (v * 255.0).floor() as u8);
                let mut rem = 255 - encoded.iter().map(|v| *v as u32).sum::<u32>();
                for i in [channel, 0, 1, 2, 3] {
                    if rem == 0 {
                        break;
                    }
                    if encoded[i] < 255 {
                        encoded[i] += 1;
                        rem -= 1
                    }
                }
                if original == encoded && !dynamic_changed {
                    continue;
                }
                self.rgba[offset..offset + 4].copy_from_slice(&encoded);
                changed.push((offset, original));
                dirty.x0 = dirty.x0.min(x as u16);
                dirty.x1 = dirty.x1.max(x as u16);
                dirty.y0 = dirty.y0.min(y as u16);
                dirty.y1 = dirty.y1.max(y as u16);
            }
        }
        (!changed.is_empty()).then_some((dirty, changed))
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MaterialWeightAtlas {
    #[serde(with = "page_list_serde")]
    pub pages: BTreeMap<PageId, MaterialWeightPage>,
}
mod page_list_serde {
    use super::*;
    pub fn serialize<S>(map: &BTreeMap<PageId, MaterialWeightPage>, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::Serialize;
        map.values().collect::<Vec<_>>().serialize(s)
    }
    pub fn deserialize<'de, D>(d: D) -> Result<BTreeMap<PageId, MaterialWeightPage>, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let entries = Vec::<MaterialWeightPage>::deserialize(d)?;
        let mut map = BTreeMap::new();
        for page in entries {
            if !page.valid() || map.insert(page.id, page.clone()).is_some() {
                return Err(serde::de::Error::custom("invalid or duplicate paint page"));
            }
        }
        Ok(map)
    }
}
#[derive(Clone, Debug)]
pub struct ChangedPage {
    pub id: PageId,
    pub dirty: DirtyRect,
    before: Vec<(usize, [u8; 4])>,
}
#[derive(Clone, Debug, Default)]
pub struct PaintTransaction {
    pub pages: Vec<ChangedPage>,
}
impl MaterialWeightAtlas {
    pub fn stroke_layer(
        &mut self,
        chart: u64,
        brush: &CircularPaintBrush,
        id: &str,
        strength: f32,
    ) -> Vec<DirtyUpload> {
        if brush.validated().is_none() || !strength.is_finite() || !(0.0..=1.0).contains(&strength)
        {
            return Vec::new();
        }
        let pages: Vec<_> = self
            .pages
            .keys()
            .filter(|id| id.chart == chart)
            .copied()
            .collect();
        self.stroke_layer_on_pages(&pages, brush, id, strength)
    }
    pub fn stroke_layer_on_pages(
        &mut self,
        pages: &[PageId],
        brush: &CircularPaintBrush,
        id: &str,
        strength: f32,
    ) -> Vec<DirtyUpload> {
        if brush.validated().is_none() || !strength.is_finite() || !(0.0..=1.0).contains(&strength)
        {
            return Vec::new();
        }
        let mut dirty = Vec::new();
        for page_id in pages {
            let Some(page) = self.pages.get_mut(page_id) else {
                continue;
            };
            let min_x = ((brush.center[0] - brush.radius - page.origin[0]) / page.texel_pitch)
                .floor()
                .max(0.0) as usize;
            let max_x = ((brush.center[0] + brush.radius - page.origin[0]) / page.texel_pitch)
                .ceil()
                .min(PAGE_DIM as f32) as usize;
            let min_y = ((brush.center[2] - brush.radius - page.origin[1]) / page.texel_pitch)
                .floor()
                .max(0.0) as usize;
            let max_y = ((brush.center[2] + brush.radius - page.origin[1]) / page.texel_pitch)
                .ceil()
                .min(PAGE_DIM as f32) as usize;
            if min_x >= max_x || min_y >= max_y {
                continue;
            }
            page.layers
                .entry(id.into())
                .or_insert_with(|| vec![0; PAGE_DIM * PAGE_DIM]);
            let mut rect = DirtyRect {
                x0: u16::MAX,
                y0: u16::MAX,
                x1: 0,
                y1: 0,
            };
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let i = y * PAGE_DIM + x;
                    let coverage = brush.weight_at([
                        page.origin[0] + (x as f32 + 0.5) * page.texel_pitch,
                        0.0,
                        page.origin[1] + (y as f32 + 0.5) * page.texel_pitch,
                    ]) * strength;
                    let old = page.layers[id][i];
                    let next = (old as f32 + (255.0 - old as f32) * coverage)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                    if next == old {
                        continue;
                    }
                    let factor = (255 - next as u32) as f32 / (255 - old as u32).max(1) as f32;
                    for (key, w) in &mut page.layers {
                        w[i] = if key == id {
                            next
                        } else {
                            (w[i] as f32 * factor).floor() as u8
                        };
                    }
                    rect.x0 = rect.x0.min(x as u16);
                    rect.x1 = rect.x1.max(x as u16);
                    rect.y0 = rect.y0.min(y as u16);
                    rect.y1 = rect.y1.max(y as u16);
                }
            }
            if rect.x0 != u16::MAX {
                let width = rect.x1 - rect.x0 + 1;
                let mut rgba = Vec::new();
                for y in rect.y0..=rect.y1 {
                    let i = (y as usize * PAGE_DIM + rect.x0 as usize) * 4;
                    rgba.extend_from_slice(&page.rgba[i..i + width as usize * 4]);
                }
                dirty.push(DirtyUpload {
                    id: page.id,
                    rect,
                    row_length: width as u32,
                    rgba,
                });
            }
        }
        dirty
    }
    pub fn stroke(
        &mut self,
        chart: u64,
        brush: &CircularPaintBrush,
        channel: usize,
        strength: f32,
    ) -> PaintTransaction {
        let mut tx = PaintTransaction::default();
        for (id, page) in &mut self.pages {
            if id.chart != chart {
                continue;
            }
            if let Some((dirty, before)) = page.paint(brush, channel, strength) {
                tx.pages.push(ChangedPage {
                    id: *id,
                    dirty,
                    before,
                });
            }
        }
        tx
    }
    pub fn undo(&mut self, transaction: PaintTransaction) -> Vec<(PageId, DirtyRect)> {
        let mut dirty = Vec::new();
        for change in transaction.pages {
            if let Some(page) = self.pages.get_mut(&change.id) {
                for (offset, original) in change.before {
                    page.rgba[offset..offset + 4].copy_from_slice(&original);
                }
                dirty.push((change.id, change.dirty));
            }
        }
        dirty
    }
    pub fn dirty_pages(tx: &PaintTransaction) -> BTreeSet<PageId> {
        tx.pages.iter().map(|change| change.id).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn brush() -> CircularPaintBrush {
        CircularPaintBrush {
            center: [32.0, 0.0, 16.0],
            radius: 5.0,
            edge_softness: 0.25,
        }
    }
    #[test]
    fn paint_is_round_and_avoids_chunk_boundary_artifacts() {
        let mut atlas = MaterialWeightAtlas::default();
        for x in 0..2 {
            let id = PageId { chart: 1, x, y: 0 };
            atlas.pages.insert(
                id,
                MaterialWeightPage::new(id, [x as f32 * 32.0, 0.0], 0.25).unwrap(),
            );
        }
        let tx = atlas.stroke(1, &brush(), 3, 1.0);
        assert_eq!(tx.pages.len(), 2);
        for page in atlas.pages.values() {
            assert!(page.valid());
            for y in 0..PAGE_DIM {
                for x in 0..PAGE_DIM {
                    let wx = page.origin[0] + (x as f32 + 0.5) * 0.25;
                    let wy = page.origin[1] + (y as f32 + 0.5) * 0.25;
                    let sand = page.rgba[(y * PAGE_DIM + x) * 4 + 3];
                    if (wx - 32.0).hypot(wy - 16.0) >= 5.0 {
                        assert_eq!(sand, 0)
                    }
                }
            }
        }
    }
    #[test]
    fn cave_chart_isolation_and_lossless_undo() {
        let mut atlas = MaterialWeightAtlas::default();
        for chart in [1, 2] {
            let id = PageId { chart, x: 0, y: 0 };
            atlas
                .pages
                .insert(id, MaterialWeightPage::new(id, [0.0, 0.0], 0.25).unwrap());
        }
        let before = atlas.pages.clone();
        let tx = atlas.stroke(
            1,
            &CircularPaintBrush {
                center: [16.0, 0.0, 16.0],
                radius: 5.0,
                edge_softness: 0.5,
            },
            2,
            1.0,
        );
        assert_eq!(tx.pages.len(), 1);
        assert_eq!(
            atlas.pages[&PageId {
                chart: 2,
                x: 0,
                y: 0
            }]
                .rgba,
            before[&PageId {
                chart: 2,
                x: 0,
                y: 0
            }]
                .rgba
        );
        atlas.undo(tx);
        assert_eq!(atlas.pages, before);
    }
    #[test]
    fn persisted_pages_roundtrip_json() {
        let id = PageId {
            chart: 55,
            x: -1,
            y: 2,
        };
        let mut atlas = MaterialWeightAtlas::default();
        atlas.pages.insert(
            id,
            MaterialWeightPage::new(id, [-32.0, 64.0], 0.25).unwrap(),
        );
        let json = serde_json::to_vec(&atlas).unwrap();
        let restored: MaterialWeightAtlas = serde_json::from_slice(&json).unwrap();
        assert!(restored.pages[&id].valid());
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirtyUpload {
    pub id: PageId,
    pub rect: DirtyRect,
    pub row_length: u32,
    pub rgba: Vec<u8>,
}
impl MaterialWeightAtlas {
    pub fn extract_dirty_uploads(&self, tx: &PaintTransaction) -> Vec<DirtyUpload> {
        tx.pages
            .iter()
            .filter_map(|change| {
                let page = self.pages.get(&change.id)?;
                let width = (change.dirty.x1 - change.dirty.x0 + 1) as usize;
                let mut rgba = Vec::with_capacity(
                    width * 4 * (change.dirty.y1 - change.dirty.y0 + 1) as usize,
                );
                for y in change.dirty.y0..=change.dirty.y1 {
                    let offset = (y as usize * PAGE_DIM + change.dirty.x0 as usize) * 4;
                    rgba.extend_from_slice(&page.rgba[offset..offset + width * 4]);
                }
                Some(DirtyUpload {
                    id: change.id,
                    rect: change.dirty,
                    row_length: width as u32,
                    rgba,
                })
            })
            .collect()
    }
}
#[derive(Clone, Debug, Default)]
pub struct StrokeDiagnostics {
    pub elapsed_us: u64,
    pub affected_pages: usize,
    pub affected_texels: usize,
    pub uploaded_bytes: usize,
}
impl MaterialWeightAtlas {
    pub fn timed_stroke(
        &mut self,
        chart: u64,
        brush: &CircularPaintBrush,
        channel: usize,
        strength: f32,
    ) -> (PaintTransaction, StrokeDiagnostics) {
        let start = std::time::Instant::now();
        let transaction = self.stroke(chart, brush, channel, strength);
        let texels = transaction.pages.iter().map(|p| p.before.len()).sum();
        let regions = self.extract_dirty_uploads(&transaction);
        let diagnostic = StrokeDiagnostics {
            elapsed_us: start.elapsed().as_micros() as u64,
            affected_pages: transaction.pages.len(),
            affected_texels: texels,
            uploaded_bytes: regions.iter().map(|r| r.rgba.len()).sum(),
        };
        (transaction, diagnostic)
    }
}
#[cfg(test)]
mod dirty_region_tests {
    use super::*;
    #[test]
    fn small_brush_upload_is_far_smaller_than_full_page() {
        let id = PageId {
            chart: 3,
            x: 0,
            y: 0,
        };
        let mut atlas = MaterialWeightAtlas::default();
        atlas
            .pages
            .insert(id, MaterialWeightPage::new(id, [0.0, 0.0], 0.25).unwrap());
        let brush = CircularPaintBrush {
            center: [16.0, 0.0, 16.0],
            radius: 1.0,
            edge_softness: 0.25,
        };
        let (tx, diag) = atlas.timed_stroke(3, &brush, 1, 1.0);
        let uploads = atlas.extract_dirty_uploads(&tx);
        assert_eq!(uploads.len(), 1);
        assert!(diag.affected_texels > 0);
        assert!(diag.uploaded_bytes < 4096);
        assert_eq!(
            uploads[0].rgba.len(),
            uploads[0].row_length as usize
                * 4
                * (uploads[0].rect.y1 - uploads[0].rect.y0 + 1) as usize
        );
        assert!(diag.uploaded_bytes < PAGE_DIM * PAGE_DIM * 4 / 16);
    }
    #[test]
    fn no_brush_work_on_empty_or_unrelated_chart() {
        let mut atlas = MaterialWeightAtlas::default();
        let (tx, stats) = atlas.timed_stroke(
            999,
            &CircularPaintBrush {
                center: [0.0; 3],
                radius: 5.0,
                edge_softness: 0.5,
            },
            3,
            1.0,
        );
        assert!(tx.pages.is_empty());
        assert_eq!(stats.uploaded_bytes, 0);
    }
}

#[cfg(test)]
mod visual_acceptance_tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn performance_test_cost_scales_with_brush_area_not_scene_size() {
        let id = PageId {
            chart: 7,
            x: 0,
            y: 0,
        };
        let mut atlas = MaterialWeightAtlas::default();
        atlas
            .pages
            .insert(id, MaterialWeightPage::new(id, [0.0, 0.0], 0.25).unwrap());
        let brush = CircularPaintBrush {
            center: [12.0, 0.0, 12.0],
            radius: 2.0,
            edge_softness: 0.25,
        };
        let (tx, perf) = atlas.timed_stroke(7, &brush, 1, 0.5);
        assert_eq!(perf.affected_pages, 1);
        assert!(perf.affected_texels < 300);
        assert!(perf.uploaded_bytes < 4096);
        atlas.undo(tx);
        assert!(
            atlas.pages[&id]
                .rgba
                .chunks_exact(4)
                .all(|p| p == [255, 0, 0, 0])
        );
    }
}

impl MaterialWeightAtlas {
    pub fn stroke_indexed(
        &mut self,
        chart: u64,
        chart_origin: [f32; 2],
        texel_pitch: f32,
        brush: &CircularPaintBrush,
        channel: usize,
        strength: f32,
    ) -> PaintTransaction {
        let mut tx = PaintTransaction::default();
        if brush.validated().is_none()
            || !texel_pitch.is_finite()
            || texel_pitch <= 0.0
            || chart_origin.iter().any(|c| !c.is_finite())
        {
            return tx;
        }
        let tile_span = texel_pitch * PAGE_DIM as f32;
        if !tile_span.is_finite() || tile_span <= 0.0 {
            return tx;
        }
        let bounds = [
            ((brush.center[0] - brush.radius - chart_origin[0]) / tile_span).floor(),
            ((brush.center[2] - brush.radius - chart_origin[1]) / tile_span).floor(),
            ((brush.center[0] + brush.radius - chart_origin[0]) / tile_span).floor(),
            ((brush.center[2] + brush.radius - chart_origin[1]) / tile_span).floor(),
        ];
        if bounds
            .iter()
            .any(|v| !v.is_finite() || v.abs() > i32::MAX as f32)
        {
            return tx;
        }
        let x0 = bounds[0] as i32;
        let y0 = bounds[1] as i32;
        let x1 = bounds[2] as i32;
        let y1 = bounds[3] as i32;
        if (x1 as i64 - x0 as i64 + 1) * (y1 as i64 - y0 as i64 + 1) > 4096 {
            return tx;
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let id = PageId { chart, x, y };
                let Some(page) = self.pages.get_mut(&id) else {
                    continue;
                };
                let expected = [
                    chart_origin[0] + x as f32 * tile_span,
                    chart_origin[1] + y as f32 * tile_span,
                ];
                if (page.origin[0] - expected[0]).abs() > texel_pitch * 0.001
                    || (page.origin[1] - expected[1]).abs() > texel_pitch * 0.001
                    || (page.texel_pitch - texel_pitch).abs() > 1e-6
                {
                    continue;
                }
                if let Some((dirty, before)) = page.paint(brush, channel, strength) {
                    tx.pages.push(ChangedPage { id, dirty, before });
                }
            }
        }
        tx
    }
    pub fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::{self, Write};
        for (id, page) in &self.pages {
            if *id != page.id || !page.valid() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid paint atlas page",
                ));
            }
        }
        let bytes =
            serde_json::to_vec(self).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "paint atlas has no parent")
        })?;
        std::fs::create_dir_all(parent)?;
        let staging = path.with_extension("painting.tmp");
        {
            let mut file = std::fs::File::create(&staging)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        if let Err(error) = std::fs::rename(&staging, path) {
            let _ = std::fs::remove_file(&staging);
            return Err(error);
        }
        Ok(())
    }
    pub fn load_from(path: &std::path::Path) -> std::io::Result<Self> {
        use std::io;
        let bytes = std::fs::read(path)?;
        let atlas: Self = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(atlas)
    }
}
#[cfg(test)]
mod indexed_spatial_tests {
    use super::*;
    #[test]
    fn indexed_brush_only_looks_up_touched_chart_pages() {
        let mut atlas = MaterialWeightAtlas::default();
        for x in -3..=3 {
            for y in -3..=3 {
                let id = PageId { chart: 8, x, y };
                atlas.pages.insert(
                    id,
                    MaterialWeightPage::new(id, [x as f32 * 32.0, y as f32 * 32.0], 0.25).unwrap(),
                );
            }
        }
        let mut baseline = atlas.pages.clone();
        let brush = CircularPaintBrush {
            center: [0.0, 0.0, 0.0],
            radius: 3.0,
            edge_softness: 0.3,
        };
        let transaction = atlas.stroke_indexed(8, [0.0, 0.0], 0.25, &brush, 2, 1.0);
        assert_eq!(transaction.pages.len(), 4);
        let touched = MaterialWeightAtlas::dirty_pages(&transaction);
        assert!(touched.iter().all(|id| id.x.abs() <= 1 && id.y.abs() <= 1));
        for id in atlas.pages.keys() {
            if !touched.contains(id) {
                assert_eq!(atlas.pages[id], baseline.remove(id).unwrap());
            }
        }
    }
    #[test]
    fn persistent_atlas_file_roundtrips() {
        let path =
            std::env::temp_dir().join(format!("worldkiln_paint_atlas_{}.json", std::process::id()));
        let mut atlas = MaterialWeightAtlas::default();
        let id = PageId {
            chart: 6,
            x: 0,
            y: 0,
        };
        atlas
            .pages
            .insert(id, MaterialWeightPage::new(id, [0.0, 0.0], 0.25).unwrap());
        atlas.save_to(&path).unwrap();
        let loaded = MaterialWeightAtlas::load_from(&path).unwrap();
        assert_eq!(loaded.pages, atlas.pages);
        std::fs::remove_file(path).unwrap();
    }
}
