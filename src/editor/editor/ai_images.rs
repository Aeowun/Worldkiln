// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::ai_provider::{Config, Provider};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Cursor, Read},
    path::Path,
};

pub(super) const MAX_IMAGES: usize = 4;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Attachment {
    pub name: String,
    pub mime: String,
    pub data: String,
    #[serde(skip)]
    pub thumbnail: Option<egui::TextureHandle>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageSupport {
    Supported,
    Unsupported,
    Unknown,
}
impl ImageSupport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Supported => "Images supported",
            Self::Unsupported => "Images unsupported",
            Self::Unknown => "Image support unknown",
        }
    }
}
pub(super) fn support(config: &Config) -> ImageSupport {
    let base = config.base_url.trim_end_matches('/');
    match (config.provider, base, config.model.as_str()) {
        (
            Provider::OpenAi,
            "https://api.openai.com/v1",
            "gpt-4o" | "gpt-4o-mini" | "gpt-4.1" | "gpt-4.1-mini" | "gpt-4.1-nano" | "gpt-5"
            | "gpt-5-mini" | "gpt-5-nano",
        ) => ImageSupport::Supported,
        (
            Provider::Gemini,
            "https://generativelanguage.googleapis.com/v1beta",
            "gemini-2.0-flash" | "gemini-2.5-flash" | "gemini-2.5-pro" | "gemini-2.5-flash-lite",
        ) => ImageSupport::Supported,
        (Provider::OpenAi, "https://api.openai.com/v1", "gpt-3.5-turbo" | "gpt-4") => {
            ImageSupport::Unsupported
        }
        _ => ImageSupport::Unknown,
    }
}
impl Attachment {
    pub fn from_path(ctx: &egui::Context, path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        Self::from_bytes(
            ctx,
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
            &bytes,
        )
    }
    pub fn from_bytes(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BYTES {
            return Err("Each image must be at most 2 MiB.".into());
        }
        let format = image::guess_format(bytes).map_err(|_| "Choose a PNG or JPEG image.")?;
        if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
            return Err("Choose a PNG or JPEG image.".into());
        }
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(MAX_PIXELS * 8);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|e| format!("Image could not be decoded: {e}"))?;
        let thumb = decoded.thumbnail(80, 60).to_rgba8();
        let mut clean = Cursor::new(Vec::new());
        decoded
            .write_to(&mut clean, format)
            .map_err(|e| e.to_string())?;
        if clean.get_ref().len() > MAX_BYTES {
            return Err("Decoded image exceeds 2 MiB; resize it first.".into());
        }
        Ok(Self {
            name: name.chars().take(120).collect(),
            mime: if format == image::ImageFormat::Png {
                "image/png"
            } else {
                "image/jpeg"
            }
            .into(),
            data: STANDARD.encode(clean.into_inner()),
            thumbnail: Some(ctx.load_texture(
                "AI image attachment",
                egui::ColorImage::from_rgba_unmultiplied(
                    [thumb.width() as usize, thumb.height() as usize],
                    &thumb,
                ),
                egui::TextureOptions::LINEAR,
            )),
        })
    }
    pub fn clipboard(ctx: &egui::Context) -> Result<Self, String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let image = clipboard.get_image().map_err(|e| e.to_string())?;
        if image.width > 4096 || image.height > 4096 || image.bytes.len() as u64 > MAX_PIXELS * 4 {
            return Err("Clipboard image exceeds 4096 × 4096.".into());
        }
        let image = image::RgbaImage::from_raw(
            image.width as u32,
            image.height as u32,
            image.bytes.into_owned(),
        )
        .ok_or("Invalid clipboard image.")?;
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Self::from_bytes(ctx, "Pasted image.png", png.get_ref())
    }
}

pub(super) fn inject(body: &mut Value, provider: Provider, turn: usize, images: &[Attachment]) {
    if images.is_empty() {
        return;
    }
    match provider {
        Provider::Gemini => {
            if let Some(parts) = body["contents"][turn]["parts"].as_array_mut() {
                parts.extend(
                    images
                        .iter()
                        .map(|im| json!({"inlineData":{"mimeType":im.mime,"data":im.data}})),
                );
            }
        }
        _ => {
            let (key, index) = if provider == Provider::OpenAi {
                ("input", turn)
            } else {
                ("messages", turn + 1)
            };
            let text = body[key][index]["content"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let mut parts = vec![if provider == Provider::OpenAi {
                json!({"type":"input_text","text":text})
            } else {
                json!({"type":"text","text":text})
            }];
            parts.extend(images.iter().map(|im| {
                let url = format!("data:{};base64,{}", im.mime, im.data);
                if provider == Provider::OpenAi {
                    json!({"type":"input_image","image_url":url})
                } else {
                    json!({"type":"image_url","image_url":{"url":url}})
                }
            }));
            body[key][index]["content"] = json!(parts);
        }
    }
}

pub(super) fn validate(config: &Config, images: &[(usize, Vec<Attachment>)]) -> Result<(), String> {
    let count: usize = images
        .iter()
        .map(|(_, attachments)| attachments.len())
        .sum();
    if count == 0 {
        return Ok(());
    }
    if support(config) != ImageSupport::Supported {
        return Err("This model has no verified image support.".into());
    }
    if count > MAX_IMAGES {
        return Err("Attach at most four images per request.".into());
    }
    for (_, attachments) in images {
        for attachment in attachments {
            if attachment.data.len() > MAX_BYTES.div_ceil(3) * 4 {
                return Err("Image exceeds 2 MiB.".into());
            }
            let bytes = STANDARD
                .decode(&attachment.data)
                .map_err(|_| "Invalid image encoding.")?;
            let format = image::guess_format(&bytes).map_err(|_| "Invalid image format.")?;
            if !matches!(
                (attachment.mime.as_str(), format),
                ("image/png", image::ImageFormat::Png) | ("image/jpeg", image::ImageFormat::Jpeg)
            ) {
                return Err("Image type does not match its content.".into());
            }
            let (width, height) = image::ImageReader::with_format(Cursor::new(bytes), format)
                .into_dimensions()
                .map_err(|_| "Invalid image dimensions.")?;
            if width > 4096 || height > 4096 {
                return Err("Image exceeds 4096 × 4096.".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_v2_image_payloads_are_bounded_and_provider_specific() {
        let ctx = egui::Context::default();
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2, 2)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let image = Attachment::from_bytes(&ctx, "reference.png", png.get_ref()).unwrap();
        assert!(Attachment::from_bytes(&ctx, "bad.png", b"not an image").is_err());
        assert!(Attachment::from_bytes(&ctx, "large.png", &vec![0; MAX_BYTES + 1]).is_err());
        for provider in [Provider::OpenAi, Provider::Gemini] {
            let config = Config {
                provider,
                base_url: provider.default_url().into(),
                key: String::new(),
                model: if provider == Provider::OpenAi {
                    "gpt-4.1"
                } else {
                    "gemini-2.5-flash"
                }
                .into(),
            };
            assert!(validate(&config, &[(0, vec![image.clone()])]).is_ok());
            let mut body = super::super::ai_provider::chat_payload(
                provider,
                &config.model,
                &[("user".into(), "Describe".into())],
            );
            inject(&mut body, provider, 0, &[image.clone()]);
            if provider == Provider::OpenAi {
                assert_eq!(body["input"][0]["content"][0]["text"], "Describe");
                assert!(
                    body["input"][0]["content"][1]["image_url"]
                        .as_str()
                        .unwrap()
                        .starts_with("data:image/png;base64,")
                );
            } else {
                assert_eq!(body["contents"][0]["parts"][0]["text"], "Describe");
                assert_eq!(
                    body["contents"][0]["parts"][1]["inlineData"]["mimeType"],
                    "image/png"
                );
            }
            let unknown = Config {
                base_url: "https://custom.example/v1".into(),
                ..config.clone()
            };
            assert!(validate(&unknown, &[(0, vec![image.clone()])]).is_err());
            assert!(validate(&config, &[(0, vec![image.clone(); 5])]).is_err());
        }
        let archived = serde_json::to_string(&image).unwrap();
        let restored: Attachment = serde_json::from_str(&archived).unwrap();
        assert_eq!(restored.data, image.data);
        assert!(restored.thumbnail.is_none());
    }
}
