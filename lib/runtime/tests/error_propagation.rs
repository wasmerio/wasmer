#[test]
fn error_propagation() {
    use std::convert::Infallible;
    use wabt::wat2wasm;
    use wasmer_runtime::{compile, error::RuntimeError, imports, Ctx, Func};

    static WAT: &'static str = r#"
        (module
        (type (;0;) (func))
        (import "env" "ret_err" (func $ret_err (type 0)))
        (func $call_panic
            call $ret_err
        )
        (export "call_err" (func $call_panic))
        )
    "#;

    #[derive(Debug)]
    struct ExitCode {
        code: i32,
    }

    fn ret_err(_ctx: &mut Ctx) -> Result<Infallible, ExitCode> {
        Err(ExitCode { code: 42 })
    }

    let wasm = wat2wasm(WAT).unwrap();

    let module = compile(&wasm).unwrap();

    let instance = module
        .instantiate(&imports! {
          "env" => {
              "ret_err" => Func::new(ret_err),
          },
        })
        .unwrap();

    let foo: Func<(), ()> = instance.func("call_err").unwrap();

    let result = foo.call();

    if let Err(RuntimeError::Error { data }) = result {
        let exit_code = data.downcast::<ExitCode>().unwrap();
        assert_eq!(exit_code.code, 42);
    } else {
        panic!("didn't return RuntimeError::Error")
    }
}
