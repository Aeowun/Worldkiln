// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::world::terrain::{
    HeightfieldData, TerrainData, TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
};
use image::{DynamicImage, GenericImageView};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageHeightmapMode {
    Grayscale,
    Illustrated,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageHeightmapSettings {
    pub mode: ImageHeightmapMode,
    pub width: u32,
    pub depth: u32,
    pub origin_x: i32,
    pub origin_z: i32,
    pub min_height: i32,
    pub max_height: i32,
    pub sea_level: f32,
    pub vertical_scale: f32,
    pub noise_reduction: u8,
}
impl Default for ImageHeightmapSettings {
    fn default() -> Self {
        Self {
            mode: ImageHeightmapMode::Grayscale,
            width: 256,
            depth: 256,
            origin_x: 0,
            origin_z: 0,
            min_height: -24,
            max_height: 96,
            sea_level: 0.0,
            vertical_scale: 1.0,
            noise_reduction: 45,
        }
    }
}

pub struct ImageHeightmapSource {
    image: DynamicImage,
    pub width: u32,
    pub height: u32,
    pub is_16_bit: bool,
}
impl ImageHeightmapSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        let extension = path
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or_default();
        if !["png", "jpg", "jpeg"].contains(&extension.to_ascii_lowercase().as_str()) {
            return Err("Choose a PNG or JPEG terrain image".into());
        }
        let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if len > 96 * 1024 * 1024 {
            return Err("Image exceeds the 96 MB source limit".into());
        }
        let (width, height) = image::image_dimensions(path).map_err(|e| e.to_string())?;
        if width < 2
            || height < 2
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 16_777_216
        {
            return Err(
                "Image dimensions must be 2–4096 on each axis (up to 16 million pixels)".into(),
            );
        }
        let image = image::open(path).map_err(|e| format!("Cannot decode image: {e}"))?;
        let is_16_bit = matches!(
            &image,
            DynamicImage::ImageLuma16(_)
                | DynamicImage::ImageLumaA16(_)
                | DynamicImage::ImageRgb16(_)
                | DynamicImage::ImageRgba16(_)
        );
        Ok(Self {
            image,
            width,
            height,
            is_16_bit,
        })
    }

    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let x = u.clamp(0.0, 1.0) * (self.width - 1) as f32;
        let z = v.clamp(0.0, 1.0) * (self.height - 1) as f32;
        let x0 = x.floor() as u32;
        let z0 = z.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let z1 = (z0 + 1).min(self.height - 1);
        let tx = x - x0 as f32;
        let tz = z - z0 as f32;
        let sample_one = |sx, sz| -> [f32; 4] {
            match &self.image {
                DynamicImage::ImageLuma16(im) => {
                    let f = im.get_pixel(sx, sz)[0] as f32 / 65535.0;
                    [f, f, f, 1.0]
                }
                DynamicImage::ImageLumaA16(im) => {
                    let p = im.get_pixel(sx, sz);
                    let f = p[0] as f32 / 65535.0;
                    [f, f, f, p[1] as f32 / 65535.0]
                }
                DynamicImage::ImageRgb16(im) => {
                    let p = im.get_pixel(sx, sz);
                    [
                        p[0] as f32 / 65535.0,
                        p[1] as f32 / 65535.0,
                        p[2] as f32 / 65535.0,
                        1.0,
                    ]
                }
                DynamicImage::ImageRgba16(im) => {
                    let p = im.get_pixel(sx, sz);
                    [
                        p[0] as f32 / 65535.0,
                        p[1] as f32 / 65535.0,
                        p[2] as f32 / 65535.0,
                        p[3] as f32 / 65535.0,
                    ]
                }
                _ => {
                    let p = self.image.get_pixel(sx, sz);
                    [
                        p[0] as f32 / 255.0,
                        p[1] as f32 / 255.0,
                        p[2] as f32 / 255.0,
                        p[3] as f32 / 255.0,
                    ]
                }
            }
        };
        let a = sample_one(x0, z0);
        let b = sample_one(x1, z0);
        let c = sample_one(x0, z1);
        let d = sample_one(x1, z1);
        std::array::from_fn(|i| {
            (a[i] * (1.0 - tx) + b[i] * tx) * (1.0 - tz) + (c[i] * (1.0 - tx) + d[i] * tx) * tz
        })
    }

    pub fn is_grayscale(&self) -> bool {
        matches!(
            &self.image,
            DynamicImage::ImageLuma8(_)
                | DynamicImage::ImageLumaA8(_)
                | DynamicImage::ImageLuma16(_)
                | DynamicImage::ImageLumaA16(_)
        )
    }

    pub fn preview_rgba(&self, size: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(size as usize * size as usize * 4);
        for z in 0..size {
            for x in 0..size {
                let [r, g, b, a] =
                    self.sample(x as f32 / (size - 1) as f32, z as f32 / (size - 1) as f32);
                for channel in [r, g, b, a] {
                    bytes.push((channel * 255.0).round() as u8);
                }
            }
        }
        bytes
    }

    pub fn into_terrain(&self, settings: &ImageHeightmapSettings) -> Result<TerrainResult, String> {
        if !settings.vertical_scale.is_finite() || !(0.01..=16.0).contains(&settings.vertical_scale)
        {
            return Err("Heightmap Y scale must be between 0.01 and 16".into());
        }
        if settings.noise_reduction > 100 {
            return Err("Noise reduction must be between 0% and 100%".into());
        }
        if settings.width < 2
            || settings.depth < 2
            || settings.width > i32::MAX as u32
            || settings.depth > i32::MAX as u32
        {
            return Err("Landscape width and depth must fit positive i32 coordinates".into());
        }
        if settings.max_height <= settings.min_height
            || !settings.sea_level.is_finite()
            || settings.sea_level < settings.min_height as f32
            || settings.sea_level > settings.max_height as f32
        {
            return Err("Require minimum < maximum and sea level within this interval".into());
        }
        let range = settings
            .max_height
            .checked_sub(settings.min_height)
            .and_then(|v| v.checked_add(1))
            .ok_or("Invalid elevation range")?;
        let request = TerrainGenerationRequest {
            position: [settings.origin_x, settings.min_height, settings.origin_z],
            size: [settings.width as i32, range, settings.depth as i32],
        };
        request.validate().map_err(|e| e.to_string())?;
        let count = (settings.width as usize)
            .checked_mul(settings.depth as usize)
            .ok_or("Landscape sample count overflow")?;
        let mut heights = Vec::new();
        heights
            .try_reserve_exact(count)
            .map_err(|e| format!("Cannot allocate landscape height samples: {e}"))?;
        for z in 0..settings.depth {
            let v = z as f32 / (settings.depth - 1) as f32;
            for x in 0..settings.width {
                let u = x as f32 / (settings.width - 1) as f32;
                let [r, g, b, a] = self.sample(u, v);
                let brightness = (r * 0.2126 + g * 0.7152 + b * 0.0722).clamp(0.0, 1.0);
                let height = match settings.mode {
                    ImageHeightmapMode::Grayscale => {
                        settings.min_height as f32
                            + brightness * (settings.max_height - settings.min_height) as f32
                    }
                    ImageHeightmapMode::Illustrated => {
                        let water =
                            a < 0.4 || (b > 0.23 && b > r * 1.20 && (b > g * 1.07 || g > r * 1.28));
                        if water {
                            settings.sea_level
                                - (settings.sea_level - settings.min_height as f32)
                                    * (0.12 + 0.78 * (1.0 - brightness))
                        } else {
                            let relief = if r > 0.65 && g > 0.65 && b > 0.60 {
                                0.58 + 0.42 * brightness
                            } else {
                                0.05 + 0.55 * brightness
                            };
                            settings.sea_level
                                + (settings.max_height as f32 - settings.sea_level) * relief
                        }
                    }
                };
                let scaled = if settings.vertical_scale == 1.0 {
                    height
                } else {
                    settings.sea_level + (height - settings.sea_level) * settings.vertical_scale
                };
                heights.push(scaled.clamp(settings.min_height as f32, settings.max_height as f32));
            }
        }
        Ok(TerrainResult {
            request,
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: request.position,
                size: [settings.width, range as u32, settings.depth],
                heights,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(img: DynamicImage) -> ImageHeightmapSource {
        let (width, height) = img.dimensions();
        ImageHeightmapSource {
            image: img,
            width,
            height,
            is_16_bit: false,
        }
    }
    #[test]
    fn grayscale_samples_are_unchanged_at_every_strength() {
        let img =
            image::GrayImage::from_raw(3, 3, vec![0, 255, 128, 64, 230, 2, 90, 180, 45]).unwrap();
        let src = source(DynamicImage::ImageLuma8(img));
        let low = src
            .into_terrain(&ImageHeightmapSettings {
                width: 3,
                depth: 3,
                noise_reduction: 0,
                ..Default::default()
            })
            .unwrap();
        let high = src
            .into_terrain(&ImageHeightmapSettings {
                width: 3,
                depth: 3,
                noise_reduction: 100,
                ..Default::default()
            })
            .unwrap();
        let (TerrainData::Heightfield(a), TerrainData::Heightfield(b)) = (low.data, high.data)
        else {
            panic!()
        };
        assert_eq!(a.heights, b.heights);
        assert_eq!(a.heights[1], 96.0);
        assert!((a.heights[4] - (-24.0 + 120.0 * 230.0 / 255.0)).abs() < 0.0001);
    }
    #[test]
    fn preserves_sixteen_bit_precision() {
        let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(
            2,
            2,
            vec![0, 1, 32768, 65535],
        )
        .unwrap();
        let src = source(DynamicImage::ImageLuma16(img));
        let result = src
            .into_terrain(&ImageHeightmapSettings {
                width: 2,
                depth: 2,
                min_height: 0,
                max_height: 10000,
                noise_reduction: 0,
                ..Default::default()
            })
            .unwrap();
        let TerrainData::Heightfield(h) = result.data else {
            panic!()
        };
        assert!(h.heights[1] > 0.1 && h.heights[1] < 0.2);
        assert!((h.heights[2] - 5000.0).abs() < 1.0);
    }
    #[test]
    fn illustrated_blue_sea_is_below_shore() {
        let img = image::RgbImage::from_raw(
            2,
            2,
            vec![10, 80, 210, 220, 190, 120, 20, 150, 30, 245, 230, 180],
        )
        .unwrap();
        let result = source(DynamicImage::ImageRgb8(img))
            .into_terrain(&ImageHeightmapSettings {
                mode: ImageHeightmapMode::Illustrated,
                width: 2,
                depth: 2,
                ..Default::default()
            })
            .unwrap();
        let TerrainData::Heightfield(h) = result.data else {
            panic!()
        };
        assert!(h.heights[0] < 0.0);
        assert!(h.heights[1] > 0.0);
    }
    #[test]
    fn heightmap_y_scale_compresses_source_relief() {
        let img = image::GrayImage::from_raw(2, 2, vec![0, 255, 128, 64]).unwrap();
        let src = source(DynamicImage::ImageLuma8(img));
        let settings = ImageHeightmapSettings {
            width: 2,
            depth: 2,
            min_height: -100,
            max_height: 100,
            sea_level: 0.0,
            vertical_scale: 0.25,
            ..Default::default()
        };
        let result = src.into_terrain(&settings).unwrap();
        let TerrainData::Heightfield(field) = result.data else {
            panic!()
        };
        assert!((field.heights[0] + 25.0).abs() < 0.001);
        assert!((field.heights[1] - 25.0).abs() < 0.001);
        assert_eq!(field.heights.len(), 4);
    }
    #[test]
    fn accepts_landscape_import_wider_than_old_limit() {
        let src = source(DynamicImage::ImageLuma8(image::GrayImage::new(2, 2)));
        let result = src
            .into_terrain(&ImageHeightmapSettings {
                width: 513,
                depth: 2,
                ..Default::default()
            })
            .unwrap();
        let TerrainData::Heightfield(h) = result.data else {
            panic!()
        };
        assert_eq!(h.size[0], 513);
        assert_eq!(h.heights.len(), 1026);
    }
}
