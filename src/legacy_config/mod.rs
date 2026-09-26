//! The config reader of v0.5.0, the last build before the canonical config, frozen.
//!
//! It reads `bioshock_headtrack.ini` exactly as that build did and fills a copy of
//! that build's settings and their defaults. It writes nothing: where v0.5.0 found
//! no readable file it wrote its template and ran on the defaults, and here the
//! caller decides what to do with [`Outcome::Unread`].
//!
//! Never edit this module. A player can update from any published build, and the
//! import is only a faithful one while this is the reader that build ran.

use std::path::Path;

/// The settings v0.5.0 read, at its defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// `[General] WorldSpaceYaw`.
    pub world_space_yaw: bool,
    /// `[Hotkeys] YawModeKey`: a virtual-key code from 0x01 to 0xFE. The yaw mode
    /// also always fired on Ctrl+Shift+H.
    pub yaw_mode_key: i32,
    /// `[Smoothing] LocalSmoothing`, finite and clamped to 0-1.
    pub local_smoothing: f64,
    /// `[Smoothing] RemoteSmoothing`, finite and clamped to 0-1.
    pub remote_smoothing: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            world_space_yaw: true,
            yaw_mode_key: 0x22,
            local_smoothing: 0.0,
            remote_smoothing: 0.15,
        }
    }
}

/// Every section and key the reader takes a value from. `[overlay] fov_h` is not
/// one: v0.5.0 only logged it as obsolete.
pub const KEYS: &[(&str, &str)] = &[
    ("General", "WorldSpaceYaw"),
    ("Hotkeys", "YawModeKey"),
    ("Smoothing", "LocalSmoothing"),
    ("Smoothing", "RemoteSmoothing"),
];

/// What reading the file gave.
#[derive(Debug)]
pub enum Outcome {
    /// `std::fs::read_to_string` failed: no file, or one that is not UTF-8. v0.5.0
    /// then wrote its template over the path and ran on the defaults.
    Unread(std::io::Error),
    Read(Config),
}

pub fn read(path: &Path) -> Outcome {
    let contents = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return Outcome::Unread(e),
    };

    let mut config = Config::default();
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
            ("overlay", "fov_h") => {
                log::warn!(
                    "config: [overlay] fov_h is obsolete; the reticle uses the game's projection"
                );
            }
            ("general", "worldspaceyaw") => match parse_bool(value) {
                Some(v) => {
                    config.world_space_yaw = v;
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
                    config.yaw_mode_key = v;
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
                    config.local_smoothing = v;
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
                    config.remote_smoothing = v;
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
        log::info!(
            "config: {} present but no recognised keys applied",
            path.display()
        );
    }
    Outcome::Read(config)
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
