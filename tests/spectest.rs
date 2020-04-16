mod utils;

use std::path::Path;

#[cfg(not(any(
    feature = "backend-llvm",
    feature = "backend-cranelift",
    feature = "backend-singlepass"
)))]
compile_error!("No compiler backend detected: please specify at least one compiler backend!");

use wasmer_wast::Wast;

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
include!(concat!(env!("OUT_DIR"), "/generated_spectests.rs"));

fn run_wast(wast_path: &str, backend: &str) -> anyhow::Result<()> {
    println!("Running wast {} with {}", wast_path, backend);
    let backend = utils::get_backend_from_str(backend)?;
    let mut wast = Wast::new_with_spectest(backend);
    let path = Path::new(wast_path);
    wast.run_file(path)
}
