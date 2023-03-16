use wasmer_cli::cli::wasmer_main;

#[cfg(not(feature = "jsc"))]
compile_error!("You need to enable the `jsc` feature\n\n\n");

fn main() {
    wasmer_main();
}
