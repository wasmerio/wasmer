//! The logic that gets executed before building the binary and tests.
//! We use it to auto-generate the Wasm spectests for each of the
//! available compilers.
//!
//! Please try to keep this file as clean as possible.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use test_generator::{
    build_ignores_from_textfile, test_directory, test_directory_module, wast_processor,
    with_features, with_test_module, Testsuite,
};
use wasi_test_generator;

static WASITESTS_ENV_VAR: &str = "WASM_WASI_GENERATE_WASITESTS";
static WASITESTS_SET_UP_TOOLCHAIN: &str = "WASM_WASI_SET_UP_TOOLCHAIN";
static WASITESTS_GENERATE_ALL: &str = "WASI_TEST_GENERATE_ALL";

fn is_truthy_env(name: &str) -> bool {
    env::var(name).map(|n| n == "1").unwrap_or_default()
}

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=tests/ignores.txt");
    println!("cargo:rerun-if-env-changed={}", WASITESTS_ENV_VAR);
    println!("cargo:rerun-if-env-changed={}", WASITESTS_SET_UP_TOOLCHAIN);
    println!("cargo:rerun-if-env-changed={}", WASITESTS_GENERATE_ALL);

    let wasi_versions = if is_truthy_env(WASITESTS_GENERATE_ALL) {
        wasi_test_generator::ALL_WASI_VERSIONS
    } else {
        wasi_test_generator::LATEST_WASI_VERSION
    };

    // Install the Rust WASI toolchains for each of the versions
    if is_truthy_env(WASITESTS_SET_UP_TOOLCHAIN) {
        wasi_test_generator::install_toolchains(wasi_versions);
    }

    // Generate the WASI Wasm files
    if is_truthy_env(WASITESTS_ENV_VAR) {
        wasi_test_generator::build(wasi_versions);
    }

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR").expect("The OUT_DIR environment variable must be set"),
    );
    let ignores = build_ignores_from_textfile("tests/ignores.txt".into())?;

    // Spectests test generation
    let mut spectests = Testsuite {
        buffer: String::new(),
        path: vec![],
        ignores,
    };

    let backends = vec!["singlepass", "cranelift", "llvm"];
    with_features(&mut spectests, &backends, |mut spectests| {
        with_test_module(&mut spectests, "spec", |spectests| {
            let _spec_tests = test_directory(spectests, "tests/wast/spec", wast_processor)?;
            test_directory_module(
                spectests,
                "tests/wast/spec/proposals/multi-value",
                wast_processor,
            )?;
            // test_directory_module(spectests, "tests/wast/spec/proposals/bulk-memory-operations", wast_processor)?;
            Ok(())
        })?;
        with_test_module(&mut spectests, "wasmer", |spectests| {
            let _spec_tests = test_directory(spectests, "tests/wast/wasmer", wast_processor)?;
            Ok(())
        })?;
        Ok(())
    })?;

    let spectests_output = out_dir.join("generated_spectests.rs");
    fs::write(&spectests_output, spectests.buffer)?;

    // Write out our auto-generated tests and opportunistically format them with
    // `rustfmt` if it's installed.
    // Note: We need drop because we don't want to run `unwrap` or `expect` as
    // the command might fail, but we don't care about it's result.
    drop(Command::new("rustfmt").arg(&spectests_output).status());

    Ok(())
}
