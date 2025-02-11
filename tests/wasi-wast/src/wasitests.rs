//! This file will run at build time to autogenerate the WASI regression tests
//! It will compile the files indicated in TESTS, to:executable and .wasm
//! - Compile with the native rust target to get the expected output
//! - Compile with the latest WASI target to get the wasm
//! - Generate the test that will compare the output of running the .wasm file
//!   with wasmer with the expected output

use glob::glob;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use std::io;
use std::io::prelude::*;

use super::util;
use super::wasi_version::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeOutput {
    stdout: String,
    stderr: String,
    result: i64,
}

/// Compile and execute the test file as native code, saving the results to be
/// compared against later.
///
/// This function attempts to clean up its output after it executes it.
fn generate_native_output(
    temp_dir: &Path,
    file: &str,
    normalized_name: &str,
    args: &[String],
    options: &WasiOptions,
) -> io::Result<NativeOutput> {
    let executable_path = temp_dir.join(normalized_name);
    println!(
        "Compiling program {} to native at {}",
        file,
        executable_path.to_string_lossy()
    );
    let native_out = Command::new("rustc")
        .arg(file)
        .arg("-o")
        .args(args)
        .arg(&executable_path)
        .output()
        .expect("Failed to compile program to native code");
    util::print_info_on_error(&native_out, "COMPILATION FAILED");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = executable_path
            .metadata()
            .expect("native executable")
            .permissions();
        perm.set_mode(0o766);
        println!(
            "Setting execute permissions on {}",
            executable_path.to_string_lossy()
        );
        fs::set_permissions(&executable_path, perm)?;
    }

    println!(
        "Executing native program at {}",
        executable_path.to_string_lossy()
    );
    // workspace root
    const EXECUTE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wasi");
    let mut native_command = Command::new(&executable_path)
        .current_dir(EXECUTE_DIR)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    if let Some(stdin_str) = &options.stdin {
        write!(native_command.stdin.as_ref().unwrap(), "{stdin_str}").unwrap();
    }

    let result = native_command
        .wait()
        .expect("Failed to execute native program");

    let stdout_str = {
        let mut stdout = native_command.stdout.unwrap();
        let mut s = String::new();
        stdout.read_to_string(&mut s).unwrap();
        s
    };
    let stderr_str = {
        let mut stderr = native_command.stderr.unwrap();
        let mut s = String::new();
        stderr.read_to_string(&mut s).unwrap();
        s
    };
    if !result.success() {
        println!("NATIVE PROGRAM FAILED");
        println!("stdout:\n{stdout_str}");
        eprintln!("stderr:\n{stderr_str}");
    }

    let result = result.code().unwrap() as i64;
    Ok(NativeOutput {
        stdout: stdout_str,
        stderr: stderr_str,
        result,
    })
}

/// compile the Wasm file for the given version of WASI
///
/// returns the path of where the wasm file is
fn compile_wasm_for_version(
    temp_dir: &Path,
    file: &str,
    out_dir: &Path,
    rs_mod_name: &str,
    version: WasiVersion,
) -> io::Result<PathBuf> {
    //let out_dir = base_dir; //base_dir.join("..").join(version.get_directory_name());
    if !out_dir.exists() {
        fs::create_dir(out_dir)?;
    }
    let wasm_out_name = {
        let mut wasm_out_name = out_dir.join(rs_mod_name);
        wasm_out_name.set_extension("wasm");
        wasm_out_name
    };
    println!("Reading contents from file `{file}`");
    let file_contents: String = {
        let mut fc = String::new();
        let mut f = fs::OpenOptions::new().read(true).open(file)?;
        f.read_to_string(&mut fc)?;
        fc
    };

    let temp_wasi_rs_file_name = temp_dir.join(format!("wasi_modified_version_{rs_mod_name}.rs"));
    {
        let mut actual_file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&temp_wasi_rs_file_name)
            .unwrap();
        actual_file.write_all(file_contents.as_bytes()).unwrap();
    }

    println!(
        "Compiling wasm module `{}` with toolchain `{}`",
        &wasm_out_name.to_string_lossy(),
        version.get_compiler_toolchain()
    );
    let mut command = Command::new("rustc");

    command
        .arg(format!("+{}", version.get_compiler_toolchain()))
        .arg("--target=wasm32-wasip1")
        .arg("-C")
        .arg("opt-level=z")
        .arg(&temp_wasi_rs_file_name)
        .arg("-o")
        .arg(&wasm_out_name);
    println!("Command {command:?}");

    let wasm_compilation_out = command.output().expect("Failed to compile program to wasm");
    util::print_info_on_error(&wasm_compilation_out, "WASM COMPILATION");
    println!(
        "Removing file `{}`",
        &temp_wasi_rs_file_name.to_string_lossy()
    );

    // to prevent commiting huge binary blobs forever
    let wasm_strip_out = Command::new("wasm-strip")
        .arg(&wasm_out_name)
        .output()
        .expect("Failed to strip compiled wasm module");
    util::print_info_on_error(&wasm_strip_out, "STRIPPING WASM");
    let wasm_opt_out = Command::new("wasm-opt")
        .arg("-Oz")
        .arg(&wasm_out_name)
        .arg("-o")
        .arg(&wasm_out_name)
        .output()
        .expect("Failed to optimize compiled wasm module with wasm-opt!");
    util::print_info_on_error(&wasm_opt_out, "OPTIMIZING WASM");

    Ok(wasm_out_name)
}

/// Returns the a Vec of the test modules created
fn compile(temp_dir: &Path, file: &str, wasi_versions: &[WasiVersion]) {
    let src_code: String = fs::read_to_string(file).unwrap();
    let options: WasiOptions = extract_args_from_source_file(&src_code).unwrap_or_default();

    assert!(file.ends_with(".rs"));
    let rs_mod_name = {
        Path::new(&file.to_lowercase())
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string()
    };
    let base_dir = Path::new(file).parent().unwrap();
    let NativeOutput {
        stdout,
        stderr,
        result,
    } = generate_native_output(temp_dir, file, &rs_mod_name, &options.args, &options)
        .expect("Generate native output");

    let test = WasiTest {
        wasm_prog_name: format!("{rs_mod_name}.wasm"),
        stdout,
        stderr,
        result,
        options,
    };
    let test_serialized = test.into_wasi_wast();
    println!("Generated test output: {}", &test_serialized);

    wasi_versions
        .iter()
        .map(|&version| {
            let out_dir = base_dir.join("..").join(version.get_directory_name());
            if !out_dir.exists() {
                fs::create_dir(&out_dir).unwrap();
            }
            let wasm_out_name = {
                let mut wasm_out_name = out_dir.join(rs_mod_name.clone());
                wasm_out_name.set_extension("wast");
                wasm_out_name
            };
            println!("Writing test output to {}", wasm_out_name.to_string_lossy());
            fs::write(&wasm_out_name, test_serialized.clone()).unwrap();

            println!("Compiling wasm version {version:?}");
            compile_wasm_for_version(temp_dir, file, &out_dir, &rs_mod_name, version)
                .unwrap_or_else(|_| panic!("Could not compile Wasm to WASI version {:?}, perhaps you need to install the `{}` rust toolchain", version, version.get_compiler_toolchain()));
        }).for_each(drop); // Do nothing with it, but let the iterator be consumed/iterated.
}

const WASI_TEST_SRC_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/wasi/tests/*.rs");
pub fn build(wasi_versions: &[WasiVersion], specific_tests: &[&str]) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    for entry in glob(WASI_TEST_SRC_DIR).unwrap() {
        match entry {
            Ok(path) => {
                let test = path.to_str().unwrap();
                if !specific_tests.is_empty() {
                    if let Some(filename) = path.file_stem().and_then(|f| f.to_str()) {
                        if specific_tests.contains(&filename) {
                            compile(temp_dir.path(), test, wasi_versions);
                        }
                    }
                } else {
                    compile(temp_dir.path(), test, wasi_versions);
                }
            }
            Err(e) => println!("{e:?}"),
        }
    }
    println!("All modules generated.");
}

/// This is the structure of the `.wast` file
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WasiTest {
    /// The name of the wasm module to run
    pub wasm_prog_name: String,
    /// The program expected output on stdout
    pub stdout: String,
    /// The program expected output on stderr
    pub stderr: String,
    /// The program expected result
    pub result: i64,
    /// The program options
    pub options: WasiOptions,
}

impl WasiTest {
    fn into_wasi_wast(self) -> String {
        use std::fmt::Write;

        let mut out = format!(
            ";; This file was generated by https://github.com/wasmerio/wasi-tests\n
(wasi_test \"{}\"",
            self.wasm_prog_name
        );
        if !self.options.env.is_empty() {
            let envs = self
                .options
                .env
                .iter()
                .map(|(name, value)| format!("\"{name}={value}\""))
                .collect::<Vec<String>>()
                .join(" ");
            let _ = write!(out, "\n  (envs {envs})");
        }
        if !self.options.args.is_empty() {
            let args = self
                .options
                .args
                .iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<String>>()
                .join(" ");
            let _ = write!(out, "\n  (args {args})");
        }

        if !self.options.dir.is_empty() {
            let preopens = self
                .options
                .dir
                .iter()
                .map(|v| format!("\"{v}\""))
                .collect::<Vec<String>>()
                .join(" ");
            let _ = write!(out, "\n  (preopens {preopens})");
        }
        if !self.options.mapdir.is_empty() {
            let map_dirs = self
                .options
                .mapdir
                .iter()
                .map(|(a, b)| format!("\"{a}:{b}\""))
                .collect::<Vec<String>>()
                .join(" ");
            let _ = write!(out, "\n  (map_dirs {map_dirs})");
        }
        if !self.options.tempdir.is_empty() {
            let temp_dirs = self
                .options
                .tempdir
                .iter()
                .map(|td| format!("\"{td}\""))
                .collect::<Vec<String>>()
                .join(" ");
            let _ = write!(out, "\n  (temp_dirs {temp_dirs})");
        }

        let _ = write!(out, "\n  (assert_return (i64.const {}))", self.result);
        if let Some(stdin) = &self.options.stdin {
            let _ = write!(out, "\n  (stdin {stdin:?})");
        }

        if !self.stdout.is_empty() {
            let _ = write!(out, "\n  (assert_stdout {:?})", self.stdout);
        }
        if !self.stderr.is_empty() {
            let _ = write!(out, "\n  (assert_stderr {:?})", self.stderr);
        }

        let _ = write!(out, "\n)\n");

        out
    }
}

/// The options provied when executed a WASI Wasm program
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WasiOptions {
    /// Mapped pre-opened dirs
    pub mapdir: Vec<(String, String)>,
    /// Environment vars
    pub env: Vec<(String, String)>,
    /// Program arguments
    pub args: Vec<String>,
    /// Pre-opened directories
    pub dir: Vec<String>,
    /// The alias of the temporary directory to use
    pub tempdir: Vec<String>,
    /// Stdin to give to the native program and WASI program.
    pub stdin: Option<String>,
}

/// Pulls args to the program out of a comment at the top of the file starting with "// WasiOptions:"
fn extract_args_from_source_file(source_code: &str) -> Option<WasiOptions> {
    if source_code.starts_with("// WASI:") {
        let mut args = WasiOptions::default();
        for arg_line in source_code
            .lines()
            .skip(1)
            .take_while(|line| line.starts_with("// "))
        {
            let arg_line = arg_line.strip_prefix("// ").unwrap();
            let arg_line = arg_line.trim();
            let colon_idx = arg_line
                .find(':')
                .expect("directives provided at the top must be separated by a `:`");

            let (command_name, value) = arg_line.split_at(colon_idx);
            let value = value.strip_prefix(':').unwrap();
            let value = value.trim();

            match command_name {
                "mapdir" =>
                // We try first splitting by `::`
                {
                    if let [alias, real_dir] = value.split("::").collect::<Vec<&str>>()[..] {
                        args.mapdir.push((alias.to_string(), real_dir.to_string()));
                    } else if let [alias, real_dir] = value.split(':').collect::<Vec<&str>>()[..] {
                        // And then we try splitting by `:` (for compatibility with previous API)
                        args.mapdir.push((alias.to_string(), real_dir.to_string()));
                    } else {
                        eprintln!("Parse error in mapdir {value} not parsed correctly");
                    }
                }
                "env" => {
                    if let [name, val] = value.split('=').collect::<Vec<&str>>()[..] {
                        args.env.push((name.to_string(), val.to_string()));
                    } else {
                        eprintln!("Parse error in env {value} not parsed correctly");
                    }
                }
                "dir" => {
                    args.dir.push(value.to_string());
                }
                "arg" => {
                    args.args.push(value.to_string());
                }
                "tempdir" => {
                    args.tempdir.push(value.to_string());
                }
                "stdin" => {
                    assert!(args.stdin.is_none(), "Only the first `stdin` directive is used! Please correct this or update this code");
                    let s = value;
                    let s = s.strip_prefix('"').expect("expected leading '\"' in stdin");
                    let s = s
                        .trim_end()
                        .strip_suffix('\"')
                        .expect("expected trailing '\"' in stdin");
                    args.stdin = Some(s.to_string());
                }
                e => {
                    eprintln!("WARN: comment arg: `{e}` is not supported");
                }
            }
        }
        return Some(args);
    }
    None
}
