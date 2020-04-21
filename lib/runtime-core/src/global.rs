//! The global module contains data structures and helper functions used to
//! manipulate and access Wasm globals.
use crate::{
    export::Export,
    import::IsExport,
    types::{GlobalType, Type, Value},
    vm,
};
use std::{
    fmt,
    sync::{Arc, Mutex},
};

/// A handle to a Wasm Global
pub struct Global {
    desc: GlobalType,
    storage: Arc<Mutex<vm::LocalGlobal>>,
}

impl Global {
    /// Create a new `Global` value.
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::global::Global;
    /// # use wasmer_runtime_core::types::Value;
    /// let global = Global::new(Value::I32(42));
    /// ```
    pub fn new(value: Value) -> Self {
        Self::new_internal(value, false)
    }

    /// Create a new, mutable `Global` value.
    ///
    /// Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::global::Global;
    /// # use wasmer_runtime_core::types::Value;
    /// let global = Global::new_mutable(Value::I32(42));
    /// ```
    pub fn new_mutable(value: Value) -> Self {
        Self::new_internal(value, true)
    }

    fn new_internal(value: Value, mutable: bool) -> Self {
        let desc = GlobalType {
            mutable,
            ty: value.ty(),
        };

        let local_global = vm::LocalGlobal {
            data: match value {
                Value::I32(x) => x as u128,
                Value::I64(x) => x as u128,
                Value::F32(x) => x.to_bits() as u128,
                Value::F64(x) => x.to_bits() as u128,
                Value::V128(x) => x,
            },
        };

        Self {
            desc,
            storage: Arc::new(Mutex::new(local_global)),
        }
    }

    /// Get the [`GlobalType`] generated for this global.
    ///
    /// [`GlobalType`]: struct.GlobalType.html
    pub fn descriptor(&self) -> GlobalType {
        self.desc
    }

    /// Set the value help by this global.
    ///
    /// This method will panic if the value is
    /// the wrong type.
    pub fn set(&self, value: Value) {
        if self.desc.mutable {
            if self.desc.ty == value.ty() {
                let local_global = vm::LocalGlobal {
                    data: match value {
                        Value::I32(x) => x as u128,
                        Value::I64(x) => x as u128,
                        Value::F32(x) => x.to_bits() as u128,
                        Value::F64(x) => x.to_bits() as u128,
                        Value::V128(x) => x,
                    },
                };
                let mut storage = self.storage.lock().unwrap();
                *storage = local_global;
            } else {
                panic!("Wrong type for setting this global")
            }
        } else {
            panic!("Cannot modify global immutable by default")
        }
    }

    /// Get the value held by this global.
    pub fn get(&self) -> Value {
        let storage = self.storage.lock().unwrap();
        let data = storage.data;

        match self.desc.ty {
            Type::I32 => Value::I32(data as i32),
            Type::I64 => Value::I64(data as i64),
            Type::F32 => Value::F32(f32::from_bits(data as u32)),
            Type::F64 => Value::F64(f64::from_bits(data as u64)),
            Type::V128 => Value::V128(data),
        }
    }

    // TODO: think about this and if this should now be unsafe
    pub(crate) fn vm_local_global(&mut self) -> *mut vm::LocalGlobal {
        let mut storage = self.storage.lock().unwrap();
        &mut *storage
    }
}

impl IsExport for Global {
    fn to_export(&self) -> Export {
        Export::Global(self.clone())
    }
}

impl Clone for Global {
    fn clone(&self) -> Self {
        Self {
            desc: self.desc,
            storage: Arc::clone(&self.storage),
        }
    }
}

impl fmt::Debug for Global {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Global")
            .field("desc", &self.desc)
            .field("value", &self.get())
            .finish()
    }
}
