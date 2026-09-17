use std::time::{Duration, Instant};

use parking_lot::Mutex;
use windows::core::{Error, Result};
use windows::Win32::Foundation::{BOOL, E_FAIL, RECT};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible, IsZoomed, SetWindowPos, GWL_STYLE,
    HWND_TOP, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION,
};

#[derive(PartialEq)]
struct Layout {
    hwnd: usize,
    width: i32,
    height: i32,
    work: RECT,
}

enum Phase {
    Settling {
        position: (i32, i32),
        since: Instant,
    },
    Moving {
        target: (i32, i32),
        since: Instant,
    },
    Done,
}

struct Placement {
    layout: Layout,
    phase: Phase,
}

static PLACEMENT: Mutex<Option<Placement>> = Mutex::new(None);

fn centered_position(width: i32, height: i32, work: RECT) -> (i32, i32) {
    // Keep the title bar reachable when the window is taller than the work area.
    (
        work.left + ((work.right - work.left - width) / 2).max(0),
        work.top + ((work.bottom - work.top - height) / 2).max(0),
    )
}

pub fn center(swap_chain: &IDXGISwapChain) -> Result<()> {
    unsafe {
        let desc = swap_chain.GetDesc()?;
        let hwnd = desc.OutputWindow;
        let mut fullscreen = BOOL::default();
        swap_chain.GetFullscreenState(Some(&mut fullscreen), None)?;
        let mut state = PLACEMENT.lock();
        if fullscreen.as_bool() || IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            *state = None;
            return Ok(());
        }
        if !IsWindowVisible(hwnd).as_bool() {
            return Ok(());
        }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect)?;
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err(Error::from_win32());
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        let borderless = GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == 0;
        if borderless
            && width >= info.rcMonitor.right - info.rcMonitor.left
            && height >= info.rcMonitor.bottom - info.rcMonitor.top
        {
            *state = None;
            return Ok(());
        }
        let layout = Layout {
            hwnd: hwnd.0 as usize,
            width,
            height,
            work: info.rcWork,
        };
        let position = (rect.left, rect.top);
        let now = Instant::now();
        if state
            .as_ref()
            .is_none_or(|placement| placement.layout != layout)
        {
            *state = Some(Placement {
                layout,
                phase: Phase::Settling {
                    position,
                    since: now,
                },
            });
        }
        let placement = state.as_mut().unwrap();
        match &mut placement.phase {
            Phase::Settling {
                position: previous,
                since,
            } => {
                if position != *previous {
                    *previous = position;
                    *since = now;
                }
                if now.duration_since(*since) < Duration::from_millis(500) {
                    return Ok(());
                }
                let target = centered_position(width, height, info.rcWork);
                // The window thread can be waiting for Present. Queue the move,
                // then confirm it on a later frame without blocking that thread.
                SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    target.0,
                    target.1,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                )?;
                placement.phase = Phase::Moving { target, since: now };
            }
            Phase::Moving { target, since } => {
                if position == *target {
                    log::info!(
                        "window: confirmed centered {width}x{height} at ({}, {})",
                        target.0,
                        target.1
                    );
                    placement.phase = Phase::Done;
                } else if now.duration_since(*since) > Duration::from_secs(2) {
                    return Err(Error::new(
                        E_FAIL,
                        format!("Window move to {target:?} was not applied; observed {position:?}"),
                    ));
                }
            }
            Phase::Done => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_height_window_on_ultrawide_is_centered_horizontally() {
        let work = RECT {
            left: 0,
            top: 0,
            right: 5120,
            bottom: 1392,
        };
        assert_eq!(centered_position(2576, 1460, work), (1272, 0));
    }

    #[test]
    fn secondary_monitor_work_area_origin_is_preserved() {
        let work = RECT {
            left: -2560,
            top: 40,
            right: 0,
            bottom: 1440,
        };
        assert_eq!(centered_position(1296, 759, work), (-1928, 360));
    }
}
