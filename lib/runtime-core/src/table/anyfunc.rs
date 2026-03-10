use crate::{
    error::CreationError,
    instance::DynFunc,
    sig_registry::SigRegistry,
    structures::TypedIndex,
    types::{FuncSig, TableDescriptor},
    vm,
};

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

pub struct AnyfuncTable {
    pub(crate) backing: Vec<vm::Anyfunc>,
    max: Option<u32>,
}

impl AnyfuncTable {
    pub fn new(
        desc: TableDescriptor,
        local: &mut vm::LocalTable,
    ) -> Result<Box<Self>, CreationError> {
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
