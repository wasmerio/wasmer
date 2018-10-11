use std::path::Path;

use wabt::script::{Value, Action};
use super::{InvokationResult, ScriptHandler, run_single_file};
use crate::webassembly::{compile, Error, ErrorKind};

struct StoreCtrl {
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
            // modules: HashMap::new(),
            // last_module: None,
        }
    }

    // fn add_module(&mut self, name: Option<String>, module: DummyEnvironment) {
    //     // if let Some(name) = name {
    //     // println!("ADD MODULE {:?}", name);
    //     // self.modules
    //     //     .insert(name.unwrap_or("__last_module".to_string()), module);
    //     // }
    //     // self.modules.insert("__last_module".to_string(), module);
    //     // self.last_module = Some(module);
    // }

    // fn get_module(&self, name: String) -> Option<&DummyEnvironment> {
    //     // return self
    //     //     .modules
    //     //     .get(&name)
    //     //     .or(self.modules.get("__last_module"));
    //     return None;
    //     // return self.modules[&name];
    //     // name.map(|name| self.modules[&name]).or(self.last_module).unwrap()
    // }
}

impl ScriptHandler for StoreCtrl {
    fn reset(&mut self) {}
    fn action_invoke(
        &mut self,
        module: Option<String>,
        field: String,
        args: Vec<Value>,
    ) -> InvokationResult {
        unimplemented!()
    }
    fn action_get(&mut self, module: Option<String>, field: String) -> Value {
        unimplemented!()
    }
    fn module(&mut self, bytes: Vec<u8>, name: Option<String>) {
        let module_wrapped = compile(bytes);
        module_wrapped.expect("Module is invalid");
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
}
