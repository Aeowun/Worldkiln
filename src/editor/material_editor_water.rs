// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::SurfaceMaterialEditor;
use crate::project::material::{
    MaterialCatalog, MaterialDefinition, MaterialOutput, Parameter, ParameterValue,
};

pub(super) fn save_water(editor: &mut SurfaceMaterialEditor) -> Result<MaterialCatalog, String> {
    let baseline = MaterialCatalog::load(&editor.project);
    prepare_water(editor, &baseline)?;
    let catalog = editor.draft.save(&editor.project)?;
    apply_saved_water(editor, &catalog)?;
    Ok(catalog)
}

pub(super) fn prepare_water(
    editor: &mut SurfaceMaterialEditor,
    baseline: &MaterialCatalog,
) -> Result<(), String> {
    let next = editor
        .water_look
        .ok_or("Water editor has no Water settings")?;
    next.validate().map_err(|e| e.to_string())?;
    let definition = match &editor.draft.document().definition {
        MaterialDefinition::Asset { .. } => MaterialDefinition::Asset {
            material: MaterialOutput::Water(next),
        },
        MaterialDefinition::Instance { parent, overrides } => {
            let MaterialOutput::Water(parent_look) = baseline.resolve(parent)? else {
                return Err("Parent Material does not resolve to Water".into());
            };
            let previous = editor
                .saved_water_look
                .ok_or("Water saved baseline is missing")?;
            let mut changes = overrides.clone();
            macro_rules! change {
                ($field:ident,$variant:ident,$kind:ident) => {
                    if next.$field != previous.$field {
                        if next.$field == parent_look.$field {
                            changes.remove(&Parameter::$variant);
                        } else {
                            changes.insert(Parameter::$variant, ParameterValue::$kind(next.$field));
                        }
                    }
                };
            }
            change!(wave_height, WaveHeight, Scalar);
            change!(wave_speed, WaveSpeed, Scalar);
            change!(wave_length, WaveLength, Scalar);
            change!(wave_direction_degrees, WaveDirectionDegrees, Scalar);
            change!(wave_steepness, WaveSteepness, Scalar);
            change!(absorption_color, AbsorptionColor, Color3);
            change!(absorption_strength, AbsorptionStrength, Scalar);
            change!(scattering_color, ScatteringColor, Color3);
            change!(scattering_strength, ScatteringStrength, Scalar);
            change!(foam_amount, FoamAmount, Scalar);
            change!(foam_scale, FoamScale, Scalar);
            change!(shallow_color, ShallowColor, Color3);
            change!(deep_color, DeepColor, Color3);
            change!(foam_color, FoamColor, Color3);
            change!(underwater_color, UnderwaterColor, Color3);
            change!(opacity, Opacity, Scalar);
            change!(underwater_opacity, UnderwaterOpacity, Scalar);
            change!(reflection_strength, ReflectionStrength, Scalar);
            change!(specular_strength, SpecularStrength, Scalar);
            change!(
                underwater_effects_enabled,
                UnderwaterEffectsEnabled,
                Boolean
            );
            change!(underwater_fog_color, UnderwaterFogColor, Color3);
            change!(underwater_fog_density, UnderwaterFogDensity, Scalar);
            change!(underwater_depth_fog, UnderwaterDepthFog, Scalar);
            change!(underwater_depth_fog_limit, UnderwaterDepthFogLimit, Scalar);
            change!(underwater_fog_limit, UnderwaterFogLimit, Scalar);
            change!(underwater_sky_tint, UnderwaterSkyTint, Scalar);
            MaterialDefinition::Instance {
                parent: parent.clone(),
                overrides: changes,
            }
        }
    };
    editor.draft.edit(definition)?;
    Ok(())
}

pub(super) fn apply_saved_water(
    editor: &mut SurfaceMaterialEditor,
    catalog: &MaterialCatalog,
) -> Result<(), String> {
    let look = match catalog.resolve(&editor.draft.document().id)? {
        MaterialOutput::Water(look) => look,
        _ => return Err("Saved Material changed output type unexpectedly".into()),
    };
    editor.saved_water_look = Some(look);
    editor.water_look = Some(look);
    editor.texture_maps = catalog.texture_maps(&editor.draft.document().id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::material::{self, MaterialDocument};
    use std::collections::BTreeMap;
    #[test]
    fn water_parent_graph_and_instance_overrides_survive_editor_save() {
        let root =
            std::env::temp_dir().join(format!("wkmat012-water-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let parent = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(Default::default()),
        });
        material::save(&root, "parent.MAT", &parent, None).unwrap();
        let child = MaterialDocument::new(MaterialDefinition::Instance {
            parent: parent.id.clone(),
            overrides: BTreeMap::new(),
        });
        material::save(&root, "child.MAT", &child, None).unwrap();
        let mut editor = SurfaceMaterialEditor::open(&root, "child.MAT").unwrap();
        assert!(editor.water_look.is_some());
        editor.water_look.as_mut().unwrap().opacity = 0.43;
        let catalog = editor.save().unwrap();
        assert_eq!(catalog.water(&child.id).unwrap().opacity, 0.43);
        let saved = material::read(&root, "child.MAT").unwrap();
        let MaterialDefinition::Instance {
            parent: actual,
            overrides,
        } = saved.definition
        else {
            panic!("flattened instance")
        };
        assert_eq!(actual, parent.id);
        assert_eq!(
            overrides.get(&Parameter::Opacity),
            Some(&ParameterValue::Scalar(0.43))
        );
        assert_eq!(material::read(&root, "parent.MAT").unwrap(), parent);
        std::fs::remove_dir_all(root).unwrap();
    }
}
