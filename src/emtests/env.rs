#[test]
fn test_env() {
    assert_emscripten_output!(
        "../../emtests/env.wasm",
        "env",
        vec![],
        "../../emtests/env.output"
    );
}
