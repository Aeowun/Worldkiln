use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub fn editor_preferences_path() -> PathBuf {
    // Settings use the same installed/home profile as projects, never release staging.
    let dir = crate::project::project::default_user_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("editor_settings.json")
}

// Worldkiln 0.8.5 | WO MAT-032 | 2026-10-10 16:23 -05:00
// One corrupt or unsupported preference must not silently discard unrelated
// valid fields. Validate each persisted field against the actual settings
// schema and retain that field's default only when its value is malformed.
fn decode_editor_preferences(bytes: &[u8]) -> ThemeSettings {
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_slice(bytes) else {
        return ThemeSettings::default();
    };
    let mut valid = serde_json::to_value(ThemeSettings::default())
        .expect("default ThemeSettings must serialize");
    for (key, value) in fields {
        if !valid
            .as_object()
            .is_some_and(|fields| fields.contains_key(&key))
        {
            continue;
        }
        let mut candidate = valid.clone();
        candidate
            .as_object_mut()
            .unwrap()
            .insert(key.clone(), value.clone());
        if serde_json::from_value::<ThemeSettings>(candidate).is_ok() {
            valid.as_object_mut().unwrap().insert(key, value);
        }
    }
    serde_json::from_value(valid).expect("validated settings must deserialize")
}

pub fn load_editor_preferences() -> ThemeSettings {
    let path = editor_preferences_path();
    match std::fs::read(&path) {
        Ok(bytes) => decode_editor_preferences(&bytes),
        Err(_) => ThemeSettings::default(),
    }
}

// Worldkiln 0.8.5 | WO MAT-032 | 2026-10-10 16:23 -05:00
// Never truncate the active editor_settings.json before the replacement
// has been fully serialized and synced. Reuse the audited atomic writer.
fn save_editor_preferences_to_path(
    path: &std::path::Path,
    settings: &ThemeSettings,
) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(settings)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    crate::project::atomic_file::write_atomically(path, &bytes)
}

pub fn save_editor_preferences(settings: &ThemeSettings) {
    let path = editor_preferences_path();
    if let Err(err) = save_editor_preferences_to_path(&path, settings) {
        crate::engine::diagnostics::report(
            "ERROR",
            "Preferences",
            format!(
                "Could not save editor preferences to {}: {err}",
                path.display()
            ),
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum EditorTheme {
    #[default]
    AeoDark,
    Light,
    Custom,
    Graphite,
    Midnight,
    Forest,
    Sandstone,
}

impl EditorTheme {
    // Keep `Custom` deserializable for existing settings files, but do not
    // advertise it until Worldkiln exposes real custom-theme controls.
    pub const ALL: &'static [Self] = &[
        Self::AeoDark,
        Self::Graphite,
        Self::Midnight,
        Self::Forest,
        Self::Sandstone,
        Self::Light,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AeoDark => "Aeo Dark",
            Self::Light => "Light",
            Self::Custom => "Custom",
            Self::Graphite => "Graphite",
            Self::Midnight => "Midnight Blue",
            Self::Forest => "Forest",
            Self::Sandstone => "Sandstone",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AccentTheme {
    #[default]
    Orange,
    Red,
    Cool,
    Cyan,
    AeoEmerald,
    Custom,
}

impl AccentTheme {
    // Keep `Custom` deserializable for existing settings files, but do not
    // advertise it until a real custom accent picker exists.
    pub const ALL: &'static [Self] = &[
        Self::Orange,
        Self::Red,
        Self::Cool,
        Self::Cyan,
        Self::AeoEmerald,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Orange => "Orange",
            Self::Red => "Red",
            Self::Cool => "Cool",
            Self::Cyan => "Cyan",
            Self::AeoEmerald => "Aeo Emerald",
            Self::Custom => "Custom",
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Orange | Self::Custom => egui::Color32::from_rgb(228, 91, 36),
            Self::Red => egui::Color32::from_rgb(202, 60, 50),
            Self::Cool => egui::Color32::from_rgb(96, 115, 125),
            Self::Cyan => egui::Color32::from_rgb(0, 235, 255),
            Self::AeoEmerald => egui::Color32::from_rgb(57, 255, 20),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum PanelRounding {
    #[default]
    Square,
    Slight,
    Rounded,
}

impl PanelRounding {
    pub const ALL: &'static [Self] = &[Self::Square, Self::Slight, Self::Rounded];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Square => "Square",
            Self::Slight => "Slight",
            Self::Rounded => "Rounded",
        }
    }

    pub const fn radius(self) -> f32 {
        match self {
            Self::Square => 0.0,
            Self::Slight => 4.0,
            Self::Rounded => 8.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum PanelContrast {
    Low,
    #[default]
    Normal,
    High,
}

impl PanelContrast {
    pub const ALL: &'static [Self] = &[Self::Low, Self::Normal, Self::High];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum SeparatorVisibility {
    Low,
    #[default]
    Normal,
    High,
}

impl SeparatorVisibility {
    pub const ALL: &'static [Self] = &[Self::Low, Self::Normal, Self::High];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum InterfaceScale {
    Percent75,
    #[default]
    Percent100,
    Percent125,
    Percent150,
}

impl InterfaceScale {
    pub const ALL: &'static [Self] = &[
        Self::Percent75,
        Self::Percent100,
        Self::Percent125,
        Self::Percent150,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Percent75 => "75%",
            Self::Percent100 => "100%",
            Self::Percent125 => "125%",
            Self::Percent150 => "150%",
        }
    }

    pub const fn factor(self) -> f32 {
        match self {
            Self::Percent75 => 0.75,
            Self::Percent100 => 1.00,
            Self::Percent125 => 1.25,
            Self::Percent150 => 1.50,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum ThemeFontSize {
    Small,
    #[default]
    Normal,
    Large,
}

impl ThemeFontSize {
    pub const ALL: &'static [Self] = &[Self::Small, Self::Normal, Self::Large];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Normal => "Normal",
            Self::Large => "Large",
        }
    }

    pub const fn scale(self) -> f32 {
        match self {
            Self::Small => 0.90,
            Self::Normal => 1.00,
            Self::Large => 1.15,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum UiAnimation {
    Off,
    Reduced,
    #[default]
    Full,
}

impl UiAnimation {
    pub const ALL: &'static [Self] = &[Self::Off, Self::Reduced, Self::Full];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Reduced => "Reduced",
            Self::Full => "Full",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CameraSpeedPreset {
    Slow,
    #[default]
    Normal,
    Fast,
    Custom,
}

impl CameraSpeedPreset {
    pub const ALL: &'static [Self] = &[Self::Slow, Self::Normal, Self::Fast, Self::Custom];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Slow => "Slow",
            Self::Normal => "Normal",
            Self::Fast => "Fast",
            Self::Custom => "Custom",
        }
    }

    pub const fn default_factor(self) -> f32 {
        match self {
            Self::Slow => 0.30,
            Self::Normal => 0.60,
            Self::Fast => 1.20,
            Self::Custom => 0.60,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum CameraSensitivityPreset {
    Low,
    #[default]
    Normal,
    High,
    Custom,
}

impl CameraSensitivityPreset {
    pub const ALL: &'static [Self] = &[Self::Low, Self::Normal, Self::High, Self::Custom];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Normal => "Normal",
            Self::High => "High",
            Self::Custom => "Custom",
        }
    }

    pub const fn default_factor(self) -> f32 {
        match self {
            Self::Low => 0.50,
            Self::Normal => 1.00,
            Self::High => 1.50,
            Self::Custom => 1.00,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AutosaveInterval {
    #[default]
    Off,
    OneMinute,
    FiveMinutes,
    TenMinutes,
    ThirtyMinutes,
}

impl AutosaveInterval {
    pub const ALL: &'static [Self] = &[
        Self::Off,
        Self::OneMinute,
        Self::FiveMinutes,
        Self::TenMinutes,
        Self::ThirtyMinutes,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::OneMinute => "1 min",
            Self::FiveMinutes => "5 min",
            Self::TenMinutes => "10 min",
            Self::ThirtyMinutes => "30 min",
        }
    }

    pub const fn seconds(self) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::OneMinute => Some(60),
            Self::FiveMinutes => Some(300),
            Self::TenMinutes => Some(600),
            Self::ThirtyMinutes => Some(1800),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum EditorKeyBinding {
    #[default]
    W,
    A,
    S,
    D,
    Q,
    E,
    Space,
    Shift,
    Ctrl,
    Delete,
    Up,
    Down,
    Left,
    Right,
}

impl EditorKeyBinding {
    pub const ALL: &'static [Self] = &[
        Self::W,
        Self::A,
        Self::S,
        Self::D,
        Self::Q,
        Self::E,
        Self::Space,
        Self::Shift,
        Self::Ctrl,
        Self::Delete,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::W => "W",
            Self::A => "A",
            Self::S => "S",
            Self::D => "D",
            Self::Q => "Q",
            Self::E => "E",
            Self::Space => "Space",
            Self::Shift => "Shift",
            Self::Ctrl => "Ctrl",
            Self::Delete => "Delete",
            Self::Up => "Up",
            Self::Down => "Down",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }

    pub fn matches_key(self, key: winit::keyboard::KeyCode) -> bool {
        use winit::keyboard::KeyCode;
        match self {
            Self::W => key == KeyCode::KeyW,
            Self::A => key == KeyCode::KeyA,
            Self::S => key == KeyCode::KeyS,
            Self::D => key == KeyCode::KeyD,
            Self::Q => key == KeyCode::KeyQ,
            Self::E => key == KeyCode::KeyE,
            Self::Space => key == KeyCode::Space,
            Self::Shift => matches!(key, KeyCode::ShiftLeft | KeyCode::ShiftRight),
            Self::Ctrl => matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight),
            Self::Delete => key == KeyCode::Delete,
            Self::Up => key == KeyCode::ArrowUp,
            Self::Down => key == KeyCode::ArrowDown,
            Self::Left => key == KeyCode::ArrowLeft,
            Self::Right => key == KeyCode::ArrowRight,
        }
    }

    pub fn is_down(self, keys_down: &std::collections::HashSet<winit::keyboard::KeyCode>) -> bool {
        use winit::keyboard::KeyCode;
        match self {
            Self::W => keys_down.contains(&KeyCode::KeyW),
            Self::A => keys_down.contains(&KeyCode::KeyA),
            Self::S => keys_down.contains(&KeyCode::KeyS),
            Self::D => keys_down.contains(&KeyCode::KeyD),
            Self::Q => keys_down.contains(&KeyCode::KeyQ),
            Self::E => keys_down.contains(&KeyCode::KeyE),
            Self::Space => keys_down.contains(&KeyCode::Space),
            Self::Shift => {
                keys_down.contains(&KeyCode::ShiftLeft) || keys_down.contains(&KeyCode::ShiftRight)
            }
            Self::Ctrl => {
                keys_down.contains(&KeyCode::ControlLeft)
                    || keys_down.contains(&KeyCode::ControlRight)
            }
            Self::Delete => keys_down.contains(&KeyCode::Delete),
            Self::Up => keys_down.contains(&KeyCode::ArrowUp),
            Self::Down => keys_down.contains(&KeyCode::ArrowDown),
            Self::Left => keys_down.contains(&KeyCode::ArrowLeft),
            Self::Right => keys_down.contains(&KeyCode::ArrowRight),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum EditorMouseBinding {
    Left,
    #[default]
    Right,
    Middle,
    Mouse4,
    Mouse5,
}

impl EditorMouseBinding {
    pub const ALL: &'static [Self] = &[
        Self::Left,
        Self::Right,
        Self::Middle,
        Self::Mouse4,
        Self::Mouse5,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "Left Mouse",
            Self::Right => "Right Mouse",
            Self::Middle => "Middle Mouse",
            Self::Mouse4 => "Mouse 4",
            Self::Mouse5 => "Mouse 5",
        }
    }

    pub fn matches_button(self, button: winit::event::MouseButton) -> bool {
        use winit::event::MouseButton;
        match self {
            Self::Left => button == MouseButton::Left,
            Self::Right => button == MouseButton::Right,
            Self::Middle => button == MouseButton::Middle,
            Self::Mouse4 => matches!(button, MouseButton::Back | MouseButton::Other(4)),
            Self::Mouse5 => matches!(button, MouseButton::Forward | MouseButton::Other(5)),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSettings {
    pub theme: EditorTheme,
    pub accent: AccentTheme,
    pub panel_rounding: PanelRounding,
    pub panel_contrast: PanelContrast,
    pub separator_visibility: SeparatorVisibility,

    pub interface_scale: InterfaceScale,
    pub font_size: ThemeFontSize,
    pub monospace_size: ThemeFontSize,
    pub animation: UiAnimation,
    pub tooltips: bool,
    pub status_bar: bool,
    pub first_run_controls_dismissed: bool,

    pub camera_speed_preset: CameraSpeedPreset,
    pub camera_speed_custom: f32,
    pub camera_sensitivity_preset: CameraSensitivityPreset,
    pub camera_sensitivity_custom: f32,

    pub undo_history_limit: usize,
    pub autosave: AutosaveInterval,
    pub confirm_destructive_actions: bool,
    pub show_navmesh: bool,

    pub bind_camera_forward: EditorKeyBinding,
    pub bind_camera_backward: EditorKeyBinding,
    pub bind_camera_left: EditorKeyBinding,
    pub bind_camera_right: EditorKeyBinding,
    pub bind_camera_up: EditorKeyBinding,
    pub bind_camera_down: EditorKeyBinding,
    pub bind_orbit_camera: EditorMouseBinding,
    pub bind_free_look: EditorMouseBinding,

    pub bind_select: EditorMouseBinding,
    pub bind_delete: EditorKeyBinding,
}

impl ThemeSettings {
    pub fn camera_speed(&self) -> f32 {
        match self.camera_speed_preset {
            CameraSpeedPreset::Slow => 0.30,
            CameraSpeedPreset::Normal => 0.60,
            CameraSpeedPreset::Fast => 1.20,
            CameraSpeedPreset::Custom => self.camera_speed_custom.clamp(0.05, 5.0),
        }
    }

    pub fn camera_sensitivity(&self) -> f32 {
        match self.camera_sensitivity_preset {
            CameraSensitivityPreset::Low => 0.50,
            CameraSensitivityPreset::Normal => 1.00,
            CameraSensitivityPreset::High => 1.50,
            CameraSensitivityPreset::Custom => self.camera_sensitivity_custom.clamp(0.1, 5.0),
        }
    }

    pub fn reset_input_bindings(&mut self) {
        self.bind_camera_forward = EditorKeyBinding::W;
        self.bind_camera_backward = EditorKeyBinding::S;
        self.bind_camera_left = EditorKeyBinding::A;
        self.bind_camera_right = EditorKeyBinding::D;
        self.bind_camera_up = EditorKeyBinding::E;
        self.bind_camera_down = EditorKeyBinding::Q;
        self.bind_orbit_camera = EditorMouseBinding::Middle;
        self.bind_free_look = EditorMouseBinding::Right;
        self.bind_select = EditorMouseBinding::Left;
        self.bind_delete = EditorKeyBinding::Delete;
    }
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            theme: EditorTheme::AeoDark,
            accent: AccentTheme::Orange,
            panel_rounding: PanelRounding::Rounded,
            panel_contrast: PanelContrast::Normal,
            separator_visibility: SeparatorVisibility::Normal,
            interface_scale: InterfaceScale::Percent100,
            font_size: ThemeFontSize::Normal,
            monospace_size: ThemeFontSize::Normal,
            animation: UiAnimation::Full,
            tooltips: true,
            status_bar: true,
            first_run_controls_dismissed: false,

            camera_speed_preset: CameraSpeedPreset::Normal,
            camera_speed_custom: 0.60,
            camera_sensitivity_preset: CameraSensitivityPreset::Normal,
            camera_sensitivity_custom: 1.00,

            undo_history_limit: 100,
            autosave: AutosaveInterval::Off,
            confirm_destructive_actions: true,
            show_navmesh: true,

            bind_camera_forward: EditorKeyBinding::W,
            bind_camera_backward: EditorKeyBinding::S,
            bind_camera_left: EditorKeyBinding::A,
            bind_camera_right: EditorKeyBinding::D,
            bind_camera_up: EditorKeyBinding::E,
            bind_camera_down: EditorKeyBinding::Q,
            bind_orbit_camera: EditorMouseBinding::Middle,
            bind_free_look: EditorMouseBinding::Right,

            bind_select: EditorMouseBinding::Left,
            bind_delete: EditorKeyBinding::Delete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Worldkiln 0.8.5 | WO MAT-032 | 2026-10-10 16:23 -05:00
    #[test]
    fn corrupted_preference_field_preserves_other_valid_values() {
        let settings = decode_editor_preferences(
            br#"{"theme":"Light","accent":"Cyan","autosave":"NotARealInterval","status_bar":false,"interface_scale":"Percent150"}"#,
        );
        assert_eq!(settings.theme, EditorTheme::Light);
        assert_eq!(settings.accent, AccentTheme::Cyan);
        assert_eq!(settings.autosave, AutosaveInterval::Off);
        assert!(!settings.status_bar);
        assert_eq!(settings.interface_scale, InterfaceScale::Percent150);
        assert_eq!(
            decode_editor_preferences(b"{ invalid json").accent,
            AccentTheme::Orange,
        );
    }

    #[test]
    fn editor_preferences_atomic_save_keeps_old_data_on_failed_replace() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "worldkiln_mat032_preferences_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("editor_settings.json");
        let mut settings = ThemeSettings::default();
        settings.accent = AccentTheme::Cyan;
        save_editor_preferences_to_path(&path, &settings).unwrap();
        assert_eq!(
            decode_editor_preferences(&std::fs::read(&path).unwrap()).accent,
            AccentTheme::Cyan
        );
        settings.accent = AccentTheme::AeoEmerald;
        save_editor_preferences_to_path(&path, &settings).unwrap();
        assert_eq!(
            decode_editor_preferences(&std::fs::read(&path).unwrap()).accent,
            AccentTheme::AeoEmerald
        );
        // An invalid replacement destination must not delete files below it.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        let sentinel = path.join("preserve-this.txt");
        std::fs::write(&sentinel, b"unchanged").unwrap();
        assert!(save_editor_preferences_to_path(&path, &settings).is_err());
        assert_eq!(std::fs::read(sentinel).unwrap(), b"unchanged");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_autosave_interval_seconds_and_labels() {
        assert_eq!(AutosaveInterval::Off.seconds(), None);
        assert_eq!(AutosaveInterval::OneMinute.seconds(), Some(60));
        assert_eq!(AutosaveInterval::FiveMinutes.seconds(), Some(300));
        assert_eq!(AutosaveInterval::TenMinutes.seconds(), Some(600));
        assert_eq!(AutosaveInterval::ThirtyMinutes.seconds(), Some(1800));

        assert_eq!(AutosaveInterval::Off.label(), "Off");
        assert_eq!(AutosaveInterval::OneMinute.label(), "1 min");
    }

    // Worldkiln 0.8.5 | WO 0.8.7-75 | 2026-10-09
    #[test]
    fn accent_cyan_and_aeo_emerald_are_selectable_and_persisted() {
        for (accent, label, rgb) in [
            (
                AccentTheme::Cyan,
                "Cyan",
                egui::Color32::from_rgb(0, 235, 255),
            ),
            (
                AccentTheme::AeoEmerald,
                "Aeo Emerald",
                egui::Color32::from_rgb(57, 255, 20),
            ),
        ] {
            assert!(AccentTheme::ALL.contains(&accent));
            assert_eq!(accent.label(), label);
            assert_eq!(accent.color(), rgb);
            let json = serde_json::to_string(&accent).unwrap();
            assert_eq!(serde_json::from_str::<AccentTheme>(&json).unwrap(), accent);
        }
    }

    #[test]
    fn test_theme_settings_defaults() {
        let defaults = ThemeSettings::default();
        assert_eq!(defaults.autosave, AutosaveInterval::Off);
        assert!(defaults.confirm_destructive_actions);
        assert!(defaults.show_navmesh);
        assert_eq!(defaults.undo_history_limit, 100);
        assert!(!defaults.first_run_controls_dismissed);
    }

    #[test]
    fn test_deserialization_from_empty_json() {
        let json = "{}";
        let settings: ThemeSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings.autosave, AutosaveInterval::Off);
        assert!(settings.confirm_destructive_actions);
        assert!(settings.show_navmesh);
        assert_eq!(settings.undo_history_limit, 100);
        assert_eq!(settings.bind_camera_forward, EditorKeyBinding::W);
        assert_eq!(settings.bind_free_look, EditorMouseBinding::Right);
        assert!(!settings.first_run_controls_dismissed);
    }

    #[test]
    fn test_editor_key_and_mouse_binding_defaults() {
        let defaults = ThemeSettings::default();
        assert_eq!(defaults.bind_camera_forward, EditorKeyBinding::W);
        assert_eq!(defaults.bind_camera_backward, EditorKeyBinding::S);
        assert_eq!(defaults.bind_orbit_camera, EditorMouseBinding::Middle);
        assert_eq!(defaults.bind_free_look, EditorMouseBinding::Right);
    }

    #[test]
    fn test_editor_key_binding_matches_key() {
        use winit::keyboard::KeyCode;
        assert!(EditorKeyBinding::W.matches_key(KeyCode::KeyW));
        assert!(EditorKeyBinding::Delete.matches_key(KeyCode::Delete));
        assert!(EditorKeyBinding::Shift.matches_key(KeyCode::ShiftLeft));
        assert!(EditorKeyBinding::Shift.matches_key(KeyCode::ShiftRight));
    }
}
