use crate::ast::InterfaceType;

/// Represents all the possible WIT instructions.
#[derive(PartialEq, Debug)]
pub enum Instruction<'input> {
    /// The `arg.get` instruction.
    ArgumentGet {
        /// The argument index.
        index: u64,
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

    /// The `as-wasm` instruction.
    AsWasm(InterfaceType),

    /// The `as-interface` instruction.
    AsInterface(InterfaceType),

    /// The `table-ref-add` instruction.
    TableRefAdd,

    /// The `table-ref-get` instruction.
    TableRefGet,

    /// The `call-method` instruction.
    CallMethod(u64),

    /// The `make-record` instruction.
    MakeRecord(InterfaceType),

    /// The `get-field` instruction.
    GetField(InterfaceType, u64),

    /// The `const` instruction.
    Const(InterfaceType, u64),

    /// The `fold-seq` instruction.
    FoldSeq(u64),

    /// The `add` instruction.
    Add(InterfaceType),

    /// The `mem-to-seq` instruction.
    MemToSeq(InterfaceType, &'input str),

    /// The `load` instruction.
    Load(InterfaceType, &'input str),

    /// The `seq.new` instruction.
    SeqNew(InterfaceType),

    /// The `list.push` instruction.
    ListPush,

    /// The `repeat-until` instruction.
    RepeatUntil(u64, u64),
}
