//! Build library to generate a program which runs all the testsuites.
//!
//! By generating a separate `#[test]` test for each file, we allow cargo test
//! to automatically run the files in parallel.
//!
//! > This program is inspired/forked from:
//! > https://github.com/bytecodealliance/wasmtime/blob/master/build.rs
mod processors;

pub use crate::processors::{emscripten_processor, wasi_processor, wast_processor};
use anyhow::Context;
use std::collections::HashSet;
use std::fmt::Write;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use target_lexicon::Triple;

pub type Ignores = HashSet<String>;
pub struct Testsuite {
    pub buffer: String,
    pub path: Vec<String>,
    pub ignores: Ignores,
}

impl Testsuite {
    fn ignore_current(&self) -> bool {
        let full = self.path.join("::");
        if self.ignores.contains(&full) {
            return true;
        }
        self.ignores.iter().any(|ignore| full.contains(ignore))
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Test {
    pub name: String,
    pub body: String,
}

pub type ProcessorType = fn(&mut Testsuite, PathBuf) -> Option<Test>;

/// Generates an Ignores struct from a text file
pub fn build_ignores_from_textfile(path: PathBuf) -> anyhow::Result<Ignores> {
    let mut ignores = HashSet::new();
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let host = Triple::host().to_string();
    for line in reader.lines() {
        let line = line.unwrap();
        // If the line has a `#` we discard all the content that comes after
        let line = if line.contains("#") {
            let l: Vec<&str> = line.split('#').collect();
            l.get(0).unwrap().to_string()
        } else {
            line
        };

        let line = line.trim().to_string();

        // If the lines contains ` on ` it means the test should be ignored
        // on that platform
        let (line, target) = if line.contains(" on ") {
            let l: Vec<&str> = line.split(" on ").collect();
            (
                l.get(0).unwrap().to_string(),
                Some(l.get(1).unwrap().to_string()),
            )
        } else {
            (line, None)
        };
        if line.len() == 0 {
            continue;
        }

        match target {
            Some(t) => {
                // We skip the ignore if doesn't apply to the current
                // host target
                if !host.contains(&t) {
                    continue;
                }
            }
            None => {}
        }
        ignores.insert(line);
    }
    Ok(ignores)
}

pub fn test_directory_module(
    out: &mut Testsuite,
    path: impl AsRef<Path>,
    processor: ProcessorType,
) -> anyhow::Result<usize> {
    let path = path.as_ref();
    let testsuite = &extract_name(path);
    with_test_module(out, testsuite, |out| test_directory(out, path, processor))
}

fn write_test(out: &mut Testsuite, testname: &str, body: &str) -> anyhow::Result<()> {
    writeln!(out.buffer, "#[test]")?;
    if out.ignore_current() {
        writeln!(out.buffer, "#[ignore]")?;
    }
    writeln!(out.buffer, "fn r#{}() -> anyhow::Result<()> {{", &testname)?;
    writeln!(out.buffer, "{}", body)?;
    writeln!(out.buffer, "}}")?;
    writeln!(out.buffer)?;
    Ok(())
}

pub fn test_directory(
    out: &mut Testsuite,
    path: impl AsRef<Path>,
    processor: ProcessorType,
) -> anyhow::Result<usize> {
    let path = path.as_ref();
    let mut dir_entries: Vec<_> = path
        .read_dir()
        .context(format!("failed to read {:?}", path))?
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
        write_test(out, &testname, &body).unwrap();
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
        .replace("-", "_")
        .replace("/", "_")
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

pub fn with_backends(
    mut out: &mut Testsuite,
    backends: &[&str],
    f: impl Fn(&mut Testsuite) -> anyhow::Result<()> + Copy,
) -> anyhow::Result<()> {
    for compiler in backends.iter() {
        writeln!(out.buffer, "#[cfg(feature=\"compiler-{}\")]", compiler)?;
        writeln!(out.buffer, "#[cfg(test)]")?;
        writeln!(out.buffer, "#[allow(non_snake_case)]")?;
        with_test_module(&mut out, &compiler, f)?;
    }
    Ok(())
}
