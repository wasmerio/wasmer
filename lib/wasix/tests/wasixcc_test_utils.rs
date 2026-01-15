use std::fmt::Debug;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use wasmer::Module;
use wasmer_wasix::VirtualFile as VirtualFileTrait;
use wasmer_wasix::runners::MappedDirectory;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};
use wasmer_wasix::runtime::module_cache::HashedModuleData;
use wasmer_wasix::virtual_fs::{AsyncRead, AsyncSeek, AsyncWrite};

/// A virtual file that captures all writes to an in-memory buffer
#[derive(Debug)]
struct CaptureFile<B: Write + Send + Debug + Unpin + 'static> {
    buffer: Arc<Mutex<Vec<u8>>>,
    file: Option<B>,
}

impl<B: Write + Send + Debug + Unpin + 'static> CaptureFile<B> {
    fn new(buffer: Arc<Mutex<Vec<u8>>>, file: Option<B>) -> Self {
        Self { buffer, file: file }
    }
}

impl<B: Write + Send + Debug + Unpin + 'static> VirtualFileTrait for CaptureFile<B> {
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

impl<B: Write + Send + Debug + Unpin + 'static> AsyncRead for CaptureFile<B> {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<B: Write + Send + Debug + Unpin + 'static> AsyncWrite for CaptureFile<B> {
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

impl<B: Write + Send + Debug + Unpin + 'static> AsyncSeek for CaptureFile<B> {
    fn start_seek(self: Pin<&mut Self>, _position: std::io::SeekFrom) -> std::io::Result<()> {
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Poll::Ready(Ok(0))
    }
}

impl<B: Write + Send + Debug + Unpin + 'static> std::io::Read for CaptureFile<B> {
    fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl<B: Write + Send + Debug + Unpin + 'static> std::io::Write for CaptureFile<B> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(output) = &mut self.file {
            output.write(buf)?;
            output.flush()?;
        }
        let mut buffer = self.buffer.lock().unwrap();
        buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<B: Write + Send + Debug + Unpin + 'static> std::io::Seek for CaptureFile<B> {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

/// A utility struct for testing wasixcc-compiled programs.
///
/// The main reason for having this utility is to simplify the process of compiling
/// C/C++ source files to WASM using wasixcc and running them in a WASI environment.
///
/// It provides methods to compile source files with specific flags and run the resulting
/// WASM modules, handling the necessary setup and teardown.
///
/// The main reason why this is a struct is to have some state for the input and output directories.
pub struct WasixccTest {
    input_dir: PathBuf,
    output_dir: PathBuf,
    test_name: String,
    default_source: PathBuf,
    default_executable: PathBuf,
}

impl WasixccTest {
    /// Create a new WasixccTest instance for a given test file and test name.
    ///
    /// ### Example
    /// ```no_run
    /// let test = WasixccTest::new(file!(), "simple_test");
    /// test.compile().unwrap();
    /// test.run().unwrap();
    /// ```
    pub fn new(test_file: &str, test_name: &str) -> Self {
        let input_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(PathBuf::from(
                test_file
                    .split('/')
                    .next_back()
                    .expect("The test file name can not be empty")
                    .trim_end_matches(".rs"),
            ));
        assert!(input_dir.exists());
        let test_name = test_name.split('.').next().unwrap();
        let output_dir = input_dir.join(format!("{test_name}.out"));
        std::fs::create_dir_all(&output_dir).unwrap();
        Self {
            default_source: input_dir.join(format!("{test_name}.c")),
            default_executable: output_dir.join(test_name),
            input_dir,
            output_dir,
            test_name: test_name.to_string(),
        }
    }

    /// Lowlevel function to invoke wasixcc with custom arguments and environment variables.
    fn wasixcc(
        &self,
        sources: &[&str],
        output_file: &Path,
        extra_args: &[&str],
        extra_env: &[(&str, &str)],
        cpp: bool,
    ) -> Result<(), anyhow::Error> {
        // Derive the test directory from the test file path
        let tool = if cpp { "wasix++" } else { "wasixcc" };

        // Check if the tool is available in PATH
        if Command::new(tool).arg("--version").output().is_err() {
            anyhow::bail!(
                "{} not found in PATH. Please install it before running these tests.",
                tool
            );
        }

        // Compile with wasixcc
        let mut command = Command::new(tool);
        for source in sources {
            let source_path = self.input_dir.join(source);
            command.arg(source_path);
        }
        command
            .arg("-o")
            .arg(&output_file)
            .current_dir(&self.input_dir);

        // Add any extra arguments
        for arg in extra_args {
            command.arg(arg);
        }
        for (key, value) in extra_env {
            command.env(key, value);
        }

        eprintln!("Running wasixcc: {:?}", command);
        let output = command
            .output()
            .expect(format!("Failed to run {tool}").as_str());

        if !output.status.success() {
            eprintln!(
                "wasixcc stdout: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            eprintln!(
                "wasixcc stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            anyhow::bail!("wasixcc compilation failed");
        }

        assert!(
            output_file.exists(),
            "Expected output file does not exist after compilation"
        );

        Ok(())
    }

    /// Compiles the default C source file with wasixcc and wasm-exceptions enabled and returns the path to the WASM file.
    ///
    /// If you need more control about the file use `compile_executable()` instead.
    pub fn compile(&self) -> Result<PathBuf, anyhow::Error> {
        self.compile_executable(&self.default_source, &[])
    }

    /// Compiles a C file with wasixcc and wasm-exceptions enabled and returns the path to the WASM file.
    ///
    /// If you need more control about the compiler flags use `wasixcc()` instead.
    #[allow(dead_code)]
    pub fn compile_executable(
        &self,
        source_file: &Path,
        extra_flags: &[&str],
    ) -> Result<PathBuf, anyhow::Error> {
        let main_c = self.input_dir.join(source_file);

        let cpp = source_file
            .extension()
            .map_or(false, |ext| ext == "cpp" || ext == "cc");

        let link_dir_flag = format!("-L{}", self.output_dir.display());
        let mut base_flags = vec![
            "-fwasm-exceptions",
            "-Wl,--pie",
            "-fPIC",
            link_dir_flag.as_str(),
        ];
        base_flags.extend_from_slice(extra_flags);

        self.wasixcc(
            &[main_c.to_str().unwrap()],
            &self.default_executable,
            &base_flags,
            &[],
            cpp,
        )?;

        Ok(self.default_executable.clone())
    }

    /// Compile a shared library (.so) from source files
    ///
    /// ### Example
    /// ```no_run
    /// let test = WasixccTest::new(file!(), "my_test");
    /// test.compile_shared_library(&["side.c"], "libside.so").unwrap();
    /// ```
    pub fn compile_shared_library(
        &self,
        sources: &[&str],
        lib_name: &str,
        extra_flags: &[&str],
    ) -> Result<PathBuf, anyhow::Error> {
        let cpp = sources
            .iter()
            .find(|source| {
                PathBuf::from(source)
                    .extension()
                    .map_or(false, |ext| ext == "cpp" || ext == "cc")
            })
            .is_some();

        let link_dir_flag = format!("-L{}", self.output_dir.display());
        let mut base_flags = vec![
            "-fwasm-exceptions",
            "-shared",
            "-fPIC",
            link_dir_flag.as_str(),
        ];
        base_flags.extend_from_slice(extra_flags);

        self.wasixcc(sources, &self.default_executable, &base_flags, &[], cpp)?;

        Ok(self.default_executable.clone())
    }

    /// Run a compiled WASM module using wasix. Returns ok if it exits with code 0.
    pub fn run_executable(&self, executable_path: &PathBuf) -> Result<(), anyhow::Error> {
        // The directory containing the WASM module
        let wasm_path = self.output_dir.join(executable_path.file_name().unwrap());

        // Load the compiled WASM module
        let wasm_bytes = std::fs::read(&wasm_path).expect("Failed to read compiled WASM file");
        let module_data = HashedModuleData::new(wasm_bytes);
        let (hash, wasm_bytes) = module_data.into_parts();
        let engine = wasmer::Engine::default();
        let module = Module::new(&engine, &wasm_bytes).expect("Failed to create module");

        // Create buffers to capture stdout and stderr
        //
        // For now these are not used, but they can be useful for debugging test failures.
        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
        let stderr_buffer = Arc::new(Mutex::new(Vec::new()));

        let stdout_capture = Box::new(CaptureFile::new(
            stdout_buffer.clone(),
            Some(std::io::stdout()),
        ));
        let stderr_capture = Box::new(CaptureFile::new(
            stderr_buffer.clone(),
            Some(std::io::stderr()),
        ));

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for running wasix programs");

        rt.block_on(async {
            tokio::task::block_in_place(move || {
                // Run the WASM module using WasiRunner
                let mut runner = WasiRunner::new();
                runner
                    .with_mapped_directories([MappedDirectory {
                        guest: self.output_dir.to_string_lossy().to_string(),
                        host: self.output_dir.clone(),
                    }])
                    .with_mapped_directories([MappedDirectory {
                        guest: "/lib".to_string(),
                        host: self.output_dir.clone(),
                    }])
                    .with_current_dir(self.output_dir.to_string_lossy().to_string())
                    .with_stdout(stdout_capture)
                    .with_stderr(stderr_capture);
                runner.run_wasm(
                    RuntimeOrEngine::Engine(engine),
                    &executable_path.to_string_lossy().to_string(),
                    module,
                    hash,
                )
            })
        })
    }

    /// Run the default compiled WASM module using wasix. Returns ok if it exits with code 0.
    ///
    /// If you need more control about the executable path use `run_executable()` instead.
    pub fn run(&self) -> Result<(), anyhow::Error> {
        self.run_executable(&self.output_dir.join(&self.test_name))
    }
}

/// Get the wasix-tests directory path
pub fn get_wasix_tests_dir() -> PathBuf {
    PathBuf::from(std::env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("wasix-tests")
}

/// Run a build.sh script in the context_switching directory
pub fn run_build_script(file: &str, test_dir: &str) -> Result<PathBuf, anyhow::Error> {
    let input_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(PathBuf::from(
            file.split('/')
                .next_back()
                .expect("The test file name cannot be empty")
                .trim_end_matches(".rs"),
        ));

    let cc = "wasixcc";
    let cxx = "wasix++";
    let test_path = input_dir.join(test_dir);
    let build_script = test_path.join("build.sh");

    let output = Command::new("bash")
        .arg(&build_script)
        .current_dir(&test_path)
        .env("CC", cc)
        .env("CXX", cxx)
        .env("WASIXCC_SYSROOT", "/home/lennart/.wasix-clang/wasix-sysroot")
        .env("WASIXCC_COMPILER_FLAGS", "-fPIC:-fwasm-exceptions:-Wl,-L/home/lennart/.wasix-clang/wasix-sysroot/usr/local/lib/wasm32-wasi:-I/home/lennart/.wasix-clang/wasix-sysroot/usr/local/include:-Wl,-mllvm,--wasm-enable-eh:-Wl,-mllvm,--wasm-enable-sjlj:-Wl,-mllvm,--wasm-use-legacy-eh=false:-Wl,-mllvm,--exception-model=wasm:-iwithsysroot:/usr/local/include/c++/v1")
        .output()?;

    if !output.status.success() {
        eprintln!("Build stdout: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("Build stderr: {}", String::from_utf8_lossy(&output.stderr));
        anyhow::bail!("Build script failed");
    }

    Ok(test_path.join("main"))
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
pub fn run_wasm_with_result(
    wasm_path: &PathBuf,
    dir: &Path,
) -> Result<WasmRunResult, anyhow::Error> {
    // Load the compiled WASM module
    let wasm_bytes = std::fs::read(&wasm_path)?;
    let module_data = HashedModuleData::new(wasm_bytes);
    let (hash, wasm_bytes) = module_data.into_parts();
    let engine = wasmer::Engine::default();
    let module = Module::new(&engine, &wasm_bytes)?;

    // Create buffers to capture stdout and stderr
    let stdout_buffer = Arc::new(Mutex::new(Vec::new()));
    let stderr_buffer = Arc::new(Mutex::new(Vec::new()));

    let stdout_capture = Box::new(CaptureFile::new(
        stdout_buffer.clone(),
        Some(std::io::stdout()),
    ));
    let stderr_capture = Box::new(CaptureFile::new(
        stderr_buffer.clone(),
        Some(std::io::stderr()),
    ));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime for running wasix programs");

    let result = rt.block_on(async {
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
                &wasm_path.to_string_lossy().to_string(),
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
    if let Some(code) = result.exit_code {
        if code != 0 {
            anyhow::bail!("WASI exited with code: {}", code);
        }
    }

    Ok(())
}
