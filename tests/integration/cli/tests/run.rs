//! Basic tests for the `run` subcommand

use assert_cmd::Command;
use predicates::str::contains;
use std::path::{Path, PathBuf};
use wasmer_integration_tests_cli::{get_wasmer_path, ASSET_PATH, C_ASSET_PATH};

fn wasi_test_python_path() -> PathBuf {
    Path::new(C_ASSET_PATH).join("python-0.1.0.wasmer")
}

fn wasi_test_wasm_path() -> PathBuf {
    Path::new(C_ASSET_PATH).join("qjs.wasm")
}

fn test_no_imports_wat_path() -> PathBuf {
    Path::new(ASSET_PATH).join("fib.wat")
}

fn test_no_start_wat_path() -> PathBuf {
    Path::new(ASSET_PATH).join("no_start.wat")
}

/// Ignored on Windows because running vendored packages does not work
/// since Windows does not allow `::` characters in filenames (every other OS does)
///
/// The syntax for vendored package atoms has to be reworked for this to be fixed, see
/// https://github.com/wasmerio/wasmer/issues/3535
// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
#[ignore]
#[test]
fn test_run_customlambda() {
    let assert = Command::new(get_wasmer_path())
        .arg("config")
        .arg("--bindir")
        .assert()
        .success();
    let bindir = std::str::from_utf8(&assert.get_output().stdout)
        .expect("wasmer config --bindir stdout failed");

    // /Users/fs/.wasmer/bin
    let checkouts_path = Path::new(bindir.trim())
        .parent()
        .expect("--bindir: no parent")
        .join("checkouts");
    println!("checkouts path: {}", checkouts_path.display());
    let _ = std::fs::remove_dir_all(&checkouts_path);

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wapm.io/ciuser/customlambda")
        // TODO: this argument should not be necessary later
        // see https://github.com/wasmerio/wasmer/issues/3514
        .arg("customlambda.py")
        .arg("55")
        .assert()
        .success();
    assert.stdout("139583862445\n");

    // Run again to verify the caching
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wapm.io/ciuser/customlambda")
        // TODO: this argument should not be necessary later
        // see https://github.com/wasmerio/wasmer/issues/3514
        .arg("customlambda.py")
        .arg("55")
        .assert()
        .success();
    assert.stdout("139583862445\n");
}

#[allow(dead_code)]
fn assert_tarball_is_present_local(target: &str) -> Result<PathBuf, anyhow::Error> {
    let wasmer_dir = std::env::var("WASMER_DIR").expect("no WASMER_DIR set");
    let directory = match target {
        "aarch64-darwin" => "wasmer-darwin-arm64.tar.gz",
        "x86_64-darwin" => "wasmer-darwin-amd64.tar.gz",
        "x86_64-linux-gnu" => "wasmer-linux-amd64.tar.gz",
        "aarch64-linux-gnu" => "wasmer-linux-aarch64.tar.gz",
        "x86_64-windows-gnu" => "wasmer-windows-gnu64.tar.gz",
        _ => return Err(anyhow::anyhow!("unknown target {target}")),
    };
    let libwasmer_cache_path = Path::new(&wasmer_dir).join("cache").join(directory);
    if !libwasmer_cache_path.exists() {
        return Err(anyhow::anyhow!(
            "targz {} does not exist",
            libwasmer_cache_path.display()
        ));
    }
    println!("using targz {}", libwasmer_cache_path.display());
    Ok(libwasmer_cache_path)
}

// FIXME: Fix and re-enable this test
// See https://github.com/wasmerio/wasmer/issues/3615
// #[test]
#[allow(dead_code)]
fn test_cross_compile_python_windows() {
    let temp_dir = tempfile::TempDir::new().unwrap();

    #[cfg(not(windows))]
    let targets = &[
        "aarch64-darwin",
        "x86_64-darwin",
        "x86_64-linux-gnu",
        "aarch64-linux-gnu",
        "x86_64-windows-gnu",
    ];

    #[cfg(windows)]
    let targets = &[
        "aarch64-darwin",
        "x86_64-darwin",
        "x86_64-linux-gnu",
        "aarch64-linux-gnu",
    ];

    // MUSL has no support for LLVM in C-API
    #[cfg(target_env = "musl")]
    let compilers = &["cranelift", "singlepass"];
    #[cfg(not(target_env = "musl"))]
    let compilers = &["cranelift", "singlepass", "llvm"];

    // llvm-objdump  --disassemble-all --demangle ./objects/wasmer_vm-50cb118b098c15db.wasmer_vm.60425a0a-cgu.12.rcgu.o
    // llvm-objdump --macho --exports-trie ~/.wasmer/cache/wasmer-darwin-arm64/lib/libwasmer.dylib
    let excluded_combinations = &[
        ("aarch64-darwin", "llvm"), // LLVM: aarch64 not supported relocation Arm64MovwG0 not supported
        ("aarch64-linux-gnu", "llvm"), // LLVM: aarch64 not supported relocation Arm64MovwG0 not supported
        // https://github.com/ziglang/zig/issues/13729
        ("x86_64-darwin", "llvm"), // undefined reference to symbol 'wasmer_vm_raise_trap' kind Unknown
        ("x86_64-windows-gnu", "llvm"), // unimplemented symbol `wasmer_vm_raise_trap` kind Unknown
    ];

    for t in targets {
        for c in compilers {
            if excluded_combinations.contains(&(t, c)) {
                continue;
            }
            println!("{t} target {c}");
            let python_wasmer_path = temp_dir.path().join(format!("{t}-python"));

            let tarball = match std::env::var("GITHUB_TOKEN") {
                Ok(_) => Some(assert_tarball_is_present_local(t).unwrap()),
                Err(_) => None,
            };
            let mut cmd = Command::new(get_wasmer_path());

            cmd.arg("create-exe");
            cmd.arg(wasi_test_python_path());
            cmd.arg("--target");
            cmd.arg(t);
            cmd.arg("-o");
            cmd.arg(python_wasmer_path.clone());
            cmd.arg(format!("--{c}"));
            if std::env::var("GITHUB_TOKEN").is_ok() {
                cmd.arg("--debug-dir");
                cmd.arg(format!("{t}-{c}"));
            }

            if t.contains("x86_64") && *c == "singlepass" {
                cmd.arg("-m");
                cmd.arg("avx");
            }

            if let Some(t) = tarball {
                cmd.arg("--tarball");
                cmd.arg(t);
            }

            let assert = cmd.assert().success();

            if !python_wasmer_path.exists() {
                let p = std::fs::read_dir(temp_dir.path())
                    .unwrap()
                    .filter_map(|e| Some(e.ok()?.path()))
                    .collect::<Vec<_>>();
                let output = assert.get_output();
                panic!("target {t} was not compiled correctly tempdir: {p:#?}, {output:?}",);
            }
        }
    }
}

#[test]
fn run_whoami_works() {
    // running test locally: should always pass since
    // developers don't have access to WAPM_DEV_TOKEN
    if std::env::var("GITHUB_TOKEN").is_err() {
        return;
    }

    let ciuser_token = std::env::var("WAPM_DEV_TOKEN").expect("no CIUSER / WAPM_DEV_TOKEN token");
    // Special case: GitHub secrets aren't visible to outside collaborators
    if ciuser_token.is_empty() {
        return;
    }

    Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.dev")
        .arg(ciuser_token)
        .assert()
        .success();

    let assert = Command::new(get_wasmer_path())
        .arg("whoami")
        .arg("--registry")
        .arg("wapm.dev")
        .assert()
        .success();

    assert
        .stdout("logged into registry \"https://registry.wapm.dev/graphql\" as user \"ciuser\"\n");
}

#[test]
fn run_wasi_works() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(wasi_test_wasm_path())
        .arg("--")
        .arg("-e")
        .arg("print(3 * (4 + 5))")
        .assert()
        .success();

    assert.stdout("27\n");
}

/// TODO: on linux-musl, the packaging of libwasmer.a doesn't work properly
/// Tracked in https://github.com/wasmerio/wasmer/issues/3271
#[cfg_attr(any(target_env = "musl", target_os = "windows"), ignore)]
#[test]
fn test_wasmer_create_exe_pirita_works() {
    // let temp_dir = Path::new("debug");
    // std::fs::create_dir_all(&temp_dir);

    use wasmer_integration_tests_cli::get_repo_root_path;
    let temp_dir = tempfile::TempDir::new().unwrap();
    let temp_dir = temp_dir.path().to_path_buf();
    let python_wasmer_path = temp_dir.join("python.wasmer");
    std::fs::copy(wasi_test_python_path(), &python_wasmer_path).unwrap();
    let python_exe_output_path = temp_dir.join("python");

    let native_target = target_lexicon::HOST;
    let tmp_targz_path = get_repo_root_path().unwrap().join("link.tar.gz");

    println!("compiling to target {native_target}");

    let mut cmd = Command::new(get_wasmer_path());
    cmd.arg("create-exe");
    cmd.arg(&python_wasmer_path);
    cmd.arg("--tarball");
    cmd.arg(&tmp_targz_path);
    cmd.arg("--target");
    cmd.arg(format!("{native_target}"));
    cmd.arg("-o");
    cmd.arg(&python_exe_output_path);
    // change temp_dir to a local path and run this test again
    // to output the compilation files into a debug folder
    //
    // cmd.arg("--debug-dir");
    // cmd.arg(&temp_dir);

    cmd.assert().success();

    println!("compilation ok!");

    if !python_exe_output_path.exists() {
        panic!(
            "python_exe_output_path {} does not exist",
            python_exe_output_path.display()
        );
    }

    println!("invoking command...");

    let mut command = Command::new(&python_exe_output_path);
    command.arg("-c");
    command.arg("print(\"hello\")");

    command.assert().success().stdout("hello\n");
}

// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
#[test]
#[ignore]
fn test_wasmer_run_pirita_works() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let python_wasmer_path = temp_dir.path().join("python.wasmer");
    std::fs::copy(wasi_test_python_path(), &python_wasmer_path).unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(python_wasmer_path)
        .arg("--")
        .arg("-c")
        .arg("print(\"hello\")")
        .assert()
        .success();

    assert.stdout("hello\n");
}

// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
#[test]
#[ignore]
fn test_wasmer_run_pirita_url_works() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wapm.dev/syrusakbary/python")
        .arg("--")
        .arg("-c")
        .arg("print(\"hello\")")
        .assert()
        .success();

    assert.stdout("hello\n");
}

#[test]
fn test_wasmer_run_works_with_dir() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let qjs_path = temp_dir.path().join("qjs.wasm");

    std::fs::copy(wasi_test_wasm_path(), &qjs_path).unwrap();
    std::fs::copy(
        format!("{}/{}", C_ASSET_PATH, "qjs-wasmer.toml"),
        temp_dir.path().join("wasmer.toml"),
    )
    .unwrap();

    assert!(temp_dir.path().exists());
    assert!(temp_dir.path().join("wasmer.toml").exists());
    assert!(temp_dir.path().join("qjs.wasm").exists());

    // test with "wasmer qjs.wasm"
    Command::new(get_wasmer_path())
        .arg(temp_dir.path())
        .arg("--")
        .arg("--quit")
        .assert()
        .success();

    // test again with "wasmer run qjs.wasm"
    Command::new(get_wasmer_path())
        .arg("run")
        .arg(temp_dir.path())
        .arg("--")
        .arg("--quit")
        .assert()
        .success();
}

// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
#[ignore]
#[cfg_attr(target_env = "musl", ignore)]
#[test]
fn test_wasmer_run_works() {
    let assert = Command::new(get_wasmer_path())
        .arg("https://wapm.io/python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // same test again, but this time with "wasmer run ..."
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wapm.io/python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // set wapm.io as the current registry
    let _ = Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.io")
        // will fail, but set wapm.io as the current registry regardless
        .arg("öladkfjasöldfkjasdölfkj")
        .assert()
        .success();

    // same test again, but this time without specifying the registry
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // same test again, but this time with only the command "python" (should be looked up locally)
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("_/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");
}

#[test]
fn run_no_imports_wasm_works() {
    Command::new(get_wasmer_path())
        .arg("run")
        .arg(test_no_imports_wat_path())
        .assert()
        .success();
}

#[test]
fn run_wasi_works_non_existent() -> anyhow::Result<()> {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("does-not/exist")
        .assert()
        .failure();

    assert
        .stderr(contains(
            "Unable to find \"does-not/exist\" in the registry",
        ))
        .stderr(contains("1: Not found"));

    Ok(())
}

// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
#[ignore]
#[test]
fn run_test_caching_works_for_packages() {
    // set wapm.io as the current registry
    Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.io")
        // will fail, but set wapm.io as the current registry regardless
        .arg("öladkfjasöldfkjasdölfkj")
        .assert()
        .success();

    let assert = Command::new(get_wasmer_path())
        .arg("python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    let time = std::time::Instant::now();

    let assert = Command::new(get_wasmer_path())
        .arg("python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // package should be cached
    assert!(std::time::Instant::now() - time < std::time::Duration::from_secs(1));
}

#[test]
fn run_test_caching_works_for_packages_with_versions() {
    // set wapm.io as the current registry
    Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.io")
        // will fail, but set wapm.io as the current registry regardless
        .arg("öladkfjasöldfkjasdölfkj")
        .assert()
        .success();

    let assert = Command::new(get_wasmer_path())
        .arg("python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", ASSET_PATH))
        .arg("/app/test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    let assert = Command::new(get_wasmer_path())
        .arg("python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", ASSET_PATH))
        .arg("/app/test.py")
        .env(
            "RUST_LOG",
            "wasmer_wasix::runtime::package_loader::builtin_loader=debug",
        )
        .assert();

    assert
        .success()
        // it should have ran like normal
        .stdout("hello\n")
        // we hit the cache while fetching the package
        .stderr(contains(
            "builtin_loader: Cache hit! pkg.name=\"python\" pkg.version=0.1.0",
        ));
}

// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
#[ignore]
#[test]
fn run_test_caching_works_for_urls() {
    let assert = Command::new(get_wasmer_path())
        .arg("https://wapm.io/python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    let time = std::time::Instant::now();

    let assert = Command::new(get_wasmer_path())
        .arg("https://wapm.io/python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // package should be cached
    assert!(std::time::Instant::now() - time < std::time::Duration::from_secs(1));
}

// This test verifies that "wasmer run --invoke _start module.wat"
// works the same as "wasmer run module.wat" (without --invoke).
#[test]
fn run_invoke_works_with_nomain_wasi() {
    // In this example the function "wasi_unstable.arg_sizes_get"
    // is a function that is imported from the WASI env.
    let wasi_wat = "
    (module
        (import \"wasi_unstable\" \"args_sizes_get\"
          (func $__wasi_args_sizes_get (param i32 i32) (result i32)))
        (func $_start)
        (memory 1)
        (export \"memory\" (memory 0))
        (export \"_start\" (func $_start))
      )
    ";

    let random = rand::random::<u64>();
    let module_file = std::env::temp_dir().join(format!("{random}.wat"));
    std::fs::write(&module_file, wasi_wat.as_bytes()).unwrap();

    Command::new(get_wasmer_path())
        .arg("run")
        .arg(&module_file)
        .assert()
        .success();

    Command::new(get_wasmer_path())
        .arg("run")
        .arg("--invoke")
        .arg("_start")
        .arg(&module_file)
        .assert()
        .success();

    std::fs::remove_file(&module_file).unwrap();
}

#[test]
fn run_no_start_wasm_report_error() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(test_no_start_wat_path())
        .assert()
        .failure();

    assert.stderr(contains("The module doesn't contain a \"_start\" function"));
}

// Test that wasmer can run a complex path
#[test]
fn test_wasmer_run_complex_url() {
    let wasm_test_path = wasi_test_wasm_path();
    let wasm_test_path = wasm_test_path.canonicalize().unwrap_or(wasm_test_path);
    let mut wasm_test_path = format!("{}", wasm_test_path.display());
    if wasm_test_path.starts_with(r#"\\?\"#) {
        wasm_test_path = wasm_test_path.replacen(r#"\\?\"#, "", 1);
    }
    #[cfg(target_os = "windows")]
    {
        wasm_test_path = wasm_test_path.replace("D:\\", "D://");
        wasm_test_path = wasm_test_path.replace("C:\\", "C://");
        wasm_test_path = wasm_test_path.replace("c:\\", "c://");
        wasm_test_path = wasm_test_path.replace("\\", "/");
        // wasmer run used to fail on c:\Users\username\wapm_packages\ ...
        assert!(
            wasm_test_path.contains("://"),
            "wasm_test_path path is not complex enough"
        );
    }

    Command::new(get_wasmer_path())
        .arg("run")
        .arg(wasm_test_path)
        .arg("--")
        .arg("-q")
        .assert()
        .success();
}
