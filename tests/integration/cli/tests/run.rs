//! Basic tests for the `run` subcommand

use anyhow::bail;
use std::path::PathBuf;
use std::process::Command;
use wasmer_integration_tests_cli::{get_repo_root_path, get_wasmer_path, ASSET_PATH, C_ASSET_PATH};

fn wasi_test_python_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "python-0.1.0.wasmer")
}

fn wasi_test_wasm_path() -> String {
    format!("{}/{}", C_ASSET_PATH, "qjs.wasm")
}

fn test_no_imports_wat_path() -> String {
    format!("{}/{}", ASSET_PATH, "fib.wat")
}

fn test_no_start_wat_path() -> String {
    format!("{}/{}", ASSET_PATH, "no_start.wat")
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn test_cross_compile_python_windows() -> anyhow::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;

    let targets = &[
        "aarch64-darwin",
        "x86_64-darwin",
        "x86_64-linux-gnu",
        "aarch64-linux-gnu",
        // TODO: this test depends on the latest release -gnu64.tar.gz
        // to be present, but we can't release the next release before
        // the integration tests are passing, so this test depends on itself
        //
        // We need to first release a version without -windows being tested
        // then do a second PR to test that Windows works.
        // "x86_64-windows-gnu",
    ];

    for t in targets {
        let python_wasmer_path = temp_dir.path().join(format!("{t}-python"));

        let mut output = Command::new(get_wasmer_path());

        output.arg("create-exe");
        output.arg(wasi_test_python_path());
        output.arg("--target");
        output.arg(t);
        output.arg("-o");
        output.arg(python_wasmer_path.clone());
        let output = output.output()?;

        let stdout = std::str::from_utf8(&output.stdout)
            .expect("stdout is not utf8! need to handle arbitrary bytes");

        let stderr = std::str::from_utf8(&output.stderr)
            .expect("stderr is not utf8! need to handle arbitrary bytes");

        if !output.status.success() {
            bail!("linking failed with: stdout: {stdout}\n\nstderr: {stderr}");
        }

        println!("stdout: {stdout}");
        println!("stderr: {stderr}");

        if !python_wasmer_path.exists() {
            let p = std::fs::read_dir(temp_dir.path())
                .unwrap()
                .filter_map(|e| Some(e.ok()?.path()))
                .collect::<Vec<_>>();
            panic!(
                "target {t} was not compiled correctly {stdout} {stderr}, tempdir: {:#?}",
                p
            );
        }
    }

    Ok(())
}

#[test]
fn run_whoami_works() -> anyhow::Result<()> {
    // running test locally: should always pass since
    // developers don't have access to WAPM_DEV_TOKEN
    if std::env::var("GITHUB_TOKEN").is_err() {
        return Ok(());
    }

    let ciuser_token = std::env::var("WAPM_DEV_TOKEN").expect("no CIUSER / WAPM_DEV_TOKEN token");

    let output = Command::new(get_wasmer_path())
        .arg("login")
        .arg("--registry")
        .arg("wapm.dev")
        .arg(ciuser_token)
        .output()?;

    if !output.status.success() {
        bail!(
            "wasmer login failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    let output = Command::new(get_wasmer_path())
        .arg("whoami")
        .arg("--registry")
        .arg("wapm.dev")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if !output.status.success() {
        bail!(
            "linking failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    assert_eq!(
        stdout,
        "logged into registry \"https://registry.wapm.dev/graphql\" as user \"ciuser\"\n"
    );

    Ok(())
}

#[test]
fn run_wasi_works() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(wasi_test_wasm_path())
        .arg("--")
        .arg("-e")
        .arg("print(3 * (4 + 5))")
        .output()?;

    if !output.status.success() {
        bail!(
            "linking failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    let stdout_output = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(stdout_output, "27\n");

    Ok(())
}

#[cfg(feature = "webc_runner")]
fn package_directory(in_dir: &PathBuf, out: &PathBuf) {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;
    let tar = File::create(out).unwrap();
    let enc = GzEncoder::new(tar, Compression::none());
    let mut a = tar::Builder::new(enc);
    a.append_dir_all("", in_dir).unwrap();
    a.finish().unwrap();
}

/// TODO: on linux-musl, the packaging of libwasmer.a doesn't work properly
/// Tracked in https://github.com/wasmerio/wasmer/issues/3271
#[cfg(not(target_env = "musl"))]
#[cfg(feature = "webc_runner")]
#[test]
fn test_wasmer_create_exe_pirita_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let python_wasmer_path = temp_dir.path().join("python.wasmer");
    std::fs::copy(wasi_test_python_path(), &python_wasmer_path)?;
    let python_exe_output_path = temp_dir.path().join("python");

    let native_target = target_lexicon::HOST;
    let root_path = get_repo_root_path().unwrap();
    let package_path = root_path.join("package");
    if !package_path.join("lib").join("libwasmer.a").exists() {
        let current_dir = std::env::current_dir().unwrap();
        println!("running make && make build-capi && make package-capi && make package...");
        println!("current dir = {}", current_dir.display());
        println!("setting current dir = {}", root_path.display());
        // make && make build-capi && make package-capi && make package
        let mut c1 = std::process::Command::new("make");
        c1.current_dir(&root_path);
        let r = c1.output().unwrap();
        if !r.status.success() {
            let stdout = String::from_utf8_lossy(&r.stdout);
            let stderr = String::from_utf8_lossy(&r.stdout);
            println!("make failed: (stdout = {stdout}, stderr = {stderr})");
        }
        println!("make ok!");
        let mut c1 = std::process::Command::new("make");
        c1.arg("build-wasmer");
        c1.current_dir(&root_path);
        let r = c1.output().unwrap();
        if !r.status.success() {
            let stdout = String::from_utf8_lossy(&r.stdout);
            let stderr = String::from_utf8_lossy(&r.stdout);
            println!("make failed: (stdout = {stdout}, stderr = {stderr})");
        }
        println!("make build-wasmer ok!");
        let mut c1 = std::process::Command::new("make");
        c1.arg("build-capi");
        c1.current_dir(&root_path);
        let r = c1.output().unwrap();
        if !r.status.success() {
            let stdout = String::from_utf8_lossy(&r.stdout);
            let stderr = String::from_utf8_lossy(&r.stdout);
            println!("make build-capi failed: (stdout = {stdout}, stderr = {stderr})");
        }
        println!("make build-capi ok!");

        let mut c1 = std::process::Command::new("make");
        c1.arg("build-wasmer");
        c1.current_dir(&root_path);
        let r = c1.output().unwrap();
        if !r.status.success() {
            let stdout = String::from_utf8_lossy(&r.stdout);
            let stderr = String::from_utf8_lossy(&r.stdout);
            println!("make build-wasmer failed: (stdout = {stdout}, stderr = {stderr})");
        }
        println!("make build-wasmer ok!");

        let mut c1 = std::process::Command::new("make");
        c1.arg("package-capi");
        c1.current_dir(&root_path);
        let r = c1.output().unwrap();
        if !r.status.success() {
            let stdout = String::from_utf8_lossy(&r.stdout);
            let stderr = String::from_utf8_lossy(&r.stdout);
            println!("make package-capi: (stdout = {stdout}, stderr = {stderr})");
        }
        println!("make package-capi ok!");

        let mut c1 = std::process::Command::new("make");
        c1.arg("package");
        c1.current_dir(&root_path);
        let r = c1.output().unwrap();
        if !r.status.success() {
            let stdout = String::from_utf8_lossy(&r.stdout);
            let stderr = String::from_utf8_lossy(&r.stdout);
            println!("make package failed: (stdout = {stdout}, stderr = {stderr})");
        }
        println!("make package ok!");
    }
    if !package_path.exists() {
        panic!("package path {} does not exist", package_path.display());
    }
    let tmp_targz_path = tempfile::TempDir::new()?;
    let tmp_targz_path = tmp_targz_path.path().join("link.tar.gz");
    println!("compiling to target {native_target}");
    println!(
        "packaging /package to .tar.gz: {}",
        tmp_targz_path.display()
    );
    package_directory(&package_path, &tmp_targz_path);
    println!("packaging done");

    let mut cmd = Command::new(get_wasmer_path());
    cmd.arg("create-exe");
    cmd.arg(&python_wasmer_path);
    cmd.arg("--tarball");
    cmd.arg(&tmp_targz_path);
    cmd.arg("--target");
    cmd.arg(format!("{native_target}"));
    cmd.arg("-o");
    cmd.arg(&python_exe_output_path);

    println!("running: {cmd:?}");

    let output = cmd.output()?;

    if !output.status.success() {
        let stdout = std::str::from_utf8(&output.stdout)
            .expect("stdout is not utf8! need to handle arbitrary bytes");

        bail!(
            "running wasmer create-exe {} failed with: stdout: {}\n\nstderr: {}",
            python_wasmer_path.display(),
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    let output = Command::new(&python_exe_output_path)
        .arg("-c")
        .arg("print(\"hello\")")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "1 running python.wasmer failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

#[cfg(feature = "webc_runner")]
#[test]
fn test_wasmer_run_pirita_works() -> anyhow::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let python_wasmer_path = temp_dir.path().join("python.wasmer");
    std::fs::copy(wasi_test_python_path(), &python_wasmer_path)?;

    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(python_wasmer_path)
        .arg("--")
        .arg("-c")
        .arg("print(\"hello\")")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "1 running python.wasmer failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

#[cfg(feature = "webc_runner")]
#[test]
fn test_wasmer_run_pirita_url_works() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wapm.dev/syrusakbary/python")
        .arg("--")
        .arg("-c")
        .arg("print(\"hello\")")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "1 running python.wasmer failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

#[test]
fn test_wasmer_run_works_with_dir() -> anyhow::Result<()> {
    let temp_dir = tempfile::TempDir::new()?;
    let qjs_path = temp_dir.path().join("qjs.wasm");

    std::fs::copy(wasi_test_wasm_path(), &qjs_path)?;
    std::fs::copy(
        format!("{}/{}", C_ASSET_PATH, "qjs-wapm.toml"),
        temp_dir.path().join("wapm.toml"),
    )?;

    assert!(temp_dir.path().exists());
    assert!(temp_dir.path().join("wapm.toml").exists());
    assert!(temp_dir.path().join("qjs.wasm").exists());

    // test with "wasmer qjs.wasm"
    let output = Command::new(get_wasmer_path())
        .arg(temp_dir.path())
        .arg("--")
        .arg("--quit")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if !output.status.success() {
        bail!(
            "running {} failed with: stdout: {}\n\nstderr: {}",
            qjs_path.display(),
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    // test again with "wasmer run qjs.wasm"
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(temp_dir.path())
        .arg("--")
        .arg("--quit")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if !output.status.success() {
        bail!(
            "running {} failed with: stdout: {}\n\nstderr: {}",
            qjs_path.display(),
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

#[cfg(not(target_env = "musl"))]
#[test]
fn test_wasmer_run_works() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("registry.wapm.io/python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "1 running python/python failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    // same test again, but this time with "wasmer run ..."
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg("registry.wapm.io/python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "2 running python/python failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    // same test again, but this time without specifying the registry
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg("python/python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "3 running python/python failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    // same test again, but this time with only the command "python" (should be looked up locally)
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg("python")
        .arg(format!("--mapdir=.:{}", ASSET_PATH))
        .arg("test.py")
        .output()?;

    let stdout = std::str::from_utf8(&output.stdout)
        .expect("stdout is not utf8! need to handle arbitrary bytes");

    if stdout != "hello\n" {
        bail!(
            "3 running python/python failed with: stdout: {}\n\nstderr: {}",
            stdout,
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

#[test]
fn run_no_imports_wasm_works() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(test_no_imports_wat_path())
        .output()?;

    if !output.status.success() {
        bail!(
            "linking failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }

    Ok(())
}

// This test verifies that "wasmer run --invoke _start module.wat"
// works the same as "wasmer run module.wat" (without --invoke).
#[test]
fn run_invoke_works_with_nomain_wasi() -> anyhow::Result<()> {
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
    let module_file = std::env::temp_dir().join(&format!("{random}.wat"));
    std::fs::write(&module_file, wasi_wat.as_bytes()).unwrap();
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(&module_file)
        .output()?;

    let stderr = std::str::from_utf8(&output.stderr).unwrap().to_string();
    let success = output.status.success();
    if !success {
        println!("ERROR in 'wasmer run [module.wat]':\r\n{stderr}");
        panic!();
    }

    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg("--invoke")
        .arg("_start")
        .arg(&module_file)
        .output()?;

    let stderr = std::str::from_utf8(&output.stderr).unwrap().to_string();
    let success = output.status.success();
    if !success {
        println!("ERROR in 'wasmer run --invoke _start [module.wat]':\r\n{stderr}");
        panic!();
    }

    std::fs::remove_file(&module_file).unwrap();
    Ok(())
}

#[test]
fn run_no_start_wasm_report_error() -> anyhow::Result<()> {
    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(test_no_start_wat_path())
        .output()?;

    assert_eq!(output.status.success(), false);
    let result = std::str::from_utf8(&output.stderr).unwrap().to_string();
    assert_eq!(result.contains("Can not find any export functions."), true);
    Ok(())
}
