// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn stars_layers_roundtrip_and_inherit_without_changing_imported_materials() {
    let root = std::env::temp_dir().join(format!("wk-stars-{:032x}", rand::random::<u128>()));
    std::fs::create_dir_all(root.join(".assets/materials")).unwrap();
    let mut authored = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::SurfacePbr {
            base_color: [0.2, 0.4, 0.7, 1.0],
            roughness: 0.5,
            metallic: 0.0,
        },
    });
    authored.layers.push(MaterialLayer {
        name: "Stars".into(),
        texture: "stars.png".into(),
        opacity: 0.35,
        blend: MaterialLayerBlend::Screen,
    });
    material_save(&root, "Skylook.MAT", &authored).unwrap();
    let reopened = read(&root, "Skylook.MAT").unwrap();
    assert_eq!(reopened.layers, authored.layers);
    let instance = MaterialDocument::new(MaterialDefinition::Instance {
        parent: authored.id.clone(),
        overrides: BTreeMap::new(),
    });
    material_save(&root, "Instance.MAT", &instance).unwrap();
    let catalog = MaterialCatalog::load(&root);
    assert_eq!(catalog.layers(&instance.id).unwrap(), authored.layers);
    let mut imported = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::SurfacePbr {
            base_color: [1.0; 4],
            roughness: 0.8,
            metallic: 0.0,
        },
    });
    imported
        .textures
        .insert("BaseColor".into(), "imported.png".into());
    material_save(&root, "Imported.MAT", &imported).unwrap();
    assert!(read(&root, "Imported.MAT").unwrap().layers.is_empty());
    assert_eq!(
        MaterialCatalog::load(&root)
            .texture_maps(&imported.id)
            .unwrap()
            .get("BaseColor")
            .unwrap(),
        "imported.png"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stars_reject_duplicate_names_and_invalid_opacity() {
    let mut doc = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::SurfacePbr {
            base_color: [1.0; 4],
            roughness: 0.5,
            metallic: 0.0,
        },
    });
    let layer = MaterialLayer {
        name: "Dirt".into(),
        texture: "dirt.png".into(),
        opacity: 0.5,
        blend: MaterialLayerBlend::Multiply,
    };
    doc.layers = vec![layer.clone(), layer];
    assert!(doc.validate().is_err());
    doc.layers.pop();
    doc.layers[0].opacity = f32::NAN;
    assert!(doc.validate().is_err());
}

fn material_save(project: &Path, name: &str, doc: &MaterialDocument) -> Result<(), String> {
    save(project, name, doc, None)
}

#[test]
fn stars_sky_and_water_layers_save_reopen_and_leave_defaults_untouched() {
    let root =
        std::env::temp_dir().join(format!("stars-sky-water-{:032x}", rand::random::<u128>()));
    std::fs::create_dir_all(&root).unwrap();
    let layer = MaterialLayer {
        name: "Clouds".into(),
        texture: "clouds.png".into(),
        opacity: 0.4,
        blend: MaterialLayerBlend::Normal,
    };
    let mut sky = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::Sky(crate::project::sky_material::SkyGradient::default()),
    });
    sky.layers.push(layer.clone());
    save(&root, "Sky.MAT", &sky, None).unwrap();
    assert_eq!(read(&root, "Sky.MAT").unwrap().layers, sky.layers);
    let mut water = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::Water(crate::world::terrain::TerrainWaterSettings::default()),
    });
    water.layers.push(MaterialLayer {
        name: "Foam".into(),
        ..layer
    });
    save(&root, "Water.MAT", &water, None).unwrap();
    let reopened = read(&root, "Water.MAT").unwrap();
    assert_eq!(reopened.layers, water.layers);
    let catalog = MaterialCatalog::load(&root);
    assert_eq!(catalog.layers(&sky.id).unwrap(), sky.layers);
    assert_eq!(catalog.layers(&water.id).unwrap(), water.layers);
    assert!(catalog.sky(&sky.id).is_some());
    assert!(catalog.water(&water.id).is_some());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn stars_runtime_alpha_updates_named_layer_without_saving_and_inherits() {
    let root = std::env::temp_dir().join(format!("wk-stars-alpha-{:032x}", rand::random::<u128>()));
    std::fs::create_dir_all(&root).unwrap();
    let mut parent = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::Sky(crate::project::sky_material::SkyGradient::default()),
    });
    parent.layers.push(MaterialLayer {
        name: "Evening".into(),
        texture: "sky.png".into(),
        opacity: 0.0,
        blend: MaterialLayerBlend::Normal,
    });
    save(&root, "Parent.MAT", &parent, None).unwrap();
    let child = MaterialDocument::new(MaterialDefinition::Instance {
        parent: parent.id.clone(),
        overrides: BTreeMap::new(),
    });
    save(&root, "Child.MAT", &child, None).unwrap();
    let original = MaterialCatalog::load(&root);
    let changed = original
        .with_runtime_layer_alpha(&child.id, "Evening", 0.7)
        .unwrap();
    assert_eq!(changed.layers(&child.id).unwrap()[0].opacity, 0.7);
    assert_eq!(changed.layers(&parent.id).unwrap()[0].opacity, 0.7);
    assert_eq!(original.layers(&child.id).unwrap()[0].opacity, 0.0);
    assert_eq!(read(&root, "Parent.MAT").unwrap().layers[0].opacity, 0.0);
    assert!(
        original
            .with_runtime_layer_alpha(&child.id, "Unknown", 0.5)
            .is_err()
    );
    assert!(
        original
            .with_runtime_layer_alpha(&child.id, "Evening", f32::NAN)
            .is_err()
    );
    assert!(
        original
            .with_runtime_layer_alpha(&child.id, "Evening", 1.1)
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}
