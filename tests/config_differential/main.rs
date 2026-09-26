//! The differential test for the config conversion. Every input is read three ways:
//!
//!   the oracle     v0.5.0, the newest published build: its reader and the code that turns
//!                  what it read into the state the game starts in. oracle/<name>.rs is
//!                  `git show v0.5.0:src/<name>.rs`, byte for byte, for config, tracking,
//!                  hotkeys, smoothing and opentrack, each a module of this crate's root under
//!                  its own name, since they reach each other as crate::<name>. The reader
//!                  reads a relative path and keeps its settings in statics, so each reading
//!                  runs in a process of its own (this binary, started with --oracle <folder>)
//!   the import     the frozen reader in src/legacy_config/, and v0.5.0's startup state
//!   the migration  the config owner in a folder holding only bioshock_headtrack.ini, importing
//!                  it into a new CameraUnlock.ini, then the canonical reader and the table on
//!                  what it wrote
//!
//! Each reading is reduced to what a player's file decides: every setting, the tracking state
//! the mod starts in, the keys it registers and how those keys fire.
//!
//! Two parts of v0.5.0 are restated, not run. Its hotkey bindings are the literals of
//! hotkeys.rs tick(), which is private and reads the live keyboard; the test checks each
//! against the source text. Its lean clamp was C++ built with a default LeanClampSettings,
//! whose release_smoothing is 0.9 at v0.5.0's core pin, c480d8a.
//!
//! Comparison 1, oracle against import, finds nothing: no commit since v0.5.0 changed how the
//! file is read. Comparison 2, import against migration, finds one change, listed in
//! KNOWN_CHANGES with its commit: how the hotkeys fire, which moved to core's poller. Otherwise
//! no approved change or normalisation in core's data/config-format.json applies to what
//! v0.5.0 read. It runs over a Defaults.ini at the built-in values and over one a player
//! changed, since the migration writes default exactly where the imported value equals what
//! Defaults.ini gives.
//!
//! The distinct migrated files go to target/config-differential-migrated, where
//! lint-migrated.mjs runs core's canonical config lint over them after this binary.
//!
//! data/ holds the first-run output of every published build, the template each wrote where it
//! found no file: v0.1.0 (the same bytes through v0.3.2), v0.3.3 (through v0.3.6), v0.4.0 and
//! v0.5.0, each the DEFAULT_INI literal of that tag's src/config.rs. No build shipped a config
//! file or a launcher seed.
//!
//! Published tags: v0.1.0, v0.1.1, v0.2.0, v0.2.2, v0.3.0, v0.3.1, v0.3.2, v0.3.3, v0.3.5,
//! v0.3.6, v0.4.0 and v0.5.0; there is no dev pre-release. SHA-256 of the frozen files, the
//! oracle's each equal to `git show v0.5.0:src/<name>.rs`:
//!   oracle/config.rs            5d3b80b8b5940728acbf91e3f72c36f8b928d8951067d8a897d6a147d670ca6c
//!   oracle/hotkeys.rs           53eec46f20ba018a61b4b46cbc8630025b6cf5d846d5a9a98dd45903489ae5ae
//!   oracle/opentrack.rs         eb01bc7cfd47c6e8a593b7a44a6e920e5df7bccc05418ea9e0649d6cf7d11ff9
//!   oracle/smoothing.rs         85c553b04d17cc080476b4ed5c133a3d7b5a75efc7b4a61c486fc0f365673687
//!   oracle/tracking.rs          30b2e9b16d64828f8f31d71bf423baee0bb0fa4123aeaf41a1a4506f2e8cc923
//!   src/legacy_config/mod.rs    dfcf17c06fb1b6ac0de35821e9b069144e7475f02ffea9210637beceb1f4cab2
//! The frozen reader is Rust's standard library and nothing of cameraunlock-core.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use bioshock_headtrack::config::{Line, Owner, Settings};
use bioshock_headtrack::legacy_config;

#[path = "../support/mod.rs"]
mod support;

use support::{corpus, Hotkey};

#[path = "oracle/config.rs"]
#[allow(dead_code, unused_imports)]
mod config;
#[path = "oracle/hotkeys.rs"]
#[allow(dead_code, unused_imports)]
mod hotkeys;
#[path = "oracle/opentrack.rs"]
#[allow(dead_code, unused_imports)]
mod opentrack;
#[path = "oracle/smoothing.rs"]
#[allow(dead_code, unused_imports)]
mod smoothing;
#[path = "oracle/tracking.rs"]
#[allow(unused_imports)]
mod tracking;

const LEGACY_NAME: &str = "bioshock_headtrack.ini";
const CTRL_SHIFT: u32 = 3;

/// How a hotkey's bindings fire, which the build decides and the file does not.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Firing {
    /// A press counts only while the game's window is in front.
    foreground_only: bool,
    /// A binding with no modifiers also fires while Ctrl and Shift are both held.
    plain_under_ctrl_shift: bool,
    /// Held down, the hotkey acts again after this many ms; None, once per press.
    repeat_held_ms: Option<u64>,
    /// A press this many ms or less after the last one that acted does nothing.
    ignored_within_ms: u64,
}

/// Core's HotkeyPoller with RegisterKeyBindings: a key fires on the poll that sees it go down
/// (hotkey_poller.cpp Poll), only while this process owns the foreground window
/// (IsOwnProcessForeground), and a binding with no modifiers not while Ctrl and Shift are
/// both held (key_binding_registration.h BindingFires).
const CORE_FIRING: Firing = Firing {
    foreground_only: true,
    plain_under_ctrl_shift: false,
    repeat_held_ms: None,
    ignored_within_ms: 0,
};

/// v0.5.0's hotkeys.rs: binding_down takes the nav key whatever else is held, nothing checks
/// the foreground window, toggle and cycle go through fired(), which acts again every
/// DEBOUNCE_MS while held, and the yaw key through fired_edge(), once per press, both
/// ignoring a press within DEBOUNCE_MS of the last one that acted.
fn v050_firing(edge: bool) -> Firing {
    Firing {
        foreground_only: false,
        plain_under_ctrl_shift: true,
        repeat_held_ms: (!edge).then_some(hotkeys::DEBOUNCE_MS),
        ignored_within_ms: hotkeys::DEBOUNCE_MS,
    }
}

/// Everything a config file decides, as the game acts on it. Hotkeys are (modifiers, virtual
/// key) pairs, modifiers as core's KeyModifiers numbers.
#[derive(Clone, Debug, PartialEq)]
struct Effective {
    enable_on_startup: bool,
    udp_port: u16,
    world_space_yaw: bool,
    local_smoothing_bits: u64,
    remote_smoothing_bits: u64,
    rotation_enabled: bool,
    position_enabled: bool,
    collision_enabled: bool,
    collision_release_smoothing_bits: u32,
    toggle: Vec<(u32, i32)>,
    cycle_mode: Vec<(u32, i32)>,
    yaw_mode: Vec<(u32, i32)>,
    toggle_firing: Firing,
    cycle_mode_firing: Firing,
    yaw_mode_firing: Firing,
}

/// The state v0.5.0 started in apart from what its reader gave, as its compiled statics hold
/// it: initialize_mod touched GLOBAL_STATE and started the receiver on OPENTRACK_PORT, and
/// the render hook read the atomics.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Startup {
    enabled: bool,
    rotation_enabled: bool,
    position_enabled: bool,
    udp_port: u16,
}

fn v050_startup() -> Startup {
    let state = tracking::GLOBAL_STATE.read();
    let startup = Startup {
        enabled: tracking::is_enabled_atomic(),
        rotation_enabled: tracking::is_rotation_enabled_atomic(),
        position_enabled: tracking::is_position_enabled_atomic(),
        udp_port: opentrack::OPENTRACK_PORT,
    };
    assert_eq!(
        (
            state.enabled,
            state.rotation_enabled,
            state.position_enabled
        ),
        (
            startup.enabled,
            startup.rotation_enabled,
            startup.position_enabled
        ),
        "v0.5.0's GLOBAL_STATE starts where its atomics do"
    );
    startup
}

/// v0.5.0 as it ran on what its reader gave: its startup state, a lean held off the walls on
/// every frame at a release of 0.9, and the bindings and firing of its hotkeys.rs, the yaw key
/// the one the reader gave.
fn from_legacy(
    startup: Startup,
    world_space_yaw: bool,
    yaw_mode_key: i32,
    local: f64,
    remote: f64,
) -> Effective {
    Effective {
        enable_on_startup: startup.enabled,
        udp_port: startup.udp_port,
        world_space_yaw,
        local_smoothing_bits: local.to_bits(),
        remote_smoothing_bits: remote.to_bits(),
        rotation_enabled: startup.rotation_enabled,
        position_enabled: startup.position_enabled,
        collision_enabled: true,
        collision_release_smoothing_bits: 0.9_f32.to_bits(),
        toggle: vec![(0, 0x23), (CTRL_SHIFT, 'Y' as i32)],
        cycle_mode: vec![(0, 0x21), (CTRL_SHIFT, 'G' as i32)],
        yaw_mode: vec![(0, yaw_mode_key), (CTRL_SHIFT, 'H' as i32)],
        toggle_firing: v050_firing(false),
        cycle_mode_firing: v050_firing(false),
        yaw_mode_firing: v050_firing(true),
    }
}

/// Each difference as (field, both values).
fn differences(a: &Effective, b: &Effective) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    macro_rules! field {
        ($f:ident) => {
            if a.$f != b.$f {
                out.push((stringify!($f), format!("{:?} / {:?}", a.$f, b.$f)));
            }
        };
    }
    field!(enable_on_startup);
    field!(udp_port);
    field!(world_space_yaw);
    field!(local_smoothing_bits);
    field!(remote_smoothing_bits);
    field!(rotation_enabled);
    field!(position_enabled);
    field!(collision_enabled);
    field!(collision_release_smoothing_bits);
    field!(toggle);
    field!(cycle_mode);
    field!(yaw_mode);
    field!(toggle_firing);
    field!(cycle_mode_firing);
    field!(yaw_mode_firing);
    out
}

/// What comparison 2 finds that the conversion changed on purpose, with the commit that did,
/// each in CHANGELOG.md. Each must still occur, so the list cannot outlive the change.
const KNOWN_CHANGES: &[(&str, &str)] = &[
    ("toggle_firing", "71b80d6: the hotkeys run on core's poller"),
    (
        "cycle_mode_firing",
        "71b80d6: the hotkeys run on core's poller",
    ),
    (
        "yaw_mode_firing",
        "71b80d6: the hotkeys run on core's poller",
    ),
];

/// The literals from_legacy restates are the ones in oracle/hotkeys.rs, which the oracle
/// cannot run: tick() is private and reads the live keyboard.
fn test_the_v050_hotkeys_are_its_source() -> Vec<String> {
    let source = include_str!("oracle/hotkeys.rs");
    [
        "const VK_SHIFT: i32 = 0x10;",
        "const VK_CONTROL: i32 = 0x11;",
        "const VK_END: i32 = 0x23;",
        "const VK_PAGE_UP: i32 = 0x21;",
        "const VK_H: i32 = 0x48;",
        "const VK_G: i32 = 0x47;",
        "const VK_Y: i32 = 0x59;",
        "is_down(nav_vk) || (is_down(VK_CONTROL) && is_down(VK_SHIFT) && is_down(chord_letter_vk))",
        "if fired(VK_END, VK_Y, &mut state.last_toggle_time, debounce) {",
        "if fired(VK_PAGE_UP, VK_G, &mut state.last_cycle_mode_time, debounce) {",
        "    if fired_edge(\n        yaw_mode_key,\n        VK_H,",
    ]
    .iter()
    .filter(|line| !source.contains(*line))
    .map(|line| format!("oracle/hotkeys.rs has no {line:?}"))
    .chain(
        source
            .contains("GetForegroundWindow")
            .then(|| "oracle/hotkeys.rs checks the foreground window".to_string()),
    )
    .collect()
}

// ---- files ----------------------------------------------------------------------------------

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

fn fresh_dir(root: &Path) -> PathBuf {
    let dir = root.join(NEXT_DIR.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn data(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/config_differential/data")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A corpus input: its name, and its bytes, or none for the no-file case.
struct Input {
    name: String,
    bytes: Option<Vec<u8>>,
}

fn place(dir: &Path, input: &Input) -> PathBuf {
    let path = dir.join(LEGACY_NAME);
    if let Some(bytes) = &input.bytes {
        std::fs::write(&path, bytes).unwrap();
    }
    path
}

// ---- the readers ----------------------------------------------------------------------------

fn read_oracle(root: &Path, input: &Input) -> Effective {
    let dir = fresh_dir(root);
    place(&dir, input);
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--oracle")
        .arg(&dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}: the oracle failed: {}",
        input.name,
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let fields: Vec<&str> = text.split_whitespace().collect();
    let flag = |i: usize| fields[i] == "1";
    from_legacy(
        Startup {
            enabled: flag(4),
            rotation_enabled: flag(5),
            position_enabled: flag(6),
            udp_port: fields[7].parse().unwrap(),
        },
        flag(0),
        fields[1].parse().unwrap(),
        f64::from_bits(u64::from_str_radix(fields[2], 16).unwrap()),
        f64::from_bits(u64::from_str_radix(fields[3], 16).unwrap()),
    )
}

/// The oracle's side of read_oracle: v0.5.0's initialize_mod in `dir` up to the threads and
/// hooks it starts, config::load() and then GLOBAL_STATE, printed.
fn run_oracle(dir: &Path) {
    std::env::set_current_dir(dir).unwrap();
    config::load();
    let startup = v050_startup();
    println!(
        "{} {} {:016x} {:016x} {} {} {} {}",
        u8::from(tracking::is_world_space_yaw_atomic()),
        config::yaw_mode_key(),
        config::local_smoothing().to_bits(),
        config::remote_smoothing().to_bits(),
        u8::from(startup.enabled),
        u8::from(startup.rotation_enabled),
        u8::from(startup.position_enabled),
        startup.udp_port
    );
}

/// The migration's settings as the mod starts on them, and the keys it registers.
fn from_migration(settings: &Settings, owner: &Owner) -> Effective {
    Effective {
        enable_on_startup: settings.enable_on_startup,
        udp_port: settings.udp_port,
        world_space_yaw: settings.world_space_yaw,
        local_smoothing_bits: settings.local_smoothing.to_bits(),
        remote_smoothing_bits: settings.remote_smoothing.to_bits(),
        rotation_enabled: settings.rotation_enabled,
        position_enabled: settings.position_enabled,
        collision_enabled: settings.collision_enabled,
        collision_release_smoothing_bits: settings.collision_release_smoothing.to_bits(),
        toggle: support::hotkey_bindings(owner, Hotkey::Toggle),
        cycle_mode: support::hotkey_bindings(owner, Hotkey::CycleMode),
        yaw_mode: support::hotkey_bindings(owner, Hotkey::YawMode),
        toggle_firing: CORE_FIRING,
        cycle_mode_firing: CORE_FIRING,
        yaw_mode_firing: CORE_FIRING,
    }
}

/// Whether the frozen reader finds a file there that it cannot read as text. v0.5.0 wrote
/// its template over such a file and ran on the defaults; the migration leaves it as it is,
/// runs on the same defaults and tries again at the next launch.
fn unreadable(path: &Path) -> bool {
    matches!(legacy_config::read(path), legacy_config::Outcome::Unread(e) if e.kind() != std::io::ErrorKind::NotFound)
}

/// The frozen reader, and what v0.5.0 ran on where it read no file.
fn import_config(path: &Path) -> legacy_config::Config {
    match legacy_config::read(path) {
        legacy_config::Outcome::Read(c) => c,
        legacy_config::Outcome::Unread(_) => legacy_config::Config::default(),
    }
}

/// This process never runs the oracle's load(), so its v0.5.0 statics are where that build
/// started them.
fn from_import(c: &legacy_config::Config) -> Effective {
    from_legacy(
        v050_startup(),
        c.world_space_yaw,
        c.yaw_mode_key,
        c.local_smoothing,
        c.remote_smoothing,
    )
}

fn read_import(root: &Path, input: &Input) -> Effective {
    let dir = fresh_dir(root);
    from_import(&import_config(&place(&dir, input)))
}

// ---- the inputs -----------------------------------------------------------------------------

/// Every key the frozen reader takes a value from, described for the corpus generator.
fn descriptors() -> Vec<corpus::Descriptor> {
    use corpus::Descriptor;
    vec![
        Descriptor {
            section: "General",
            key: "WorldSpaceYaw",
            alternate: "false",
            out_of_range: &[],
            hotkey: false,
        },
        Descriptor {
            section: "Hotkeys",
            key: "YawModeKey",
            alternate: "0x71",
            out_of_range: &["0xFF", "255", "0x0", "-34"],
            hotkey: true,
        },
        Descriptor {
            section: "Smoothing",
            key: "LocalSmoothing",
            alternate: "0.25",
            out_of_range: &["-0.5", "1.5"],
            hotkey: false,
        },
        Descriptor {
            section: "Smoothing",
            key: "RemoteSmoothing",
            alternate: "0.4",
            out_of_range: &["-0.5", "1.5"],
            hotkey: false,
        },
    ]
}

const FIRST_RUNS: &[&str] = &[
    "v0.1.0-first-run.ini",
    "v0.3.3-first-run.ini",
    "v0.4.0-first-run.ini",
    "v0.5.0-first-run.ini",
];

fn replaced(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = std::str::from_utf8(bytes).unwrap();
    assert!(text.contains(from), "no {from:?} to replace");
    text.replacen(from, to, 1).into_bytes()
}

fn inputs() -> Vec<Input> {
    let newest = data("v0.5.0-first-run.ini");
    let mut out: Vec<Input> = FIRST_RUNS
        .iter()
        .map(|name| Input {
            name: name.to_string(),
            bytes: Some(data(name)),
        })
        .collect();
    out.push(Input {
        name: "no file".into(),
        bytes: None,
    });
    out.push(Input {
        name: "empty file".into(),
        bytes: Some(Vec::new()),
    });
    // v0.4.0 and earlier read fov_h; v0.5.0 only logs it.
    out.push(Input {
        name: "v0.4.0 first run, fov_h set".into(),
        bytes: Some(replaced(
            &data("v0.4.0-first-run.ini"),
            "; fov_h = 100",
            "fov_h = 100",
        )),
    });
    // Saved by an editor in a code page: read_to_string fails, and v0.5.0 ran on the
    // defaults after writing its template over the file. The migration defers.
    let mut code_page = newest.clone();
    code_page.extend_from_slice(b"; 40\xB0\n");
    out.push(Input {
        name: "v0.5.0 first run with a non-UTF-8 comment".into(),
        bytes: Some(code_page),
    });
    for (name, from, to) in [
        (
            "LocalSmoothing=-0",
            "LocalSmoothing=0.0",
            "LocalSmoothing=-0",
        ),
        (
            "RemoteSmoothing=1e-300",
            "RemoteSmoothing=0.15",
            "RemoteSmoothing=1e-300",
        ),
        (
            "RemoteSmoothing=.5",
            "RemoteSmoothing=0.15",
            "RemoteSmoothing=.5",
        ),
        ("YawModeKey=0x48", "YawModeKey=0x22", "YawModeKey=0x48"),
        ("YawModeKey=0x23", "YawModeKey=0x22", "YawModeKey=0x23"),
        ("YawModeKey=0x11", "YawModeKey=0x22", "YawModeKey=0x11"),
        ("YawModeKey=0xFE", "YawModeKey=0x22", "YawModeKey=0xFE"),
        ("YawModeKey=1", "YawModeKey=0x22", "YawModeKey=1"),
    ] {
        out.push(Input {
            name: format!("v0.5.0 first run, {name}"),
            bytes: Some(replaced(&newest, from, to)),
        });
    }
    for (name, bytes) in corpus::generate(&newest, legacy_config::KEYS, &descriptors()) {
        out.push(Input {
            name,
            bytes: Some(bytes),
        });
    }
    out
}

// ---- the tests ------------------------------------------------------------------------------

/// The committed v0.5.0 first-run file is what the oracle writes where it finds none.
fn test_the_committed_first_run_is_the_oracles(root: &Path) -> Vec<String> {
    let dir = fresh_dir(root);
    let output = Command::new(std::env::current_exe().unwrap())
        .arg("--oracle")
        .arg(&dir)
        .output()
        .unwrap();
    assert!(output.status.success());
    let written = std::fs::read(dir.join(LEGACY_NAME)).unwrap();
    if written == data("v0.5.0-first-run.ini") {
        Vec::new()
    } else {
        vec!["data/v0.5.0-first-run.ini is not the oracle's first-run output".into()]
    }
}

/// Comparison 1: no commit since v0.5.0 changed how the file is read.
fn test_comparison_one_oracle_against_import(root: &Path, inputs: &[Input]) -> Vec<String> {
    let mut failures = Vec::new();
    for input in inputs {
        let o = read_oracle(root, input);
        let i = read_import(root, input);
        for (field, d) in differences(&o, &i) {
            failures.push(format!("{}: oracle/import {field} {d}", input.name));
        }
    }
    failures
}

const CANONICAL: i32 = 0;
const MIGRATED: i32 = 1;
const CREATED: i32 = 2;
const DEFERRED: i32 = 3;
const FILE_ATTRIBUTE_READONLY: u32 = 1;

/// A file as the tests hold it to: its bytes, its last write time and its attributes.
#[derive(Debug, PartialEq)]
struct Stamp {
    bytes: Vec<u8>,
    written: std::time::SystemTime,
    attributes: u32,
}

fn stamp(path: &Path) -> Stamp {
    use std::os::windows::fs::MetadataExt;
    let metadata = std::fs::metadata(path).unwrap();
    Stamp {
        bytes: std::fs::read(path).unwrap(),
        written: metadata.modified().unwrap(),
        attributes: metadata.file_attributes(),
    }
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).unwrap();
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn mentions(lines: &[Line], text: &str) -> bool {
    lines.iter().any(|line| match line {
        Line::Info(l) | Line::Warning(l) => l.contains(text),
    })
}

/// One migration, in a game folder of its own that holds the input as the legacy file, and
/// what it left there.
struct Migration {
    dir: PathBuf,
    config: PathBuf,
    legacy: PathBuf,
    legacy_before: Option<Stamp>,
    status: i32,
    settings: Settings,
    log: Vec<Line>,
    owner: Owner,
}

fn migrate(root: &Path, input: &Input, defaults: &Path, read_only: bool) -> Migration {
    let dir = fresh_dir(root);
    let legacy = place(&dir, input);
    let legacy_before = input.bytes.as_ref().map(|_| {
        if read_only {
            set_read_only(&legacy, true);
        }
        stamp(&legacy)
    });
    let owner = Owner::at(&dir, defaults);
    let (status, settings, log) = owner.load();
    Migration {
        config: dir.join("CameraUnlock.ini"),
        dir,
        legacy,
        legacy_before,
        status,
        settings,
        log,
        owner,
    }
}

fn committed() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("config/CameraUnlock.ini")).unwrap()
}

/// Comparison 2, and everything the conversion promises about the files it leaves behind,
/// over one Defaults.ini. `migrated` collects every distinct CameraUnlock.ini written.
fn test_comparison_two_import_against_migration(
    root: &Path,
    inputs: &[Input],
    defaults: &Path,
    builtin: bool,
    migrated: &mut BTreeSet<Vec<u8>>,
    known: &mut BTreeSet<&'static str>,
) -> Vec<String> {
    let over = if builtin {
        "Defaults.ini at the built-in values"
    } else {
        "Defaults.ini changed"
    };
    let committed = committed();
    let defaults_before = stamp(defaults);
    let mut failures = Vec::new();
    let mut check = |ok: bool, what: String| {
        if !ok {
            failures.push(what);
        }
    };
    for input in inputs {
        let n = format!("{} ({over})", input.name);
        let i = read_import(root, input);
        let m = migrate(root, input, defaults, false);
        let g = from_migration(&m.settings, &m.owner);

        let Some(legacy_before) = &m.legacy_before else {
            // Not a migration: a fresh install, which follows Defaults.ini.
            check(m.status == CREATED, format!("{n}: the file is created"));
            check(
                std::fs::read(&m.config).unwrap() == committed,
                format!("{n}: the created file is the committed one"),
            );
            check(
                listing(&m.dir) == ["CameraUnlock.ini"],
                format!("{n}: the folder holds CameraUnlock.ini and nothing else"),
            );
            if builtin {
                for (field, d) in differences(&i, &g) {
                    if KNOWN_CHANGES.iter().any(|(k, _)| *k == field) {
                        known.insert(field);
                    } else {
                        check(false, format!("{n}: import/created {field} {d}"));
                    }
                }
            }
            continue;
        };

        for (field, d) in differences(&i, &g) {
            if KNOWN_CHANGES.iter().any(|(k, _)| *k == field) {
                known.insert(field);
            } else {
                check(false, format!("{n}: import/migration {field} {d}"));
            }
        }
        check(
            &stamp(&m.legacy) == legacy_before,
            format!("{n}: {LEGACY_NAME} is left exactly as it was"),
        );
        check(
            stamp(defaults) == defaults_before,
            format!("{n}: Defaults.ini is left exactly as it was"),
        );
        if unreadable(&m.legacy) {
            check(
                m.status == DEFERRED,
                format!(
                    "{n}: the import is deferred, status {}: {:?}",
                    m.status, m.log
                ),
            );
            check(
                listing(&m.dir) == [LEGACY_NAME],
                format!("{n}: the folder holds {LEGACY_NAME} and nothing else"),
            );
            check(
                mentions(
                    &m.log,
                    "it is not UTF-8 text. Save it as UTF-8 to import it",
                ),
                format!("{n}: the log says why {LEGACY_NAME} was not imported"),
            );
            let (status, _, _) = Owner::at(&m.dir, defaults).load();
            check(
                status == DEFERRED && listing(&m.dir) == [LEGACY_NAME],
                format!("{n}: the next launch tries the import again"),
            );
            check(
                &stamp(&m.legacy) == legacy_before,
                format!("{n}: the next launch leaves {LEGACY_NAME} as it was"),
            );
            continue;
        }
        check(
            m.status == MIGRATED,
            format!(
                "{n}: the file is imported, status {}: {:?}",
                m.status, m.log
            ),
        );
        if m.status != MIGRATED {
            continue;
        }
        check(
            listing(&m.dir) == ["CameraUnlock.ini", LEGACY_NAME],
            format!("{n}: the folder holds CameraUnlock.ini and {LEGACY_NAME} and nothing else"),
        );
        check(
            mentions(&m.log, "created from"),
            format!("{n}: the log says where CameraUnlock.ini came from"),
        );
        let written = std::fs::read(&m.config).unwrap();
        migrated.insert(written.clone());

        // The next launch reads CameraUnlock.ini over the same Defaults.ini, to the same
        // settings, does not import, and writes neither file.
        let again = Owner::at(&m.dir, defaults);
        let (status, settings, log) = again.load();
        check(
            status == CANONICAL,
            format!("{n}: the next launch reads CameraUnlock.ini"),
        );
        check(
            settings == m.settings
                && support::render_loaded(&again) == support::render_loaded(&m.owner),
            format!("{n}: the next launch runs on the same settings"),
        );
        check(
            !mentions(&log, "created from"),
            format!("{n}: the next launch does not import"),
        );
        check(
            mentions(&log, "is left as it was and is not read"),
            format!("{n}: the next launch says {LEGACY_NAME} is not read"),
        );
        check(
            std::fs::read(&m.config).unwrap() == written,
            format!("{n}: the next launch leaves CameraUnlock.ini as it was"),
        );
        check(
            &stamp(&m.legacy) == legacy_before,
            format!("{n}: the next launch leaves {LEGACY_NAME} as it was"),
        );

        // A read-only legacy file imports as a writable one does and stays read-only.
        if builtin {
            let r = migrate(root, input, defaults, true);
            check(
                r.status == m.status
                    && support::render_loaded(&r.owner) == support::render_loaded(&m.owner)
                    && std::fs::read(&r.config).unwrap() == written,
                format!("{n}: a read-only {LEGACY_NAME} imports as a writable one does"),
            );
            let before = r.legacy_before.as_ref().unwrap();
            check(
                &stamp(&r.legacy) == before && before.attributes & FILE_ATTRIBUTE_READONLY != 0,
                format!("{n}: a read-only {LEGACY_NAME} keeps its attribute, bytes and write time"),
            );
            set_read_only(&r.legacy, false);
        }
    }
    failures
}

/// Every published build's first-run file imports into exactly the file a fresh install
/// creates, with Defaults.ini at the built-in values.
fn test_fresh_equals_upgrade(root: &Path, defaults: &Path) -> Vec<String> {
    let committed = committed();
    FIRST_RUNS
        .iter()
        .filter_map(|name| {
            let input = Input {
                name: name.to_string(),
                bytes: Some(data(name)),
            };
            let m = migrate(root, &input, defaults, false);
            (m.status != MIGRATED || std::fs::read(&m.config).unwrap() != committed).then(|| {
                format!("{name} does not import into config/CameraUnlock.ini byte for byte")
            })
        })
        .collect()
}

/// Defaults.ini as a player may have changed it, from the one the owner created: every value
/// this game reads differs from the built-in one, the tracking mode pair naming rotation only.
fn write_altered_defaults(builtin: &Path, altered: &Path) {
    let mut text = String::from_utf8(std::fs::read(builtin).unwrap()).unwrap();
    for (from, to) in [
        ("UdpPort=4242", "UdpPort=5000"),
        ("EnableOnStartup=true", "EnableOnStartup=false"),
        ("WorldSpaceYaw=true", "WorldSpaceYaw=false"),
        ("PositionEnabled=true", "PositionEnabled=false"),
        ("CollisionEnabled=true", "CollisionEnabled=false"),
        (
            "CollisionReleaseSmoothing=0.9",
            "CollisionReleaseSmoothing=0.5",
        ),
        ("LocalSmoothing=0.0", "LocalSmoothing=0.3"),
        ("RemoteSmoothing=0.15", "RemoteSmoothing=0.5"),
        ("ToggleKey=End, Ctrl+Shift+Y", "ToggleKey=F8"),
        (
            "CycleTrackingModeKey=PageUp, Ctrl+Shift+G",
            "CycleTrackingModeKey=F9",
        ),
        ("YawModeKey=PageDown, Ctrl+Shift+H", "YawModeKey=F10"),
    ] {
        let line = format!("\r\n{from}\r\n");
        assert!(
            text.contains(&line),
            "the created Defaults.ini has no line {from}"
        );
        text = text.replacen(&line, &format!("\r\n{to}\r\n"), 1);
    }
    std::fs::create_dir_all(altered.parent().unwrap()).unwrap();
    std::fs::write(altered, text).unwrap();
}

/// Each distinct migrated file, for lint-migrated.mjs.
fn write_migrated_files(migrated: &BTreeSet<Vec<u8>>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/config-differential-migrated");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (n, file) in migrated.iter().enumerate() {
        std::fs::write(dir.join(format!("{n}.ini")), file).unwrap();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--oracle" {
        run_oracle(Path::new(&args[2]));
        return;
    }

    let root = std::env::temp_dir().join(format!("bsr-config-differential-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    // The owner creates Defaults.ini here at the built-in values on the first load, in a
    // CameraUnlock folder whose parent has to exist.
    let builtin_defaults = root.join("user-builtin/CameraUnlock/Defaults.ini");
    let altered_defaults = root.join("user-altered/CameraUnlock/Defaults.ini");
    std::fs::create_dir_all(root.join("user-builtin")).unwrap();

    let inputs = inputs();
    let mut failures = test_the_v050_hotkeys_are_its_source();
    failures.extend(test_the_committed_first_run_is_the_oracles(&root));
    failures.extend(test_fresh_equals_upgrade(&root, &builtin_defaults));
    write_altered_defaults(&builtin_defaults, &altered_defaults);
    failures.extend(test_comparison_one_oracle_against_import(&root, &inputs));
    let mut migrated = BTreeSet::new();
    let mut known = BTreeSet::new();
    failures.extend(test_comparison_two_import_against_migration(
        &root,
        &inputs,
        &builtin_defaults,
        true,
        &mut migrated,
        &mut known,
    ));
    failures.extend(test_comparison_two_import_against_migration(
        &root,
        &inputs,
        &altered_defaults,
        false,
        &mut migrated,
        &mut known,
    ));
    for (field, commit) in KNOWN_CHANGES {
        if !known.contains(field) {
            failures.push(format!(
                "the known change to {field} ({commit}) no longer occurs"
            ));
        }
    }
    write_migrated_files(&migrated);

    std::fs::remove_dir_all(&root).unwrap();
    for f in failures.iter().take(200) {
        println!("FAIL {f}");
    }
    println!(
        "{} inputs, {} distinct migrated files; comparison 1 finds no difference and comparison 2 only the known changes: {}",
        inputs.len(),
        migrated.len(),
        failures.is_empty()
    );
    assert!(failures.is_empty(), "{} failures", failures.len());
}
