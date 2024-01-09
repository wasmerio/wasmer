use wasmer_derive::ValueType;

#[derive(Debug, Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_asyncify_t<O>
where
    O: wasmer_types::ValueType,
{
    pub start: O,
    pub end: O,
}
