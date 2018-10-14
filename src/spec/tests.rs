use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use super::{run_single_file, InvokationResult, ScriptHandler};
use crate::webassembly::{compile, instantiate, Error, ErrorKind, Module};
use wabt::script::{Action, Value};
// use crate::webassembly::instance::InvokeResult;

struct StoreCtrl<'module> {
    last_module: Option<Module>,
    modules: HashMap<String, Rc<&'module Module>>,
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

    fn add_module(&mut self, name: Option<String>, module: &'module Module) {
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

    fn get_module(self, name: Option<String>) -> &'module Module {
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
        // let modu = (&self.last_module);
        // let x = modu.unwrap();
        unimplemented!()
        // if let Some(m) = &mut self.last_module {
        //     // let function = module.exports.get(field).expect("field not found");
        //     // let mut m = &mut m;
        //     let mut instance = &mut m.instance;
        //     // println!("HEEY {:?}", module.instance);
        //     let x = instance.execute_fn(
        //         &m.module,
        //         &m.compilation,
        //         field,
        //     ).unwrap();
        //     println!("X value {:?}", x);
        //     let res = match x {
        //         InvokeResult::VOID => {
        //             vec![]
        //         },
        //         InvokeResult::I32(v) => vec![Value::I32(v)],
        //         InvokeResult::I64(v) => vec![Value::I64(v)],
        //         InvokeResult::F32(v) => vec![Value::F32(v)],
        //         InvokeResult::F64(v) => vec![Value::F64(v)],
        //     };
        //     InvokationResult::Vals(res)
        //     // unimplemented!()
        //     // InvokationResult::Vals(vec![Value::I32(*x)])
        //     // unimplemented!();
        //     // let result = Rc::try_unwrap(module);
        //     // let mut mutinstance = Rc::make_mut(&module.instance);
        //     // execute_fn(
        //     //     &module.module,
        //     //     &module.compilation,
        //     //     &mut (&mut module.instance),
        //     //     field,
        //     // );
        // }
        // else {
        //     panic!("module not found");
        // }
        // match module {
        //     Some(m) => {
        //         println!("HEEY {:?}", m);
        //     },
        //     _ => unimplemented!()
        // }
        // println!("action invoke {}", module.unwrap_or("as".to_string()));
        // let modul = &self.last_module;
        // modul.expect("a");
        //
    }
    fn action_get(&mut self, module: Option<String>, field: String) -> Value {
        // println!("action get");
        unimplemented!()
    }
    fn module(&mut self, bytes: Vec<u8>, name: Option<String>) {
        let module_wrapped = instantiate(bytes, None);
        let mut result = module_wrapped.expect("Module is invalid").module;
        // let module: &'module Module = result.module;
        self.last_module = Some(result);
        // self.add_module(name, &mut result);
        // println!("ADD MODULE {}", name.unwrap_or("no name".to_string()))
    }
    fn assert_malformed(&mut self, bytes: Vec<u8>) {
        let module_wrapped = instantiate(bytes, None);
        match module_wrapped {
            Err(ErrorKind::CompileError(v)) => {}
            _ => panic!("Module compilation should have failed"),
        }
    }
    fn assert_invalid(&mut self, bytes: Vec<u8>) {
        // print!("IS INVALID");
        let module_wrapped = instantiate(bytes, None);
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

fn do_test(test_name: String) {
    let mut handler = &mut StoreCtrl::new();
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

wasm_tests!{
    _type,
    br_if,
    // call,
}
