#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub use wai_bindgen_wasmer_impl::{export, import};

#[cfg(feature = "async")]
pub use async_trait::async_trait;
#[cfg(feature = "tracing-lib")]
pub use tracing_lib as tracing;
#[doc(hidden)]
pub use {anyhow, bitflags, once_cell, wasmer};

mod error;
mod le;
mod region;
mod slab;
mod table;

pub use error::GuestError;
pub use le::{Endian, Le};
pub use region::{AllBytesValid, BorrowChecker, Region};
pub use table::*;

pub struct RawMemory {
    pub slice: *mut [u8],
}

// This type is threadsafe despite its internal pointer because it allows no
// safe access to the internal pointer. Consumers must uphold Send/Sync
// guarantees themselves.
unsafe impl Send for RawMemory {}
unsafe impl Sync for RawMemory {}

#[doc(hidden)]
pub mod rt {
    use crate::slab::Slab;
    use crate::{Endian, Le};
    use std::mem;
    use wasmer::*;

    pub trait RawMem {
        fn store<T: Endian>(&mut self, offset: i32, val: T) -> Result<(), RuntimeError>;
        fn store_many<T: Endian>(&mut self, offset: i32, vals: &[T]) -> Result<(), RuntimeError>;
        fn load<T: Endian>(&self, offset: i32) -> Result<T, RuntimeError>;
    }

    impl RawMem for [u8] {
        fn store<T: Endian>(&mut self, offset: i32, val: T) -> Result<(), RuntimeError> {
            let mem = self
                .get_mut(offset as usize..)
                .and_then(|m| m.get_mut(..mem::size_of::<T>()))
                .ok_or_else(|| RuntimeError::new("out of bounds write"))?;
            Le::from_slice_mut(mem)[0].set(val);
            Ok(())
        }

        fn store_many<T: Endian>(&mut self, offset: i32, val: &[T]) -> Result<(), RuntimeError> {
            let mem = self
                .get_mut(offset as usize..)
                .and_then(|m| {
                    let len = mem::size_of::<T>().checked_mul(val.len())?;
                    m.get_mut(..len)
                })
                .ok_or_else(|| RuntimeError::new("out of bounds write"))?;
            for (slot, val) in Le::from_slice_mut(mem).iter_mut().zip(val) {
                slot.set(*val);
            }
            Ok(())
        }

        fn load<T: Endian>(&self, offset: i32) -> Result<T, RuntimeError> {
            let mem = self
                .get(offset as usize..)
                .and_then(|m| m.get(..mem::size_of::<Le<T>>()))
                .ok_or_else(|| RuntimeError::new("out of bounds read"))?;
            Ok(Le::from_slice(mem)[0].get())
        }
    }

    pub fn char_from_i32(val: i32) -> Result<char, RuntimeError> {
        core::char::from_u32(val as u32)
            .ok_or_else(|| RuntimeError::new("char value out of valid range"))
    }

    pub fn invalid_variant(name: &str) -> RuntimeError {
        let msg = format!("invalid discriminant for `{name}`");
        RuntimeError::new(msg)
    }

    pub fn validate_flags<T, U>(
        bits: T,
        all: T,
        name: &str,
        mk: impl FnOnce(T) -> U,
    ) -> Result<U, RuntimeError>
    where
        T: std::ops::Not<Output = T> + std::ops::BitAnd<Output = T> + From<u8> + PartialEq + Copy,
    {
        if bits & !all != 0u8.into() {
            let msg = format!("invalid flags specified for `{name}`");
            Err(RuntimeError::new(msg))
        } else {
            Ok(mk(bits))
        }
    }

    pub fn bad_int(_: std::num::TryFromIntError) -> RuntimeError {
        let msg = "out-of-bounds integer conversion";
        RuntimeError::new(msg)
    }

    pub fn copy_slice<T: Endian>(
        store: &mut wasmer::Store,
        memory: &Memory,
        free: &TypedFunction<(i32, i32, i32), ()>,
        base: i32,
        len: i32,
        align: i32,
    ) -> Result<Vec<T>, RuntimeError> {
        let size = (len as u32)
            .checked_mul(mem::size_of::<T>() as u32)
            .ok_or_else(|| RuntimeError::new("array too large to fit in wasm memory"))?;
        let memory_view = memory.view(store);
        let slice = unsafe {
            memory_view
                .data_unchecked()
                .get(base as usize..)
                .and_then(|s| s.get(..size as usize))
                .ok_or_else(|| RuntimeError::new("out of bounds read"))?
        };
        let result = Le::from_slice(slice).iter().map(|s| s.get()).collect();
        free.call(store, base, size as i32, align)?;
        Ok(result)
    }

    macro_rules! as_traits {
        ($(($name:ident $tr:ident $ty:ident ($($tys:ident)*)))*) => ($(
            pub fn $name<T: $tr>(t: T) -> $ty {
                t.$name()
            }

            pub trait $tr {
                #[allow(clippy::wrong_self_convention)]
                fn $name(self) -> $ty;
            }

            impl<'a, T: Copy + $tr> $tr for &'a T {
                fn $name(self) -> $ty {
                    (*self).$name()
                }
            }

            $(
                impl $tr for $tys {
                    #[inline]
                    fn $name(self) -> $ty {
                        self as $ty
                    }
                }
            )*
        )*)
    }

    as_traits! {
        (as_i32 AsI32 i32 (char i8 u8 i16 u16 i32 u32))
        (as_i64 AsI64 i64 (i64 u64))
        (as_f32 AsF32 f32 (f32))
        (as_f64 AsF64 f64 (f64))
    }

    #[derive(Default, Debug)]
    pub struct IndexSlab {
        slab: Slab<ResourceIndex>,
    }

    impl IndexSlab {
        pub fn insert(&mut self, resource: ResourceIndex) -> u32 {
            self.slab.insert(resource)
        }

        pub fn get(&self, slab_idx: u32) -> Result<ResourceIndex, RuntimeError> {
            match self.slab.get(slab_idx) {
                Some(idx) => Ok(*idx),
                None => Err(RuntimeError::new("invalid index specified for handle")),
            }
        }

        pub fn remove(&mut self, slab_idx: u32) -> Result<ResourceIndex, RuntimeError> {
            match self.slab.remove(slab_idx) {
                Some(idx) => Ok(idx),
                None => Err(RuntimeError::new("invalid index specified for handle")),
            }
        }
    }

    #[derive(Default, Debug)]
    pub struct ResourceSlab {
        slab: Slab<Resource>,
    }

    #[derive(Debug)]
    struct Resource {
        wasm: i32,
        refcnt: u32,
    }

    #[derive(Debug, Copy, Clone)]
    pub struct ResourceIndex(u32);

    impl ResourceSlab {
        pub fn insert(&mut self, wasm: i32) -> ResourceIndex {
            ResourceIndex(self.slab.insert(Resource { wasm, refcnt: 1 }))
        }

        pub fn get(&self, idx: ResourceIndex) -> i32 {
            self.slab.get(idx.0).unwrap().wasm
        }

        pub fn clone(&mut self, idx: ResourceIndex) -> Result<(), RuntimeError> {
            let resource = self.slab.get_mut(idx.0).unwrap();
            resource.refcnt = match resource.refcnt.checked_add(1) {
                Some(cnt) => cnt,
                None => return Err(RuntimeError::new("resource index count overflow")),
            };
            Ok(())
        }

        pub fn drop(&mut self, idx: ResourceIndex) -> Option<i32> {
            let resource = self.slab.get_mut(idx.0).unwrap();
            assert!(resource.refcnt > 0);
            resource.refcnt -= 1;
            if resource.refcnt != 0 {
                return None;
            }
            let resource = self.slab.remove(idx.0).unwrap();
            Some(resource.wasm)
        }
    }
}
