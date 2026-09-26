fn cpp() -> cc::Build {
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flag("/EHsc")
        .define("WIN32_LEAN_AND_MEAN", None)
        .define("NOMINMAX", None)
        .include("cameraunlock-core/cpp/include");
    build
}

// cameraunlock-core's config owner and its dependencies, the hotkey list parser and the
// poller that runs the lists.
const CORE_SOURCES: &[&str] = &[
    "config/canonical_ini.cpp",
    "config/checked_file_writer.cpp",
    "config/config_owner.cpp",
    "config/config_table.cpp",
    "config/defaults_ini.cpp",
    "config/defaults_location.cpp",
    "config/head_tracking_config.cpp",
    "config/hotkey_codec.cpp",
    "config/ini_editor.cpp",
    "config/ini_reader.cpp",
    "config/legacy_import.cpp",
    "config/value_codecs.cpp",
    "config/value_guards.cpp",
    "input/hotkey_poller.cpp",
    "input/key_bindings.cpp",
];

fn main() {
    let mut mod_cpp = cpp();
    mod_cpp
        .file("src/lean_clamp.cpp")
        .file("src/config_owner.cpp");
    for source in CORE_SOURCES {
        mod_cpp.file(format!("cameraunlock-core/cpp/src/{source}"));
    }
    mod_cpp.compile("bsr_cpp");
    println!("cargo:rerun-if-changed=src/lean_clamp.cpp");
    println!("cargo:rerun-if-changed=src/config_owner.cpp");
    println!("cargo:rerun-if-changed=src/config_owner.h");
    println!("cargo:rerun-if-changed=cameraunlock-core/cpp/include");
    println!("cargo:rerun-if-changed=cameraunlock-core/cpp/src");

    // The differential and config tests reach core through these. Cargo sets the variable
    // only for a build with the test-support feature, so the shipped DLL never carries them.
    if std::env::var_os("CARGO_FEATURE_TEST_SUPPORT").is_some() {
        cpp()
            .include("src")
            .file("tests/support/test_support.cpp")
            .compile("bsr_test_support");
        println!("cargo:rerun-if-changed=tests/support");
    }

    // Link the module definition file to set correct ordinal exports
    // This is required for XInput DLL proxy - games call by ordinal
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let def_path = format!("{}\\xinput1_3.def", manifest_dir);

    // MSVC linker syntax
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def_path);

    // Rerun if def file changes
    println!("cargo:rerun-if-changed=xinput1_3.def");
}
