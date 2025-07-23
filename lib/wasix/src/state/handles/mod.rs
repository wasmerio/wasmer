mod global;
mod thread_local;

#[cfg(any(feature = "sys", feature = "sys-minimal"))]
pub(crate) use global::*;
#[cfg(feature = "js")]
pub(crate) use thread_local::*;

use tracing::{error, trace};
use wasmer::{
    AsStoreMut, AsStoreRef, Function, Global, Instance, Memory, MemoryView, Module, Table,
    TypedFunction, Value,
};
use wasmer_wasix_types::wasi::Errno;

use super::Linker;

/// Various [`TypedFunction`] and [`Global`] handles for an active WASI(X) instance.
///
/// Used to access and modify runtime state.
// TODO: make fields private
#[derive(Debug, Clone)]
pub struct WasiModuleInstanceHandles {
    // TODO: the two fields below are instance specific, while all others are module specific.
    // Should be split up.
    /// Represents a reference to the memory
    pub(crate) memory: Memory,
    pub(crate) instance: wasmer::Instance,

    /// Points to the indirect function table
    pub(crate) indirect_function_table: Option<Table>,

    /// Points to the current location of the memory stack pointer
    pub(crate) stack_pointer: Option<Global>,

    /// Points to the end of the data section
    pub(crate) data_end: Option<Global>,

    /// Points to the lower end of the stack
    pub(crate) stack_low: Option<Global>,

    /// Points to the higher end of the stack
    pub(crate) stack_high: Option<Global>,

    /// Points to the start of the TLS area
    pub(crate) tls_base: Option<Global>,

    /// Main function that will be invoked (name = "_start")
    pub(crate) start: Option<TypedFunction<(), ()>>,

    /// Function thats invoked to initialize the WASM module (name = "_initialize")
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) initialize: Option<TypedFunction<(), ()>>,

    /// Represents the callback for spawning a thread (name = "wasi_thread_start")
    /// (due to limitations with i64 in browsers the parameters are broken into i32 pairs)
    /// [this takes a user_data field]
    pub(crate) thread_spawn: Option<TypedFunction<(i32, i32), ()>>,

    /// Represents the callback for signals (name = "__wasm_signal")
    /// Signals are triggered asynchronously at idle times of the process
    // TODO: why is this here? It can exist in WasiEnv
    pub(crate) signal: Option<TypedFunction<i32, ()>>,

    /// Flag that indicates if the signal callback has been set by the WASM
    /// process - if it has not been set then the runtime behaves differently
    /// when a CTRL-C is pressed.
    pub(crate) signal_set: bool,

    /// Flag that indicates if the stack capture exports are being used by
    /// this WASM process which means that it will be using asyncify
    pub(crate) has_stack_checkpoint: bool,

    /// asyncify_start_unwind(data : i32): call this to start unwinding the
    /// stack from the current location. "data" must point to a data
    /// structure as described above (with fields containing valid data).
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_start_unwind: Option<TypedFunction<i32, ()>>,

    /// asyncify_stop_unwind(): call this to note that unwinding has
    /// concluded. If no other code will run before you start to rewind,
    /// this is not strictly necessary, however, if you swap between
    /// coroutines, or even just want to run some normal code during a
    /// "sleep", then you must call this at the proper time. Otherwise,
    /// the code will think it is still unwinding when it should not be,
    /// which means it will keep unwinding in a meaningless way.
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_stop_unwind: Option<TypedFunction<(), ()>>,

    /// asyncify_start_rewind(data : i32): call this to start rewinding the
    /// stack vack up to the location stored in the provided data. This prepares
    /// for the rewind; to start it, you must call the first function in the
    /// call stack to be unwound.
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_start_rewind: Option<TypedFunction<i32, ()>>,

    /// asyncify_stop_rewind(): call this to note that rewinding has
    /// concluded, and normal execution can resume.
    // TODO: review allow...
    #[allow(dead_code)]
    pub(crate) asyncify_stop_rewind: Option<TypedFunction<(), ()>>,

    /// asyncify_get_state(): call this to get the current value of the
    /// internal "__asyncify_state" variable as described above.
    /// It can be used to distinguish between unwinding/rewinding and normal
    /// calls, so that you know when to start an asynchronous operation and
    /// when to propagate results back.
    #[allow(dead_code)]
    pub(crate) asyncify_get_state: Option<TypedFunction<(), i32>>,
}

impl WasiModuleInstanceHandles {
    pub fn new(
        memory: Memory,
        store: &impl AsStoreRef,
        instance: Instance,
        indirect_function_table: Option<Table>,
    ) -> Self {
        let has_stack_checkpoint = instance
            .module()
            .imports()
            .any(|f| f.name() == "stack_checkpoint");
        Self {
            memory,
            indirect_function_table: indirect_function_table.or_else(|| {
                instance
                    .exports
                    .get_table("__indirect_function_table")
                    .cloned()
                    .ok()
            }),
            stack_pointer: instance.exports.get_global("__stack_pointer").cloned().ok(),
            data_end: instance.exports.get_global("__data_end").cloned().ok(),
            stack_low: instance.exports.get_global("__stack_low").cloned().ok(),
            stack_high: instance.exports.get_global("__stack_high").cloned().ok(),
            tls_base: instance.exports.get_global("__tls_base").cloned().ok(),
            start: instance.exports.get_typed_function(store, "_start").ok(),
            initialize: instance
                .exports
                .get_typed_function(store, "_initialize")
                .ok(),
            thread_spawn: instance
                .exports
                .get_typed_function(store, "wasi_thread_start")
                .ok(),
            signal: instance
                .exports
                .get_typed_function(&store, "__wasm_signal")
                .ok(),
            has_stack_checkpoint,
            signal_set: false,
            asyncify_start_unwind: instance
                .exports
                .get_typed_function(store, "asyncify_start_unwind")
                .ok(),
            asyncify_stop_unwind: instance
                .exports
                .get_typed_function(store, "asyncify_stop_unwind")
                .ok(),
            asyncify_start_rewind: instance
                .exports
                .get_typed_function(store, "asyncify_start_rewind")
                .ok(),
            asyncify_stop_rewind: instance
                .exports
                .get_typed_function(store, "asyncify_stop_rewind")
                .ok(),
            asyncify_get_state: instance
                .exports
                .get_typed_function(store, "asyncify_get_state")
                .ok(),
            instance,
        }
    }

    pub fn module(&self) -> &Module {
        self.instance.module()
    }

    pub fn module_clone(&self) -> Module {
        self.instance.module().clone()
    }

    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory_view<'a>(&'a self, store: &'a (impl AsStoreRef + ?Sized)) -> MemoryView<'a> {
        self.memory.view(store)
    }

    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory(&self) -> &Memory {
        &self.memory
    }

    /// Copy the lazy reference so that when it's initialized during the
    /// export phase, all the other references get a copy of it
    pub fn memory_clone(&self) -> Memory {
        self.memory.clone()
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }
}

#[derive(Debug, Clone)]
pub enum WasiModuleTreeHandles {
    Static(WasiModuleInstanceHandles),
    Dynamic {
        linker: Linker,
        main_module_instance_handles: WasiModuleInstanceHandles,
    },
}

impl WasiModuleTreeHandles {
    /// Can be used to get the `WasiModuleInstanceHandles` of the main module.
    /// If access to the side modules' instance handles is required, one must go
    /// through the `Linker` to retrieve the one they need.
    pub(crate) fn main_module_instance_handles(&self) -> &WasiModuleInstanceHandles {
        match self {
            WasiModuleTreeHandles::Static(ref handles) => handles,
            WasiModuleTreeHandles::Dynamic {
                ref main_module_instance_handles,
                ..
            } => main_module_instance_handles,
        }
    }

    /// See comments on `main_module_instance_handles`.
    pub(crate) fn main_module_instance_handles_mut(&mut self) -> &mut WasiModuleInstanceHandles {
        match self {
            WasiModuleTreeHandles::Static(ref mut handles) => handles,
            WasiModuleTreeHandles::Dynamic {
                ref mut main_module_instance_handles,
                ..
            } => main_module_instance_handles,
        }
    }

    /// Helper function to get the instance handles of a static module, or fail otherwise.
    /// See comments on ensure_static_module for more details.
    pub(crate) fn static_module_instance_handles(&self) -> Option<&WasiModuleInstanceHandles> {
        match self {
            WasiModuleTreeHandles::Static(ref handles) => Some(handles),
            WasiModuleTreeHandles::Dynamic { .. } => None,
        }
    }

    /// See comments on `static_module_instance_handles`.
    #[allow(dead_code)]
    pub(crate) fn static_module_instance_handles_mut(
        &mut self,
    ) -> Option<&mut WasiModuleInstanceHandles> {
        match self {
            WasiModuleTreeHandles::Static(ref mut handles) => Some(handles),
            WasiModuleTreeHandles::Dynamic { .. } => None,
        }
    }

    /// Helper function to ensure the module isn't dynamically linked, needed since
    /// we only support a subset of WASIX functionality for dynamically linked modules.
    /// Specifically, anything that requires asyncify is not supported right now.
    pub(crate) fn ensure_static_module(&self) -> Result<(), ()> {
        match self {
            WasiModuleTreeHandles::Static(_) => Ok(()),
            _ => Err(()),
        }
    }

    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory_view<'a>(&'a self, store: &'a (impl AsStoreRef + ?Sized)) -> MemoryView<'a> {
        self.main_module_instance_handles().memory.view(store)
    }

    /// Providers safe access to the memory
    /// (it must be initialized before it can be used)
    pub fn memory(&self) -> &Memory {
        &self.main_module_instance_handles().memory
    }

    /// Copy the lazy reference so that when it's initialized during the
    /// export phase, all the other references get a copy of it
    pub fn memory_clone(&self) -> Memory {
        self.main_module_instance_handles().memory.clone()
    }

    pub fn linker(&self) -> Option<&Linker> {
        match self {
            Self::Static(_) => None,
            Self::Dynamic { linker, .. } => Some(linker),
        }
    }

    pub fn indirect_function_table_lookup(
        &self,
        store: &mut impl AsStoreMut,
        index: u32,
    ) -> Result<Option<Function>, Errno> {
        let value = self
            .main_module_instance_handles()
            .indirect_function_table
            .as_ref()
            .ok_or(Errno::Notsup)?
            .get(store, index);
        let Some(value) = value else {
            trace!(
                function_id = index,
                "Function not found in indirect function table"
            );
            return Ok(None);
        };
        let Value::FuncRef(funcref) = value else {
            error!("Function table contains something other than a funcref");
            return Err(Errno::Inval);
        };
        let Some(funcref) = funcref else {
            trace!(function_id = index, "No function at the supplied index");
            return Ok(None);
        };
        Ok(Some(funcref))
    }
}
