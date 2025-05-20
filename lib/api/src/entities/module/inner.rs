//! Defines the [`BackendModule`] data type and various useful traits and data types to interact with
//! a concrete module from a backend.

use std::{fs, path::Path};

use bytes::Bytes;
use thiserror::Error;
#[cfg(feature = "wat")]
use wasmer_types::WasmError;
use wasmer_types::{
    CompileError, DeserializeError, ExportType, ExportsIterator, ImportType, ImportsIterator,
    ModuleInfo, SerializeError,
};

use crate::{
    macros::backend::{gen_rt_ty, match_rt},
    utils::IntoBytes,
    AsEngineRef,
};

/// A WebAssembly Module contains stateless WebAssembly
/// code that has already been compiled and can be instantiated
/// multiple times.
///
/// ## Cloning a module
///
/// Cloning a module is cheap: it does a shallow copy of the compiled
/// contents rather than a deep copy.
gen_rt_ty!(Module
    @cfg feature = "artifact-size" => derive(loupe::MemoryUsage)
    @derives Clone, PartialEq, Eq, derive_more::From
);

impl BackendModule {
    #[inline]
    pub fn new(engine: &impl AsEngineRef, bytes: impl AsRef<[u8]>) -> Result<Self, CompileError> {
        #[cfg(feature = "wat")]
        let bytes = wat::parse_bytes(bytes.as_ref()).map_err(|e| {
            CompileError::Wasm(WasmError::Generic(format!(
                "Error when converting wat: {e}",
            )))
        })?;
        Self::from_binary(engine, bytes.as_ref())
    }

    /// Creates a new WebAssembly module from a file path.
    #[inline]
    pub fn from_file(
        engine: &impl AsEngineRef,
        file: impl AsRef<Path>,
    ) -> Result<Self, super::IoCompileError> {
        let file_ref = file.as_ref();
        let canonical = file_ref.canonicalize()?;
        let wasm_bytes = std::fs::read(file_ref)?;
        let mut module = Self::new(engine, wasm_bytes)?;
        // Set the module name to the absolute path of the filename.
        // This is useful for debugging the stack traces.
        let filename = canonical.as_path().to_str().unwrap();
        module.set_name(filename);
        Ok(module)
    }

    /// Creates a new WebAssembly module from a Wasm binary.
    ///
    /// Opposed to [`Self::new`], this function is not compatible with
    /// the WebAssembly text format (if the "wat" feature is enabled for
    /// this crate).
    #[inline]
    pub fn from_binary(engine: &impl AsEngineRef, binary: &[u8]) -> Result<Self, CompileError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::module::Module::from_binary(engine, binary)?,
            )),

            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::module::Module::from_binary(engine, binary)?,
            )),

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::module::Module::from_binary(engine, binary)?,
            )),

            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => Ok(Self::V8(
                crate::backend::v8::entities::module::Module::from_binary(engine, binary)?,
            )),

            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => Ok(Self::Js(
                crate::backend::js::entities::module::Module::from_binary(engine, binary)?,
            )),

            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::module::Module::from_binary(engine, binary)?,
            )),
        }
    }

    /// Creates a new WebAssembly module from a Wasm binary,
    /// skipping any kind of validation on the WebAssembly file.
    ///
    /// # Safety
    ///
    /// This can speed up compilation time a bit, but it should be only used
    /// in environments where the WebAssembly modules are trusted and validated
    /// beforehand.
    #[inline]
    pub unsafe fn from_binary_unchecked(
        engine: &impl AsEngineRef,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::module::Module::from_binary_unchecked(
                    engine, binary,
                )?,
            )),

            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::module::Module::from_binary_unchecked(
                    engine, binary,
                )?,
            )),

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::module::Module::from_binary_unchecked(
                    engine, binary,
                )?,
            )),

            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => Ok(Self::V8(
                crate::backend::v8::entities::module::Module::from_binary_unchecked(
                    engine, binary,
                )?,
            )),
            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => Ok(Self::Js(
                crate::backend::js::entities::module::Module::from_binary_unchecked(
                    engine, binary,
                )?,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::module::Module::from_binary_unchecked(
                    engine, binary,
                )?,
            )),
        }
    }

    /// Validates a new WebAssembly Module given the configuration
    /// in the Store.
    ///
    /// This validation is normally pretty fast and checks the enabled
    /// WebAssembly features in the Store Engine to assure deterministic
    /// validation of the Module.
    #[inline]
    pub fn validate(engine: &impl AsEngineRef, binary: &[u8]) -> Result<(), CompileError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => {
                crate::backend::sys::entities::module::Module::validate(engine, binary)?
            }
            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => {
                crate::backend::wamr::entities::module::Module::validate(engine, binary)?
            }

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => {
                crate::backend::wasmi::entities::module::Module::validate(engine, binary)?
            }
            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => {
                crate::backend::v8::entities::module::Module::validate(engine, binary)?
            }
            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => {
                crate::backend::js::entities::module::Module::validate(engine, binary)?
            }
            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => {
                crate::backend::jsc::entities::module::Module::validate(engine, binary)?
            }
        }
        Ok(())
    }

    /// Serializes a module into a binary representation that the `Engine`
    /// can later process via [`Self::deserialize`].
    ///
    /// # Important
    ///
    /// This function will return a custom binary format that will be different than
    /// the `wasm` binary format, but faster to load in Native hosts.
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
    #[inline]
    pub fn serialize(&self) -> Result<Bytes, SerializeError> {
        match_rt!(on self => s {
            s.serialize()
        })
    }

    /// Serializes a module into a file that the `Engine`
    /// can later process via [`Self::deserialize_from_file`].
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
    #[inline]
    pub fn serialize_to_file(&self, path: impl AsRef<Path>) -> Result<(), SerializeError> {
        let serialized = self.serialize()?;
        fs::write(path, serialized)?;
        Ok(())
    }

    /// Deserializes a serialized module binary into a `Module`.
    ///
    /// Note: You should usually prefer the safer [`Self::deserialize`].
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
    ///
    /// # Usage
    ///
    /// ```ignore
    /// # use wasmer::*;
    /// # fn main() -> anyhow::Result<()> {
    /// # let mut store = Store::default();
    /// let module = Module::deserialize_unchecked(&store, serialized_data)?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub unsafe fn deserialize_unchecked(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::module::Module::deserialize_unchecked(
                    engine, bytes,
                )?,
            )),
            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::module::Module::deserialize_unchecked(
                    engine, bytes,
                )?,
            )),

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::module::Module::deserialize_unchecked(
                    engine, bytes,
                )?,
            )),
            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => Ok(Self::V8(
                crate::backend::v8::entities::module::Module::deserialize_unchecked(engine, bytes)?,
            )),
            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => Ok(Self::Js(
                crate::backend::js::entities::module::Module::deserialize_unchecked(engine, bytes)?,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::module::Module::deserialize_unchecked(
                    engine, bytes,
                )?,
            )),
        }
    }

    /// Deserializes a serialized Module binary into a `Module`.
    ///
    /// # Important
    ///
    /// This function only accepts a custom binary format, which will be different
    /// than the `wasm` binary format and may change among Wasmer versions.
    /// (it should be the result of the serialization of a Module via the
    /// [`Self::serialize`] method.).
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
    ///
    /// # Safety
    /// This function is inherently **unsafe**, because it loads executable code
    /// into memory.
    /// The loaded bytes must be trusted to contain a valid artifact previously
    /// built with [`Self::serialize`].
    #[inline]
    pub unsafe fn deserialize(
        engine: &impl AsEngineRef,
        bytes: impl IntoBytes,
    ) -> Result<Self, DeserializeError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::module::Module::deserialize(engine, bytes)?,
            )),
            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::module::Module::deserialize(engine, bytes)?,
            )),

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::module::Module::deserialize(engine, bytes)?,
            )),
            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => Ok(Self::V8(
                crate::backend::v8::entities::module::Module::deserialize(engine, bytes)?,
            )),
            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => Ok(Self::Js(
                crate::backend::js::entities::module::Module::deserialize(engine, bytes)?,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::module::Module::deserialize(engine, bytes)?,
            )),
        }
    }

    /// Deserializes a serialized Module located in a `Path` into a `Module`.
    /// > Note: the module has to be serialized before with the `serialize` method.
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
    ///
    /// # Safety
    ///
    /// See [`Self::deserialize`].
    #[inline]
    pub unsafe fn deserialize_from_file(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::module::Module::deserialize_from_file(engine, path)?,
            )),
            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::module::Module::deserialize_from_file(
                    engine, path,
                )?,
            )),

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::module::Module::deserialize_from_file(
                    engine, path,
                )?,
            )),
            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => Ok(Self::V8(
                crate::backend::v8::entities::module::Module::deserialize_from_file(engine, path)?,
            )),
            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => Ok(Self::Js(
                crate::backend::js::entities::module::Module::deserialize_from_file(engine, path)?,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::module::Module::deserialize_from_file(engine, path)?,
            )),
        }
    }

    /// Deserializes a serialized Module located in a `Path` into a `Module`.
    /// > Note: the module has to be serialized before with the `serialize` method.
    ///
    /// You should usually prefer the safer [`Self::deserialize_from_file`].
    ///
    /// # Safety
    ///
    /// Please check [`Self::deserialize_unchecked`].
    ///
    /// # Usage
    ///
    /// ```ignore
    /// # use wasmer::*;
    /// # let mut store = Store::default();
    /// # fn main() -> anyhow::Result<()> {
    /// let module = Module::deserialize_from_file_unchecked(&store, path)?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    pub unsafe fn deserialize_from_file_unchecked(
        engine: &impl AsEngineRef,
        path: impl AsRef<Path>,
    ) -> Result<Self, DeserializeError> {
        match engine.as_engine_ref().inner.be {
            #[cfg(feature = "sys")]
            crate::BackendEngine::Sys(_) => Ok(Self::Sys(
                crate::backend::sys::entities::module::Module::deserialize_from_file_unchecked(
                    engine, path,
                )?,
            )),
            #[cfg(feature = "wamr")]
            crate::BackendEngine::Wamr(_) => Ok(Self::Wamr(
                crate::backend::wamr::entities::module::Module::deserialize_from_file_unchecked(
                    engine, path,
                )?,
            )),

            #[cfg(feature = "wasmi")]
            crate::BackendEngine::Wasmi(_) => Ok(Self::Wasmi(
                crate::backend::wasmi::entities::module::Module::deserialize_from_file_unchecked(
                    engine, path,
                )?,
            )),
            #[cfg(feature = "v8")]
            crate::BackendEngine::V8(_) => Ok(Self::V8(
                crate::backend::v8::entities::module::Module::deserialize_from_file_unchecked(
                    engine, path,
                )?,
            )),
            #[cfg(feature = "js")]
            crate::BackendEngine::Js(_) => Ok(Self::Js(
                crate::backend::js::entities::module::Module::deserialize_from_file_unchecked(
                    engine, path,
                )?,
            )),
            #[cfg(feature = "jsc")]
            crate::BackendEngine::Jsc(_) => Ok(Self::Jsc(
                crate::backend::jsc::entities::module::Module::deserialize_from_file_unchecked(
                    engine, path,
                )?,
            )),
        }
    }

    /// Returns the name of the current module.
    ///
    /// This name is normally set in the WebAssembly bytecode by some
    /// compilers, but can be also overwritten using the [`Self::set_name`] method.
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
    #[inline]
    pub fn name(&self) -> Option<&str> {
        match_rt!(on self => s {
            s.name()
        })
    }

    /// Sets the name of the current module.
    /// This is normally useful for stacktraces and debugging.
    ///
    /// It will return `true` if the module name was changed successfully,
    /// and return `false` otherwise (in case the module is cloned or
    /// already instantiated).
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
    #[inline]
    pub fn set_name(&mut self, name: &str) -> bool {
        match_rt!(on self => s {
            s.set_name(name)
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
    #[inline]
    pub fn imports(&self) -> ImportsIterator<Box<dyn Iterator<Item = ImportType> + '_>> {
        match_rt!(on self => s {
            s.imports()
        })
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
    #[inline]
    pub fn exports(&self) -> ExportsIterator<Box<dyn Iterator<Item = ExportType> + '_>> {
        match_rt!(on self => s {
            s.exports()
        })
    }

    /// Get the custom sections of the module given a `name`.
    ///
    /// # Important
    ///
    /// Following the WebAssembly spec, one name can have multiple
    /// custom sections. That's why an iterator (rather than one element)
    /// is returned.
    #[inline]
    pub fn custom_sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Box<[u8]>> + 'a {
        match_rt!(on self => s {
            s.custom_sections(name)
        })
    }

    /// The ABI of the [`ModuleInfo`] is very unstable, we refactor it very often.
    /// This function is public because in some cases it can be useful to get some
    /// extra information from the module.
    ///
    /// However, the usage is highly discouraged.
    #[doc(hidden)]
    #[inline]
    pub fn info(&self) -> &ModuleInfo {
        match_rt!(on self => s {
            s.info()
        })
    }
}

impl std::fmt::Debug for BackendModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendModule")
            .field("name", &self.name())
            .finish()
    }
}
