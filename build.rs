//! A kind of meta-build.rs that can be configured to do different things.
//!
//! Please try to keep this file as clean as possible.

use generate_emscripten_tests;
use generate_wasi_tests;
use std::env;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use test_generator::{
    build_ignores_from_textfile, extract_name, test_directory, test_directory_module,
    with_test_module, Test, Testsuite,
};

/// Given a Testsuite and a path, process the path in case is a wast
/// file.
fn wast_processor(out: &mut Testsuite, p: PathBuf) -> Option<Test> {
    let ext = p.extension()?;
    // Only look at wast files.
    if ext != "wast" {
        return None;
    }

    // Ignore files starting with `.`, which could be editor temporary files
    if p.file_stem()?.to_str()?.starts_with(".") {
        return None;
    }

    let testname = extract_name(&p);
    let body = format!(
        "crate::run_wast(r#\"{}\"#, \"{}\")",
        p.display(),
        out.path.get(0).unwrap()
    );

    Some(Test {
        name: testname.to_string(),
        body: body.to_string(),
    })
}

fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=test/ignores.txt");

    generate_wasi_tests::build();
    generate_emscripten_tests::build();

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR").expect("The OUT_DIR environment variable must be set"),
    );
    let ignores = build_ignores_from_textfile("tests/ignores.txt".into())?;
    let mut out = Testsuite {
        buffer: String::new(),
        path: vec![],
        ignores: ignores,
    };

    for compiler in &["singlepass", "cranelift", "llvm"] {
        writeln!(out.buffer, "#[cfg(feature=\"backend-{}\")]", compiler);
        writeln!(out.buffer, "#[cfg(test)]")?;
        writeln!(out.buffer, "#[allow(non_snake_case)]")?;
        with_test_module(&mut out, compiler, |mut out| {
            with_test_module(&mut out, "spec", |out| {
                let spec_tests = test_directory(out, "tests/spectests", wast_processor)?;
                // Skip running spec_testsuite tests if the submodule isn't checked
                // out.
                // if spec_tests > 0 {
                //     test_directory_module(
                //         out,
                //         "tests/spec_testsuite/proposals/simd",
                //         wast_processor,
                //     )?;
                //     test_directory_module(
                //         out,
                //         "tests/spec_testsuite/proposals/multi-value",
                //         wast_processor,
                //     )?;
                //     test_directory_module(
                //         out,
                //         "tests/spec_testsuite/proposals/reference-types",
                //         wast_processor,
                //     )?;
                //     test_directory_module(
                //         out,
                //         "tests/spec_testsuite/proposals/bulk-memory-operations",
                //         wast_processor,
                //     )?;
                // } else {
                //     println!(
                //         "cargo:warning=The spec testsuite is disabled. To enable, run `git submodule \
                //     update --remote`."
                //     );
                // }
                Ok(())
            })?;
            Ok(())
        })?;
    }

    // println!("{}", out.buffer);
    // std::process::exit(1);
    // Write out our auto-generated tests and opportunistically format them with
    // `rustfmt` if it's installed.
    let output = out_dir.join("generated_tests.rs");
    fs::write(&output, out.buffer)?;
    drop(Command::new("rustfmt").arg(&output).status());
    Ok(())
}
