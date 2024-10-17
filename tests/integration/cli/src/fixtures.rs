//! Paths for commonly used test files.

use std::path::{Path, PathBuf};

use crate::{asset_path, c_asset_path};

pub fn resources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

pub fn packages() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("packages")
}

pub fn php() -> (PathBuf, PathBuf, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let resources = resources().join("php");
    (
        root.join("tests").join("wasm").join("php.wasm"),
        resources.clone(),
        resources.join("db"),
    )
}

/// A WEBC file containing the Python interpreter, compiled to WASI.
pub fn python() -> PathBuf {
    c_asset_path().join("python-0.1.0.wasmer")
}

/// A WEBC file containing the coreutils.
pub fn coreutils() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("webc")
        .join("coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc")
}

/// A WEBC file containing bash.
pub fn bash() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("webc")
        .join("bash-1.0.16-f097441a-a80b-4e0d-87d7-684918ef4bb6.webc")
}

/// A WEBC file containing `wat2wasm`, `wasm-validate`, and other helpful
/// WebAssembly-related commands.
pub fn wabt() -> PathBuf {
    c_asset_path().join("wabt-1.0.37.wasmer")
}

/// A WEBC file containing the WCGI static server.
pub fn static_server() -> PathBuf {
    c_asset_path().join("staticserver.webc")
}

/// The QuickJS interpreter, compiled to a WASI module.
pub fn qjs() -> PathBuf {
    c_asset_path().join("qjs.wasm")
}

pub fn hello() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("webc")
        .join("hello-0.1.0-665d2ddc-80e6-4845-85d3-4587b1693bb7.webc")
}

/// The `wasmer.toml` file for QuickJS.
pub fn qjs_wasmer_toml() -> PathBuf {
    c_asset_path().join("qjs-wasmer.toml")
}

/// A `*.wat` file which calculates fib(40) and exits with no output.
pub fn fib() -> PathBuf {
    asset_path().join("fib.wat")
}

/// A `*.wat` file with no `_start()` function.
pub fn wat_no_start() -> PathBuf {
    asset_path().join("no_start.wat")
}
