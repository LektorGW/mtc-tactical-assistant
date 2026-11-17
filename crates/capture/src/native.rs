use super::*;
use std::ffi::c_void;
use windows::core::BOOL;
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::{Dwm::*, Gdi::*},
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

pub(crate) fn hwnd(handle: i64) -> HWND {
    HWND(handle as *mut c_void)
}

/// Avoids logical/physical pixel mismatches without changing the application's DPI mode.
pub(crate) struct DpiScope(DPI_AWARENESS_CONTEXT);

impl DpiScope {
    pub(crate) fn enter() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DpiScope {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }
}

pub(crate) fn validate_window(handle: i64) -> Result<(), CaptureError> {
    unsafe {
        let window = hwnd(handle);
        if !IsWindow(Some(window)).as_bool() {
            return Err(CaptureError::Closed);
        }
        if IsIconic(window).as_bool() {
            return Err(CaptureError::Minimized);
        }
        if !IsWindowVisible(window).as_bool() {
            return Err(CaptureError::Hidden);
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return Err(CaptureError::Hidden);
        }
        Ok(())
    }
}

unsafe extern "system" fn collect(window: HWND, lparam: LPARAM) -> BOOL {
    if validate_window(window.0 as i64).is_err() {
        return BOOL(1);
    }
    let len = GetWindowTextLengthW(window);
    if len == 0 {
        return BOOL(1);
    }
    let mut title = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(window, &mut title);
    let title = String::from_utf16_lossy(&title[..n as usize]);
    if title.starts_with("MTC Tactical") {
        return BOOL(1);
    }
    let windows = &mut *(lparam.0 as *mut Vec<CaptureWindow>);
    windows.push(CaptureWindow {
        handle: window.0 as i64,
        title,
    });
    BOOL(1)
}

pub fn list_windows() -> Result<Vec<CaptureWindow>, String> {
    let mut windows = Vec::new();
    unsafe {
        EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize))
            .map_err(|e| e.to_string())?;
    }
    Ok(windows)
}

pub(crate) fn client_bounds(handle: i64) -> Result<(POINT, u32, u32), CaptureError> {
    validate_window(handle)?;
    let _dpi = DpiScope::enter();
    unsafe {
        let window = hwnd(handle);
        let mut rect = RECT::default();
        GetClientRect(window, &mut rect).map_err(|e| CaptureError::Backend(e.to_string()))?;
        let mut origin = POINT { x: 0, y: 0 };
        if !ClientToScreen(window, &mut origin).as_bool() {
            return Err(CaptureError::Backend("Brak pozycji okna".into()));
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 || width > 7680 || height > 4320 {
            return Err(CaptureError::InvalidSize);
        }
        Ok((origin, width as u32, height as u32))
    }
}

/// Mouse cursor position in physical client pixels of `handle` (the same pixel
/// space as captured frames), and whether it lies inside the client area.
/// Read-only: the cursor is never moved and no input is sent.
pub fn cursor_client_position(handle: i64) -> Result<(i32, i32, bool), CaptureError> {
    let (_, width, height) = client_bounds(handle)?;
    let _dpi = DpiScope::enter();
    unsafe {
        let mut point = POINT::default();
        GetCursorPos(&mut point).map_err(|e| CaptureError::Backend(e.to_string()))?;
        if !ScreenToClient(hwnd(handle), &mut point).as_bool() {
            return Err(CaptureError::Backend(
                "Brak pozycji kursora względem okna".into(),
            ));
        }
        let inside =
            point.x >= 0 && point.y >= 0 && (point.x as u32) < width && (point.y as u32) < height;
        Ok((point.x, point.y, inside))
    }
}

/// Explicit GDI fallback: captures screen pixels under the visible client area.
/// Occluding windows are intentionally preserved. No PrintWindow or hooks are used.
pub fn capture_window(handle: i64) -> Result<RgbaImage, String> {
    capture_gdi(handle).map_err(|e| e.to_string())
}

pub(crate) fn capture_gdi(handle: i64) -> Result<RgbaImage, CaptureError> {
    let _dpi = DpiScope::enter();
    let (origin, width, height) = client_bounds(handle)?;
    unsafe {
        let screen = GetDC(None);
        if screen.0.is_null() {
            return Err(CaptureError::Backend("GetDC nie powiodło się".into()));
        }
        let memory = CreateCompatibleDC(Some(screen));
        if memory.0.is_null() {
            ReleaseDC(None, screen);
            return Err(CaptureError::Backend(
                "CreateCompatibleDC nie powiodło się".into(),
            ));
        }
        let bitmap = CreateCompatibleBitmap(screen, width as i32, height as i32);
        if bitmap.0.is_null() {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return Err(CaptureError::Backend(
                "CreateCompatibleBitmap nie powiodło się".into(),
            ));
        }
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let copied = BitBlt(
            memory,
            0,
            0,
            width as i32,
            height as i32,
            Some(screen),
            origin.x,
            origin.y,
            SRCCOPY,
        );
        SelectObject(memory, previous);
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width as i32;
        info.bmiHeader.biHeight = -(height as i32);
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let rows = if copied.is_ok() {
            GetDIBits(
                memory,
                bitmap,
                0,
                height,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        copied.map_err(|e| CaptureError::Backend(e.to_string()))?;
        if rows != height as i32 {
            return Err(CaptureError::Backend(
                "Nie udało się odczytać klatki".into(),
            ));
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
            pixel[3] = 255;
        }
        RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| CaptureError::Backend("Niepoprawna klatka".into()))
    }
}

/// Matches the actual WGC content size to physical window bounds before cropping.
/// During resize an old surface must never be interpreted using a new client geometry.
pub(crate) fn client_crop(handle: i64, width: u32, height: u32) -> Result<CropRect, CaptureError> {
    let _dpi = DpiScope::enter();
    let (client, client_width, client_height) = client_bounds(handle)?;
    unsafe {
        let mut visible = RECT::default();
        let has_visible = DwmGetWindowAttribute(
            hwnd(handle),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut visible as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        let mut full = RECT::default();
        GetWindowRect(hwnd(handle), &mut full).map_err(|e| CaptureError::Backend(e.to_string()))?;
        let source = [has_visible.then_some(visible), Some(full)]
            .into_iter()
            .flatten()
            .find(|rect| {
                rect.right - rect.left == width as i32 && rect.bottom - rect.top == height as i32
            });
        let (left, top) = if let Some(source) = source {
            (client.x - source.left, client.y - source.top)
        } else if client_width == width && client_height == height {
            (0, 0)
        } else {
            return Err(CaptureError::GeometryChanged);
        };
        let crop = CropRect {
            x: left,
            y: top,
            width: client_width,
            height: client_height,
        };
        crop.validate(width, height)?;
        Ok(crop)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CropRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub(crate) fn validate(
        &self,
        source_width: u32,
        source_height: u32,
    ) -> Result<(), CaptureError> {
        if self.x < 0
            || self.y < 0
            || self.width == 0
            || self.height == 0
            || self.x as u64 + self.width as u64 > source_width as u64
            || self.y as u64 + self.height as u64 > source_height as u64
        {
            return Err(CaptureError::GeometryChanged);
        }
        Ok(())
    }
}
