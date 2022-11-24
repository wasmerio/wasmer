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

    if let Ok(target) = std::env::var("CARGO_TARGET") {
        cmd.args(&["--target", &target]);
    }

    println!("running {cmd:?}");

    let cmd = cmd.output().unwrap();

    if !cmd.status.success() {
        println!("test capi paniced with status {}", cmd.status);
        println!("stdout: {}", String::from_utf8_lossy(&cmd.stdout));
        println!("stderr: {}", String::from_utf8_lossy(&cmd.stdout));
        panic!("test capi failed");
    }

    println!("{cmd:?} succeeded, compilers = {compilers}");
}

fn main() {
    /*

    test-capi: build-capi package-capi $(foreach compiler_engine,$(capi_compilers_engines),test-capi-crate-$(compiler_engine) test-capi-integration-$(compiler_engine))

    test-capi-crate-%:
        WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-crate-//) $(CARGO_BINARY) test $(CARGO_TARGET) --manifest-path lib/c-api/Cargo.toml --release \
            --no-default-features --features wat,compiler,wasi,middlewares,webc_runner $(capi_compiler_features) -- --nocapture

    test-capi-integration-%:
        # note: you need to do make build-capi and make package-capi first!
        # Test the Wasmer C API tests for C
        cd lib/c-api/tests; WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-integration-//) WASMER_DIR=`pwd`/../../../package make test
        # Test the Wasmer C API examples
        cd lib/c-api/examples; WASMER_CAPI_CONFIG=$(shell echo $@ | sed -e s/test-capi-integration-//) WASMER_DIR=`pwd`/../../../package make run

    */
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());
    let mut compiler_features = compilers
        .replace(' ', ",")
        .split(',')
        .collect::<Vec<_>>()
        .join(",");

    if !compiler_features.is_empty() {
        compiler_features.push(',');
    };
    let mut capi_compiler_features = compilers
        .replace(' ', ",")
        .split(',')
        .filter(|i| *i != "llvm")
        .collect::<Vec<_>>()
        .join(",");
    if !capi_compiler_features.is_empty() {
        capi_compiler_features.push(',');
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

    let exclude_tests = exclude_tests.join("--exclude ");

    start_test(&[
        "test",
        "--release",
        "--tests",
        "--features",
        "cranelift",
        "--features",
        &compiler_features,
    ]);

    start_test(&[
        "test",
        "--all",
        "--release",
        &exclude_tests,
        "--features",
        "cranelift",
    ]);

    start_test(&[
        "test",
        &compiler_features,
        "--features",
        "wasi,cranelift",
        "--examples",
    ]);

    start_test(&[
        "test",
        &compiler_features,
        "--features",
        "wasi,cranelift",
        "--examples",
        "--release", // <-
    ]);

    start_test(&["test", "--doc", "--all", "--features", "cranelift,std"]);
}
