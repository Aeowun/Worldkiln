// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::animation::{
    AnimationAsset, AnimationEvent, EventValue, Interpolation, QuatKey, TransformTrack, Vec3Key,
    sample_pose,
};
use crate::character::rig::{Skeleton, Transform};
use crate::project::asset_registry::{AssetRegistry, AssetType};
use egui::{Color32, RichText, Sense, Stroke, Ui};
use glam::{EulerRot, Quat, Vec3};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimelineMode {
    Timeline,
    Curves,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformTool {
    Move,
    Rotate,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartPose {
    RestPose,
    CurrentPose,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorAction {
    None,
    Saved,
    Close,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationEditorState {
    pub project: PathBuf,
    pub draft: AnimationAsset,
    pub asset_path: Option<PathBuf>,
    pub rig_path: PathBuf,
    pub skeleton: Skeleton,
    preview_package: Option<std::sync::Arc<crate::character::package::CharacterPackage>>,
    preview_textures:
        std::collections::HashMap<std::path::PathBuf, std::sync::Arc<image::RgbaImage>>,
    pub status: Option<(String, bool)>,
    pub setup_open: bool,
    pub(crate) importing: Option<crate::editor::animation_import::AnimationImportState>,
    pub current_time: f32,
    pub playing: bool,
    pub playback_rate: f32,
    pub auto_key: bool,
    pub local_space: bool,
    pub selected_joint: Option<usize>,
    pub selected_event: Option<String>,
    pub timeline_mode: TimelineMode,
    pub transform_tool: TransformTool,
    pub start_pose: StartPose,
    pub dirty: bool,
    undo: Vec<AnimationAsset>,
    redo: Vec<AnimationAsset>,
    last_tick: Instant,
    last_disk_check: Instant,
    disk_modified: Option<SystemTime>,
}

impl AnimationEditorState {
    pub fn create(project: &Path, rig_asset_id: &str) -> Result<Self, String> {
        let registry = AssetRegistry::load(project)?;
        let rig = registry
            .find(rig_asset_id)
            .ok_or_else(|| format!("Rig asset '{rig_asset_id}' is unavailable"))?;
        if !matches!(rig.kind, AssetType::Actor | AssetType::Character) {
            return Err("Animations can only be added to Actors or Characters".into());
        }
        let rig_path = project.join(&rig.path);
        let package = std::sync::Arc::new(crate::character::package::load_package(&rig_path)?);
        let skeleton = package.skeleton.clone();
        let mut editor = Self::base(
            project,
            AnimationAsset::new(format!("{} Animation", rig.name), rig.id.clone()),
            None,
            rig_path,
            skeleton,
        );
        editor.set_preview_package(package);
        editor.setup_open = true;
        Ok(editor)
    }

    pub fn import(project: &Path, rig_asset_id: &str) -> Result<Self, String> {
        let mut editor = Self::create(project, rig_asset_id)?;
        editor.setup_open = false;
        let target_name = editor.draft.name.trim_end_matches(" Animation").to_owned();
        editor.importing = Some(crate::editor::animation_import::AnimationImportState::new(
            rig_asset_id.to_owned(),
            target_name,
        ));
        Ok(editor)
    }

    pub fn edit_imported(
        project: &Path,
        rig_asset_id: &str,
        clip_name: &str,
    ) -> Result<Self, String> {
        let registry = AssetRegistry::load(project)?;
        let rig = registry
            .find(rig_asset_id)
            .ok_or_else(|| format!("Rig asset '{rig_asset_id}' is unavailable"))?;
        let rig_path = project.join(&rig.path);
        let package = crate::character::package::load_package(&rig_path)?;
        let clip = package
            .clip_named(clip_name)
            .ok_or_else(|| format!("Imported clip '{clip_name}' is unavailable"))?;
        let mut draft = AnimationAsset::new(clip.name.clone(), rig.id.clone());
        draft.duration = clip.duration;
        draft.looping = clip.looping;
        draft.source.imported_clip = Some(clip.name.clone());
        draft.tracks = clip
            .tracks
            .iter()
            .filter_map(|track| {
                let joint = package.skeleton.joints.get(track.joint_index)?.name.clone();
                Some(TransformTrack {
                    joint_id: joint.clone(),
                    joint_name: joint,
                    translation: track
                        .translations
                        .iter()
                        .map(|key| Vec3Key {
                            time: key.time,
                            value: key.value.to_array(),
                            interpolation: Interpolation::Linear,
                            in_tangent: None,
                            out_tangent: None,
                        })
                        .collect(),
                    rotation: track
                        .rotations
                        .iter()
                        .map(|key| QuatKey {
                            time: key.time,
                            value: key.value.to_array(),
                            interpolation: Interpolation::Linear,
                        })
                        .collect(),
                    scale: track
                        .scales
                        .iter()
                        .map(|key| Vec3Key {
                            time: key.time,
                            value: key.value.to_array(),
                            interpolation: Interpolation::Linear,
                            in_tangent: None,
                            out_tangent: None,
                        })
                        .collect(),
                })
            })
            .collect();
        let skeleton = package.skeleton.clone();
        let mut editor = Self::base(project, draft, None, rig_path, skeleton);
        editor.set_preview_package(std::sync::Arc::new(package));
        editor.dirty = true;
        editor.write_recovery();
        editor.status = Some((
            "Editing a project override; imported source files stay unchanged.".into(),
            true,
        ));
        Ok(editor)
    }

    pub fn open(project: &Path, animation_dir: &Path) -> Result<Self, String> {
        let path = animation_dir.join(crate::animation::asset::ANIMATION_FILE);
        let draft = AnimationAsset::load(&path)?;
        let registry = AssetRegistry::load(project)?;
        let rig = registry.find(&draft.source.rig_asset_id).ok_or_else(|| {
            format!(
                "Animation rig '{}' is unavailable",
                draft.source.rig_asset_id
            )
        })?;
        let rig_path = project.join(&rig.path);
        let package = std::sync::Arc::new(crate::character::package::load_package(&rig_path)?);
        let skeleton = package.skeleton.clone();
        let modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok();
        let mut editor = Self::base(project, draft, Some(path), rig_path, skeleton);
        editor.set_preview_package(package);
        editor.disk_modified = modified;
        Ok(editor)
    }

    fn base(
        project: &Path,
        draft: AnimationAsset,
        asset_path: Option<PathBuf>,
        rig_path: PathBuf,
        skeleton: Skeleton,
    ) -> Self {
        Self {
            project: project.to_owned(),
            draft,
            asset_path,
            rig_path,
            skeleton,
            preview_package: None,
            preview_textures: std::collections::HashMap::new(),
            status: None,
            setup_open: false,
            importing: None,
            current_time: 0.0,
            playing: false,
            playback_rate: 1.0,
            auto_key: true,
            local_space: true,
            selected_joint: None,
            selected_event: None,
            timeline_mode: TimelineMode::Timeline,
            transform_tool: TransformTool::Rotate,
            start_pose: StartPose::RestPose,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_tick: Instant::now(),
            last_disk_check: Instant::now(),
            disk_modified: None,
        }
    }

    fn set_preview_package(
        &mut self,
        package: std::sync::Arc<crate::character::package::CharacterPackage>,
    ) {
        let mut textures = std::collections::HashMap::new();
        for path in package
            .geometry
            .iter()
            .filter_map(|part| part.base_color_texture.as_ref())
        {
            if textures.len() >= 8 {
                break;
            }
            if textures.contains_key(path) {
                continue;
            }
            if let Ok(image) = image::open(path) {
                textures.insert(
                    path.clone(),
                    std::sync::Arc::new(image.thumbnail(768, 768).to_rgba8()),
                );
            }
        }
        self.preview_textures = textures;
        self.preview_package = Some(package);
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.draft.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
        self.write_recovery();
    }

    fn undo(&mut self) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.draft, previous));
            self.dirty = true;
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.draft, next));
            self.dirty = true;
        }
    }

    fn save(&mut self) -> Result<(), String> {
        self.draft.normalize();
        let path = self.draft.save_authoritative(&self.project)?;
        AssetRegistry::reconcile(&self.project)?;
        self.disk_modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok();
        self.asset_path = Some(path);
        self.dirty = false;
        self.remove_recovery();
        self.status = Some(("Animation saved.".into(), true));
        Ok(())
    }

    fn recovery_path(&self) -> PathBuf {
        self.project
            .join(".assets/animations")
            .join(&self.draft.id)
            .join("animation.recovery.json")
    }

    fn write_recovery(&self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.draft) {
            let path = self.recovery_path();
            let _ = std::fs::create_dir_all(path.parent().expect("recovery has parent"));
            let _ = std::fs::write(path, bytes);
        }
    }

    fn remove_recovery(&self) {
        let _ = std::fs::remove_file(self.recovery_path());
    }

    fn refresh_external_edit(&mut self) {
        if self.last_disk_check.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_disk_check = Instant::now();
        let Some(path) = self.asset_path.as_ref() else {
            return;
        };
        let modified = std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok();
        if modified.is_none() || modified == self.disk_modified {
            return;
        }
        if self.dirty {
            self.disk_modified = modified;
            self.status = Some((
                "The file changed outside Worldkiln. Save will keep this Worldkiln version.".into(),
                false,
            ));
            return;
        }
        match AnimationAsset::load(path) {
            Ok(asset) => {
                self.draft = asset;
                self.disk_modified = modified;
                self.undo.clear();
                self.redo.clear();
                self.status = Some(("Animation refreshed from disk.".into(), true));
            }
            Err(error) => {
                self.status = Some((format!("External animation is invalid: {error}"), false))
            }
        }
    }

    fn tick(&mut self, ui: &Ui) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        if self.playing {
            self.current_time += elapsed.min(0.1) * self.playback_rate;
            if self.draft.looping {
                self.current_time = self.current_time.rem_euclid(self.draft.duration.max(0.001));
            } else if self.current_time >= self.draft.duration {
                self.current_time = self.draft.duration;
                self.playing = false;
            }
            ui.ctx().request_repaint();
        }
        self.refresh_external_edit();
    }

    fn selected_transform(&self) -> Option<Transform> {
        let index = self.selected_joint?;
        sample_pose(&self.draft, &self.skeleton, self.current_time)
            .get(index)
            .copied()
    }

    fn set_selected_rotation(&mut self, rotation: Quat) {
        let Some(index) = self.selected_joint else {
            return;
        };
        let joint = self.skeleton.joints[index].name.clone();
        let time = self.current_time;
        self.checkpoint();
        let track = self.track_mut(&joint);
        upsert_quat(&mut track.rotation, time, rotation);
    }

    fn set_selected_translation(&mut self, translation: Vec3) {
        let Some(index) = self.selected_joint else {
            return;
        };
        let joint = self.skeleton.joints[index].name.clone();
        let time = self.current_time;
        self.checkpoint();
        let track = self.track_mut(&joint);
        upsert_vec3(&mut track.translation, time, translation);
    }

    fn set_selected_scale(&mut self, scale: Vec3) {
        let Some(index) = self.selected_joint else {
            return;
        };
        let joint = self.skeleton.joints[index].name.clone();
        let time = self.current_time;
        self.checkpoint();
        let track = self.track_mut(&joint);
        upsert_vec3(&mut track.scale, time, scale);
    }

    fn track_mut(&mut self, joint: &str) -> &mut TransformTrack {
        if let Some(index) = self.draft.tracks.iter().position(|track| {
            track.joint_id.eq_ignore_ascii_case(joint)
                || track.joint_name.eq_ignore_ascii_case(joint)
        }) {
            return &mut self.draft.tracks[index];
        }
        self.draft.tracks.push(TransformTrack {
            joint_id: joint.into(),
            joint_name: joint.into(),
            ..Default::default()
        });
        self.draft.tracks.last_mut().expect("track was inserted")
    }

    fn add_event(&mut self) {
        self.checkpoint();
        let id = format!("{:032x}", rand::random::<u128>());
        self.draft.events.push(AnimationEvent {
            id: id.clone(),
            time: self.current_time,
            name: "event".into(),
            payload: BTreeMap::new(),
        });
        self.draft.normalize();
        self.selected_event = Some(id);
    }
}

fn upsert_vec3(keys: &mut Vec<Vec3Key>, time: f32, value: Vec3) {
    if let Some(key) = keys.iter_mut().find(|key| (key.time - time).abs() < 0.0001) {
        key.value = value.to_array();
    } else {
        keys.push(Vec3Key {
            time,
            value: value.to_array(),
            interpolation: Interpolation::Linear,
            in_tangent: None,
            out_tangent: None,
        });
        keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}

fn upsert_quat(keys: &mut Vec<QuatKey>, time: f32, value: Quat) {
    if let Some(key) = keys.iter_mut().find(|key| (key.time - time).abs() < 0.0001) {
        key.value = value.normalize().to_array();
    } else {
        keys.push(QuatKey {
            time,
            value: value.normalize().to_array(),
            interpolation: Interpolation::Linear,
        });
        keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}

pub(crate) fn draw(ui: &mut Ui, editor: &mut AnimationEditorState) -> EditorAction {
    if let Some(mut importer) = editor.importing.take() {
        let action = crate::editor::animation_import::draw(ui, editor, &mut importer);
        if matches!(action, EditorAction::None) {
            editor.importing = Some(importer);
        }
        return action;
    }
    editor.tick(ui);
    let modifiers = ui.ctx().input(|input| input.modifiers);
    if modifiers.command && ui.ctx().input(|input| input.key_pressed(egui::Key::S)) {
        if let Err(error) = editor.save() {
            editor.status = Some((error, false));
        }
    }
    if modifiers.command && ui.ctx().input(|input| input.key_pressed(egui::Key::Z)) {
        editor.undo();
    }
    if modifiers.command && ui.ctx().input(|input| input.key_pressed(egui::Key::Y)) {
        editor.redo();
    }
    if ui.ctx().input(|input| input.key_pressed(egui::Key::Space)) {
        editor.playing = !editor.playing;
    }

    let mut action = EditorAction::None;
    ui.horizontal_wrapped(|ui| {
        ui.heading("Animation Editor");
        ui.separator();
        let available = crate::animation::asset::registered_clips_for_rig(
            &editor.project,
            &editor.draft.source.rig_asset_id,
        );
        let mut selected_clip = None;
        egui::ComboBox::from_id_salt("animation_clip_selector")
            .selected_text(&editor.draft.name)
            .show_ui(ui, |ui| {
                if editor.asset_path.is_none() {
                    ui.selectable_label(true, &editor.draft.name);
                }
                for (name, path) in &available {
                    if ui
                        .selectable_label(
                            editor
                                .asset_path
                                .as_ref()
                                .is_some_and(|current| current.parent() == Some(path.as_path())),
                            name,
                        )
                        .clicked()
                    {
                        selected_clip = Some(path.clone());
                    }
                }
            });
        if let Some(path) = selected_clip {
            if editor.dirty {
                editor.status = Some((
                    "Save or discard unsaved animation edits before changing clips.".into(),
                    false,
                ));
            } else {
                match AnimationEditorState::open(&editor.project, &path) {
                    Ok(next) => *editor = next,
                    Err(error) => editor.status = Some((error, false)),
                }
            }
        }
        if ui.button("New").clicked() {
            editor.status = Some((
                "Use Add Animation... on an Actor or Character.".into(),
                true,
            ));
        }
        if ui.button("Duplicate").clicked() {
            editor.checkpoint();
            editor.draft.id = format!("{:032x}", rand::random::<u128>());
            editor.draft.name.push_str(" Copy");
            editor.asset_path = None;
        }
        if ui.button("Save").clicked() {
            match editor.save() {
                Ok(()) => action = EditorAction::Saved,
                Err(error) => editor.status = Some((error, false)),
            }
        }
        if ui.button("Save As").clicked() {
            editor.checkpoint();
            editor.draft.id = format!("{:032x}", rand::random::<u128>());
            editor.draft.name.push_str(" Copy");
            editor.asset_path = None;
            match editor.save() {
                Ok(()) => action = EditorAction::Saved,
                Err(error) => editor.status = Some((error, false)),
            }
        }
        if ui
            .add_enabled(!editor.undo.is_empty(), egui::Button::new("Undo"))
            .clicked()
        {
            editor.undo();
        }
        if ui
            .add_enabled(!editor.redo.is_empty(), egui::Button::new("Redo"))
            .clicked()
        {
            editor.redo();
        }
        ui.separator();
        let has_joint = editor.selected_joint.is_some();
        if ui
            .add_enabled(
                has_joint,
                egui::Button::new("Move").selected(editor.transform_tool == TransformTool::Move),
            )
            .clicked()
        {
            editor.transform_tool = TransformTool::Move;
        }
        if ui
            .add_enabled(
                has_joint,
                egui::Button::new("Rotate")
                    .selected(editor.transform_tool == TransformTool::Rotate),
            )
            .clicked()
        {
            editor.transform_tool = TransformTool::Rotate;
        }
        if ui
            .add_enabled(
                has_joint,
                egui::Button::new("Scale").selected(editor.transform_tool == TransformTool::Scale),
            )
            .clicked()
        {
            editor.transform_tool = TransformTool::Scale;
        }
        ui.selectable_value(&mut editor.local_space, true, "Local");
        ui.selectable_value(&mut editor.local_space, false, "World");
        ui.toggle_value(&mut editor.auto_key, "Auto Key");
        if ui.button("Keyframe").clicked() {
            if let Some(transform) = editor.selected_transform() {
                editor.set_selected_rotation(transform.rotation);
            }
        }
        if ui.button("Event").clicked() {
            editor.add_event();
        }
        if ui.button("Close").clicked() {
            action = EditorAction::Close;
        }
    });
    ui.separator();

    if editor.setup_open {
        draw_setup(ui, editor);
        return action;
    }

    ui.columns(3, |columns| {
        columns[0].set_min_width(180.0);
        columns[0].label(RichText::new("Rig").strong());
        egui::ScrollArea::vertical()
            .id_salt("animation_rig_tree")
            .max_height(410.0)
            .show(&mut columns[0], |ui| {
                for (index, joint) in editor.skeleton.joints.iter().enumerate() {
                    let depth = joint_depth(&editor.skeleton, index);
                    ui.horizontal(|ui| {
                        ui.add_space(depth as f32 * 12.0);
                        if ui
                            .selectable_label(editor.selected_joint == Some(index), &joint.name)
                            .clicked()
                        {
                            editor.selected_joint = Some(index);
                            editor.selected_event = None;
                        }
                    });
                }
            });

        columns[1].label(RichText::new("Preview").strong());
        let (preview, _) = columns[1].allocate_exact_size(
            egui::vec2(columns[1].available_width(), 410.0),
            Sense::click_and_drag(),
        );
        columns[1]
            .painter()
            .rect_filled(preview, 3.0, columns[1].visuals().faint_bg_color);
        let animated = editor.preview_package.as_ref().is_some_and(|package| {
            crate::editor::animation_import::paint_skinned_preview(
                &columns[1],
                preview,
                package,
                &editor.draft,
                editor.current_time,
                &editor.preview_textures,
            )
        });
        if !animated {
            columns[1].painter().text(
                preview.center(),
                egui::Align2::CENTER_CENTER,
                "No compatible skinned geometry is available for this animation.",
                egui::TextStyle::Body.resolve(columns[1].style()),
                columns[1].visuals().text_color(),
            );
        }

        columns[2].set_min_width(220.0);
        columns[2].label(RichText::new("Properties").strong());
        draw_properties(&mut columns[2], editor);
    });

    ui.separator();
    draw_transport(ui, editor);
    ui.horizontal(|ui| {
        ui.selectable_value(
            &mut editor.timeline_mode,
            TimelineMode::Timeline,
            "Timeline",
        );
        ui.selectable_value(&mut editor.timeline_mode, TimelineMode::Curves, "Curves");
    });
    match editor.timeline_mode {
        TimelineMode::Timeline => draw_timeline(ui, editor),
        TimelineMode::Curves => draw_curves(ui, editor),
    }
    if let Some((message, ok)) = &editor.status {
        ui.colored_label(
            if *ok {
                ui.visuals().text_color()
            } else {
                ui.visuals().error_fg_color
            },
            message,
        );
    }
    action
}

fn draw_setup(ui: &mut Ui, editor: &mut AnimationEditorState) {
    ui.heading("Add Animation");
    egui::Grid::new("animation_setup")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut editor.draft.name);
            ui.end_row();
            ui.label("Start from");
            egui::ComboBox::from_id_salt("animation_start_pose")
                .selected_text(match editor.start_pose {
                    StartPose::RestPose => "Rest pose",
                    StartPose::CurrentPose => "Current pose",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor.start_pose, StartPose::RestPose, "Rest pose");
                    ui.selectable_value(
                        &mut editor.start_pose,
                        StartPose::CurrentPose,
                        "Current pose",
                    );
                });
            ui.end_row();
            ui.label("Length");
            ui.add(
                egui::DragValue::new(&mut editor.draft.duration)
                    .range(0.01..=3600.0)
                    .suffix(" s"),
            );
            ui.end_row();
            ui.label("FPS");
            ui.add(egui::DragValue::new(&mut editor.draft.frames_per_second).range(1.0..=240.0));
            ui.end_row();
            ui.label("Loop");
            ui.checkbox(&mut editor.draft.looping, "");
            ui.end_row();
        });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !editor.draft.name.trim().is_empty(),
                egui::Button::new("Create"),
            )
            .clicked()
        {
            editor.setup_open = false;
            editor.dirty = true;
            editor.write_recovery();
        }
        if ui.button("Cancel").clicked() {
            editor.status = Some(("Close the editor to cancel.".into(), true));
        }
    });
}

fn draw_properties(ui: &mut Ui, editor: &mut AnimationEditorState) {
    if let Some(event_id) = editor.selected_event.clone() {
        let Some(index) = editor
            .draft
            .events
            .iter()
            .position(|event| event.id == event_id)
        else {
            return;
        };
        let before = editor.draft.clone();
        let event = &mut editor.draft.events[index];
        ui.label(RichText::new("Event Frame").strong());
        ui.text_edit_singleline(&mut event.name);
        ui.add(
            egui::DragValue::new(&mut event.time)
                .range(0.0..=editor.draft.duration)
                .suffix(" s"),
        );
        ui.label("Payload");
        if event.payload.is_empty() {
            ui.small("No payload values");
        }
        if ui.button("Add text value").clicked() {
            event
                .payload
                .insert("value".into(), EventValue::Text(String::new()));
        }
        if before != editor.draft {
            editor.undo.push(before);
            editor.redo.clear();
            editor.dirty = true;
        }
        return;
    }
    let Some(transform) = editor.selected_transform() else {
        ui.label("Select a joint or event.");
        return;
    };
    let joint_name = &editor.skeleton.joints[editor.selected_joint.unwrap()].name;
    ui.label(RichText::new(joint_name).strong());
    let mut translation = transform.translation;
    let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
    let mut degrees = Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees());
    let mut scale = transform.scale;
    ui.label("Translation");
    let changed_translation = vector_controls(ui, &mut translation, 0.01);
    ui.label("Rotation");
    let changed_rotation = vector_controls(ui, &mut degrees, 0.25);
    ui.label("Scale");
    let changed_scale = vector_controls(ui, &mut scale, 0.01);
    if changed_translation && editor.auto_key {
        editor.set_selected_translation(translation);
    }
    if changed_rotation && editor.auto_key {
        let radians = Vec3::new(
            degrees.x.to_radians(),
            degrees.y.to_radians(),
            degrees.z.to_radians(),
        );
        editor.set_selected_rotation(Quat::from_euler(
            EulerRot::XYZ,
            radians.x,
            radians.y,
            radians.z,
        ));
    }
    if changed_scale && editor.auto_key {
        editor.set_selected_scale(scale);
    }
}

fn vector_controls(ui: &mut Ui, value: &mut Vec3, speed: f64) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        for (label, component) in [
            ("X", &mut value.x),
            ("Y", &mut value.y),
            ("Z", &mut value.z),
        ] {
            ui.label(label);
            changed |= ui
                .add(egui::DragValue::new(component).speed(speed))
                .changed();
        }
    });
    changed
}

fn draw_transport(ui: &mut Ui, editor: &mut AnimationEditorState) {
    ui.horizontal(|ui| {
        if ui.button("|<").clicked() {
            editor.current_time = 0.0;
            editor.playing = false;
        }
        if ui
            .button(if editor.playing { "Pause" } else { "Play" })
            .clicked()
        {
            editor.playing = !editor.playing;
        }
        if ui.button(">|").clicked() {
            editor.current_time = editor.draft.duration;
            editor.playing = false;
        }
        ui.add(
            egui::Slider::new(&mut editor.current_time, 0.0..=editor.draft.duration)
                .show_value(false),
        );
        ui.monospace(format!(
            "{:.3} s / {:.3} s",
            editor.current_time, editor.draft.duration
        ));
        ui.add(
            egui::DragValue::new(&mut editor.playback_rate)
                .range(0.05..=4.0)
                .suffix("x"),
        );
        ui.checkbox(&mut editor.draft.looping, "Loop");
    });
}

fn draw_timeline(ui: &mut Ui, editor: &mut AnimationEditorState) {
    let height = (editor.draft.tracks.len().max(1) as f32 * 24.0 + 46.0).min(220.0);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    ui.painter()
        .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    let label_width = 150.0;
    let time_left = rect.left() + label_width;
    let time_width = (rect.width() - label_width).max(1.0);
    if let Some(pointer) = response.interact_pointer_pos() {
        editor.current_time = ((pointer.x - time_left) / time_width * editor.draft.duration)
            .clamp(0.0, editor.draft.duration);
    }
    for (row, track) in editor.draft.tracks.iter().enumerate() {
        let y = rect.top() + 24.0 + row as f32 * 24.0;
        ui.painter().text(
            egui::pos2(rect.left() + 5.0, y),
            egui::Align2::LEFT_CENTER,
            &track.joint_name,
            egui::TextStyle::Small.resolve(ui.style()),
            ui.visuals().text_color(),
        );
        for time in track
            .translation
            .iter()
            .map(|key| key.time)
            .chain(track.rotation.iter().map(|key| key.time))
            .chain(track.scale.iter().map(|key| key.time))
        {
            let x = time_left + time / editor.draft.duration * time_width;
            let diamond = [
                egui::pos2(x, y - 5.0),
                egui::pos2(x + 5.0, y),
                egui::pos2(x, y + 5.0),
                egui::pos2(x - 5.0, y),
            ];
            ui.painter().add(egui::Shape::convex_polygon(
                diamond.to_vec(),
                Color32::from_rgb(255, 196, 64),
                Stroke::NONE,
            ));
        }
    }
    let event_y = rect.bottom() - 14.0;
    ui.painter().text(
        egui::pos2(rect.left() + 5.0, event_y),
        egui::Align2::LEFT_CENTER,
        "Events",
        egui::TextStyle::Small.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    let mut event_click = None;
    for event in &editor.draft.events {
        let x = time_left + event.time / editor.draft.duration * time_width;
        let event_rect =
            egui::Rect::from_center_size(egui::pos2(x, event_y), egui::vec2(10.0, 14.0));
        ui.painter()
            .rect_filled(event_rect, 2.0, Color32::from_rgb(80, 190, 245));
        if response.clicked()
            && response
                .interact_pointer_pos()
                .is_some_and(|point| event_rect.contains(point))
        {
            event_click = Some(event.id.clone());
        }
    }
    if let Some(id) = event_click {
        editor.selected_event = Some(id);
        editor.selected_joint = None;
    }
    let playhead_x = time_left + editor.current_time / editor.draft.duration * time_width;
    ui.painter().line_segment(
        [
            egui::pos2(playhead_x, rect.top()),
            egui::pos2(playhead_x, rect.bottom()),
        ],
        Stroke::new(1.5, Color32::LIGHT_RED),
    );
}

fn draw_curves(ui: &mut Ui, editor: &AnimationEditorState) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 180.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    for fraction in [0.25, 0.5, 0.75] {
        let y = egui::lerp(rect.top()..=rect.bottom(), fraction);
        ui.painter().line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );
    }
    if let Some(index) = editor.selected_joint {
        let name = &editor.skeleton.joints[index].name;
        if let Some(track) = editor
            .draft
            .tracks
            .iter()
            .find(|track| track.joint_name.eq_ignore_ascii_case(name))
        {
            let colors = [Color32::RED, Color32::GREEN, Color32::LIGHT_BLUE];
            for axis in 0..3 {
                let points = track
                    .translation
                    .iter()
                    .map(|key| {
                        let x = rect.left() + key.time / editor.draft.duration * rect.width();
                        let y = rect.center().y - key.value[axis] * 20.0;
                        egui::pos2(x, y)
                    })
                    .collect::<Vec<_>>();
                if points.len() >= 2 {
                    ui.painter()
                        .add(egui::Shape::line(points, Stroke::new(1.5, colors[axis])));
                }
            }
        }
    } else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Select a joint to edit curves",
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
    }
}

fn paint_bones(ui: &Ui, rect: egui::Rect, editor: &AnimationEditorState) {
    let pose = sample_pose(&editor.draft, &editor.skeleton, editor.current_time);
    if pose.is_empty() {
        return;
    }
    let evaluated = editor.skeleton.evaluate_pose(&pose);
    let points = evaluated
        .matrices
        .iter()
        .map(|matrix| matrix.transform_point3(Vec3::ZERO))
        .collect::<Vec<_>>();
    let min = points
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |value, point| value.min(*point));
    let max = points
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |value, point| {
            value.max(*point)
        });
    let span = (max - min).max(Vec3::splat(0.001));
    let project = |point: Vec3| {
        egui::pos2(
            rect.left() + 0.15 * rect.width() + (point.x - min.x) / span.x * rect.width() * 0.7,
            rect.bottom() - 0.1 * rect.height() - (point.y - min.y) / span.y * rect.height() * 0.8,
        )
    };
    for (index, joint) in editor.skeleton.joints.iter().enumerate() {
        let point = project(points[index]);
        if let Some(parent) = joint.parent_index {
            ui.painter().line_segment(
                [project(points[parent]), point],
                Stroke::new(2.0, Color32::from_rgb(255, 180, 50)),
            );
        }
        ui.painter().circle_filled(
            point,
            if editor.selected_joint == Some(index) {
                5.0
            } else {
                3.0
            },
            if editor.selected_joint == Some(index) {
                Color32::WHITE
            } else {
                Color32::from_rgb(255, 180, 50)
            },
        );
    }
}

fn joint_depth(skeleton: &Skeleton, mut index: usize) -> usize {
    let mut depth = 0;
    while let Some(parent) = skeleton.joints[index].parent_index {
        depth += 1;
        index = parent;
        if depth > skeleton.joints.len() {
            break;
        }
    }
    depth
}
