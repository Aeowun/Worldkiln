// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::super::{GRID_FRAGMENT_SHADER, GRID_VERTEX_SHADER, shader};
use super::{GraphPreviewProgram, PreviewDraw};
use crate::project::material::PinType;
use crate::project::material::graph_glsl::{GraphUniform, SurfaceShader};

pub(super) fn fragment_variant(compiled: &SurfaceShader) -> Result<String, String> {
    let entry = "void main() {\n    vec3 normal =";
    let graph_result = "    vec3 color =\n        v_color.rgb *\n        tex_color.rgb;";
    if !GRID_FRAGMENT_SHADER.contains(entry) || !GRID_FRAGMENT_SHADER.contains(graph_result) {
        return Err(
            "Existing OpenGL shader hook has changed; graph preview disabled safely".into(),
        );
    }
    let definitions = format!("uniform vec4 wk_default_base;\n{}\n", compiled.source);
    let with_function = GRID_FRAGMENT_SHADER.replacen(entry, &format!("{definitions}{entry}"), 1);
    let graph_call = format!(
        "    vec4 wk_surface_color;\n    float wk_surface_roughness;\n    float wk_surface_metallic;\n    wk_surface_graph(v_uv, wk_default_base, u_material_preview.x, u_material_preview.y, wk_surface_color, wk_surface_roughness, wk_surface_metallic);\n    tex_color = wk_surface_color;\n{graph_result}"
    );
    let hooked = with_function.replacen(graph_result, &graph_call, 1);
    let hooked = hooked
        .replacen(
            "float roughness = clamp(u_material_preview.x, 0.04, 1.0);",
            "float roughness = clamp(wk_surface_roughness, 0.04, 1.0);",
            1,
        )
        .replacen(
            "float metallic = clamp(u_material_preview.y, 0.0, 1.0);",
            "float metallic = clamp(wk_surface_metallic, 0.0, 1.0);",
            1,
        );
    if !hooked.contains("float roughness = clamp(wk_surface_roughness")
        || !hooked.contains("float metallic = clamp(wk_surface_metallic")
    {
        return Err("Existing OpenGL PBR hook has changed; graph preview disabled safely".into());
    }
    Ok(hooked)
}

const COMMON: [&str; 21] = [
    "u_material_preview",
    "u_view_projection",
    "u_model",
    "u_base_color",
    "u_alpha",
    "u_use_texture",
    "u_ambient_intensity",
    "u_global_light_enabled",
    "u_global_light_direction",
    "u_global_light_color",
    "u_global_light_intensity",
    "u_point_light_count",
    "u_shadows_enabled",
    "u_indirect_enabled",
    "u_debug_view",
    "u_terrain_blend_enabled",
    "u_terrain_layer_pass",
    "u_terrain_vertex_layers",
    "u_atmosphere_enabled",
    "u_underwater_enabled",
    "u_texture",
];

pub(super) fn compile_graph_variant(
    compiled: &SurfaceShader,
) -> Result<GraphPreviewProgram, String> {
    let source = fragment_variant(compiled)?;
    let program = shader::try_create_program(GRID_VERTEX_SHADER, &source)?;
    let locations = std::array::from_fn(|index| {
        super::super::resources::get_uniform_location(program, COMMON[index])
    });
    let uniform_locations = compiled
        .uniforms
        .iter()
        .map(|u| super::super::resources::get_uniform_location(program, &u.name))
        .collect();
    let default_base = super::super::resources::get_uniform_location(program, "wk_default_base");
    Ok(GraphPreviewProgram {
        program,
        locations,
        uniform_locations,
        default_base,
    })
}

impl PreviewDraw {
    pub(super) fn use_graph_program(
        &mut self,
        variant: &GraphPreviewProgram,
        uniforms: &[GraphUniform],
    ) {
        self.program = variant.program;
        self.material_preview_location = variant.locations[0];
        self.view_projection = variant.locations[1];
        self.model = variant.locations[2];
        self.base_color = variant.locations[3];
        self.alpha = variant.locations[4];
        self.use_texture_uniform = variant.locations[5];
        self.ambient = variant.locations[6];
        self.light_enabled = variant.locations[7];
        self.light_direction = variant.locations[8];
        self.light_color = variant.locations[9];
        self.light_intensity = variant.locations[10];
        self.point_light_count = variant.locations[11];
        self.shadow_enabled = variant.locations[12];
        self.indirect_enabled = variant.locations[13];
        self.debug_view = variant.locations[14];
        self.terrain_blend_enabled = variant.locations[15];
        self.terrain_layer_pass = variant.locations[16];
        self.terrain_vertex_layers = variant.locations[17];
        self.atmosphere_enabled = variant.locations[18];
        self.underwater_enabled = variant.locations[19];
        self.graph_texture_sampler = variant.locations[20];
        self.graph_default_base = variant.default_base;
        self.graph_uniforms = variant
            .uniform_locations
            .iter()
            .zip(uniforms.iter())
            .map(|(loc, u)| (*loc, u.kind, u.value))
            .collect();
        self.color = [1.0; 4];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::material::{GraphNode, GraphOperation, MaterialGraph, MaterialOutput};
    use std::collections::BTreeMap;
    #[test]
    fn variant_changes_fragment_color_and_pbr_without_replacing_scene_shader() {
        let graph = MaterialGraph {
            output_node: 1,
            nodes: vec![GraphNode {
                id: 1,
                operation: GraphOperation::SurfaceOutput,
                position: [0.0; 2],
            }],
            links: vec![],
        };
        let compiled = crate::project::material::graph_glsl::compile_surface(
            &graph,
            &MaterialOutput::SurfacePbr {
                base_color: [0.5; 4],
                roughness: 0.5,
                metallic: 0.2,
            },
            &BTreeMap::new(),
        )
        .unwrap();
        let generated = fragment_variant(&compiled).unwrap();
        assert!(generated.contains("tex_color = wk_surface_color"));
        assert!(generated.contains("clamp(wk_surface_roughness"));
        assert!(generated.contains("clamp(wk_surface_metallic"));
        assert!(generated.contains("u_terrain_blend_enabled"));
        assert!(!GRID_FRAGMENT_SHADER.contains("tex_color = wk_surface_color"));
    }
}
