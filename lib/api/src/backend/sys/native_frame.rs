//! Opaque native-engine custody whose shape does not depend on a consumer's features.

use crate::RuntimeError;
use std::{marker::PhantomData, rc::Rc};

#[cfg(feature = "experimental-async")]
use super::async_runtime::{
    MemoryGrowCallbackGuard, NativeCleanupGuard, NativeFrameGuard, WeakNativeInvocation,
    defer_cleanup, resume_guard_cleanup_panics,
};

/// Usable numeric bounds of the actual native API coroutine stack.
///
/// These values grant no pointer/data access and retain no coroutine or Store.
/// A consumer must still check its own current stack address against the range.
/// None means no active API coroutine allocation contains this physical call;
/// native consumers then use their supported OS-thread bounds.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeEngineStackBounds {
    /// Lowest guaranteed usable address, excluding guard pages.
    pub usable_low: usize,
    /// Exclusive high address of this downward-growing stack allocation.
    pub base: usize,
}
impl NativeEngineStackBounds {
    /// Copy the used top of a live paused stack. `None` selects the original
    /// OS caller; `Some` must describe the exact usable coroutine allocation.
    /// This supplies numeric evidence only; external frame custody must keep
    /// every selected allocation alive and blocked until scanning completes.
    pub fn paused_top(bounds: Option<Self>) -> Option<usize> {
        #[cfg(feature = "experimental-async")]
        {
            super::async_runtime::native_paused_stack_top(bounds.map(|bounds| {
                super::async_runtime::NativeStackBounds {
                    low: bounds.usable_low,
                    base: bounds.base,
                }
            }))
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            let _ = bounds;
            None
        }
    }

    /// Find the innermost active allocation containing the physical stack.
    /// This remains available while logical/root metadata is detached.
    pub fn current() -> Option<Self> {
        #[cfg(feature = "experimental-async")]
        {
            super::async_runtime::current_native_stack_bounds().map(|bounds| Self {
                usable_low: bounds.low,
                base: bounds.base,
            })
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            None
        }
    }
}

/// Weak native invocation identity, without Store, coroutine or data access.
#[doc(hidden)]
#[derive(Clone)]
pub struct NativeEngineInvocation {
    #[cfg(feature = "experimental-async")]
    root: WeakNativeInvocation,
    _local: PhantomData<Rc<()>>,
}

impl NativeEngineInvocation {
    /// Capture only the physically executing native coroutine, if present.
    pub fn current() -> Option<Self> {
        #[cfg(feature = "experimental-async")]
        {
            WeakNativeInvocation::current().map(|root| Self {
                root,
                _local: PhantomData,
            })
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            None
        }
    }

    /// Whether this token names the exact physically executing coroutine.
    pub fn is_current(&self) -> bool {
        #[cfg(feature = "experimental-async")]
        {
            self.root.is_current()
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            false
        }
    }

    fn retirement_error(&self) -> Option<RuntimeError> {
        #[cfg(feature = "experimental-async")]
        {
            self.root.retirement_error()
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            None
        }
    }
}

/// Exact execution metadata for selecting an import's bookkeeping row.
///
/// This is separate from physical frame ancestry. It includes the existing
/// logical host-future boundary when no native coroutine is executing, and
/// matches an unowned entry only while the current identity is also unowned.
/// No Store, coroutine owner or data access is retained.
#[doc(hidden)]
#[derive(Clone)]
pub struct NativeEngineEntryIdentity {
    #[cfg(feature = "experimental-async")]
    identity: super::async_runtime::NativeEngineEntry,
    _local: PhantomData<Rc<()>>,
}

impl NativeEngineEntryIdentity {
    /// Capture the common physical-authoritative/logical-fallback identity.
    pub fn capture() -> Self {
        Self {
            #[cfg(feature = "experimental-async")]
            identity: super::async_runtime::NativeEngineEntry::capture(),
            _local: PhantomData,
        }
    }

    /// Compare exact optional identity, including an unowned None entry.
    pub fn is_current(&self) -> bool {
        #[cfg(feature = "experimental-async")]
        {
            self.identity.is_current()
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            true
        }
    }
}

/// Forbids native suspension during a synchronous host-only engine callback.
///
/// This supplies no Store access, allocation pin or ancestry. Keep it only for
/// the synchronous callback body; ordinary managed guest callbacks use frame
/// admission instead. The callback must not retain or transfer this guard.
#[doc(hidden)]
pub struct NativeSynchronousCallback {
    #[cfg(feature = "experimental-async")]
    guard: Option<MemoryGrowCallbackGuard>,
    _local: PhantomData<Rc<()>>,
}

impl NativeSynchronousCallback {
    /// Enter without consulting a consumer crate's optional async feature.
    pub fn enter() -> Self {
        Self {
            #[cfg(feature = "experimental-async")]
            guard: Some(MemoryGrowCallbackGuard::enter()),
            _local: PhantomData,
        }
    }

    /// Cancellation of the exact physical control captured at entry, or its
    /// logical host-body fallback when no physical coroutine was executing.
    /// This query performs no failure transport. The caller preserves an
    /// original body error/panic and an already-active Rust unwind first.
    pub fn retirement_error(&self) -> Option<RuntimeError> {
        #[cfg(feature = "experimental-async")]
        {
            self.guard
                .as_ref()
                .and_then(|guard| guard.retirement_error())
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            None
        }
    }
}

impl Drop for NativeSynchronousCallback {
    fn drop(&mut self) {
        #[cfg(feature = "experimental-async")]
        {
            let already_unwinding = std::thread::panicking();
            let mut failures = Vec::new();
            defer_cleanup(&mut failures, || drop(self.guard.take()));
            resume_guard_cleanup_panics(failures, already_unwinding);
        }
    }
}

/// Custody for an actual native engine frame, independent of consumer cfg.
///
/// Engine scopes and physical headers must exit before this guard drops. The
/// final blocker release can drive an ancestor's retirement immediately.
#[doc(hidden)]
pub struct NativeEngineFrame {
    invocation: Option<NativeEngineInvocation>,
    #[cfg(feature = "experimental-async")]
    synchronous: Option<NativeSynchronousCallback>,
    #[cfg(feature = "experimental-async")]
    managed: Option<NativeFrameGuard>,
    #[cfg(feature = "experimental-async")]
    cleanup: Option<NativeCleanupGuard>,
    _local: PhantomData<Rc<()>>,
}

impl NativeEngineFrame {
    /// Admit ordinary managed engine work.
    ///
    /// # Safety
    /// `previous` identifies the immediately preceding live physical engine
    /// header. Keep its owner alive throughout this constructor, and leave
    /// nested engine scopes/headers in LIFO order before dropping the result.
    /// Without a current coroutine, synchronous NoYield prevents independently
    /// parked descendants from outliving this otherwise unowned engine frame.
    pub unsafe fn enter_managed(
        previous: Option<&NativeEngineInvocation>,
    ) -> Result<Self, RuntimeError> {
        let current = NativeEngineInvocation::current();
        #[cfg(feature = "experimental-async")]
        let synchronous = current.is_none().then(NativeSynchronousCallback::enter);
        if let Some(error) = current.as_ref().and_then(|root| root.retirement_error()) {
            return Err(error);
        }
        #[cfg(feature = "experimental-async")]
        if let Some(error) = synchronous
            .as_ref()
            .and_then(|guard| guard.retirement_error())
        {
            return Err(error);
        }
        #[cfg(feature = "experimental-async")]
        let managed = match previous {
            // SAFETY: the caller supplies the actual preceding physical frame.
            Some(previous) => Some(unsafe { previous.root.enter_descendant_frame()? }),
            None => None,
        };
        Ok(Self {
            invocation: current.or_else(|| previous.cloned()),
            #[cfg(feature = "experimental-async")]
            synchronous,
            #[cfg(feature = "experimental-async")]
            managed,
            #[cfg(feature = "experimental-async")]
            cleanup: None,
            _local: PhantomData,
        })
    }

    /// Admit inert host disposal without adding cancellation ancestry.
    ///
    /// # Safety
    /// `previous` identifies the actual preceding physical engine frame. This
    /// guard inherits NativeCleanupGuard's synchronous host-only contract:
    /// guest finalizers require managed admission, and the cleanup isolate and
    /// header must have exited before Drop. No independent descendant may park.
    pub unsafe fn enter_cleanup(
        previous: Option<&NativeEngineInvocation>,
    ) -> Result<Self, RuntimeError> {
        #[cfg(feature = "experimental-async")]
        let cleanup =
            unsafe { NativeCleanupGuard::enter(previous.map(|previous| &previous.root))? };
        #[cfg(not(feature = "experimental-async"))]
        let _ = previous;
        Ok(Self {
            // The preceding root remains privately blocked, but cleanup is not
            // a managed parent of otherwise independent synchronous work.
            invocation: None,
            #[cfg(feature = "experimental-async")]
            synchronous: None,
            #[cfg(feature = "experimental-async")]
            managed: None,
            #[cfg(feature = "experimental-async")]
            cleanup: Some(cleanup),
            _local: PhantomData,
        })
    }

    /// Weak managed ancestry to cache in the corresponding physical header.
    /// Inert cleanup always exposes None while retaining its private blocker.
    pub fn invocation(&self) -> Option<NativeEngineInvocation> {
        self.invocation.clone()
    }

    /// Managed failure for this entry's captured controls, never a later root.
    /// Inert cleanup contributes no cancellation error to its physical caller.
    pub fn retirement_error(&self) -> Option<RuntimeError> {
        #[cfg(feature = "experimental-async")]
        {
            if self.cleanup.is_some() {
                return None;
            }
            self.synchronous
                .as_ref()
                .and_then(|guard| guard.retirement_error())
                .or_else(|| {
                    self.invocation
                        .as_ref()
                        .and_then(|root| root.retirement_error())
                })
                .or_else(|| {
                    self.managed
                        .as_ref()
                        .and_then(|guard| guard.retirement_error())
                })
        }
        #[cfg(not(feature = "experimental-async"))]
        {
            None
        }
    }
}

impl Drop for NativeEngineFrame {
    fn drop(&mut self) {
        #[cfg(feature = "experimental-async")]
        {
            let already_unwinding = std::thread::panicking();
            let mut failures = Vec::new();
            defer_cleanup(&mut failures, || drop(self.synchronous.take()));
            defer_cleanup(&mut failures, || drop(self.managed.take()));
            defer_cleanup(&mut failures, || drop(self.cleanup.take()));
            defer_cleanup(&mut failures, || drop(self.invocation.take()));
            resume_guard_cleanup_panics(failures, already_unwinding);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_frame_facade_is_available_without_a_coroutine() {
        let identity = NativeEngineEntryIdentity::capture();
        assert!(identity.is_current());
        assert!(NativeEngineInvocation::current().is_none());
        assert!(NativeEngineStackBounds::paused_top(None).is_none());
        let frame = unsafe { NativeEngineFrame::enter_managed(None) }.unwrap();
        let cleanup = unsafe { NativeEngineFrame::enter_cleanup(None) }.unwrap();
        let synchronous = NativeSynchronousCallback::enter();
        let no_invocation = frame.invocation().is_none() && cleanup.invocation().is_none();
        let no_error = frame.retirement_error().is_none()
            && cleanup.retirement_error().is_none()
            && synchronous.retirement_error().is_none();
        drop(synchronous);
        drop(cleanup);
        drop(frame);
        assert!(no_invocation && no_error && identity.is_current());
    }
}
