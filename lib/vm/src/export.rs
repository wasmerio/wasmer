// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

use crate::global::VMGlobal;
use crate::memory::VMMemory;
use crate::store::InternalStoreHandle;
use crate::table::VMTable;
use crate::vmcontext::VMFunctionKind;
use crate::{MaybeInstanceOwned, VMCallerCheckedAnyfunc};
use std::{
    any::Any,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
use wasmer_types::{FunctionType, TagKind};

/// The value of an export passed from one instance to another.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub enum VMExtern {
    /// A function export value.
    Function(InternalStoreHandle<VMFunction>),

    /// A table export value.
    Table(InternalStoreHandle<VMTable>),

    /// A memory export value.
    Memory(InternalStoreHandle<VMMemory>),

    /// A global export value.
    Global(InternalStoreHandle<VMGlobal>),

    /// A tag export value.
    Tag(InternalStoreHandle<VMTag>),
}

/// An immutable function-target allocation identity. It grants no call authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct VMFunctionId(u64);

static NEXT_FUNCTION_ID: AtomicU64 = AtomicU64::new(1);

impl VMFunctionId {
    /// Resolve only this exact target at its original Store handle.
    pub fn lookup<'a>(
        &self,
        handle: &crate::StoreHandle<VMFunction>,
        store: &'a crate::StoreObjects,
    ) -> Option<&'a VMFunction> {
        use crate::StoreObject;
        if handle.store_id() != store.id() {
            return None;
        }
        let target = VMFunction::list(store).get(handle.internal_handle().index() - 1)?;
        (target.id() == *self && target.is_from_store(store)).then_some(target)
    }
}

/// A function export value. Clones retain the same immutable call target.
#[derive(Clone)]
pub struct VMFunction {
    target: Rc<VMFunctionTarget>,
}

struct VMFunctionTarget {
    id: VMFunctionId,
    store: usize,
    store_id: wasmer_types::StoreId,
    anyfunc: MaybeInstanceOwned<VMCallerCheckedAnyfunc>,
    signature: FunctionType,
    kind: VMFunctionKind,
    host_data: Box<dyn Any>,
    // Last: context/payload destruction must precede final Instance release.
    _instance: Option<Rc<crate::instance::InstanceAllocation>>,
}

/// Allocation custody for one exact function target, without Store access.
#[derive(Clone)]
pub struct VMFunctionPin {
    target: Rc<VMFunctionTarget>,
}

impl std::fmt::Debug for VMFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunction")
            .field("id", &self.target.id)
            .field("signature", &self.target.signature)
            .field("kind", &self.target.kind)
            .finish_non_exhaustive()
    }
}
impl std::fmt::Debug for VMFunctionPin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VMFunctionPin")
            .field("id", &self.target.id)
            .finish_non_exhaustive()
    }
}

impl VMFunction {
    /// Create an immutable target from its already initialized VM metadata.
    ///
    /// # Safety
    /// The anyfunc's context and code pointers must match `host_data` or their
    /// independently retained instance allocation, signature, and calling kind.
    /// Owned host context pointers must point into the provided Box allocation;
    /// moving that Box here preserves the pointee's address. The caller must
    /// preserve instance-backed pointers for every subsequent use. `store` must
    /// be the actual stable Store allocation whose context the target uses.
    pub unsafe fn new(
        anyfunc: MaybeInstanceOwned<VMCallerCheckedAnyfunc>,
        signature: FunctionType,
        kind: VMFunctionKind,
        host_data: Box<dyn Any>,
        store: &crate::StoreObjects,
    ) -> Self {
        unsafe { Self::new_target(anyfunc, signature, kind, host_data, store, None) }
    }

    /// Create a local Wasm target whose metadata and vmctx belong to `instance`.
    ///
    /// # Safety
    /// The anyfunc must belong to the supplied live allocation and match the
    /// signature and Store. The retained allocation grants no Store access.
    pub(crate) unsafe fn new_local(
        anyfunc: std::ptr::NonNull<VMCallerCheckedAnyfunc>,
        signature: FunctionType,
        store: &crate::StoreObjects,
        instance: Rc<crate::instance::InstanceAllocation>,
    ) -> Self {
        unsafe {
            Self::new_target(
                MaybeInstanceOwned::Instance(anyfunc),
                signature,
                VMFunctionKind::Static,
                Box::new(()),
                store,
                Some(instance),
            )
        }
    }

    // Both constructors establish the raw descriptor's existing invariants.
    unsafe fn new_target(
        anyfunc: MaybeInstanceOwned<VMCallerCheckedAnyfunc>,
        signature: FunctionType,
        kind: VMFunctionKind,
        host_data: Box<dyn Any>,
        store: &crate::StoreObjects,
        instance: Option<Rc<crate::instance::InstanceAllocation>>,
    ) -> Self {
        let id = NEXT_FUNCTION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("function target identity exhausted");
        Self {
            target: Rc::new(VMFunctionTarget {
                id: VMFunctionId(id),
                store: store as *const crate::StoreObjects as usize,
                store_id: store.id(),
                anyfunc,
                signature,
                kind,
                host_data,
                _instance: instance,
            }),
        }
    }

    /// Identity of this immutable target, independent of its Store slot.
    pub fn id(&self) -> VMFunctionId {
        self.target.id
    }

    /// Whether this target belongs to this exact Store allocation and identity.
    pub fn is_from_store(&self, store: &crate::StoreObjects) -> bool {
        self.target.store_id == store.id()
            && self.target.store == store as *const crate::StoreObjects as usize
    }

    /// Pointer to the existing VM call metadata. Reading or changing its
    /// contents requires the usual VM/Store access and pointer safety contract.
    pub fn anyfunc(&self) -> std::ptr::NonNull<VMCallerCheckedAnyfunc> {
        self.target.anyfunc.as_ptr()
    }

    /// The immutable function signature.
    pub fn signature(&self) -> &FunctionType {
        &self.target.signature
    }

    /// The immutable function calling convention.
    pub fn kind(&self) -> VMFunctionKind {
        self.target.kind
    }

    /// Read-only host context metadata. The context cannot be safely retargeted.
    pub fn host_data(&self) -> &dyn Any {
        &*self.target.host_data
    }

    /// Retain the exact call metadata and host context through guest execution.
    pub fn pin(&self) -> VMFunctionPin {
        VMFunctionPin {
            target: self.target.clone(),
        }
    }

    /// Number of owners retaining this call-target allocation.
    #[doc(hidden)]
    pub fn owner_count(&self) -> usize {
        Rc::strong_count(&self.target)
    }
}

impl VMFunctionPin {
    pub(crate) fn id(&self) -> VMFunctionId {
        self.target.id
    }

    pub(crate) fn clone_function(&self) -> VMFunction {
        VMFunction {
            target: self.target.clone(),
        }
    }

    /// Pointer to the metadata retained by this pin. Local exported targets
    /// also retain their Instance allocation; raw externally supplied descriptors
    /// retain the constructor's independent lifetime requirements.
    pub fn anyfunc(&self) -> std::ptr::NonNull<VMCallerCheckedAnyfunc> {
        self.target.anyfunc.as_ptr()
    }
}

/// A tag export value.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VMTag {
    /// The kind of tag.
    // Note: currently it can only be exception.
    pub kind: TagKind,
    /// The tag type, used for compatibility checking.
    pub signature: FunctionType,
}

impl VMTag {
    /// Create a new [`VMTag`].
    pub fn new(kind: TagKind, signature: FunctionType) -> Self {
        Self { kind, signature }
    }
}
