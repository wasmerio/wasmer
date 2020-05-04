// Placeholder.

use std::{
    cell::UnsafeCell,
    ffi::c_void,
    mem,
    ptr::{self, NonNull},
    sync::atomic::{AtomicUsize, Ordering},
    sync::Once,
};

pub struct Ctx {}

#[doc(hidden)]
impl Ctx {
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_memories() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_tables() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_globals() -> u8 {
        2 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_memories() -> u8 {
        3 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_tables() -> u8 {
        4 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_globals() -> u8 {
        5 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_imported_funcs() -> u8 {
        6 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_signatures() -> u8 {
        7 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_intrinsics() -> u8 {
        8 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_stack_lower_bound() -> u8 {
        9 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_memory_base() -> u8 {
        10 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_memory_bound() -> u8 {
        11 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_internals() -> u8 {
        12 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_interrupt_signal_mem() -> u8 {
        13 * (mem::size_of::<usize>() as u8)
    }

    pub const fn offset_local_functions() -> u8 {
        14 * (mem::size_of::<usize>() as u8)
    }
}

pub struct Anyfunc {}

impl Anyfunc {
    /// Offset to the `func` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `vmctx` field..
    pub const fn offset_vmctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `sig_id` field.
    pub const fn offset_sig_id() -> u8 {
        2 * (mem::size_of::<usize>() as u8)
    }

    /// The size of `Anyfunc`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

pub struct LocalTable {}

impl LocalTable {
    pub fn offset_count() -> usize {
        0
    }
    pub fn offset_base() -> usize {
        0
    }
}

pub struct Intrinsics {}

impl Intrinsics {
    /// Offset of the `memory_grow` field.
    #[allow(clippy::erasing_op)]
    pub const fn offset_memory_grow() -> u8 {
        (0 * mem::size_of::<usize>()) as u8
    }
    /// Offset of the `memory_size` field.
    pub const fn offset_memory_size() -> u8 {
        (1 * mem::size_of::<usize>()) as u8
    }
}

/// An imported function is a function pointer associated to a
/// function context.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct ImportedFunc {
    /// Pointer to the function itself.
    pub(crate) func: *const Func,

    /// Mutable non-null pointer to [`FuncCtx`].
    pub(crate) func_ctx: NonNull<FuncCtx>,
}

// Manually implemented because ImportedFunc contains raw pointers
// directly; `Func` is marked Send (But `Ctx` actually isn't! (TODO:
// review this, shouldn't `Ctx` be Send?))
unsafe impl Send for ImportedFunc {}

impl ImportedFunc {
    /// Offset to the `func` field.
    #[allow(clippy::erasing_op)] // TODO
    pub const fn offset_func() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `func_ctx` field.
    pub const fn offset_func_ctx() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Size of an `ImportedFunc`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}

/// Represents a function pointer. It is mostly used in the
/// `typed_func` module within the `wrap` functions, to wrap imported
/// functions.
#[repr(transparent)]
pub struct Func(*mut c_void);

/// Represents a function environment pointer, like a captured
/// environment of a closure. It is mostly used in the `typed_func`
/// module within the `wrap` functions, to wrap imported functions.
#[repr(transparent)]
pub struct FuncEnv(*mut c_void);

/// Represents a function context. It is used by imported functions
/// only.
#[derive(Debug)]
#[repr(C)]
pub struct FuncCtx {
    /// The `Ctx` pointer.
    pub(crate) vmctx: NonNull<Ctx>,

    /// A pointer to the function environment. It is used by imported
    /// functions only to store the pointer to the real host function,
    /// whether it is a regular function, or a closure with or without
    /// a captured environment.
    pub(crate) func_env: Option<NonNull<FuncEnv>>,
}

impl FuncCtx {
    /// Offset to the `vmctx` field.
    #[allow(clippy::erasing_op)]
    pub const fn offset_vmctx() -> u8 {
        0 * (mem::size_of::<usize>() as u8)
    }

    /// Offset to the `func_env` field.
    pub const fn offset_func_env() -> u8 {
        1 * (mem::size_of::<usize>() as u8)
    }

    /// Size of a `FuncCtx`.
    pub const fn size() -> u8 {
        mem::size_of::<Self>() as u8
    }
}
