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
//! ```
//!
//! Loaded once at mod init. If absent or malformed, the overlay falls
//! back to reading `DefaultFOV` from the PlayerController (which is
//! correct for users who haven't changed the in-game FOV slider).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

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

/// Returns the user's INI-configured horizontal FOV in degrees, or
/// `None` if no override was set.
pub fn fov_h_override() -> Option<f32> {
    if FOV_H_SET.load(Ordering::Acquire) {
        Some(f32::from_bits(FOV_H_DEG_BITS.load(Ordering::Relaxed)))
    } else {
        None
    }
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
        if section == "overlay" && key == "fov_h" {
            match value.parse::<f32>() {
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
            }
        }
    }

    if applied == 0 {
        log::info!("config: {} present but no recognised keys applied", path);
    }
}
