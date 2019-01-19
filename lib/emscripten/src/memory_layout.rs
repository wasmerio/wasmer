use crate::storage::align_memory;

// TODO: How is this calculated?
const TOTAL_STACK: u32 = 5_242_880;

// TODO: How is this calculated?
const DYNAMICTOP_PTR_DIFF: u32 = 1088;

// TODO: make this variable
pub const STATIC_BUMP: u32 = 215_536;

pub fn stacktop(static_bump: u32) -> u32 {
    align_memory(dynamictop_ptr(static_bump) + 4)
}

pub fn stack_max(static_bump: u32) -> u32 {
    stacktop(static_bump) + TOTAL_STACK
}

pub fn dynamic_base(static_bump: u32) -> u32 {
    align_memory(stack_max(static_bump))
}

pub fn dynamictop_ptr(static_bump: u32) -> u32 {
    static_bump + DYNAMICTOP_PTR_DIFF
}
