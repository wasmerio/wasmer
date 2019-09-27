use rayon::prelude::*;
use wasmer_runtime::{compile_with, compiler_for_backend, func, imports, instantiate, Backend};
use wasmer_runtime_core::{
    memory::ptr::{Array, WasmPtr},
    vm::Ctx,
};

static PLUGIN_LOCATION: &'static str = "../parallel-guest.wasm";

fn get_hashed_password(ctx: &mut Ctx, ptr: WasmPtr<u8, Array>, len: u32) -> u32 {
    // "hard" password - 7 characters
    //let password = b"2ab96390c7dbe3439de74d0c9b0b1767";
    // "easy" password - 5 characters
    let password = b"ab56b4d92b40713acc5af89985d4b786";
    let memory = ctx.memory(0);
    if let Some(writer) = ptr.deref(memory, 0, len) {
        for (i, byte) in password.iter().enumerate() {
            writer[i].set(*byte)
        }

        0
    } else {
        u32::max_value()
    }
}

#[repr(C)]
struct RetStr {
    ptr: u32,
    len: u32,
}

fn print_char(_cxt: &mut Ctx, c: u32) {
    print!("{}", c as u8 as char);
}

fn main() {
    let wasm_bytes = std::fs::read(PLUGIN_LOCATION).expect(&format!(
        "Could not read in WASM plugin at {}",
        PLUGIN_LOCATION
    ));

    let imports = imports! {
        "env" => {
            "get_hashed_password" => func!(get_hashed_password),
            "print_char" => func!(print_char),
        },
    };
    let compiler = compiler_for_backend(Backend::default()).unwrap();
    let module = compile_with(&wasm_bytes[..], compiler.as_ref()).unwrap();

    println!("Parallel");
    let start_ts = time::SteadyTime::now();
    for outer in 0..1000u64 {
        let start = outer * 1000;
        let end = start + 1000;
        let out = (start..=end)
            .into_par_iter()
            .filter_map(|i| {
                let instance = module
                    .clone()
                    .instantiate(&imports)
                    .expect("failed to instantiate wasm module");
                let check_password = instance.func::<(u64, u64), u64>("check_password").unwrap();
                let j = i * 10000;
                let result = check_password.call(j, j + 10000).unwrap();
                print!(".");
                use std::io::Write;
                std::io::stdout().flush().unwrap();
                if result != 0 {
                    let res: RetStr = unsafe { std::mem::transmute(result) };

                    let ctx = instance.context();
                    let memory = ctx.memory(0);
                    let wasm_ptr: WasmPtr<u8, Array> = WasmPtr::new(res.ptr);
                    let password_str = wasm_ptr
                        .get_utf8_string(memory, res.len)
                        .unwrap()
                        .to_string();
                    Some(password_str)
                } else {
                    None
                }
            })
            .find_first(|_: &String| true);
        if out.is_some() {
            let end_ts = time::SteadyTime::now();
            let delta = end_ts - start_ts;
            println!(
                "Password cracked: \"{}\" in {}.{:03}",
                out.unwrap(),
                delta.num_seconds(),
                (delta.num_milliseconds() % 1000),
            );
            break;
        }
    }

    println!("Serial:");
    let start_ts = time::SteadyTime::now();
    let instance =
        instantiate(&wasm_bytes[..], &imports).expect("failed to instantiate wasm module");

    let check_password = instance.func::<(u64, u64), u64>("check_password").unwrap();

    let mut out: Option<RetStr> = None;
    for i in (0..=u64::max_value()).step_by(10000) {
        let result = check_password.call(i, i + 10000).unwrap();
        print!(".");
        use std::io::Write;
        std::io::stdout().flush().unwrap();
        if result != 0 {
            out = Some(unsafe { std::mem::transmute(result) });
            break;
        }
    }
    println!("");

    if let Some(res) = out {
        let ctx = instance.context();
        let memory = ctx.memory(0);
        let wasm_ptr: WasmPtr<u8, Array> = WasmPtr::new(res.ptr);

        let password_str = wasm_ptr.get_utf8_string(memory, res.len).unwrap();

        let end_ts = time::SteadyTime::now();
        let delta = end_ts - start_ts;
        println!(
            "Password cracked: \"{}\" in {}.{:03}",
            password_str,
            delta.num_seconds(),
            (delta.num_milliseconds() % 1000),
        );
    } else {
        println!("Password not found!");
    }
}
