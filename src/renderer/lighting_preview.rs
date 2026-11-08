// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::Renderer;

#[derive(Default)]
pub(super) struct LightingPreviewCapture {
    resolve_fbo: u32,
    resolve_color: u32,
    resolve_size: (i32, i32),
    thumbnail_fbo: u32,
    thumbnail_color: u32,
    thumbnail_size: (i32, i32),
    logged_failure: bool,
}

impl LightingPreviewCapture {
    unsafe fn color_fbo(width: i32, height: i32) -> Option<(u32, u32)> {
        let (mut fbo, mut color) = (0, 0);
        gl::GenFramebuffers(1, &mut fbo);
        gl::GenTextures(1, &mut color);
        gl::BindTexture(gl::TEXTURE_2D, color);
        gl::TexImage2D(
            gl::TEXTURE_2D,
            0,
            gl::RGBA8 as i32,
            width,
            height,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            std::ptr::null(),
        );
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
        gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
        gl::FramebufferTexture2D(
            gl::FRAMEBUFFER,
            gl::COLOR_ATTACHMENT0,
            gl::TEXTURE_2D,
            color,
            0,
        );
        if gl::CheckFramebufferStatus(gl::FRAMEBUFFER) != gl::FRAMEBUFFER_COMPLETE {
            gl::DeleteFramebuffers(1, &fbo);
            gl::DeleteTextures(1, &color);
            return None;
        }
        Some((fbo, color))
    }

    pub(super) unsafe fn capture(
        &mut self,
        source_w: i32,
        source_h: i32,
        target_w: i32,
        target_h: i32,
    ) -> Option<egui::ColorImage> {
        if source_w < 2 || source_h < 2 || target_w < 2 || target_h < 2 {
            return None;
        }
        let mut previous_read_fbo = 0;
        let mut previous_draw_fbo = 0;
        let mut previous_texture = 0;
        let mut previous_active_texture = 0;
        let mut previous_pack = 0;
        gl::GetIntegerv(gl::READ_FRAMEBUFFER_BINDING, &mut previous_read_fbo);
        gl::GetIntegerv(gl::DRAW_FRAMEBUFFER_BINDING, &mut previous_draw_fbo);
        gl::GetIntegerv(gl::ACTIVE_TEXTURE, &mut previous_active_texture);
        gl::GetIntegerv(gl::PACK_ALIGNMENT, &mut previous_pack);
        let scissor_was_enabled = gl::IsEnabled(gl::SCISSOR_TEST) == gl::TRUE;
        gl::Disable(gl::SCISSOR_TEST);
        gl::ActiveTexture(gl::TEXTURE0);
        gl::GetIntegerv(gl::TEXTURE_BINDING_2D, &mut previous_texture);
        let result = self.capture_inner(source_w, source_h, target_w, target_h);
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, previous_read_fbo as u32);
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, previous_draw_fbo as u32);
        gl::BindTexture(gl::TEXTURE_2D, previous_texture as u32);
        gl::ActiveTexture(previous_active_texture as u32);
        gl::PixelStorei(gl::PACK_ALIGNMENT, previous_pack);
        if scissor_was_enabled {
            gl::Enable(gl::SCISSOR_TEST);
        }
        result
    }

    unsafe fn capture_inner(
        &mut self,
        source_w: i32,
        source_h: i32,
        target_w: i32,
        target_h: i32,
    ) -> Option<egui::ColorImage> {
        if self.resolve_size != (source_w, source_h) || self.resolve_fbo == 0 {
            Self::release(&mut self.resolve_fbo, &mut self.resolve_color);
            let (fbo, color) = Self::color_fbo(source_w, source_h)?;
            self.resolve_fbo = fbo;
            self.resolve_color = color;
            self.resolve_size = (source_w, source_h);
        }
        if self.thumbnail_size != (target_w, target_h) || self.thumbnail_fbo == 0 {
            Self::release(&mut self.thumbnail_fbo, &mut self.thumbnail_color);
            let (fbo, color) = Self::color_fbo(target_w, target_h)?;
            self.thumbnail_fbo = fbo;
            self.thumbnail_color = color;
            self.thumbnail_size = (target_w, target_h);
        }
        for _ in 0..4 {
            if gl::GetError() == gl::NO_ERROR {
                break;
            }
        }
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, 0);
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.resolve_fbo);
        gl::BlitFramebuffer(
            0,
            0,
            source_w,
            source_h,
            0,
            0,
            source_w,
            source_h,
            gl::COLOR_BUFFER_BIT,
            gl::NEAREST,
        );

        let source_aspect = source_w as f32 / source_h as f32;
        let target_aspect = target_w as f32 / target_h as f32;
        let (crop_w, crop_h) = if source_aspect > target_aspect {
            (
                ((source_h as f32 * target_aspect).round() as i32).clamp(1, source_w),
                source_h,
            )
        } else {
            (
                source_w,
                ((source_w as f32 / target_aspect).round() as i32).clamp(1, source_h),
            )
        };
        let crop_x = (source_w - crop_w) / 2;
        let crop_y = (source_h - crop_h) / 2;
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, self.resolve_fbo);
        gl::BindFramebuffer(gl::DRAW_FRAMEBUFFER, self.thumbnail_fbo);
        gl::BlitFramebuffer(
            crop_x,
            crop_y,
            crop_x + crop_w,
            crop_y + crop_h,
            0,
            0,
            target_w,
            target_h,
            gl::COLOR_BUFFER_BIT,
            gl::LINEAR,
        );
        gl::BindFramebuffer(gl::READ_FRAMEBUFFER, self.thumbnail_fbo);
        gl::ReadBuffer(gl::COLOR_ATTACHMENT0);
        gl::PixelStorei(gl::PACK_ALIGNMENT, 1);
        let row_bytes = (target_w as usize) * 4;
        let mut pixels = vec![0u8; row_bytes * target_h as usize];
        gl::ReadPixels(
            0,
            0,
            target_w,
            target_h,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            pixels.as_mut_ptr().cast(),
        );
        let error = gl::GetError();
        if error != gl::NO_ERROR {
            if !self.logged_failure {
                eprintln!(
                    "[Horizon Mirage] World Lighting preview GPU capture failed ({error:#x})"
                );
                self.logged_failure = true;
            }
            return None;
        }
        for y in 0..(target_h as usize / 2) {
            let bottom = (target_h as usize - 1 - y) * row_bytes;
            let top = y * row_bytes;
            for x in 0..row_bytes {
                pixels.swap(top + x, bottom + x);
            }
        }
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [target_w as usize, target_h as usize],
            &pixels,
        ))
    }

    unsafe fn release(fbo: &mut u32, color: &mut u32) {
        if *fbo != 0 {
            gl::DeleteFramebuffers(1, fbo);
            *fbo = 0;
        }
        if *color != 0 {
            gl::DeleteTextures(1, color);
            *color = 0;
        }
    }

    pub(super) fn free(&mut self) {
        unsafe {
            Self::release(&mut self.resolve_fbo, &mut self.resolve_color);
            Self::release(&mut self.thumbnail_fbo, &mut self.thumbnail_color);
        }
        self.resolve_size = (0, 0);
        self.thumbnail_size = (0, 0);
    }
}

impl Renderer {
    pub fn capture_lighting_preview(&self, width: i32, height: i32) -> Option<egui::ColorImage> {
        unsafe {
            self.lighting_preview.borrow_mut().capture(
                self.width.round() as i32,
                self.height.round() as i32,
                width,
                height,
            )
        }
    }
}
