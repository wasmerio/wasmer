#[test]
#[ignore]
fn test_test_uname() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_uname.wasm",
        "test_uname",
        vec![],
        "../emscripten_resources/emtests/test_uname.out"
    );
}
