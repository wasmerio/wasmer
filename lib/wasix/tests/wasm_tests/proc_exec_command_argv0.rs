use std::sync::Arc;

use tempfile::TempDir;
use wasmer_wasix::{
    PluggableRuntime, Runtime,
    bin_factory::BinaryPackage,
    runners::wasi::{RuntimeOrEngine, WasiRunner},
    runtime::{
        module_cache::{FileSystemCache, ModuleCache, SharedCache},
        package_loader::BuiltinPackageLoader,
        task_manager::tokio::TokioTaskManager,
    },
};

use super::run_build_script;

/// Verify that a process re-exec'd via argv[0] does not inherit the command's
/// main_args a second time. The C program re-execs itself with a "child"
/// marker; in child mode it asserts argc == 2 (only argv[0] and the marker).
/// Before the fix, the command's main_args would be re-injected, causing
/// argc > 2 and a test failure.
#[cfg_attr(
    not(feature = "sys-thread"),
    ignore = "The tokio task manager isn't available on this platform"
)]
#[tokio::test(flavor = "multi_thread")]
async fn test_proc_exec_command_argv0() {
    let wasm = run_build_script(file!(), ".").unwrap();
    let wasm_bytes = std::fs::read(&wasm).unwrap();

    // Create a temp dir with a wasmer.toml that has:
    //   - atom "inner" (the compiled wasm)
    //   - command "outer" using atom "inner" with extra main_args
    // The command name and atom name differ on purpose so we can verify that
    // argv[0] is set to the atom name (not the command name) after the fix.
    let temp = TempDir::new().unwrap();
    let wasmer_toml = r#"
[package]
name = "test/command-argv0"
version = "0.0.0"
description = "test package"

[[module]]
name = "inner"
source = "inner.wasm"
abi = "wasi"

[[command]]
name = "outer"
module = "inner"
main_args = "--extra-arg"
"#;
    std::fs::write(temp.path().join("wasmer.toml"), wasmer_toml).unwrap();
    std::fs::write(temp.path().join("inner.wasm"), &wasm_bytes).unwrap();

    let tasks = Arc::new(TokioTaskManager::new(tokio::runtime::Handle::current()));
    let mut rt = PluggableRuntime::new(Arc::clone(&tasks) as Arc<_>);
    let cache = SharedCache::default().with_fallback(FileSystemCache::new(
        std::env::temp_dir().join("wasmer-test-command-argv0"),
        tasks.clone(),
    ));
    rt.set_module_cache(cache)
        .set_package_loader(BuiltinPackageLoader::new());

    let pkg = BinaryPackage::from_dir(temp.path(), &rt).await.unwrap();
    let rt: Arc<dyn Runtime + Send + Sync> = Arc::new(rt);

    let result = std::thread::spawn(move || {
        let _guard = tasks.runtime_handle().enter();
        WasiRunner::new().run_command("outer", &pkg, RuntimeOrEngine::Runtime(Arc::clone(&rt)))
    })
    .join()
    .unwrap();

    result.unwrap();
}
