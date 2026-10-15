//! Backend-independent model of the wasixv2 definitions.
//!
//! Produced by [`crate::parse`]: annotations are parsed and validated, `use` copies are resolved
//! to their originals and every type carries its C layout for the chosen [`Target`].

/// The guest data model a [`Model`] is laid out for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// Width in bytes of pointers, `long` and `size_t` (4 for wasm32, 8 for wasm64).
    pub pointer_width: u32,
}

impl Target {
    pub const WASM32: Target = Target { pointer_width: 4 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prim {
    U8,
    U16,
    U32,
    U64,
    S8,
    S16,
    S32,
    S64,
}

impl Prim {
    pub fn size(self) -> u32 {
        match self {
            Prim::U8 | Prim::S8 => 1,
            Prim::U16 | Prim::S16 => 2,
            Prim::U32 | Prim::S32 => 4,
            Prim::U64 | Prim::S64 => 8,
        }
    }

    pub fn is_signed(self) -> bool {
        matches!(self, Prim::S8 | Prim::S16 | Prim::S32 | Prim::S64)
    }

    pub fn wit_name(self) -> &'static str {
        match self {
            Prim::U8 => "u8",
            Prim::U16 => "u16",
            Prim::U32 => "u32",
            Prim::U64 => "u64",
            Prim::S8 => "s8",
            Prim::S16 => "s16",
            Prim::S32 => "s32",
            Prim::S64 => "s64",
        }
    }

    pub fn from_wit_name(name: &str) -> Option<Prim> {
        [
            Prim::U8,
            Prim::U16,
            Prim::U32,
            Prim::U64,
            Prim::S8,
            Prim::S16,
            Prim::S32,
            Prim::S64,
        ]
        .into_iter()
        .find(|p| p.wit_name() == name)
    }

    /// The integer of the same signedness with `size` bytes.
    pub fn with_size(self, size: u32) -> Prim {
        match (self.is_signed(), size) {
            (false, 1) => Prim::U8,
            (false, 2) => Prim::U16,
            (false, 4) => Prim::U32,
            (false, _) => Prim::U64,
            (true, 1) => Prim::S8,
            (true, 2) => Prim::S16,
            (true, 4) => Prim::S32,
            (true, _) => Prim::S64,
        }
    }

    pub fn range(self) -> (i128, i128) {
        let bits = self.size() * 8;
        if self.is_signed() {
            (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
        } else {
            (0, (1i128 << bits) - 1)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u32,
    pub align: u32,
}

/// A reference to a type, as used by fields, params and pointees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TyRef {
    Prim(Prim),
    /// A named [`TypeDef`] (kebab-case WIT name).
    Named(String),
    /// A fixed-size array (`list<T, N>`).
    Array(Box<TyRef>, u32),
}

/// Wasm value type of a scalar crossing the import boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    I32,
    I64,
}

#[derive(Clone, Debug)]
pub struct Model {
    /// `namespace:name@version`.
    pub package: String,
    pub target: Target,
    /// All named types, across interfaces, in definition order.
    pub types: Vec<TypeDef>,
    pub interfaces: Vec<Interface>,
}

#[derive(Clone, Debug)]
pub struct TypeDef {
    /// Kebab-case WIT name.
    pub name: String,
    /// Name of the defining interface.
    pub interface: String,
    pub docs: Vec<String>,
    /// `@c(...)`: the C spelling musl uses for this type.
    pub c_type: Option<String>,
    pub kind: TypeKind,
    pub layout: Layout,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    Alias(Alias),
    Pointer(Pointer),
    Record(Record),
    Constants(Constants),
}

#[derive(Clone, Debug)]
pub struct Alias {
    /// For `@pointer-sized` aliases this is the declared wasm32 type; see [`Model::scalar_prim`].
    pub target: TyRef,
    pub pointer_sized: bool,
    /// `@newtype`: Rust backends emit a distinct type instead of a type alias.
    pub newtype: bool,
}

#[derive(Clone, Debug)]
pub struct Pointer {
    pub mutable: bool,
    /// `None` is `void`.
    pub pointee: Option<TyRef>,
    /// Points to the first element of an array (`T[]`).
    pub array: bool,
    pub nullable: bool,
}

#[derive(Clone, Debug)]
pub struct Record {
    pub is_union: bool,
    /// `@kernel-only`: musl has no layout-compatible type, so C asserts skip it.
    pub kernel_only: bool,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug)]
pub struct Field {
    /// Kebab-case WIT name.
    pub name: String,
    /// `@c-name(...)`: exact C member name when it differs from the snake-cased name.
    pub c_name: Option<String>,
    /// `@c-skip`: musl's type has no counterpart of this member; no `offsetof` assert.
    pub c_skip: bool,
    pub docs: Vec<String>,
    pub ty: TyRef,
    pub offset: u32,
    pub layout: Layout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstKind {
    /// Distinct values (WIT `enum`), e.g. errno.
    Enum,
    /// Bitmask values (WIT `flags`), e.g. `O_*`.
    Flags,
}

#[derive(Clone, Debug)]
pub struct Constants {
    pub kind: ConstKind,
    /// `@abi-error`: the error type of syscall results (at most one per model).
    pub abi_error: bool,
    /// `@c-enum`: musl defines the members as C enumerators, which `#ifdef` cannot see.
    pub c_enum: bool,
    pub repr: Prim,
    pub members: Vec<Constant>,
}

#[derive(Clone, Debug)]
pub struct Constant {
    /// Kebab-case WIT name.
    pub name: String,
    /// Exact C/Rust constant name: `@name(...)`, else the SCREAMING_SNAKE form of `name`.
    pub c_name: String,
    /// `@c-skip`: musl's value intentionally differs (musl translates); no value assert.
    pub c_skip: bool,
    pub docs: Vec<String>,
    pub value: i128,
}

#[derive(Clone, Debug)]
pub struct Interface {
    pub name: String,
    pub docs: Vec<String>,
    /// `@module(...)`: wasm import module of its functions.
    pub module: Option<String>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Debug)]
pub struct Function {
    /// Kebab-case WIT name.
    pub name: String,
    pub import_name: String,
    pub docs: Vec<String>,
    pub nr: Option<u32>,
    /// `@sys-name(...)`: suffix of musl's `SYS_*`/`__NR_*` macro when it differs from
    /// `import_name`.
    pub sys_name: Option<String>,
    pub noreturn: bool,
    pub unsupported: bool,
    /// Params that riscv32 splits into a register pair.
    pub merged_i64: Vec<MergedI64>,
    /// `@c-one-slot(param)`: 64-bit params that musl passes in a single `long` slot.
    pub c_one_slot: Vec<String>,
    /// `(param, C type)` prototype overrides.
    pub c_params: Vec<(String, String)>,
    /// `@slice(ptr, len)`: pointer/length pairs the Rust backends merge into one user slice.
    pub slices: Vec<SliceSpec>,
    /// `@returns(T)`: the type of a successful result, for the Rust backends. `None` keeps the
    /// declared result type.
    pub returns: Option<TyRef>,
    pub params: Vec<Param>,
    pub result: Option<TyRef>,
}

/// `@slice(ptr, len)`: a byte buffer and the param holding its length in bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliceSpec {
    /// Kebab-case WIT name of the pointer param.
    pub ptr: String,
    /// Kebab-case WIT name of the length param.
    pub len: String,
}

/// `@merged-i64(param[, lo-hi|hi-lo])`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergedI64 {
    /// Kebab-case WIT param name.
    pub param: String,
    pub order: SplitOrder,
}

/// Order of the two 32-bit halves of a split 64-bit argument in the original (riscv32) call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SplitOrder {
    /// Low word first: the `__SYSCALL_LL_E`/`_O` layout.
    #[default]
    LoHi,
    /// High word first (riscv32 `llseek`'s `offset_high, offset_low`).
    HiLo,
}

impl Function {
    /// Halves order of a 64-bit param; params without `@merged-i64` default to [`SplitOrder::LoHi`].
    pub fn split_order(&self, param: &str) -> SplitOrder {
        self.merged_i64
            .iter()
            .find(|m| m.param == param)
            .map_or_else(SplitOrder::default, |m| m.order)
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    /// Kebab-case WIT name.
    pub name: String,
    pub ty: TyRef,
}

impl Model {
    pub fn type_def(&self, name: &str) -> Option<&TypeDef> {
        self.types.iter().find(|t| t.name == name)
    }

    /// Layout of an already-validated type reference.
    pub fn layout(&self, ty: &TyRef) -> Layout {
        match ty {
            TyRef::Prim(p) => Layout {
                size: p.size(),
                align: p.size(),
            },
            TyRef::Named(n) => self.expect_type(n).layout,
            TyRef::Array(elem, n) => {
                let l = self.layout(elem);
                Layout {
                    size: l
                        .size
                        .checked_mul(*n)
                        .expect("array sizes are checked during parsing"),
                    align: l.align,
                }
            }
        }
    }

    /// The integer a scalar type is passed/stored as on the target, or `None` for aggregates.
    pub fn scalar_prim(&self, ty: &TyRef) -> Option<Prim> {
        match ty {
            TyRef::Prim(p) => Some(*p),
            TyRef::Array(..) => None,
            TyRef::Named(n) => {
                let def = self.expect_type(n);
                match &def.kind {
                    TypeKind::Alias(a) if a.pointer_sized => self
                        .scalar_prim(&a.target)
                        .map(|p| p.with_size(self.target.pointer_width)),
                    TypeKind::Alias(a) => self.scalar_prim(&a.target),
                    TypeKind::Pointer(_) => Some(Prim::U32.with_size(self.target.pointer_width)),
                    TypeKind::Constants(c) => Some(c.repr),
                    TypeKind::Record(_) => None,
                }
            }
        }
    }

    pub fn valtype(&self, ty: &TyRef) -> Option<ValType> {
        self.scalar_prim(ty).map(|p| match p.size() {
            8 => ValType::I64,
            _ => ValType::I32,
        })
    }

    /// Whether `ty` is (an alias of) a `@pointer-sized` alias or a typed pointer.
    pub fn is_pointer_sized(&self, ty: &TyRef) -> bool {
        let TyRef::Named(n) = ty else {
            return false;
        };
        match &self.expect_type(n).kind {
            TypeKind::Alias(a) => a.pointer_sized || self.is_pointer_sized(&a.target),
            TypeKind::Pointer(_) => true,
            _ => false,
        }
    }

    /// The `@abi-error` constant family, if any.
    pub fn abi_error(&self) -> Option<(&TypeDef, &Constants)> {
        self.types.iter().find_map(|t| match &t.kind {
            TypeKind::Constants(c) if c.abi_error => Some((t, c)),
            _ => None,
        })
    }

    /// The result type most functions return (first wins ties): `slong` for Linux syscalls.
    pub fn common_result(&self) -> Option<&TyRef> {
        let results: Vec<&TyRef> = self
            .interfaces
            .iter()
            .flat_map(|i| &i.functions)
            .filter_map(|f| f.result.as_ref())
            .collect();
        let count = |r: &TyRef| results.iter().filter(|x| **x == r).count();
        let mut best: Option<(&TyRef, usize)> = None;
        for r in &results {
            let n = count(r);
            if best.is_none_or(|(_, m)| n > m) {
                best = Some((r, n));
            }
        }
        best.map(|(r, _)| r)
    }

    fn expect_type(&self, name: &str) -> &TypeDef {
        self.type_def(name)
            .unwrap_or_else(|| panic!("validated model references unknown type `{name}`"))
    }
}

/// Name conversions shared by all backends.
pub mod names {
    /// `stx-mode` -> `stx_mode`.
    pub fn snake(kebab: &str) -> String {
        kebab.replace('-', "_").to_ascii_lowercase()
    }

    /// `o-creat` -> `O_CREAT`.
    pub fn screaming(kebab: &str) -> String {
        kebab.replace('-', "_").to_ascii_uppercase()
    }

    /// `statx-timestamp` -> `StatxTimestamp`.
    pub fn upper_camel(kebab: &str) -> String {
        kebab
            .split('-')
            .map(|seg| {
                let lower = seg.to_ascii_lowercase();
                let mut chars = lower.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect()
    }
}
