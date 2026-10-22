//! Native window adjustments for the overlay panel.

use meetcue_core::MeetCueError;

/// Makes the overlay never take focus and hides it from Alt+Tab
/// (`WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`). `hwnd` is the native handle.
#[cfg(windows)]
pub fn make_non_activating(hwnd: isize) -> Result<(), MeetCueError> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    unsafe {
        let hwnd = HWND(hwnd as *mut core::ffi::c_void);
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let style = style | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
    }
    Ok(())
}

/// Excludes the window from screen capture and sharing (`WDA_EXCLUDEFROMCAPTURE`).
#[cfg(windows)]
pub fn exclude_from_capture(hwnd: isize, exclude: bool) -> Result<(), MeetCueError> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };
    let affinity = if exclude {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        WDA_NONE
    };
    unsafe { SetWindowDisplayAffinity(HWND(hwnd as *mut core::ffi::c_void), affinity) }
        .map_err(|e| MeetCueError::new(meetcue_core::codes::SYS_UNSUPPORTED, e.to_string()))
}

/// Captures a screen rectangle as 0xRRGGBB pixels using the same path screen
/// recorders use, so the self-test can check that the panel is hidden.
#[cfg(windows)]
pub fn capture_screen(x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u32>, MeetCueError> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT,
        DIB_RGB_COLORS, SRCCOPY,
    };
    let err = |m: &str| MeetCueError::new(meetcue_core::codes::SYS_UNSUPPORTED, m);
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut pixels = vec![0u32; (w * h) as usize];
        let lines = GetDIBits(
            mem,
            bmp,
            0,
            h as u32,
            Some(pixels.as_mut_ptr() as *mut core::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if ok.is_err() || lines == 0 {
            return Err(err("screen capture failed"));
        }
        Ok(pixels.into_iter().map(|p| p & 0x00FF_FFFF).collect())
    }
}

#[cfg(not(windows))]
pub fn make_non_activating(_hwnd: isize) -> Result<(), MeetCueError> {
    Ok(())
}

#[cfg(not(windows))]
pub fn exclude_from_capture(_hwnd: isize, _exclude: bool) -> Result<(), MeetCueError> {
    Ok(())
}

#[cfg(not(windows))]
pub fn capture_screen(_x: i32, _y: i32, _w: i32, _h: i32) -> Result<Vec<u32>, MeetCueError> {
    Err(MeetCueError::new(
        meetcue_core::codes::SYS_UNSUPPORTED,
        std::env::consts::OS,
    ))
}

/// Share of pixels within `tolerance` of `rgb`.
pub fn color_share(pixels: &[u32], rgb: u32, tolerance: u8) -> f64 {
    if pixels.is_empty() {
        return 0.0;
    }
    let close = |a: u32, b: u32, shift: u32| {
        let x = ((a >> shift) & 0xFF) as i32;
        let y = ((b >> shift) & 0xFF) as i32;
        (x - y).unsigned_abs() <= tolerance as u32
    };
    pixels
        .iter()
        .filter(|&&p| close(p, rgb, 16) && close(p, rgb, 8) && close(p, rgb, 0))
        .count() as f64
        / pixels.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_of_marker_color() {
        let px = vec![0xFF00FF, 0xFE01FE, 0x000000, 0x123456];
        assert!((color_share(&px, 0xFF00FF, 2) - 0.5).abs() < 1e-9);
        assert_eq!(color_share(&[], 0xFF00FF, 2), 0.0);
    }
}
