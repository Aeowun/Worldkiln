use glam::{Mat4, Vec3};
use std::collections::HashMap;

use super::RenderMode;
use crate::renderer::mesh::{BLOCK_VERTEX_FLOATS, CubeFace, add_block_quad};
use crate::world::{CHUNK_SIZE, CellType, ChunkCoord, DirtyReason, World, WorldCoord};

#[derive(Clone, Debug)]
pub struct TextureBatchRange {
    pub texture_id: String,
    pub start_vertex: i32,
    pub vertex_count: i32,
}

pub struct ChunkMesh {
    pub chunk_coord: ChunkCoord,
    pub main_vao: u32,
    pub main_vbo: u32,
    pub main_texture_batches: Vec<TextureBatchRange>,

    pub shadow_vao: u32,
    pub shadow_vbo: u32,
    pub shadow_vertex_count: i32,
}

impl ChunkMesh {
    pub fn free_gl_resources(&mut self) {
        if self.main_vao == 0 && self.main_vbo == 0 && self.shadow_vao == 0 && self.shadow_vbo == 0
        {
            return;
        }
        unsafe {
            if self.main_vao != 0 {
                gl::DeleteVertexArrays(1, &self.main_vao);
                self.main_vao = 0;
            }
            if self.main_vbo != 0 {
                gl::DeleteBuffers(1, &self.main_vbo);
                self.main_vbo = 0;
            }
            if self.shadow_vao != 0 {
                gl::DeleteVertexArrays(1, &self.shadow_vao);
                self.shadow_vao = 0;
            }
            if self.shadow_vbo != 0 {
                gl::DeleteBuffers(1, &self.shadow_vbo);
                self.shadow_vbo = 0;
            }
        }
    }
}

impl Drop for ChunkMesh {
    fn drop(&mut self) {
        self.free_gl_resources();
    }
}

pub struct GhostChunkMesh {
    pub chunk_coord: ChunkCoord,
    pub vao: u32,
    pub vbo: u32,
    pub vertex_count: i32,
}

impl GhostChunkMesh {
    pub fn free_gl_resources(&mut self) {
        unsafe {
            if self.vao != 0 {
                gl::DeleteVertexArrays(1, &self.vao);
                self.vao = 0;
            }

            if self.vbo != 0 {
                gl::DeleteBuffers(1, &self.vbo);
                self.vbo = 0;
            }
        }
    }
}

impl Drop for GhostChunkMesh {
    fn drop(&mut self) {
        self.free_gl_resources();
    }
}

#[derive(Clone, Debug, Default)]
pub struct CpuChunkData {
    pub main_texture_vertices: HashMap<String, Vec<f32>>,
    pub shadow_positions: Vec<f32>,
}

#[derive(Clone, Debug, Default)]
pub struct CpuGhostChunkData {
    pub vertices: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
struct MainFaceInfo {
    texture: String,
    color: Vec3,
    visual_offset: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
struct GhostFaceInfo {
    color: Vec3,
    visual_offset: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
struct ShadowFaceInfo {
    visual_offset: Vec3,
}

#[inline(always)]
fn chunk_local_index(lx: i32, ly: i32, lz: i32) -> usize {
    (lx as usize) | ((ly as usize) << 4) | ((lz as usize) << 8)
}

pub fn build_cpu_chunk_data(
    world: &World,
    chunk_coord: ChunkCoord,
    coords_in_chunk: &[WorldCoord],
    mode: RenderMode,
) -> CpuChunkData {
    let mut main_texture_vertices: HashMap<String, Vec<f32>> = HashMap::new();
    let mut shadow_positions: Vec<f32> = Vec::new();

    if coords_in_chunk.is_empty() {
        return CpuChunkData::default();
    }

    let mut main_grid: Vec<Option<(u8, MainFaceInfo)>> = vec![None; 4096];
    let mut shadow_grid: Vec<Option<(u8, ShadowFaceInfo)>> = vec![None; 4096];

    for &coord in coords_in_chunk {
        let Some(cell) = world.get_effective_cell(coord) else {
            continue;
        };

        let (lx, ly, lz) = ChunkCoord::local_offset(coord);

        let should_draw = match mode {
            RenderMode::Editor => true,
            RenderMode::Game => world.is_cell_anchored(coord),
        };

        if !should_draw {
            continue;
        }

        let idx = chunk_local_index(lx, ly, lz);

        // --- Main Pass Static Geometry ---
        let main_renderable = matches!(
            cell.cell_type,
            CellType::Block | CellType::SpawnPoint | CellType::Light
        );

        if main_renderable && world.is_cell_visible(coord) {
            let mask = crate::renderer::mesh::compute_exposed_faces_main(world, coord, mode);
            if mask != 0 {
                let color = world.get_effective_color(coord);
                let visual_offset = world.get_visual_offset(coord);
                let info = MainFaceInfo {
                    texture: cell.texture.clone(),
                    color,
                    visual_offset,
                };
                main_grid[idx] = Some((mask, info));
            }
        }

        // --- Shadow Pass Static Geometry ---
        let shadow_renderable = world.lighting.shadows_enabled
            && matches!(cell.cell_type, CellType::Block | CellType::SpawnPoint);

        if shadow_renderable && world.is_cell_visible(coord) && world.is_cell_solid(coord) {
            let mask = crate::renderer::mesh::compute_exposed_faces_shadow(world, coord, mode);
            if mask != 0 {
                let visual_offset = world.get_visual_offset(coord);
                let info = ShadowFaceInfo { visual_offset };
                shadow_grid[idx] = Some((mask, info));
            }
        }
    }

    greedy_mesh_main_pass(&main_grid, &mut main_texture_vertices);
    greedy_mesh_shadow_pass(&shadow_grid, &mut shadow_positions);

    CpuChunkData {
        main_texture_vertices,
        shadow_positions,
    }
}

pub fn build_cpu_ghost_chunk_data(
    world: &World,
    _chunk_coord: ChunkCoord,
    coords_in_chunk: &[WorldCoord],
) -> CpuGhostChunkData {
    if coords_in_chunk.is_empty() {
        return CpuGhostChunkData::default();
    }

    let mut ghost_grid: Vec<Option<(u8, GhostFaceInfo)>> = vec![None; 4096];

    for &coord in coords_in_chunk {
        let Some(cell) = world.get_effective_cell(coord) else {
            continue;
        };

        if !matches!(cell.cell_type, CellType::Block | CellType::SpawnPoint) {
            continue;
        }

        if world.is_cell_visible(coord) {
            continue;
        }

        let (lx, ly, lz) = ChunkCoord::local_offset(coord);
        let idx = chunk_local_index(lx, ly, lz);

        let mask = crate::renderer::mesh::compute_exposed_faces_ghost(world, coord);

        if mask == 0 {
            continue;
        }

        let info = GhostFaceInfo {
            color: world.get_effective_color(coord),
            visual_offset: world.get_visual_offset(coord),
        };

        ghost_grid[idx] = Some((mask, info));
    }

    let mut vertices = Vec::new();

    greedy_mesh_ghost_pass(&ghost_grid, &mut vertices);

    CpuGhostChunkData { vertices }
}

fn map_uvw_to_local_xyz(face: CubeFace, u: i32, v: i32, w: i32) -> (i32, i32, i32) {
    match face {
        CubeFace::Top | CubeFace::Bottom => (v, u, w),
        CubeFace::Front | CubeFace::Back => (v, w, u),
        CubeFace::Left | CubeFace::Right => (u, w, v),
    }
}

fn greedy_mesh_main_pass(
    main_grid: &[Option<(u8, MainFaceInfo)>],
    main_texture_vertices: &mut HashMap<String, Vec<f32>>,
) {
    for face in &CubeFace::ALL {
        let mask_bit = face.mask();

        for u in 0..16 {
            let mut slice: [[Option<MainFaceInfo>; 16]; 16] = Default::default();

            for w in 0..16 {
                for v in 0..16 {
                    let (lx, ly, lz) = map_uvw_to_local_xyz(*face, u, v, w);
                    let idx = chunk_local_index(lx, ly, lz);
                    if let Some((mask, info)) = &main_grid[idx] {
                        if (mask & mask_bit) != 0 {
                            slice[w as usize][v as usize] = Some(info.clone());
                        }
                    }
                }
            }

            let mut visited = [[false; 16]; 16];

            for w in 0..16 {
                for v in 0..16 {
                    if visited[w][v] {
                        continue;
                    }

                    let Some(info) = &slice[w][v] else {
                        continue;
                    };

                    let mut width = 1;
                    while v + width < 16 && !visited[w][v + width] {
                        if let Some(other) = &slice[w][v + width] {
                            if other == info {
                                width += 1;
                                continue;
                            }
                        }
                        break;
                    }

                    let mut height = 1;
                    'outer: while w + height < 16 {
                        for dw in 0..width {
                            let cur_v = v + dw;
                            let cur_w = w + height;
                            if visited[cur_w][cur_v] {
                                break 'outer;
                            }
                            if let Some(other) = &slice[cur_w][cur_v] {
                                if other != info {
                                    break 'outer;
                                }
                            } else {
                                break 'outer;
                            }
                        }
                        height += 1;
                    }

                    for dw in 0..height {
                        for dv in 0..width {
                            visited[w + dw][v + dv] = true;
                        }
                    }

                    let vertices = main_texture_vertices
                        .entry(info.texture.clone())
                        .or_default();

                    emit_merged_main_quad(
                        vertices,
                        *face,
                        u as f32,
                        v as f32,
                        w as f32,
                        width as f32,
                        height as f32,
                        info.color,
                        info.visual_offset,
                    );
                }
            }
        }
    }
}

fn emit_merged_main_quad(
    vertices: &mut Vec<f32>,
    face: CubeFace,
    u: f32,
    v: f32,
    w: f32,
    width: f32,
    height: f32,
    color: Vec3,
    visual_offset: Vec3,
) {
    let (v1, v2, v3, v4, normal) = match face {
        CubeFace::Top => (
            Vec3::new(v, u + 1.0, w + height),
            Vec3::new(v + width, u + 1.0, w + height),
            Vec3::new(v + width, u + 1.0, w),
            Vec3::new(v, u + 1.0, w),
            [0.0, 1.0, 0.0],
        ),
        CubeFace::Bottom => (
            Vec3::new(v, u, w),
            Vec3::new(v + width, u, w),
            Vec3::new(v + width, u, w + height),
            Vec3::new(v, u, w + height),
            [0.0, -1.0, 0.0],
        ),
        CubeFace::Front => (
            Vec3::new(v, w, u + 1.0),
            Vec3::new(v + width, w, u + 1.0),
            Vec3::new(v + width, w + height, u + 1.0),
            Vec3::new(v, w + height, u + 1.0),
            [0.0, 0.0, 1.0],
        ),
        CubeFace::Back => (
            Vec3::new(v + width, w, u),
            Vec3::new(v, w, u),
            Vec3::new(v, w + height, u),
            Vec3::new(v + width, w + height, u),
            [0.0, 0.0, -1.0],
        ),
        CubeFace::Left => (
            Vec3::new(u, w, v),
            Vec3::new(u, w, v + width),
            Vec3::new(u, w + height, v + width),
            Vec3::new(u, w + height, v),
            [-1.0, 0.0, 0.0],
        ),
        CubeFace::Right => (
            Vec3::new(u + 1.0, w, v + width),
            Vec3::new(u + 1.0, w, v),
            Vec3::new(u + 1.0, w + height, v),
            Vec3::new(u + 1.0, w + height, v + width),
            [1.0, 0.0, 0.0],
        ),
    };

    let o1 = (v1 + visual_offset).to_array();
    let o2 = (v2 + visual_offset).to_array();
    let o3 = (v3 + visual_offset).to_array();
    let o4 = (v4 + visual_offset).to_array();

    let color_arr = [color.x, color.y, color.z, 1.0];

    crate::renderer::mesh::add_merged_block_quad(
        vertices, o1, o2, o3, o4, color_arr, normal, width, height,
    );
}

fn greedy_mesh_ghost_pass(ghost_grid: &[Option<(u8, GhostFaceInfo)>], vertices: &mut Vec<f32>) {
    for face in &CubeFace::ALL {
        let mask_bit = face.mask();

        for u in 0..16 {
            let mut slice: [[Option<GhostFaceInfo>; 16]; 16] = Default::default();

            for w in 0..16 {
                for v in 0..16 {
                    let (lx, ly, lz) = map_uvw_to_local_xyz(*face, u, v, w);
                    let idx = chunk_local_index(lx, ly, lz);

                    if let Some((mask, info)) = &ghost_grid[idx] {
                        if (mask & mask_bit) != 0 {
                            slice[w as usize][v as usize] = Some(info.clone());
                        }
                    }
                }
            }

            let mut visited = [[false; 16]; 16];

            for w in 0..16 {
                for v in 0..16 {
                    if visited[w][v] {
                        continue;
                    }

                    let Some(info) = &slice[w][v] else {
                        continue;
                    };

                    let mut width = 1;

                    while v + width < 16 && !visited[w][v + width] {
                        if let Some(other) = &slice[w][v + width] {
                            if other == info {
                                width += 1;
                                continue;
                            }
                        }

                        break;
                    }

                    let mut height = 1;

                    'outer: while w + height < 16 {
                        for dw in 0..width {
                            let cur_v = v + dw;
                            let cur_w = w + height;

                            if visited[cur_w][cur_v] {
                                break 'outer;
                            }

                            if let Some(other) = &slice[cur_w][cur_v] {
                                if other != info {
                                    break 'outer;
                                }
                            } else {
                                break 'outer;
                            }
                        }

                        height += 1;
                    }

                    for dw in 0..height {
                        for dv in 0..width {
                            visited[w + dw][v + dv] = true;
                        }
                    }

                    emit_merged_ghost_quad(
                        vertices,
                        *face,
                        u as f32,
                        v as f32,
                        w as f32,
                        width as f32,
                        height as f32,
                        info.color,
                        info.visual_offset,
                    );
                }
            }
        }
    }
}

fn emit_merged_ghost_quad(
    vertices: &mut Vec<f32>,
    face: CubeFace,
    u: f32,
    v: f32,
    w: f32,
    width: f32,
    height: f32,
    color: Vec3,
    visual_offset: Vec3,
) {
    let (v1, v2, v3, v4, normal) = match face {
        CubeFace::Top => (
            Vec3::new(v, u + 1.0, w + height),
            Vec3::new(v + width, u + 1.0, w + height),
            Vec3::new(v + width, u + 1.0, w),
            Vec3::new(v, u + 1.0, w),
            [0.0, 1.0, 0.0],
        ),
        CubeFace::Bottom => (
            Vec3::new(v, u, w),
            Vec3::new(v + width, u, w),
            Vec3::new(v + width, u, w + height),
            Vec3::new(v, u, w + height),
            [0.0, -1.0, 0.0],
        ),
        CubeFace::Front => (
            Vec3::new(v, w, u + 1.0),
            Vec3::new(v + width, w, u + 1.0),
            Vec3::new(v + width, w + height, u + 1.0),
            Vec3::new(v, w + height, u + 1.0),
            [0.0, 0.0, 1.0],
        ),
        CubeFace::Back => (
            Vec3::new(v + width, w, u),
            Vec3::new(v, w, u),
            Vec3::new(v, w + height, u),
            Vec3::new(v + width, w + height, u),
            [0.0, 0.0, -1.0],
        ),
        CubeFace::Left => (
            Vec3::new(u, w, v),
            Vec3::new(u, w, v + width),
            Vec3::new(u, w + height, v + width),
            Vec3::new(u, w + height, v),
            [-1.0, 0.0, 0.0],
        ),
        CubeFace::Right => (
            Vec3::new(u + 1.0, w, v + width),
            Vec3::new(u + 1.0, w, v),
            Vec3::new(u + 1.0, w + height, v),
            Vec3::new(u + 1.0, w + height, v + width),
            [1.0, 0.0, 0.0],
        ),
    };

    let o1 = (v1 + visual_offset).to_array();
    let o2 = (v2 + visual_offset).to_array();
    let o3 = (v3 + visual_offset).to_array();
    let o4 = (v4 + visual_offset).to_array();

    let color_arr = [color.x, color.y, color.z, 1.0];

    crate::renderer::mesh::add_merged_block_quad(
        vertices, o1, o2, o3, o4, color_arr, normal, width, height,
    );
}

fn greedy_mesh_shadow_pass(
    shadow_grid: &[Option<(u8, ShadowFaceInfo)>],
    shadow_positions: &mut Vec<f32>,
) {
    for face in &CubeFace::ALL {
        let mask_bit = face.mask();

        for u in 0..16 {
            let mut slice: [[Option<ShadowFaceInfo>; 16]; 16] = Default::default();

            for w in 0..16 {
                for v in 0..16 {
                    let (lx, ly, lz) = map_uvw_to_local_xyz(*face, u, v, w);
                    let idx = chunk_local_index(lx, ly, lz);
                    if let Some((mask, info)) = &shadow_grid[idx] {
                        if (mask & mask_bit) != 0 {
                            slice[w as usize][v as usize] = Some(info.clone());
                        }
                    }
                }
            }

            let mut visited = [[false; 16]; 16];

            for w in 0..16 {
                for v in 0..16 {
                    if visited[w][v] {
                        continue;
                    }

                    let Some(info) = &slice[w][v] else {
                        continue;
                    };

                    let mut width = 1;
                    while v + width < 16 && !visited[w][v + width] {
                        if let Some(other) = &slice[w][v + width] {
                            if other == info {
                                width += 1;
                                continue;
                            }
                        }
                        break;
                    }

                    let mut height = 1;
                    'outer: while w + height < 16 {
                        for dw in 0..width {
                            let cur_v = v + dw;
                            let cur_w = w + height;
                            if visited[cur_w][cur_v] {
                                break 'outer;
                            }
                            if let Some(other) = &slice[cur_w][cur_v] {
                                if other != info {
                                    break 'outer;
                                }
                            } else {
                                break 'outer;
                            }
                        }
                        height += 1;
                    }

                    for dw in 0..height {
                        for dv in 0..width {
                            visited[w + dw][v + dv] = true;
                        }
                    }

                    emit_merged_shadow_quad(
                        shadow_positions,
                        *face,
                        u as f32,
                        v as f32,
                        w as f32,
                        width as f32,
                        height as f32,
                        info.visual_offset,
                    );
                }
            }
        }
    }
}

fn emit_merged_shadow_quad(
    vertices: &mut Vec<f32>,
    face: CubeFace,
    u: f32,
    v: f32,
    w: f32,
    width: f32,
    height: f32,
    visual_offset: Vec3,
) {
    let (v1, v2, v3, v4) = match face {
        CubeFace::Top => (
            Vec3::new(v, u + 1.0, w + height),
            Vec3::new(v + width, u + 1.0, w + height),
            Vec3::new(v + width, u + 1.0, w),
            Vec3::new(v, u + 1.0, w),
        ),
        CubeFace::Bottom => (
            Vec3::new(v, u, w),
            Vec3::new(v + width, u, w),
            Vec3::new(v + width, u, w + height),
            Vec3::new(v, u, w + height),
        ),
        CubeFace::Front => (
            Vec3::new(v, w, u + 1.0),
            Vec3::new(v + width, w, u + 1.0),
            Vec3::new(v + width, w + height, u + 1.0),
            Vec3::new(v, w + height, u + 1.0),
        ),
        CubeFace::Back => (
            Vec3::new(v + width, w, u),
            Vec3::new(v, w, u),
            Vec3::new(v, w + height, u),
            Vec3::new(v + width, w + height, u),
        ),
        CubeFace::Left => (
            Vec3::new(u, w, v),
            Vec3::new(u, w, v + width),
            Vec3::new(u, w + height, v + width),
            Vec3::new(u, w + height, v),
        ),
        CubeFace::Right => (
            Vec3::new(u + 1.0, w, v + width),
            Vec3::new(u + 1.0, w, v),
            Vec3::new(u + 1.0, w + height, v),
            Vec3::new(u + 1.0, w + height, v + width),
        ),
    };

    let o1 = (v1 + visual_offset).to_array();
    let o2 = (v2 + visual_offset).to_array();
    let o3 = (v3 + visual_offset).to_array();
    let o4 = (v4 + visual_offset).to_array();

    // Triangle 1
    vertices.extend_from_slice(&o1);
    vertices.extend_from_slice(&o2);
    vertices.extend_from_slice(&o3);

    // Triangle 2
    vertices.extend_from_slice(&o1);
    vertices.extend_from_slice(&o3);
    vertices.extend_from_slice(&o4);
}

pub fn upload_position_only_vertices(vertices: &[f32]) -> (u32, u32, i32) {
    let mut vao = 0;
    let mut vbo = 0;

    unsafe {
        gl::GenVertexArrays(1, &mut vao);
        gl::GenBuffers(1, &mut vbo);

        gl::BindVertexArray(vao);
        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

        gl::BufferData(
            gl::ARRAY_BUFFER,
            (vertices.len() * std::mem::size_of::<f32>()) as isize,
            vertices.as_ptr() as *const _,
            gl::STATIC_DRAW,
        );

        let stride = (3 * std::mem::size_of::<f32>()) as i32;

        // Position at location 0.
        gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
        gl::EnableVertexAttribArray(0);

        gl::BindVertexArray(0);
    }

    (vao, vbo, (vertices.len() / 3) as i32)
}

#[derive(Clone, Copy, Debug)]
pub struct FrustumPlane {
    pub normal: Vec3,
    pub d: f32,
}

impl FrustumPlane {
    pub fn new(a: f32, b: f32, c: f32, d: f32) -> Self {
        let length = Vec3::new(a, b, c).length();
        if length > 0.00001 {
            Self {
                normal: Vec3::new(a / length, b / length, c / length),
                d: d / length,
            }
        } else {
            Self {
                normal: Vec3::ZERO,
                d: 0.0,
            }
        }
    }

    pub fn distance_to_point(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.d
    }
}

pub struct CameraFrustum {
    pub planes: [FrustumPlane; 6],
}

impl CameraFrustum {
    pub fn from_view_projection(vp: Mat4) -> Self {
        let m = vp.to_cols_array_2d();

        let row0 = [m[0][0], m[1][0], m[2][0], m[3][0]];
        let row1 = [m[0][1], m[1][1], m[2][1], m[3][1]];
        let row2 = [m[0][2], m[1][2], m[2][2], m[3][2]];
        let row3 = [m[0][3], m[1][3], m[2][3], m[3][3]];

        let left = FrustumPlane::new(
            row3[0] + row0[0],
            row3[1] + row0[1],
            row3[2] + row0[2],
            row3[3] + row0[3],
        );
        let right = FrustumPlane::new(
            row3[0] - row0[0],
            row3[1] - row0[1],
            row3[2] - row0[2],
            row3[3] - row0[3],
        );
        let bottom = FrustumPlane::new(
            row3[0] + row1[0],
            row3[1] + row1[1],
            row3[2] + row1[2],
            row3[3] + row1[3],
        );
        let top = FrustumPlane::new(
            row3[0] - row1[0],
            row3[1] - row1[1],
            row3[2] - row1[2],
            row3[3] - row1[3],
        );
        let near = FrustumPlane::new(
            row3[0] + row2[0],
            row3[1] + row2[1],
            row3[2] + row2[2],
            row3[3] + row2[3],
        );
        let far = FrustumPlane::new(
            row3[0] - row2[0],
            row3[1] - row2[1],
            row3[2] - row2[2],
            row3[3] - row2[3],
        );

        Self {
            planes: [left, right, bottom, top, near, far],
        }
    }

    pub fn intersects_aabb(&self, min: Vec3, max: Vec3) -> bool {
        for plane in &self.planes {
            let p = Vec3::new(
                if plane.normal.x >= 0.0 { max.x } else { min.x },
                if plane.normal.y >= 0.0 { max.y } else { min.y },
                if plane.normal.z >= 0.0 { max.z } else { min.z },
            );

            if plane.distance_to_point(p) < 0.0 {
                return false;
            }
        }
        true
    }
}

pub fn expand_dirty_coords_to_chunks(
    dirty_coords: &HashMap<WorldCoord, DirtyReason>,
) -> std::collections::HashSet<ChunkCoord> {
    let mut dirty_chunks = std::collections::HashSet::new();

    for (&coord, &reason) in dirty_coords {
        let own_chunk = ChunkCoord::from_world_coord(coord);
        dirty_chunks.insert(own_chunk);

        if reason == DirtyReason::Geometry {
            for face in &CubeFace::ALL {
                let neighbor_coord = face.neighbor_coord(coord);
                let neighbor_chunk = ChunkCoord::from_world_coord(neighbor_coord);
                dirty_chunks.insert(neighbor_chunk);
            }
        }
    }

    dirty_chunks
}

#[cfg(test)]
mod tests {
    use super::RenderMode;
    use super::*;
    use crate::world::{CellType, World, WorldCoord};

    #[test]
    fn test_chunk_coord_mapping_positive() {
        let c0 = WorldCoord::new(0, 0, 0);
        let chunk0 = ChunkCoord::from_world_coord(c0);
        assert_eq!(chunk0, ChunkCoord::new(0, 0, 0));
        assert_eq!(ChunkCoord::local_offset(c0), (0, 0, 0));

        let c15 = WorldCoord::new(15, 15, 15);
        let chunk15 = ChunkCoord::from_world_coord(c15);
        assert_eq!(chunk15, ChunkCoord::new(0, 0, 0));
        assert_eq!(ChunkCoord::local_offset(c15), (15, 15, 15));

        let c16 = WorldCoord::new(16, 0, 0);
        let chunk16 = ChunkCoord::from_world_coord(c16);
        assert_eq!(chunk16, ChunkCoord::new(1, 0, 0));
        assert_eq!(ChunkCoord::local_offset(c16), (0, 0, 0));
    }

    #[test]
    fn test_chunk_coord_mapping_negative() {
        let c_neg1 = WorldCoord::new(-1, 0, 0);
        let chunk_neg1 = ChunkCoord::from_world_coord(c_neg1);
        assert_eq!(chunk_neg1, ChunkCoord::new(-1, 0, 0));
        assert_eq!(ChunkCoord::local_offset(c_neg1), (15, 0, 0));

        let c_neg16 = WorldCoord::new(-16, 0, 0);
        let chunk_neg16 = ChunkCoord::from_world_coord(c_neg16);
        assert_eq!(chunk_neg16, ChunkCoord::new(-1, 0, 0));
        assert_eq!(ChunkCoord::local_offset(c_neg16), (0, 0, 0));

        let c_neg17 = WorldCoord::new(-17, 0, 0);
        let chunk_neg17 = ChunkCoord::from_world_coord(c_neg17);
        assert_eq!(chunk_neg17, ChunkCoord::new(-2, 0, 0));
        assert_eq!(ChunkCoord::local_offset(c_neg17), (15, 0, 0));
    }

    #[test]
    fn test_chunk_boundary_occlusion() {
        let mut world = World::new();
        let c1 = WorldCoord::new(15, 0, 0); // Chunk (0,0,0)
        let c2 = WorldCoord::new(16, 0, 0); // Chunk (1,0,0)
        world.set_cell(c1, CellType::Block);
        world.set_cell(c2, CellType::Block);

        let mask1 =
            crate::renderer::mesh::compute_exposed_faces_main(&world, c1, RenderMode::Editor);
        let mask2 =
            crate::renderer::mesh::compute_exposed_faces_main(&world, c2, RenderMode::Editor);

        // Right face (+X) of c1 must be occluded
        assert_eq!(mask1 & CubeFace::Right.mask(), 0);
        // Left face (-X) of c2 must be occluded
        assert_eq!(mask2 & CubeFace::Left.mask(), 0);
    }

    #[test]
    fn test_texture_batching_inside_chunk() {
        let mut world = World::new();
        let c1 = WorldCoord::new(0, 0, 0);
        let c2 = WorldCoord::new(1, 0, 0);

        world.set_cell(c1, CellType::Block);
        world.set_cell(c2, CellType::Block);

        world.get_mut(c1).unwrap().texture = "Block_tx".to_string();
        world.get_mut(c2).unwrap().texture = "brick".to_string();

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &[c1, c2], RenderMode::Editor);

        assert_eq!(cpu_data.main_texture_vertices.len(), 2);
        assert!(cpu_data.main_texture_vertices.contains_key("Block_tx"));
        assert!(cpu_data.main_texture_vertices.contains_key("brick"));
    }

    #[test]
    fn test_shadow_mesh_eligibility() {
        let mut world = World::new();
        let c1 = WorldCoord::new(0, 0, 0);
        let c2 = WorldCoord::new(1, 0, 0);
        let c3 = WorldCoord::new(2, 0, 0);

        world.set_cell(c1, CellType::Block);
        world.set_cell(c2, CellType::SpawnPoint);
        world.set_cell(c3, CellType::Light);
        world.set_cell_visible_runtime(c3, true);

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &[c1, c2, c3], RenderMode::Editor);

        // c1 and c2 cast shadows (Block, SpawnPoint). c3 (Light) does NOT.
        // c1 and c2 are adjacent along X axis.
        // Greedy meshing merges Top, Bottom, Front, Back faces into 4 quads of 2x1, plus 2 end quads (Left, Right) -> 6 quads * 6 vertices * 3 floats = 108 floats.
        assert_eq!(cpu_data.shadow_positions.len(), 108);
    }

    #[test]
    fn test_ghost_chunk_data_is_generated_for_invisible_cells() {
        let mut world = World::new();

        let coord = WorldCoord::new(0, 0, 0);

        world.set_cell(coord, CellType::Block);
        world.set_cell_visible_runtime(coord, false);

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_ghost_chunk_data(&world, chunk_coord, &[coord]);

        assert_eq!(cpu_data.vertices.len(), 36 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_ghost_greedy_meshing_2x2_plane() {
        let mut world = World::new();
        let mut coords = Vec::new();

        for x in 0..2 {
            for z in 0..2 {
                let coord = WorldCoord::new(x, 0, z);

                world.set_cell(coord, CellType::Block);
                world.set_cell_visible_runtime(coord, false);

                coords.push(coord);
            }
        }

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_ghost_chunk_data(&world, chunk_coord, &coords);

        assert_eq!(cpu_data.vertices.len(), 36 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_frustum_culling_aabb() {
        let proj =
            glam::camera::rh::proj::opengl::perspective(60.0_f32.to_radians(), 1.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
        let vp = proj * view;

        let frustum = CameraFrustum::from_view_projection(vp);

        let chunk_in_front = ChunkCoord::new(0, 0, 0);
        let (min1, max1) = chunk_in_front.aabb_min_max();
        assert!(frustum.intersects_aabb(min1, max1));

        let chunk_far_away = ChunkCoord::new(100, 100, 100);
        let (min2, max2) = chunk_far_away.aabb_min_max();
        assert!(!frustum.intersects_aabb(min2, max2));
    }

    #[test]
    fn test_dirty_expansion_geometry_reason() {
        let mut dirty_coords = HashMap::new();
        let c = WorldCoord::new(0, 0, 0);
        dirty_coords.insert(c, crate::world::DirtyReason::Geometry);

        let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);

        // Own chunk is (0,0,0)
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, 0, 0)));
        // +Y (0,1,0) -> (0,0,0), -Y (0,-1,0) -> (0,-1,0), +Z (0,0,1) -> (0,0,0), -Z (0,0,-1) -> (0,0,-1), -X (-1,0,0) -> (-1,0,0), +X (1,0,0) -> (0,0,0)
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, -1, 0)));
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, 0, -1)));
        assert!(dirty_chunks.contains(&ChunkCoord::new(-1, 0, 0)));
    }

    #[test]
    fn test_dirty_expansion_material_reason() {
        let mut dirty_coords = HashMap::new();
        let c = WorldCoord::new(0, 0, 0);
        dirty_coords.insert(c, crate::world::DirtyReason::MaterialOrOffset);

        let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);

        // Material or offset changes invalidate ONLY own chunk
        assert_eq!(dirty_chunks.len(), 1);
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, 0, 0)));
    }

    #[test]
    fn test_boundary_invalidation_across_chunks() {
        let mut dirty_coords = HashMap::new();
        let boundary_c = WorldCoord::new(15, 0, 0); // At edge of Chunk (0,0,0)
        dirty_coords.insert(boundary_c, crate::world::DirtyReason::Geometry);

        let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);

        // Own chunk is (0,0,0)
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, 0, 0)));
        // +X neighbor is (16,0,0) -> Chunk (1,0,0)
        assert!(dirty_chunks.contains(&ChunkCoord::new(1, 0, 0)));
    }

    #[test]
    fn test_runtime_movement_invalidates_old_and_new() {
        let mut world = World::new();
        let id = world.create_runtime_cell(CellType::Block);
        world
            .move_runtime_cell(id, WorldCoord::new(15, 0, 0))
            .unwrap();

        // Drain first move
        let _ = world.drain_render_dirty_cells();

        // Move across chunk boundary
        world
            .move_runtime_cell(id, WorldCoord::new(16, 0, 0))
            .unwrap();

        let dirty_coords = world.drain_render_dirty_cells();
        let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);

        // Must invalidate both old chunk (0,0,0) and new chunk (1,0,0)
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, 0, 0)));
        assert!(dirty_chunks.contains(&ChunkCoord::new(1, 0, 0)));
    }

    #[test]
    fn test_multiple_mutations_same_chunk_deduplicate() {
        let mut dirty_coords = HashMap::new();
        for x in 0..10 {
            for y in 0..10 {
                dirty_coords.insert(
                    WorldCoord::new(x, y, 0),
                    crate::world::DirtyReason::Geometry,
                );
            }
        }

        let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);
        // All coords are in interior or edges of Chunk (0,0,0). Deduplication ensures small chunk set.
        assert!(dirty_chunks.contains(&ChunkCoord::new(0, 0, 0)));
    }

    #[test]
    fn test_greedy_meshing_single_voxel() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &[coord], RenderMode::Editor);

        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();
        // 1 isolated voxel has 6 exposed faces = 6 quads = 36 vertices = 432 floats
        assert_eq!(verts.len(), 36 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_greedy_meshing_2x2_plane() {
        let mut world = World::new();
        let mut coords = Vec::new();
        for x in 0..2 {
            for z in 0..2 {
                let c = WorldCoord::new(x, 0, z);
                world.set_cell(c, CellType::Block);
                coords.push(c);
            }
        }

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &coords, RenderMode::Editor);
        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();

        // 2x2 flat top faces merge into 1 quad! (6 verts)
        // 2x2 flat bottom faces merge into 1 quad! (6 verts)
        // 4 side faces (each 2x1) merge into 4 quads! (24 verts)
        // Total quads = 1 + 1 + 4 = 6 quads = 36 vertices = 432 floats.
        // Compare unmerged: 4 blocks * 5 faces = 20 quads = 120 vertices = 1440 floats!
        assert_eq!(verts.len(), 36 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_greedy_meshing_4x4_plane() {
        let mut world = World::new();
        let mut coords = Vec::new();
        for x in 0..4 {
            for z in 0..4 {
                let c = WorldCoord::new(x, 0, z);
                world.set_cell(c, CellType::Block);
                coords.push(c);
            }
        }

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &coords, RenderMode::Editor);
        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();

        // Top: 4x4 merge -> 1 quad (6 verts)
        // Bottom: 4x4 merge -> 1 quad (6 verts)
        // 4 sides (each 4x1) -> 4 quads (24 verts)
        // Total = 36 vertices. Unmerged would be 16 blocks * 5 faces = 80 quads = 480 vertices!
        assert_eq!(verts.len(), 36 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_greedy_meshing_full_16x16_layer() {
        let mut world = World::new();
        let mut coords = Vec::new();
        for x in 0..16 {
            for z in 0..16 {
                let c = WorldCoord::new(x, 0, z);
                world.set_cell(c, CellType::Block);
                coords.push(c);
            }
        }

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &coords, RenderMode::Editor);
        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();

        // Top: 16x16 merge -> 1 quad
        // Bottom: 16x16 merge -> 1 quad
        // 4 sides (each 16x1) -> 4 quads
        // Total = 6 quads = 36 vertices. Unmerged would be 256 * 5 = 1280 quads = 7680 vertices!
        assert_eq!(verts.len(), 36 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_greedy_meshing_preserves_holes() {
        let mut world = World::new();
        let mut coords = Vec::new();
        for x in 0..3 {
            for z in 0..3 {
                if x == 1 && z == 1 {
                    continue; // hole
                }
                let c = WorldCoord::new(x, 0, z);
                world.set_cell(c, CellType::Block);
                coords.push(c);
            }
        }

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &coords, RenderMode::Editor);
        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();

        assert!(!verts.is_empty());
    }

    #[test]
    fn test_greedy_meshing_texture_boundary() {
        let mut world = World::new();
        let mut coords = Vec::new();
        for x in 0..2 {
            for z in 0..2 {
                let c = WorldCoord::new(x, 0, z);
                world.set_cell(c, CellType::Block);
                coords.push(c);
            }
        }

        world.get_mut(WorldCoord::new(0, 0, 0)).unwrap().texture = "Block_tx".to_string();
        world.get_mut(WorldCoord::new(0, 0, 1)).unwrap().texture = "Block_tx".to_string();
        world.get_mut(WorldCoord::new(1, 0, 0)).unwrap().texture = "brick".to_string();
        world.get_mut(WorldCoord::new(1, 0, 1)).unwrap().texture = "brick".to_string();

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &coords, RenderMode::Editor);

        assert_eq!(cpu_data.main_texture_vertices.len(), 2);
        assert!(cpu_data.main_texture_vertices.contains_key("Block_tx"));
        assert!(cpu_data.main_texture_vertices.contains_key("brick"));
    }

    #[test]
    fn test_greedy_meshing_color_boundary() {
        let mut world = World::new();
        let c1 = WorldCoord::new(0, 0, 0);
        let c2 = WorldCoord::new(1, 0, 0);
        world.set_cell(c1, CellType::Block);
        world.set_cell(c2, CellType::Block);

        world.set_cell_color_runtime(c1, Vec3::new(1.0, 0.0, 0.0));
        world.set_cell_color_runtime(c2, Vec3::new(0.0, 1.0, 0.0));

        let chunk_coord = ChunkCoord::new(0, 0, 0);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &[c1, c2], RenderMode::Editor);
        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();

        assert!(verts.len() >= 24 * BLOCK_VERTEX_FLOATS);
    }

    #[test]
    fn test_greedy_meshing_negative_coordinates() {
        let mut world = World::new();
        let mut coords = Vec::new();
        for x in -4..-2 {
            for z in -4..-2 {
                let c = WorldCoord::new(x, -10, z);
                world.set_cell(c, CellType::Block);
                coords.push(c);
            }
        }

        let chunk_coord = ChunkCoord::new(-1, -1, -1);
        let cpu_data = build_cpu_chunk_data(&world, chunk_coord, &coords, RenderMode::Editor);
        let verts = cpu_data.main_texture_vertices.get("Block_tx").unwrap();

        assert_eq!(verts.len(), 36 * BLOCK_VERTEX_FLOATS);
    }
}
