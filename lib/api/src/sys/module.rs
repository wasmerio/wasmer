use crate::sys::InstantiationError;
use crate::AsStoreMut;
use crate::AsStoreRef;
use bytes::Bytes;
use std::borrow::Cow;
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use wasmer_compiler::Artifact;
use wasmer_compiler::ArtifactCreate;
use wasmer_compiler::AsEngineRef;
#[cfg(feature = "wat")]
use wasmer_types::WasmError;
use wasmer_types::{
    CompileError, DeserializeError, ExportsIterator, ImportsIterator, ModuleInfo, SerializeError,
};
use wasmer_types::{ExportType, ImportType};
use wasmer_vm::InstanceHandle;

/// IO Error on a Module Compilation
#[derive(Error, Debug)]
pub enum IoCompileError {
    /// An IO error
    #[error(transparent)]
    Io(#[from] io::Error),
    /// A compilation error
    #[error(transparent)]
    Compile(#[from] CompileError),
}

/// A WebAssembly Module contains stateless WebAssembly
/// code that has already been compiled and can be instantiated
/// multiple times.
///
/// ## Cloning a module
///
/// Cloning a module is cheap: it does a shallow copy of the compiled
/// contents rather than a deep copy.
#[derive(Clone)]
pub struct Module {
    // The field ordering here is actually significant because of the drop
    // order: we want to drop the artifact before dropping the engine.
    //
    // The reason for this is that dropping the Artifact will de-register the
    // trap handling metadata from the global registry. This must be done before
    // the code memory for the artifact is freed (which happens when the store
    // is dropped) since there is a chance that this memory could be reused by
    // another module which will try to register its own trap information.
    //
    // Note that in Rust, the drop order for struct fields is from top to
    // bottom: the opposite of C++.
    //
    // In the future, this code should be refactored to properly describe the
    // ownership of the code and its metadata.
    artifact: Arc<Artifact>,
    module_info: Arc<ModuleInfo>,
}

pub trait IntoBytes {
    fn into_bytes(self) -> Bytes;
}

impl IntoBytes for Bytes {
    fn into_bytes(self) -> Bytes {
        self
    }
}

impl IntoBytes for Vec<u8> {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self)
    }
}

impl IntoBytes for &[u8] {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self.to_vec())
    }
}

impl<const N: usize> IntoBytes for &[u8; N] {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self.to_vec())
    }
}

impl IntoBytes for &str {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self.as_bytes().to_vec())
    }
}

impl IntoBytes for Cow<'_, [u8]> {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self.to_vec())
    }
}

impl Module {
    #[cfg(feature = "compiler")]
    /// Creates a new WebAssembly Module given the configuration
    /// in the store.
    ///
    /// If the provided bytes are not WebAssembly-like (start with `b"\0asm"`),
    /// and the "wat" feature is enabled for this crate, this function will try to
    /// to convert the bytes assuming they correspond to the WebAssembly text
    /// format.
    ///
    /// ## Security
    ///
    /// Before the code is compiled, it will be validated using the store
    /// features.
    ///
    /// ## Errors
    ///
    /// Creating a WebAssembly module from bytecode can result in a
    /// [`CompileError`] since this operation requires to transorm the Wasm
    /// bytecode into code the machine can easily execute.
    ///
    /// ## Example
    ///
    /// Reading from a WAT file.
    ///
    /// ```
    /// use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let wat = "(module)";
    /// let module = Module::new(&store, wat)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Reading from bytes:
    ///
    /// ```
    /// use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// // The following is the same as:
    /// // (module
    /// //   (type $t0 (func (param i32) (result i32)))
    /// //   (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
    /// //     get_local $p0
    /// //     i32.const 1
    /// //     i32.add)
    /// // )
    /// let bytes: Vec<u8> = vec![
    ///     0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60,
    ///     0x01, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x0b, 0x01, 0x07,
    ///     0x61, 0x64, 0x64, 0x5f, 0x6f, 0x6e, 0x65, 0x00, 0x00, 0x0a, 0x09, 0x01,
    ///     0x07, 0x00, 0x20, 0x00, 0x41, 0x01, 0x6a, 0x0b, 0x00, 0x1a, 0x04, 0x6e,
    ///     0x61, 0x6d, 0x65, 0x01, 0x0a, 0x01, 0x00, 0x07, 0x61, 0x64, 0x64, 0x5f,
    ///     0x6f, 0x6e, 0x65, 0x02, 0x07, 0x01, 0x00, 0x01, 0x00, 0x02, 0x70, 0x30,
    /// ];
    /// let module = Module::new(&store, bytes)?;
    /// # Ok(())
    /// # }
    /// ```
    /// # Example of loading a module using just an `Engine` and no `Store`
    ///
    /// ```
    /// # use wasmer::*;
    /// #
    /// # let compiler = Cranelift::default();
    /// # let engine = EngineBuilder::new(compiler).engine();
    ///
    /// let module = Module::from_file(&engine, "path/to/foo.wasm");
    /// ```
    #[allow(unreachable_code)]
    pub fn new(engine: &impl AsEngineRef, bytes: impl AsRef<[u8]>) -> Result<Self, CompileError> {
        #[cfg(feature = "wat")]
        let bytes = wat::parse_bytes(bytes.as_ref()).map_err(|e| {
            CompileError::Wasm(WasmError::Generic(format!(
                "Error when converting wat: {}",
                e
            )))
        })?;
        Self::from_binary(engine, bytes.as_ref())
    }

    #[cfg(feature = "compiler")]
    /// Creates a new WebAssembly module from a file path.
    pub fn from_file(
        engine: &impl AsEngineRef,
        file: impl AsRef<Path>,
    ) -> Result<Self, IoCompileError> {
        let file_ref = file.as_ref();
        let canonical = file_ref.canonicalize()?;
        let wasm_bytes = std::fs::read(file_ref)?;
        let mut module = Self::new(engine, &wasm_bytes)?;
        // Set the module name to the absolute path of the filename.
        // This is useful for debugging the stack traces.
        let filename = canonical.as_path().to_str().unwrap();
        module.set_name(filename);
        Ok(module)
    }

    #[cfg(feature = "compiler")]
    /// Creates a new WebAssembly module from a binary.
    ///
    /// Opposed to [`Module::new`], this function is not compatible with
    /// the WebAssembly text format (if the "wat" feature is enabled for
    /// this crate).
    pub fn from_binary(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        Self::validate(engine, binary)?;
        unsafe { Self::from_binary_unchecked(engine, binary) }
    }

    #[cfg(feature = "compiler")]
    /// Creates a new WebAssembly module skipping any kind of validation.
    ///
    /// # Safety
    ///
    /// This can speed up compilation time a bit, but it should be only used
    /// in environments where the WebAssembly modules are trusted and validated
    /// beforehand.
    pub unsafe fn from_binary_unchecked(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        let module = Self::compile(engine, binary)?;
        Ok(module)
    }

    #[cfg(feature = "compiler")]
    /// Validates a new WebAssembly Module given the configuration
    /// in the Store.
    ///
    /// This validation is normally pretty fast and checks the enabled
    /// WebAssembly features in the Store Engine to assure deterministic
    /// validation of the Module.
    pub fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        engine.as_engine_ref().engine().validate(binary)
    }

    #[cfg(feature = "compiler")]
    fn compile(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        let artifact = engine.as_engine_ref().engine().compile(binary)?;
        Ok(Self::from_artifact(artifact))
    }

    /// Serializes a module into a binary representation that the `Engine`
    /// can later process via
    #[cfg_attr(feature = "compiler", doc = "[`Module::deserialize`].")]
    #[cfg_attr(not(feature = "compiler"), doc = "`Module::deserialize`.")]
    ///
    /// # Usage
    ///
    /// ```ignore
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// # let module = Module::from_file(&store, "path/to/foo.wasm")?;
    /// let serialized = module.serialize()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        self.artifact.serialize().map(|bytes| bytes.into())
    }

    /// Serializes a module into a file that the `Engine`
    /// can later process via
    #[cfg_attr(feature = "compiler", doc = "[`Module::deserialize_from_file`].")]
    #[cfg_attr(not(feature = "compiler"), doc = "`Module::deserialize_from_file`.")]
    ///
    /// # Usage
    ///
    /// ```ignore
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// # let module = Module::from_file(&store, "path/to/foo.wasm")?;
    /// module.serialize_to_file("path/to/foo.so")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn serialize_to_file(&self, path: impl AsRef<Path>) -> Result<(), SerializeError> {
        self.artifact.serialize_to_file(path.as_ref())
    }

    #[cfg(feature = "compiler")]
    /// Deserializes a serialized Module binary into a `Module`.
    /// > Note: the module has to be serialized before with the `serialize` method.
    ///
    /// # Safety
    ///
    /// This function is inherently **unsafe** as the provided bytes:
    /// 1. Are going to be deserialized directly into Rust objects.
    /// 2. Contains the function assembly bodies and, if intercepted,
    ///    a malicious actor could inject code into executable
    ///    memory.
    ///
    /// And as such, the `deserialize` method is unsafe.
    ///
    /// # Usage
    ///
    /// ```ignore
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let module = Module::deserialize(&store, serialized_data)?;
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        let bytes = bytes.into_bytes();
        let artifact = engine.as_engine_ref().engine().deserialize(&bytes)?;
        Ok(Self::from_artifact(artifact))
    }

    #[cfg(feature = "compiler")]
    /// Deserializes a a serialized Module located in a `Path` into a `Module`.
    /// > Note: the module has to be serialized before with the `serialize` method.
    ///
    /// # Safety
    ///
    /// Please check [`Module::deserialize`].
    ///
    /// # Usage
    ///
    /// ```ignore
    /// # use wasmer::*;
    /// # let mut store = Store::default();
    /// # fn main() -> anyhow::Result<()> {
    /// let module = Module::deserialize_from_file(&store, path)?;
    /// # Ok(())
    /// # }
    /// ```
    pub unsafe fn deserialize_from_file(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        let artifact = engine
            .as_engine_ref()
            .engine()
            .deserialize_from_file(path.as_ref())?;
        Ok(Self::from_artifact(artifact))
    }

    fn from_artifact(artifact: Arc<Artifact>) -> Self {
        Self {
            module_info: Arc::new(artifact.create_module_info()),
            artifact,
        }
    }

    #[cfg(feature = "compiler")]
    pub(crate) fn instantiate(
        &self,
        store: &mut impl AsStoreMut,
        imports: &[crate::Extern],
    ) -> Result<InstanceHandle, InstantiationError> {
        // Ensure all imports come from the same context.
        for import in imports {
            if !import.is_from_store(store) {
                return Err(InstantiationError::DifferentStores);
            }
        }
        let mut store_mut = store.as_store_mut();
        let (tunables, objects) = store_mut.tunables_and_objects_mut();
        unsafe {
            let mut instance_handle = self.artifact.instantiate(
                tunables,
                &imports
                    .iter()
                    .map(crate::Extern::to_vm_extern)
                    .collect::<Vec<_>>(),
                objects,
            )?;

            // After the instance handle is created, we need to initialize
            // the data, call the start function and so. However, if any
            // of this steps traps, we still need to keep the instance alive
            // as some of the Instance elements may have placed in other
            // instance tables.
            self.artifact.finish_instantiation(
                store.as_store_ref().signal_handler(),
                &mut instance_handle,
            )?;

            Ok(instance_handle)
        }
    }

    /// Returns the name of the current module.
    ///
    /// This name is normally set in the WebAssembly bytecode by some
    /// compilers, but can be also overwritten using the [`Module::set_name`] method.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let wat = "(module $moduleName)";
    /// let module = Module::new(&store, wat)?;
    /// assert_eq!(module.name(), Some("moduleName"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn name(&self) -> Option<&str> {
        self.module_info.name.as_deref()
    }

    /// Sets the name of the current module.
    /// This is normally useful for stacktraces and debugging.
    ///
    /// It will return `true` if the module name was changed successfully,
    /// and return `false` otherwise (in case the module is already
    /// instantiated).
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let wat = "(module)";
    /// let mut module = Module::new(&store, wat)?;
    /// assert_eq!(module.name(), None);
    /// module.set_name("foo");
    /// assert_eq!(module.name(), Some("foo"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_name(&mut self, name: &str) -> bool {
        Arc::get_mut(&mut self.module_info).map_or(false, |mut module_info| {
            module_info.name = Some(name.to_string());
            true
        })
    }

    /// Returns an iterator over the imported types in the Module.
    ///
    /// The order of the imports is guaranteed to be the same as in the
    /// WebAssembly bytecode.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let wat = r#"(module
    ///     (import "host" "func1" (func))
    ///     (import "host" "func2" (func))
    /// )"#;
    /// let module = Module::new(&store, wat)?;
    /// for import in module.imports() {
    ///     assert_eq!(import.module(), "host");
    ///     assert!(import.name().contains("func"));
    ///     import.ty();
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn imports(&self) -> ImportsIterator<impl Iterator<Item = ImportType> + '_> {
        self.module_info.imports()
    }

    /// Returns an iterator over the exported types in the Module.
    ///
    /// The order of the exports is guaranteed to be the same as in the
    /// WebAssembly bytecode.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let wat = r#"(module
    ///     (func (export "namedfunc"))
    ///     (memory (export "namedmemory") 1)
    /// )"#;
    /// let module = Module::new(&store, wat)?;
    /// for export_ in module.exports() {
    ///     assert!(export_.name().contains("named"));
    ///     export_.ty();
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn exports(&self) -> ExportsIterator<impl Iterator<Item = ExportType> + '_> {
        self.module_info.exports()
    }

    /// Get the custom sections of the module given a `name`.
    ///
    /// # Important
    ///
    /// Following the WebAssembly spec, one name can have multiple
    /// custom sections. That's why an iterator (rather than one element)
    /// is returned.
    pub fn custom_sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Box<[u8]>> + 'a {
        self.module_info.custom_sections(name)
    }

    /// The ABI of the ModuleInfo is very unstable, we refactor it very often.
    /// This function is public because in some cases it can be useful to get some
    /// extra information from the module.
    ///
    /// However, the usage is highly discouraged.
    #[doc(hidden)]
    pub fn info(&self) -> &ModuleInfo {
        &self.module_info
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Module")
            .field("name", &self.name())
            .finish()
    }
}
