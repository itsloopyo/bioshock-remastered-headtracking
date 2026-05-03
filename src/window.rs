//! Center the BioShock window on its monitor at startup.
//!
//! On super-ultrawide setups (e.g. 5120x1440) BSR launches its window in
//! the top-left corner of the screen and won't accept drag input from
//! the title bar, so the head tracking experience starts off-axis with
//! no easy way to fix it. We center the window on its current monitor's
//! work area on the first frame.
//!
//! Skips fullscreen-shaped windows (true exclusive or borderless that
//! already covers the whole monitor) so we don't perturb users who are
//! already happy with their setup.

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

static CENTERED: AtomicBool = AtomicBool::new(false);

/// Center `hwnd` on its current monitor's work area. Runs at most once
/// per process; subsequent calls are no-ops so the user can drag the
/// window afterwards without us yanking it back. Skips windows whose
/// width or height already meets/exceeds the work area (fullscreen, or
/// a borderless window the user has already maximized).
pub fn center_once(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    if CENTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe {
        let mut win_rect = RECT::default();
        if let Err(e) = GetWindowRect(hwnd, &mut win_rect) {
            log::warn!("window: GetWindowRect failed: {:?}", e);
            return;
        }
        let win_w = win_rect.right - win_rect.left;
        let win_h = win_rect.bottom - win_rect.top;

        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            log::warn!("window: GetMonitorInfoW failed");
            return;
        }
        let work = info.rcWork;
        let work_w = work.right - work.left;
        let work_h = work.bottom - work.top;

        if win_w >= work_w || win_h >= work_h {
            log::info!(
                "window: window {}x{} fills work area {}x{}, leaving in place",
                win_w,
                win_h,
                work_w,
                work_h
            );
            return;
        }

        let new_x = work.left + (work_w - win_w) / 2;
        let new_y = work.top + (work_h - win_h) / 2;
        if let Err(e) = SetWindowPos(
            hwnd,
            HWND_TOP,
            new_x,
            new_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        ) {
            log::warn!("window: SetWindowPos failed: {:?}", e);
            return;
        }
        log::info!(
            "window: centered {}x{} window at ({}, {}) on work area {}x{}",
            win_w,
            win_h,
            new_x,
            new_y,
            work_w,
            work_h
        );
    }
}
