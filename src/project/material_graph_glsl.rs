// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{GraphOperation, MaterialGraph, MaterialOutput, PinType};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub struct GraphUniform {
    pub name: String,
    pub kind: PinType,
    pub value: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceShader {
    pub source: String,
    pub uniforms: Vec<GraphUniform>,
}

struct Emitter<'a> {
    graph: &'a MaterialGraph,
    nodes: BTreeMap<u32, &'a GraphOperation>,
    incoming: BTreeMap<(u32, &'a str), u32>,
    emitted: BTreeSet<u32>,
    lines: Vec<String>,
    uniforms: Vec<GraphUniform>,
}

impl Emitter<'_> {
    fn pin(&mut self, target: u32, pin: &str, default: &str) -> Result<String, String> {
        let incoming = self.incoming.get(&(target, pin)).copied();
        match incoming {
            Some(source_node) => {
                self.emit(source_node)?;
                Ok(format!("wk_n{source_node}"))
            }
            None => Ok(default.to_owned()),
        }
    }

    fn emit(&mut self, id: u32) -> Result<(), String> {
        if self.emitted.contains(&id) {
            return Ok(());
        }
        let operation = *self
            .nodes
            .get(&id)
            .ok_or_else(|| format!("Missing graph node {id}"))?;
        let (ty, expression) = match operation {
            GraphOperation::Scalar { value } => {
                self.uniforms.push(GraphUniform {
                    name: format!("wk_u{id}"),
                    kind: PinType::Scalar,
                    value: [*value, 0.0, 0.0, 0.0],
                });
                ("float", format!("wk_u{id}"))
            }
            GraphOperation::Vec2 { value } => {
                self.uniforms.push(GraphUniform {
                    name: format!("wk_u{id}"),
                    kind: PinType::Vec2,
                    value: [value[0], value[1], 0.0, 0.0],
                });
                ("vec2", format!("wk_u{id}"))
            }
            GraphOperation::Vec3 { value } => {
                self.uniforms.push(GraphUniform {
                    name: format!("wk_u{id}"),
                    kind: PinType::Vec3,
                    value: [value[0], value[1], value[2], 0.0],
                });
                ("vec3", format!("wk_u{id}"))
            }
            GraphOperation::Color { value } => {
                self.uniforms.push(GraphUniform {
                    name: format!("wk_u{id}"),
                    kind: PinType::Color,
                    value: *value,
                });
                ("vec4", format!("wk_u{id}"))
            }
            GraphOperation::Uv => ("vec2", "uv".into()),
            GraphOperation::SceneColor
            | GraphOperation::SceneDepth
            | GraphOperation::SceneNormal
            | GraphOperation::ObjectMask
            | GraphOperation::ScreenUv
            | GraphOperation::Time
            | GraphOperation::Parameter { .. } => {
                return Err("Scene post-process nodes cannot run in Surface Materials".into());
            }
            GraphOperation::TextureSample { slot } => {
                if slot != "BaseColor" {
                    return Err(format!(
                        "Node {id}: OpenGL preview does not support sampling {slot} yet"
                    ));
                }
                let uv = self.pin(id, "uv", "uv")?;
                ("vec4", format!("texture(u_texture, {uv})"))
            }
            GraphOperation::Add { value_type } | GraphOperation::Multiply { value_type } => {
                let a = self.pin(id, "a", default_for(*value_type))?;
                let b = self.pin(id, "b", default_for(*value_type))?;
                let op = if matches!(operation, GraphOperation::Add { .. }) {
                    "+"
                } else {
                    "*"
                };
                (glsl_type(*value_type), format!("({a} {op} {b})"))
            }
            GraphOperation::Mix { value_type } => {
                let a = self.pin(id, "a", default_for(*value_type))?;
                let b = self.pin(id, "b", default_for(*value_type))?;
                let factor = self.pin(id, "factor", "0.5")?;
                (
                    glsl_type(*value_type),
                    format!("mix({a}, {b}, clamp({factor}, 0.0, 1.0))"),
                )
            }
            GraphOperation::LayerBlend => {
                let base = self.pin(id, "base", "vec4(1.0)")?;
                let layer = self.pin(id, "layer", "vec4(1.0)")?;
                let weight = self.pin(id, "weight", "0.5")?;
                let height = self.pin(id, "height", "0.5")?;
                (
                    "vec4",
                    format!(
                        "mix({base}, {layer}, clamp({weight} + ({height} - 0.5) * 0.25, 0.0, 1.0))"
                    ),
                )
            }
            GraphOperation::WaterWave => {
                let uv = self.pin(id, "uv", "uv")?;
                let time = self.pin(id, "time", "0.0")?;
                let amplitude = self.pin(id, "amplitude", "0.0")?;
                (
                    "float",
                    format!("sin(({uv}).x * 8.0 + ({uv}).y * 6.0 + {time}) * {amplitude}"),
                )
            }
            GraphOperation::CustomFunction {
                source,
                inputs,
                outputs,
            } => {
                let compiled = super::graph_function::compile(source, inputs, outputs, id)
                    .map_err(|error| format!("Node {id}: {error}"))?;
                for pin in inputs {
                    let expression = self.pin(id, &pin.name, default_for(pin.value_type))?;
                    self.lines.push(format!(
                        "    {} wk_f{id}_{} = {expression};",
                        glsl_type(pin.value_type),
                        pin.name
                    ));
                }
                (glsl_type(compiled.value_type), compiled.glsl)
            }
            GraphOperation::SurfaceOutput
            | GraphOperation::WaterOutput
            | GraphOperation::SkyOutput
            | GraphOperation::PostProcessOutput => {
                return Err(format!("Node {id}: output nodes cannot feed another node"));
            }
        };
        self.lines
            .push(format!("    {ty} wk_n{id} = {expression};"));
        self.emitted.insert(id);
        Ok(())
    }
}

fn glsl_type(kind: PinType) -> &'static str {
    match kind {
        PinType::Scalar => "float",
        PinType::Vec2 => "vec2",
        PinType::Vec3 => "vec3",
        PinType::Color => "vec4",
    }
}
fn default_for(kind: PinType) -> &'static str {
    match kind {
        PinType::Scalar => "0.0",
        PinType::Vec2 => "vec2(0.0)",
        PinType::Vec3 => "vec3(0.0)",
        PinType::Color => "vec4(1.0)",
    }
}

pub fn compile_surface(
    graph: &MaterialGraph,
    material: &MaterialOutput,
    textures: &BTreeMap<String, String>,
) -> Result<SurfaceShader, String> {
    graph.validate(material, textures)?;
    if !matches!(material, MaterialOutput::SurfacePbr { .. }) {
        return Err("OpenGL graph compiler currently supports Surface PBR only".into());
    }
    for link in &graph.links {
        if link.target_node == graph.output_node
            && matches!(link.target_pin.as_str(), "normal" | "ao" | "opacity")
        {
            return Err(format!(
                "Surface output {} is not wired to the OpenGL material shader yet",
                link.target_pin
            ));
        }
    }
    let nodes: BTreeMap<u32, &GraphOperation> = graph
        .nodes
        .iter()
        .map(|node| (node.id, &node.operation))
        .collect();
    let mut emit = Emitter {
        graph,
        nodes,
        incoming: graph
            .links
            .iter()
            .map(|link| {
                (
                    (link.target_node, link.target_pin.as_str()),
                    link.source_node,
                )
            })
            .collect(),
        emitted: BTreeSet::new(),
        lines: Vec::new(),
        uniforms: Vec::new(),
    };
    let color = emit.pin(graph.output_node, "base_color", "wk_default_color")?;
    let rough = emit.pin(graph.output_node, "roughness", "wk_default_roughness")?;
    let metal = emit.pin(graph.output_node, "metallic", "wk_default_metallic")?;
    let mut declarations = String::new();
    for uniform in &emit.uniforms {
        declarations.push_str(&format!(
            "uniform {} {};\n",
            glsl_type(uniform.kind),
            uniform.name
        ));
    }
    let body = emit.lines.join("\n");
    let source = format!(
        "{declarations}\nvoid wk_surface_graph(vec2 uv, vec4 wk_default_color, float wk_default_roughness, float wk_default_metallic, out vec4 wk_color, out float wk_roughness, out float wk_metallic) {{\n{body}\n    wk_color = {color};\n    wk_roughness = {rough};\n    wk_metallic = {metal};\n}}\n"
    );
    if source.len() > 96 * 1024 {
        return Err("Generated Material GLSL exceeds 96 KiB".into());
    }
    Ok(SurfaceShader {
        source,
        uniforms: emit.uniforms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::material::{GraphLink, GraphNode};
    fn n(id: u32, operation: GraphOperation) -> GraphNode {
        GraphNode {
            id,
            operation,
            position: [0.0; 2],
        }
    }
    fn mat() -> MaterialOutput {
        MaterialOutput::SurfacePbr {
            base_color: [0.8; 4],
            roughness: 0.4,
            metallic: 0.1,
        }
    }
    #[test]
    fn constants_generate_uniforms_without_baking_values() {
        let mut graph = MaterialGraph {
            output_node: 2,
            nodes: vec![
                n(
                    1,
                    GraphOperation::Color {
                        value: [0.1, 0.3, 0.6, 1.0],
                    },
                ),
                n(2, GraphOperation::SurfaceOutput),
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 2,
                target_pin: "base_color".into(),
            }],
        };
        let compiled = compile_surface(&graph, &mat(), &BTreeMap::new()).unwrap();
        assert!(compiled.source.contains("uniform vec4 wk_u1"));
        assert!(!compiled.source.contains("0.3,0.6"));
        assert_eq!(compiled.uniforms[0].value, [0.1, 0.3, 0.6, 1.0]);
        graph.nodes[0].operation = GraphOperation::Color {
            value: [0.4, 0.5, 0.6, 1.0],
        };
        let edited = compile_surface(&graph, &mat(), &BTreeMap::new()).unwrap();
        assert_eq!(edited.source, compiled.source);
        assert_ne!(edited.uniforms, compiled.uniforms);
    }
    #[test]
    fn connected_operations_compile_only_reachable_nodes() {
        let graph = MaterialGraph {
            output_node: 4,
            nodes: vec![
                n(1, GraphOperation::Scalar { value: 0.2 }),
                n(2, GraphOperation::Scalar { value: 0.8 }),
                n(
                    3,
                    GraphOperation::Add {
                        value_type: PinType::Scalar,
                    },
                ),
                n(4, GraphOperation::SurfaceOutput),
                n(5, GraphOperation::WaterWave),
            ],
            links: vec![
                GraphLink {
                    source_node: 1,
                    source_pin: "value".into(),
                    target_node: 3,
                    target_pin: "a".into(),
                },
                GraphLink {
                    source_node: 2,
                    source_pin: "value".into(),
                    target_node: 3,
                    target_pin: "b".into(),
                },
                GraphLink {
                    source_node: 3,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "metallic".into(),
                },
            ],
        };
        let compiled = compile_surface(&graph, &mat(), &BTreeMap::new()).unwrap();
        assert!(compiled.source.contains("wk_n3 = (wk_n1 + wk_n2)"));
        assert!(!compiled.source.contains("wk_n5"));
    }
    #[test]
    fn unsafe_custom_source_never_reaches_generated_glsl() {
        let graph = MaterialGraph {
            output_node: 2,
            nodes: vec![
                n(
                    1,
                    GraphOperation::CustomFunction {
                        source: "discard;".into(),
                        inputs: vec![],
                        outputs: vec![crate::project::material::CustomPin {
                            name: "value".into(),
                            value_type: PinType::Scalar,
                        }],
                    },
                ),
                n(2, GraphOperation::SurfaceOutput),
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 2,
                target_pin: "metallic".into(),
            }],
        };
        let error = compile_surface(&graph, &mat(), &BTreeMap::new()).unwrap_err();
        assert!(error.contains("Custom Function must assign"));
        assert!(!error.contains("discard;"));
    }
}
