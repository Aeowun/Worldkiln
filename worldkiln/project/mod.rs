// Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
pub(crate) mod atomic_file;
pub mod editor_state;
pub mod gltf_import;
pub mod hierarchy;
pub mod index;
pub mod metadata;
pub mod project;
pub mod recent;
pub mod script_tree;

pub use hierarchy::{ProjectHierarchy, ProjectNode, ProjectNodeId, ProjectNodeKind};

pub use gltf_import::{
    CharacterGlbImportResult, GlbAssetKind, GlbAssetSummary, GlbFolderImportResult,
    GlbMaterialSummary, GlbMeshSummary, GlbSkinnedMesh, GlbSkinnedPrimitive, GlbSkinnedVertex,
    GlbStaticMesh, GlbStaticPrimitive, GlbStaticVertex, ImportedActorPackage,
    StaticGlbImportResult, import_character_glb, import_glb_folder, import_static_glb,
    import_static_glb_folder, inspect_glb, load_skinned_glb_mesh, load_static_glb_mesh,
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
