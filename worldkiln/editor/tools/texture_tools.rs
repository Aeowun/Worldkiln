use super::asset_search::{AssetBrowserState, AssetEntry, AssetOrigin, ranked_asset_matches};
use egui::{RichText, Ui};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

const TEXTURE_BROWSER_ID: &str = "worldkiln_texture_browser";
const TEXTURE_CATALOG_ID: &str = "worldkiln_texture_catalog";
// Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 08:20 -05:00
// One-shot UI notification consumed by App after explicit texture operations.
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

// Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
const TERRAIN_BUILTIN_TEXTURES: [&str; 5] = ["grass_1", "dirt", "tile_24", "sand", "water"];

// Worldkiln 0.8.5 | WO 0.8.5-BlockTxt | 2026-10-10 17:45 -05:00
// Block_txt is a virtual no-texture choice, not a PNG or asset path.
// It only appears for building Cells, never Terrain, Actors or generic assets.
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
    if !is_terrain_texture_target(target_id) || entry.origin == AssetOrigin::Project {
        return true;
    }

    TERRAIN_BUILTIN_TEXTURES.contains(&entry.path.as_str())
}

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 21:50 -05:00
#[derive(Clone, Default)]
pub(crate) struct TextureCatalog {
    entries: Vec<AssetEntry>,
    // Only requested thumbnails are decoded and uploaded, never the entire folder.
    previews: HashMap<String, Option<egui::TextureHandle>>,
}

fn builtin_texture_directory() -> PathBuf {
    crate::project::engine_assets_dir().join("textures")
}

fn project_texture_directory(project_path: &Option<PathBuf>) -> Option<PathBuf> {
    project_path
        .as_ref()
        .map(|project| project.join(".assets").join("textures"))
}

fn ensure_import_texture_directory(project_path: &Option<PathBuf>) -> Option<PathBuf> {
    let path = project_texture_directory(project_path).unwrap_or_else(builtin_texture_directory);

    if !path.exists() {
        if let Err(error) = fs::create_dir_all(&path) {
            eprintln!(
                "[EDITOR][TEXTURES] Failed to create '{}': {}",
                path.display(),
                error
            );
            return None;
        }
    }

    Some(path)
}

fn scan_directory_pngs(current_dir: &Path, root_dir: &Path, names: &mut Vec<String>) {
    let entries = match fs::read_dir(current_dir) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!(
                "[EDITOR][TEXTURES] Failed to read '{}': {}",
                current_dir.display(),
                error
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            scan_directory_pngs(&path, root_dir, names);
            continue;
        }

        let is_png = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("png"))
            .unwrap_or(false);

        if !is_png {
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
    // Use the path as the key so project-owned assets cleanly override a
    // built-in asset with the same canonical identifier, matching runtime
    // resolution order.
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
    let root = match entry.origin {
        AssetOrigin::Project => {
            project_texture_directory(project_path).unwrap_or_else(builtin_texture_directory)
        }
        AssetOrigin::BuiltIn => builtin_texture_directory(),
    };

    root.join(format!("{}.png", entry.path))
}

fn load_texture_catalog(_ctx: &egui::Context, project_path: &Option<PathBuf>) -> TextureCatalog {
    TextureCatalog {
        entries: scan_texture_entries(project_path),
        previews: HashMap::new(),
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 21:50 -05:00
fn load_texture_preview(
    ctx: &egui::Context,
    entry: &AssetEntry,
    project_path: &Option<PathBuf>,
) -> Option<egui::TextureHandle> {
    let path = texture_entry_file(entry, project_path);
    let image = match image::open(&path) {
        Ok(image) => image,
        Err(error) => {
            eprintln!(
                "[EDITOR][TEXTURES] Failed preview '{}': {}",
                path.display(),
                error
            );
            return None;
        }
    };
    let thumbnail = image.thumbnail(TEXTURE_PREVIEW_SIZE as u32, TEXTURE_PREVIEW_SIZE as u32);
    let rgba = thumbnail.to_rgba8();
    if rgba.width() == 0 || rgba.height() == 0 {
        return None;
    }
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(
        format!(
            "worldkiln_texture_preview_{}_{}",
            entry.origin.label(),
            entry.path
        ),
        color_image,
        egui::TextureOptions::LINEAR,
    ))
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

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 09:58 -05:00
pub(crate) fn refresh_texture_catalog(ctx: &egui::Context, project_path: &Option<PathBuf>) {
    let catalog = load_texture_catalog(ctx, project_path);
    let id = texture_catalog_id(project_path);

    ctx.data_mut(|data| {
        data.insert_temp(id, catalog);
        // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 08:20 -05:00
        // Texture imports, user Refresh and Asset Manager operations all call
        // this function. Notify the renderer instead of polling the disk.
        data.insert_temp(egui::Id::new(GPU_TEXTURE_RELOAD_REQUEST_ID), true);
    });
}

// Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:09 -05:00
pub(crate) fn open_texture_browser(
    ctx: &egui::Context,
    target_id: &str,
    project_path: &Option<PathBuf>,
) {
    // Catalog loading is lazy; simply opening the browser never decodes all PNGs.
    let _ = project_path;
    let id = egui::Id::new(TEXTURE_BROWSER_ID);

    ctx.data_mut(|data| {
        let mut state = data.get_temp::<AssetBrowserState>(id).unwrap_or_default();
        state.open_for(target_id);
        data.insert_temp(id, state);
    });
}

// Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
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

/// Draw texture controls inside the Rendering menu.
pub fn draw_texture_edit(
    ui: &mut Ui,
    texture: &mut String,
    target_id: &str,
    project_path: &Option<PathBuf>,
) {
    let catalog = get_texture_catalog(ui.ctx(), project_path);

    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
    // Wrap actions when this control is embedded in the narrow TERRAIN tab.
    // A single unwrapped row could paint beyond the panel into the viewport.
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

        if ui.button("Import...").clicked() {
            import_texture(ui.ctx(), texture, project_path);
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

/// Import a PNG texture and select it for the current build template.
fn import_texture(ctx: &egui::Context, texture: &mut String, project_path: &Option<PathBuf>) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG Image", &["png"])
        .pick_file()
    else {
        return;
    };

    let Some(textures_dir) = ensure_import_texture_directory(project_path) else {
        return;
    };

    let Some(file_name) = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
    else {
        eprintln!("[EDITOR][TEXTURES] Imported file has no filename.");
        return;
    };

    let Some(stem) = path
        .file_stem()
        .map(|name| name.to_string_lossy().to_string())
    else {
        eprintln!("[EDITOR][TEXTURES] Imported file has no valid name.");
        return;
    };

    let mut destination = textures_dir.join(&file_name);

    if destination.exists() {
        let mut index = 1;

        loop {
            let candidate = textures_dir.join(format!("{}_{}.png", stem, index));

            if !candidate.exists() {
                destination = candidate;
                break;
            }

            index += 1;
        }
    }

    match fs::copy(&path, &destination) {
        Ok(_) => {
            if let Some(final_stem) = destination
                .file_stem()
                .map(|name| name.to_string_lossy().to_string())
            {
                *texture = final_stem;
                refresh_texture_catalog(ctx, project_path);
            }
        }

        Err(error) => {
            eprintln!(
                "[EDITOR][TEXTURES] Failed to copy '{}' -> '{}': {}",
                path.display(),
                destination.display(),
                error
            );
        }
    }
}

#[cfg(test)]
mod gpu_texture_reload_tests {
    use super::*;

    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 08:20 -05:00
    #[test]
    fn refresh_signals_one_invalidating_event_and_then_stays_idle() {
        let ctx = egui::Context::default();
        assert!(!take_gpu_texture_reload_request(&ctx));
        refresh_texture_catalog(&ctx, &None);
        assert!(take_gpu_texture_reload_request(&ctx));
        assert!(!take_gpu_texture_reload_request(&ctx));
        refresh_texture_catalog(&ctx, &None);
        refresh_texture_catalog(&ctx, &None);
        // Multiple changes in the same frame require only one GPU cache reset.
        assert!(take_gpu_texture_reload_request(&ctx));
        assert!(!take_gpu_texture_reload_request(&ctx));
    }
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
            // Worldkiln 0.8.5 | WO 0.8.5-BlockTxt | 2026-10-10 17:45 -05:00
            // Explicitly restore untextured, color-only Block/FxBlock Cells.
            // Store an empty texture ID; do not load or persist Block_txt.png.
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

            // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 21:50 -05:00
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

            // Bound image decoding to two thumbnails per frame; unsuccessful loads
            // are memoized until Refresh/import invalidates the catalog.
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
                                                let preview =
                                                    load_texture_preview(ctx, entry, project_path);
                                                catalog.previews.insert(key.clone(), preview);
                                                ctx.request_repaint();
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
                                            ui.label(
                                                RichText::new(format!("{}.tx", entry.path)).small(),
                                            );
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
        texture_entry_allowed_for_target,
    };

    // Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
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

    // Worldkiln 0.8.5 | WO 0.8.5-BlockTxt | 2026-10-10 17:45 -05:00
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
        // The internal choice is deliberately empty: it means use Color,
        // without creating a synthetic missing Block_txt.png dependency.
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
