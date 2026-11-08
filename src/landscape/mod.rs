// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
pub mod cluster;
pub mod erosion_walkers;
pub mod heightmap;
pub mod image_heightmap;
pub mod legacy_import;
pub mod lod;
pub mod material_weights;
pub mod mesh;
pub mod paint_mask;
pub mod pgr_precompute;
pub mod portal;
pub mod stroke_resample;

pub use cluster::{
    Bounds, ClusterLimits, CompiledGeometry, Meshlet, VisibilityNode, compile_meshlets,
};
pub use heightmap::{Heightmap, HeightmapError, SurfaceLod};
pub use mesh::{IndexedMesh, MeshError, TopologyReport, Vertex};
