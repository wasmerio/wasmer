use crate::{
    export::Export,
    import::IsExport,
    types::{GlobalDescriptor, Type, Value},
    vm,
};
use std::{cell::RefCell, fmt, rc::Rc};

pub struct Global {
    desc: GlobalDescriptor,
    storage: Rc<RefCell<vm::LocalGlobal>>,
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
        let desc = GlobalDescriptor {
            mutable,
            ty: value.ty(),
        };

        let local_global = vm::LocalGlobal {
            data: match value {
                Value::I32(x) => x as u64,
                Value::I64(x) => x as u64,
                Value::F32(x) => x.to_bits() as u64,
                Value::F64(x) => x.to_bits(),
            },
        };

        Self {
            desc,
            storage: Rc::new(RefCell::new(local_global)),
        }
    }

    /// Get the [`GlobalDescriptor`] generated for this global.
    ///
    /// [`GlobalDescriptor`]: struct.GlobalDescriptor.html
    pub fn descriptor(&self) -> GlobalDescriptor {
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
                        Value::I32(x) => x as u64,
                        Value::I64(x) => x as u64,
                        Value::F32(x) => x.to_bits() as u64,
                        Value::F64(x) => x.to_bits(),
                    },
                };
                *self.storage.borrow_mut() = local_global;
            } else {
                panic!("Wrong type for setting this global")
            }
        } else {
            panic!("Cannot modify global immutable by default")
        }
    }

    /// Get the value held by this global.
    pub fn get(&self) -> Value {
        let data = self.storage.borrow().data;

        match self.desc.ty {
            Type::I32 => Value::I32(data as i32),
            Type::I64 => Value::I64(data as i64),
            Type::F32 => Value::F32(f32::from_bits(data as u32)),
            Type::F64 => Value::F64(f64::from_bits(data)),
        }
    }

    pub(crate) fn vm_local_global(&mut self) -> *mut vm::LocalGlobal {
        &mut *self.storage.borrow_mut()
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
            storage: Rc::clone(&self.storage),
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
