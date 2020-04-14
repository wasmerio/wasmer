use std::path::Path;

#[cfg(not(any(
    feature = "backend-llvm",
    feature = "backend-cranelift",
    feature = "backend-singlepass"
)))]
compile_error!("No compiler backend detected: please specify at least one compiler backend!");
use anyhow::bail;
use wasmer::compiler::Backend;
use wasmer_wast::Wast;

// #[cfg(test)]
// mod spectests {
//     mod cranelift {
//         #[test]
//         fn address() -> Result<(), String> {
//             crate::run_wast("tests/spectests/address.wast", "llvm")
//         }
//     }
// }
include!(concat!(env!("OUT_DIR"), "/generated_tests.rs"));

fn run_wast(wast_path: &str, backend: &str) -> anyhow::Result<()> {
    let backend = match backend {
        #[cfg(feature = "backend-singlepass")]
        "singlepass" => Backend::Singlepass,
        #[cfg(feature = "backend-cranelift")]
        "cranelift" => Backend::Cranelift,
        #[cfg(feature = "backend-llvm")]
        "llvm" => Backend::LLVM,
        _ => bail!("Backend {} not found", backend),
    };
    let mut wast = Wast::new_with_spectest(backend);
    let path = Path::new(wast_path);
    wast.run_file(path)
}
