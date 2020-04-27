// Placeholder.

use std::mem;

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
    pub fn offset_count() -> usize { 0 }
    pub fn offset_base() -> usize { 0 }
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