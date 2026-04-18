fn main() {
    // Link the module definition file to set correct ordinal exports
    // This is required for XInput DLL proxy - games call by ordinal
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let def_path = format!("{}\\xinput1_3.def", manifest_dir);

    // MSVC linker syntax
    println!("cargo:rustc-cdylib-link-arg=/DEF:{}", def_path);

    // Rerun if def file changes
    println!("cargo:rerun-if-changed=xinput1_3.def");
}
