//! User-configurable INI loader.
//!
//! BSR's FOV slider writes to memory we can't currently locate, so the
//! mod can't auto-detect non-default FOV values. This module provides
//! a tiny INI override at `bioshock_headtrack.ini` (next to the DLL)
//! so users with non-default settings can declare their FOV manually.
//!
//! File format:
//! ```ini
//! [overlay]
//! fov_h = 100
//!
//! [Smoothing]
//! LocalSmoothing = 0.0
//! RemoteSmoothing = 0.15
//! ```
//!
//! Loaded once at mod init. If absent or malformed, the overlay falls
//! back to reading `DefaultFOV` from the PlayerController (which is
//! correct for users who haven't changed the in-game FOV slider).

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

/// Accepted horizontal FOV range for the INI override. Anything outside
/// this is rejected (logged then ignored). 40° matches the lower bound
/// of the in-game slider; 150° is well above any sane gameplay FOV.
const FOV_H_MIN_DEG: f32 = 40.0;
const FOV_H_MAX_DEG: f32 = 150.0;

/// User's configured horizontal FOV in degrees, stored as f32 bits in
/// an AtomicU32. Set by `load()` from the INI file. `FOV_H_SET`
/// indicates whether `load()` actually found a value (true) vs. left
/// the field at its sentinel (false).
static FOV_H_DEG_BITS: AtomicU32 = AtomicU32::new(0);
static FOV_H_SET: AtomicBool = AtomicBool::new(false);
static YAW_MODE_KEY: AtomicI32 = AtomicI32::new(0x22);

/// User-configured smoothing, stored as f64 bits. Both cover rotation
/// and position; the receiver's connection locality picks which one the
/// pipeline uses per frame.
static LOCAL_SMOOTHING_BITS: AtomicU64 =
    AtomicU64::new(crate::smoothing::DEFAULT_LOCAL_SMOOTHING.to_bits());
static REMOTE_SMOOTHING_BITS: AtomicU64 =
    AtomicU64::new(crate::smoothing::DEFAULT_REMOTE_SMOOTHING.to_bits());

/// Returns the user's INI-configured horizontal FOV in degrees, or
/// `None` if no override was set.
pub fn fov_h_override() -> Option<f32> {
    if FOV_H_SET.load(Ordering::Acquire) {
        Some(f32::from_bits(FOV_H_DEG_BITS.load(Ordering::Relaxed)))
    } else {
        None
    }
}

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

/// Validation only - rejects non-finite values and clamps to the
/// documented 0.0-1.0 range. This is not a floor: 0.0 stays 0.0.
fn parse_smoothing(value: &str) -> Option<f64> {
    let parsed = value.trim().parse::<f64>().ok()?;
    if !parsed.is_finite() {
        return None;
    }
    Some(parsed.clamp(0.0, 1.0))
}

/// Default INI written on first launch when no config file exists.
/// Self-documenting placeholder - the actual override is commented
/// out so first-launch behaviour is unchanged (auto-detected FOV).
const DEFAULT_INI: &str = "\
; BioShock Remastered Head Tracking - user overrides.
;
; If you've changed BSR's in-game FOV slider away from the default,
; uncomment `fov_h` below and set it to your chosen value (40–150°).
; The mod can't auto-detect the slider, so without this line the
; head-tracked reticle will drift away from the actual aim point.
;
; If you're running the stock FOV, leave this file alone.

[overlay]
; fov_h = 100

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
/// DLL was loaded - Build/Final/) and parse out any recognised keys.
/// Logs what was found. If the file is missing, writes the
/// `DEFAULT_INI` template so the user can find it without hunting.
pub fn load() {
    let path = "bioshock_headtrack.ini";
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => {
            match std::fs::write(path, DEFAULT_INI) {
                Ok(()) => log::info!("config: wrote default {} (no overrides active)", path),
                Err(e) => log::warn!("config: no {} found and couldn't create one: {}", path, e),
            }
            log_yaw_mode_startup();
            return;
        }
    };

    let mut section = String::new();
    let mut applied = 0usize;
    for raw in contents.lines() {
        let line = raw
            .split(';')
            .next()
            .unwrap_or("")
            .split('#')
            .next()
            .unwrap_or("")
            .trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = rest.trim().to_ascii_lowercase();
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        match (section.as_str(), key.as_str()) {
            ("overlay", "fov_h") => match value.parse::<f32>() {
                Ok(v) if v.is_finite() && (FOV_H_MIN_DEG..=FOV_H_MAX_DEG).contains(&v) => {
                    FOV_H_DEG_BITS.store(v.to_bits(), Ordering::Relaxed);
                    FOV_H_SET.store(true, Ordering::Release);
                    log::info!("config: [overlay] fov_h = {} (override active)", v);
                    applied += 1;
                }
                _ => {
                    log::warn!(
                        "config: [overlay] fov_h = {:?} is not a valid FOV ({}–{}°), ignoring",
                        value,
                        FOV_H_MIN_DEG,
                        FOV_H_MAX_DEG
                    );
                }
            },
            ("general", "worldspaceyaw") => match parse_bool(value) {
                Some(v) => {
                    crate::tracking::set_world_space_yaw_initial(v);
                    log::info!("config: [General] WorldSpaceYaw = {}", v);
                    applied += 1;
                }
                None => log::warn!(
                    "config: [General] WorldSpaceYaw = {:?} is not a boolean, using default",
                    value
                ),
            },
            ("hotkeys", "yawmodekey") => match parse_vk(value) {
                Some(v) => {
                    YAW_MODE_KEY.store(v, Ordering::Release);
                    log::info!("config: [Hotkeys] YawModeKey = 0x{:02X}", v);
                    applied += 1;
                }
                None => log::warn!(
                    "config: [Hotkeys] YawModeKey = {:?} is not a valid VK code, using default",
                    value
                ),
            },
            ("smoothing", "localsmoothing") => match parse_smoothing(value) {
                Some(v) => {
                    LOCAL_SMOOTHING_BITS.store(v.to_bits(), Ordering::Release);
                    log::info!("config: [Smoothing] LocalSmoothing = {}", v);
                    applied += 1;
                }
                None => log::warn!(
                    "config: [Smoothing] LocalSmoothing = {:?} is not a number, using default",
                    value
                ),
            },
            ("smoothing", "remotesmoothing") => match parse_smoothing(value) {
                Some(v) => {
                    REMOTE_SMOOTHING_BITS.store(v.to_bits(), Ordering::Release);
                    log::info!("config: [Smoothing] RemoteSmoothing = {}", v);
                    applied += 1;
                }
                None => log::warn!(
                    "config: [Smoothing] RemoteSmoothing = {:?} is not a number, using default",
                    value
                ),
            },
            _ => {}
        }
    }

    if applied == 0 {
        log::info!("config: {} present but no recognised keys applied", path);
    }
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

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_vk(value: &str) -> Option<i32> {
    let value = value.trim();
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        i32::from_str_radix(hex, 16).ok()?
    } else {
        value.parse::<i32>().ok()?
    };
    (1..=0xFE).contains(&parsed).then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_zero_is_kept() {
        // No floor: a configured 0.0 must survive validation intact.
        assert_eq!(parse_smoothing("0.0"), Some(0.0));
    }

    #[test]
    fn smoothing_out_of_range_is_clamped() {
        assert_eq!(parse_smoothing("-1"), Some(0.0));
        assert_eq!(parse_smoothing("5"), Some(1.0));
    }

    #[test]
    fn smoothing_rejects_non_numbers_and_non_finite() {
        assert_eq!(parse_smoothing("heavy"), None);
        assert_eq!(parse_smoothing("NaN"), None);
        assert_eq!(parse_smoothing("inf"), None);
    }

    #[test]
    fn smoothing_defaults_match_the_core() {
        assert_eq!(local_smoothing(), crate::smoothing::DEFAULT_LOCAL_SMOOTHING);
        assert_eq!(
            remote_smoothing(),
            crate::smoothing::DEFAULT_REMOTE_SMOOTHING
        );
    }
}
