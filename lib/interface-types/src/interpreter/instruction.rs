use crate::ast::InterfaceType;

#[derive(PartialEq, Debug)]
pub enum Instruction<'input> {
    ArgumentGet { index: u64 },
    Call { function_index: usize },
    CallExport { export_name: &'input str },
    ReadUtf8,
    WriteUtf8 { allocator_name: &'input str },
    AsWasm(InterfaceType),
    AsInterface(InterfaceType),
    TableRefAdd,
    TableRefGet,
    CallMethod(u64),
    MakeRecord(InterfaceType),
    GetField(InterfaceType, u64),
    Const(InterfaceType, u64),
    FoldSeq(u64),
    Add(InterfaceType),
    MemToSeq(InterfaceType, &'input str),
    Load(InterfaceType, &'input str),
    SeqNew(InterfaceType),
    ListPush,
    RepeatWhile(u64, u64),
}
