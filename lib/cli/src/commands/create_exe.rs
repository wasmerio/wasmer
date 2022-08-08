//! Create a standalone native executable for a given Wasm file.

use super::ObjectFormat;
use crate::store::CompilerOptions;
use anyhow::{Context, Result};
#[cfg(feature = "pirita_file")]
use pirita::{ParseOptions, PiritaFileMmap};
use clap::Parser;
use std::env;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasmer::*;
use wasmer_object::{emit_serialized, get_object_for_target};

const WASMER_MAIN_C_SOURCE: &str = include_str!("./wasmer_create_exe_main.c");
#[cfg(feature = "static-artifact-create")]
const WASMER_STATIC_MAIN_C_SOURCE: &[u8] = include_bytes!("./wasmer_static_create_exe_main.c");

#[derive(Debug, Clone)]
struct CrossCompile {
    /// Cross-compilation library path.
    library_path: Option<PathBuf>,

    /// Cross-compilation tarball library path.
    tarball: Option<PathBuf>,

    /// Specify `zig` binary path
    zig_binary_path: Option<PathBuf>,
}

struct CrossCompileSetup {
    target: Triple,
    zig_binary_path: PathBuf,
    library: PathBuf,
}

#[derive(Debug, Parser)]
/// The options for the `wasmer create-exe` subcommand
pub struct CreateExe {
    /// Input file
    #[clap(name = "FILE", parse(from_os_str))]
    path: PathBuf,

    /// Output file
    #[clap(name = "OUTPUT PATH", short = 'o', parse(from_os_str))]
    output: PathBuf,

    /// Compilation Target triple
    #[clap(long = "target")]
    target_triple: Option<Triple>,

    // Cross-compile with `zig`
    /// Cross-compilation library path.
    #[clap(long = "library-path")]
    library_path: Option<PathBuf>,

    /// Cross-compilation tarball library path.
    #[clap(long = "tarball")]
    tarball: Option<PathBuf>,

    /// Specify `zig` binary path
    #[clap(long = "zig-binary-path")]
    zig_binary_path: Option<PathBuf>,

    /// Object format options
    ///
    /// This flag accepts two options: `symbols` or `serialized`.
    /// - (default) `symbols` creates an
    /// executable where all functions and metadata of the module are regular object symbols
    /// - `serialized` creates an executable where the module is zero-copy serialized as raw data
    #[clap(name = "OBJECT_FORMAT", long = "object-format", verbatim_doc_comment)]
    object_format: Option<ObjectFormat>,

    /// Header file for object input
    ///
    /// If given, the input `PATH` is assumed to be an object created with `wasmer create-obj` and
    /// this is its accompanying header file.
    #[clap(name = "HEADER", long = "header", verbatim_doc_comment)]
    header: Option<PathBuf>,

    #[clap(short = 'm')]
    cpu_features: Vec<CpuFeature>,

    /// Additional libraries to link against.
    /// This is useful for fixing linker errors that may occur on some systems.
    #[clap(short = 'l')]
    libraries: Vec<String>,

    #[clap(flatten)]
    compiler: CompilerOptions,
}

impl CreateExe {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        /* Making library_path, tarball zig_binary_path flags require that target_triple flag
         * is set cannot be encoded with structopt, so we have to perform cli flag validation
         * manually here */
        let cross_compile: Option<CrossCompile> = if self.target_triple.is_none()
            && (self.library_path.is_some()
                || self.tarball.is_some()
                || self.zig_binary_path.is_some())
        {
            return Err(anyhow!(
                "To cross-compile an executable, you must specify a target triple with --target"
            ));
        } else if self.target_triple.is_some() {
            Some(CrossCompile {
                library_path: self.library_path.clone(),
                zig_binary_path: self.zig_binary_path.clone(),
                tarball: self.tarball.clone(),
            })
        } else {
            None
        };

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
                if target_triple.architecture == Architecture::X86_64 {
                    features |= CpuFeature::SSE2;
                }
                Target::new(target_triple.clone(), features)
            })
            .unwrap_or_default();

        let starting_cd = env::current_dir()?;
        let wasm_module_path = starting_cd.join(&self.path);
        #[cfg(feature = "pirita_file")]
        {
            if let Ok(pirita) =
                PiritaFileMmap::parse(wasm_module_path.clone(), &ParseOptions::default())
            {
                return self.create_exe_pirita(&pirita, target);
            }
        }

        let (store, compiler_type) = self.compiler.get_store_for_target(target.clone())?;
        let object_format = self.object_format.unwrap_or(ObjectFormat::Symbols);

        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());
        println!("Format: {:?}", object_format);

        let object_format = self.object_format.unwrap_or(ObjectFormat::Symbols);
        let working_dir = tempfile::tempdir()?;
        let working_dir = working_dir.path().to_path_buf();
        let output_path = starting_cd.join(&self.output);

        let cross_compilation: Option<CrossCompileSetup> = if let Some(mut cross_subc) =
            cross_compile.or_else(|| {
                if self.target_triple.is_some() {
                    Some(CrossCompile {
                        library_path: None,
                        tarball: None,
                        zig_binary_path: None,
                    })
                } else {
                    None
                }
            }) {
            if let ObjectFormat::Serialized = object_format {
                return Err(anyhow!(
                    "Cross-compilation with serialized object format is not implemented."
                ));
            }

            let target = if let Some(target_triple) = self.target_triple.clone() {
                target_triple
            } else {
                return Err(anyhow!(
                        "To cross-compile an executable, you must specify a target triple with --target"
                ));
            };
            if let Some(tarball_path) = cross_subc.tarball.as_mut() {
                if tarball_path.is_relative() {
                    *tarball_path = starting_cd.join(&tarball_path);
                    if !tarball_path.exists() {
                        return Err(anyhow!(
                            "Tarball path `{}` does not exist.",
                            tarball_path.display()
                        ));
                    } else if tarball_path.is_dir() {
                        return Err(anyhow!(
                            "Tarball path `{}` is a directory.",
                            tarball_path.display()
                        ));
                    }
                }
            }
            let zig_binary_path =
                find_zig_binary(cross_subc.zig_binary_path.as_ref().and_then(|p| {
                    if p.is_absolute() {
                        p.canonicalize().ok()
                    } else {
                        starting_cd.join(p).canonicalize().ok()
                    }
                }))?;
            let library = if let Some(v) = cross_subc.library_path.clone() {
                v
            } else {
                {
                    let libwasmer_path = if self
                        .target_triple
                        .clone()
                        .unwrap_or(Triple::host())
                        .operating_system
                        == wasmer_types::OperatingSystem::Windows
                    {
                        "lib/wasmer.lib"
                    } else {
                        "lib/libwasmer.a"
                    };
                    let filename = if let Some(local_tarball) = cross_subc.tarball {
                        let files = untar(local_tarball)?;
                        files.into_iter().find(|f| f.contains(libwasmer_path)).ok_or_else(|| {
                            anyhow!("Could not find libwasmer for {} target in the provided tarball path.", target)})?
                    } else {
                        #[cfg(feature = "http")]
                        {
                            let release = http_fetch::get_latest_release()?;
                            let tarball = http_fetch::download_release(release, target.clone())?;
                            let files = untar(tarball)?;
                            files.into_iter().find(|f| f.contains(libwasmer_path)).ok_or_else(|| {
                                anyhow!("Could not find libwasmer for {} target in the fetched release from Github: you can download it manually and specify its path with the --cross-compilation-library-path LIBRARY_PATH flag.", target)})?
                        }
                        #[cfg(not(feature = "http"))]
                        return Err(anyhow!("This wasmer binary isn't compiled with an HTTP request library (feature flag `http`). To cross-compile, specify the path of the non-native libwasmer or release tarball with the --library-path LIBRARY_PATH or --tarball TARBALL_PATH flag."));
                    };
                    filename.into()
                }
            };
            Some(CrossCompileSetup {
                target,
                zig_binary_path,
                library,
            })
        } else {
            None
        };

        let (store, compiler_type) = self.compiler.get_store_for_target(target.clone())?;

        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());
        println!("Format: {:?}", object_format);

        #[cfg(not(windows))]
        let wasm_object_path = working_dir.clone().join("wasm.o");
        #[cfg(windows)]
        let wasm_object_path = PathBuf::from("wasm.obj");

        let wasm_module_path = starting_cd.join(&self.path);

        if let Some(header_path) = self.header.as_ref() {
            /* In this case, since a header file is given, the input file is expected to be an
             * object created with `create-obj` subcommand */
            let header_path = starting_cd.join(&header_path);
            std::fs::copy(&header_path, Path::new("static_defs.h"))
                .context("Could not access given header file")?;
            if let Some(setup) = cross_compilation.as_ref() {
                self.compile_zig(
                    output_path,
                    wasm_module_path,
                    std::path::Path::new("static_defs.h").into(),
                    setup,
                )?;
            } else {
                self.link(
                    output_path,
                    wasm_module_path,
                    std::path::Path::new("static_defs.h").into(),
                )?;
            }
        } else {
            match object_format {
                ObjectFormat::Serialized => {
                    let module = Module::from_file(&store, &wasm_module_path)
                        .context("failed to compile Wasm")?;
                    let bytes = module.serialize()?;
                    let mut obj = get_object_for_target(target.triple())?;
                    emit_serialized(&mut obj, &bytes, target.triple())?;
                    let mut writer = BufWriter::new(File::create(&wasm_object_path)?);
                    obj.write_stream(&mut writer)
                        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                    writer.flush()?;
                    drop(writer);

                    let cli_given_triple = self.target_triple.clone();
                    self.compile_c(wasm_object_path, cli_given_triple, output_path)?;
                }
                #[cfg(not(feature = "static-artifact-create"))]
                ObjectFormat::Symbols => {
                    return Err(anyhow!("This version of wasmer-cli hasn't been compiled with static artifact support. You need to enable the `static-artifact-create` feature during compilation."));
                }
                #[cfg(feature = "static-artifact-create")]
                ObjectFormat::Symbols => {
                    let engine = store.engine();
                    let engine_inner = engine.inner();
                    let compiler = engine_inner.compiler()?;
                    let features = engine_inner.features();
                    let tunables = store.tunables();
                    let data: Vec<u8> = fs::read(wasm_module_path)?;
                    let prefixer: Option<PrefixerFn> = None;
                    let (module_info, obj, metadata_length, symbol_registry) =
                        Artifact::generate_object(
                            compiler, &data, prefixer, &target, tunables, features,
                        )?;

                    let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
                        &module_info,
                        &*symbol_registry,
                        metadata_length,
                    );
                    // Write object file with functions
                    let object_file_path: std::path::PathBuf =
                        std::path::Path::new("functions.o").into();
                    let mut writer = BufWriter::new(File::create(&object_file_path)?);
                    obj.write_stream(&mut writer)
                        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
                    writer.flush()?;
                    // Write down header file that includes pointer arrays and the deserialize function
                    let mut writer = BufWriter::new(File::create("static_defs.h")?);
                    writer.write_all(header_file_src.as_bytes())?;
                    writer.flush()?;
                    if let Some(setup) = cross_compilation.as_ref() {
                        self.compile_zig(
                            output_path,
                            object_file_path,
                            std::path::Path::new("static_defs.h").into(),
                            setup,
                        )?;
                    } else {
                        self.link(
                            output_path,
                            object_file_path,
                            std::path::Path::new("static_defs.h").into(),
                        )?;
                    }
                }
            }
        }

        if cross_compilation.is_some() {
            eprintln!(
                "✔ Cross-compiled executable for `{}` target compiled successfully to `{}`.",
                target.triple(),
                self.output.display(),
            );
        } else {
            eprintln!(
                "✔ Native executable compiled successfully to `{}`.",
                self.output.display(),
            );
        }

        Ok(())
    }

    #[cfg(feature = "pirita_file")]
    fn create_exe_pirita(&self, file: &PiritaFileMmap, target: Target) -> anyhow::Result<()> {
        let starting_cd = env::current_dir()?;
        let working_dir = tempfile::tempdir()?;
        let working_dir = working_dir.path().to_path_buf();
        let output_path = starting_cd.join(&self.output);

        let volume_bytes = file.get_volumes_as_fileblock();
        let mut volumes_object = get_object_for_target(&target.triple())?;
        emit_serialized(
            &mut volumes_object,
            volume_bytes.as_slice(),
            target.triple(),
            "VOLUMES",
        )?;

        let mut link_objects = Vec::new();

        #[cfg(not(windows))]
        let volume_object_path = working_dir.clone().join("volumes.o");
        #[cfg(windows)]
        let volume_object_path = working_dir.clone().join("volumes.obj");

        let (store, _) = self.compiler.get_store_for_target(target.clone())?;

        let mut c_code_to_add = String::new();
        let mut c_code_to_instantiate = String::new();
        let mut deallocate_module = String::new();

        let atom_to_run = match file.manifest.entrypoint.as_ref() {
            Some(s) => file
                .get_atom_name_for_command("wasi", s)
                .map_err(|e| anyhow!("Could not get atom for entrypoint: {e}"))?,
            None => {
                return Err(anyhow!(
                    "Cannot compile to exe: no entrypoint to run package with"
                ));
            }
        };

        let compiled_modules = file
            .get_all_atoms()
            .into_iter()
            .map(|(atom_name, atom_bytes)| {
                let module = Module::new(&store, &atom_bytes)
                    .context(format!("Failed to compile atom {atom_name:?} to wasm"))?;
                let bytes = module.serialize()?;
                let mut obj = get_object_for_target(target.triple())?;
                let atom_name_uppercase = atom_name.to_uppercase();
                emit_serialized(&mut obj, &bytes, target.triple(), &atom_name_uppercase)?;

                c_code_to_add.push_str(&format!("
                extern size_t {atom_name_uppercase}_LENGTH asm(\"{atom_name_uppercase}_LENGTH\");
                extern char {atom_name_uppercase}_DATA asm(\"{atom_name_uppercase}_DATA\");
                "));

                c_code_to_instantiate.push_str(&format!("
                wasm_byte_vec_t atom_{atom_name}_byte_vec = {{
                    .size = {atom_name_uppercase}_LENGTH,
                    .data = (const char*)&{atom_name_uppercase}_DATA,
                }};
                wasm_module_t *atom_{atom_name} = wasm_module_deserialize(store, &atom_{atom_name}_byte_vec);
    
                if (!atom_{atom_name}) {{
                    fprintf(stderr, \"Failed to create module from atom \\\"{atom_name}\\\"\\n\");
                    print_wasmer_error();
                    return -1;
                }}

                "));
                deallocate_module.push_str(&format!("wasm_module_delete(atom_{atom_name});"));
                Ok((atom_name.clone(), obj))
            })
            .collect::<Result<Vec<_>, anyhow::Error>>()?;

        c_code_to_instantiate.push_str(&format!("wasm_module_t *module = atom_{atom_to_run};"));

        let mut writer = BufWriter::new(File::create(&volume_object_path)?);
        volumes_object
            .write_stream(&mut writer)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        writer.flush()?;
        drop(writer);

        link_objects.push(volume_object_path.clone());

        for (name, obj) in compiled_modules {
            #[cfg(not(windows))]
            let object_path = working_dir.clone().join(&format!("{name}.o"));
            #[cfg(windows)]
            let object_path = working_dir.clone().join(&format!("{name}.obj"));

            let mut writer = BufWriter::new(File::create(&object_path)?);
            obj.write_stream(&mut writer)
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            writer.flush()?;
            drop(writer);

            link_objects.push(object_path.clone());
        }

        // write C src to disk
        let c_src_path = working_dir.clone().join("wasmer_main.c");
        #[cfg(not(windows))]
        let c_src_obj = working_dir.clone().join("wasmer_main.o");
        #[cfg(windows)]
        let c_src_obj = working_dir.clone().join("wasmer_main.obj");

        let c_code = WASMER_MAIN_C_SOURCE
            .replace("#define WASI", "#define WASI\r\n#define WASI_PIRITA")
            .replace("// DECLARE_MODULES", &c_code_to_add)
            .replace("// INSTANTIATE_MODULES", &c_code_to_instantiate)
            .replace("##atom-name##", &atom_to_run)
            .replace("wasm_module_delete(module);", &deallocate_module);

        std::fs::write(&c_src_path, c_code.as_bytes())
            .context("Failed to open C source code file")?;

        run_c_compile(c_src_path.as_path(), &c_src_obj, self.target_triple.clone())
            .context("Failed to compile C source code")?;

        link_objects.push(c_src_obj.clone());

        LinkCode {
            object_paths: link_objects,
            output_path,
            additional_libraries: self.libraries.clone(),
            target: self.target_triple.clone(),
            ..Default::default()
        }
        .run()
        .context("Failed to link objects together")?;

        Ok(())
    }

    fn compile_c(
        &self,
        wasm_object_path: PathBuf,
        target_triple: Option<wasmer::Triple>,
        output_path: PathBuf,
    ) -> anyhow::Result<()> {
        // write C src to disk
        let c_src_path = Path::new("wasmer_main.c");
        #[cfg(not(windows))]
        let c_src_obj = PathBuf::from("wasmer_main.o");
        #[cfg(windows)]
        let c_src_obj = PathBuf::from("wasmer_main.obj");

        std::fs::write(
            &c_src_path,
            WASMER_MAIN_C_SOURCE.replace("// WASI_DEFINES", "#define WASI"),
        )?;

        run_c_compile(c_src_path, &c_src_obj, self.target_triple.clone())
            .context("Failed to compile C source code")?;

        LinkCode {
            object_paths: vec![c_src_obj, wasm_object_path],
            output_path,
            additional_libraries: self.libraries.clone(),
            target: target_triple,
            ..Default::default()
        }
        .run()
        .context("Failed to link objects together")?;

        Ok(())
    }

    fn compile_zig(
        &self,
        output_path: PathBuf,
        object_path: PathBuf,
        mut header_code_path: PathBuf,
        setup: &CrossCompileSetup,
    ) -> anyhow::Result<()> {
        let c_src_path = Path::new("wasmer_main.c");
        let CrossCompileSetup {
            ref target,
            ref zig_binary_path,
            ref library,
        } = setup;
        let mut libwasmer_path = library.to_path_buf();

        println!("Library Path: {}", libwasmer_path.display());
        /* Cross compilation is only possible with zig */
        println!("Using zig binary: {}", zig_binary_path.display());
        let zig_triple = triple_to_zig_triple(target);
        eprintln!("Using zig target triple: {}", &zig_triple);

        let lib_filename = libwasmer_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        libwasmer_path.pop();
        {
            let mut c_src_file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&c_src_path)
                .context("Failed to open C source code file")?;
            c_src_file.write_all(WASMER_STATIC_MAIN_C_SOURCE)?;
        }

        if !header_code_path.is_dir() {
            header_code_path.pop();
        }

        if header_code_path.display().to_string().is_empty() {
            header_code_path = std::env::current_dir()?;
        }

        /* Compile main function */
        let compilation = {
            let mut include_dir = libwasmer_path.clone();
            include_dir.pop();
            include_dir.push("include");

            let mut cmd = Command::new(zig_binary_path);
            let mut cmd_mut: &mut Command = cmd
                .arg("cc")
                .arg("-target")
                .arg(&zig_triple)
                .arg(&format!("-L{}", libwasmer_path.display()))
                .arg(&format!("-l:{}", lib_filename))
                .arg(&format!("-I{}", include_dir.display()))
                .arg(&format!("-I{}", header_code_path.display()));
            if !zig_triple.contains("windows") {
                cmd_mut = cmd_mut.arg("-lunwind");
            }
            cmd_mut
                .arg(&object_path)
                .arg(&c_src_path)
                .arg("-o")
                .arg(&output_path)
                .output()
                .context("Could not execute `zig`")?
        };
        if !compilation.status.success() {
            return Err(anyhow::anyhow!(String::from_utf8_lossy(
                &compilation.stderr
            )
            .to_string()));
        }
        Ok(())
    }

    #[cfg(feature = "static-artifact-create")]
    fn link(
        &self,
        output_path: PathBuf,
        object_path: PathBuf,
        mut header_code_path: PathBuf,
    ) -> anyhow::Result<()> {
        let linkcode = LinkCode {
            object_paths: vec![object_path, "main_obj.obj".into()],
            output_path,
            ..Default::default()
        };
        let c_src_path = Path::new("wasmer_main.c");
        let mut libwasmer_path = get_libwasmer_path()?
            .canonicalize()
            .context("Failed to find libwasmer")?;

        println!("Using libwasmer file: {}", libwasmer_path.display());

        let lib_filename = libwasmer_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        libwasmer_path.pop();
        {
            let mut c_src_file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&c_src_path)
                .context("Failed to open C source code file")?;
            c_src_file.write_all(WASMER_STATIC_MAIN_C_SOURCE)?;
        }

        if !header_code_path.is_dir() {
            header_code_path.pop();
        }

        if header_code_path.display().to_string().is_empty() {
            header_code_path = std::env::current_dir()?;
        }

        /* Compile main function */
        let compilation = {
            Command::new("cc")
                .arg("-c")
                .arg(&c_src_path)
                .arg(if linkcode.optimization_flag.is_empty() {
                    "-O2"
                } else {
                    linkcode.optimization_flag.as_str()
                })
                .arg(&format!("-L{}", libwasmer_path.display()))
                .arg(&format!("-I{}", get_wasmer_include_directory()?.display()))
                .arg(&format!("-l:{}", lib_filename))
                //.arg("-lwasmer")
                // Add libraries required per platform.
                // We need userenv, sockets (Ws2_32), advapi32 for some system calls and bcrypt for random numbers.
                //#[cfg(windows)]
                //    .arg("-luserenv")
                //    .arg("-lWs2_32")
                //    .arg("-ladvapi32")
                //    .arg("-lbcrypt")
                // On unix we need dlopen-related symbols, libmath for a few things, and pthreads.
                //#[cfg(not(windows))]
                .arg("-ldl")
                .arg("-lm")
                .arg("-pthread")
                .arg(&format!("-I{}", header_code_path.display()))
                .arg("-v")
                .arg("-o")
                .arg("main_obj.obj")
                .output()?
        };
        if !compilation.status.success() {
            return Err(anyhow::anyhow!(String::from_utf8_lossy(
                &compilation.stderr
            )
            .to_string()));
        }
        linkcode.run().context("Failed to link objects together")?;
        Ok(())
    }
}

fn triple_to_zig_triple(target_triple: &Triple) -> String {
    let arch = match target_triple.architecture {
        wasmer_types::Architecture::X86_64 => "x86_64".into(),
        wasmer_types::Architecture::Aarch64(wasmer_types::Aarch64Architecture::Aarch64) => {
            "aarch64".into()
        }
        v => v.to_string(),
    };
    let os = match target_triple.operating_system {
        wasmer_types::OperatingSystem::Linux => "linux".into(),
        wasmer_types::OperatingSystem::Darwin => "macos".into(),
        wasmer_types::OperatingSystem::Windows => "windows".into(),
        v => v.to_string(),
    };
    let env = match target_triple.environment {
        wasmer_types::Environment::Musl => "musl",
        wasmer_types::Environment::Gnu => "gnu",
        wasmer_types::Environment::Msvc => "msvc",
        _ => "none",
    };
    format!("{}-{}-{}", arch, os, env)
}

fn get_wasmer_dir() -> anyhow::Result<PathBuf> {
    let wasmer_dir = PathBuf::from(
        env::var("WASMER_DIR")
            .or_else(|e| {
                option_env!("WASMER_INSTALL_PREFIX")
                    .map(str::to_string)
                    .ok_or(e)
            })
            .context("Trying to read env var `WASMER_DIR`")?,
    );
    let wasmer_dir = wasmer_dir.clone().canonicalize().unwrap_or(wasmer_dir);
    Ok(wasmer_dir)
}

fn get_wasmer_include_directory() -> anyhow::Result<PathBuf> {
    let mut path = get_wasmer_dir()?;
    if path.clone().join("wasmer.h").exists() {
        return Ok(path);
    }
    path.push("include");
    if !path.clone().join("wasmer.h").exists() {
        println!(
            "wasmer.h does not exist in {}, will probably default to the system path",
            path.canonicalize().unwrap().display()
        );
    }

    Ok(path)
}

/// path to the static libwasmer
fn get_libwasmer_path() -> anyhow::Result<PathBuf> {
    let path = get_wasmer_dir()?;

    // TODO: prefer headless Wasmer if/when it's a separate library.
    #[cfg(not(windows))]
    let libwasmer_static_name = "libwasmer.a";
    #[cfg(windows)]
    let libwasmer_static_name = "libwasmer.lib";

    if path.exists() && path.join(libwasmer_static_name).exists() {
        Ok(path.join(libwasmer_static_name))
    } else {
        Ok(path.join("lib").join(libwasmer_static_name))
    }
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
        let libwasmer_path = self
            .libwasmer_path
            .clone()
            .canonicalize()
            .unwrap_or(self.libwasmer_path.clone());
        println!(
            "Using path `{}` as libwasmer path.",
            libwasmer_path.display()
        );
        let mut command = Command::new(&self.linker_path);
        let command = command
            .arg(&self.optimization_flag)
            .args(
                self.object_paths
                    .iter()
                    .map(|path| path.canonicalize().unwrap()),
            )
            .arg(&libwasmer_path);
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
        let link_against_extra_libs = self
            .additional_libraries
            .iter()
            .map(|lib| format!("-l{}", lib));
        let command = command.args(link_against_extra_libs);
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

#[cfg(feature = "http")]
mod http_fetch {
    use anyhow::{anyhow, Context, Result};
    use http_req::{
        request::Request,
        response::{Response, StatusCode},
        uri::Uri,
    };
    use std::convert::TryFrom;

    pub fn get_latest_release() -> Result<serde_json::Value> {
        let mut writer = Vec::new();
        let uri = Uri::try_from("https://api.github.com/repos/wasmerio/wasmer/releases").unwrap();

        let response = Request::new(&uri)
            .header("User-Agent", "wasmer")
            .header("Accept", "application/vnd.github.v3+json")
            .timeout(Some(std::time::Duration::new(30, 0)))
            .send(&mut writer)
            .map_err(anyhow::Error::new)
            .context("Could not lookup wasmer repository on Github.")?;

        if response.status_code() != StatusCode::new(200) {
            return Err(anyhow!(
                "Github API replied with non-200 status code: {}",
                response.status_code()
            ));
        }

        let v: std::result::Result<serde_json::Value, _> = serde_json::from_reader(&*writer);
        let mut response = v.map_err(anyhow::Error::new)?;

        if let Some(releases) = response.as_array_mut() {
            releases.retain(|r| {
                r["tag_name"].is_string() && !r["tag_name"].as_str().unwrap().is_empty()
            });
            releases.sort_by_cached_key(|r| r["tag_name"].as_str().unwrap_or_default().to_string());
            if let Some(latest) = releases.pop() {
                return Ok(latest);
            }
        }

        Err(anyhow!(
            "Could not get expected Github API response.\n\nReason: response format is not recognized:\n{:#?}", ""
        ))
    }

    pub fn download_release(
        mut release: serde_json::Value,
        target_triple: wasmer::Triple,
    ) -> Result<std::path::PathBuf> {
        if let Some(assets) = release["assets"].as_array_mut() {
            assets.retain(|a| {
                if let Some(name) = a["name"].as_str() {
                    match target_triple.architecture {
                        wasmer_types::Architecture::X86_64 => {
                            name.contains("x86_64") || name.contains("amd64")
                        }
                        wasmer_types::Architecture::Aarch64(
                            wasmer_types::Aarch64Architecture::Aarch64,
                        ) => name.contains("arm64") || name.contains("aarch64"),
                        _ => false,
                    }
                } else {
                    false
                }
            });
            assets.retain(|a| {
                if let Some(name) = a["name"].as_str() {
                    match target_triple.vendor {
                        wasmer_types::Vendor::Apple => {
                            name.contains("apple")
                                || name.contains("macos")
                                || name.contains("darwin")
                        }
                        wasmer_types::Vendor::Pc => name.contains("windows"),
                        _ => true,
                    }
                } else {
                    false
                }
            });
            assets.retain(|a| {
                if let Some(name) = a["name"].as_str() {
                    match target_triple.operating_system {
                        wasmer_types::OperatingSystem::Darwin => {
                            name.contains("apple")
                                || name.contains("darwin")
                                || name.contains("macos")
                        }
                        wasmer_types::OperatingSystem::Windows => name.contains("windows"),
                        wasmer_types::OperatingSystem::Linux => name.contains("linux"),
                        _ => false,
                    }
                } else {
                    false
                }
            });
            assets.retain(|a| {
                if let Some(name) = a["name"].as_str() {
                    match target_triple.environment {
                        wasmer_types::Environment::Musl => name.contains("musl"),
                        _ => !name.contains("musl"),
                    }
                } else {
                    false
                }
            });

            if assets.len() == 1 {
                let browser_download_url =
                    if let Some(url) = assets[0]["browser_download_url"].as_str() {
                        url.to_string()
                    } else {
                        return Err(anyhow!(
                            "Could not get download url from Github API response."
                        ));
                    };
                let filename = browser_download_url
                    .split('/')
                    .last()
                    .unwrap_or("output")
                    .to_string();
                let mut file = std::fs::File::create(&filename)?;
                println!("Downloading {} to {}", browser_download_url, &filename);
                let download_thread: std::thread::JoinHandle<Result<Response, anyhow::Error>> =
                    std::thread::spawn(move || {
                        let uri = Uri::try_from(browser_download_url.as_str())?;
                        let mut response = Request::new(&uri)
                            .header("User-Agent", "wasmer")
                            .send(&mut file)
                            .map_err(anyhow::Error::new)
                            .context("Could not lookup wasmer artifact on Github.")?;
                        if response.status_code() == StatusCode::new(302) {
                            let redirect_uri =
                                Uri::try_from(response.headers().get("Location").unwrap().as_str())
                                    .unwrap();
                            response = Request::new(&redirect_uri)
                                .header("User-Agent", "wasmer")
                                .send(&mut file)
                                .map_err(anyhow::Error::new)
                                .context("Could not lookup wasmer artifact on Github.")?;
                        }
                        Ok(response)
                    });
                let _response = download_thread
                    .join()
                    .expect("Could not join downloading thread");
                return Ok(filename.into());
            }
        }
        Err(anyhow!("Could not get release artifact."))
    }
}

fn untar(tarball: std::path::PathBuf) -> Result<Vec<String>> {
    let files = std::process::Command::new("tar")
        .arg("-tf")
        .arg(&tarball)
        .output()
        .expect("failed to execute process")
        .stdout;

    let files_s = String::from_utf8(files)?;

    let files = files_s
        .lines()
        .filter(|p| !p.ends_with('/'))
        .map(|s| s.to_string())
        .collect::<Vec<String>>();

    let _output = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&tarball)
        .output()
        .expect("failed to execute process");
    Ok(files)
}

fn find_zig_binary(path: Option<PathBuf>) -> Result<PathBuf> {
    use std::env::split_paths;
    use std::ffi::OsStr;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStrExt;
    let path_var = std::env::var("PATH").unwrap_or_default();
    #[cfg(unix)]
    let system_path_var = std::process::Command::new("getconf")
        .args(&["PATH"])
        .output()
        .map(|output| output.stdout)
        .unwrap_or_default();
    let retval = if let Some(p) = path {
        if p.exists() {
            p
        } else {
            return Err(anyhow!("Could not find `zig` binary in {}.", p.display()));
        }
    } else {
        let mut retval = None;
        for mut p in split_paths(&path_var).chain(split_paths(
            #[cfg(unix)]
            {
                &OsStr::from_bytes(&system_path_var[..])
            },
            #[cfg(not(unix))]
            {
                OsStr::new("")
            },
        )) {
            p.push("zig");
            if p.exists() {
                retval = Some(p);
                break;
            }
        }
        retval.ok_or_else(|| anyhow!("Could not find `zig` binary in PATH."))?
    };

    let version = std::process::Command::new(&retval)
        .arg("version")
        .output()
        .with_context(|| {
            format!(
                "Could not execute `zig` binary at path `{}`",
                retval.display()
            )
        })?
        .stdout;
    let version_slice = if let Some(pos) = version
        .iter()
        .position(|c| !(c.is_ascii_digit() || (*c == b'.')))
    {
        &version[..pos]
    } else {
        &version[..]
    };

    if version_slice < b"0.10.0".as_ref() {
        Err(anyhow!("`zig` binary in PATH (`{}`) is not a new enough version (`{}`): please use version `0.10.0` or newer.", retval.display(), String::from_utf8_lossy(version_slice)))
    } else {
        Ok(retval)
    }
}
