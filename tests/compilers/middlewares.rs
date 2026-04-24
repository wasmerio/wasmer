use anyhow::Result;

use std::sync::{Arc, Mutex};
use wasmer::FunctionEnv;
use wasmer::wasmparser::{Operator, ValType};
use wasmer::{sys::*, *};

#[derive(Debug)]
struct Add2MulGen {
    value_off: i32,
}

#[derive(Debug)]
struct Add2Mul {
    value_off: i32,
}

impl ModuleMiddleware for Add2MulGen {
    fn generate_function_middleware<'a>(
        &self,
        _: LocalFunctionIndex,
    ) -> Box<dyn FunctionMiddleware<'a> + 'a> {
        Box::new(Add2Mul {
            value_off: self.value_off,
        })
    }
}

impl<'a> FunctionMiddleware<'a> for Add2Mul {
    fn feed(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> Result<(), MiddlewareError> {
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

impl ModuleMiddleware for FusionGen {
    fn generate_function_middleware<'a>(
        &self,
        _: LocalFunctionIndex,
    ) -> Box<dyn FunctionMiddleware<'a> + 'a> {
        Box::new(Fusion { state: 0 })
    }
}

impl<'a> FunctionMiddleware<'a> for Fusion {
    fn feed(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> Result<(), MiddlewareError> {
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

#[compiler_test(middlewares)]
fn middleware_basic(mut config: crate::Config) -> Result<()> {
    config.set_middlewares(vec![
        Arc::new(Add2MulGen { value_off: 0 }) as Arc<dyn ModuleMiddleware>
    ]);
    let mut store = config.store();
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let mut env = FunctionEnv::new(&mut store, ());

    let import_object = imports! {};

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> = instance.exports.get_typed_function(&store, "add")?;
    let result = f.call(&mut store, 4, 6)?;
    assert_eq!(result, 24);
    Ok(())
}

#[compiler_test(middlewares)]
fn middleware_one_to_multi(mut config: crate::Config) -> Result<()> {
    config.set_middlewares(vec![
        Arc::new(Add2MulGen { value_off: 1 }) as Arc<dyn ModuleMiddleware>
    ]);
    let mut store = config.store();
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let mut env = FunctionEnv::new(&mut store, ());
    let import_object = imports! {};

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> = instance.exports.get_typed_function(&store, "add")?;
    let result = f.call(&mut store, 4, 6)?;
    assert_eq!(result, 25);
    Ok(())
}

#[compiler_test(middlewares)]
fn middleware_multi_to_one(mut config: crate::Config) -> Result<()> {
    config.set_middlewares(vec![Arc::new(FusionGen) as Arc<dyn ModuleMiddleware>]);
    let mut store = config.store();
    let wat = r#"(module
        (func (export "testfunc") (param i32 i32) (result i32)
           (local.get 0)
           (local.get 1)
           (i32.const 1)
           (i32.add)
           (i32.mul))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let mut env = FunctionEnv::new(&mut store, ());
    let import_object = imports! {};

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> =
        instance.exports.get_typed_function(&store, "testfunc")?;
    let result = f.call(&mut store, 10, 20)?;
    assert_eq!(result, 10);
    Ok(())
}

#[compiler_test(middlewares)]
fn middleware_chain_order_1(mut config: crate::Config) -> Result<()> {
    config.set_middlewares(vec![
        Arc::new(Add2MulGen { value_off: 0 }) as Arc<dyn ModuleMiddleware>,
        Arc::new(Add2MulGen { value_off: 2 }) as Arc<dyn ModuleMiddleware>,
    ]);
    let mut store = config.store();
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let import_object = imports! {};

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> = instance.exports.get_typed_function(&store, "add")?;
    let result = f.call(&mut store, 4, 6)?;
    assert_eq!(result, 24);
    Ok(())
}

#[compiler_test(middlewares)]
fn middleware_chain_order_2(mut config: crate::Config) -> Result<()> {
    config.set_middlewares(vec![
        Arc::new(Add2MulGen { value_off: 2 }) as Arc<dyn ModuleMiddleware>,
        Arc::new(Add2MulGen { value_off: 0 }) as Arc<dyn ModuleMiddleware>,
    ]);
    let mut store = config.store();
    let wat = r#"(module
        (func (export "add") (param i32 i32) (result i32)
           (i32.add (local.get 0)
                    (local.get 1)))
)"#;
    let module = Module::new(&store, wat).unwrap();
    let import_object = imports! {};

    let instance = Instance::new(&mut store, &module, &import_object)?;

    let f: TypedFunction<(i32, i32), i32> = instance.exports.get_typed_function(&store, "add")?;
    let result = f.call(&mut store, 4, 6)?;
    assert_eq!(result, 48);
    Ok(())
}

/// Middleware that captures the locals information passed via `locals_info`.
#[derive(Debug)]
struct LocalsInfoCapture {
    captured: Arc<Mutex<Vec<Vec<ValType>>>>,
}

impl ModuleMiddleware for LocalsInfoCapture {
    fn generate_function_middleware<'a>(
        &self,
        _: LocalFunctionIndex,
    ) -> Box<dyn FunctionMiddleware<'a> + 'a> {
        Box::new(LocalsInfoCaptureMiddleware {
            captured: self.captured.clone(),
        })
    }
}

#[derive(Debug)]
struct LocalsInfoCaptureMiddleware {
    captured: Arc<Mutex<Vec<Vec<ValType>>>>,
}

impl<'a> FunctionMiddleware<'a> for LocalsInfoCaptureMiddleware {
    fn locals_info(&mut self, locals: &[ValType]) {
        self.captured.lock().unwrap().push(locals.to_vec());
    }
}

#[compiler_test(middlewares)]
fn middleware_locals_info(mut config: crate::Config) -> Result<()> {
    let captured: Arc<Mutex<Vec<Vec<ValType>>>> = Arc::new(Mutex::new(Vec::new()));
    config.set_middlewares(vec![Arc::new(LocalsInfoCapture {
        captured: captured.clone(),
    }) as Arc<dyn ModuleMiddleware>]);
    let store = config.store();
    // Function with two i32 params and three explicit locals (one i64, two f32).
    // Note: params are NOT included in locals_info — only declared locals are.
    let wat = r#"(module
        (func (export "test") (param i32 i32) (result i32)
           (local i64)
           (local f32 f32)
           (i32.add (local.get 0) (local.get 1)))
    )"#;
    let _module = Module::new(&store, wat)?;

    let locals = captured.lock().unwrap();
    assert_eq!(
        *locals,
        vec![vec![ValType::I64, ValType::F32, ValType::F32]]
    );
    Ok(())
}

#[compiler_test(middlewares)]
fn middleware_locals_info_no_locals(mut config: crate::Config) -> Result<()> {
    let captured: Arc<Mutex<Vec<Vec<ValType>>>> = Arc::new(Mutex::new(Vec::new()));
    config.set_middlewares(vec![Arc::new(LocalsInfoCapture {
        captured: captured.clone(),
    }) as Arc<dyn ModuleMiddleware>]);
    let store = config.store();
    // Function with no locals at all — locals_info should still be called with an empty slice.
    let wat = r#"(module
        (func (export "test") (param i32 i32) (result i32)
           (i32.add (local.get 0) (local.get 1)))
    )"#;
    let _module = Module::new(&store, wat)?;

    let locals = captured.lock().unwrap();
    assert_eq!(*locals, vec![vec![]]);
    Ok(())
}

#[compiler_test(middlewares)]
fn middleware_locals_info_multiple_functions(mut config: crate::Config) -> Result<()> {
    let captured: Arc<Mutex<Vec<Vec<ValType>>>> = Arc::new(Mutex::new(Vec::new()));
    config.set_middlewares(vec![Arc::new(LocalsInfoCapture {
        captured: captured.clone(),
    }) as Arc<dyn ModuleMiddleware>]);
    let store = config.store();
    let wat = r#"(module
        (func (export "f1") (result i32)
           (local i32 i32)
           (i32.add (local.get 0) (local.get 1)))
        (func (export "f2") (result f64)
           (local f64)
           (local.get 0))
    )"#;
    let _module = Module::new(&store, wat)?;

    let locals = captured.lock().unwrap();
    // Order may vary due to concurrent compilation, so sort before comparing.
    let mut sorted: Vec<Vec<ValType>> = locals.clone();
    sorted.sort_by_key(|v| v.len());
    assert_eq!(
        sorted,
        vec![vec![ValType::F64], vec![ValType::I32, ValType::I32]]
    );
    Ok(())
}
