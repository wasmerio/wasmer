#![no_main]
#![deny(unused_variables)]

use anyhow::Result;
use libfuzzer_sys::{arbitrary, arbitrary::Arbitrary, fuzz_target};
use wasm_smith::{Config, ConfiguredModule};
use wasmer::{imports, CompilerConfig, EngineBuilder, Instance, Module, Store, Val};
#[cfg(feature = "cranelift")]
use wasmer_compiler_cranelift::Cranelift;
#[cfg(feature = "llvm")]
use wasmer_compiler_llvm::LLVM;
#[cfg(feature = "singlepass")]
use wasmer_compiler_singlepass::Singlepass;

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

struct WasmSmithModule(ConfiguredModule<ExportedFunctionConfig>);
impl<'a> arbitrary::Arbitrary<'a> for WasmSmithModule {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let mut module = ConfiguredModule::<ExportedFunctionConfig>::arbitrary(u)?;
        module.ensure_termination(100000);
        Ok(WasmSmithModule(module))
    }
}
impl std::fmt::Debug for WasmSmithModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&wasmprinter::print_bytes(self.0.to_bytes()).unwrap())
    }
}

#[cfg(feature = "singlepass")]
fn maybe_instantiate_singlepass(wasm_bytes: &[u8]) -> Result<Option<Instance>> {
    let compiler = Singlepass::default();
    let mut store = Store::new(compiler);
    let module = Module::new(&store, &wasm_bytes);
    let module = match module {
        Ok(m) => m,
        Err(e) => {
            let error_message = format!("{}", e);
            if error_message.contains("Validation error: invalid result arity: func type returns multiple values") || error_message.contains("Validation error: blocks, loops, and ifs may only produce a resulttype when multi-value is not enabled") || error_message.contains("multi-value returns not yet implemented") {
                return Ok(None);
            }
            return Err(e.into());
        }
    };
    let instance = Instance::new(&module, &imports! {})?;
    Ok(Some(instance))
}

#[cfg(feature = "cranelift")]
fn maybe_instantiate_cranelift(wasm_bytes: &[u8]) -> Result<Option<Instance>> {
    let mut compiler = Cranelift::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let mut store = Store::new(compiler);
    let module = Module::new(&store, &wasm_bytes)?;
    let instance = Instance::new(&module, &imports! {})?;
    Ok(Some(instance))
}

#[cfg(feature = "llvm")]
fn maybe_instantiate_llvm(wasm_bytes: &[u8]) -> Result<Option<Instance>> {
    let mut compiler = LLVM::default();
    compiler.canonicalize_nans(true);
    compiler.enable_verifier();
    let mut store = Store::new(compiler);
    let module = Module::new(&store, &wasm_bytes)?;
    let instance = Instance::new(&module, &imports! {})?;
    Ok(Some(instance))
}

#[derive(Debug)]
enum FunctionResult {
    Error(String),
    Values(Vec<Val>),
}

#[derive(Debug, PartialEq, Eq)]
enum InstanceResult {
    Error(String),
    Functions(Vec<FunctionResult>),
}

impl PartialEq for FunctionResult {
    fn eq(&self, other: &Self) -> bool {
        /*
        match self {
            FunctionResult::Error(self_message) => {
                if let FunctionResult::Error(other_message) = other {
                    return self_message == other_message;
                }
            }
            FunctionResult::Values(self_values) => {
                if let FunctionResult::Values(other_values) = other {
                    return self_values == other_values;
                }
            }
        }
        false
         */
        match (self, other) {
            (FunctionResult::Values(self_values), FunctionResult::Values(other_values)) => {
                self_values.len() == other_values.len()
                    && self_values
                        .iter()
                        .zip(other_values.iter())
                        .all(|(x, y)| match (x, y) {
                            (Val::F32(x), Val::F32(y)) => x.to_bits() == y.to_bits(),
                            (Val::F64(x), Val::F64(y)) => x.to_bits() == y.to_bits(),
                            _ => x == y,
                        })
            }
            _ => true,
        }
    }
}

impl Eq for FunctionResult {}

fn evaluate_instance(instance: Result<Instance>) -> InstanceResult {
    if let Err(_err) = instance {
        /*let mut error_message = format!("{}", err);
        // Remove the stack trace.
        if error_message.starts_with("RuntimeError: unreachable\n") {
            error_message = "RuntimeError: unreachable\n".into();
        }
        InstanceResult::Error(error_message)*/
        InstanceResult::Error("".into())
    } else {
        let instance = instance.unwrap();
        let mut results = vec![];
        for it in instance.exports.iter().functions() {
            let (_, f) = it;
            // TODO: support functions which take params.
            if f.ty().params().is_empty() {
                let result = f.call(&[]);
                let result = if let Ok(values) = result {
                    FunctionResult::Values(values.into())
                } else {
                    /*
                    let err = result.unwrap_err();
                    let error_message = err.message();
                    FunctionResult::Error(error_message)
                     */
                    FunctionResult::Error("".into())
                };
                results.push(result);
            }
        }
        InstanceResult::Functions(results)
    }
}

fuzz_target!(|module: WasmSmithModule| {
    let wasm_bytes = module.0.to_bytes();

    if let Ok(path) = std::env::var("DUMP_TESTCASE") {
        use std::fs::File;
        use std::io::Write;
        let mut file = File::create(path).unwrap();
        file.write_all(&wasm_bytes).unwrap();
        return;
    }

    #[cfg(feature = "singlepass")]
    let singlepass = maybe_instantiate_singlepass(&wasm_bytes)
        .transpose()
        .map(evaluate_instance);
    #[cfg(feature = "cranelift")]
    let cranelift = maybe_instantiate_cranelift(&wasm_bytes)
        .transpose()
        .map(evaluate_instance);
    #[cfg(feature = "llvm")]
    let llvm = maybe_instantiate_llvm(&wasm_bytes)
        .transpose()
        .map(evaluate_instance);

    #[cfg(all(feature = "singlepass", feature = "cranelift"))]
    if singlepass.is_some() && cranelift.is_some() {
        assert_eq!(singlepass.as_ref().unwrap(), cranelift.as_ref().unwrap());
    }
    #[cfg(all(feature = "singlepass", feature = "llvm"))]
    if singlepass.is_some() && llvm.is_some() {
        assert_eq!(singlepass.as_ref().unwrap(), llvm.as_ref().unwrap());
    }
    #[cfg(all(feature = "cranelift", feature = "llvm"))]
    if cranelift.is_some() && llvm.is_some() {
        assert_eq!(cranelift.as_ref().unwrap(), llvm.as_ref().unwrap());
    }
});
