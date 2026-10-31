// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::animation::retarget::{self, MappingReport, RetargetSettings, RootMotion, SourceInfo};
use crate::animation::{AnimationAsset, sample_pose};
use crate::character::package::{CharacterPackage, GeometryPrimitive};
use crate::character::rig::{Skeleton, Transform};
use crate::editor::animation_editor::{AnimationEditorState, EditorAction};
use egui::{Color32, RichText, Sense, Stroke, Ui};
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};

#[derive(Clone, Debug)]
struct ClipChoice {
    name: String,
    selected: bool,
    looping: bool,
}

#[derive(Debug)]
struct PreviewData {
    clip: AnimationAsset,
    package: Arc<CharacterPackage>,
    textures: HashMap<PathBuf, Arc<image::RgbaImage>>,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationImportState {
    target_id: String,
    target_name: String,
    source_file: Option<PathBuf>,
    entries: Vec<(String, SourceInfo)>,
    entry_index: usize,
    skin_index: usize,
    clips: Vec<ClipChoice>,
    source_skeleton: Option<Skeleton>,
    mapping: Option<MappingReport>,
    manual: BTreeMap<String, String>,
    settings: RetargetSettings,
    preview_clip: usize,
    preview_active: bool,
    target_package: Option<Arc<CharacterPackage>>,
    preview_textures: HashMap<PathBuf, Arc<image::RgbaImage>>,
    preview_working: Option<Arc<Mutex<mpsc::Receiver<Result<PreviewData, String>>>>>,
    status: Option<(String, bool)>,
    working: Option<Arc<Mutex<mpsc::Receiver<Result<Vec<String>, String>>>>>,
    inspect_working: Option<Arc<Mutex<mpsc::Receiver<Result<Vec<(String, SourceInfo)>, String>>>>>,
}

impl AnimationImportState {
    pub(crate) fn new(target_id: String, target_name: String) -> Self {
        Self {
            target_id,
            target_name,
            source_file: None,
            entries: Vec::new(),
            entry_index: 0,
            skin_index: 0,
            clips: Vec::new(),
            source_skeleton: None,
            mapping: None,
            manual: BTreeMap::new(),
            settings: RetargetSettings::default(),
            preview_clip: 0,
            preview_active: false,
            target_package: None,
            preview_textures: HashMap::new(),
            preview_working: None,
            status: None,
            working: None,
            inspect_working: None,
        }
    }

    fn is_archive(&self) -> bool {
        self.source_file
            .as_ref()
            .is_some_and(|p| p.extension().is_some_and(|v| v.eq_ignore_ascii_case("zip")))
    }
    fn current(&self) -> Option<&SourceInfo> {
        self.entries.get(self.entry_index).map(|(_, v)| v)
    }
    fn source(&self) -> Result<retarget::SourceLibrary, String> {
        let file = self
            .source_file
            .as_ref()
            .ok_or("Choose an animation file")?;
        let skin = self.skin_index;
        let name = self
            .clips
            .iter()
            .find(|c| c.selected)
            .or_else(|| self.clips.first())
            .map(|c| c.name.clone())
            .ok_or("No animation clips found")?;
        self.load_named(file, skin, &[name])
    }
    fn load_named(
        &self,
        file: &std::path::Path,
        skin: usize,
        names: &[String],
    ) -> Result<retarget::SourceLibrary, String> {
        if self.is_archive() {
            let entry = &self
                .entries
                .get(self.entry_index)
                .ok_or("No archive GLB selected")?
                .0;
            retarget::load_zip_glb(file, entry, skin, names)
        } else {
            retarget::load_glb(file, skin, names)
        }
    }
    fn inspect_file(&mut self, path: PathBuf) {
        self.entries.clear();
        self.clips.clear();
        self.mapping = None;
        self.source_skeleton = None;
        self.preview_active = false;
        self.source_file = Some(path.clone());
        let (send, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("zip"))
            {
                retarget::inspect_archive(&path)
            } else {
                retarget::inspect(&path).map(|info| {
                    vec![(
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string(),
                        info,
                    )]
                })
            };
            let _ = send.send(result);
        });
        self.inspect_working = Some(Arc::new(Mutex::new(receiver)));
        self.status = Some(("Inspecting animations...".into(), true));
    }
    fn poll_inspect(&mut self) {
        let Some(receiver) = self.inspect_working.as_ref() else {
            return;
        };
        let result = receiver.lock().ok().map(|rx| rx.try_recv());
        match result {
            Some(Ok(Ok(entries))) => {
                self.inspect_working = None;
                self.entries = entries;
                self.entry_index = 0;
                self.skin_index = 0;
                self.manual.clear();
                self.status = None;
                self.choose_entry();
            }
            Some(Ok(Err(error))) => {
                self.inspect_working = None;
                self.source_file = None;
                crate::engine::diagnostics::report("ERROR", "Animation Import", error.clone());
                self.status = Some((error, false));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.inspect_working = None;
                self.status = Some(("Animation inspection stopped unexpectedly".into(), false));
            }
            _ => {}
        }
    }
    fn choose_entry(&mut self) {
        self.clips = self
            .current()
            .map(|entry| {
                entry
                    .clips
                    .iter()
                    .map(|clip| ClipChoice {
                        name: clip.name.clone(),
                        selected: true,
                        looping: crate::animation::asset::suggested_looping(&clip.name),
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.skin_index = self
            .current()
            .and_then(|s| s.skins.first().map(|skin| skin.0))
            .unwrap_or(0);
        self.manual.clear();
        self.mapping = None;
        self.source_skeleton = None;
        self.preview_active = false;
        self.preview_clip = 0;
        self.reload_mapping();
    }
    fn reload_mapping(&mut self) {
        match self.source() {
            Ok(library) => {
                self.source_skeleton = Some(library.skeleton.clone());
                self.mapping = None;
            }
            Err(error) => {
                self.source_skeleton = None;
                self.mapping = None;
                self.status = Some((error, false));
            }
        }
    }
    fn update_mapping(&mut self, target: &Skeleton) {
        if let Some(source) = &self.source_skeleton {
            match retarget::map_joints(source, target, &self.manual) {
                Ok(mapping) => {
                    self.mapping = Some(mapping);
                    self.status = None;
                }
                Err(error) => {
                    self.mapping = None;
                    self.status = Some((error, false));
                }
            }
        }
    }
    fn selected_names(&self) -> Vec<String> {
        self.clips
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.name.clone())
            .collect()
    }
    fn retarget_settings(&self) -> RetargetSettings {
        let mut settings = self.settings.clone();
        settings.clip_looping = self
            .clips
            .iter()
            .map(|c| (c.name.clone(), c.looping))
            .collect();
        settings
    }
    fn preview_selected(&mut self, editor: &AnimationEditorState) {
        if self.preview_working.is_some() {
            return;
        }
        let Some(file) = self.source_file.clone() else {
            return;
        };
        let Some(clip) = self.clips.get(self.preview_clip) else {
            return;
        };
        let names = vec![clip.name.clone()];
        let archive = self.is_archive();
        let entry = self
            .entries
            .get(self.entry_index)
            .map(|item| item.0.clone())
            .unwrap_or_default();
        let skin = self.skin_index;
        let manual = self.manual.clone();
        let settings = self.retarget_settings();
        let target_id = self.target_id.clone();
        let target_skeleton = editor.skeleton.clone();
        let rig_path = editor.rig_path.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = (|| -> Result<PreviewData, String> {
                let library = if archive {
                    retarget::load_zip_glb(&file, &entry, skin, &names)?
                } else {
                    retarget::load_glb(&file, skin, &names)?
                };
                let mapping = retarget::map_joints(&library.skeleton, &target_skeleton, &manual)?;
                if mapping.pairs.is_empty() {
                    return Err("No compatible joints mapped".into());
                }
                if !mapping.ambiguous.is_empty() {
                    return Err("Resolve ambiguous bone mappings to preview".into());
                }
                let clip = retarget::retarget_clip(
                    &library.skeleton,
                    &target_skeleton,
                    &library.clips[0],
                    &mapping,
                    &settings,
                    &target_id,
                )?;
                let package = Arc::new(crate::character::package::load_package(&rig_path)?);
                let mut textures = HashMap::new();
                for texture in package
                    .geometry
                    .iter()
                    .filter_map(|part| part.base_color_texture.as_ref())
                {
                    if textures.contains_key(texture) || textures.len() >= 8 {
                        continue;
                    }
                    if let Ok(image) = image::open(texture) {
                        textures.insert(
                            texture.clone(),
                            Arc::new(image.thumbnail(768, 768).to_rgba8()),
                        );
                    }
                }
                Ok(PreviewData {
                    clip,
                    package,
                    textures,
                })
            })();
            let _ = sender.send(result);
        });
        self.preview_working = Some(Arc::new(Mutex::new(receiver)));
        self.status = Some(("Preparing character preview...".into(), true));
    }
    fn poll_preview(&mut self, editor: &mut AnimationEditorState) {
        let Some(receiver) = self.preview_working.as_ref() else {
            return;
        };
        let response = receiver.lock().ok().map(|rx| rx.try_recv());
        match response {
            Some(Ok(Ok(preview))) => {
                self.preview_working = None;
                editor.draft = preview.clip;
                editor.current_time = 0.0;
                editor.playing = false;
                self.target_package = Some(preview.package);
                self.preview_textures = preview.textures;
                self.preview_active = true;
                self.status = None;
            }
            Some(Ok(Err(error))) => {
                self.preview_working = None;
                crate::engine::diagnostics::report("ERROR", "Animation Import", error.clone());
                self.status = Some((error, false));
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.preview_working = None;
                self.status = Some((
                    "Character preview worker stopped unexpectedly".into(),
                    false,
                ));
            }
            _ => {}
        }
    }

    fn start_import(&mut self, project: PathBuf) {
        let Some(file) = self.source_file.clone() else {
            return;
        };
        let names = self.selected_names();
        if names.is_empty() {
            self.status = Some(("Select at least one animation".into(), false));
            return;
        }
        let Some(mapping) = self.mapping.as_ref() else {
            self.status = Some(("Resolve the joint mapping before importing".into(), false));
            return;
        };
        if mapping.pairs.is_empty() {
            self.status = Some((
                "No mapped joints; use Bone Mapping before importing".into(),
                false,
            ));
            return;
        }
        if !mapping.ambiguous.is_empty() {
            self.status = Some((
                "Resolve ambiguous bone mappings before importing".into(),
                false,
            ));
            return;
        }
        let archive = self.is_archive();
        let entry = self.entries[self.entry_index].0.clone();
        let skin = self.skin_index;
        let manual = self.manual.clone();
        let settings = self.retarget_settings();
        let target = self.target_id.clone();
        let (send, recv) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = if archive {
                retarget::import_archive_clips(
                    &project, &target, &file, &entry, skin, &names, &manual, &settings,
                )
            } else {
                retarget::import_clips(&project, &target, &file, skin, &names, &manual, &settings)
            };
            let result = result.map(|(ids, _, warnings)| {
                if !warnings.is_empty() {
                    crate::engine::diagnostics::report(
                        "WARNING",
                        "Animation Import",
                        warnings.join("; "),
                    );
                }
                ids
            });
            let _ = send.send(result);
        });
        self.working = Some(Arc::new(Mutex::new(recv)));
        self.status = Some(("Importing selected animations...".into(), true));
    }
    fn poll_import(&mut self) -> Option<Result<Vec<String>, String>> {
        let receiver = self.working.as_ref()?;
        let response = receiver.lock().ok()?.try_recv();
        match response {
            Ok(result) => {
                self.working = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.working = None;
                Some(Err("Animation import worker stopped unexpectedly".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}

pub(crate) fn draw(
    ui: &mut Ui,
    editor: &mut AnimationEditorState,
    state: &mut AnimationImportState,
) -> EditorAction {
    state.poll_inspect();
    state.poll_preview(editor);
    if state.inspect_working.is_none() && state.mapping.is_none() && state.source_skeleton.is_some()
    {
        state.update_mapping(&editor.skeleton);
    }
    if let Some(result) = state.poll_import() {
        match result {
            Ok(ids) => {
                if let Some(id) = ids.first() {
                    if let Ok(path) =
                        crate::animation::asset::animation_asset_path(&editor.project, id)
                    {
                        let dir = path.parent().expect("animation path has parent");
                        match AnimationEditorState::open(&editor.project, dir) {
                            Ok(next) => {
                                *editor = next;
                                editor.status =
                                    Some((format!("Imported {} animations.", ids.len()), true));
                                return EditorAction::Close;
                            }
                            Err(error) => {
                                state.status = Some((
                                    format!("Clips were saved but could not be reopened: {error}"),
                                    false,
                                ));
                            }
                        }
                    }
                }
            }
            Err(error) => {
                crate::engine::diagnostics::report("ERROR", "Animation Import", error.clone());
                state.status = Some((error, false));
            }
        }
    }
    let busy = state.working.is_some()
        || state.inspect_working.is_some()
        || state.preview_working.is_some();
    let mut cancelled = false;
    ui.horizontal(|ui| {
        ui.heading("Import Animations");
        ui.separator();
        ui.label("Target rig:");
        ui.strong(&state.target_name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(!busy, egui::Button::new("✕"))
                .on_hover_text("Close Animation Import")
                .clicked()
            {
                cancelled = true;
            }
        });
    });
    ui.separator();

    egui::ScrollArea::vertical().id_salt("animation_import_scroll").show(ui,|ui|{
        ui.horizontal(|ui|{
            ui.label("Source File");
            let file_label=state.source_file.as_ref().and_then(|p|p.file_name())
                .map(|p|p.to_string_lossy().into_owned())
                .unwrap_or_else(||"Choose an animation file".into());
            ui.label(file_label);
            if ui.add_enabled(!busy,egui::Button::new("Browse...")).clicked(){
                if let Some(path)=rfd::FileDialog::new()
                    .add_filter("Animation files",&["glb","zip"]).pick_file(){
                    state.inspect_file(path);
                    state.update_mapping(&editor.skeleton);
                }
            }
        });
        if state.is_archive() && state.entries.len()>1 {
            ui.horizontal(|ui|{
                ui.label("Archive GLB");
                let mut choice=state.entry_index;
                egui::ComboBox::from_id_salt("import_archive_source")
                    .selected_text(&state.entries[choice].0)
                    .show_ui(ui,|ui|{
                        for (index,(name,_)) in state.entries.iter().enumerate(){
                            ui.selectable_value(&mut choice,index,name);
                        }
                    });
                if choice!=state.entry_index && !busy{
                    state.entry_index=choice;
                    state.choose_entry();
                    state.update_mapping(&editor.skeleton);
                }
            });
        }
        if let Some(info)=state.current(){
            if info.skins.len()>1{
                let choices=info.skins.clone();
                ui.horizontal(|ui|{
                    ui.label("Source Skin");
                    let mut selected=state.skin_index;
                    egui::ComboBox::from_id_salt("import_source_skin")
                        .selected_text(choices.iter().find(|s|s.0==selected)
                            .map(|s|s.1.as_str()).unwrap_or("Select skeleton"))
                        .show_ui(ui,|ui|{
                            for (index,name,count) in &choices{
                                ui.selectable_value(&mut selected,*index,
                                    format!("{name} ({count} joints)"));
                            }
                        });
                    if selected!=state.skin_index && !busy{
                        state.skin_index=selected;
                        state.manual.clear();
                        state.reload_mapping();
                        state.update_mapping(&editor.skeleton);
                    }
                });
            }
        }
        if !state.clips.is_empty() {
            ui.separator();
            ui.horizontal(|ui|{
                ui.strong(format!("Animation clips ({})",state.clips.len()));
                if ui.add_enabled(!busy,egui::Button::new("Select All")).clicked(){
                    for clip in &mut state.clips{clip.selected=true;}
                }
                if ui.add_enabled(!busy,egui::Button::new("Clear")).clicked(){
                    for clip in &mut state.clips{clip.selected=false;}
                }
            });
            let mut clip_preview_requested=false;
            egui::CollapsingHeader::new(format!("Clips ({})",state.clips.len()))
                .id_salt("animation_import_clips")
                .default_open(false)
                .show(ui,|ui|{
                    for (index,clip) in state.clips.iter_mut().enumerate(){
                        ui.horizontal_wrapped(|ui|{
                            ui.add_enabled_ui(!busy,|ui|{
                                ui.checkbox(&mut clip.selected,&clip.name);
                                ui.checkbox(&mut clip.looping,"Loop");
                                if ui.selectable_label(state.preview_clip==index,"Preview").clicked(){
                                    state.preview_clip=index;
                                    clip_preview_requested=true;
                                }
                            });
                        });
                    }
                });
            if clip_preview_requested {state.preview_selected(editor);}
            ui.separator();
            let mapping_counts=state.mapping.as_ref().map(|m|
                (m.pairs.len(),m.unmapped_source.len(),m.unmapped_target.len(),m.ambiguous.len())
            ).unwrap_or_default();
            ui.horizontal(|ui|{
                if ui.add_enabled(!busy,egui::Button::new("Auto-map")).clicked(){
                    state.manual.clear();
                    state.update_mapping(&editor.skeleton);
                }
                ui.label(format!("Mapped: {} · Unmapped source: {} · Unmapped target: {} · Ambiguous: {}",
                    mapping_counts.0,mapping_counts.1,mapping_counts.2,mapping_counts.3));
            });
            egui::CollapsingHeader::new("Bone Mapping").id_salt("animation_import_mapping")
                .default_open(false).show(ui,|ui|{
                    if let Some(source)=state.source_skeleton.as_ref(){
                        let pairs=state.mapping.as_ref().map(|m|m.pairs.clone()).unwrap_or_default();
                        let mut changed=false;
                        egui::ScrollArea::vertical().id_salt("import_map_rows")
                            .max_height(190.0).show(ui,|ui|{
                                egui::Grid::new("import_mapping_grid").num_columns(3)
                                    .striped(true).show(ui,|ui|{
                                        ui.strong("Source");ui.strong("Target");ui.strong("Status");ui.end_row();
                                        for (i,joint) in source.joints.iter().enumerate(){
                                            ui.label(&joint.name);
                                            let current=state.manual.get(&joint.name).cloned()
                                                .or_else(||pairs.iter().find(|(s,_)|*s==i)
                                                    .map(|(_,t)|editor.skeleton.joints[*t].name.clone()));
                                            let mut desired=current.clone();
                                            egui::ComboBox::from_id_salt(("retarget_joint",i))
                                                .selected_text(desired.as_deref().unwrap_or("Ignore"))
                                                .show_ui(ui,|ui|{
                                                    ui.selectable_value(&mut desired,None,"Ignore");
                                                    for target in &editor.skeleton.joints{
                                                        ui.selectable_value(&mut desired,
                                                            Some(target.name.clone()),&target.name);
                                                    }
                                                });
                                            if desired!=current {
                                                if let Some(name)=desired {
                                                    state.manual.insert(joint.name.clone(),name);
                                                }else{
                                                    state.manual.remove(&joint.name);
                                                    state.manual.insert(joint.name.clone(),String::new());
                                                }
                                                changed=true;
                                            }
                                            ui.label(if current.is_some(){"Mapped"}else{"Unmapped"});
                                            ui.end_row();
                                        }
                                    });
                            });
                        if changed{
                            state.update_mapping(&editor.skeleton);
                        }
                    }
                });
            ui.collapsing("Advanced",|ui|{
                ui.horizontal(|ui|{
                    ui.label("Root Motion");
                    ui.selectable_value(&mut state.settings.root_motion,RootMotion::InPlace,"In Place");
                    ui.selectable_value(&mut state.settings.root_motion,RootMotion::Preserve,"Preserve");
                });
                ui.horizontal(|ui|{
                    ui.label("Translation Scale");
                    ui.add(egui::DragValue::new(&mut state.settings.translation_scale)
                        .range(0.001..=1000.0).speed(0.01));
                    ui.label("FPS");
                    ui.add(egui::DragValue::new(&mut state.settings.frames_per_second)
                        .range(1.0..=120.0).speed(1.0));
                });
                let (mut yaw,mut pitch,mut roll)=state.settings.source_to_target_orientation
                    .to_euler(glam::EulerRot::YXZ);
                let mut edited=false;
                ui.horizontal(|ui|{
                    ui.label("Coordinate Alignment");
                    edited|=ui.add(egui::DragValue::new(&mut yaw).speed(0.01).suffix(" yaw")).changed();
                    edited|=ui.add(egui::DragValue::new(&mut pitch).speed(0.01).suffix(" pitch")).changed();
                    edited|=ui.add(egui::DragValue::new(&mut roll).speed(0.01).suffix(" roll")).changed();
                });
                if edited{
                    state.settings.source_to_target_orientation=Quat::from_euler(
                        glam::EulerRot::YXZ,yaw,pitch,roll).normalize();
                }
            });
            ui.separator();
            ui.horizontal(|ui|{
                if ui.add_enabled(!busy,egui::Button::new("Retarget Preview")).clicked(){
                    state.preview_selected(editor);
                }
                if state.preview_active {
                    ui.small(format!("Preview: {}",editor.draft.name));
                    if ui.button(if editor.playing{"Pause"}else{"Play"}).clicked(){
                        editor.playing=!editor.playing;
                    }
                    ui.add(egui::Slider::new(&mut editor.current_time,
                        0.0..=editor.draft.duration).text("Time"));
                }
            });
            if state.preview_active{
                if editor.playing {
                    editor.current_time=(editor.current_time+ui.ctx().input(|i|i.stable_dt)
                        .clamp(0.0,0.1)).rem_euclid(editor.draft.duration.max(0.001));
                    ui.ctx().request_repaint();
                }
                let width=ui.available_width().min(640.0);
                let (rect,_)=ui.allocate_exact_size(egui::vec2(width,270.0),Sense::hover());
                ui.painter().rect_filled(rect,0.0,ui.visuals().faint_bg_color);
                let mut animated=false;
                if let Some(package)=state.target_package.as_ref(){
                    animated=paint_skinned_preview(ui,rect,package,&editor.draft,
                        editor.current_time,&state.preview_textures);
                }
                if !animated{
                    let mesh_visible=crate::editor::actor_model_preview::paint_model(
                        ui,rect,&editor.rig_path);
                    if !mesh_visible {
                        paint_rig_preview(ui,rect,&editor.skeleton,
                            &editor.draft,editor.current_time);
                    }
                    ui.small("Animated mesh unavailable for this rig; showing a static asset preview.");
                }
            }
            ui.separator();
            let valid=state.mapping.as_ref().is_some_and(|m|
                !m.pairs.is_empty() && m.ambiguous.is_empty())
                && !state.selected_names().is_empty();
            ui.horizontal(|ui|{
                if ui.add_enabled(!busy&&valid,egui::Button::new("Import Selected")).clicked(){
                    state.start_import(editor.project.clone());
                }
                if ui.add_enabled(!busy,egui::Button::new("Close")).clicked(){
                    cancelled=true;
                }
                if state.working.is_some(){
                    ui.spinner();ui.label("Importing selected animations...");
                    ui.ctx().request_repaint();
                }
            });
        }
        if state.inspect_working.is_some() || state.preview_working.is_some(){
            ui.spinner();
            ui.ctx().request_repaint();
        }
        if let Some((message,success))=state.status.as_ref(){
            ui.colored_label(if *success{ui.visuals().text_color()}
                else{ui.visuals().error_fg_color},message);
        }
    });
    if cancelled {
        if busy {
            return EditorAction::None;
        }
        return EditorAction::Close;
    }
    EditorAction::None
}

fn paint_rig_preview(
    ui: &Ui,
    rect: egui::Rect,
    skeleton: &Skeleton,
    clip: &AnimationAsset,
    time: f32,
) {
    let pose = sample_pose(clip, skeleton, time);
    if pose.is_empty() {
        return;
    }
    let evaluated = skeleton.evaluate_pose(&pose);
    let points = evaluated
        .matrices
        .iter()
        .map(|m| m.transform_point3(Vec3::ZERO))
        .collect::<Vec<_>>();
    let min = points
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |a, p| a.min(*p));
    let max = points
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
    let width = (max.x - min.x).max(0.001);
    let height = (max.y - min.y).max(0.001);
    let scale = (rect.width() * 0.75 / width).min(rect.height() * 0.83 / height);
    let xmid = (min.x + max.x) * 0.5;
    let ymin = min.y;
    let project = |p: Vec3| {
        egui::pos2(
            rect.center().x + (p.x - xmid) * scale,
            rect.bottom() - 14.0 - (p.y - ymin) * scale,
        )
    };
    for (i, joint) in skeleton.joints.iter().enumerate() {
        if let Some(parent) = joint.parent_index {
            ui.painter().line_segment(
                [project(points[parent]), project(points[i])],
                Stroke::new(2.0, Color32::from_rgb(255, 171, 52)),
            );
        }
    }
}

pub(crate) fn paint_skinned_preview(
    ui: &Ui,
    rect: egui::Rect,
    package: &CharacterPackage,
    clip: &AnimationAsset,
    time: f32,
    textures: &HashMap<PathBuf, Arc<image::RgbaImage>>,
) -> bool {
    let pose = sample_pose(clip, &package.skeleton, time);
    if pose.is_empty() {
        return false;
    }
    let evaluated = package.skeleton.evaluate_pose(&pose);
    let mut input_min = Vec3::splat(f32::INFINITY);
    let mut input_max = Vec3::splat(f32::NEG_INFINITY);
    for part in &package.geometry {
        if let GeometryPrimitive::SkinnedTriangleMesh { vertices, .. } = &part.primitive {
            for vertex in vertices.iter().step_by((vertices.len() / 4000).max(1)) {
                input_min = input_min.min(vertex.position);
                input_max = input_max.max(vertex.position);
            }
        }
    }
    let orientation =
        crate::editor::actor_model_preview::orientation_for_mesh_bounds(input_min, input_max);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut faces = Vec::<([Vec3; 3], Color32, f32)>::new();
    let total = package
        .geometry
        .iter()
        .filter_map(|part| match &part.primitive {
            GeometryPrimitive::SkinnedTriangleMesh { vertices, .. } => Some(vertices.len() / 3),
            _ => None,
        })
        .sum::<usize>();
    if total == 0 {
        return false;
    }
    let stride = (total / 3000).max(1);
    let mut index = 0usize;
    let (yaw_s, yaw_c) = 0.35f32.sin_cos();
    for part in &package.geometry {
        let GeometryPrimitive::SkinnedTriangleMesh {
            vertices,
            inverse_bind_matrices,
        } = &part.primitive
        else {
            continue;
        };
        let matrices = evaluated
            .matrices
            .iter()
            .enumerate()
            .map(|(i, m)| {
                *m * inverse_bind_matrices
                    .get(i)
                    .copied()
                    .unwrap_or(Mat4::IDENTITY)
            })
            .collect::<Vec<_>>();
        for verts in vertices.chunks_exact(3) {
            let draw = index % stride == 0;
            index += 1;
            if !draw {
                continue;
            }
            let mut world_tri = [Vec3::ZERO; 3];
            let mut valid = true;
            for (corner, v) in verts.iter().enumerate() {
                let mut point = Vec3::ZERO;
                let mut weight_sum = 0.0f32;
                for (joint, weight) in v.joints.iter().zip(v.weights) {
                    if weight <= 0.0 {
                        continue;
                    }
                    let Some(matrix) = matrices.get(*joint) else {
                        valid = false;
                        break;
                    };
                    point += matrix.transform_point3(v.position) * weight;
                    weight_sum += weight;
                }
                if !valid || weight_sum < 0.001 || !point.is_finite() {
                    valid = false;
                    break;
                }
                point /= weight_sum;
                let point = orientation * point;
                world_tri[corner] = Vec3::new(
                    point.x * yaw_c + point.z * yaw_s,
                    point.y,
                    point.z * yaw_c - point.x * yaw_s,
                );
            }
            if !valid {
                continue;
            }
            let mut inflated = false;
            for edge in 0..3 {
                let next = (edge + 1) % 3;
                let original = (verts[edge].position - verts[next].position).length();
                let deformed = (world_tri[edge] - world_tri[next]).length();
                if !deformed.is_finite() || deformed > original * 5.0 + 0.025 {
                    inflated = true;
                    break;
                }
            }
            if inflated {
                continue;
            }
            let normal = (world_tri[1] - world_tri[0])
                .cross(world_tri[2] - world_tri[0])
                .normalize_or_zero();
            if normal.length_squared() < 1e-8 {
                continue;
            }
            let light = Vec3::new(0.4, 0.7, 0.5).normalize();
            let shade = 0.45 + 0.47 * normal.dot(light).abs();
            let tint = verts[0].color;
            let base = package.appearance.material(&part.material);
            let texel = part
                .base_color_texture
                .as_ref()
                .and_then(|path| textures.get(path))
                .map(|texture| {
                    let uv = verts.iter().map(|v| v.tex_coord).sum::<glam::Vec2>() / 3.0;
                    crate::editor::actor_model_preview::gltf_atlas_texel(texture, uv)
                });
            let color = Color32::from_rgb(
                (tint[0].clamp(0.0, 1.0)
                    * base[0].clamp(0.0, 1.0)
                    * shade
                    * texel.as_ref().map_or(255.0, |px| px[0] as f32)) as u8,
                (tint[1].clamp(0.0, 1.0)
                    * base[1].clamp(0.0, 1.0)
                    * shade
                    * texel.as_ref().map_or(255.0, |px| px[1] as f32)) as u8,
                (tint[2].clamp(0.0, 1.0)
                    * base[2].clamp(0.0, 1.0)
                    * shade
                    * texel.as_ref().map_or(255.0, |px| px[2] as f32)) as u8,
            );
            for point in &world_tri {
                min = min.min(*point);
                max = max.max(*point);
            }
            let depth = world_tri.iter().map(|point| point.z).sum::<f32>() / 3.0;
            faces.push((world_tri, color, depth));
        }
    }
    if faces.is_empty() || !min.is_finite() || !max.is_finite() {
        return false;
    }
    faces.sort_by(|a, b| a.2.total_cmp(&b.2));
    let size = (max - min).max(Vec3::splat(0.001));
    let zoom = (rect.width() * 0.85 / size.x).min(rect.height() * 0.85 / size.y);
    if !zoom.is_finite() {
        return false;
    }
    let center = (min + max) * 0.5;
    let project = |p: Vec3| {
        egui::pos2(
            rect.center().x + (p.x - center.x) * zoom,
            rect.center().y - (p.y - center.y) * zoom,
        )
    };
    let painter = ui.painter().with_clip_rect(rect);
    for (points, color, _) in faces {
        let projected = points.map(project);
        let signed_area = (projected[1].x - projected[0].x) * (projected[2].y - projected[0].y)
            - (projected[1].y - projected[0].y) * (projected[2].x - projected[0].x);
        if signed_area.abs() < 0.25 || !signed_area.is_finite() {
            continue;
        }
        painter.add(egui::Shape::convex_polygon(
            projected.to_vec(),
            color,
            Stroke::NONE,
        ));
    }
    true
}

#[cfg(test)]
mod regression_tests {
    use super::*;
}
