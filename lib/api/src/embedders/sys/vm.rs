use crate::{
    embedders::{vm_gen_impl, vm_impex},
    vm::*,
};
use wasmer_vm::InternalStoreHandle;

vm_impex!(
    wasmer_vm,
    Sys,
    VMConfig,
    VMExtern,
    VMExternRef,
    VMFuncRef,
    VMFunction,
    VMFunctionBody,
    VMFunctionEnvironment,
    VMGlobal,
    VMInstance,
    VMMemory,
    VMSharedMemory,
    VMTable,
    VMTrampoline
);

pub(crate) type SysVMExternTable = InternalStoreHandle<SysVMTable>;
pub(crate) type SysVMExternMemory = InternalStoreHandle<SysVMMemory>;
pub(crate) type SysVMExternGlobal = InternalStoreHandle<SysVMGlobal>;
pub(crate) type SysVMExternFunction = InternalStoreHandle<SysVMFunction>;
pub(crate) type SysVMFunctionCallback = *const SysVMFunctionBody;

vm_gen_impl!(
    sys,
    Sys,
    VMConfig,
    VMExtern,
    VMExternRef,
    VMFuncRef,
    VMFunction,
    VMFunctionBody,
    VMFunctionEnvironment,
    VMGlobal,
    VMInstance,
    VMMemory,
    VMSharedMemory,
    VMTable,
    VMTrampoline,
    VMExternTable,
    VMExternMemory,
    VMExternGlobal,
    VMExternFunction,
    VMFunctionCallback
);
