#[test]
fn test_localtime() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/localtime.wasm",
        "localtime",
        vec![],
        "../emscripten_resources/emtests/localtime.out"
    );
}
