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
}
