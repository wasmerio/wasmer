/// Gets a 16-bit memory-aligned offset
pub fn align_memory(ptr: u32) -> u32 {
    (ptr + 15) & !15
}

// EMSCRIPTEN MEMORY LAYOUT
// ++++++++++++++++++++++++++ <- static_base            [1024]
// |         STATIC            |
// |           ↓              <- dynamictop             [216_624]
// ++++++++++++++++++++++++++ <- static_top, stack_base [216_640]
// |         STACK             |
// |           ↓               |
// |           :               | - total_stack
// |           ↑               |
// |          HEAP             |
// ++++++++++++++++++++++++++ <- dynamic_base, stack_top [5_242_880]

/// Where the globals start.
const GLOBAL_BASE: u32 = 1024;

/// The entire stack size.
const TOTAL_STACK: u32 = 5_242_880;

/// The space between the dynamic heap and the stack.
const DYNAMICTOP_PTR_DIFF: u32 = 1088;

// Space below the stack, where emscripten globals are stored.
pub const STATIC_BUMP: u32 = 215_536;

/// Gets the top of the stack.
pub fn stacktop(static_bump: u32) -> u32 {
    align_memory(dynamictop_ptr(static_bump) + 4)
}

/// Gets entire stack size.
pub fn stack_max(static_bump: u32) -> u32 {
    stacktop(static_bump) + TOTAL_STACK
}

/// Gets the base of the dynamic memory.
pub fn dynamic_base(static_bump: u32) -> u32 {
    align_memory(stack_max(static_bump))
}

/// Gets the top of the dynamic memory.
pub fn dynamictop_ptr(static_bump: u32) -> u32 {
    static_bump + DYNAMICTOP_PTR_DIFF
}

/// Gets the base of the entire memory.
pub fn static_base() -> u32 {
    GLOBAL_BASE
}

/// Gets the base of the entire memory.
pub fn memory_base() -> u32 {
    static_base()
}

// TODO: Fix implemntation.
// Gets the top of global memory.
pub fn statictop(static_bump: u32) -> u32 {
    static_bump + GLOBAL_BASE
}

// pub fn static_alloc(size: u32, static_top: &mut u32, memory: &LinearMemory) -> u32 {
//     let old_static_top = *static_top;
//     let total_memory = memory.maximum_size() * LinearMemory::PAGE_SIZE;
//     // NOTE: The `4294967280` is a u32 conversion of -16 as gotten from emscripten.
//     *static_top = (*static_top + size + 15) & 4294967280;
//     assert!(
//         *static_top < total_memory,
//         "not enough memory for static allocation - increase total_memory!"
//     );
//     old_static_top
// }
