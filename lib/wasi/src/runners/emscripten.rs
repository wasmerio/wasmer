#![cfg(feature = "webc_runner_rt_wasi")]
//! WebC container support for running Emscripten modules

use crate::runners::WapmContainer;
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;
use std::sync::Arc;
use wasmer::{Cranelift, FunctionEnv, Instance, Module, Store};
use wasmer_emscripten::{
    generate_emscripten_env, is_emscripten_module, run_emscripten_instance, EmEnv,
    EmscriptenGlobals,
};
use webc::{Command, WebCMmap};

#[derive(Debug, Default, Clone, PartialEq, PartialOrd, Hash, Serialize, Deserialize)]
pub struct EmscriptenRunner {
    args: Vec<String>,
}

impl EmscriptenRunner {
    pub fn set_args(&mut self, args: Vec<String>) {
        self.args = args;
    }
}

impl crate::runners::Runner for EmscriptenRunner {
    type Output = ();

    fn can_run_command(&self, _: &str, command: &Command) -> Result<bool, Box<dyn StdError>> {
        Ok(command
            .runner
            .starts_with("https://webc.org/runner/emscripten"))
    }

    fn run_command(
        &mut self,
        command_name: &str,
        _command: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Box<dyn StdError>> {
        let atom_name = container.get_atom_name_for_command("emscripten", command_name)?;
        let main_args = container.get_main_args_for_command(command_name);
        let atom_bytes = container.get_atom(&container.get_package_name(), &atom_name)?;

        let compiler = Cranelift::default();
        let mut store = Store::new(compiler);
        let mut module = Module::new(&store, atom_bytes)?;
        module.set_name(&atom_name);

        let (mut globals, env) =
            prepare_emscripten_env(&mut store, &module, container.webc.clone(), &atom_name)?;

        exec_module(
            &mut store,
            &module,
            &mut globals,
            env,
            container.webc.clone(),
            &atom_name,
            main_args.unwrap_or_default(),
        )?;

        Ok(())
    }
}

fn prepare_emscripten_env(
    store: &mut Store,
    module: &Module,
    _atom: Arc<WebCMmap>,
    name: &str,
) -> Result<(EmscriptenGlobals, FunctionEnv<EmEnv>), anyhow::Error> {
    if !is_emscripten_module(module) {
        return Err(anyhow!("Atom {name:?} is not an emscripten module"));
    }

    let env = FunctionEnv::new(store, EmEnv::new());
    let emscripten_globals = EmscriptenGlobals::new(store, &env, module);
    let emscripten_globals = emscripten_globals.map_err(|e| anyhow!("{}", e))?;
    env.as_mut(store)
        .set_data(&emscripten_globals.data, Default::default());

    Ok((emscripten_globals, env))
}

fn exec_module(
    store: &mut Store,
    module: &Module,
    globals: &mut EmscriptenGlobals,
    em_env: FunctionEnv<EmEnv>,
    _atom: Arc<WebCMmap>,
    name: &str,
    args: Vec<String>,
) -> Result<(), anyhow::Error> {
    let import_object = generate_emscripten_env(store, &em_env, globals);

    let mut instance = Instance::new(store, module, &import_object)
        .map_err(|e| anyhow!("Cant instantiate emscripten module {name:?}: {e}"))?;

    run_emscripten_instance(
        &mut instance,
        em_env.into_mut(store),
        globals,
        name,
        args.iter().map(|arg| arg.as_str()).collect(),
        None,
    )?;

    Ok(())
}
