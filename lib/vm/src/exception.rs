use backtrace::Backtrace;
use std::{cell::UnsafeCell, ptr::NonNull};
use wasmer_types::{RawValue, StoreId};

use crate::{StoreHandle, StoreObjects, VMTag, store::InternalStoreHandle};

/// Underlying object referenced by a `VMExceptionRef`.
#[derive(Debug)]
pub struct VMExceptionObj {
    tag: u32,
    payload: Box<UnsafeCell<[RawValue]>>,
    backtrace: Backtrace,
}

impl VMExceptionObj {
    /// Creates a new VMExceptionObj from the given tag and values; the tag is assumed
    /// to be from the same store as the VMExceptionObj itself.
    pub fn new(tag: InternalStoreHandle<VMTag>, payload: Box<[RawValue]>) -> Self {
        let payload = Box::into_raw(payload);
        let backtrace = Backtrace::new_unresolved();
        // SAFETY: [RawValue] and UnsafeCell[RawValue] have the same memory layout, and Box itself
        // does not enable any niche optimizations (of the kind that break Outer<UnsafeCell<T>>).
        Self {
            tag: tag.index() as u32,
            payload: unsafe { Box::from_raw(payload as *mut UnsafeCell<[RawValue]>) },
            backtrace,
        }
    }

    /// Creates a new VMExceptionObj from the given tag with all values initialized to
    /// zero; the tag is assumed to be from the same store as the VMExceptionObj itself.
    pub fn new_zeroed(ctx: &StoreObjects, tag: InternalStoreHandle<VMTag>) -> Self {
        let value_count = tag.get(ctx).signature.params().len();
        let values = Box::into_raw(vec![RawValue::default(); value_count].into_boxed_slice());
        let backtrace = Backtrace::new_unresolved();
        // SAFETY: [RawValue] and UnsafeCell[RawValue] have the same memory layout, and Box itself
        // does not enable any niche optimizations (of the kind that break Outer<UnsafeCell<T>>).
        Self {
            tag: tag.index() as u32,
            payload: unsafe { Box::from_raw(values as *mut UnsafeCell<[RawValue]>) },
            backtrace,
        }
    }

    #[cfg_attr(
        not(any(
            all(target_family = "windows", target_env = "gnu"),
            target_family = "unix",
        )),
        allow(unused)
    )]
    pub(crate) fn tag_index(&self) -> u32 {
        self.tag
    }

    /// Gets the tag of this exception.
    pub fn tag(&self) -> InternalStoreHandle<VMTag> {
        InternalStoreHandle::from_index(self.tag as usize).unwrap()
    }

    /// Gets the payload of this exception.
    pub fn payload(&self) -> NonNull<[RawValue]> {
        // SAFETY: UnsafeCell::get always returns a non-null pointer.
        unsafe { NonNull::new_unchecked(self.payload.get()) }
    }

    /// Gets the backtrace of this exception at the time it was constructed.
    pub fn backtrace(&self) -> &Backtrace {
        &self.backtrace
    }
}

// TODO: This is probably the place to do some reference-counting of exception objects.
/// Represents a reference to a VMExceptionObj.
#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VMExceptionRef(pub StoreHandle<VMExceptionObj>);

impl VMExceptionRef {
    /// Converts the [`VMExceptionRef`] into a `RawValue`.
    pub fn into_raw(self) -> RawValue {
        RawValue {
            exnref: self.to_u32_exnref(),
        }
    }

    /// Gets the raw u32 exnref value.
    pub fn to_u32_exnref(&self) -> u32 {
        self.0.internal_handle().index() as u32
    }

    /// Extracts a `VMExceptionRef` from a `RawValue`.
    ///
    /// # Safety
    /// `raw` must be a valid `VMExceptionRef` instance.
    pub unsafe fn from_raw(store_id: StoreId, raw: RawValue) -> Option<Self> {
        unsafe {
            InternalStoreHandle::from_index(raw.exnref as usize)
                .map(|handle| Self(StoreHandle::from_internal(store_id, handle)))
        }
    }
}
