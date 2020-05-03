//! Common module with common used structures across different
//! commands.

use crate::common::WasmFeatures;
use anyhow::{bail, Error, Result};
use std::str::FromStr;
use std::string::ToString;
use structopt::StructOpt;
use wasmer::*;

#[derive(Debug, Clone, StructOpt)]
/// The compiler options
pub struct StoreOptions {
    /// Use Singlepass compiler
    #[structopt(long, conflicts_with_all = &["cranelift", "llvm", "backend"])]
    singlepass: bool,

    /// Use Cranelift compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "llvm", "backend"])]
    cranelift: bool,

    /// Use LLVM compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "cranelift", "backend"])]
    llvm: bool,

    /// The deprecated backend flag - Please not use
    #[structopt(long = "backend", hidden = true, conflicts_with_all = &["singlepass", "cranelift", "llvm"])]
    backend: Option<String>,

    #[structopt(flatten)]
    features: WasmFeatures,
    // #[structopt(flatten)]
    // llvm_options: LLVMCLIOptions,
}

#[derive(Debug)]
enum Compiler {
    Singlepass,
    Cranelift,
    LLVM,
}

impl ToString for Compiler {
    fn to_string(&self) -> String {
        match self {
            Self::Singlepass => "singlepass".to_string(),
            Self::Cranelift => "cranelift".to_string(),
            Self::LLVM => "llvm".to_string(),
        }
    }
}

impl FromStr for Compiler {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "singlepass" => Ok(Self::Singlepass),
            "cranelift" => Ok(Self::Cranelift),
            "llvm" => Ok(Self::LLVM),
            backend => bail!("The `{}` compiler does not exist.", backend),
        }
    }
}

impl StoreOptions {
    fn get_compiler(&self) -> Result<Compiler> {
        if self.cranelift {
            return Ok(Compiler::Cranelift);
        } else if self.llvm {
            return Ok(Compiler::LLVM);
        } else if self.singlepass {
            return Ok(Compiler::Singlepass);
        } else if let Some(backend) = self.backend.clone() {
            eprintln!(
                "warning: the `--backend={0}` flag is deprecated, please use `--{0}` instead",
                backend
            );
            return Compiler::from_str(&backend);
        } else {
            // Auto mode, we choose the best compiler for that platform
            if cfg!(feature = "compiler-cranelift") && cfg!(target_arch = "x86_64") {
                return Ok(Compiler::Cranelift);
            } else if cfg!(feature = "compiler-singlepass") && cfg!(target_arch = "x86_64") {
                return Ok(Compiler::Singlepass);
            } else if cfg!(feature = "compiler-llvm") {
                return Ok(Compiler::LLVM);
            } else {
                bail!("There are no available compilers for your architecture")
            }
        }
    }

    /// Get the Compiler Config for the current options
    fn get_config(&self, compiler: Compiler) -> Result<Box<dyn CompilerConfig>> {
        let config: Box<dyn CompilerConfig> = match compiler {
            #[cfg(feature = "compiler-singlepass")]
            Compiler::Singlepass => {
                let config = SinglepassConfig::default();
                Box::new(config)
            }
            #[cfg(feature = "compiler-cranelift")]
            Compiler::Cranelift => {
                let config = CraneliftConfig::default();
                Box::new(config)
            }
            #[cfg(feature = "compiler-llvm")]
            Compiler::LLVM => {
                let config = LLVMConfig::default();
                Box::new(config)
            }
            #[cfg(not(all(
                feature = "compiler-singlepass",
                feature = "compiler-cranelift",
                feature = "compiler-llvm",
            )))]
            compiler => bail!(
                "The `{}` compiler is not included in this binary.",
                compiler.to_string()
            ),
        };
        return Ok(config);
    }

    /// Get's the store
    pub fn get_store(&self) -> Result<(Store, String)> {
        let compiler = self.get_compiler()?;
        let compiler_name = compiler.to_string();
        let compiler_config = self.get_config(compiler)?;
        let tunables = Tunables::for_target(compiler_config.target().triple());
        let engine = Engine::new(&*compiler_config, tunables);
        let store = Store::new(&engine);
        Ok((store, compiler_name))
    }
}
