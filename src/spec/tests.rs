use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use cranelift_codegen::ir::types;
use cranelift_entity::EntityRef;
use libffi::high::call::*;
use libffi::high::types::CType;
use std::iter::Iterator;
use wabt::script::{Action, Value};
// use crate::webassembly::instance::InvokeResult;

use super::{run_single_file, InvokationResult, ScriptHandler};
use crate::webassembly::{
    compile, instantiate, Error, ErrorKind, Export, ImportObject, Instance, Module, ResultObject,
};

struct StoreCtrl<'module> {
    last_module: Option<ResultObject>,
    modules: HashMap<String, Rc<&'module ResultObject>>,
}

impl<'module> StoreCtrl<'module> {
    fn new() -> Self {
        // let (tx, rx) = channel();

        // let _handle = thread::spawn(|| {
        //     let _: FrameWitness = store_thread_frame(
        //         StSt { store: Store::new(), stack: Stack::new(), recv: rx});
        // });

        StoreCtrl {
            // tx,
            // _handle,
            modules: HashMap::new(),
            last_module: None,
        }
    }

    fn add_module(&mut self, name: Option<String>, module: &'module ResultObject) {
        // self.last_module = Some(Rc::new(module));
        // if let Some(name) = name {
        //     // self.modules[&name] = module;
        //     self.modules.insert(name, Rc::new(self.last_module.unwrap()));
        // }
        // println!("ADD MODULE {:?}", name);
        // self.modules
        //     .insert(name.unwrap_or("__last_module".to_string()), module);
        // }
        // self.modules.insert("__last_module".to_string(), module);
        // self.last_module = Some(module);
    }

    fn get_module(self, name: Option<String>) -> &'module ResultObject {
        unimplemented!()
        // self.last_module.expect("exists")
        // return self
        //     .modules
        //     .get(&name)
        //     .or(self.modules.get("__last_module"));
        // return None;
        // return self.modules[&name];
        // name.map(|name| self.modules[&name]).or(self.last_module).unwrap()
    }
}

impl<'module> ScriptHandler for StoreCtrl<'module> {
    fn reset(&mut self) {}
    fn action_invoke(
        &mut self,
        module: Option<String>,
        field: String,
        args: Vec<Value>,
    ) -> InvokationResult {
        if let Some(result) = &mut self.last_module {
            let instance = &result.instance;
            let module = &result.module;
            let func_index = match module.info.exports.get(&field) {
                Some(&Export::Function(index)) => index,
                _ => panic!("Function not found"),
            };
            // We map the arguments provided into the raw Arguments provided
            // to libffi
            let call_args: Vec<Arg> = args
                .iter()
                .map(|a| match a {
                    Value::I32(v) => arg(v),
                    Value::I64(v) => arg(v),
                    Value::F32(v) => arg(v),
                    Value::F64(v) => arg(v),
                })
                .collect();
            // We use libffi to call a function with a vector of arguments
            let call_func: fn() = instance.get_function(func_index);
            let result: i64 = unsafe { call(CodePtr(call_func as *mut _), &call_args) };

            // We retrieve the return type of the function, and wrap the result with it
            let signature_index = module.info.functions[func_index].entity;
            let signature = &module.info.signatures[signature_index];
            let return_values = if signature.returns.len() > 0 {
                let val = match signature.returns[0].value_type {
                    types::I32 => Value::I32(result as _),
                    types::I64 => Value::I64(result as _),
                    types::F32 => Value::F32(result as f32),
                    types::F64 => Value::F64(result as f64),
                    _ => panic!("Unexpected type"),
                };
                vec![val]
            } else {
                vec![]
            };

            println!(
                "Function {:?}(index: {:?}) ({:?}) => returned {:?}",
                field.to_string(),
                func_index,
                call_args,
                return_values
            );

            return InvokationResult::Vals(return_values);
        }
        panic!("module not found");
    }
    fn action_get(&mut self, module: Option<String>, field: String) -> Value {
        // println!("action get");
        unimplemented!()
    }
    fn module(&mut self, bytes: Vec<u8>, name: Option<String>) {
        let mut import_object = ImportObject::new();
        extern "C" fn identity(x: i32) -> i32 {
            x
        };
        import_object.set("test", "identity", identity as *const u8);
        // let import_object = import_object!{
        //     test.identity => fn(x: i32) {x},
        // }
        let module_wrapped = instantiate(bytes, import_object);
        let mut result = module_wrapped.expect("Module is invalid");
        // let module: &'module Module = result.module;
        self.last_module = Some(result);
        // self.add_module(name, &mut result);
        // println!("ADD MODULE {}", name.unwrap_or("no name".to_string()))
    }
    fn assert_malformed(&mut self, bytes: Vec<u8>) {
        let module_wrapped = instantiate(bytes, ImportObject::new());
        match module_wrapped {
            Err(ErrorKind::CompileError(v)) => {}
            _ => panic!("Module compilation should have failed"),
        }
    }
    fn assert_invalid(&mut self, bytes: Vec<u8>) {
        // print!("IS INVALID");
        let module_wrapped = instantiate(bytes, ImportObject::new());
        // print!("IS INVALID?? {:?}", module_wrapped);
        match module_wrapped {
            Err(ErrorKind::CompileError(v)) => {}
            _ => assert!(false, "Module compilation should have failed"),
        }
    }
    fn assert_uninstantiable(&mut self, bytes: Vec<u8>) {
        // unimplemented!()
    }
    fn assert_exhaustion(&mut self, action: Action) {
        // unimplemented!()
    }
    fn register(&mut self, name: Option<String>, as_name: String) {
        // println!("ADD MODULE {:?} {:?}", name.unwrap(), as_name);
        unimplemented!()
    }
}

mod tests {
    use super::run_single_file;
    use crate::test::Bencher;
    use std::mem;
    use std::path::Path;
    #[macro_use]
    use crate::webassembly::{
        compile, instantiate, Error, ErrorKind, Export, Instance, Module, ResultObject,
        ImportObject,
    };
    use wabt::wat2wasm;

    fn do_test(test_name: String) {
        let mut handler = &mut super::StoreCtrl::new();
        let test_path_str = format!(
            "{}/src/spec/tests/{}.wast",
            env!("CARGO_MANIFEST_DIR"),
            test_name
        );
        let test_path = Path::new(&test_path_str);
        let res = run_single_file(&test_path, handler);
        res.present()
    }

    macro_rules! wasm_tests {
        ($($name:ident,)*) => {
        $(
            #[test]
            fn $name() {
                // let test_filename = $value;
                // assert_eq!(expected, fib(input));
                do_test(stringify!($name).to_string());
            }
        )*
        }
    }

    macro_rules! instantiate_from_wast {
        ($x:expr) => {{
            let wasm_bytes = include_wast2wasm_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), $x));
            let result_object =
                instantiate(wasm_bytes, ImportObject::new()).expect("Not compiled properly");
            result_object
        }};
    }

    #[bench]
    fn bench_identity(b: &mut Bencher) {
        let result_object = instantiate_from_wast!("/src/spec/tests/benchmark.wast");
        let instance = result_object.instance;
        let module = result_object.module;
        let func_index = match module.info.exports.get("identity") {
            Some(&Export::Function(index)) => index,
            _ => panic!("Function not found"),
        };
        let func: fn(i32) -> i32 = get_instance_function!(instance, func_index);
        assert_eq!(func(1), 1, "Identity function not working.");
        b.iter(|| {
            func(1);
        });
    }

    wasm_tests!{
        _type,
        br_if,
        call,
        import,
        memory,
    }

}
