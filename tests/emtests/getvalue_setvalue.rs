#[test]
#[ignore]
fn test_getvalue_setvalue() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/getValue_setValue.wasm",
        "getvalue_setvalue",
        vec![],
        "../emscripten_resources/emtests/getValue_setValue.txt"
    );
}
