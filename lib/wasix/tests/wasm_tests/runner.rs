use std::borrow::Cow;
use std::io::{self, Write};
use std::num::NonZero;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use wasmer_wasix::PluggableRuntime;
use wasmer_wasix::VirtualFile as VirtualFileTrait;
use wasmer_wasix::runners::MappedDirectory;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};
use wasmer_wasix::runtime::module_cache::{HashedModuleData, ModuleCache};
use wasmer_wasix::virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite};

use crate::Engine;
use crate::error::exit_code_from_error;

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

fn init_trace_capture() {
    TRACE_SUBSCRIBER_INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new("off"));
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .without_time()
                .with_writer(TraceMakeWriter),
        );

        if std::env::var("WASIX_TRACE_TO_STDERR").is_ok() {
            let subscriber = subscriber.with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_thread_ids(true)
                    .with_writer(std::io::stderr),
            );
            let _ = tracing::subscriber::set_global_default(subscriber);
        } else {
            let _ = tracing::subscriber::set_global_default(subscriber);
        }
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

fn create_engine_for_wasm(wasm_bytes: &[u8], engine: Engine) -> wasmer::Engine {
    use wasmer::{sys::EngineBuilder, sys::Target};

    let target = Target::default();
    let backend = match engine {
        Engine::Cranelift => wasmer::BackendKind::Cranelift,
        #[cfg(feature = "llvm")]
        Engine::LLVM => wasmer::BackendKind::LLVM,
        #[cfg(feature = "v8")]
        Engine::V8 => wasmer::BackendKind::V8,
    };
    let features = wasmer_types::Features::detect_from_wasm(wasm_bytes)
        .unwrap_or_else(|_| wasmer::Engine::default_features_for_backend(&backend, &target));

    // We're going to run many parallel tests and so we use just a single thread for compilation.
    let engine = match engine {
        Engine::Cranelift => {
            let mut config = wasmer::sys::Cranelift::default();
            config.num_threads(NonZero::new(1).unwrap());
            EngineBuilder::new(config)
        }
        #[cfg(feature = "llvm")]
        Engine::LLVM => {
            let mut config = wasmer::sys::LLVM::default();
            config.num_threads(NonZero::new(1).unwrap());
            EngineBuilder::new(config)
        }
        #[cfg(feature = "v8")]
        Engine::V8 => return wasmer::v8::engine::Engine::new().into(),
    };
    engine
        .set_features(Some(features))
        .set_target(Some(target))
        .engine()
        .into()
}

/// Result from running a WASM program, including captured output and exit status
pub(crate) struct WasmRunResult {
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) trace_output: Vec<u8>,
    pub(crate) exit_code: i32,
    pub(crate) error: Option<String>,
}

pub(crate) fn run_wasm_with_runner_config(
    wasm_path: &PathBuf,
    dir: &Path,
    compiler: Engine,
    program_name: Option<&str>,
    include_default_mounts: bool,
    configure_runner: impl FnOnce(&mut WasiRunner) -> Result<(), anyhow::Error>,
) -> Result<WasmRunResult, anyhow::Error> {
    run_wasm_with_runner_config_inner(
        wasm_path,
        dir,
        compiler,
        program_name,
        include_default_mounts,
        configure_runner,
        None::<fn(&mut PluggableRuntime) -> Result<(), anyhow::Error>>,
    )
}

pub(crate) fn run_wasm_with_runner_and_runtime_config(
    wasm_path: &PathBuf,
    dir: &Path,
    compiler: Engine,
    program_name: Option<&str>,
    include_default_mounts: bool,
    configure_runner: impl FnOnce(&mut WasiRunner) -> Result<(), anyhow::Error>,
    configure_runtime: impl FnOnce(&mut PluggableRuntime) -> Result<(), anyhow::Error>,
) -> Result<WasmRunResult, anyhow::Error> {
    run_wasm_with_runner_config_inner(
        wasm_path,
        dir,
        compiler,
        program_name,
        include_default_mounts,
        configure_runner,
        Some(configure_runtime),
    )
}

fn run_wasm_with_runner_config_inner<ConfigureRunner, ConfigureRuntime>(
    wasm_path: &PathBuf,
    dir: &Path,
    compiler: Engine,
    program_name: Option<&str>,
    include_default_mounts: bool,
    configure_runner: ConfigureRunner,
    configure_runtime: Option<ConfigureRuntime>,
) -> Result<WasmRunResult, anyhow::Error>
where
    ConfigureRunner: FnOnce(&mut WasiRunner) -> Result<(), anyhow::Error>,
    ConfigureRuntime: FnOnce(&mut PluggableRuntime) -> Result<(), anyhow::Error>,
{
    // Load the compiled WASM module
    let wasm_bytes = std::fs::read(wasm_path)?;
    let engine = create_engine_for_wasm(&wasm_bytes, compiler);
    let module_data = HashedModuleData::new(wasm_bytes);
    let hash = *module_data.hash();
    let program_name = program_name
        .map(str::to_owned)
        .unwrap_or_else(|| wasm_path.to_string_lossy().to_string());

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
            std::fs::create_dir_all(&cache_dir).expect("failed to create cache directory");

            let rt_handle = wasmer_wasix::runtime::task_manager::tokio::RuntimeOrHandle::Handle(
                tokio::runtime::Handle::current(),
            );
            let tokio_task_manager = Arc::new(
                wasmer_wasix::runtime::task_manager::tokio::TokioTaskManager::new(rt_handle),
            );
            let module_cache = wasmer_wasix::runtime::module_cache::SharedCache::default()
                .with_fallback(wasmer_wasix::runtime::module_cache::FileSystemCache::new(
                    cache_dir,
                    tokio_task_manager.clone(),
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
                if include_default_mounts {
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
                        .with_current_dir(dir.to_string_lossy().to_string());
                }
                runner
                    .with_stdout(stdout_capture)
                    .with_stderr(stderr_capture);
                configure_runner(&mut runner)?;
                let runtime_or_engine = match configure_runtime {
                    Some(configure_runtime) => {
                        let mut runtime = PluggableRuntime::new(tokio_task_manager);
                        runtime.set_engine(engine.clone());
                        configure_runtime(&mut runtime)?;
                        RuntimeOrEngine::Runtime(Arc::new(runtime))
                    }
                    None => RuntimeOrEngine::Engine(engine),
                };
                runner.run_wasm(runtime_or_engine, &program_name, module, hash)
            })
        })
    });

    // Extract the captured output
    let stdout = stdout_buffer.lock().unwrap().clone();
    let stderr = stderr_buffer.lock().unwrap().clone();

    let error = result.as_ref().err().map(ToString::to_string);
    // Extract exit code from result
    let exit_code = match result {
        Ok(_) => 0,
        Err(e) => exit_code_from_error(&e).ok_or(e)?,
    };

    Ok(WasmRunResult {
        stdout,
        stderr,
        trace_output,
        exit_code,
        error,
    })
}

pub(crate) fn format_captured_output(result: &WasmRunResult) -> String {
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
