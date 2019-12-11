#[test]
#[ignore]
fn test_test_mathfuncptr() {
    assert_emscripten_output!(
        "../../emtests/test_mathfuncptr.wasm",
        "test_mathfuncptr",
        vec![],
        "../../emtests/test_mathfuncptr.out"
    );
}
