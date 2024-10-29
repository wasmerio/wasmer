//! Here we define the processors usable for each test genrator
use crate::{extract_name, Test, Testsuite};
use std::path::PathBuf;

/// Given a Testsuite and a path, process the path in case is a wast
/// file.
pub fn wast_processor(_out: &mut Testsuite, p: PathBuf) -> Option<Test> {
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

    // The implementation of `run_wast` lives in /tests/spectest.rs
    let body = format!("crate::run_wast(config, r#\"{}\"#)", p.display());

    Some(Test {
        name: testname,
        body,
    })
}

/// Given a Testsuite and a path, process the path in case is a WASI
/// wasm file.
pub fn wasi_processor(
    _out: &mut Testsuite,
    p: PathBuf,
    wasi_filesystem_kind: &str,
) -> Option<Test> {
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

    let body = format!(
        "crate::run_wasi(config, r#\"{}\"#, \"{}\", crate::{})",
        p.display(),
        wasm_dir.display(),
        wasi_filesystem_kind,
    );

    Some(Test {
        name: testname,
        body,
    })
}
