use std::ffi::CString;
use std::path::Path;
// Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
// Track binds only inside one uninterrupted draw pass, never across GL users.
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

// Worldkiln 0.8.5 | WO MAT-023 | 2026-10-10 16:17 -05:00
// Sky assets resolve against the active project before built-ins, never
// against the editor process working directory. Reject upward traversal.
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
        let mut entity_mesh_cache = self.entity_mesh_cache.borrow_mut();
        for mesh in entity_mesh_cache.values() {
            unsafe { mesh.free_gl_resources() };
        }
        entity_mesh_cache.clear();

        *self.project_assets_dir.borrow_mut() = project_path.map(|path| path.join(".assets"));
        // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
        // A project switch invalidates all resolved material handles. Include
        // the implicit Block_tx fallback so a project override can win on first
        // resolution without any subsequent per-frame filesystem probe.
        self.invalidate_texture_cache();
        // Worldkiln 0.8.5 | WO MAT-023 | 2026-10-10 16:17 -05:00
        // Identically named skyboxes in different projects must not reuse
        // the previous project's GPU handle, even if World revisions match.
        for (_, cubemap) in self.cubemaps.borrow_mut().drain() {
            if cubemap != 0 && gl::DeleteTextures::is_loaded() {
                unsafe { gl::DeleteTextures(1, &cubemap) };
            }
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
    // Only project loads, explicit asset refresh/import, rename or deletion
    // should invalidate material resolution. Cached misses are valid until then.
    pub fn invalidate_texture_cache(&self) {
        self.textures.borrow_mut().clear();
    }

    pub fn get_texture(&self, identifier: &str) -> u32 {
        if identifier.is_empty() {
            return self.fallback_tex;
        }

        // Cache successes, built-in fallback and missing identifiers alike.
        // Normal frame submission performs one in-memory lookup with no stat()
        // calls, allocations for paths, or wall-clock retry deadlines.
        if let Some(&texture) = self.textures.borrow().get(identifier) {
            return texture;
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

        // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 08:20 -05:00
        // Block_tx is the canonical synthesized fallback for ordinary Cells.
        // Its absence is expected, not a missing user asset; project overrides
        // still won if a matching file existed above.
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

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    pub fn get_texture_file(&self, path: &Path) -> u32 {
        let cache_key = format!("file::{}", path.to_string_lossy().to_ascii_lowercase());
        if let Some(&texture) = self.textures.borrow().get(&cache_key) {
            return texture;
        }
        // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
        // Missing file textures are also resolved once until an asset refresh.
        let texture = load_texture_from_file(path).unwrap_or(self.fallback_tex);
        self.textures.borrow_mut().insert(cache_key, texture);
        texture
    }

    pub fn get_cubemap(&self, identifier: &str) -> u32 {
        let mut cubemaps = self.cubemaps.borrow_mut();

        if let Some(&tex) = cubemaps.get(identifier) {
            return tex;
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

    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
    // Worldkiln 0.8.5 | WO MAT-023 | 2026-10-10 16:17 -05:00
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
        assert!(texture_binding_changed(&mut last, 0)); // procedural grass
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
        // A repeated draw never re-scans disk or emits another warning.
        assert_eq!(renderer.get_texture(name), renderer.fallback_tex);
        renderer.invalidate_texture_cache();
        assert!(!renderer.textures.borrow().contains_key(name));
        // Switching projects must likewise invalidate an already-resolved
        // identifier before a new project override is considered.
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

        // Position.
        gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
        gl::EnableVertexAttribArray(0);

        // Normal.
        gl::VertexAttribPointer(
            1,
            3,
            gl::FLOAT,
            gl::FALSE,
            stride,
            (3 * std::mem::size_of::<f32>()) as *const _,
        );
        gl::EnableVertexAttribArray(1);

        // Color.
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

    let color = [0.0, 1.0, 1.0, 1.0];

    let radius = 0.15;
    let center = [0.0, 0.0, 0.0];

    let cross_size = 0.3;

    add_line(
        &mut vertices,
        [-cross_size, 0.0, 0.0],
        [cross_size, 0.0, 0.0],
        color,
    );

    add_line(
        &mut vertices,
        [0.0, -cross_size, 0.0],
        [0.0, cross_size, 0.0],
        color,
    );

    add_line(
        &mut vertices,
        [0.0, 0.0, -cross_size],
        [0.0, 0.0, cross_size],
        color,
    );

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

pub(super) fn load_texture_from_file(path: impl AsRef<Path>) -> Option<u32> {
    let path = path.as_ref();

    match image::open(path) {
        Ok(image) => {
            let rgba = image.to_rgba8();

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

            Some(texture)
        }

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
