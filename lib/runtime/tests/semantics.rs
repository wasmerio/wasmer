#[cfg(test)]
mod tests {
    use wabt::wat2wasm;
    use wasmer_clif_backend::CraneliftCompiler;
    use wasmer_runtime::import::Imports;

    // The semantics of stack overflow are documented at:
    // https://webassembly.org/docs/semantics/#stack-overflow
    #[test]
    #[ignore]
    fn test_stack_overflow() {
        let module_str = r#"(module
      (type (;0;) (func))
      (func (;0;) (type 0)
        i32.const 0
        call_indirect (type 0))
      (table (;0;) 1 anyfunc)
      (export "stack-overflow" (func 0))
      (elem (;0;) (i32.const 0) 0))
    "#;
        let wasm_binary = wat2wasm(module_str.as_bytes()).expect("WAST not valid or malformed");
        let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new())
            .expect("WASM can't be compiled");
        let mut instance = module
            .instantiate(&Imports::new())
            .expect("WASM can't be instantiated");
        let result = instance.call("stack-overflow", &[]);
        assert!(
            result.is_err(),
            "should fail with error due to stack overflow"
        );
        // TODO The kind of error and message needs to be defined, not spec defined, maybe RuntimeError or RangeError
        if let Err(message) = result {
            assert!(!message.contains("segmentation violation"));
            assert!(!message.contains("bus error"));
        }
    }

}
