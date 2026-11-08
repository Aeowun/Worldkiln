// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::ffi::CString;
use std::path::Path;
pub(super) fn texture_binding_changed(last: &mut Option<u32>, texture: u32) -> bool {
    if *last == Some(texture) {
        return false;
    }
    *last = Some(texture);
    true
}

use image::GenericImageView;

#[cfg(feature = "editor")]
use crate::editor::GridPlane;

#[cfg(not(feature = "editor"))]
use super::GridPlane;

use super::Renderer;
use super::mesh::{VERTEX_3D_FLOATS, add_block_quad, add_line, upload_vertices_3d};

fn texture_candidate_paths(
    identifier: &str,
    project_assets: Option<&Path>,
) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();

    if let Some(project_assets) = project_assets {
        paths.push(project_assets.join("textures").join(identifier));
        paths.push(
            project_assets
                .join("textures")
                .join(format!("{identifier}.png")),
        );
    }

    let engine_assets = crate::project::engine_assets_dir();
    paths.push(engine_assets.join("textures").join(identifier));
    paths.push(
        engine_assets
            .join("textures")
            .join(format!("{identifier}.png")),
    );
    paths.push(engine_assets.join("skybox").join(identifier));
    paths.push(
        engine_assets
            .join("skybox")
            .join(format!("{identifier}.png")),
    );

    paths
}

fn skybox_candidate_paths(
    identifier: &str,
    project_assets: Option<&Path>,
) -> Vec<std::path::PathBuf> {
    let relative = Path::new(identifier);
    if identifier.trim().is_empty()
        || identifier.contains('\\')
        || !relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        return Vec::new();
    }
    let engine_assets = crate::project::engine_assets_dir();
    let roots = project_assets
        .into_iter()
        .map(|dir| dir.join("skybox"))
        .chain(std::iter::once(engine_assets.join("skybox")));
    roots
        .flat_map(|root| [root.join(relative), root.join(format!("{identifier}.png"))])
        .collect()
}

impl Renderer {
    pub fn set_project_asset_root(&self, project_path: Option<&Path>) {
        let catalog = project_path
            .map(crate::project::material::MaterialCatalog::load)
            .unwrap_or_default();
        self.set_project_asset_root_with_materials(project_path, catalog);
    }
    pub(crate) fn set_project_asset_root_with_materials(
        &self,
        project_path: Option<&Path>,
        catalog: crate::project::material::MaterialCatalog,
    ) {
        super::effekseer::shutdown();
        self.clear_baked_graph_textures();
        let mut entity_mesh_cache = self.entity_mesh_cache.borrow_mut();
        for mesh in entity_mesh_cache.values() {
            unsafe { mesh.free_gl_resources() };
        }
        entity_mesh_cache.clear();

        *self.project_assets_dir.borrow_mut() = project_path.map(|path| path.join(".assets"));
        self.post_process.borrow_mut().load(project_path);
        *self.materials.borrow_mut() = std::sync::Arc::new(catalog);
        self.invalidate_texture_cache();
        self.paint_gpu.borrow_mut().invalidate();
        for (_, cubemap) in self.cubemaps.borrow_mut().drain() {
            if cubemap != 0 && gl::DeleteTextures::is_loaded() {
                unsafe { gl::DeleteTextures(1, &cubemap) };
            }
        }
    }

    pub fn refresh_post_process_pipeline(&self, project: Option<&Path>) {
        let next = match project {
            Some(root) => {
                match crate::project::post_process_pipeline::PostProcessPipeline::load(root) {
                    Ok(pipeline) => pipeline,
                    Err(error) => {
                        crate::engine::diagnostics::report("ERROR", "Post Processing", error);
                        return;
                    }
                }
            }
            None => crate::project::post_process_pipeline::PostProcessPipeline::new(),
        };
        let mut state = self.post_process.borrow_mut();
        if state.authored != next {
            state.authored = next;
            state.runtime.reset();
        }
    }

    pub fn apply_runtime_post_process_command(
        &self,
        command: crate::project::post_process_pipeline::PostProcessCommand,
    ) -> Result<(), String> {
        use crate::project::post_process_pipeline::PostProcessCommand;
        let mut state = self.post_process.borrow_mut();
        match command {
            PostProcessCommand::SetParameter {
                pass_id,
                name,
                value,
            } => {
                let authored = state.authored.clone();
                state
                    .runtime
                    .set_parameter(&authored, &pass_id, &name, value)
            }
            PostProcessCommand::SetEnabled { pass_id, enabled } => {
                let authored = state.authored.clone();
                state.runtime.set_enabled(&authored, &pass_id, enabled)
            }
            PostProcessCommand::SetObjectChannel { object, channel } => {
                state.runtime.set_object_channel(object, channel)
            }
        }
    }

    pub fn reset_runtime_post_process(&self) {
        self.post_process.borrow_mut().runtime.reset();
    }

    pub fn reload_material_assets(&self) {
        self.clear_baked_graph_textures();
        let project = self
            .project_assets_dir
            .borrow()
            .as_ref()
            .and_then(|root| root.parent().map(Path::to_owned));
        let mut catalog = project
            .as_deref()
            .map(crate::project::material::MaterialCatalog::load)
            .unwrap_or_default();
        catalog.retain_last_valid(&self.materials.borrow());
        for (name, error) in &catalog.errors() {
            crate::engine::diagnostics::report("ERROR", "Material", format!("{name}: {error}"));
        }
        *self.materials.borrow_mut() = std::sync::Arc::new(catalog);
        self.invalidate_sky_reflections();
        self.post_process.borrow_mut().invalidate_programs();
    }

    pub fn invalidate_sky_reflections(&self) {
        for (_, texture) in self.cubemaps.borrow_mut().drain() {
            if texture != 0 && gl::DeleteTextures::is_loaded() {
                unsafe {
                    gl::DeleteTextures(1, &texture);
                }
            }
        }
    }

    pub fn material_snapshot(&self) -> std::sync::Arc<crate::project::material::MaterialCatalog> {
        self.materials.borrow().clone()
    }
    pub fn set_runtime_material_layer_alpha(
        &self,
        id: &str,
        layer: &str,
        alpha: f32,
    ) -> Result<(), String> {
        let previous = self.material_snapshot();
        let next = previous.with_runtime_layer_alpha(id, layer, alpha)?;
        let affected: Vec<_> = previous
            .documents()
            .values()
            .filter_map(|document| {
                (previous.layers(&document.id).ok() != next.layers(&document.id).ok())
                    .then(|| document.id.clone())
            })
            .collect();
        for affected_id in &affected {
            let prefix = format!("MaterialPreview:{affected_id}:");
            let names = [
                format!("Material:{affected_id}"),
                format!("MaterialOverlay:{affected_id}"),
            ];
            let keys: Vec<String> = self
                .graph_baked_textures
                .borrow()
                .keys()
                .filter(|key| names.contains(key) || key.starts_with(&prefix))
                .cloned()
                .collect();
            for key in keys {
                if let Some(Ok(texture)) = self.graph_baked_textures.borrow_mut().remove(&key) {
                    if texture != 0
                        && texture != self.fallback_tex
                        && gl::DeleteTextures::is_loaded()
                    {
                        unsafe { gl::DeleteTextures(1, &texture) };
                    }
                }
            }
            self.textures
                .borrow_mut()
                .remove(&format!("Material:{affected_id}"));
        }
        *self.materials.borrow_mut() = std::sync::Arc::new(next);
        if !affected.is_empty() {
            self.invalidate_sky_reflections();
        }
        Ok(())
    }
    pub fn publish_material_snapshot(
        &self,
        catalog: crate::project::material::MaterialCatalog,
    ) -> Result<(), String> {
        let root = self.project_assets_dir.borrow();
        let project = root.as_ref().and_then(|root| root.parent());
        if project.is_none() || catalog.project() != project {
            return Err("Material snapshot belongs to another project".into());
        }
        self.clear_baked_graph_textures();
        *self.materials.borrow_mut() = std::sync::Arc::new(catalog);
        self.post_process.borrow_mut().invalidate_programs();
        self.invalidate_sky_reflections();
        Ok(())
    }

    pub fn publish_saved_material_snapshot(
        &self,
        catalog: crate::project::material::MaterialCatalog,
        material_id: &str,
    ) -> Result<(), String> {
        let root = self.project_assets_dir.borrow();
        let project = root.as_ref().and_then(|p| p.parent());
        if project.is_none() || catalog.project() != project {
            return Err("Material snapshot belongs to another project".into());
        }
        let mut children = std::collections::HashMap::<&str, Vec<&str>>::new();
        for doc in catalog.documents().values() {
            if let crate::project::material::MaterialDefinition::Instance { parent, .. } =
                &doc.definition
            {
                children
                    .entry(parent.as_str())
                    .or_default()
                    .push(doc.id.as_str());
            }
        }
        let mut affected = std::collections::HashSet::<String>::new();
        affected.insert(material_id.to_owned());
        let mut queue = vec![material_id.to_owned()];
        let mut cursor = 0;
        while cursor < queue.len() {
            if let Some(dependents) = children.get(queue[cursor].as_str()) {
                for &child in dependents {
                    if affected.insert(child.to_owned()) {
                        queue.push(child.to_owned());
                    }
                }
            }
            cursor += 1;
        }
        {
            let mut baked = self.graph_baked_textures.borrow_mut();
            let keys: Vec<_> = baked
                .keys()
                .filter(|key| {
                    key.strip_prefix("Material:")
                        .is_some_and(|id| affected.contains(id))
                        || key
                            .strip_prefix("MaterialOverlay:")
                            .is_some_and(|id| affected.contains(id))
                        || key
                            .strip_prefix("MaterialPreview:")
                            .and_then(|rest| rest.split_once(':'))
                            .is_some_and(|(id, _)| affected.contains(id))
                })
                .cloned()
                .collect();
            for key in keys {
                if let Some(Ok(handle)) = baked.remove(&key) {
                    if handle != 0 && handle != self.fallback_tex && gl::DeleteTextures::is_loaded()
                    {
                        unsafe {
                            gl::DeleteTextures(1, &handle);
                        }
                    }
                }
            }
        }
        self.textures.borrow_mut().retain(|key, _| {
            !key.strip_prefix("Material:")
                .is_some_and(|id| affected.contains(id))
        });
        if self
            .active_sky_material
            .borrow()
            .as_ref()
            .is_some_and(|id| affected.contains(id))
        {
            self.invalidate_sky_reflections();
        }
        *self.materials.borrow_mut() = std::sync::Arc::new(catalog);
        Ok(())
    }

    pub fn invalidate_texture_cache(&self) {
        self.textures.borrow_mut().clear();
    }

    pub fn get_texture(&self, identifier: &str) -> u32 {
        if identifier.is_empty() {
            return self.fallback_tex;
        }

        if let Some(&texture) = self.textures.borrow().get(identifier) {
            return texture;
        }

        if identifier.starts_with("pack:") {
            let root = self.project_assets_dir.borrow();
            let project = root.as_ref().and_then(|p| p.parent());
            let texture = project
                .and_then(|p| crate::project::asset_packs::resource_path(p, identifier).ok())
                .and_then(|p| load_texture_from_file(&p))
                .unwrap_or(self.fallback_tex);
            self.textures
                .borrow_mut()
                .insert(identifier.into(), texture);
            return texture;
        }
        if identifier.starts_with("MaterialPreview:") {
            return self
                .graph_baked_textures
                .borrow()
                .get(identifier)
                .and_then(|result| result.as_ref().ok().copied())
                .unwrap_or(self.fallback_tex);
        }
        if let Some(id) = identifier.strip_prefix("Material:") {
            if let Some(baked) = self.graph_material_texture(id) {
                return baked;
            }
            let snapshot = self.material_snapshot();
            if snapshot.layers(id).is_ok_and(|layers| !layers.is_empty()) {
                return self.layered_material_texture(id, &snapshot);
            }
            let source = snapshot
                .texture_maps(id)
                .ok()
                .and_then(|maps| maps.get("BaseColor").cloned());
            let Some(source) = source else {
                return self.solid_material_texture(id, &snapshot);
            };
            let texture = self.get_texture(&source);
            self.textures
                .borrow_mut()
                .insert(identifier.into(), texture);
            return texture;
        }

        if let Some(name) = identifier.strip_prefix("PBR:") {
            let resolved = crate::renderer::rhi::material_library::MaterialLibrary::default_root()
                .and_then(|root| {
                    crate::renderer::rhi::material_library::MaterialLibrary::load(root).ok()
                })
                .and_then(|catalog| catalog.map_path(name, "BaseColor"));
            if let Some(path) = resolved {
                let tex = self.get_pbr_texture_file(&path);
                self.textures.borrow_mut().insert(identifier.into(), tex);
                return tex;
            }
            self.textures
                .borrow_mut()
                .insert(identifier.into(), self.fallback_tex);
            return self.fallback_tex;
        }

        let project_assets = self.project_assets_dir.borrow();
        for path in texture_candidate_paths(identifier, project_assets.as_deref()) {
            if path.exists() {
                if let Some(texture) = load_texture_from_file(&path) {
                    self.textures
                        .borrow_mut()
                        .insert(identifier.to_string(), texture);
                    return texture;
                }
            }
        }

        if let Some(png) = builtin_editor_marker_png(identifier) {
            if let Ok(image) = image::load_from_memory(png) {
                let texture = upload_rgba_texture(image.to_rgba8());
                self.textures
                    .borrow_mut()
                    .insert(identifier.into(), texture);
                return texture;
            }
        }

        if identifier != "Block_tx" {
            crate::engine::diagnostics::report(
                "WARNING",
                "Asset",
                format!(
                    "Texture '{identifier}' could not be loaded from the active project's .assets/textures or the engine .assets/textures/.assets/skybox roots; using fallback texture. Check the path and PNG contents."
                ),
            );
        }
        self.textures
            .borrow_mut()
            .insert(identifier.to_string(), self.fallback_tex);
        self.fallback_tex
    }

    fn get_pbr_texture_file(&self, path: &Path) -> u32 {
        let key = format!("pbr::{}", path.to_string_lossy().to_ascii_lowercase());
        if let Some(&texture) = self.textures.borrow().get(&key) {
            return texture;
        }
        let texture = load_pbr_texture_from_file(path).unwrap_or(self.fallback_tex);
        self.textures.borrow_mut().insert(key, texture);
        texture
    }

    pub fn get_texture_file(&self, path: &Path) -> u32 {
        let cache_key = format!("file::{}", path.to_string_lossy().to_ascii_lowercase());
        if let Some(&texture) = self.textures.borrow().get(&cache_key) {
            return texture;
        }
        let texture = load_texture_from_file(path).unwrap_or(self.fallback_tex);
        self.textures.borrow_mut().insert(cache_key, texture);
        texture
    }

    pub fn get_cubemap(&self, identifier: &str) -> u32 {
        let active_id = self.active_sky_material.borrow().clone();
        let active_gradient = active_id
            .as_deref()
            .and_then(|id| self.materials.borrow().sky(id));
        if let Some(id) = active_id.as_ref() {
            let key = format!("sky-material:{id}");
            let mut cache = self.cubemaps.borrow_mut();
            if let Some(&texture) = cache.get(&key) {
                return texture;
            }
            let gradient = active_gradient
                .as_ref()
                .unwrap_or_else(|| crate::project::sky_material::default_sky());
            if let Some(texture) = load_default_sky_reflection_cubemap(gradient) {
                cache.insert(key, texture);
                return texture;
            }
        }
        let mut cubemaps = self.cubemaps.borrow_mut();

        if let Some(&tex) = cubemaps.get(identifier) {
            return tex;
        }

        let built_in = identifier == crate::project::sky_material::DEFAULT_SKY_ID
            || matches!(
                identifier,
                "Cubemap_Temperate_01-512x512.png"
                    | "Cubemap_Tropical_01-512x512.png"
                    | "Cubemap_Desert_01-512x512.png"
                    | "Cubemap_Snowy_01-512x512.png"
                    | "Cubemap_Mars_01-512x512.png"
                    | "Cubemap_Island_02-512x512.png"
            );
        let has_project_override = self
            .project_assets_dir
            .borrow()
            .as_ref()
            .is_some_and(|assets| assets.join("skybox").join(identifier).is_file());
        if built_in && !has_project_override {
            if let Some(texture) =
                load_default_sky_reflection_cubemap(crate::project::sky_material::default_sky())
            {
                cubemaps.insert(identifier.to_owned(), texture);
                return texture;
            }
            return 0;
        }

        let project_assets = self.project_assets_dir.borrow();
        for path in skybox_candidate_paths(identifier, project_assets.as_deref()) {
            if path.exists() {
                if let Some(tex) = load_cubemap_from_file(&path.to_string_lossy()) {
                    cubemaps.insert(identifier.to_string(), tex);
                    return tex;
                }
            }
        }

        if !identifier.is_empty() {
            crate::engine::diagnostics::report(
                "WARNING",
                "Renderer",
                format!(
                    "Skybox '{identifier}' could not be loaded from the active project's .assets/skybox or engine .assets/skybox; no skybox will be rendered. Check the relative path and cross-layout PNG."
                ),
            );
        }
        0
    }
}

pub(super) fn get_uniform_location(program: u32, name: &str) -> i32 {
    let name = CString::new(name).expect("uniform name must not contain NUL");

    unsafe { gl::GetUniformLocation(program, name.as_ptr()) }
}

#[cfg(all(test, feature = "editor"))]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn skybox_paths_are_project_relative_and_project_switch_invalidates_cubemap_cache() {
        let first = Path::new("world_a").join(".assets");
        let second = Path::new("world_b").join(".assets");
        let paths = skybox_candidate_paths("islands/cross", Some(&first));
        assert_eq!(paths[0], first.join("skybox/islands/cross"));
        assert_eq!(paths[1], first.join("skybox/islands/cross.png"));
        assert!(paths[2].starts_with(crate::project::engine_assets_dir()));
        let other = skybox_candidate_paths("islands/cross", Some(&second));
        assert_eq!(other[0], second.join("skybox/islands/cross"));
        assert_ne!(paths[0], other[0]);

        for forbidden in [
            "../cross.png",
            "./cross.png",
            "/cross.png",
            "C:\\bad\\cross.png",
            "",
        ] {
            assert!(
                skybox_candidate_paths(forbidden, Some(&first)).is_empty(),
                "refuse skybox traversal: {forbidden}"
            );
        }
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        renderer.cubemaps.borrow_mut().insert("cross".into(), 0);
        renderer.set_project_asset_root(Some(Path::new("world_a")));
        assert!(renderer.cubemaps.borrow().is_empty());
        renderer.cubemaps.borrow_mut().insert("cross".into(), 0);
        renderer.set_project_asset_root(Some(Path::new("world_b")));
        assert!(renderer.cubemaps.borrow().is_empty());
    }

    #[test]
    fn texture_bindings_skip_only_consecutive_duplicates() {
        let mut last = None;
        assert!(texture_binding_changed(&mut last, 7));
        assert!(!texture_binding_changed(&mut last, 7));
        assert!(texture_binding_changed(&mut last, 0));
        assert!(texture_binding_changed(&mut last, 7));
        assert!(!texture_binding_changed(&mut last, 7));
    }

    #[test]
    fn texture_resolution_misses_are_cached_until_explicit_invalidation() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        renderer.set_project_asset_root(Some(Path::new("nonexistent_wo51_project")));
        let name = "wo51_uncached_texture_should_not_exist";
        assert_eq!(renderer.get_texture(name), renderer.fallback_tex);
        assert_eq!(
            renderer.textures.borrow().get(name),
            Some(&renderer.fallback_tex)
        );
        assert_eq!(renderer.get_texture(name), renderer.fallback_tex);
        renderer.invalidate_texture_cache();
        assert!(!renderer.textures.borrow().contains_key(name));
        renderer.get_texture(name);
        renderer.set_project_asset_root(Some(Path::new("another_wo51_project")));
        assert!(!renderer.textures.borrow().contains_key(name));
    }

    #[test]
    fn block_fallback_may_be_overridden_after_project_switch() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        renderer
            .textures
            .borrow_mut()
            .insert("Block_tx".into(), renderer.fallback_tex);
        renderer.set_project_asset_root(Some(Path::new("wo51_project")));
        assert!(!renderer.textures.borrow().contains_key("Block_tx"));
    }

    #[test]
    fn builtin_marker_icons_decode_without_optional_asset_packs() {
        for name in ["speaker", "lightbulb"] {
            let bytes = builtin_editor_marker_png(name).unwrap();
            let image = image::load_from_memory(bytes).unwrap();
            assert!(image.width() > 0 && image.height() > 0);
        }
        assert!(builtin_editor_marker_png("unrelated-texture").is_none());
    }

    #[test]
    fn project_texture_candidates_precede_engine_assets() {
        let project_assets = Path::new("ProjectRoot").join(".assets");
        let paths = texture_candidate_paths("terrain/stone", Some(&project_assets));

        assert_eq!(
            paths[0],
            project_assets.join("textures").join("terrain/stone")
        );
        assert_eq!(
            paths[1],
            project_assets.join("textures").join("terrain/stone.png")
        );
        assert!(paths[2].to_string_lossy().contains("textures"));
    }

    #[test]
    fn missing_texture_and_skybox_are_visible_without_frame_spam() {
        crate::engine::diagnostics::clear();
        let renderer = Renderer::new_for_tests(1280.0, 720.0);
        for _ in 0..60 {
            renderer.get_texture("wo04_missing_texture.png");
            renderer.get_cubemap("wo04_missing_skybox.png");
        }
        let mut editor = crate::editor::Editor::new();
        editor.script_editor.show_output = false;
        editor.collect_diagnostics();
        assert!(editor.script_editor.show_output);
        assert_eq!(
            editor
                .terminal_output
                .matches("[WARNING] [Asset] Texture 'wo04_missing_texture.png'")
                .count(),
            1
        );
        assert_eq!(
            editor
                .terminal_output
                .matches("[WARNING] [Renderer] Skybox 'wo04_missing_skybox.png'")
                .count(),
            1
        );
        assert!(editor.terminal_output.contains("using fallback texture"));
    }
}

pub(super) fn create_character_vao_vbo() -> (u32, u32) {
    unsafe {
        let mut vao = 0;
        let mut vbo = 0;

        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);

        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

        let stride = (VERTEX_3D_FLOATS * std::mem::size_of::<f32>()) as i32;

        gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
        gl::EnableVertexAttribArray(0);

        gl::VertexAttribPointer(
            1,
            3,
            gl::FLOAT,
            gl::FALSE,
            stride,
            (3 * std::mem::size_of::<f32>()) as *const _,
        );
        gl::EnableVertexAttribArray(1);

        gl::VertexAttribPointer(
            2,
            3,
            gl::FLOAT,
            gl::FALSE,
            stride,
            (6 * std::mem::size_of::<f32>()) as *const _,
        );
        gl::EnableVertexAttribArray(2);

        gl::BindVertexArray(0);

        (vao, vbo)
    }
}

pub(super) fn create_grid_plane_vao(plane: GridPlane) -> (u32, u32, i32) {
    #[cfg(feature = "editor")]
    let vertices = crate::editor::grid::generate_grid_vertices(plane);
    #[cfg(not(feature = "editor"))]
    let vertices: Vec<f32> = Vec::new();

    upload_vertices_3d(&vertices)
}

pub(super) fn create_axes() -> (u32, u32, i32) {
    let length = 3.0;
    let mut vertices = Vec::new();

    add_line(
        &mut vertices,
        [0.0, 0.0, 0.0],
        [length, 0.0, 0.0],
        [1.0, 0.2, 0.2, 1.0],
    );

    add_line(
        &mut vertices,
        [0.0, 0.0, 0.0],
        [0.0, length, 0.0],
        [0.2, 1.0, 0.2, 1.0],
    );

    add_line(
        &mut vertices,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, length],
        [0.2, 0.2, 1.0, 1.0],
    );

    upload_vertices_3d(&vertices)
}

pub(super) fn create_highlight_box() -> (u32, u32, i32) {
    let mut vertices = Vec::new();

    let color = [1.0, 0.8, 0.1, 0.9];

    let min = 0.0;
    let max = 1.0;

    add_line(&mut vertices, [min, min, min], [max, min, min], color);
    add_line(&mut vertices, [max, min, min], [max, min, max], color);
    add_line(&mut vertices, [max, min, max], [min, min, max], color);
    add_line(&mut vertices, [min, min, max], [min, min, min], color);

    add_line(&mut vertices, [min, max, min], [max, max, min], color);
    add_line(&mut vertices, [max, max, min], [max, max, max], color);
    add_line(&mut vertices, [max, max, max], [min, max, max], color);
    add_line(&mut vertices, [min, max, max], [min, max, min], color);

    add_line(&mut vertices, [min, min, min], [min, max, min], color);
    add_line(&mut vertices, [max, min, min], [max, max, min], color);
    add_line(&mut vertices, [max, min, max], [max, max, max], color);
    add_line(&mut vertices, [min, min, max], [min, max, max], color);

    upload_vertices_3d(&vertices)
}

pub(super) fn create_anchor_marker() -> (u32, u32, i32) {
    use std::f32::consts::PI;

    let mut vertices = Vec::new();

    let color = [1.0, 1.0, 1.0, 1.0];

    let radius = 0.15;
    let center = [0.0, 0.0, 0.0];

    let latitudes = 8;
    let longitudes = 16;

    for i in 0..=latitudes {
        let lat = PI * (i as f32 / latitudes as f32 - 0.5);

        let y = lat.sin() * radius + center[1];

        let r = lat.cos() * radius;

        for j in 0..longitudes {
            let lon1 = 2.0 * PI * (j as f32 / longitudes as f32);

            let lon2 = 2.0 * PI * ((j + 1) as f32 / longitudes as f32);

            add_line(
                &mut vertices,
                [lon1.cos() * r + center[0], y, lon1.sin() * r + center[2]],
                [lon2.cos() * r + center[0], y, lon2.sin() * r + center[2]],
                color,
            );
        }
    }

    for i in 0..longitudes {
        let lon = 2.0 * PI * (i as f32 / longitudes as f32);

        let cos_lon = lon.cos();

        let sin_lon = lon.sin();

        for j in 0..latitudes {
            let lat1 = PI * (j as f32 / latitudes as f32 - 0.5);

            let lat2 = PI * ((j + 1) as f32 / latitudes as f32 - 0.5);

            add_line(
                &mut vertices,
                [
                    cos_lon * lat1.cos() * radius + center[0],
                    lat1.sin() * radius + center[1],
                    sin_lon * lat1.cos() * radius + center[2],
                ],
                [
                    cos_lon * lat2.cos() * radius + center[0],
                    lat2.sin() * radius + center[1],
                    sin_lon * lat2.cos() * radius + center[2],
                ],
                color,
            );
        }
    }

    upload_vertices_3d(&vertices)
}

pub(super) fn create_billboard_vao() -> (u32, u32) {
    let mut vertices = Vec::new();

    let color = [1.0, 1.0, 1.0, 1.0];

    let normal = [0.0, 0.0, 1.0];

    add_block_quad(
        &mut vertices,
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
        color,
        normal,
    );

    let (vao, vbo, _) = super::mesh::upload_block_vertices_3d(&vertices);

    (vao, vbo)
}

fn builtin_editor_marker_png(identifier: &str) -> Option<&'static [u8]> {
    match identifier {
        "speaker" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.assets/textures/speaker.png"
        ))),
        "lightbulb" => Some(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/.assets/textures/lightbulb.png"
        ))),
        _ => None,
    }
}

fn upload_rgba_texture(rgba: image::RgbaImage) -> u32 {
    let (width, height) = rgba.dimensions();
    let mut texture = 0;
    unsafe {
        gl::GenTextures(1, &mut texture);
        gl::BindTexture(gl::TEXTURE_2D, texture);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as i32,
            width as i32,
            height as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            rgba.as_raw().as_ptr() as *const _,
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
        gl::BindTexture(gl::TEXTURE_2D, 0);
    }
    texture
}

pub(super) fn load_texture_from_file(path: impl AsRef<Path>) -> Option<u32> {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("asset", "texture_decode_upload", false)
            .map(|span| span.asset(path.as_ref()));
    let path = path.as_ref();
    match image::open(path) {
        Ok(image) => Some(upload_rgba_texture(image.to_rgba8())),
        Err(error) => {
            crate::engine::diagnostics::report(
                "WARNING",
                "Asset",
                format!("Failed to load texture {}: {error:?}", path.display()),
            );
            None
        }
    }
}

fn load_pbr_texture_from_file(path: &Path) -> Option<u32> {
    let rgba = match image::open(path) {
        Ok(img) => img.into_rgba8(),
        Err(error) => {
            crate::engine::diagnostics::report(
                "WARNING",
                "Asset",
                format!("Failed to load PBR texture {}: {error}", path.display()),
            );
            return None;
        }
    };
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let mut texture = 0;
    unsafe {
        gl::GenTextures(1, &mut texture);
        gl::BindTexture(gl::TEXTURE_2D, texture);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::SRGB8_ALPHA8 as i32,
            width as i32,
            height as i32,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            rgba.as_raw().as_ptr() as *const _,
        );
        gl::GenerateMipmap(gl::TEXTURE_2D);
        gl::TexParameteri(
            gl::TEXTURE_2D,
            gl::TEXTURE_MIN_FILTER,
            gl::LINEAR_MIPMAP_LINEAR as i32,
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::REPEAT as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::REPEAT as i32);
        gl::BindTexture(gl::TEXTURE_2D, 0);
    }
    Some(texture)
}

pub(super) fn load_cubemap_from_file(path: &str) -> Option<u32> {
    match image::open(path) {
        Ok(image) => {
            let (width, height) = image.dimensions();

            if width != height * 4 / 3 {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Asset",
                    format!(
                        "Invalid cubemap {path} dimensions for cross layout: {}x{}. Expected 4:3 ratio.",
                        width, height
                    ),
                );

                return None;
            }

            let face_size = height / 3;

            let rgba_image = image.to_rgba8();

            let mut texture = 0;

            unsafe {
                gl::GenTextures(1, &mut texture);

                gl::BindTexture(gl::TEXTURE_CUBE_MAP, texture);
            }

            let faces = [
                (0, 1, gl::TEXTURE_CUBE_MAP_POSITIVE_X),
                (2, 1, gl::TEXTURE_CUBE_MAP_NEGATIVE_X),
                (1, 0, gl::TEXTURE_CUBE_MAP_POSITIVE_Y),
                (1, 2, gl::TEXTURE_CUBE_MAP_NEGATIVE_Y),
                (3, 1, gl::TEXTURE_CUBE_MAP_POSITIVE_Z),
                (1, 1, gl::TEXTURE_CUBE_MAP_NEGATIVE_Z),
            ];

            for (column, row, target) in faces {
                let x = column * face_size;

                let y = row * face_size;

                let face = rgba_image.view(x, y, face_size, face_size).to_image();

                unsafe {
                    gl::TexImage2D(
                        target,
                        0,
                        gl::RGBA as i32,
                        face_size as i32,
                        face_size as i32,
                        0,
                        gl::RGBA,
                        gl::UNSIGNED_BYTE,
                        face.as_raw().as_ptr() as *const _,
                    );
                }
            }

            unsafe {
                gl::TexParameteri(
                    gl::TEXTURE_CUBE_MAP,
                    gl::TEXTURE_MIN_FILTER,
                    gl::LINEAR as i32,
                );

                gl::TexParameteri(
                    gl::TEXTURE_CUBE_MAP,
                    gl::TEXTURE_MAG_FILTER,
                    gl::LINEAR as i32,
                );

                gl::TexParameteri(
                    gl::TEXTURE_CUBE_MAP,
                    gl::TEXTURE_WRAP_S,
                    gl::CLAMP_TO_EDGE as i32,
                );

                gl::TexParameteri(
                    gl::TEXTURE_CUBE_MAP,
                    gl::TEXTURE_WRAP_T,
                    gl::CLAMP_TO_EDGE as i32,
                );

                gl::TexParameteri(
                    gl::TEXTURE_CUBE_MAP,
                    gl::TEXTURE_WRAP_R,
                    gl::CLAMP_TO_EDGE as i32,
                );

                gl::BindTexture(gl::TEXTURE_CUBE_MAP, 0);
            }

            Some(texture)
        }

        Err(error) => {
            crate::engine::diagnostics::report(
                "WARNING",
                "Asset",
                format!("Failed to load cubemap {path}: {error:?}"),
            );

            None
        }
    }
}

fn load_default_sky_reflection_cubemap(
    gradient: &crate::project::sky_material::SkyGradient,
) -> Option<u32> {
    if !gl::GenTextures::is_loaded() {
        return None;
    }
    const SIZE: usize = 64;
    let mut texture = 0_u32;
    unsafe {
        gl::GenTextures(1, &mut texture);
        if texture == 0 {
            return None;
        }
        gl::BindTexture(gl::TEXTURE_CUBE_MAP, texture);
        for (face, target) in [
            gl::TEXTURE_CUBE_MAP_POSITIVE_X,
            gl::TEXTURE_CUBE_MAP_NEGATIVE_X,
            gl::TEXTURE_CUBE_MAP_POSITIVE_Y,
            gl::TEXTURE_CUBE_MAP_NEGATIVE_Y,
            gl::TEXTURE_CUBE_MAP_POSITIVE_Z,
            gl::TEXTURE_CUBE_MAP_NEGATIVE_Z,
        ]
        .into_iter()
        .enumerate()
        {
            let mut pixels = vec![0_u8; SIZE * SIZE * 3];
            for row in 0..SIZE {
                for column in 0..SIZE {
                    let u = 2.0 * (column as f32 + 0.5) / SIZE as f32 - 1.0;
                    let v = 2.0 * (row as f32 + 0.5) / SIZE as f32 - 1.0;
                    let direction: [f32; 3] = match face {
                        0 => [1.0, -v, -u],
                        1 => [-1.0, -v, u],
                        2 => [u, 1.0, v],
                        3 => [u, -1.0, -v],
                        4 => [u, -v, 1.0],
                        _ => [-u, -v, -1.0],
                    };
                    let length = (direction[0] * direction[0]
                        + direction[1] * direction[1]
                        + direction[2] * direction[2])
                        .sqrt();
                    let color = gradient.sample_direction(direction[1] / length.max(0.00001));
                    let index = (row * SIZE + column) * 3;
                    for channel in 0..3 {
                        pixels[index + channel] =
                            (color[channel].clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                }
            }
            gl::TexImage2D(
                target,
                0,
                gl::RGB8 as i32,
                SIZE as i32,
                SIZE as i32,
                0,
                gl::RGB,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
        }
        for wrap in [gl::TEXTURE_WRAP_S, gl::TEXTURE_WRAP_T, gl::TEXTURE_WRAP_R] {
            gl::TexParameteri(gl::TEXTURE_CUBE_MAP, wrap, gl::CLAMP_TO_EDGE as i32);
        }
        gl::TexParameteri(
            gl::TEXTURE_CUBE_MAP,
            gl::TEXTURE_MIN_FILTER,
            gl::LINEAR as i32,
        );
        gl::TexParameteri(
            gl::TEXTURE_CUBE_MAP,
            gl::TEXTURE_MAG_FILTER,
            gl::LINEAR as i32,
        );
        gl::BindTexture(gl::TEXTURE_CUBE_MAP, 0);
    }
    Some(texture)
}
