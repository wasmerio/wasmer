#![cfg(test)]

#[macro_use]
mod utils;

wasmer_backends! {
    use wasmer::{compiler::compile_with, vm::Ctx, Func};
    use wasmer_wasi::{state::*, *};
    use std::ffi::c_void;

    // TODO: fix this test!
    #[ignore]
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
        let wasm_binary = include_bytes!("wasi_test_resources/unstable/fd_read.wasm");
        let module = compile_with(&wasm_binary[..], &*get_compiler())
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
                std::path::PathBuf::from("wasi_test_resources/test_fs/hamlet"),
            )],
        );

        let state_bytes = {
            let mut instance = module.instantiate(&import_object).unwrap();

            let start: Func<(), ()> = instance.exports.get("_start").unwrap();
            start.call().unwrap();
            let state = get_wasi_state(instance.context_mut());

            assert_eq!(state.args, args);
            assert_eq!(state.envs, envs);
            let bytes = state.freeze().unwrap();

            bytes
        };

        let mut instance = module.instantiate(&import_object).unwrap();

        let wasi_state = Box::new(WasiState::unfreeze(&state_bytes).unwrap());

        instance.context_mut().data = Box::into_raw(wasi_state) as *mut c_void;

        let second_entry: Func<(), i32> = instance.exports.get("second_entry").unwrap();
        let result = second_entry.call().unwrap();
        assert_eq!(result, true as i32);
    }

    pub(crate) fn get_wasi_state(ctx: &mut Ctx) -> &mut WasiState {
        unsafe { state::get_wasi_state(ctx) }
    }
}
