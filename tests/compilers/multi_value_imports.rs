//! Testing the imports with different provided functions.
//! This tests checks that the provided functions (both native and
//! dynamic ones) work properly.

use wasmer::*;

macro_rules! mvr_test {
    ($test_name:ident, $( $result_type:ty ),* ) => {
        mod $test_name {
            use wasmer::*;

            fn get_module(store: &Store) -> anyhow::Result<wasmer::Module> {
                let wat: String = r#"
  (type $type (func (param i32) (result
"#.to_string() +
                    &stringify!( $( $result_type ),* ).replace(",", "").replace("(", "").replace(")", "") + &r#")))
  (import "host" "callback_fn" (func $callback_fn (type $type)))
  (func (export "test_call") (type $type)
    local.get 0
    call $callback_fn)
  (func (export "test_call_indirect") (type $type)
    (i32.const 1)
    (call_indirect (type $type) (i32.const 0))
  )
  (table funcref
    (elem
      $callback_fn
    )
  )
"#.to_string();
                Ok(wasmer::Module::new(&store, &wat)?)
            }

            fn callback_fn(n: i32) -> ( $( $result_type ),* ) {
                ( $( <$result_type>::expected_value(n) ),* )
            }

            #[compiler_test(multi_value_imports)]
            fn native(config: crate::Config) -> anyhow::Result<()> {
                let mut store = config.store();
                let module = get_module(&store)?;
                let instance = wasmer::Instance::new(
                    &module,
                    &wasmer::imports! {
                        "host" => {
                            "callback_fn" => wasmer::Function::new_native(&store, callback_fn)
                        }
                    }
                )?;
                let expected_value = vec![ $( <$result_type>::expected_val(1) ),* ].into_boxed_slice();
                assert_eq!(instance.exports.get_function("test_call")?.call(&[wasmer::Val::I32(1)])?,
                           expected_value);
                assert_eq!(instance.exports.get_function("test_call_indirect")?.call(&[wasmer::Val::I32(1)])?,
                           expected_value);
                Ok(())
            }

            fn dynamic_callback_fn(values: &[wasmer::Value]) -> anyhow::Result<Vec<wasmer::Val>, wasmer::RuntimeError> {
                assert_eq!(values[0], wasmer::Value::I32(1));
                Ok(vec![ $( <$result_type>::expected_val(1) ),* ])
            }

            #[compiler_test(multi_value_imports)]
            fn dynamic(config: crate::Config) -> anyhow::Result<()> {
                let mut store = config.store();
                let module = get_module(&store)?;
                let callback_fn = wasmer::Function::new(&store, &wasmer::FunctionType::new(vec![wasmer::ValType::I32], vec![ $( <$result_type>::expected_valtype() ),* ]), dynamic_callback_fn);
                let instance = wasmer::Instance::new(
                    &module,
                    &wasmer::imports! {
                        "host" => {
                            "callback_fn" => callback_fn
                        }
                    }
                )?;
                let expected_value = vec![ $( <$result_type>::expected_val(1) ),* ].into_boxed_slice();
                assert_eq!(instance.exports.get_function("test_call")?.call(&[wasmer::Val::I32(1)])?,
                           expected_value);
                assert_eq!(instance.exports.get_function("test_call_indirect")?.call(&[wasmer::Val::I32(1)])?,
                           expected_value);
                Ok(())
            }
        }
    }
}

trait ExpectedExpr {
    fn expected_value(n: i32) -> Self;
    fn expected_val(n: i32) -> wasmer::Val;
    fn expected_valtype() -> wasmer::ValType;
}
impl ExpectedExpr for i32 {
    fn expected_value(n: i32) -> i32 {
        n + 1
    }
    fn expected_val(n: i32) -> wasmer::Val {
        wasmer::Val::I32(Self::expected_value(n))
    }
    fn expected_valtype() -> wasmer::ValType {
        wasmer::ValType::I32
    }
}
impl ExpectedExpr for i64 {
    fn expected_value(n: i32) -> i64 {
        n as i64 + 2i64
    }
    fn expected_val(n: i32) -> wasmer::Val {
        wasmer::Val::I64(Self::expected_value(n))
    }
    fn expected_valtype() -> wasmer::ValType {
        wasmer::ValType::I64
    }
}
impl ExpectedExpr for f32 {
    fn expected_value(n: i32) -> f32 {
        n as f32 * 0.1
    }
    fn expected_val(n: i32) -> wasmer::Val {
        wasmer::Val::F32(Self::expected_value(n))
    }
    fn expected_valtype() -> wasmer::ValType {
        wasmer::ValType::F32
    }
}
impl ExpectedExpr for f64 {
    fn expected_value(n: i32) -> f64 {
        n as f64 * 0.12
    }
    fn expected_val(n: i32) -> wasmer::Val {
        wasmer::Val::F64(Self::expected_value(n))
    }
    fn expected_valtype() -> wasmer::ValType {
        wasmer::ValType::F64
    }
}

mvr_test!(test_mvr_i32_i32, i32, i32);
mvr_test!(test_mvr_i32_f32, i32, f32);
mvr_test!(test_mvr_f32_i32, f32, i32);
mvr_test!(test_mvr_f32_f32, f32, f32);

mvr_test!(test_mvr_i64_i32, i64, i32);
mvr_test!(test_mvr_i64_f32, i64, f32);
mvr_test!(test_mvr_f64_i32, f64, i32);
mvr_test!(test_mvr_f64_f32, f64, f32);

mvr_test!(test_mvr_i32_i64, i32, i64);
mvr_test!(test_mvr_f32_i64, f32, i64);
mvr_test!(test_mvr_i32_f64, i32, f64);
mvr_test!(test_mvr_f32_f64, f32, f64);

mvr_test!(test_mvr_i32_i32_i32, i32, i32, i32);
mvr_test!(test_mvr_i32_i32_f32, i32, i32, f32);
mvr_test!(test_mvr_i32_f32_i32, i32, f32, i32);
mvr_test!(test_mvr_i32_f32_f32, i32, f32, f32);
mvr_test!(test_mvr_f32_i32_i32, f32, i32, i32);
mvr_test!(test_mvr_f32_i32_f32, f32, i32, f32);
mvr_test!(test_mvr_f32_f32_i32, f32, f32, i32);
mvr_test!(test_mvr_f32_f32_f32, f32, f32, f32);

mvr_test!(test_mvr_i32_i32_i64, i32, i32, i64);
mvr_test!(test_mvr_i32_f32_i64, i32, f32, i64);
mvr_test!(test_mvr_f32_i32_i64, f32, i32, i64);
mvr_test!(test_mvr_f32_f32_i64, f32, f32, i64);
mvr_test!(test_mvr_i32_i32_f64, i32, i32, f64);
mvr_test!(test_mvr_i32_f32_f64, i32, f32, f64);
mvr_test!(test_mvr_f32_i32_f64, f32, i32, f64);
mvr_test!(test_mvr_f32_f32_f64, f32, f32, f64);

mvr_test!(test_mvr_i32_i64_i32, i32, i64, i32);
mvr_test!(test_mvr_i32_i64_f32, i32, i64, f32);
mvr_test!(test_mvr_f32_i64_i32, f32, i64, i32);
mvr_test!(test_mvr_f32_i64_f32, f32, i64, f32);
mvr_test!(test_mvr_i32_f64_i32, i32, f64, i32);
mvr_test!(test_mvr_i32_f64_f32, i32, f64, f32);
mvr_test!(test_mvr_f32_f64_i32, f32, f64, i32);
mvr_test!(test_mvr_f32_f64_f32, f32, f64, f32);

mvr_test!(test_mvr_i64_i32_i32, i64, i32, i32);
mvr_test!(test_mvr_i64_i32_f32, i64, i32, f32);
mvr_test!(test_mvr_i64_f32_i32, i64, f32, i32);
mvr_test!(test_mvr_i64_f32_f32, i64, f32, f32);
mvr_test!(test_mvr_f64_i32_i32, f64, i32, i32);
mvr_test!(test_mvr_f64_i32_f32, f64, i32, f32);
mvr_test!(test_mvr_f64_f32_i32, f64, f32, i32);
mvr_test!(test_mvr_f64_f32_f32, f64, f32, f32);

mvr_test!(test_mvr_i32_i32_i32_i32, i32, i32, i32, i32);
mvr_test!(test_mvr_i32_i32_i32_f32, i32, i32, i32, f32);
mvr_test!(test_mvr_i32_i32_f32_i32, i32, i32, f32, i32);
mvr_test!(test_mvr_i32_i32_f32_f32, i32, i32, f32, f32);
mvr_test!(test_mvr_i32_f32_i32_i32, i32, f32, i32, i32);
mvr_test!(test_mvr_i32_f32_i32_f32, i32, f32, i32, f32);
mvr_test!(test_mvr_i32_f32_f32_i32, i32, f32, f32, i32);
mvr_test!(test_mvr_i32_f32_f32_f32, i32, f32, f32, f32);
mvr_test!(test_mvr_f32_i32_i32_i32, f32, i32, i32, i32);
mvr_test!(test_mvr_f32_i32_i32_f32, f32, i32, i32, f32);
mvr_test!(test_mvr_f32_i32_f32_i32, f32, i32, f32, i32);
mvr_test!(test_mvr_f32_i32_f32_f32, f32, i32, f32, f32);
mvr_test!(test_mvr_f32_f32_i32_i32, f32, f32, i32, i32);
mvr_test!(test_mvr_f32_f32_i32_f32, f32, f32, i32, f32);
mvr_test!(test_mvr_f32_f32_f32_i32, f32, f32, f32, i32);
mvr_test!(test_mvr_f32_f32_f32_f32, f32, f32, f32, f32);

mvr_test!(test_mvr_i32_i32_i32_i32_i32, i32, i32, i32, i32, i32);
