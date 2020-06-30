use crate::{
    error::ExportError,
    get_global_store, new,
    types::{GlobalDescriptor, Value},
};
use std::fmt;

#[derive(Clone)]
pub struct Global {
    new_global: new::wasmer::Global,
}

impl Global {
    pub fn new(value: Value) -> Self {
        Self {
            new_global: new::wasmer::Global::new(get_global_store(), value),
        }
    }

    pub fn new_mutable(value: Value) -> Self {
        Self {
            new_global: new::wasmer::Global::new_mut(get_global_store(), value),
        }
    }

    pub fn descriptor(&self) -> GlobalDescriptor {
        self.new_global.ty().into()
    }

    pub fn set(&self, value: Value) {
        self.new_global.set(value).unwrap()
    }

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
    fn to_export(&self) -> new::wasmer_runtime::Export {
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
}
