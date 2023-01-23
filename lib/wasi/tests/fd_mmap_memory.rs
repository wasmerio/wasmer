// TODO: make this a sensible test before merge
// NOTE: commented out since it's mostly here for demonstration purposes.
// use std::sync::Arc;

// use wasmer::{BaseTunables, Engine, Module, Store, Tunables};
// use wasmer_vm::VMMemory;
// use wasmer_wasi::{
//     fd_memory::{VMOwnedMemory, VMSharedMemory},
//     WasiControlPlane, WasiEnv, WasiState,
// };

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
//     let mut wasi_state_builder = WasiState::builder("fdmem");

//     let mut store = Store::default();
//     // let engine = wasmer_compiler_cranelift::Cranelift::default();

//     // let engine = Engine::default();

//     let code = std::fs::read("/home/theduke/dev/github.com/wasmerio/wasix-integration-tests/rust/simple/target/wasm32-wasmer-wasi/debug/examples/spawn_threads_and_sleep.wasm").unwrap();
//     let module = Module::new(&store, code).unwrap();

//     let control_plane = WasiControlPlane::default();
//     let wasi_process = control_plane
//         .new_process()
//         .expect("creating processes on new control planes should always work");
//     let wasi_thread = wasi_process
//         .new_thread()
//         .expect("creating the main thread should always work");

//     let mut wasi_env = wasi_state_builder
//         // .stdin(Box::new(stdin_pipe.clone()))
//         // .stdout(Box::new(stdout.clone()))
//         // .stderr(Box::new(stdout.clone()))
//         .args(["1000"])
//         .finalize(&mut store)
//         .unwrap();

//     let mut env = WasiEnv::new_ext(
//         Arc::new(state),
//         self.compiled_modules.clone(),
//         wasi_process.clone(),
//         wasi_thread,
//         self.runtime.clone(),
//     );

//     // Generate an `ImportObject`.
//     let instance = wasmer_wasi::build_wasi_instance(&module, &mut wasi_env, &mut store).unwrap();

//     // Let's call the `_start` function, which is our `main` function in Rust.
//     let start = instance.exports.get_function("_start").unwrap();
//     start.call(&mut store, &[]).unwrap();
// }
