//! A shim that makes the `wasmer` CLI available to integration tests via the
//! [`$CARGO_BIN_EXE_wasmer-cli-shim`][var] environment variable.
//!
//! [var]: https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-sets-for-crates

fn main() {
    wasmer_cli::cli::wasmer_main();
}
