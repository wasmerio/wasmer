//! Checks that wasmer's generated wasixv2 crates match the ABI in the `wasixv2-abi` submodule
//! (`lib/wasixv2/codegen`).
//!
//! Set `WASIXV2_CODEGEN_BLESS=1` to regenerate them.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Result;
use wasixv2_codegen::{
    Target, generate_abi, generate_exec, generate_syscalls, rust::source_from_header,
};

const SUBMODULE: &str = "lib/wasixv2/codegen";
const ABI_RS: &str = "lib/wasixv2/abi/src/generated.rs";
const SYSCALLS_RS: &str = "lib/wasixv2/syscalls/src/generated.rs";
const EXEC_RS: &str = "lib/wasixv2/exec/src/generated.rs";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn bless() -> bool {
    std::env::var_os("WASIXV2_CODEGEN_BLESS").is_some()
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A submodule left at another commit than the one wasmer pins makes every generated file look
/// stale, so this says what is actually wrong. Compares against the index, so a bump that is
/// staged with `git add` passes. Skipped outside a git checkout.
#[test]
fn abi_submodule_is_at_the_pinned_commit() {
    let root = repo_root();
    // `git ls-files -s` prints `160000 <commit> 0\t<path>` for a submodule.
    let Some(pinned) = git(&root, &["ls-files", "-s", "--", SUBMODULE]) else {
        return;
    };
    let pinned = pinned
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let Some(checked_out) = git(&root.join(SUBMODULE), &["rev-parse", "HEAD"]) else {
        return;
    };
    assert!(
        pinned == checked_out,
        "the wasixv2-abi submodule ({SUBMODULE}) is at {checked_out}, but wasmer pins {pinned}.\n\
         Run `git submodule update --init {SUBMODULE}`, or `git add {SUBMODULE}` if you are \
         bumping the ABI on purpose."
    );
}

/// Regenerates the checked-in `path` from the WIT source recorded in its header.
fn check_generated(
    path: &str,
    generate: fn(&Path, &str, Target) -> Result<String>,
    subcommand: &str,
) {
    let root = repo_root();
    let current = std::fs::read_to_string(root.join(path)).unwrap();
    let source = source_from_header(&current).expect("generated header names its source");
    let fresh = generate(&root.join(source), source, Target::WASM32).unwrap();
    if bless() {
        std::fs::write(root.join(path), &fresh).unwrap();
        return;
    }
    assert!(
        fresh == current,
        "{path} is stale; from the repo root run:\n  \
         WASIXV2_CODEGEN_BLESS=1 cargo test -p wasixv2-abi --test generated\n\
         or: cargo run -p wasixv2-codegen -- {subcommand} --wit {source} --out {path}\n\
         If abi_submodule_is_at_the_pinned_commit fails too, update the submodule instead."
    );
}

#[test]
fn abi_is_up_to_date() {
    check_generated(ABI_RS, generate_abi, "abi");
}

#[test]
fn syscalls_is_up_to_date() {
    check_generated(SYSCALLS_RS, generate_syscalls, "syscalls");
}

#[test]
fn exec_is_up_to_date() {
    check_generated(EXEC_RS, generate_exec, "exec");
}
