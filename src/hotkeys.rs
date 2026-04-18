//! Hotkey handling for head tracking control.
//!
//! Polls the Windows keyboard state at ~100Hz on a dedicated thread.
//! Two equivalent binding sets per the project standard:
//!
//! | Action          | Nav-cluster | Chord          |
//! |-----------------|-------------|----------------|
//! | Recenter        | Home        | Ctrl+Shift+T   |
//! | Toggle tracking | End         | Ctrl+Shift+Y   |
//! | Toggle position | PageUp      | Ctrl+Shift+G   |
//!
//! The chord letters T/Y/G form a vertical 1x3 strip in the
//! T/Y/U/G/H/J cluster on the keyboard - easy to recall.
//! `Ctrl+Shift+<letter>` is universally avoided by games (Ctrl is
//! crouch / interact, Shift is sprint / weapon-wheel, both together
//! is well outside any in-game bind set), so the chord set works
//! reliably across every game in the CameraUnlock project.
//!
//! Each action has an independent 300ms debounce to prevent held-key
//! repeats.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

use crate::tracking::{TrackingState, GLOBAL_STATE};

// Virtual key codes (Windows VK_* constants).
const VK_SHIFT: i32 = 0x10;
const VK_CONTROL: i32 = 0x11;
const VK_END: i32 = 0x23;
const VK_HOME: i32 = 0x24;
const VK_PAGE_UP: i32 = 0x21;
const VK_G: i32 = 0x47;
const VK_T: i32 = 0x54;
const VK_Y: i32 = 0x59;

/// Debounce window per action.
pub const DEBOUNCE_MS: u64 = 300;

/// Polling interval (~100Hz).
pub const POLL_INTERVAL_MS: u64 = 10;

fn is_down(vk: i32) -> bool {
    // High bit of GetAsyncKeyState indicates currently pressed.
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

/// True if the nav-cluster key OR the Ctrl+Shift+<letter> chord is
/// currently pressed.
fn binding_down(nav_vk: i32, chord_letter_vk: i32) -> bool {
    is_down(nav_vk) || (is_down(VK_CONTROL) && is_down(VK_SHIFT) && is_down(chord_letter_vk))
}

/// If the binding is pressed and its debounce has elapsed, update the
/// timer and return true.
fn fired(nav_vk: i32, chord_letter_vk: i32, last: &mut Instant, debounce: Duration) -> bool {
    if !binding_down(nav_vk, chord_letter_vk) {
        return false;
    }
    let now = Instant::now();
    if now.duration_since(*last) <= debounce {
        return false;
    }
    *last = now;
    true
}

fn tick(state: &mut TrackingState) {
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    if fired(VK_HOME, VK_T, &mut state.last_recenter_time, debounce) {
        state.set_recenter();
    }
    if fired(VK_END, VK_Y, &mut state.last_toggle_time, debounce) {
        state.toggle();
    }
    if fired(VK_PAGE_UP, VK_G, &mut state.last_position_time, debounce) {
        state.toggle_position();
    }
}

/// Start the hotkey polling thread.
pub fn start_hotkey_thread() -> JoinHandle<()> {
    thread::spawn(move || loop {
        if GLOBAL_STATE.read().shutdown_requested {
            break;
        }
        tick(&mut GLOBAL_STATE.write());
        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_is_300ms() {
        assert_eq!(DEBOUNCE_MS, 300);
    }

    #[test]
    fn poll_rate_is_100hz() {
        assert_eq!(POLL_INTERVAL_MS, 10);
        assert_eq!(1000 / POLL_INTERVAL_MS, 100);
    }

    #[test]
    fn vk_constants_match_standard_bindings() {
        assert_eq!(VK_HOME, 0x24);
        assert_eq!(VK_END, 0x23);
        assert_eq!(VK_PAGE_UP, 0x21);
        assert_eq!(VK_T, 0x54);
        assert_eq!(VK_Y, 0x59);
        assert_eq!(VK_G, 0x47);
    }
}
