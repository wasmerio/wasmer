#![cfg(test)]

#[macro_use]
mod utils;

static TEST_WAT: &str = r#"
(module
  (import "env" "outside-global" (global $outside-global (mut i32)))
  (import "env" "update-func" (func $update-func (param i32) (result i32)))
  (table $test-table (export "test-table") 2 anyfunc)
  (global $test-global (export "test-global") (mut i32) (i32.const 3))
  (elem (;0;) (i32.const 0) $ret_2)
  (func $ret_2 (export "ret_2") (result i32)
    i32.const 2)
  (func $ret_4 (export "ret_4") (result i32)
    i32.const 4)
  (func $set_test_global (export "set_test_global") (param i32)
    (global.set $test-global
                (local.get 0)))
  (func $update-outside-global (export "update_outside_global")
    (global.set $outside-global
                (call $update-func (global.get $outside-global))))
)
"#;

fn append_custom_section(
    wasm: &mut Vec<u8>,
    custom_section_name: &str,
    custom_section_contents: &[u8],
) {
    wasm.reserve(
        // 1 for custom section id, 5 for max length of custom section, 5 for max length of name
        custom_section_name.len() + custom_section_contents.len() + 1 + 5 + 5,
    );

    wasm.push(0);

    let name_length = custom_section_name.as_bytes().len() as u32;
    let mut name_length_as_leb128 = vec![];
    write_u32_leb128(&mut name_length_as_leb128, name_length);

    let custom_section_length = (custom_section_contents.len()
        + custom_section_name.as_bytes().len()
        + name_length_as_leb128.len()) as u32;

    let mut custom_section_length_as_leb128 = vec![];
    write_u32_leb128(&mut custom_section_length_as_leb128, custom_section_length);

    wasm.extend(&custom_section_length_as_leb128);
    wasm.extend(&name_length_as_leb128);
    wasm.extend(custom_section_name.as_bytes());
    wasm.extend(custom_section_contents);
}

wasmer_backends! {
    use super::{TEST_WAT, append_custom_section};

    #[test]
    fn custom_section_parsing_works() {
        use wasmer::{CompiledModule, Module};
        let wasm = {
            let mut wasm = wabt::wat2wasm(TEST_WAT).unwrap();
            append_custom_section(&mut wasm, "test_custom_section", b"hello, world!");
            append_custom_section(&mut wasm, "test_custom_section", b"goodbye, world!");
            append_custom_section(&mut wasm, "different_name", b"different value");
            wasm
        };

        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();

        assert_eq!(
            module.custom_sections("test_custom_section"),
            Some(&[b"hello, world!".to_vec(), b"goodbye, world!".to_vec()][..])
        );
    }

    #[test]
    fn module_exports_are_ordered() {
        use wasmer::types::{ElementType, FuncSig, GlobalType, TableType, Type};
        use wasmer::{export, CompiledModule, Module};

        let wasm = wabt::wat2wasm(TEST_WAT).unwrap();
        // TODO: review error messages when `CompiledModule` is not in scope. My hypothesis is that they'll be
        // misleading, if so we may want to do something about it.
        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();
        let exports = module.exports();
        assert_eq!(
            exports,
            vec![
                export::ExportType {
                    name: "test-table",
                    ty: export::ExternType::Table(TableType {
                        element: ElementType::Anyfunc,
                        minimum: 2,
                        maximum: None,
                    }),
                },
                export::ExportType {
                    name: "test-global",
                    ty: export::ExternType::Global(GlobalType {
                        mutable: true,
                        ty: Type::I32,
                    }),
                },
                export::ExportType {
                    name: "ret_2",
                    ty: export::ExternType::Function(FuncSig::new(vec![], vec![Type::I32])),
                },
                export::ExportType {
                    name: "ret_4",
                    ty: export::ExternType::Function(FuncSig::new(vec![], vec![Type::I32])),
                },
                export::ExportType {
                    name: "set_test_global",
                    ty: export::ExternType::Function(FuncSig::new(vec![Type::I32], vec![])),
                },
                export::ExportType {
                    name: "update_outside_global",
                    ty: export::ExternType::Function(FuncSig::new(vec![], vec![])),
                },
            ]
        );
    }

    #[test]
    fn module_imports_are_correct() {
        use wasmer::{CompiledModule, Module};

        let wasm = wabt::wat2wasm(TEST_WAT).unwrap();
        // TODO: review error messages when `CompiledModule` is not in scope. My hypothesis is that they'll be
        // misleading, if so we may want to do something about it.
        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();

        // TODO: test this more later
        let imports = module.imports();
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn module_can_be_instantiated() {
        use wasmer::wasm::{Global, Value};
        use wasmer::{func, imports, CompiledModule, Module};

        let wasm = wabt::wat2wasm(TEST_WAT).unwrap();
        // TODO: review error messages when `CompiledModule` is not in scope. My hypothesis is that they'll be
        // misleading, if so we may want to do something about it.
        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();

        let outside_global = Global::new_mutable(Value::I32(15));
        let import_object = imports! {
            "env" => {
                "update-func" => func!(|x: i32| x * 2),
                "outside-global" => outside_global.clone(),
            }
        };
        let instance = module.instantiate(&import_object);

        assert!(instance.is_ok());
    }

    #[test]
    fn exports_work() {
        use wasmer::wasm::{Global, Value};
        use wasmer::{func, imports, CompiledModule, Func, Module, Table};

        let wasm = wabt::wat2wasm(TEST_WAT).unwrap();
        // TODO: review error messages when `CompiledModule` is not in scope. My hypothesis is that they'll be
        // misleading, if so we may want to do something about it.
        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();

        let outside_global = Global::new_mutable(Value::I32(15));

        let import_object = imports! {
            "env" => {
                "update-func" => func!(|x: i32| x * 2),
                "outside-global" => outside_global.clone(),
            }
        };
        let instance = module.instantiate(&import_object).unwrap();

        let ret_2: Func<(), i32> = instance.exports.get("ret_2").unwrap();
        let ret_4: Func<(), i32> = instance.exports.get("ret_4").unwrap();
        let set_test_global: Func<i32> = instance.exports.get("set_test_global").unwrap();
        let update_outside_global: Func = instance.exports.get("update_outside_global").unwrap();

        assert_eq!(ret_2.call(), Ok(2));
        assert_eq!(ret_4.call(), Ok(4));

        let _test_table: Table = instance.exports.get("test-table").unwrap();
        // TODO: when table get is stablized test this

        let test_global: Global = instance.exports.get("test-global").unwrap();
        // TODO: do we want to make global.get act like exports.get()?
        assert_eq!(test_global.get(), Value::I32(3));
        set_test_global.call(17).unwrap();
        assert_eq!(test_global.get(), Value::I32(17));

        assert_eq!(outside_global.get(), Value::I32(15));
        update_outside_global.call().unwrap();
        assert_eq!(outside_global.get(), Value::I32(15 * 2));
        update_outside_global.call().unwrap();
        assert_eq!(outside_global.get(), Value::I32(15 * 2 * 2));
    }

    #[test]
    fn allow_missing() {
        use wabt::wat2wasm;
        use wasmer::{imports, CompiledModule, Module};

        static WAT: &'static str = r#"
            (module
            (type (;0;) (func))
            (type (;1;) (func (result i32)))
            (import "env" "ret_err" (func $ret_err (type 0)))
            (func $get_num (type 1)
                i32.const 42
            )
            (export "get_num" (func $get_num))
            )
        "#;

        let wasm = wat2wasm(WAT).unwrap();
        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();

        let mut import_object = imports! {};
        import_object.allow_missing_functions = true;

        assert!(module.instantiate(&import_object).is_ok());
    }

    #[test]
    fn error_propagation() {
        use std::convert::Infallible;
        use wabt::wat2wasm;
        use wasmer::{func, imports, error::RuntimeError, vm::Ctx, CompiledModule, Func, Module};

        static WAT: &'static str = r#"
            (module
            (type (;0;) (func))
            (import "env" "ret_err" (func $ret_err (type 0)))
            (func $call_panic
                call $ret_err
            )
            (export "call_err" (func $call_panic))
            )
        "#;

        #[derive(Debug)]
        struct ExitCode {
            code: i32,
        }

        fn ret_err(_ctx: &mut Ctx) -> Result<Infallible, ExitCode> {
            Err(ExitCode { code: 42 })
        }

        let wasm = wat2wasm(WAT).unwrap();
        let module = Module::new_with_compiler(wasm, get_compiler()).unwrap();
        let instance = module
            .instantiate(&imports! {
                "env" => {
                    "ret_err" => Func::new(ret_err),
                },
            })
            .unwrap();

        let foo: Func<(), ()> = instance.exports.get("call_err").unwrap();

        let result = foo.call();

        if let Err(RuntimeError(e)) = result {
            let exit_code = e.downcast::<ExitCode>().unwrap();
            assert_eq!(exit_code.code, 42);
        } else {
            panic!("didn't return RuntimeError")
        }
    }
}

// copied from Rust stdlib: https://doc.rust-lang.org/nightly/nightly-rustc/src/serialize/leb128.rs.html#4-14
macro_rules! impl_write_unsigned_leb128 {
    ($fn_name:ident, $int_ty:ident) => {
        #[inline]
        pub fn $fn_name(out: &mut Vec<u8>, mut value: $int_ty) {
            loop {
                if value < 0x80 {
                    out.push(value as u8);
                    break;
                } else {
                    out.push(((value & 0x7f) | 0x80) as u8);
                    value >>= 7;
                }
            }
        }
    };
}

impl_write_unsigned_leb128!(write_u16_leb128, u16);
impl_write_unsigned_leb128!(write_u32_leb128, u32);
impl_write_unsigned_leb128!(write_u64_leb128, u64);
impl_write_unsigned_leb128!(write_u128_leb128, u128);
impl_write_unsigned_leb128!(write_usize_leb128, usize);
