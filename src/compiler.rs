//! Common module with common used structures across different
//! commands.

use crate::common::WasmFeatures;
use anyhow::{bail, Result};
use structopt::StructOpt;
use wasmer::*;

#[derive(Debug, Clone, StructOpt)]
/// The compiler options
pub struct CompilerOptions {
    /// Use Singlepass compiler
    #[structopt(long, conflicts_with_all = &["cranelift", "llvm"])]
    singlepass: bool,

    /// Use Cranelift compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "llvm"])]
    cranelift: bool,

    /// Use LLVM compiler
    #[structopt(long, conflicts_with_all = &["singlepass", "cranelifft"])]
    llvm: bool,

    #[structopt(flatten)]
    features: WasmFeatures,
}

#[derive(Debug)]
enum Compiler {
    Singlepass,
    Cranelift,
    LLVM,
}

impl CompilerOptions {
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
    pub fn get_config(&self) -> Result<Box<dyn CompilerConfig>> {
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
            #[cfg(not(all(
                feature = "compiler-singlepass",
                feature = "compiler-cranelift",
                feature = "compiler-llvm",
            )))]
            compiler => bail!(
                "The compiler {:?} is not included in this binary.",
                compiler
            ),
        }
    }
}
