// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::asset_search::{AssetBrowserState, AssetEntry, AssetOrigin, ranked_asset_matches};
use egui::{RichText, Ui};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

const TEXTURE_BROWSER_ID: &str = "worldkiln_texture_browser";
const TEXTURE_CATALOG_ID: &str = "worldkiln_texture_catalog";
const GPU_TEXTURE_RELOAD_REQUEST_ID: &str = "worldkiln_gpu_texture_reload_request";

pub(crate) fn take_gpu_texture_reload_request(ctx: &egui::Context) -> bool {
    let id = egui::Id::new(GPU_TEXTURE_RELOAD_REQUEST_ID);
    ctx.data_mut(|data| {
        let requested = data.get_temp::<bool>(id).unwrap_or(false);
        if requested {
            data.insert_temp(id, false);
        }
        requested
    })
}

const TEXTURE_PREVIEW_SIZE: f32 = 88.0;
const TEXTURE_TILE_WIDTH: f32 = 112.0;
const TEXTURE_TILE_SPACING: f32 = 10.0;

const TERRAIN_BUILTIN_TEXTURES: [&str; 5] = ["grass_1", "dirt", "tile_24", "sand", "water"];

fn allows_plain_block_texture(target_id: &str) -> bool {
    target_id == "build_template.texture" || target_id.starts_with("inspector.block.")
}

fn is_terrain_texture_target(target_id: &str) -> bool {
    matches!(
        target_id,
        "terrain_surface_texture"
            | "terrain_subsurface_texture"
            | "terrain_rock_texture"
            | "terrain_water_texture"
    )
}

fn texture_entry_allowed_for_target(entry: &AssetEntry, target_id: &str) -> bool {
    if entry.path.starts_with("Material:") {
        return allows_plain_block_texture(target_id);
    }
    if !is_terrain_texture_target(target_id) || entry.origin == AssetOrigin::Project {
        return true;
    }

    TERRAIN_BUILTIN_TEXTURES.contains(&entry.path.as_str())
}

#[derive(Clone, Default)]
pub(crate) struct TextureCatalog {
    entries: Vec<AssetEntry>,
    previews: HashMap<String, Option<egui::TextureHandle>>,
    material_labels: HashMap<String, String>,
}

fn builtin_texture_directory() -> PathBuf {
    crate::project::engine_assets_dir().join("textures")
}

fn project_texture_directory(project_path: &Option<PathBuf>) -> Option<PathBuf> {
    project_path
        .as_ref()
        .map(|project| project.join(".assets").join("textures"))
}

fn scan_directory_pngs(current_dir: &Path, root_dir: &Path, names: &mut Vec<String>) {
    let found = match crate::project::fs_walk::scan(current_dir, 64, 250_000) {
        Ok(found) => found,
        Err(error) => {
            eprintln!(
                "[EDITOR][TEXTURES] Failed to scan {}: {error}",
                current_dir.display()
            );
            return;
        }
    };
    for (path, is_dir) in found {
        if is_dir {
            continue;
        }
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(root_dir) {
            let mut identifier = relative.to_path_buf();
            identifier.set_extension("");
            names.push(identifier.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn scan_texture_entries(project_path: &Option<PathBuf>) -> Vec<AssetEntry> {
    let mut entries = BTreeMap::<String, AssetEntry>::new();

    let builtin_dir = builtin_texture_directory();
    if builtin_dir.is_dir() {
        let mut names = Vec::new();
        scan_directory_pngs(&builtin_dir, &builtin_dir, &mut names);
        names.sort_unstable();
        names.dedup();

        for name in names {
            entries.insert(name.clone(), AssetEntry::new(name, AssetOrigin::BuiltIn));
        }
    }

    if let Some(project_dir) = project_texture_directory(project_path) {
        if project_dir.is_dir() {
            let mut names = Vec::new();
            scan_directory_pngs(&project_dir, &project_dir, &mut names);
            names.sort_unstable();
            names.dedup();

            for name in names {
                entries.insert(name.clone(), AssetEntry::new(name, AssetOrigin::Project));
            }
        }
    }

    if let Some(project) = project_path.as_deref() {
        for (identity, _) in crate::project::asset_packs::project_texture_entries(project) {
            entries.insert(
                identity.clone(),
                AssetEntry::new(identity, AssetOrigin::BuiltIn),
            );
        }
    }
    entries.into_values().collect()
}

fn texture_catalog_id(project_path: &Option<PathBuf>) -> egui::Id {
    let project_key = project_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();

    egui::Id::new((TEXTURE_CATALOG_ID, project_key))
}

fn texture_entry_file(entry: &AssetEntry, project_path: &Option<PathBuf>) -> PathBuf {
    if let Some(id) = entry.path.strip_prefix("Material:") {
        let Some(project) = project_path.as_deref() else {
            return PathBuf::new();
        };
        let catalog = crate::project::material::MaterialCatalog::load(project);
        let Some(texture) = catalog
            .texture_maps(id)
            .ok()
            .and_then(|maps| maps.get("BaseColor").cloned())
        else {
            return PathBuf::new();
        };
        if texture.starts_with("pack:") {
            return crate::project::asset_packs::resource_path(project, &texture)
                .unwrap_or_default();
        }
        let relative = if texture.to_ascii_lowercase().ends_with(".png") {
            texture
        } else {
            format!("{texture}.png")
        };
        if crate::project::asset_packs::safe_relative(&relative).is_err() {
            return PathBuf::new();
        }
        return project.join(".assets/textures").join(relative);
    }
    if entry.path.starts_with("pack:")
        && let Some(project) = project_path.as_deref()
    {
        return crate::project::asset_packs::resource_path(project, &entry.path)
            .unwrap_or_default();
    }
    let root = match entry.origin {
        AssetOrigin::Project => {
            project_texture_directory(project_path).unwrap_or_else(builtin_texture_directory)
        }
        AssetOrigin::BuiltIn => builtin_texture_directory(),
    };

    root.join(format!("{}.png", entry.path))
}

fn load_texture_catalog(_ctx: &egui::Context, project_path: &Option<PathBuf>) -> TextureCatalog {
    let mut entries = scan_texture_entries(project_path);
    let mut material_labels = HashMap::new();
    if let Some(project) = project_path.as_deref() {
        let catalog = crate::project::material::MaterialCatalog::cached(project);
        for (name, doc) in catalog.documents() {
            if matches!(
                catalog.resolve(&doc.id),
                Ok(crate::project::material::MaterialOutput::SurfacePbr { .. })
            ) {
                let key = format!("Material:{}", doc.id);
                material_labels.insert(key.clone(), name.clone());
                entries.push(AssetEntry::new(key, AssetOrigin::Project));
            }
        }
    }
    TextureCatalog {
        entries,
        previews: HashMap::new(),
        material_labels,
    }
}

fn load_texture_preview(
    ctx: &egui::Context,
    entry: &AssetEntry,
    project_path: &Option<PathBuf>,
) -> crate::editor::asset_management::ThumbnailLoad {
    let path = texture_entry_file(entry, project_path);
    crate::editor::asset_management::load_browser_thumbnail(ctx, &path)
}

fn get_texture_catalog(ctx: &egui::Context, project_path: &Option<PathBuf>) -> TextureCatalog {
    let id = texture_catalog_id(project_path);

    if let Some(catalog) = ctx.data(|data| data.get_temp::<TextureCatalog>(id)) {
        return catalog;
    }

    let catalog = load_texture_catalog(ctx, project_path);

    ctx.data_mut(|data| {
        data.insert_temp(id, catalog.clone());
    });

    catalog
}

pub(crate) fn refresh_texture_catalog(ctx: &egui::Context, project_path: &Option<PathBuf>) {
    if let Some(project) = project_path.as_deref() {
        crate::project::material::MaterialCatalog::invalidate_cached(project);
    }
    let catalog = load_texture_catalog(ctx, project_path);
    let id = texture_catalog_id(project_path);

    ctx.data_mut(|data| {
        data.insert_temp(id, catalog);
        data.insert_temp(egui::Id::new(GPU_TEXTURE_RELOAD_REQUEST_ID), true);
    });
}

pub(crate) fn open_texture_browser(
    ctx: &egui::Context,
    target_id: &str,
    project_path: &Option<PathBuf>,
) {
    let _ = project_path;
    let id = egui::Id::new(TEXTURE_BROWSER_ID);

    let catalog = load_texture_catalog(ctx, project_path);
    ctx.data_mut(|data| {
        data.insert_temp(texture_catalog_id(project_path), catalog);
        let mut state = data.get_temp::<AssetBrowserState>(id).unwrap_or_default();
        state.open_for(target_id);
        data.insert_temp(id, state);
    });
}

fn texture_matches(catalog: &TextureCatalog, texture: &str, target_id: &str) -> Vec<String> {
    ranked_asset_matches(
        catalog
            .entries
            .iter()
            .filter(|entry| texture_entry_allowed_for_target(entry, target_id))
            .map(|entry| entry.path.as_str()),
        texture,
        6,
    )
}

pub fn draw_texture_edit(
    ui: &mut Ui,
    texture: &mut String,
    target_id: &str,
    project_path: &Option<PathBuf>,
) {
    let catalog = get_texture_catalog(ui.ctx(), project_path);

    ui.horizontal_wrapped(|ui| {
        ui.label("Texture:");

        let texture_field_width = ui.available_width().min(120.0).max(72.0);
        ui.add(
            egui::TextEdit::singleline(texture)
                .desired_width(texture_field_width)
                .hint_text("Texture name"),
        );

        if ui.button("Browse...").clicked() {
            open_texture_browser(ui.ctx(), target_id, project_path);
        }
    });

    if texture.trim().is_empty() {
        return;
    }

    let matches = texture_matches(&catalog, texture, target_id);

    if matches.is_empty() {
        return;
    }

    ui.add_space(3.0);

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(120.0);

        for match_name in matches {
            let selected = match_name == *texture;

            if ui
                .add(egui::SelectableLabel::new(
                    selected,
                    format!("{}.tx", match_name),
                ))
                .clicked()
            {
                *texture = match_name;
            }
        }
    });
}

pub(crate) fn draw_texture_browser(
    ctx: &egui::Context,
    target_id: &str,
    selected_texture: &mut String,
    project_path: &Option<PathBuf>,
) {
    let id = egui::Id::new(TEXTURE_BROWSER_ID);

    let mut state = ctx
        .data(|data| data.get_temp::<AssetBrowserState>(id))
        .unwrap_or_default();

    if !state.is_open_for(target_id) {
        return;
    }

    let mut open = state.open;
    let mut selected_texture_name = None;

    egui::Window::new("Texture Browser")
        .open(&mut open)
        .default_size(egui::vec2(700.0, 500.0))
        .min_size(egui::vec2(420.0, 320.0))
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");

                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .desired_width(240.0)
                        .hint_text("Filter textures..."),
                );

                if ui.button("Refresh").clicked() {
                    refresh_texture_catalog(ctx, project_path);
                }
            });

            ui.separator();

            let search = state.search.trim().to_lowercase();
            if allows_plain_block_texture(target_id)
                && (search.is_empty() || "block_txt".contains(&state.search.trim().to_lowercase()))
            {
                if ui
                    .selectable_label(selected_texture.is_empty(), "Block_txt")
                    .on_hover_text("No texture — use the block's Color")
                    .clicked()
                {
                    selected_texture_name = Some(String::new());
                }
                ui.separator();
            }

            let mut catalog = get_texture_catalog(ctx, project_path);
            let has_eligible_textures = catalog
                .entries
                .iter()
                .any(|entry| texture_entry_allowed_for_target(entry, target_id));

            let entries: Vec<AssetEntry> = catalog
                .entries
                .iter()
                .filter(|entry| texture_entry_allowed_for_target(entry, target_id))
                .filter(|entry| {
                    search.is_empty()
                        || entry.path.to_lowercase().contains(&search)
                        || entry.folder.to_lowercase().contains(&search)
                })
                .cloned()
                .collect();

            if entries.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.label(if has_eligible_textures {
                        "No textures match the search."
                    } else {
                        "No textures are available for this property."
                    });
                });
                return;
            }

            let available_width = ui.available_width().max(TEXTURE_TILE_WIDTH);
            let columns = ((available_width + TEXTURE_TILE_SPACING)
                / (TEXTURE_TILE_WIDTH + TEXTURE_TILE_SPACING))
                .floor()
                .max(1.0) as usize;

            let mut groups: BTreeMap<(AssetOrigin, String), Vec<AssetEntry>> = BTreeMap::new();
            for entry in entries {
                groups
                    .entry((entry.origin, entry.folder.clone()))
                    .or_default()
                    .push(entry);
            }

            let mut thumbnail_budget = 2usize;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for ((origin, folder), entries) in groups {
                        egui::CollapsingHeader::new(format!(
                            "{} ({}) · {}",
                            folder,
                            entries.len(),
                            origin.label()
                        ))
                        .default_open(true)
                        .show(ui, |ui| {
                            for row in entries.chunks(columns) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing =
                                        egui::vec2(TEXTURE_TILE_SPACING, 8.0);
                                    for entry in row {
                                        ui.vertical(|ui| {
                                            ui.set_min_width(TEXTURE_TILE_WIDTH);
                                            let key =
                                                format!("{}:{}", entry.origin.label(), entry.path);
                                            if !catalog.previews.contains_key(&key)
                                                && thumbnail_budget > 0
                                            {
                                                thumbnail_budget -= 1;
                                                match load_texture_preview(ctx, entry, project_path) {
                                                    crate::editor::asset_management::ThumbnailLoad::Ready(texture) => {
                                                        catalog.previews.insert(key.clone(), Some(texture));
                                                        ctx.request_repaint();
                                                    }
                                                    crate::editor::asset_management::ThumbnailLoad::Failed => {
                                                        catalog.previews.insert(key.clone(), None);
                                                    }
                                                    crate::editor::asset_management::ThumbnailLoad::Pending => {
                                                        ctx.request_repaint_after(std::time::Duration::from_millis(250));
                                                    }
                                                }
                                            }
                                            if let Some(Some(texture)) = catalog.previews.get(&key)
                                            {
                                                let [width, height] = texture.size();
                                                let aspect = if height == 0 {
                                                    1.0
                                                } else {
                                                    width as f32 / height as f32
                                                };
                                                let (w, h) = if aspect > 1.0 {
                                                    (
                                                        TEXTURE_PREVIEW_SIZE,
                                                        TEXTURE_PREVIEW_SIZE / aspect,
                                                    )
                                                } else {
                                                    (
                                                        TEXTURE_PREVIEW_SIZE * aspect,
                                                        TEXTURE_PREVIEW_SIZE,
                                                    )
                                                };
                                                if ui
                                                    .add(egui::ImageButton::new((
                                                        texture.id(),
                                                        egui::vec2(w.max(1.0), h.max(1.0)),
                                                    )))
                                                    .clicked()
                                                {
                                                    selected_texture_name =
                                                        Some(entry.path.clone());
                                                }
                                            } else if ui
                                                .add_sized(
                                                    [TEXTURE_PREVIEW_SIZE, TEXTURE_PREVIEW_SIZE],
                                                    egui::Button::new("Select"),
                                                )
                                                .clicked()
                                            {
                                                selected_texture_name = Some(entry.path.clone());
                                            }
                                            let label = if let Some(name) =
                                                catalog.material_labels.get(&entry.path)
                                            {
                                                format!("Material: {name}")
                                            } else {
                                                format!("{}.tx", entry.path)
                                            };
                                            ui.label(RichText::new(label).small());
                                        });
                                    }
                                });
                                ui.add_space(8.0);
                            }
                        });
                    }
                });

            ctx.data_mut(|data| {
                data.insert_temp(texture_catalog_id(project_path), catalog);
            });
        });

    if let Some(name) = selected_texture_name {
        *selected_texture = name;
        open = false;
    }

    state.open = open;

    ctx.data_mut(|data| {
        data.insert_temp(id, state);
    });
}

#[cfg(test)]
mod terrain_texture_filter_tests {
    use super::{
        AssetEntry, AssetOrigin, TERRAIN_BUILTIN_TEXTURES, is_terrain_texture_target,
        load_texture_catalog, texture_entry_allowed_for_target, texture_entry_file,
    };

    #[test]
    fn terrain_picker_allows_only_approved_builtins_and_project_assets() {
        for name in TERRAIN_BUILTIN_TEXTURES {
            assert!(texture_entry_allowed_for_target(
                &AssetEntry::new(name, AssetOrigin::BuiltIn),
                "terrain_surface_texture",
            ));
        }

        assert!(!texture_entry_allowed_for_target(
            &AssetEntry::new("grass_14", AssetOrigin::BuiltIn),
            "terrain_surface_texture",
        ));
        assert!(!texture_entry_allowed_for_target(
            &AssetEntry::new("brick", AssetOrigin::BuiltIn),
            "terrain_rock_texture",
        ));
        assert!(texture_entry_allowed_for_target(
            &AssetEntry::new("my/custom_grass", AssetOrigin::Project),
            "terrain_surface_texture",
        ));
    }

    #[test]
    fn material_created_from_png_is_selectable_and_previews_its_real_basecolor() {
        use crate::project::{
            material::{MaterialDefinition, MaterialDocument, MaterialOutput},
            texture_material,
        };
        let project = std::env::temp_dir().join(format!(
            "worldkiln_ui_material_{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(project.join(".assets/textures")).unwrap();
        let original = project.join(".assets/textures/Creator.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([58, 175, 95, 255]))
            .save(&original)
            .unwrap();
        let document = texture_material::from_texture(&project, "Creator", "Creator.png").unwrap();
        let key = format!("Material:{}", document.id);
        let catalog = load_texture_catalog(&egui::Context::default(), &Some(project.clone()));
        let entry = catalog
            .entries
            .iter()
            .find(|entry| entry.path == key)
            .unwrap();
        assert_eq!(
            catalog.material_labels.get(&key).map(String::as_str),
            Some("Creator")
        );
        assert_eq!(texture_entry_file(entry, &Some(project.clone())), original);
        assert!(texture_entry_allowed_for_target(
            entry,
            "inspector.block.14.texture"
        ));
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn project_material_is_assignable_via_existing_block_texture_browser() {
        let asset = AssetEntry::new("Material:mat-creator", AssetOrigin::Project);
        assert!(texture_entry_allowed_for_target(
            &asset,
            "build_template.texture"
        ));
        assert!(texture_entry_allowed_for_target(
            &asset,
            "inspector.block.42.texture"
        ));
        assert!(!texture_entry_allowed_for_target(
            &asset,
            "terrain_surface_texture"
        ));
        assert!(!texture_entry_allowed_for_target(
            &asset,
            "inspector.cell.12.texture"
        ));
    }

    #[test]
    fn block_txt_plain_color_choice_is_only_in_cell_block_texture_browsers() {
        use super::allows_plain_block_texture;
        assert!(allows_plain_block_texture("build_template.texture"));
        assert!(allows_plain_block_texture("inspector.block.42.texture"));
        for target in [
            "terrain_surface_texture",
            "inspector.cell.42.texture",
            "actor.texture",
            "world_material.texture",
        ] {
            assert!(!allows_plain_block_texture(target));
        }
        let mut chosen = "brick".to_owned();
        chosen.clear();
        assert!(chosen.is_empty());
    }

    #[test]
    fn non_terrain_texture_picker_keeps_full_builtin_catalog() {
        assert!(!is_terrain_texture_target("build_texture"));
        assert!(texture_entry_allowed_for_target(
            &AssetEntry::new("grass_14", AssetOrigin::BuiltIn),
            "build_texture",
        ));
    }
}
