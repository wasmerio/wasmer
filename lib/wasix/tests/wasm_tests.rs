use anyhow::Result;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::bail;
use libtest_mimic::Trial;
use walkdir::WalkDir;

struct Filter {
    filter: Option<String>,
    exact: bool,
}

impl Filter {
    fn new(args: &libtest_mimic::Arguments) -> Self {
        Self {
            filter: args.filter.clone(),
            exact: args.exact,
        }
    }

    /// TODO: write comment
    fn excludes(&self, prefix: &str) -> bool {
        let Some(filter) = self.filter.as_ref() else {
            return false;
        };

        if self.exact {
            !filter.starts_with(prefix) && !prefix.starts_with(filter)
        } else {
            false
        }
    }
}

fn should_emit_colour() -> bool {
    std::io::stdout().is_terminal()
        || std::env::var("CARGO_TERM_COLOR").as_deref() == Ok("always")
        || std::env::var("NEXTEST").is_ok()
}

fn main() -> Result<std::process::ExitCode> {
    let mut args = libtest_mimic::Arguments::from_args();
    if should_emit_colour() {
        args.color = Some(libtest_mimic::ColorSetting::Always);
    }
    let filter = Filter::new(&args);
    let mut tests = Vec::new();
    collect_tests(&mut tests, &filter)?;
    Ok(libtest_mimic::run(&args, tests).exit_code())
}

fn identify_primary_source(test_src_dir: &Path) -> Result<PathBuf> {
    const FILES: &[&str] = &["main.c", "main.cpp", "build.sh"];

    for file in FILES {
        let path = test_src_dir.join(file);
        if path.exists() {
            return Ok(path);
        }
    }

    bail!(
        "{} must contain {}",
        test_src_dir.display(),
        FILES.join(",")
    );
}

fn collect_tests(tests: &mut Vec<Trial>, filter: &Filter) -> Result<()> {
    let tests_dir = PathBuf::from_str(env!("CARGO_MANIFEST_DIR"))?.join("tests/wasm_tests/");

    for entry in WalkDir::new(&tests_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.path() != tests_dir)
        // Skip temporary helper directories (like 'a', 'b', etc.).
        .filter(|e| {
            e.path()
                .file_name()
                .expect("file name must be valid")
                .to_str()
                .expect("valid path")
                .len()
                > 1
        })
        .filter(|e| e.file_type().is_dir())
        // TODO
        .filter(|e| {
            std::fs::read_dir(e.path())
                .expect("valid directory entry")
                .filter_map(Result::ok)
                .any(|entry| entry.file_type().expect("cannot read file type").is_file())
        })
    {
        let test_name = entry.path().strip_prefix(&tests_dir)?.display().to_string();

        tests.push(libtest_mimic::Trial::ignorable_test(test_name, move || {
            let primary_source = identify_primary_source(entry.path())?;

            Ok(libtest_mimic::Completion::Completed)
        }));
    }

    Ok(())
}
