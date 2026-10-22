use super::*;
use crate::syscalls::*;

/// ### `callback_signal()`
/// Sets the callback to invoke signals
///
/// ### Parameters
///
/// * `name` - Name of the function that will be invoked
#[instrument(level = "trace", skip_all, fields(name = field::Empty, funct_is_some = field::Empty), ret)]
pub fn callback_signal<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    name: WasmPtr<u8, M>,
    name_len: M::Offset,
) -> Result<(), WasiError> {
    let env = ctx.data();
    let memory = unsafe { env.memory_view(&ctx) };
    let name = match name.read_utf8_string(&memory, name_len) {
        Ok(a) => a,
        Err(err) => {
            warn!(
                "failed to access memory that holds the name of the signal callback: {}",
                err
            );
            return Ok(());
        }
    };
    Span::current().record("name", name.as_str());

    let funct = env
        .inner()
        .main_module_instance_handles()
        .instance
        .exports
        .get_typed_function::<i32, ()>(&ctx, &name)
        .ok();
    Span::current().record("funct_is_some", funct.is_some());
    if funct.is_none() {
        warn!(%name, "signal callback must export a function taking i32 and returning nothing");
        return Ok(());
    }

    {
        let mut env_inner = ctx.data_mut().inner_mut();
        env_inner.main_module_instance_handles_mut().signal_handler = Some(name.clone());
    }
    {
        let mut process_handler = ctx.data().state.signal_handler.lock().unwrap();
        process_handler.get_or_insert(name);
    }

    WasiEnv::do_pending_operations(&mut ctx)?;

    Ok(())
}

#[cfg(all(test, feature = "sys"))]
mod tests {
    use super::*;
    use crate::WasiFunctionEnv;
    use wasmer::{Instance, Module, Store, Value};

    fn instance() -> (Store, Instance, WasiFunctionEnv) {
        let mut store = Store::default();
        let module = Module::new(
            &store,
            r#"(module
                (import "wasix_32v1" "callback_signal" (func (param i32 i32)))
                (memory (export "memory") 32)
                (global $hits (export "hits") (mut i32) (i32.const 0))
                (func (export "handler") (param i32)
                    (global.set $hits (i32.add (global.get $hits) (i32.const 1))))
                (func (export "wrong")))"#,
        )
        .unwrap();
        let (instance, env) = WasiEnv::builder("signal-callback")
            .engine(store.engine().clone())
            .instantiate(module, &mut store)
            .unwrap();
        (store, instance, env)
    }

    fn register(store: &mut Store, instance: &Instance, env: &WasiFunctionEnv, name: &str) {
        instance
            .exports
            .get_memory("memory")
            .unwrap()
            .view(store)
            .write(0, name.as_bytes())
            .unwrap();
        callback_signal::<Memory32>(
            env.env.clone().into_mut(store),
            WasmPtr::new(0),
            name.len() as u32,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn invalid_callback_preserves_pending_default_signal() {
        for name in ["missing", "wrong"] {
            let (mut store, instance, env) = instance();
            env.data(&store).thread.signal(Signal::Sigpipe);

            register(&mut store, &instance, &env, name);

            assert!(
                env.data(&store)
                    .state
                    .signal_handler
                    .lock()
                    .unwrap()
                    .is_none()
            );
            assert!(env.data(&store).thread.has_signal(&[Signal::Sigpipe]));
            let result = WasiEnv::process_signals_and_exit(&mut env.env.into_mut(&mut store));
            assert!(matches!(result, Err(WasiError::Exit(code)) if code == Errno::Pipe.into()));
        }
    }

    #[tokio::test]
    async fn invalid_callback_preserves_registered_handler_and_pending_signal() {
        for name in ["missing", "wrong"] {
            let (mut store, instance, env) = instance();
            register(&mut store, &instance, &env, "handler");
            env.data(&store).thread.signal(Signal::Sigusr1);

            register(&mut store, &instance, &env, name);
            WasiEnv::process_signals_and_exit(&mut env.env.clone().into_mut(&mut store))
                .unwrap()
                .unwrap();

            assert_eq!(
                env.data(&store)
                    .inner()
                    .main_module_instance_handles()
                    .signal_handler
                    .as_deref(),
                Some("handler")
            );
            assert_eq!(
                env.data(&store)
                    .state
                    .signal_handler
                    .lock()
                    .unwrap()
                    .as_deref(),
                Some("handler")
            );
            assert_eq!(
                instance.exports.get_global("hits").unwrap().get(&mut store),
                Value::I32(1)
            );
        }
    }

    #[tokio::test]
    async fn shutdown_wakeup_never_calls_guest_handler() {
        let (mut store, instance, env) = instance();
        register(&mut store, &instance, &env, "handler");
        WasiEnv::process_signals_internal(
            &mut env.env.clone().into_mut(&mut store),
            vec![Signal::Sigwakeup],
        )
        .unwrap();
        env.data(&store).process.terminate(ExitCode::from(37));
        env.data(&store).process.terminate(ExitCode::from(0));
        let result = WasiEnv::process_signals_and_exit(&mut env.env.into_mut(&mut store));
        assert!(matches!(result, Err(WasiError::Exit(code)) if code == ExitCode::from(37)));
        assert_eq!(
            instance.exports.get_global("hits").unwrap().get(&mut store),
            Value::I32(0)
        );
    }
}
