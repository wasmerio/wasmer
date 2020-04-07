#[test]
#[ignore]
fn test_modularize_closure_pre() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/modularize_closure_pre.wasm",
        "modularize_closure_pre",
        vec![],
        "../emscripten_resources/emtests/modularize_closure_pre.out"
    );
}
