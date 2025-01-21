use std::fs::File;
use std::io::Read;
use wasmer_wast::{WasiFileSystemKind, WasiTest};

// The generated tests (from build.rs) look like:
// #[cfg(test)]
// mod [compiler] {
//     mod [spec] {
//         mod [vfs] {
//             #[test]
//             fn [test_name]() -> anyhow::Result<()> {
//                 crate::run_wasi("tests/spectests/[test_name].wast", "[compiler]", WasiFileSystemKind::[vfs])
//             }
//         }
//     }
// }
include!(concat!(env!("OUT_DIR"), "/generated_wasitests.rs"));

pub fn run_wasi(
    config: crate::Config,
    wast_path: &str,
    base_dir: &str,
    filesystem_kind: WasiFileSystemKind,
) -> anyhow::Result<()> {
    println!("Running wasi wast `{wast_path}`");
    let mut store = config.store();

    let source = {
        let mut out = String::new();
        let mut f = File::open(wast_path)?;
        f.read_to_string(&mut out)?;
        out
    };
    let tokens = WasiTest::lex_string(&source)?;
    let wasi_test = WasiTest::parse_tokens(&tokens)?;

    let succeeded = wasi_test.run(&mut store, base_dir, filesystem_kind)?;

    assert!(succeeded);

    Ok(())
}
