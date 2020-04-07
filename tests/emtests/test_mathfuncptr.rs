#[test]
#[ignore]
fn test_test_mathfuncptr() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_mathfuncptr.wasm",
        "test_mathfuncptr",
        vec![],
        "../emscripten_resources/emtests/test_mathfuncptr.out"
    );
}
