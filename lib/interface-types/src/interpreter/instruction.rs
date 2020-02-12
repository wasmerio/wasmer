use crate::ast::InterfaceType;

/// Represents all the possible WIT instructions.
#[derive(PartialEq, Debug)]
pub enum Instruction<'input> {
    /// `arg.get`
    ArgumentGet { index: u64 },

    /// `call`
    Call { function_index: usize },

    /// `call-export`
    CallExport { export_name: &'input str },

    /// `read-utf8`
    ReadUtf8,

    /// `write-utf8`
    WriteUtf8 { allocator_name: &'input str },

    /// `as-wasm`
    AsWasm(InterfaceType),

    /// `as-interface`
    AsInterface(InterfaceType),

    /// `table-ref-add`
    TableRefAdd,

    /// `table-ref-get`
    TableRefGet,

    /// `call-method`
    CallMethod(u64),

    /// `make-record`
    MakeRecord(InterfaceType),

    /// `get-field`
    GetField(InterfaceType, u64),

    /// `const`
    Const(InterfaceType, u64),

    /// `fold-seq`
    FoldSeq(u64),

    /// `add`
    Add(InterfaceType),

    /// `mem-to-seq`
    MemToSeq(InterfaceType, &'input str),

    /// `load`
    Load(InterfaceType, &'input str),

    /// `seq.new`
    SeqNew(InterfaceType),

    /// `list.push`
    ListPush,

    /// `repeat-until`
    RepeatUntil(u64, u64),
}
