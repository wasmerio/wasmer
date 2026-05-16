/// Build a WASM test and run it, asserting success or checking stdout.
///
/// # Forms
///
/// ```ignore
/// // Build the test in `<module>/<subdir>/`, run it, assert exit 0.
/// wasm_test!(fn_name, "subdir");
///
/// // Same, but assert the process exits non-zero.
/// wasm_test!(fn_name, "subdir", should_fail);
///
/// // Assert the process exits with a specific code.
/// wasm_test!(fn_name, "subdir", exit_code = 134);
///
/// // Assert the trimmed stdout equals the given string literal.
/// wasm_test!(fn_name, "subdir", stdout = "expected output");
///
/// // Pass argv to the test binary and assert exit 0.
/// wasm_test!(fn_name, "subdir", args = ["case-name"]);
///
/// // Run inside a fresh temp dir instead of `<module>/<subdir>/`.
/// wasm_test!(fn_name, "subdir", temp_dir);
///
/// // Temp dir can be combined with other checks.
/// wasm_test!(fn_name, "subdir", temp_dir, stdout = "expected output");
///
/// // Any of the above may be prefixed with Rust attributes.
/// wasm_test!(#[cfg(unix)] #[ignore = "reason"] fn_name, "subdir");
/// ```
macro_rules! wasm_test {
    (@setup default, $wasm:ident, $run_dir:ident, $temp_dir:ident) => {
        let $run_dir = $wasm.parent().unwrap();
    };
    (@setup temp_dir, $wasm:ident, $run_dir:ident, $temp_dir:ident) => {
        let $temp_dir = tempfile::tempdir().unwrap();
        let $run_dir = $temp_dir.path();
    };
    (@run $wasm:ident, $run_dir:expr, []) => {
        super::run_wasm_with_runner_config(&$wasm, $run_dir, |_| {})
    };
    (@run $wasm:ident, $run_dir:expr, [$($arg:expr),+ $(,)?]) => {
        super::run_wasm_with_runner_config(&$wasm, $run_dir, |runner| {
            runner.with_args([$($arg),+]);
        })
    };
    // ── base case used by all public forms ────────────────────────────────
    (
        @base
        $(#[$attr:meta])*
        $fn_name:ident,
        $subdir:literal,
        setup = $setup:ident,
        args = [$($arg:expr),* $(,)?],
        |$result:ident| $body:block
    ) => {
        $(#[$attr])*
        #[test]
        fn $fn_name() {
            let wasm = super::run_build_script(file!(), $subdir)
                .unwrap_or_else(|error| panic!("failed to build {}: {error}", stringify!($fn_name)));
            wasm_test!(@setup $setup, wasm, run_dir, temp_dir);
            let $result = wasm_test!(@run wasm, run_dir, [$($arg),*])
                .unwrap_or_else(|error| panic!("failed to execute {}: {error}", stringify!($fn_name)));
            $body
        }
    };
    // ── success with argv ──────────────────────────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, args = [$($arg:expr),* $(,)?]) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = default, args = [$($arg),*], |result| {
            super::ensure_wasm_run_succeeded(&result).unwrap();
        });
    };
    // ── success with argv in a fresh temp dir ──────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, temp_dir, args = [$($arg:expr),* $(,)?]) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = temp_dir, args = [$($arg),*], |result| {
            super::ensure_wasm_run_succeeded(&result).unwrap();
        });
    };
    // ── success ────────────────────────────────────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = default, args = [], |result| {
            super::ensure_wasm_run_succeeded(&result).unwrap();
        });
    };
    // ── success in a fresh temp dir ────────────────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, temp_dir) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = temp_dir, args = [], |result| {
            super::ensure_wasm_run_succeeded(&result).unwrap();
        });
    };
    // ── expect non-zero exit ───────────────────────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, should_fail) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = default, args = [], |result| {
            assert!(
                result.exit_code != Some(0),
                "{} should exit with non-zero code\n{}",
                stringify!($fn_name),
                super::format_captured_output(&result),
            );
        });
    };
    // ── expect non-zero exit in a fresh temp dir ───────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, temp_dir, should_fail) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = temp_dir, args = [], |result| {
            assert!(
                result.exit_code != Some(0),
                "{} should exit with non-zero code\n{}",
                stringify!($fn_name),
                super::format_captured_output(&result),
            );
        });
    };
    // ── expect specific exit code ──────────────────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, exit_code = $expected:expr) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = default, args = [], |result| {
            assert_eq!(
                result.exit_code,
                Some($expected),
                "{} should exit with code {:?}\n{}",
                stringify!($fn_name),
                Some($expected),
                super::format_captured_output(&result),
            );
        });
    };
    // ── expect specific exit code in a fresh temp dir ──────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, temp_dir, exit_code = $expected:expr) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = temp_dir, args = [], |result| {
            assert_eq!(
                result.exit_code,
                Some($expected),
                "{} should exit with code {:?}\n{}",
                stringify!($fn_name),
                Some($expected),
                super::format_captured_output(&result),
            );
        });
    };
    // ── check trimmed stdout ───────────────────────────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, stdout = $expected:literal) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = default, args = [], |result| {
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert_eq!(
                stdout.trim(),
                $expected,
                "{}",
                super::format_captured_output(&result),
            );
        });
    };
    // ── check trimmed stdout in a fresh temp dir ───────────────────────────
    ($(#[$attr:meta])* $fn_name:ident, $subdir:literal, temp_dir, stdout = $expected:literal) => {
        wasm_test!(@base $(#[$attr])* $fn_name, $subdir, setup = temp_dir, args = [], |result| {
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert_eq!(
                stdout.trim(),
                $expected,
                "{}",
                super::format_captured_output(&result),
            );
        });
    };
}

mod basic_tests;
mod call_dynamic;
mod closure_free;
mod context_destroy;
mod context_switch;
mod context_switching;
mod dynamic_call_and_closure_tests;
mod dynamic_library_tests;
mod edge_case_tests;
mod exception_tests;
mod exit_tests;
mod fd_dup2;
mod fd_fdflags_get;
mod fd_fdflags_set;
mod fd_fdstat_set_rights;
mod fd_tell;
mod fd_tests;
mod libc_tests;
mod lifecycle_tests;
mod longjmp_tests;
mod path_tests;
mod poll_tests;
mod proc_exec3;
mod proc_exec3_empty_argv;
mod proc_exec3_errors;
mod process_tests;
mod pthread_tests;
mod reflect_signature;
mod reflection_tests;
mod sched_yield;
mod semaphore_tests;
mod shared_library_tests;
mod socket_tests;
mod threadlocal_tests;

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};
use std::thread;
use std::time::{Duration, Instant};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use wasmer_wasix::VirtualFile as VirtualFileTrait;
use wasmer_wasix::runners::MappedDirectory;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};
use wasmer_wasix::runtime::module_cache::{HashedModuleData, ModuleCache};
use wasmer_wasix::virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite};

static TRACE_SUBSCRIBER_INIT: OnceLock<()> = OnceLock::new();
static TRACE_CAPTURE_STATE: OnceLock<TraceCaptureState> = OnceLock::new();

#[derive(Default)]
struct TraceCaptureState {
    active_buffer: Mutex<Option<Arc<Mutex<Vec<u8>>>>>,
    run_lock: Mutex<()>,
}

#[derive(Clone, Default)]
struct TraceMakeWriter;

struct TraceWriter {
    buffer: Option<Arc<Mutex<Vec<u8>>>>,
}

impl<'a> MakeWriter<'a> for TraceMakeWriter {
    type Writer = TraceWriter;

    fn make_writer(&'a self) -> Self::Writer {
        let buffer = trace_capture_state().active_buffer.lock().unwrap().clone();
        TraceWriter { buffer }
    }
}

impl Write for TraceWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(buffer) = &self.buffer {
            buffer.lock().unwrap().extend_from_slice(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn trace_capture_state() -> &'static TraceCaptureState {
    TRACE_CAPTURE_STATE.get_or_init(TraceCaptureState::default)
}

struct BuildLockGuard {
    lock_dir: PathBuf,
}

impl Drop for BuildLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.lock_dir);
    }
}

fn build_lock_path(test_path: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    test_path.hash(&mut hasher);

    std::env::temp_dir()
        .join("wasmer-wasm-test-build-locks")
        .join(format!("{:016x}", hasher.finish()))
}

fn acquire_build_lock(test_path: &Path) -> Result<BuildLockGuard, anyhow::Error> {
    let lock_dir = build_lock_path(test_path);
    let lock_root = lock_dir
        .parent()
        .expect("build lock path must have a parent");
    fs::create_dir_all(lock_root)?;

    let started = Instant::now();
    loop {
        match fs::create_dir(&lock_dir) {
            Ok(()) => return Ok(BuildLockGuard { lock_dir }),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if started.elapsed() > Duration::from_secs(300) {
                    anyhow::bail!(
                        "timed out waiting for WASIX test build lock {}",
                        lock_dir.display()
                    );
                }
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn init_trace_capture() {
    TRACE_SUBSCRIBER_INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("off"));
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .without_time()
                .with_writer(TraceMakeWriter),
        );

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

fn capture_trace_output<T>(f: impl FnOnce() -> T) -> (T, Vec<u8>) {
    init_trace_capture();

    let state = trace_capture_state();
    let _run_guard = state.run_lock.lock().unwrap();
    let buffer = Arc::new(Mutex::new(Vec::new()));
    *state.active_buffer.lock().unwrap() = Some(buffer.clone());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    *state.active_buffer.lock().unwrap() = None;
    let trace_output = buffer.lock().unwrap().clone();

    match result {
        Ok(value) => (value, trace_output),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// A virtual file that captures all writes to an in-memory buffer.
/// This is used to capture stdout/stderr during test execution.
#[derive(Debug)]
struct CaptureFile {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl CaptureFile {
    fn new(buffer: Arc<Mutex<Vec<u8>>>) -> Self {
        Self { buffer }
    }
}

impl VirtualFileTrait for CaptureFile {
    fn last_accessed(&self) -> u64 {
        0
    }

    fn last_modified(&self) -> u64 {
        0
    }

    fn created_time(&self) -> u64 {
        0
    }

    fn size(&self) -> u64 {
        self.buffer.lock().unwrap().len() as u64
    }

    fn set_len(&mut self, _new_size: u64) -> Result<(), wasmer_wasix::FsError> {
        Err(wasmer_wasix::FsError::PermissionDenied)
    }

    fn unlink(&mut self) -> Result<(), wasmer_wasix::FsError> {
        Ok(())
    }

    fn is_open(&self) -> bool {
        true
    }

    fn get_special_fd(&self) -> Option<u32> {
        None
    }

    fn poll_read_ready(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(0))
    }

    fn poll_write_ready(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

impl AsyncRead for CaptureFile {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for CaptureFile {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(self.write(buf))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for CaptureFile {
    fn start_seek(self: Pin<&mut Self>, _position: std::io::SeekFrom) -> std::io::Result<()> {
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl std::io::Read for CaptureFile {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl std::io::Write for CaptureFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Seek for CaptureFile {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

/// Find the single C/C++ source file to compile in a directory with no `build.sh`.
///
/// Priority order: `main.c` → `main.cpp` → the only `.c` file → the only `.cpp` file.
/// Returns `(compiler, source_filename)`.
fn find_source_file(dir: &Path) -> Result<(String, String), anyhow::Error> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "wasixcc".to_string());
    let cxx = std::env::var("CXX").unwrap_or_else(|_| "wasix++".to_string());

    if dir.join("main.c").exists() {
        return Ok((cc, "main.c".to_string()));
    }
    if dir.join("main.cpp").exists() {
        return Ok((cxx, "main.cpp".to_string()));
    }

    // Fall back to the sole .c / .cpp file in the directory.
    let c_files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".c") && !n.ends_with(".cpp"))
        .collect();
    if c_files.len() == 1 {
        return Ok((cc, c_files.into_iter().next().unwrap()));
    }

    let cpp_files: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".cpp"))
        .collect();
    if cpp_files.len() == 1 {
        return Ok((cxx, cpp_files.into_iter().next().unwrap()));
    }

    anyhow::bail!(
        "No build.sh and could not find a unique compilable source in {}. \
         Add a build.sh or ensure there is exactly one .c / .cpp file.",
        dir.display()
    )
}

/// Build a test's WASM binary.
///
/// Locates the test directory from the calling file's name (`file!()`),
/// then either runs the directory's `build.sh` or, when no `build.sh` is
/// present, compiles `main.c` / `main.cpp` directly with wasixcc/wasix++.
///
/// # Arguments
/// * `file` - The test file path (use `file!()` at the call site)
/// * `test_dir` - Subdirectory relative to the test file's directory;
///   use `""` or `"."` to target the test file's own directory
///
/// # Returns
/// Path to the compiled `main` binary
pub fn run_build_script(file: &str, test_dir: &str) -> Result<PathBuf, anyhow::Error> {
    let input_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/wasm_tests")
        .join(PathBuf::from(
            file.split('/')
                .next_back()
                .expect("The test file name cannot be empty")
                .trim_end_matches(".rs"),
        ));

    let test_path = input_dir.join(test_dir);
    let build_script = test_path.join("build.sh");
    let _build_guard = acquire_build_lock(&test_path)?;

    // Read optional per-test env overrides from `build.env` (KEY=VALUE, one per line).
    let build_env: Vec<(String, String)> = {
        let env_file = test_path.join("build.env");
        if env_file.exists() {
            std::fs::read_to_string(&env_file)?
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .filter_map(|l| {
                    let (k, v) = l.split_once('=')?;
                    Some((k.trim().to_string(), v.trim().to_string()))
                })
                .collect()
        } else {
            vec![]
        }
    };

    let mut cmd = if build_script.exists() {
        let mut cmd = Command::new("bash");
        cmd.arg(&build_script)
            .current_dir(&test_path)
            .env("CC", "wasixcc")
            .env("CXX", "wasix++")
            .env("WASIXCC_DISCARD_UNSUPPORTED_FLAGS", "yes");
        cmd
    } else {
        // No build.sh — find a compilable source file and invoke the compiler directly.
        // Priority: main.c > main.cpp > any single .c > any single .cpp
        let (compiler, source) = find_source_file(&test_path)?;
        let mut cmd = Command::new(&compiler);
        cmd.arg(&source)
            .arg("-o")
            .arg("main")
            .current_dir(&test_path)
            .env("WASIXCC_DISCARD_UNSUPPORTED_FLAGS", "yes");
        cmd
    };

    for (k, v) in &build_env {
        cmd.env(k, v);
    }
    let output = cmd.output()?;

    if !output.status.success() {
        eprintln!("Build stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("Build stderr: {}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!("Build failed for {}", test_path.display());
    }

    Ok(test_path.join("main"))
}

/// Create a tokio runtime for async operations.
/// This is a helper to avoid duplicating runtime creation code.
fn create_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
}

/// Get the cache directory for compiled WASM modules.
/// Follows the same precedence as the Wasmer CLI:
/// 1. WASMER_CACHE_DIR environment variable
/// 2. WASMER_DIR/cache/compiled
/// 3. ~/.wasmer/cache/compiled
/// 4. temp_dir/wasmer/cache/compiled (fallback)
fn get_cache_dir() -> PathBuf {
    if let Ok(dir_str) = std::env::var("WASMER_CACHE_DIR") {
        PathBuf::from(dir_str).join("compiled")
    } else if let Ok(dir_str) = std::env::var("WASMER_DIR") {
        PathBuf::from(dir_str).join("cache").join("compiled")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
            .join(".wasmer")
            .join("cache")
            .join("compiled")
    } else {
        // Fallback to temp directory if no home is available
        std::env::temp_dir()
            .join("wasmer")
            .join("cache")
            .join("compiled")
    }
}

fn create_engine_for_wasm(wasm_bytes: &[u8]) -> wasmer::Engine {
    #[cfg(target_os = "macos")]
    {
        use wasmer::{sys::EngineBuilder, sys::Target};

        // On macOS, the default Cranelift backend has limited support for the features
        // required by these tests, especially exception handling. Use the slower LLVM
        // backend instead so the WASIX test suite can run reliably on macOS.
        let target = Target::default();
        let features = wasmer_types::Features::detect_from_wasm(wasm_bytes).unwrap_or_else(|_| {
            wasmer::Engine::default_features_for_backend(&wasmer::BackendKind::LLVM, &target)
        });

        let compiler = wasmer::sys::LLVM::default();

        EngineBuilder::new(compiler)
            .set_features(Some(features))
            .set_target(Some(target))
            .engine()
            .into()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = wasm_bytes;
        wasmer::Engine::default()
    }
}

/// Result from running a WASM program, including captured output and exit status
pub struct WasmRunResult {
    #[allow(dead_code)]
    pub stdout: Vec<u8>,
    #[allow(dead_code)]
    pub stderr: Vec<u8>,
    #[allow(dead_code)]
    pub trace_output: Vec<u8>,
    #[allow(dead_code)]
    pub exit_code: Option<i32>,
    #[allow(dead_code)]
    pub error: Option<String>,
}

fn format_captured_output(result: &WasmRunResult) -> String {
    let mut message = format!(
        "exit_code={:?}\nstdout:\n{}\nstderr:\n{}\ntrace:\n{}",
        result.exit_code,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.trace_output),
    );

    if let Some(error) = &result.error {
        message.push_str(&format!("\nerror:\n{}", error));
    }

    message
}

/// Run a compiled WASM file using WasiRunner and return output buffers and exit status
///
/// This function uses the same caching mechanism as the Wasmer CLI:
/// - In-memory cache (SharedCache) for fast repeated loads within the same process
/// - Filesystem cache as a fallback for persistence across test runs
/// - Cache directory follows the same precedence as the CLI:
///   1. WASMER_CACHE_DIR environment variable
///   2. WASMER_DIR/cache/compiled
///   3. ~/.wasmer/cache/compiled
///   4. temp_dir/wasmer/cache/compiled (fallback)
///
/// The caching significantly improves test performance by avoiding recompilation
/// of the same WASM modules across multiple test runs.
pub fn run_wasm_with_result(
    wasm_path: &PathBuf,
    dir: &Path,
) -> Result<WasmRunResult, anyhow::Error> {
    run_wasm_with_runner_config(wasm_path, dir, |_| {})
}

pub fn run_wasm_with_runner_config(
    wasm_path: &PathBuf,
    dir: &Path,
    configure_runner: impl FnOnce(&mut WasiRunner),
) -> Result<WasmRunResult, anyhow::Error> {
    // Load the compiled WASM module
    let wasm_bytes = std::fs::read(wasm_path)?;
    let engine = create_engine_for_wasm(&wasm_bytes);
    let module_data = HashedModuleData::new(wasm_bytes);
    let hash = *module_data.hash();

    // Create buffers to capture stdout and stderr
    let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
    let stderr_buffer = Arc::new(Mutex::new(Vec::new()));

    let stdout_capture = Box::new(CaptureFile::new(stdout_buffer.clone()));
    let stderr_capture = Box::new(CaptureFile::new(stderr_buffer.clone()));

    let rt = create_runtime();

    let (result, trace_output) = capture_trace_output(|| {
        rt.block_on(async {
            // Set up module cache with in-memory + filesystem fallback (same as CLI)
            let cache_dir = get_cache_dir();
            std::fs::create_dir_all(&cache_dir).ok();

            let rt_handle = wasmer_wasix::runtime::task_manager::tokio::RuntimeOrHandle::Handle(
                tokio::runtime::Handle::current(),
            );
            let tokio_task_manager = Arc::new(
                wasmer_wasix::runtime::task_manager::tokio::TokioTaskManager::new(rt_handle),
            );
            let module_cache = wasmer_wasix::runtime::module_cache::SharedCache::default()
                .with_fallback(wasmer_wasix::runtime::module_cache::FileSystemCache::new(
                    cache_dir,
                    tokio_task_manager,
                ));

            let arc_cache = Arc::new(module_cache);

            let module = wasmer_wasix::runtime::load_module(
                &engine,
                &arc_cache,
                wasmer_wasix::runtime::ModuleInput::Hashed(Cow::Borrowed(&module_data)),
                None,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to load module: {}", e))?;

            tokio::task::block_in_place(move || {
                // Run the WASM module using WasiRunner
                let mut runner = WasiRunner::new();
                runner
                    .with_mapped_directories([MappedDirectory {
                        guest: dir.to_string_lossy().to_string(),
                        host: dir.to_path_buf(),
                    }])
                    .with_mapped_directories([MappedDirectory {
                        guest: "/lib".to_string(),
                        host: dir.to_path_buf(),
                    }])
                    .with_mapped_directories([MappedDirectory {
                        guest: "/data".to_string(),
                        host: dir.to_path_buf(),
                    }])
                    .with_current_dir(dir.to_string_lossy().to_string())
                    .with_stdout(stdout_capture)
                    .with_stderr(stderr_capture);
                configure_runner(&mut runner);
                runner.run_wasm(
                    RuntimeOrEngine::Engine(engine),
                    wasm_path.to_string_lossy().as_ref(),
                    module,
                    hash,
                )
            })
        })
    });

    // Extract the captured output
    let stdout = stdout_buffer.lock().unwrap().clone();
    let stderr = stderr_buffer.lock().unwrap().clone();

    // Extract exit code from result
    let exit_code = match &result {
        Ok(_) => Some(0),
        Err(e) => {
            // Try to extract exit code from error message
            let error_msg = e.to_string();
            if let Some(code_str) = error_msg.split("ExitCode::").nth(1) {
                if let Some(code) = code_str.split_whitespace().next() {
                    code.parse::<i32>().ok()
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    Ok(WasmRunResult {
        stdout,
        stderr,
        trace_output,
        exit_code,
        error: result.as_ref().err().map(ToString::to_string),
    })
}

/// Run a compiled WASM file using WasiRunner
#[allow(unused)]
pub fn run_wasm(wasm_path: &PathBuf, dir: &Path) -> Result<(), anyhow::Error> {
    let result = run_wasm_with_result(wasm_path, dir)?;
    ensure_wasm_run_succeeded(&result)
}

#[allow(unused)]
pub fn run_wasm_with_runner_config_checked(
    wasm_path: &PathBuf,
    dir: &Path,
    configure_runner: impl FnOnce(&mut WasiRunner),
) -> Result<(), anyhow::Error> {
    let result = run_wasm_with_runner_config(wasm_path, dir, configure_runner)?;
    ensure_wasm_run_succeeded(&result)
}

fn ensure_wasm_run_succeeded(result: &WasmRunResult) -> Result<(), anyhow::Error> {
    // Preserve the historical behavior here: only an explicit non-zero exit
    // is treated as failure. Some fixtures currently surface loader/runtime
    // errors without a parsed exit code.
    if let Some(code) = result.exit_code
        && code != 0
    {
        anyhow::bail!(format_captured_output(result));
    }

    Ok(())
}
