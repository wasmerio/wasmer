use crate::utils::get_store_with_middlewares;
use anyhow::Result;

use std::sync::Arc;
use wasmer::wasmparser::{Operator, Result as WpResult};
use wasmer::*;

#[derive(Debug)]
struct Add2MulGen {
    value_off: i32,
}

#[derive(Debug)]
struct Add2Mul {
    value_off: i32,
}

impl FunctionMiddlewareGenerator for Add2MulGen {
    fn generate<'a>(&self, _: LocalFunctionIndex) -> Box<dyn FunctionMiddleware> {
        Box::new(Add2Mul {
            value_off: self.value_off,
        })
    }
}

impl FunctionMiddleware for Add2Mul {
    fn feed<'a>(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> WpResult<()> {
        match operator {
            Operator::I32Add => {
                state.push_operator(Operator::I32Mul);
                if self.value_off != 0 {
                    state.push_operator(Operator::I32Const {
                        value: self.value_off,
                    });
                    state.push_operator(Operator::I32Add);
                }
            }
            _ => {
                state.push_operator(operator);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
struct FusionGen;

#[derive(Debug)]
struct Fusion {
    state: i32,
}

impl FunctionMiddlewareGenerator for FusionGen {
    fn generate<'a>(&self, _: LocalFunctionIndex) -> Box<dyn FunctionMiddleware> {
        Box::new(Fusion { state: 0 })
    }
}

impl FunctionMiddleware for Fusion {
    fn feed<'a>(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> WpResult<()> {
        match (operator, self.state) {
            (Operator::I32Add, 0) => {
                self.state = 1;
            }
            (Operator::I32Mul, 1) => {
                state.push_operator(Operator::Select);
                self.state = 0;
            }
            (operator, 1) => {
                state.push_operator(Operator::I32Add);
                state.push_operator(operator);
                self.state = 0;
            }
            (operator, _) => {
                state.push_operator(operator);
            }
        }
        Ok(())
    }
}

#[test]
fn middleware_basic() -> Result<()> {
    let store = get_store_with_middlewares(std::iter::once(
        Arc::new(Add2MulGen { value_off: 0 }) as Arc<dyn FunctionMiddlewareGenerator>
    ));
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    assert_eq!(result, 24);
    Ok(())
}

#[test]
fn middleware_one_to_multi() -> Result<()> {
    let store = get_store_with_middlewares(std::iter::once(
        Arc::new(Add2MulGen { value_off: 1 }) as Arc<dyn FunctionMiddlewareGenerator>
    ));
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    assert_eq!(result, 25);
    Ok(())
}

#[test]
fn middleware_multi_to_one() -> Result<()> {
    let store = get_store_with_middlewares(std::iter::once(
        Arc::new(FusionGen) as Arc<dyn FunctionMiddlewareGenerator>
    ));
    let wat = r#"(module
        (func (export "testfunc") (param i32 i32) (result i32)
           (local.get 0)
           (local.get 1)
           (i32.const 1)
           (i32.add)
           (i32.mul))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("testfunc")?;
    let result = f.call(10, 20)?;
    assert_eq!(result, 10);
    Ok(())
}

#[test]
fn middleware_chain_order_1() -> Result<()> {
    let store = get_store_with_middlewares(
        vec![
            Arc::new(Add2MulGen { value_off: 0 }) as Arc<dyn FunctionMiddlewareGenerator>,
            Arc::new(Add2MulGen { value_off: 2 }) as Arc<dyn FunctionMiddlewareGenerator>,
        ]
        .into_iter(),
    );
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    assert_eq!(result, 24);
    Ok(())
}

#[test]
fn middleware_chain_order_2() -> Result<()> {
    let store = get_store_with_middlewares(
        vec![
            Arc::new(Add2MulGen { value_off: 2 }) as Arc<dyn FunctionMiddlewareGenerator>,
            Arc::new(Add2MulGen { value_off: 0 }) as Arc<dyn FunctionMiddlewareGenerator>,
        ]
        .into_iter(),
    );
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();

    let import_object = imports! {};

    let instance = Instance::new(&module, &import_object)?;

    let f: NativeFunc<(i32, i32), i32> = instance.exports.get_native_function("add")?;
    let result = f.call(4, 6)?;
    assert_eq!(result, 48);
    Ok(())
}
