// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::editor::effect_curves::CurveUiState;
use crate::project::fx_authoring::{
    CurveChannel, CurveKey, EffectDocument, EffectEditHistory, EffectNode, EmitterKind,
};
use crate::project::fx_export::EfkEfc;
use crate::project::fx_import::{EffectCompatibility, inspect};
use crate::project::fx_standalone::AeoFxCompiler;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const BUILTIN_TEXTURE: &[u8] = include_bytes!("../../assets/effekseer/Texture/fire_tex.png");

fn reassign_texture(nodes: &mut [EffectNode], old: &str, new: &str) {
    for node in nodes {
        if node.texture.as_deref() == Some(old) {
            node.texture = Some(new.into());
        }
        reassign_texture(&mut node.children, old, new);
    }
}

fn apply_shared_texture(nodes: &mut [EffectNode], texture: &str) {
    for node in nodes {
        if node.kind == EmitterKind::Sprite {
            node.texture = Some(texture.into());
        }
        apply_shared_texture(&mut node.children, texture);
    }
}

#[derive(Debug)]
pub struct EffectWorkspace {
    pub path: PathBuf,
    pub document: EffectDocument,
    compatibility: EffectCompatibility,
    pub selected_id: Option<u64>,
    pub playing: bool,
    pub dirty: bool,
    pub pending_preview_update: bool,
    pub preview_path: Option<PathBuf>,
    pub preview_revision: u64,
    pub last_edit_at: f64,
    pub edit_history: EffectEditHistory,
    pub curves: CurveUiState,
    temporary_previews: Vec<PathBuf>,
    saved_bytes: Vec<u8>,
}

impl EffectWorkspace {
    pub fn create(project: &Path, name: &str) -> Result<Self, String> {
        Self::create_with_kind(project, name, EmitterKind::Sprite)
    }

    pub fn create_ring(project: &Path, name: &str) -> Result<Self, String> {
        Self::create_with_kind(project, name, EmitterKind::Ring)
    }

    fn create_with_kind(project: &Path, name: &str, kind: EmitterKind) -> Result<Self, String> {
        if !matches!(kind, EmitterKind::Sprite | EmitterKind::Ring) {
            return Err("Emitter type is not yet supported for project creation".into());
        }
        let name = name.trim();
        if name.is_empty()
            || name.len() > 64
            || name.starts_with('.')
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ' ')
        {
            return Err("Invalid Effect asset name".into());
        }
        let id = name.replace(' ', "_");
        let mut doc = EffectDocument::new(&id, name);
        doc.roots.push(EffectNode {
            id: 1,
            kind,
            name: if kind == EmitterKind::Ring {
                "Ring".into()
            } else {
                "Fire".into()
            },
            enabled: true,
            emission_rate: Some(if kind == EmitterKind::Ring {
                12.0
            } else {
                20.0
            }),
            lifetime_seconds: Some(2.0),
            texture: if kind == EmitterKind::Ring {
                None
            } else {
                Some("Texture/fire_tex.png".into())
            },
            color_r: Vec::new(),
            color_g: Vec::new(),
            color_b: Vec::new(),
            spawn_shape: crate::project::fx_authoring::SpawnShape::Point,
            spawn_radius: 0.0,
            velocity: [0.0; 3],
            acceleration: [0.0; 3],
            drag: 0.0,
            size: Vec::new(),
            opacity: Vec::new(),
            children: Vec::new(),
        });
        let data = if kind == EmitterKind::Ring {
            crate::project::fx_import::compile_ring_document(&doc)?
        } else {
            AeoFxCompiler::export(&doc)?
        };
        let effects_root = project.join(".assets").join("effects");
        if !project.is_dir() {
            return Err("Project directory does not exist".into());
        }
        for root in [
            project.to_path_buf(),
            project.join(".assets"),
            effects_root.clone(),
        ] {
            if let Ok(meta) = fs::symlink_metadata(&root) {
                if meta.file_type().is_symlink() {
                    return Err("Linked project effect destination".into());
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err("Reparse-point effect destination".into());
                    }
                }
            }
        }
        fs::create_dir_all(&effects_root).map_err(|e| e.to_string())?;
        let root = effects_root.canonicalize().map_err(|e| e.to_string())?;
        if !root.starts_with(project.canonicalize().map_err(|e| e.to_string())?) {
            return Err("Effects root escapes project".into());
        }
        let target = effects_root.join(&id);
        if target.exists() {
            return Err("Effect asset name already exists".into());
        }
        let stage = effects_root.join(format!(".aeofx-create-{:032x}", rand::random::<u128>()));
        fs::create_dir(&stage).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            if kind == EmitterKind::Sprite {
                fs::create_dir(stage.join("Texture")).map_err(|e| e.to_string())?;
                create_file(&stage.join("Texture/fire_tex.png"), BUILTIN_TEXTURE)?;
            }
            create_file(&stage.join(format!("{id}.efkefc")), &data)?;
            if target.exists() {
                return Err("Effect asset name already exists".into());
            }
            fs::rename(&stage, &target).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result?;
        if let Err(error) = crate::project::asset_registry::AssetRegistry::reconcile(project) {
            let _ = fs::remove_dir_all(&target);
            return Err(format!(
                "Effect creation could not publish its AssetID: {error}"
            ));
        }
        Self::open(project, &target.join(format!("{id}.efkefc")))
    }

    pub fn open(project: &Path, path: &Path) -> Result<Self, String> {
        let id = crate::project::fx_assets::id_from_path(project, path)
            .ok_or("Effect is not in this project's Effects library")?;
        let path = crate::project::fx_assets::resolve(project, &id)
            .ok_or("Effect cannot be safely resolved")?;
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let effect = EfkEfc::parse(&bytes)?;
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("Effect has no readable filename")?;
        let (compatibility, mut doc) = inspect(&bytes)?;
        doc.id = name.to_owned();
        doc.name = name.to_owned();
        doc.validate()?;
        if compatibility.imported() {
            fn check_resources(nodes: &[EffectNode], root: &Path) -> Result<(), String> {
                for node in nodes {
                    if let Some(texture) = &node.texture {
                        if !crate::project::fx_authoring::valid_resource_path(texture) {
                            return Err("Unsafe imported effect texture reference".into());
                        }
                        let canonical = root.join(texture).canonicalize().map_err(|_| {
                            "Imported effect texture dependency is missing".to_string()
                        })?;
                        if !canonical.starts_with(root) || !canonical.is_file() {
                            return Err("Imported effect texture escapes its package".into());
                        }
                    }
                    check_resources(&node.children, root)?;
                }
                Ok(())
            }
            let root = path
                .parent()
                .ok_or("Imported Effect package missing")?
                .canonicalize()
                .map_err(|e| e.to_string())?;
            check_resources(&doc.roots, &root)?;
        }
        if doc.roots.is_empty() {
            return Err("Effect has no editable emitters".into());
        }
        let selected_id = doc.roots.first().map(|node| node.id);
        Ok(Self {
            path,
            document: doc,
            compatibility,
            selected_id,
            playing: false,
            dirty: false,
            pending_preview_update: false,
            preview_path: None,
            preview_revision: 0,
            last_edit_at: 0.0,
            edit_history: EffectEditHistory::default(),
            curves: CurveUiState::default(),
            temporary_previews: Vec::new(),
            saved_bytes: bytes,
        })
    }

    pub fn is_imported(&self) -> bool {
        self.compatibility.imported()
    }
    pub fn is_ring(&self) -> bool {
        matches!(self.compatibility, EffectCompatibility::NativeRing)
    }
    fn native_only(&self) -> Result<(), String> {
        if !matches!(self.compatibility, EffectCompatibility::NativeAeoFx) {
            return Err("Only Sprite effects support hierarchy or curve edits".into());
        }
        Ok(())
    }
    fn compile_edit(&self) -> Result<Vec<u8>, String> {
        self.compatibility.compile(&self.document)
    }

    pub fn set_emission_rate(&mut self, node_id: u64, rate: f32) -> Result<(), String> {
        self.set_emission_rate_drag(node_id, rate, false)
    }
    fn set_emission_rate_drag(
        &mut self,
        node_id: u64,
        rate: f32,
        dragging: bool,
    ) -> Result<(), String> {
        if !self.compatibility.can_edit_rate(node_id) {
            return Err("Imported emitter emission cannot be safely edited".into());
        }
        self.edit(dragging, |doc| {
            doc.set_emission_rate(node_id, rate).map(|_| ())
        })
    }
    pub fn set_lifetime(&mut self, node_id: u64, seconds: f32) -> Result<(), String> {
        self.set_lifetime_drag(node_id, seconds, false)
    }
    fn set_lifetime_drag(
        &mut self,
        node_id: u64,
        seconds: f32,
        dragging: bool,
    ) -> Result<(), String> {
        if !self.compatibility.can_edit_lifetime(node_id) {
            return Err("Imported emitter lifetime cannot be safely edited".into());
        }
        let seconds = if self
            .document
            .node(node_id)
            .is_some_and(|n| matches!(n.kind, EmitterKind::Ribbon | EmitterKind::Track))
        {
            (seconds * 60.0).round() / 60.0
        } else {
            seconds
        };
        self.edit(dragging, |doc| {
            doc.set_lifetime_seconds(node_id, seconds).map(|_| ())
        })
    }

    fn edit(
        &mut self,
        gesture: bool,
        change: impl FnOnce(&mut EffectDocument) -> Result<(), String>,
    ) -> Result<(), String> {
        let started = self.edit_history.begin(&self.document);
        match change(&mut self.document) {
            Ok(()) => {
                if !gesture {
                    self.edit_history.commit(&mut self.document)?;
                }
                self.dirty = true;
                self.pending_preview_update = true;
                Ok(())
            }
            Err(error) => {
                if started {
                    self.edit_history.cancel(&mut self.document);
                }
                Err(error)
            }
        }
    }

    fn finish_edit(&mut self) -> Result<(), String> {
        if self.edit_history.has_pending() {
            self.edit_history.commit(&mut self.document)?;
        }
        Ok(())
    }

    pub fn undo(&mut self) {
        let _ = self.finish_edit();
        if self.edit_history.undo(&mut self.document) {
            self.dirty = true;
            self.pending_preview_update = true;
        }
    }
    pub fn redo(&mut self) {
        let _ = self.finish_edit();
        if self.edit_history.redo(&mut self.document) {
            self.dirty = true;
            self.pending_preview_update = true;
        }
    }

    pub fn add_emitter(&mut self, kind: EmitterKind) -> Result<u64, String> {
        match kind {
            EmitterKind::Sprite => self.add_sprite(kind.label()),
            EmitterKind::Ring => self.add_ring(),
            EmitterKind::Ribbon | EmitterKind::Track => self.add_trail(kind),
            _ => Err(format!(
                "{} native composition is not yet supported in this Effect",
                kind.label()
            )),
        }
    }
    fn contains_trail(&self) -> bool {
        fn visit(nodes: &[EffectNode]) -> bool {
            nodes.iter().any(|node| {
                matches!(node.kind, EmitterKind::Ribbon | EmitterKind::Track)
                    || visit(&node.children)
            })
        }
        visit(&self.document.roots)
    }
    fn last_serialized_sprite_id(&self) -> Option<u64> {
        fn walk(nodes: &[EffectNode], last: &mut Option<u64>) {
            for node in nodes {
                if node.kind == EmitterKind::Sprite {
                    *last = Some(node.id);
                }
                walk(&node.children, last);
            }
        }
        let mut last = None;
        walk(&self.document.roots, &mut last);
        last
    }
    fn trail_can_add(&self, kind: EmitterKind) -> bool {
        if !matches!(kind, EmitterKind::Ribbon | EmitterKind::Track)
            || self.is_imported()
            || self.is_ring()
            || self
                .document
                .roots
                .iter()
                .any(|n| n.kind == EmitterKind::Ring)
        {
            return false;
        }
        let Some(selected) = self.selected_id else {
            return false;
        };
        if Some(selected) != self.last_serialized_sprite_id() {
            return false;
        }
        !self.document.node(selected).is_some_and(|n| {
            n.children.iter().any(|c| {
                c.kind == kind || (kind == EmitterKind::Ribbon && c.kind == EmitterKind::Track)
            })
        })
    }
    fn add_trail(&mut self, kind: EmitterKind) -> Result<u64, String> {
        self.native_only()?;
        if !self.trail_can_add(kind) {
            return Err(
                "Select the final Sprite to add a new checked Ribbon or Track child".into(),
            );
        }
        let parent = self.selected_id.ok_or("Select the final Sprite")?;
        let mut highest = 0_u64;
        fn max_id(nodes: &[EffectNode], max: &mut u64) {
            for n in nodes {
                *max = (*max).max(n.id);
                max_id(&n.children, max);
            }
        }
        max_id(&self.document.roots, &mut highest);
        let id = highest.checked_add(1).ok_or("FX ID exhausted")?;
        let mut node = EffectNode::new(id, format!("{} {id}", kind.label()));
        node.kind = kind;
        node.emission_rate = Some(30.0);
        node.lifetime_seconds = Some(100.0 / 60.0);
        self.edit(false, |doc| doc.insert_node(Some(parent), node))?;
        self.selected_id = Some(id);
        Ok(id)
    }
    fn can_add_emitter(&self, kind: EmitterKind) -> bool {
        if self.is_imported() || self.is_ring() {
            return false;
        }
        match kind {
            EmitterKind::Sprite => !self.contains_trail(),
            EmitterKind::Ring => {
                !self.contains_trail()
                    && !self
                        .document
                        .roots
                        .iter()
                        .any(|n| n.kind == EmitterKind::Ring)
            }
            EmitterKind::Ribbon | EmitterKind::Track => self.trail_can_add(kind),
            EmitterKind::Mesh => false,
        }
    }

    fn add_ring(&mut self) -> Result<u64, String> {
        self.native_only()?;
        if self.contains_trail() {
            return Err("Ribbon/Track and Ring native composition is not yet verified".into());
        }
        if self
            .document
            .roots
            .iter()
            .any(|n| n.kind == EmitterKind::Ring)
        {
            return Err("This Effect already has a Ring emitter".into());
        }
        let mut pending: Vec<_> = self.document.roots.iter().collect();
        let mut max = 0;
        while let Some(n) = pending.pop() {
            max = max.max(n.id);
            pending.extend(n.children.iter());
        }
        let id = max.checked_add(1).ok_or("Effect node IDs exhausted")?;
        let mut ring = EffectNode::new(id, format!("Ring {id}"));
        ring.kind = EmitterKind::Ring;
        ring.texture = None;
        ring.emission_rate = Some(12.0);
        self.edit(false, |doc| doc.insert_node(None, ring))?;
        self.selected_id = Some(id);
        Ok(id)
    }

    pub fn add_sprite(&mut self, name: &str) -> Result<u64, String> {
        self.native_only()?;
        if self.contains_trail() {
            return Err("Remove Ribbon/Track children before adding a Sprite node".into());
        }
        let mut max = 0_u64;
        let mut pending: Vec<_> = self.document.roots.iter().collect();
        while let Some(node) = pending.pop() {
            max = max.max(node.id);
            pending.extend(node.children.iter());
        }
        let id = max.checked_add(1).ok_or("Effect node IDs exhausted")?;
        let texture = self.document.roots.first().and_then(|n| n.texture.clone());
        let label = if name == "Sprite" {
            format!("Sprite {id}")
        } else {
            name.to_owned()
        };
        let mut sprite = EffectNode::new(id, label);
        sprite.texture = texture;
        self.edit(false, |doc| {
            if let Some(pos) = doc.roots.iter().position(|n| n.kind == EmitterKind::Ring) {
                doc.roots.insert(pos, sprite);
                doc.validate()
            } else {
                doc.insert_node(None, sprite)
            }
        })?;
        self.selected_id = Some(id);
        Ok(id)
    }
    pub fn remove_selected(&mut self) -> Result<(), String> {
        self.native_only()?;
        let id = self.selected_id.ok_or("Select an Effect node")?;
        if self.document.roots.len() == 1 && self.document.roots[0].id == id {
            return Err("Cannot remove the last Effect node".into());
        }
        if self
            .document
            .roots
            .iter()
            .any(|n| n.kind == EmitterKind::Ring)
        {
            let selected_is_sprite = self
                .document
                .node(id)
                .is_some_and(|n| n.kind == EmitterKind::Sprite);
            let mut total_sprite = 0_usize;
            fn count_sprites(nodes: &[EffectNode], out: &mut usize) {
                for node in nodes {
                    if node.kind == EmitterKind::Sprite {
                        *out += 1;
                    }
                    count_sprites(&node.children, out);
                }
            }
            count_sprites(&self.document.roots, &mut total_sprite);
            if selected_is_sprite && total_sprite == 1 {
                return Err("Cannot remove the final Sprite while Ring is present".into());
            }
        }
        if self.document.node(id).is_some_and(|n| {
            n.children
                .iter()
                .any(|c| matches!(c.kind, EmitterKind::Ribbon | EmitterKind::Track))
        }) {
            return Err("Remove Ribbon/Track children before deleting their parent Sprite".into());
        }
        self.edit(false, |doc| doc.remove_node(id).map(|_| ()))?;
        self.selected_id = self.document.roots.first().map(|n| n.id);
        Ok(())
    }
    pub fn reparent_selected(&mut self, parent: Option<u64>) -> Result<(), String> {
        self.native_only()?;
        let id = self.selected_id.ok_or("Select an Effect node")?;
        if self.contains_trail()
            || self
                .document
                .node(id)
                .is_some_and(|n| n.kind != EmitterKind::Sprite)
            || parent
                .and_then(|id| self.document.node(id))
                .is_some_and(|n| n.kind != EmitterKind::Sprite)
        {
            return Err("Ring cannot be reparented or contain Sprite children".into());
        }
        self.edit(false, |doc| doc.reparent_node(id, parent))
    }
    pub fn set_spawn_motion_drag(
        &mut self,
        id: u64,
        shape: crate::project::fx_authoring::SpawnShape,
        radius: f32,
        velocity: [f32; 3],
        acceleration: [f32; 3],
        drag: f32,
        gesture: bool,
    ) -> Result<(), String> {
        self.native_only()?;
        if self
            .document
            .node(id)
            .is_some_and(|n| n.kind != EmitterKind::Sprite)
        {
            return Err("Advanced emitter spawn/motion writer is not verified".into());
        }
        self.edit(gesture, |doc| {
            doc.set_spawn_motion(id, shape, radius, velocity, acceleration, drag)
        })
    }

    pub fn rename_selected(&mut self, name: &str, gesture: bool) -> Result<(), String> {
        if !self.is_ring() {
            self.native_only()?;
        }
        let id = self.selected_id.ok_or("Select an Effect node")?;
        self.edit(gesture, |doc| doc.rename_node(id, name))
    }

    pub fn set_curve(
        &mut self,
        id: u64,
        channel: CurveChannel,
        keys: Vec<CurveKey>,
        gesture: bool,
    ) -> Result<(), String> {
        self.native_only()?;
        if self
            .document
            .node(id)
            .is_some_and(|n| n.kind != EmitterKind::Sprite)
        {
            return Err("Advanced emitter curve writer is not verified".into());
        }
        self.edit(gesture, move |doc| {
            doc.set_curve(id, channel, keys).map(|_| ())
        })
    }
    pub fn set_shared_texture(&mut self, asset: &Path, project: &Path) -> Result<(), String> {
        if self.is_ring() {
            return Err("Ring has no Sprite texture slot".into());
        }
        if self
            .selected_id
            .and_then(|id| self.document.node(id))
            .is_some_and(|n| n.kind != EmitterKind::Sprite)
        {
            return Err("Advanced emitter has no Sprite texture slot".into());
        }
        let canonical = asset.canonicalize().map_err(|e| e.to_string())?;
        let tex_root = project
            .join(".assets/textures")
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !canonical.starts_with(&tex_root)
            || canonical
                .extension()
                .and_then(|s| s.to_str())
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("png"))
        {
            return Err("Drop a project PNG texture from Content Browser".into());
        }
        let asset_name = canonical
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid texture filename")?;
        let package = self.path.parent().ok_or("Effect has no package")?;
        let target = package.join("Texture").join(asset_name);
        if target.exists()
            && fs::read(&target).map_err(|e| e.to_string())?
                != fs::read(&canonical).map_err(|e| e.to_string())?
        {
            return Err("A different texture already uses this name".into());
        }
        let reference = format!("Texture/{asset_name}");
        let old_reference = if self.is_imported() {
            let node_id = self
                .selected_id
                .ok_or("Select an imported effect emitter")?;
            Some(
                self.document
                    .node(node_id)
                    .and_then(|n| n.texture.as_deref())
                    .ok_or("This imported emitter has no editable texture slot")?
                    .to_owned(),
            )
        } else {
            None
        };
        let apply = |doc: &mut EffectDocument| {
            if let Some(old) = old_reference.as_deref() {
                reassign_texture(&mut doc.roots, old, &reference);
            } else {
                apply_shared_texture(&mut doc.roots, &reference);
            }
        };
        let mut test = self.document.clone();
        apply(&mut test);
        self.compatibility.compile(&test)?;
        if !target.exists() {
            fs::copy(&canonical, &target).map_err(|e| e.to_string())?;
        }
        self.edit(false, |doc| {
            apply(doc);
            Ok(())
        })
    }

    pub fn refresh_preview(&mut self) -> Result<(), String> {
        if !self.pending_preview_update {
            return Ok(());
        }
        let data = self.compile_edit()?;
        let folder = std::env::temp_dir().join(format!(
            "worldkiln-aeofx-preview-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&folder).map_err(|e| e.to_string())?;
        let result = (|| -> Result<PathBuf, String> {
            let source = self.path.parent().ok_or("Effect directory missing")?;
            let source = source.canonicalize().map_err(|e| e.to_string())?;
            let mut resources = std::collections::BTreeSet::<String>::new();
            fn gather(nodes: &[EffectNode], paths: &mut std::collections::BTreeSet<String>) {
                for node in nodes {
                    if let Some(texture) = &node.texture {
                        paths.insert(texture.clone());
                    }
                    gather(&node.children, paths);
                }
            }
            gather(&self.document.roots, &mut resources);
            if resources.len() > 32 {
                return Err("Too many preview dependencies".into());
            }
            let mut bytes_total = 0_u64;
            for texture in resources {
                if !crate::project::fx_authoring::valid_resource_path(&texture) {
                    return Err("Unsafe Effect preview texture path".into());
                }
                let source_texture = source.join(&texture);
                let resolved = source_texture.canonicalize().map_err(|e| e.to_string())?;
                if !resolved.starts_with(&source) || !resolved.is_file() {
                    return Err("Effect preview dependency missing or unsafe".into());
                }
                let size = fs::metadata(&resolved).map_err(|e| e.to_string())?.len();
                bytes_total = bytes_total
                    .checked_add(size)
                    .ok_or("Preview resource size overflow")?;
                if bytes_total > 64 * 1024 * 1024 {
                    return Err("Effect preview resources exceed 64 MiB".into());
                }
                let destination = folder.join(&texture);
                fs::create_dir_all(destination.parent().ok_or("Texture parent missing")?)
                    .map_err(|e| e.to_string())?;
                fs::copy(resolved, destination).map_err(|e| e.to_string())?;
            }
            let output = folder.join("AeoFXPreview.efkefc");
            create_file(&output, &data)?;
            Ok(output)
        })();
        match result {
            Ok(path) => {
                self.preview_path = Some(path);
                self.temporary_previews.push(folder);
                while self.temporary_previews.len() > 4 {
                    let retired = self.temporary_previews.remove(0);
                    let _ = fs::remove_dir_all(retired);
                }
                self.preview_revision = self.preview_revision.wrapping_add(1);
                self.pending_preview_update = false;
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_dir_all(folder);
                Err(error)
            }
        }
    }

    pub fn reset_preview(&mut self) {
        self.preview_revision = self.preview_revision.wrapping_add(1);
    }

    pub fn save(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if fs::read(&self.path).map_err(|e| e.to_string())? != self.saved_bytes {
            return Err("Effect file changed externally; reopen before saving".into());
        }
        let data = self.compile_edit()?;
        let parent = self.path.parent().ok_or("Effect path has no parent")?;
        let stage = parent.join(format!(".aeofx-save-{:032x}", rand::random::<u128>()));
        create_file(&stage, &data)?;
        let backup = parent.join(format!(".aeofx-old-{:032x}", rand::random::<u128>()));
        if fs::read(&self.path).map_err(|e| e.to_string())? != self.saved_bytes {
            let _ = fs::remove_file(&stage);
            return Err("Effect file changed externally; reopen before saving".into());
        }
        if let Err(err) = fs::rename(&self.path, &backup) {
            let _ = fs::remove_file(&stage);
            return Err(err.to_string());
        }
        if let Err(err) = fs::rename(&stage, &self.path) {
            let _ = fs::rename(&backup, &self.path);
            let _ = fs::remove_file(&stage);
            return Err(format!("Effect save failed: {err}"));
        }
        let _ = fs::remove_file(&backup);
        crate::renderer::effekseer::invalidate_project_fx_asset(&self.path);
        if let Some(project) = self
            .path
            .ancestors()
            .find(|path| {
                path.file_name().is_some_and(|name| name == "effects")
                    && path
                        .parent()
                        .is_some_and(|parent| parent.file_name().is_some_and(|n| n == ".assets"))
            })
            .and_then(|effects| effects.parent())
            .and_then(|assets| assets.parent())
        {
            crate::project::asset_registry::AssetRegistry::notify_content_changed(project)
                .map_err(|error| format!("Effect saved, but registry refresh failed: {error}"))?;
        }
        self.saved_bytes = data;
        self.dirty = false;
        self.pending_preview_update = false;
        self.preview_path = None;
        self.reset_preview();
        Ok(())
    }
}

impl Drop for EffectWorkspace {
    fn drop(&mut self) {
        for directory in self.temporary_previews.drain(..) {
            let _ = fs::remove_dir_all(directory);
        }
    }
}

fn create_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_immediate_effect_reopens_and_saves_without_overwrite() {
        let project = std::env::temp_dir().join(format!(
            "wk-aeofx-workspace-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&project).unwrap();
        let mut asset = EffectWorkspace::create(&project, "My Fire").unwrap();
        assert!(asset.path.exists());
        assert!(
            asset
                .path
                .parent()
                .unwrap()
                .join("Texture/fire_tex.png")
                .is_file()
        );
        assert!(EffectWorkspace::create(&project, "My Fire").is_err());
        let path = asset.path.clone();
        asset.set_emission_rate(1, 33.0).unwrap();
        assert!(asset.dirty);
        asset.save().unwrap();
        assert!(!asset.dirty);
        let again = EffectWorkspace::open(&project, &path).unwrap();
        assert_eq!(again.document.roots[0].emission_rate, Some(33.0));
        let mut conflict = again;
        conflict.set_lifetime(1, 3.0).unwrap();
        fs::write(&path, b"external modification").unwrap();
        assert!(conflict.save().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external modification");
        fs::remove_dir_all(project).unwrap();
    }
    #[test]
    fn workspace_draws_layout_without_per_frame_export() {
        let project =
            std::env::temp_dir().join(format!("wk-aeofx-layout-{:032x}", rand::random::<u128>()));
        fs::create_dir(&project).unwrap();
        let mut editor = crate::editor::Editor::new();
        editor.effect_workspace = Some(EffectWorkspace::create(&project, "Layout").unwrap());
        editor.effect_visible = true;
        let source = editor.effect_workspace.as_ref().unwrap().path.clone();
        let original = fs::read(&source).unwrap();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1250.0, 850.0));
        let mut samples = Vec::new();
        for frame in 0..80 {
            let start = std::time::Instant::now();
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(frame as f64 / 60.0),
                    ..Default::default()
                },
                |ctx| editor.draw_aeofx_workspace(ctx),
            );
            samples.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        samples.sort_by(f64::total_cmp);
        assert!(editor.viewport_rect.is_positive());
        assert!(editor.viewport_rect.width() > 150.0);
        assert!(!editor.effect_workspace.as_ref().unwrap().dirty);
        assert!(
            editor
                .effect_workspace
                .as_ref()
                .unwrap()
                .preview_path
                .is_none()
        );
        assert_eq!(fs::read(&source).unwrap(), original);
        println!(
            "AEOFX_UI_HEADLESS frames=80 p50_ms={:.3} p95_ms={:.3}",
            samples[40], samples[76]
        );
        drop(editor);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn content_browser_dock_stays_visible_under_aeofx_curves() {
        use crate::editor::editor::shell::DockTab;
        let project = std::env::temp_dir().join(format!(
            "wk-aeofx-docked-browser-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(project.join(".assets/textures")).unwrap();
        fs::write(
            project.join(".assets/textures").join("drag.png"),
            BUILTIN_TEXTURE,
        )
        .unwrap();
        let mut editor = crate::editor::Editor::new();
        editor.effect_workspace = Some(EffectWorkspace::create(&project, "Docked").unwrap());
        editor.effect_visible = true;
        editor.shell.dock = DockTab::Content;
        editor.shell.dock_expanded = true;
        editor.shell.dock_initialized = true;
        editor.shell.dock_height = 240.0;
        editor.script_editor.show_output = false;
        editor.script_editor.show_problems = false;
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1250.0, 850.0));
        ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                editor.draw_shell_dock(ctx, &Some(project.clone()));
                editor.draw_aeofx_workspace(ctx);
            },
        );
        assert_eq!(editor.shell.dock, DockTab::Content);
        assert!(editor.shell.dock_expanded);
        assert!(editor.viewport_rect.is_positive());
        assert!(
            editor.viewport_rect.bottom() < 525.0,
            "Curve panel must stack above the outer Content Browser dock"
        );
        assert!(project.join(".assets/textures/drag.png").is_file());
        assert!(editor.effect_visible);
        drop(editor);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn spawn_motion_drag_is_native_and_undoable_across_reopen() {
        use crate::project::fx_authoring::SpawnShape;
        let project =
            std::env::temp_dir().join(format!("wk-aeofx-shape-{:032x}", rand::random::<u128>()));
        fs::create_dir_all(&project).unwrap();
        let mut effect = EffectWorkspace::create(&project, "Motion").unwrap();
        assert!(
            effect
                .set_spawn_motion_drag(1, SpawnShape::Sphere, -1.0, [0.0; 3], [0.0; 3], 0.0, false)
                .is_err()
        );
        assert!(!effect.dirty);
        effect
            .set_spawn_motion_drag(
                1,
                SpawnShape::Sphere,
                0.8,
                [1.0, 2.0, 3.0],
                [0.0, -0.4, 0.0],
                0.5,
                false,
            )
            .unwrap();
        assert!(effect.pending_preview_update);
        effect.undo();
        assert_eq!(
            effect.document.node(1).unwrap().spawn_shape,
            SpawnShape::Point
        );
        effect.redo();
        assert_eq!(effect.document.node(1).unwrap().velocity, [1.0, 2.0, 3.0]);
        effect.save().unwrap();
        let reopened = EffectWorkspace::open(&project, &effect.path).unwrap();
        let node = reopened.document.node(1).unwrap();
        assert_eq!(node.spawn_shape, SpawnShape::Sphere);
        assert_eq!(node.spawn_radius, 0.8);
        assert_eq!(node.drag, 0.5);
        assert_eq!(node.acceleration, [0.0, -0.4, 0.0]);
        drop(reopened);
        drop(effect);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn authored_ribbon_and_track_children_survive_preview_save_and_reopen() {
        let project =
            std::env::temp_dir().join(format!("wk-native-trail-{:032x}", rand::random::<u128>()));
        fs::create_dir_all(&project).unwrap();
        let mut fx = EffectWorkspace::create(&project, "Ribbon Track").unwrap();
        let sprites_before = fx.document.roots.clone();
        assert!(fx.trail_can_add(EmitterKind::Ribbon));
        let ribbon = fx.add_emitter(EmitterKind::Ribbon).unwrap();
        assert_eq!(fx.document.node(ribbon).unwrap().kind, EmitterKind::Ribbon);
        assert!(!fx.trail_can_add(EmitterKind::Track));
        fx.set_emission_rate(ribbon, 75.0).unwrap();
        fx.set_lifetime(ribbon, 5.0).unwrap();
        assert!(
            fx.set_curve(ribbon, CurveChannel::Size, vec![], false)
                .is_err()
        );
        fx.selected_id = Some(1);
        let track = fx.add_emitter(EmitterKind::Track).unwrap();
        assert_eq!(fx.document.node(track).unwrap().kind, EmitterKind::Track);
        assert_eq!(fx.document.roots.len(), 1);
        assert_eq!(fx.document.roots[0].children.len(), 2);
        assert_eq!(fx.document.roots[0].name, sprites_before[0].name);
        assert!(
            fx.document.roots[0]
                .children
                .iter()
                .all(|n| matches!(n.kind, EmitterKind::Ribbon | EmitterKind::Track))
        );
        let authored = fx.document.clone();
        assert!(fx.add_emitter(EmitterKind::Ring).is_err());
        assert!(fx.add_emitter(EmitterKind::Sprite).is_err());
        assert_eq!(fx.document, authored);
        fx.refresh_preview().unwrap();
        assert!(fx.preview_path.as_ref().unwrap().is_file());
        fx.save().unwrap();
        let saved = fs::read(&fx.path).unwrap();
        let reopened = EffectWorkspace::open(&project, &fx.path).unwrap();
        assert_eq!(reopened.document.roots, authored.roots);
        assert_eq!(fs::read(&fx.path).unwrap(), saved);
        assert!(
            fx.path
                .parent()
                .unwrap()
                .join("Texture/fire_tex.png")
                .is_file()
        );
        drop(reopened);
        drop(fx);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn project_owned_ring_creation_edit_preview_reopen_preserves_sprite_assets() {
        let project = std::env::temp_dir().join(format!(
            "wk-aeofx-ring-owned-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&project).unwrap();
        let sprite = EffectWorkspace::create(&project, "Original").unwrap();
        let sprite_bytes = fs::read(&sprite.path).unwrap();
        let mut ring = EffectWorkspace::create_ring(&project, "New Ring").unwrap();
        assert!(ring.is_ring());
        assert!(!ring.is_imported());
        assert_eq!(ring.document.roots.len(), 1);
        assert_eq!(ring.document.roots[0].name, "Ring");
        let id = ring.document.roots[0].id;
        assert_eq!(ring.document.roots[0].texture, None);
        assert!(ring.add_emitter(EmitterKind::Sprite).is_err());
        assert!(ring.add_emitter(EmitterKind::Ring).is_err());
        assert!(ring.remove_selected().is_err());
        assert!(
            ring.set_curve(id, CurveChannel::Size, vec![], false)
                .is_err()
        );
        assert!(!ring.dirty);
        ring.set_emission_rate(id, 30.0).unwrap();
        ring.set_lifetime(id, 3.0).unwrap();
        ring.rename_selected("Magic Ring", false).unwrap();
        ring.refresh_preview().unwrap();
        let preview = ring.preview_path.clone().unwrap();
        assert!(preview.is_file());
        assert_eq!(fs::read(&sprite.path).unwrap(), sprite_bytes);
        ring.save().unwrap();
        let bytes = fs::read(&ring.path).unwrap();
        assert_eq!(
            crate::project::fx_import::compile_ring_document(&ring.document).unwrap(),
            bytes
        );
        let mut reopened = EffectWorkspace::open(&project, &ring.path).unwrap();
        assert!(reopened.is_ring());
        assert_eq!(reopened.document.roots[0].name, "Magic Ring");
        assert_eq!(reopened.document.roots[0].emission_rate, Some(30.0));
        assert_eq!(reopened.document.roots[0].lifetime_seconds, Some(3.0));
        reopened.set_emission_rate(id, 15.0).unwrap();
        fs::write(&ring.path, b"externally changed").unwrap();
        assert!(reopened.save().is_err());
        assert_eq!(fs::read(&ring.path).unwrap(), b"externally changed");
        assert_eq!(fs::read(&sprite.path).unwrap(), sprite_bytes);
        drop(reopened);
        drop(ring);
        drop(sprite);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn adding_ring_to_authored_sprites_preserves_all_fields_undo_and_disk() {
        use crate::project::fx_authoring::{EmitterKind, SpawnShape};
        let project = std::env::temp_dir().join(format!(
            "wk-ring-full-preserve-{:032x}",
            rand::random::<u128>()
        ));
        let library = project.join(".assets").join("textures");
        fs::create_dir_all(&library).unwrap();
        let source = library.join("custom-flame.png");
        fs::write(&source, BUILTIN_TEXTURE).unwrap();
        let mut effect = EffectWorkspace::create(&project, "Original").unwrap();
        effect.set_shared_texture(&source, &project).unwrap();
        effect
            .set_spawn_motion_drag(
                1,
                SpawnShape::Sphere,
                1.5,
                [0.25, 2.0, 0.0],
                [0.0, -0.3, 0.0],
                0.6,
                false,
            )
            .unwrap();
        effect
            .set_curve(
                1,
                CurveChannel::Size,
                vec![
                    CurveKey {
                        time: 0.0,
                        value: 0.5,
                    },
                    CurveKey {
                        time: 1.0,
                        value: 2.5,
                    },
                ],
                false,
            )
            .unwrap();
        effect
            .set_curve(
                1,
                CurveChannel::Opacity,
                vec![
                    CurveKey {
                        time: 0.0,
                        value: 1.0,
                    },
                    CurveKey {
                        time: 1.0,
                        value: 0.25,
                    },
                ],
                false,
            )
            .unwrap();
        let smoke_id = effect.add_emitter(EmitterKind::Sprite).unwrap();
        effect
            .set_curve(
                smoke_id,
                CurveChannel::Red,
                vec![
                    CurveKey {
                        time: 0.0,
                        value: 0.2,
                    },
                    CurveKey {
                        time: 1.0,
                        value: 0.9,
                    },
                ],
                false,
            )
            .unwrap();
        effect.reparent_selected(Some(1)).unwrap();
        let authored_sprites = effect.document.roots.clone();
        let original_bytes = AeoFxCompiler::export(&effect.document).unwrap();
        effect.save().unwrap();
        assert_eq!(fs::read(&effect.path).unwrap(), original_bytes);
        let ring_id = effect.add_emitter(EmitterKind::Ring).unwrap();
        assert_eq!(
            effect.document.node(ring_id).unwrap().kind,
            EmitterKind::Ring
        );
        assert_eq!(effect.document.roots.len(), 2);
        assert_eq!(effect.document.roots[0], authored_sprites[0]);
        assert!(effect.add_emitter(EmitterKind::Ring).is_err());
        assert!(
            effect
                .set_curve(ring_id, CurveChannel::Size, vec![], false)
                .is_err()
        );
        assert!(
            effect
                .set_spawn_motion_drag(
                    ring_id,
                    SpawnShape::Point,
                    0.0,
                    [0.0; 3],
                    [0.0; 3],
                    0.0,
                    false
                )
                .is_err()
        );
        assert!(effect.reparent_selected(Some(1)).is_err());
        assert!(effect.set_shared_texture(&source, &project).is_err());
        assert_eq!(effect.document.roots[0], authored_sprites[0]);
        effect.undo();
        assert_eq!(effect.document.roots, authored_sprites);
        effect.redo();
        assert_eq!(
            effect.document.node(ring_id).unwrap().kind,
            EmitterKind::Ring
        );
        effect.set_emission_rate(ring_id, 15.0).unwrap();
        effect.set_lifetime(ring_id, 3.0).unwrap();
        effect.rename_selected("Magic Ring", false).unwrap();
        effect.refresh_preview().unwrap();
        assert!(effect.preview_path.as_ref().unwrap().is_file());
        let edited = effect.document.clone();
        let encoded = AeoFxCompiler::export(&edited).unwrap();
        let original_native = EfkEfc::parse(&original_bytes).unwrap();
        let merged_native = EfkEfc::parse(&encoded).unwrap();
        let old = original_native
            .chunks
            .iter()
            .find(|n| &n.tag == b"BIN_")
            .unwrap()
            .bytes
            .as_slice();
        let new = merged_native
            .chunks
            .iter()
            .find(|n| &n.tag == b"BIN_")
            .unwrap()
            .bytes
            .as_slice();
        let texture = "Texture/custom-flame.png";
        let delta = (texture.encode_utf16().count() as isize
            - "Texture/fire_tex.png".encode_utf16().count() as isize)
            * 2;
        let start = 298_usize.checked_add_signed(delta).unwrap();
        assert_eq!(
            &old[start..],
            &new[start..old.len()],
            "Adding Ring must preserve every original Sprite record byte"
        );
        assert_eq!(&old[..start - 40], &new[..start - 40]);
        assert_eq!(&old[start - 36..start - 4], &new[start - 36..start - 4]);
        effect.save().unwrap();
        let readback = EffectWorkspace::open(&project, &effect.path).unwrap();
        assert_eq!(readback.document.roots, edited.roots);
        assert_eq!(readback.document.roots[0], authored_sprites[0]);
        assert_eq!(readback.document.roots[1].kind, EmitterKind::Ring);
        assert_eq!(fs::read(&effect.path).unwrap(), encoded);
        assert!(
            effect
                .path
                .parent()
                .unwrap()
                .join("Texture/custom-flame.png")
                .exists()
        );
        drop(readback);
        drop(effect);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn approved_add_emitter_uses_mesh_not_model_and_guards_unsupported_nodes() {
        assert_eq!(
            EmitterKind::MENU.map(EmitterKind::label),
            ["Sprite", "Ring", "Ribbon", "Track", "Mesh"]
        );
        let project = std::env::temp_dir().join(format!(
            "wk-aeofx-emitter-menu-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&project).unwrap();
        let mut effect = EffectWorkspace::create(&project, "Choices").unwrap();
        let original = effect.document.clone();
        assert!(EmitterKind::Ribbon.supports_existing_effect());
        assert!(EmitterKind::Track.supports_existing_effect());
        for kind in [EmitterKind::Mesh] {
            assert!(!kind.supports_existing_effect());
            assert!(effect.add_emitter(kind).is_err());
            assert_eq!(effect.document, original);
            assert!(!effect.dirty);
        }
        assert!(EmitterKind::Ring.supports_existing_effect());
        let id = effect.add_emitter(EmitterKind::Sprite).unwrap();
        assert_eq!(
            effect.document.node(id).unwrap().name,
            format!("Sprite {id}")
        );
        assert!(effect.dirty);
        effect.save().unwrap();
        let reopened = EffectWorkspace::open(&project, &effect.path).unwrap();
        assert!(reopened.document.node(id).is_some());
        drop(reopened);
        drop(effect);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn hierarchy_curves_shared_texture_and_undo_survive_save_reopen() {
        let project = std::env::temp_dir().join(format!(
            "wk-aeofx-hierarchy-test-{:032x}",
            rand::random::<u128>()
        ));
        let library = project.join(".assets/textures");
        fs::create_dir_all(&library).unwrap();
        let texture = library.join("creator-fire.png");
        fs::write(&texture, BUILTIN_TEXTURE).unwrap();
        let mut workspace = EffectWorkspace::create(&project, "Created").unwrap();
        assert_eq!(workspace.add_sprite("Smoke").unwrap(), 2);
        assert_eq!(workspace.add_sprite("Sparks").unwrap(), 3);
        workspace.reparent_selected(Some(1)).unwrap();
        assert_eq!(workspace.document.roots[0].children[0].id, 3);
        workspace
            .set_curve(
                3,
                CurveChannel::Size,
                vec![
                    CurveKey {
                        time: 0.0,
                        value: 0.5,
                    },
                    CurveKey {
                        time: 1.0,
                        value: 2.0,
                    },
                ],
                false,
            )
            .unwrap();
        workspace.rename_selected("Sparks Glow", false).unwrap();
        workspace.undo();
        assert_eq!(workspace.document.node(3).unwrap().name, "Sparks");
        workspace.redo();
        assert_eq!(workspace.document.node(3).unwrap().name, "Sparks Glow");
        workspace.set_shared_texture(&texture, &project).unwrap();
        assert_eq!(
            workspace.document.node(3).unwrap().texture.as_deref(),
            Some("Texture/creator-fire.png")
        );
        assert!(
            workspace
                .path
                .parent()
                .unwrap()
                .join("Texture/creator-fire.png")
                .is_file()
        );
        let selected = workspace.selected_id;
        workspace.save().unwrap();
        let reopened = EffectWorkspace::open(&project, &workspace.path).unwrap();
        assert_eq!(reopened.document.roots, workspace.document.roots);
        assert_eq!(selected, Some(3));
        assert_eq!(reopened.document.node(3).unwrap().name, "Sparks Glow");
        assert_eq!(reopened.document.node(3).unwrap().size.len(), 2);
        assert_eq!(
            reopened.document.node(3).unwrap().texture.as_deref(),
            Some("Texture/creator-fire.png")
        );
        drop(reopened);
        drop(workspace);
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn preview_is_explicit_and_keeps_project_effect_unchanged() {
        let project = std::env::temp_dir().join(format!(
            "wk-aeofx-preview-test-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&project).unwrap();
        let mut effect = EffectWorkspace::create(&project, "Preview").unwrap();
        let original = fs::read(&effect.path).unwrap();
        effect.refresh_preview().unwrap();
        assert!(
            effect.preview_path.is_none(),
            "idle frames must not recompile"
        );
        effect.set_emission_rate(1, 43.0).unwrap();
        effect.refresh_preview().unwrap();
        let first_path = effect.preview_path.as_ref().unwrap().clone();
        assert!(first_path.is_file());
        assert!(
            first_path
                .parent()
                .unwrap()
                .join("Texture/fire_tex.png")
                .is_file()
        );
        assert_eq!(fs::read(&effect.path).unwrap(), original);
        let preview = EfkEfc::parse(&fs::read(&first_path).unwrap()).unwrap();
        let restored = EffectDocument::from_effekseer_template("test", "test", &preview).unwrap();
        assert_eq!(restored.roots[0].emission_rate, Some(43.0));
        let first_revision = effect.preview_revision;
        effect.refresh_preview().unwrap();
        assert_eq!(effect.preview_path.as_deref(), Some(first_path.as_path()));
        assert_eq!(effect.preview_revision, first_revision);
        effect.set_lifetime(1, 3.0).unwrap();
        effect.refresh_preview().unwrap();
        assert_ne!(effect.preview_path.as_deref(), Some(first_path.as_path()));
        effect.save().unwrap();
        assert!(!effect.dirty);
        assert_eq!(fs::read(&effect.path).unwrap(), effect.saved_bytes);
        drop(effect);
        assert!(
            !first_path.exists(),
            "preview copies must be released on close"
        );
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn invalid_names_and_corrupt_effects_are_not_silently_overwritten() {
        let project =
            std::env::temp_dir().join(format!("wk-aeofx-invalid-{:032x}", rand::random::<u128>()));
        fs::create_dir(&project).unwrap();
        for name in ["", "..", "../other", "Name:Bad", "  ", ".hidden"] {
            assert!(EffectWorkspace::create(&project, name).is_err());
        }
        let effect = EffectWorkspace::create(&project, "Demo").unwrap();
        fs::write(&effect.path, b"broken").unwrap();
        assert!(EffectWorkspace::open(&project, &effect.path).is_err());
        fs::remove_dir_all(project).unwrap();
    }
}

#[derive(Clone, Debug)]
struct EffectNodePayload {
    id: u64,
}

impl crate::editor::Editor {
    pub(crate) fn draw_aeofx_workspace(&mut self, ctx: &egui::Context) {
        let Some(workspace) = self.effect_workspace.as_mut() else {
            return;
        };
        let mut issue = self.effect_issue.take();
        let mut close = false;
        let mut saved = false;
        egui::TopBottomPanel::top("aeofx_editor_actions").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("AeoFX Editor");
                if ui
                    .add_enabled(
                        workspace.edit_history.undo_steps() > 0,
                        egui::Button::new("Undo"),
                    )
                    .clicked()
                {
                    workspace.undo();
                }
                if ui
                    .add_enabled(
                        workspace.edit_history.redo_steps() > 0,
                        egui::Button::new("Redo"),
                    )
                    .clicked()
                {
                    workspace.redo();
                }
                if ui.button("Save").clicked() {
                    let result = workspace.finish_edit().and_then(|()| workspace.save());
                    match result {
                        Ok(()) => {
                            saved = true;
                            issue = None;
                        }
                        Err(error) => issue = Some(error),
                    }
                }
                if ui
                    .button(if workspace.playing {
                        "Pause"
                    } else {
                        "Preview"
                    })
                    .clicked()
                {
                    workspace.playing = !workspace.playing;
                }
                if ui.button("Reset").clicked() {
                    workspace.reset_preview();
                    workspace.playing = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
            if workspace.dirty {
                ui.small("Unsaved changes");
            }
            if let Some(message) = issue.as_deref() {
                ui.colored_label(ui.visuals().error_fg_color, message);
            }
        });
        egui::TopBottomPanel::bottom("aeofx_curves")
            .resizable(true)
            .default_height(185.0)
            .min_height(110.0)
            .show(ctx, |ui| {
                ui.heading("Timeline/Curves");
                let target = workspace
                    .selected_id
                    .and_then(|id| workspace.document.node(id).cloned().map(|node| (id, node)));
                if let Some((id, node)) = target {
                    let native = !workspace.is_imported()
                        && !workspace.is_ring()
                        && node.kind == EmitterKind::Sprite;
                    let changed = ui
                        .add_enabled_ui(native, |ui| {
                            crate::editor::effect_curves::draw(ui, &node, &mut workspace.curves)
                        })
                        .inner;
                    if let Some(changed) = changed {
                        let dragging = ui.input(|input| input.pointer.any_down());
                        if let Err(error) =
                            workspace.set_curve(id, changed.channel, changed.keys, dragging)
                        {
                            issue = Some(error);
                        } else {
                            workspace.last_edit_at = ui.input(|input| input.time);
                        }
                    }
                }
            });
        egui::SidePanel::left("aeofx_hierarchy")
            .resizable(true)
            .default_width(225.0)
            .min_width(150.0)
            .show(ctx, |ui| {
                ui.heading("Effect Hierarchy");
                ui.horizontal(|ui| {
                    ui.menu_button("Add Emitter", |ui| {
                        for kind in EmitterKind::MENU {
                            let enabled = workspace.can_add_emitter(kind);
                            if ui
                                .add_enabled(enabled, egui::Button::new(kind.label()))
                                .clicked()
                            {
                                if let Err(error) = workspace.add_emitter(kind) {
                                    issue = Some(error);
                                }
                                ui.close_menu();
                            }
                        }
                    });
                    if ui
                        .add_enabled(
                            !workspace.is_imported() && !workspace.is_ring(),
                            egui::Button::new("Delete"),
                        )
                        .clicked()
                    {
                        if let Err(error) = workspace.remove_selected() {
                            issue = Some(error);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !workspace.is_imported() && !workspace.is_ring(),
                            egui::Button::new("Move to Root"),
                        )
                        .clicked()
                    {
                        if let Err(error) = workspace.reparent_selected(None) {
                            issue = Some(error);
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    fn render_nodes(
                        ui: &mut egui::Ui,
                        nodes: &[EffectNode],
                        selected: &mut Option<u64>,
                        level: usize,
                        moved: &mut Option<(u64, u64)>,
                        editable: bool,
                    ) {
                        for node in nodes {
                            ui.horizontal(|ui| {
                                ui.add_space((level as f32 * 12.0).min(160.0));
                                let response =
                                    ui.selectable_label(*selected == Some(node.id), &node.name);
                                if response.clicked() {
                                    *selected = Some(node.id);
                                }
                                if editable {
                                    response
                                        .dnd_set_drag_payload(EffectNodePayload { id: node.id });
                                    if let Some(dropped) =
                                        response.dnd_release_payload::<EffectNodePayload>()
                                    {
                                        if dropped.id != node.id {
                                            *moved = Some((dropped.id, node.id));
                                        }
                                    }
                                }
                            });
                            render_nodes(ui, &node.children, selected, level + 1, moved, editable);
                        }
                    }
                    let mut moved = None;
                    let editable = !workspace.is_imported() && !workspace.is_ring();
                    render_nodes(
                        ui,
                        &workspace.document.roots,
                        &mut workspace.selected_id,
                        0,
                        &mut moved,
                        editable,
                    );
                    if let Some((source, target)) = moved {
                        workspace.selected_id = Some(source);
                        if let Err(error) = workspace.reparent_selected(Some(target)) {
                            issue = Some(error);
                        }
                    }
                });
            });
        egui::SidePanel::right("aeofx_properties")
            .resizable(true)
            .default_width(275.0)
            .min_width(190.0)
            .show(ctx, |ui| {
                ui.heading("Properties");
                ui.separator();
                let selected = workspace
                    .selected_id
                    .and_then(|id| workspace.document.node(id))
                    .map(|node| {
                        (
                            node.id,
                            node.name.clone(),
                            node.emission_rate,
                            node.lifetime_seconds,
                            node.kind,
                        )
                    });
                if let Some((id, name, rate, lifetime, kind)) = selected {
                    ui.strong(&name);
                    ui.label("Emission Rate");
                    let mut value = rate.unwrap_or(20.0);
                    let changed = ui
                        .add_enabled(
                            workspace.compatibility.can_edit_rate(id),
                            egui::DragValue::new(&mut value)
                                .speed(0.1)
                                .range(0.1..=100_000.0),
                        )
                        .changed();
                    if changed {
                        if let Err(error) = workspace.set_emission_rate_drag(
                            id,
                            value,
                            ctx.input(|input| input.pointer.any_down()),
                        ) {
                            issue = Some(error);
                        } else {
                            workspace.last_edit_at = ctx.input(|input| input.time);
                        }
                    }
                    ui.label("Lifetime");
                    let mut value = lifetime.unwrap_or(2.0);
                    let changed = ui
                        .add_enabled(
                            workspace.compatibility.can_edit_lifetime(id),
                            egui::DragValue::new(&mut value)
                                .speed(0.05)
                                .range(0.001..=1_000.0),
                        )
                        .changed();
                    if changed {
                        if let Err(error) = workspace.set_lifetime_drag(
                            id,
                            value,
                            ctx.input(|input| input.pointer.any_down()),
                        ) {
                            issue = Some(error);
                        } else {
                            workspace.last_edit_at = ctx.input(|input| input.time);
                        }
                    }
                    ui.separator();
                    ui.label("Name");
                    let mut edited_name = name.clone();
                    let name_response = ui.add_enabled(
                        !workspace.is_imported(),
                        egui::TextEdit::singleline(&mut edited_name),
                    );
                    if name_response.changed() {
                        if let Err(error) = workspace.rename_selected(&edited_name, true) {
                            issue = Some(error);
                        } else {
                            workspace.last_edit_at = ctx.input(|input| input.time);
                        }
                    }
                    if let Some(current) = workspace.document.node(id).cloned() {
                        use crate::project::fx_authoring::SpawnShape;
                        ui.separator();
                        ui.label("Spawn Shape");
                        let mut shape = current.spawn_shape;
                        let mut radius = current.spawn_radius;
                        let mut velocity = current.velocity;
                        let mut acceleration = current.acceleration;
                        let mut drag = current.drag;
                        let mut changed = false;
                        ui.add_enabled_ui(
                            !workspace.is_imported()
                                && !workspace.is_ring()
                                && current.kind == EmitterKind::Sprite,
                            |ui| {
                                egui::ComboBox::from_id_salt("aeofx_spawn_shape")
                                    .selected_text(format!("{shape:?}"))
                                    .show_ui(ui, |ui| {
                                        for choice in [
                                            SpawnShape::Point,
                                            SpawnShape::Circle,
                                            SpawnShape::Sphere,
                                            SpawnShape::Line,
                                        ] {
                                            changed |= ui
                                                .selectable_value(
                                                    &mut shape,
                                                    choice,
                                                    format!("{choice:?}"),
                                                )
                                                .changed();
                                        }
                                    });
                                if shape != SpawnShape::Point {
                                    ui.label("Spawn Radius");
                                    changed |= ui
                                        .add(
                                            egui::DragValue::new(&mut radius)
                                                .speed(0.1)
                                                .range(0.0..=1000.0),
                                        )
                                        .changed();
                                }
                                ui.label("Velocity (X / Y / Z)");
                                ui.horizontal(|ui| {
                                    for component in &mut velocity {
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(component)
                                                    .speed(0.05)
                                                    .range(-1000.0..=1000.0),
                                            )
                                            .changed();
                                    }
                                });
                                ui.label("Acceleration (X / Y / Z)");
                                ui.horizontal(|ui| {
                                    for component in &mut acceleration {
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(component)
                                                    .speed(0.05)
                                                    .range(-1000.0..=1000.0),
                                            )
                                            .changed();
                                    }
                                });
                                ui.label("Drag");
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut drag)
                                            .speed(0.05)
                                            .range(0.0..=100.0),
                                    )
                                    .changed();
                            },
                        );
                        if changed {
                            if let Err(error) = workspace.set_spawn_motion_drag(
                                id,
                                shape,
                                radius,
                                velocity,
                                acceleration,
                                drag,
                                ctx.input(|input| input.pointer.any_down()),
                            ) {
                                issue = Some(error);
                            } else {
                                workspace.last_edit_at = ctx.input(|input| input.time);
                            }
                        }
                    }
                    ui.separator();
                    ui.label("Texture");
                    let displayed = workspace
                        .document
                        .node(id)
                        .and_then(|node| node.texture.as_deref())
                        .unwrap_or("None")
                        .to_owned();
                    let (rect, drop_response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width().clamp(72.0, 300.0), 44.0),
                        egui::Sense::hover(),
                    );
                    let hovering=drop_response.dnd_hover_payload::<
                        crate::editor::editor::asset_browser::ProjectAssetDragPayload>();
                    let stroke = egui::Stroke::new(
                        if hovering.is_some() { 2.0 } else { 1.0 },
                        if hovering.is_some() {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().widgets.inactive.fg_stroke.color
                        },
                    );
                    ui.painter().rect_stroke(rect, 5.0, stroke);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        displayed,
                        egui::TextStyle::Small.resolve(ui.style()),
                        ui.visuals().text_color(),
                    );
                    if let Some(payload)=drop_response.dnd_release_payload::<
                        crate::editor::editor::asset_browser::ProjectAssetDragPayload>()
                    {
                        let project=workspace.path.ancestors().nth(4);
                        if payload.category=="Textures" && project.is_some_and(|root|
                            root.canonicalize().ok()==payload.project_root.canonicalize().ok()) {
                            if let Err(error)=workspace.set_shared_texture(
                                &payload.path,&payload.project_root) {issue=Some(error);}
                        } else {
                            issue=Some("Drop a Texture from the current project".into());
                        }
                    }
                }
            });
        let preview = egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.heading("Preview");
                ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
            });
        self.viewport_rect = preview.response.rect;
        self.viewport_ppp = ctx.pixels_per_point();
        if !ctx.input(|input| input.pointer.any_down()) {
            if let Err(error) = workspace.finish_edit() {
                issue = Some(error);
            }
        }
        if workspace.pending_preview_update {
            let ready = ctx.input(|input| {
                !input.pointer.any_down() && input.time - workspace.last_edit_at >= 0.40
            });
            if ready {
                if let Err(error) = workspace.refresh_preview() {
                    workspace.pending_preview_update = false;
                    issue = Some(error);
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(420));
            }
        }
        if workspace.playing {
            ctx.request_repaint();
        }
        if close {
            self.effect_visible = false;
        }
        if saved {
            self.shell.refresh(None);
        }
        self.effect_issue = issue;
    }
}
