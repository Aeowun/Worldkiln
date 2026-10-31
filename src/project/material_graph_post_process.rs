// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{GraphOperation, MaterialGraph, MaterialOutput, PinType};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct EffectUniform {
    pub shader_name: String,
    pub parameter_name: Option<String>,
    pub kind: PinType,
    pub value: [f32; 4],
}
#[derive(Clone, Debug)]
pub struct EffectShader {
    pub fragment: String,
    pub uniforms: Vec<EffectUniform>,
}

fn glsl_type(kind: PinType) -> &'static str {
    match kind {
        PinType::Scalar => "float",
        PinType::Vec2 => "vec2",
        PinType::Vec3 => "vec3",
        PinType::Color => "vec4",
    }
}
fn fallback(kind: PinType) -> &'static str {
    match kind {
        PinType::Scalar => "0.0",
        PinType::Vec2 => "vec2(0.0)",
        PinType::Vec3 => "vec3(0.0)",
        PinType::Color => "vec4(1.0)",
    }
}

struct Emitter<'a> {
    operations: BTreeMap<u32, &'a GraphOperation>,
    links: BTreeMap<(u32, String), u32>,
    emitted: BTreeSet<u32>,
    code: String,
    uniforms: Vec<EffectUniform>,
}
impl Emitter<'_> {
    fn input(&mut self, id: u32, name: &str, default: &str) -> Result<String, String> {
        if let Some(source) = self.links.get(&(id, name.to_owned())).copied() {
            self.emit(source)?;
            Ok(format!("wk_n{source}"))
        } else {
            Ok(default.to_owned())
        }
    }
    fn emit(&mut self, id: u32) -> Result<(), String> {
        if self.emitted.contains(&id) {
            return Ok(());
        }
        let op = *self.operations.get(&id).ok_or("Unknown graph node")?;
        let (kind, expression) = match op {
            GraphOperation::Scalar { value } => {
                self.constant(id, PinType::Scalar, [*value, 0.0, 0.0, 0.0]);
                (PinType::Scalar, format!("wk_u{id}"))
            }
            GraphOperation::Vec2 { value } => {
                self.constant(id, PinType::Vec2, [value[0], value[1], 0.0, 0.0]);
                (PinType::Vec2, format!("wk_u{id}"))
            }
            GraphOperation::Vec3 { value } => {
                self.constant(id, PinType::Vec3, [value[0], value[1], value[2], 0.0]);
                (PinType::Vec3, format!("wk_u{id}"))
            }
            GraphOperation::Color { value } => {
                self.constant(id, PinType::Color, *value);
                (PinType::Color, format!("wk_u{id}"))
            }
            GraphOperation::Parameter { name, value_type } => {
                self.uniforms.push(EffectUniform {
                    shader_name: format!("wk_u{id}"),
                    parameter_name: Some(name.clone()),
                    kind: *value_type,
                    value: [0.0; 4],
                });
                (*value_type, format!("wk_u{id}"))
            }
            GraphOperation::SceneColor => (PinType::Color, "texture(u_scene_color, v_uv)".into()),
            GraphOperation::SceneDepth => {
                (PinType::Scalar, "texture(u_scene_depth, v_uv).r".into())
            }
            GraphOperation::SceneNormal => (PinType::Vec3, "wk_reconstructed_normal(v_uv)".into()),
            GraphOperation::ObjectMask => (
                PinType::Scalar,
                "texture(u_scene_mask, v_uv).r * 255.0".into(),
            ),
            GraphOperation::Uv | GraphOperation::ScreenUv => (PinType::Vec2, "v_uv".into()),
            GraphOperation::Time => (PinType::Scalar, "u_time".into()),
            GraphOperation::TextureSample { .. } => {
                return Err("Post-process texture sample requires a scene input node".into());
            }
            GraphOperation::Add { value_type } | GraphOperation::Multiply { value_type } => {
                let a = self.input(id, "a", fallback(*value_type))?;
                let b = self.input(id, "b", fallback(*value_type))?;
                let operator = if matches!(op, GraphOperation::Add { .. }) {
                    "+"
                } else {
                    "*"
                };
                (*value_type, format!("({a} {operator} {b})"))
            }
            GraphOperation::Mix { value_type } => {
                let a = self.input(id, "a", fallback(*value_type))?;
                let b = self.input(id, "b", fallback(*value_type))?;
                let k = self.input(id, "factor", "0.5")?;
                (*value_type, format!("mix({a}, {b}, clamp({k}, 0.0, 1.0))"))
            }
            GraphOperation::LayerBlend => {
                let a = self.input(id, "base", "vec4(1.0)")?;
                let b = self.input(id, "layer", "vec4(1.0)")?;
                let w = self.input(id, "weight", "0.5")?;
                let h = self.input(id, "height", "0.5")?;
                (
                    PinType::Color,
                    format!("mix({a},{b},clamp({w}+({h}-0.5)*0.25,0.0,1.0))"),
                )
            }
            GraphOperation::WaterWave => {
                let uv = self.input(id, "uv", "v_uv")?;
                let t = self.input(id, "time", "u_time")?;
                let amp = self.input(id, "amplitude", "0.0")?;
                (
                    PinType::Scalar,
                    format!("sin(({uv}).x*8.0+({uv}).y*6.0+{t})*{amp}"),
                )
            }
            GraphOperation::CustomFunction {
                source,
                inputs,
                outputs,
            } => {
                let compiled = super::graph_function::compile(source, inputs, outputs, id)?;
                for pin in inputs {
                    let value = self.input(id, &pin.name, fallback(pin.value_type))?;
                    self.code.push_str(&format!(
                        "    {} wk_f{id}_{} = {value};\n",
                        glsl_type(pin.value_type),
                        pin.name
                    ));
                }
                (compiled.value_type, compiled.glsl)
            }
            GraphOperation::SurfaceOutput
            | GraphOperation::SkyOutput
            | GraphOperation::WaterOutput
            | GraphOperation::PostProcessOutput => {
                return Err("Output nodes cannot feed other nodes".into());
            }
        };
        self.code.push_str(&format!(
            "    {} wk_n{id} = {expression};\n",
            glsl_type(kind)
        ));
        self.emitted.insert(id);
        Ok(())
    }
    fn constant(&mut self, id: u32, kind: PinType, value: [f32; 4]) {
        self.uniforms.push(EffectUniform {
            shader_name: format!("wk_u{id}"),
            parameter_name: None,
            kind,
            value,
        });
    }
}
pub fn compile(graph: &MaterialGraph) -> Result<EffectShader, String> {
    graph.validate(&MaterialOutput::PostProcess, &BTreeMap::new())?;
    let mut emitter = Emitter {
        operations: graph.nodes.iter().map(|n| (n.id, &n.operation)).collect(),
        links: graph
            .links
            .iter()
            .map(|l| ((l.target_node, l.target_pin.clone()), l.source_node))
            .collect(),
        emitted: BTreeSet::new(),
        code: String::new(),
        uniforms: Vec::new(),
    };
    let color = emitter.input(graph.output_node, "color", "texture(u_scene_color, v_uv)")?;
    let declarations = emitter
        .uniforms
        .iter()
        .map(|u| format!("uniform {} {};\n", glsl_type(u.kind), u.shader_name))
        .collect::<String>();
    let fragment = format!(
        r#"#version 330 core
in vec2 v_uv;
out vec4 out_color;
uniform sampler2D u_scene_color;
uniform sampler2D u_scene_depth;
uniform sampler2D u_scene_normal;
uniform sampler2D u_scene_mask;
uniform mat4 u_inverse_projection;
uniform float u_time;
uniform float u_mask_channel;
uniform int u_mask_mode;
{declarations}
vec3 wk_view_position(vec2 uv) {{
    float z=texture(u_scene_depth,uv).r;
    vec4 p=u_inverse_projection*vec4(uv*2.0-1.0,z*2.0-1.0,1.0);
    return p.xyz/max(abs(p.w),0.000001);
}}
vec3 wk_reconstructed_normal(vec2 uv) {{
    float z=texture(u_scene_depth,uv).r;
    if(z>=0.999999) return vec3(0.0,0.0,1.0);
    vec2 d=1.0/vec2(textureSize(u_scene_depth,0));
    vec3 p=wk_view_position(uv);
    vec3 dx=wk_view_position(clamp(uv+vec2(d.x,0.0),0.0,1.0))-p;
    vec3 dy=wk_view_position(clamp(uv+vec2(0.0,d.y),0.0,1.0))-p;
    vec3 n=cross(dx,dy);
    if(dot(n,n)<0.0000001) return vec3(0.0,0.0,1.0);
    return normalize(n);
}}
void main() {{
{code}
    vec4 scene = texture(u_scene_color, v_uv);
    vec4 shaded = {color};
    float object_channel = floor(texture(u_scene_mask,v_uv).r*255.0+0.5);
    float selected = 1.0;
    if (u_mask_mode == 1) selected = 1.0 - step(0.5, abs(object_channel-u_mask_channel));
    if (u_mask_mode == 2) selected = step(0.5, abs(object_channel-u_mask_channel));
    out_color = mix(scene, shaded, selected);
}}
"#,
        code = emitter.code
    );
    if fragment.len() > 96 * 1024 {
        return Err("Post-process GLSL exceeds size limit".into());
    }
    Ok(EffectShader {
        fragment,
        uniforms: emitter.uniforms,
    })
}

#[cfg(test)]
mod tests {
    use super::super::{GraphLink, GraphNode};
    use super::*;
    #[test]
    fn object_mask_can_drive_an_authored_graph_mix() {
        let graph = MaterialGraph {
            output_node: 5,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::SceneColor,
                    position: [0.0; 2],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::Color {
                        value: [1.0, 0.0, 0.0, 1.0],
                    },
                    position: [0.0; 2],
                },
                GraphNode {
                    id: 3,
                    operation: GraphOperation::ObjectMask,
                    position: [0.0; 2],
                },
                GraphNode {
                    id: 4,
                    operation: GraphOperation::Mix {
                        value_type: PinType::Color,
                    },
                    position: [0.0; 2],
                },
                GraphNode {
                    id: 5,
                    operation: GraphOperation::PostProcessOutput,
                    position: [0.0; 2],
                },
            ],
            links: vec![
                GraphLink {
                    source_node: 1,
                    source_pin: "color".into(),
                    target_node: 4,
                    target_pin: "a".into(),
                },
                GraphLink {
                    source_node: 2,
                    source_pin: "value".into(),
                    target_node: 4,
                    target_pin: "b".into(),
                },
                GraphLink {
                    source_node: 3,
                    source_pin: "channel".into(),
                    target_node: 4,
                    target_pin: "factor".into(),
                },
                GraphLink {
                    source_node: 4,
                    source_pin: "value".into(),
                    target_node: 5,
                    target_pin: "color".into(),
                },
            ],
        };
        let shader = compile(&graph).unwrap();
        assert!(
            shader
                .fragment
                .contains("texture(u_scene_mask, v_uv).r * 255.0")
        );
        assert!(shader.fragment.contains("u_mask_mode"));
        assert!(shader.fragment.contains("mix("));
    }

    #[test]
    fn graph_emits_scene_sampling_and_uniforms_without_baking_values() {
        let graph = MaterialGraph {
            output_node: 3,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::SceneColor,
                    position: [0.0, 0.0],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::Color {
                        value: [0.3, 0.4, 0.5, 1.0],
                    },
                    position: [0.0, 0.0],
                },
                GraphNode {
                    id: 3,
                    operation: GraphOperation::PostProcessOutput,
                    position: [0.0, 0.0],
                },
            ],
            links: vec![GraphLink {
                source_node: 2,
                source_pin: "value".into(),
                target_node: 3,
                target_pin: "color".into(),
            }],
        };
        let shader = compile(&graph).unwrap();
        assert!(shader.fragment.contains("u_scene_color"));
        assert!(shader.fragment.contains("wk_n2"));
        assert!(!shader.fragment.contains("0.3,0.4,0.5"));
        assert_eq!(shader.uniforms[0].value, [0.3, 0.4, 0.5, 1.0]);
    }
}
