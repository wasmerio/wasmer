use std::any::{Any, TypeId};

/// Underlying FunctionEnvironment used by a `VMFunction`.
pub struct VMFunctionEnvironment {
    /// The contents of the environment. Private, so that its type stays the
    /// one recorded in `type_id`: a new type takes a new environment.
    contents: Box<dyn Any + Send + 'static>,
    /// The type of `contents`, recorded when the environment is made, which
    /// [`Self::downcast_ref`] and [`Self::downcast_mut`] compare without
    /// calling into `contents`' vtable, as `<dyn Any>::downcast_ref` does.
    type_id: TypeId,
}

impl std::fmt::Debug for VMFunctionEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunctionEnvironment")
            .field("contents", &(&*self.contents as *const _))
            .finish()
    }
}

impl VMFunctionEnvironment {
    /// Wraps the given value to expose it to Wasm code as a function context.
    pub fn new<T: Any + Send + 'static>(val: T) -> Self {
        Self {
            contents: Box::new(val),
            type_id: TypeId::of::<T>(),
        }
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a reference to the underlying value.
    pub fn as_ref(&self) -> &(dyn Any + Send + 'static) {
        &*self.contents
    }

    #[allow(clippy::should_implement_trait)]
    /// Returns a mutable reference to the underlying value.
    pub fn as_mut(&mut self) -> &mut (dyn Any + Send + 'static) {
        &mut *self.contents
    }

    /// Returns the underlying value, consuming the environment.
    pub fn into_contents(self) -> Box<dyn Any + Send + 'static> {
        self.contents
    }

    /// Returns the underlying value if it is a `T`.
    #[inline]
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        if self.type_id == TypeId::of::<T>() {
            // SAFETY: `contents` holds a value of the type recorded in
            // `type_id`, which is `T`.
            Some(unsafe { &*(&*self.contents as *const (dyn Any + Send) as *const T) })
        } else {
            None
        }
    }

    /// Returns the underlying value mutably if it is a `T`.
    #[inline]
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        if self.type_id == TypeId::of::<T>() {
            // SAFETY: as in `downcast_ref`.
            Some(unsafe { &mut *(&mut *self.contents as *mut (dyn Any + Send) as *mut T) })
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{StoreHandle, StoreId, StoreObject, StoreObjects};

    #[test]
    fn downcasts_to_the_recorded_type_only() {
        let mut env = VMFunctionEnvironment::new(7u32);
        assert_eq!(env.downcast_ref::<u32>(), Some(&7));
        assert!(env.downcast_ref::<i32>().is_none());
        *env.downcast_mut::<u32>().unwrap() += 1;
        assert_eq!(env.as_ref().downcast_ref::<u32>(), Some(&8));
        assert!(env.downcast_mut::<u64>().is_none());
    }

    /// The environments of a store can be replaced through `StoreObject`,
    /// with values of another type: the type recorded with each environment
    /// goes with it, and a handle made for the old type is refused.
    #[test]
    fn a_replaced_environment_carries_its_own_type() {
        let mut objects = StoreObjects::new(
            StoreId::default(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let handle = StoreHandle::new(&mut objects, VMFunctionEnvironment::new(7u32));
        VMFunctionEnvironment::list_mut(&mut objects)[0] = VMFunctionEnvironment::new(1u8);
        let env = handle.get(&objects);
        assert!(env.downcast_ref::<u32>().is_none());
        assert_eq!(env.downcast_ref::<u8>(), Some(&1));
    }
}
