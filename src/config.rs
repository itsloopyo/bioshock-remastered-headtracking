use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

static YAW_MODE_KEY: AtomicI32 = AtomicI32::new(0x22);

/// User-configured smoothing, stored as f64 bits. Both cover rotation
/// and position; the receiver's connection locality picks which one the
/// pipeline uses per frame.
static LOCAL_SMOOTHING_BITS: AtomicU64 =
    AtomicU64::new(crate::smoothing::DEFAULT_LOCAL_SMOOTHING.to_bits());
static REMOTE_SMOOTHING_BITS: AtomicU64 =
    AtomicU64::new(crate::smoothing::DEFAULT_REMOTE_SMOOTHING.to_bits());

pub fn yaw_mode_key() -> i32 {
    YAW_MODE_KEY.load(Ordering::Acquire)
}

/// Smoothing applied when the tracker runs on this machine (loopback).
pub fn local_smoothing() -> f64 {
    f64::from_bits(LOCAL_SMOOTHING_BITS.load(Ordering::Acquire))
}

/// Smoothing applied when the tracker is a remote device on the network.
pub fn remote_smoothing() -> f64 {
    f64::from_bits(REMOTE_SMOOTHING_BITS.load(Ordering::Acquire))
}

const DEFAULT_INI: &str = "\
; BioShock Remastered Head Tracking.

[General]
; Yaw mode: true = horizon-locked yaw (default), false = camera-local
WorldSpaceYaw=true

[Hotkeys]
; Page Down - toggle world/local yaw
YawModeKey=0x22

[Smoothing]
; Smoothing applied when the tracker runs on this machine (loopback).
; 0 = no smoothing, 1 = heavy. Covers rotation and position.
LocalSmoothing=0.0
; Smoothing applied when the tracker is a remote device on the network.
; 0 = no smoothing, 1 = heavy. Covers rotation and position.
RemoteSmoothing=0.15
";

/// Read `bioshock_headtrack.ini` from the working directory (where the
/// DLL was loaded - Build/Final/) through the frozen reader and apply it.
/// If the file cannot be read, writes the `DEFAULT_INI` template so the
/// user can find it without hunting.
pub fn load() {
    let path = "bioshock_headtrack.ini";
    let config = match crate::legacy_config::read(std::path::Path::new(path)) {
        crate::legacy_config::Outcome::Read(config) => config,
        crate::legacy_config::Outcome::Unread(_) => {
            match std::fs::write(path, DEFAULT_INI) {
                Ok(()) => log::info!("config: wrote default {} (no overrides active)", path),
                Err(e) => log::warn!("config: no {} found and couldn't create one: {}", path, e),
            }
            crate::legacy_config::Config::default()
        }
    };
    crate::tracking::set_world_space_yaw_initial(config.world_space_yaw);
    YAW_MODE_KEY.store(config.yaw_mode_key, Ordering::Release);
    LOCAL_SMOOTHING_BITS.store(config.local_smoothing.to_bits(), Ordering::Release);
    REMOTE_SMOOTHING_BITS.store(config.remote_smoothing.to_bits(), Ordering::Release);
    log_yaw_mode_startup();
}

fn log_yaw_mode_startup() {
    log::info!(
        "config: yaw mode startup = {}, YawModeKey = 0x{:02X}",
        if crate::tracking::is_world_space_yaw_atomic() {
            "world-space"
        } else {
            "camera-local"
        },
        yaw_mode_key()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_defaults_match_the_core() {
        assert_eq!(local_smoothing(), crate::smoothing::DEFAULT_LOCAL_SMOOTHING);
        assert_eq!(
            remote_smoothing(),
            crate::smoothing::DEFAULT_REMOTE_SMOOTHING
        );
    }
}
