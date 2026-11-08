// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::MaterialOutput;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_NODES: usize = 256;
const MAX_LINKS: usize = 1024;
const MAX_CUSTOM_SOURCE_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinType {
    Scalar,
    Vec2,
    Vec3,
    Color,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomPin {
    pub name: String,
    pub value_type: PinType,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphOperation {
    Scalar {
        value: f32,
    },
    Vec2 {
        value: [f32; 2],
    },
    Vec3 {
        value: [f32; 3],
    },
    Color {
        value: [f32; 4],
    },
    Uv,
    SceneColor,
    SceneDepth,
    SceneNormal,
    ObjectMask,
    ScreenUv,
    Time,
    Parameter {
        name: String,
        value_type: PinType,
    },
    TextureSample {
        slot: String,
    },
    Add {
        value_type: PinType,
    },
    Multiply {
        value_type: PinType,
    },
    Mix {
        value_type: PinType,
    },
    LayerBlend,
    WaterWave,
    CustomFunction {
        source: String,
        inputs: Vec<CustomPin>,
        outputs: Vec<CustomPin>,
    },
    SurfaceOutput,
    WaterOutput,
    SkyOutput,
    PostProcessOutput,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphNode {
    pub id: u32,
    pub operation: GraphOperation,
    #[serde(default, skip_serializing_if = "is_default_position")]
    pub position: [f32; 2],
}

fn is_default_position(position: &[f32; 2]) -> bool {
    *position == [0.0, 0.0]
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GraphLink {
    pub source_node: u32,
    pub source_pin: String,
    pub target_node: u32,
    pub target_pin: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialGraph {
    pub output_node: u32,
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub links: Vec<GraphLink>,
}

impl GraphOperation {
    pub fn pins(&self) -> Result<(BTreeMap<String, PinType>, BTreeMap<String, PinType>), String> {
        use GraphOperation::*;
        let mut inputs = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        match self {
            Scalar { value } => {
                validate_values(&[*value])?;
                outputs.insert("value".into(), PinType::Scalar);
            }
            Vec2 { value } => {
                validate_values(value)?;
                outputs.insert("value".into(), PinType::Vec2);
            }
            Vec3 { value } => {
                validate_values(value)?;
                outputs.insert("value".into(), PinType::Vec3);
            }
            Color { value } => {
                validate_values(value)?;
                outputs.insert("value".into(), PinType::Color);
            }
            Uv | ScreenUv => {
                outputs.insert("uv".into(), PinType::Vec2);
            }
            SceneColor => {
                outputs.insert("color".into(), PinType::Color);
            }
            SceneDepth => {
                outputs.insert("depth".into(), PinType::Scalar);
            }
            SceneNormal => {
                outputs.insert("normal".into(), PinType::Vec3);
            }
            ObjectMask => {
                outputs.insert("channel".into(), PinType::Scalar);
            }
            Time => {
                outputs.insert("seconds".into(), PinType::Scalar);
            }
            Parameter { name, value_type } => {
                validate_pin_name(name)?;
                outputs.insert("value".into(), *value_type);
            }
            TextureSample { slot } => {
                if !matches!(
                    slot.as_str(),
                    "BaseColor" | "NormalGL" | "Roughness" | "AO" | "Height" | "ARM"
                ) {
                    return Err(format!("Unknown Material texture slot {slot}"));
                }
                inputs.insert("uv".into(), PinType::Vec2);
                outputs.insert(
                    "value".into(),
                    match slot.as_str() {
                        "NormalGL" => PinType::Vec3,
                        "Roughness" | "AO" | "Height" => PinType::Scalar,
                        _ => PinType::Color,
                    },
                );
            }
            Add { value_type } | Multiply { value_type } => {
                inputs.insert("a".into(), *value_type);
                inputs.insert("b".into(), *value_type);
                outputs.insert("value".into(), *value_type);
            }
            Mix { value_type } => {
                inputs.insert("a".into(), *value_type);
                inputs.insert("b".into(), *value_type);
                inputs.insert("factor".into(), PinType::Scalar);
                outputs.insert("value".into(), *value_type);
            }
            LayerBlend => {
                inputs.insert("base".into(), PinType::Color);
                inputs.insert("layer".into(), PinType::Color);
                inputs.insert("weight".into(), PinType::Scalar);
                inputs.insert("height".into(), PinType::Scalar);
                outputs.insert("value".into(), PinType::Color);
            }
            WaterWave => {
                inputs.insert("uv".into(), PinType::Vec2);
                inputs.insert("time".into(), PinType::Scalar);
                inputs.insert("amplitude".into(), PinType::Scalar);
                outputs.insert("height".into(), PinType::Scalar);
            }
            CustomFunction {
                source,
                inputs: declared_inputs,
                outputs: declared_outputs,
            } => {
                if source.trim().is_empty() || source.len() > MAX_CUSTOM_SOURCE_BYTES {
                    return Err("Custom Material shader source must contain 1–8192 bytes".into());
                }
                if declared_inputs.len() > 16
                    || declared_outputs.is_empty()
                    || declared_outputs.len() > 16
                {
                    return Err("Custom Material function exceeds pin limits".into());
                }
                for pin in declared_inputs {
                    validate_pin_name(&pin.name)?;
                    if inputs.insert(pin.name.clone(), pin.value_type).is_some() {
                        return Err(format!("Duplicate input pin {}", pin.name));
                    }
                }
                for pin in declared_outputs {
                    validate_pin_name(&pin.name)?;
                    if outputs.insert(pin.name.clone(), pin.value_type).is_some() {
                        return Err(format!("Duplicate output pin {}", pin.name));
                    }
                }
            }
            SurfaceOutput => {
                inputs.insert("base_color".into(), PinType::Color);
                inputs.insert("roughness".into(), PinType::Scalar);
                inputs.insert("metallic".into(), PinType::Scalar);
                inputs.insert("normal".into(), PinType::Vec3);
                inputs.insert("ao".into(), PinType::Scalar);
                inputs.insert("opacity".into(), PinType::Scalar);
            }
            WaterOutput => {
                inputs.insert("color".into(), PinType::Color);
                inputs.insert("wave_height".into(), PinType::Scalar);
                inputs.insert("opacity".into(), PinType::Scalar);
            }
            SkyOutput | PostProcessOutput => {
                inputs.insert("color".into(), PinType::Color);
            }
        }
        Ok((inputs, outputs))
    }

    fn is_output(&self) -> bool {
        matches!(
            self,
            Self::SurfaceOutput | Self::WaterOutput | Self::SkyOutput | Self::PostProcessOutput
        )
    }
}

fn validate_values(values: &[f32]) -> Result<(), String> {
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1_000_000.0)
    {
        return Err("Graph constants must be finite and within supported bounds".into());
    }
    Ok(())
}

fn validate_pin_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 32
        || !name.as_bytes()[0].is_ascii_alphabetic()
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        return Err(format!("Invalid graph pin name {name:?}"));
    }
    Ok(())
}

impl MaterialGraph {
    pub fn validate(
        &self,
        output: &MaterialOutput,
        textures: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        if self.nodes.is_empty() || self.nodes.len() > MAX_NODES || self.links.len() > MAX_LINKS {
            return Err("Material graph exceeds node/link limits or is empty".into());
        }
        let mut nodes = BTreeMap::new();
        for node in &self.nodes {
            validate_values(&node.position)?;
            if nodes.insert(node.id, node).is_some() {
                return Err(format!("Duplicate graph node {}", node.id));
            }
            node.operation
                .pins()
                .map_err(|e| format!("Node {}: {e}", node.id))?;
            if let GraphOperation::TextureSample { slot } = &node.operation {
                if !textures.contains_key(slot) {
                    return Err(format!("Node {}: missing texture slot {slot}", node.id));
                }
            }
            if node.operation.is_output() && node.id != self.output_node {
                return Err("Material graph contains more than one output node".into());
            }
        }
        let root = nodes
            .get(&self.output_node)
            .ok_or("Missing Material graph output node")?;
        let matches_output = matches!(
            (&root.operation, output),
            (
                GraphOperation::SurfaceOutput,
                MaterialOutput::SurfacePbr { .. }
            ) | (GraphOperation::WaterOutput, MaterialOutput::Water(_))
                | (GraphOperation::SkyOutput, MaterialOutput::Sky(_))
                | (
                    GraphOperation::PostProcessOutput,
                    MaterialOutput::PostProcess
                )
        );
        if !matches_output {
            return Err("Material graph output does not match the Material asset type".into());
        }
        let mut occupied = BTreeSet::new();
        let mut edges: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let mut degrees: BTreeMap<u32, usize> = nodes.keys().map(|id| (*id, 0)).collect();
        for link in &self.links {
            let source = nodes
                .get(&link.source_node)
                .ok_or_else(|| format!("Missing graph source node {}", link.source_node))?;
            let target = nodes
                .get(&link.target_node)
                .ok_or_else(|| format!("Missing graph target node {}", link.target_node))?;
            let (_, source_pins) = source.operation.pins()?;
            let (target_pins, _) = target.operation.pins()?;
            let source_type = source_pins.get(&link.source_pin).ok_or_else(|| {
                format!(
                    "Node {} has no output pin {}",
                    link.source_node, link.source_pin
                )
            })?;
            let target_type = target_pins.get(&link.target_pin).ok_or_else(|| {
                format!(
                    "Node {} has no input pin {}",
                    link.target_node, link.target_pin
                )
            })?;
            if source_type != target_type {
                return Err(format!(
                    "Incompatible graph link: {:?} cannot feed {:?}",
                    source_type, target_type
                ));
            }
            if !occupied.insert((link.target_node, link.target_pin.as_str())) {
                return Err(format!(
                    "Input {} on node {} has multiple connections",
                    link.target_pin, link.target_node
                ));
            }
            edges
                .entry(link.source_node)
                .or_default()
                .push(link.target_node);
            *degrees.get_mut(&link.target_node).unwrap() += 1;
        }
        let mut queue: VecDeque<u32> = degrees
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut visited = 0;
        while let Some(id) = queue.pop_front() {
            visited += 1;
            if let Some(targets) = edges.get(&id) {
                for target in targets {
                    let degree = degrees.get_mut(target).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(*target);
                    }
                }
            }
        }
        if visited != nodes.len() {
            return Err("Material graph contains a cycle".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn surface() -> MaterialOutput {
        MaterialOutput::SurfacePbr {
            base_color: [1.0; 4],
            roughness: 0.5,
            metallic: 0.0,
        }
    }
    fn node(id: u32, operation: GraphOperation) -> GraphNode {
        GraphNode {
            id,
            operation,
            position: [0.0, 0.0],
        }
    }
    fn link(a: u32, out: &str, b: u32, input: &str) -> GraphLink {
        GraphLink {
            source_node: a,
            source_pin: out.into(),
            target_node: b,
            target_pin: input.into(),
        }
    }
    fn basic_graph() -> MaterialGraph {
        MaterialGraph {
            output_node: 3,
            nodes: vec![
                node(
                    1,
                    GraphOperation::Color {
                        value: [0.2, 0.5, 0.8, 1.0],
                    },
                ),
                node(2, GraphOperation::Scalar { value: 0.4 }),
                node(3, GraphOperation::SurfaceOutput),
            ],
            links: vec![
                link(1, "value", 3, "base_color"),
                link(2, "value", 3, "roughness"),
            ],
        }
    }
    #[test]
    fn graph_roundtrip_preserves_typed_links() {
        let graph = basic_graph();
        let restored: MaterialGraph =
            serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
        assert_eq!(restored, graph);
        assert!(graph.validate(&surface(), &BTreeMap::new()).is_ok());
    }
    #[test]
    fn graph_rejects_bad_links_and_ambiguous_inputs() {
        let mut graph = basic_graph();
        graph.links.push(link(2, "value", 3, "base_color"));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("Incompatible")
        );
        graph.links.pop();
        graph.links.push(link(1, "value", 3, "base_color"));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("multiple")
        );
        graph.links.pop();
        graph.links.push(link(99, "value", 3, "opacity"));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("Missing")
        );
    }
    #[test]
    fn graph_detects_cycles_and_duplicate_ids() {
        let mut graph = basic_graph();
        graph.nodes.insert(
            0,
            node(
                4,
                GraphOperation::Add {
                    value_type: PinType::Scalar,
                },
            ),
        );
        graph.links.push(link(2, "value", 4, "a"));
        graph.links.push(link(4, "value", 4, "b"));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("cycle")
        );
        graph.links.pop();
        graph
            .nodes
            .push(node(4, GraphOperation::Scalar { value: 1.0 }));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("Duplicate")
        );
    }
    #[test]
    fn graph_checks_type_constants_custom_code_and_texture_slots() {
        let mut graph = basic_graph();
        assert!(
            graph
                .validate(
                    &MaterialOutput::Water(crate::world::terrain::TerrainWaterSettings::default()),
                    &BTreeMap::new()
                )
                .is_err()
        );
        graph
            .nodes
            .push(node(7, GraphOperation::Scalar { value: f32::NAN }));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("finite")
        );
        graph.nodes.pop();
        graph.nodes.push(node(
            7,
            GraphOperation::TextureSample {
                slot: "BaseColor".into(),
            },
        ));
        assert!(
            graph
                .validate(&surface(), &BTreeMap::new())
                .unwrap_err()
                .contains("missing texture")
        );
        graph.nodes.pop();
        graph.nodes.push(node(
            7,
            GraphOperation::CustomFunction {
                source: "out_color = in_color;".into(),
                inputs: vec![CustomPin {
                    name: "in_color".into(),
                    value_type: PinType::Color,
                }],
                outputs: vec![CustomPin {
                    name: "out_color".into(),
                    value_type: PinType::Color,
                }],
            },
        ));
        assert!(graph.validate(&surface(), &BTreeMap::new()).is_ok());
    }
    #[test]
    fn graph_persists_and_instances_inherit_without_migrating_legacy_materials() {
        use crate::project::material::{
            MaterialCatalog, MaterialDefinition, MaterialDocument, file_path, read, save,
        };
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = Fixture(std::env::temp_dir().join(format!(
            "worldkiln-material-graph-{:032x}",
            rand::random::<u128>()
        )));
        std::fs::create_dir_all(&root.0).unwrap();
        let mut parent = MaterialDocument::new(MaterialDefinition::Asset {
            material: surface(),
        });
        parent.graph = Some(basic_graph());
        save(&root.0, "surface", &parent, None).unwrap();
        let child = MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent.id.clone(),
            overrides: BTreeMap::new(),
        });
        save(&root.0, "child", &child, None).unwrap();
        assert_eq!(read(&root.0, "surface").unwrap().graph, parent.graph);
        let catalog = MaterialCatalog::load(&root.0);
        assert_eq!(catalog.graph(&child.id).unwrap(), parent.graph.as_ref());

        let previous = std::fs::read(file_path(&root.0, "surface").unwrap()).unwrap();
        let mut invalid = parent.clone();
        invalid.graph.as_mut().unwrap().links.push(GraphLink {
            source_node: 2,
            source_pin: "value".into(),
            target_node: 3,
            target_pin: "base_color".into(),
        });
        assert!(save(&root.0, "surface", &invalid, Some(&previous)).is_err());
        assert_eq!(
            std::fs::read(file_path(&root.0, "surface").unwrap()).unwrap(),
            previous
        );

        let legacy = MaterialDocument::new(MaterialDefinition::Asset {
            material: surface(),
        });
        let json = serde_json::to_string(&legacy).unwrap();
        assert!(!json.contains("graph"));
        assert!(
            serde_json::from_str::<MaterialDocument>(&json)
                .unwrap()
                .graph
                .is_none()
        );
    }

    #[test]
    fn graph_draft_reuses_undo_and_conflict_checked_saves() {
        use crate::project::material::{MaterialDefinition, MaterialDocument};
        use crate::project::material_draft::MaterialDraft;
        let root = std::env::temp_dir().join(format!(
            "worldkiln-graph-draft-{:032x}",
            rand::random::<u128>()
        ));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let mut draft = MaterialDraft::create(
            &root,
            "custom",
            MaterialDefinition::Asset {
                material: surface(),
            },
        )
        .unwrap();
        draft.save(&root).unwrap();
        let mut invalid = basic_graph();
        invalid.links.push(link(2, "value", 3, "base_color"));
        assert!(draft.edit_graph(Some(invalid)).is_err());
        assert!(!draft.is_dirty());
        let valid = basic_graph();
        draft.edit_graph(Some(valid.clone())).unwrap();
        assert!(draft.is_dirty());
        assert!(draft.undo());
        assert_eq!(draft.document().graph, None);
        assert!(draft.redo());
        assert_eq!(draft.document().graph, Some(valid.clone()));
        let catalog = draft.save(&root).unwrap();
        assert!(!draft.is_dirty());
        assert_eq!(catalog.graph(&draft.document().id).unwrap(), Some(&valid));
        let mut instance = MaterialDraft::create(
            &root,
            "child",
            MaterialDefinition::Instance {
                parent: draft.document().id.clone(),
                overrides: BTreeMap::new(),
            },
        )
        .unwrap();
        assert!(instance.edit_graph(Some(valid)).is_err());
        assert!(
            MaterialDocument::new(instance.document().definition.clone())
                .graph
                .is_none()
        );
    }
}
