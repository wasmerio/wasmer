use crate::{
    instance::InstanceInner,
    module::ExportIndex,
    module::ModuleInner,
    types::{FuncSig, GlobalDesc, Memory, Table},
    vm,
};
use hashbrown::hash_map;

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
        signature: FuncSig,
    },
    Memory {
        local: MemoryPointer,
        ctx: Context,
        memory: Memory,
    },
    Table {
        local: TablePointer,
        ctx: Context,
        table: Table,
    },
    Global {
        local: GlobalPointer,
        global: GlobalDesc,
    },
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

#[derive(Debug, Clone)]
pub struct MemoryPointer(*mut vm::LocalMemory);

impl MemoryPointer {
    /// This needs to be unsafe because there is
    /// no way to check whether the passed function
    /// is valid and has the right signature.
    pub unsafe fn new(f: *mut vm::LocalMemory) -> Self {
        MemoryPointer(f)
    }

    pub(crate) fn inner(&self) -> *mut vm::LocalMemory {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct TablePointer(*mut vm::LocalTable);

impl TablePointer {
    /// This needs to be unsafe because there is
    /// no way to check whether the passed function
    /// is valid and has the right signature.
    pub unsafe fn new(f: *mut vm::LocalTable) -> Self {
        TablePointer(f)
    }

    pub(crate) fn inner(&self) -> *mut vm::LocalTable {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct GlobalPointer(*mut vm::LocalGlobal);

impl GlobalPointer {
    /// This needs to be unsafe because there is
    /// no way to check whether the passed function
    /// is valid and has the right signature.
    pub unsafe fn new(f: *mut vm::LocalGlobal) -> Self {
        GlobalPointer(f)
    }

    pub(crate) fn inner(&self) -> *mut vm::LocalGlobal {
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
            iter: module.exports.iter(),
            module: module,
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
