//! [`Model`] -> Rust source for `wasixv2-abi`: guest types, constants and syscall metadata.
//!
//! The emitted code is unformatted; [`crate::format::rustfmt`] normalizes it.
//!
//! The emitted layout asserts assume the host aligns `u64` to 8 bytes, as 64-bit hosts and wasm
//! do. On i686 Linux (`u64` aligned to 4) the generated module fails to compile.

use std::fmt::Write as _;

use anyhow::Result;

use crate::ir::{
    Alias, ConstKind, Constants, Function, Model, Pointer, Prim, Record, TypeDef, TypeKind,
    ValType, names,
};
use crate::rust::{Namespace, header, ident, prim, push_docs, raw, ty, type_name, wit_ty};

/// Generates the (unformatted) Rust module for `model`.
pub fn generate(model: &Model, source: &str) -> Result<String> {
    check_names(model)?;
    let mut g = Gen {
        model,
        out: String::new(),
    };
    g.out.push_str(&header(source));
    g.prelude();
    for t in &model.types {
        g.type_def(t);
    }
    g.syscalls();
    Ok(g.out)
}

struct Gen<'a> {
    model: &'a Model,
    out: String,
}

/// Type-namespace names the generated module defines or uses unqualified.
const RESERVED_TYPES: &[&str] = &[
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Eq",
    "Err",
    "GuestLayout",
    "GuestPtr",
    "GuestPtrMut",
    "Hash",
    "None",
    "Ok",
    "Option",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "PhantomData",
    "Result",
    "Sized",
    "Some",
    "SyscallDef",
    "SyscallParam",
    "ValType",
];

/// Associated items generated on constant families.
const RESERVED_CONST_ITEMS: &[&str] = &[
    "MEMBERS",
    "bits",
    "contains",
    "empty",
    "from_ret",
    "intersects",
    "is_empty",
    "name",
    "to_ret",
];

/// Union methods that would shadow the `GuestLayout` calls the generated code makes.
const RESERVED_UNION_ITEMS: &[&str] = &["read_from", "write_to"];

/// Rejects models whose names would produce uncompilable Rust.
fn check_names(model: &Model) -> Result<()> {
    let mut types = Namespace::new("type", RESERVED_TYPES);
    for t in &model.types {
        types.claim(type_name(&t.name), format!("type `{}`", t.name))?;
        match &t.kind {
            TypeKind::Record(r) if r.is_union => {
                let mut items = Namespace::new("method", RESERVED_UNION_ITEMS);
                for f in &r.fields {
                    let snake = names::snake(&f.name);
                    let origin =
                        |kind: &str| format!("{kind} of union field `{}.{}`", t.name, f.name);
                    items.claim(format!("from_{snake}"), origin("constructor"))?;
                    items.claim(format!("set_{snake}"), origin("setter"))?;
                    items.claim(snake, origin("getter"))?;
                }
            }
            TypeKind::Record(r) => {
                let mut fields = Namespace::new("field", &[]);
                for f in &r.fields {
                    let origin = format!("field `{}.{}`", t.name, f.name);
                    fields.claim(names::snake(&f.name), origin)?;
                }
            }
            TypeKind::Constants(c) => {
                let mut items = Namespace::new("associated item", RESERVED_CONST_ITEMS);
                for m in &c.members {
                    let origin = format!("constant `{}.{}`", t.name, m.name);
                    items.claim(m.c_name.clone(), origin)?;
                }
            }
            TypeKind::Alias(_) | TypeKind::Pointer(_) => {}
        }
    }
    Ok(())
}

fn valtype(v: ValType) -> &'static str {
    match v {
        ValType::I32 => "ValType::I32",
        ValType::I64 => "ValType::I64",
    }
}

impl Gen<'_> {
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn docs(&mut self, docs: &[String]) {
        push_docs(&mut self.out, docs);
    }

    fn docs_with_c(&mut self, t: &TypeDef) {
        self.docs(&t.docs);
        if let Some(c) = &t.c_type {
            if !t.docs.is_empty() {
                self.line("///");
            }
            let _ = writeln!(self.out, "/// C: `{c}`");
        }
    }

    fn prelude(&mut self) {
        let pw = self.model.target.pointer_width;
        let addr = prim(Prim::U32.with_size(pw));
        let mut s = PRELUDE.replace("$ADDR", addr);
        for p in [
            Prim::U8,
            Prim::U16,
            Prim::U32,
            Prim::U64,
            Prim::S8,
            Prim::S16,
            Prim::S32,
            Prim::S64,
        ] {
            s.push_str(
                &INT_LAYOUT
                    .replace("$T", prim(p))
                    .replace("$N", &p.size().to_string()),
            );
        }
        for (name, kind) in [("GuestPtr", "const"), ("GuestPtrMut", "mutable")] {
            s.push_str(
                &PTR.replace("$NAME", name)
                    .replace("$KIND", kind)
                    .replace("$ADDR", addr)
                    .replace("$N", &pw.to_string()),
            );
        }
        self.out.push_str(&s);
    }

    fn type_def(&mut self, t: &TypeDef) {
        match &t.kind {
            TypeKind::Alias(a) if a.newtype => self.newtype(t, a),
            TypeKind::Alias(a) => {
                self.docs_with_c(t);
                let target = match self.model.scalar_prim(&a.target) {
                    Some(p) if a.pointer_sized => prim(p.with_size(t.layout.size)).to_string(),
                    _ => ty(&a.target),
                };
                let _ = writeln!(self.out, "pub type {} = {target};", type_name(&t.name));
            }
            TypeKind::Pointer(p) => self.pointer(t, p),
            TypeKind::Record(r) if r.is_union => self.union(t, r),
            TypeKind::Record(r) => self.record(t, r),
            TypeKind::Constants(c) => self.constants(t, c),
        }
    }

    /// `@newtype`: a distinct type with the alias target's layout.
    fn newtype(&mut self, t: &TypeDef, a: &Alias) {
        let name = type_name(&t.name);
        let repr = ty(&a.target);
        self.docs_with_c(t);
        self.line("#[repr(transparent)]");
        self.line("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]");
        let _ = writeln!(self.out, "pub struct {name}(pub {repr});");
        self.line(&CONST_LAYOUT.replace("$NAME", &name).replace("$R", &repr));
    }

    fn pointer(&mut self, t: &TypeDef, p: &Pointer) {
        self.docs_with_c(t);
        if !t.docs.is_empty() || t.c_type.is_some() {
            self.line("///");
        }
        let what = if p.array { "an array of" } else { "a" };
        let nullable = if p.nullable { " May be null." } else { "" };
        let pointee = p.pointee.as_ref().map_or("void".into(), wit_ty);
        let _ = writeln!(
            self.out,
            "/// Guest pointer to {what} `{pointee}`.{nullable}"
        );
        let wrapper = if p.mutable { "GuestPtrMut" } else { "GuestPtr" };
        let target = p.pointee.as_ref().map_or("core::ffi::c_void".into(), ty);
        let _ = writeln!(
            self.out,
            "pub type {} = {wrapper}<{target}>;",
            type_name(&t.name)
        );
    }

    fn layout_asserts(&mut self, name: &str, t: &TypeDef, r: &Record) {
        let _ = writeln!(self.out, "const _: () = {{");
        let _ = writeln!(
            self.out,
            "assert!(core::mem::size_of::<{name}>() == {});",
            t.layout.size
        );
        let _ = writeln!(
            self.out,
            "assert!(core::mem::align_of::<{name}>() == {});",
            t.layout.align
        );
        if !r.is_union {
            for f in &r.fields {
                let _ = writeln!(
                    self.out,
                    "assert!(core::mem::offset_of!({name}, {}) == {});",
                    ident(&f.name),
                    f.offset
                );
            }
        }
        self.line("};");
    }

    fn layout_impl_head(&mut self, name: &str, t: &TypeDef) {
        let _ = writeln!(self.out, "impl GuestLayout for {name} {{");
        let _ = writeln!(self.out, "const SIZE: usize = {};", t.layout.size);
        let _ = writeln!(self.out, "const ALIGN: usize = {};", t.layout.align);
    }

    fn default_impl(&mut self, name: &str) {
        let _ = writeln!(
            self.out,
            "impl Default for {name} {{ fn default() -> Self {{ <Self as GuestLayout>::ZERO }} }}"
        );
    }

    fn record(&mut self, t: &TypeDef, r: &Record) {
        let name = type_name(&t.name);
        self.docs_with_c(t);
        self.line("#[repr(C)]");
        self.line("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]");
        let _ = writeln!(self.out, "pub struct {name} {{");
        for f in &r.fields {
            self.docs(&f.docs);
            let _ = writeln!(self.out, "pub {}: {},", ident(&f.name), ty(&f.ty));
        }
        self.line("}");
        self.layout_asserts(&name, t, r);
        self.default_impl(&name);

        self.layout_impl_head(&name, t);
        self.line("const ZERO: Self = Self {");
        for f in &r.fields {
            let _ = writeln!(
                self.out,
                "{}: <{} as GuestLayout>::ZERO,",
                ident(&f.name),
                ty(&f.ty)
            );
        }
        self.line("};");
        self.line("fn read_from(bytes: &[u8]) -> Option<Self> {");
        self.line("let bytes = bytes.get(..Self::SIZE)?;");
        self.line("Some(Self {");
        for f in &r.fields {
            let _ = writeln!(
                self.out,
                "{}: GuestLayout::read_from(&bytes[{}..])?,",
                ident(&f.name),
                f.offset
            );
        }
        self.line("})");
        self.line("}");
        self.line("fn write_to(&self, out: &mut [u8]) {");
        self.line("let out = &mut out[..Self::SIZE];");
        self.line("out.fill(0);");
        for f in &r.fields {
            let _ = writeln!(
                self.out,
                "self.{}.write_to(&mut out[{}..]);",
                ident(&f.name),
                f.offset
            );
        }
        self.line("}");
        self.line("}");
    }

    fn union(&mut self, t: &TypeDef, r: &Record) {
        let name = type_name(&t.name);
        let size = t.layout.size;
        self.docs_with_c(t);
        if !t.docs.is_empty() || t.c_type.is_some() {
            self.line("///");
        }
        self.line("/// C union stored as raw bytes; members are accessed by (de)serializing.");
        let _ = writeln!(self.out, "#[repr(C, align({}))]", t.layout.align);
        self.line("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]");
        let _ = writeln!(self.out, "pub struct {name}(pub [u8; {size}]);");
        self.layout_asserts(&name, t, r);
        self.default_impl(&name);

        let _ = writeln!(self.out, "impl {name} {{");
        for f in &r.fields {
            let fty = ty(&f.ty);
            let snake = names::snake(&f.name);
            self.docs(&f.docs);
            let _ = writeln!(self.out, "pub fn {}(&self) -> {fty} {{", ident(&f.name));
            // Members never exceed the union size, so the read cannot fail.
            let _ = writeln!(
                self.out,
                "<{fty} as GuestLayout>::read_from(&self.0).unwrap_or(<{fty} as GuestLayout>::ZERO)"
            );
            self.line("}");
            let _ = writeln!(
                self.out,
                "pub fn set_{snake}(&mut self, value: {fty}) {{ value.write_to(&mut self.0); }}"
            );
            let _ = writeln!(
                self.out,
                "pub fn from_{snake}(value: {fty}) -> Self {{ let mut v = <Self as GuestLayout>::ZERO; v.set_{snake}(value); v }}"
            );
        }
        self.line("}");

        self.layout_impl_head(&name, t);
        let _ = writeln!(self.out, "const ZERO: Self = Self([0; {size}]);");
        self.line("fn read_from(bytes: &[u8]) -> Option<Self> {");
        self.line("Some(Self(GuestLayout::read_from(bytes)?))");
        self.line("}");
        self.line("fn write_to(&self, out: &mut [u8]) { self.0.write_to(out); }");
        self.line("}");
    }

    fn constants(&mut self, t: &TypeDef, c: &Constants) {
        let name = type_name(&t.name);
        let repr = prim(c.repr);
        self.docs_with_c(t);
        self.line("#[repr(transparent)]");
        self.line("#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]");
        let _ = writeln!(self.out, "pub struct {name}(pub {repr});");

        let _ = writeln!(self.out, "impl {name} {{");
        for m in &c.members {
            self.docs(&m.docs);
            // Linux names like `__WALL`, `_NSIG` or `STATX__RESERVED` are kept as they are, though
            // rust-analyzer's case check rejects leading and doubled underscores.
            if m.c_name.chars().any(|ch| ch.is_ascii_lowercase())
                || m.c_name.starts_with('_')
                || m.c_name.contains("__")
            {
                self.line("#[allow(non_upper_case_globals)]");
            }
            let _ = writeln!(
                self.out,
                "pub const {}: Self = Self({});",
                raw(&m.c_name),
                literal(c, m.value)
            );
        }
        self.line("/// All named values, in definition order (aliases included).");
        let _ = writeln!(
            self.out,
            "pub const MEMBERS: &'static [(&'static str, Self)] = &["
        );
        for m in &c.members {
            let _ = writeln!(self.out, "(\"{}\", Self::{}),", m.c_name, raw(&m.c_name));
        }
        self.line("];");
        match c.kind {
            ConstKind::Enum => self.enum_methods(c),
            ConstKind::Flags => self.line(&FLAGS_METHODS.replace("$R", repr)),
        }
        self.line("}");

        if c.abi_error {
            self.abi_error(&name, c);
        }
        match c.kind {
            ConstKind::Enum => self.line(&ENUM_DEBUG.replace("$NAME", &name)),
            ConstKind::Flags => self.line(&FLAGS_TRAITS.replace("$NAME", &name)),
        }
        self.line(&CONST_LAYOUT.replace("$NAME", &name).replace("$R", repr));
    }

    /// The raw return value conversions of the `@abi-error` type.
    fn abi_error(&mut self, name: &str, c: &Constants) {
        let ret = self.model.common_result();
        let ret_ty = ret.map_or_else(|| prim(c.repr).to_string(), ty);
        let ret_prim = ret
            .and_then(|r| self.model.scalar_prim(r))
            .unwrap_or(c.repr);
        let to_ret = match cast_from(c.repr, ret_prim) {
            cast if cast.is_empty() => "self.0".to_string(),
            cast => format!("(self.0{cast})"),
        };
        let from_ret = match cast_from(ret_prim, c.repr) {
            cast if cast.is_empty() => "-ret".to_string(),
            cast => format!("(-ret){cast}"),
        };
        self.out.push_str(
            &ABI_ERROR
                .replace("$NAME", name)
                .replace("$RET", &ret_ty)
                .replace("$TO_RET", &to_ret)
                .replace("$FROM_RET", &from_ret),
        );
    }

    fn enum_methods(&mut self, c: &Constants) {
        self.line("/// Name of the first constant with this value, if any.");
        self.line("pub const fn name(self) -> Option<&'static str> {");
        self.line("match self.0 {");
        let mut seen = Vec::new();
        for m in &c.members {
            // Aliases (EAGAIN == EWOULDBLOCK) would be unreachable patterns.
            if seen.contains(&m.value) {
                continue;
            }
            seen.push(m.value);
            let _ = writeln!(
                self.out,
                "{} => Some(\"{}\"),",
                literal(c, m.value),
                m.c_name
            );
        }
        self.line("_ => None,");
        self.line("}");
        self.line("}");
    }

    fn syscalls(&mut self) {
        self.line(SYSCALL_TYPES);
        self.line("/// Every function imported by guests, across all import modules.");
        self.line("pub const SYSCALLS: &[SyscallDef] = &[");
        for iface in &self.model.interfaces {
            let module = iface.module.clone().unwrap_or_default();
            for f in &iface.functions {
                self.syscall(&module, f);
            }
        }
        self.line("];");
    }

    fn syscall(&mut self, module: &str, f: &Function) {
        let model = self.model;
        let nr = f.nr.map_or("None".into(), |n| format!("Some({n})"));
        let result = f
            .result
            .as_ref()
            .and_then(|r| model.valtype(r))
            .map_or("None".into(), |v| format!("Some({})", valtype(v)));
        let _ = writeln!(
            self.out,
            "SyscallDef {{ name: \"{}\", module: \"{module}\", import_name: \"{}\", nr: {nr},",
            names::snake(&f.name),
            f.import_name
        );
        self.line("params: &[");
        for p in &f.params {
            let v = model.valtype(&p.ty).expect("validated scalar param");
            let _ = writeln!(
                self.out,
                "SyscallParam {{ name: \"{}\", ty: {}, wit_type: \"{}\" }},",
                names::snake(&p.name),
                valtype(v),
                wit_ty(&p.ty)
            );
        }
        self.line("],");
        let merged: Vec<String> = f
            .merged_i64
            .iter()
            .map(|m| format!("\"{}\"", names::snake(&m.param)))
            .collect();
        let _ = writeln!(
            self.out,
            "result: {result}, noreturn: {}, unsupported: {}, merged_i64: &[{}] }},",
            f.noreturn,
            f.unsupported,
            merged.join(", ")
        );
    }
}

/// `" as to"` when converting from `from` to `to` needs a cast.
fn cast_from(from: Prim, to: Prim) -> String {
    if from == to {
        String::new()
    } else {
        format!(" as {}", prim(to))
    }
}

/// Flags read best in hex, enums in decimal.
fn literal(c: &Constants, value: i128) -> String {
    match c.kind {
        ConstKind::Flags if value >= 0 => format!("{value:#x}"),
        _ => value.to_string(),
    }
}

const PRELUDE: &str = r#"
use core::fmt;
use core::marker::PhantomData;

/// A value with a fixed guest (wasm32, little-endian) memory layout.
///
/// `read_from` and `write_to` (de)serialize explicitly, so values never alias guest memory.
pub trait GuestLayout: Copy {
    /// Size in guest memory, in bytes.
    const SIZE: usize;
    /// Alignment in guest memory, in bytes.
    const ALIGN: usize;
    /// The all-zeroes value.
    const ZERO: Self;
    /// Reads a value from the start of `bytes`; `None` if `bytes` is shorter than `SIZE`.
    fn read_from(bytes: &[u8]) -> Option<Self>;
    /// Writes the value to the start of `out`, zeroing padding.
    ///
    /// Panics if `out` is shorter than `SIZE`.
    fn write_to(&self, out: &mut [u8]);
}

impl<T: GuestLayout, const N: usize> GuestLayout for [T; N] {
    const SIZE: usize = T::SIZE * N;
    const ALIGN: usize = T::ALIGN;
    const ZERO: Self = [T::ZERO; N];
    fn read_from(bytes: &[u8]) -> Option<Self> {
        let bytes = bytes.get(..Self::SIZE)?;
        let mut out = Self::ZERO;
        for (item, chunk) in out.iter_mut().zip(bytes.chunks_exact(T::SIZE)) {
            *item = T::read_from(chunk)?;
        }
        Some(out)
    }
    fn write_to(&self, out: &mut [u8]) {
        for (item, chunk) in self.iter().zip(out[..Self::SIZE].chunks_exact_mut(T::SIZE)) {
            item.write_to(chunk);
        }
    }
}
"#;

const INT_LAYOUT: &str = r#"
impl GuestLayout for $T {
    const SIZE: usize = $N;
    const ALIGN: usize = $N;
    const ZERO: Self = 0;
    fn read_from(bytes: &[u8]) -> Option<Self> {
        Some(Self::from_le_bytes(bytes.get(..$N)?.try_into().ok()?))
    }
    fn write_to(&self, out: &mut [u8]) {
        out[..$N].copy_from_slice(&self.to_le_bytes());
    }
}
"#;

const PTR: &str = r#"
/// A $KIND guest pointer to `T`: a guest address, never dereferenced directly.
#[repr(transparent)]
pub struct $NAME<T>(pub $ADDR, PhantomData<fn() -> T>);

impl<T> $NAME<T> {
    pub const NULL: Self = Self(0, PhantomData);
    pub const fn new(addr: $ADDR) -> Self {
        Self(addr, PhantomData)
    }
    pub const fn addr(self) -> $ADDR {
        self.0
    }
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl<T> Clone for $NAME<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for $NAME<T> {}
impl<T> PartialEq for $NAME<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl<T> Eq for $NAME<T> {}
impl<T> core::hash::Hash for $NAME<T> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}
impl<T> Default for $NAME<T> {
    fn default() -> Self {
        Self::NULL
    }
}
impl<T> fmt::Debug for $NAME<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "$NAME({:#x})", self.0)
    }
}
impl<T> GuestLayout for $NAME<T> {
    const SIZE: usize = $N;
    const ALIGN: usize = $N;
    const ZERO: Self = Self::NULL;
    fn read_from(bytes: &[u8]) -> Option<Self> {
        <$ADDR as GuestLayout>::read_from(bytes).map(Self::new)
    }
    fn write_to(&self, out: &mut [u8]) {
        self.0.write_to(out);
    }
}
"#;

const FLAGS_METHODS: &str = r#"
pub const fn empty() -> Self {
    Self(0)
}
pub const fn bits(self) -> $R {
    self.0
}
pub const fn is_empty(self) -> bool {
    self.0 == 0
}
/// Whether all bits of `other` are set.
pub const fn contains(self, other: Self) -> bool {
    self.0 & other.0 == other.0
}
/// Whether any bit of `other` is set.
pub const fn intersects(self, other: Self) -> bool {
    self.0 & other.0 != 0
}
"#;

const ENUM_DEBUG: &str = r#"
impl fmt::Debug for $NAME {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "$NAME({})", self.0),
        }
    }
}
"#;

const FLAGS_TRAITS: &str = r#"
impl fmt::Debug for $NAME {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("$NAME(")?;
        let mut rest = self.0;
        let mut first = true;
        for (name, flag) in Self::MEMBERS {
            if flag.0 != 0 && self.contains(*flag) && rest & flag.0 != 0 {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                first = false;
                rest &= !flag.0;
            }
        }
        if rest != 0 || first {
            if !first {
                f.write_str(" | ")?;
            }
            write!(f, "{rest:#x}")?;
        }
        f.write_str(")")
    }
}
impl core::ops::BitOr for $NAME {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl core::ops::BitAnd for $NAME {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}
impl core::ops::BitXor for $NAME {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}
impl core::ops::Not for $NAME {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}
impl core::ops::BitOrAssign for $NAME {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
impl core::ops::BitAndAssign for $NAME {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}
impl core::ops::BitXorAssign for $NAME {
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0 ^= rhs.0;
    }
}
"#;

const CONST_LAYOUT: &str = r#"
impl GuestLayout for $NAME {
    const SIZE: usize = <$R as GuestLayout>::SIZE;
    const ALIGN: usize = <$R as GuestLayout>::ALIGN;
    const ZERO: Self = Self(0);
    fn read_from(bytes: &[u8]) -> Option<Self> {
        <$R as GuestLayout>::read_from(bytes).map(Self)
    }
    fn write_to(&self, out: &mut [u8]) {
        self.0.write_to(out);
    }
}
"#;

const ABI_ERROR: &str = r#"
impl $NAME {
    /// Encodes the error as a raw syscall return value (`-errno`).
    pub const fn to_ret(self) -> $RET {
        $TO_RET.wrapping_neg()
    }
    /// Decodes a raw syscall return value: `Some` for Linux's error range `-4095..=-1`.
    pub fn from_ret(ret: $RET) -> Option<Self> {
        if (-4095..=-1).contains(&ret) {
            Some(Self($FROM_RET))
        } else {
            None
        }
    }
}
"#;

const SYSCALL_TYPES: &str = r#"
/// Wasm value type of an import param or result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValType {
    I32,
    I64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SyscallParam {
    pub name: &'static str,
    pub ty: ValType,
    /// The WIT type the param is declared with.
    pub wit_type: &'static str,
}

/// Metadata of one imported function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SyscallDef {
    /// Linux (snake_case) name.
    pub name: &'static str,
    /// Wasm import module.
    pub module: &'static str,
    /// Wasm import name.
    pub import_name: &'static str,
    /// riscv32 syscall number, if this is a Linux syscall.
    pub nr: Option<u32>,
    pub params: &'static [SyscallParam],
    pub result: Option<ValType>,
    pub noreturn: bool,
    /// Declared for completeness; hosts may return `-ENOSYS`.
    pub unsupported: bool,
    /// Params riscv32 splits into a register pair but wasm passes as one i64.
    pub merged_i64: &'static [&'static str],
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Target;
    use crate::parse::parse_str;

    fn gen_err(body: &str) -> String {
        let src = format!("package t:t@0.1.0;\ninterface types {{\n{body}\n}}\n");
        let model = parse_str("t.wit", &src, Target::WASM32).unwrap();
        match generate(&model, "t.wit") {
            Ok(_) => panic!("expected a generator error for:\n{body}"),
            Err(e) => format!("{e:#}"),
        }
    }

    #[test]
    fn unusable_identifiers_are_rejected() {
        for field in ["self", "crate", "super"] {
            let e = gen_err(&format!("record r {{ {field}: u32 }}"));
            assert!(e.contains("not a usable identifier"), "{e}");
        }
        let e = gen_err("record self { a: u32 }");
        assert!(e.contains("Rust type `Self`"), "{e}");
        for name in ["Self", "_"] {
            let e = gen_err(&format!(
                "/// @repr(u32)\nflags f {{\n/// @value(1)\n/// @name({name})\na }}"
            ));
            assert!(e.contains("not a usable identifier"), "{e}");
        }
    }

    #[test]
    fn reserved_names_are_rejected() {
        let e = gen_err("/// @repr(u32)\nenum e {\n/// @value(1)\nmembers }");
        assert!(e.contains("`MEMBERS`, which is reserved"), "{e}");
        let e = gen_err("/// @repr(u32)\nenum e {\n/// @value(1)\n/// @name(name)\na }");
        assert!(e.contains("`name`, which is reserved"), "{e}");
        for ty in [
            "guest-layout",
            "guest-ptr",
            "val-type",
            "syscall-def",
            "default",
            "%option",
        ] {
            let e = gen_err(&format!("record {ty} {{ a: u32 }}"));
            assert!(e.contains("reserved by generated code"), "{ty}: {e}");
        }
        let e = gen_err("/// @union\nrecord u { write-to: u32 }");
        assert!(e.contains("`write_to`, which is reserved"), "{e}");
    }

    #[test]
    fn case_conversion_collisions_are_rejected() {
        let e = gen_err("record x-1 { a: u32 }\nrecord x1 { a: u32 }");
        assert!(
            e.contains("type `x1` and type `x-1` both map to Rust type `X1`"),
            "{e}"
        );
        let e = gen_err("record r { ab-c: u32, AB-c: u32 }");
        assert!(e.contains("both map to Rust field `ab_c`"), "{e}");
        let e = gen_err("/// @union\nrecord u { x: u32, set-x: u8 }");
        assert!(e.contains("both map to Rust method `set_x`"), "{e}");
    }

    #[test]
    fn keyword_constant_names_are_escaped() {
        let src = "package t:t@0.1.0;\ninterface types {\n/// @repr(u32)\nenum e {\n/// @value(1)\n/// @name(type)\na }\n}\n";
        let model = parse_str("t.wit", src, Target::WASM32).unwrap();
        let out = generate(&model, "t.wit").unwrap();
        assert!(out.contains("pub const r#type: Self = Self(1);"), "{out}");
        assert!(out.contains("(\"type\", Self::r#type)"), "{out}");
    }
}
