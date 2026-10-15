//! Shapes each WIT function into the `Syscalls` method the [`crate::syscalls`] and
//! [`crate::exec`] backends emit: `@slice` pairs become one user slice, `@returns` types the
//! success value, and every raw wasm param is tracked so the import shim can decode it.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

use crate::ir::{Function, Model, Prim, Target, TyRef, TypeKind, ValType};
use crate::rust::{Namespace, cast, ident, prim, ty, type_name};

/// Locals and free functions of the generated code that params and methods must not shadow.
const RESERVED_PARAMS: &[&str] = &["call", "caller", "env", "memory", "ret", "syscalls", "task"];
const RESERVED_METHODS: &[&str] = &["caller_parts", "dispatch", "imports", "mismatch"];

/// One import as a `Syscalls` method.
pub struct Method<'a> {
    pub f: &'a Function,
    pub module: &'a str,
    /// Trait method and import shim name (snake case).
    pub name: String,
    /// `Syscall` and `SyscallRet` variant.
    pub variant: String,
    /// The wasm params, in import order.
    pub raw: Vec<RawParam>,
    /// The trait method params, in import order of their first raw param.
    pub params: Vec<Param>,
    /// Type of a successful result; `None` for `@noreturn` functions.
    pub ret: Option<TyRef>,
    /// Wasm type of the import's result (`i32` or `i64`); `S32` for `@noreturn` functions.
    pub raw_ret: Prim,
}

pub struct RawParam {
    pub name: String,
    pub ty: TyRef,
    /// `i32` or `i64`.
    pub prim: Prim,
}

pub enum Param {
    Value {
        name: String,
        ty: TyRef,
        raw: usize,
    },
    /// `@slice`: a `UserSlice` (`mutable == false`) or `UserSliceMut`.
    Slice {
        name: String,
        mutable: bool,
        ptr: usize,
        len: usize,
    },
}

impl Param {
    pub fn name(&self) -> &str {
        match self {
            Param::Value { name, .. } | Param::Slice { name, .. } => name,
        }
    }
}

/// How a scalar crosses the wasm boundary.
pub enum Repr {
    Int(Prim),
    /// A `GuestPtr`/`GuestPtrMut` alias.
    Ptr,
    /// A newtype or constant family, built with the tuple constructor of the named type.
    Wrapped {
        ctor: String,
        prim: Prim,
    },
}

pub fn repr(model: &Model, t: &TyRef) -> Repr {
    let TyRef::Named(n) = t else {
        return Repr::Int(model.scalar_prim(t).expect("validated scalar"));
    };
    let def = model.type_def(n).expect("validated type");
    match &def.kind {
        TypeKind::Alias(a) if a.newtype => Repr::Wrapped {
            ctor: type_name(n),
            prim: model.scalar_prim(&a.target).expect("validated scalar"),
        },
        TypeKind::Alias(a) if !a.pointer_sized => repr(model, &a.target),
        TypeKind::Pointer(_) => Repr::Ptr,
        TypeKind::Constants(c) => Repr::Wrapped {
            ctor: type_name(n),
            prim: c.repr,
        },
        _ => Repr::Int(model.scalar_prim(t).expect("validated scalar")),
    }
}

/// Every function of every interface, shaped. All functions must share one import module.
pub fn methods(model: &Model) -> Result<Vec<Method<'_>>> {
    if model.target != Target::WASM32 {
        bail!("the Rust host backends only support wasm32 (user slices hold 32-bit addresses)");
    }
    let mut names = Namespace::new("method", RESERVED_METHODS);
    let mut variants = Namespace::new("variant", &[]);
    let mut module: Option<&str> = None;
    let mut out = Vec::new();
    for iface in &model.interfaces {
        for f in &iface.functions {
            let m = iface.module.as_deref().unwrap_or_default();
            match module {
                Some(prev) if prev != m => {
                    bail!("functions are imported from both `{prev}` and `{m}`; use one module")
                }
                _ => module = Some(m),
            }
            let origin = format!("function `{}.{}`", iface.name, f.name);
            let method = method(model, m, f)?;
            names.claim(method.name.clone(), origin.clone())?;
            variants.claim(method.variant.clone(), origin)?;
            out.push(method);
        }
    }
    Ok(out)
}

fn method<'a>(model: &Model, module: &'a str, f: &'a Function) -> Result<Method<'a>> {
    let raw = raw_params(model, f)?;
    let index = |name: &str| {
        f.params
            .iter()
            .position(|p| p.name == name)
            .expect("validated")
    };
    let mut params = Vec::new();
    for (i, p) in f.params.iter().enumerate() {
        if let Some(s) = f.slices.iter().find(|s| s.ptr == p.name) {
            let TypeKind::Pointer(ptr) = &model
                .type_def(&type_ref_name(&p.ty))
                .expect("validated")
                .kind
            else {
                unreachable!("`@slice` pointers are validated");
            };
            params.push(Param::Slice {
                name: raw[i].name.clone(),
                mutable: ptr.mutable,
                ptr: i,
                len: index(&s.len),
            });
        } else if !f.slices.iter().any(|s| s.len == p.name) {
            params.push(Param::Value {
                name: raw[i].name.clone(),
                ty: p.ty.clone(),
                raw: i,
            });
        }
    }
    let (ret, raw_ret) = match (&f.result, f.noreturn) {
        (_, true) => (None, Prim::S32),
        (None, false) => bail!("function `{}` has no result but is not `@noreturn`", f.name),
        (Some(r), false) => {
            let raw_ret = match model.valtype(r).expect("validated scalar") {
                ValType::I32 => Prim::S32,
                ValType::I64 => Prim::S64,
            };
            let ret = match &f.returns {
                Some(t) => t.clone(),
                None => TyRef::Prim(model.scalar_prim(r).expect("validated scalar")),
            };
            (Some(ret), raw_ret)
        }
    };
    Ok(Method {
        f,
        module,
        name: ident(&f.name),
        variant: type_name(&f.name),
        raw,
        params,
        ret,
        raw_ret,
    })
}

fn type_ref_name(t: &TyRef) -> String {
    match t {
        TyRef::Named(n) => n.clone(),
        _ => unreachable!("`@slice` pointers are named types"),
    }
}

/// Snake-cased raw params; names of generated locals get a `_` suffix.
fn raw_params(model: &Model, f: &Function) -> Result<Vec<RawParam>> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for p in &f.params {
        let mut name = ident(&p.name);
        if RESERVED_PARAMS.contains(&name.as_str()) {
            name.push('_');
        }
        if !seen.insert(name.clone()) {
            bail!(
                "function `{}`: param `{}` maps to duplicate `{name}`",
                f.name,
                p.name
            );
        }
        let prim = match model.valtype(&p.ty).expect("validated scalar") {
            ValType::I32 => Prim::S32,
            ValType::I64 => Prim::S64,
        };
        out.push(RawParam {
            name,
            ty: p.ty.clone(),
            prim,
        });
    }
    Ok(out)
}

impl Method<'_> {
    /// The trait method's Rust return type.
    pub fn result_type(&self) -> String {
        match &self.ret {
            None => "SysResult<()>".to_string(),
            Some(t) => format!("SysResult<{}>", ty(t)),
        }
    }

    /// `Ok` payload type, `None` for `()`.
    pub fn ret_type(&self) -> Option<String> {
        self.ret.as_ref().map(ty)
    }

    /// `name: Type` for each trait param.
    pub fn typed_params(&self) -> Vec<String> {
        self.params
            .iter()
            .map(|p| match p {
                Param::Value { name, ty: t, .. } => format!("{name}: {}", ty(t)),
                Param::Slice { name, mutable, .. } => format!("{name}: {}", slice_type(*mutable)),
            })
            .collect()
    }

    pub fn param_names(&self) -> Vec<&str> {
        self.params.iter().map(Param::name).collect()
    }

    /// Clippy counts `self` and `caller` too; its default limit is 7.
    pub fn too_many_args(&self) -> bool {
        self.params.len() + 2 > 7
    }

    /// The doc line naming the import.
    pub fn import_doc(&self) -> String {
        let nr = self
            .f
            .nr
            .map_or(String::new(), |n| format!(" (riscv32 nr {n})"));
        format!(
            "/// Wasm import `{}.{}`{nr}.",
            self.module, self.f.import_name
        )
    }
}

pub fn slice_type(mutable: bool) -> &'static str {
    if mutable { "UserSliceMut" } else { "UserSlice" }
}

/// Rust types the trait signatures name, for the `use` list.
pub fn signature_types(methods: &[Method]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut add = |t: &TyRef| {
        if let TyRef::Named(n) = t {
            out.insert(type_name(n));
        }
    };
    for m in methods {
        for p in &m.params {
            if let Param::Value { ty, .. } = p {
                add(ty);
            }
        }
        if let Some(r) = &m.ret {
            add(r);
        }
    }
    out
}

/// Whether any method takes a `UserSlice` (`false`) or `UserSliceMut` (`true`).
pub fn uses_slice(methods: &[Method], mutable: bool) -> bool {
    methods.iter().any(|m| {
        m.params
            .iter()
            .any(|p| matches!(p, Param::Slice { mutable: x, .. } if *x == mutable))
    })
}

/// Rust expression turning raw wasm param `r` into its typed value.
pub fn decode(model: &Model, r: &RawParam) -> String {
    match repr(model, &r.ty) {
        Repr::Int(p) => cast(&r.name, r.prim, p),
        Repr::Ptr => format!("{}::new({})", ty(&r.ty), cast(&r.name, r.prim, Prim::U32)),
        Repr::Wrapped { ctor, prim } => format!("{ctor}({})", cast(&r.name, r.prim, prim)),
    }
}

/// Rust expression turning the typed success value `v` into the raw `to` result, or `None` when
/// `v` already is one.
pub fn encode(model: &Model, t: &TyRef, v: &str, to: Prim) -> Option<String> {
    let expr = match repr(model, t) {
        Repr::Int(p) => cast(v, p, to),
        Repr::Ptr => format!("{v}.addr() as {}", prim(to)),
        Repr::Wrapped { prim: p, .. } => cast(&format!("{v}.0"), p, to),
    };
    (expr != v).then_some(expr)
}

/// Constructors the decode expressions call, for the `use` list.
pub fn decode_types(model: &Model, methods: &[Method]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for m in methods {
        for p in &m.params {
            let Param::Value { raw, .. } = p else {
                continue;
            };
            match repr(model, &m.raw[*raw].ty) {
                Repr::Int(_) => {}
                Repr::Ptr => {
                    out.insert(ty(&m.raw[*raw].ty));
                }
                Repr::Wrapped { ctor, .. } => {
                    out.insert(ctor);
                }
            }
        }
    }
    out
}
