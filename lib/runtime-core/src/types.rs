use crate::{memory::MemoryType, module::ModuleInfo, structures::TypedIndex, units::Pages};
use std::{borrow::Cow, mem};

/// Represents a WebAssembly type.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    /// The `i32` type.
    I32,
    /// The `i64` type.
    I64,
    /// The `f32` type.
    F32,
    /// The `f64` type.
    F64,
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// Represents a WebAssembly value.
///
/// As the number of types in WebAssembly expand,
/// this structure will expand as well.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Value {
    /// The `i32` type.
    I32(i32),
    /// The `i64` type.
    I64(i64),
    /// The `f32` type.
    F32(f32),
    /// The `f64` type.
    F64(f64),
}

impl Value {
    pub fn ty(&self) -> Type {
        match self {
            Value::I32(_) => Type::I32,
            Value::I64(_) => Type::I64,
            Value::F32(_) => Type::F32,
            Value::F64(_) => Type::F64,
        }
    }
}

impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Value::I32(i)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::I64(i)
    }
}

impl From<f32> for Value {
    fn from(f: f32) -> Self {
        Value::F32(f)
    }
}

impl From<f64> for Value {
    fn from(f: f64) -> Self {
        Value::F64(f)
    }
}

pub unsafe trait WasmExternType: Copy + Clone
where
    Self: Sized,
{
    const TYPE: Type;
}

unsafe impl WasmExternType for i8 {
    const TYPE: Type = Type::I32;
}
unsafe impl WasmExternType for u8 {
    const TYPE: Type = Type::I32;
}
unsafe impl WasmExternType for i16 {
    const TYPE: Type = Type::I32;
}
unsafe impl WasmExternType for u16 {
    const TYPE: Type = Type::I32;
}
unsafe impl WasmExternType for i32 {
    const TYPE: Type = Type::I32;
}
unsafe impl WasmExternType for u32 {
    const TYPE: Type = Type::I32;
}
unsafe impl WasmExternType for i64 {
    const TYPE: Type = Type::I64;
}
unsafe impl WasmExternType for u64 {
    const TYPE: Type = Type::I64;
}
unsafe impl WasmExternType for f32 {
    const TYPE: Type = Type::F32;
}
unsafe impl WasmExternType for f64 {
    const TYPE: Type = Type::F64;
}

// pub trait IntegerAtomic
// where
//     Self: Sized
// {
//     type Primitive;

//     fn add(&self, other: Self::Primitive) -> Self::Primitive;
//     fn sub(&self, other: Self::Primitive) -> Self::Primitive;
//     fn and(&self, other: Self::Primitive) -> Self::Primitive;
//     fn or(&self, other: Self::Primitive) -> Self::Primitive;
//     fn xor(&self, other: Self::Primitive) -> Self::Primitive;
//     fn load(&self) -> Self::Primitive;
//     fn store(&self, other: Self::Primitive) -> Self::Primitive;
//     fn compare_exchange(&self, expected: Self::Primitive, new: Self::Primitive) -> Self::Primitive;
//     fn swap(&self, other: Self::Primitive) -> Self::Primitive;
// }

pub enum ValueError {
    BufferTooSmall,
}

pub trait ValueType: Copy
where
    Self: Sized,
{
    fn into_le(self, buffer: &mut [u8]);
    fn from_le(buffer: &[u8]) -> Result<Self, ValueError>;
}

macro_rules! convert_value_impl {
    ($t:ty) => {
        impl ValueType for $t {
            fn into_le(self, buffer: &mut [u8]) {
                buffer[..mem::size_of::<Self>()].copy_from_slice(&self.to_le_bytes());
            }
            fn from_le(buffer: &[u8]) -> Result<Self, ValueError> {
                if buffer.len() >= mem::size_of::<Self>() {
                    let mut array = [0u8; mem::size_of::<Self>()];
                    array.copy_from_slice(&buffer[..mem::size_of::<Self>()]);
                    Ok(Self::from_le_bytes(array))
                } else {
                    Err(ValueError::BufferTooSmall)
                }
            }
        }
    };
    ( $($t:ty),* ) => {
        $(
            convert_value_impl!($t);
        )*
    };
}

convert_value_impl!(u8, i8, u16, i16, u32, i32, u64, i64);

impl ValueType for f32 {
    fn into_le(self, buffer: &mut [u8]) {
        self.to_bits().into_le(buffer);
    }
    fn from_le(buffer: &[u8]) -> Result<Self, ValueError> {
        Ok(f32::from_bits(<u32 as ValueType>::from_le(buffer)?))
    }
}

impl ValueType for f64 {
    fn into_le(self, buffer: &mut [u8]) {
        self.to_bits().into_le(buffer);
    }
    fn from_le(buffer: &[u8]) -> Result<Self, ValueError> {
        Ok(f64::from_bits(<u64 as ValueType>::from_le(buffer)?))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Any wasm function.
    Anyfunc,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct TableDescriptor {
    /// Type of data stored in this table.
    pub element: ElementType,
    /// The minimum number of elements that must be stored in this table.
    pub minimum: u32,
    /// The maximum number of elements in this table.
    pub maximum: Option<u32>,
}

impl TableDescriptor {
    pub(crate) fn fits_in_imported(&self, imported: TableDescriptor) -> bool {
        // TODO: We should define implementation limits.
        let imported_max = imported.maximum.unwrap_or(u32::max_value());
        let self_max = self.maximum.unwrap_or(u32::max_value());
        self.element == imported.element
            && imported_max <= self_max
            && self.minimum <= imported.minimum
    }
}

/// A const value initializer.
/// Over time, this will be able to represent more and more
/// complex expressions.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Initializer {
    /// Corresponds to a `const.*` instruction.
    Const(Value),
    /// Corresponds to a `get_global` instruction.
    GetGlobal(ImportedGlobalIndex),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalDescriptor {
    pub mutable: bool,
    pub ty: Type,
}

/// A wasm global.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GlobalInit {
    pub desc: GlobalDescriptor,
    pub init: Initializer,
}

/// A wasm memory.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryDescriptor {
    /// The minimum number of allowed pages.
    pub minimum: Pages,
    /// The maximum number of allowed pages.
    pub maximum: Option<Pages>,
    /// This memory can be shared between wasm threads.
    pub shared: bool,
}

impl MemoryDescriptor {
    pub fn memory_type(self) -> MemoryType {
        match (self.maximum.is_some(), self.shared) {
            (true, true) => MemoryType::SharedStatic,
            (true, false) => MemoryType::Static,
            (false, false) => MemoryType::Dynamic,
            (false, true) => panic!("shared memory without a max is not allowed"),
        }
    }

    pub(crate) fn fits_in_imported(&self, imported: MemoryDescriptor) -> bool {
        let imported_max = imported.maximum.unwrap_or(Pages(65_536));
        let self_max = self.maximum.unwrap_or(Pages(65_536));

        self.shared == imported.shared
            && imported_max <= self_max
            && self.minimum <= imported.minimum
    }
}

/// The signature of a function that is either implemented
/// in a wasm module or exposed to wasm by the host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FuncSig {
    params: Cow<'static, [Type]>,
    returns: Cow<'static, [Type]>,
}

impl FuncSig {
    pub fn new<Params, Returns>(params: Params, returns: Returns) -> Self
    where
        Params: Into<Cow<'static, [Type]>>,
        Returns: Into<Cow<'static, [Type]>>,
    {
        Self {
            params: params.into(),
            returns: returns.into(),
        }
    }

    pub fn params(&self) -> &[Type] {
        &self.params
    }

    pub fn returns(&self) -> &[Type] {
        &self.returns
    }

    pub fn check_param_value_types(&self, params: &[Value]) -> bool {
        self.params.len() == params.len()
            && self
                .params
                .iter()
                .zip(params.iter().map(|val| val.ty()))
                .all(|(t0, ref t1)| t0 == t1)
    }
}

impl std::fmt::Display for FuncSig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let params = self
            .params
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let returns = self
            .returns
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "[{}] -> [{}]", params, returns)
    }
}

pub trait LocalImport {
    type Local: TypedIndex;
    type Import: TypedIndex;
}

#[rustfmt::skip]
macro_rules! define_map_index {
    ($ty:ident) => {
        #[derive(Serialize, Deserialize)]
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
        pub struct $ty (u32);
        impl TypedIndex for $ty {
            #[doc(hidden)]
            fn new(index: usize) -> Self {
                $ty (index as _)
            }

            #[doc(hidden)]
            fn index(&self) -> usize {
                self.0 as usize
            }
        }
    };
    ($($normal_ty:ident,)* | local: $($local_ty:ident,)* | imported: $($imported_ty:ident,)*) => {
        $(
            define_map_index!($normal_ty);
            define_map_index!($local_ty);
            define_map_index!($imported_ty);

            impl LocalImport for $normal_ty {
                type Local = $local_ty;
                type Import = $imported_ty;
            }
        )*
    };
}

#[rustfmt::skip]
define_map_index![
    FuncIndex, MemoryIndex, TableIndex, GlobalIndex,
    | local: LocalFuncIndex, LocalMemoryIndex, LocalTableIndex, LocalGlobalIndex,
    | imported: ImportedFuncIndex, ImportedMemoryIndex, ImportedTableIndex, ImportedGlobalIndex,
];

#[rustfmt::skip]
macro_rules! define_local_or_import {
    ($ty:ident, $local_ty:ident, $imported_ty:ident, $imports:ident) => {
        impl $ty {
            pub fn local_or_import(self, info: &ModuleInfo) -> LocalOrImport<$ty> {
                if self.index() < info.$imports.len() {
                    LocalOrImport::Import(<Self as LocalImport>::Import::new(self.index()))
                } else {
                    LocalOrImport::Local(<Self as LocalImport>::Local::new(self.index() - info.$imports.len()))
                }
            }
        }

        impl $local_ty {
            pub fn convert_up(self, info: &ModuleInfo) -> $ty {
                $ty ((self.index() + info.$imports.len()) as u32)
            }
        }

        impl $imported_ty {
            pub fn convert_up(self, _info: &ModuleInfo) -> $ty {
                $ty (self.index() as u32)
            }
        }
    };
    ($(($ty:ident | ($local_ty:ident, $imported_ty:ident): $imports:ident),)*) => {
        $(
            define_local_or_import!($ty, $local_ty, $imported_ty, $imports);
        )*
    };
}

#[rustfmt::skip]
define_local_or_import![
    (FuncIndex | (LocalFuncIndex, ImportedFuncIndex): imported_functions),
    (MemoryIndex | (LocalMemoryIndex, ImportedMemoryIndex): imported_memories),
    (TableIndex | (LocalTableIndex, ImportedTableIndex): imported_tables),
    (GlobalIndex | (LocalGlobalIndex, ImportedGlobalIndex): imported_globals),
];

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct SigIndex(u32);
impl TypedIndex for SigIndex {
    #[doc(hidden)]
    fn new(index: usize) -> Self {
        SigIndex(index as _)
    }

    #[doc(hidden)]
    fn index(&self) -> usize {
        self.0 as usize
    }
}

pub enum LocalOrImport<T>
where
    T: LocalImport,
{
    Local(T::Local),
    Import(T::Import),
}

impl<T> LocalOrImport<T>
where
    T: LocalImport,
{
    pub fn local(self) -> Option<T::Local> {
        match self {
            LocalOrImport::Local(local) => Some(local),
            LocalOrImport::Import(_) => None,
        }
    }

    pub fn import(self) -> Option<T::Import> {
        match self {
            LocalOrImport::Import(import) => Some(import),
            LocalOrImport::Local(_) => None,
        }
    }
}
