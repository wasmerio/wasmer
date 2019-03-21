use parking_lot::{Once, ONCE_INIT};
use std::cell::UnsafeCell;
use std::ops::Deref;

unsafe impl<T: Sync, F> Sync for Lazy<T, F> {}
unsafe impl<T: Send + Sync, F> Send for Lazy<T, F> {}

pub struct Lazy<T, F = fn() -> T> {
    once: Once,
    value: UnsafeCell<Option<T>>,
    init: UnsafeCell<Option<F>>,
}

impl<T, F> Lazy<T, F> {
    pub const fn new(init: F) -> Self {
        Self {
            once: ONCE_INIT,
            value: UnsafeCell::new(None),
            init: UnsafeCell::new(Some(init)),
        }
    }
}

impl<T, F> Deref for Lazy<T, F>
where
    F: FnOnce() -> T,
{
    type Target = T;

    fn deref(&self) -> &T {
        self.once.call_once(|| {
            let init = unsafe {
                if let Some(init) = (&mut *self.init.get()).take() {
                    init
                } else {
                    std::hint::unreachable_unchecked()
                }
            };

            let v = init();
            unsafe {
                self.value.get().write(Some(v));
            }
        });

        unsafe {
            if let Some(ref v) = &*self.value.get() {
                v
            } else {
                std::hint::unreachable_unchecked()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn as_static() {
        static LAZY: Lazy<i32> = Lazy::new(|| 42);

        for _ in 0..1000 {
            assert_eq!(*LAZY, 42);
        }
    }

    #[test]
    pub fn as_local() {
        let lazy = Lazy::new(|| 42);

        for _ in 0..1000 {
            assert_eq!(*lazy, 42);
        }
    }
}
