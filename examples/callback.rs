/// This example demonstrates the use of callbacks: calling functions (Host and Wasm)
/// passed to us from the Wasm via hostcall
use wasmer_runtime::{compile_with, compiler_for_backend, func, imports, Backend, Ctx};
use wasmer_runtime_core::{structures::TypedIndex, types::TableIndex};

static WASM: &'static str = "examples/callback-guest/callback-guest.wasm";

/// This function matches our arbitrarily decided callback signature
/// in this example we'll only call functions that take no arguments and return one value
fn host_callback(_ctx: &mut Ctx) -> u32 {
    55
}

fn call_guest_fn(ctx: &mut Ctx, guest_fn: u32) -> u32 {
    // We get a TableIndex from our raw value passed in
    let guest_fn_typed = TableIndex::new(guest_fn as usize);
    // and use it to call the corresponding function
    let result = ctx.call_with_table_index(guest_fn_typed, &[]).unwrap();

    println!("Guest fn {} returned {:?}", guest_fn, result);

    0
}

fn main() {
    let wasm_bytes =
        std::fs::read(WASM).expect(&format!("Could not read in WASM plugin at {}", WASM));

    let imports = imports! {
        "env" => {
            "call_guest_fn" => func!(call_guest_fn),
            "call_guest_fn2" => func!(call_guest_fn),
            "host_callback" => func!(host_callback),
        },
    };

    let compiler = compiler_for_backend(Backend::default()).unwrap();
    let module = compile_with(&wasm_bytes[..], compiler.as_ref()).unwrap();
    let instance = module
        .instantiate(&imports)
        .expect("failed to instantiate wasm module");

    let entry_point = instance.func::<(u32, u32), u32>("main").unwrap();

    entry_point.call(0, 0).expect("START");
}
