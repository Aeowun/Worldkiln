// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver},
    },
};

use crate::project::asset_packs::{self, Catalog, Pack};

pub(super) static SHARED_PACK_INSTALL: std::sync::Mutex<()> = std::sync::Mutex::new(());

enum QueueEvent {
    Stage(String),
    Published(String),
    Finished(Result<(), String>),
}

pub(crate) struct PackInstallQueue {
    pub(crate) project: PathBuf,
    root: PathBuf,
    packs: Vec<Pack>,
    receiver: Receiver<QueueEvent>,
    progress: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
    pub(crate) status: String,
    pub(crate) error: Option<String>,
    pub(crate) finished: bool,
    pub(crate) refresh_pending: bool,
}

impl PackInstallQueue {
    pub(crate) fn start(
        catalog: &Catalog,
        selected: &BTreeSet<String>,
        root: PathBuf,
        project: PathBuf,
    ) -> Result<Self, String> {
        if !project.is_dir() {
            return Err("Asset pack project is unavailable".into());
        }
        let packs = asset_packs::order_by_download_size(catalog.selected(selected)?);
        if packs.is_empty() {
            return Err("Select an asset pack to install".into());
        }
        let (_, receiver) = mpsc::channel();
        let mut queue = Self {
            project,
            root,
            packs,
            receiver,
            progress: Arc::new(AtomicU64::new(0)),
            cancel: Arc::new(AtomicBool::new(false)),
            status: "Preparing asset packs...".into(),
            error: None,
            finished: false,
            refresh_pending: false,
        };
        queue.spawn();
        Ok(queue)
    }

    fn spawn(&mut self) {
        let (sender, receiver) = mpsc::channel();
        self.receiver = receiver;
        self.progress.store(0, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.error = None;
        self.finished = false;
        self.status = "Preparing asset packs...".into();
        let root = self.root.clone();
        let project = self.project.clone();
        let packs = self.packs.clone();
        let progress = self.progress.clone();
        let cancel = self.cancel.clone();
        std::thread::spawn(move || {
            let total = packs.len();
            let mut index = 0usize;
            let result = asset_packs::run_ordered_packs(
                &packs,
                cancel.as_ref(),
                |pack| {
                    let _guard = SHARED_PACK_INSTALL
                        .lock()
                        .map_err(|_| "Shared pack installer is unavailable".to_owned())?;
                    if cancel.load(Ordering::Acquire) {
                        return Err("Asset installation canceled".into());
                    }
                    index += 1;
                    let descriptor = format!("Pack {index}/{total}: {}", pack.name);
                    let _ = sender.send(QueueEvent::Stage(format!(
                        "{descriptor} - Downloading / verifying"
                    )));
                    progress.store(0, Ordering::Relaxed);
                    if !asset_packs::installed(&root, &pack.reference()) {
                        asset_packs::download_install(
                            &root,
                            pack,
                            cancel.as_ref(),
                            &|done, size| {
                                progress.store(
                                    if size == 0 {
                                        0
                                    } else {
                                        done.saturating_mul(7000) / size
                                    }
                                    .min(7000),
                                    Ordering::Relaxed,
                                );
                            },
                        )?;
                    }
                    asset_packs::verify_installed_cache(&root, &pack.reference())?;
                    if cancel.load(Ordering::Acquire) {
                        return Err("Asset installation canceled".into());
                    }
                    let reference = pack.reference();
                    let before = asset_packs::references(&project)?;
                    if before
                        .iter()
                        .any(|bound| bound.id == reference.id && bound != &reference)
                    {
                        return Err(format!(
                            "Project already uses another revision of {}",
                            pack.name
                        ));
                    }
                    let _ = sender.send(QueueEvent::Stage(format!(
                        "{descriptor} - Installing into project"
                    )));
                    progress.store(7000, Ordering::Relaxed);
                    asset_packs::clone_into_project(
                        &root,
                        &project,
                        &[pack.reference()],
                        cancel.as_ref(),
                        &|done, size| {
                            progress.store(
                                7000 + if size == 0 {
                                    0
                                } else {
                                    done.saturating_mul(2900) / size
                                }
                                .min(2900),
                                Ordering::Relaxed,
                            );
                        },
                    )?;
                    if cancel.load(Ordering::Acquire) {
                        return Err("Asset installation canceled".into());
                    }
                    let mut bound = asset_packs::references(&project)?;
                    let reference = pack.reference();
                    if let Some(existing) = bound.iter().find(|r| r.id == reference.id) {
                        if existing != &reference {
                            return Err(format!(
                                "Project already uses another revision of {}",
                                pack.name
                            ));
                        }
                    } else {
                        bound.push(reference);
                    }
                    asset_packs::bind(&root, &project, bound)?;
                    progress.store(10000, Ordering::Relaxed);
                    Ok(())
                },
                |pack| {
                    let _ = sender.send(QueueEvent::Published(pack.name.clone()));
                },
            );
            let _ = sender.send(QueueEvent::Finished(result));
        });
    }

    pub(crate) fn poll(&mut self) {
        use std::sync::mpsc::TryRecvError;
        loop {
            match self.receiver.try_recv() {
                Ok(QueueEvent::Stage(status)) => self.status = status,
                Ok(QueueEvent::Published(name)) => {
                    self.refresh_pending = true;
                    crate::engine::diagnostics::report(
                        "INFO",
                        "Assets",
                        format!(
                            "Installed and published {name} into {}",
                            self.project.display()
                        ),
                    );
                }
                Ok(QueueEvent::Finished(result)) => {
                    self.finished = true;
                    match result {
                        Ok(()) => self.status = "Asset packs installed".into(),
                        Err(error) => {
                            self.refresh_pending = true;
                            self.status = "Asset installation failed".into();
                            crate::engine::diagnostics::report(
                                "ERROR",
                                "Assets",
                                format!("{}: {error}", self.project.display()),
                            );
                            self.error = Some(error);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.finished {
                        self.finished = true;
                        self.error = Some("Asset installation worker stopped unexpectedly".into());
                    }
                    break;
                }
            }
        }
    }

    pub(crate) fn fraction(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / 10000.0
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(crate) fn retry(&mut self) {
        if self.finished && self.error.is_some() {
            self.spawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, File},
        io::Write,
        path::Path,
        time::{Duration, Instant},
    };

    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "wk-queue-project-owner-{:032x}",
                rand::random::<u128>()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn cached_pack(root: &Path, fixture: &Path) -> asset_packs::Pack {
        let source = fixture.join("controller.aeo");
        fs::write(&source, b"// controller fixture\n").unwrap();
        let (entry_hash, entry_bytes) = crate::engine::game_package::sha256_file(&source).unwrap();
        let archive = fixture.join("controller-pack.zip");
        let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
        zip.start_file(
            "files/controller.aeo",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&fs::read(source).unwrap()).unwrap();
        zip.finish().unwrap();
        let (sha256, bytes) = crate::engine::game_package::sha256_file(&archive).unwrap();
        let pack = asset_packs::Pack {
            id: "queue-owner-test".into(),
            name: "Queue Owner Fixture".into(),
            description: "Deterministic project ownership regression".into(),
            category: "Controllers".into(),
            version: "1.0".into(),
            min_schema: 1,
            max_schema: 1,
            min_engine_schema: 1,
            max_engine_schema: 1,
            tag: "fixture-v1".into(),
            asset_id: 123,
            asset_name: "controller-pack.zip".into(),
            bytes,
            sha256,
            license: "CC0".into(),
            attribution: "Test-generated".into(),
            entries: vec![asset_packs::Entry {
                path: "files/controller.aeo".into(),
                target: ".assets/controllers/fixture-controller.aeo".into(),
                bytes: entry_bytes,
                sha256: entry_hash,
            }],
            dependencies: Vec::new(),
            materials: Vec::new(),
        };
        asset_packs::install_archive(root, &pack, &archive, &AtomicBool::new(false), &|_, _| {})
            .unwrap();
        pack
    }

    #[test]
    fn queue_installs_only_into_owning_project_after_active_project_switch() {
        let f = Scratch::new();
        let root = f.0.join("cache");
        let a = f.0.join("ProjectA");
        let b = f.0.join("ProjectB");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let pack = cached_pack(&root, &f.0);
        let catalog = asset_packs::Catalog {
            schema: 1,
            packs: vec![pack.clone()],
        };
        catalog.validate().unwrap();
        let selected = BTreeSet::from([pack.id.clone()]);
        let mut job = PackInstallQueue::start(&catalog, &selected, root, a.clone()).unwrap();
        let active_project = b.clone();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !job.finished && Instant::now() < deadline {
            job.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            job.finished,
            "Cached fixture should install without a network request"
        );
        assert!(job.error.is_none(), "{:?}", job.error);
        assert_eq!(job.project, a);
        assert_ne!(job.project, active_project);
        assert!(job.refresh_pending);
        assert!(
            job.project
                .join(".assets/controllers/fixture-controller.aeo")
                .is_file()
        );
        assert!(
            !b.join(".assets/controllers/fixture-controller.aeo")
                .exists()
        );
        assert_eq!(
            asset_packs::references(&job.project).unwrap(),
            vec![pack.reference()]
        );
        assert!(asset_packs::references(&b).unwrap().is_empty());
    }
}
