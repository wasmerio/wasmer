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
    let mut capi_compiler_features = compilers
        .replace(" ", ",")
        .split(",")
        .filter(|i| *i != "llvm")
        .collect::<Vec<_>>()
        .join(",");
    if !capi_compiler_features.is_empty() {
        capi_compiler_features.push(',');
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
        "lib/c-api/Cargo.toml",
        "--no-default-features",
        "--features",
        "wat,compiler,wasi,middlewares,webc_runner",
        "--features",
        &format!("{capi_compiler_features}wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
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
