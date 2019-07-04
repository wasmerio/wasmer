
use wasmer_runtime::{
    imports,
    instantiate,
    error,
    func,
    Ctx,
    compile,
    Func,
    Value,
};
use wabt::wat2wasm;

static WAT: &'static str = r#"
        (module
          (type $t0 (func (param i32 i32) (result i32)))
          (import "env" "host_func" (func $main.host_func (type $t0)))
          (func $call_host (export "call_host") (type $t0) (param $p0 i32) (param $p1 i32) (result i32)
            get_local $p0
            get_local $p1
            call $main.host_func))
    "#;

#[test]
fn test_polymorphic_host_function(){
    let wasm = wat2wasm(WAT).unwrap();
    let module = compile(&wasm).unwrap();
    let instance = module
        .instantiate(&imports! {
          "env" => {
              "host_func" => FuncPolymorphic::new(host_function),
          },
        })
        .unwrap();

    let foo: Func<(u32, u32), u32> = instance.func("call_host").unwrap();

    let result = foo.call(1, 2);
    if let Ok(res) = result {
        assert!(res == 3);
    } else {
        panic!("error result");
    }

}

fn host_function(ctx: &mut Ctx, params: &[Value], results: &mut [Value]) {
    // sum params 1 and 2
    // store sum in results
}