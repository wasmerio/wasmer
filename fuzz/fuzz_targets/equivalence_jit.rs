#![no_main]

use anyhow::Result;
use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{imports, CompilerConfig, Instance, Module, Store, Val};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_compiler_llvm::LLVM;
use wasmer_compiler_singlepass::Singlepass;
use wasmer_engine_jit::JIT;

#[derive(Arbitrary, Debug, Default, Copy, Clone)]
struct ExportedFunctionConfig;
impl Config for ExportedFunctionConfig {
    fn max_imports(&self) -> usize {
        0
    }
    fn max_memory_pages(&self) -> u32 {
        // https://github.com/wasmerio/wasmer/issues/2187
        65535
    }
    fn min_funcs(&self) -> usize {
        1
    }
    fn min_exports(&self) -> usize {
        1
    }
}

fn maybe_instantiate_singlepass(wasm_bytes: &[u8]) -> Result<Option<Instance>> {
    let compiler = Singlepass::default();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(&store, &wasm_bytes);
    let module = match module {
        Ok(m) => m,
        Err(e) => {
            let error_message = format!("{}", e);
            if error_message.contains("Validation error: invalid result arity: func type returns multiple values") || error_message.contains("Validation error: blocks, loops, and ifs accept no parameters when multi-value is not enabled") || error_message.contains("multi-value returns not yet implemented") {
                return Ok(None);
            }
            return Err(e.into());
        }
    };
    let instance = Instance::new(&module, &imports! {})?;
    Ok(Some(instance))
}

fn maybe_instantiate_cranelift(wasm_bytes: &[u8]) -> Result<Option<Instance>> {
    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(&store, &wasm_bytes)?;
    let instance = Instance::new(&module, &imports! {})?;
    Ok(Some(instance))
}

fn maybe_instantiate_llvm(wasm_bytes: &[u8]) -> Result<Option<Instance>> {
    let mut compiler = LLVM::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let store = Store::new(&JIT::new(compiler).engine());
    let module = Module::new(&store, &wasm_bytes)?;
    let instance = Instance::new(&module, &imports! {})?;
    Ok(Some(instance))
}

#[derive(Debug)]
enum InstanceResult {
    Error(String),
    Values(Vec<Val>),
}

impl PartialEq for InstanceResult {
    fn eq(&self, other: &Self) -> bool {
        match self {
            InstanceResult::Error(self_message) => {
                if let InstanceResult::Error(other_message) = other {
                    return self_message == other_message;
                }
                return false;
            }
            InstanceResult::Values(self_values) => {
                if let InstanceResult::Values(other_values) = other {
                    return self_values == other_values;
                }
                return false;
            }
        }
    }
}

impl Eq for InstanceResult {}

fn evaluate_instance(instance: Result<Instance>) -> Vec<InstanceResult> {
    let mut results = vec![];

    if let Err(err) = instance {
        let mut error_message = format!("{}", err);
        // Remove the stack trace.
        if error_message.starts_with("RuntimeError: unreachable\n") {
            error_message = "RuntimeError: unreachable\n".into();
        }
        results.push(InstanceResult::Error(error_message));
    } else {
        let instance = instance.unwrap();
        for it in instance.exports.iter().functions() {
            let (_, f) = it;
            // TODO: support functions which take params.
            if f.ty().params().is_empty() {
                let result = f.call(&[]);
                let result = if result.is_ok() {
                    let values = result.unwrap();
                    InstanceResult::Values(values.into())
                } else {
                    let err = result.unwrap_err();
                    let error_message = err.message();
                    InstanceResult::Error(error_message)
                };
                results.push(result);
            }
        }
    }
    results
}

fuzz_target!(|module: ConfiguredModule<ExportedFunctionConfig>| {
    let mut module = module;
    module.ensure_termination(100000);
    let wasm_bytes = module.to_bytes();

    let singlepass = maybe_instantiate_singlepass(&wasm_bytes).transpose().map(evaluate_instance);
    let cranelift = maybe_instantiate_cranelift(&wasm_bytes).transpose().map(evaluate_instance);
    let llvm = maybe_instantiate_llvm(&wasm_bytes).transpose().map(evaluate_instance);

    if singlepass.is_some() && cranelift.is_some() {
        assert_eq!(singlepass.as_ref().unwrap(), cranelift.as_ref().unwrap());
    }
    if singlepass.is_some() && llvm.is_some() {
        assert_eq!(singlepass.as_ref().unwrap(), llvm.as_ref().unwrap());
    }
    if cranelift.is_some() && llvm.is_some() {
        assert_eq!(cranelift.as_ref().unwrap(), llvm.as_ref().unwrap());
    }
});
