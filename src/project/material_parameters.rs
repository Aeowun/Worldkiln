// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::MaterialOutput;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterValue {
    Scalar(f32),
    Vec2([f32; 2]),
    Boolean(bool),
    Color3([f32; 3]),
    Color4([f32; 4]),
}

macro_rules! water_parameters {
    ($($variant:ident => $field:ident : $kind:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all="snake_case")]
        pub enum Parameter { $($variant,)* BaseColor, Roughness, Metallic }
        impl Parameter {
            pub fn apply(self, output: &mut MaterialOutput, value: &ParameterValue) -> Result<(), String> {
                match (output,self,value) {
                    $((MaterialOutput::Water(settings),Self::$variant,ParameterValue::$kind(value)) => settings.$field = *value,)*
                    (MaterialOutput::SurfacePbr {base_color,..},Self::BaseColor,ParameterValue::Color4(value)) => *base_color = *value,
                    (MaterialOutput::SurfacePbr {roughness,..},Self::Roughness,ParameterValue::Scalar(value)) => *roughness = *value,
                    (MaterialOutput::SurfacePbr {metallic,..},Self::Metallic,ParameterValue::Scalar(value)) => *metallic = *value,
                    _ => return Err(format!("Material parameter {self:?} has incompatible output or value type")),
                }
                Ok(())
            }
        }
    }
}
water_parameters! {
    WaveHeight => wave_height:Scalar, WaveSpeed => wave_speed:Scalar,
    WaveLength => wave_length:Scalar, WaveDirectionDegrees => wave_direction_degrees:Scalar,
    WaveSteepness => wave_steepness:Scalar,
    AbsorptionColor => absorption_color:Color3, AbsorptionStrength => absorption_strength:Scalar,
    ScatteringColor => scattering_color:Color3, ScatteringStrength => scattering_strength:Scalar,
    FoamAmount => foam_amount:Scalar, FoamScale => foam_scale:Scalar,
    ShallowColor => shallow_color:Color3, DeepColor => deep_color:Color3,
    FoamColor => foam_color:Color3, UnderwaterColor => underwater_color:Color3,
    Opacity => opacity:Scalar, UnderwaterOpacity => underwater_opacity:Scalar,
    ReflectionStrength => reflection_strength:Scalar, SpecularStrength => specular_strength:Scalar,
    UnderwaterEffectsEnabled => underwater_effects_enabled:Boolean,
    UnderwaterFogColor => underwater_fog_color:Color3, UnderwaterFogDensity => underwater_fog_density:Scalar,
    UnderwaterDepthFog => underwater_depth_fog:Scalar, UnderwaterDepthFogLimit => underwater_depth_fog_limit:Scalar,
    UnderwaterFogLimit => underwater_fog_limit:Scalar, UnderwaterSkyTint => underwater_sky_tint:Scalar,
}
