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
    collections::HashSet,
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

    /// Capture the current allocation of a valid table reference before any
    /// anyfunc dereference. Unknown owners are refused. Pointer equality cannot
    /// recover historical provenance after arbitrary raw-reference reuse.
    pub fn resolve_table_reference(
        store: &crate::StoreObjects,
        reference: crate::VMFuncRef,
    ) -> Option<Self> {
        use crate::StoreObject;
        let mut targets: Vec<_> = Self::list(store).iter().map(Self::pin).collect();
        let mut instances: Vec<_> = crate::VMInstance::list(store)
            .iter()
            .map(crate::VMInstance::allocation_pin)
            .collect();
        let mut seen_targets = HashSet::new();
        let mut seen_instances = HashSet::new();
        while !targets.is_empty() || !instances.is_empty() {
            if let Some(pin) = targets.pop() {
                if !seen_targets.insert(pin.id()) || !pin.clone_function().is_from_store(store) {
                    continue;
                }
                // Raw Unit descriptors created by the trusted unsafe value
                // path do not, by themselves, establish allocation custody.
                let owns_metadata = matches!(&pin.target.anyfunc, MaybeInstanceOwned::Host(_))
                    || pin.target._instance.is_some()
                    || pin.target.host_data.is::<VMFunctionPin>();
                if owns_metadata && pin.anyfunc() == reference.0 {
                    if pin.target.kind == VMFunctionKind::Static {
                        return Some(pin.clone_function());
                    }
                    let signature = pin.target.signature.clone();
                    // SAFETY: this exact pin owns the metadata and host context.
                    // Keep a non-Unit payload: converted host targets do not use
                    // the Wasm-only memory-zero calling-convention parameter.
                    return Some(unsafe {
                        Self::new(
                            MaybeInstanceOwned::Instance(reference.0),
                            signature,
                            VMFunctionKind::Static,
                            Box::new(pin),
                            store,
                        )
                    });
                }
                if let Some(instance) = &pin.target._instance {
                    instances.push(instance.clone());
                }
            } else if let Some(owner) = instances.pop() {
                if !seen_instances.insert(Rc::as_ptr(&owner)) {
                    continue;
                }
                if let Some(target) =
                    crate::instance::InstanceAllocation::local_ref_target(&owner, store, reference)
                {
                    return Some(target);
                }
                owner.add_import_targets(store, &mut targets);
            }
        }
        None
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

    /// Number of owners of a local target's Instance allocation, if present.
    #[doc(hidden)]
    pub fn instance_owner_count(&self) -> Option<usize> {
        self.target._instance.as_ref().map(Rc::strong_count)
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
