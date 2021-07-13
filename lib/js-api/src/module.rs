use crate::export::{Export, VMFunction};
use crate::iterators::{ExportsIterator, ImportsIterator};
use crate::resolver::Resolver;
use crate::store::Store;
use crate::types::{ExportType, ImportType};
// use crate::InstantiationError;
use crate::error::CompileError;
#[cfg(feature = "wat")]
use crate::error::WasmError;
use js_sys::{Reflect, Uint8Array, WebAssembly};
use std::fmt;
use std::io;
use std::path::Path;
use thiserror::Error;
use wasmer_types::{
    ExternType, FunctionType, GlobalType, MemoryType, Mutability, Pages, TableType, Type,
};

#[derive(Error, Debug)]
pub enum IoCompileError {
    /// An IO error
    #[error(transparent)]
    Io(#[from] io::Error),
    /// A compilation error
    #[error(transparent)]
    Compile(#[from] CompileError),
}

/// WebAssembly in the browser doesn't yet output the descriptor/types
/// corresponding to each extern (import and export).
///
/// This should be fixed once the JS-Types Wasm proposal is adopted
/// by the browsers:
/// https://github.com/WebAssembly/js-types/blob/master/proposals/js-types/Overview.md
///
/// Until that happens, we annotate the module with the expected
/// types so we can built on top of them at runtime.
#[derive(Clone)]
pub struct ModuleTypeHints {
    /// The type hints for the imported types
    pub imports: Vec<ExternType>,
    /// The type hints for the exported types
    pub exports: Vec<ExternType>,
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
    store: Store,
    module: WebAssembly::Module,
    name: Option<String>,
    // WebAssembly type hints
    type_hints: Option<ModuleTypeHints>,
}

impl Module {
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
    /// # let store = Store::default();
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
    /// # let store = Store::default();
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
    #[allow(unreachable_code)]
    pub fn new(store: &Store, bytes: impl AsRef<[u8]>) -> Result<Self, CompileError> {
        #[cfg(feature = "wat")]
        let bytes = wat::parse_bytes(bytes.as_ref()).map_err(|e| {
            CompileError::Wasm(WasmError::Generic(format!(
                "Error when converting wat: {}",
                e
            )))
        })?;
        Self::from_binary(store, bytes.as_ref())
    }

    /// Creates a new WebAssembly module from a file path.
    pub fn from_file(store: &Store, file: impl AsRef<Path>) -> Result<Self, IoCompileError> {
        unimplemented!();
    }

    /// Creates a new WebAssembly module from a binary.
    ///
    /// Opposed to [`Module::new`], this function is not compatible with
    /// the WebAssembly text format (if the "wat" feature is enabled for
    /// this crate).
    pub fn from_binary(store: &Store, binary: &[u8]) -> Result<Self, CompileError> {
        //
        // Self::validate(store, binary)?;
        unsafe { Self::from_binary_unchecked(store, binary) }
    }

    /// Creates a new WebAssembly module skipping any kind of validation.
    ///
    /// # Safety
    ///
    /// This is safe since the JS vm should be safe already.
    /// We maintain the `unsafe` to preserve the same API as Wasmer
    pub unsafe fn from_binary_unchecked(
        store: &Store,
        binary: &[u8],
    ) -> Result<Self, CompileError> {
        let js_bytes = unsafe { Uint8Array::view(binary) };
        let module = WebAssembly::Module::new(&js_bytes.into()).unwrap();

        // The module is now validated, so we can safely parse it's types
        let info = crate::module_info_polyfill::translate_module(binary).unwrap();
        #[cfg(feature = "wasm-types-polyfill")]
        let (type_hints, name) = (
            Some(ModuleTypeHints {
                imports: info
                    .imports()
                    .map(|import| import.ty().clone())
                    .collect::<Vec<_>>(),
                exports: info
                    .exports()
                    .map(|export| export.ty().clone())
                    .collect::<Vec<_>>(),
            }),
            info.name,
        );
        #[cfg(not(feature = "wasm-types-polyfill"))]
        let (type_hints, name) = (None, None);

        Ok(Self {
            store: store.clone(),
            module,
            type_hints,
            name,
        })
    }

    /// Validates a new WebAssembly Module given the configuration
    /// in the Store.
    ///
    /// This validation is normally pretty fast and checks the enabled
    /// WebAssembly features in the Store Engine to assure deterministic
    /// validation of the Module.
    pub fn validate(store: &Store, binary: &[u8]) -> Result<(), CompileError> {
        let js_bytes = unsafe { Uint8Array::view(binary) };
        match WebAssembly::validate(&js_bytes.into()) {
            Ok(true) => Ok(()),
            _ => Err(CompileError::Validate("Invalid Wasm file".to_owned())),
        }
    }

    fn compile(store: &Store, binary: &[u8]) -> Result<Self, CompileError> {
        unimplemented!();
    }

    // fn from_artifact(store: &Store, artifact: Arc<dyn Artifact>) -> Self {
    //     unimplemented!();
    // }

    pub(crate) fn instantiate(
        &self,
        resolver: &dyn Resolver,
    ) -> Result<(WebAssembly::Instance, Vec<VMFunction>), ()> {
        let imports = js_sys::Object::new();
        let mut functions: Vec<VMFunction> = vec![];
        for (i, import_type) in self.imports().enumerate() {
            let resolved_import =
                resolver.resolve(i as u32, import_type.module(), import_type.name());
            if let Some(import) = resolved_import {
                match js_sys::Reflect::get(&imports, &import_type.module().into()) {
                    Ok(val) => {
                        if !val.is_undefined() {
                            // If the namespace is already set
                            js_sys::Reflect::set(
                                &val,
                                &import_type.name().into(),
                                import.as_jsvalue(),
                            );
                        } else {
                            // If the namespace doesn't exist
                            let import_namespace = js_sys::Object::new();
                            js_sys::Reflect::set(
                                &import_namespace,
                                &import_type.name().into(),
                                import.as_jsvalue(),
                            );
                            js_sys::Reflect::set(
                                &imports,
                                &import_type.module().into(),
                                &import_namespace.into(),
                            );
                        }
                    }
                    Err(_) => return Err(()),
                };
                if let Export::Function(func) = import {
                    functions.push(func);
                }
            }
            // in case the import is not found, the JS Wasm VM will handle
            // the error for us, so we don't need to handle it
        }
        Ok((
            WebAssembly::Instance::new(&self.module, &imports).unwrap(),
            functions,
        ))
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
    /// # let store = Store::default();
    /// let wat = "(module $moduleName)";
    /// let module = Module::new(&store, wat)?;
    /// assert_eq!(module.name(), Some("moduleName"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn name(&self) -> Option<&str> {
        self.name.as_ref().map(|s| s.as_ref())
        // self.artifact.module_ref().name.as_deref()
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
    /// # let store = Store::default();
    /// let wat = "(module)";
    /// let mut module = Module::new(&store, wat)?;
    /// assert_eq!(module.name(), None);
    /// module.set_name("foo");
    /// assert_eq!(module.name(), Some("foo"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_name(&mut self, name: &str) -> bool {
        self.name = Some(name.to_string());
        true
        // match Reflect::set(self.module.as_ref(), &"wasmer_name".into(), &name.into()) {
        //     Ok(_) => true,
        //     _ => false
        // }
        // Arc::get_mut(&mut self.artifact)
        //     .and_then(|artifact| artifact.module_mut())
        //     .map(|mut module_info| {
        //         module_info.name = Some(name.to_string());
        //         true
        //     })
        //     .unwrap_or(false)
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
    /// # let store = Store::default();
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
    pub fn imports<'a>(&'a self) -> ImportsIterator<impl Iterator<Item = ImportType> + 'a> {
        let imports = WebAssembly::Module::imports(&self.module);
        let iter = imports
            .iter()
            .map(move |val| {
                let module = Reflect::get(val.as_ref(), &"module".into())
                    .unwrap()
                    .as_string()
                    .unwrap();
                let field = Reflect::get(val.as_ref(), &"name".into())
                    .unwrap()
                    .as_string()
                    .unwrap();
                let kind = Reflect::get(val.as_ref(), &"kind".into())
                    .unwrap()
                    .as_string()
                    .unwrap();
                let extern_type = match kind.as_str() {
                    "function" => {
                        let func_type = FunctionType::new(vec![], vec![]);
                        ExternType::Function(func_type)
                    }
                    "global" => {
                        let global_type = GlobalType::new(Type::I32, Mutability::Const);
                        ExternType::Global(global_type)
                    }
                    "memory" => {
                        let memory_type = MemoryType::new(Pages(1), None, false);
                        ExternType::Memory(memory_type)
                    }
                    "table" => {
                        let table_type = TableType::new(Type::FuncRef, 1, None);
                        ExternType::Table(table_type)
                    }
                    _ => unimplemented!(),
                };
                ImportType::new(&module, &field, extern_type)
            })
            .collect::<Vec<_>>()
            .into_iter();
        ImportsIterator {
            iter,
            size: imports.length() as usize,
        }
    }

    /// Set the type hints for this module.
    ///
    /// Returns an error if the hints doesn't match the shape of
    /// import or export types of the module.
    pub fn set_type_hints(&mut self, type_hints: ModuleTypeHints) -> Result<(), String> {
        let exports = WebAssembly::Module::exports(&self.module);
        // Check exports
        if exports.length() as usize != type_hints.exports.len() {
            return Err("The exports length must match the type hints lenght".to_owned());
        }
        for (i, val) in exports.iter().enumerate() {
            let kind = Reflect::get(val.as_ref(), &"kind".into())
                .unwrap()
                .as_string()
                .unwrap();
            // It is safe to unwrap as we have already checked for the exports length
            let type_hint = type_hints.exports.get(i).unwrap();
            let expected_kind = match type_hint {
                ExternType::Function(_) => "function",
                ExternType::Global(_) => "global",
                ExternType::Memory(_) => "memory",
                ExternType::Table(_) => "table",
            };
            if expected_kind != kind.as_str() {
                return Err(format!("The provided type hint for the export {} is {} which doesn't match the expected kind: {}", i, kind.as_str(), expected_kind));
            }
        }
        self.type_hints = Some(type_hints);
        Ok(())
    }

    // /// Get the custom sections of the module given a `name`.
    // pub fn custom_sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Arc<[u8]>> + 'a {
    //     self.custom_sections
    //         .iter()
    //         .filter_map(move |(section_name, section_index)| {
    //             if name != section_name {
    //                 return None;
    //             }
    //             Some(self.custom_sections_data[*section_index].clone())
    //         })
    // }

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
    /// # let store = Store::default();
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
    pub fn exports<'a>(&'a self) -> ExportsIterator<impl Iterator<Item = ExportType> + 'a> {
        let exports = WebAssembly::Module::exports(&self.module);
        let iter = exports
            .iter()
            .enumerate()
            .map(move |(i, val)| {
                let field = Reflect::get(val.as_ref(), &"name".into())
                    .unwrap()
                    .as_string()
                    .unwrap();
                let kind = Reflect::get(val.as_ref(), &"kind".into())
                    .unwrap()
                    .as_string()
                    .unwrap();
                let type_hint = self
                    .type_hints
                    .as_ref()
                    .map(|hints| hints.exports.get(i).unwrap().clone());
                let extern_type = if let Some(hint) = type_hint {
                    hint
                } else {
                    // The default types
                    match kind.as_str() {
                        "function" => {
                            let func_type = FunctionType::new(vec![], vec![]);
                            ExternType::Function(func_type)
                        }
                        "global" => {
                            let global_type = GlobalType::new(Type::I32, Mutability::Const);
                            ExternType::Global(global_type)
                        }
                        "memory" => {
                            let memory_type = MemoryType::new(Pages(1), None, false);
                            ExternType::Memory(memory_type)
                        }
                        "table" => {
                            let table_type = TableType::new(Type::FuncRef, 1, None);
                            ExternType::Table(table_type)
                        }
                        _ => unimplemented!(),
                    }
                };
                ExportType::new(&field, extern_type)
            })
            .collect::<Vec<_>>()
            .into_iter();
        ExportsIterator {
            iter,
            size: exports.length() as usize,
        }
    }
    // /// Get the custom sections of the module given a `name`.
    // ///
    // /// # Important
    // ///
    // /// Following the WebAssembly spec, one name can have multiple
    // /// custom sections. That's why an iterator (rather than one element)
    // /// is returned.
    // pub fn custom_sections<'a>(&'a self, name: &'a str) -> impl Iterator<Item = Arc<[u8]>> + 'a {
    //     unimplemented!();
    //     // self.artifact.module_ref().custom_sections(name)
    // }

    /// Returns the [`Store`] where the `Instance` belongs.
    pub fn store(&self) -> &Store {
        // unimplemented!();
        &self.store
    }
}

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Module")
            .field("name", &self.name())
            .finish()
    }
}

// use anyhow::{bail, Result};
// use std::fmt::Write;
// use wasmparser::*;

// pub fn wasm_types(bytes: &[u8]) -> Result<ModuleTypes> {
//     let mut d = ModuleTypes::new(bytes);
//     d.parse()?;
//     Ok(d.dst)
// }

// struct ModuleTypes<'a> {
//     bytes: &'a [u8],
//     cur: usize,
// }

// #[derive(Default)]
// struct ModuleTypesIndices {
//     funcs: u32,
//     globals: u32,
//     tables: u32,
//     memories: u32,
// }

// const NBYTES: usize = 4;

// impl<'a> ModuleTypes<'a> {
//     fn new(bytes: &'a [u8]) -> Dump<'a> {
//         Dump {
//             bytes,
//             cur: 0,
//             nesting: 0,
//             state: String::new(),
//             dst: String::new(),
//         }
//     }

//     fn run(&mut self) -> Result<()> {
//         self.print_module()?;
//         assert_eq!(self.cur, self.bytes.len());
//         Ok(())
//     }

//     fn print_module(&mut self) -> Result<()> {
//         let mut stack = Vec::new();
//         let mut i = ModuleTypesIndices::default();
//         self.nesting += 1;

//         for item in Parser::new(0).parse_all(self.bytes) {
//             match item? {
//                 Payload::Version { num, range } => {
//                     write!(self.state, "version {}", num)?;
//                     self.print(range.end)?;
//                 }
//                 Payload::TypeSection(s) => self.section(s, "type", |me, end, t| {
//                     write!(me.state, "[type {}] {:?}", i.types, t)?;
//                     i.types += 1;
//                     me.print(end)
//                 })?,
//                 Payload::ImportSection(s) => self.section(s, "import", |me, end, imp| {
//                     write!(me.state, "import ")?;
//                     match imp.ty {
//                         ImportSectionEntryType::Function(_) => {
//                             write!(me.state, "[func {}]", i.funcs)?;
//                             i.funcs += 1;
//                         }
//                         ImportSectionEntryType::Memory(_) => {
//                             write!(me.state, "[memory {}]", i.memories)?;
//                             i.memories += 1;
//                         }
//                         ImportSectionEntryType::Tag(_) => {
//                             write!(me.state, "[tag {}]", i.tags)?;
//                             i.tags += 1;
//                         }
//                         ImportSectionEntryType::Table(_) => {
//                             write!(me.state, "[table {}]", i.tables)?;
//                             i.tables += 1;
//                         }
//                         ImportSectionEntryType::Global(_) => {
//                             write!(me.state, "[global {}]", i.globals)?;
//                             i.globals += 1;
//                         }
//                         ImportSectionEntryType::Instance(_) => {
//                             write!(me.state, "[instance {}]", i.instances)?;
//                             i.instances += 1;
//                         }
//                         ImportSectionEntryType::Module(_) => {
//                             write!(me.state, "[module {}]", i.modules)?;
//                             i.modules += 1;
//                         }
//                     }
//                     write!(me.state, " {:?}", imp)?;
//                     me.print(end)
//                 })?,
//                 Payload::FunctionSection(s) => {
//                     let mut cnt = 0;
//                     self.section(s, "func", |me, end, f| {
//                         write!(me.state, "[func {}] type {:?}", cnt + i.funcs, f)?;
//                         cnt += 1;
//                         me.print(end)
//                     })?
//                 }
//                 Payload::TableSection(s) => self.section(s, "table", |me, end, t| {
//                     write!(me.state, "[table {}] {:?}", i.tables, t)?;
//                     i.tables += 1;
//                     me.print(end)
//                 })?,
//                 Payload::MemorySection(s) => self.section(s, "memory", |me, end, m| {
//                     write!(me.state, "[memory {}] {:?}", i.memories, m)?;
//                     i.memories += 1;
//                     me.print(end)
//                 })?,
//                 Payload::TagSection(s) => self.section(s, "tag", |me, end, m| {
//                     write!(me.state, "[tag {}] {:?}", i.tags, m)?;
//                     i.tags += 1;
//                     me.print(end)
//                 })?,
//                 Payload::ExportSection(s) => self.section(s, "export", |me, end, e| {
//                     write!(me.state, "export {:?}", e)?;
//                     me.print(end)
//                 })?,
//                 Payload::GlobalSection(s) => self.section(s, "global", |me, _end, g| {
//                     write!(me.state, "[global {}] {:?}", i.globals, g.ty)?;
//                     i.globals += 1;
//                     me.print(g.init_expr.get_binary_reader().original_position())?;
//                     me.print_ops(g.init_expr.get_operators_reader())
//                 })?,
//                 Payload::AliasSection(s) => self.section(s, "alias", |me, end, a| {
//                     write!(me.state, "[alias] {:?}", a)?;
//                     match a {
//                         Alias::InstanceExport { kind, .. } => match kind {
//                             ExternalKind::Function => i.funcs += 1,
//                             ExternalKind::Global => i.globals += 1,
//                             ExternalKind::Module => i.modules += 1,
//                             ExternalKind::Table => i.tables += 1,
//                             ExternalKind::Instance => i.instances += 1,
//                             ExternalKind::Memory => i.memories += 1,
//                             ExternalKind::Tag => i.tags += 1,
//                             ExternalKind::Type => i.types += 1,
//                         },
//                         Alias::OuterType { .. } => i.types += 1,
//                         Alias::OuterModule { .. } => i.modules += 1,
//                     }
//                     me.print(end)
//                 })?,
//                 Payload::InstanceSection(s) => {
//                     self.section(s, "instance", |me, _end, instance| {
//                         write!(
//                             me.state,
//                             "[instance {}] instantiate module:{}",
//                             i.instances,
//                             instance.module()
//                         )?;
//                         me.print(instance.original_position())?;
//                         i.instances += 1;
//                         me.print_iter(instance.args()?, |me, end, arg| {
//                             write!(me.state, "[instantiate arg] {:?}", arg)?;
//                             me.print(end)
//                         })
//                     })?
//                 }
//                 Payload::StartSection { func, range } => {
//                     write!(self.state, "start section")?;
//                     self.print(range.start)?;
//                     write!(self.state, "start function {}", func)?;
//                     self.print(range.end)?;
//                 }
//                 Payload::DataCountSection { count, range } => {
//                     write!(self.state, "data count section")?;
//                     self.print(range.start)?;
//                     write!(self.state, "data count {}", count)?;
//                     self.print(range.end)?;
//                 }
//                 Payload::ElementSection(s) => self.section(s, "element", |me, _end, i| {
//                     write!(me.state, "element {:?}", i.ty)?;
//                     let mut items = i.items.get_items_reader()?;
//                     match i.kind {
//                         ElementKind::Passive => {
//                             write!(me.state, " passive, {} items", items.get_count())?;
//                         }
//                         ElementKind::Active {
//                             table_index,
//                             init_expr,
//                         } => {
//                             write!(me.state, " table[{}]", table_index)?;
//                             me.print(init_expr.get_binary_reader().original_position())?;
//                             me.print_ops(init_expr.get_operators_reader())?;
//                             write!(me.state, "{} items", items.get_count())?;
//                         }
//                         ElementKind::Declared => {
//                             write!(me.state, " declared {} items", items.get_count())?;
//                         }
//                     }
//                     me.print(items.original_position())?;
//                     for _ in 0..items.get_count() {
//                         let item = items.read()?;
//                         write!(me.state, "item {:?}", item)?;
//                         me.print(items.original_position())?;
//                     }
//                     Ok(())
//                 })?,

//                 Payload::DataSection(s) => self.section(s, "data", |me, end, i| {
//                     match i.kind {
//                         DataKind::Passive => {
//                             write!(me.state, "data passive")?;
//                             me.print(end - i.data.len())?;
//                         }
//                         DataKind::Active {
//                             memory_index,
//                             init_expr,
//                         } => {
//                             write!(me.state, "data memory[{}]", memory_index)?;
//                             me.print(init_expr.get_binary_reader().original_position())?;
//                             me.print_ops(init_expr.get_operators_reader())?;
//                         }
//                     }
//                     write!(me.dst, "0x{:04x} |", me.cur)?;
//                     for _ in 0..NBYTES {
//                         write!(me.dst, "---")?;
//                     }
//                     write!(me.dst, "-| ... {} bytes of data\n", i.data.len())?;
//                     me.cur = end;
//                     Ok(())
//                 })?,

//                 Payload::CodeSectionStart { count, range, size } => {
//                     write!(self.state, "code section")?;
//                     self.print(range.start)?;
//                     write!(self.state, "{} count", count)?;
//                     self.print(range.end - size as usize)?;
//                 }

//                 Payload::CodeSectionEntry(body) => {
//                     write!(
//                         self.dst,
//                         "============== func {} ====================\n",
//                         i.funcs
//                     )?;
//                     i.funcs += 1;
//                     write!(self.state, "size of function")?;
//                     self.print(body.get_binary_reader().original_position())?;
//                     let mut locals = body.get_locals_reader()?;
//                     write!(self.state, "{} local blocks", locals.get_count())?;
//                     self.print(locals.original_position())?;
//                     for _ in 0..locals.get_count() {
//                         let (amt, ty) = locals.read()?;
//                         write!(self.state, "{} locals of type {:?}", amt, ty)?;
//                         self.print(locals.original_position())?;
//                     }
//                     self.print_ops(body.get_operators_reader()?)?;
//                 }

//                 Payload::ModuleSectionStart { count, range, size } => {
//                     write!(self.state, "module section")?;
//                     self.print(range.start)?;
//                     write!(self.state, "{} count", count)?;
//                     self.print(range.end - size as usize)?;
//                 }
//                 Payload::ModuleSectionEntry { parser: _, range } => {
//                     write!(self.state, "inline module size")?;
//                     self.print(range.start)?;
//                     self.nesting += 1;
//                     stack.push(i);
//                     i = Indices::default();
//                 }

//                 Payload::CustomSection {
//                     name,
//                     data_offset,
//                     data,
//                     range,
//                 } => {
//                     write!(self.state, "custom section")?;
//                     self.print(range.start)?;
//                     write!(self.state, "name: {:?}", name)?;
//                     self.print(data_offset)?;
//                     if name == "name" {
//                         let mut iter = NameSectionReader::new(data, data_offset)?;
//                         while !iter.eof() {
//                             self.print_custom_name_section(iter.read()?, iter.original_position())?;
//                         }
//                     } else {
//                         write!(self.dst, "0x{:04x} |", self.cur)?;
//                         for _ in 0..NBYTES {
//                             write!(self.dst, "---")?;
//                         }
//                         write!(self.dst, "-| ... {} bytes of data\n", data.len())?;
//                         self.cur += data.len();
//                     }
//                 }
//                 Payload::UnknownSection {
//                     id,
//                     range,
//                     contents,
//                 } => {
//                     write!(self.state, "unknown section: {}", id)?;
//                     self.print(range.start)?;
//                     write!(self.dst, "0x{:04x} |", self.cur)?;
//                     for _ in 0..NBYTES {
//                         write!(self.dst, "---")?;
//                     }
//                     write!(self.dst, "-| ... {} bytes of data\n", contents.len())?;
//                     self.cur += contents.len();
//                 }
//                 Payload::End => {
//                     self.nesting -= 1;
//                     if self.nesting > 0 {
//                         i = stack.pop().unwrap();
//                     }
//                 }
//             }
//         }

//         Ok(())
//     }

//     fn print_name_map(&mut self, thing: &str, n: NameMap<'_>) -> Result<()> {
//         write!(self.state, "{} names", thing)?;
//         self.print(n.original_position())?;
//         let mut map = n.get_map()?;
//         write!(self.state, "{} count", map.get_count())?;
//         self.print(map.original_position())?;
//         for _ in 0..map.get_count() {
//             write!(self.state, "{:?}", map.read()?)?;
//             self.print(map.original_position())?;
//         }
//         Ok(())
//     }

//     fn print_indirect_name_map(
//         &mut self,
//         thing_a: &str,
//         thing_b: &str,
//         n: IndirectNameMap<'_>,
//     ) -> Result<()> {
//         write!(self.state, "{} names", thing_b)?;
//         self.print(n.original_position())?;
//         let mut outer_map = n.get_indirect_map()?;
//         write!(self.state, "{} count", outer_map.get_indirect_count())?;
//         self.print(outer_map.original_position())?;
//         for _ in 0..outer_map.get_indirect_count() {
//             let inner = outer_map.read()?;
//             write!(
//                 self.state,
//                 "{} {} {}s",
//                 thing_a, inner.indirect_index, thing_b,
//             )?;
//             self.print(inner.original_position())?;
//             let mut map = inner.get_map()?;
//             write!(self.state, "{} count", map.get_count())?;
//             self.print(map.original_position())?;
//             for _ in 0..map.get_count() {
//                 write!(self.state, "{:?}", map.read()?)?;
//                 self.print(map.original_position())?;
//             }
//         }
//         Ok(())
//     }

//     fn print_custom_name_section(&mut self, name: Name<'_>, end: usize) -> Result<()> {
//         match name {
//             Name::Module(n) => {
//                 write!(self.state, "module name")?;
//                 self.print(n.original_position())?;
//                 write!(self.state, "{:?}", n.get_name()?)?;
//                 self.print(end)?;
//             }
//             Name::Function(n) => self.print_name_map("function", n)?,
//             Name::Local(n) => self.print_indirect_name_map("function", "local", n)?,
//             Name::Label(n) => self.print_indirect_name_map("function", "label", n)?,
//             Name::Type(n) => self.print_name_map("type", n)?,
//             Name::Table(n) => self.print_name_map("table", n)?,
//             Name::Memory(n) => self.print_name_map("memory", n)?,
//             Name::Global(n) => self.print_name_map("global", n)?,
//             Name::Element(n) => self.print_name_map("element", n)?,
//             Name::Data(n) => self.print_name_map("data", n)?,
//             Name::Unknown { ty, range, .. } => {
//                 write!(self.state, "unknown names: {}", ty)?;
//                 self.print(range.start)?;
//                 self.print(end)?;
//             }
//         }
//         Ok(())
//     }

//     fn section<T>(
//         &mut self,
//         iter: T,
//         name: &str,
//         print: impl FnMut(&mut Self, usize, T::Item) -> Result<()>,
//     ) -> Result<()>
//     where
//         T: SectionReader + SectionWithLimitedItems,
//     {
//         write!(self.state, "{} section", name)?;
//         self.print(iter.range().start)?;
//         self.print_iter(iter, print)
//     }

//     fn print_iter<T>(
//         &mut self,
//         mut iter: T,
//         mut print: impl FnMut(&mut Self, usize, T::Item) -> Result<()>,
//     ) -> Result<()>
//     where
//         T: SectionReader + SectionWithLimitedItems,
//     {
//         write!(self.state, "{} count", iter.get_count())?;
//         self.print(iter.original_position())?;
//         for _ in 0..iter.get_count() {
//             let item = iter.read()?;
//             print(self, iter.original_position(), item)?;
//         }
//         if !iter.eof() {
//             bail!("too many bytes in section");
//         }
//         Ok(())
//     }

//     fn print_ops(&mut self, mut i: OperatorsReader) -> Result<()> {
//         while !i.eof() {
//             match i.read() {
//                 Ok(op) => write!(self.state, "{:?}", op)?,
//                 Err(_) => write!(self.state, "??")?,
//             }
//             self.print(i.original_position())?;
//         }
//         Ok(())
//     }

//     fn print(&mut self, end: usize) -> Result<()> {
//         assert!(
//             self.cur < end,
//             "{:#x} >= {:#x}\ntrying to print: {}\n{}",
//             self.cur,
//             end,
//             self.state,
//             self.dst
//         );
//         let bytes = &self.bytes[self.cur..end];
//         for _ in 0..self.nesting - 1 {
//             write!(self.dst, "  ")?;
//         }
//         write!(self.dst, "0x{:04x} |", self.cur)?;
//         for (i, chunk) in bytes.chunks(NBYTES).enumerate() {
//             if i > 0 {
//                 for _ in 0..self.nesting - 1 {
//                     write!(self.dst, "  ")?;
//                 }
//                 self.dst.push_str("       |");
//             }
//             for j in 0..NBYTES {
//                 match chunk.get(j) {
//                     Some(b) => write!(self.dst, " {:02x}", b)?,
//                     None => write!(self.dst, "   ")?,
//                 }
//             }
//             if i == 0 {
//                 self.dst.push_str(" | ");
//                 self.dst.push_str(&self.state);
//                 self.state.truncate(0);
//             }
//             self.dst.push_str("\n");
//         }
//         self.cur = end;
//         Ok(())
//     }
// }
