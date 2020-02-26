//use crate::ast::InterfaceType;

/// Represents all the possible WIT instructions.
#[derive(PartialEq, Debug)]
pub enum Instruction<'input> {
    /// The `arg.get` instruction.
    ArgumentGet {
        /// The argument index.
        index: u32,
    },

    /// The `call` instruction.
    Call {
        /// The function index.
        function_index: usize,
    },

    /// The `call-export` instruction.
    CallExport {
        /// The exported function name.
        export_name: &'input str,
    },

    /// The `read-utf8` instruction.
    ReadUtf8,

    /// The `write-utf8` instruction.
    WriteUtf8 {
        /// The allocator function name.
        allocator_name: &'input str,
    },

    /// The `i32-to-s8,` instruction.
    I32ToS8,

    /// The `i32-to-s8x,` instruction.
    I32ToS8X,

    /// The `i32-to-u8,` instruction.
    I32ToU8,

    /// The `i32-to-s16,` instruction.
    I32ToS16,

    /// The `i32-to-s16x,` instruction.
    I32ToS16X,

    /// The `i32-to-u16,` instruction.
    I32ToU16,

    /// The `i32-to-s32,` instruction.
    I32ToS32,

    /// The `i32-to-u32,` instruction.
    I32ToU32,

    /// The `i32-to-s64,` instruction.
    I32ToS64,

    /// The `i32-to-u64,` instruction.
    I32ToU64,

    /// The `i64-to-s8,` instruction.
    I64ToS8,

    /// The `i64-to-s8x,` instruction.
    I64ToS8X,

    /// The `i64-to-u8,` instruction.
    I64ToU8,

    /// The `i64-to-s16,` instruction.
    I64ToS16,

    /// The `i64-to-s16x,` instruction.
    I64ToS16X,

    /// The `i64-to-u16,` instruction.
    I64ToU16,

    /// The `i64-to-s32,` instruction.
    I64ToS32,

    /// The `i64-to-s32x,` instruction.
    I64ToS32X,

    /// The `i64-to-u32,` instruction.
    I64ToU32,

    /// The `i64-to-s64,` instruction.
    I64ToS64,

    /// The `i64-to-u64,` instruction.
    I64ToU64,

    /// The `s8-to-i32,` instruction.
    S8ToI32,

    /// The `u8-to-i32,` instruction.
    U8ToI32,

    /// The `s16-to-i32,` instruction.
    S16ToI32,

    /// The `u16-to-i32,` instruction.
    U16ToI32,

    /// The `s32-to-i32,` instruction.
    S32ToI32,

    /// The `u32-to-i32,` instruction.
    U32ToI32,

    /// The `s64-to-i32,` instruction.
    S64ToI32,

    /// The `s64-to-i32x,` instruction.
    S64ToI32X,

    /// The `u64-to-i32,` instruction.
    U64ToI32,

    /// The `u64-to-i32x,` instruction.
    U64ToI32X,

    /// The `s8-to-i64,` instruction.
    S8ToI64,

    /// The `u8-to-i64,` instruction.
    U8ToI64,

    /// The `s16-to-i64,` instruction.
    S16ToI64,

    /// The `u16-to-i64,` instruction.
    U16ToI64,

    /// The `s32-to-i64,` instruction.
    S32ToI64,

    /// The `u32-to-i64,` instruction.
    U32ToI64,

    /// The `s64-to-i64,` instruction.
    S64ToI64,

    /// The `u64-to-i64,` instruction.
    U64ToI64,
}
