//! The runtime types modules represent type used within the wasm runtime and helper functions to
//! convert to other represenations.

use crate::{memory::BackingMemoryType, module::ModuleInfo, structures::TypedIndex, units::Pages};
use std::{borrow::Cow, convert::TryFrom};

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
    /// The `v128` type.
    V128,
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
    /// The `v128` type.
    V128(u128),
}

impl Value {
    /// The `Type` of this `Value`.
    pub fn ty(&self) -> Type {
        match self {
            Value::I32(_) => Type::I32,
            Value::I64(_) => Type::I64,
            Value::F32(_) => Type::F32,
            Value::F64(_) => Type::F64,
            Value::V128(_) => Type::V128,
        }
    }

    /// Convert this `Value` to a u128 binary representation.
    pub fn to_u128(&self) -> u128 {
        match *self {
            Value::I32(x) => x as u128,
            Value::I64(x) => x as u128,
            Value::F32(x) => f32::to_bits(x) as u128,
            Value::F64(x) => f64::to_bits(x) as u128,
            Value::V128(x) => x,
        }
    }
}

macro_rules! value_conversions {
    ($native_type:ty, $value_variant:ident) => {
        impl From<$native_type> for Value {
            fn from(n: $native_type) -> Self {
                Self::$value_variant(n)
            }
        }

        impl TryFrom<&Value> for $native_type {
            type Error = &'static str;

            fn try_from(value: &Value) -> Result<Self, Self::Error> {
                match value {
                    Value::$value_variant(value) => Ok(*value),
                    _ => Err("Invalid cast."),
                }
            }
        }
    };
}

value_conversions!(i32, I32);
value_conversions!(i64, I64);
value_conversions!(f32, F32);
value_conversions!(f64, F64);
value_conversions!(u128, V128);

/// Represents a native wasm type.
pub unsafe trait NativeWasmType: Copy + Into<Value>
where
    Self: Sized,
{
    /// Type for this `NativeWasmType`.
    const TYPE: Type;

    /// Convert from u64 bites to self.
    fn from_binary(bits: u64) -> Self;

    /// Convert self to u64 binary representation.
    fn to_binary(self) -> u64;
}

unsafe impl NativeWasmType for i32 {
    const TYPE: Type = Type::I32;

    fn from_binary(bits: u64) -> Self {
        bits as _
    }

    fn to_binary(self) -> u64 {
        self as _
    }
}

unsafe impl NativeWasmType for i64 {
    const TYPE: Type = Type::I64;

    fn from_binary(bits: u64) -> Self {
        bits as _
    }

    fn to_binary(self) -> u64 {
        self as _
    }
}

unsafe impl NativeWasmType for f32 {
    const TYPE: Type = Type::F32;

    fn from_binary(bits: u64) -> Self {
        f32::from_bits(bits as u32)
    }

    fn to_binary(self) -> u64 {
        self.to_bits() as _
    }
}

unsafe impl NativeWasmType for f64 {
    const TYPE: Type = Type::F64;

    fn from_binary(bits: u64) -> Self {
        f64::from_bits(bits)
    }

    fn to_binary(self) -> u64 {
        self.to_bits()
    }
}

/// A trait to represent a wasm extern type.
pub unsafe trait WasmExternType: Copy
where
    Self: Sized,
{
    /// Native wasm type for this `WasmExternType`.
    type Native: NativeWasmType;

    /// Convert from given `Native` type to self.
    fn from_native(native: Self::Native) -> Self;

    /// Convert self to `Native` type.
    fn to_native(self) -> Self::Native;
}

macro_rules! wasm_extern_type {
    ($type:ty => $native_type:ty) => {
        unsafe impl WasmExternType for $type {
            type Native = $native_type;

            fn from_native(native: Self::Native) -> Self {
                native as _
            }

            fn to_native(self) -> Self::Native {
                self as _
            }
        }
    };
}

wasm_extern_type!(i8 => i32);
wasm_extern_type!(u8 => i32);
wasm_extern_type!(i16 => i32);
wasm_extern_type!(u16 => i32);
wasm_extern_type!(i32 => i32);
wasm_extern_type!(u32 => i32);
wasm_extern_type!(i64 => i64);
wasm_extern_type!(u64 => i64);
wasm_extern_type!(f32 => f32);
wasm_extern_type!(f64 => f64);

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

/// Trait for a Value type. A Value type is a type that is always valid and may
/// be safely copied.
///
/// That is, for all possible bit patterns a valid Value type can be constructed
/// from those bits.
///
/// Concretely a `u32` is a Value type because every combination of 32 bits is
/// a valid `u32`. However a `bool` is _not_ a Value type because any bit patterns
/// other than `0` and `1` are invalid in Rust and may cause undefined behavior if
/// a `bool` is constructed from those bytes.
pub unsafe trait ValueType: Copy
where
    Self: Sized,
{
}

macro_rules! convert_value_impl {
    ($t:ty) => {
        unsafe impl ValueType for $t {}
    };
    ( $($t:ty),* ) => {
        $(
            convert_value_impl!($t);
        )*
    };
}

convert_value_impl!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

/// Kinds of element types.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Any wasm function.
    Anyfunc,
}

/// Legacy wrapper around [`TableType`].
#[deprecated(note = "Please use `TableType` instead.")]
pub type TableDescriptor = TableType;

/// Describes the properties of a table including the element types, minimum and optional maximum,
/// number of elements in the table.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableType {
    /// Type of data stored in this table.
    pub element: ElementType,
    /// The minimum number of elements that must be stored in this table.
    pub minimum: u32,
    /// The maximum number of elements in this table.
    pub maximum: Option<u32>,
}

impl TableType {
    pub(crate) fn fits_in_imported(&self, imported: TableType) -> bool {
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

/// Legacy wrapper around [`GlobalType`].
#[deprecated(note = "Please use `GlobalType` instead.")]
pub type GlobalDescriptor = GlobalType;

/// Describes the mutability and type of a Global
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalType {
    /// Mutable flag.
    pub mutable: bool,
    /// Wasm type.
    pub ty: Type,
}

/// A wasm global.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GlobalInit {
    /// Global descriptor.
    pub desc: GlobalType,
    /// Global initializer.
    pub init: Initializer,
}

/// Legacy wrapper around [`MemoryType`].
#[deprecated(note = "Please use `MemoryType` instead.")]
pub type MemoryDescriptor = MemoryType;

/// A wasm memory descriptor.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryType {
    /// The minimum number of allowed pages.
    pub minimum: Pages,
    /// The maximum number of allowed pages.
    pub maximum: Option<Pages>,
    /// This memory can be shared between wasm threads.
    pub shared: bool,
    /// The type of the memory
    pub memory_type: BackingMemoryType,
}

impl MemoryType {
    /// Create a new memory descriptor with the given min/max pages and shared flag.
    pub fn new(minimum: Pages, maximum: Option<Pages>, shared: bool) -> Result<Self, String> {
        let memory_type = match (maximum.is_some(), shared) {
            (true, true) => BackingMemoryType::SharedStatic,
            (true, false) => BackingMemoryType::Static,
            (false, false) => BackingMemoryType::Dynamic,
            (false, true) => {
                return Err("Max number of pages is required for shared memory".to_string());
            }
        };
        Ok(MemoryType {
            minimum,
            maximum,
            shared,
            memory_type,
        })
    }

    /// Returns the `MemoryType` for this descriptor.
    pub fn memory_type(&self) -> BackingMemoryType {
        self.memory_type
    }

    pub(crate) fn fits_in_imported(&self, imported: MemoryType) -> bool {
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

/// Information about a function.
pub type FuncType = FuncSig;

impl FuncSig {
    /// Creates a new function signatures with the given parameter and return types.
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

    /// Parameter types.
    pub fn params(&self) -> &[Type] {
        &self.params
    }

    /// Return types.
    pub fn returns(&self) -> &[Type] {
        &self.returns
    }

    /// Returns true if parameter types match the function signature.
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

/// Trait that represents Local or Import.
pub trait LocalImport {
    /// Local type.
    type Local: TypedIndex;
    /// Import type.
    type Import: TypedIndex;
}

#[rustfmt::skip]
macro_rules! define_map_index {
    ($ty:ident) => {
        /// Typed Index
        #[derive(Serialize, Deserialize)]
        #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
            /// Converts self into `LocalOrImport`.
            pub fn local_or_import(self, info: &ModuleInfo) -> LocalOrImport<$ty> {
                if self.index() < info.$imports.len() {
                    LocalOrImport::Import(<Self as LocalImport>::Import::new(self.index()))
                } else {
                    LocalOrImport::Local(<Self as LocalImport>::Local::new(self.index() - info.$imports.len()))
                }
            }
        }

        impl $local_ty {
            /// Convert up.
            pub fn convert_up(self, info: &ModuleInfo) -> $ty {
                $ty ((self.index() + info.$imports.len()) as u32)
            }
        }

        impl $imported_ty {
            /// Convert up.
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

/// Index for signature.
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

/// Kind of local or import type.
pub enum LocalOrImport<T>
where
    T: LocalImport,
{
    /// Local.
    Local(T::Local),
    /// Import.
    Import(T::Import),
}

impl<T> LocalOrImport<T>
where
    T: LocalImport,
{
    /// Returns `Some` if self is local,  `None` if self is an import.
    pub fn local(self) -> Option<T::Local> {
        match self {
            LocalOrImport::Local(local) => Some(local),
            LocalOrImport::Import(_) => None,
        }
    }

    /// Returns `Some` if self is an import,  `None` if self is local.
    pub fn import(self) -> Option<T::Import> {
        match self {
            LocalOrImport::Import(import) => Some(import),
            LocalOrImport::Local(_) => None,
        }
    }
}

/// Information about an import such as its type and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternType {
    /// The import is a function.
    Function(FuncType),
    /// The import is a global variable.
    Global(GlobalType),
    /// The import is a Wasm linear memory.
    Memory(MemoryType),
    /// The import is a Wasm table.
    Table(TableType),
}

impl From<FuncType> for ExternType {
    fn from(other: FuncType) -> Self {
        ExternType::Function(other)
    }
}
impl From<&FuncType> for ExternType {
    fn from(other: &FuncType) -> Self {
        ExternType::Function(other.clone())
    }
}
impl From<MemoryType> for ExternType {
    fn from(other: MemoryType) -> Self {
        ExternType::Memory(other)
    }
}
impl From<&MemoryType> for ExternType {
    fn from(other: &MemoryType) -> Self {
        ExternType::Memory(*other)
    }
}

impl From<TableType> for ExternType {
    fn from(other: TableType) -> Self {
        ExternType::Table(other)
    }
}
impl From<&TableType> for ExternType {
    fn from(other: &TableType) -> Self {
        ExternType::Table(*other)
    }
}
impl From<GlobalType> for ExternType {
    fn from(other: GlobalType) -> Self {
        ExternType::Global(other)
    }
}
impl From<&GlobalType> for ExternType {
    fn from(other: &GlobalType) -> Self {
        ExternType::Global(*other)
    }
}

/// A type describing an import that a [`Module`] needs to be instantiated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportType {
    /// The namespace that this import is in.
    pub namespace: String,
    /// The name of the import.
    pub name: String,
    /// The type of the import and information about the import.
    pub ty: ExternType,
}

/// Type describing an export that the [`Module`] provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportType<'a> {
    /// The name identifying the export.
    pub name: &'a str,
    /// The type of the export.
    pub ty: ExternType,
}

#[cfg(test)]
mod tests {
    use crate::types::NativeWasmType;
    use crate::types::WasmExternType;

    #[test]
    fn test_native_types_round_trip() {
        assert_eq!(
            42i32,
            i32::from_native(i32::from_binary((42i32).to_native().to_binary()))
        );

        assert_eq!(
            -42i32,
            i32::from_native(i32::from_binary((-42i32).to_native().to_binary()))
        );

        use std::i64;
        let xi64 = i64::MAX;
        assert_eq!(
            xi64,
            i64::from_native(i64::from_binary((xi64).to_native().to_binary()))
        );
        let yi64 = i64::MIN;
        assert_eq!(
            yi64,
            i64::from_native(i64::from_binary((yi64).to_native().to_binary()))
        );

        assert_eq!(
            16.5f32,
            f32::from_native(f32::from_binary((16.5f32).to_native().to_binary()))
        );

        assert_eq!(
            -16.5f32,
            f32::from_native(f32::from_binary((-16.5f32).to_native().to_binary()))
        );

        use std::f64;
        let xf64: f64 = f64::MAX;
        assert_eq!(
            xf64,
            f64::from_native(f64::from_binary((xf64).to_native().to_binary()))
        );

        let yf64: f64 = f64::MIN;
        assert_eq!(
            yf64,
            f64::from_native(f64::from_binary((yf64).to_native().to_binary()))
        );
    }
}
