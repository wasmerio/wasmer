use wasmer_compiler_cli::cli::wasmer_main;

#[cfg(not(any(feature = "cranelift", feature = "singlepass", feature = "llvm")))]
compile_error!(
    "Either enable at least one compiler, or compile the wasmer-headless binary instead"
);

#[cfg(featue = "run")]
compile_error!("Cannot enable run with the compile-only build");

fn main() {
    wasmer_main();
}
