//! The export module contains the implementation data structures and helper functions used to
//! manipulate and access a wasm module's exports including memories, tables, globals, and
//! functions.
use crate::{
    global::Global, instance::InstanceInner, memory::Memory, module::ExportIndex,
    module::ModuleInner, table::Table, types::FuncSig, vm,
};
use indexmap::map::Iter as IndexMapIter;
use std::{ptr::NonNull, sync::Arc};

/// A kind of Context.
#[derive(Debug, Copy, Clone)]
pub enum Context {
    /// External context include a mutable pointer to `Ctx`.
    External(*mut vm::Ctx),

    /// External context with an environment include a mutable pointer
    /// to `Ctx` and an optional non-null pointer to `FuncEnv`.
    ExternalWithEnv(*mut vm::Ctx, Option<NonNull<vm::FuncEnv>>),

    /// Internal context.
    Internal,
}

// Manually implemented because context contains a raw pointer to Ctx
unsafe impl Send for Context {}

/// Kind of WebAssembly export.
#[derive(Debug, Clone)]
pub enum Export {
    /// Function export.
    Function {
        /// A pointer to a function.
        func: FuncPointer,
        /// A kind of context.
        ctx: Context,
        /// The signature of the function.
        signature: Arc<FuncSig>,
    },
    /// Memory export.
    Memory(Memory),
    /// Table export.
    Table(Table),
    /// Global export.
    Global(Global),
}

/// Const pointer to a `Func`.
#[derive(Debug, Clone)]
pub struct FuncPointer(*const vm::Func);

// Manually implemented because FuncPointer contains a raw pointer to Ctx
unsafe impl Send for FuncPointer {}

impl FuncPointer {
    /// This needs to be unsafe because there is
    /// no way to check whether the passed function
    /// is valid and has the right signature.
    pub unsafe fn new(f: *const vm::Func) -> Self {
        FuncPointer(f)
    }

    pub(crate) fn inner(&self) -> *const vm::Func {
        self.0
    }
}

/// An iterator to an instance's exports.
pub struct ExportIter<'a> {
    inner: &'a InstanceInner,
    iter: IndexMapIter<'a, String, ExportIndex>,
    module: &'a ModuleInner,
}

impl<'a> ExportIter<'a> {
    pub(crate) fn new(module: &'a ModuleInner, inner: &'a InstanceInner) -> Self {
        Self {
            inner,
            iter: module.info.exports.iter(),
            module,
        }
    }
}

impl<'a> Iterator for ExportIter<'a> {
    type Item = (String, Export);
    fn next(&mut self) -> Option<(String, Export)> {
        let (name, export_index) = self.iter.next()?;
        Some((
            name.clone(),
            self.inner.get_export_from_index(&self.module, export_index),
        ))
    }
}
