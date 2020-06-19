(wasi_test "quine.wasm"
  (preopens ".")
  (assert_return (i64.const 0))
  (assert_stdout "// WASI:\n// dir: .\n\nuse std::fs;\nuse std::io::Read;\n\nfn main() {\n    let mut this_file = fs::File::open(\"tests/quine.rs\").expect(\"could not find src file\");\n    let md = this_file.metadata().unwrap();\n    let mut in_str = String::new();\n    this_file.read_to_string(&mut in_str).unwrap();\n    println!(\"{}\", in_str);\n}\n\n")
)