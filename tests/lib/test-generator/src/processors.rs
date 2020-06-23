//! Here we define the processors usable for each test genrator
use crate::{extract_name, Test, Testsuite};
use std::path::PathBuf;

/// Given a Testsuite and a path, process the path in case is a wast
/// file.
pub fn wast_processor(out: &mut Testsuite, p: PathBuf) -> Option<Test> {
    let ext = p.extension()?;
    // Only look at wast files.
    if ext != "wast" {
        return None;
    }

    // Ignore files starting with `.`, which could be editor temporary files
    if p.file_stem()?.to_str()?.starts_with('.') {
        return None;
    }

    let testname = extract_name(&p);
    let compiler = out.path.get(0).unwrap();

    // The implementation of `run_wast` lives in /tests/spectest.rs
    let body = format!("crate::run_wast(r#\"{}\"#, \"{}\")", p.display(), compiler);

    Some(Test {
        name: testname,
        body,
    })
}

/// Given a Testsuite and a path, process the path in case is a Emscripten
/// wasm file.
pub fn emscripten_processor(out: &mut Testsuite, p: PathBuf) -> Option<Test> {
    let ext = p.extension()?;
    // Only look at wast files.
    if ext != "wasm" {
        return None;
    }

    let outfile = {
        let mut out_ext = p.clone();
        out_ext.set_extension("out");
        if out_ext.exists() {
            out_ext
        } else {
            return None;
        }
    };

    let testname = extract_name(&p);
    let compiler = out.path.get(0).unwrap();

    // The implementation of `run_emscripten` lives in /tests/emtest.rs
    let body = format!(
        "crate::emscripten::run_emscripten(r#\"{}\"#, r#\"{}\"#, \"{}\")",
        p.display(),
        outfile.display(),
        compiler
    );

    Some(Test {
        name: testname,
        body,
    })
}

/// Given a Testsuite and a path, process the path in case is a WASI
/// wasm file.
pub fn wasi_processor(out: &mut Testsuite, p: PathBuf) -> Option<Test> {
    let ext = p.extension()?;
    // Only look at wast files.
    if ext != "wast" {
        return None;
    }

    let wasm_dir = {
        let mut inner = p.clone();
        inner.pop();
        inner
    };
    let testname = extract_name(&p);
    let compiler = out.path.get(0).unwrap();

    // The implementation of `run_wasi` lives in /tests/wasitest.rs
    let body = format!(
        "crate::run_wasi(r#\"{}\"#, \"{}\", \"{}\")",
        p.display(),
        wasm_dir.display(),
        compiler
    );

    Some(Test {
        name: testname,
        body,
    })
}
