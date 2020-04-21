use crate::{
    error::CreationError,
    instance::DynFunc,
    sig_registry::SigRegistry,
    structures::TypedIndex,
    types::{FuncSig, TableType},
    vm,
};

use std::convert::TryFrom;
use std::{ptr, sync::Arc};

enum AnyfuncInner<'a> {
    Host {
        ptr: *const vm::Func,
        signature: Arc<FuncSig>,
    },
    Managed(DynFunc<'a>),
}

/// Anyfunc data type.
pub struct Anyfunc<'a> {
    inner: AnyfuncInner<'a>,
}

impl<'a> Anyfunc<'a> {
    /// Create a new `Anyfunc`.
    pub unsafe fn new<Sig>(func: *const vm::Func, signature: Sig) -> Self
    where
        Sig: Into<Arc<FuncSig>>,
    {
        Self {
            inner: AnyfuncInner::Host {
                ptr: func as _,
                signature: signature.into(),
            },
        }
    }
}

impl<'a> From<DynFunc<'a>> for Anyfunc<'a> {
    fn from(function: DynFunc<'a>) -> Self {
        Anyfunc {
            inner: AnyfuncInner::Managed(function),
        }
    }
}

impl<'a> TryFrom<Anyfunc<'a>> for DynFunc<'a> {
    type Error = ();

    fn try_from(anyfunc: Anyfunc<'a>) -> Result<Self, Self::Error> {
        match anyfunc.inner {
            AnyfuncInner::Managed(df) => Ok(df),
            _ => Err(()),
        }
    }
}

/*
// TODO: implement this when `vm::Anyfunc` is updated (aka avoiding the linear scan in `wrap`)
impl<'a, Args: WasmTypeList, Rets: WasmTypeList> TryFrom<Anyfunc<'a>> for Func<'a, Args, Rets> {
    type Error = ();

    fn try_from(anyfunc: Anyfunc<'a>) -> Result<Self, Self::Error> {
        match anyfunc.inner {
            AnyfuncInner::Host {
                ptr,
                ctx,
                signature,
            } => {
                // TODO: return more specific error
                let ptr = NonNull::new(ptr as _).ok_or(())?;
                if signature.params() != Args::types() || signature.returns() != Rets::types() {
                    // TODO: return more specific error
                    return Err(());
                }
                let wasm = todo!("Figure out how to get typed_func::Wasm");
                // TODO: handle func_env
                let func_env = None;
                Ok(unsafe { Func::from_raw_parts(wasm, ptr, func_env, ctx) })
            }
            _ => Err(()),
        }
    }
}
*/

pub struct AnyfuncTable {
    pub(crate) backing: Vec<vm::Anyfunc>,
    max: Option<u32>,
}

impl AnyfuncTable {
    pub fn new(desc: TableType, local: &mut vm::LocalTable) -> Result<Box<Self>, CreationError> {
        let initial_table_backing_len = desc.minimum as usize;

        let mut storage = Box::new(AnyfuncTable {
            backing: vec![vm::Anyfunc::null(); initial_table_backing_len],
            max: desc.maximum,
        });

        let storage_ptr: *mut AnyfuncTable = &mut *storage;

        local.base = storage.backing.as_mut_ptr() as *mut u8;
        local.count = storage.backing.len();
        local.table = storage_ptr as *mut ();

        Ok(storage)
    }

    pub fn current_size(&self) -> u32 {
        self.backing.len() as u32
    }

    pub fn internal_buffer(&mut self) -> &mut [vm::Anyfunc] {
        &mut self.backing
    }

    pub fn grow(&mut self, delta: u32, local: &mut vm::LocalTable) -> Option<u32> {
        let starting_len = self.backing.len() as u32;

        let new_len = starting_len.checked_add(delta)?;

        if let Some(max) = self.max {
            if new_len > max {
                return None;
            }
        }

        self.backing.resize(new_len as usize, vm::Anyfunc::null());

        local.base = self.backing.as_mut_ptr() as *mut u8;
        local.count = self.backing.len();

        Some(starting_len)
    }

    // hidden and `pub(crate)` due to incomplete implementation (blocked on `wrap` issue)
    #[doc(hidden)]
    /// Get The vm::AnyFunc at the given index.
    pub(crate) fn get<'outer_table>(&self, index: u32) -> Option<Anyfunc<'outer_table>> {
        let vm_any_func = self.backing.get(index as usize)?;
        let signature = SigRegistry.lookup_signature(vm_any_func.sig_id.into());
        // TODO: this function should take a generic type param indicating what type of
        // anyfunc we want `host` or `managed` (or perhaps we should just return DynFunc/Func directly here).
        //
        // The issue with the current implementation is that through `StorableInTable`, we'll call
        // `TryFrom<Anyfuc> for Dynfunc` which will always fail because we always return a `Host` function here.
        Some(Anyfunc {
            inner: AnyfuncInner::Host {
                ptr: vm_any_func.func,
                signature,
            },
        })
    }

    pub fn set(&mut self, index: u32, element: Anyfunc) -> Result<(), ()> {
        if let Some(slot) = self.backing.get_mut(index as usize) {
            let anyfunc = match element.inner {
                AnyfuncInner::Host { ptr, signature } => {
                    let sig_index = SigRegistry.lookup_sig_index(signature);
                    let sig_id = vm::SigId(sig_index.index() as u32);

                    vm::Anyfunc {
                        func: ptr,
                        ctx: ptr::null_mut(),
                        sig_id,
                    }
                }
                AnyfuncInner::Managed(ref func) => {
                    let sig_index = SigRegistry.lookup_sig_index(Arc::clone(&func.signature));
                    let sig_id = vm::SigId(sig_index.index() as u32);

                    vm::Anyfunc {
                        func: func.raw(),
                        ctx: func.instance_inner.vmctx,
                        sig_id,
                    }
                }
            };

            *slot = anyfunc;

            Ok(())
        } else {
            Err(())
        }
    }
}
