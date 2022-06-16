use std::env;
use std::path::PathBuf;

pub const C_ASSET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../lib/c-api/examples/assets"
);
pub const ASSET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tests/examples");

pub const WASMER_INCLUDE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../lib/c-api");

#[cfg(feature = "debug")]
pub const WASMER_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../target/debug/wasmer");

#[cfg(not(feature = "debug"))]
pub const WASMER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/release/wasmer"
);

#[cfg(feature = "debug")]
pub const WASMER_TARGET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../",
    env!("CARGO_CFG_TARGET"),
    "/debug/wasmer"
);

/* env var TARGET is set by tests/integration/cli/build.rs on compile-time */

#[cfg(not(feature = "debug"))]
pub const WASMER_TARGET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/",
    env!("TARGET"),
    "/release/wasmer"
);

#[cfg(not(windows))]
pub const LIBWASMER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/release/libwasmer.a"
);

#[cfg(windows)]
pub const LIBWASMER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target/release/wasmer.lib"
);

#[cfg(not(windows))]
pub const LIBWASMER_TARGET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../target",
    env!("TARGET"),
    "/release/libwasmer.a"
);

#[cfg(windows)]
pub const LIBWASMER_TARGET_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../",
    env!("TARGET"),
    "/release/wasmer.lib"
);

/// Get the path to the `libwasmer.a` static library.
pub fn get_libwasmer_path() -> PathBuf {
    let mut ret = PathBuf::from(
        env::var("WASMER_TEST_LIBWASMER_PATH").unwrap_or_else(|_| LIBWASMER_PATH.to_string()),
    );
    if !ret.exists() {
        ret = PathBuf::from(LIBWASMER_TARGET_PATH.to_string());
    }
    if !ret.exists() {
        panic!("Could not find libwasmer path! {:?}", ret);
    }
    ret
}

/// Get the path to the `wasmer` executable to be used in this test.
pub fn get_wasmer_path() -> PathBuf {
    let mut ret = PathBuf::from(
        env::var("WASMER_TEST_WASMER_PATH").unwrap_or_else(|_| WASMER_PATH.to_string()),
    );
    if !ret.exists() {
        ret = PathBuf::from(WASMER_TARGET_PATH.to_string());
    }
    if !ret.exists() {
        panic!("Could not find wasmer executable path! {:?}", ret);
    }
    ret
}
