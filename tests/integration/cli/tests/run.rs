//! Basic tests for the `run` subcommand

use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
};

use assert_cmd::{assert::Assert, prelude::OutputAssertExt};
use once_cell::sync::Lazy;
use predicates::str::contains;
use rand::RngExt;
use tempfile::TempDir;
use wasmer_integration_tests_cli::{
    asset_path,
    fixtures::{self, packages, php, resources},
    integration_webc_path, wasmer_command,
};

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

#[test]
fn list_cwd() {
    let package = packages().join("list-cwd");

    let output = wasmer_command().arg("run").arg(package).output().unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    eprintln!("{}", String::from_utf8(output.stderr).unwrap());

    // As under v1, wasix-libc resolves "./" through the `/` preopen, so this lists the root,
    // where the package directory is mapped at `/data`.
    let listed: Vec<_> = stdout.lines().collect();
    for entry in [".", "..", "data", "etc", "tmp"] {
        assert!(listed.contains(&entry), "{entry} is missing:\n{stdout}");
    }
}

#[test]
fn nested_mounted_paths() {
    let package = packages().join("nested-mounted-paths");

    let webc = package.join("out.webc");

    let host_output = wasmer_command().arg("run").arg(package).output().unwrap();
    let host_stdout = String::from_utf8(host_output.stdout).unwrap();

    let webc_output = wasmer_command()
        .arg("run")
        .arg(webc)
        .arg(".")
        .output()
        .unwrap();

    let webc_stdout = String::from_utf8(webc_output.stdout).unwrap();

    // Each directory's entries, sorted, since a directory lists in no particular order.
    let listings = |stdout: &str| -> Vec<(String, Vec<String>)> {
        stdout
            .split("\n\n")
            .map(|section| {
                let mut lines = section.lines();
                let dir = lines.next().unwrap_or_default().to_string();
                let mut entries: Vec<String> = lines.map(str::to_string).collect();
                entries.sort_unstable();
                (dir, entries)
            })
            .collect()
    };
    let entries = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    for stdout in [&host_stdout, &webc_stdout] {
        let listed = listings(stdout);
        assert_eq!(listed.len(), 4, "{stdout}");
        // The root holds the machine's own directories too; only the mounts matter here.
        assert_eq!(listed[0].0, "/:");
        assert!(listed[0].1.contains(&"app".to_string()), "{stdout}");
        assert_eq!(listed[1], ("/app:".into(), entries(&[".", "..", "a", "b"])));
        assert_eq!(
            listed[2],
            ("/app/a:".into(), entries(&[".", "..", "data-a.txt"]))
        );
        assert_eq!(
            listed[3],
            ("/app/b:".into(), entries(&[".", "..", "data-b.txt"]))
        );
    }
}

// The test would be very slow on Windows or macOS
#[cfg_attr(any(target_os = "macos", target_os = "windows"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn run_python_create_temp_dir_in_subprocess() {
    let resources = resources().join("python").join("temp-dir-in-child");

    let output = wasmer_command()
        .arg("run")
        .arg("python/python")
        .arg("--cranelift")
        .arg("--volume")
        .arg(format!("{}:/code", resources.display()))
        .arg("--")
        .arg("/code/main.py")
        .output()
        .unwrap();

    assert_eq!(output.stdout, "0".as_bytes().to_vec());
}

#[test]
fn run_php_with_sqlite() {
    let (php_wasm, app_dir, db) = php();

    let output = wasmer_command()
        .arg("-q")
        .arg("run")
        .arg(php_wasm)
        .arg("--volume")
        .arg(format!("{}:/db", db.display()))
        .arg("--volume")
        .arg(format!("{}:/app", app_dir.display()))
        .arg("--")
        .arg("/app/test.php")
        .output()
        .unwrap();

    assert_eq!(output.stdout, "0".as_bytes().to_vec());
}

#[test]
fn run_wasi_works() {
    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::qjs())
        .arg("--")
        .arg("-e")
        .arg("print(3 * (4 + 5))")
        .assert()
        .success();

    assert.stdout("27\n");
}

/// See <https://github.com/wasmerio/wasmer/issues/6835>. Needs a WASI module:
/// `fixtures::fib()` imports nothing, so it never reaches the WASI runner.
#[cfg(unix)]
#[test]
fn issue_6835_run_wasi_forwards_non_utf8_host_env() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let assert = wasmer_command()
        .arg("run")
        .arg("--forward-host-env")
        .arg(fixtures::qjs())
        .arg("--")
        .arg("-e")
        .arg("print(3 * (4 + 5))")
        .env("WASMER_TEST_NON_UTF8", OsStr::from_bytes(b"V\xffW"))
        .assert()
        .success();

    assert.stdout("27\n");
}

// The test would be very slow on Windows and macOS
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn test_wasmer_run_pirita_works() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let python_wasmer_path = temp_dir.path().join("python.wasmer");
    std::fs::copy(fixtures::python(), &python_wasmer_path).unwrap();

    let output = wasmer_command()
        .arg("run")
        .arg(python_wasmer_path)
        .arg("--")
        .arg("-c")
        .arg("print(\"hello\")")
        .output()
        .unwrap();

    output.assert().success().stdout("hello\n");
}

// The test would be very slow on Windows or macOS
#[cfg_attr(any(target_os = "macos", target_os = "windows"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn test_wasmer_run_pirita_url_works() {
    let assert = wasmer_command()
        .arg("run")
        .arg("python/python")
        .arg("--cranelift")
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
    wasmer_command()
        .arg(temp_dir.path())
        .arg("--")
        .arg("--quit")
        .assert()
        .success();

    // test again with "wasmer run qjs.wasm"
    wasmer_command()
        .arg("run")
        .arg(temp_dir.path())
        .arg("--")
        .arg("--quit")
        .assert()
        .success();
}

#[test]
// The test would be very slow on Windows and macOS
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn test_wasmer_run_works() {
    let assert = wasmer_command()
        .arg(PYTHON_PACKAGE_WITH_VERSION)
        .arg(format!("--volume={}:.", asset_path().display()))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // same test again, but this time with "wasmer run ..."
    let assert = wasmer_command()
        .arg("run")
        .arg(PYTHON_PACKAGE_WITH_VERSION)
        .arg(format!("--volume={}:.", asset_path().display()))
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");

    // same test again, but this time without specifying the registry in the URL
    let assert = wasmer_command()
        .arg("run")
        .arg(PYTHON_PACKAGE_WITH_VERSION)
        .arg(format!("--volume={}:.", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("test.py")
        .assert()
        .success();

    assert.stdout("hello\n");
}

#[test]
fn run_no_imports_wasm_works() {
    wasmer_command()
        .arg("run")
        .arg(fixtures::fib())
        .assert()
        .success();
}

#[test]
fn run_wasi_works_non_existent() -> anyhow::Result<()> {
    let assert = wasmer_command()
        .arg("run")
        .arg("does-not/exist")
        .assert()
        .failure();

    assert
        .stderr(contains(
            "Unable to find \"does-not/exist\" in the registry",
        ))
        .stderr(contains("1: failed to query package"));

    Ok(())
}

const PYTHON_PACKAGE_WITH_VERSION: &str = "python/python@3.13.5";

// The test would be very slow on Windows or macOS
#[cfg_attr(any(target_os = "macos", target_os = "windows"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn run_test_caching_works_for_packages() {
    // we're testing the cache, so we don't want to reuse the current user's
    // $WASMER_DIR
    let wasmer_dir = TempDir::new().unwrap();

    let assert = wasmer_command()
        .arg("python/python")
        .arg("--cranelift")
        .arg(format!("--volume={}:/app", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("/app/test.py")
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .env("RUST_LOG", "debug")
        .assert();

    assert
        .success()
        .stderr(contains("backend_source: Querying the GraphQL API"))
        .stderr(contains("webc_package_download_start"))
        .stderr(contains("builtin_loader: Saved to disk"));

    let assert = wasmer_command()
        .arg("python/python")
        .arg("--cranelift")
        .arg(format!("--volume={}:/app", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("/app/test.py")
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .env("RUST_LOG", "debug")
        .assert()
        .success();

    assert
        .stderr(contains("backend_source: Cache hit!"))
        .stderr(contains("builtin_loader: Cache hit!"))
        .stderr(contains("module_cache::filesystem: Cache hit!"));
}

// The test would be very slow on Windows or macOS
#[cfg_attr(any(target_os = "macos", target_os = "windows"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn run_test_caching_works_for_packages_with_versions() {
    let wasmer_dir = TempDir::new().unwrap();

    let assert = wasmer_command()
        .arg(PYTHON_PACKAGE_WITH_VERSION)
        .arg(format!("--volume={}:/app", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("--cranelift")
        .arg("/app/test.py")
        .env("RUST_LOG", "debug")
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert
        .success()
        .stderr(contains("backend_source: Querying the GraphQL API"))
        .stderr(contains("webc_package_download_start"))
        .stderr(contains("builtin_loader: Saved to disk"));

    let assert = wasmer_command()
        .arg(PYTHON_PACKAGE_WITH_VERSION)
        .arg(format!("--volume={}:/app", asset_path().display()))
        .arg("--registry=wasmer.io")
        .arg("--cranelift")
        .arg("/app/test.py")
        .env("RUST_LOG", "debug")
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert();

    assert
        .stderr(contains("backend_source: Cache hit!"))
        .stderr(contains("builtin_loader: Cache hit!"))
        .stderr(contains("module_cache::filesystem: Cache hit!"));
}

// The test would be very slow on Windows or macOS
#[cfg_attr(any(target_os = "macos", target_os = "windows"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn run_test_caching_works_for_urls() {
    let wasmer_dir = TempDir::new().unwrap();

    let assert = wasmer_command()
        .arg("run")
        .arg(format!("https://wasmer.io/{PYTHON_PACKAGE_WITH_VERSION}"))
        .arg(format!("--volume={}:/app", asset_path().display()))
        .arg("--cranelift")
        .arg("/app/test.py")
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert
        .success()
        .stderr(contains("webc_package_download_start"))
        .stderr(contains("builtin_loader: Saved to disk"));

    let assert = wasmer_command()
        .arg("run")
        .arg(format!("https://wasmer.io/{PYTHON_PACKAGE_WITH_VERSION}"))
        .arg(format!("--volume={}:/app", asset_path().display()))
        .arg("--cranelift")
        .arg("/app/test.py")
        .env("RUST_LOG", &*CACHE_RUST_LOG)
        .env("WASMER_CACHE_DIR", wasmer_dir.path())
        .assert()
        .success();

    assert
        // Got a cache hit downloading the *.webc file's metadata
        .stderr(contains("web_source: Cache hit"))
        // Cache hit downloading the *.webc file (it's identified based on sha256 hash - not the package name)
        .stderr(contains("builtin_loader: Cache hit!"))
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

    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .assert()
        .success();

    wasmer_command()
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
    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::wat_no_start())
        .assert()
        .failure();

    assert.stderr(contains("The module doesn't export a \"_start\" function"));
}

#[test]
fn run_invoke_reports_unsupported_funcref_result() {
    let wat = r#"
        (module
            (table 1 funcref)
            (func $target)
            (elem (i32.const 0) func $target)
            (func (export "return_funcref") (result funcref)
                (table.get (i32.const 0))))
    "#;
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("funcref-result.wat");
    std::fs::write(&module_file, wat).unwrap();

    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .arg("--invoke")
        .arg("return_funcref")
        .assert()
        .failure()
        .stderr(contains(
            "Function result type FuncRef is not supported by --invoke",
        ));
}

#[cfg(feature = "v8")]
#[test]
fn run_v8_wasi_proc_exit_zero_is_success() {
    let wasi_wat = "
    (module
        (import \"wasi_snapshot_preview1\" \"proc_exit\"
          (func $__wasi_proc_exit (param i32)))
        (func $_start
          i32.const 0
          call $__wasi_proc_exit)
        (memory 1)
        (export \"memory\" (memory 0))
        (export \"_start\" (func $_start))
      )
    ";

    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("proc_exit_zero.wat");
    std::fs::write(&module_file, wasi_wat.as_bytes()).unwrap();

    wasmer_command()
        .arg("run")
        .arg("--v8")
        .arg(&module_file)
        .assert()
        .success();
}

const WASIXV2_HELLO_WAT: &str = r#"
    (module
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "hello")
      (func (export "_start")
        (drop (call $write (i32.const 1) (i32.const 16) (i32.const 5)))
        (call $exit_group (i32.const 7))
        unreachable))
"#;

#[test]
fn run_wasixv2_module_forwards_stdout_and_exit_status() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("hello.wat");
    std::fs::write(&module_file, WASIXV2_HELLO_WAT).unwrap();

    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .assert()
        .code(7)
        .stdout("hello");
}

#[test]
fn run_wasixv2_module_rejects_unsupported_options() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("hello.wat");
    std::fs::write(&module_file, WASIXV2_HELLO_WAT).unwrap();

    wasmer_command()
        .arg("run")
        .arg("--net=ipv4:allow=127.0.0.1:80/in")
        .arg("--invoke")
        .arg("main")
        .arg(&module_file)
        .assert()
        .failure()
        .stdout("")
        .stderr(contains(
            "the WASIX v2 kernel does not support network rulesets (--net=<RULES>), --invoke yet",
        ));
}

/// Plain `--net` changes nothing on the WASIX v2 kernel, whose guests always reach the host's
/// network, so it is accepted (`binfmt_misc` registrations pass it), as is `--invoke _start`.
#[test]
fn run_wasixv2_module_accepts_plain_net_and_invoking_start() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("hello.wat");
    std::fs::write(&module_file, WASIXV2_HELLO_WAT).unwrap();

    wasmer_command()
        .arg("run")
        .arg("--net")
        .arg("--invoke=_start")
        .arg(&module_file)
        .assert()
        .code(7)
        .stdout("hello");
}

/// Writes `hello` with `fd_write` and exits with 7 through `proc_exit`.
const WASIX1_HELLO_WAT: &str = r#"
    (module
      (import "wasi_snapshot_preview1" "fd_write"
        (func $fd_write (param i32 i32 i32 i32) (result i32)))
      (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
      (memory (export "memory") 1)
      ;; One iovec at 0: "hello" at 16.
      (data (i32.const 0) "\10\00\00\00\05\00\00\00")
      (data (i32.const 16) "hello")
      (func (export "_start")
        (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 8)))
        (call $proc_exit (i32.const 7))))
"#;

/// A WASI preview1 module runs on the WASIX v2 kernel through its v1 compatibility ABI.
#[test]
fn run_wasix1_module_forwards_stdout_and_exit_status() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("hello.wat");
    std::fs::write(&module_file, WASIX1_HELLO_WAT).unwrap();

    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .assert()
        .code(7)
        .stdout("hello");
}

#[test]
fn run_wasix1_wasm64_module_is_rejected() {
    let wat = r#"
        (module
          (import "wasix_64v1" "proc_exit" (func $proc_exit (param i32)))
          (memory (export "memory") i64 1)
          (func (export "_start") (call $proc_exit (i32.const 0))))
    "#;
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("wasm64.wat");
    std::fs::write(&module_file, wat).unwrap();

    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .assert()
        .failure()
        .stderr(contains("wasm64 WASIX v1 modules are not supported"));
}

/// Prints `/data/msg.txt`, then creates `/data/new.txt`. Exits with 0, or the errno of the
/// first call that failed.
const WASIXV2_PACKAGE_WAT: &str = r#"
    (module
      (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "/data/msg.txt\00")
      (data (i32.const 48) "/data/new.txt\00")
      (func $check (param $ret i32) (result i32)
        (if (i32.lt_s (local.get $ret) (i32.const 0))
          (then (call $exit_group (i32.sub (i32.const 0) (local.get $ret))) unreachable))
        (local.get $ret))
      (func (export "_start")
        (local $fd i32)
        (local.set $fd (call $check
          (call $openat (i32.const -100) (i32.const 16) (i32.const 0) (i32.const 0))))
        (drop (call $write (i32.const 1) (i32.const 1024)
          (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 512)))))
        ;; O_WRONLY | O_CREAT, 0644: package volumes are writable, as on v1.
        (drop (call $check
          (call $openat (i32.const -100) (i32.const 48) (i32.const 0x41) (i32.const 420))))
        (call $exit_group (i32.const 0))
        unreachable))
"#;

/// A package directory whose command runs a WASIX v2 module, with `data/` mounted at `/data`.
fn wasixv2_package_dir() -> TempDir {
    let temp = TempDir::new().unwrap();
    let wasm = wat::parse_str(WASIXV2_PACKAGE_WAT).unwrap();
    std::fs::write(temp.path().join("main.wasm"), wasm).unwrap();
    std::fs::create_dir(temp.path().join("data")).unwrap();
    std::fs::write(temp.path().join("data/msg.txt"), "from the package\n").unwrap();
    std::fs::write(
        temp.path().join("wasmer.toml"),
        r#"
            [[module]]
            name = "main"
            source = "main.wasm"

            [fs]
            "/data" = "data"

            [[command]]
            name = "main"
            module = "main"
            runner = "wasi"
        "#,
    )
    .unwrap();
    temp
}

#[test]
fn run_wasixv2_webc_mounts_package_volumes() {
    let package = wasixv2_package_dir();
    let out = TempDir::new().unwrap();
    let webc = out.path().join("wasixv2.webc");
    wasmer_command()
        .arg("-q")
        .arg("package")
        .arg("build")
        .arg(package.path())
        .arg("-o")
        .arg(&webc)
        .assert()
        .success();
    // The package's files are only in the webc from here on.
    drop(package);

    wasmer_command()
        .arg("run")
        .arg(&webc)
        .assert()
        .success()
        .stdout("from the package\n");
}

/// A webc v2 file is migrated to v3 in memory, whose volumes the WASIX v2 kernel mounts.
#[test]
fn run_webc_v2_file_is_migrated() {
    let package = wasixv2_package_dir();
    let out = TempDir::new().unwrap();
    let v3 = out.path().join("v3.webc");
    wasmer_command()
        .arg("-q")
        .arg("package")
        .arg("build")
        .arg(package.path())
        .arg("-o")
        .arg(&v3)
        .assert()
        .success();
    drop(package);
    let v2 = out.path().join("v2.webc");
    let migrated = webc::migration::v3_to_v2(std::fs::read(&v3).unwrap()).unwrap();
    std::fs::write(&v2, migrated).unwrap();

    wasmer_command()
        .arg("run")
        .arg(&v2)
        .assert()
        .success()
        .stdout("from the package\n")
        .stderr(contains("WebC v2 is a deprecated format"));
}

#[test]
fn run_wasixv2_package_dir_maps_its_fs_table() {
    let package = wasixv2_package_dir();

    wasmer_command()
        .arg("run")
        .arg(package.path())
        .assert()
        .success()
        .stdout("from the package\n");
    assert!(package.path().join("data/new.txt").exists());
}

/// Prints `uname -n` and `uname -r`, space-separated, and exits with its effective uid.
const WASIXV2_UNAME_WAT: &str = r#"
    (module
      (import "wasixv2" "uname" (func $uname (param i32) (result i32)))
      (import "wasixv2" "geteuid" (func $geteuid (result i32)))
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 8) " ")
      (func $strlen (param $p i32) (result i32) (local $n i32)
        (block $done
          (loop $next
            (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $p) (local.get $n)))))
            (local.set $n (i32.add (local.get $n) (i32.const 1)))
            (br $next)))
        (local.get $n))
      (func $print (param $p i32)
        (drop (call $write (i32.const 1) (local.get $p) (call $strlen (local.get $p)))))
      (func (export "_start")
        (drop (call $uname (i32.const 1024)))
        ;; nodename is the second 65-byte field of `struct new_utsname`, release the third.
        (call $print (i32.const 1089))
        (drop (call $write (i32.const 1) (i32.const 8) (i32.const 1)))
        (call $print (i32.const 1154))
        (call $exit_group (call $geteuid))
        unreachable))
"#;

#[test]
fn run_wasixv2_reports_the_configured_uname_and_user() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("uname.wat");
    std::fs::write(&module_file, WASIXV2_UNAME_WAT).unwrap();

    wasmer_command()
        .arg("run")
        .args([
            "--user",
            "42:7",
            "--hostname",
            "box",
            "--uname-release",
            "6.1.0",
        ])
        .arg(&module_file)
        .assert()
        .code(42)
        .stdout("box 6.1.0");
    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .assert()
        .code(1)
        .stdout("localhost 2.0.0");
    wasmer_command()
        .arg("run")
        .args(["--uname-release", &"x".repeat(65)])
        .arg(&module_file)
        .assert()
        .failure()
        .stderr(contains("invalid --uname-release"));
}

/// Binds TCP port 80, then creates `/f`, gives it to uid 1234, makes it mode 0 and opens it for
/// reading: all things only root may do. Exits with 0, or with the number of the step that
/// failed after printing its errno as a decimal number.
const WASIXV2_ROOT_ONLY_WAT: &str = r#"
    (module
      (import "wasixv2" "socket" (func $socket (param i32 i32 i32) (result i32)))
      (import "wasixv2" "bind" (func $bind (param i32 i32 i32) (result i32)))
      (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "fchownat" (func $fchownat (param i32 i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "fchmodat" (func $fchmodat (param i32 i32 i32) (result i32)))
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      ;; sockaddr_in for 127.0.0.1:80
      (data (i32.const 16) "\02\00\00\50\7f\00\00\01\00\00\00\00\00\00\00\00")
      (data (i32.const 32) "/f\00")
      (func $check (param $v i32) (param $step i32) (result i32)
        (local $errno i32)
        (if (i32.lt_s (local.get $v) (i32.const 0))
          (then
            (local.set $errno (i32.sub (i32.const 0) (local.get $v)))
            ;; Two decimal digits.
            (i32.store8 (i32.const 0)
              (i32.add (i32.const 48) (i32.div_u (local.get $errno) (i32.const 10))))
            (i32.store8 (i32.const 1)
              (i32.add (i32.const 48) (i32.rem_u (local.get $errno) (i32.const 10))))
            (drop (call $write (i32.const 1) (i32.const 0) (i32.const 2)))
            (call $exit_group (local.get $step))
            unreachable))
        (local.get $v))
      (func (export "_start") (local $fd i32)
        (local.set $fd (call $check (call $socket (i32.const 2) (i32.const 1) (i32.const 0)) (i32.const 1)))
        (drop (call $check (call $bind (local.get $fd) (i32.const 16) (i32.const 16)) (i32.const 2)))
        ;; O_WRONLY | O_CREAT, 0600
        (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0x41) (i32.const 384)) (i32.const 3)))
        (drop (call $check (call $fchownat (i32.const -100) (i32.const 32) (i32.const 1234) (i32.const 1234) (i32.const 0)) (i32.const 4)))
        (drop (call $check (call $fchmodat (i32.const -100) (i32.const 32) (i32.const 0)) (i32.const 5)))
        (drop (call $check (call $openat (i32.const -100) (i32.const 32) (i32.const 0) (i32.const 0)) (i32.const 6)))
        (call $exit_group (i32.const 0))
        unreachable))
"#;

#[test]
fn run_wasixv2_as_root_binds_low_ports_and_overrides_file_permissions() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("root.wat");
    std::fs::write(&module_file, WASIXV2_ROOT_ONLY_WAT).unwrap();

    wasmer_command()
        .arg("run")
        .args(["--user", "0"])
        .arg(&module_file)
        .assert()
        .success()
        .stdout("");
    // The default user is unprivileged: binding port 80 is EACCES.
    wasmer_command()
        .arg("run")
        .arg(&module_file)
        .assert()
        .code(2)
        .stdout("13");
}

/// Prints `/msg.txt`, then opens `/etc/hosts` and creates `/new.txt`. Exits with 0, or the
/// errno of the first call that failed.
const WASIXV2_ROOT_VOLUME_WAT: &str = r#"
    (module
      (import "wasixv2" "openat" (func $openat (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "/msg.txt\00")
      (data (i32.const 32) "/etc/hosts\00")
      (data (i32.const 48) "/new.txt\00")
      (func $check (param $ret i32) (result i32)
        (if (i32.lt_s (local.get $ret) (i32.const 0))
          (then (call $exit_group (i32.sub (i32.const 0) (local.get $ret))) unreachable))
        (local.get $ret))
      (func (export "_start")
        (local $fd i32)
        (local.set $fd (call $check
          (call $openat (i32.const -100) (i32.const 16) (i32.const 0) (i32.const 0))))
        (drop (call $write (i32.const 1) (i32.const 1024)
          (call $check (call $read (local.get $fd) (i32.const 1024) (i32.const 512)))))
        (drop (call $check
          (call $openat (i32.const -100) (i32.const 32) (i32.const 0) (i32.const 0))))
        ;; O_WRONLY | O_CREAT, 0644
        (drop (call $check
          (call $openat (i32.const -100) (i32.const 48) (i32.const 0x41) (i32.const 420))))
        (call $exit_group (i32.const 0))
        unreachable))
"#;

/// A package directory whose `fs` table maps its `root/` to `/`.
fn root_volume_package_dir() -> TempDir {
    let temp = TempDir::new().unwrap();
    let wasm = wat::parse_str(WASIXV2_ROOT_VOLUME_WAT).unwrap();
    std::fs::write(temp.path().join("main.wasm"), wasm).unwrap();
    std::fs::create_dir(temp.path().join("root")).unwrap();
    std::fs::write(temp.path().join("root/msg.txt"), "from the root volume\n").unwrap();
    std::fs::write(
        temp.path().join("wasmer.toml"),
        r#"
            [[module]]
            name = "main"
            source = "main.wasm"

            [fs]
            "/" = "root"

            [[command]]
            name = "main"
            module = "main"
            runner = "wasi"
        "#,
    )
    .unwrap();
    temp
}

/// A volume at `/` is a layer of the root, under the machine's own `/etc` and `/tmp`; changes
/// stay in memory.
#[test]
fn run_package_with_a_root_volume() {
    let package = root_volume_package_dir();
    wasmer_command()
        .arg("run")
        .arg(package.path())
        .assert()
        .success()
        .stdout("from the root volume\n");
    assert!(!package.path().join("root/new.txt").exists());

    let out = TempDir::new().unwrap();
    let webc = out.path().join("root-volume.webc");
    wasmer_command()
        .arg("-q")
        .arg("package")
        .arg("build")
        .arg(package.path())
        .arg("-o")
        .arg(&webc)
        .assert()
        .success();
    drop(package);
    wasmer_command()
        .arg("run")
        .arg(&webc)
        .assert()
        .success()
        .stdout("from the root volume\n");
}

/// Prints `ready`, then waits in `ppoll` with nothing to poll and no timeout, forever.
const WASIXV2_BLOCK_WAT: &str = r#"
    (module
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "ppoll_time64" (func $ppoll (param i32 i32 i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "ready\n")
      (func (export "_start")
        (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
        (loop $forever
          (drop (call $ppoll (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 8)))
          (br $forever))))
"#;

/// Blocks `SIGINT`, prints `ready`, then waits for it with `rt_sigtimedwait` and exits with the
/// number of the signal it took, or 100 + errno.
const WASIXV2_SIGWAIT_WAT: &str = r#"
    (module
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "rt_sigprocmask" (func $sigprocmask (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "rt_sigtimedwait_time64"
        (func $sigtimedwait (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      ;; The set {SIGINT}.
      (data (i32.const 8) "\02\00\00\00\00\00\00\00")
      (data (i32.const 16) "ready\n")
      (func (export "_start")
        (local $ret i32)
        ;; SIG_BLOCK
        (local.set $ret (call $sigprocmask (i32.const 0) (i32.const 8) (i32.const 0) (i32.const 8)))
        (if (i32.lt_s (local.get $ret) (i32.const 0))
          (then (call $exit_group (i32.sub (i32.const 100) (local.get $ret)))))
        (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
        (local.set $ret
          (call $sigtimedwait (i32.const 8) (i32.const 256) (i32.const 0) (i32.const 8)))
        (if (i32.lt_s (local.get $ret) (i32.const 0))
          (then (call $exit_group (i32.sub (i32.const 100) (local.get $ret)))))
        (call $exit_group (local.get $ret))
        unreachable))
"#;

/// Blocks `SIGINT`, prints `ready`, then waits in `ppoll` forever: Ctrl-C never reaches it.
const WASIXV2_DEAF_WAT: &str = r#"
    (module
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "rt_sigprocmask" (func $sigprocmask (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "ppoll_time64" (func $ppoll (param i32 i32 i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      ;; The set {SIGINT}.
      (data (i32.const 8) "\02\00\00\00\00\00\00\00")
      (data (i32.const 16) "ready\n")
      (func (export "_start")
        ;; SIG_BLOCK
        (drop (call $sigprocmask (i32.const 0) (i32.const 8) (i32.const 0) (i32.const 8)))
        (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
        (loop $forever
          (drop (call $ppoll (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 0) (i32.const 8)))
          (br $forever))))
"#;

/// Blocks `SIGINT`, prints `ready`, then takes four `SIGINT`s one by one with `rt_sigtimedwait`
/// and exits with 4, or 100 + errno.
const WASIXV2_SIGWAIT_FOUR_WAT: &str = r#"
    (module
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (import "wasixv2" "rt_sigprocmask" (func $sigprocmask (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "rt_sigtimedwait_time64"
        (func $sigtimedwait (param i32 i32 i32 i32) (result i32)))
      (import "wasixv2" "exit_group" (func $exit_group (param i32)))
      (memory (export "memory") 1)
      ;; The set {SIGINT}.
      (data (i32.const 8) "\02\00\00\00\00\00\00\00")
      (data (i32.const 16) "ready\n")
      (func (export "_start")
        (local $ret i32)
        (local $taken i32)
        ;; SIG_BLOCK
        (drop (call $sigprocmask (i32.const 0) (i32.const 8) (i32.const 0) (i32.const 8)))
        (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
        (loop $next
          (local.set $ret
            (call $sigtimedwait (i32.const 8) (i32.const 256) (i32.const 0) (i32.const 8)))
          (if (i32.lt_s (local.get $ret) (i32.const 0))
            (then (call $exit_group (i32.sub (i32.const 100) (local.get $ret)))))
          (local.set $taken (i32.add (local.get $taken) (i32.const 1)))
          (br_if $next (i32.lt_u (local.get $taken) (i32.const 4))))
        (call $exit_group (local.get $taken))
        unreachable))
"#;

/// Prints `ready`, then spins without ever making a syscall.
const WASIXV2_SPIN_WAT: &str = r#"
    (module
      (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 16) "ready\n")
      (func (export "_start")
        (drop (call $write (i32.const 1) (i32.const 16) (i32.const 6)))
        (loop $forever (br $forever))))
"#;

/// Runs `wat` with `wasmer run`, waits for it to print `ready`, sends the `wasmer` process
/// `signals`, a little apart, and returns how it exited. Every signal but the last must leave
/// it running.
#[cfg(unix)]
fn run_wasixv2_and_signal(wat: &str, signals: &[i32]) -> ExitStatus {
    use std::io::{BufRead, BufReader};
    use std::time::{Duration, Instant};

    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("guest.wat");
    std::fs::write(&module_file, wat).unwrap();
    let mut child = wasmer_command()
        .arg("run")
        .arg(&module_file)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line, "ready\n");
    for (i, &sig) in signals.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(200));
            assert!(child.try_wait().unwrap().is_none(), "wasmer exited early");
        }
        assert_eq!(unsafe { libc::kill(child.id() as libc::pid_t, sig) }, 0);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("wasmer did not exit after the signal");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A host `SIGTERM` reaches the guest, which has no handler and so dies of the default action:
/// `wasmer` exits with 128 + 15 itself rather than being killed by the signal.
#[cfg(unix)]
#[test]
fn run_wasixv2_forwards_sigterm_to_the_guest() {
    use std::os::unix::process::ExitStatusExt as _;

    let status = run_wasixv2_and_signal(WASIXV2_BLOCK_WAT, &[libc::SIGTERM]);
    assert_eq!(status.signal(), None, "wasmer died of the signal: {status}");
    assert_eq!(status.code(), Some(128 + 15));
}

/// A guest that waits for `SIGINT` takes the host's.
#[cfg(unix)]
#[test]
fn run_wasixv2_delivers_sigint_to_a_waiting_guest() {
    let status = run_wasixv2_and_signal(WASIXV2_SIGWAIT_WAT, &[libc::SIGINT]);
    assert_eq!(status.code(), Some(2), "{status}");
}

/// The third Ctrl-C ends `wasmer` with 130 even when the guest never takes them.
#[cfg(unix)]
#[test]
fn run_wasixv2_exits_on_the_third_sigint() {
    use std::os::unix::process::ExitStatusExt as _;

    let sigints = [libc::SIGINT; 3];
    let status = run_wasixv2_and_signal(WASIXV2_DEAF_WAT, &sigints);
    assert_eq!(status.signal(), None, "wasmer died of the signal: {status}");
    assert_eq!(status.code(), Some(130));
}

/// Ctrl-C only forces an exit while the guest leaves the earlier ones untaken: a guest that takes
/// every one of them keeps running past the third.
#[cfg(unix)]
#[test]
fn run_wasixv2_keeps_a_guest_that_takes_each_sigint() {
    let sigints = [libc::SIGINT; 4];
    let status = run_wasixv2_and_signal(WASIXV2_SIGWAIT_FOUR_WAT, &sigints);
    assert_eq!(status.code(), Some(4), "{status}");
}

/// A guest spinning without syscalls still takes a signal, at a preemption point.
#[cfg(unix)]
#[test]
fn run_wasixv2_signals_a_busy_guest() {
    let status = run_wasixv2_and_signal(WASIXV2_SPIN_WAT, &[libc::SIGTERM]);
    assert_eq!(status.code(), Some(128 + 15), "{status}");
}

/// A trap kills the guest with the signal of its fault, reported as 128 + the signal, and
/// `wasmer` says why on stderr.
#[test]
fn run_wasixv2_reports_a_trap_as_death_by_its_signal() {
    let temp = TempDir::new().unwrap();
    let module_file = temp.path().join("guest.wat");
    let wat = r#"
        (module
          (import "wasixv2" "exit_group" (func $exit_group (param i32)))
          (memory (export "memory") 1)
          (func (export "_start") unreachable))
    "#;
    std::fs::write(&module_file, wat).unwrap();

    let output = wasmer_command()
        .arg("run")
        .arg(&module_file)
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(128 + 4), "{stderr}");
    assert!(
        stderr.contains("trapped, killing its process with SIGILL"),
        "{stderr}"
    );
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

    wasmer_command()
        .arg("run")
        .arg(wasm_test_path)
        .arg("--")
        .arg("-q")
        .assert()
        .success();
}

#[test]
fn wasi_runner_on_disk() {
    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::qjs())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

// See <https://github.com/wasmerio/wasmer/issues/4010> for more.
// The test would be very slow on Windows and macOS
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn wasi_runner_on_disk_mount_using_relative_directory_on_the_host() {
    let temp = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::python())
        .arg("--volume=.:/app")
        .arg("--")
        .arg("/app/main.py")
        .env("RUST_LOG", &*RUST_LOG)
        .current_dir(temp.path())
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
fn wasi_runner_on_disk_with_mounted_directories() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("index.js"), "console.log('Hello, World!')").unwrap();

    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::qjs())
        .arg(format!("--volume={}:/app", temp.path().display()))
        .arg("--")
        .arg("/app/index.js")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[cfg(target_os = "linux")]
#[test]
fn local_package_fs_mounts_work_for_dir_and_webc() {
    let fixture = packages().join("fs-mount");
    let temp = TempDir::new().unwrap();
    let webc = temp.path().join("fs-mount-test.webc");

    compile_wasix_source(
        &fixture.join("main.c"),
        &temp.path().join("main.wasm"),
        false,
    );
    std::fs::copy(
        fixture.join("testfile.txt"),
        temp.path().join("testfile.txt"),
    )
    .unwrap();
    std::fs::copy(fixture.join("wasmer.toml"), temp.path().join("wasmer.toml")).unwrap();

    wasmer_command()
        .arg("run")
        .arg(temp.path())
        .env("RUST_LOG", &*RUST_LOG)
        .assert()
        .success();

    wasmer_command()
        .arg("-q")
        .arg("package")
        .arg("build")
        .arg(".")
        .arg("-o")
        .arg(&webc)
        .current_dir(temp.path())
        .assert()
        .success();

    wasmer_command()
        .arg("run")
        .arg(&webc)
        .env("RUST_LOG", &*RUST_LOG)
        .assert()
        .success();
}

/// The `wasm_c_api_v0` host imports were an extension of the legacy WASIX runtime, which
/// `wasmer run` no longer uses; the WASIX v2 kernel does not provide them (yet), and says so
/// instead of running the guest without them.
#[cfg(target_os = "linux")]
#[test]
fn wasmer_run_wasix_wasm_c_api_guest_is_refused() {
    let temp = TempDir::new().unwrap();
    let wasm = temp.path().join("wasix-wasm-c-api-smoke.wasm");

    compile_wasix_source(&fixtures::wasix_wasm_c_api_smoke_c(), &wasm, false);

    wasmer_command()
        .arg("run")
        .arg(&wasm)
        .env("RUST_LOG", &*RUST_LOG)
        .assert()
        .failure()
        .stderr(contains("wasm_c_api_v0"));
}

#[test]
// The test would be very slow on Windows and macOS
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn wasi_runner_on_disk_with_mounted_directories_and_webc_volumes() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::python())
        .arg(format!("--volume={}:/app", temp.path().display()))
        .arg("--")
        .arg("-B")
        .arg("/app/main.py")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
// For some reason the port forwarding does not work on macOS
#[cfg_attr(target_os = "macos", ignore)]
fn wasi_runner_on_disk_with_dependencies() {
    let port = random_port();
    let mut cmd = wasmer_command();
    // The guest listens inside the machine; `--publish` makes the port reachable from here.
    cmd.arg("run")
        .arg("wasmer/hello")
        .arg(format!("--env=SERVER_PORT={port}"))
        .arg(format!("--publish={port}:{port}"))
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
    assert!(html.contains("<title>wasmer/hello</title>"), "{html}");

    // and make sure our request was logged
    child
        .join()
        .stderr(contains("incoming request: method=GET uri=/"));
}

#[test]
fn webc_v2_emits_a_deprecation_warning() {
    let webc = integration_webc_path()
        .join("static-web-server-async-1.0.3-5d739d1a-20b7-4edf-8cf4-44e813f96b25.webc");

    wasmer_command()
        .arg("run")
        .arg(webc)
        .arg("--")
        .arg("--help")
        .assert()
        .success()
        .stderr(contains("WebC v2 is a deprecated format"));
}

#[test]
fn webc_files_on_disk_with_multiple_commands_require_an_entrypoint_flag() {
    let assert = wasmer_command()
        .arg("run")
        .arg("wabt")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    let msg = r#"Unable to determine the package's entrypoint. Please choose one of ["wasm-interp", "wasm-strip", "wasm-validate", "wasm2wat", "wast2json", "wat2wasm"]"#;
    assert.failure().stderr(contains(msg));
}

// The test would be very slow on Windows and macOS
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
fn wasi_runner_on_disk_with_env_vars() {
    let assert = wasmer_command()
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

#[cfg_attr(
    target_os = "windows",
    ignore = "wasmer/bash packages require wasm exception handling support on Windows"
)]
#[test]
fn issue_3794_unable_to_mount_relative_paths() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("message.txt"), b"Hello, World!").unwrap();

    let assert = wasmer_command()
        .arg("run")
        .arg("wasmer/bash")
        .arg("--entrypoint=bash")
        .arg(format!("--volume={}:./some-dir/", temp.path().display()))
        .arg("--")
        .arg("-c")
        .arg("cat ./some-dir/message.txt")
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

#[test]
#[ignore = "`--use` (injecting packages into the filesystem) is not supported on the WASIX v2 kernel yet"]
fn merged_filesystem_contains_all_files() {
    let assert = wasmer_command()
        .arg("run")
        .arg("--use")
        .arg("wasmer/bash")
        .arg("python/python")
        .arg("--")
        .arg("-c")
        .arg("import this")
        .assert();

    assert
        .success()
        .stdout(contains("Beautiful is better than ugly."));
}

#[test]
fn run_a_wasi_executable() {
    let assert = wasmer_command()
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
    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::fib())
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success();
}

#[test]
fn error_if_no_start_function_found() {
    let assert = wasmer_command()
        .arg("run")
        .arg(fixtures::wat_no_start())
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert
        .failure()
        .stderr(contains("The module doesn't export a \"_start\" function"));
}

#[test]
#[cfg_attr(
    any(feature = "v8", target_os = "windows"),
    ignore = "wasmer may not have the 'compile' command with this backend"
)]
fn run_a_pre_compiled_wasm_file() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("qjs.wasmu");
    let qjs = fixtures::qjs();
    // Make sure it is compiled
    wasmer_command()
        .arg("compile")
        .arg("-o")
        .arg(&dest)
        .arg(&qjs)
        .assert()
        .success();
    assert!(dest.exists());

    // Now we can try to run the compiled artifact
    let assert = wasmer_command()
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
fn wasmer_run_some_directory() {
    let temp = TempDir::new().unwrap();
    std::fs::copy(fixtures::qjs(), temp.path().join("qjs.wasm")).unwrap();
    std::fs::copy(fixtures::qjs_wasmer_toml(), temp.path().join("wasmer.toml")).unwrap();

    let assert = wasmer_command()
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
fn run_quickjs_via_package_name() {
    let assert = wasmer_command()
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
fn run_quickjs_via_url() {
    let assert = wasmer_command()
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
#[ignore = "`--use` (injecting packages into the filesystem) is not supported on the WASIX v2 kernel yet"]
fn run_bash_using_coreutils() {
    let assert = wasmer_command()
        .arg("run")
        .arg("wasmer/bash")
        .arg("--entrypoint=bash")
        .arg("--use=wasmer/coreutils")
        .arg("--registry=wasmer.io")
        .arg("--")
        .arg("-c")
        .arg("ls /bin")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    // Note: the resulting filesystem should contain the main command as
    // well as the commands from all the --use packages

    assert
        .success()
        .stdout(contains("arch"))
        .stdout(contains("base32"))
        .stdout(contains("base64"))
        .stdout(contains("basenc"))
        .stdout(contains("basename"))
        .stdout(contains("bash"))
        .stdout(contains("cat"));
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

    let assert = wasmer_command()
        .arg("run")
        .arg(&js_script_dir)
        .arg("--registry=wasmer.io")
        .env("RUST_LOG", &*RUST_LOG)
        .assert();

    assert.success().stdout(contains("Hello, World!"));
}

// The test would be very slow on Windows and macOS
#[cfg_attr(any(target_os = "windows", target_os = "macos"), ignore)]
#[test]
#[ignore = "python/python is dynamically linked, which the WASIX v2 kernel does not support yet"]
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

    wasmer_command()
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

#[cfg(target_os = "linux")]
#[test]
fn shared_fd_closes_the_host_file_only_after_the_last_fd_is_closed() {
    let fixture = packages().join("shared-fd");
    let temp = TempDir::new().unwrap();
    let wasm = temp.path().join("main.wasm");
    let combined_log = temp.path().join("combined.log");
    let output_path = temp.path().join("output");

    compile_wasix_source(&fixture.join("main.c"), &wasm, false);

    let mut cmd = wasmer_command();
    cmd.arg("run")
        .arg(&wasm)
        .arg("--volume")
        .arg(".")
        .current_dir(temp.path());

    let (status, combined_output) = run_with_combined_output(&mut cmd, &combined_log);
    assert!(status.success(), "{combined_output}");

    // "parent 1" is written after the child closed both its descriptors: a host file closed
    // with the first descriptor instead of the last would have lost it. (The legacy runtime's
    // `virtual_fs` trace of the host close, which this test also checked, does not exist on
    // the WASIX v2 kernel.)
    let output = std::fs::read_to_string(&output_path).unwrap();
    for expected in ["parent 1", "parent 2", "child 1", "child 2"] {
        assert!(
            output.contains(expected),
            "missing `{expected}` in output file:\n{output}"
        );
    }
    let closing_marker = combined_output.find("closing last fd").unwrap();
    let closed_marker = combined_output.find("last fd closed").unwrap();
    assert!(closing_marker < closed_marker, "{combined_output}");
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

    let line = String::from_utf8(line).map_err(std::io::Error::other)?;
    Ok(line)
}

#[cfg(target_os = "linux")]
fn compile_wasix_source(source: &Path, output: &Path, use_eh: bool) {
    let output = Command::new("wasixcc")
        .arg(source)
        .arg("-o")
        .arg(output)
        .env("WASIXCC_WASM_EXCEPTIONS", if use_eh { "yes" } else { "no" })
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "failed to compile {}:\nstdout:\n{}\nstderr:\n{}",
        source.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_with_combined_output(cmd: &mut Command, log_path: &Path) -> (ExitStatus, String) {
    let stdout = File::create(log_path).unwrap();
    let stderr = stdout.try_clone().unwrap();
    let status = cmd
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()
        .unwrap();
    let output = std::fs::read_to_string(log_path).unwrap();
    (status, output)
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

fn random_port() -> u16 {
    rand::rng().random_range(10_000_u16..u16::MAX)
}
