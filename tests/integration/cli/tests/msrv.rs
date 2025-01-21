//! Integration tests which makes sure various parts of the project (CI,
//! Dockerfiles, etc.) are all using the Rust version specified in `/Cargo.toml`.

use std::path::Path;

use once_cell::sync::Lazy;
use regex::Regex;

static MSRV: Lazy<String> = Lazy::new(|| {
    let cargo_toml = project_root().join("Cargo.toml");
    let contents = std::fs::read_to_string(cargo_toml).unwrap();
    let rust_version_line = contents
        .lines()
        .find(|line| line.contains("rust-version") && line.contains('"'))
        .unwrap();
    let pieces: Vec<_> = rust_version_line.split('"').collect();
    let [_, msrv, _] = pieces.as_slice() else {
        panic!();
    };

    msrv.to_string()
});

#[test]
fn docker_file_is_up_to_date() {
    let pattern = Regex::new(r"1\.\d\d").unwrap();
    let dockerfile = project_root()
        .join(".github")
        .join("cross-linux-riscv64")
        .join("Dockerfile");

    let contents = std::fs::read_to_string(&dockerfile).unwrap();
    let expected = pattern.replace_all(&contents, MSRV.as_str());

    ensure_file_contents(dockerfile, expected);
}

#[test]
fn rust_toolchain_file_is_up_to_date() {
    let pattern = Regex::new(r"1\.\d\d").unwrap();
    let rust_toolchain = project_root().join("rust-toolchain");

    let contents = std::fs::read_to_string(&rust_toolchain).unwrap();
    let expected = pattern.replace_all(&contents, MSRV.as_str());

    ensure_file_contents(rust_toolchain, expected);
}

#[test]
fn ci_files_are_up_to_date() {
    let pattern = Regex::new(r#"MSRV:\s*"\d+\.\d+""#).unwrap();
    let replacement = format!("MSRV: \"{}\"", MSRV.as_str());
    let workflows = project_root().join(".github").join("workflows");

    for result in workflows.read_dir().unwrap() {
        let path = result.unwrap().path();

        let contents = std::fs::read_to_string(&path).unwrap();
        let expected = pattern.replace_all(&contents, &replacement);

        ensure_file_contents(path, expected);
    }
}

/// Get the root directory for this repository.
fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
}

/// Check that a particular file has the desired contents.
///
/// If the file is missing or outdated, this function will update the file and
/// trigger a panic to fail any test this is called from.
fn ensure_file_contents(path: impl AsRef<Path>, contents: impl AsRef<str>) {
    let path = path.as_ref();
    let contents = contents.as_ref();

    if let Ok(old_contents) = std::fs::read_to_string(path) {
        if contents == old_contents {
            // File is already up to date
            return;
        }
    }

    let display_path = path.strip_prefix(project_root()).unwrap_or(path);

    eprintln!("{} was not up-to-date, updating...", display_path.display());

    if std::env::var("CI").is_ok() {
        eprintln!("Note: run `cargo test` locally and commit the updated files");
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, contents).unwrap();
    panic!(
        "\"{}\" was not up to date and has been updated. Please commit the changes and re-run the tests.",
        path.strip_prefix(project_root()).unwrap_or(path).display()
    );
}
