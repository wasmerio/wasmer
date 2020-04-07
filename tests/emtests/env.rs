#[test]
fn test_env() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/env.wasm",
        "env",
        vec![],
        "../emscripten_resources/emtests/env.out"
    );
}
