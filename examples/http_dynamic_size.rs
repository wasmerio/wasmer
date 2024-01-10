use anyhow::Result;
use wasmer::{
    imports, wat2wasm, AsStoreRef, Function, FunctionEnv, FunctionEnvMut, Instance, Memory,
    MemoryView, Module, Store, WasmPtr,
};

// Utils
pub fn read_string(view: &MemoryView, start: u32, len: u32) -> Result<String> {
    Ok(WasmPtr::<u8>::new(start).read_utf8_string(view, len)?)
}

// Environment
pub struct ExampleEnv {
    memory: Option<Memory>,
}

impl ExampleEnv {
    fn set_memory(&mut self, memory: Memory) {
        self.memory = Some(memory);
    }

    fn get_memory(&self) -> &Memory {
        self.memory.as_ref().unwrap()
    }

    fn view<'a>(&'a self, store: &'a impl AsStoreRef) -> MemoryView<'a> {
        self.get_memory().view(store)
    }
}

fn http_get(mut ctx: FunctionEnvMut<ExampleEnv>, url: u32, url_len: u32) -> u32 {
    // Setup environment
    let (response, memory_size) = {
        // Read url from memory
        let view = ctx.data().view(&ctx);
        let memory_size = view.data_size() as usize;
        let address = read_string(&view, url, url_len).unwrap();

        // Get request
        let response = ureq::get(&address).call().unwrap();
        let capacity = match response
            .header("Content-Length")
            .map(|it| it.parse::<usize>())
        {
            Some(Ok(len)) => len,
            _ => 1024,
        };
        let mut buffer = Vec::with_capacity(capacity);
        let mut reader = response.into_reader();
        reader.read_to_end(&mut buffer).unwrap();
        (buffer, memory_size)
    };

    // If the response is too big, grow memory
    if 1114112 + response.len() > memory_size {
        let delta = (1114112 + response.len() - memory_size) / wasmer::WASM_PAGE_SIZE + 1;
        let memory = ctx.data().get_memory().clone();
        memory.grow(&mut ctx, delta as u32).unwrap();
    }

    // Write response as string [ptr, cap, len] to wasm memory and return pointer
    let view = ctx.data().view(&ctx);
    view.write(1114112, &u32::to_le_bytes(1114124)).unwrap();
    view.write(1114116, &u32::to_le_bytes(response.len() as u32))
        .unwrap();
    view.write(1114120, &u32::to_le_bytes(response.len() as u32))
        .unwrap();
    view.write(1114124, &response).unwrap();
    1114112
}

fn main() -> Result<()> {
    let wasm_bytes = wat2wasm(
        br#"
(module
  (type (;0;) (func (param i32 i32) (result i32)))
  (type (;1;) (func (result i32)))
  (type (;2;) (func))
  (import "env" "http_get" (func (;0;) (type 0)))
  (func (;1;) (type 1) (result i32)
    i32.const 1048576
    i32.const 45
    call 0
    i32.const 8
    i32.add
    i32.load)
  (func (;2;) (type 2))
  (func (;3;) (type 2)
    call 2
    call 2)
  (func (;4;) (type 1) (result i32)
    call 1
    call 3)
  (table (;0;) 1 1 funcref)
  (memory (;0;) 17)
  (global (;0;) (mut i32) (i32.const 1048576))
  (export "memory" (memory 0))
  (export "fetch" (func 4))
  (data (;0;) (i32.const 1048576) "https://postman-echo.com/bytes/5/mb?type=json"))
"#,
    )?;

    // Load module
    let mut store = Store::default();
    let module = Module::new(&store, wasm_bytes)?;

    // Add host functions
    let function_env = FunctionEnv::new(&mut store, ExampleEnv { memory: None });
    let import_object = imports! {
        // We use the default namespace "env".
        "env" => {
            // And call our function "http_get".
            "http_get" => Function::new_typed_with_env(&mut store, &function_env, http_get),
        }
    };

    // Create instance
    let instance = Instance::new(&mut store, &module, &import_object)?;
    let memory = instance.exports.get_memory("memory")?;

    // Give reference to memory
    function_env.as_mut(&mut store).set_memory(memory.clone());

    // Call function
    let fetch = instance.exports.get_function("fetch")?;
    let result = fetch.call(&mut store, &[])?;
    println!("Response size: {result:?}");

    Ok(())
}
