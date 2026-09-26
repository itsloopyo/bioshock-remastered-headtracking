//! Hotkey handling for head tracking control.
//!
//! The keys are the lists in `CameraUnlock.ini`: ToggleKey, CycleTrackingModeKey and
//! YawModeKey, each defaulting to its nav-cluster key and a Ctrl+Shift chord. core's
//! hotkey poller registers them and runs these actions on its own thread, once per press.
//!
//! The mode cycle and the yaw toggle apply the new state, then save it. The on/off toggle
//! changes the session only: at startup tracking follows EnableOnStartup.

use crate::tracking::GLOBAL_STATE;

extern "C" fn toggle() {
    GLOBAL_STATE.write().toggle();
}

extern "C" fn cycle_tracking_mode() {
    let (rotation, position) = {
        let mut state = GLOBAL_STATE.write();
        state.cycle_tracking_mode();
        (state.rotation_enabled, state.position_enabled)
    };
    crate::config::save_tracking_mode(rotation, position);
}

extern "C" fn toggle_yaw_mode() {
    let world_space_yaw = {
        let mut state = GLOBAL_STATE.write();
        state.toggle_yaw_mode();
        state.world_space_yaw
    };
    crate::config::save_world_space_yaw(world_space_yaw);
}

/// Registers the three lists and starts polling.
pub fn start() {
    crate::config::start_hotkeys(toggle, cycle_tracking_mode, toggle_yaw_mode);
}
