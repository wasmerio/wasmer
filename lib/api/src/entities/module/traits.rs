use bytes::Bytes;
use std::path::Path;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ImportType, ImportsIterator,
    ModuleInfo, SerializeError,
};

/// The trait implemented by all those that can create new modules.
pub trait ModuleCreator {
    /// Creates a new WebAssembly module from a Wasm binary.
    /// This function is not compatible with the WebAssembly text format.
    fn from_binary(&self, binary: &[u8]) -> Result<Box<dyn ModuleLike>, CompileError>;

    /// Creates a new WebAssembly module from a Wasm binary,
    /// skipping any kind of validation on the WebAssembly file.
    ///
    /// # Safety
    ///
    /// This can speed up compilation time a bit, but it should be only used
    /// in environments where the WebAssembly modules are trusted and validated
    /// beforehand.
    unsafe fn from_binary_unchecked(
        &self,
        binary: &[u8],
    ) -> Result<Box<dyn ModuleLike>, CompileError>;

    /// Validates a new WebAssembly Module given the configuration
    /// in the Store.
    ///
    /// This validation is normally pretty fast and checks the enabled
    /// WebAssembly features in the Store Engine to assure deterministic
    /// validation of the Module.
    fn validate(&self, binary: &[u8]) -> Result<(), CompileError>;

    /// Deserializes a serialized module binary into a `Module`.
    ///
    /// Note: You should usually prefer the safer [`Module::deserialize`].
    ///
    /// # Important
    ///
    /// This function only accepts a custom binary format, which will be different
    /// than the `wasm` binary format and may change among Wasmer versions.
    /// (it should be the result of the serialization of a Module via the
    /// `Module::serialize` method.).
    ///
    /// # Safety
    ///
    /// This function is inherently **unsafe** as the provided bytes:
    /// 1. Are going to be deserialized directly into Rust objects.
    /// 2. Contains the function assembly bodies and, if intercepted,
    ///    a malicious actor could inject code into executable
    ///    memory.
    ///
    /// And as such, the `deserialize_unchecked` method is unsafe.
    unsafe fn deserialize_unchecked(
        &self,
        bytes: bytes::Bytes,
    ) -> Result<Box<dyn ModuleLike>, DeserializeError>;

    /// Deserializes a serialized Module binary into a `Module`.
    ///
    /// # Important
    ///
    /// This function only accepts a custom binary format, which will be different
    /// than the `wasm` binary format and may change among Wasmer versions.
    /// (it should be the result of the serialization of a Module via the
    /// `Module::serialize` method.).
    ///
    /// # Safety
    /// This function is inherently **unsafe**, because it loads executable code
    /// into memory.
    /// The loaded bytes must be trusted to contain a valid artifact previously
    /// built with [`Self::serialize`].
    unsafe fn deserialize(
        &self,
        bytes: bytes::Bytes,
    ) -> Result<Box<dyn ModuleLike>, DeserializeError>;

    /// Deserializes a serialized Module located in a `Path` into a `Module`.
    /// > Note: the module has to be serialized before with the `serialize` method.
    ///
    /// # Safety
    ///
    /// See [`ModuleCreator::deserialize`].
    unsafe fn deserialize_from_file(
        &self,
        path: &Path,
    ) -> Result<Box<dyn ModuleLike>, DeserializeError>;

    /// Deserializes a serialized Module located in a `Path` into a `Module`.
    /// > Note: the module has to be serialized before with the `serialize` method.
    ///
    /// You should usually prefer the safer [`Module::deserialize_from_file`].
    ///
    /// # Safety
    ///
    /// Please check [`Module::deserialize_unchecked`].
    unsafe fn deserialize_from_file_unchecked(
        &self,
        path: &Path,
    ) -> Result<Box<dyn ModuleLike>, DeserializeError>;
}

/// The trait that every concrete module must implement.
pub trait ModuleLike {
    /// Serializes a module into a binary representation that the `Engine`
    /// can later process via [`Module::deserialize`].
    ///
    /// # Important
    ///
    /// This function will return a custom binary format that will be different than
    /// the `wasm` binary format, but faster to load in Native hosts.
    fn serialize(&self) -> Result<Bytes, SerializeError>;

    /// Returns the name of the current module.
    ///
    /// This name is normally set in the WebAssembly bytecode by some
    /// compilers, but can be also overwritten using the [`ModuleLike::set_name`] method.
    fn name(&self) -> Option<&str>;

    /// Sets the name of the current module.
    /// This is normally useful for stacktraces and debugging.
    ///
    /// It will return [`true`] if the module name was changed successfully,
    /// and return [`false`] otherwise (in case the module is cloned or
    /// already instantiated).
    fn set_name(&mut self, name: &str) -> bool;

    /// The ABI of the [`ModuleInfo`] is very unstable, we refactor it very often.
    /// This function is public because in some cases it can be useful to get some
    /// extra information from the module.
    ///
    /// However, the usage is highly discouraged.
    #[doc(hidden)]
    fn info(&self) -> &ModuleInfo;

    /// Returns an iterator over the imported types in the Module.
    ///
    /// The order of the imports is guaranteed to be the same as in the
    /// WebAssembly bytecode.
    ///
    fn imports(&self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + '_>>;

    /// Returns an iterator over the exported types in the Module.
    ///
    /// The order of the exports is guaranteed to be the same as in the
    /// WebAssembly bytecode.
    fn exports(&self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + '_>>;

    /// Get the custom sections of the module given a `name`.
    ///
    /// # Important
    ///
    /// Following the WebAssembly spec, one name can have multiple
    /// custom sections. That's why an iterator (rather than one element)
    /// is returned.
    fn custom_sections<'a>(&'a self, name: &'a str) -> Box<dyn Iterator<Item = Box<[u8]>> + 'a>;

    /// Create a boxed clone of this implementer.
    fn clone_box(&self) -> Box<dyn ModuleLike>;

    /// Compare this module to another.
    ///
    /// # Note
    /// This function is here just because one can't impose [`PartialEq`] to be implemented.
    fn cmp(&self, other: &dyn ModuleLike) -> std::cmp::Ordering;

    /// Cast to [`std::any::Any`].
    ///
    /// # Note
    /// This function is here just because one can't impose [`PartialEq`] to be implemented,
    /// see [`ModuleLike::cmp`].
    fn as_any(&self) -> &dyn std::any::Any;
}
