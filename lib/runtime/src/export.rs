use crate::{
    instance::FuncRef,
    types::{FuncSig, GlobalDesc, Memory, Table},
    vm,
};

#[derive(Debug, Copy, Clone)]
pub enum Context {
    External(*mut vm::Ctx),
    Internal,
}

#[derive(Debug, Clone)]
pub enum Export {
    Function {
        func: FuncRef,
        ctx: Context,
        signature: FuncSig,
    },
    Memory {
        local: *mut vm::LocalMemory,
        ctx: Context,
        memory: Memory,
    },
    Table {
        local: *mut vm::LocalTable,
        ctx: Context,
        table: Table,
    },
    Global {
        local: *mut vm::LocalGlobal,
        global: GlobalDesc,
    },
}
