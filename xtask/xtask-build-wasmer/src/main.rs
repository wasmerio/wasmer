use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn project_root() -> PathBuf {
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn main() {
    let compilers = env::var("COMPILERS").unwrap_or("cranelift".to_string());
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut compiler_features = compilers
        .replace(" ", ",")
        .split(",")
        .collect::<Vec<_>>()
        .join(",");
    if !compiler_features.is_empty() {
        compiler_features.push(',');
    };
    let mut cmd = Command::new(cargo);
    cmd.current_dir(project_root());
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    cmd.args(&[
        "build", 
        "--release",
        "--manifest-path",
        "lib/cli/Cargo.toml",
        "--features",
        "webc_runner",
        "--features",
        &format!("{compiler_features}wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
    ]);

    println!("running {cmd:?}");

    let cmd = cmd.output().unwrap();

    if !cmd.status.success() {
        println!("build capi paniced with status {}", cmd.status);
        println!("stdout: {}", String::from_utf8_lossy(&cmd.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&cmd.stdout));
        panic!("build capi failed");
    }

    println!("build capi succeeded, compilers = {compilers}");
}
