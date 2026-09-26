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

fn main() {
    cpp().file("src/lean_clamp.cpp").compile("lean_clamp");
    println!("cargo:rerun-if-changed=src/lean_clamp.cpp");
    println!("cargo:rerun-if-changed=cameraunlock-core/cpp/include");

    // The differential and config tests reach core through these. Cargo sets the variable
    // only for a build with the test-support feature, so the shipped DLL never carries them.
    if std::env::var_os("CARGO_FEATURE_TEST_SUPPORT").is_some() {
        cpp()
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
