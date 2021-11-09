//! Create a standalone native executable for a given Wasm file.

use crate::store::{CompilerOptions, EngineType};
use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use structopt::StructOpt;
use wasmer::*;

const WASMER_MAIN_C_SOURCE: &[u8] = include_bytes!("wasmer_create_exe_main.c");

#[derive(Debug, StructOpt)]
/// The options for the `wasmer create-exe` subcommand
pub struct CreateExe {
    /// Input file
    #[structopt(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Output file
    #[structopt(name = "OUTPUT PATH", short = "o", parse(from_os_str))]
    output: PathBuf,

    /// Compilation Target triple
    #[structopt(long = "target")]
    target_triple: Option<Triple>,

    #[structopt(flatten)]
    compiler: CompilerOptions,

    #[structopt(short = "m", multiple = true, number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,

    /// Additional libraries to link against.
    /// This is useful for fixing linker errors that may occur on some systems.
    #[structopt(short = "l", multiple = true, number_of_values = 1)]
    libraries: Vec<String>,
}

impl CreateExe {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        let target = self
            .target_triple
            .as_ref()
            .map(|target_triple| {
                let mut features = self
                    .cpu_features
                    .clone()
                    .into_iter()
                    .fold(CpuFeature::set(), |a, b| a | b);
                // Cranelift requires SSE2, so we have this "hack" for now to facilitate
                // usage
                features |= CpuFeature::SSE2;
                Target::new(target_triple.clone(), features)
            })
            .unwrap_or_default();
        let engine_type = EngineType::Staticlib;
        let (store, compiler_type) = self
            .compiler
            .get_store_for_target_and_engine(target.clone(), engine_type)?;

        println!("Engine: {}", engine_type.to_string());
        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());

        let working_dir = tempfile::tempdir()?;
        let starting_cd = env::current_dir()?;
        let output_path = starting_cd.join(&self.output);
        env::set_current_dir(&working_dir)?;

        #[cfg(not(windows))]
        let wasm_object_path = PathBuf::from("wasm.o");
        #[cfg(windows)]
        let wasm_object_path = PathBuf::from("wasm.obj");

        let wasm_module_path = starting_cd.join(&self.path);

        let module =
            Module::from_file(&store, &wasm_module_path).context("failed to compile Wasm")?;
        let _ = module.serialize_to_file(&wasm_object_path)?;

        let artifact: &wasmer_engine_staticlib::StaticlibArtifact =
            module.artifact().as_ref().downcast_ref().context(
                "Engine type is Staticlib but could not downcast artifact into StaticlibArtifact",
            )?;
        let symbol_registry = artifact.symbol_registry();
        let metadata_length = artifact.metadata_length();
        let module_info = module.info();
        let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
            module_info,
            symbol_registry,
            metadata_length,
        );

        generate_header(header_file_src.as_bytes())?;
        self.compile_c(wasm_object_path, output_path)?;

        eprintln!(
            "✔ Native executable compiled successfully to `{}`.",
            self.output.display(),
        );

        Ok(())
    }

    fn compile_c(&self, wasm_object_path: PathBuf, output_path: PathBuf) -> anyhow::Result<()> {
        use std::io::Write;

        // write C src to disk
        let c_src_path = Path::new("wasmer_main.c");
        #[cfg(not(windows))]
        let c_src_obj = PathBuf::from("wasmer_main.o");
        #[cfg(windows)]
        let c_src_obj = PathBuf::from("wasmer_main.obj");

        {
            let mut c_src_file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&c_src_path)
                .context("Failed to open C source code file")?;
            c_src_file.write_all(WASMER_MAIN_C_SOURCE)?;
        }
        run_c_compile(&c_src_path, &c_src_obj, self.target_triple.clone())
            .context("Failed to compile C source code")?;
        LinkCode {
            object_paths: vec![c_src_obj, wasm_object_path],
            output_path,
            additional_libraries: self.libraries.clone(),
            target: self.target_triple.clone(),
            ..Default::default()
        }
        .run()
        .context("Failed to link objects together")?;

        Ok(())
    }
}

fn generate_header(header_file_src: &[u8]) -> anyhow::Result<()> {
    let header_file_path = Path::new("my_wasm.h");
    let mut header = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&header_file_path)?;

    use std::io::Write;
    header.write_all(header_file_src)?;

    Ok(())
}

fn get_wasmer_dir() -> anyhow::Result<PathBuf> {
    Ok(PathBuf::from(
        env::var("WASMER_DIR")
            .or_else(|e| {
                option_env!("WASMER_INSTALL_PREFIX")
                    .map(str::to_string)
                    .ok_or(e)
            })
            .context("Trying to read env var `WASMER_DIR`")?,
    ))
}

fn get_wasmer_include_directory() -> anyhow::Result<PathBuf> {
    let mut path = get_wasmer_dir()?;
    path.push("include");
    Ok(path)
}

/// path to the static libwasmer
fn get_libwasmer_path() -> anyhow::Result<PathBuf> {
    let mut path = get_wasmer_dir()?;
    path.push("lib");

    // TODO: prefer headless Wasmer if/when it's a separate library.
    #[cfg(not(windows))]
    path.push("libwasmer.a");
    #[cfg(windows)]
    path.push("wasmer.lib");

    Ok(path)
}

/// Compile the C code.
fn run_c_compile(
    path_to_c_src: &Path,
    output_name: &Path,
    target: Option<Triple>,
) -> anyhow::Result<()> {
    #[cfg(not(windows))]
    let c_compiler = "cc";
    // We must use a C++ compiler on Windows because wasm.h uses `static_assert`
    // which isn't available in `clang` on Windows.
    #[cfg(windows)]
    let c_compiler = "clang++";

    let mut command = Command::new(c_compiler);
    let command = command
        .arg("-O2")
        .arg("-c")
        .arg(path_to_c_src)
        .arg("-I")
        .arg(get_wasmer_include_directory()?);

    let command = if let Some(target) = target {
        command.arg("-target").arg(format!("{}", target))
    } else {
        command
    };

    let output = command.arg("-o").arg(output_name).output()?;

    if !output.status.success() {
        bail!(
            "C code compile failed with: stdout: {}\n\nstderr: {}",
            std::str::from_utf8(&output.stdout)
                .expect("stdout is not utf8! need to handle arbitrary bytes"),
            std::str::from_utf8(&output.stderr)
                .expect("stderr is not utf8! need to handle arbitrary bytes")
        );
    }
    Ok(())
}

/// Data used to run a linking command for generated artifacts.
#[derive(Debug)]
struct LinkCode {
    /// Path to the linker used to run the linking command.
    linker_path: PathBuf,
    /// String used as an optimization flag.
    optimization_flag: String,
    /// Paths of objects to link.
    object_paths: Vec<PathBuf>,
    /// Additional libraries to link against.
    additional_libraries: Vec<String>,
    /// Path to the output target.
    output_path: PathBuf,
    /// Path to the dir containing the static libwasmer library.
    libwasmer_path: PathBuf,
    /// The target to link the executable for.
    target: Option<Triple>,
}

impl Default for LinkCode {
    fn default() -> Self {
        #[cfg(not(windows))]
        let linker = "cc";
        #[cfg(windows)]
        let linker = "clang";
        Self {
            linker_path: PathBuf::from(linker),
            optimization_flag: String::from("-O2"),
            object_paths: vec![],
            additional_libraries: vec![],
            output_path: PathBuf::from("a.out"),
            libwasmer_path: get_libwasmer_path().unwrap(),
            target: None,
        }
    }
}

impl LinkCode {
    fn run(&self) -> anyhow::Result<()> {
        let mut command = Command::new(&self.linker_path);
        let command = command
            .arg(&self.optimization_flag)
            .args(
                self.object_paths
                    .iter()
                    .map(|path| path.canonicalize().unwrap()),
            )
            .arg(
                &self
                    .libwasmer_path
                    .canonicalize()
                    .context("Failed to find libwasmer")?,
            );
        let command = if let Some(target) = &self.target {
            command.arg("-target").arg(format!("{}", target))
        } else {
            command
        };
        // Add libraries required per platform.
        // We need userenv, sockets (Ws2_32), advapi32 for some system calls and bcrypt for random numbers.
        #[cfg(windows)]
        let command = command
            .arg("-luserenv")
            .arg("-lWs2_32")
            .arg("-ladvapi32")
            .arg("-lbcrypt");
        // On unix we need dlopen-related symbols, libmath for a few things, and pthreads.
        #[cfg(not(windows))]
        let command = command.arg("-ldl").arg("-lm").arg("-pthread");
        let link_aganist_extra_libs = self
            .additional_libraries
            .iter()
            .map(|lib| format!("-l{}", lib));
        let command = command.args(link_aganist_extra_libs);
        let output = command.arg("-o").arg(&self.output_path).output()?;

        if !output.status.success() {
            bail!(
                "linking failed with: stdout: {}\n\nstderr: {}",
                std::str::from_utf8(&output.stdout)
                    .expect("stdout is not utf8! need to handle arbitrary bytes"),
                std::str::from_utf8(&output.stderr)
                    .expect("stderr is not utf8! need to handle arbitrary bytes")
            );
        }
        Ok(())
    }
}
