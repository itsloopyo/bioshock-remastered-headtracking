//! The config table and owner as the mod runs them: the committed file, the defaults, the
//! rows that follow Defaults.ini, and what the two saving hotkeys write. Every owner here
//! reads a scratch Defaults.ini through Owner::at, never the player's own.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use bioshock_headtrack::config::{Line, Owner, SaveStatus, Settings};

#[path = "support/mod.rs"]
mod support;

use support::Hotkey;

const CANONICAL: i32 = 0;
const CREATED: i32 = 2;
const CTRL_SHIFT: u32 = 3;

fn committed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config/CameraUnlock.ini")
}

/// A game folder of its own under the temp folder, and a Defaults.ini path whose parent
/// exists, removed when the test ends.
struct Scratch {
    root: PathBuf,
    game: PathBuf,
    defaults: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn scratch() -> Scratch {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "bsr-config-owner-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let game = root.join("game");
    std::fs::create_dir_all(&game).unwrap();
    std::fs::create_dir_all(root.join("user")).unwrap();
    Scratch {
        defaults: root.join("user/CameraUnlock/Defaults.ini"),
        game,
        root,
    }
}

fn mentions(lines: &[Line], text: &str) -> bool {
    lines.iter().any(|line| match line {
        Line::Info(l) | Line::Warning(l) => l.contains(text),
    })
}

fn built_in() -> Settings {
    Settings {
        udp_port: 4242,
        enable_on_startup: true,
        world_space_yaw: true,
        rotation_enabled: true,
        position_enabled: true,
        local_smoothing: 0.0,
        remote_smoothing: 0.15,
        collision_enabled: true,
        collision_release_smoothing: 0.9,
    }
}

/// The lines of `after` that differ from `before`, which must have as many.
fn changed_lines(before: &[u8], after: &[u8]) -> Vec<(String, String)> {
    let before = String::from_utf8(before.to_vec()).unwrap();
    let after = String::from_utf8(after.to_vec()).unwrap();
    let before: Vec<&str> = before.split("\r\n").collect();
    let after: Vec<&str> = after.split("\r\n").collect();
    assert_eq!(before.len(), after.len(), "a save added or removed lines");
    before
        .iter()
        .zip(&after)
        .filter(|(b, a)| b != a)
        .map(|(b, a)| (b.to_string(), a.to_string()))
        .collect()
}

/// config/CameraUnlock.ini is the table's fresh render, byte for byte. With
/// CAMERAUNLOCK_RENDER_CONFIG=write (pixi run render-config) this writes it instead.
#[test]
fn the_committed_file_is_the_fresh_render() {
    let rendered = support::render_fresh();
    match std::env::var("CAMERAUNLOCK_RENDER_CONFIG").as_deref() {
        Ok("write") => std::fs::write(committed_path(), &rendered).unwrap(),
        Ok(other) => panic!("CAMERAUNLOCK_RENDER_CONFIG={other}: only write is a mode"),
        Err(_) => assert!(
            std::fs::read(committed_path()).unwrap() == rendered,
            "config/CameraUnlock.ini is stale: pixi run render-config"
        ),
    }
}

#[test]
fn the_import_names_every_key_the_frozen_reader_reads() {
    let expected: Vec<(String, String)> = bioshock_headtrack::legacy_config::KEYS
        .iter()
        .map(|(s, k)| (s.to_string(), k.to_string()))
        .collect();
    assert_eq!(support::import_keys(), expected);
}

#[test]
fn the_first_launch_creates_the_committed_file_on_the_built_in_values() {
    let Scratch { game, defaults, .. } = &scratch();
    let owner = Owner::at(game, defaults);
    let (status, settings, log) = owner.load();
    assert_eq!(status, CREATED, "{log:?}");
    assert_eq!(settings, built_in());
    assert_eq!(
        std::fs::read(game.join("CameraUnlock.ini")).unwrap(),
        std::fs::read(committed_path()).unwrap()
    );
    assert!(defaults.is_file(), "Defaults.ini is created where none is");
    assert_eq!(
        support::hotkey_bindings(&owner, Hotkey::Toggle),
        [(0, 0x23), (CTRL_SHIFT, 'Y' as i32)]
    );
    assert_eq!(
        support::hotkey_bindings(&owner, Hotkey::CycleMode),
        [(0, 0x21), (CTRL_SHIFT, 'G' as i32)]
    );
    assert_eq!(
        support::hotkey_bindings(&owner, Hotkey::YawMode),
        [(0, 0x22), (CTRL_SHIFT, 'H' as i32)]
    );
}

#[test]
fn default_rows_follow_defaults_ini() {
    let Scratch { game, defaults, .. } = &scratch();
    std::fs::create_dir_all(defaults.parent().unwrap()).unwrap();
    std::fs::write(
        defaults,
        "[Network]\r\nUdpPort=5000\r\n[General]\r\nEnableOnStartup=false\r\nWorldSpaceYaw=false\r\n\
         [Position]\r\nPositionEnabled=false\r\nCollisionEnabled=false\r\n\
         CollisionReleaseSmoothing=0.5\r\n[Smoothing]\r\nLocalSmoothing=0.3\r\n\
         [Hotkeys]\r\nToggleKey=F8\r\n",
    )
    .unwrap();
    let owner = Owner::at(game, defaults);
    let (status, settings, _) = owner.load();
    assert_eq!(status, CREATED);
    assert_eq!(
        settings,
        Settings {
            udp_port: 5000,
            enable_on_startup: false,
            world_space_yaw: false,
            rotation_enabled: true,
            position_enabled: false,
            local_smoothing: 0.3,
            remote_smoothing: 0.15,
            collision_enabled: false,
            collision_release_smoothing: 0.5,
        }
    );
    assert_eq!(
        support::hotkey_bindings(&owner, Hotkey::Toggle),
        [(0, 0x77)]
    );
}

#[test]
fn values_in_the_file_are_read_over_defaults_ini() {
    let Scratch { game, defaults, .. } = &scratch();
    std::fs::write(
        game.join("CameraUnlock.ini"),
        "[CameraUnlock]\r\nConfigFormat=1\r\n[General]\r\nWorldSpaceYaw=false\r\n\
         RotationEnabled=false\r\n[Position]\r\nPositionEnabled=true\r\n\
         [Smoothing]\r\nRemoteSmoothing=0.4\r\n[Hotkeys]\r\nYawModeKey=F10, Ctrl+Shift+H\r\n",
    )
    .unwrap();
    let owner = Owner::at(game, defaults);
    let (status, settings, log) = owner.load();
    assert_eq!(status, CANONICAL, "{log:?}");
    assert!(!settings.world_space_yaw);
    assert!(!settings.rotation_enabled && settings.position_enabled);
    assert_eq!(settings.remote_smoothing, 0.4);
    assert_eq!(
        support::hotkey_bindings(&owner, Hotkey::YawMode),
        [(0, 0x79), (CTRL_SHIFT, 'H' as i32)]
    );
}

/// The yaw hotkey writes WorldSpaceYaw and the mode hotkey the pair, each over `default`,
/// and nothing else in the file changes. The next launch starts where they left off.
#[test]
fn the_mode_and_yaw_hotkeys_save_their_rows_only() {
    let Scratch { game, defaults, .. } = &scratch();
    let config = game.join("CameraUnlock.ini");
    let owner = Owner::at(game, defaults);
    owner.load();
    let fresh = std::fs::read(&config).unwrap();

    let (status, log) = owner.save_world_space_yaw(false);
    assert_eq!(status, SaveStatus::Saved);
    assert!(
        mentions(&log, "WorldSpaceYaw=false is now set for this game"),
        "{log:?}"
    );
    let yaw_saved = std::fs::read(&config).unwrap();
    assert_eq!(
        changed_lines(&fresh, &yaw_saved),
        [(
            "WorldSpaceYaw=default".to_string(),
            "WorldSpaceYaw=false".to_string()
        )]
    );

    let (status, _) = owner.save_tracking_mode(true, false);
    assert_eq!(status, SaveStatus::Saved);
    let mode_saved = std::fs::read(&config).unwrap();
    assert_eq!(
        changed_lines(&yaw_saved, &mode_saved),
        [
            (
                "RotationEnabled=default".to_string(),
                "RotationEnabled=true".to_string()
            ),
            (
                "PositionEnabled=default".to_string(),
                "PositionEnabled=false".to_string()
            ),
        ]
    );

    let (status, settings, _) = Owner::at(game, defaults).load();
    assert_eq!(status, CANONICAL);
    assert!(!settings.world_space_yaw);
    assert!(settings.rotation_enabled && !settings.position_enabled);
    assert!(settings.enable_on_startup);
}
