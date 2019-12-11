#[test]
#[ignore]
fn test_legacy_exported_runtime_numbers() {
    assert_emscripten_output!(
        "../../emtests/legacy_exported_runtime_numbers.wasm",
        "legacy_exported_runtime_numbers",
        vec![],
        "../../emtests/legacy_exported_runtime_numbers.txt"
    );
}
