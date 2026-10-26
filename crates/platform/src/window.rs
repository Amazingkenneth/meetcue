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

/// A screen or window rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// Where the panel goes when moved by (`dx`, `dy`) from `win`. It stays on
/// the screen under its top centre, falling back to the screen it was on, so
/// it can cross to a neighbouring screen sideways. Horizontally it stays fully
/// visible; vertically its top stays on screen and at least `keep` pixels of
/// it remain visible, since the full panel is nearly as tall as the screen.
pub fn nudge_target(screens: &[Rect], win: Rect, dx: i32, dy: i32, keep: i32) -> (i32, i32) {
    let (x, y) = (win.x + dx, win.y + dy);
    let on = |x: i32, y: i32| screens.iter().find(|s| s.contains(x + win.w / 2, y));
    let Some(s) = on(x, y)
        .or_else(|| on(win.x, win.y))
        .or_else(|| screens.first())
    else {
        return (x, y);
    };
    let x = x.min(s.x + s.w - win.w).max(s.x);
    let y = y.min(s.y + s.h - keep.min(win.h)).max(s.y);
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
    };
    const RIGHT: Rect = Rect {
        x: 1920,
        y: 0,
        w: 1920,
        h: 1080,
    };

    fn panel(x: i32, y: i32) -> Rect {
        Rect {
            x,
            y,
            w: 440,
            h: 1000,
        }
    }

    #[test]
    fn nudge_moves_and_stops_at_the_edges() {
        assert_eq!(
            nudge_target(&[LEFT], panel(500, 40), 0, -10, 120),
            (500, 30)
        );
        assert_eq!(nudge_target(&[LEFT], panel(500, 4), 0, -10, 120), (500, 0));
        assert_eq!(nudge_target(&[LEFT], panel(4, 40), -10, 0, 120), (0, 40));
        assert_eq!(
            nudge_target(&[LEFT], panel(1475, 40), 10, 0, 120),
            (1480, 40)
        );
        assert_eq!(
            nudge_target(&[LEFT], panel(500, 955), 0, 10, 120),
            (500, 960)
        );
    }

    #[test]
    fn nudge_crosses_to_a_neighbouring_screen() {
        let screens = [LEFT, RIGHT];
        // Still mostly on the left screen: held at its right edge.
        assert_eq!(
            nudge_target(&screens, panel(1480, 40), 10, 0, 120),
            (1480, 40)
        );
        // Once the top centre is past the edge, the right screen takes over.
        assert_eq!(
            nudge_target(&screens, panel(1480, 40), 300, 0, 120),
            (1920, 40)
        );
    }

    #[test]
    fn nudge_from_off_screen_comes_back() {
        assert_eq!(nudge_target(&[LEFT], panel(-900, -50), 10, 0, 120), (0, 0));
        assert_eq!(nudge_target(&[], panel(5, 5), 10, 0, 120), (15, 5));
    }

    #[test]
    fn share_of_marker_color() {
        let px = vec![0xFF00FF, 0xFE01FE, 0x000000, 0x123456];
        assert!((color_share(&px, 0xFF00FF, 2) - 0.5).abs() < 1e-9);
        assert_eq!(color_share(&[], 0xFF00FF, 2), 0.0);
    }
}
