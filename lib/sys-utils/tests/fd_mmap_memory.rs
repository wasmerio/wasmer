// // TODO: make this a sensible test before merge NOTE: commented out since it's mostly here for demonstration purposes.
// use std::sync::Arc;

// use wasmer::{BaseTunables, Engine, Module, Store, Tunables};
// use wasmer_vm::VMMemory;
// use wasmer_wasix::{
//     bin_factory::spawn_exec_module, virtual_fs::host_fs::File, BusSpawnedProcessJoin,
//     PluggableRuntimeImplementation, WasiControlPlane, WasiEnv, WasiRuntime,
//     WasiState,
// };

// use wasmer_sys_utils::memory::fd_memory::{VMOwnedMemory, VMSharedMemory};

// struct FdTunables {
//     base: BaseTunables,
// }

// impl Tunables for FdTunables {
//     fn memory_style(&self, memory: &wasmer::MemoryType) -> wasmer::vm::MemoryStyle {
//         self.base.memory_style(memory)
//     }

//     fn table_style(&self, table: &wasmer::TableType) -> wasmer::vm::TableStyle {
//         self.base.table_style(table)
//     }

//     fn create_host_memory(
//         &self,
//         ty: &wasmer::MemoryType,
//         style: &wasmer::vm::MemoryStyle,
//     ) -> Result<wasmer::vm::VMMemory, wasmer::MemoryError> {
//         Ok(VMMemory(Box::new(VMOwnedMemory::new(ty, style)?)))
//     }

//     unsafe fn create_vm_memory(
//         &self,
//         ty: &wasmer::MemoryType,
//         style: &wasmer::vm::MemoryStyle,
//         vm_definition_location: std::ptr::NonNull<wasmer::vm::VMMemoryDefinition>,
//     ) -> Result<wasmer::vm::VMMemory, wasmer::MemoryError> {
//         if ty.shared {
//             let mem = VMSharedMemory::from_definition(ty, style, vm_definition_location)?;
//             Ok(VMMemory(Box::new(mem)))
//         } else {
//             let mem = VMOwnedMemory::from_definition(ty, style, vm_definition_location)?;
//             Ok(VMMemory(Box::new(mem)))
//         }
//     }

//     fn create_host_table(
//         &self,
//         ty: &wasmer::TableType,
//         style: &wasmer::vm::TableStyle,
//     ) -> Result<wasmer::vm::VMTable, String> {
//         self.base.create_host_table(ty, style)
//     }

//     unsafe fn create_vm_table(
//         &self,
//         ty: &wasmer::TableType,
//         style: &wasmer::vm::TableStyle,
//         vm_definition_location: std::ptr::NonNull<wasmer::vm::VMTableDefinition>,
//     ) -> Result<wasmer::vm::VMTable, String> {
//         self.base.create_vm_table(ty, style, vm_definition_location)
//     }
// }

// #[test]
// fn test_fd_mmap_memory() {
//     tracing_subscriber::fmt()
//         .with_level(true)
//         .with_test_writer()
//         .with_max_level(tracing::Level::TRACE)
//         .try_init()
//         .unwrap();

//     let mut store = Store::default();
//     // let engine = wasmer_compiler_cranelift::Cranelift::default();

//     // let engine = Engine::default();

//     let code = std::fs::read("/home/theduke/dev/github.com/wasmerio/wasix-integration-tests/rust/simple/target/wasm32-wasmer-wasi/debug/examples/spawn_threads_and_sleep.wasm").unwrap();
//     let module = Module::new(&store, code).unwrap();

//     let control_plane = WasiControlPlane::default();

//     let rt = Arc::new(PluggableRuntimeImplementation::default());
//     let wasi_env = WasiState::builder("fdmem")
//         .args(["500", "100", "10"])
//         .runtime(&rt)
//         .finalize_with(&mut store, &control_plane)
//         .unwrap();

//     // let rt = wasi_env
//     //     .data(&store)
//     //     .runtime()
//     //     .as_any()
//     //     .unwrap()
//     //     .downcast_ref::<PluggableRuntimeImplementation>()
//     //     .unwrap()
//     //     .clone();

//     // Generate an `ImportObject`.
//     // let instance = wasmer_wasix::build_wasi_instance(&module, &mut wasi_env, &mut store).unwrap();

//     let config = wasmer_wasix::wasmer_vbus::SpawnOptionsConfig {
//         reuse: false,
//         env: wasi_env.data(&store).clone(),
//         remote_instance: None,
//         access_token: None,
//     };

//     let rt2: Arc<dyn WasiRuntime + Send + Sync> = rt.clone();
//     let bus = spawn_exec_module(module, store, config, &rt2).unwrap();

//     dbg!("spawned, sleeping!");
//     let _joiner = BusSpawnedProcessJoin::new(bus);

//     std::thread::sleep(std::time::Duration::from_secs(100000000000000));

//     // Let's call the `_start` function, which is our `main` function in Rust.
//     // let start = instance.exports.get_function("_start").unwrap();
//     // start.call(&mut store, &[]).unwrap();
// }
