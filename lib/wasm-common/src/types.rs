use crate::indexes::{FunctionIndex, GlobalIndex};
use crate::units::Pages;
use crate::values::Value;

#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};

// Type Representations

// Value Types

/// A list of all possible value types in WebAssembly.
#[derive(Copy, Debug, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Type {
    /// Signed 32 bit integer.
    I32,
    /// Signed 64 bit integer.
    I64,
    /// Floating point 32 bit integer.
    F32,
    /// Floating point 64 bit integer.
    F64,
    /// A 128 bit number.
    V128,
    /// A reference to opaque data in the Wasm instance.
    AnyRef, /* = 128 */
    /// A reference to a Wasm function.
    FuncRef,
}

impl Type {
    /// Returns true if `Type` matches any of the numeric types. (e.g. `I32`,
    /// `I64`, `F32`, `F64`, `V128`).
    pub fn is_num(&self) -> bool {
        match self {
            Self::I32 | Self::I64 | Self::F32 | Self::F64 | Self::V128 => true,
            _ => false,
        }
    }

    /// Returns true if `Type` matches either of the reference types.
    pub fn is_ref(&self) -> bool {
        match self {
            Self::AnyRef | Self::FuncRef => true,
            _ => false,
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
/// The WebAssembly V128 type
pub struct V128(pub(crate) [u8; 16]);

impl V128 {
    /// Get the bytes corresponding to the V128 value
    pub fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
    /// Iterate over the bytes in the constant.
    pub fn iter(&self) -> impl Iterator<Item = &u8> {
        self.0.iter()
    }

    /// Convert the immediate into a vector.
    pub fn to_vec(self) -> Vec<u8> {
        self.0.to_vec()
    }

    /// Convert the immediate into a slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<&[u8]> for V128 {
    fn from(slice: &[u8]) -> Self {
        assert_eq!(slice.len(), 16);
        let mut buffer = [0; 16];
        buffer.copy_from_slice(slice);
        Self(buffer)
    }
}

// External Types

/// A list of all possible types which can be externally referenced from a
/// WebAssembly module.
///
/// This list can be found in [`ImportType`] or [`ExportType`], so these types
/// can either be imported or exported.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum ExternType {
    /// This external type is the type of a WebAssembly function.
    Function(FunctionType),
    /// This external type is the type of a WebAssembly global.
    Global(GlobalType),
    /// This external type is the type of a WebAssembly table.
    Table(TableType),
    /// This external type is the type of a WebAssembly memory.
    Memory(MemoryType),
}

fn is_global_compatible(exported: &GlobalType, imported: &GlobalType) -> bool {
    let GlobalType {
        ty: exported_ty,
        mutability: exported_mutability,
    } = exported;
    let GlobalType {
        ty: imported_ty,
        mutability: imported_mutability,
    } = imported;
    exported_ty == imported_ty && imported_mutability == exported_mutability
}

fn is_table_element_type_compatible(exported_type: Type, imported_type: Type) -> bool {
    match exported_type {
        Type::FuncRef => true,
        _ => imported_type == exported_type,
    }
}

fn is_table_compatible(exported: &TableType, imported: &TableType) -> bool {
    let TableType {
        ty: exported_ty,
        minimum: exported_minimum,
        maximum: exported_maximum,
    } = exported;
    let TableType {
        ty: imported_ty,
        minimum: imported_minimum,
        maximum: imported_maximum,
    } = imported;

    is_table_element_type_compatible(*exported_ty, *imported_ty)
        && imported_minimum <= exported_minimum
        && (imported_maximum.is_none()
            || (!exported_maximum.is_none()
                && imported_maximum.unwrap() >= exported_maximum.unwrap()))
}

fn is_memory_compatible(exported: &MemoryType, imported: &MemoryType) -> bool {
    let MemoryType {
        minimum: exported_minimum,
        maximum: exported_maximum,
        shared: exported_shared,
    } = exported;
    let MemoryType {
        minimum: imported_minimum,
        maximum: imported_maximum,
        shared: imported_shared,
    } = imported;

    imported_minimum <= exported_minimum
        && (imported_maximum.is_none()
            || (!exported_maximum.is_none()
                && imported_maximum.unwrap() >= exported_maximum.unwrap()))
        && exported_shared == imported_shared
}

macro_rules! accessors {
    ($(($variant:ident($ty:ty) $get:ident $unwrap:ident))*) => ($(
        /// Attempt to return the underlying type of this external type,
        /// returning `None` if it is a different type.
        pub fn $get(&self) -> Option<&$ty> {
            if let Self::$variant(e) = self {
                Some(e)
            } else {
                None
            }
        }

        /// Returns the underlying descriptor of this [`ExternType`], panicking
        /// if it is a different type.
        ///
        /// # Panics
        ///
        /// Panics if `self` is not of the right type.
        pub fn $unwrap(&self) -> &$ty {
            self.$get().expect(concat!("expected ", stringify!($ty)))
        }
    )*)
}

impl ExternType {
    accessors! {
        (Function(FunctionType) func unwrap_func)
        (Global(GlobalType) global unwrap_global)
        (Table(TableType) table unwrap_table)
        (Memory(MemoryType) memory unwrap_memory)
    }
    /// Check if two externs are compatible
    pub fn is_compatible_with(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Function(a), Self::Function(b)) => a == b,
            (Self::Global(a), Self::Global(b)) => is_global_compatible(a, b),
            (Self::Table(a), Self::Table(b)) => is_table_compatible(a, b),
            (Self::Memory(a), Self::Memory(b)) => is_memory_compatible(a, b),
            // The rest of possibilities, are not compatible
            _ => false,
        }
    }
}

// TODO: `shrink_to_fit` these or change it to `Box<[Type]>` if not using
// Cow or something else
/// The signature of a function that is either implemented
/// in a Wasm module or exposed to Wasm by the host.
///
/// WebAssembly functions can have 0 or more parameters and results.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct FunctionType {
    /// The parameters of the function
    params: Vec<Type>,
    /// The return values of the function
    results: Vec<Type>,
}

impl FunctionType {
    /// Creates a new Function Type with the given parameter and return types.
    pub fn new<Params, Returns>(params: Params, returns: Returns) -> Self
    where
        Params: Into<Vec<Type>>,
        Returns: Into<Vec<Type>>,
    {
        Self {
            params: params.into(),
            results: returns.into(),
        }
    }

    /// Parameter types.
    pub fn params(&self) -> &[Type] {
        &self.params
    }

    /// Return types.
    pub fn results(&self) -> &[Type] {
        &self.results
    }

    // /// Returns true if parameter types match the function signature.
    // pub fn check_params(&self, params: &[Value<T>]) -> bool {
    //     self.params.len() == params.len()
    //         && self
    //             .params
    //             .iter()
    //             .zip(params.iter().map(|val| val.ty()))
    //             .all(|(t0, ref t1)| t0 == t1)
    // }
}

impl std::fmt::Display for FunctionType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let params = self
            .params
            .iter()
            .map(|p| format!("{:?}", p))
            .collect::<Vec<_>>()
            .join(", ");
        let results = self
            .results
            .iter()
            .map(|p| format!("{:?}", p))
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "[{}] -> [{}]", params, results)
    }
}

/// Indicator of whether a global is mutable or not
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum Mutability {
    /// The global is constant and its value does not change
    Const,
    /// The value of the global can change over time
    Var,
}

impl Mutability {
    /// Returns a boolean indicating if the enum is set to mutable.
    pub fn is_mutable(self) -> bool {
        self.into()
    }
}

impl From<bool> for Mutability {
    fn from(value: bool) -> Self {
        if value {
            Self::Var
        } else {
            Self::Const
        }
    }
}

impl From<Mutability> for bool {
    fn from(value: Mutability) -> Self {
        match value {
            Mutability::Var => true,
            Mutability::Const => false,
        }
    }
}

/// WebAssembly global.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct GlobalType {
    /// The type of the value stored in the global.
    pub ty: Type,
    /// A flag indicating whether the value may change at runtime.
    pub mutability: Mutability,
}

// Global Types

/// A WebAssembly global descriptor.
///
/// This type describes an instance of a global in a WebAssembly module. Globals
/// are local to an [`Instance`](crate::Instance) and are either immutable or
/// mutable.
impl GlobalType {
    /// Create a new Global variable
    /// # Usage:
    /// ```
    /// use wasm_common::{GlobalType, Type, Mutability, Value};
    ///
    /// // An I32 constant global
    /// let global = GlobalType::new(Type::I32, Mutability::Const);
    /// // An I64 mutable global
    /// let global = GlobalType::new(Type::I64, Mutability::Var);
    /// ```
    pub fn new(ty: Type, mutability: Mutability) -> Self {
        Self { ty, mutability }
    }
}

impl std::fmt::Display for GlobalType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let mutability = match self.mutability {
            Mutability::Const => "constant",
            Mutability::Var => "mutable",
        };
        write!(f, "{} ({})", self.ty, mutability)
    }
}

/// Globals are initialized via the `const` operators or by referring to another import.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub enum GlobalInit {
    /// An `i32.const`.
    I32Const(i32),
    /// An `i64.const`.
    I64Const(i64),
    /// An `f32.const`.
    F32Const(f32),
    /// An `f64.const`.
    F64Const(f64),
    /// A `v128.const`.
    V128Const(V128),
    /// A `global.get` of another global.
    GetGlobal(GlobalIndex),
    /// A `ref.null`.
    RefNullConst,
    /// A `ref.func <index>`.
    RefFunc(FunctionIndex),
}

impl GlobalInit {
    /// Get the `GlobalInit` from a given `Value`
    pub fn from_value<T>(value: Value<T>) -> Self {
        match value {
            Value::I32(i) => Self::I32Const(i),
            Value::I64(i) => Self::I64Const(i),
            Value::F32(f) => Self::F32Const(f),
            Value::F64(f) => Self::F64Const(f),
            _ => unimplemented!("GlobalInit from_value for {:?}", value),
        }
    }
    /// Get the `Value` from the Global init value
    pub fn to_value<T>(&self) -> Value<T> {
        match self {
            Self::I32Const(i) => Value::I32(*i),
            Self::I64Const(i) => Value::I64(*i),
            Self::F32Const(f) => Value::F32(*f),
            Self::F64Const(f) => Value::F64(*f),
            _ => unimplemented!("GlobalInit to_value for {:?}", self),
        }
    }
}

// Table Types

/// A descriptor for a table in a WebAssembly module.
///
/// Tables are contiguous chunks of a specific element, typically a `funcref` or
/// an `anyref`. The most common use for tables is a function table through
/// which `call_indirect` can invoke other functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct TableType {
    /// The type of data stored in elements of the table.
    pub ty: Type,
    /// The minimum number of elements in the table.
    pub minimum: u32,
    /// The maximum number of elements in the table.
    pub maximum: Option<u32>,
}

impl TableType {
    /// Creates a new table descriptor which will contain the specified
    /// `element` and have the `limits` applied to its length.
    pub fn new(ty: Type, minimum: u32, maximum: Option<u32>) -> Self {
        Self {
            ty,
            minimum,
            maximum,
        }
    }
}

impl std::fmt::Display for TableType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if let Some(maximum) = self.maximum {
            write!(f, "{} ({}..{})", self.ty, self.minimum, maximum)
        } else {
            write!(f, "{} ({}..)", self.ty, self.minimum)
        }
    }
}

// Memory Types

/// A descriptor for a WebAssembly memory type.
///
/// Memories are described in units of pages (64KB) and represent contiguous
/// chunks of addressable memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct MemoryType {
    /// The minimum number of pages in the memory.
    pub minimum: Pages,
    /// The maximum number of pages in the memory.
    pub maximum: Option<Pages>,
    /// Whether the memory may be shared between multiple threads.
    pub shared: bool,
}

impl MemoryType {
    /// Creates a new descriptor for a WebAssembly memory given the specified
    /// limits of the memory.
    pub fn new<IntoPages>(minimum: IntoPages, maximum: Option<IntoPages>, shared: bool) -> Self
    where
        IntoPages: Into<Pages>,
    {
        Self {
            minimum: minimum.into(),
            maximum: maximum.map(|m| m.into()),
            shared,
        }
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let shared = if self.shared { "shared" } else { "not shared" };
        if let Some(maximum) = self.maximum {
            write!(f, "{} ({:?}..{:?})", shared, self.minimum, maximum)
        } else {
            write!(f, "{} ({:?}..)", shared, self.minimum)
        }
    }
}

// Import Types

/// A descriptor for an imported value into a wasm module.
///
/// This type is primarily accessed from the
/// [`Module::imports`](crate::Module::imports) API. Each [`ImportType`]
/// describes an import into the wasm module with the module/name that it's
/// imported from as well as the type of item that's being imported.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct ImportType<T = ExternType> {
    module: String,
    name: String,
    ty: T,
}

impl<T> ImportType<T> {
    /// Creates a new import descriptor which comes from `module` and `name` and
    /// is of type `ty`.
    pub fn new(module: &str, name: &str, ty: T) -> Self {
        Self {
            module: module.to_owned(),
            name: name.to_owned(),
            ty,
        }
    }

    /// Returns the module name that this import is expected to come from.
    pub fn module(&self) -> &str {
        &self.module
    }

    /// Returns the field name of the module that this import is expected to
    /// come from.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the expected type of this import.
    pub fn ty(&self) -> &T {
        &self.ty
    }
}

// Export Types

/// A descriptor for an exported WebAssembly value.
///
/// This type is primarily accessed from the
/// [`Module::exports`](crate::Module::exports) accessor and describes what
/// names are exported from a wasm module and the type of the item that is
/// exported.
///
/// The `<T>` refefers to `ExternType`, however it can also refer to use
/// `MemoryType`, `TableType`, `FunctionType` and `GlobalType` for ease of
/// use.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub struct ExportType<T = ExternType> {
    name: String,
    ty: T,
}

impl<T> ExportType<T> {
    /// Creates a new export which is exported with the given `name` and has the
    /// given `ty`.
    pub fn new(name: &str, ty: T) -> Self {
        Self {
            name: name.to_string(),
            ty,
        }
    }

    /// Returns the name by which this export is known by.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the type of this export.
    pub fn ty(&self) -> &T {
        &self.ty
    }
}
