use crate::{
    resolve_imports, DeserializeError, InstantiationError, Resolver, RuntimeError, SerializeError,
    Tunables,
};
use enumset::EnumSet;
use loupe::MemoryUsage;
use std::any::Any;
use std::convert::TryInto;
use std::path::Path;
use std::sync::Arc;
use std::{fs, mem};
use wasmer_compiler::{CpuFeature, Features};
use wasmer_types::entity::{BoxedSlice, PrimaryMap};
use wasmer_types::{
    DataInitializer, FunctionIndex, LocalFunctionIndex, MemoryIndex, ModuleInfo,
    OwnedDataInitializer, SignatureIndex, TableIndex,
};
use wasmer_vm::{
    FuncDataRegistry, FunctionBodyPtr, InstanceAllocator, InstanceHandle, MemoryStyle, TableStyle,
    TrapHandler, VMSharedSignatureIndex, VMTrampoline,
};

/// An `Artifact` is the product that the `Engine`
/// implementation produce and use.
///
/// The `Artifact` contains the compiled data for a given
/// module as well as extra information needed to run the
/// module at runtime, such as [`ModuleInfo`] and [`Features`].
pub trait Artifact: Send + Sync + Upcastable + MemoryUsage {
    /// Return a reference-counted pointer to the module
    fn module(&self) -> Arc<ModuleInfo>;

    /// Return a pointer to a module.
    fn module_ref(&self) -> &ModuleInfo;

    /// Gets a mutable reference to the info.
    ///
    /// Note: this will return `None` if the module is already instantiated.
    fn module_mut(&mut self) -> Option<&mut ModuleInfo>;

    /// Register thie `Artifact` stack frame information into the global scope.
    ///
    /// This is required to ensure that any traps can be properly symbolicated.
    fn register_frame_info(&self);

    /// Returns the features for this Artifact
    fn features(&self) -> &Features;

    /// Returns the CPU features for this Artifact
    fn cpu_features(&self) -> EnumSet<CpuFeature>;

    /// Returns the memory styles associated with this `Artifact`.
    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle>;

    /// Returns the table plans associated with this `Artifact`.
    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle>;

    /// Returns data initializers to pass to `InstanceHandle::initialize`
    fn data_initializers(&self) -> &[OwnedDataInitializer];

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

    /// Get the func data registry
    fn func_data_registry(&self) -> &FuncDataRegistry;

    /// Serializes an artifact into bytes
    fn serialize(&self) -> Result<Vec<u8>, SerializeError>;

    /// Serializes an artifact into a file path
    fn serialize_to_file(&self, path: &Path) -> Result<(), SerializeError> {
        let serialized = self.serialize()?;
        fs::write(&path, serialized)?;
        Ok(())
    }

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
        resolver: &dyn Resolver,
        host_state: Box<dyn Any>,
    ) -> Result<InstanceHandle, InstantiationError> {
        // Validate the CPU features this module was compiled with against the
        // host CPU features.
        let host_cpu_features = CpuFeature::for_host();
        if !host_cpu_features.is_superset(self.cpu_features()) {
            Err(InstantiationError::CpuFeature(format!(
                "{:?}",
                self.cpu_features().difference(host_cpu_features)
            )))?;
        }

        self.preinstantiate()?;

        let module = self.module();
        let (imports, import_function_envs) = {
            let mut imports = resolve_imports(
                &module,
                resolver,
                &self.finished_dynamic_function_trampolines(),
                self.memory_styles(),
                self.table_styles(),
            )
            .map_err(InstantiationError::Link)?;

            // Get the `WasmerEnv::init_with_instance` function pointers and the pointers
            // to the envs to call it on.
            let import_function_envs = imports.get_imported_function_envs();

            (imports, import_function_envs)
        };

        // Get pointers to where metadata about local memories should live in VM memory.
        // Get pointers to where metadata about local tables should live in VM memory.

        let (allocator, memory_definition_locations, table_definition_locations) =
            InstanceAllocator::new(&*module);
        let finished_memories = tunables
            .create_memories(&module, self.memory_styles(), &memory_definition_locations)
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_tables = tunables
            .create_tables(&module, self.table_styles(), &table_definition_locations)
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();
        let finished_globals = tunables
            .create_globals(&module)
            .map_err(InstantiationError::Link)?
            .into_boxed_slice();

        self.register_frame_info();

        let handle = InstanceHandle::new(
            allocator,
            module,
            self.finished_functions().clone(),
            self.finished_function_call_trampolines().clone(),
            finished_memories,
            finished_tables,
            finished_globals,
            imports,
            self.signatures().clone(),
            host_state,
            import_function_envs,
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
        trap_handler: &dyn TrapHandler,
        handle: &InstanceHandle,
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

// Implementation of `Upcastable` taken from https://users.rust-lang.org/t/why-does-downcasting-not-work-for-subtraits/33286/7 .
/// Trait needed to get downcasting of `Engine`s to work.
pub trait Upcastable {
    fn upcast_any_ref(&'_ self) -> &'_ dyn Any;
    fn upcast_any_mut(&'_ mut self) -> &'_ mut dyn Any;
    fn upcast_any_box(self: Box<Self>) -> Box<dyn Any>;
}

impl<T: Any + Send + Sync + 'static> Upcastable for T {
    #[inline]
    fn upcast_any_ref(&'_ self) -> &'_ dyn Any {
        self
    }
    #[inline]
    fn upcast_any_mut(&'_ mut self) -> &'_ mut dyn Any {
        self
    }
    #[inline]
    fn upcast_any_box(self: Box<Self>) -> Box<dyn Any> {
        self
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

/// Metadata header which holds an ABI version and the length of the remaining
/// metadata.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MetadataHeader {
    magic: [u8; 8],
    version: u32,
    len: u32,
}

impl MetadataHeader {
    /// Current ABI version. Increment this any time breaking changes are made
    /// to the format of the serialized data.
    const CURRENT_VERSION: u32 = 1;

    /// Magic number to identify wasmer metadata.
    const MAGIC: [u8; 8] = *b"WASMER\0\0";

    /// Length of the metadata header.
    pub const LEN: usize = 16;

    /// Alignment of the metadata.
    pub const ALIGN: usize = 16;

    /// Creates a new header for metadata of the given length.
    pub fn new(len: usize) -> [u8; 16] {
        let header = MetadataHeader {
            magic: Self::MAGIC,
            version: Self::CURRENT_VERSION,
            len: len.try_into().expect("metadata exceeds maximum length"),
        };
        unsafe { mem::transmute(header) }
    }

    /// Parses the header and returns the length of the metadata following it.
    pub fn parse(bytes: &[u8]) -> Result<usize, DeserializeError> {
        if bytes.as_ptr() as usize % 16 != 0 {
            return Err(DeserializeError::CorruptedBinary(
                "misaligned metadata".to_string(),
            ));
        }
        let bytes: [u8; 16] = bytes
            .get(..16)
            .ok_or_else(|| {
                DeserializeError::CorruptedBinary("invalid metadata header".to_string())
            })?
            .try_into()
            .unwrap();
        let header: MetadataHeader = unsafe { mem::transmute(bytes) };
        if header.magic != Self::MAGIC {
            return Err(DeserializeError::Incompatible(
                "The provided bytes were not serialized by Wasmer".to_string(),
            ));
        }
        if header.version != Self::CURRENT_VERSION {
            return Err(DeserializeError::Incompatible(
                "The provided bytes were serialized by an incompatible version of Wasmer"
                    .to_string(),
            ));
        }
        Ok(header.len as usize)
    }
}
