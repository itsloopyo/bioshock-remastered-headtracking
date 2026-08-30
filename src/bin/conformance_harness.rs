//! Entry point for the shared pipeline conformance vectors. All the work is in
//! `conformance::run`; this exists only because a Rust binary has to be its own
//! crate and cannot see the library's internals from here.
fn main() {
    bioshock_headtrack::conformance::run();
}
