use std::path::Path;
use std::collections::HashMap;
use std::rc::Rc;

use wabt::script::{Value, Action};
use super::{InvokationResult, ScriptHandler, run_single_file};
use crate::webassembly::{compile, instantiate, Error, ErrorKind, Module, Instance};

struct StoreCtrl {
    last_module: Option<Rc<Instance>>,
    modules: HashMap<String, Rc<Instance>>
}

impl StoreCtrl {
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

    fn add_module(&mut self, name: Option<String>, module: Rc<Instance>) {
        if let Some(name) = name {
            // self.modules[&name] = module;
            self.modules.insert(name, Rc::clone(&module));
        }
        self.last_module = Some(Rc::clone(&module));
        // println!("ADD MODULE {:?}", name);
        // self.modules
        //     .insert(name.unwrap_or("__last_module".to_string()), module);
        // }
        // self.modules.insert("__last_module".to_string(), module);
        // self.last_module = Some(module);
    }

    fn get_module(self, name: Option<String>) -> Rc<Instance> {
        self.last_module.unwrap()
        // return self
        //     .modules
        //     .get(&name)
        //     .or(self.modules.get("__last_module"));
        // return None;
        // return self.modules[&name];
        // name.map(|name| self.modules[&name]).or(self.last_module).unwrap()
    }
}

impl ScriptHandler for StoreCtrl {
    fn reset(&mut self) {}
    fn action_invoke(
        &mut self,
        module: Option<String>,
        field: String,
        args: Vec<Value>,
    ) -> InvokationResult {
        if let Some(module) = &self.last_module {
            // let function = module.exports.get(field).expect("field not found");
            println!("HEEY {:?}", module);
        }
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
        unimplemented!()
    }
    fn action_get(&mut self, module: Option<String>, field: String) -> Value {
        // println!("action get");
        unimplemented!()
    }
    fn module(&mut self, bytes: Vec<u8>, name: Option<String>) {
        let module_wrapped = instantiate(bytes, None);
        let result = module_wrapped.expect("Module is invalid");
        // let module: &'module Module = result.module;
        // self.last_module = Some(result.module);
        self.add_module(name, Rc::new(result.instance));
        // println!("ADD MODULE {}", name.unwrap_or("no name".to_string()))
    }
    fn assert_malformed(&mut self, bytes: Vec<u8>) {
        let module_wrapped = compile(bytes);
        match module_wrapped {
            Err(Error(ErrorKind::CompileError(v), _)) => {}
            _ => panic!("Module compilation should have failed")
        }
    }
    fn assert_invalid(&mut self, bytes: Vec<u8>) {
        let module_wrapped = compile(bytes);
        match module_wrapped {
            Err(Error(ErrorKind::CompileError(v), _)) => {}
            _ => assert!(false, "Module compilation should have failed")
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
    let test_path_str = format!("{}/src/spec/tests/{}.wast", env!("CARGO_MANIFEST_DIR"), test_name);
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
}
