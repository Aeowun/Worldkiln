// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{Renderer, shader};
use crate::project::material::{MaterialGraph, MaterialOutput, PinType, graph_glsl};
use std::collections::BTreeMap;

const SIZE: i32 = 256;

fn blend_channel(
    dst: f32,
    input: f32,
    alpha: f32,
    blend: &crate::project::material::MaterialLayerBlend,
) -> f32 {
    use crate::project::material::MaterialLayerBlend;
    let mixed = match blend {
        MaterialLayerBlend::Normal => input,
        MaterialLayerBlend::Multiply => dst * input,
        MaterialLayerBlend::Add => (dst + input).min(1.0),
        MaterialLayerBlend::Screen => 1.0 - (1.0 - dst) * (1.0 - input),
    };
    dst * (1.0 - alpha) + mixed * alpha
}
const VERTEX: &str = r#"#version 330 core
out vec2 uv;
void main() {
    vec2 p = vec2(gl_VertexID == 1 ? 3.0 : -1.0, gl_VertexID == 2 ? 3.0 : -1.0);
    uv = p * 0.5 + 0.5;
    gl_Position = vec4(p, 0.0, 1.0);
}
"#;

fn fragment_shader(compiled: &graph_glsl::SurfaceShader) -> String {
    format!(
        r#"#version 330 core
in vec2 uv;
out vec4 FragColor;
uniform sampler2D u_texture;
uniform vec4 wk_default_color;
uniform float wk_default_roughness;
uniform float wk_default_metallic;
{}
void main() {{
    vec4 color;
    float roughness;
    float metallic;
    wk_surface_graph(uv, wk_default_color, wk_default_roughness, wk_default_metallic, color, roughness, metallic);
    FragColor = clamp(color, 0.0, 1.0);
}}
"#,
        compiled.source
    )
}

#[derive(Clone, Copy)]
struct GlRestore {
    program: i32,
    vao: i32,
    draw_fbo: i32,
    read_fbo: i32,
    viewport: [i32; 4],
    active_texture: i32,
    texture_zero: i32,
    depth: bool,
    blend: bool,
    cull: bool,
    scissor: bool,
    depth_mask: u8,
    color_mask: [u8; 4],
}

impl GlRestore {
    unsafe fn capture() -> Self {
        let mut state = Self {
            program: 0,
            vao: 0,
            draw_fbo: 0,
            read_fbo: 0,
            viewport: [0; 4],
            active_texture: 0,
            texture_zero: 0,
            depth: gl::IsEnabled(gl::DEPTH_TEST) == gl::TRUE,
            blend: gl::IsEnabled(gl::BLEND) == gl::TRUE,
            cull: gl::IsEnabled(gl::CULL_FACE) == gl::TRUE,
            scissor: gl::IsEnabled(gl::SCISSOR_TEST) == gl::TRUE,
            depth_mask: 0,
            color_mask: [0; 4],
        };
        gl::GetIntegerv(gl::CURRENT_PROGRAM, &mut state.program);
        gl::GetIntegerv(gl::VERTEX_ARRAY_BINDING, &mut state.vao);
        gl::GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut state.draw_fbo);
        gl::GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut state.read_fbo);
        gl::GetIntegerv(gl::VIEWPORT, state.viewport.as_mut_ptr());
        gl::GetIntegerv(gl::ACTIVE_TEXTURE, &mut state.active_texture);
        gl::GetBooleanv(gl::DEPTH_WRITEMASK, &mut state.depth_mask);
        gl::GetBooleanv(gl::COLOR_WRITEMASK, state.color_mask.as_mut_ptr());
        gl::ActiveTexture(gl::TEXTURE0);
        gl::GetIntegerv(gl::TEXTURE_BINDING_2D, &mut state.texture_zero);
        state
    }

    unsafe fn restore(&self) {
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.draw_fbo as u32);
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, self.read_fbo as u32);
        gl::Viewport(
            self.viewport[0],
            self.viewport[1],
            self.viewport[2],
            self.viewport[3],
        );
        gl::UseProgram(self.program as u32);
        gl::BindVertexArray(self.vao as u32);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::BindTexture(gl::TEXTURE_2D, self.texture_zero as u32);
        gl::ActiveTexture(self.active_texture as u32);
        gl::DepthMask(self.depth_mask);
        gl::ColorMask(
            self.color_mask[0],
            self.color_mask[1],
            self.color_mask[2],
            self.color_mask[3],
        );
        for (enabled, cap) in [
            (self.depth, gl::DEPTH_TEST),
            (self.blend, gl::BLEND),
            (self.cull, gl::CULL_FACE),
            (self.scissor, gl::SCISSOR_TEST),
        ] {
            if enabled {
                gl::Enable(cap);
            } else {
                gl::Disable(cap);
            }
        }
    }
}

fn preview_layer_key(
    draft: &crate::project::material::MaterialDocument,
    color: [f32; 4],
) -> Option<String> {
    use std::hash::{Hash, Hasher};
    if draft.layers.is_empty()
        || !matches!(
            draft.definition,
            crate::project::material::MaterialDefinition::Asset { .. }
        )
    {
        return None;
    }
    let maps: Vec<_> = draft
        .textures
        .iter()
        .filter(|(slot, _)| color[3] != 0.0 || slot.as_str() != "BaseColor")
        .collect();
    let json = serde_json::to_vec(&(&draft.id, &draft.layers, &maps, &color)).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    Some(format!(
        "MaterialPreview:{}:{:016x}",
        draft.id,
        hasher.finish()
    ))
}

impl Renderer {
    pub(super) fn bind_cell_pbr(
        &self,
        texture_id: &str,
        catalog: &crate::project::material::MaterialCatalog,
    ) {
        let params = texture_id
            .strip_prefix("Material:")
            .and_then(|id| catalog.surface_pbr(id));
        unsafe {
            if let Some([roughness, metallic]) = params {
                gl::Uniform3f(self.grid_cell_pbr_location, roughness, metallic, 1.0);
            } else {
                gl::Uniform3f(self.grid_cell_pbr_location, 0.0, 0.0, 0.0);
            }
        }
    }

    pub(super) fn clear_baked_graph_textures(&self) {
        let mut textures = self.graph_baked_textures.borrow_mut();
        if gl::DeleteTextures::is_loaded() {
            for texture in textures.values() {
                if let Ok(texture) = texture {
                    if *texture != 0 {
                        unsafe {
                            gl::DeleteTextures(1, texture);
                        }
                    }
                }
            }
        }
        textures.clear();
        self.layer_source_images.borrow_mut().clear();
        self.textures
            .borrow_mut()
            .retain(|id, _| !id.starts_with("Material:"));
    }

    pub(crate) fn material_overlay_texture(&self, id: &str) -> Option<u32> {
        use crate::project::material::{MaterialCatalog, MaterialDefinition};
        let key = format!("MaterialOverlay:{id}");
        if let Some(existing) = self.graph_baked_textures.borrow().get(&key) {
            return existing.as_ref().ok().copied();
        }
        let catalog = self.material_snapshot();
        let layers = catalog.layers(id).ok()?;
        if layers.is_empty() {
            return None;
        }
        let project = catalog.project()?;
        let mut documents = catalog.documents().clone();
        let document_name = documents.iter().find(|(_, doc)| doc.id == id)?.0.clone();
        let doc = documents.get_mut(&document_name)?;
        doc.definition = MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.0; 4],
                roughness: 0.5,
                metallic: 0.0,
            },
        };
        doc.graph = None;
        doc.layers = layers;
        doc.textures.remove("BaseColor");
        let overlay = MaterialCatalog::preview(project, documents);
        let result = self.layered_material_texture_with_key(id, &overlay, key);
        (result != self.fallback_tex).then_some(result)
    }

    pub(crate) fn preview_layered_material(
        &self,
        project: &std::path::Path,
        draft: &crate::project::material::MaterialDocument,
        color: [f32; 4],
        roughness: f32,
        metallic: f32,
    ) -> Option<String> {
        use crate::project::material::{MaterialCatalog, MaterialDefinition};
        let key = preview_layer_key(draft, color)?;
        if let Some(cached) = self.graph_baked_textures.borrow().get(&key) {
            return cached
                .as_ref()
                .ok()
                .copied()
                .filter(|handle| *handle != self.fallback_tex)
                .map(|_| key);
        }
        let mut doc = draft.clone();
        let MaterialDefinition::Asset { material } = &mut doc.definition else {
            return None;
        };
        *material = MaterialOutput::SurfacePbr {
            base_color: color,
            roughness,
            metallic,
        };
        if color[3] == 0.0 {
            doc.textures.remove("BaseColor");
        }
        let prefix = format!("MaterialPreview:{}:", doc.id);
        let outdated: Vec<String> = self
            .graph_baked_textures
            .borrow()
            .keys()
            .filter(|existing| existing.starts_with(&prefix) && *existing != &key)
            .cloned()
            .collect();
        for old in outdated {
            if let Some(Ok(handle)) = self.graph_baked_textures.borrow_mut().remove(&old) {
                if gl::DeleteTextures::is_loaded() && handle != self.fallback_tex && handle != 0 {
                    unsafe { gl::DeleteTextures(1, &handle) };
                }
            }
        }
        let mut docs = self.material_snapshot().documents().clone();
        let name = docs
            .iter()
            .find(|(_, existing)| existing.id == doc.id)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| doc.id.clone());
        docs.insert(name, doc.clone());
        let catalog = MaterialCatalog::preview(project, docs);
        let handle = self.layered_material_texture_with_key(&doc.id, &catalog, key.clone());
        (handle != self.fallback_tex).then_some(key)
    }

    pub(super) fn layered_material_texture(
        &self,
        id: &str,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> u32 {
        self.layered_material_texture_with_key(id, catalog, format!("Material:{id}"))
    }

    fn layered_material_texture_with_key(
        &self,
        id: &str,
        catalog: &crate::project::material::MaterialCatalog,
        key: String,
    ) -> u32 {
        if let Some(result) = self.graph_baked_textures.borrow().get(&key) {
            return result.as_ref().copied().unwrap_or(self.fallback_tex);
        }
        let result = (|| -> Result<u32, String> {
            use crate::project::material::MaterialLayerBlend;
            let layers = catalog.layers(id)?;
            let output = catalog.resolve(id)?;
            let MaterialOutput::SurfacePbr { base_color, .. } = output else {
                return Err("Layer baking requires a Surface Material".into());
            };
            let project = catalog.project().ok_or("No active Material project")?;
            let size = 256;
            let mut pixels =
                vec![[base_color[0], base_color[1], base_color[2], base_color[3]]; size * size];
            let mut sources = Vec::new();
            if let Some(base) = catalog.texture_maps(id)?.get("BaseColor") {
                sources.push((base.clone(), 1.0, MaterialLayerBlend::Normal, true));
            }
            for layer in layers {
                sources.push((layer.texture, layer.opacity, layer.blend, false));
            }
            for (identity, opacity, blend, is_base) in sources {
                if opacity == 0.0 {
                    continue;
                }
                let path = crate::project::texture_material::texture_path(project, &identity)?;
                let cached_image = { self.layer_source_images.borrow().get(&path).cloned() };
                let image = if let Some(cached) = cached_image {
                    cached
                } else {
                    let decoded = std::sync::Arc::new(
                        image::open(&path)
                            .map_err(|e| format!("Layer texture {}: {e}", path.display()))?
                            .resize_exact(
                                size as u32,
                                size as u32,
                                image::imageops::FilterType::Triangle,
                            )
                            .to_rgba8(),
                    );
                    let mut cache = self.layer_source_images.borrow_mut();
                    if cache.len() >= 128 {
                        if let Some(evict) = cache.keys().next().cloned() {
                            cache.remove(&evict);
                        }
                    }
                    cache.entry(path).or_insert_with(|| decoded.clone()).clone()
                };
                for (dst, src) in pixels.iter_mut().zip(image.pixels()) {
                    let alpha = opacity * src[3] as f32 / 255.0;
                    let input = [
                        src[0] as f32 / 255.0,
                        src[1] as f32 / 255.0,
                        src[2] as f32 / 255.0,
                    ];
                    let out_alpha = alpha + dst[3] * (1.0 - alpha);
                    let coverage = if out_alpha > 0.000001 {
                        alpha / out_alpha
                    } else {
                        0.0
                    };
                    for c in 0..3 {
                        let source = if is_base {
                            input[c] * base_color[c]
                        } else {
                            input[c]
                        };
                        dst[c] = blend_channel(dst[c], source, coverage, &blend);
                    }
                    dst[3] = out_alpha;
                }
            }
            if !gl::GenTextures::is_loaded() {
                return Err("OpenGL unavailable".into());
            }
            let bytes: Vec<u8> = pixels
                .into_iter()
                .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
                .collect();
            let mut texture = 0;
            unsafe {
                let mut previous = 0;
                gl::GetIntegerv(gl::TEXTURE_BINDING_2D, &mut previous);
                gl::GenTextures(1, &mut texture);
                if texture == 0 {
                    return Err("Unable to allocate layered texture".into());
                }
                gl::BindTexture(gl::TEXTURE_2D, texture);
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA8 as i32,
                    size as i32,
                    size as i32,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    bytes.as_ptr().cast(),
                );
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                gl::BindTexture(gl::TEXTURE_2D, previous as u32);
            }
            Ok(texture)
        })();
        if let Err(error) = &result {
            crate::engine::diagnostics::report(
                "ERROR",
                "Material Layers",
                format!("Material {id}: {error}"),
            );
        }
        let handle = result.as_ref().copied().unwrap_or(self.fallback_tex);
        self.graph_baked_textures.borrow_mut().insert(key, result);
        handle
    }

    pub(super) fn solid_material_texture(
        &self,
        id: &str,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> u32 {
        let key = format!("Material:{id}");
        if let Some(result) = self.graph_baked_textures.borrow().get(&key) {
            return result.as_ref().copied().unwrap_or(self.fallback_tex);
        }
        let result = (|| -> Result<u32, String> {
            let MaterialOutput::SurfacePbr { base_color, .. } = catalog.resolve(id)? else {
                return Err("Not a Surface Material".into());
            };
            if !gl::GenTextures::is_loaded() {
                return Err("OpenGL unavailable".into());
            }
            let rgba = base_color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
            let mut texture = 0;
            unsafe {
                let mut previous = 0;
                gl::GetIntegerv(gl::TEXTURE_BINDING_2D, &mut previous);
                gl::GenTextures(1, &mut texture);
                if texture == 0 {
                    return Err("Could not allocate Material texture".into());
                }
                gl::BindTexture(gl::TEXTURE_2D, texture);
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA8 as i32,
                    1,
                    1,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    rgba.as_ptr().cast(),
                );
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                gl::BindTexture(gl::TEXTURE_2D, previous as u32);
            }
            Ok(texture)
        })();
        let handle = result.as_ref().copied().unwrap_or(self.fallback_tex);
        self.graph_baked_textures.borrow_mut().insert(key, result);
        handle
    }

    pub(super) fn graph_material_texture(&self, id: &str) -> Option<u32> {
        let key = format!("Material:{id}");
        if let Some(result) = self.graph_baked_textures.borrow().get(&key) {
            return Some(result.as_ref().copied().unwrap_or(self.fallback_tex));
        }
        let snapshot = self.material_snapshot();
        let Some(graph) = snapshot.graph(id).ok().flatten() else {
            return None;
        };
        let result = snapshot.resolve(id).and_then(|output| {
            snapshot
                .texture_maps(id)
                .and_then(|maps| self.bake_material_graph(graph, &output, &maps))
        });
        if let Err(error) = &result {
            crate::engine::diagnostics::report(
                "ERROR",
                "Material Graph",
                format!("Material {id}: {error}; using existing fallback texture"),
            );
        }
        let handle = result.as_ref().copied().unwrap_or(self.fallback_tex);
        self.graph_baked_textures.borrow_mut().insert(key, result);
        Some(handle)
    }

    fn bake_material_graph(
        &self,
        graph: &MaterialGraph,
        output: &MaterialOutput,
        maps: &BTreeMap<String, String>,
    ) -> Result<u32, String> {
        let compiled = graph_glsl::compile_surface(graph, output, maps)?;
        let MaterialOutput::SurfacePbr {
            base_color,
            roughness,
            metallic,
        } = output
        else {
            return Err("Only Surface Materials can be baked for Cells".into());
        };
        let input = maps
            .get("BaseColor")
            .map(|path| self.get_texture(path))
            .unwrap_or(self.fallback_tex);
        unsafe {
            let previous = GlRestore::capture();
            let result = (|| -> Result<u32, String> {
                let program = shader::try_create_program(VERTEX, &fragment_shader(&compiled))?;
                let mut vao = 0u32;
                let mut color = 0u32;
                let mut fbo = 0u32;
                gl::GenVertexArrays(1, &mut vao);
                gl::GenTextures(1, &mut color);
                gl::GenFramebuffers(1, &mut fbo);
                let baked = (|| -> Result<(), String> {
                    if vao == 0 || color == 0 || fbo == 0 {
                        return Err("OpenGL could not allocate a Material render target".into());
                    }
                    gl::ActiveTexture(gl::TEXTURE0);
                    gl::BindTexture(gl::TEXTURE_2D, color);
                    gl::TexImage2D(
                        gl::TEXTURE_2D,
                        0,
                        gl::RGBA8 as i32,
                        SIZE,
                        SIZE,
                        0,
                        gl::RGBA,
                        gl::UNSIGNED_BYTE,
                        std::ptr::null(),
                    );
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
                    gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
                    gl::FramebufferTexture2D(
                        gl::FRAMEBUFFER,
                        gl::COLOR_ATTACHMENT0,
                        gl::TEXTURE_2D,
                        color,
                        0,
                    );
                    if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
                        return Err("OpenGL Material bake framebuffer is incomplete".into());
                    }
                    gl::Viewport(0, 0, SIZE, SIZE);
                    gl::Disable(gl::DEPTH_TEST);
                    gl::Disable(gl::BLEND);
                    gl::Disable(gl::SCISSOR_TEST);
                    gl::Disable(gl::CULL_FACE);
                    gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
                    gl::UseProgram(program);
                    gl::Uniform4f(
                        super::resources::get_uniform_location(program, "wk_default_color"),
                        base_color[0],
                        base_color[1],
                        base_color[2],
                        base_color[3],
                    );
                    gl::Uniform1f(
                        super::resources::get_uniform_location(program, "wk_default_roughness"),
                        *roughness,
                    );
                    gl::Uniform1f(
                        super::resources::get_uniform_location(program, "wk_default_metallic"),
                        *metallic,
                    );
                    gl::Uniform1i(
                        super::resources::get_uniform_location(program, "u_texture"),
                        0,
                    );
                    for uniform in &compiled.uniforms {
                        let at = super::resources::get_uniform_location(program, &uniform.name);
                        let v = uniform.value;
                        match uniform.kind {
                            PinType::Scalar => gl::Uniform1f(at, v[0]),
                            PinType::Vec2 => gl::Uniform2f(at, v[0], v[1]),
                            PinType::Vec3 => gl::Uniform3f(at, v[0], v[1], v[2]),
                            PinType::Color => gl::Uniform4f(at, v[0], v[1], v[2], v[3]),
                        };
                    }
                    gl::ActiveTexture(gl::TEXTURE0);
                    gl::BindTexture(gl::TEXTURE_2D, input);
                    gl::BindVertexArray(vao);
                    gl::DrawArrays(gl::TRIANGLES, 0, 3);
                    let error = gl::GetError();
                    if error != gl::NO_ERROR {
                        return Err(format!("OpenGL Material bake error 0x{error:04X}"));
                    }
                    Ok(())
                })();
                gl::DeleteProgram(program);
                if vao != 0 {
                    gl::DeleteVertexArrays(1, &vao);
                }
                if fbo != 0 {
                    gl::DeleteFramebuffers(1, &fbo);
                }
                if let Err(error) = baked {
                    if color != 0 {
                        gl::DeleteTextures(1, &color);
                    }
                    return Err(error);
                }
                Ok(color)
            })();
            previous.restore();
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stars_blend_math_respects_opacity_and_modes() {
        use crate::project::material::MaterialLayerBlend;
        let close = |a: f32, b: f32| assert!((a - b).abs() < 0.0001, "{a} != {b}");
        close(
            blend_channel(0.2, 1.0, 0.0, &MaterialLayerBlend::Normal),
            0.2,
        );
        close(
            blend_channel(0.2, 1.0, 0.5, &MaterialLayerBlend::Normal),
            0.6,
        );
        close(
            blend_channel(0.8, 0.5, 1.0, &MaterialLayerBlend::Multiply),
            0.4,
        );
        close(blend_channel(0.8, 0.5, 1.0, &MaterialLayerBlend::Add), 1.0);
        close(
            blend_channel(0.2, 0.5, 1.0, &MaterialLayerBlend::Screen),
            0.6,
        );
    }
    #[test]
    fn preview_key_tracks_layers_and_color_but_not_nonbake_parameters() {
        use crate::project::material::{
            MaterialDefinition, MaterialDocument, MaterialLayer, MaterialLayerBlend, MaterialOutput,
        };
        let mut doc = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.4; 4],
                roughness: 0.5,
                metallic: 0.0,
            },
        });
        doc.layers.push(MaterialLayer {
            name: "Test".into(),
            texture: "test.png".into(),
            opacity: 0.75,
            blend: MaterialLayerBlend::Normal,
        });
        let key = preview_layer_key(&doc, [0.1, 0.2, 0.3, 1.0]).unwrap();
        let mut different = doc.clone();
        different.definition = MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.4; 4],
                roughness: 0.1,
                metallic: 1.0,
            },
        };
        assert_eq!(
            key,
            preview_layer_key(&different, [0.1, 0.2, 0.3, 1.0]).unwrap()
        );
        different.layers[0].opacity = 0.5;
        assert_ne!(
            key,
            preview_layer_key(&different, [0.1, 0.2, 0.3, 1.0]).unwrap()
        );
        assert_ne!(key, preview_layer_key(&doc, [0.3, 0.2, 0.1, 1.0]).unwrap());
        doc.textures.insert("BaseColor".into(), "image.png".into());
        let alpha0 = preview_layer_key(&doc, [0.1, 0.2, 0.3, 0.0]);
        doc.textures.remove("BaseColor");
        assert_eq!(alpha0, preview_layer_key(&doc, [0.1, 0.2, 0.3, 0.0]));
    }
    #[test]
    #[ignore = "Targeted allocation/time profiling: run on the editor Windows machine"]
    fn preview_catalog_scaling_probe() {
        use crate::project::material::{
            MaterialCatalog, MaterialDefinition, MaterialDocument, MaterialLayer,
            MaterialLayerBlend, MaterialOutput,
        };
        use std::time::Instant;
        let one = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.3, 0.5, 0.7, 1.0],
                roughness: 0.5,
                metallic: 0.0,
            },
        });
        let mut draft = one.clone();
        draft.layers.push(MaterialLayer {
            name: "test".into(),
            texture: "test.png".into(),
            opacity: 1.0,
            blend: MaterialLayerBlend::Normal,
        });
        let key = preview_layer_key(&draft, [0.3, 0.5, 0.7, 1.0]).unwrap();
        for count in [1usize, 64, 512, 4096] {
            let docs: std::collections::BTreeMap<_, _> = (0..count)
                .map(|i| {
                    let mut doc = one.clone();
                    doc.id = format!("id-{i}");
                    (format!("{i:04}"), doc)
                })
                .collect();
            let start = Instant::now();
            let mut baseline = 0u128;
            for _ in 0..8 {
                let before = Instant::now();
                let cloned = docs.clone();
                let catalog = MaterialCatalog::preview(std::path::Path::new("mat-probe"), cloned);
                std::hint::black_box(catalog);
                baseline += before.elapsed().as_nanos();
            }
            let mut cached = 0u128;
            for _ in 0..256 {
                let before = Instant::now();
                std::hint::black_box(
                    preview_layer_key(&draft, [0.3, 0.5, 0.7, 1.0]).unwrap() == key,
                );
                cached += before.elapsed().as_nanos();
            }
            eprintln!(
                "MAT014 M={count} old_catalog_clone_build_mean_us={} preview_key_mean_us={} wall_ms={}",
                baseline as f64 / 8_000.0,
                cached as f64 / 256_000.0,
                start.elapsed().as_millis()
            );
        }
    }
    #[test]
    fn baked_material_glsl_calls_graph_function_with_uv() {
        let shader=graph_glsl::SurfaceShader { source: "void wk_surface_graph(vec2 uv, vec4 c, float r, float m, out vec4 o, out float ro, out float me) {o=c;ro=r;me=m;}".into(),uniforms:vec![]};
        let source = fragment_shader(&shader);
        assert!(source.contains("wk_surface_graph(uv, wk_default_color"));
        assert!(source.contains("FragColor = clamp(color"));
    }
}

#[cfg(all(test, windows))]
mod gpu_tests {
    use super::*;
    use crate::project::material::{
        GraphLink, GraphNode, GraphOperation, MaterialCatalog, MaterialDefinition, MaterialDocument,
    };
    use glutin::{
        config::ConfigTemplateBuilder,
        context::ContextAttributesBuilder,
        display::GetGlDisplay,
        prelude::*,
        surface::{SurfaceAttributesBuilder, WindowSurface},
    };
    use glutin_winit::DisplayBuilder;
    use std::{collections::BTreeMap, ffi::CString, num::NonZeroU32};
    use winit::{
        event_loop::EventLoop, platform::windows::EventLoopBuilderExtWindows,
        raw_window_handle::HasWindowHandle, window::WindowAttributes,
    };

    #[test]
    #[ignore = "Requires the active Windows OpenGL desktop; draws to an offscreen Material texture"]
    fn graph_base_color_changes_real_opengl_pixels() {
        let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(
                WindowAttributes::default()
                    .with_visible(false)
                    .with_inner_size(winit::dpi::PhysicalSize::new(64, 64)),
            ))
            .build(&event_loop, ConfigTemplateBuilder::new(), |configs| {
                configs.max_by_key(|c| c.num_samples()).unwrap()
            })
            .unwrap();
        let window = window.unwrap();
        let handle = window.window_handle().unwrap().as_raw();
        let context = unsafe {
            config
                .display()
                .create_context(
                    &config,
                    &ContextAttributesBuilder::new().build(Some(handle)),
                )
                .unwrap()
        };
        let surface = unsafe {
            config
                .display()
                .create_window_surface(
                    &config,
                    &SurfaceAttributesBuilder::<WindowSurface>::new().build(
                        handle,
                        NonZeroU32::new(64).unwrap(),
                        NonZeroU32::new(64).unwrap(),
                    ),
                )
                .unwrap()
        };
        let _context = context.make_current(&surface).unwrap();
        gl::load_with(|name| {
            config
                .display()
                .get_proc_address(&CString::new(name).unwrap())
                .cast()
        });

        let graph = MaterialGraph {
            output_node: 2,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::CustomFunction {
                        source: "result = color(0.0, 1.0, 0.0, 1.0);".into(),
                        inputs: vec![],
                        outputs: vec![crate::project::material::CustomPin {
                            name: "result".into(),
                            value_type: crate::project::material::PinType::Color,
                        }],
                    },
                    position: [0.0; 2],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::SurfaceOutput,
                    position: [0.0; 2],
                },
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "result".into(),
                target_node: 2,
                target_pin: "base_color".into(),
            }],
        };
        let mut doc = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.2, 0.2, 0.2, 1.0],
                roughness: 0.5,
                metallic: 0.0,
            },
        });
        doc.graph = Some(graph);
        let id = doc.id.clone();
        let materials =
            MaterialCatalog::from_documents(BTreeMap::from([("Green.MAT".into(), doc)]));
        let project =
            std::env::temp_dir().join(format!("mat012-gpu-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&project).unwrap();
        let renderer = Renderer::new(64.0, 64.0);
        renderer.set_project_asset_root_with_materials(Some(&project), materials);
        let baked = renderer.get_texture(&format!("Material:{id}"));
        assert_ne!(
            baked, renderer.fallback_tex,
            "Surface graph must not fall back to Block_tx"
        );
        let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, baked);
            gl::GetTexImage(
                gl::TEXTURE_2D,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr().cast(),
            );
        }
        let middle = ((SIZE * SIZE / 2 + SIZE / 2) * 4) as usize;
        assert!(
            pixels[middle] < 12 && pixels[middle + 1] > 230 && pixels[middle + 2] < 12,
            "OpenGL Surface Material graph must turn the Cell texture green: {:?}",
            &pixels[middle..middle + 4]
        );
        let snapshot = renderer.material_snapshot();
        let output = snapshot.resolve(&id).unwrap();
        let authored_graph = snapshot.graph(&id).unwrap().unwrap();
        let maps = snapshot.texture_maps(&id).unwrap();
        let preview = renderer.surface_material_preview(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)),
            [0.2, 0.2, 0.2, 1.0],
            0.5,
            0.0,
            None,
            Some((authored_graph, &output, &maps)),
            false,
        );
        assert!(
            preview.is_ok(),
            "OpenGL graph preview shader failed: {:?}",
            preview.err()
        );
        assert!(
            renderer.grid_cell_pbr_location >= 0,
            "World shader must expose Cell PBR uniform"
        );
        let water = renderer.water_material_preview(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)),
            crate::world::terrain::TerrainWaterSettings::default(),
        );
        assert!(
            water.is_ok(),
            "World Water preview must link the actual OpenGL Water shader: {:?}",
            water.err()
        );
        assert!(
            renderer.landscape_preview_geometry().is_some(),
            "Landscape preview must allocate an OpenGL mesh"
        );
        drop(renderer);
        std::fs::remove_dir_all(project).unwrap();
    }
}
