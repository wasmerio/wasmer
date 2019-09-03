#![no_main]
#[macro_use] extern crate libfuzzer_sys;

extern crate wasmer_runtime_core;
extern crate wasmer;

use wasmer_runtime_core::{
    backend::{Features},
};

fuzz_target!(|data: &[u8]| {
            let _ = wasmer::utils::is_wasm_binary(data);
            let _ = wasmer_runtime_core::validate_and_report_errors_with_features(
                &data,
                Features {
                    // modify those values to explore additionnal part of wasmer
                    simd: false, threads: false,              },
            );
});
