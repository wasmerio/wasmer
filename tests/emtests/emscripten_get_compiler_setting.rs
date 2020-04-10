#[test]
#[ignore]
fn test_emscripten_get_compiler_setting() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/emscripten_get_compiler_setting.wasm",
        "emscripten_get_compiler_setting",
        vec![],
        "../emscripten_resources/emtests/emscripten_get_compiler_setting.out"
    );
}
