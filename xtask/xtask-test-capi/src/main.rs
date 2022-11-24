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


fn start_test(args: &[&str], env_vars: &[(&str, String)]) {
    let args = args.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());

    let mut cmd = Command::new(&cargo);
    cmd.current_dir(project_root());
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    cmd.args(args);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }

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
    std::fs::create_dir_all(project_root().join("package")).expect("could not create package dir");

    wasmer_registry::try_unpack_targz(
        project_root().join("build-capi.tar.gz"),
        project_root().join("package"),
        false,
    )
    .expect("could not unpack build-capi.tar.gz, run cargo build-capi first!");

    let compilers = env::var("COMPILERS").unwrap_or_else(|_| "cranelift".to_string());
    let capi_compiler_features = compilers
        .replace(' ', ",")
        .split(',')
        .filter(|i| *i != "llvm")
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    // we only have one engine
    let capi_compiler_engines = capi_compiler_features
        .iter()
        .map(|f| format!("{f}-universal"))
        .collect::<Vec<_>>();

    for engine in capi_compiler_engines {
        let envs = &[
            (
                "WASMER_DIR",
                format!("{}", project_root().join("package").display()),
            ),
            ("ROOT_DIR", format!("{}", project_root().display())),
            ("WASMER_CAPI_CONFIG", engine.to_string()),
        ];

        // run capi test
        start_test(
            &[
                "test",
                "--manifest-path",
                &format!("{}/lib/c-api/Cargo.toml", project_root().display()),
                "--release",
                "--no-default-features",
                "--features",
                "wat,compiler,wasi,middlewares,webc_runner",
                "--features",
                &capi_compiler_features.join(","),
                "--",
                "--nocapture"
            ],
            envs,
        );

        // run integration tests

        // cargo test --manifest-path="lib/c-api/tests/wasmer-c-api-test-runner/Cargo.toml"
        start_test(
            &[
                "test",
                "--manifest-path",
                &format!(
                    "{}/lib/c-api/tests/wasmer-c-api-test-runner/Cargo.toml",
                    project_root().display()
                ),
                "--",
                "--nocapture"
            ],
            envs,
        );

        // cargo test --manifest-path="lib/c-api/examples/wasmer-capi-examples-runner/Cargo.toml"
        start_test(
            &[
                "test",
                "--manifest-path",
                &format!(
                    "{}/lib/c-api/examples/wasmer-capi-examples-runner/Cargo.toml",
                    project_root().display()
                ),
                "--",
                "--nocapture"
            ],
            envs,
        );
    }
}
