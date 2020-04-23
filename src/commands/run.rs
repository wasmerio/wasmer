use crate::common::{get_cache_dir, WasmFeatures};
use anyhow::{anyhow, bail, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use wasmer::*;

use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone)]
/// The options for the `wasmer run` subcommand
pub struct Run {
    /// Disable the cache
    #[structopt(long = "disable-cache")]
    disable_cache: bool,

    /// Input file
    #[structopt(parse(from_os_str))]
    path: PathBuf,

    /// Invoke a specified function
    #[structopt(long = "invoke", short = "i")]
    invoke: Option<String>,

    /// The command name is a string that will override the first argument passed
    /// to the wasm program. This is used in wapm to provide nicer output in
    /// help commands and error messages of the running wasm program
    #[structopt(long = "command-name", hidden = true)]
    command_name: Option<String>,

    /// A prehashed string, used to speed up start times by avoiding hashing the
    /// wasm module. If the specified hash is not found, Wasmer will hash the module
    /// as if no `cache-key` argument was passed.
    #[structopt(long = "cache-key", hidden = true)]
    cache_key: Option<String>,

    /// Use Singlepass compiler
    #[structopt(long, conflicts_with_all = &["cranelift", "llvm"])]
    singlepass: bool,

    /// Use Cranelift compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "llvm"])]
    cranelift: bool,

    /// Use LLVM compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "cranelifft"])]
    llvm: bool,

    // #[structopt(flatten)]
    // llvm: LLVMCLIOptions,

    // #[structopt(flatten)]
    // wasi: WasiOptions,
    #[structopt(flatten)]
    features: WasmFeatures,

    /// Enable non-standard experimental IO devices
    #[cfg(feature = "io-devices")]
    #[structopt(long = "enable-io-devices")]
    enable_experimental_io_devices: bool,

    /// Enable debug output
    #[cfg(feature = "debug")]
    #[structopt(long = "debug", short = "d")]
    debug: bool,

    /// Application arguments
    #[structopt(name = "--", multiple = true)]
    args: Vec<String>,
}

#[derive(Debug)]
enum Compiler {
    Singlepass,
    Cranelift,
    LLVM,
}

impl Run {
    /// Execute the run command
    pub fn execute(&self) -> Result<()> {
        let compiler_config = self.get_compiler_config()?;
        let engine = Engine::new(&*compiler_config);
        let store = Store::new(&engine);
        let module = Module::from_file(&store, &self.path)?;
        let imports = imports! {};
        let instance = Instance::new(&module, &imports)?;

        if let Some(ref invoke) = self.invoke {
            let result = self
                .invoke_function(&instance, &invoke, &self.args)
                .with_context(|| format!("Failed to invoke `{}`", invoke))?;
            println!(
                "{}",
                result
                    .iter()
                    .map(|val| val.to_string())
                    .collect::<Vec<String>>()
                    .join(" ")
            );
            return Ok(());
        }

        let start: &Func = instance.exports.get("_start")?;
        start.call(&[])?;

        Ok(())
    }

    fn invoke_function(
        &self,
        instance: &Instance,
        invoke: &str,
        args: &Vec<String>,
    ) -> Result<Box<[Val]>> {
        let func: &Func = instance.exports.get(&invoke)?;
        let func_ty = func.ty();
        let required_arguments = func_ty.params().len();
        let provided_arguments = args.len();
        if required_arguments != provided_arguments {
            bail!(
                "Function expected {} arguments, but received {}: \"{}\"",
                required_arguments,
                provided_arguments,
                self.args.join(" ")
            );
        }
        let invoke_args = args
            .iter()
            .zip(func_ty.params().iter())
            .map(|(arg, param_type)| match param_type {
                ValType::I32 => {
                    Ok(Val::I32(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a i32", arg)
                    })?))
                }
                ValType::I64 => {
                    Ok(Val::I64(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a i64", arg)
                    })?))
                }
                ValType::F32 => {
                    Ok(Val::F32(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a f32", arg)
                    })?))
                }
                ValType::F64 => {
                    Ok(Val::F64(arg.parse().map_err(|_| {
                        anyhow!("Can't convert `{}` into a f64", arg)
                    })?))
                }
                _ => Err(anyhow!(
                    "Don't know how to convert {} into {:?}",
                    arg,
                    param_type
                )),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(func.call(&invoke_args)?)
    }

    fn get_compiler(&self) -> Result<Compiler> {
        if self.cranelift {
            return Ok(Compiler::Cranelift);
        } else if self.llvm {
            return Ok(Compiler::LLVM);
        } else if self.singlepass {
            return Ok(Compiler::Singlepass);
        } else {
            // Auto mode, we choose the best compiler for that platform
            if cfg!(feature = "compiler-cranelift") && cfg!(target_arch = "x86_64") {
                return Ok(Compiler::Cranelift);
            }
            if cfg!(feature = "compiler-singlepass") && cfg!(target_arch = "x86_64") {
                return Ok(Compiler::Singlepass);
            }
            if cfg!(feature = "compiler-llvm") {
                return Ok(Compiler::LLVM);
            }
            bail!("There are no available compilers for your architecture");
        }
    }

    fn get_compiler_config(&self) -> Result<Box<dyn CompilerConfig>> {
        let compiler = self.get_compiler()?;
        match compiler {
            #[cfg(feature = "compiler-singlepass")]
            Compiler::Singlepass => {
                let config = SinglepassConfig::default();
                return Ok(Box::new(config));
            }
            #[cfg(feature = "compiler-cranelift")]
            Compiler::Cranelift => {
                let config = CraneliftConfig::default();
                return Ok(Box::new(config));
            }
            #[cfg(feature = "compiler-llvm")]
            Compiler::LLVM => {
                let config = LLVMConfig::default();
                return Ok(Box::new(config));
            }
            compiler => bail!(
                "The compiler {:?} is not included in this binary.",
                compiler
            ),
        }
    }
}
