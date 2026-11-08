// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{Editor, ai_project::ProjectAccess};
use crate::{
    project::asset_registry::AssetRegistry,
    world::{World, WorldCoord},
};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn tool_specs() -> Vec<Value> {
    let v3 = json!({"type":"array","minItems":3,"maxItems":3,"items":{"type":"number"}});
    let s = json!({"type":"string"});
    let mut tools = Vec::new();
    let mut add = |name: &str, description: &str, properties: Value, required: Vec<&str>| {
        tools.push(json!({"name":name,"description":description,"parameters":{"type":"object","properties":properties,"required":required}}));
    };
    add(
        "editor_capabilities",
        "Inspect the running build's real tool manifest and existing editor destinations. Tools are already offered; do not call just to introduce yourself.",
        json!({}),
        vec![],
    );
    add(
        "list_project_assets",
        "List registered project assets with stable AssetIDs, native types, names and paths; query filters these fields, 100/page. Prefer this over reading asset registry JSON and package files one by one.",
        json!({"query":s,"offset":{"type":"integer"}}),
        vec![],
    );
    add(
        "inspect_asset",
        "Read native asset details: meshes, materials, rig, animations, Character/controller/camera associations and dependencies.",
        json!({"asset_id":s}),
        vec!["asset_id"],
    );
    let sources =
        json!({"paths":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"string"}}});
    add(
        "plan_asset_import",
        "Preview native Asset Manager import for existing project-relative source files, e.g. generated GLBs/images. Read-only. Does not place objects.",
        sources.clone(),
        vec!["paths"],
    );
    add(
        "import_project_assets",
        "Execute a conflict-free native Asset Manager import plan, registering supported GLBs/images/audio/video/effects properly. Use plan_asset_import first. Existing asset conflicts rejected. Native imported files are not removed by scene undo; output reports per-file failures. Scene placement is separate.",
        sources,
        vec!["paths"],
    );
    add(
        "create_character_asset",
        "Create and save a native Character from an existing rigged Actor AssetID or package name; native validation checks rig, scale, optional material/controller/camera. Does not turn arbitrary static geometry into a rig. No overwrite. Native asset-file creation is outside scene undo; choosing Player and placement are separate.",
        json!({"name":s,"source_actor":s,"scale":v3,"controller":s,"camera":s,"material":s}),
        vec!["name", "source_actor"],
    );
    add(
        "create_texture_from_image",
        "Use native Images > Create Texture to promote a project .assets/media PNG/JPEG into a Cell-compatible PNG Texture. Source preserved; existing Texture not overwritten. Saved asset-file creation is outside scene undo.",
        json!({"path":s}),
        vec!["path"],
    );
    add(
        "create_material_from_textures",
        "Create and save a native PBR Material from existing Texture identities. maps requires BaseColor; optional NormalGL, Roughness, AO, Height, ARM; native checks validate PNGs and dimensions. No overwrite. Native asset-file creation is outside scene undo; assignment separate.",
        json!({"name":s,"maps":{"type":"object","additionalProperties":{"type":"string"},"description":"PBR map name -> existing Texture identity, e.g. {BaseColor: Stone.png}"}}),
        vec!["name", "maps"],
    );
    add(
        "place_actors",
        "Place 1–256 real registered Actor assets with appearance and native package collision in one saved, undoable scene operation. Preflights every Actor before creating any. Use actual Actor AssetIDs from list_project_assets; Character creation is a separate native asset workflow.",
        json!({"actors":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"object","properties":{"asset_id":s,"name":s,"position":v3,"rotation":v3,"scale":v3},"required":["asset_id","position"]}}}),
        vec!["actors"],
    );
    add(
        "assign_cell_material",
        "Assign an existing native surface Material ID to 1–256 authored Cells, with whole-plan preflight, one scene undo and save. Uses actual Material IDs returned by material creation/inspection; rejects Water/Sky materials.",
        json!({"material_id":s,"cell_ids":{"type":"array","minItems":1,"maxItems":256,"items":{"type":"integer"}}}),
        vec!["material_id", "cell_ids"],
    );
    add(
        "generate_terrain",
        "Generate native authored terrain using current project terrain configuration and request {position:[x,y,z],size:[x,y,z]}. Existing terrain requires replace_existing:true. Native scene undo supported; existing Cells preserved. Does not configure all generator settings.",
        json!({"request":{"type":"object","properties":{"position":v3,"size":v3},"required":["position","size"]},"replace_existing":{"type":"boolean"}}),
        vec!["request"],
    );
    add(
        "sculpt_terrain",
        "Apply native Raise, Lower, Flatten, FlattenRaiseOnly, FlattenCutOnly or Smooth sculpt to authored terrain. Request: tool, center:[x,y,z], radius>0, strength>=0, flatten_target:number or null. Uses scene undo and requests save.",
        json!({"request":{"type":"object","properties":{"tool":{"type":"string","enum":["Raise","Lower","Flatten","FlattenRaiseOnly","FlattenCutOnly","Smooth"]},"center":v3,"radius":{"type":"number"},"strength":{"type":"number"},"flatten_target":{"type":["number","null"]}},"required":["tool","center","radius","strength","flatten_target"]}}),
        vec!["request"],
    );
    add(
        "paint_terrain",
        "Paint an existing native terrain material/layer. Request: center:[x,y,z],radius>0,material:string,strength:0..1, optional falloff:0..1. Uses native scene undo and save; does not create a Material asset.",
        json!({"request":{"type":"object","properties":{"center":v3,"radius":{"type":"number"},"material":s,"strength":{"type":"number"},"falloff":{"type":"number"}},"required":["center","radius","material","strength"]}}),
        vec!["request"],
    );
    add(
        "query_navigation",
        "Build a native static navigation snapshot under the current project Nav Mesh Bounds/global-navigation policy and find a path. Does not change policy or prove Character movement.",
        json!({"start":v3,"target":v3,"width_cells":{"type":"integer"},"height_cells":{"type":"integer"},"max_step_up":{"type":"integer"},"max_step_down":{"type":"integer"}}),
        vec!["start", "target"],
    );
    add(
        "navigate_editor",
        "Open an existing editor workspace, inspector, tool or settings destination by exact id from editor_capabilities. Navigation is not content authoring; File/build/Play actions use dedicated tools.",
        json!({"destination":s}),
        vec!["destination"],
    );
    add(
        "set_play_mode",
        "Request native Play (playing:true) or return to Editor (false). Requires mutation permission; normal dirty-script/startup handling remains. Read runtime state/diagnostics afterward; entering Play is not gameplay verification.",
        json!({"playing":{"type":"boolean"}}),
        vec!["playing"],
    );
    add(
        "read_runtime_state",
        "Read actual app mode transition, script-runtime availability, player position when present, and available runtime test results. Does not simulate inputs or prove a feature works.",
        json!({}),
        vec![],
    );
    add(
        "read_ui_document",
        "Read actual authored UI workspace metadata and one live document (default active), native validation errors and a native default node template. Use this document as expected_document when writing.",
        json!({"document_id":{"type":"integer"}}),
        vec![],
    );
    add(
        "create_ui_document",
        "Create an empty native UI workspace document; does not replace an existing document. Workspace creation is outside scene undo.",
        json!({"name":s}),
        vec!["name"],
    );
    add(
        "write_ui_document",
        "Apply a complete authored UI document through native validation and per-document UI history. Requires exact expected_document from a fresh read; rejects conflicting live edits and invalid hierarchy/keys. One UI undo entry; scene undo is separate. Does not invent script handlers.",
        json!({"document_id":{"type":"integer"},"document":{"type":"object"},"expected_document":{"type":"object"}}),
        vec!["document_id", "document", "expected_document"],
    );
    add(
        "undo_ui_document",
        "Undo the latest native UI edit on the specified document only if expected_document matches current live state. Uses the existing per-document UI undo system, not scene undo.",
        json!({"document_id":{"type":"integer"},"expected_document":{"type":"object"}}),
        vec!["document_id", "expected_document"],
    );
    tools
}

fn text<'a>(args: &'a Value, field: &str) -> Result<&'a str, String> {
    args[field]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing {field}"))
}
fn vector(value: &Value) -> Result<glam::Vec3, String> {
    let a = value
        .as_array()
        .filter(|a| a.len() == 3)
        .ok_or("Expected three coordinates")?;
    let mut xyz = [0.0f32; 3];
    for i in 0..3 {
        let n = a[i]
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 1_000_000.0)
            .ok_or("Invalid coordinate")?;
        xyz[i] = n as f32;
    }
    Ok(glam::Vec3::from_array(xyz))
}

impl Editor {
    pub(super) fn native_ui_operation(
        &mut self,
        name: &str,
        args: &Value,
    ) -> Option<Result<(String, bool), String>> {
        if !matches!(
            name,
            "read_ui_document" | "create_ui_document" | "write_ui_document" | "undo_ui_document"
        ) {
            return None;
        }
        Some((|| {
            self.sync_active_ui_document();
            if name == "create_ui_document" {
                let id = self.ui_workspace.create_document(text(args, "name")?)?;
                return Ok((
                    json!({"created_document_id":id,"save_requested":true,"undo_supported":false})
                        .to_string(),
                    true,
                ));
            }
            let id = args["document_id"]
                .as_u64()
                .map(u32::try_from)
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or(self.ui_workspace.active_document_id);
            let current = self
                .ui_workspace
                .document(id)
                .ok_or("UI document missing")?
                .document
                .clone();
            let current_json = serde_json::to_value(&current).map_err(|e| e.to_string())?;
            if name == "read_ui_document" {
                return Ok((json!({"document_id":id,"document":current_json,"validation_errors":current.validation_errors(),
                    "node_template":crate::engine::ui_authoring::AuthoredUiNode::default(),
                    "documents":self.ui_workspace.documents.iter().map(|d|json!({"id":d.id,"name":d.name,"key":d.key})).collect::<Vec<_>>()}).to_string(),false));
            }
            if args["expected_document"] != current_json {
                return Err("UI document changed; read it again before applying".into());
            }
            if name == "undo_ui_document" {
                if self
                    .ui_document_history
                    .get(&id)
                    .is_none_or(|h| h.undo.is_empty())
                {
                    return Err("No native UI undo entry for this document".into());
                }
                self.open_ui_document(id)?;
                self.undo_ui_document();
                return Ok((
                    json!({"undone":true,"document_id":id,"save_requested":true}).to_string(),
                    true,
                ));
            }
            let candidate: crate::engine::ui_authoring::AuthoredUiDocument =
                serde_json::from_value(args["document"].clone()).map_err(|e| e.to_string())?;
            let errors = candidate.validation_errors();
            if !errors.is_empty() {
                return Err(format!("UI validation failed: {}", errors.join("; ")));
            }
            let history = self.ui_document_history.entry(id).or_default();
            history.undo.push(current);
            if history.undo.len() > self.history.limit.max(1) {
                history.undo.remove(0);
            }
            history.redo.clear();
            history.edit_start = None;
            self.ui_workspace.document_mut(id).unwrap().document = candidate.clone();
            if id == self.ui_workspace.active_document_id {
                self.ui_document = candidate;
                self.ui_selected_node = None;
            }
            Ok((json!({"applied":true,"document_id":id,"save_requested":true,"runtime_tested":false,"undo_system":"native per-document UI history"}).to_string(),true))
        })())
    }
    pub(super) fn native_authoring_operation(
        &mut self,
        world: &World,
        root: &Path,
        name: &str,
        args: &Value,
    ) -> Option<Result<(String, bool), String>> {
        let result = match name {
            "editor_capabilities" => Ok(json!({"tools":super::ai_project::tool_specs(),
                "editor_destinations":super::feature_nav::DIRECTORY.iter().map(|d| json!({"id":format!("{:?}",d.id),"title":d.title})).collect::<Vec<_>>(),
                "evidence":"Native tool definitions in this running build; UI destinations are not all exposed as mutation APIs."})),
            "list_project_assets" => (|| {
                let registry = AssetRegistry::load(root)?;
                let query = args["query"].as_str().unwrap_or("").to_ascii_lowercase();
                let offset = args["offset"].as_u64().unwrap_or(0) as usize;
                let records: Vec<_> = registry
                    .assets
                    .iter()
                    .filter(|r| {
                        format!("{} {} {:?}", r.name, r.id, r.kind)
                            .to_ascii_lowercase()
                            .contains(&query)
                    })
                    .collect();
                Ok(
                    json!({"total":records.len(),"assets":records.iter().skip(offset).take(100).collect::<Vec<_>>(),"next_offset":(records.len()>offset.saturating_add(100)).then_some(offset.saturating_add(100))}),
                )
            })(),
            "inspect_asset" => AssetRegistry::load(root)
                .and_then(|r| r.inspect(root, text(args, "asset_id")?))
                .and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
            "plan_asset_import" | "import_project_assets" => {
                (|| {
                    let access = ProjectAccess::open(root)?;
                    let sources = args["paths"]
                        .as_array()
                        .filter(|p| !p.is_empty() && p.len() <= 32)
                        .ok_or("Supply 1–32 project-relative source paths")?;
                    let paths = sources
                        .iter()
                        .map(|p| access.resolve(p.as_str().ok_or("Invalid source path")?, false))
                        .collect::<Result<Vec<_>, String>>()?;
                    let plan = crate::editor::asset_management::prepare_asset_import(root, &paths)?;
                    if name == "plan_asset_import" {
                        return Ok(json!({"plan":plan,"applied":false}));
                    }
                    if !plan.can_import() {
                        return Err("Import plan is empty or has conflicts; inspect plan_asset_import first".into());
                    }
                    let result =
                        crate::editor::asset_management::apply_asset_import(root, &plan, None)?;
                    Ok(
                        json!({"result":result,"undo_supported":false,"note":"Native Asset Manager import; scene placement is separate. Existing scene undo does not remove imported files."}),
                    )
                })()
            }
            "create_character_asset" => (|| {
                let asset = crate::project::character_asset::CharacterAsset {
                    version: crate::project::character_asset::CHARACTER_ASSET_VERSION,
                    name: text(args, "name")?.into(),
                    source_actor: text(args, "source_actor")?.into(),
                    scale: if args.get("scale").is_some() {
                        vector(&args["scale"])?.to_array()
                    } else {
                        [1.0; 3]
                    },
                    controller: args["controller"].as_str().map(str::to_owned),
                    camera: args["camera"].as_str().map(str::to_owned),
                    material: args["material"].as_str().map(str::to_owned),
                };
                let path = crate::project::character_asset::save(root, None, &asset)?;
                Ok(
                    json!({"created":path.strip_prefix(root).unwrap_or(&path),"saved":true,"undo_supported":false,
                    "note":"Native Character asset created from a validated rigged Actor. Player selection and scene placement are separate."}),
                )
            })(),
            "create_texture_from_image" => (|| {
                let access = ProjectAccess::open(root)?;
                let image = access.resolve(text(args, "path")?, false)?;
                let relative = image
                    .strip_prefix(root.canonicalize().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                let path = crate::editor::asset_management::create_texture_asset(
                    root,
                    &root.join(relative),
                )?;
                AssetRegistry::reconcile(root)?;
                Ok(
                    json!({"created":path.strip_prefix(root).unwrap_or(&path),"saved":true,"undo_supported":false}),
                )
            })(),
            "create_material_from_textures" => (|| {
                let maps: std::collections::BTreeMap<String, String> =
                    serde_json::from_value(args["maps"].clone()).map_err(|e| e.to_string())?;
                let name = text(args, "name")?;
                let material = crate::project::texture_material::from_maps(root, name, maps)?;
                AssetRegistry::reconcile(root)?;
                Ok(
                    json!({"created":crate::project::material::file_path(root,name)?.strip_prefix(root).map_err(|e|e.to_string())?,
                    "material_id":material.id,"saved":true,"undo_supported":false,"note":"Native Material asset saved; assignment is separate."}),
                )
            })(),
            "query_navigation" => (|| {
                let agent = crate::navigation::NavigationAgent {
                    width_cells: args["width_cells"].as_u64().unwrap_or(1).clamp(1, 32) as u32,
                    height_cells: args["height_cells"].as_u64().unwrap_or(2).clamp(1, 64) as u32,
                    max_step_up: args["max_step_up"].as_u64().unwrap_or(1).min(32) as u32,
                    max_step_down: args["max_step_down"].as_u64().unwrap_or(1).min(32) as u32,
                };
                let grid = crate::navigation::NavigationGrid::build_for_project(world, agent)
                    .map_err(|e| e.to_string())?;
                let start = WorldCoord::from_vec3(vector(&args["start"])?);
                let target = WorldCoord::from_vec3(vector(&args["target"])?);
                let path = grid
                    .find_path(
                        grid.project_endpoint(start),
                        grid.project_endpoint(target),
                        Default::default(),
                    )
                    .map_err(|e| e.to_string())?;
                let points = path.world_points();
                Ok(
                    json!({"points":points.iter().take(256).map(|v|v.to_array()).collect::<Vec<_>>(),"point_count":points.len(),"truncated":points.len()>256,"cost":path.total_cost,
                    "visited":path.visited_nodes,"evidence":"Static project-policy navigation query; not observed Character movement."}),
                )
            })(),
            _ => return None,
        };
        Some(result.map(|v| (v.to_string(), false)))
    }

    pub(super) fn native_scene_operation(
        &mut self,
        world: &mut World,
        root: &Path,
        name: &str,
        args: &Value,
    ) -> Result<String, String> {
        let result = match name {
            "assign_cell_material" => {
                let material = text(args, "material_id")?;
                let material = material.strip_prefix("Material:").unwrap_or(material);
                let catalog = crate::project::material::MaterialCatalog::cached(root);
                if !matches!(
                    catalog.resolve(material)?,
                    crate::project::material::MaterialOutput::SurfacePbr { .. }
                ) {
                    return Err("Expected a surface Material".into());
                }
                let ids = args["cell_ids"]
                    .as_array()
                    .filter(|a| !a.is_empty() && a.len() <= 256)
                    .ok_or("Supply 1–256 Cell IDs")?;
                let coords = ids
                    .iter()
                    .map(|id| {
                        world
                            .resolve_cell_id(id.as_u64().ok_or("Invalid Cell ID")?)
                            .ok_or_else(|| "Authored Cell missing".to_owned())
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                for coord in coords {
                    world.get_mut(coord).ok_or("Cell missing")?.texture =
                        format!("Material:{material}");
                    world.touch_authored_cell(coord, crate::world::world::DirtyReason::Geometry);
                }
                json!({"assigned":ids.len(),"save_requested":true,"runtime_tested":false})
            }
            "place_actors" => {
                let entries = args["actors"]
                    .as_array()
                    .filter(|e| !e.is_empty() && e.len() <= 256)
                    .ok_or("Supply 1–256 Actors")?;
                let registry = AssetRegistry::load(root)?;
                let mut plan = Vec::new();
                for entry in entries {
                    let id = text(entry, "asset_id")?;
                    let package = registry.resolve_actor(root, id)?;
                    let position = vector(&entry["position"])?;
                    let rotation = entry
                        .get("rotation")
                        .map(vector)
                        .transpose()?
                        .unwrap_or(glam::Vec3::ZERO);
                    let scale = entry
                        .get("scale")
                        .map(vector)
                        .transpose()?
                        .unwrap_or(glam::Vec3::ONE);
                    if scale.min_element() <= 0.0 {
                        return Err("Actor scale must be positive".into());
                    }
                    let actor_name = entry["name"]
                        .as_str()
                        .unwrap_or(&package.metadata.name)
                        .to_owned();
                    if actor_name.is_empty() || actor_name.len() > 120 {
                        return Err("Invalid Actor name".into());
                    }
                    plan.push((
                        id.to_owned(),
                        package,
                        position,
                        rotation,
                        scale,
                        actor_name,
                    ));
                }
                let mut placed = Vec::new();
                for (asset_id, package, position, rotation, scale, name) in plan {
                    let id = world.add_actor(name, position);
                    let actor = world.actors.get_mut(&id).unwrap();
                    actor.package = Some(asset_id);
                    actor.rotation = rotation;
                    actor.scale = scale;
                    crate::engine::collision_geometry::set_actor_package_collision(actor, &package);
                    placed.push(id);
                }
                self.synchronize_project_actor_nodes(world);
                json!({"actor_ids":placed,"save_requested":true,"runtime_tested":false})
            }
            "generate_terrain" => {
                if world.terrain.authored.is_some() && args["replace_existing"] != true {
                    return Err("Authored terrain exists; explicitly request replace_existing to replace it".into());
                }
                let request: crate::world::terrain::TerrainGenerationRequest =
                    serde_json::from_value(args["request"].clone()).map_err(|e| e.to_string())?;
                request.validate().map_err(|e| e.to_string())?;
                let generated =
                    crate::world::terrain::generate_terrain(&world.terrain.config, request)
                        .map_err(|e| e.to_string())?;
                world.terrain.replace_authored(Some(generated));
                json!({"generated":true,"save_requested":true,"runtime_tested":false})
            }
            "sculpt_terrain" => {
                let request: crate::world::terrain::TerrainSculptRequest =
                    serde_json::from_value(args["request"].clone()).map_err(|e| e.to_string())?;
                if !request.radius.is_finite()
                    || request.radius <= 0.0
                    || !request.strength.is_finite()
                    || request.strength < 0.0
                    || request.center.iter().any(|v| !v.is_finite())
                    || request.flatten_target.is_some_and(|v| !v.is_finite())
                {
                    return Err("Invalid sculpt request".into());
                }
                json!({"changed":world.sculpt_authored_terrain(&request),"save_requested":true,"runtime_tested":false})
            }
            "paint_terrain" => {
                let request: crate::world::terrain::TerrainPaintRequest =
                    serde_json::from_value(args["request"].clone()).map_err(|e| e.to_string())?;
                if !request.radius.is_finite()
                    || request.radius <= 0.0
                    || !request.strength.is_finite()
                    || !(0.0..=1.0).contains(&request.strength)
                    || request.center.iter().any(|v| !v.is_finite())
                    || request
                        .falloff
                        .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                {
                    return Err("Invalid paint request".into());
                }
                json!({"changed":world.paint_authored_terrain(&request),"save_requested":true,"runtime_tested":false})
            }
            _ => return Err("Unsupported native scene operation".into()),
        };
        Ok(result.to_string())
    }
}
