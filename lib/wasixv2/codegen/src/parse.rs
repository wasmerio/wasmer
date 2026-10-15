//! WIT (via wit-parser) -> [`Model`].

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use wit_parser::{FunctionKind, PackageId, Resolve, Span, Type, TypeDefKind, TypeId};

use crate::annot::{Annotations, parse_docs, parse_int, parse_u32};
use crate::ir::{
    Alias, ConstKind, Constant, Constants, Field, Function, Interface, Layout, MergedI64, Model,
    Param, Pointer, Prim, Record, SliceSpec, SplitOrder, Target, TyRef, TypeDef, TypeKind,
};

/// Parses a `.wit` file or a directory of `.wit` files.
pub fn parse_path(path: &Path, target: Target) -> Result<Model> {
    let mut resolve = Resolve::default();
    let pkg = match resolve.push_path(path) {
        Ok((pkg, _)) => pkg,
        Err(e) => bail!("{}", resolve.render_error(&e)),
    };
    Builder::new(&resolve, target).build(pkg)
}

/// Parses WIT source text; `file_name` is only used in diagnostics.
pub fn parse_str(file_name: &str, src: &str, target: Target) -> Result<Model> {
    let mut resolve = Resolve::default();
    let pkg = match resolve.push_str(file_name, src) {
        Ok(pkg) => pkg,
        Err(e) => bail!("{}", resolve.render_error(&e)),
    };
    Builder::new(&resolve, target).build(pkg)
}

/// Facts only needed during validation, keyed by type name.
#[derive(Default)]
struct Pending {
    locations: HashMap<String, String>,
    expected: HashMap<String, (Option<u32>, Option<u32>)>,
    ptr_bases: Vec<(String, TyRef)>,
    function_locations: HashMap<(String, String), String>,
}

struct Builder<'a> {
    r: &'a Resolve,
    target: Target,
}

impl<'a> Builder<'a> {
    fn new(r: &'a Resolve, target: Target) -> Self {
        Self { r, target }
    }

    fn loc(&self, span: Span, what: &str) -> String {
        format!("{} ({what})", self.r.render_location(span))
    }

    fn build(&self, pkg_id: PackageId) -> Result<Model> {
        let pkg = &self.r.packages[pkg_id];
        let (_, ann) = parse_docs(
            pkg.docs.contents.as_deref(),
            &format!("package {}", pkg.name),
        )?;
        ann.finish("a package")?;

        let mut pending = Pending::default();
        let mut types = Vec::new();
        let mut interfaces = Vec::new();
        for (iname, iid) in &pkg.interfaces {
            let iface = &self.r.interfaces[*iid];
            for tid in iface.types.values() {
                if self.is_use_copy(*tid) {
                    continue;
                }
                let def = self.type_def(iname, *tid, &mut pending)?;
                if types.iter().any(|t: &TypeDef| t.name == def.name) {
                    bail!(
                        "{}: duplicate type name `{}`",
                        pending.locations[&def.name],
                        def.name
                    );
                }
                types.push(def);
            }
            interfaces.push(self.interface(iname, iface, &mut pending)?);
        }

        let mut model = Model {
            package: pkg.name.to_string(),
            target: self.target,
            types,
            interfaces,
        };
        validate_refs(&model, &pending)?;
        compute_layouts(&mut model, &pending)?;
        validate_functions(&model, &pending)?;
        validate_abi_error(&model, &pending)?;
        Ok(model)
    }

    /// `use iface.{t}` creates an alias TypeDef in the using interface; we want the original.
    fn is_use_copy(&self, id: TypeId) -> bool {
        let td = &self.r.types[id];
        match td.kind {
            TypeDefKind::Type(Type::Id(orig)) => {
                let o = &self.r.types[orig];
                // A different owner means `use`, possibly renamed (`use t.{a as b}`).
                o.owner != td.owner
            }
            _ => false,
        }
    }

    fn canonical(&self, mut id: TypeId) -> TypeId {
        while self.is_use_copy(id) {
            let TypeDefKind::Type(Type::Id(orig)) = self.r.types[id].kind else {
                unreachable!()
            };
            id = orig;
        }
        id
    }

    fn ty(&self, t: &Type, loc: &str) -> Result<TyRef> {
        let prim = match t {
            Type::U8 => Prim::U8,
            Type::U16 => Prim::U16,
            Type::U32 => Prim::U32,
            Type::U64 => Prim::U64,
            Type::S8 => Prim::S8,
            Type::S16 => Prim::S16,
            Type::S32 => Prim::S32,
            Type::S64 => Prim::S64,
            Type::Id(id) => return self.ty_id(*id, loc),
            other => bail!("{loc}: unsupported type `{other:?}` (only integers and named types)"),
        };
        Ok(TyRef::Prim(prim))
    }

    fn ty_id(&self, id: TypeId, loc: &str) -> Result<TyRef> {
        let td = &self.r.types[self.canonical(id)];
        if let Some(name) = &td.name {
            return Ok(TyRef::Named(name.clone()));
        }
        self.ty_kind(&td.kind, loc)
    }

    /// Converts an anonymous type (only fixed-size lists are supported).
    fn ty_kind(&self, kind: &TypeDefKind, loc: &str) -> Result<TyRef> {
        match kind {
            TypeDefKind::FixedLengthList(elem, 0) => {
                bail!("{loc}: zero-length list<{elem:?}, 0> is not allowed")
            }
            TypeDefKind::FixedLengthList(elem, n) => {
                Ok(TyRef::Array(Box::new(self.ty(elem, loc)?), *n))
            }
            other => bail!("{loc}: unsupported anonymous type {other:?}"),
        }
    }

    fn type_def(&self, iface: &str, id: TypeId, pending: &mut Pending) -> Result<TypeDef> {
        let td = &self.r.types[id];
        let name = td.name.clone().unwrap_or_default();
        let loc = self.loc(td.span, &format!("type `{name}`"));
        pending.locations.insert(name.clone(), loc.clone());
        let (docs, mut ann) = parse_docs(td.docs.contents.as_deref(), &loc)?;
        let c_type = ann.value("c")?;
        let (kind, placement) = match &td.kind {
            TypeDefKind::Type(t) => {
                let target = self.ty(t, &loc)?;
                (alias(&name, target, &mut ann, pending)?, "a type alias")
            }
            // `type x = list<T, N>` is stored as the list itself, not as an alias.
            TypeDefKind::FixedLengthList(..) => {
                let target = self.ty_kind(&td.kind, &loc)?;
                (alias(&name, target, &mut ann, pending)?, "a type alias")
            }
            TypeDefKind::Record(rec) => {
                let size = ann
                    .value("size")?
                    .map(|s| parse_u32(&s, &loc))
                    .transpose()?;
                let align = ann
                    .value("align")?
                    .map(|s| parse_u32(&s, &loc))
                    .transpose()?;
                pending.expected.insert(name.clone(), (size, align));
                let is_union = ann.flag("union")?;
                let kernel_only = ann.flag("kernel-only")?;
                let kind = self.record(&name, rec, is_union, kernel_only, &loc)?;
                (kind, "a record")
            }
            TypeDefKind::Enum(e) => {
                let members = e.cases.iter().map(|c| (&c.name, &c.docs, c.span));
                let consts = self.constants(ConstKind::Enum, members, &mut ann)?;
                (consts, "an enum")
            }
            TypeDefKind::Flags(f) => {
                let members = f.flags.iter().map(|m| (&m.name, &m.docs, m.span));
                let consts = self.constants(ConstKind::Flags, members, &mut ann)?;
                (consts, "a flags type")
            }
            other => bail!("{loc}: unsupported type kind {other:?}"),
        };
        ann.finish(placement)?;
        Ok(TypeDef {
            name,
            interface: iface.to_string(),
            docs,
            c_type,
            kind,
            layout: Layout { size: 0, align: 0 },
        })
    }

    fn record(
        &self,
        name: &str,
        rec: &wit_parser::Record,
        is_union: bool,
        kernel_only: bool,
        loc: &str,
    ) -> Result<TypeKind> {
        if rec.fields.is_empty() {
            bail!("{loc}: empty records are not allowed");
        }
        let mut fields = Vec::new();
        for f in &rec.fields {
            let floc = self.loc(f.span, &format!("field `{name}.{}`", f.name));
            let (docs, mut ann) = parse_docs(f.docs.contents.as_deref(), &floc)?;
            let c_name = ann.value("c-name")?;
            let c_skip = ann.flag("c-skip")?;
            ann.finish("a record field")?;
            fields.push(Field {
                name: f.name.clone(),
                c_name,
                c_skip,
                docs,
                ty: self.ty(&f.ty, &floc)?,
                offset: 0,
                layout: Layout { size: 0, align: 0 },
            });
        }
        Ok(TypeKind::Record(Record {
            is_union,
            kernel_only,
            fields,
        }))
    }

    fn constants<'m>(
        &self,
        kind: ConstKind,
        members: impl Iterator<Item = (&'m String, &'m wit_parser::Docs, Span)>,
        ann: &mut Annotations,
    ) -> Result<TypeKind> {
        // Only enums carry `@abi-error`; elsewhere `finish` reports it as misplaced.
        let abi_error = kind == ConstKind::Enum && ann.flag("abi-error")?;
        let c_enum = ann.flag("c-enum")?;
        let repr_name = ann.required_value("repr")?;
        let repr = Prim::from_wit_name(&repr_name)
            .ok_or_else(|| anyhow!("{}: invalid @repr({repr_name})", ann.location()))?;
        let (min, max) = repr.range();
        let mut out = Vec::new();
        for (name, docs, span) in members {
            let mloc = self.loc(span, &format!("constant `{name}`"));
            let (docs, mut mann) = parse_docs(docs.contents.as_deref(), &mloc)?;
            let value = parse_int(&mann.required_value("value")?, &mloc)?;
            let c_name = match mann.value("name")? {
                Some(n) if is_c_ident(&n) => n,
                Some(n) => bail!("{mloc}: `@name({n})` is not a valid C identifier"),
                None => crate::ir::names::screaming(name),
            };
            let c_skip = mann.flag("c-skip")?;
            mann.finish("a constant")?;
            if value < min || value > max {
                bail!("{mloc}: value {value} does not fit @repr({repr_name})");
            }
            if out.iter().any(|c: &Constant| c.c_name == c_name) {
                bail!("{mloc}: duplicate constant name `{c_name}`");
            }
            out.push(Constant {
                name: name.clone(),
                c_name,
                c_skip,
                docs,
                value,
            });
        }
        if abi_error && !repr.is_signed() {
            bail!("{}: `@abi-error` needs a signed @repr", ann.location());
        }
        Ok(TypeKind::Constants(Constants {
            kind,
            abi_error,
            c_enum,
            repr,
            members: out,
        }))
    }

    fn interface(
        &self,
        name: &str,
        iface: &wit_parser::Interface,
        pending: &mut Pending,
    ) -> Result<Interface> {
        let loc = self.loc(iface.span, &format!("interface `{name}`"));
        let (docs, mut ann) = parse_docs(iface.docs.contents.as_deref(), &loc)?;
        let module = ann.value("module")?;
        ann.finish("an interface")?;
        if module.is_none() && !iface.functions.is_empty() {
            bail!("{loc}: interfaces with functions need `@module(...)`");
        }
        let mut functions = Vec::new();
        for f in iface.functions.values() {
            let floc = self.loc(f.span, &format!("function `{}`", f.name));
            pending
                .function_locations
                .insert((name.to_string(), f.name.clone()), floc.clone());
            functions.push(self.function(f, &floc)?);
        }
        Ok(Interface {
            name: name.to_string(),
            docs,
            module,
            functions,
        })
    }

    fn function(&self, f: &wit_parser::Function, loc: &str) -> Result<Function> {
        if !matches!(f.kind, FunctionKind::Freestanding) {
            bail!("{loc}: only freestanding functions are supported");
        }
        let (docs, mut ann) = parse_docs(f.docs.contents.as_deref(), loc)?;
        let nr = ann.value("nr")?.map(|s| parse_u32(&s, loc)).transpose()?;
        let noreturn = ann.flag("noreturn")?;
        let unsupported = ann.flag("unsupported")?;
        let import_name = ann
            .value("import")?
            .unwrap_or_else(|| crate::ir::names::snake(&f.name));
        let sys_name = match ann.value("sys-name")? {
            Some(n) if is_c_ident(&n) => Some(n),
            Some(n) => bail!("{loc}: `@sys-name({n})` is not a valid C identifier"),
            None => None,
        };
        let merged_i64 = ann
            .values("merged-i64")?
            .into_iter()
            .map(|s| parse_merged_i64(&s, loc))
            .collect::<Result<_>>()?;
        let c_one_slot = ann
            .values("c-one-slot")?
            .into_iter()
            .map(|s| s.trim().to_string())
            .collect();
        let c_params = ann
            .values("c-param")?
            .into_iter()
            .map(|s| parse_c_param(&s, loc))
            .collect::<Result<_>>()?;
        let slices = ann
            .values("slice")?
            .into_iter()
            .map(|s| parse_slice(&s, loc))
            .collect::<Result<_>>()?;
        let returns = ann
            .value("returns")?
            .map(|t| parse_type_name(&t, loc))
            .transpose()?;
        ann.finish("a function")?;
        let params = f
            .params
            .iter()
            .map(|p| {
                Ok(Param {
                    name: p.name.clone(),
                    ty: self.ty(&p.ty, loc)?,
                })
            })
            .collect::<Result<_>>()?;
        let result = f.result.as_ref().map(|t| self.ty(t, loc)).transpose()?;
        Ok(Function {
            name: f.name.clone(),
            import_name,
            docs,
            nr,
            sys_name,
            noreturn,
            unsupported,
            merged_i64,
            c_one_slot,
            c_params,
            slices,
            returns,
            params,
            result,
        })
    }
}

fn alias(name: &str, target: TyRef, ann: &mut Annotations, p: &mut Pending) -> Result<TypeKind> {
    let pointer_sized = ann.flag("pointer-sized")?;
    let nullable = ann.flag("nullable")?;
    let newtype = ann.flag("newtype")?;
    let loc = ann.location().to_string();
    if newtype && (pointer_sized || !matches!(target, TyRef::Prim(_))) {
        bail!(
            "{loc}: `@newtype` aliases must alias a fixed-width integer, without `@pointer-sized`"
        );
    }
    if let Some(spec) = ann.value("ptr")? {
        if pointer_sized || newtype {
            bail!("{loc}: `@ptr` excludes `@pointer-sized` and `@newtype`");
        }
        p.ptr_bases.push((name.to_string(), target));
        return parse_ptr(&spec, nullable, &loc).map(TypeKind::Pointer);
    }
    if nullable {
        bail!("{loc}: `@nullable` requires `@ptr(...)`");
    }
    if pointer_sized && !matches!(target, TyRef::Prim(Prim::U32 | Prim::S32)) {
        bail!("{loc}: `@pointer-sized` aliases must be declared as u32 or s32");
    }
    Ok(TypeKind::Alias(Alias {
        target,
        pointer_sized,
        newtype,
    }))
}

/// `const T`, `mut T[]`, `const void`, ...
fn parse_ptr(spec: &str, nullable: bool, loc: &str) -> Result<Pointer> {
    let (mutability, rest) = spec
        .split_once(char::is_whitespace)
        .ok_or_else(|| anyhow!("{loc}: expected `@ptr(const T)` or `@ptr(mut T)`"))?;
    let mutable = match mutability {
        "const" => false,
        "mut" => true,
        other => bail!("{loc}: expected `const` or `mut`, found `{other}`"),
    };
    let rest = rest.trim();
    let (elem, array) = match rest.strip_suffix("[]") {
        Some(e) => (e.trim(), true),
        None => (rest, false),
    };
    let pointee = match elem {
        "void" if array => bail!("{loc}: `void[]` is not a valid pointee"),
        "void" => None,
        _ => match Prim::from_wit_name(elem) {
            Some(p) => Some(TyRef::Prim(p)),
            None if is_kebab(elem) => Some(TyRef::Named(elem.to_string())),
            None => bail!("{loc}: invalid pointee `{elem}`"),
        },
    };
    Ok(Pointer {
        mutable,
        pointee,
        array,
        nullable,
    })
}

fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

fn is_c_ident(s: &str) -> bool {
    s.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn parse_c_param(s: &str, loc: &str) -> Result<(String, String)> {
    match s.split_once(':') {
        Some((name, ty)) if !name.trim().is_empty() && !ty.trim().is_empty() => {
            Ok((name.trim().to_string(), ty.trim().to_string()))
        }
        _ => bail!("{loc}: expected `@c-param(name: C type)`, found `{s}`"),
    }
}

/// `ptr, len`.
fn parse_slice(s: &str, loc: &str) -> Result<SliceSpec> {
    match s.split_once(',') {
        Some((ptr, len)) if is_kebab(ptr.trim()) && is_kebab(len.trim()) => Ok(SliceSpec {
            ptr: ptr.trim().to_string(),
            len: len.trim().to_string(),
        }),
        _ => bail!("{loc}: expected `@slice(ptr-param, len-param)`, found `{s}`"),
    }
}

/// A primitive (`u32`) or a named type (`fd`).
fn parse_type_name(s: &str, loc: &str) -> Result<TyRef> {
    match Prim::from_wit_name(s) {
        Some(p) => Ok(TyRef::Prim(p)),
        None if is_kebab(s) => Ok(TyRef::Named(s.to_string())),
        None => bail!("{loc}: invalid type `{s}`"),
    }
}

/// `param` or `param, lo-hi|hi-lo`.
fn parse_merged_i64(s: &str, loc: &str) -> Result<MergedI64> {
    let (param, order) = match s.split_once(',') {
        Some((p, o)) => (p.trim(), o.trim()),
        None => (s.trim(), "lo-hi"),
    };
    let order = match order {
        "lo-hi" => SplitOrder::LoHi,
        "hi-lo" => SplitOrder::HiLo,
        other => bail!("{loc}: `@merged-i64({s})`: expected `lo-hi` or `hi-lo`, found `{other}`"),
    };
    if param.is_empty() {
        bail!("{loc}: `@merged-i64({s})` names no param");
    }
    Ok(MergedI64 {
        param: param.to_string(),
        order,
    })
}

fn validate_refs(model: &Model, pending: &Pending) -> Result<()> {
    fn check(model: &Model, ty: &TyRef, loc: &str) -> Result<()> {
        match ty {
            TyRef::Prim(_) => Ok(()),
            TyRef::Array(elem, _) => check(model, elem, loc),
            TyRef::Named(n) if model.type_def(n).is_some() => Ok(()),
            TyRef::Named(n) => bail!("{loc}: reference to unknown type `{n}`"),
        }
    }
    for t in &model.types {
        let loc = &pending.locations[&t.name];
        match &t.kind {
            TypeKind::Alias(a) => check(model, &a.target, loc)?,
            TypeKind::Pointer(p) => {
                if let Some(pointee) = &p.pointee {
                    check(model, pointee, loc)?;
                }
            }
            TypeKind::Record(r) => {
                for f in &r.fields {
                    check(model, &f.ty, loc)?;
                }
            }
            TypeKind::Constants(_) => {}
        }
    }
    for (name, base) in &pending.ptr_bases {
        let loc = &pending.locations[name];
        check(model, base, loc)?;
        if !model.is_pointer_sized(base) {
            bail!("{loc}: `@ptr` aliases must alias a `@pointer-sized` type");
        }
    }
    for iface in &model.interfaces {
        for f in &iface.functions {
            let loc = &pending.function_locations[&(iface.name.clone(), f.name.clone())];
            for ty in f
                .params
                .iter()
                .map(|p| &p.ty)
                .chain(&f.result)
                .chain(&f.returns)
            {
                check(model, ty, loc)?;
            }
        }
    }
    Ok(())
}

fn validate_functions(model: &Model, pending: &Pending) -> Result<()> {
    let mut imports = HashSet::new();
    let mut nrs = HashSet::new();
    for iface in &model.interfaces {
        for f in &iface.functions {
            let loc = &pending.function_locations[&(iface.name.clone(), f.name.clone())];
            if !imports.insert((iface.module.clone(), f.import_name.clone())) {
                bail!("{loc}: duplicate import name `{}`", f.import_name);
            }
            if let Some(nr) = f.nr
                && !nrs.insert((iface.module.clone(), nr))
            {
                bail!("{loc}: duplicate syscall number {nr}");
            }
            for p in &f.params {
                if model.valtype(&p.ty).is_none() {
                    bail!(
                        "{loc}: param `{}` must be a scalar, not an aggregate",
                        p.name
                    );
                }
            }
            if let Some(r) = &f.result
                && model.valtype(r).is_none()
            {
                bail!("{loc}: result must be a scalar, not an aggregate");
            }
            let param = |name: &str| f.params.iter().find(|p| p.name == name);
            for (i, m) in f.merged_i64.iter().enumerate() {
                let m = &m.param;
                match param(m) {
                    Some(p) if model.valtype(&p.ty) == Some(crate::ir::ValType::I64) => {}
                    Some(_) => bail!("{loc}: `@merged-i64({m})` param is not 64-bit"),
                    None => bail!("{loc}: `@merged-i64({m})` names an unknown param"),
                }
                if f.merged_i64[..i].iter().any(|prev| &prev.param == m) {
                    bail!("{loc}: duplicate `@merged-i64({m})`");
                }
            }
            for (i, name) in f.c_one_slot.iter().enumerate() {
                match param(name) {
                    Some(p) if model.valtype(&p.ty) == Some(crate::ir::ValType::I64) => {}
                    Some(_) => bail!("{loc}: `@c-one-slot({name})` param is not 64-bit"),
                    None => bail!("{loc}: `@c-one-slot({name})` names an unknown param"),
                }
                if f.c_one_slot[..i].contains(name) {
                    bail!("{loc}: duplicate `@c-one-slot({name})`");
                }
                if f.merged_i64.iter().any(|m| &m.param == name) {
                    bail!("{loc}: `@c-one-slot({name})` param is also `@merged-i64`");
                }
            }
            for (name, _) in &f.c_params {
                if param(name).is_none() {
                    bail!("{loc}: `@c-param({name}: ...)` names an unknown param");
                }
            }
            validate_result(model, f, loc)?;
            validate_slices(model, f, loc)?;
        }
    }
    Ok(())
}

/// `@noreturn` functions have no result; `@returns` must fit the result.
fn validate_result(model: &Model, f: &Function, loc: &str) -> Result<()> {
    if f.noreturn && f.result.is_some() {
        bail!("{loc}: `@noreturn` functions have no result");
    }
    let Some(returns) = &f.returns else {
        return Ok(());
    };
    let result = f.result.as_ref().map(|r| model.layout(r).size);
    match model.scalar_prim(returns) {
        _ if result.is_none() => bail!("{loc}: `@returns` needs a result"),
        Some(p) if Some(p.size()) <= result => Ok(()),
        _ => bail!("{loc}: `@returns` type must be a scalar no wider than the result"),
    }
}

/// `@slice(ptr, len)`: `ptr` is a byte pointer, `len` an unsigned length of at most 32 bits, and
/// no param belongs to two slices.
fn validate_slices(model: &Model, f: &Function, loc: &str) -> Result<()> {
    let mut used = HashSet::new();
    for s in &f.slices {
        let what = format!("`@slice({}, {})`", s.ptr, s.len);
        let ty = |name: &str| f.params.iter().find(|p| p.name == name).map(|p| &p.ty);
        let (Some(ptr), Some(len)) = (ty(&s.ptr), ty(&s.len)) else {
            bail!("{loc}: {what} names an unknown param");
        };
        if !used.insert(&s.ptr) || !used.insert(&s.len) {
            bail!("{loc}: {what} reuses a param of another slice");
        }
        let byte_ptr = match ptr {
            TyRef::Named(n) => match &model.type_def(n).expect("validated").kind {
                TypeKind::Pointer(p) => matches!(p.pointee, None | Some(TyRef::Prim(Prim::U8))),
                _ => false,
            },
            _ => false,
        };
        if !byte_ptr {
            bail!("{loc}: {what}: `{}` is not a `u8` or `void` pointer", s.ptr);
        }
        if !matches!(
            model.scalar_prim(len),
            Some(Prim::U8 | Prim::U16 | Prim::U32)
        ) {
            bail!(
                "{loc}: {what}: `{}` is not an unsigned 32-bit length",
                s.len
            );
        }
    }
    Ok(())
}

fn validate_abi_error(model: &Model, pending: &Pending) -> Result<()> {
    let mut found = model
        .types
        .iter()
        .filter(|t| matches!(&t.kind, TypeKind::Constants(c) if c.abi_error));
    if let (Some(first), Some(second)) = (found.next(), found.next()) {
        bail!(
            "{}: `@abi-error` is already on type `{}`; only one type may have it",
            pending.locations[&second.name],
            first.name
        );
    }
    Ok(())
}

/// Computes C layouts for every type and record field.
fn compute_layouts(model: &mut Model, pending: &Pending) -> Result<()> {
    let mut cx = LayoutCx {
        model,
        pending,
        done: HashMap::new(),
        fields: HashMap::new(),
        visiting: HashSet::new(),
    };
    for t in &model.types {
        cx.named(&t.name)?;
    }
    let LayoutCx {
        mut done,
        mut fields,
        ..
    } = cx;
    for t in &mut model.types {
        t.layout = done.remove(&t.name).expect("layout computed");
        if let TypeKind::Record(r) = &mut t.kind {
            let offsets = fields.remove(&t.name).expect("field layouts computed");
            for (f, (offset, layout)) in r.fields.iter_mut().zip(offsets) {
                f.offset = offset;
                f.layout = layout;
            }
        }
    }
    Ok(())
}

struct LayoutCx<'a> {
    model: &'a Model,
    pending: &'a Pending,
    done: HashMap<String, Layout>,
    fields: HashMap<String, Vec<(u32, Layout)>>,
    visiting: HashSet<String>,
}

impl LayoutCx<'_> {
    /// `owner` is the type whose layout is being computed (for diagnostics).
    fn of(&mut self, ty: &TyRef, owner: &str) -> Result<Layout> {
        match ty {
            TyRef::Prim(p) => Ok(prim_layout(p.size())),
            TyRef::Named(n) => self.named(n),
            TyRef::Array(elem, n) => {
                let l = self.of(elem, owner)?;
                Ok(Layout {
                    size: l.size.checked_mul(*n).ok_or_else(|| self.overflow(owner))?,
                    align: l.align,
                })
            }
        }
    }

    fn overflow(&self, owner: &str) -> anyhow::Error {
        anyhow!(
            "{}: size of `{owner}` overflows u32",
            self.pending.locations[owner]
        )
    }

    fn named(&mut self, name: &str) -> Result<Layout> {
        if let Some(l) = self.done.get(name) {
            return Ok(*l);
        }
        let loc = &self.pending.locations[name];
        if !self.visiting.insert(name.to_string()) {
            bail!("{loc}: type `{name}` contains itself");
        }
        let model = self.model;
        let def = model.type_def(name).expect("validated");
        let pw = model.target.pointer_width;
        let layout = match &def.kind {
            TypeKind::Alias(a) if a.pointer_sized => prim_layout(pw),
            TypeKind::Alias(a) => self.of(&a.target, name)?,
            TypeKind::Pointer(_) => prim_layout(pw),
            TypeKind::Constants(c) => prim_layout(c.repr.size()),
            TypeKind::Record(r) => self.record(name, r)?,
        };
        self.check_expected(name, layout)?;
        self.visiting.remove(name);
        self.done.insert(name.to_string(), layout);
        Ok(layout)
    }

    fn record(&mut self, name: &str, r: &Record) -> Result<Layout> {
        let mut offsets = Vec::new();
        let (mut end, mut align) = (0u32, 1u32);
        for f in &r.fields {
            let l = self.of(&f.ty, name)?;
            let offset = if r.is_union {
                Some(0)
            } else {
                end.checked_next_multiple_of(l.align)
            };
            let offset = offset.ok_or_else(|| self.overflow(name))?;
            offsets.push((offset, l));
            let field_end = offset.checked_add(l.size);
            end = end.max(field_end.ok_or_else(|| self.overflow(name))?);
            align = align.max(l.align);
        }
        self.fields.insert(name.to_string(), offsets);
        Ok(Layout {
            size: end
                .checked_next_multiple_of(align)
                .ok_or_else(|| self.overflow(name))?,
            align,
        })
    }

    /// `@size`/`@align` describe riscv32, so they only apply to the wasm32 target.
    fn check_expected(&self, name: &str, layout: Layout) -> Result<()> {
        if self.model.target != Target::WASM32 {
            return Ok(());
        }
        let Some((size, align)) = self.pending.expected.get(name) else {
            return Ok(());
        };
        let loc = &self.pending.locations[name];
        if let Some(size) = size
            && *size != layout.size
        {
            bail!(
                "{loc}: computed size {} does not match @size({size})",
                layout.size
            );
        }
        if let Some(align) = align
            && *align != layout.align
        {
            bail!(
                "{loc}: computed alignment {} does not match @align({align})",
                layout.align
            );
        }
        Ok(())
    }
}

fn prim_layout(size: u32) -> Layout {
    Layout { size, align: size }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ValType;

    const PRELUDE: &str =
        "package t:t@0.1.0;\ninterface types {\n/// @pointer-sized\ntype uptr = u32;\n";

    fn parse(body: &str) -> Result<Model> {
        parse_str("t.wit", &format!("{PRELUDE}{body}\n}}\n"), Target::WASM32)
    }

    fn err(body: &str) -> String {
        format!("{:#}", parse(body).unwrap_err())
    }

    fn record<'m>(m: &'m Model, name: &str) -> (&'m TypeDef, &'m Record) {
        let t = m.type_def(name).unwrap();
        match &t.kind {
            TypeKind::Record(r) => (t, r),
            _ => panic!("not a record"),
        }
    }

    #[test]
    fn struct_layout_follows_c_rules() {
        let m = parse("record s { a: u8, b: u64, c: u16, d: list<u8, 3>, p: uptr }").unwrap();
        let (t, r) = record(&m, "s");
        let offsets: Vec<_> = r.fields.iter().map(|f| f.offset).collect();
        assert_eq!(offsets, [0, 8, 16, 18, 24]);
        assert_eq!((t.layout.size, t.layout.align), (32, 8));
    }

    #[test]
    fn union_layout_and_nesting() {
        let m = parse(
            "/// @union\nrecord u { a: u8, b: u64, c: list<u8, 9> }\nrecord s { x: u32, u: u }",
        )
        .unwrap();
        let (u, ur) = record(&m, "u");
        assert_eq!((u.layout.size, u.layout.align), (16, 8));
        assert!(ur.fields.iter().all(|f| f.offset == 0));
        let (s, sr) = record(&m, "s");
        assert_eq!(sr.fields[1].offset, 8);
        assert_eq!(s.layout.size, 24);
    }

    #[test]
    fn pointer_sized_follows_target() {
        let src = format!(
            "{PRELUDE}/// @ptr(const u8[])\ntype cstr = uptr;\nrecord s {{ a: u32, p: cstr }}\n}}\n"
        );
        let m64 = parse_str("t.wit", &src, Target { pointer_width: 8 }).unwrap();
        let (t, r) = record(&m64, "s");
        assert_eq!((r.fields[1].offset, t.layout.size), (8, 16));
        assert_eq!(
            m64.valtype(&TyRef::Named("cstr".into())),
            Some(ValType::I64)
        );
        let m32 = parse_str("t.wit", &src, Target::WASM32).unwrap();
        assert_eq!(m32.type_def("s").unwrap().layout.size, 8);
    }

    #[test]
    fn size_mismatch_is_reported_with_location() {
        let e = err("/// @size(12)\nrecord s { a: u64, b: u32 }");
        assert!(e.contains("t.wit:"), "{e}");
        assert!(
            e.contains("computed size 16 does not match @size(12)"),
            "{e}"
        );
        let e = err("/// @align(4)\nrecord s { a: u64 }");
        assert!(e.contains("computed alignment 8"), "{e}");
    }

    #[test]
    fn unknown_and_misplaced_annotations() {
        let e = err("/// @bogus\ntype x = u32;");
        assert!(e.contains("(type `x`): unknown annotation `@bogus`"), "{e}");
        let e = err("record s {\n/// @value(1)\na: u32 }");
        assert!(e.contains("field `s.a`"), "{e}");
        assert!(
            e.contains("`@value` is not allowed on a record field"),
            "{e}"
        );
        let e = err("/// @c-skip\nrecord s { a: u32 }");
        assert!(e.contains("`@c-skip` is not allowed on a record"), "{e}");
        let e = err("/// @kernel-only\ntype x = u32;");
        assert!(e.contains("not allowed on a type alias"), "{e}");
        let m = parse("/// @kernel-only\nrecord s {\n/// @c-skip\na: u32 }").unwrap();
        let (_, r) = record(&m, "s");
        assert!(r.kernel_only && r.fields[0].c_skip);
    }

    #[test]
    fn abi_error_validation() {
        let e = err(
            "/// @repr(s32)\n/// @abi-error\nenum a {\n/// @value(1)\nx }\n/// @repr(s32)\n/// @abi-error\nenum b {\n/// @value(1)\nx }",
        );
        assert!(
            e.contains("(type `b`): `@abi-error` is already on type `a`"),
            "{e}"
        );
        let e = err("/// @repr(u32)\n/// @abi-error\nflags f {\n/// @value(1)\nx }");
        assert!(
            e.contains("`@abi-error` is not allowed on a flags type"),
            "{e}"
        );
        let e = err("/// @repr(u32)\n/// @abi-error\nenum e {\n/// @value(1)\nx }");
        assert!(e.contains("needs a signed @repr"), "{e}");
        let m = parse("/// @repr(s32)\n/// @abi-error\nenum e {\n/// @value(1)\nx }").unwrap();
        assert_eq!(m.abi_error().unwrap().0.name, "e");
    }

    #[test]
    fn constants_validation() {
        assert!(err("enum e { a }").contains("missing required `@repr(...)`"));
        assert!(err("/// @repr(u32)\nenum e { a }").contains("missing required `@value(...)`"));
        let e = err("/// @repr(u8)\nflags f {\n/// @value(0x100)\na }");
        assert!(e.contains("does not fit @repr(u8)"), "{e}");
        let e = err("/// @repr(s32)\nenum e {\n/// @value(1)\n/// @name(x-y)\na }");
        assert!(e.contains("not a valid C identifier"), "{e}");
        let e =
            err("/// @repr(s32)\nenum e {\n/// @value(1)\n/// @name(B)\na,\n/// @value(2)\nb }");
        assert!(e.contains("duplicate constant name `B`"), "{e}");
        let m = parse(
            "/// @repr(s32)\nenum e {\n/// @value(-1)\na,\n/// @value(-1)\n/// @name(__A)\nb }",
        )
        .unwrap();
        let TypeKind::Constants(c) = &m.type_def("e").unwrap().kind else {
            panic!()
        };
        let names: Vec<_> = c
            .members
            .iter()
            .map(|m| (m.c_name.as_str(), m.value))
            .collect();
        assert_eq!(names, [("A", -1), ("__A", -1)]);
    }

    #[test]
    fn pointer_validation() {
        assert!(err("/// @ptr(const u8)\ntype p = u32;").contains("must alias a `@pointer-sized`"));
        assert!(err("/// @ptr(const nope)\ntype p = uptr;").contains("unknown type `nope`"));
        assert!(err("/// @ptr(owned u8)\ntype p = uptr;").contains("expected `const` or `mut`"));
        assert!(err("/// @nullable\ntype p = uptr;").contains("requires `@ptr"));
        assert!(err("/// @pointer-sized\ntype p = u64;").contains("u32 or s32"));
    }

    #[test]
    fn newtype_validation() {
        let m = parse("/// @newtype\ntype fd = s32;").unwrap();
        let TypeKind::Alias(a) = &m.type_def("fd").unwrap().kind else {
            panic!("not an alias")
        };
        assert!(a.newtype);
        assert!(err("/// @newtype\ntype x = uptr;").contains("fixed-width integer"));
        let e = err("/// @newtype\n/// @pointer-sized\ntype x = u32;");
        assert!(e.contains("fixed-width integer"), "{e}");
        assert!(err("/// @newtype\n/// @ptr(const u8)\ntype p = uptr;").contains("fixed-width"));
        assert!(err("/// @newtype\nrecord r { a: u32 }").contains("not allowed on a record"));
    }

    fn func_err(func: &str) -> String {
        let src = format!(
            "{PRELUDE}record r {{ a: u32 }}\n}}\n/// @module(m)\ninterface f {{\nuse types.{{uptr, r}};\n{func}\n}}\n"
        );
        format!(
            "{:#}",
            parse_str("t.wit", &src, Target::WASM32).unwrap_err()
        )
    }

    #[test]
    fn function_validation() {
        assert!(func_err("x: func(a: r);").contains("param `a` must be a scalar"));
        assert!(func_err("/// @merged-i64(a)\nx: func(a: u32);").contains("is not 64-bit"));
        assert!(func_err("/// @merged-i64(b)\nx: func(a: s64);").contains("unknown param"));
        assert!(func_err("/// @c-param(b: int)\nx: func(a: s64);").contains("unknown param"));
        assert!(func_err("/// @import(y)\nx: func();\ny: func();").contains("duplicate import"));
        assert!(func_err("/// @union\nx: func();").contains("not allowed on a function"));
        let e = func_err("/// @merged-i64(a, mid)\nx: func(a: s64);");
        assert!(e.contains("expected `lo-hi` or `hi-lo`"), "{e}");
        let e = func_err("/// @merged-i64(a)\n/// @merged-i64(a, hi-lo)\nx: func(a: s64);");
        assert!(e.contains("duplicate `@merged-i64(a)`"), "{e}");
        assert!(func_err("/// @sys-name(-x)\nx: func();").contains("not a valid C identifier"));
        let e = func_err("/// @nr(1)\nx: func();\n/// @nr(1)\ny: func();");
        assert!(e.contains("duplicate syscall number 1"), "{e}");
    }

    #[test]
    fn import_names_are_unique_across_interfaces_of_a_module() {
        let src = format!(
            "{PRELUDE}}}\n/// @module(m)\ninterface a {{\nx-y: func();\n}}\n/// @module(m)\ninterface b {{\n/// @import(x_y)\nz: func();\n}}\n"
        );
        let e = format!(
            "{:#}",
            parse_str("t.wit", &src, Target::WASM32).unwrap_err()
        );
        assert!(e.contains("duplicate import name `x_y`"), "{e}");
    }

    #[test]
    fn slice_and_returns_validation() {
        let types = "/// @ptr(mut u8[])\ntype buf = uptr;\n/// @ptr(mut u32)\ntype u32-ptr = uptr;\n/// @pointer-sized\ntype usize = u32;\ntype fd = s32;\n";
        let f = |func: &str| {
            let src = format!(
                "{PRELUDE}{types}}}\n/// @module(m)\ninterface f {{\nuse types.{{buf, u32-ptr, usize, fd}};\n{func}\n}}\n"
            );
            parse_str("t.wit", &src, Target::WASM32).map_err(|e| format!("{e:#}"))
        };
        let m =
            f("/// @slice(b, n)\n/// @returns(u32)\nx: func(b: buf, n: usize) -> s32;").unwrap();
        let x = &m.interfaces[1].functions[0];
        assert_eq!(x.slices[0].ptr, "b");
        assert_eq!(x.returns, Some(TyRef::Prim(Prim::U32)));
        let e = f("/// @slice(b, m)\nx: func(b: buf, n: usize) -> s32;").unwrap_err();
        assert!(e.contains("names an unknown param"), "{e}");
        let e = f("/// @slice(p, n)\nx: func(p: u32-ptr, n: usize) -> s32;").unwrap_err();
        assert!(e.contains("not a `u8` or `void` pointer"), "{e}");
        let e = f("/// @slice(b, n)\nx: func(b: buf, n: s32) -> s32;").unwrap_err();
        assert!(e.contains("not an unsigned 32-bit length"), "{e}");
        let e = f("/// @slice(b, n)\n/// @slice(c, n)\nx: func(b: buf, c: buf, n: usize) -> s32;")
            .unwrap_err();
        assert!(e.contains("reuses a param"), "{e}");
        let e = f("/// @slice(b n)\nx: func(b: buf, n: usize) -> s32;").unwrap_err();
        assert!(e.contains("expected `@slice(ptr-param, len-param)`"), "{e}");
        let e = f("/// @returns(u64)\nx: func() -> s32;").unwrap_err();
        assert!(e.contains("no wider than the result"), "{e}");
        let e = f("/// @returns(nope)\nx: func() -> s32;").unwrap_err();
        assert!(e.contains("unknown type `nope`"), "{e}");
        assert!(f("/// @returns(fd)\nx: func() -> s32;").is_ok());
        let e = f("/// @noreturn\nx: func() -> s32;").unwrap_err();
        assert!(e.contains("have no result"), "{e}");
    }

    #[test]
    fn merged_i64_order_and_sys_name() {
        let src = format!(
            "{PRELUDE}}}\n/// @module(m)\ninterface f {{\n/// @merged-i64(a, hi-lo)\n/// @merged-i64(b)\n/// @sys-name(_x)\nx: func(a: s64, b: u64, c: s64);\n}}\n"
        );
        let m = parse_str("t.wit", &src, Target::WASM32).unwrap();
        let f = &m.interfaces[1].functions[0];
        assert_eq!(f.sys_name.as_deref(), Some("_x"));
        let orders: Vec<_> = ["a", "b", "c"].map(|p| f.split_order(p)).into();
        assert_eq!(
            orders,
            [SplitOrder::HiLo, SplitOrder::LoHi, SplitOrder::LoHi]
        );
    }

    #[test]
    fn use_copies_resolve_to_originals() {
        let src = format!(
            "{PRELUDE}/// The fd.\ntype fd = s32;\n}}\n/// @module(m)\ninterface f {{\nuse types.{{fd}};\nclose: func(fd: fd) -> s32;\n}}\n"
        );
        let m = parse_str("t.wit", &src, Target::WASM32).unwrap();
        assert_eq!(m.types.iter().filter(|t| t.name == "fd").count(), 1);
        assert_eq!(m.type_def("fd").unwrap().docs, ["The fd."]);
        let f = &m.interfaces[1].functions[0];
        assert_eq!(f.params[0].ty, TyRef::Named("fd".into()));
        assert_eq!(f.import_name, "close");
    }

    #[test]
    fn array_size_overflow_is_an_error() {
        let e = err("record s { a: list<list<u64, 65536>, 65536> }");
        assert!(e.contains("size of `s` overflows u32"), "{e}");
        let e = err("type big = list<u8, 4294967295>;\nrecord s { a: big, b: u8 }");
        assert!(e.contains("overflows u32"), "{e}");
    }

    #[test]
    fn named_fixed_list_is_an_alias() {
        let m = parse("type sigset = list<u32, 2>;\nrecord s { a: u8, set: sigset }").unwrap();
        let t = m.type_def("sigset").unwrap();
        assert_eq!((t.layout.size, t.layout.align), (8, 4));
        let TypeKind::Alias(a) = &t.kind else {
            panic!("not an alias")
        };
        assert_eq!(a.target, TyRef::Array(Box::new(TyRef::Prim(Prim::U32)), 2));
        assert_eq!(record(&m, "s").1.fields[1].offset, 4);
    }

    #[test]
    fn renamed_use_resolves_to_original() {
        let src = format!(
            "{PRELUDE}type fd = s32;\n}}\n/// @module(m)\ninterface f {{\nuse types.{{fd as handle}};\nclose: func(h: handle);\n}}\n"
        );
        let m = parse_str("t.wit", &src, Target::WASM32).unwrap();
        assert!(m.type_def("handle").is_none());
        let f = &m.interfaces[1].functions[0];
        assert_eq!(f.params[0].ty, TyRef::Named("fd".into()));
    }

    #[test]
    fn block_comment_annotations_apply() {
        let m = parse("/**\n * @union\n */\nrecord u { a: u32, b: u8 }").unwrap();
        let (t, r) = record(&m, "u");
        assert!(r.is_union);
        assert_eq!(t.layout.size, 4);
    }

    #[test]
    fn interface_with_functions_needs_module() {
        let src = format!("{PRELUDE}}}\ninterface f {{\nx: func();\n}}\n");
        let e = format!(
            "{:#}",
            parse_str("t.wit", &src, Target::WASM32).unwrap_err()
        );
        assert!(e.contains("need `@module(...)`"), "{e}");
    }
}
