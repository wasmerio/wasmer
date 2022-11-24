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

fn start_test(args: &[&str]) {
    let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());

    let mut cmd = Command::new(&cargo);
    cmd.current_dir(project_root());
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    cmd.args(args);

    println!("running {cmd:?}");

    let cmd = cmd.output().unwrap();

    if !cmd.status.success() {
        println!("test wasmer paniced with status {}", cmd.status);
        println!("stdout: {}", String::from_utf8_lossy(&cmd.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&cmd.stdout));
        panic!("test wasmer failed");
    }

    println!("{cmd:?} succeeded, compilers = {compilers}");
}

fn main() {
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());
    let stage = env::var("STAGE").unwrap_or_else(|_| "all".to_string());

    let mut compiler_features = compilers
        .replace(' ', ",")
        .split(',')
        .collect::<Vec<_>>()
        .join(",");

    if !compiler_features.is_empty() {
        compiler_features.push(',');
    };

    let mut exclude_tests = vec![
        "wasmer-c-api",
        "wasmer-cli",
        "wasmer-compiler-cli",
        "wasmer-wasi-experimental-io-devices",
        "wasmer-integration-tests-ios",
    ];

    if !compiler_features.contains("llvm") {
        exclude_tests.push("wasmer-compiler-llvm");
    }

    let exclude_tests = exclude_tests.join(" --exclude ");

    // cargo test  --release --tests --features cranelift,singlepass,
    // wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load
    if stage == "1" || stage == "all" {
        start_test(&[
            "test",
            "--release",
            "--tests",
            "--features",
            &format!("{compiler_features},wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
        ]);
    }

    if stage == "2" || stage == "all" {
        // cargo test  --all --release --exclude wasmer-c-api --exclude wasmer-cli --exclude wasmer-compiler-cli --exclude wasmer-wasi-experimental-io-devices --exclude wasmer-integration-tests-cli
        // --exclude wasmer-integration-tests-ios --exclude wasmer-compiler-llvm
        let f = format!("{compiler_features},wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load");
        let mut args = vec!["test", "--all", "--features", &f, "--release"];
        for i in exclude_tests.split_whitespace() {
            args.push(i);
        }
        start_test(&args);
    }

    if stage == "3" || stage == "all" {
        // cargo test  --manifest-path lib/compiler-cranelift/Cargo.toml --release --no-default-features --features=std
        // cargo test  --manifest-path lib/compiler-singlepass/Cargo.toml --release --no-default-features --features=std
        for compiler in compilers.split(',') {
            start_test(&[
                "test",
                "--manifest-path",
                &format!("lib/compiler-{compiler}/Cargo.toml"),
                "--release",
                "--no-default-features",
                "--features=std",
            ]);
        }
    }

    // cargo test  --manifest-path lib/cli/Cargo.toml --features cranelift,singlepass,wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load --release
    if stage == "4" || stage == "all" {
        start_test(&[
            "test",
            "--manifest-path",
            "lib/cli/Cargo.toml",
            "--features",
            &format!("{compiler_features},wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
            "--features",
            "wasi",
            "--examples",
        ]);
    }

    if stage == "5" || stage == "all" {
        // cargo test  --features cranelift,singlepass,wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load --features wasi --examples
        start_test(&[
            "test",
            "--features",
            &format!("{compiler_features},wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
            "--features",
            "wasi",
            "--examples",
        ]);
    }

    if stage == "6" || stage == "all" {
        // cargo test  --release --features cranelift,singlepass,wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load --features wasi --examples
        start_test(&[
            "test",
            &format!("{compiler_features},wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
            "--features",
            "wasi",
            "--examples",
            "--release", // <-
        ]);
    }

    if stage == "7" || stage == "all" {
        start_test(&[
            "test", 
            "--doc", 
            "--all", 
            "--features", 
            &format!("{compiler_features},wasmer-artifact-create,static-artifact-create,wasmer-artifact-load,static-artifact-load"),
        ]);
    }
}
