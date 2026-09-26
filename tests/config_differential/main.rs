//! The differential test for the config conversion. Every input is read two ways:
//!
//!   the oracle   the reader of v0.5.0, the newest published build: oracle/config.rs is
//!                `git show v0.5.0:src/config.rs`, byte for byte, with the two crate items it
//!                used restated below at their v0.5.0 values. It reads a relative path and
//!                keeps its settings in statics, so each reading runs in a process of its own
//!                (this binary, started with --oracle <folder>)
//!   the import   the frozen reader in src/legacy_config/
//!
//! Each reading is reduced to what a player's file decides: every setting, the tracking state
//! the mod starts in and the keys it binds.
//!
//! Comparison 1, oracle against import, finds nothing: no commit since v0.5.0 changed how the
//! file is read.
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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use bioshock_headtrack::legacy_config;

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
    toggle: Vec<(u32, i32)>,
    cycle_mode: Vec<(u32, i32)>,
    yaw_mode: Vec<(u32, i32)>,
}

/// v0.5.0 as it ran on what its reader gave: tracking on at start, both axes on, port 4242,
/// End or Ctrl+Shift+Y, PageUp or Ctrl+Shift+G, and the yaw key or Ctrl+Shift+H.
fn from_legacy(world_space_yaw: bool, yaw_mode_key: i32, local: f64, remote: f64) -> Effective {
    Effective {
        enable_on_startup: true,
        udp_port: 4242,
        world_space_yaw,
        local_smoothing_bits: local.to_bits(),
        remote_smoothing_bits: remote.to_bits(),
        rotation_enabled: true,
        position_enabled: true,
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

mod corpus {
    use std::ffi::{c_char, c_int, c_void, CString};

    #[repr(C)]
    struct LegacyKey {
        section: *const c_char,
        key: *const c_char,
    }

    #[repr(C)]
    struct MutationKey {
        section: *const c_char,
        key: *const c_char,
        alternate: *const c_char,
        out_of_range: *const *const c_char,
        out_of_range_len: usize,
        hotkey: c_int,
    }

    type EmitPair = unsafe extern "C" fn(*mut c_void, *const c_char, usize, *const c_char, usize);
    type EmitText = unsafe extern "C" fn(*mut c_void, *const c_char, usize);

    extern "C" {
        fn bsr_test_ini_mutations(
            base: *const c_char,
            base_len: usize,
            reads: *const LegacyKey,
            reads_len: usize,
            keys: *const MutationKey,
            keys_len: usize,
            emit: EmitPair,
            error: EmitText,
            context: *mut c_void,
        ) -> c_int;
    }

    /// One key the frozen reader reads, for core's corpus generator.
    pub struct Descriptor {
        pub section: &'static str,
        pub key: &'static str,
        pub alternate: &'static str,
        pub out_of_range: &'static [&'static str],
        pub hotkey: bool,
    }

    struct Sink {
        out: Vec<(String, Vec<u8>)>,
        error: Option<String>,
    }

    unsafe fn slice<'a>(data: *const c_char, len: usize) -> &'a [u8] {
        std::slice::from_raw_parts(data.cast(), len)
    }

    unsafe extern "C" fn emit(
        context: *mut c_void,
        name: *const c_char,
        name_len: usize,
        bytes: *const c_char,
        bytes_len: usize,
    ) {
        let sink = &mut *context.cast::<Sink>();
        sink.out.push((
            String::from_utf8(slice(name, name_len).to_vec()).unwrap(),
            slice(bytes, bytes_len).to_vec(),
        ));
    }

    unsafe extern "C" fn error(context: *mut c_void, text: *const c_char, len: usize) {
        let sink = &mut *context.cast::<Sink>();
        sink.error = Some(String::from_utf8_lossy(slice(text, len)).into_owned());
    }

    /// core's GenerateIniMutations over `base`, reading `reads` and describing each with
    /// `keys`. It refuses keys and descriptors that differ.
    pub fn generate(
        base: &[u8],
        reads: &[(&str, &str)],
        keys: &[Descriptor],
    ) -> Vec<(String, Vec<u8>)> {
        let c = |s: &str| CString::new(s).unwrap();
        let read_strings: Vec<(CString, CString)> =
            reads.iter().map(|(s, k)| (c(s), c(k))).collect();
        let read_keys: Vec<LegacyKey> = read_strings
            .iter()
            .map(|(s, k)| LegacyKey {
                section: s.as_ptr(),
                key: k.as_ptr(),
            })
            .collect();
        let key_strings: Vec<(CString, CString, CString, Vec<CString>)> = keys
            .iter()
            .map(|d| {
                (
                    c(d.section),
                    c(d.key),
                    c(d.alternate),
                    d.out_of_range.iter().map(|v| c(v)).collect(),
                )
            })
            .collect();
        let range_pointers: Vec<Vec<*const c_char>> = key_strings
            .iter()
            .map(|(_, _, _, r)| r.iter().map(|v| v.as_ptr()).collect())
            .collect();
        let mutation_keys: Vec<MutationKey> = keys
            .iter()
            .enumerate()
            .map(|(i, d)| MutationKey {
                section: key_strings[i].0.as_ptr(),
                key: key_strings[i].1.as_ptr(),
                alternate: key_strings[i].2.as_ptr(),
                out_of_range: range_pointers[i].as_ptr(),
                out_of_range_len: range_pointers[i].len(),
                hotkey: c_int::from(d.hotkey),
            })
            .collect();
        let mut sink = Sink {
            out: Vec::new(),
            error: None,
        };
        let status = unsafe {
            bsr_test_ini_mutations(
                base.as_ptr().cast(),
                base.len(),
                read_keys.as_ptr(),
                read_keys.len(),
                mutation_keys.as_ptr(),
                mutation_keys.len(),
                emit,
                error,
                (&mut sink as *mut Sink).cast(),
            )
        };
        assert_eq!(status, 0, "the corpus generator refused: {:?}", sink.error);
        sink.out
    }
}

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
    // defaults after writing its template over the file.
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--oracle" {
        run_oracle(Path::new(&args[2]));
        return;
    }

    let root = std::env::temp_dir().join(format!("bsr-config-differential-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let inputs = inputs();
    let mut failures = test_the_committed_first_run_is_the_oracles(&root);
    failures.extend(test_comparison_one_oracle_against_import(&root, &inputs));

    std::fs::remove_dir_all(&root).unwrap();
    for f in failures.iter().take(200) {
        println!("FAIL {f}");
    }
    println!(
        "{} inputs; comparison 1 finds no difference from v0.5.0: {}",
        inputs.len(),
        failures.is_empty()
    );
    assert!(failures.is_empty(), "{} failures", failures.len());
}
