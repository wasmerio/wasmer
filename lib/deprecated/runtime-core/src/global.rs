use crate::{
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
