use crate::{
    error::ExportError,
    get_global_store, new,
    types::{GlobalDescriptor, Value},
};
use std::fmt;

/// A handle to a Wasm Global
#[derive(Clone)]
pub struct Global {
    new_global: new::wasmer::Global,
}

impl Global {
    /// Create a new `Global` value.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer_runtime_core::{global::Global, types::Value};
    /// let global = Global::new(Value::I32(42));
    /// ```
    pub fn new(value: Value) -> Self {
        Self {
            new_global: new::wasmer::Global::new(&get_global_store(), value),
        }
    }

    /// Create a new, mutable `Global` value.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer_runtime_core::{global::Global, types::Value};
    /// let global = Global::new_mutable(Value::I32(42));
    /// ```
    pub fn new_mutable(value: Value) -> Self {
        Self {
            new_global: new::wasmer::Global::new_mut(&get_global_store(), value),
        }
    }

    /// Get the [`GlobalDescriptor`] generated for this global.
    ///
    /// [`GlobalDescriptor`]: struct.GlobalDescriptor.html
    pub fn descriptor(&self) -> GlobalDescriptor {
        self.new_global.ty().into()
    }

    /// Set the value help by this global.
    ///
    /// This method will panic if the value is
    /// the wrong type.
    pub fn set(&self, value: Value) {
        self.new_global.set(value).unwrap()
    }

    /// Get the value held by this global.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer_runtime_core::{global::Global, types::Value};
    /// let global = Global::new_mutable(Value::I32(42));
    /// assert_eq!(global.get(), Value::I32(42));
    ///
    /// global.set(Value::I32(7));
    /// assert_eq!(global.get(), Value::I32(7));
    /// ```
    pub fn get(&self) -> Value {
        self.new_global.get()
    }
}

impl fmt::Debug for Global {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{:?}", &self.new_global)
    }
}

impl From<&new::wasmer::Global> for Global {
    fn from(new_global: &new::wasmer::Global) -> Self {
        Self {
            new_global: new_global.clone(),
        }
    }
}

impl<'a> new::wasmer::Exportable<'a> for Global {
    fn to_export(&self) -> new::wasmer::Export {
        self.new_global.to_export()
    }

    fn get_self_from_extern(r#extern: &'a new::wasmer::Extern) -> Result<&'a Self, ExportError> {
        match r#extern {
            new::wasmer::Extern::Global(global) => Ok(
                // It's not ideal to call `Box::leak` here, but it
                // would introduce too much changes in the
                // `new::wasmer` API to support `Cow` or similar.
                Box::leak(Box::<Global>::new(global.into())),
            ),
            _ => Err(ExportError::IncompatibleType),
        }
    }

    fn into_weak_instance_ref(&mut self) {
        self.new_global.into_weak_instance_ref();
    }
}
