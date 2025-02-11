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
    test_directory, test_directory_module, wasi_processor, wast_processor, with_test_module,
    Testsuite,
};

fn main() -> anyhow::Result<()> {
    // As rerun-if-changed doesn't support globs, we use another crate
    // to check changes in directories.
    build_deps::rerun_if_changed_paths("tests/wasi-wast/wasi/unstable/*")
        .expect("Can't get directory");
    build_deps::rerun_if_changed_paths("tests/wasi-wast/wasi/snapshot1/*")
        .expect("Can't get directory");
    build_deps::rerun_if_changed_paths("tests/wasi-wast/wasi/nightly-2022-10-18/*")
        .expect("Can't get directory");
    build_deps::rerun_if_changed_paths("tests/wast/spec/proposals/*").expect("Can't get directory");

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR").expect("The OUT_DIR environment variable must be set"),
    );

    // Spectests test generation
    {
        let mut spectests = Testsuite {
            buffer: String::new(),
            path: vec![],
        };

        with_test_module(&mut spectests, "spec", |spectests| {
            let _spec_tests = test_directory(spectests, "tests/wast/spec", wast_processor)?;
            test_directory_module(
                spectests,
                "tests/wast/spec/proposals/multi-value",
                wast_processor,
            )?;
            test_directory_module(spectests, "tests/wast/spec/proposals/simd", wast_processor)?;
            test_directory_module(
                spectests,
                "tests/wast/spec/proposals/exception-handling",
                wast_processor,
            )?;
            test_directory_module(
                spectests,
                "tests/wast/spec/proposals/threads",
                wast_processor,
            )?;
            // test_directory_module(spectests, "tests/wast/spec/proposals/bulk-memory-operations", wast_processor)?;
            Ok(())
        })?;
        with_test_module(&mut spectests, "wasmer", |spectests| {
            let _spec_tests = test_directory(spectests, "tests/wast/wasmer", wast_processor)?;
            Ok(())
        })?;

        let spectests_output = out_dir.join("generated_spectests.rs");
        fs::write(&spectests_output, spectests.buffer)?;

        // Write out our auto-generated tests and opportunistically format them with
        // `rustfmt` if it's installed.
        // Note: We need drop because we don't want to run `unwrap` or `expect` as
        // the command might fail, but we don't care about it's result.
        drop(Command::new("rustfmt").arg(&spectests_output).status());
    }

    // Wasitest test generation
    {
        let mut wasitests = Testsuite {
            buffer: String::new(),
            path: vec![],
        };

        with_test_module(&mut wasitests, "wasitests", |wasitests| {
            for wasi_version in &["unstable", "snapshot1", "nightly_2022_10_18"] {
                with_test_module(wasitests, wasi_version, |wasitests| {
                    for (wasi_filesystem_test_name, wasi_filesystem_kind) in &[
                        ("host_fs", "WasiFileSystemKind::Host"),
                        ("mem_fs", "WasiFileSystemKind::InMemory"),
                        ("tmp_fs", "WasiFileSystemKind::Tmp"),
                        ("passthru_fs", "WasiFileSystemKind::PassthruMemory"),
                        ("union_fs", "WasiFileSystemKind::UnionHostMemory"),
                        ("root_fs", "WasiFileSystemKind::RootFileSystemBuilder"),
                    ] {
                        with_test_module(wasitests, wasi_filesystem_test_name, |wasitests| {
                            test_directory(
                                wasitests,
                                format!("tests/wasi-wast/wasi/{wasi_version}"),
                                |out, path| wasi_processor(out, path, wasi_filesystem_kind),
                            )
                        })?;
                    }

                    Ok(())
                })?;
            }

            Ok(())
        })?;

        let wasitests_output = out_dir.join("generated_wasitests.rs");
        fs::write(&wasitests_output, wasitests.buffer)?;

        drop(Command::new("rustfmt").arg(&wasitests_output).status());
    }

    Ok(())
}
