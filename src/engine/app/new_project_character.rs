// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::App;
use crate::project::character_offerings::{self, Offering, Setup};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
};

pub(crate) struct CharacterSetupJob {
    pub project: PathBuf,
    offering: Offering,
    setup: Setup,
    rx: Receiver<Result<String, String>>,
    progress: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    pub status: String,
    pub error: Option<String>,
    pub ready: Option<String>,
    pub finished: bool,
}
impl CharacterSetupJob {
    pub fn start(project: PathBuf, offering: Offering, setup: Setup) -> Self {
        let (_, rx) = mpsc::channel();
        let mut job = Self {
            project,
            offering,
            setup,
            rx,
            progress: Arc::new(AtomicU64::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            status: "Preparing Character".into(),
            error: None,
            ready: None,
            finished: false,
        };
        job.spawn();
        job
    }
    fn spawn(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.rx = rx;
        self.cancel = Arc::new(AtomicBool::new(false));
        self.progress.store(0, Ordering::Relaxed);
        self.finished = false;
        self.error = None;
        self.ready = None;
        let project = self.project.clone();
        let c = self.offering.clone();
        let setup = self.setup;
        let cancel = self.cancel.clone();
        let progress = self.progress.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                let root = character_offerings::cache_root();
                if !character_offerings::installed(&root, &c) {
                    character_offerings::download(&root, &c, cancel.as_ref(), &|done, total| {
                        progress.store(
                            if total > 0 {
                                done.saturating_mul(7000) / total
                            } else {
                                0
                            },
                            Ordering::Relaxed,
                        );
                    })?;
                }
                if cancel.load(Ordering::Acquire) {
                    return Err("Character download cancelled".into());
                }
                progress.store(7500, Ordering::Relaxed);
                let _gate = super::pack_install_queue::SHARED_PACK_INSTALL
                    .lock()
                    .map_err(|_| "Shared asset installer unavailable".to_owned())?;
                let name = character_offerings::install_character(&project, &root, &c, setup)?;
                progress.store(10000, Ordering::Relaxed);
                Ok(name)
            })();
            let _ = tx.send(result);
        });
    }
    pub fn poll(&mut self) {
        if self.finished {
            return;
        }
        match self.rx.try_recv() {
            Ok(Ok(name)) => {
                self.ready = Some(name);
                self.finished = true;
                self.status = "Ready".into();
            }
            Ok(Err(e)) => {
                self.error = Some(e);
                self.finished = true;
                self.status = "Failed".into();
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Character setup worker disconnected".into());
                self.finished = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
    pub fn retry(&mut self) {
        if self.finished {
            self.spawn();
        }
    }
    pub fn fraction(&self) -> f32 {
        self.progress.load(Ordering::Relaxed).min(10000) as f32 / 10000.0
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl App {
    pub(crate) fn service_character_setup_jobs(&mut self, ctx: &egui::Context) {
        for job in &mut self.background_character_jobs {
            job.poll();
            if job.finished && job.error.is_none() {
                self.asset_manager_packs
                    .characters
                    .mark_installed(&job.offering.id);
                self.new_project_packs
                    .characters
                    .mark_installed(&job.offering.id);
            }
        }
        let active = self.loaded_project_path.clone();
        if let Some(project) = active
            .as_ref()
            .filter(|p| !self.project_loading() && self.editor.mode == super::EditorMode::Editor)
        {
            for job in &mut self.background_character_jobs {
                if !crate::project::project::same_path(project, &job.project) {
                    continue;
                }
                let Some(name) = job.ready.take() else {
                    continue;
                };
                let result = crate::project::character_asset::assign_to_world(
                    project,
                    &name,
                    &mut self.world,
                )
                .and_then(|_| {
                    crate::world::persistence::save_world(&self.world, &project.join("world.dat"))
                        .map_err(|e| e.to_string())
                });
                if let Err(error) = result {
                    job.error = Some(format!(
                        "Character imported but could not be activated: {error}"
                    ));
                } else {
                    crate::character::package::reload_packages(Some(project));
                    self.editor.invalidate_asset_pack_catalog();
                    crate::editor::tools::refresh_asset_catalogs(ctx, project);
                    self.renderer.invalidate_texture_cache();
                }
            }
        }
        if let Some(job) = self
            .background_character_jobs
            .iter()
            .find(|job| !job.finished)
        {
            if crate::engine::progress_widget::draw_cancellable_progress(
                ctx,
                "wk_character_notification",
                "Character Download",
                Some(job.fraction().min(0.99)),
                &format!(
                    "{}: {}",
                    job.project
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                    job.offering.name
                ),
            ) {
                job.cancel();
            }
        }
        let mut retry = None;
        let mut dismiss = None;
        for (i, job) in self.background_character_jobs.iter().enumerate() {
            if let Some(error) = job.error.as_ref() {
                egui::Window::new("Character Setup")
                    .id(egui::Id::new(("wk_character_error", i)))
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("{}: {error}", job.offering.name));
                        ui.horizontal(|ui| {
                            if ui.button("Retry").clicked() {
                                retry = Some(i);
                            }
                            if ui.button("Dismiss").clicked() {
                                dismiss = Some(i);
                            }
                        });
                    });
            }
        }
        if let Some(i) = retry {
            self.background_character_jobs[i].retry();
        }
        if let Some(i) = dismiss {
            self.background_character_jobs[i].error = None;
        }
        self.background_character_jobs
            .retain(|job| !job.finished || job.ready.is_some() || job.error.is_some());
        if self
            .background_character_jobs
            .iter()
            .any(|job| !job.finished || job.ready.is_some())
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
