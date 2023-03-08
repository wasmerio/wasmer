use assert_cmd::{assert::Assert, prelude::OutputAssertExt, Command};
use tempfile::TempDir;
use wasmer_integration_tests_cli::get_wasmer_path;

fn wasmer_cli() -> Command {
    Command::new(get_wasmer_path())
}

mod webc_on_disk {
    use super::*;

    #[test]
    fn wasi_runner() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::python())
            .arg("--")
            .arg("--version")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    fn wasi_runner_with_mounted_directories() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::python())
            .arg("--mapdir")
            .arg(format!("/app:{}", temp.path().display()))
            .arg("--")
            .arg("/app/main.py")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    fn wasi_runner_with_env_vars() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("main.py"), "print('Hello, World!')").unwrap();

        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::python())
            .arg("--env")
            .arg("SOME_VAR=Hello, World!")
            .arg("--")
            .arg("-c")
            .arg("import os; print(os.environ['SOME_VAR'])")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    fn wcgi_runner() {
        // Start the WCGI server in the background
        let child = std::process::Command::new(get_wasmer_path())
            .arg("run2")
            .arg(fixtures::static_server())
            .spawn()
            .map(Child::new)
            .unwrap();

        let assert = child.join();

        assert.stdout("Hello, World!");
    }
}

mod wasm_on_disk {
    use super::*;

    #[test]
    fn wasi_executable() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::qjs())
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    fn no_abi() {
        let assert = wasmer_cli().arg("run2").arg(fixtures::fib()).assert();

        assert.success();
    }

    #[test]
    fn error_if_no_start_function_found() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg(fixtures::wat_no_start())
            .assert();

        assert
            .failure()
            .stderr("Can not find any export functions.");
    }
}

#[test]
fn wasmer_package_directory() {
    let temp = TempDir::new().unwrap();
    std::fs::copy(fixtures::qjs(), temp.path().join("qjs.wasm")).unwrap();
    std::fs::copy(fixtures::qjs_wasmer_toml(), temp.path().join("wasmer.toml")).unwrap();

    let assert = wasmer_cli()
        .arg("run2")
        .arg(temp.path())
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .assert();

    assert.success().stdout("Hello, World!");
}

#[test]
fn pre_compiled_wasm() {
    let temp = TempDir::new().unwrap();
    let dest = temp.path().join("qjs.wasmu");
    let qjs = fixtures::qjs();
    // Make sure it is compiled
    wasmer_cli()
        .arg("compile")
        .arg("-o")
        .arg(&dest)
        .arg(&qjs)
        .assert()
        .success();
    assert!(dest.exists());

    // Now we can try to run the compiled artifact
    let assert = wasmer_cli()
        .arg("run2")
        .arg(&dest)
        .arg("--")
        .arg("--eval")
        .arg("console.log('Hello, World!')")
        .assert();

    assert.success().stdout("Hello, World!");
}

mod remote_webc {
    use super::*;

    #[test]
    fn quickjs_as_package_name() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg("saghul/quickjs")
            .arg("--registry=https://wapm.io/")
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout("Hello, World!");
    }

    #[test]
    fn quickjs_as_url() {
        let assert = wasmer_cli()
            .arg("run2")
            .arg("https://wapm.io/saghul/quickjs")
            .arg("--")
            .arg("--eval")
            .arg("console.log('Hello, World!')")
            .assert();

        assert.success().stdout("Hello, World!");
    }
}

mod fixtures {
    use std::path::{Path, PathBuf};

    use wasmer_integration_tests_cli::{ASSET_PATH, C_ASSET_PATH};

    /// A WEBC file containing the Python interpreter, compiled to WASI.
    pub fn python() -> PathBuf {
        Path::new(C_ASSET_PATH).join("python-0.1.0.wasmer")
    }

    /// A WEBC file containing the WCGI static server.
    pub fn static_server() -> PathBuf {
        Path::new(C_ASSET_PATH).join("staticserver.webc")
    }

    /// The QuickJS interpreter, compiled to a WASI module.
    pub fn qjs() -> PathBuf {
        Path::new(C_ASSET_PATH).join("qjs.wasm")
    }

    /// The `wasmer.toml` file for QuickJS.
    pub fn qjs_wasmer_toml() -> PathBuf {
        Path::new(C_ASSET_PATH).join("qjs-wasmer.toml")
    }

    /// An executable which calculates fib(40) and exits with no output.
    pub fn fib() -> PathBuf {
        Path::new(ASSET_PATH).join("fib.wat")
    }

    pub fn wat_no_start() -> PathBuf {
        Path::new(ASSET_PATH).join("no_start.wat")
    }
}

/// A helper that wraps [`std::process::Child`] to make sure it gets terminated
/// when it is no longer needed.
struct Child(Option<std::process::Child>);

impl Child {
    fn new(child: std::process::Child) -> Self {
        Child(Some(child))
    }

    fn join(mut self) -> Assert {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait_with_output().unwrap().assert()
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
        }
    }
}
