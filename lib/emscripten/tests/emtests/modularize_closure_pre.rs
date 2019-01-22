#[test]
#[ignore]
fn test_modularize_closure_pre() {
    assert_emscripten_output!(
        "../../emtests/modularize_closure_pre.wasm",
        "modularize_closure_pre",
        vec![],
        "../../emtests/modularize_closure_pre.out"
    );
}
