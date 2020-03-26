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
    wasm: &[u8],
    custom_section_name: &str,
    custom_section_contents: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        // 1 for custom section id, 5 for max length of custom section, 5 for max length of name
        wasm.len() + custom_section_name.len() + custom_section_contents.len() + 1 + 5 + 5,
    );

    out.extend(wasm);
    out.push(0);

    let name_length = custom_section_name.as_bytes().len() as u32;
    let mut name_length_as_leb128 = vec![];
    write_u32_leb128(&mut name_length_as_leb128, name_length);

    let custom_section_length = (custom_section_contents.len()
        + custom_section_name.as_bytes().len()
        + name_length_as_leb128.len()) as u32;

    let mut custom_section_length_as_leb128 = vec![];
    write_u32_leb128(&mut custom_section_length_as_leb128, custom_section_length);

    out.extend(&custom_section_length_as_leb128);
    out.extend(&name_length_as_leb128);
    out.extend(custom_section_name.as_bytes());
    out.extend(custom_section_contents);

    out
}

#[test]
fn it_works() {
    use wasmer::wasm::{Global, Value};
    use wasmer::{export, func, imports, CompiledModule, Func, Module, Table};
    let wasm_no_custom_section = wabt::wat2wasm(TEST_WAT).unwrap();
    let wasm = append_custom_section(
        &wasm_no_custom_section,
        "test_custom_section",
        b"hello, world!",
    );
    let wasm = append_custom_section(&wasm, "test_custom_section", b"goodbye, world!");
    // TODO: review error messages when `CompiledModule` is not in scope. My hypothesis is that they'll be
    // misleading, if so we may want to do something about it.
    let module = Module::new(wasm).unwrap();
    let imports = module.imports();
    assert_eq!(imports.len(), 2);
    let num_exports = module.exports().count();
    assert_eq!(num_exports, 6);
    let (
        mut test_table_seen,
        mut test_global_seen,
        mut test_ret_2_seen,
        mut test_ret_4_seen,
        mut set_test_global_seen,
        mut update_outside_global_seen,
    ) = (false, false, false, false, false, false);
    for export in module.exports() {
        match (export.name.as_ref(), export.kind) {
            ("test-table", export::ExportKind::Table) => {
                assert!(!test_table_seen);
                test_table_seen = true;
            }
            ("test-global", export::ExportKind::Global) => {
                assert!(!test_global_seen);
                test_global_seen = true;
            }
            ("ret_2", export::ExportKind::Function) => {
                assert!(!test_ret_2_seen);
                test_ret_2_seen = true;
            }
            ("ret_4", export::ExportKind::Function) => {
                assert!(!test_ret_4_seen);
                test_ret_4_seen = true;
            }
            ("set_test_global", export::ExportKind::Function) => {
                assert!(!set_test_global_seen);
                set_test_global_seen = true;
            }
            ("update_outside_global", export::ExportKind::Function) => {
                assert!(!update_outside_global_seen);
                update_outside_global_seen = true;
            }
            _ => {
                assert!(false, "Unknown export found!");
            }
        }
    }
    assert!(test_table_seen);
    assert!(test_global_seen);
    assert!(test_ret_2_seen);
    assert!(test_ret_4_seen);
    assert!(set_test_global_seen);
    assert!(update_outside_global_seen);

    assert_eq!(
        module.custom_sections("test_custom_section"),
        Some(&[b"hello, world!".to_vec(), b"goodbye, world!".to_vec()][..])
    );
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
