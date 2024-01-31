use ::wasmer::sys::Features;
use std::path::Path;
use wasmer_wast::Wast;

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
include!(concat!(env!("OUT_DIR"), "/generated_spectests.rs"));

pub fn run_wast(mut config: crate::Config, wast_path: &str) -> anyhow::Result<()> {
    println!("Running wast `{}`", wast_path);
    let try_nan_canonicalization = wast_path.contains("nan-canonicalization");
    let mut features = Features::default();
    let is_bulkmemory = wast_path.contains("bulk-memory");
    let is_simd = wast_path.contains("simd");
    let is_threads = wast_path.contains("threads");
    if is_bulkmemory {
        features.bulk_memory(true);
    }
    if is_simd {
        features.simd(true);
    }
    if is_threads {
        features.threads(true);
    }
    if config.compiler == crate::Compiler::Singlepass {
        features.multi_value(false);
    }
    config.set_features(features);
    config.set_nan_canonicalization(try_nan_canonicalization);

    let store = config.store();
    let mut wast = Wast::new_with_spectest(store);
    // `bulk-memory-operations/bulk.wast` checks for a message that
    // specifies which element is uninitialized, but our traps don't
    // shepherd that information out.
    wast.allow_trap_message("uninitialized element 2", "uninitialized element");
    // `liking.wast` has different wording but the same meaning
    wast.allow_trap_message("out of bounds memory access", "memory out of bounds");
    if cfg!(feature = "coverage") {
        wast.disable_assert_and_exhaustion();
    }
    if is_simd {
        // We allow this, so tests can be run properly for `simd_const` test.
        wast.allow_instantiation_failures(&[
            "Validation error: multiple tables",
            "Validation error: unknown memory 0",
            "Validation error: Invalid var_u32",
        ]);
    }
    if is_threads {
        // We allow this, so tests can be run properly for `simd_const` test.
        wast.allow_instantiation_failures(&["Validation error: multiple tables"]);
    }
    if config.compiler == crate::Compiler::Singlepass {
        // We don't support multivalue yet in singlepass
        wast.allow_instantiation_failures(&[
            "Validation error: invalid result arity: func type returns multiple values",
            "Validation error: blocks, loops, and ifs may only produce a resulttype when multi-value is not enabled",
            "Validation error: func type returns multiple values but the multi-value feature is not enabled",
        ]);
    }
    wast.fail_fast = false;
    let path = Path::new(wast_path);
    wast.run_file(path)
}
