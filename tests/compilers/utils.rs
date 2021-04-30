use std::sync::Arc;
use wasmer::{ModuleMiddleware, Store};
use wasmer_compiler::CompilerConfig;
use wasmer_engine::Engine;
#[cfg(feature = "test-jit")]
use wasmer_engine_jit::JIT;
#[cfg(feature = "test-native")]
use wasmer_engine_native::Native;

pub fn get_compiler(canonicalize_nans: bool, compiler: &str) -> Box<dyn CompilerConfig> {
    #[cfg(not(any(
        feature = "test-cranelift",
        feature = "test-llvm",
        feature = "test-singlepass"
    )))]
    compile_error!("No compiler chosen for the tests");
    match compiler {
        #[cfg(feature = "test-cranelift")]
        "cranelift" => {
            let mut compiler = wasmer_compiler_cranelift::Cranelift::new();
            compiler.canonicalize_nans(canonicalize_nans);
            compiler.enable_verifier();
            Box::new(compiler)
        }
        #[cfg(feature = "test-llvm")]
        "llvm" => {
            let mut compiler = wasmer_compiler_llvm::LLVM::new();
            compiler.canonicalize_nans(canonicalize_nans);
            compiler.enable_verifier();
            Box::new(compiler)
        }
        #[cfg(feature = "test-singlepass")]
        "singlepass" => {
            let mut compiler = wasmer_compiler_singlepass::Singlepass::new();
            compiler.canonicalize_nans(canonicalize_nans);
            compiler.enable_verifier();
            Box::new(compiler)
        }
        _ => panic!("Unsupported compiler `{}`!", compiler),
    }
}

const ENABLED_COMPILERS: &'static [&str] = &[
    #[cfg(feature = "test-cranelift")]
    "cranelift",
    #[cfg(feature = "test-llvm")]
    "llvm",
    #[cfg(feature = "test-singlepass")]
    "singlepass",
];

pub fn get_compilers() -> &'static [&'static str] {
    &ENABLED_COMPILERS[..]
}

#[cfg(feature = "test-jit")]
pub fn get_engine(canonicalize_nans: bool, compiler: &str) -> impl Engine {
    let compiler_config = get_compiler(canonicalize_nans, compiler);
    JIT::new(compiler_config).engine()
}
#[cfg(feature = "test-native")]
pub fn get_engine(canonicalize_nans: bool) -> impl Engine {
    let compiler_config = get_compiler(canonicalize_nans);
    Native::new(compiler_config).engine()
}

pub fn get_store(canonicalize_nans: bool, compiler: &str) -> Store {
    Store::new(&get_engine(canonicalize_nans, compiler))
}

pub fn get_store_with_middlewares<I: Iterator<Item = Arc<dyn ModuleMiddleware>>>(
    middlewares: I,
    compiler: &str,
) -> Store {
    let mut compiler_config = get_compiler(false, compiler);
    for x in middlewares {
        compiler_config.push_middleware(x);
    }
    #[cfg(feature = "test-jit")]
    let engine = JIT::new(compiler_config).engine();
    #[cfg(feature = "test-native")]
    let engine = Native::new(compiler_config).engine();
    Store::new(&engine)
}

#[cfg(feature = "test-jit")]
pub fn get_headless_store() -> Store {
    Store::new(&JIT::headless().engine())
}

#[cfg(feature = "test-native")]
pub fn get_headless_store() -> Store {
    Store::new(&Native::headless().engine())
}
