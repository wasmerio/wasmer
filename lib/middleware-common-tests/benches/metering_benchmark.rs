#[macro_use]
extern crate criterion;

use criterion::black_box;
use criterion::{Benchmark, Criterion};

use wabt::wat2wasm;

use wasmer_middleware_common::metering::Metering;
use wasmer_runtime_core::vm::Ctx;
use wasmer_runtime_core::{backend::Compiler, compile_with, imports, Func};

//export function add_to(x: i32, y: i32): i32 {
//   for(var i = 0; i < x; i++){
//     if(i % 1 == 0){
//       y += i;
//     } else {
//       y *= i
//     }
//   }
//   return y;
//}
static WAT: &'static str = r#"
            (module
              (type $t0 (func (param i32 i32) (result i32)))
              (type $t1 (func))
              (func $add_to (export "add_to") (type $t0) (param $p0 i32) (param $p1 i32) (result i32)
                (local $l0 i32)
                block $B0
                  i32.const 0
                  set_local $l0
                  loop $L1
                    get_local $l0
                    get_local $p0
                    i32.lt_s
                    i32.eqz
                    br_if $B0
                    get_local $l0
                    i32.const 1
                    i32.rem_s
                    i32.const 0
                    i32.eq
                    if $I2
                      get_local $p1
                      get_local $l0
                      i32.add
                      set_local $p1
                    else
                      get_local $p1
                      get_local $l0
                      i32.mul
                      set_local $p1
                    end
                    get_local $l0
                    i32.const 1
                    i32.add
                    set_local $l0
                    br $L1
                    unreachable
                  end
                  unreachable
                end
                get_local $p1)
              (func $f1 (type $t1))
              (table $table (export "table") 1 anyfunc)
              (memory $memory (export "memory") 0)
              (global $g0 i32 (i32.const 8))
              (elem (i32.const 0) $f1))
        "#;

static WAT_GAS: &'static str = r#"
            (module
              (type $t0 (func (param i32 i32) (result i32)))
              (type $t1 (func))
              (type $t2 (func (param i32)))
              (import "env" "gas" (func $env.gas (type $t2)))
              (func $add_to (type $t0) (param $p0 i32) (param $p1 i32) (result i32)
                (local $l0 i32)
                i32.const 3
                call $env.gas
                block $B0
                  i32.const 5
                  call $env.gas
                  i32.const 0
                  set_local $l0
                  loop $L1
                    i32.const 18
                    call $env.gas
                    get_local $l0
                    get_local $p0
                    i32.lt_s
                    i32.eqz
                    br_if $B0
                    get_local $l0
                    i32.const 1
                    i32.rem_s
                    i32.const 0
                    i32.eq
                    if $I2
                      i32.const 5
                      call $env.gas
                      get_local $p1
                      get_local $l0
                      i32.add
                      set_local $p1
                    else
                      i32.const 5
                      call $env.gas
                      get_local $p1
                      get_local $l0
                      i32.mul
                      set_local $p1
                    end
                    get_local $l0
                    i32.const 1
                    i32.add
                    set_local $l0
                    br $L1
                    unreachable
                  end
                  unreachable
                end
                get_local $p1)
              (func $f2 (type $t1)
                i32.const 1
                call $env.gas)
              (table $table 1 anyfunc)
              (memory $memory 0)
              (global $g0 i32 (i32.const 8))
              (export "memory" (memory 0))
              (export "table" (table 0))
              (export "add_to" (func $add_to))
              (elem (i32.const 0) $f2))
        "#;

#[cfg(feature = "llvm")]
fn get_compiler(limit: u64, metering: bool) -> impl Compiler {
    use wasmer_llvm_backend::ModuleCodeGenerator;
    use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
    let c: StreamingCompiler<ModuleCodeGenerator, _, _, _, _> = StreamingCompiler::new(move || {
        let mut chain = MiddlewareChain::new();
        if metering {
            chain.push(Metering::new(limit));
        }
        chain
    });

    c
}

#[cfg(feature = "singlepass")]
fn get_compiler(limit: u64, metering: bool) -> impl Compiler {
    use wasmer_runtime_core::codegen::{MiddlewareChain, StreamingCompiler};
    use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;
    let c: StreamingCompiler<SinglePassMCG, _, _, _, _> = StreamingCompiler::new(move || {
        let mut chain = MiddlewareChain::new();
        if metering {
            chain.push(Metering::new(limit));
        }
        chain
    });
    c
}

#[cfg(not(any(feature = "llvm", feature = "clif", feature = "singlepass")))]
compile_error!("compiler not specified, activate a compiler via features");

#[cfg(feature = "clif")]
fn get_compiler(_limit: u64, metering: bool) -> impl Compiler {
    compile_error!("cranelift does not implement metering");
    use wasmer_clif_backend::CraneliftCompiler;
    CraneliftCompiler::new()
}

fn gas(ctx: &mut Ctx, gas_amount: u32) {
    use wasmer_middleware_common::metering;
    let used = metering::get_points_used_ctx(ctx);
    metering::set_points_used_ctx(ctx, used + u64::from(gas_amount));
    ()
}

fn bench_metering(c: &mut Criterion) {
    use wasmer_middleware_common::metering;

    c.bench(
        "Meter",
        Benchmark::new("No Metering", |b| {
            let compiler = get_compiler(0, false);
            let wasm_binary = wat2wasm(WAT).unwrap();
            let module = compile_with(&wasm_binary, &compiler).unwrap();
            let import_object = imports! {};
            let instance = module.instantiate(&import_object).unwrap();
            let add_to: Func<(i32, i32), i32> = instance.func("add_to").unwrap();
            b.iter(|| black_box(add_to.call(100, 4)))
        })
        .with_function("Gas Metering", |b| {
            let compiler = get_compiler(0, false);
            let gas_wasm_binary = wat2wasm(WAT_GAS).unwrap();
            let gas_module = compile_with(&gas_wasm_binary, &compiler).unwrap();
            let gas_import_object = imports! {
                "env" => {
                   "gas" => Func::new(gas),
                },
            };
            let gas_instance = gas_module.instantiate(&gas_import_object).unwrap();
            let gas_add_to: Func<(i32, i32), i32> = gas_instance.func("add_to").unwrap();
            b.iter(|| black_box(gas_add_to.call(100, 4)))
        })
        .with_function("Built-in Metering", |b| {
            let metering_compiler = get_compiler(std::u64::MAX, true);
            let wasm_binary = wat2wasm(WAT).unwrap();
            let metering_module = compile_with(&wasm_binary, &metering_compiler).unwrap();
            let metering_import_object = imports! {};
            let mut metering_instance = metering_module
                .instantiate(&metering_import_object)
                .unwrap();
            metering::set_points_used(&mut metering_instance, 0u64);
            let metering_add_to: Func<(i32, i32), i32> = metering_instance.func("add_to").unwrap();
            b.iter(|| black_box(metering_add_to.call(100, 4)))
        }),
    );
}

criterion_group!(benches, bench_metering);
criterion_main!(benches);
