// Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:42 -05:00
// Generic independent generator recipes. New layers have no stored names.
use super::{
    DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS, HeightfieldData, SculptTool, SmoothVoxel,
    SmoothVoxelGrid, TerrainConfig, TerrainData, TerrainGenerationRequest, TerrainGeneratorType,
    TerrainMaterials, TerrainPaintRequest, TerrainResult, TerrainSculptRequest, TerrainState,
    VoxelLayerSettings, voxelize_heightfield_layers_seeded,
};
use fastnoise_lite::{FastNoiseLite, FractalType, NoiseType};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Landform {
    #[default]
    Plains,
    Hills,
    Mountains,
    Islands,
    Archipelago,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeneratorSettings {
    pub kind: Landform,
    pub request: TerrainGenerationRequest,
    pub seed: i32,
    pub base_height: f32,
    pub relief: f32,
    pub frequency: f32,
    pub water_level: f32,
    pub radius: f32,
    pub beach_width: f32,
    pub coast_noise: f32,
    pub count: u32,
}
impl Default for GeneratorSettings {
    fn default() -> Self {
        Self {
            kind: Landform::Plains,
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [128, 48, 128],
            },
            seed: 1337,
            base_height: 16.0,
            relief: 20.0,
            frequency: 0.015,
            water_level: 12.0,
            radius: 50.0,
            beach_width: 28.0,
            coast_noise: 0.15,
            count: 3,
        }
    }
}
impl GeneratorSettings {
    pub fn validate(&self) -> Result<(), String> {
        self.request.validate().map_err(|e| e.to_string())?;
        if ![
            self.base_height,
            self.relief,
            self.frequency,
            self.water_level,
            self.radius,
            self.beach_width,
            self.coast_noise,
        ]
        .iter()
        .all(|v| v.is_finite())
            || self.relief < 0.0
            || self.frequency <= 0.0
            || self.radius <= 0.0
            || self.beach_width < 2.0
            || !(0.0..=0.85).contains(&self.coast_noise)
            || !(1..=24).contains(&self.count)
        {
            return Err("Generator parameters must be finite with positive scales, 1-24 features and Coast Variation <= 0.85".into());
        }
        Ok(())
    }
}
fn default_visible() -> bool {
    true
}
// Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
// Brush dabs belong to a specific layer. They are saved in separate chunk
// files rather than bloating the terrain.json generation recipe metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerBrushEdit {
    Sculpt(TerrainSculptRequest),
    Paint {
        center: [f32; 3],
        radius: f32,
        material: String,
    },
}
impl LayerBrushEdit {
    pub fn radius(&self) -> f32 {
        match self {
            Self::Sculpt(r) => r.radius,
            Self::Paint { radius, .. } => *radius,
        }
    }
    pub fn center(&self) -> [f32; 3] {
        match self {
            Self::Sculpt(r) => r.center,
            Self::Paint { center, .. } => *center,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let center = self.center();
        let radius = match self {
            Self::Sculpt(r) => r.radius,
            Self::Paint { radius, .. } => *radius,
        };
        if !center.iter().all(|v| v.is_finite())
            || !radius.is_finite()
            || !(0.0..=256.0).contains(&radius)
        {
            return Err("Invalid stored layer brush center/radius".into());
        }
        match self {
            Self::Sculpt(r)
                if !r.strength.is_finite()
                    || r.strength <= 0.0
                    || (r.tool.is_flatten() && r.flatten_target.is_none_or(|v| !v.is_finite()))
                    || matches!(r.tool, SculptTool::None | SculptTool::Paint) =>
            {
                Err("Invalid stored layer sculpt operation".into())
            }
            Self::Paint { material, .. } if material.is_empty() || material.len() > 256 => {
                Err("Invalid stored layer paint material".into())
            }
            _ => Ok(()),
        }
    }
    // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:20 -05:00
    fn apply(
        &self,
        result: &mut TerrainResult,
        materials: &TerrainMaterials,
        settings: VoxelLayerSettings,
        seed: i32,
        water_level: f32,
    ) -> bool {
        match self {
            Self::Sculpt(request) => {
                let changed = result.apply_sculpt(request);
                if changed {
                    if let TerrainData::Voxel(grid) = &mut result.data {
                        grid.refresh_local_slope_materials(
                            request.center,
                            request.radius,
                            materials,
                            settings,
                            seed,
                            water_level,
                        );
                    }
                }
                changed
            }
            Self::Paint {
                center,
                radius,
                material,
            } => result.apply_paint(&TerrainPaintRequest {
                center: *center,
                radius: *radius,
                material: material.clone(),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GenerationLayer {
    pub id: u64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub settings: GeneratorSettings,
    // A recipe is rendered only after Generate is explicitly pressed.
    // Changing draft controls never silently changes already generated Terrain.
    #[serde(default)]
    pub applied: Option<GeneratorSettings>,
    // Stored as layer-specific edit chunks, not in terrain.json.
    #[serde(skip)]
    pub brush_edits: Vec<LayerBrushEdit>,
}
impl GenerationLayer {
    pub fn new(id: u64, request: TerrainGenerationRequest) -> Self {
        Self {
            id,
            name: None,
            visible: true,
            locked: false,
            settings: GeneratorSettings {
                request,
                ..GeneratorSettings::default()
            },
            applied: None,
            brush_edits: Vec::new(),
        }
    }
}
fn smooth(t: f32) -> f32 {
    let v = t.clamp(0.0, 1.0);
    v * v * (3.0 - 2.0 * v)
}

fn height(settings: &GeneratorSettings, x: f32, z: f32, n: f32, d: f32) -> f32 {
    let request = settings.request;
    let cx = request.position[0] as f32 + request.size[0] as f32 * 0.5;
    let cz = request.position[2] as f32 + request.size[2] as f32 * 0.5;
    let floor = request.position[1] as f32;
    let dist = |px: f32, pz: f32, r: f32| {
        (((x - px) / r).powi(2) + ((z - pz) / (r * 0.78)).powi(2)).sqrt()
    };
    match settings.kind {
        Landform::Plains => settings.base_height + settings.relief * (n * 0.78 + d * 0.22),
        Landform::Hills => {
            settings.base_height
                + settings.relief
                    * (n * 0.65 + d * 0.22 + (x * 0.021).sin() * (z * 0.026).cos() * 0.13)
        }
        Landform::Mountains => {
            let mut peaks = 0.0_f32;
            for i in 0..settings.count {
                let angle = i as f32 * 2.3999632 + settings.seed as f32 * 0.019;
                let offset = ((i + 1) as f32 / (settings.count + 1) as f32).sqrt();
                let r = dist(
                    cx + angle.cos() * settings.radius * 0.55 * offset,
                    cz + angle.sin() * settings.radius * 0.55 * offset,
                    settings.radius * 0.60,
                );
                peaks = peaks.max((1.0 - smooth(r / 1.8)).powf(1.35));
            }
            let edge = dist(cx, cz, request.size[0] as f32 * 0.53);
            let envelope = 1.0 - smooth((edge - 0.63) / 0.37);
            floor
                + (settings.base_height - floor) * 0.30 * envelope
                + settings.relief * peaks * envelope * (0.9 + 0.15 * n)
        }
        Landform::Islands | Landform::Archipelago => {
            let count = if settings.kind == Landform::Islands {
                1
            } else {
                settings.count
            };
            let mut coast = -1.0e6_f32;
            for i in 0..count {
                let angle = i as f32 * 2.3999632 + settings.seed as f32 * 0.015;
                let shift = if count == 1 { 0.0 } else { 0.32 };
                let r = settings.radius
                    * (if count == 1 {
                        1.0
                    } else {
                        0.55 + 0.13 * (i % 3) as f32
                    });
                let distance = dist(
                    cx + angle.cos() * settings.radius * shift,
                    cz + angle.sin() * settings.radius * shift,
                    r,
                );
                coast =
                    coast.max((1.0 - distance + settings.coast_noise * (n * 0.78 + d * 0.22)) * r);
            }
            if coast <= 0.0 {
                return floor;
            }
            let sand = smooth(coast / settings.beach_width);
            let inland = smooth((coast - settings.beach_width) / (settings.radius * 0.50));
            settings.water_level
                + 0.05
                + sand * settings.beach_width * 0.11
                + settings.relief * inland * (0.68 + 0.26 * n + 0.06 * d).max(0.10)
        }
    }
}
pub fn generate_layer(
    settings: &GeneratorSettings,
    cfg: &TerrainConfig,
) -> Result<TerrainResult, String> {
    settings.validate()?;
    let request = settings.request;
    let mut noise = FastNoiseLite::with_seed(settings.seed);
    noise.set_noise_type(Some(NoiseType::OpenSimplex2S));
    noise.set_frequency(Some(settings.frequency));
    noise.set_fractal_type(Some(FractalType::FBm));
    noise.set_fractal_octaves(Some(4));
    let mut detail = FastNoiseLite::with_seed(settings.seed ^ 0x2a78_43cb);
    detail.set_noise_type(Some(NoiseType::OpenSimplex2));
    detail.set_frequency(Some(settings.frequency * 3.8));
    let mut heights = Vec::with_capacity(request.size[0] as usize * request.size[2] as usize);
    let ymin = request.position[1] as f32;
    let ymax = ymin + request.size[1] as f32;
    for z in 0..request.size[2] {
        for x in 0..request.size[0] {
            let wx = request.position[0] as f32 + x as f32 + 0.5;
            let wz = request.position[2] as f32 + z as f32 + 0.5;
            heights.push(
                height(
                    settings,
                    wx,
                    wz,
                    noise.get_noise_2d(wx, wz),
                    detail.get_noise_2d(wx, wz),
                )
                .clamp(ymin, ymax),
            );
        }
    }
    let hf = HeightfieldData {
        origin: request.position,
        size: request.size.map(|v| v as u32),
        heights,
    };
    let mut grid =
        voxelize_heightfield_layers_seeded(&hf, &cfg.materials, cfg.voxel_layers, settings.seed)
            .map_err(|e| e.to_string())?;
    super::apply_waterline_beach_material(
        &mut grid,
        settings.water_level,
        "sand",
        &cfg.materials.surface,
        &cfg.materials.subsurface,
        &cfg.materials.rock,
    );
    Ok(TerrainResult {
        request,
        generator: TerrainGeneratorType::Programmatic,
        generator_implementation_version: 1,
        data: TerrainData::Voxel(grid),
    })
}

// Merge into a NEW temporary volume. The authored source is never overwritten.
fn composite(
    base: Option<&TerrainResult>,
    layers: &[GenerationLayer],
    cfg: &TerrainConfig,
) -> Result<Option<TerrainResult>, String> {
    let mut sources = Vec::new();
    if let Some(original) = base {
        let payload = match &original.data {
            TerrainData::Voxel(_) => original.clone(),
            TerrainData::Heightfield(hf) => TerrainResult {
                data: TerrainData::Voxel(
                    voxelize_heightfield_layers_seeded(
                        hf,
                        &cfg.materials,
                        cfg.voxel_layers,
                        cfg.plains.seed,
                    )
                    .map_err(|e| e.to_string())?,
                ),
                ..original.clone()
            },
        };
        sources.push(payload);
    }
    for layer in layers.iter().filter(|l| l.visible) {
        if let Some(settings) = &layer.applied {
            let mut source = generate_layer(settings, cfg)?;
            for edit in &layer.brush_edits {
                edit.validate()?;
                edit.apply(
                    &mut source,
                    &cfg.materials,
                    cfg.voxel_layers,
                    settings.seed,
                    settings.water_level,
                );
            }
            sources.push(source);
        }
    }
    if sources.is_empty() {
        return Ok(None);
    }
    if sources.len() == 1 {
        return Ok(Some(sources.remove(0)));
    }
    let TerrainData::Voxel(first) = &sources[0].data else {
        unreachable!()
    };
    let spacing = first.spacing_studs;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for input in &sources {
        let TerrainData::Voxel(grid) = &input.data else {
            unreachable!()
        };
        if (grid.spacing_studs - spacing).abs() > 0.00001 {
            return Err(
                "Terrain layers use incompatible voxel sample spacing; source data was preserved"
                    .into(),
            );
        }
        for axis in 0..3 {
            min[axis] = min[axis].min(grid.origin[axis] as f64);
            max[axis] =
                max[axis].max(grid.origin[axis] as f64 + grid.size[axis] as f64 * spacing as f64);
        }
    }
    let mut origin = [0_i32; 3];
    let mut size = [0_u32; 3];
    for axis in 0..3 {
        let low = (min[axis] / spacing as f64).floor() * spacing as f64;
        let high = (max[axis] / spacing as f64).ceil() * spacing as f64;
        if low < i32::MIN as f64 || high > i32::MAX as f64 {
            return Err("Composite Terrain is outside supported coordinates".into());
        }
        origin[axis] = low as i32;
        size[axis] = ((high - low) / spacing as f64).ceil() as u32;
    }
    let volume = size
        .iter()
        .try_fold(1_u64, |v, n| v.checked_mul(*n as u64))
        .ok_or("Composite sample count overflow")?;
    if volume > 40_000_000 {
        return Err(format!(
            "Composite would require {volume} voxel samples (limit 40,000,000). No source was changed."
        ));
    }
    let mut grid = SmoothVoxelGrid::empty_with_spacing(origin, size, spacing, vec![])
        .map_err(|e| e.to_string())?;
    let mut palette: Vec<String> = vec![];
    for result in sources {
        let TerrainData::Voxel(input) = result.data else {
            unreachable!()
        };
        let mut remap = vec![];
        for material in &input.materials {
            let idx = if let Some(i) = palette.iter().position(|m| m == material) {
                i
            } else {
                let i = palette.len();
                palette.push(material.clone());
                i
            };
            remap.push(idx as u16);
        }
        let offset = [0, 1, 2]
            .map(|axis| ((input.origin[axis] - origin[axis]) as f32 / spacing).round() as usize);
        let (width, depth) = (size[0] as usize, size[2] as usize);
        let (w, d) = (input.size[0] as usize, input.size[2] as usize);
        for y in 0..input.size[1] as usize {
            for z in 0..d {
                for x in 0..w {
                    let src = (y * d + z) * w + x;
                    let voxel = input.voxels[src];
                    if voxel.occupancy == 0 {
                        continue;
                    }
                    let target =
                        ((y + offset[1]) * depth + (z + offset[2])) * width + x + offset[0];
                    if voxel.occupancy >= grid.voxels[target].occupancy {
                        grid.voxels[target] = SmoothVoxel {
                            material: remap[voxel.material as usize],
                            occupancy: voxel.occupancy,
                        };
                    }
                }
            }
        }
    }
    grid.materials = palette;
    Ok(Some(TerrainResult {
        request: TerrainGenerationRequest {
            position: origin,
            size: size.map(|s| (s as f32 * spacing).ceil() as i32),
        },
        generator: TerrainGeneratorType::Programmatic,
        generator_implementation_version: 1,
        data: TerrainData::Voxel(grid),
    }))
}
impl TerrainState {
    // Worldkiln 0.8.5 | WO MAT-052 | 2026-10-10 16:54 -05:00
    // A brush on the sole visible generated layer already mutates its exact
    // effective preview and journals its edit. Re-generating the entire
    // multi-million-voxel layer after every mouse-up is redundant and caused
    // a long UI hitch. Composites with a base or multiple visible layers
    // still require recomposition to restore proper layer ownership.
    pub(crate) fn brush_recompose_required(&self) -> bool {
        self.authored.is_some()
            || self
                .config
                .generation_layers
                .iter()
                .filter(|layer| layer.visible && layer.applied.is_some())
                .take(2)
                .count()
                != 1
    }

    pub fn rebuild_generation_layers(&mut self) -> Result<(), String> {
        let next = if self
            .config
            .generation_layers
            .iter()
            .any(|l| l.applied.is_some())
        {
            composite(
                self.authored.as_ref(),
                &self.config.generation_layers,
                &self.config,
            )?
        } else {
            None
        };
        self.layered_authored = next;
        self.mark_full_render_dirty();
        Ok(())
    }
    pub fn add_generation_layer(
        &mut self,
        request: TerrainGenerationRequest,
    ) -> Result<u64, String> {
        request.validate().map_err(|e| e.to_string())?;
        let id = self
            .config
            .generation_layers
            .iter()
            .map(|l| l.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Terrain layer id overflow")?;
        self.config
            .generation_layers
            .push(GenerationLayer::new(id, request));
        Ok(id)
    }
    pub fn generate_selected_layer(&mut self, id: u64) -> Result<(), String> {
        let index = self
            .config
            .generation_layers
            .iter()
            .position(|l| l.id == id)
            .ok_or("Selected layer was removed")?;
        if self.config.generation_layers[index].locked {
            return Err("Layer is locked".into());
        }
        self.config.generation_layers[index].settings.validate()?;
        let old = self.config.generation_layers[index].applied.clone();
        let previous_water_level = self.config.plains.water_level;
        // Worldkiln 0.8.5 | WO MAT-055 | 2026-10-10 16:45 -05:00
        // Water is a shared World rendering plane. The selected generator's
        // shoreline/water level must drive that same plane when applied;
        // otherwise the beach moves while the rendered water stays behind.
        self.config.plains.water_level = self.config.generation_layers[index].settings.water_level;
        // Regeneration retains existing layer brushes; editing their original
        // generator recipe can thus be tested without erasing layer-local work.
        self.config.generation_layers[index].applied =
            Some(self.config.generation_layers[index].settings.clone());
        if let Err(error) = self.rebuild_generation_layers() {
            self.config.generation_layers[index].applied = old;
            self.config.plains.water_level = previous_water_level;
            return Err(error);
        }
        Ok(())
    }
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    /// Begin/continue a Layer brush. Rebuild is performed when the stroke
    /// ends; meanwhile the effective preview is updated locally and the base
    /// authored Terrain and other layers' recorded operations remain intact.
    pub fn apply_generation_layer_brush(
        &mut self,
        id: u64,
        brush: LayerBrushEdit,
    ) -> Result<bool, String> {
        brush.validate()?;
        let materials = self.config.materials.clone();
        let voxel_settings = self.config.voxel_layers;
        let layer = self
            .config
            .generation_layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("Select an existing Terrain layer first")?;
        if layer.locked || !layer.visible || layer.applied.is_none() {
            return Err("Select a generated, visible, unlocked layer to Sculpt/Paint".into());
        }
        // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
        // Do not record a stroke on a different layer's terrain by accident.
        let area = layer.applied.as_ref().expect("validated").request;
        let center = brush.center();
        let inside = (0..2).all(|axis| {
            let idx = if axis == 0 { 0 } else { 2 };
            let start = area.position[idx] as f64;
            let end = start + area.size[idx] as f64;
            let coord = center[idx] as f64;
            coord >= start && coord < end
        });
        if !inside {
            return Err("Brush hit is outside the selected layer region".into());
        }
        // Snapshot the currently rendered composition only for the local
        // immediate brush preview; authoritative playback happens on its
        // selected generation contribution at rebuild/reload.
        let Some(ref mut result) = self.layered_authored else {
            return Err("Generate the selected layer before editing".into());
        };
        let seed = layer.applied.as_ref().expect("validated").seed;
        let water_level = layer.applied.as_ref().expect("validated").water_level;
        let changed = brush.apply(result, &materials, voxel_settings, seed, water_level);
        if changed {
            layer.brush_edits.push(brush.clone());
            self.mark_authored_brush_dirty(brush.center(), brush.radius());
        }
        Ok(changed)
    }

    pub fn set_generation_layer_visible(&mut self, id: u64, value: bool) -> Result<(), String> {
        let index = self
            .config
            .generation_layers
            .iter()
            .position(|l| l.id == id)
            .ok_or("Selected layer was removed")?;
        let old = self.config.generation_layers[index].visible;
        self.config.generation_layers[index].visible = value;
        if let Err(error) = self.rebuild_generation_layers() {
            self.config.generation_layers[index].visible = old;
            return Err(error);
        }
        Ok(())
    }
    pub fn delete_generation_layer(&mut self, id: u64) -> Result<(), String> {
        let index = self
            .config
            .generation_layers
            .iter()
            .position(|l| l.id == id)
            .ok_or("Selected layer was removed")?;
        if self.config.generation_layers[index].locked {
            return Err("Layer is locked".into());
        }
        let removed = self.config.generation_layers.remove(index);
        if let Err(error) = self.rebuild_generation_layers() {
            self.config.generation_layers.insert(index, removed);
            return Err(error);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    #[test]
    fn selected_layer_paint_is_saved_independently_and_reopen_replays() {
        let mut state = TerrainState::default();
        let first_request = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 32, 24],
        };
        let second_request = TerrainGenerationRequest {
            position: [32, 0, 0],
            size: [24, 32, 24],
        };
        state.generate_authored(first_request).unwrap();
        let original = state.authored.clone();
        let first = state.add_generation_layer(first_request).unwrap();
        let second = state.add_generation_layer(second_request).unwrap();
        state.generate_selected_layer(first).unwrap();
        state.generate_selected_layer(second).unwrap();
        let other = state.config.generation_layers[1].clone();
        let before_other = state.config.generation_layers[1].brush_edits.clone();
        assert!(
            state
                .apply_generation_layer_brush(
                    first,
                    LayerBrushEdit::Paint {
                        center: [12.5, 20.0, 12.5],
                        radius: 3.0,
                        material: "wo56_custom_paint".to_owned()
                    }
                )
                .unwrap()
        );
        assert_eq!(state.config.generation_layers[0].brush_edits.len(), 1);
        assert_eq!(state.config.generation_layers[1].brush_edits, before_other);
        assert_eq!(state.config.generation_layers[1], other);
        assert_eq!(state.authored, original);
        state.rebuild_generation_layers().unwrap();
        let expected = state.layered_authored.clone();

        let base = std::env::temp_dir().join(format!(
            "worldkiln_wo56_layer_edits_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let storage = super::super::storage::TerrainStorage::new(&base);
        storage.save(&state).unwrap();
        assert!(base.join("terrain/layer_1_0.layerchunk").exists());
        let stored_meta = std::fs::read_to_string(base.join("terrain/terrain.json")).unwrap();
        assert!(
            !stored_meta.contains("wo56_custom_paint"),
            "Brush edits must be kept out of the global metadata"
        );
        let loaded = storage.load().unwrap().unwrap();
        assert_eq!(
            loaded.config.generation_layers,
            state.config.generation_layers
        );
        assert_eq!(loaded.authored, original);
        assert_eq!(loaded.layered_authored, expected);
        let first_modified = std::fs::metadata(base.join("terrain/layer_1_0.layerchunk"))
            .unwrap()
            .modified()
            .unwrap();
        storage.save(&loaded).unwrap();
        let second_modified = std::fs::metadata(base.join("terrain/layer_1_0.layerchunk"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            first_modified, second_modified,
            "Unchanged saved brush chunk must not be rewritten"
        );
        std::fs::remove_dir_all(base).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-052 | 2026-10-10 16:55 -05:00
    #[test]
    fn sole_generated_layer_preview_matches_recomposed_brush_and_skips_redundant_release_work() {
        let mut state = TerrainState::default();
        let request = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 32, 24],
        };
        let id = state.add_generation_layer(request).unwrap();
        state.generate_selected_layer(id).unwrap();
        assert!(!state.brush_recompose_required());
        let surface_y = match &state.layered_authored.as_ref().unwrap().data {
            TerrainData::Voxel(grid) => grid
                .surface_height_at(12.0, 12.0, crate::world::terrain::TerrainMode::Smooth)
                .unwrap(),
            _ => panic!("generated layer must be voxel"),
        };
        let brush = LayerBrushEdit::Paint {
            center: [12.0, surface_y, 12.0],
            radius: 3.0,
            material: "brush_test_sand".to_string(),
        };
        assert!(state.apply_generation_layer_brush(id, brush).unwrap());
        assert_eq!(state.config.generation_layers[0].brush_edits.len(), 1);
        let preview = state.layered_authored.clone();
        state.rebuild_generation_layers().unwrap();
        assert_eq!(
            preview, state.layered_authored,
            "skipped release rebuild must yield the same saved layer"
        );
        state.generate_authored(request).unwrap();
        assert!(
            state.brush_recompose_required(),
            "base + generated layer must composite"
        );
        state.authored = None;
        let second = state.add_generation_layer(request).unwrap();
        state.generate_selected_layer(second).unwrap();
        assert!(
            state.brush_recompose_required(),
            "two visible generated layers must composite"
        );
    }

    #[test]
    fn selected_layer_brush_rejects_locked_and_hidden_and_unapplied() {
        let mut state = TerrainState::default();
        let r = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 32, 24],
        };
        let id = state.add_generation_layer(r).unwrap();
        let edit = LayerBrushEdit::Sculpt(TerrainSculptRequest {
            tool: SculptTool::Lower,
            center: [12.5, 22.0, 12.5],
            radius: 3.0,
            strength: 1.0,
            flatten_target: None,
        });
        assert!(
            state
                .apply_generation_layer_brush(id, edit.clone())
                .is_err()
        );
        state.generate_selected_layer(id).unwrap();
        state.config.generation_layers[0].locked = true;
        assert!(
            state
                .apply_generation_layer_brush(id, edit.clone())
                .is_err()
        );
        state.config.generation_layers[0].locked = false;
        state.set_generation_layer_visible(id, false).unwrap();
        assert!(
            state
                .apply_generation_layer_brush(id, edit.clone())
                .is_err()
        );
        state.set_generation_layer_visible(id, true).unwrap();
        assert!(state.apply_generation_layer_brush(id, edit).unwrap());
        assert_eq!(state.config.generation_layers[0].brush_edits.len(), 1);
    }

    // Worldkiln 0.8.5 | WO MAT-055 | 2026-10-10 16:51 -05:00
    #[test]
    fn selected_generated_layer_waterline_updates_shared_water_plane() {
        let mut state = TerrainState::default();
        let request = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 32, 24],
        };
        let first = state.add_generation_layer(request).unwrap();
        state.config.generation_layers[0].settings.water_level = 19.0;
        let original = state.config.plains.water_level;
        assert_ne!(original, 19.0);
        state.generate_selected_layer(first).unwrap();
        assert_eq!(state.config.plains.water_level, 19.0);
        assert_eq!(
            state.config.generation_layers[0]
                .applied
                .as_ref()
                .unwrap()
                .water_level,
            state.config.plains.water_level
        );
        let second = state.add_generation_layer(request).unwrap();
        state.config.generation_layers[1].settings.water_level = 9.0;
        state.generate_selected_layer(second).unwrap();
        assert_eq!(state.config.plains.water_level, 9.0);
        assert_eq!(
            state.config.generation_layers[0]
                .applied
                .as_ref()
                .unwrap()
                .water_level,
            19.0,
            "Updating the shared water plane must not rewrite another layer's recipe"
        );
    }

    #[test]
    fn new_layer_does_not_get_a_name() {
        let layer = GenerationLayer::new(
            1,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 16, 16],
            },
        );
        assert!(layer.name.is_none());
        assert!(layer.applied.is_none());
        assert!(serde_json::to_value(&layer).unwrap()["name"].is_null());
    }
    #[test]
    fn generator_repeats_for_same_seed() {
        let cfg = TerrainConfig::default();
        let mut s = GeneratorSettings::default();
        s.request.size = [32, 32, 32];
        assert_eq!(
            generate_layer(&s, &cfg).unwrap(),
            generate_layer(&s, &cfg).unwrap()
        );
        s.kind = Landform::Islands;
        assert_ne!(
            generate_layer(&s, &cfg).unwrap(),
            generate_layer(
                &GeneratorSettings {
                    kind: Landform::Hills,
                    ..s.clone()
                },
                &cfg
            )
            .unwrap()
        );
    }
    #[test]
    fn regenerating_only_the_selected_layer_preserves_base() {
        let mut state = TerrainState::default();
        let r = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [32, 32, 32],
        };
        state.generate_authored(r).unwrap();
        let before = state.authored.clone();
        let id = state.add_generation_layer(r).unwrap();
        state.config.generation_layers[0].settings.kind = Landform::Mountains;
        state.generate_selected_layer(id).unwrap();
        assert_eq!(state.authored, before);
        assert!(state.layered_authored.is_some());
        state.set_generation_layer_visible(id, false).unwrap();
        assert_eq!(state.authored, before);
        state.delete_generation_layer(id).unwrap();
        assert_eq!(state.authored, before);
        assert!(state.layered_authored.is_none());
    }

    // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09
    #[test]
    fn generated_layer_settings_survive_real_project_save_and_reopen() {
        let mut state = TerrainState::default();
        let r = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 24, 24],
        };
        state.generate_authored(r).unwrap();
        let base = state.authored.clone();
        let first = state.add_generation_layer(r).unwrap();
        let second = state.add_generation_layer(r).unwrap();
        assert_eq!(state.config.generation_layers.len(), 2);
        state.config.generation_layers[0].settings.kind = Landform::Islands;
        state.config.generation_layers[0].settings.seed = 718;
        state.config.generation_layers[1].settings.kind = Landform::Mountains;
        state.config.generation_layers[1].settings.seed = 123;
        state.generate_selected_layer(first).unwrap();
        state.generate_selected_layer(second).unwrap();
        let unchanged = state.config.generation_layers[0].applied.clone();
        state.config.generation_layers[1].settings.seed = 124;
        state.generate_selected_layer(second).unwrap();
        assert_eq!(state.config.generation_layers[0].applied, unchanged);
        assert_eq!(state.authored, base);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "Worldkiln_wo23_layers_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = super::super::storage::TerrainStorage::new(&dir);
        storage.save(&state).unwrap();
        let loaded = storage.load().unwrap().unwrap();
        assert_eq!(loaded.authored, base);
        assert_eq!(
            loaded.config.generation_layers,
            state.config.generation_layers
        );
        assert_eq!(loaded.layered_authored, state.layered_authored);
        assert!(
            loaded
                .config
                .generation_layers
                .iter()
                .all(|layer| layer.name.is_none())
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
