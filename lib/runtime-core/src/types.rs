use crate::{module::ModuleInner, structures::TypedIndex};

/// Represents a WebAssembly type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Represents a WebAssembly value.
///
/// As the number of types in WebAssembly expand,
/// this structure will expand as well.
#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Any wasm function.
    Anyfunc,
}

#[derive(Debug, Clone, Copy)]
pub struct Table {
    /// Type of data stored in this table.
    pub ty: ElementType,
    /// The minimum number of elements that must be stored in this table.
    pub min: u32,
    /// The maximum number of elements in this table.
    pub max: Option<u32>,
}

impl Table {
    pub(crate) fn fits_in_imported(&self, imported: &Table) -> bool {
        // TODO: We should define implementation limits.
        let imported_max = imported.max.unwrap_or(u32::max_value());
        let self_max = self.max.unwrap_or(u32::max_value());
        self.ty == imported.ty && imported_max <= self_max && self.min <= imported.min
    }
}

/// A const value initializer.
/// Over time, this will be able to represent more and more
/// complex expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Initializer {
    /// Corresponds to a `const.*` instruction.
    Const(Value),
    /// Corresponds to a `get_global` instruction.
    GetGlobal(ImportedGlobalIndex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalDesc {
    pub mutable: bool,
    pub ty: Type,
}

/// A wasm global.
#[derive(Debug, Clone)]
pub struct Global {
    pub desc: GlobalDesc,
    pub init: Initializer,
}

/// A wasm memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    /// The minimum number of allowed pages.
    pub min: u32,
    /// The maximum number of allowed pages.
    pub max: Option<u32>,
    /// This memory can be shared between wasm threads.
    pub shared: bool,
}

impl Memory {
    pub fn is_static_heap(&self) -> bool {
        self.max.is_some()
    }

    pub(crate) fn fits_in_imported(&self, imported: &Memory) -> bool {
        let imported_max = imported.max.unwrap_or(65_536);
        let self_max = self.max.unwrap_or(65_536);

        self.shared == imported.shared && imported_max <= self_max && self.min <= imported.min
    }
}

/// The signature of a function that is either implemented
/// in a wasm module or exposed to wasm by the host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FuncSig {
    pub params: Vec<Type>,
    pub returns: Vec<Type>,
}

impl FuncSig {
    pub fn check_sig(&self, params: &[Value]) -> bool {
        self.params.len() == params.len()
            && self
                .params
                .iter()
                .zip(params.iter().map(|val| val.ty()))
                .all(|(t0, ref t1)| t0 == t1)
    }
}

pub trait LocalImport {
    type Local: TypedIndex;
    type Import: TypedIndex;
}

#[rustfmt::skip]
macro_rules! define_map_index {
    ($ty:ident) => {
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
            pub fn local_or_import(self, module: &ModuleInner) -> LocalOrImport<$ty> {
                if self.index() < module.$imports.len() {
                    LocalOrImport::Import(<Self as LocalImport>::Import::new(self.index()))
                } else {
                    LocalOrImport::Local(<Self as LocalImport>::Local::new(self.index() - module.$imports.len()))
                }
            }
        }

        impl $local_ty {
            pub fn convert_up(self, module: &ModuleInner) -> $ty {
                $ty ((self.index() + module.$imports.len()) as u32)
            }
        }

        impl $imported_ty {
            pub fn convert_up(self, _module: &ModuleInner) -> $ty {
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

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
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
