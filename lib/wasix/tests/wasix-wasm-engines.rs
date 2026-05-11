#[cfg(all(unix, not(feature = "js")))]
#[allow(dead_code, unused_imports)]
mod wasm_tests;

#[cfg(all(unix, not(feature = "js")))]
use libtest_mimic::{Arguments, Failed, Trial};

#[cfg(all(unix, not(feature = "js")))]
fn failed(error: anyhow::Error) -> Failed {
    Failed::from(error.to_string())
}

#[cfg(all(unix, not(feature = "js")))]
fn test_helloworld(engine: wasm_tests::WasmTestEngine) -> Result<(), Failed> {
    let wasm = wasm_tests::run_build_script("basic_tests.rs", "helloworld").map_err(failed)?;
    let result = wasm_tests::run_wasm_with_runner_config_and_engine(
        &wasm,
        wasm.parent().unwrap(),
        engine,
        |_| {},
    )
    .map_err(failed)?;

    wasm_tests::ensure_wasm_run_succeeded(&result).map_err(failed)
}

#[cfg(all(unix, not(feature = "js")))]
fn main() -> std::process::ExitCode {
    let args = Arguments::from_args();
    let tests = [
        wasm_tests::WasmTestEngine::Llvm,
        wasm_tests::WasmTestEngine::Cranelift,
    ]
    .into_iter()
    .map(|engine| {
        Trial::test(
            format!("wasix_wasm::basic_tests::helloworld::{}", engine.name()),
            move || test_helloworld(engine),
        )
    })
    .collect();

    libtest_mimic::run(&args, tests).exit_code()
}

#[cfg(not(all(unix, not(feature = "js"))))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::SUCCESS
}
