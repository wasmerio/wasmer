#![cfg(all(feature = "compiler", feature = "engine"))]

use crate::utils::get_compiler;
use std::fs::File;
use std::io::Read;
use wasmer::{Features, Store};
#[cfg(feature = "jit")]
use wasmer_engine_jit::JIT;
use wasmer_wast::WasiTest;

// The generated tests (from build.rs) look like:
// #[cfg(test)]
// mod singlepass {
//     mod spec {
//         #[test]
//         fn address() -> anyhow::Result<()> {
//             crate::run_wast("tests/spectests/address.wast", "singlepass")
//         }
//     }
// }
include!(concat!(env!("OUT_DIR"), "/generated_wasitests.rs"));

pub fn run_wasi(wast_path: &str, base_dir: &str, compiler: &str) -> anyhow::Result<()> {
    println!(
        "Running wasi wast `{}` with the {} compiler",
        wast_path, compiler
    );
    let compiler_config = get_compiler(true);
    let store = Store::new(&JIT::new(&compiler_config).engine());

    let source = {
        let mut out = String::new();
        let mut f = File::open(wast_path)?;
        f.read_to_string(&mut out)?;
        out
    };
    let tokens = WasiTest::lex_string(&source)?;
    let wasi_test = WasiTest::parse_tokens(&tokens)?;

    let succeeded = wasi_test.run(&store, base_dir)?;
    assert!(succeeded);

    Ok(())
}
