use std::fs::File;
use std::io::Read;
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

pub fn run_wasi(config: crate::Config, wast_path: &str, base_dir: &str) -> anyhow::Result<()> {
    println!("Running wasi wast `{}`", wast_path);
    let store = config.store();

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
