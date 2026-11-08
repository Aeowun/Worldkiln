// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::material::{
    self, MaterialCatalog, MaterialDefinition, MaterialDocument, Parameter, ParameterValue,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct MaterialSaveRequest {
    pub project: PathBuf,
    pub name: String,
    pub document: MaterialDocument,
    pub expected: Option<Vec<u8>>,
}
impl MaterialSaveRequest {
    pub fn run(&self) -> Result<MaterialCatalog, String> {
        material::save_and_load(
            &self.project,
            &self.name,
            &self.document,
            self.expected.as_deref(),
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaterialDraft {
    project: PathBuf,
    name: String,
    document: MaterialDocument,
    expected: Option<Vec<u8>>,
    saved: Option<MaterialDocument>,
    undo: Vec<MaterialDocument>,
    redo: Vec<MaterialDocument>,
    #[serde(skip)]
    drag_origin: Option<MaterialDocument>,
    dependencies: BTreeMap<String, MaterialDocument>,
}
impl MaterialDraft {
    pub fn create(
        project: &Path,
        name: &str,
        definition: MaterialDefinition,
    ) -> Result<Self, String> {
        let name = material::identifier(name)?;
        material::file_path(project, &name)?;
        let document = MaterialDocument::new(definition);
        let catalog = MaterialCatalog::load(project);
        let dependencies = if matches!(&document.definition, MaterialDefinition::Asset { .. }) {
            document.validate()?;
            if catalog
                .documents()
                .values()
                .any(|entry| entry.id == document.id)
            {
                return Err("Material identity already belongs to another asset".into());
            }
            BTreeMap::new()
        } else {
            let docs = catalog.documents().clone();
            Self::validate_document(&docs, &name, &document)?;
            docs
        };
        Ok(Self {
            project: project.to_owned(),
            name,
            document,
            expected: None,
            saved: None,
            undo: Vec::new(),
            redo: Vec::new(),
            drag_origin: None,
            dependencies,
        })
    }
    pub fn open(project: &Path, name: &str) -> Result<Self, String> {
        let name = material::identifier(name)?;
        let expected = material::read_bytes(&material::file_path(project, &name)?)
            .map_err(|e| e.to_string())?;
        let document: MaterialDocument =
            serde_json::from_slice(&expected).map_err(|e| e.to_string())?;
        let catalog = MaterialCatalog::load(project);
        let matches_disk = catalog.documents().get(&name) == Some(&document);
        let dependencies = if matches!(&document.definition, MaterialDefinition::Asset { .. }) {
            if matches_disk {
                catalog.resolve(&document.id)?;
            } else {
                Self::validate_document(catalog.documents(), &name, &document)?;
            }
            BTreeMap::new()
        } else {
            let docs = catalog.documents().clone();
            if matches_disk {
                catalog.resolve(&document.id)?;
            } else {
                Self::validate_document(&docs, &name, &document)?;
            }
            docs
        };
        Ok(Self {
            project: project.to_owned(),
            name,
            document: document.clone(),
            expected: Some(expected),
            saved: Some(document),
            undo: Vec::new(),
            redo: Vec::new(),
            drag_origin: None,
            dependencies,
        })
    }
    pub fn open_with_snapshot(
        project: &Path,
        name: &str,
        catalog: &MaterialCatalog,
    ) -> Result<Self, String> {
        let name = material::identifier(name)?;
        if catalog.project() != Some(project) {
            return Self::open(project, &name);
        }
        let expected = material::read_bytes(&material::file_path(project, &name)?)
            .map_err(|e| e.to_string())?;
        let document: MaterialDocument =
            serde_json::from_slice(&expected).map_err(|e| e.to_string())?;
        if catalog.documents().get(&name) != Some(&document) {
            return Self::open(project, &name);
        }
        catalog.resolve(&document.id)?;
        let dependencies = if matches!(&document.definition, MaterialDefinition::Asset { .. }) {
            BTreeMap::new()
        } else {
            catalog.documents().clone()
        };
        Ok(Self {
            project: project.to_owned(),
            name,
            document: document.clone(),
            expected: Some(expected),
            saved: Some(document),
            undo: Vec::new(),
            redo: Vec::new(),
            drag_origin: None,
            dependencies,
        })
    }
    fn validate_document(
        dependencies: &BTreeMap<String, MaterialDocument>,
        name: &str,
        document: &MaterialDocument,
    ) -> Result<(), String> {
        let mut documents = dependencies.clone();
        documents.insert(name.into(), document.clone());
        MaterialCatalog::from_documents(documents)
            .resolve(&document.id)
            .map(|_| ())
    }
    pub fn document(&self) -> &MaterialDocument {
        &self.document
    }
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.document)
    }
    fn edit_document(&mut self, candidate: MaterialDocument) -> Result<(), String> {
        if matches!(&self.document.definition, MaterialDefinition::Asset { .. })
            && matches!(&candidate.definition, MaterialDefinition::Asset { .. })
        {
            candidate.validate()?;
        } else {
            Self::validate_document(&self.dependencies, &self.name, &candidate)?;
        }
        self.finish_graph_drag();
        if candidate != self.document {
            self.undo
                .push(std::mem::replace(&mut self.document, candidate));
            if self.undo.len() > 64 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        Ok(())
    }
    pub fn edit(&mut self, definition: MaterialDefinition) -> Result<(), String> {
        self.edit_document(MaterialDocument {
            definition,
            ..self.document.clone()
        })
    }
    pub fn edit_graph(&mut self, graph: Option<material::MaterialGraph>) -> Result<(), String> {
        if !matches!(&self.document.definition, MaterialDefinition::Asset { .. }) {
            return Err("Edit the parent Material graph; instances inherit its graph".into());
        }
        let candidate = MaterialDocument {
            graph,
            ..self.document.clone()
        };
        candidate.validate()?;
        self.finish_graph_drag();
        if candidate != self.document {
            self.undo
                .push(std::mem::replace(&mut self.document, candidate));
            if self.undo.len() > 64 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        Ok(())
    }
    pub fn edit_graph_transient(&mut self, graph: material::MaterialGraph) -> Result<(), String> {
        if !matches!(self.document.definition, MaterialDefinition::Asset { .. }) {
            return Err("Edit the parent Material graph; instances inherit its graph".into());
        }
        let candidate = MaterialDocument {
            graph: Some(graph),
            ..self.document.clone()
        };
        candidate.validate()?;
        if candidate != self.document {
            if self.drag_origin.is_none() {
                self.drag_origin = Some(self.document.clone());
            }
            self.document = candidate;
        }
        Ok(())
    }
    pub fn finish_graph_drag(&mut self) {
        if let Some(origin) = self.drag_origin.take() {
            if origin != self.document {
                self.undo.push(origin);
                if self.undo.len() > 64 {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
        }
    }
    pub fn edit_texture_maps(&mut self, textures: BTreeMap<String, String>) -> Result<(), String> {
        if !matches!(self.document.definition, MaterialDefinition::Asset { .. }) {
            return Err("Edit texture maps on the parent Material".into());
        }
        self.edit_document(MaterialDocument {
            textures,
            ..self.document.clone()
        })
    }
    pub fn edit_layers(&mut self, layers: Vec<material::MaterialLayer>) -> Result<(), String> {
        if !matches!(self.document.definition, MaterialDefinition::Asset { .. }) {
            return Err("Edit layers on the parent Material".into());
        }
        self.edit_document(MaterialDocument {
            layers,
            ..self.document.clone()
        })
    }
    pub fn override_parameter(
        &mut self,
        parameter: Parameter,
        value: ParameterValue,
    ) -> Result<(), String> {
        let MaterialDefinition::Instance { parent, overrides } = &self.document.definition else {
            return Err("Only instances have overrides".into());
        };
        let mut overrides = overrides.clone();
        overrides.insert(parameter, value);
        self.edit(MaterialDefinition::Instance {
            parent: parent.clone(),
            overrides,
        })
    }
    pub fn restore_editor_snapshot(&mut self, document: MaterialDocument) -> Result<(), String> {
        if document.id != self.document.id {
            return Err("Cannot undo across Material identities".into());
        }
        document.validate()?;
        self.drag_origin = None;
        self.document = document;
        self.undo.clear();
        self.redo.clear();
        Ok(())
    }
    pub fn undo(&mut self) -> bool {
        self.finish_graph_drag();
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.document, previous));
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        self.finish_graph_drag();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.document, next));
            true
        } else {
            false
        }
    }
    pub fn save_request(&mut self) -> MaterialSaveRequest {
        self.finish_graph_drag();
        MaterialSaveRequest {
            project: self.project.clone(),
            name: self.name.clone(),
            document: self.document.clone(),
            expected: self.expected.clone(),
        }
    }
    pub fn accept_saved(
        &mut self,
        request: &MaterialSaveRequest,
        _catalog: &MaterialCatalog,
    ) -> Result<(), String> {
        if self.project != request.project
            || self.name != request.name
            || self.document.id != request.document.id
        {
            return Err("Saved Material changed identity".into());
        }
        self.expected =
            Some(serde_json::to_vec_pretty(&request.document).map_err(|e| e.to_string())?);
        self.saved = Some(request.document.clone());
        self.dependencies
            .insert(self.name.clone(), request.document.clone());
        Ok(())
    }
    pub fn save(&mut self, project: &Path) -> Result<MaterialCatalog, String> {
        self.finish_graph_drag();
        if project != self.project {
            return Err("Material draft belongs to another project".into());
        }
        let snapshot = material::save_and_load(
            project,
            &self.name,
            &self.document,
            self.expected.as_deref(),
        )?;
        self.expected = Some(serde_json::to_vec_pretty(&self.document).map_err(|e| e.to_string())?);
        self.dependencies = snapshot.documents().clone();
        self.saved = Some(self.document.clone());
        Ok(snapshot)
    }
}

#[derive(Default)]
pub struct MaterialSessions {
    drafts: BTreeMap<(PathBuf, String), MaterialDraft>,
}
impl MaterialSessions {
    pub fn insert(&mut self, draft: MaterialDraft) -> Result<(), String> {
        let key = (draft.project.clone(), draft.name.clone());
        if self.drafts.get(&key).is_some_and(MaterialDraft::is_dirty) {
            return Err("Unsaved Material draft must be saved or explicitly discarded".into());
        }
        self.drafts.insert(key, draft);
        Ok(())
    }
    pub fn get_mut(&mut self, project: &Path, name: &str) -> Option<&mut MaterialDraft> {
        self.drafts
            .get_mut(&(project.to_owned(), material::identifier(name).ok()?))
    }
    pub fn discard(&mut self, project: &Path, name: &str) -> Result<(), String> {
        self.drafts
            .remove(&(project.to_owned(), material::identifier(name)?));
        Ok(())
    }
}

#[cfg(test)]
mod water_material_draft_tests {
    use super::*;
    use crate::{project::material::MaterialOutput, world::terrain::TerrainWaterSettings};
    #[test]
    fn water_material_sessions_preserve_dirty_history_and_project_ownership() {
        let project = std::env::temp_dir().join(format!(
            "water-material-draft-{:032x}",
            rand::random::<u128>()
        ));
        let definition = MaterialDefinition::Asset {
            material: MaterialOutput::Water(TerrainWaterSettings::default()),
        };
        let draft = MaterialDraft::create(&project, "a", definition.clone()).unwrap();
        let mut sessions = MaterialSessions::default();
        sessions.insert(draft.clone()).unwrap();
        assert!(sessions.insert(draft).is_err());
        let draft = sessions.get_mut(&project, "a").unwrap();
        let mut look = TerrainWaterSettings::default();
        look.wave_height = 2.0;
        draft
            .edit(MaterialDefinition::Asset {
                material: MaterialOutput::Water(look),
            })
            .unwrap();
        let edited = draft.document().clone();
        assert!(draft.undo());
        assert!(draft.redo());
        assert_eq!(draft.document(), &edited);
        assert!(draft.save(Path::new("other_project")).is_err());
        draft.save(&project).unwrap();
        assert!(!draft.is_dirty());
        draft.undo();
        let restored: MaterialDraft =
            serde_json::from_slice(&serde_json::to_vec(draft).unwrap()).unwrap();
        assert!(restored.is_dirty());
        assert_eq!(restored.document(), draft.document());
        std::fs::remove_dir_all(project).unwrap();
    }
}

#[cfg(test)]
mod graph_edit_performance_tests {
    use super::*;
    use crate::project::material::{
        GraphLink, GraphNode, GraphOperation, MaterialGraph, MaterialOutput,
    };
    fn graph(position: f32) -> MaterialGraph {
        MaterialGraph {
            output_node: 2,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::Color {
                        value: [0.3, 0.5, 0.7, 1.0],
                    },
                    position: [position, 1.0],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::SurfaceOutput,
                    position: [10.0, 10.0],
                },
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 2,
                target_pin: "base_color".into(),
            }],
        }
    }
    fn new_draft() -> MaterialDraft {
        MaterialDraft::create(
            Path::new("mat014-test-no-disk"),
            "test",
            MaterialDefinition::Asset {
                material: MaterialOutput::SurfacePbr {
                    base_color: [0.3, 0.5, 0.7, 1.0],
                    roughness: 0.5,
                    metallic: 0.0,
                },
            },
        )
        .unwrap()
    }
    #[test]
    fn one_drag_creates_one_undo_and_preserves_saving() {
        let mut draft = new_draft();
        draft.edit_graph(Some(graph(0.0))).unwrap();
        let base = draft.document().clone();
        for i in 1..128 {
            draft.edit_graph_transient(graph(i as f32)).unwrap();
        }
        draft.finish_graph_drag();
        assert_eq!(draft.undo.len(), 2);
        assert!(draft.undo());
        assert_eq!(draft.document(), &base);
        assert!(draft.redo());
        assert_eq!(draft.document().graph.as_ref().unwrap(), &graph(127.0));
        let before = draft.document().clone();
        let mut invalid = graph(127.0);
        invalid.links[0].source_node = 999;
        assert!(draft.edit_graph_transient(invalid).is_err());
        assert_eq!(draft.document(), &before);
        assert!(!draft.drag_origin.is_some());
    }
    #[test]
    fn reverting_drag_without_net_changes_adds_no_undo_step() {
        let mut draft = new_draft();
        draft.edit_graph(Some(graph(0.0))).unwrap();
        draft.edit_graph_transient(graph(1.0)).unwrap();
        draft.edit_graph_transient(graph(0.0)).unwrap();
        draft.finish_graph_drag();
        assert_eq!(draft.undo.len(), 1);
    }
    #[test]
    #[ignore = "Profiling only: dependency count versus graph edit latency"]
    fn graph_edit_scaling_probe() {
        use std::time::Instant;
        for count in [1usize, 64, 512, 4096] {
            let mut draft = new_draft();
            for i in 0..count {
                let mut doc = draft.document().clone();
                doc.id = format!("dep-{i:04}");
                draft.dependencies.insert(format!("dep_{i:04}"), doc);
            }
            let start = Instant::now();
            for i in 0..64 {
                draft.edit_graph_transient(graph(i as f32)).unwrap();
            }
            draft.finish_graph_drag();
            eprintln!(
                "MAT014_GRAPH deps={count} transient_drag_64_ms={:.3} undo_depth={}",
                start.elapsed().as_secs_f64() * 1000.0,
                draft.undo.len()
            );
        }
    }
}
