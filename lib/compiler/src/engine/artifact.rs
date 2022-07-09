use crate::CpuFeature;
use crate::{resolve_imports, InstantiationError, RuntimeError, Tunables};
use crate::{ArtifactCreate, Upcastable};
use wasmer_types::entity::BoxedSlice;
use wasmer_types::{DataInitializer, FunctionIndex, LocalFunctionIndex, SignatureIndex};
use wasmer_vm::{
    FunctionBodyPtr, InstanceAllocator, InstanceHandle, StoreObjects, TrapHandlerFn,
    VMExtern, VMSharedSignatureIndex, VMTrampoline,
};

/// An `Artifact` is the product that the `Engine`
/// implementation produce and use.
///

/// An `Artifact` is the product that the `Engine`
/// implementation produce and use.
///
/// The `ArtifactRun` contains the extra information needed to run the
/// module at runtime, such as [`ModuleInfo`] and [`Features`].
pub trait Artifact: Send + Sync + Upcastable + ArtifactCreate {
    /// Register thie `Artifact` stack frame information into the global scope.
    ///
    /// This is required to ensure that any traps can be properly symbolicated.
    fn register_frame_info(&self);

    /// Returns the functions allocated in memory or this `Artifact`
    /// ready to be run.
    fn finished_functions(&self) -> &BoxedSlice<LocalFunctionIndex, FunctionBodyPtr>;

    /// Returns the function call trampolines allocated in memory of this
    /// `Artifact`, ready to be run.
    fn finished_function_call_trampolines(&self) -> &BoxedSlice<SignatureIndex, VMTrampoline>;

    /// Returns the dynamic function trampolines allocated in memory
    /// of this `Artifact`, ready to be run.
    fn finished_dynamic_function_trampolines(&self) -> &BoxedSlice<FunctionIndex, FunctionBodyPtr>;

    /// Returns the associated VM signatures for this `Artifact`.
    fn signatures(&self) -> &BoxedSlice<SignatureIndex, VMSharedSignatureIndex>;

    /// Do preinstantiation logic that is executed before instantiating
    fn preinstantiate(&self) -> Result<(), InstantiationError> {
        Ok(())
    }
    /// Crate an `Instance` from this `Artifact`.
    ///
    /// # Safety
    ///
    /// See [`InstanceHandle::new`].
    unsafe fn instantiate(
        &self,
        tunables: &dyn Tunables,
        imports: &[VMExtern],
        context: &mut StoreObjects,
    ) -> Result<InstanceHandle, InstantiationError> {
        // Validate the CPU features this module was compiled with against the
        // host CPU features.
        let host_cpu_features = CpuFeature::for_host();
        if !host_cpu_features.is_superset(self.cpu_features()) {
            return Err(InstantiationError::CpuFeature(format!(
                "{:?}",
                self.cpu_features().difference(host_cpu_features)
            )));
        }

        self.preinstantiate()?;

        let module = self.module();
        let imports = resolve_imports(
            &module,
            imports,
            context,
            self.finished_dynamic_function_trampolines(),
            self.memory_styles(),
            self.table_styles(),
        )
        .map_err(InstantiationError::Link)?;

        // Get pointers to where metadata about local memories should live in VM memory.
        // Get pointers to where metadata about local tables should live in VM memory.

        let (allocator, memory_definition_locations, table_definition_locations) =
            InstanceAllocator::new(&*module);
        let finished_memories = tunables
            .create_memories(
                context,
                &module,
                self.memory_styles(),
                &memory_definition_locations,
            )
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_tables = tunables
            .create_tables(
                context,
                &module,
                self.table_styles(),
                &table_definition_locations,
            )
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_globals = tunables
            .create_globals(context, &module)
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();

        self.register_frame_info();

        let handle = InstanceHandle::new(
            allocator,
            module,
            context,
            self.finished_functions().clone(),
            self.finished_function_call_trampolines().clone(),
            finished_memories,
            finished_tables,
            finished_globals,
            imports,
            self.signatures().clone(),
        )
        .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))?;
        Ok(handle)
    }
    /// Finishes the instantiation of a just created `InstanceHandle`.
    ///
    /// # Safety
    ///
    /// See [`InstanceHandle::finish_instantiation`].
    unsafe fn finish_instantiation(
        &self,
        trap_handler: Option<*const TrapHandlerFn<'static>>,
        handle: &mut InstanceHandle,
    ) -> Result<(), InstantiationError> {
        let data_initializers = self
            .data_initializers()
            .iter()
            .map(|init| DataInitializer {
                location: init.location.clone(),
                data: &*init.data,
            })
            .collect::<Vec<_>>();
        handle
            .finish_instantiation(trap_handler, &data_initializers)
            .map_err(|trap| InstantiationError::Start(RuntimeError::from_trap(trap)))
    }
}

impl dyn Artifact + 'static {
    /// Try to downcast the artifact into a given type.
    #[inline]
    pub fn downcast_ref<T: 'static>(&'_ self) -> Option<&'_ T> {
        self.upcast_any_ref().downcast_ref::<T>()
    }

    /// Try to downcast the artifact into a given type mutably.
    #[inline]
    pub fn downcast_mut<T: 'static>(&'_ mut self) -> Option<&'_ mut T> {
        self.upcast_any_mut().downcast_mut::<T>()
    }
}
