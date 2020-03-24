//use crate::ast::InterfaceType;

/// Represents all the possible WIT instructions.
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Instruction {
    /// The `arg.get` instruction.
    ArgumentGet {
        /// The argument index.
        index: u32,
    },

    /// The `call-core` instruction.
    CallCore {
        /// The function index.
        function_index: usize,
    },

    /// The `memory-to-string` instruction.
    MemoryToString,

    /// The `string-to-memory` instruction.
    StringToMemory {
        /// The allocator function index.
        allocator_index: u32,
    },

    /// The `s8.from_i32` instruction.
    S8FromI32,

    /// The `s8.from_i64` instruction.
    S8FromI64,

    /// The `s16.from_i32` instruction.
    S16FromI32,

    /// The `s16.from_i64` instruction.
    S16FromI64,

    /// The `s32.from_i32` instruction.
    S32FromI32,

    /// The `s32.from_i64` instruction.
    S32FromI64,

    /// The `s64.from_i32` instruction.
    S64FromI32,

    /// The `s64.from_i64` instruction.
    S64FromI64,

    /// The `i32.from_s8` instruction.
    I32FromS8,

    /// The `i32.from_s16` instruction.
    I32FromS16,

    /// The `i32.from_s32` instruction.
    I32FromS32,

    /// The `i32.from_s64` instruction.
    I32FromS64,

    /// The `i64.from_s8` instruction.
    I64FromS8,

    /// The `i64.from_s16` instruction.
    I64FromS16,

    /// The `i64.from_s32` instruction.
    I64FromS32,

    /// The `i64.from_s64` instruction.
    I64FromS64,

    /// The `u8.from_i32` instruction.
    U8FromI32,

    /// The `u8.from_i64` instruction.
    U8FromI64,

    /// The `u16.from_i32` instruction.
    U16FromI32,

    /// The `u16.from_i64` instruction.
    U16FromI64,

    /// The `u32.from_i32` instruction.
    U32FromI32,

    /// The `u32.from_i64` instruction.
    U32FromI64,

    /// The `u64.from_i32` instruction.
    U64FromI32,

    /// The `u64.from_i64` instruction.
    U64FromI64,

    /// The `i32.from_u8` instruction.
    I32FromU8,

    /// The `i32.from_u16` instruction.
    I32FromU16,

    /// The `i32.from_u32` instruction.
    I32FromU32,

    /// The `i32.from_u64` instruction.
    I32FromU64,

    /// The `i64.from_u8` instruction.
    I64FromU8,

    /// The `i64.from_u16` instruction.
    I64FromU16,

    /// The `i64.from_u32` instruction.
    I64FromU32,

    /// The `i64.from_u64` instruction.
    I64FromU64,
}
