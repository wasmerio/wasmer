use wasmer_runtime_core::{
    backend::{Compiler, Token},
    error::CompileError,
    module::{ModuleInfo, ModuleInner},
};

mod code;
mod intrinsics;
mod read_info;
mod state;

pub struct LLVMCompiler {
    _private: (),
}

impl LLVMCompiler {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Compiler for LLVMCompiler {
    fn compile(&self, wasm: &[u8], _: Token) -> Result<ModuleInner, CompileError> {
        let (_info, _code_reader) = read_info::read_module(wasm).unwrap();

        unimplemented!()
    }
}

#[test]
fn test_read_module() {
    use wabt::wat2wasm;
    let WAT: &'static str = r#"
        (module
        (type $t0 (func (param i32) (result i32)))
        (import "env" "memory" (memory 1 1))
        (import "env" "table" (table 10 anyfunc))
        (import "env" "global" (global i32))
        (import "env" "print_i32" (func $print_i32 (type $t0)))
        (func $identity (type $t0) (param $p0 i32) (result i32)
            get_local $p0)
        (func $print_num (export "print_num") (type $t0) (param $p0 i32) (result i32)
            get_global 0
            call $identity
            call $print_i32))
    "#;
    let wasm = wat2wasm(WAT).unwrap();

    let (info, code_reader) = read_info::read_module(&wasm).unwrap();

    code::parse_function_bodies(&info, code_reader).unwrap();
}
