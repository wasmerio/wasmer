//! Build library to generate a program which runs all the testsuites.
//!
//! By generating a separate `#[test]` test for each file, we allow cargo test
//! to automatically run the files in parallel.
//!
//! > This program is inspired/forked from:
//! > https://github.com/bytecodealliance/wasmtime/blob/master/build.rs
mod processors;

pub use crate::processors::{wasi_processor, wast_processor};
use anyhow::Context;
use std::fmt::Write;
use std::path::{Path, PathBuf};

pub struct Testsuite {
    pub buffer: String,
    pub path: Vec<String>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Test {
    pub name: String,
    pub body: String,
}

pub fn test_directory_module(
    out: &mut Testsuite,
    path: impl AsRef<Path>,
    processor: impl Fn(&mut Testsuite, PathBuf) -> Option<Test>,
) -> anyhow::Result<usize> {
    let path = path.as_ref();
    let testsuite = &extract_name(path);
    with_test_module(out, testsuite, |out| test_directory(out, path, processor))
}

fn write_test(out: &mut Testsuite, testname: &str, body: &str) -> anyhow::Result<()> {
    writeln!(
        out.buffer,
        "#[compiler_test({})]",
        out.path[..out.path.len() - 1].join("::")
    )?;
    writeln!(
        out.buffer,
        "fn r#{}(config: crate::Config) -> anyhow::Result<()> {{",
        &testname
    )?;
    writeln!(out.buffer, "{body}")?;
    writeln!(out.buffer, "}}")?;
    writeln!(out.buffer)?;
    Ok(())
}

pub fn test_directory(
    out: &mut Testsuite,
    path: impl AsRef<Path>,
    processor: impl Fn(&mut Testsuite, PathBuf) -> Option<Test>,
) -> anyhow::Result<usize> {
    let path = path.as_ref();
    let mut dir_entries: Vec<_> = path
        .read_dir()
        .context(format!("failed to read {path:?}"))?
        .map(|r| r.expect("reading testsuite directory entry"))
        .filter_map(|dir_entry| processor(out, dir_entry.path()))
        .collect();

    dir_entries.sort();

    for Test {
        name: testname,
        body,
    } in dir_entries.iter()
    {
        out.path.push(testname.to_string());
        write_test(out, testname, body).unwrap();
        out.path.pop().unwrap();
    }

    Ok(dir_entries.len())
}

/// Extract a valid Rust identifier from the stem of a path.
pub fn extract_name(path: impl AsRef<Path>) -> String {
    path.as_ref()
        .file_stem()
        .expect("filename should have a stem")
        .to_str()
        .expect("filename should be representable as a string")
        .replace(['-', '/'], "_")
}

pub fn with_test_module<T>(
    out: &mut Testsuite,
    testsuite: &str,
    f: impl FnOnce(&mut Testsuite) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    out.path.push(testsuite.to_string());
    out.buffer.push_str("mod ");
    out.buffer.push_str(testsuite);
    out.buffer.push_str(" {\n");

    let result = f(out)?;

    out.buffer.push_str("}\n");
    out.path.pop().unwrap();
    Ok(result)
}
