use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use wasmer_wasix::VirtualFile as VirtualFileTrait;
use wasmer_wasix::runners::MappedDirectory;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};
use wasmer_wasix::runtime::module_cache::{HashedModuleData, ModuleCache};
use wasmer_wasix::virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite};

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

fn find_compatible_sysroot() -> Result<String, anyhow::Error> {
    if let Ok(sysroot) = std::env::var("WASIXCC_SYSROOT") {
        if !Path::new(&sysroot).exists() {
            anyhow::bail!("WASIXCC_SYSROOT is set but does not exist: {}", sysroot);
        }
        return Ok(sysroot);
    }

    if let Ok(sysroot) = std::env::var("WASIXCC_PYTHON_SYSROOT") {
        if !Path::new(&sysroot).exists() {
            anyhow::bail!(
                "WASIXCC_PYTHON_SYSROOT is set but does not exist: {}",
                sysroot
            );
        }
        return Ok(sysroot);
    }

    // Try to find a build-scripts style sysroot in common locations
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());

    // A wasix-clang sysroot should
    let sysroot = format!("{}/.wasix-clang/wasix-sysroot", home);
    if Path::new(&sysroot).exists() {
        return Ok(sysroot);
    }

    let sysroot = format!("{}/.build-scripts/pkgs", home);
    if Path::new(&sysroot).exists() {
        return Ok(sysroot);
    }

    anyhow::bail!(
        "Could not find a sysroot compatible with the wasix tests. Use the following command to download a compatible sysroot from build-scripts into the correct location:\ncurl -sSfL https://raw.githubusercontent.com/wasix-org/build-scripts/refs/heads/main/assemble-pkgs.sh | bash -s -- -i wasix-libc -i libcxx -i compiler-rt -i libffi -o ~/.build-scripts/pkgs"
    );
}

/// Run a build.sh script for a test directory.
///
/// This function locates the test directory based on the test file path,
/// runs the build.sh script within that directory using wasixcc/wasix++,
/// and returns the path to the compiled WASM binary.
///
/// # Arguments
/// * `file` - The test file path (typically `file!()`)
/// * `test_dir` - The test directory name relative to the test file's directory
///
/// # Returns
/// The path to the compiled `main` binary
pub fn run_build_script(file: &str, test_dir: &str) -> Result<PathBuf, anyhow::Error> {
    let input_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(PathBuf::from(
            file.split('/')
                .next_back()
                .expect("The test file name cannot be empty")
                .trim_end_matches(".rs"),
        ));

    let test_path = input_dir.join(test_dir);
    let build_script = test_path.join("build.sh");

    // Use wasixcc environment variables if available, otherwise use defaults
    let sysroot = find_compatible_sysroot()?;

    let compiler_flags = std::env::var("WASIXCC_COMPILER_FLAGS")
        .unwrap_or_else(|_| format!(
            "-fPIC:-fwasm-exceptions:-Wl,-L{}/usr/local/lib/wasm32-wasi:-I{}/usr/local/include:-Wl,-mllvm,--wasm-enable-eh:-Wl,-mllvm,--wasm-enable-sjlj:-Wl,-mllvm,--wasm-use-legacy-eh=false:-Wl,-mllvm,--exception-model=wasm:-iwithsysroot:/usr/local/include/c++/v1",
            sysroot, sysroot
        ));

    let output = Command::new("bash")
        .arg(&build_script)
        .current_dir(&test_path)
        .env("CC", "wasixcc")
        .env("CXX", "wasix++")
        .env("WASIXCC_SYSROOT", &sysroot)
        .env("WASIXCC_COMPILER_FLAGS", &compiler_flags)
        .output()?;

    if !output.status.success() {
        eprintln!("Build stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("Build stderr: {}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!("Build script failed");
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

/// Result from running a WASM program, including captured output and exit status
pub struct WasmRunResult {
    #[allow(dead_code)]
    pub stdout: Vec<u8>,
    #[allow(dead_code)]
    pub stderr: Vec<u8>,
    #[allow(dead_code)]
    pub exit_code: Option<i32>,
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
    // Load the compiled WASM module
    let wasm_bytes = std::fs::read(wasm_path)?;
    let module_data = HashedModuleData::new(wasm_bytes);
    let hash = *module_data.hash();
    let engine = wasmer::Engine::default();

    // Create buffers to capture stdout and stderr
    let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
    let stderr_buffer = Arc::new(Mutex::new(Vec::new()));

    let stdout_capture = Box::new(CaptureFile::new(stdout_buffer.clone()));
    let stderr_capture = Box::new(CaptureFile::new(stderr_buffer.clone()));

    let rt = create_runtime();

    let result = rt.block_on(async {
        // Set up module cache with in-memory + filesystem fallback (same as CLI)
        let cache_dir = get_cache_dir();
        std::fs::create_dir_all(&cache_dir).ok();

        let rt_handle = wasmer_wasix::runtime::task_manager::tokio::RuntimeOrHandle::Handle(
            tokio::runtime::Handle::current(),
        );
        let tokio_task_manager =
            Arc::new(wasmer_wasix::runtime::task_manager::tokio::TokioTaskManager::new(rt_handle));
        let module_cache = wasmer_wasix::runtime::module_cache::SharedCache::default()
            .with_fallback(wasmer_wasix::runtime::module_cache::FileSystemCache::new(
                cache_dir,
                tokio_task_manager,
            ));

        let module = wasmer_wasix::runtime::load_module(&engine, &module_cache, &module_data)
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
                .with_current_dir(dir.to_string_lossy().to_string())
                .with_stdout(stdout_capture)
                .with_stderr(stderr_capture);
            runner.run_wasm(
                RuntimeOrEngine::Engine(engine),
                wasm_path.to_string_lossy().as_ref(),
                module,
                hash,
            )
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
        exit_code,
    })
}

/// Run a compiled WASM file using WasiRunner
pub fn run_wasm(wasm_path: &PathBuf, dir: &Path) -> Result<(), anyhow::Error> {
    let result = run_wasm_with_result(wasm_path, dir)?;

    // If exit code is non-zero, return an error
    if let Some(code) = result.exit_code
        && code != 0
    {
        anyhow::bail!("WASI exited with code: {}", code);
    }

    Ok(())
}
