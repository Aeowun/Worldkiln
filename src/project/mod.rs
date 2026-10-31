// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub mod asset_import_plan;
pub mod asset_packs;
pub mod asset_registry;
pub(crate) mod atomic_file;
pub mod character_asset;
pub mod character_attachment_manifest;
pub mod character_offerings;
pub mod editor_state;
pub(crate) mod fs_walk;
pub mod fx_assets;
pub mod fx_authoring;
pub mod fx_export;
pub mod fx_import;
pub mod fx_standalone;
pub mod gltf_import;
pub mod hierarchy;
pub mod index;
pub mod material;
pub mod material_draft;
pub mod metadata;
pub mod native_logic;
pub mod post_process_pipeline;
pub mod project;
pub mod recent;
pub mod script_tree;
pub mod shared_inventory;
pub mod sky_material;
pub mod texture_material;

pub use hierarchy::{ProjectHierarchy, ProjectNode, ProjectNodeId, ProjectNodeKind};

pub use gltf_import::{
    CharacterGlbImportResult, GlbAssetKind, GlbAssetSummary, GlbFolderImportResult,
    GlbMaterialSummary, GlbMeshSummary, GlbSkinnedMesh, GlbSkinnedPrimitive, GlbSkinnedVertex,
    GlbStaticMesh, GlbStaticPrimitive, GlbStaticVertex, ImportedActorPackage,
    StaticGlbImportResult, import_character_glb, import_character_glb_named, import_glb_folder,
    import_glb_folder_named, import_glb_folder_with_progress, import_static_glb,
    import_static_glb_folder, import_static_glb_folder_named, import_static_glb_named, inspect_glb,
    load_skinned_glb_mesh, load_static_glb_mesh, load_static_glb_mesh_at, replace_glb_folder,
    replace_single_glb,
};

pub use editor_state::{
    EDITOR_STATE_FILE, EDITOR_STATE_VERSION, EditorProjectState, editor_state_path,
    load_editor_state, save_editor_state,
};

pub use index::{IndexedModule, ProjectScriptIndex, ScriptRoot};

pub use metadata::{
    PROJECT_METADATA_FILE, PROJECT_METADATA_VERSION, ProjectMetadata, ensure_project_metadata,
    load_project_metadata, metadata_path, save_project_metadata,
};

pub use project::{
    ProjectManager, available_actor_package_names, discover_actor_packages, discover_cameras,
    discover_characters, discover_controllers, engine_assets_dir, ensure_project_audio,
    ensure_project_cameras, ensure_project_characters, ensure_project_controllers,
    import_character_package, is_character_package,
};

pub use script_tree::{ScriptTree, ScriptTreeNode, ScriptTreeNodeKind};
