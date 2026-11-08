// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{GraphOperation, MaterialGraph, PinType};
use crate::world::terrain::TerrainWaterSettings;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
struct Value {
    kind: PinType,
    data: [f32; 4],
}
impl Value {
    fn scalar(value: f32) -> Self {
        Self {
            kind: PinType::Scalar,
            data: [value, 0.0, 0.0, 0.0],
        }
    }
    fn color(value: [f32; 4]) -> Self {
        Self {
            kind: PinType::Color,
            data: value,
        }
    }
    fn default(kind: PinType) -> Self {
        match kind {
            PinType::Color => Self::color([1.0; 4]),
            PinType::Scalar => Self::scalar(0.0),
            other => Self {
                kind: other,
                data: [0.0; 4],
            },
        }
    }
    fn dimension(&self) -> usize {
        match self.kind {
            PinType::Scalar => 1,
            PinType::Vec2 => 2,
            PinType::Vec3 => 3,
            PinType::Color => 4,
        }
    }
}
struct Interpreter<'a> {
    graph: &'a MaterialGraph,
    nodes: BTreeMap<u32, &'a GraphOperation>,
    values: BTreeMap<u32, Value>,
    busy: BTreeSet<u32>,
}
impl Interpreter<'_> {
    fn input(&mut self, node: u32, pin: &str, kind: PinType) -> Result<Option<Value>, String> {
        let link = self
            .graph
            .links
            .iter()
            .find(|link| link.target_node == node && link.target_pin == pin);
        if let Some(link) = link {
            let result = self.node(link.source_node)?;
            if result.kind != kind {
                return Err(format!("Node {node}.{pin} expected {kind:?}"));
            }
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
    fn value_or(&mut self, node: u32, pin: &str, default: Value) -> Result<Value, String> {
        Ok(self.input(node, pin, default.kind)?.unwrap_or(default))
    }
    fn node(&mut self, id: u32) -> Result<Value, String> {
        if let Some(value) = self.values.get(&id) {
            return Ok(*value);
        }
        if !self.busy.insert(id) {
            return Err("Material graph contains a cycle".into());
        }
        let result = (|| -> Result<Value, String> {
            let operation = *self
                .nodes
                .get(&id)
                .ok_or_else(|| format!("Missing Material node {id}"))?;
            match operation {
                GraphOperation::Scalar { value } => Ok(Value::scalar(*value)),
                GraphOperation::Vec2 { value } => Ok(Value {
                    kind: PinType::Vec2,
                    data: [value[0], value[1], 0.0, 0.0],
                }),
                GraphOperation::Vec3 { value } => Ok(Value {
                    kind: PinType::Vec3,
                    data: [value[0], value[1], value[2], 0.0],
                }),
                GraphOperation::Color { value } => Ok(Value::color(*value)),
                GraphOperation::Add { value_type } | GraphOperation::Multiply { value_type } => {
                    let a = self.value_or(id, "a", Value::default(*value_type))?;
                    let b = self.value_or(id, "b", Value::default(*value_type))?;
                    let mut data = [0.0; 4];
                    for i in 0..a.dimension() {
                        data[i] = if matches!(operation, GraphOperation::Add { .. }) {
                            a.data[i] + b.data[i]
                        } else {
                            a.data[i] * b.data[i]
                        };
                    }
                    Ok(Value {
                        kind: *value_type,
                        data,
                    })
                }
                GraphOperation::Mix { value_type } => {
                    let a = self.value_or(id, "a", Value::default(*value_type))?;
                    let b = self.value_or(id, "b", Value::default(*value_type))?;
                    let t =
                        self.value_or(id, "factor", Value::scalar(0.5))?.data[0].clamp(0.0, 1.0);
                    let mut data = [0.0; 4];
                    for i in 0..a.dimension() {
                        data[i] = a.data[i] * (1.0 - t) + b.data[i] * t;
                    }
                    Ok(Value {
                        kind: *value_type,
                        data,
                    })
                }
                GraphOperation::LayerBlend => {
                    let a = self.value_or(id, "base", Value::color([1.0; 4]))?;
                    let b = self.value_or(id, "layer", Value::color([1.0; 4]))?;
                    let weight = self.value_or(id, "weight", Value::scalar(0.5))?.data[0];
                    let height = self.value_or(id, "height", Value::scalar(0.5))?.data[0];
                    let t = (weight + (height - 0.5) * 0.25).clamp(0.0, 1.0);
                    Ok(Value::color(std::array::from_fn(|i| {
                        a.data[i] * (1.0 - t) + b.data[i] * t
                    })))
                }
                GraphOperation::Uv
                | GraphOperation::SceneColor
                | GraphOperation::SceneDepth
                | GraphOperation::SceneNormal
                | GraphOperation::ObjectMask
                | GraphOperation::ScreenUv
                | GraphOperation::Time
                | GraphOperation::Parameter { .. }
                | GraphOperation::TextureSample { .. }
                | GraphOperation::WaterWave
                | GraphOperation::CustomFunction { .. } => Err(format!(
                    "Node {id} needs per-pixel GPU Water rendering; uniform-only Water graph output cannot run this node"
                )),
                GraphOperation::SurfaceOutput
                | GraphOperation::WaterOutput
                | GraphOperation::SkyOutput
                | GraphOperation::PostProcessOutput => {
                    Err("Output node cannot provide an input value".into())
                }
            }
        })();
        self.busy.remove(&id);
        let v = result?;
        if v.data
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        {
            return Err(format!("Node {id} overflows Material limits"));
        }
        self.values.insert(id, v);
        Ok(v)
    }
}
pub fn evaluate_water(
    graph: &MaterialGraph,
    base: TerrainWaterSettings,
    textures: &BTreeMap<String, String>,
) -> Result<TerrainWaterSettings, String> {
    graph.validate(&super::MaterialOutput::Water(base), textures)?;
    let mut run = Interpreter {
        graph,
        nodes: graph.nodes.iter().map(|n| (n.id, &n.operation)).collect(),
        values: BTreeMap::new(),
        busy: BTreeSet::new(),
    };
    let out = graph.output_node;
    let mut settings = base;
    let color = run.input(out, "color", PinType::Color)?;
    let wave = run.input(out, "wave_height", PinType::Scalar)?;
    let opacity = run.input(out, "opacity", PinType::Scalar)?;
    if let Some(value) = color {
        let rgb = [
            value.data[0].clamp(0.0, 1.0),
            value.data[1].clamp(0.0, 1.0),
            value.data[2].clamp(0.0, 1.0),
        ];
        settings.shallow_color = rgb;
        settings.deep_color = rgb;
    }
    if let Some(value) = wave {
        settings.wave_height = value.data[0].clamp(0.0, 10.0);
    }
    if let Some(value) = opacity {
        settings.opacity = value.data[0].clamp(0.0, 1.0);
    }
    settings.validate().map_err(|e| e.to_string())?;
    Ok(settings)
}

pub fn evaluate_surface_pbr(
    graph: &MaterialGraph,
    base: &super::MaterialOutput,
    textures: &BTreeMap<String, String>,
) -> Result<[f32; 2], String> {
    graph.validate(base, textures)?;
    let super::MaterialOutput::SurfacePbr {
        roughness,
        metallic,
        ..
    } = base
    else {
        return Err("Surface graph requires a Surface PBR Material".into());
    };
    let mut run = Interpreter {
        graph,
        nodes: graph.nodes.iter().map(|n| (n.id, &n.operation)).collect(),
        values: BTreeMap::new(),
        busy: BTreeSet::new(),
    };
    let rough = run
        .input(graph.output_node, "roughness", PinType::Scalar)?
        .map(|v| v.data[0])
        .unwrap_or(*roughness);
    let metal = run
        .input(graph.output_node, "metallic", PinType::Scalar)?
        .map(|v| v.data[0])
        .unwrap_or(*metallic);
    Ok([rough.clamp(0.0, 1.0), metal.clamp(0.0, 1.0)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::material::{GraphLink, GraphNode, MaterialOutput};
    fn n(id: u32, operation: GraphOperation) -> GraphNode {
        GraphNode {
            id,
            operation,
            position: [0.0; 2],
        }
    }
    #[test]
    fn water_uniform_graph_updates_renderable_water_colors_and_opacity() {
        let graph = MaterialGraph {
            output_node: 4,
            nodes: vec![
                n(
                    1,
                    GraphOperation::Color {
                        value: [0.0, 0.7, 0.4, 1.0],
                    },
                ),
                n(2, GraphOperation::Scalar { value: 0.65 }),
                n(3, GraphOperation::Scalar { value: 0.8 }),
                n(4, GraphOperation::WaterOutput),
            ],
            links: vec![
                GraphLink {
                    source_node: 1,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "color".into(),
                },
                GraphLink {
                    source_node: 2,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "opacity".into(),
                },
                GraphLink {
                    source_node: 3,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "wave_height".into(),
                },
            ],
        };
        let base = TerrainWaterSettings::default();
        graph
            .validate(&MaterialOutput::Water(base), &BTreeMap::new())
            .unwrap();
        let resolved = evaluate_water(&graph, base, &BTreeMap::new()).unwrap();
        assert_eq!(resolved.shallow_color, [0.0, 0.7, 0.4]);
        assert_eq!(resolved.deep_color, [0.0, 0.7, 0.4]);
        assert_eq!(resolved.opacity, 0.65);
        assert_eq!(resolved.wave_height, 0.8);
        assert_eq!(resolved.foam_color, base.foam_color);
    }

    #[test]
    fn surface_pbr_graph_reaches_cached_world_shader_parameters() {
        let graph = MaterialGraph {
            output_node: 3,
            nodes: vec![
                n(1, GraphOperation::Scalar { value: 0.18 }),
                n(2, GraphOperation::Scalar { value: 0.83 }),
                n(3, GraphOperation::SurfaceOutput),
            ],
            links: vec![
                GraphLink {
                    source_node: 1,
                    source_pin: "value".into(),
                    target_node: 3,
                    target_pin: "roughness".into(),
                },
                GraphLink {
                    source_node: 2,
                    source_pin: "value".into(),
                    target_node: 3,
                    target_pin: "metallic".into(),
                },
            ],
        };
        let mut asset = crate::project::material::MaterialDocument::new(
            crate::project::material::MaterialDefinition::Asset {
                material: MaterialOutput::SurfacePbr {
                    base_color: [0.5, 0.5, 0.5, 1.0],
                    roughness: 0.5,
                    metallic: 0.0,
                },
            },
        );
        let id = asset.id.clone();
        asset.graph = Some(graph);
        let catalog =
            crate::project::material::MaterialCatalog::from_documents(BTreeMap::from([(
                "PBR.MAT".to_owned(),
                asset,
            )]));
        assert!(catalog.errors().is_empty(), "{:?}", catalog.errors());
        assert_eq!(catalog.surface_pbr(&id), Some([0.18, 0.83]));
    }

    #[test]
    fn material_catalog_applies_water_graph_to_instances_once() {
        use crate::project::material::{MaterialCatalog, MaterialDefinition, MaterialDocument};
        let mut asset = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(TerrainWaterSettings::default()),
        });
        let parent_id = asset.id.clone();
        asset.graph = Some(MaterialGraph {
            output_node: 2,
            nodes: vec![
                n(1, GraphOperation::Scalar { value: 0.37 }),
                n(2, GraphOperation::WaterOutput),
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 2,
                target_pin: "opacity".into(),
            }],
        });
        let instance = MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent_id.clone(),
            overrides: BTreeMap::new(),
        });
        let instance_id = instance.id.clone();
        let catalog = MaterialCatalog::from_documents(BTreeMap::from([
            ("ocean.MAT".into(), asset),
            ("ocean_child.MAT".into(), instance),
        ]));
        assert!(catalog.errors().is_empty(), "{:?}", catalog.errors());
        assert_eq!(catalog.water(&parent_id).unwrap().opacity, 0.37);
        assert_eq!(catalog.water(&instance_id).unwrap().opacity, 0.37);
        assert_eq!(catalog.graph(&instance_id).unwrap().unwrap().nodes.len(), 2);
    }

    #[test]
    fn dynamic_water_graph_fails_without_lying_about_shader_support() {
        let graph = MaterialGraph {
            output_node: 2,
            nodes: vec![
                n(1, GraphOperation::WaterWave),
                n(2, GraphOperation::WaterOutput),
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "height".into(),
                target_node: 2,
                target_pin: "wave_height".into(),
            }],
        };
        let base = TerrainWaterSettings::default();
        assert!(
            evaluate_water(&graph, base, &BTreeMap::new())
                .unwrap_err()
                .contains("per-pixel GPU")
        );
    }
}
