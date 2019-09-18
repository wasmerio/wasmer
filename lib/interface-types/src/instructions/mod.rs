use crate::ast::InterfaceType;

pub mod interpreter;
mod stack;

#[derive(PartialEq, Debug)]
pub enum Instruction<'input> {
    ArgumentGet(u64),
    Call(u64),
    CallExport(&'input str),
    ReadUtf8,
    WriteUtf8(&'input str),
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
