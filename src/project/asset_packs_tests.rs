// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
#[test]
fn legacy_engine_release_packs_are_excluded() {
    let fixture = Fixture::new();
    let (mut supported, _) = pack(&fixture);
    supported.tag = "asset-packs-v1.8.0".into();
    let mut legacy = supported.clone();
    legacy.id = "legacy-starter-pack".into();
    legacy.asset_id = 9999;
    legacy.tag = "win11-v0.8.7".into();

    assert_eq!(api_for_pack(&legacy), ASSET_API);
    assert!(Catalog {
        schema: 1,
        packs: vec![legacy.clone()],
    }
    .validate()
    .is_err());
    let bytes = serde_json::to_vec(&Catalog {
        schema: 1,
        packs: vec![legacy, supported.clone()],
    })
    .unwrap();
    let filtered = Catalog::parse(&bytes).unwrap();
    assert_eq!(filtered.packs.len(), 1);
    assert_eq!(filtered.packs[0].id, supported.id);
}
#[test]
fn catalogue_blocking_response_reads_without_tokio_runtime() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut buffer = [0; 4096];
        stream.read(&mut buffer).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
            .unwrap();
    });
    let c = client_builder()
        .https_only(false)
        .no_proxy()
        .build()
        .unwrap();
    assert_eq!(json_get(&c, &url).unwrap(), serde_json::json!({}));
    server.join().unwrap();
    assert!(json_get(&c, &url).is_err());
}

#[test]
#[ignore = "Explicit live GitHub verification; isolated cache only"]
fn catalogue_live_fetch() {
    let f = Fixture::new();
    let c = fetch_catalog(&f.0.join("cache")).unwrap();
    assert!(c.packs.iter().any(|p| p.id == "pbr-4k"));
    assert!(c.packs.iter().any(|p| p.id == "pbr-style"));
    let refreshed = fetch_catalog(&f.0.join("cache")).unwrap();
    assert_eq!(c.packs.len(), refreshed.packs.len());
    assert_eq!(
        cached_catalog(&f.0.join("cache")).unwrap().packs.len(),
        c.packs.len()
    );
    let p = c.packs.iter().find(|p| p.id == "pbr-style").unwrap();
    let cancel = AtomicBool::new(false);
    assert!(
        download_install(&f.0.join("cache"), p, &cancel, &|got, _| {
            if got > 0 {
                cancel.store(true, Ordering::Release);
            }
        })
        .is_err()
    );
    assert!(!installed(&f.0.join("cache"), &p.reference()));
    download_install(&f.0.join("cache"), p, &AtomicBool::new(false), &|_, _| {}).unwrap();
    verify_published_install(&f.0.join("cache"), &f.0.join("Project"), p);
}
fn verify_published_install(cache: &Path, project: &Path, p: &Pack) {
    assert!(installed(cache, &p.reference()));
    fs::create_dir_all(project).unwrap();
    bind(cache, project, vec![p.reference()]).unwrap();
    assert_eq!(
        project_roots_at(cache, project).unwrap(),
        vec![location(cache, &p.reference()).unwrap()]
    );
    assert!(!project.join(".assets/textures").exists());
    for entry in &p.entries {
        let path = resource_path_at(
            cache,
            project,
            &resource_identity(&p.reference(), &entry.target),
        )
        .unwrap();
        assert_eq!(
            crate::engine::game_package::sha256_file(&path).unwrap(),
            (entry.sha256.clone(), entry.bytes)
        );
    }
    let base = location(cache, &p.reference()).unwrap();
    let documents = p
        .materials
        .iter()
        .map(|m| {
            (
                m.name.clone(),
                super::super::material::read(&base, &m.name).unwrap(),
            )
        })
        .collect();
    let materials = super::super::material::MaterialCatalog::from_documents(documents);
    for m in &p.materials {
        materials.resolve(&m.document.id).unwrap();
    }
    let assets = crate::editor::asset_management::scan_assets(&base);
    assert_eq!(
        assets
            .iter()
            .filter(|a| a.kind == crate::editor::asset_management::ManagedAssetKind::Material)
            .count(),
        6
    );
    assert_eq!(
        assets
            .iter()
            .filter(|a| a.kind == crate::editor::asset_management::ManagedAssetKind::Texture)
            .count(),
        36
    );
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("wk-asset004-{:032x}", rand::random::<u128>()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn pack(f: &Fixture) -> (Pack, PathBuf) {
    let file = f.0.join("original.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 40, 60, 255]))
        .save(&file)
        .unwrap();
    let (digest, size) = crate::engine::game_package::sha256_file(&file).unwrap();
    let zip = f.0.join("pack.zip");
    let mut writer = zip::ZipWriter::new(File::create(&zip).unwrap());
    writer
        .start_file("maps/color.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(&fs::read(file).unwrap()).unwrap();
    writer.finish().unwrap();
    let (hash, bytes) = crate::engine::game_package::sha256_file(&zip).unwrap();
    (
        Pack {
            id: "basic-textures".into(),
            name: "Basic Texture Pack".into(),
            description: "Optional PNG textures".into(),
            category: "Textures".into(),
            version: "1.0.0".into(),
            min_schema: 1,
            max_schema: 1,
            min_engine_schema: 1,
            max_engine_schema: 1,
            tag: "asset-packs-v1.0.0".into(),
            asset_id: 42,
            asset_name: "BasicTextures.zip".into(),
            bytes,
            sha256: hash,
            license: "CC0-1.0".into(),
            attribution: "Fixture".into(),
            entries: vec![Entry {
                path: "maps/color.png".into(),
                target: ".assets/textures/color.png".into(),
                bytes: size,
                sha256: digest,
            }],
            dependencies: Vec::new(),
            materials: Vec::new(),
        },
        zip,
    )
}
fn catalogue_live_micro_single_prop_install() {
    let f = Fixture::new();
    let cache = f.0.join("cache");
    let catalogue = fetch_catalog(&cache).unwrap();
    assert!(catalogue.packs.iter().any(|p| p.id == "pbr-4k"));
    assert!(catalogue.packs.iter().any(|p| p.id == "kenney-nature-kit"));
    assert!(
        catalogue
            .packs
            .iter()
            .any(|p| p.id == "kenney-blocky-characters")
    );
    let pack = catalogue
        .packs
        .iter()
        .find(|p| p.id == "furniture-chairs")
        .unwrap();
    assert_eq!(api_for_pack(pack), ASSET_API);
    assert!(pack.bytes < 64 * 1024);
    let cancel = AtomicBool::new(false);
    download_install(&cache, pack, &cancel, &|_, _| {}).unwrap();
    assert!(installed(&cache, &pack.reference()));
    let project = f.0.join("Game");
    fs::create_dir(&project).unwrap();
    clone_into_project(&cache, &project, &[pack.reference()], &cancel, &|_, _| {}).unwrap();
    let actors = project.join(".assets/entities");
    let matching = fs::read_dir(&actors)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path.join("package.json").is_file()
                && path.join("source.glb").is_file()
        })
        .filter(|path| {
            crate::engine::game_package::sha256_file(&path.join("source.glb"))
                .ok()
                .is_some_and(|(sha, _)| {
                    pack.entries.iter().any(|e| {
                        e.target == ".assets/entities/furniture-chairs/chair.glb" && e.sha256 == sha
                    })
                })
        })
        .count();
    assert_eq!(
        matching, 1,
        "A chair must be imported as one native Actor package, with its original GLB preserved"
    );
    assert!(
        project
            .join(".assets/licenses/furniture-chairs/License.txt")
            .is_file()
    );
}

#[test]
#[ignore = "Live GitHub check of published v1.3.0 curated catalogue; does not change installed user data"]
fn asset006_live_release_catalogue_identifies_compatible_packs() {
    let f = Fixture::new();
    let catalogue = fetch_catalog(&f.0.join("cache")).unwrap();
    assert_eq!(
        catalogue.packs.len(),
        15,
        "Expected 13 game packs + 2 starter material packs"
    );
    for (id, version) in [
        ("kenney-pirate-kit", "2.2.0"),
        ("kenney-survival-kit", "2.1.0"),
        ("pirate-props", "1.1.0"),
    ] {
        let pack = catalogue.packs.iter().find(|p| p.id == id).unwrap();
        assert_eq!(pack.version, version);
        assert_eq!(pack.tag, "asset-packs-v1.3.0");
        assert!(pack.asset_id > 0);
        assert!(
            pack.entries
                .iter()
                .any(|e| e.target.ends_with("/chest.glb"))
        );
        assert!(pack.name.contains("Compatible"));
        assert_eq!(pack.license, "CC0-1.0");
    }
    assert!(catalogue.packs.iter().any(|p| p.id == "pbr-style"));
    assert!(catalogue.packs.iter().any(|p| p.id == "pbr-4k"));
    println!("LIVE PUBLISHED v1.3.0 CATALOGUE VERIFIED: 15 entries, 3 compatible replacements");
}

#[test]
fn pack_queue_publishes_small_pack_before_large_pack_finishes() {
    let fixture = Fixture::new();
    let (small, small_zip) = pack(&fixture);
    let large_png = fixture.0.join("large.png");
    let mut image = image::RgbaImage::new(128, 128);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        *pixel = image::Rgba([x as u8, y as u8, (x ^ y) as u8, 255]);
    }
    image.save(&large_png).unwrap();
    let (entry_digest, entry_size) = crate::engine::game_package::sha256_file(&large_png).unwrap();
    let large_zip = fixture.0.join("large-pack.zip");
    let mut archive = zip::ZipWriter::new(File::create(&large_zip).unwrap());
    archive
        .start_file("maps/color.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    archive.write_all(&fs::read(&large_png).unwrap()).unwrap();
    archive.finish().unwrap();
    let (digest, archive_size) = crate::engine::game_package::sha256_file(&large_zip).unwrap();
    let mut large = small.clone();
    large.id = "larger-pack".into();
    large.name = "Larger Pack".into();
    large.asset_id = 43;
    large.bytes = archive_size;
    large.sha256 = digest;
    large.entries[0].target = ".assets/textures/large.png".into();
    large.entries[0].bytes = entry_size;
    large.entries[0].sha256 = entry_digest;
    assert!(large.bytes > small.bytes);
    let ordered = order_by_download_size(vec![large.clone(), small.clone()]);
    assert_eq!(
        ordered.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        vec!["basic-textures", "larger-pack"]
    );

    let project = fixture.0.join("Game");
    fs::create_dir(&project).unwrap();
    let root = fixture.0.join("cache");
    let (published_sender, published_receiver) = std::sync::mpsc::channel();
    let (second_sender, second_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let worker_root = root.clone();
        let worker_project = project.clone();
        let worker_small_zip = small_zip.clone();
        let worker_large_zip = large_zip.clone();
        let worker = scope.spawn(move || {
            run_ordered_packs(
                &ordered,
                &AtomicBool::new(false),
                |p| {
                    if p.id == "larger-pack" {
                        second_sender.send(()).unwrap();
                        release_receiver
                            .recv_timeout(Duration::from_secs(5))
                            .unwrap();
                    }
                    let zip = if p.id == "larger-pack" {
                        &worker_large_zip
                    } else {
                        &worker_small_zip
                    };
                    install_archive(&worker_root, p, zip, &AtomicBool::new(false), &|_, _| {})?;
                    verify_installed_cache(&worker_root, &p.reference())?;
                    clone_into_project(
                        &worker_root,
                        &worker_project,
                        &[p.reference()],
                        &AtomicBool::new(false),
                        &|_, _| {},
                    )?;
                    let mut refs = references(&worker_project)?;
                    refs.push(p.reference());
                    bind(&worker_root, &worker_project, refs)
                },
                |p| {
                    published_sender.send(p.id.clone()).unwrap();
                },
            )
            .unwrap();
        });
        assert_eq!(
            published_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap(),
            small.id
        );
        second_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(project.join(".assets/textures/color.png").is_file());
        assert!(!project.join(".assets/textures/large.png").exists());
        assert_eq!(references(&project).unwrap(), vec![small.reference()]);
        release_sender.send(()).unwrap();
        worker.join().unwrap();
    });
    assert_eq!(
        published_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap(),
        large.id
    );
    assert!(project.join(".assets/textures/large.png").is_file());
    assert_eq!(
        references(&project).unwrap(),
        vec![small.reference(), large.reference()]
    );
    clone_into_project(
        &root,
        &project,
        &[small.reference()],
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    assert_eq!(references(&project).unwrap().len(), 2);
}

#[test]
fn queued_packs_stop_on_error_or_cancellation_without_publishing_later_packs() {
    let fixture = Fixture::new();
    let (small, _) = pack(&fixture);
    let mut large = small.clone();
    large.id = "later-pack".into();
    let packs = vec![small, large];
    let published = std::cell::RefCell::new(Vec::new());
    let attempted = std::cell::RefCell::new(Vec::new());
    let cancel = AtomicBool::new(false);
    let error = run_ordered_packs(
        &packs,
        &cancel,
        |p| {
            attempted.borrow_mut().push(p.id.clone());
            if p.id == "later-pack" {
                Err("disk full".into())
            } else {
                Ok(())
            }
        },
        |p| published.borrow_mut().push(p.id.clone()),
    )
    .unwrap_err();
    assert_eq!(error, "disk full");
    assert_eq!(published.borrow().len(), 1);
    assert_eq!(attempted.borrow().len(), 2);

    let attempted = std::cell::RefCell::new(0);
    let published = std::cell::RefCell::new(0);
    let error = run_ordered_packs(
        &packs,
        &cancel,
        |_| {
            *attempted.borrow_mut() += 1;
            Ok(())
        },
        |_| {
            *published.borrow_mut() += 1;
            cancel.store(true, Ordering::Release);
        },
    )
    .unwrap_err();
    assert!(error.contains("canceled"));
    assert_eq!(*attempted.borrow(), 1);
    assert_eq!(*published.borrow(), 1);
}

#[test]
fn previously_cached_pack_is_reverified_before_reuse() {
    let f = Fixture::new();
    let (pack, archive) = pack(&f);
    let root = f.0.join("cache");
    install_archive(&root, &pack, &archive, &AtomicBool::new(false), &|_, _| {}).unwrap();
    verify_installed_cache(&root, &pack.reference()).unwrap();
    let path = location(&root, &pack.reference())
        .unwrap()
        .join(".assets/textures/color.png");
    let mut contents = fs::read(&path).unwrap();
    let index = contents.len() - 1;
    contents[index] ^= 1;
    fs::write(&path, contents).unwrap();
    assert!(installed(&root, &pack.reference()));
    assert!(
        verify_installed_cache(&root, &pack.reference())
            .unwrap_err()
            .contains("checksum")
    );
}
