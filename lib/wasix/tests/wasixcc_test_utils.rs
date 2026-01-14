use std::path::{Path, PathBuf};
use std::process::Command;
use wasmer::Module;
use wasmer_wasix::runners::MappedDirectory;
use wasmer_wasix::runners::wasi::{RuntimeOrEngine, WasiRunner};
use wasmer_wasix::runtime::module_cache::HashedModuleData;

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
        test_file: &Path,
        output_file: &Path,
        extra_args: &[&str],
        extra_env: &[(&str, &str)],
        cpp: bool,
    ) -> Result<(), anyhow::Error> {
        // Derive the test directory from the test file path
        let input_file_path = self.input_dir.join(test_file);

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
        command
            .arg(&input_file_path)
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
        let compile_status = command
            .status()
            .expect(format!("Failed to run {tool}").as_str());
        assert!(compile_status.success(), "wasixcc compilation failed");

        assert!(
            output_file.exists(),
            "Expected output file does not exist after compilation"
        );

        Ok(())
    }

    /// Compiles the default C source file with wasixcc and wasm-exceptions enabled and returns the path to the WASM file.
    ///
    /// If you need more control about the file use `compile_c_executable()` instead.
    pub fn compile(&self) -> Result<PathBuf, anyhow::Error> {
        self.compile_c_executable(&self.default_source)
    }

    /// Compiles a C file with wasixcc and wasm-exceptions enabled and returns the path to the WASM file.
    ///
    /// If you need more control about the compiler flags use `wasixcc()` instead.
    #[allow(dead_code)]
    pub fn compile_c_executable(&self, source_file: &Path) -> Result<PathBuf, anyhow::Error> {
        let main_c = self.input_dir.join(source_file);

        self.wasixcc(
            &main_c,
            &self.default_executable,
            &["-fwasm-exceptions"],
            &[],
            false,
        )?;

        Ok(self.default_executable.clone())
    }

    /// Compiles a C++ file with wasixcc and wasm-exceptions enabled and returns the path to the WASM file.
    ///
    /// If you need more control about the compiler flags use `wasixcc()` instead.
    #[allow(dead_code)]
    pub fn compile_cpp_executable(&self, source_file: &Path) -> Result<PathBuf, anyhow::Error> {
        let main_c = self.input_dir.join(source_file);

        self.wasixcc(
            &main_c,
            &self.default_executable,
            &["-fwasm-exceptions"],
            &[],
            true,
        )?;

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
                    .with_current_dir(self.output_dir.to_string_lossy().to_string());
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
