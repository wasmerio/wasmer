#![cfg(test)]
use wasmer_runtime::{compile, Ctx, Func};
use wasmer_wasi::{state::*, *};

use std::ffi::c_void;

#[cfg(not(feature = "singlepass"))]
#[test]
fn serializing_works() {
    let args = vec![
        b"program_name".into_iter().cloned().collect(),
        b"arg1".into_iter().cloned().collect(),
    ];
    let envs = vec![
        b"PATH=/bin".into_iter().cloned().collect(),
        b"GOROOT=$HOME/.cargo/bin".into_iter().cloned().collect(),
    ];
    let wasm_binary = include_bytes!("../wasitests/fd_read.wasm");
    let module = compile(&wasm_binary[..])
        .map_err(|e| format!("Can't compile module: {:?}", e))
        .unwrap();

    let wasi_version = get_wasi_version(&module, true).expect("WASI module");
    let import_object = generate_import_object_for_version(
        wasi_version,
        args.clone(),
        envs.clone(),
        vec![],
        vec![(
            ".".to_string(),
            std::path::PathBuf::from("wasitests/test_fs/hamlet"),
        )],
    );

    let state_bytes = {
        let instance = module.instantiate(&import_object).unwrap();

        let start: Func<(), ()> = instance.func("_start").unwrap();
        start.call().unwrap();
        let state = get_wasi_state(instance.context());

        assert_eq!(state.args, args);
        assert_eq!(state.envs, envs);
        let bytes = state.freeze().unwrap();

        bytes
    };

    let mut instance = module.instantiate(&import_object).unwrap();

    let wasi_state = Box::new(WasiState::unfreeze(&state_bytes).unwrap());

    instance.context_mut().data = Box::into_raw(wasi_state) as *mut c_void;

    let second_entry: Func<(), i32> = instance.func("second_entry").unwrap();
    let result = second_entry.call().unwrap();
    assert_eq!(result, true as i32);
}

#[allow(clippy::mut_from_ref)]
pub(crate) fn get_wasi_state(ctx: &Ctx) -> &mut WasiState {
    unsafe { state::get_wasi_state(&mut *(ctx as *const Ctx as *mut Ctx)) }
}
