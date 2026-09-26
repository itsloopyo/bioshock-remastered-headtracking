//! The differential test for the config conversion. Every input is read three ways:
//!
//!   the oracle     the reader of v0.5.0, the newest published build: oracle/config.rs is
//!                  `git show v0.5.0:src/config.rs`, byte for byte, with the two crate items it
//!                  used restated below at their v0.5.0 values. It reads a relative path and
//!                  keeps its settings in statics, so each reading runs in a process of its own
//!                  (this binary, started with --oracle <folder>)
//!   the import     the frozen reader in src/legacy_config/
//!   the migration  the config owner in a folder holding only bioshock_headtrack.ini, importing
//!                  it into a new CameraUnlock.ini, then the canonical reader and the table on
//!                  what it wrote
//!
//! Each reading is reduced to what a player's file decides: every setting, the tracking state
//! the mod starts in and the keys it registers.
//!
//! Comparison 1, oracle against import, finds nothing: no commit since v0.5.0 changed how the
//! file is read. Comparison 2, import against migration, finds nothing either: no approved
//! change or normalisation in core's data/config-format.json applies to what v0.5.0 read. It
//! runs over a Defaults.ini at the built-in values and over one a player changed, since the
//! migration writes default exactly where the imported value equals what Defaults.ini gives.
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
//! v0.3.6, v0.4.0 and v0.5.0; there is no dev pre-release. SHA-256 of the frozen files:
//!   oracle/config.rs            5d3b80b8b5940728acbf91e3f72c36f8b928d8951067d8a897d6a147d670ca6c,
//!                               equal to `git show v0.5.0:src/config.rs`
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

mod smoothing {
    // src/smoothing.rs at v0.5.0.
    pub const DEFAULT_LOCAL_SMOOTHING: f64 = 0.0;
    pub const DEFAULT_REMOTE_SMOOTHING: f64 = 0.15;
}

mod tracking {
    // src/tracking.rs at v0.5.0: ATOMIC_WORLD_SPACE_YAW starts true.
    use std::sync::atomic::{AtomicBool, Ordering};

    static WORLD_SPACE_YAW: AtomicBool = AtomicBool::new(true);

    pub fn set_world_space_yaw_initial(enabled: bool) {
        WORLD_SPACE_YAW.store(enabled, Ordering::Release);
    }

    pub fn is_world_space_yaw_atomic() -> bool {
        WORLD_SPACE_YAW.load(Ordering::Acquire)
    }
}

#[path = "oracle/config.rs"]
#[allow(dead_code, unused_imports)]
mod oracle;

const LEGACY_NAME: &str = "bioshock_headtrack.ini";
const CTRL_SHIFT: u32 = 3;

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
}

/// v0.5.0 as it ran on what its reader gave: tracking on at start, both axes on, port 4242,
/// a lean held off the walls on every frame at LeanClampSettings' release_smoothing of 0.9
/// (src/lean_clamp.cpp and core's lean_clamp.h at v0.5.0's pin, c480d8a), End or
/// Ctrl+Shift+Y, PageUp or Ctrl+Shift+G, and the yaw key or Ctrl+Shift+H.
fn from_legacy(world_space_yaw: bool, yaw_mode_key: i32, local: f64, remote: f64) -> Effective {
    Effective {
        enable_on_startup: true,
        udp_port: 4242,
        world_space_yaw,
        local_smoothing_bits: local.to_bits(),
        remote_smoothing_bits: remote.to_bits(),
        rotation_enabled: true,
        position_enabled: true,
        collision_enabled: true,
        collision_release_smoothing_bits: 0.9_f32.to_bits(),
        toggle: vec![(0, 0x23), (CTRL_SHIFT, 'Y' as i32)],
        cycle_mode: vec![(0, 0x21), (CTRL_SHIFT, 'G' as i32)],
        yaw_mode: vec![(0, yaw_mode_key), (CTRL_SHIFT, 'H' as i32)],
    }
}

fn differences(a: &Effective, b: &Effective) -> Vec<String> {
    let mut out = Vec::new();
    macro_rules! field {
        ($f:ident) => {
            if a.$f != b.$f {
                out.push(format!("{} {:?} / {:?}", stringify!($f), a.$f, b.$f));
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
    out
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
    from_legacy(
        fields[0] == "1",
        fields[1].parse().unwrap(),
        f64::from_bits(u64::from_str_radix(fields[2], 16).unwrap()),
        f64::from_bits(u64::from_str_radix(fields[3], 16).unwrap()),
    )
}

/// The oracle's side of read_oracle: v0.5.0's load() in `dir`, printed.
fn run_oracle(dir: &Path) {
    std::env::set_current_dir(dir).unwrap();
    oracle::load();
    println!(
        "{} {} {:016x} {:016x}",
        u8::from(tracking::is_world_space_yaw_atomic()),
        oracle::yaw_mode_key(),
        oracle::local_smoothing().to_bits(),
        oracle::remote_smoothing().to_bits()
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

fn from_import(c: &legacy_config::Config) -> Effective {
    from_legacy(
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
        for d in differences(&o, &i) {
            failures.push(format!("{}: oracle/import {d}", input.name));
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
                for d in differences(&i, &g) {
                    check(false, format!("{n}: import/created {d}"));
                }
            }
            continue;
        };

        for d in differences(&i, &g) {
            check(false, format!("{n}: import/migration {d}"));
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
    let mut failures = test_the_committed_first_run_is_the_oracles(&root);
    failures.extend(test_fresh_equals_upgrade(&root, &builtin_defaults));
    write_altered_defaults(&builtin_defaults, &altered_defaults);
    failures.extend(test_comparison_one_oracle_against_import(&root, &inputs));
    let mut migrated = BTreeSet::new();
    failures.extend(test_comparison_two_import_against_migration(
        &root,
        &inputs,
        &builtin_defaults,
        true,
        &mut migrated,
    ));
    failures.extend(test_comparison_two_import_against_migration(
        &root,
        &inputs,
        &altered_defaults,
        false,
        &mut migrated,
    ));
    write_migrated_files(&migrated);

    std::fs::remove_dir_all(&root).unwrap();
    for f in failures.iter().take(200) {
        println!("FAIL {f}");
    }
    println!(
        "{} inputs, {} distinct migrated files; comparisons 1 and 2 find no difference: {}",
        inputs.len(),
        migrated.len(),
        failures.is_empty()
    );
    assert!(failures.is_empty(), "{} failures", failures.len());
}
