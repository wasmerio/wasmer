use crate::{
    global::Global, instance::InstanceInner, memory::Memory, module::ExportIndex,
    module::ModuleInner, table::Table, types::FuncSig, vm,
};
use hashbrown::hash_map;
use std::sync::Arc;

#[derive(Debug, Copy, Clone)]
pub enum Context {
    External(*mut vm::Ctx),
    Internal,
}

#[derive(Debug, Clone)]
pub enum Export {
    Function {
        func: FuncPointer,
        ctx: Context,
        signature: Arc<FuncSig>,
    },
    Memory(Memory),
    Table(Table),
    Global(Global),
}

#[derive(Debug, Clone)]
pub struct FuncPointer(*const vm::Func);

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

pub struct ExportIter<'a> {
    inner: &'a mut InstanceInner,
    iter: hash_map::Iter<'a, String, ExportIndex>,
    module: &'a ModuleInner,
}

impl<'a> ExportIter<'a> {
    pub(crate) fn new(module: &'a ModuleInner, inner: &'a mut InstanceInner) -> Self {
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
