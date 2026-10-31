// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use winit::window::{ResizeDirection, Window};

#[derive(Default)]
pub struct WindowShell {
    geometry: Option<(u32, u32, u64, bool)>,
    resize: Option<ResizeDirection>,
}

impl WindowShell {
    pub fn update(&mut self, window: &Window) {
        let size = window.inner_size();
        let geometry = (
            size.width,
            size.height,
            window.scale_factor().to_bits(),
            window.is_maximized(),
        );
        if size.width == 0 || size.height == 0 || self.geometry == Some(geometry) {
            return;
        }
        let applied = apply_region(window);
        self.geometry = Some(geometry);
        self.resize = None;
        if !applied {
            eprintln!("[Worldkiln shell] Native window region failed for {geometry:?}");
        } else if std::env::var_os("WORLDKILN_SHELL_DIAGNOSTICS").is_some() {
            eprintln!("[Worldkiln shell] Native region updated: {geometry:?}");
        }
    }

    pub fn cursor_moved(&mut self, window: &Window, position: winit::dpi::PhysicalPosition<f64>) {
        let size = window.inner_size();
        self.resize = if window.is_maximized() || window.fullscreen().is_some() {
            None
        } else {
            resize_direction(
                position.x,
                position.y,
                size.width as f64,
                size.height as f64,
                6.0 * window.scale_factor(),
            )
        };
    }

    pub fn resize_cursor(&self) -> Option<egui::CursorIcon> {
        Some(match self.resize? {
            ResizeDirection::North | ResizeDirection::South => egui::CursorIcon::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => egui::CursorIcon::ResizeHorizontal,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => egui::CursorIcon::ResizeNwSe,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => egui::CursorIcon::ResizeNeSw,
        })
    }

    pub fn begin_resize(&self, window: &Window) -> bool {
        self.resize
            .filter(|_| !window.is_maximized())
            .is_some_and(|direction| window.drag_resize_window(direction).is_ok())
    }
}

fn resize_direction(x: f64, y: f64, width: f64, height: f64, edge: f64) -> Option<ResizeDirection> {
    if x < 0.0 || y < 0.0 || x >= width || y >= height {
        return None;
    }
    use ResizeDirection::*;
    match (x < edge, x >= width - edge, y < edge, y >= height - edge) {
        (true, _, true, _) => Some(NorthWest),
        (_, true, true, _) => Some(NorthEast),
        (true, _, _, true) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, _, _, _) => Some(West),
        (_, true, _, _) => Some(East),
        (_, _, true, _) => Some(North),
        (_, _, _, true) => Some(South),
        _ => None,
    }
}

#[cfg(windows)]
fn apply_region(window: &Window) -> bool {
    use std::ffi::c_void;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetWindowRect(hwnd: *mut c_void, rect: *mut Rect) -> i32;
        fn SetWindowRgn(hwnd: *mut c_void, region: *mut c_void, redraw: i32) -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateRoundRectRgn(
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
            ellipse_width: i32,
            ellipse_height: i32,
        ) -> *mut c_void;
        fn DeleteObject(object: *mut c_void) -> i32;
    }
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return false;
    };
    let hwnd = handle.hwnd.get() as *mut c_void;
    unsafe {
        if window.is_maximized() || window.fullscreen().is_some() {
            return SetWindowRgn(hwnd, std::ptr::null_mut(), 1) != 0;
        }
        let mut rect = Rect::default();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return false;
        }
        let diameter = (28.0 * window.scale_factor()).round() as i32;
        let region = CreateRoundRectRgn(
            0,
            0,
            rect.right - rect.left + 1,
            rect.bottom - rect.top + 1,
            diameter,
            diameter,
        );
        if region.is_null() {
            return false;
        }
        if SetWindowRgn(hwnd, region, 1) == 0 {
            DeleteObject(region);
            return false;
        }
    }
    true
}

#[cfg(not(windows))]
fn apply_region(_: &Window) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_resize_edges_corners_and_scaled_interior() {
        assert_eq!(
            resize_direction(2.0, 2.0, 800.0, 600.0, 6.0),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_direction(798.0, 598.0, 800.0, 600.0, 6.0),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(
            resize_direction(400.0, 598.0, 800.0, 600.0, 6.0),
            Some(ResizeDirection::South)
        );
        assert_eq!(
            resize_direction(8.0, 400.0, 1600.0, 1200.0, 12.0),
            Some(ResizeDirection::West)
        );
        assert_eq!(resize_direction(40.0, 40.0, 800.0, 600.0, 6.0), None);
        assert_eq!(resize_direction(900.0, 40.0, 800.0, 600.0, 6.0), None);
    }
}
