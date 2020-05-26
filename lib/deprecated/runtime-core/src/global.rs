use crate::{
    new,
    types::{GlobalDescriptor, Value},
};

pub struct Global {
    new_global: new::wasmer::Global,
}

impl Global {
    pub fn new(value: Value) -> Self {
        let store = Default::default();

        Self {
            new_global: new::wasmer::Global::new(&store, value),
        }
    }

    pub fn new_mutable(value: Value) -> Self {
        let store = Default::default();

        Self {
            new_global: new::wasmer::Global::new_mut(&store, value),
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
