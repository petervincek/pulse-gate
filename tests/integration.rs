// Rust auto-discovers every file in the top-level tests directory as a separate integration test target
// that means that these files are valid standalone binaries, so the shareable infra is shared just in the
// context of that individual binary
mod common;
mod integration_tests;
