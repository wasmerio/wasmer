#[test]
#[ignore]
fn test_test_i16_emcc_intrinsic() {
    assert_emscripten_output!(
        "../emscripten_resources/emtests/test_i16_emcc_intrinsic.wasm",
        "test_i16_emcc_intrinsic",
        vec![],
        "../emscripten_resources/emtests/test_i16_emcc_intrinsic.out"
    );
}
