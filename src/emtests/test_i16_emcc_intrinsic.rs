#[test]
#[ignore]
fn test_test_i16_emcc_intrinsic() {
    assert_emscripten_output!(
        "../../emtests/test_i16_emcc_intrinsic.wasm",
        "test_i16_emcc_intrinsic",
        vec![],
        "../../emtests/test_i16_emcc_intrinsic.out"
    );
}
