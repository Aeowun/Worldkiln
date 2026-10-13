use glam::Vec3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderDistanceSettings {
    // Zero preserves the legacy Unlimited horizon. These limit drawing only.
    pub editor_chunks: i32,
    pub play_chunks: i32,
}

impl RenderDistanceSettings {
    pub fn radius(self, playing: bool) -> i32 {
        if playing {
            self.play_chunks
        } else {
            self.editor_chunks
        }
    }
}

#[derive(Clone)]
pub struct LightingSettings {
    pub shadows_enabled: bool,
    pub global_light_enabled: bool,
    /// The direction light travels through the scene.
    pub global_light_direction: Vec3,
    pub global_light_color: Vec3,
    pub global_light_intensity: f32,
    pub ambient_intensity: f32,
}

impl Default for LightingSettings {
    fn default() -> Self {
        Self {
            shadows_enabled: true,
            global_light_enabled: true,
            // Default downward diagonal.
            global_light_direction: Vec3::new(0.5, -1.0, 0.5).normalize(),
            global_light_color: Vec3::ONE,
            global_light_intensity: 1.0,
            ambient_intensity: 0.20,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SkySettings {
    pub enabled: bool,
    pub preset: String,
    pub texture: String,
    // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
    pub atmosphere_enabled: bool,
    pub fog_start_studs: f32,
    pub fog_end_studs: f32,
    pub fog_color: Vec3,
    pub horizon_strength: f32,
}

impl Default for SkySettings {
    fn default() -> Self {
        let mut s = Self {
            enabled: true,
            preset: "Temperate".to_string(),
            texture: String::new(),
            atmosphere_enabled: true,
            fog_start_studs: 96.0,
            fog_end_studs: 320.0,
            fog_color: Vec3::new(0.70, 0.78, 0.84),
            horizon_strength: 0.38,
        };
        s.update_preset_textures();
        s
    }
}

impl SkySettings {
    pub fn update_preset_textures(&mut self) {
        self.texture = match self.preset.as_str() {
            "Tropical" => "Cubemap_Tropical_01-512x512.png",
            "Desert" => "Cubemap_Desert_01-512x512.png",
            "Snowy" => "Cubemap_Snowy_01-512x512.png",
            "Mars" => "Cubemap_Mars_01-512x512.png",
            "Night" => "Cubemap_Island_02-512x512.png",
            _ => "Cubemap_Temperate_01-512x512.png",
        }
        .to_string();
    }
}
//Worldkiln\.assets\skybox\
