//! Basic tests for the `run` subcommand

use std::{
    io::{ErrorKind, Read},
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use assert_cmd::{assert::Assert, prelude::OutputAssertExt};
use once_cell::sync::Lazy;
use predicates::str::{contains, is_match};
use rand::Rng;
use reqwest::{blocking::Client, IntoUrl};
use tempfile::TempDir;
use wasmer_integration_tests_cli::{
    asset_path,
    fixtures::{self, packages, php, resources},
    get_wasmer_path,
};

const HTTP_GET_TIMEOUT: Duration = Duration::from_secs(5);

static RUST_LOG: Lazy<String> = Lazy::new(|| {
    if cfg!(feature = "debug") {
        "trace".to_string()
    } else {
        [
            "info",
            "wasmer_wasix::resolve=debug",
            "wasmer_wasix::runners=debug",
            "wasmer_wasix=debug",
            "virtual_fs::trace_fs=trace",
        ]
        .join(",")
    }
});

/// A version of `$RUST_LOG` used for checking cache messages.
static CACHE_RUST_LOG: Lazy<String> = Lazy::new(|| {
    [
        "wasmer_wasix::runtime::resolver::wapm_source=debug",
        "wasmer_wasix::runtime::resolver::web_source=debug",
        "wasmer_wasix::runtime::package_loader::builtin_loader=debug",
        "wasmer_wasix::runtime::module_cache::filesystem=debug",
    ]
    .join(",")
});

#[tokio::test]
async fn aio_http() {
    let status = tokio::process::Command::new(get_wasmer_path())
        .kill_on_drop(true)
        .arg("package")
        .arg("download")
        .arg("wasmer-integration-tests/aio-http-hello-world")
        .arg("-o")
        .arg("aio-http-hello-world.webc")
        .arg("--quiet")
        .spawn()
        .unwrap()
        .wait()
        .await
        .unwrap();

    assert!(status.success());

    let mut wasmer = tokio::process::Command::new(get_wasmer_path())
        .kill_on_drop(true)
        .arg("run")
        .arg("aio-http-hello-world.webc")
        .arg("--net")
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let rsp = reqwest::Client::new()
        .get("http://localhost:34343")
        .send()
        .await
        .unwrap();

    let body = rsp.text().await.unwrap();

    assert_eq!(body, "Hello, World!");

    wasmer.kill().await.unwrap();
    wasmer.wait().await.unwrap();
}

#[test]
#[cfg_attr(feature = "wasmi", ignore = "wasmi currently does not support threads")]
fn list_cwd() {
    let package = packages().join("list-cwd");

    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(package)
        .output()
        .unwrap();

    let stdout = output.stdout;
    eprintln!("{}", String::from_utf8(output.stderr).unwrap());

    let expected = ".
..
main.c
main.wasm
wasmer.toml
"
    .to_owned();

    assert_eq!(expected, String::from_utf8(stdout).unwrap());
}

#[test]
#[cfg_attr(feature = "wasmi", ignore = "wasmi currently does not support threads")]
fn nested_mounted_paths() {
    let package = packages().join("nested-mounted-paths");

    let webc = package.join("out.webc");

    let host_output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(package)
        .output()
        .unwrap();
    let host_stdout = host_output.stdout;
    println!("{}", String::from_utf8(host_output.stderr).unwrap());

    let webc_output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(webc)
        .arg(".")
        .output()
        .unwrap();

    let webc_stdout = webc_output.stdout;
    println!("{}", String::from_utf8(webc_output.stderr).unwrap());

    let expected = "/:
.
..
.app
.private
app
bin
dev
etc
tmp

/app:
.
..
a
b

/app/a:
.
..
data-a.txt

/app/b:
.
..
data-b.txt
"
    .as_bytes()
    .to_vec();

    assert_eq!(&host_stdout, &expected);
    assert_eq!(&webc_stdout, &expected);
}

#[test]
fn run_python_create_temp_dir_in_subprocess() {
    let resources = resources().join("python").join("temp-dir-in-child");

    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg("python/python")
        .arg("--mapdir")
        .arg(format!("/code:{}", resources.display()))
        .arg("--")
        .arg("/code/main.py")
        .output()
        .unwrap();

    if cfg!(not(feature = "wamr")) {
        assert_eq!(output.stdout, "0".as_bytes().to_vec());
    } else {
        // WAMR can print spurious warnings to stdout when running python, so we can't assert that it's exactly `[48]`.
        assert!(output.status.success())
    }
}

#[test]
fn run_php_with_sqlite() {
    let (php_wasm, app_dir, db) = php();

    let output = Command::new(get_wasmer_path())
        .arg("-q")
        .arg("run")
        .arg(php_wasm)
        .arg("--mapdir")
        .arg(format!("/db:{}", db.display()))
        .arg("--mapdir")
        .arg(format!("/app:{}", app_dir.display()))
        .arg("--")
        .arg("/app/test.php")
        .output()
        .unwrap();

    if cfg!(not(feature = "wamr")) {
        assert_eq!(output.stdout, "0".as_bytes().to_vec());
    } else {
        // WAMR can print spurious warnings to stdout when running php, so we can't assert that it's exactly `[48]`.
        assert!(output.status.success())
    }
}

/// Ignored on Windows because running vendored packages does not work
/// since Windows does not allow `::` characters in filenames (every other OS does)
///
/// The syntax for vendored package atoms has to be reworked for this to be fixed, see
/// https://github.com/wasmerio/wasmer/issues/3535
// FIXME: Re-enable. See https://github.com/wasmerio/wasmer/issues/3717
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
        .arg("https://wasmer.io/ciuser/customlambda")
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
        .arg("https://wasmer.io/ciuser/customlambda")
        // TODO: this argument should not be necessary later
        // see https://github.com/wasmerio/wasmer/issues/3514
        .arg("customlambda.py")
        .arg("55")
        .assert()
        .success();
    assert.stdout("139583862445\n");
}

#[test]
fn run_wasi_works() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::qjs())
        .arg("--")
        .arg("-e")
        .arg("print(3 * (4 + 5))")
        .assert()
        .success();

    assert.stdout("27\n");
}

#[test]
fn test_wasmer_run_pirita_works() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let python_wasmer_path = temp_dir.path().join("python.wasmer");
    std::fs::copy(fixtures::python(), &python_wasmer_path).unwrap();

    let output = Command::new(get_wasmer_path())
        .arg("run")
        .arg(python_wasmer_path)
        .arg("--")
        .arg("-c")
        .arg("print(\"hello\")")
        .output()
        .unwrap();

    output.assert().success().stdout("hello\n");
}

#[test]
fn test_wasmer_run_pirita_url_works() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wasmer.wtf/syrusakbary/python")
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

    std::fs::copy(fixtures::qjs(), qjs_path).unwrap();
    std::fs::copy(
        fixtures::qjs_wasmer_toml(),
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
#[test]
#[cfg_attr(feature = "wasmi", ignore = "wasmi currently does not support threads")]
fn test_wasmer_run_works() {
    let assert = Command::new(get_wasmer_path())
        .arg("https://wasmer.io/python/python@0.2.0")
        .arg(format!("--mapdir=.:{}", asset_path().display()))
        .arg("test.py")
        .assert()
        .success();

    if cfg!(not(feature = "wamr")) {
        assert.stdout("hello\n");
    } else {
        // WAMR can print spurious warnings to stdout when running python, so it's better to use
        // `contains` rather than asserting that stdout *is exactly* that
        assert.stdout(contains("hello\n"));
    }

    // same test again, but this time with "wasmer run ..."
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wasmer.io/python/python@0.2.0")
        .arg(format!("--mapdir=.:{}", asset_path().display()))
        .arg("test.py")
        .assert()
        .success();

    if cfg!(not(feature = "wamr")) {
        assert.stdout("hello\n");
    } else {
        // See above
        assert.stdout(contains("hello\n"));
    }

    // same test again, but this time without specifying the registry in the URL
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("python/python@0.2.0")
        .arg(format!("--mapdir=.:{}", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("test.py")
        .assert()
        .success();

    if cfg!(not(feature = "wamr")) {
        assert.stdout("hello\n");
    } else {
        // See above
        assert.stdout(contains("hello\n"));
    }

    // same test again, but this time with only the command "python" (should be looked up locally)
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("_/python")
        .arg(format!("--mapdir=.:{}", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("test.py")
        .assert()
        .success();

    if cfg!(not(feature = "wamr")) {
        assert.stdout("hello\n");
    } else {
        // See above
        assert.stdout(contains("hello\n"));
    }
}

#[test]
fn run_no_imports_wasm_works() {
    Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::fib())
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

#[test]
fn run_test_caching_works_for_packages() {
    // we're testing the cache, so we don't want to reuse the current user's
    // $WASMER_DIR
    let wasmer_dir = TempDir::new().unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("/app/test.py")
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .assert();

    assert
        .success()
        .stderr(contains("wapm_source: Querying the GraphQL API"))
        .stderr(contains("builtin_loader: Downloading a webc file"))
        .stderr(contains("module_cache::filesystem: Saved to disk"));

    let assert = Command::new(get_wasmer_path())
        .arg("python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("/app/test.py")
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .assert()
        .success();

    assert
        .stderr(contains("wapm_source: Cache hit!"))
        .stderr(contains("builtin_loader: Cache hit!"))
        .stderr(contains("module_cache::filesystem: Cache hit!"));
}

#[test]
fn run_test_caching_works_for_packages_with_versions() {
    let wasmer_dir = TempDir::new().unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("/app/test.py")
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert
        .success()
        .stderr(contains("wapm_source: Querying the GraphQL API"))
        .stderr(contains("builtin_loader: Downloading a webc file"))
        .stderr(contains("module_cache::filesystem: Saved to disk"));

    let assert = Command::new(get_wasmer_path())
        .arg("python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("/app/test.py")
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert();

    assert
        .success()
        .stderr(contains("wapm_source: Cache hit!"))
        .stderr(contains("builtin_loader: Cache hit!"))
        .stderr(contains("module_cache::filesystem: Cache hit!"));
}

#[test]
fn run_test_caching_works_for_urls() {
    let wasmer_dir = TempDir::new().unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wasmer.io/python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", asset_path().display()))
        .arg("/app/test.py")
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert
        .success()
        .stderr(contains("builtin_loader: Downloading a webc file"))
        .stderr(contains("module_cache::filesystem: Saved to disk"));

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wasmer.io/python/python@0.1.0")
        .arg(format!("--mapdir=/app:{}", asset_path().display()))
        .arg("/app/test.py")
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert
        // Got a cache hit downloading the *.webc file's metadata
        .stderr(contains("web_source: Cache hit"))
        // Cache hit downloading the *.webc file
        .stderr(contains("builtin_loader: Cache hit! pkg=python@0.1.0"))
        // Cache hit compiling the module
        .stderr(contains("module_cache::filesystem: Cache hit!"));
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
        .arg(fixtures::wat_no_start())
        .assert()
        .failure();

    assert.stderr(contains("The module doesn't contain a \"_start\" function"));
}

// Test that wasmer can run a complex path
#[test]
fn test_wasmer_run_complex_url() {
    let wasm_test_path = fixtures::qjs();
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

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wasi_runner_on_disk() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::qjs())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

/// See <https://github.com/wasmerio/wasmer/issues/4010> for more.
#[test]
fn wasi_runner_on_disk_mount_using_relative_directory_on_the_host() {
    let temp = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::python())
        .arg("--mapdir=/app:.")
        .arg("--")
        .arg("/app/main.py")
        .env("RUST_LOG", &*RUST_LOG)
        .current_dir(temp.path())
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wasi_runner_on_disk_with_mounted_directories() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("index.js"), "console.log('Hello, World!')").unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::qjs())
        .arg(format!("--mapdir=/app:{}", temp.path().display()))
        .arg("--")
        .arg("/app/index.js")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wasi_runner_on_disk_with_mounted_directories_and_webc_volumes() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::python())
        .arg(format!("--mapdir=/app:{}", temp.path().display()))
        .arg("--")
        .arg("-B")
        .arg("/app/main.py")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
#[cfg_attr(feature = "wamr", ignore = "wamr does not support multiple memories")]
fn wasi_runner_on_disk_with_dependencies() {
    let port = random_port();
    let mut cmd = Command::new(get_wasmer_path());
    cmd.arg("run")
        .arg(fixtures::hello())
        .arg(format!("--env=SERVER_PORT={port}"))
        .arg("--net")
        .arg("--")
        .arg("--log-level=info")
        .env("RUST_LOG", &*RUST_LOG);
    let mut child = JoinableChild::spawn(cmd);
    child.wait_for_stderr("listening");

    // Make sure we get the page we want
    let html = reqwest::blocking::get(format!("http://localhost:{port}/"))
        .unwrap()
        .text()
        .unwrap();
    assert!(html.contains("<title>Hello World</title>"), "{html}");

    // and make sure our request was logged
    child
        .join()
        .stderr(contains("incoming request: method=GET uri=/"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn webc_files_on_disk_with_multiple_commands_require_an_entrypoint_flag() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::wabt())
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    let msg = r#"Unable to determine the WEBC file's entrypoint. Please choose one of ["wasm-interp", "wasm-strip", "wasm-validate", "wasm2wat", "wast2json", "wat2wasm"]"#;
    assert.failure().stderr(contains(msg));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wasi_runner_on_disk_with_env_vars() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::python())
        .arg("--env=SOME_VAR=Hello, World!")
        .arg("--")
        .arg("-B")
        .arg("-c")
        .arg("import os; print(os.environ['SOME_VAR'])")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wcgi_runner_on_disk() {
    // Start the WCGI server in the background
    let port = random_port();
    let mut cmd = Command::new(get_wasmer_path());
    cmd.arg("run")
        .arg(format!("--addr=127.0.0.1:{port}"))
        .arg(fixtures::static_server())
        .env("RUST_LOG", &*RUST_LOG);

    // Let's run the command and wait until the server has started
    let mut child = JoinableChild::spawn(cmd);
    child.wait_for_stdout("WCGI Server running");

    // make the request
    let body = http_get(format!("http://127.0.0.1:{port}/")).unwrap();
    assert!(body.contains("<title>Index of /</title>"), "{body}");

    // Let's make sure 404s work too
    let err = http_get(format!("http://127.0.0.1:{port}/this/does/not/exist.html")).unwrap_err();
    assert_eq!(err.status().unwrap(), reqwest::StatusCode::NOT_FOUND);

    // And kill the server, making sure it generated the expected logs
    let assert = child.join();

    assert
        .stderr(contains("Starting the server"))
        .stderr(contains(
            "response generated method=GET uri=/ status_code=200 OK",
        ))
        .stderr(contains(
            "response generated method=GET uri=/this/does/not/exist.html status_code=404 Not Found",
        ));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wcgi_runner_on_disk_with_mounted_directories() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("file.txt"), "Hello, World!").unwrap();
    // Start the WCGI server in the background
    let port = random_port();
    let mut cmd = Command::new(get_wasmer_path());
    cmd.arg("run")
        .arg(format!("--addr=127.0.0.1:{port}"))
        .arg(format!("--mapdir=/path/to:{}", temp.path().display()))
        .arg(fixtures::static_server())
        .env("RUST_LOG", &*RUST_LOG);

    // Let's run the command and wait until the server has started
    let mut child = JoinableChild::spawn(cmd);
    child.wait_for_stdout("WCGI Server running");

    let body = http_get(format!("http://127.0.0.1:{port}/path/to/file.txt")).unwrap();
    assert!(body.contains("Hello, World!"), "{body}");

    // And kill the server, making sure it generated the expected logs
    let assert = child.join();

    assert
        .stderr(contains("Starting the server"))
        .stderr(contains(
            "response generated method=GET uri=/path/to/file.txt status_code=200 OK",
        ));
}

/// See https://github.com/wasmerio/wasmer/issues/3794
#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
#[cfg_attr(feature = "wasmi", ignore = "wasmi currently does not support threads")]
fn issue_3794_unable_to_mount_relative_paths() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("message.txt"), b"Hello, World!").unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::coreutils())
        .arg(format!("--mapdir=./some-dir/:{}", temp.path().display()))
        .arg("--command-name=cat")
        .arg("--")
        .arg("./some-dir/message.txt")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
#[cfg_attr(
    windows,
    ignore = "FIXME(Michael-F-Bryan): Temporarily broken on Windows - https://github.com/wasmerio/wasmer/issues/3929"
)]
#[cfg_attr(
    feature = "wamr",
    ignore = "FIXME(xdoardo): Bash is currently not working in wamr"
)]
fn merged_filesystem_contains_all_files() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::bash())
        .arg("--entrypoint=bash")
        .arg("--use")
        .arg(fixtures::coreutils())
        .arg("--use")
        .arg(fixtures::python())
        .arg("--")
        .arg("-c")
        .arg("ls -l /usr/coreutils/*.md && ls -l /lib/python3.6/*.py")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert
        .success()
        .stdout(contains("/usr/coreutils/README.md"))
        .stdout(contains("/lib/python3.6/this.py"));
}

#[test]
fn run_a_wasi_executable() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::qjs())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
fn wasm_file_with_no_abi() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::fib())
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success();
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn error_if_no_start_function_found() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(fixtures::wat_no_start())
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert
        .failure()
        .stderr(contains("The module doesn't contain a \"_start\" function"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
#[cfg_attr(
    any(feature = "wamr", feature = "v8", feature = "wasmi"),
    ignore = "wasmer using a c_api backend only may not have the 'compile' command"
)]
fn run_a_pre_compiled_wasm_file() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("qjs.wasmu");
    let qjs = fixtures::qjs();
    // Make sure it is compiled
    Command::new(get_wasmer_path())
        .arg("compile")
        .arg("-o")
        .arg(&dest)
        .arg(&qjs)
        .assert()
        .success();
    assert!(dest.exists());

    // Now we can try to run the compiled artifact
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(&dest)
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn wasmer_run_some_directory() {
    let temp = TempDir::new().unwrap();
    std::fs::copy(fixtures::qjs(), temp.path().join("qjs.wasm")).unwrap();
    std::fs::copy(fixtures::qjs_wasmer_toml(), temp.path().join("wasmer.toml")).unwrap();

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(temp.path())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn run_quickjs_via_package_name() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("saghul/quickjs")
        .arg("--entrypoint=quickjs")
        .arg("--registry=wasmer.io")
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
fn run_quickjs_via_url() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("https://wasmer.io/saghul/quickjs")
        .arg("--entrypoint=quickjs")
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(
    all(target_env = "musl", target_os = "linux"),
    ignore = "wasmer run-unstable segfaults on musl"
)]
#[cfg_attr(
    windows,
    ignore = "TODO(Michael-F-Bryan): Figure out why WasiFs::get_inode_at_path_inner() returns Errno::notcapable on Windows"
)]
#[cfg_attr(
    feature = "wamr",
    ignore = "FIXME(xdoardo): Bash is currently not working in wamr"
)]
#[cfg_attr(feature = "wasmi", ignore = "wasmi currently does not support threads")]
fn run_bash_using_coreutils() {
    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg("sharrattj/bash")
        .arg("--entrypoint=bash")
        .arg("--use=sharrattj/coreutils")
        .arg("--registry=wasmer.io")
        .arg("--")
        .arg("-c")
        .arg("ls /bin")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    // Note: the resulting filesystem should contain the main command as
    // well as the commands from all the --use packages

    let some_expected_binaries = [
        "", "arch", "base32", "base64", "baseenc", "basename", "bash", "cat", "",
    ]
    .join("((?s)(.*))");

    assert
        .success()
        .stdout(is_match(some_expected_binaries).unwrap());
}

#[test]
fn run_a_package_that_uses_an_atom_from_a_dependency() {
    let js_script_dir = project_root()
        .join("tests")
        .join("integration")
        .join("cli")
        .join("tests")
        .join("packages")
        .join("js-script");

    let assert = Command::new(get_wasmer_path())
        .arg("run")
        .arg(&js_script_dir)
        .arg("--registry=wasmer.io")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[cfg_attr(feature = "wasmi", ignore = "wasmi currently does not support threads")]
fn local_package_has_write_access_to_its_volumes() {
    let temp = tempfile::tempdir().unwrap();

    std::fs::write(
        temp.path().join("wasmer.toml"),
        r#"
[dependencies]
"python/python" = "*"

[fs]
"/mounted" = "."

[[command]]
name = "run"
module = "python/python:python"
runner = "wasi"

[command.annotations.wasi]
main-args = ["/mounted/script.py"]

        "#,
    )
    .unwrap();

    std::fs::write(
        temp.path().join("script.py"),
        r#"
file = open("/mounted/hello.txt", "w")
file.write("Hello, world!")
        "#,
    )
    .unwrap();

    Command::new(get_wasmer_path())
        .arg("run")
        .arg(temp.path())
        .arg("--registry=wasmer.io")
        .env("RUST_LOG", &*RUST_LOG)
        .assert()
        .success();

    let file_contents =
        String::from_utf8(std::fs::read(temp.path().join("hello.txt")).unwrap()).unwrap();
    assert_eq!(file_contents, "Hello, world!");
}

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
}

/// A helper that wraps [`Child`] to make sure it gets terminated
/// when it is no longer needed.
struct JoinableChild {
    command: Command,
    child: Option<Child>,
}

impl JoinableChild {
    fn spawn(mut cmd: Command) -> Self {
        let child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();

        JoinableChild {
            child: Some(child),
            command: cmd,
        }
    }

    /// Keep reading lines from the child's stdout until a line containing the
    /// desired text is found.
    fn wait_for_stdout(&mut self, text: &str) -> String {
        let stdout = self
            .child
            .as_mut()
            .and_then(|child| child.stdout.as_mut())
            .unwrap();

        wait_for(text, stdout)
    }

    /// Keep reading lines from the child's stderr until a line containing the
    /// desired text is found.
    fn wait_for_stderr(&mut self, text: &str) -> String {
        let stderr = self
            .child
            .as_mut()
            .and_then(|child| child.stderr.as_mut())
            .unwrap();

        wait_for(text, stderr)
    }

    /// Kill the underlying [`Child`] and get an [`Assert`] we
    /// can use to check it.
    fn join(mut self) -> Assert {
        let mut child = self.child.take().unwrap();
        child.kill().unwrap();
        child.wait_with_output().unwrap().assert()
    }
}

fn wait_for(text: &str, reader: &mut dyn Read) -> String {
    let mut all_output = String::new();

    loop {
        let line = read_line(reader).unwrap();

        if line.is_empty() {
            eprintln!("=== All Output === ");
            eprintln!("{all_output}");
            panic!("EOF before \"{text}\" was found");
        }

        let found = line.contains(text);
        all_output.push_str(&line);

        if found {
            return all_output;
        }
    }
}

fn read_line(reader: &mut dyn Read) -> Result<String, std::io::Error> {
    let mut line = Vec::new();

    while !line.ends_with(b"\n") {
        let mut buffer = [0_u8];
        match reader.read_exact(&mut buffer) {
            Ok(_) => {
                line.push(buffer[0]);
            }
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e),
        }
    }

    let line = String::from_utf8(line).map_err(|e| std::io::Error::new(ErrorKind::Other, e))?;
    Ok(line)
}

impl Drop for JoinableChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            eprintln!("==== WARNING: Child was dropped before being joined ====");
            eprintln!("Command: {:?}", self.command);

            let _ = child.kill();

            if let Some(mut stderr) = child.stderr.take() {
                let mut buffer = String::new();
                if stderr.read_to_string(&mut buffer).is_ok() {
                    eprintln!("---- STDERR ----");
                    eprintln!("{buffer}");
                }
            }

            if let Some(mut stdout) = child.stdout.take() {
                let mut buffer = String::new();
                if stdout.read_to_string(&mut buffer).is_ok() {
                    eprintln!("---- STDOUT ----");
                    eprintln!("{buffer}");
                }
            }

            if !std::thread::panicking() {
                panic!("Child was dropped before being joined");
            }
        }
    }
}

/// Send a GET request to a particular URL, automatically retrying (with
/// a timeout) if there are any connection errors.
fn http_get(url: impl IntoUrl) -> Result<String, reqwest::Error> {
    let start = Instant::now();
    let url = url.into_url().unwrap();

    let client = Client::new();

    while start.elapsed() < HTTP_GET_TIMEOUT {
        match client.get(url.clone()).send() {
            Ok(response) => {
                return response.error_for_status()?.text();
            }
            Err(e) if e.is_connect() => continue,
            Err(other) => return Err(other),
        }
    }

    panic!("Didn't receive a response from \"{url}\" within the allocated time");
}

fn random_port() -> u16 {
    rand::thread_rng().gen_range(10_000_u16..u16::MAX)
}
