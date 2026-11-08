// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::world::terrain::TerrainWaterSettings;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
#[path = "material_graph.rs"]
mod graph;
#[path = "material_graph_function.rs"]
pub mod graph_function;
#[path = "material_graph_glsl.rs"]
pub mod graph_glsl;
#[path = "material_graph_post_process.rs"]
pub mod graph_post_process;
#[path = "material_graph_uniform.rs"]
mod graph_uniform;
#[path = "material_parameters.rs"]
mod parameters;
#[path = "material_storage.rs"]
mod storage;
pub use graph::{CustomPin, GraphLink, GraphNode, GraphOperation, MaterialGraph, PinType};
pub use parameters::{Parameter, ParameterValue};
pub fn evaluate_graph_water_for_preview(
    graph: &MaterialGraph,
    base: TerrainWaterSettings,
    maps: &BTreeMap<String, String>,
) -> Result<TerrainWaterSettings, String> {
    graph_uniform::evaluate_water(graph, base, maps)
}

#[cfg(test)]
#[path = "material_stars_tests.rs"]
mod stars_material_tests;

pub const MATERIAL_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "output", content = "parameters", deny_unknown_fields)]
pub enum MaterialOutput {
    Water(TerrainWaterSettings),
    Sky(crate::project::sky_material::SkyGradient),
    PostProcess,
    SurfacePbr {
        base_color: [f32; 4],
        roughness: f32,
        metallic: f32,
    },
}
impl MaterialOutput {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Water(settings) => settings.validate().map_err(|e| e.to_string()),
            Self::Sky(gradient) => gradient.validate(),
            Self::PostProcess => Ok(()),
            Self::SurfacePbr {
                base_color,
                roughness,
                metallic,
            } => {
                if base_color
                    .iter()
                    .chain([roughness, metallic])
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                {
                    Err("Surface PBR parameters must be finite and between 0 and 1".into())
                } else {
                    Ok(())
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum MaterialDefinition {
    Asset {
        material: MaterialOutput,
    },
    Instance {
        parent: String,
        #[serde(default)]
        overrides: BTreeMap<Parameter, ParameterValue>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialLayerBlend {
    Normal,
    Multiply,
    Add,
    Screen,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialLayer {
    pub name: String,
    pub texture: String,
    pub opacity: f32,
    pub blend: MaterialLayerBlend,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDocument {
    pub version: u32,
    pub id: String,
    pub definition: MaterialDefinition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<MaterialGraph>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub textures: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<MaterialLayer>,
}
impl MaterialDocument {
    pub fn new(definition: MaterialDefinition) -> Self {
        Self {
            version: MATERIAL_VERSION,
            id: format!("mat-{:032x}", rand::random::<u128>()),
            definition,
            graph: None,
            textures: BTreeMap::new(),
            layers: Vec::new(),
        }
    }
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != MATERIAL_VERSION || self.id.is_empty() || self.id.len() > 128 {
            return Err("Unsupported Material version or invalid stable ID".into());
        }
        match &self.definition {
            MaterialDefinition::Asset { material } => {
                material.validate()?;
                if let Some(graph) = &self.graph {
                    graph.validate(material, &self.textures)?;
                }
            }
            MaterialDefinition::Instance { .. } if self.graph.is_some() => {
                return Err(
                    "Material instances inherit parent graphs; edit the parent graph".into(),
                );
            }
            MaterialDefinition::Instance { .. } => {}
        }
        if self.textures.iter().any(|(kind, path)| {
            !["BaseColor", "NormalGL", "Roughness", "AO", "Height", "ARM"].contains(&kind.as_str())
                || path.trim().is_empty()
                || path.starts_with("Material:")
        }) {
            return Err("Invalid Material texture map".into());
        }
        if self.layers.len() > 16 {
            return Err("Too many Material layers".into());
        }
        let mut names = BTreeSet::new();
        for layer in &self.layers {
            if layer.name.trim().is_empty()
                || layer.name.len() > 64
                || !names.insert(layer.name.clone())
                || layer.texture.trim().is_empty()
                || layer.texture.starts_with("Material:")
                || !layer.opacity.is_finite()
                || !(0.0..=1.0).contains(&layer.opacity)
            {
                return Err("Invalid Material layer".into());
            }
        }
        Ok(())
    }
}

pub fn identifier(value: &str) -> Result<String, String> {
    let value = value.trim().replace('\\', "/");
    let value = if value.to_ascii_lowercase().ends_with(".json") {
        &value[..value.len() - 5]
    } else {
        &value
    };
    if value.is_empty()
        || value.len() > 240
        || value.contains(':')
        || value
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || !Path::new(value)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("Material path must remain inside .assets/materials".into());
    }
    Ok(value.to_owned())
}
pub fn file_path(project: &Path, name: &str) -> Result<PathBuf, String> {
    let root = project.join(".assets/materials");
    let path = root.join(format!("{}.json", identifier(name)?));
    let mut ancestor = path.as_path();
    while ancestor != project {
        if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Material paths cannot traverse links".into());
        }
        ancestor = ancestor.parent().ok_or("Invalid Material root")?;
    }
    Ok(path)
}
pub(crate) fn read_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(std::io::Error::other("Material exceeds 256 KiB"));
    }
    Ok(bytes)
}
pub fn read(project: &Path, name: &str) -> Result<MaterialDocument, String> {
    let path = file_path(project, name)?;
    let document: MaterialDocument =
        serde_json::from_slice(&read_bytes(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
    document.validate()?;
    Ok(document)
}
pub fn save(
    project: &Path,
    name: &str,
    document: &MaterialDocument,
    expected: Option<&[u8]>,
) -> Result<(), String> {
    save_and_load(project, name, document, expected).map(|_| ())
}
pub fn save_and_load(
    project: &Path,
    name: &str,
    document: &MaterialDocument,
    expected: Option<&[u8]>,
) -> Result<MaterialCatalog, String> {
    document.validate()?;
    let path = file_path(project, name)?;
    let transaction = storage::Transaction::begin(project)?;
    let actual = match read_bytes(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if actual.as_deref() != expected {
        return Err("Material changed on disk; reopen it before saving".into());
    }
    if let Some(bytes) = actual.as_deref() {
        let previous: MaterialDocument = serde_json::from_slice(bytes)
            .map_err(|e| format!("Cannot safely replace an unreadable Material: {e}"))?;
        if previous.id != document.id {
            return Err("An existing Material identity cannot be replaced".into());
        }
        if let (
            MaterialDefinition::Asset { material: old },
            MaterialDefinition::Asset { material: new },
        ) = (&previous.definition, &document.definition)
        {
            if std::mem::discriminant(old) != std::mem::discriminant(new) {
                return Err(
                    "An existing Material output type cannot change; create a new asset".into(),
                );
            }
        }
    }
    let current = MaterialCatalog::load(project);
    let name = identifier(name)?;
    if let Some(old) = current.documents.get(&name) {
        if old.id != document.id {
            return Err("An existing Material identity cannot be replaced".into());
        }
    }
    let mut documents = current.documents.clone();
    documents.insert(name, document.clone());
    let mut candidate = MaterialCatalog::from_documents(documents);
    candidate.source_errors = current.source_errors.clone();
    candidate.project = current.project.clone();
    let new_output = candidate.resolve(&document.id)?;
    if let Ok(old_output) = current.resolve(&document.id) {
        if std::mem::discriminant(&old_output) != std::mem::discriminant(&new_output) {
            return Err(
                "An existing Material output type cannot change; create a new asset".into(),
            );
        }
    }
    for (id, output) in &current.resolved {
        if output.is_ok() {
            candidate
                .resolve(id)
                .map_err(|e| format!("Save invalidates Material {id}: {e}"))?;
        }
    }
    let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("Material exceeds 256 KiB".into());
    }
    transaction.publish(&path, &bytes, expected)?;
    MaterialCatalog::invalidate_cached(project);
    crate::project::asset_registry::AssetRegistry::reconcile(project)?;
    crate::project::asset_registry::AssetRegistry::notify_content_changed(project)?;
    Ok(candidate)
}
pub fn recover_interrupted_save(project: &Path) -> Result<(), String> {
    storage::recover(project)
}

#[derive(Clone, Debug, Default)]
pub struct MaterialCatalog {
    documents: BTreeMap<String, MaterialDocument>,
    source_errors: BTreeMap<String, String>,
    resolved: BTreeMap<String, Result<MaterialOutput, String>>,
    dependents: BTreeMap<String, Vec<String>>,
    project: Option<PathBuf>,
    last_valid_water: BTreeMap<String, TerrainWaterSettings>,
    graph_water: BTreeMap<String, Result<TerrainWaterSettings, String>>,
    graph_surface: BTreeMap<String, Result<[f32; 2], String>>,
}
fn material_cache() -> &'static std::sync::Mutex<BTreeMap<PathBuf, std::sync::Arc<MaterialCatalog>>>
{
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<BTreeMap<PathBuf, std::sync::Arc<MaterialCatalog>>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(BTreeMap::new()))
}
impl MaterialCatalog {
    pub fn cached(project: &Path) -> std::sync::Arc<Self> {
        use std::sync::Arc;
        let store = material_cache();
        let mut entries = store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries
            .entry(project.to_path_buf())
            .or_insert_with(|| Arc::new(Self::load(project)))
            .clone()
    }
    pub fn invalidate_cached(project: &Path) {
        material_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(project);
    }
    pub fn project(&self) -> Option<&Path> {
        self.project.as_deref()
    }
    pub fn retain_last_valid(&mut self, previous: &Self) {
        if self.project == previous.project {
            for (id, look) in &previous.last_valid_water {
                self.last_valid_water.entry(id.clone()).or_insert(*look);
            }
        }
    }
    pub fn last_known_water(&self, id: &str) -> Option<TerrainWaterSettings> {
        self.water(id)
            .or_else(|| self.last_valid_water.get(id).copied())
    }
    pub fn documents(&self) -> &BTreeMap<String, MaterialDocument> {
        &self.documents
    }
    pub fn errors(&self) -> BTreeMap<String, String> {
        let mut errors = self.source_errors.clone();
        for (name, doc) in &self.documents {
            if let Some(Err(error)) = self.resolved.get(&doc.id) {
                errors.insert(name.clone(), error.clone());
            }
            if let Some(Err(error)) = self.graph_water.get(&doc.id) {
                errors.insert(name.clone(), format!("Water graph: {error}"));
            }
            if let Some(Err(error)) = self.graph_surface.get(&doc.id) {
                errors.insert(name.clone(), format!("Surface graph: {error}"));
            }
        }
        errors
    }
    pub fn from_documents(documents: BTreeMap<String, MaterialDocument>) -> Self {
        fn compile(
            id: &str,
            index: &BTreeMap<String, Vec<&MaterialDocument>>,
            cache: &mut BTreeMap<String, Result<(MaterialOutput, usize), String>>,
            stack: &mut BTreeSet<String>,
        ) -> Result<(MaterialOutput, usize), String> {
            if let Some(cached) = cache.get(id) {
                return cached.clone();
            }
            if stack.len() >= 32 || !stack.insert(id.into()) {
                return Err("Material instance cycle or inheritance limit exceeded".into());
            }
            let result = (|| {
                let matches = index
                    .get(id)
                    .ok_or_else(|| format!("Missing parent Material {id}"))?;
                if matches.len() != 1 {
                    return Err(format!("Duplicate Material ID {id}"));
                }
                let document = matches[0];
                document.validate()?;
                let (output, depth) = match &document.definition {
                    MaterialDefinition::Asset { material } => (material.clone(), 1),
                    MaterialDefinition::Instance { parent, overrides } => {
                        let (mut output, depth) = compile(parent, index, cache, stack)?;
                        if depth >= 32 {
                            return Err("Material inheritance limit exceeded".into());
                        }
                        for (parameter, value) in overrides {
                            parameter.apply(&mut output, value)?;
                        }
                        (output, depth + 1)
                    }
                };
                output.validate()?;
                Ok((output, depth))
            })();
            stack.remove(id);
            cache.insert(id.into(), result.clone());
            result
        }
        let mut index: BTreeMap<String, Vec<&MaterialDocument>> = BTreeMap::new();
        let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, doc) in &documents {
            index.entry(doc.id.clone()).or_default().push(doc);
            if let MaterialDefinition::Instance { parent, .. } = &doc.definition {
                dependents
                    .entry(parent.clone())
                    .or_default()
                    .push(name.clone());
            }
        }
        let mut cache = BTreeMap::new();
        for id in index.keys() {
            let _ = compile(id, &index, &mut cache, &mut BTreeSet::new());
        }
        let resolved: BTreeMap<String, Result<MaterialOutput, String>> = cache
            .into_iter()
            .map(|(id, result)| (id, result.map(|(output, _)| output)))
            .collect();
        let last_valid_water = resolved
            .iter()
            .filter_map(|(id, result)| match result {
                Ok(MaterialOutput::Water(look)) => Some((id.clone(), *look)),
                _ => None,
            })
            .collect();
        let mut catalog = Self {
            documents,
            source_errors: BTreeMap::new(),
            resolved,
            dependents,
            project: None,
            last_valid_water,
            graph_water: BTreeMap::new(),
            graph_surface: BTreeMap::new(),
        };
        let water_ids: Vec<_> = catalog
            .resolved
            .iter()
            .filter_map(|(id, result)| match result {
                Ok(MaterialOutput::Water(water)) => Some((id.clone(), *water)),
                _ => None,
            })
            .collect();
        for (id, water) in water_ids {
            if let Ok(Some(graph)) = catalog.graph(&id) {
                let result = catalog
                    .texture_maps(&id)
                    .and_then(|maps| graph_uniform::evaluate_water(graph, water, &maps));
                if let Ok(settings) = &result {
                    catalog.last_valid_water.insert(id.clone(), *settings);
                }
                catalog.graph_water.insert(id, result);
            }
        }
        let surface_ids: Vec<_> = catalog
            .resolved
            .iter()
            .filter_map(|(id, result)| match result {
                Ok(output @ MaterialOutput::SurfacePbr { .. }) => {
                    Some((id.clone(), output.clone()))
                }
                _ => None,
            })
            .collect();
        for (id, output) in surface_ids {
            if let Ok(Some(graph)) = catalog.graph(&id) {
                let result = catalog
                    .texture_maps(&id)
                    .and_then(|maps| graph_uniform::evaluate_surface_pbr(graph, &output, &maps));
                catalog.graph_surface.insert(id, result);
            }
        }
        catalog
    }
    pub fn preview(project: &Path, documents: BTreeMap<String, MaterialDocument>) -> Self {
        let mut catalog = Self::from_documents(documents);
        catalog.project = Some(project.to_owned());
        catalog
    }
    pub fn load(project: &Path) -> Self {
        fn scan(
            project: &Path,
            dir: &Path,
            depth: usize,
            documents: &mut BTreeMap<String, MaterialDocument>,
            errors: &mut BTreeMap<String, String>,
        ) {
            if depth > 16 {
                errors.insert(
                    dir.display().to_string(),
                    "Material directory depth limit exceeded".into(),
                );
                return;
            }
            if let Ok(Some(false) | None) = super::fs_walk::safe_kind(dir) {
                return;
            }
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                Err(e) => {
                    errors.insert(dir.display().to_string(), e.to_string());
                    return;
                }
            };
            for entry in entries {
                if documents.len() + errors.len() >= 4096 {
                    errors.insert("catalog".into(), "Material catalog limit exceeded".into());
                    return;
                }
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        errors.insert(dir.display().to_string(), e.to_string());
                        continue;
                    }
                };
                let kind = match super::fs_walk::safe_kind(&entry.path()) {
                    Ok(Some(kind)) => kind,
                    Ok(None) => continue,
                    Err(e) => {
                        errors.insert(entry.path().display().to_string(), e.to_string());
                        continue;
                    }
                };
                if kind {
                    scan(project, &entry.path(), depth + 1, documents, errors);
                } else if entry
                    .path()
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                {
                    let path = entry.path();
                    let Ok(relative) = path.strip_prefix(project.join(".assets/materials")) else {
                        continue;
                    };
                    let name = relative
                        .with_extension("")
                        .to_string_lossy()
                        .replace('\\', "/");
                    match read(project, &name) {
                        Ok(doc) => {
                            documents.insert(name, doc);
                        }
                        Err(e) => {
                            errors.insert(name, e);
                        }
                    }
                }
            }
        }
        let mut documents = BTreeMap::new();
        let mut errors = BTreeMap::new();
        scan(
            project,
            &project.join(".assets/materials"),
            0,
            &mut documents,
            &mut errors,
        );
        let mut catalog = Self::from_documents(documents);
        catalog.source_errors = errors;
        catalog.project = Some(project.to_owned());
        catalog
    }
    pub fn resolve(&self, id: &str) -> Result<MaterialOutput, String> {
        self.resolved
            .get(id)
            .cloned()
            .unwrap_or_else(|| Err(format!("Missing Material {id}")))
    }
    pub fn graph(&self, id: &str) -> Result<Option<&MaterialGraph>, String> {
        self.resolve(id)?;
        let mut current = id;
        for _ in 0..=32 {
            let document = self
                .documents
                .values()
                .find(|document| document.id == current)
                .ok_or_else(|| format!("Missing Material {current}"))?;
            match &document.definition {
                MaterialDefinition::Asset { .. } => return Ok(document.graph.as_ref()),
                MaterialDefinition::Instance { parent, .. } => current = parent,
            }
        }
        Err("Material graph inheritance limit exceeded".into())
    }
    pub fn with_runtime_layer_alpha(
        &self,
        id: &str,
        name: &str,
        alpha: f32,
    ) -> Result<Self, String> {
        let mut next = self.clone();
        next.set_runtime_layer_alpha(id, name, alpha)?;
        Ok(next)
    }

    pub fn set_runtime_layer_alpha(
        &mut self,
        id: &str,
        name: &str,
        alpha: f32,
    ) -> Result<(), String> {
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err("Material layer alpha must be between 0 and 1".into());
        }
        let mut current = id.to_string();
        for _ in 0..=32 {
            let key = self
                .documents
                .iter()
                .find(|(_, doc)| doc.id == current)
                .map(|(key, _)| key.clone())
                .ok_or_else(|| format!("Missing Material {current}"))?;
            let doc = self.documents.get_mut(&key).expect("indexed Material");
            match &doc.definition {
                MaterialDefinition::Instance { parent, .. } => current = parent.clone(),
                MaterialDefinition::Asset { .. } => {
                    let layer = doc
                        .layers
                        .iter_mut()
                        .find(|layer| layer.name == name)
                        .ok_or_else(|| format!("Unknown layer {name}"))?;
                    let previous = layer.opacity;
                    layer.opacity = alpha;
                    if let Err(error) = doc.validate() {
                        if let Some(layer) = doc.layers.iter_mut().find(|layer| layer.name == name)
                        {
                            layer.opacity = previous;
                        }
                        return Err(error);
                    }
                    return Ok(());
                }
            }
        }
        Err("Material layer inheritance limit exceeded".into())
    }

    pub fn layers(&self, id: &str) -> Result<Vec<MaterialLayer>, String> {
        let mut current = id;
        for _ in 0..=32 {
            let doc = self
                .documents
                .values()
                .find(|doc| doc.id == current)
                .ok_or_else(|| format!("Missing Material {current}"))?;
            match &doc.definition {
                MaterialDefinition::Asset { .. } => return Ok(doc.layers.clone()),
                MaterialDefinition::Instance { parent, .. } => current = parent,
            }
        }
        Err("Material layer inheritance limit exceeded".into())
    }

    pub fn texture_maps(&self, id: &str) -> Result<BTreeMap<String, String>, String> {
        self.resolve(id)?;
        let mut chain = Vec::new();
        let mut current = id;
        for _ in 0..=32 {
            let document = self
                .documents
                .values()
                .find(|d| d.id == current)
                .ok_or_else(|| format!("Missing Material {current}"))?;
            chain.push(document);
            match &document.definition {
                MaterialDefinition::Instance { parent, .. } => current = parent,
                MaterialDefinition::Asset { .. } => {
                    let mut maps = BTreeMap::new();
                    for doc in chain.into_iter().rev() {
                        maps.extend(doc.textures.clone());
                    }
                    return Ok(maps);
                }
            }
        }
        Err("Material texture inheritance limit exceeded".into())
    }
    pub fn sky(&self, id: &str) -> Option<crate::project::sky_material::SkyGradient> {
        use crate::project::sky_material::{SkyGradient, SkyInterpolation, SkyStop};
        let color = match self.resolved.get(id)?.as_ref().ok()? {
            MaterialOutput::Sky(gradient) => return Some(gradient.clone()),
            MaterialOutput::PostProcess => return None,
            MaterialOutput::Water(water) => water.shallow_color,
            MaterialOutput::SurfacePbr { base_color, .. } => {
                [base_color[0], base_color[1], base_color[2]]
            }
        };
        Some(SkyGradient {
            stops: vec![
                SkyStop {
                    position: 0.0,
                    color,
                },
                SkyStop {
                    position: 1.0,
                    color,
                },
            ],
            interpolation: SkyInterpolation::SmoothLinear,
        })
    }
    pub fn surface_pbr(&self, id: &str) -> Option<[f32; 2]> {
        if let Some(Ok(params)) = self.graph_surface.get(id) {
            return Some(*params);
        }
        match self.resolved.get(id)? {
            Ok(MaterialOutput::SurfacePbr {
                roughness,
                metallic,
                ..
            }) => Some([*roughness, *metallic]),
            _ => None,
        }
    }
    pub fn water(&self, id: &str) -> Option<TerrainWaterSettings> {
        if let Some(Ok(settings)) = self.graph_water.get(id) {
            return Some(*settings);
        }
        match self.resolved.get(id)?.as_ref().ok()? {
            MaterialOutput::Water(settings) => Some(*settings),
            MaterialOutput::SurfacePbr { base_color, .. } => {
                let mut water = TerrainWaterSettings::default();
                let rgb = [base_color[0], base_color[1], base_color[2]];
                water.shallow_color = rgb;
                water.deep_color = rgb;
                water.opacity = base_color[3];
                Some(water)
            }
            MaterialOutput::PostProcess => None,
            MaterialOutput::Sky(gradient) => {
                let mut water = TerrainWaterSettings::default();
                let rgb = gradient.sample(0.0);
                water.shallow_color = rgb;
                water.deep_color = rgb;
                Some(water)
            }
        }
    }
    pub fn dependents(&self, id: &str) -> Vec<String> {
        self.dependents.get(id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod water_material_tests {
    use super::*;
    use crate::world::terrain::{WaterBody, WaterBodyKind};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(
                MaterialDocument::new(MaterialDefinition::Asset {
                    material: MaterialOutput::Water(TerrainWaterSettings::default()),
                })
                .id,
            );
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn water() -> MaterialDocument {
        MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(TerrainWaterSettings::default()),
        })
    }
    fn instance(parent: &str, overrides: &[(&str, serde_json::Value)]) -> MaterialDocument {
        MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent.into(),
            overrides: overrides
                .iter()
                .map(|(k, v)| {
                    (
                        serde_json::from_value(serde_json::json!(k)).unwrap(),
                        serde_json::from_value(v.clone()).unwrap(),
                    )
                })
                .collect(),
        })
    }
    #[test]
    fn water_material_instances_follow_parent_and_keep_independent_overrides() {
        let fixture = Fixture::new();
        let mut parent = water();
        save(&fixture.0, "looks/water", &parent, None).unwrap();
        let child = instance(&parent.id, &[("wave_height", serde_json::json!(1.5))]);
        save(&fixture.0, "river", &child, None).unwrap();
        let bytes = std::fs::read(file_path(&fixture.0, "looks/water").unwrap()).unwrap();
        if let MaterialDefinition::Asset {
            material: MaterialOutput::Water(settings),
        } = &mut parent.definition
        {
            settings.wave_height = 0.2;
            settings.underwater_fog_density = 0.3;
        }
        save(&fixture.0, "looks/water", &parent, Some(&bytes)).unwrap();
        let catalog = MaterialCatalog::load(&fixture.0);
        assert_eq!(catalog.water(&parent.id).unwrap().wave_height, 0.2);
        assert_eq!(catalog.water(&child.id).unwrap().wave_height, 1.5);
        assert_eq!(
            catalog.water(&child.id).unwrap().underwater_fog_density,
            0.3
        );
        assert_eq!(read(&fixture.0, "river").unwrap(), child);
    }
    #[test]
    fn water_material_rejects_cycles_unknown_parameters_and_wrong_types() {
        assert!(serde_json::from_value::<Parameter>(serde_json::json!("unsupported")).is_err());
        assert!(serde_json::from_value::<ParameterValue>(serde_json::json!("bad")).is_err());
        let mut base = water();
        let child = instance(&base.id, &[("wave_height", serde_json::json!(true))]);
        let catalog = MaterialCatalog::from_documents(BTreeMap::from([
            ("base".into(), base.clone()),
            ("child".into(), child.clone()),
        ]));
        assert!(catalog.resolve(&child.id).is_err());
        base.definition = instance(&child.id, &[]).definition;
        let catalog = MaterialCatalog::from_documents(BTreeMap::from([
            ("base".into(), base),
            ("child".into(), child.clone()),
        ]));
        assert!(catalog.resolve(&child.id).unwrap_err().contains("cycle"));
    }
    #[test]
    fn water_material_rejects_duplicate_ids_missing_parents_and_invalid_values() {
        let base = water();
        let mut catalog = MaterialCatalog::default();
        catalog.documents.insert("a".into(), base.clone());
        catalog.documents.insert("b".into(), base.clone());
        catalog = MaterialCatalog::from_documents(catalog.documents);
        assert!(catalog.resolve(&base.id).unwrap_err().contains("Duplicate"));
        catalog.documents.remove("b");
        let child = instance("missing", &[]);
        catalog.documents.insert("child".into(), child.clone());
        catalog = MaterialCatalog::from_documents(catalog.documents);
        assert!(catalog.resolve(&child.id).unwrap_err().contains("Missing"));
        catalog.documents.get_mut("child").unwrap().definition =
            instance(&base.id, &[("wave_height", serde_json::json!(-5))]).definition;
        catalog = MaterialCatalog::from_documents(catalog.documents);
        assert!(catalog.resolve(&child.id).is_err());
        catalog.documents.get_mut("a").unwrap().version = MATERIAL_VERSION + 1;
        catalog = MaterialCatalog::from_documents(catalog.documents);
        assert!(catalog.resolve(&base.id).is_err());
    }
    #[test]
    fn water_material_save_conflicts_and_unsafe_paths_preserve_original_bytes() {
        let fixture = Fixture::new();
        let doc = water();
        save(&fixture.0, "a", &doc, None).unwrap();
        let original = std::fs::read(file_path(&fixture.0, "a").unwrap()).unwrap();
        assert!(save(&fixture.0, "a", &water(), None).is_err());
        assert!(save(&fixture.0, "a", &doc, Some(b"stale")).is_err());
        for name in ["../a", "/a", "C:/a", "foo/../a", "foo//a", ""] {
            assert!(save(&fixture.0, name, &doc, None).is_err());
        }
        assert_eq!(
            std::fs::read(file_path(&fixture.0, "a").unwrap()).unwrap(),
            original
        );
    }
    #[test]
    fn water_material_body_roundtrip_fallback_and_output_validation() {
        let fixture = Fixture::new();
        let mut doc = water();
        if let MaterialDefinition::Asset {
            material: MaterialOutput::Water(settings),
        } = &mut doc.definition
        {
            settings.wave_height = 1.2;
        }
        save(&fixture.0, "lake", &doc, None).unwrap();
        let surface = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [1.0; 4],
                roughness: 0.5,
                metallic: 0.0,
            },
        });
        save(&fixture.0, "surface", &surface, None).unwrap();
        let catalog = MaterialCatalog::load(&fixture.0);
        let mut body = WaterBody::new(1, WaterBodyKind::Lake, [0.0, 0.0], 5.0);
        body.assign_material(&catalog, &doc.id).unwrap();
        let before = body.clone();
        assert!(body.assign_material(&catalog, &surface.id).is_ok());
        assert_ne!(before, body);
        assert_eq!(body.material_asset.as_deref(), Some(surface.id.as_str()));
        assert_eq!(body.appearance.unwrap().shallow_color, [1.0; 3]);
        let restored: WaterBody =
            serde_json::from_slice(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(restored, body);
        assert_eq!(
            restored
                .resolved_appearance(
                    &MaterialCatalog::default(),
                    &TerrainWaterSettings::default()
                )
                .wave_height,
            TerrainWaterSettings::default().wave_height
        );
        let mut legacy = serde_json::to_value(&body).unwrap();
        legacy.as_object_mut().unwrap().remove("material_asset");
        assert!(
            serde_json::from_value::<WaterBody>(legacy)
                .unwrap()
                .material_asset
                .is_none()
        );
    }
    #[test]
    fn water_material_lookup_is_cached_and_isolated_between_projects() {
        let a = Fixture::new();
        let b = Fixture::new();
        let doc = water();
        save(&a.0, "water", &doc, None).unwrap();
        let cached = MaterialCatalog::load(&a.0);
        std::fs::remove_file(file_path(&a.0, "water").unwrap()).unwrap();
        assert!(cached.water(&doc.id).is_some());
        assert!(MaterialCatalog::load(&a.0).water(&doc.id).is_none());
        assert!(MaterialCatalog::load(&b.0).water(&doc.id).is_none());
    }
    #[test]
    fn water_material_parent_publication_preserves_identity_output_and_descendants() {
        let fixture = Fixture::new();
        let mut base = water();
        save(&fixture.0, "base", &base, None).unwrap();
        let child = instance(&base.id, &[("wave_height", serde_json::json!(1.0))]);
        save(&fixture.0, "child", &child, None).unwrap();
        let expected = std::fs::read(file_path(&fixture.0, "base").unwrap()).unwrap();
        let mut changed = base.clone();
        changed.id = "different".into();
        assert!(save(&fixture.0, "base", &changed, Some(&expected)).is_err());
        base.definition = MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [1.0; 4],
                roughness: 0.5,
                metallic: 0.0,
            },
        };
        assert!(save(&fixture.0, "base", &base, Some(&expected)).is_err());
        base.definition = instance(&child.id, &[]).definition;
        assert!(save(&fixture.0, "base", &base, Some(&expected)).is_err());
        assert_eq!(
            std::fs::read(file_path(&fixture.0, "base").unwrap()).unwrap(),
            expected
        );
        assert!(MaterialCatalog::load(&fixture.0).water(&child.id).is_some());
    }
}

#[cfg(test)]
mod cross_target_material_tests {
    use super::*;
    fn asset(material: MaterialOutput) -> MaterialDocument {
        MaterialDocument::new(MaterialDefinition::Asset { material })
    }
    #[test]
    fn any_valid_material_can_supply_sky_and_water_without_mutating_source() {
        let color = [0.15, 0.42, 0.72, 0.85];
        let surface = asset(MaterialOutput::SurfacePbr {
            base_color: color,
            roughness: 0.5,
            metallic: 0.0,
        });
        let sky = asset(MaterialOutput::Sky(
            crate::project::sky_material::SkyGradient::default(),
        ));
        let water = asset(MaterialOutput::Water(TerrainWaterSettings::default()));
        let catalog = MaterialCatalog::from_documents(BTreeMap::from([
            ("grass".into(), surface.clone()),
            ("sky".into(), sky.clone()),
            ("water".into(), water.clone()),
        ]));
        assert_eq!(
            catalog.sky(&surface.id).unwrap().sample(0.5),
            [color[0], color[1], color[2]]
        );
        let adapted = catalog.water(&surface.id).unwrap();
        assert_eq!(adapted.shallow_color, [color[0], color[1], color[2]]);
        assert_eq!(adapted.deep_color, [color[0], color[1], color[2]]);
        assert_eq!(adapted.opacity, color[3]);
        assert!(catalog.sky(&water.id).is_some());
        assert_eq!(
            catalog.water(&sky.id).unwrap().shallow_color,
            crate::project::sky_material::SkyGradient::default().sample(0.0)
        );
        assert!(matches!(
            catalog.resolve(&surface.id).unwrap(),
            MaterialOutput::SurfacePbr { .. }
        ));
        assert!(matches!(
            catalog.resolve(&sky.id).unwrap(),
            MaterialOutput::Sky(_)
        ));
        assert!(matches!(
            catalog.resolve(&water.id).unwrap(),
            MaterialOutput::Water(_)
        ));
    }
}
