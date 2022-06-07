use wasmer_compiler_cli::cli::wasmer_main;

#[cfg(feature = "run")]
compile_error!("Cannot enable run with the compile-only build");

fn main() {
    wasmer_main();
}
