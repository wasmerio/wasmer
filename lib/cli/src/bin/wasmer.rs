use wasmer_cli::cli::wasmer_main;

#[cfg(not(any(feature = "cranelift", feature = "singlepass", feature = "llvm")))]
compile_error!(
    "Either enable at least one compiler, or compile the wasmer-headless binary instead.\nWith cargo, you can provide a compiler option with the --features flag.\n\nExample values:\n\n\t\t--features compiler,cranelift\n\t\t--features compiler,cranelift,singlepass\n\n\n"
);

fn main() {
    wasmer_main();
}
