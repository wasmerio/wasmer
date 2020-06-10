use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::{Store, StoreObject};
use crate::types::{Val, ValType};
use crate::GlobalType;
use crate::Mutability;
use crate::RuntimeError;
use std::fmt;
use wasmer_runtime::{Export, ExportGlobal, VMGlobalDefinition};

#[derive(Clone)]
pub struct Global {
    store: Store,
    exported: ExportGlobal,
}

impl Global {
    pub fn new(store: &Store, val: Val) -> Global {
        // Note: we unwrap because the provided type should always match
        // the value type, so it's safe to unwrap.
        Self::from_type(store, GlobalType::new(val.ty(), Mutability::Const), val).unwrap()
    }

    pub fn new_mut(store: &Store, val: Val) -> Global {
        // Note: we unwrap because the provided type should always match
        // the value type, so it's safe to unwrap.
        Self::from_type(store, GlobalType::new(val.ty(), Mutability::Var), val).unwrap()
    }

    fn from_type(store: &Store, ty: GlobalType, val: Val) -> Result<Global, RuntimeError> {
        if !val.comes_from_same_store(store) {
            return Err(RuntimeError::new("cross-`Store` globals are not supported"));
        }
        let mut definition = VMGlobalDefinition::new();
        unsafe {
            match val {
                Val::I32(x) => *definition.as_i32_mut() = x,
                Val::I64(x) => *definition.as_i64_mut() = x,
                Val::F32(x) => *definition.as_f32_mut() = x,
                Val::F64(x) => *definition.as_f64_mut() = x,
                _ => return Err(RuntimeError::new(format!("create_global for {:?}", val))),
                // Val::V128(x) => *definition.as_u128_bits_mut() = x,
            }
        };
        let exported = ExportGlobal {
            definition: Box::leak(Box::new(definition)),
            global: ty,
        };
        Ok(Global {
            store: store.clone(),
            exported,
        })
    }

    pub fn ty(&self) -> &GlobalType {
        &self.exported.global
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn get(&self) -> Val {
        unsafe {
            let definition = &mut *self.exported.definition;
            match self.ty().ty {
                ValType::I32 => Val::from(*definition.as_i32()),
                ValType::I64 => Val::from(*definition.as_i64()),
                ValType::F32 => Val::F32(*definition.as_f32()),
                ValType::F64 => Val::F64(*definition.as_f64()),
                _ => unimplemented!("Global::get for {:?}", self.ty().ty),
            }
        }
    }

    pub fn set(&self, val: Val) -> Result<(), RuntimeError> {
        if self.ty().mutability != Mutability::Var {
            return Err(RuntimeError::new(
                "immutable global cannot be set".to_string(),
            ));
        }
        if val.ty() != self.ty().ty {
            return Err(RuntimeError::new(format!(
                "global of type {:?} cannot be set to {:?}",
                self.ty().ty,
                val.ty()
            )));
        }
        if !val.comes_from_same_store(&self.store) {
            return Err(RuntimeError::new("cross-`Store` values are not supported"));
        }
        unsafe {
            let definition = &mut *self.exported.definition;
            match val {
                Val::I32(i) => *definition.as_i32_mut() = i,
                Val::I64(i) => *definition.as_i64_mut() = i,
                Val::F32(f) => *definition.as_f32_mut() = f,
                Val::F64(f) => *definition.as_f64_mut() = f,
                _ => unimplemented!("Global::set for {:?}", val.ty()),
            }
        }
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportGlobal) -> Global {
        Global {
            store: store.clone(),
            exported: wasmer_export,
        }
    }

    /// Returns whether or not these two globals refer to the same data.
    pub fn same(&self, other: &Global) -> bool {
        self.exported.same(&other.exported)
    }
}

impl fmt::Debug for Global {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter
            .debug_struct("Global")
            .field("ty", &self.ty())
            .field("value", &self.get())
            .finish()
    }
}

impl<'a> Exportable<'a> for Global {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }

    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
