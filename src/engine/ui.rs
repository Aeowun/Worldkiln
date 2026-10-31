// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UiAnchor {
    #[default]
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
    Stretch,
}
impl UiAnchor {
    pub const ALL: [Self; 10] = [
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::CenterLeft,
        Self::Center,
        Self::CenterRight,
        Self::BottomLeft,
        Self::BottomCenter,
        Self::BottomRight,
        Self::Stretch,
    ];
    pub fn resolve(
        self,
        parent_size: [f32; 2],
        offset: [f32; 2],
        size: [f32; 2],
    ) -> ([f32; 2], [f32; 2]) {
        if self == Self::Stretch {
            return (
                offset,
                [
                    (parent_size[0] - offset[0] - size[0]).max(0.0),
                    (parent_size[1] - offset[1] - size[1]).max(0.0),
                ],
            );
        }
        let pivot = match self {
            Self::TopLeft => [0.0, 0.0],
            Self::TopCenter => [0.5, 0.0],
            Self::TopRight => [1.0, 0.0],
            Self::CenterLeft => [0.0, 0.5],
            Self::Center => [0.5, 0.5],
            Self::CenterRight => [1.0, 0.5],
            Self::BottomLeft => [0.0, 1.0],
            Self::BottomCenter => [0.5, 1.0],
            Self::BottomRight => [1.0, 1.0],
            Self::Stretch => unreachable!(),
        };
        (
            [
                offset[0] + (parent_size[0] - size[0]) * pivot[0],
                offset[1] + (parent_size[1] - size[1]) * pivot[1],
            ],
            size,
        )
    }
}

use crate::scripting::value::{HandleKind, Value};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
include!("ui_order_impl.rs");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UiZIndexBehavior {
    #[default]
    Legacy,
    Global,
    Sibling,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiCommon {
    pub key: String,
    pub anchor: UiAnchor,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub position_scale: [f32; 2],
    pub size_scale: [f32; 2],
    pub visible: bool,
    pub enabled: bool,
    pub color: [f32; 4],
    pub background_color: [f32; 4],

    pub parent: Option<u32>,
    pub z_index: f32,

    pub corner_radius: f32,
    pub border_width: f32,
    pub border_color: [f32; 4],
}

impl Default for UiCommon {
    fn default() -> Self {
        Self {
            key: String::new(),
            anchor: UiAnchor::TopLeft,
            position: [0.0, 0.0],
            size: [100.0, 50.0],
            position_scale: [0.0, 0.0],
            size_scale: [0.0, 0.0],
            visible: true,
            enabled: true,
            color: [1.0, 1.0, 1.0, 1.0],
            background_color: [8.0 / 255.0, 10.0 / 255.0, 13.0 / 255.0, 225.0 / 255.0],
            parent: None,
            z_index: 0.0,
            corner_radius: 4.0,
            border_width: 1.0,
            border_color: [1.0, 1.0, 1.0, 0.09],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UiHorizontalAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl UiHorizontalAlign {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UiVerticalAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

impl UiVerticalAlign {
    pub const ALL: [Self; 3] = [Self::Top, Self::Center, Self::Bottom];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UiFontFamily {
    #[default]
    Proportional,
    Monospace,
}
impl UiFontFamily {
    pub const ALL: [Self; 2] = [Self::Proportional, Self::Monospace];
    fn font_id(self, size: f32) -> egui::FontId {
        match self {
            Self::Proportional => egui::FontId::proportional(size),
            Self::Monospace => egui::FontId::monospace(size),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiText {
    pub common: UiCommon,
    pub text: String,
    pub font_size: f32,
    pub text_scaled: bool,
    pub horizontal_align: UiHorizontalAlign,
    pub vertical_align: UiVerticalAlign,
    pub wrap: bool,
    pub padding: [f32; 4],
    pub line_spacing: f32,
    pub font_family: UiFontFamily,
    pub bold: bool,
    pub italic: bool,
    pub rich_text: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSymbolicEventBinding {
    pub module_key: String,
    pub handler: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiSymbolicEventDispatch {
    pub document_key: String,
    pub element_key: String,
    pub element_type: String,
    pub event: String,
    pub module_key: String,
    pub handler: String,
}

#[derive(Clone, Debug)]
pub struct UiButton {
    pub common: UiCommon,
    pub text: String,
    pub on_click: Option<Value>,
    pub font_size: f32,
    pub horizontal_align: UiHorizontalAlign,
    pub vertical_align: UiVerticalAlign,
    pub wrap: bool,
    pub padding: [f32; 4],
    pub line_spacing: f32,
    pub font_family: UiFontFamily,
    pub bold: bool,
    pub italic: bool,
    pub rich_text: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UiMediaFit {
    #[default]
    Contain,
    Cover,
    Stretch,
}
impl UiMediaFit {
    pub const ALL: [Self; 3] = [Self::Contain, Self::Cover, Self::Stretch];
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiMedia {
    pub common: UiCommon,
    pub source: String,
    pub fit: UiMediaFit,
    pub playing: bool,
    pub looped: bool,
    pub playback_rate: f32,
}

impl UiMedia {
    fn new(common: UiCommon) -> Self {
        Self {
            common,
            source: String::new(),
            fit: UiMediaFit::Contain,
            playing: false,
            looped: false,
            playback_rate: 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub enum UiElement {
    Panel(UiCommon),
    Text(UiText),
    Button(UiButton),
    Image(UiMedia),
    Video(UiMedia),
}

impl UiElement {
    pub fn common(&self) -> &UiCommon {
        match self {
            Self::Panel(c) => c,
            Self::Text(t) => &t.common,
            Self::Button(b) => &b.common,
            Self::Image(i) | Self::Video(i) => &i.common,
        }
    }

    pub fn common_mut(&mut self) -> &mut UiCommon {
        match self {
            Self::Panel(c) => c,
            Self::Text(t) => &mut t.common,
            Self::Button(b) => &mut b.common,
            Self::Image(i) | Self::Video(i) => &mut i.common,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiResolvedLayout {
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub visible: bool,
    pub enabled: bool,
    pub z_index: f32,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeUi {
    pub document_key: String,
    pub z_index_behavior: UiZIndexBehavior,
    pub display_order: i32,
    next_id: u32,
    pub elements: HashMap<u32, UiElement>,
    pub pending_clicks: Vec<u32>,
    symbolic_events: HashMap<(u32, String), UiSymbolicEventBinding>,
}

impl RuntimeUi {
    pub fn new() -> Self {
        Self {
            document_key: String::new(),
            z_index_behavior: UiZIndexBehavior::Legacy,
            display_order: 0,
            next_id: 1,
            elements: HashMap::new(),
            pending_clicks: Vec::new(),
            symbolic_events: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.document_key.clear();
        self.z_index_behavior = UiZIndexBehavior::Legacy;
        self.display_order = 0;
        self.next_id = 1;
        self.elements.clear();
        self.pending_clicks.clear();
        self.symbolic_events.clear();
    }

    pub fn find(&self, document: Option<&str>, key: &str) -> Option<u64> {
        if document.is_some_and(|document| document != self.document_key) {
            return None;
        }
        if key.is_empty() {
            return None;
        }
        self.elements
            .iter()
            .find(|(_, element)| element.common().key == key)
            .map(|(id, _)| *id as u64)
    }

    pub fn allocate(&mut self, element_type: &str) -> Result<u64, String> {
        let mut common = UiCommon::default();

        let element = match element_type {
            "Panel" => {
                common.color = [0.12, 0.12, 0.15, 0.85];
                common.border_color = [1.0, 1.0, 1.0, 0.10];

                UiElement::Panel(common)
            }

            "Text" => UiElement::Text(UiText {
                common,
                text: String::new(),
                font_size: 14.0,
                text_scaled: false,
                horizontal_align: UiHorizontalAlign::Left,
                vertical_align: UiVerticalAlign::Top,
                wrap: true,
                padding: [0.0, 0.0, 0.0, 0.0],
                line_spacing: 0.0,
                font_family: UiFontFamily::default(),
                bold: false,
                italic: false,
                rich_text: false,
            }),

            "Button" => UiElement::Button(UiButton {
                common,
                text: String::new(),
                on_click: None,
                font_size: 14.0,
                horizontal_align: UiHorizontalAlign::Center,
                vertical_align: UiVerticalAlign::Center,
                wrap: true,
                padding: [0.0, 0.0, 0.0, 0.0],
                line_spacing: 0.0,
                font_family: UiFontFamily::default(),
                bold: false,
                italic: false,
                rich_text: false,
            }),

            "Image" => UiElement::Image(UiMedia::new(common)),
            "Video" => UiElement::Video(UiMedia::new(common)),
            unknown => return Err(format!("unknown UI element type '{}'", unknown)),
        };

        let id = self.next_id;
        self.next_id += 1;

        self.elements.insert(id, element);

        Ok(id as u64)
    }

    pub fn delete(&mut self, id: u64) -> Result<(), String> {
        let key = id as u32;

        if self.elements.remove(&key).is_none() {
            return Err(format!("invalid UI handle {}", id));
        }

        self.pending_clicks.retain(|&click_id| click_id != key);
        self.symbolic_events
            .retain(|(element_id, _), _| *element_id != key);

        for element in self.elements.values_mut() {
            if element.common().parent == Some(key) {
                element.common_mut().parent = None;
            }
        }

        Ok(())
    }

    pub fn get_property(&self, id: u64, name: &str) -> Result<Option<Value>, String> {
        let key = id as u32;

        let Some(element) = self.elements.get(&key) else {
            return Err(format!("invalid UI handle {}", id));
        };

        let common = element.common();

        match name {
            "key" => Ok(Some(Value::String(common.key.clone()))),
            "anchor" => Ok(Some(Value::String(format!("{:?}", common.anchor)))),
            "position" => Ok(Some(Value::array(vec![
                Value::Number(common.position[0] as f64),
                Value::Number(common.position[1] as f64),
            ]))),

            "size" => Ok(Some(Value::array(vec![
                Value::Number(common.size[0] as f64),
                Value::Number(common.size[1] as f64),
            ]))),

            "visible" => Ok(Some(Value::Bool(common.visible))),

            "enabled" => Ok(Some(Value::Bool(common.enabled))),

            "color" => Ok(Some(Value::array(vec![
                Value::Number(common.color[0] as f64),
                Value::Number(common.color[1] as f64),
                Value::Number(common.color[2] as f64),
                Value::Number(common.color[3] as f64),
            ]))),
            "background_color" => Ok(Some(Value::array(
                common
                    .background_color
                    .iter()
                    .map(|n| Value::Number(*n as f64))
                    .collect(),
            ))),

            "parent" => Ok(Some(match common.parent {
                Some(parent_id) => Value::Handle {
                    kind: HandleKind::Ui,
                    id: parent_id as u64,
                },
                None => Value::Nil,
            })),

            "z_index" => Ok(Some(Value::Number(common.z_index as f64))),

            "corner_radius" => Ok(Some(Value::Number(common.corner_radius as f64))),

            "border_width" => Ok(Some(Value::Number(common.border_width as f64))),

            "border_color" => Ok(Some(Value::array(vec![
                Value::Number(common.border_color[0] as f64),
                Value::Number(common.border_color[1] as f64),
                Value::Number(common.border_color[2] as f64),
                Value::Number(common.border_color[3] as f64),
            ]))),

            "source" => match element {
                UiElement::Image(media) | UiElement::Video(media) => {
                    Ok(Some(Value::String(media.source.clone())))
                }
                _ => Err(format!(
                    "property 'source' is not valid on {}",
                    element_type_name(element)
                )),
            },
            "fit" => match element {
                UiElement::Image(media) | UiElement::Video(media) => {
                    Ok(Some(Value::String(format!("{:?}", media.fit))))
                }
                _ => Err(format!(
                    "property 'fit' is not valid on {}",
                    element_type_name(element)
                )),
            },
            "playing" | "looped" | "playback_rate" => match element {
                UiElement::Video(media) => Ok(Some(match name {
                    "playing" => Value::Bool(media.playing),
                    "looped" => Value::Bool(media.looped),
                    _ => Value::Number(media.playback_rate as f64),
                })),
                _ => Err(format!(
                    "property '{name}' is not valid on {}",
                    element_type_name(element)
                )),
            },
            "font_size" => match element {
                UiElement::Text(t) => Ok(Some(Value::Number(t.font_size as f64))),
                UiElement::Button(b) => Ok(Some(Value::Number(b.font_size as f64))),
                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'font_size' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "font" => match element {
                UiElement::Text(t) => Ok(Some(Value::String(format!("{:?}", t.font_family)))),
                UiElement::Button(b) => Ok(Some(Value::String(format!("{:?}", b.font_family)))),
                _ => Err(format!(
                    "property 'font' is not valid on {}",
                    element_type_name(element)
                )),
            },
            "bold" | "italic" | "rich_text" => match element {
                UiElement::Text(t) => Ok(Some(Value::Bool(match name {
                    "bold" => t.bold,
                    "italic" => t.italic,
                    _ => t.rich_text,
                }))),
                UiElement::Button(b) => Ok(Some(Value::Bool(match name {
                    "bold" => b.bold,
                    "italic" => b.italic,
                    _ => b.rich_text,
                }))),
                _ => Err(format!(
                    "property '{name}' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "text_scaled" => match element {
                UiElement::Text(t) => Ok(Some(Value::Bool(t.text_scaled))),
                _ => Err(format!(
                    "property 'text_scaled' is only valid on Text, not {}",
                    element_type_name(element)
                )),
            },

            "horizontal_align" => match element {
                UiElement::Text(t) => Ok(Some(Value::String(format!("{:?}", t.horizontal_align)))),
                UiElement::Button(b) => {
                    Ok(Some(Value::String(format!("{:?}", b.horizontal_align))))
                }
                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'horizontal_align' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "vertical_align" => match element {
                UiElement::Text(t) => Ok(Some(Value::String(format!("{:?}", t.vertical_align)))),
                UiElement::Button(b) => Ok(Some(Value::String(format!("{:?}", b.vertical_align)))),
                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'vertical_align' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "wrap" => match element {
                UiElement::Text(t) => Ok(Some(Value::Bool(t.wrap))),
                UiElement::Button(b) => Ok(Some(Value::Bool(b.wrap))),
                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'wrap' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "padding" => match element {
                UiElement::Text(t) => Ok(Some(Value::array(vec![
                    Value::Number(t.padding[0] as f64),
                    Value::Number(t.padding[1] as f64),
                    Value::Number(t.padding[2] as f64),
                    Value::Number(t.padding[3] as f64),
                ]))),
                UiElement::Button(b) => Ok(Some(Value::array(vec![
                    Value::Number(b.padding[0] as f64),
                    Value::Number(b.padding[1] as f64),
                    Value::Number(b.padding[2] as f64),
                    Value::Number(b.padding[3] as f64),
                ]))),
                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'padding' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "line_spacing" => match element {
                UiElement::Text(t) => Ok(Some(Value::Number(t.line_spacing as f64))),
                UiElement::Button(b) => Ok(Some(Value::Number(b.line_spacing as f64))),
                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'line_spacing' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "text" => match element {
                UiElement::Text(t) => Ok(Some(Value::String(t.text.clone()))),
                UiElement::Button(b) => Ok(Some(Value::String(b.text.clone()))),

                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => Err(format!(
                    "property 'text' is not valid on {}",
                    element_type_name(element)
                )),
            },

            "on_click" => match element {
                UiElement::Button(b) => Ok(Some(b.on_click.clone().unwrap_or(Value::Nil))),

                _ => Err(format!(
                    "property 'on_click' is not valid on {}",
                    element_type_name(element)
                )),
            },

            _ => Err(format!("unknown property '{}' on UI element", name)),
        }
    }

    pub fn set_property(&mut self, id: u64, name: &str, value: Value) -> Result<bool, String> {
        if name == "anchor" {
            let value = value.as_string()?;
            let anchor = UiAnchor::ALL
                .into_iter()
                .find(|anchor| format!("{:?}", anchor) == value)
                .ok_or_else(|| format!("unknown UI anchor '{}'", value))?;
            let element = self
                .elements
                .get_mut(&(id as u32))
                .ok_or_else(|| format!("invalid UI handle {}", id))?;
            element.common_mut().anchor = anchor;
            return Ok(true);
        }
        if name == "key" {
            let value = value.as_string()?.trim().to_string();
            if !value.is_empty() && !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err("UI key must contain only letters, digits, or underscores".to_string());
            }
            if !value.is_empty()
                && self
                    .elements
                    .iter()
                    .any(|(other, element)| *other as u64 != id && element.common().key == value)
            {
                return Err(format!("duplicate UI key '{}'", value));
            }
            let element = self
                .elements
                .get_mut(&(id as u32))
                .ok_or_else(|| format!("invalid UI handle {}", id))?;
            element.common_mut().key = value;
            return Ok(true);
        }
        let key = id as u32;

        if !self.elements.contains_key(&key) {
            return Err(format!("invalid UI handle {}", id));
        }

        match name {
            "source" => {
                let path = value.as_string()?;
                crate::engine::ui_media::validate_ui_media_source(path).map_err(|error| {
                    format!(
                        "{} property 'source': {error}",
                        element_label(self.elements.get(&key).unwrap(), key)
                    )
                })?;
                let is_video = matches!(self.elements.get(&key), Some(UiElement::Video(_)));
                if !path.is_empty() && is_video != path.to_ascii_lowercase().ends_with(".mp4") {
                    return Err("Video source must be MP4, Image source must be PNG/JPEG".into());
                }
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Image(media) | UiElement::Video(media) => {
                        media.source = path.to_string();
                        Ok(true)
                    }
                    _ => Err(format!(
                        "{} property 'source': unsupported element",
                        element_label(self.elements.get(&key).unwrap(), key)
                    )),
                }
            }
            "fit" => {
                let fit = value.as_string()?;
                let fit = UiMediaFit::ALL
                    .into_iter()
                    .find(|variant| format!("{variant:?}") == fit)
                    .ok_or_else(|| format!("fit must be Contain, Cover or Stretch; got '{fit}'"))?;
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Image(media) | UiElement::Video(media) => {
                        media.fit = fit;
                        Ok(true)
                    }
                    _ => Err(format!(
                        "{} property 'fit': unsupported element",
                        element_label(self.elements.get(&key).unwrap(), key)
                    )),
                }
            }
            "playing" | "looped" | "playback_rate" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                if !matches!(elem, UiElement::Video(_)) {
                    return Err(format!(
                        "{label} property '{name}': only Video supports this"
                    ));
                }
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Video(media) => {
                        match name {
                            "playing" => media.playing = value.as_bool()?,
                            "looped" => media.looped = value.as_bool()?,
                            _ => {
                                let rate = value.as_number()?;
                                if !rate.is_finite() || rate <= 0.0 || rate > 4.0 {
                                    return Err(format!(
                                        "{label} property 'playback_rate': expected a rate between 0 and 4"
                                    ));
                                }
                                media.playback_rate = rate as f32;
                            }
                        }
                        Ok(true)
                    }
                    _ => unreachable!(),
                }
            }
            "position" => {
                let basket = value.as_basket()?;
                let borrowed = basket.borrow();

                if borrowed.elements.len() != 2 {
                    return Err("position expects an array of 2 numbers [x, y]".to_string());
                }

                let x = borrowed.elements[0].as_number()? as f32;
                let y = borrowed.elements[1].as_number()? as f32;

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .position = [x, y];

                Ok(true)
            }

            "size" => {
                let basket = value.as_basket()?;
                let borrowed = basket.borrow();

                if borrowed.elements.len() != 2 {
                    return Err("size expects an array of 2 numbers [width, height]".to_string());
                }

                let width = borrowed.elements[0].as_number()? as f32;
                let height = borrowed.elements[1].as_number()? as f32;

                if width < 0.0 || height < 0.0 {
                    return Err("size values must be greater than or equal to zero".to_string());
                }

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .size = [width, height];

                Ok(true)
            }

            "visible" => {
                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .visible = value.as_bool()?;

                Ok(true)
            }

            "enabled" => {
                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .enabled = value.as_bool()?;

                Ok(true)
            }

            "color" => {
                let basket = value.as_basket()?;
                let borrowed = basket.borrow();

                if borrowed.elements.len() != 4 {
                    return Err("color expects an array of 4 numbers [r, g, b, a]".to_string());
                }

                let r = borrowed.elements[0].as_number()? as f32;
                let g = borrowed.elements[1].as_number()? as f32;
                let b = borrowed.elements[2].as_number()? as f32;
                let a = borrowed.elements[3].as_number()? as f32;

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .color = [r, g, b, a];

                Ok(true)
            }

            "background_color" => {
                let basket = value.as_basket()?;
                let borrowed = basket.borrow();
                if borrowed.elements.len() != 4 {
                    return Err("background_color expects [r, g, b, a]".into());
                }
                let mut rgba = [0.0_f32; 4];
                for (index, value) in borrowed.elements.iter().enumerate() {
                    let number = value.as_number()? as f32;
                    if !number.is_finite() {
                        return Err("background_color values must be finite".into());
                    }
                    rgba[index] = number.clamp(0.0, 1.0);
                }
                self.elements
                    .get_mut(&key)
                    .unwrap()
                    .common_mut()
                    .background_color = rgba;
                Ok(true)
            }

            "parent" => {
                let new_parent = match value {
                    Value::Nil => None,

                    Value::Handle {
                        kind: HandleKind::Ui,
                        id,
                    } if id >= 1 && id <= u32::MAX as u64 => Some(id as u32),

                    Value::Number(number) => {
                        if !number.is_finite()
                            || number < 1.0
                            || number.fract() != 0.0
                            || number > u32::MAX as f64
                        {
                            return Err("parent expects a valid UI handle or nil".to_string());
                        }

                        Some(number as u32)
                    }

                    Value::Handle { kind, .. } => {
                        return Err(format!(
                            "parent expects a UI handle or nil, got {} handle",
                            kind.name()
                        ));
                    }

                    other => {
                        return Err(format!(
                            "parent expects a UI handle or nil, got {}",
                            other.type_name()
                        ));
                    }
                };

                if new_parent == Some(key) {
                    return Err("a UI element cannot be its own parent".to_string());
                }

                if let Some(parent_id) = new_parent {
                    if !self.elements.contains_key(&parent_id) {
                        return Err(format!("invalid UI parent handle {}", parent_id));
                    }

                    self.ensure_no_parent_cycle(key, parent_id)?;
                }

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .parent = new_parent;

                Ok(true)
            }

            "z_index" => {
                let number = value.as_number()?;

                if !number.is_finite() {
                    return Err("z_index must be a finite number".to_string());
                }

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .z_index = number as f32;

                Ok(true)
            }

            "corner_radius" => {
                let radius = value.as_number()? as f32;

                if !radius.is_finite() || radius < 0.0 {
                    return Err(
                        "corner_radius must be a finite number greater than or equal to zero"
                            .to_string(),
                    );
                }

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .corner_radius = radius;

                Ok(true)
            }

            "border_width" => {
                let width = value.as_number()? as f32;

                if !width.is_finite() || width < 0.0 {
                    return Err(
                        "border_width must be a finite number greater than or equal to zero"
                            .to_string(),
                    );
                }

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .border_width = width;

                Ok(true)
            }

            "border_color" => {
                let basket = value.as_basket()?;
                let borrowed = basket.borrow();

                if borrowed.elements.len() != 4 {
                    return Err(
                        "border_color expects an array of 4 numbers [r, g, b, a]".to_string()
                    );
                }

                let r = borrowed.elements[0].as_number()? as f32;
                let g = borrowed.elements[1].as_number()? as f32;
                let b = borrowed.elements[2].as_number()? as f32;
                let a = borrowed.elements[3].as_number()? as f32;

                self.elements
                    .get_mut(&key)
                    .expect("UI element disappeared during property update")
                    .common_mut()
                    .border_color = [r, g, b, a];

                Ok(true)
            }

            "text" => match self
                .elements
                .get_mut(&key)
                .expect("UI element disappeared during property update")
            {
                UiElement::Text(text) => {
                    text.text = value.as_string()?.to_string();
                    Ok(true)
                }

                UiElement::Button(button) => {
                    button.text = value.as_string()?.to_string();
                    Ok(true)
                }

                UiElement::Panel(_) | UiElement::Image(_) | UiElement::Video(_) => {
                    let label = element_label(self.elements.get(&key).unwrap(), key);
                    Err(format!("{label}, property 'text': is not valid on Panel"))
                }
            },

            "font_size" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let size = match value.as_number() {
                    Ok(num) if num.is_finite() && num > 0.0 => num as f32,
                    _ => {
                        return Err(format!(
                            "{label}, property 'font_size': expected a finite number greater than 0, got {}",
                            value.type_name()
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.font_size = size,
                    UiElement::Button(b) => b.font_size = size,
                    UiElement::Panel(..) | UiElement::Image(..) | UiElement::Video(..) => {
                        return Err(format!(
                            "{label}, property 'font_size': is not valid on Panel"
                        ));
                    }
                }
                Ok(true)
            }

            "font" => {
                let family = match value.as_string()? {
                    "Proportional" | "proportional" => UiFontFamily::Proportional,
                    "Monospace" | "monospace" => UiFontFamily::Monospace,
                    _ => {
                        return Err(
                            "property 'font': expected 'Proportional' or 'Monospace'".into()
                        );
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.font_family = family,
                    UiElement::Button(b) => b.font_family = family,
                    other => {
                        return Err(format!(
                            "property 'font' is not valid on {}",
                            element_type_name(other)
                        ));
                    }
                }
                Ok(true)
            }

            "bold" | "italic" | "rich_text" => {
                let enabled = value.as_bool().map_err(|_| {
                    format!(
                        "property '{name}': expected boolean, got {}",
                        value.type_name()
                    )
                })?;
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => match name {
                        "bold" => t.bold = enabled,
                        "italic" => t.italic = enabled,
                        _ => t.rich_text = enabled,
                    },
                    UiElement::Button(b) => match name {
                        "bold" => b.bold = enabled,
                        "italic" => b.italic = enabled,
                        _ => b.rich_text = enabled,
                    },
                    other => {
                        return Err(format!(
                            "property '{name}' is not valid on {}",
                            element_type_name(other)
                        ));
                    }
                }
                Ok(true)
            }

            "text_scaled" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let enabled = match value {
                    Value::Bool(enabled) => enabled,
                    _ => {
                        return Err(format!(
                            "{label}, property 'text_scaled': expected Bool, got {}",
                            value.type_name()
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(text) => text.text_scaled = enabled,
                    _ => {
                        return Err(format!(
                            "{label}, property 'text_scaled': is only valid on Text"
                        ));
                    }
                }
                Ok(true)
            }

            "horizontal_align" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let align_str = match value.as_string() {
                    Ok(s) => s,
                    Err(_) => {
                        return Err(format!(
                            "{label}, property 'horizontal_align': expected 'Left', 'Center', or 'Right', got {}",
                            value.type_name()
                        ));
                    }
                };
                let align = match align_str {
                    "Left" => UiHorizontalAlign::Left,
                    "Center" => UiHorizontalAlign::Center,
                    "Right" => UiHorizontalAlign::Right,
                    _ => {
                        return Err(format!(
                            "{label}, property 'horizontal_align': expected 'Left', 'Center', or 'Right', got string"
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.horizontal_align = align,
                    UiElement::Button(b) => b.horizontal_align = align,
                    UiElement::Panel(..) | UiElement::Image(..) | UiElement::Video(..) => {
                        return Err(format!(
                            "{label}, property 'horizontal_align': is not valid on Panel"
                        ));
                    }
                }
                Ok(true)
            }

            "vertical_align" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let align_str = match value.as_string() {
                    Ok(s) => s,
                    Err(_) => {
                        return Err(format!(
                            "{label}, property 'vertical_align': expected 'Top', 'Center', or 'Bottom', got {}",
                            value.type_name()
                        ));
                    }
                };
                let align = match align_str {
                    "Top" => UiVerticalAlign::Top,
                    "Center" => UiVerticalAlign::Center,
                    "Bottom" => UiVerticalAlign::Bottom,
                    _ => {
                        return Err(format!(
                            "{label}, property 'vertical_align': expected 'Top', 'Center', or 'Bottom', got string"
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.vertical_align = align,
                    UiElement::Button(b) => b.vertical_align = align,
                    UiElement::Panel(..) | UiElement::Image(..) | UiElement::Video(..) => {
                        return Err(format!(
                            "{label}, property 'vertical_align': is not valid on Panel"
                        ));
                    }
                }
                Ok(true)
            }

            "wrap" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let wrap = match value.as_bool() {
                    Ok(b) => b,
                    Err(_) => {
                        return Err(format!(
                            "{label}, property 'wrap': expected boolean, got {}",
                            value.type_name()
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.wrap = wrap,
                    UiElement::Button(b) => b.wrap = wrap,
                    UiElement::Panel(..) | UiElement::Image(..) | UiElement::Video(..) => {
                        return Err(format!("{label}, property 'wrap': is not valid on Panel"));
                    }
                }
                Ok(true)
            }

            "padding" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let padding = match (|| -> Result<[f32; 4], ()> {
                    let basket = value.as_basket().map_err(|_| ())?;
                    let borrowed = basket.borrow();
                    if borrowed.elements.len() != 4 {
                        return Err(());
                    }
                    let l = borrowed.elements[0].as_number().map_err(|_| ())? as f32;
                    let t = borrowed.elements[1].as_number().map_err(|_| ())? as f32;
                    let r = borrowed.elements[2].as_number().map_err(|_| ())? as f32;
                    let b = borrowed.elements[3].as_number().map_err(|_| ())? as f32;
                    if !l.is_finite()
                        || !t.is_finite()
                        || !r.is_finite()
                        || !b.is_finite()
                        || l < 0.0
                        || t < 0.0
                        || r < 0.0
                        || b < 0.0
                    {
                        return Err(());
                    }
                    Ok([l, t, r, b])
                })() {
                    Ok(arr) => arr,
                    Err(_) => {
                        return Err(format!(
                            "{label}, property 'padding': expected an array of 4 non-negative numbers [left, top, right, bottom], got {}",
                            value.type_name()
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.padding = padding,
                    UiElement::Button(b) => b.padding = padding,
                    UiElement::Panel(..) | UiElement::Image(..) | UiElement::Video(..) => {
                        return Err(format!(
                            "{label}, property 'padding': is not valid on Panel"
                        ));
                    }
                }
                Ok(true)
            }

            "line_spacing" => {
                let elem = self.elements.get(&key).unwrap();
                let label = element_label(elem, key);
                let spacing = match value.as_number() {
                    Ok(num) if num.is_finite() && num >= 0.0 => num as f32,
                    _ => {
                        return Err(format!(
                            "{label}, property 'line_spacing': expected a finite number greater than or equal to 0, got {}",
                            value.type_name()
                        ));
                    }
                };
                match self.elements.get_mut(&key).unwrap() {
                    UiElement::Text(t) => t.line_spacing = spacing,
                    UiElement::Button(b) => b.line_spacing = spacing,
                    UiElement::Panel(..) | UiElement::Image(..) | UiElement::Video(..) => {
                        return Err(format!(
                            "{label}, property 'line_spacing': is not valid on Panel"
                        ));
                    }
                }
                Ok(true)
            }

            "on_click" => match self
                .elements
                .get_mut(&key)
                .expect("UI element disappeared during property update")
            {
                UiElement::Button(button) => {
                    match value {
                        Value::Function(..) => button.on_click = Some(value),

                        Value::Nil => button.on_click = None,

                        other => {
                            let label = element_label(self.elements.get(&key).unwrap(), key);
                            return Err(format!(
                                "{label}, property 'on_click': expected a function or nil, got {}",
                                other.type_name()
                            ));
                        }
                    }

                    Ok(true)
                }

                _ => {
                    let label = element_label(self.elements.get(&key).unwrap(), key);
                    Err(format!(
                        "{label}, property 'on_click': is not valid on {}",
                        element_type_name(
                            self.elements
                                .get(&key)
                                .expect("UI element disappeared during property update")
                        )
                    ))
                }
            },

            _ => {
                let label = element_label(self.elements.get(&key).unwrap(), key);
                Err(format!("{label}, unknown property '{name}'"))
            }
        }
    }

    pub fn resolve_layout(&self, id: u64) -> Result<UiResolvedLayout, String> {
        self.resolve_layout_in(id, [1920.0, 1080.0])
    }

    pub fn resolve_layout_in(
        &self,
        id: u64,
        viewport: [f32; 2],
    ) -> Result<UiResolvedLayout, String> {
        let key = id as u32;

        if !self.elements.contains_key(&key) {
            return Err(format!("invalid UI handle {}", id));
        }

        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = key;

        loop {
            if !visited.insert(current) {
                return Err(format!(
                    "cyclic UI parent hierarchy detected at handle {}",
                    current
                ));
            }

            let Some(current_element) = self.elements.get(&current) else {
                return Err(format!(
                    "UI element {} references missing parent {}",
                    key, current
                ));
            };

            chain.push(current);

            match current_element.common().parent {
                Some(parent) => current = parent,
                None => break,
            }
        }

        chain.reverse();

        let mut position = [0.0_f32, 0.0_f32];
        let mut size = viewport;
        let mut visible = true;
        let mut enabled = true;
        let mut z_index = 0.0_f32;

        for chain_id in chain {
            let chain_element = self
                .elements
                .get(&chain_id)
                .expect("UI hierarchy changed while resolving layout");

            let common = chain_element.common();

            let relative_position = [
                common.position[0] + size[0] * common.position_scale[0],
                common.position[1] + size[1] * common.position_scale[1],
            ];
            let relative_size = [
                common.size[0] + size[0] * common.size_scale[0],
                common.size[1] + size[1] * common.size_scale[1],
            ];
            let relative_size = if common.anchor == UiAnchor::Stretch {
                relative_size
            } else {
                [relative_size[0].max(0.0), relative_size[1].max(0.0)]
            };
            let (offset, resolved_size) =
                common
                    .anchor
                    .resolve(size, relative_position, relative_size);
            position[0] += offset[0];
            position[1] += offset[1];
            size = resolved_size;

            visible &= common.visible;
            enabled &= common.enabled;
            z_index += common.z_index;
        }

        Ok(UiResolvedLayout {
            position,
            size,
            visible,
            enabled,
            z_index,
        })
    }

    fn ensure_no_parent_cycle(&self, child_id: u32, parent_id: u32) -> Result<(), String> {
        let mut visited = HashSet::new();
        let mut current = parent_id;

        loop {
            if !visited.insert(current) {
                return Err(format!(
                    "cyclic UI parent hierarchy detected at handle {}",
                    current
                ));
            }

            if current == child_id {
                return Err("setting this parent would create a cyclic UI hierarchy".to_string());
            }

            let Some(element) = self.elements.get(&current) else {
                return Err(format!("invalid UI parent handle {}", current));
            };

            match element.common().parent {
                Some(parent) => current = parent,
                None => break,
            }
        }

        Ok(())
    }

    pub fn set_symbolic_event_binding(
        &mut self,
        id: u64,
        event: &str,
        binding: Option<UiSymbolicEventBinding>,
    ) -> Result<(), String> {
        let key = id as u32;
        let Some(element) = self.elements.get(&key) else {
            return Err(format!("invalid UI handle {}", id));
        };
        if event != "on_click" || !matches!(element, UiElement::Button(_)) {
            return Err(format!(
                "symbolic event '{}' is not valid on {}",
                event,
                element_type_name(element)
            ));
        }
        let map_key = (key, event.to_string());
        match binding {
            Some(binding) => {
                self.symbolic_events.insert(map_key, binding);
            }
            None => {
                self.symbolic_events.remove(&map_key);
            }
        }
        Ok(())
    }

    pub fn symbolic_event_dispatch(&self, id: u64, event: &str) -> Option<UiSymbolicEventDispatch> {
        let key = id as u32;
        let element = self.elements.get(&key)?;
        let binding = self.symbolic_events.get(&(key, event.to_string()))?;
        Some(UiSymbolicEventDispatch {
            document_key: self.document_key.clone(),
            element_key: element.common().key.clone(),
            element_type: element_type_name(element).to_string(),
            event: event.to_string(),
            module_key: binding.module_key.clone(),
            handler: binding.handler.clone(),
        })
    }

    pub fn drain_pending_clicks(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.pending_clicks)
            .into_iter()
            .map(|id| id as u64)
            .collect()
    }
}

fn element_type_name(element: &UiElement) -> &'static str {
    match element {
        UiElement::Panel(_) => "Panel",
        UiElement::Text(_) => "Text",
        UiElement::Button(_) => "Button",
        UiElement::Image(_) => "Image",
        UiElement::Video(_) => "Video",
    }
}

fn element_label(element: &UiElement, id: u32) -> String {
    let key = &element.common().key;
    let type_name = element_type_name(element);
    if key.is_empty() {
        format!("UI element #{id} ({type_name})")
    } else {
        format!("UI element '{key}' ({type_name})")
    }
}

fn ui_text_horizontal_origin(rect: egui::Rect, align: UiHorizontalAlign) -> f32 {
    match align {
        UiHorizontalAlign::Left => rect.min.x,
        UiHorizontalAlign::Center => rect.center().x,
        UiHorizontalAlign::Right => rect.max.x,
    }
}

pub fn paint_ui_text(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: egui::Rect,
    text: &str,
    font_size: f32,
    text_scaled: bool,
    horizontal_align: UiHorizontalAlign,
    vertical_align: UiVerticalAlign,
    wrap: bool,
    padding: [f32; 4],
    line_spacing: f32,
    color: egui::Color32,
) {
    paint_ui_text_styled(
        painter,
        ctx,
        rect,
        text,
        font_size,
        text_scaled,
        horizontal_align,
        vertical_align,
        wrap,
        padding,
        line_spacing,
        color,
        UiFontFamily::Monospace,
        false,
        false,
        false,
    );
}

pub fn paint_ui_text_styled(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: egui::Rect,
    text: &str,
    font_size: f32,
    text_scaled: bool,
    horizontal_align: UiHorizontalAlign,
    vertical_align: UiVerticalAlign,
    wrap: bool,
    padding: [f32; 4],
    line_spacing: f32,
    color: egui::Color32,
    font_family: UiFontFamily,
    bold: bool,
    italic: bool,
    rich_text: bool,
) {
    if text.is_empty() {
        return;
    }

    let padded_rect = egui::Rect::from_min_max(
        egui::pos2(rect.min.x + padding[0], rect.min.y + padding[1]),
        egui::pos2(
            (rect.max.x - padding[2]).max(rect.min.x + padding[0]),
            (rect.max.y - padding[3]).max(rect.min.y + padding[1]),
        ),
    );
    let wrap_width = if wrap {
        padded_rect.width().max(1.0)
    } else {
        f32::INFINITY
    };

    let layout = |size: f32| {
        let mut format = egui::TextFormat::simple(font_family.font_id(size), color);
        format.italics = italic;
        if line_spacing > 0.0 {
            format.line_height = Some(size + line_spacing);
        }
        let mut job = egui::text::LayoutJob::default();
        if rich_text {
            append_color_markup(&mut job, text, &format);
        } else {
            job.append(text, 0.0, format);
        }
        job.break_on_newline = true;
        job.wrap.max_width = wrap_width;
        job.halign = match horizontal_align {
            UiHorizontalAlign::Left => egui::Align::Min,
            UiHorizontalAlign::Center => egui::Align::Center,
            UiHorizontalAlign::Right => egui::Align::Max,
        };
        ctx.fonts(|fonts| fonts.layout_job(job))
    };

    let actual_size = if text_scaled && padded_rect.width() >= 1.0 && padded_rect.height() >= 1.0 {
        let fits = |size: f32| {
            let galley = layout(size);
            galley.size().x <= padded_rect.width() + 0.01
                && galley.size().y <= padded_rect.height() + 0.01
        };
        let mut lower = 1.0_f32;
        let mut upper = 256.0_f32;
        if fits(upper) {
            upper
        } else {
            for _ in 0..10 {
                let mid = (lower + upper) * 0.5;
                if fits(mid) {
                    lower = mid;
                } else {
                    upper = mid;
                }
            }
            lower
        }
    } else if font_size.is_finite() && font_size > 0.0 {
        font_size
    } else {
        14.0
    };
    let galley = layout(actual_size);
    let x = ui_text_horizontal_origin(padded_rect, horizontal_align);
    let y = match vertical_align {
        UiVerticalAlign::Top => padded_rect.min.y,
        UiVerticalAlign::Center => {
            padded_rect.min.y + (padded_rect.height() - galley.size().y).max(0.0) * 0.5
        }
        UiVerticalAlign::Bottom => (padded_rect.max.y - galley.size().y).max(padded_rect.min.y),
    };

    let painter = painter.with_clip_rect(padded_rect.intersect(painter.clip_rect()));
    if bold {
        painter.galley(egui::pos2(x + 0.65, y), galley.clone(), color);
    }
    painter.galley(egui::pos2(x, y), galley, color);
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiColorSpan {
    pub text: String,
    pub color: [f32; 4],
}

pub fn parse_ui_color_spans(text: &str, fallback: [f32; 4]) -> Vec<UiColorSpan> {
    let mut spans = Vec::new();
    let mut rest = text;
    loop {
        let Some(start) = rest.find("[color=#") else {
            if !rest.is_empty() || spans.is_empty() {
                spans.push(UiColorSpan {
                    text: rest.to_string(),
                    color: fallback,
                });
            }
            break;
        };
        if start > 0 {
            spans.push(UiColorSpan {
                text: rest[..start].to_string(),
                color: fallback,
            });
        }
        let tag = &rest[start + 8..];
        let Some(close) = tag.find(']') else {
            spans.push(UiColorSpan {
                text: rest[start..].to_string(),
                color: fallback,
            });
            break;
        };
        let hex = &tag[..close];
        let body = &tag[close + 1..];
        let Some(end) = body.find("[/color]") else {
            spans.push(UiColorSpan {
                text: rest[start..].to_string(),
                color: fallback,
            });
            break;
        };
        let parsed = (hex.len() == 6 || hex.len() == 8)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten();
        let Some(value) = parsed else {
            spans.push(UiColorSpan {
                text: rest[start..start + 9 + close].to_string(),
                color: fallback,
            });
            rest = body;
            continue;
        };
        let rgba = if hex.len() == 6 {
            [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
        } else {
            [
                (value >> 24) as u8,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            ]
        };
        spans.push(UiColorSpan {
            text: body[..end].to_string(),
            color: rgba.map(|component| component as f32 / 255.0),
        });
        rest = &body[end + 8..];
    }
    spans
}

pub fn encode_ui_color_spans(spans: &[UiColorSpan]) -> String {
    let mut encoded = String::new();
    for span in spans {
        let rgba = span
            .color
            .map(|component| (component.clamp(0.0, 1.0) * 255.0).round() as u8);
        encoded.push_str(&format!(
            "[color=#{:02X}{:02X}{:02X}{:02X}]{}[/color]",
            rgba[0], rgba[1], rgba[2], rgba[3], span.text
        ));
    }
    encoded
}

fn append_color_markup(job: &mut egui::text::LayoutJob, text: &str, base: &egui::TextFormat) {
    let mut rest = text;
    loop {
        let Some(start) = rest.find("[color=#") else {
            job.append(rest, 0.0, base.clone());
            break;
        };
        job.append(&rest[..start], 0.0, base.clone());
        let tag = &rest[start + 8..];
        let Some(close) = tag.find(']') else {
            job.append(&rest[start..], 0.0, base.clone());
            break;
        };
        let hex = &tag[..close];
        let body = &tag[close + 1..];
        let Some(end) = body.find("[/color]") else {
            job.append(&rest[start..], 0.0, base.clone());
            break;
        };
        let parsed = (hex.len() == 6 || hex.len() == 8)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten();
        let Some(value) = parsed else {
            job.append(&rest[start..start + 9 + close], 0.0, base.clone());
            rest = body;
            continue;
        };
        let mut format = base.clone();
        format.color = if hex.len() == 6 {
            egui::Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
        } else {
            egui::Color32::from_rgba_unmultiplied(
                (value >> 24) as u8,
                (value >> 16) as u8,
                (value >> 8) as u8,
                value as u8,
            )
        };
        job.append(&body[..end], 0.0, format);
        rest = &body[end + 8..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::value::Value;

    #[test]
    fn text_scaled_and_centered_text_contract() {
        let mut runtime = RuntimeUi::new();
        let id = runtime.allocate("Text").unwrap();
        assert_eq!(
            runtime.get_property(id, "text_scaled").unwrap(),
            Some(Value::Bool(false))
        );
        runtime
            .set_property(id, "text_scaled", Value::Bool(true))
            .unwrap();
        assert_eq!(
            runtime.get_property(id, "text_scaled").unwrap(),
            Some(Value::Bool(true))
        );
        assert!(
            runtime
                .set_property(id, "text_scaled", Value::Number(1.0))
                .is_err()
        );
        let panel = runtime.allocate("Panel").unwrap();
        assert!(
            runtime
                .set_property(panel, "text_scaled", Value::Bool(true))
                .is_err()
        );
        let area = egui::Rect::from_min_size(egui::pos2(50.0, 25.0), egui::vec2(400.0, 100.0));
        assert_eq!(
            ui_text_horizontal_origin(area, UiHorizontalAlign::Left),
            50.0
        );
        assert_eq!(
            ui_text_horizontal_origin(area, UiHorizontalAlign::Center),
            250.0
        );
        assert_eq!(
            ui_text_horizontal_origin(area, UiHorizontalAlign::Right),
            450.0
        );

        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("text_scaled_test"),
            ));
            paint_ui_text(
                &painter,
                ctx,
                area,
                "+1",
                18.0,
                true,
                UiHorizontalAlign::Center,
                UiVerticalAlign::Center,
                false,
                [0.0; 4],
                0.0,
                egui::Color32::WHITE,
            );
        });
        let drawn = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::epaint::Shape::Text(text) = &s.shape {
                    Some(text)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(drawn.len(), 1);
        assert!((drawn[0].pos.x - area.center().x).abs() < 0.01);
        assert!(drawn[0].galley.size().x <= area.width() + 0.1);
        assert!(drawn[0].galley.size().y <= area.height() + 0.1);
        assert!(drawn[0].galley.job.sections[0].format.font_id.size > 18.0);
    }

    #[test]
    fn parent_relative_position_is_resolved() {
        let mut ui = RuntimeUi::new();

        let parent = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Text").unwrap();

        ui.set_property(
            parent,
            "position",
            Value::array(vec![Value::Number(100.0), Value::Number(50.0)]),
        )
        .unwrap();

        ui.set_property(
            child,
            "position",
            Value::array(vec![Value::Number(25.0), Value::Number(10.0)]),
        )
        .unwrap();

        ui.set_property(child, "parent", Value::Number(parent as f64))
            .unwrap();

        let layout = ui.resolve_layout(child).unwrap();

        assert_eq!(layout.position, [125.0, 60.0]);
    }

    #[test]
    fn parent_accepts_ui_handle_returned_by_script_api() {
        let mut ui = RuntimeUi::new();
        let parent = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Text").unwrap();

        ui.set_property(
            child,
            "parent",
            Value::Handle {
                kind: HandleKind::Ui,
                id: parent,
            },
        )
        .unwrap();

        assert_eq!(
            ui.elements.get(&(child as u32)).unwrap().common().parent,
            Some(parent as u32)
        );
        let error = ui
            .set_property(
                child,
                "parent",
                Value::Handle {
                    kind: HandleKind::Actor,
                    id: parent,
                },
            )
            .unwrap_err();
        assert!(
            error.contains("got Actor handle"),
            "wrong handle diagnostic was: {error}"
        );
    }

    #[test]
    fn parent_visibility_and_enabled_state_propagate() {
        let mut ui = RuntimeUi::new();

        let parent = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Button").unwrap();

        ui.set_property(child, "parent", Value::Number(parent as f64))
            .unwrap();

        ui.set_property(parent, "visible", Value::Bool(false))
            .unwrap();

        ui.set_property(parent, "enabled", Value::Bool(false))
            .unwrap();

        let layout = ui.resolve_layout(child).unwrap();

        assert!(!layout.visible);
        assert!(!layout.enabled);
    }

    #[test]
    fn z_index_is_accumulated_through_parents() {
        let mut ui = RuntimeUi::new();

        let parent = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Text").unwrap();

        ui.set_property(parent, "z_index", Value::Number(10.0))
            .unwrap();

        ui.set_property(child, "z_index", Value::Number(2.0))
            .unwrap();

        ui.set_property(child, "parent", Value::Number(parent as f64))
            .unwrap();

        let layout = ui.resolve_layout(child).unwrap();

        assert_eq!(layout.z_index, 12.0);
    }

    #[test]
    fn cyclic_parenting_is_rejected() {
        let mut ui = RuntimeUi::new();

        let first = ui.allocate("Panel").unwrap();
        let second = ui.allocate("Panel").unwrap();

        ui.set_property(second, "parent", Value::Number(first as f64))
            .unwrap();

        assert!(
            ui.set_property(first, "parent", Value::Number(second as f64))
                .is_err()
        );
    }

    #[test]
    fn typography_properties_get_and_set_with_validation() {
        let mut ui = RuntimeUi::new();
        let text_id = ui.allocate("Text").unwrap();
        ui.set_property(text_id, "key", Value::String("speech_body".into()))
            .unwrap();

        assert_eq!(
            ui.get_property(text_id, "font_size").unwrap(),
            Some(Value::Number(14.0))
        );
        assert_eq!(
            ui.get_property(text_id, "horizontal_align").unwrap(),
            Some(Value::String("Left".into()))
        );
        assert_eq!(
            ui.get_property(text_id, "vertical_align").unwrap(),
            Some(Value::String("Top".into()))
        );
        assert_eq!(
            ui.get_property(text_id, "wrap").unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(
            ui.get_property(text_id, "padding").unwrap(),
            Some(Value::array(vec![
                Value::Number(0.0),
                Value::Number(0.0),
                Value::Number(0.0),
                Value::Number(0.0),
            ]))
        );
        assert_eq!(
            ui.get_property(text_id, "line_spacing").unwrap(),
            Some(Value::Number(0.0))
        );
        assert_eq!(
            ui.get_property(text_id, "font").unwrap(),
            Some(Value::String("Proportional".into()))
        );
        assert_eq!(
            ui.get_property(text_id, "bold").unwrap(),
            Some(Value::Bool(false))
        );
        assert_eq!(
            ui.get_property(text_id, "italic").unwrap(),
            Some(Value::Bool(false))
        );
        assert_eq!(
            ui.get_property(text_id, "rich_text").unwrap(),
            Some(Value::Bool(false))
        );

        ui.set_property(text_id, "font_size", Value::Number(22.0))
            .unwrap();
        ui.set_property(text_id, "horizontal_align", Value::String("Center".into()))
            .unwrap();
        ui.set_property(text_id, "vertical_align", Value::String("Bottom".into()))
            .unwrap();
        ui.set_property(text_id, "wrap", Value::Bool(false))
            .unwrap();
        ui.set_property(
            text_id,
            "padding",
            Value::array(vec![
                Value::Number(24.0),
                Value::Number(16.0),
                Value::Number(24.0),
                Value::Number(16.0),
            ]),
        )
        .unwrap();
        ui.set_property(text_id, "line_spacing", Value::Number(4.0))
            .unwrap();
        ui.set_property(text_id, "font", Value::String("Monospace".into()))
            .unwrap();
        ui.set_property(text_id, "bold", Value::Bool(true)).unwrap();
        ui.set_property(text_id, "italic", Value::Bool(true))
            .unwrap();
        ui.set_property(text_id, "rich_text", Value::Bool(true))
            .unwrap();

        assert_eq!(
            ui.get_property(text_id, "font_size").unwrap(),
            Some(Value::Number(22.0))
        );
        assert_eq!(
            ui.get_property(text_id, "horizontal_align").unwrap(),
            Some(Value::String("Center".into()))
        );
        assert_eq!(
            ui.get_property(text_id, "vertical_align").unwrap(),
            Some(Value::String("Bottom".into()))
        );
        assert_eq!(
            ui.get_property(text_id, "wrap").unwrap(),
            Some(Value::Bool(false))
        );
        assert_eq!(
            ui.get_property(text_id, "padding").unwrap(),
            Some(Value::array(vec![
                Value::Number(24.0),
                Value::Number(16.0),
                Value::Number(24.0),
                Value::Number(16.0),
            ]))
        );
        assert_eq!(
            ui.get_property(text_id, "line_spacing").unwrap(),
            Some(Value::Number(4.0))
        );
        assert_eq!(
            ui.get_property(text_id, "font").unwrap(),
            Some(Value::String("Monospace".into()))
        );
        assert_eq!(
            ui.get_property(text_id, "bold").unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(
            ui.get_property(text_id, "italic").unwrap(),
            Some(Value::Bool(true))
        );
        assert_eq!(
            ui.get_property(text_id, "rich_text").unwrap(),
            Some(Value::Bool(true))
        );

        let err = ui
            .set_property(text_id, "font_size", Value::String("invalid".into()))
            .unwrap_err();
        assert!(
            err.contains("UI element 'speech_body' (Text), property 'font_size': expected a finite number greater than 0, got string"),
            "got error: {err}"
        );

        let err = ui
            .set_property(
                text_id,
                "horizontal_align",
                Value::String("LeftAlign".into()),
            )
            .unwrap_err();
        assert!(
            err.contains("UI element 'speech_body' (Text), property 'horizontal_align': expected 'Left', 'Center', or 'Right'"),
            "got error: {err}"
        );

        let panel_id = ui.allocate("Panel").unwrap();
        ui.set_property(panel_id, "key", Value::String("box".into()))
            .unwrap();
        let err = ui
            .set_property(panel_id, "font_size", Value::Number(18.0))
            .unwrap_err();
        assert!(
            err.contains("UI element 'box' (Panel), property 'font_size': is not valid on Panel"),
            "got error: {err}"
        );
    }

    #[test]
    fn rich_text_color_markup_creates_independent_colored_sections() {
        let mut job = egui::text::LayoutJob::default();
        let base = egui::TextFormat::simple(egui::FontId::proportional(14.0), egui::Color32::WHITE);
        append_color_markup(
            &mut job,
            "Hello, [color=#2980ff]HERO[/color]! Find the [color=#ff3030]SWORD[/color].",
            &base,
        );
        assert_eq!(job.sections.len(), 5);
        assert_eq!(
            job.sections[1].format.color,
            egui::Color32::from_rgb(0x29, 0x80, 0xff)
        );
        assert_eq!(
            job.sections[3].format.color,
            egui::Color32::from_rgb(0xff, 0x30, 0x30)
        );
    }

    #[test]
    fn color_spans_round_trip_editor_text_and_rgba() {
        let spans = vec![
            UiColorSpan {
                text: "Hello, ".into(),
                color: [1.0, 1.0, 1.0, 1.0],
            },
            UiColorSpan {
                text: "HERO".into(),
                color: [0.1, 0.4, 1.0, 0.5],
            },
        ];
        let encoded = encode_ui_color_spans(&spans);
        let decoded = parse_ui_color_spans(&encoded, [0.0; 4]);
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].text, "Hello, ");
        assert_eq!(decoded[1].text, "HERO");
        for (actual, expected) in decoded[1].color.iter().zip(spans[1].color) {
            assert!((actual - expected).abs() <= 1.0 / 255.0);
        }
    }

    #[test]
    fn button_typography_defaults_match_authored_contract_and_validate_updates() {
        let mut ui = RuntimeUi::new();
        let button = ui.allocate("Button").unwrap();

        assert_eq!(
            ui.get_property(button, "horizontal_align").unwrap(),
            Some(Value::String("Center".into()))
        );
        assert_eq!(
            ui.get_property(button, "vertical_align").unwrap(),
            Some(Value::String("Center".into()))
        );

        for (property, value) in [
            ("font_size", Value::Number(f64::NAN)),
            ("line_spacing", Value::Number(-1.0)),
            ("wrap", Value::String("true".into())),
            ("vertical_align", Value::String("Middle".into())),
            (
                "padding",
                Value::array(vec![
                    Value::Number(1.0),
                    Value::Number(2.0),
                    Value::Number(3.0),
                ]),
            ),
        ] {
            assert!(
                ui.set_property(button, property, value).is_err(),
                "{property} accepted an invalid value"
            );
        }
    }
}

#[cfg(test)]
mod responsive_tests {
    use super::*;
    #[test]
    fn anchored_nested_layout_tracks_viewport_and_parent() {
        let mut ui = RuntimeUi::new();
        let panel = ui.allocate("Panel").unwrap();
        let child = ui.allocate("Button").unwrap();
        let p = ui.elements.get_mut(&(panel as u32)).unwrap().common_mut();
        p.anchor = UiAnchor::BottomRight;
        p.position = [-32.0, -32.0];
        p.size = [300.0, 100.0];
        let c = ui.elements.get_mut(&(child as u32)).unwrap().common_mut();
        c.parent = Some(panel as u32);
        c.anchor = UiAnchor::Center;
        c.size = [100.0, 40.0];
        assert_eq!(
            ui.resolve_layout_in(child, [1920.0, 1080.0])
                .unwrap()
                .position,
            [1688.0, 978.0]
        );
        assert_eq!(
            ui.resolve_layout_in(child, [2560.0, 1440.0])
                .unwrap()
                .position,
            [2328.0, 1338.0]
        );
    }
    #[test]
    fn stretch_respects_margins_and_clamps_small_windows() {
        assert_eq!(
            UiAnchor::Stretch.resolve([800.0, 600.0], [20.0, 30.0], [40.0, 50.0]),
            ([20.0, 30.0], [740.0, 520.0])
        );
        assert_eq!(
            UiAnchor::Stretch
                .resolve([10.0, 10.0], [20.0, 30.0], [40.0, 50.0])
                .1,
            [0.0, 0.0]
        );
    }
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    #[test]
    fn script_and_authored_controls_share_lookup_and_properties() {
        let mut ui = RuntimeUi::new();
        ui.document_key = "game_hud".into();
        let id = ui.allocate("Text").unwrap();
        ui.set_property(id, "key", Value::String("health_text".into()))
            .unwrap();
        assert_eq!(ui.find(None, "health_text"), Some(id));
        assert_eq!(ui.find(Some("game_hud"), "health_text"), Some(id));
        assert_eq!(ui.find(Some("other"), "health_text"), None);
        let duplicate = ui.allocate("Text").unwrap();
        assert!(
            ui.set_property(duplicate, "key", Value::String("health_text".into()))
                .is_err()
        );
        ui.set_property(id, "text", Value::String("75 / 100".into()))
            .unwrap();
        assert!(
            matches!(ui.get_property(id, "text").unwrap(), Some(Value::String(text)) if text == "75 / 100")
        );
        ui.delete(id).unwrap();
        assert_eq!(ui.find(None, "health_text"), None);
        ui.clear();
        assert_eq!(ui.find(None, "health_text"), None);
    }
}

#[cfg(test)]
mod symbolic_event_binding_tests {
    use super::*;

    #[test]
    fn symbolic_event_binding_is_separate_from_runtime_closure_property() {
        let mut ui = RuntimeUi::new();
        ui.document_key = "game_hud".to_string();
        let button = ui.allocate("Button").unwrap();
        ui.set_property(button, "key", Value::String("accept".to_string()))
            .unwrap();
        ui.set_symbolic_event_binding(
            button,
            "on_click",
            Some(UiSymbolicEventBinding {
                module_key: "dialogue".to_string(),
                handler: "accept_dialogue".to_string(),
            }),
        )
        .unwrap();

        assert_eq!(
            ui.get_property(button, "on_click").unwrap(),
            Some(Value::Nil)
        );
        let dispatch = ui.symbolic_event_dispatch(button, "on_click").unwrap();
        assert_eq!(dispatch.document_key, "game_hud");
        assert_eq!(dispatch.element_key, "accept");
        assert_eq!(dispatch.element_type, "Button");
        assert_eq!(dispatch.module_key, "dialogue");
        assert_eq!(dispatch.handler, "accept_dialogue");
    }

    #[test]
    fn deleting_ui_removes_symbolic_event_binding() {
        let mut ui = RuntimeUi::new();
        let button = ui.allocate("Button").unwrap();
        ui.set_symbolic_event_binding(
            button,
            "on_click",
            Some(UiSymbolicEventBinding {
                module_key: "menu".to_string(),
                handler: "close".to_string(),
            }),
        )
        .unwrap();
        ui.delete(button).unwrap();
        assert!(ui.symbolic_event_dispatch(button, "on_click").is_none());
    }
}

#[cfg(test)]
mod wo52_media_contract_tests {
    use super::*;
    use crate::scripting::value::Value;

    #[test]
    fn media_ui_handles_support_images_videos_and_reject_wrong_types() {
        let mut ui = RuntimeUi::new();
        let image = ui.allocate("Image").unwrap();
        let video = ui.allocate("Video").unwrap();
        ui.set_property(image, "source", Value::String("textures/grass.png".into()))
            .unwrap();
        ui.set_property(video, "source", Value::String("Media/intro.mp4".into()))
            .unwrap();
        ui.set_property(image, "fit", Value::String("Cover".into()))
            .unwrap();
        ui.set_property(video, "playing", Value::Bool(true))
            .unwrap();
        ui.set_property(video, "looped", Value::Bool(true)).unwrap();
        assert_eq!(
            ui.get_property(image, "fit").unwrap(),
            Some(Value::String("Cover".into()))
        );
        assert_eq!(
            ui.get_property(video, "playing").unwrap(),
            Some(Value::Bool(true))
        );
        assert!(
            ui.set_property(image, "playing", Value::Bool(true))
                .is_err()
        );
        assert!(
            ui.set_property(video, "source", Value::String("textures/grass.png".into()))
                .is_err()
        );
        assert!(
            ui.set_property(image, "source", Value::String("Media/intro.mp4".into()))
                .is_err()
        );
        assert!(
            ui.set_property(video, "playback_rate", Value::Number(0.0))
                .is_err()
        );
    }
}
