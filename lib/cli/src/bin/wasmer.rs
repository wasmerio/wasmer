use wasmer_cli::cli::wasmer_main;

#[cfg(not(any(feature = "cranelift", feature = "singlepass", feature = "llvm")))]
compile_error!(
    "Either enable at least one compiler, or compile the wasmer-headless binary instead"
);

fn main() {
    wasmer_main();
}
