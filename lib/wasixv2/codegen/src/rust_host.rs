//! [`Model`] -> Rust host bindings for `wasix-syscalls2`: the `Syscalls` trait, one wasmer host
//! function per import, and the `imports` constructor.
//!
//! Host functions only convert: raw wasm params are decoded into `wasix-types2` types, the trait
//! method is called, and its result is encoded back (`Ok(v)` -> `v`, `Err(e)` -> `e.to_ret()`).

use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::{Context, Result, bail};

use crate::ir::{Function, Model, Prim, TyRef, TypeKind, ValType};
use crate::rust::{Namespace, header, ident, prim, ty, type_name};

/// Items the generated module defines next to its glob import of `wasix_types2`.
const LOCAL_ITEMS: &[&str] = &["Syscalls", "host", "imports"];

/// Name of the env parameter of trait methods and host functions.
const CTX: &str = "ctx";

/// Generates the (unformatted) host bindings module for `model`.
pub fn generate(model: &Model, source: &str) -> Result<String> {
    let (err_def, err) = model
        .abi_error()
        .context("the rust-host backend needs an `@abi-error` type")?;
    if !err.members.iter().any(|m| m.c_name == "ENOSYS") {
        bail!("the `@abi-error` type `{}` has no ENOSYS", err_def.name);
    }
    for t in &model.types {
        let name = type_name(&t.name);
        if LOCAL_ITEMS.contains(&name.as_str()) {
            bail!("type `{}` collides with generated item `{name}`", t.name);
        }
    }
    let funcs = functions(model)?;
    let mut g = Gen {
        model,
        err: type_name(&err_def.name),
        default_result: model.common_result(),
        out: String::new(),
    };
    g.out.push_str(&header(source));
    g.line("\nuse wasix_types2::*;");
    g.trait_def(&funcs);
    g.host_fns(&funcs);
    g.imports(&funcs);
    Ok(g.out)
}

/// A function with its import module and generated Rust names.
struct HostFn<'a> {
    module: &'a str,
    f: &'a Function,
    /// Trait method and host function name.
    method: String,
    /// Rust param names, in order.
    params: Vec<String>,
}

/// Linux syscalls (interfaces with `@nr` functions) keep their names; other interfaces prefix
/// theirs with the interface name (`ext_thread_spawn`), so names cannot collide across modules.
fn functions(model: &Model) -> Result<Vec<HostFn<'_>>> {
    let mut methods = Namespace::new("method", &[]);
    let mut out = Vec::new();
    for iface in &model.interfaces {
        let module = iface.module.as_deref().unwrap_or_default();
        let p = if iface.functions.iter().any(|f| f.nr.is_some()) {
            String::new()
        } else {
            format!("{}-", iface.name)
        };
        for f in &iface.functions {
            let method = ident(&format!("{p}{}", f.name));
            methods.claim(
                method.clone(),
                format!("function `{}.{}`", iface.name, f.name),
            )?;
            out.push(HostFn {
                module,
                f,
                method,
                params: param_names(f)?,
            });
        }
    }
    Ok(out)
}

/// Snake-cased param names; a param named like the env param gets a `_` suffix.
fn param_names(f: &Function) -> Result<Vec<String>> {
    let mut seen = HashSet::from([CTX.to_string()]);
    let mut out = Vec::new();
    for p in &f.params {
        let mut name = ident(&p.name);
        if name == CTX {
            name.push('_');
        }
        if !seen.insert(name.clone()) {
            bail!(
                "function `{}`: param `{}` maps to duplicate `{name}`",
                f.name,
                p.name
            );
        }
        out.push(name);
    }
    Ok(out)
}

/// How a scalar type crosses the wasm boundary.
enum Repr {
    Int(Prim),
    /// A `GuestPtr`/`GuestPtrMut` alias.
    Ptr,
    /// A constant family newtype, by Rust type name.
    Const(String, Prim),
}

fn repr(model: &Model, t: &TyRef) -> Repr {
    let TyRef::Named(n) = t else {
        return Repr::Int(model.scalar_prim(t).expect("validated scalar"));
    };
    let def = model.type_def(n).expect("validated type");
    match &def.kind {
        TypeKind::Alias(a) if !a.pointer_sized => repr(model, &a.target),
        TypeKind::Pointer(_) => Repr::Ptr,
        TypeKind::Constants(c) => Repr::Const(type_name(n), c.repr),
        _ => Repr::Int(model.scalar_prim(t).expect("validated scalar")),
    }
}

fn raw_prim(v: ValType) -> Prim {
    match v {
        ValType::I32 => Prim::S32,
        ValType::I64 => Prim::S64,
    }
}

/// `expr as to` unless `from` already is `to`.
fn cast(expr: &str, from: Prim, to: Prim) -> String {
    if from == to {
        expr.to_string()
    } else {
        format!("{expr} as {}", prim(to))
    }
}

struct Gen<'a> {
    model: &'a Model,
    /// Rust name of the `@abi-error` type.
    err: String,
    default_result: Option<&'a TyRef>,
    out: String,
}

impl Gen<'_> {
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn docs(&mut self, docs: &[String]) {
        for d in docs {
            if d.trim().is_empty() {
                self.line("///");
            } else {
                let _ = writeln!(self.out, "/// {}", d.trim_start());
            }
        }
    }

    fn valtype(&self, t: &TyRef) -> ValType {
        self.model.valtype(t).expect("validated scalar")
    }

    /// Rust return type of a trait method.
    fn method_result(&self, f: &Function) -> String {
        match &f.result {
            None => "Result<(), wasmer::RuntimeError>".to_string(),
            Some(r) if Some(r) == self.default_result => "SyscallResult".to_string(),
            Some(r) => format!("SyscallResult<{}>", ty(r)),
        }
    }

    fn too_many_args(&mut self, h: &HostFn) {
        // ctx counts too; clippy's default limit is 7.
        if h.params.len() + 1 > 7 {
            self.line("#[allow(clippy::too_many_arguments)]");
        }
    }

    fn trait_def(&mut self, funcs: &[HostFn]) {
        let err = self.err.clone();
        self.line("");
        self.line("/// A wasix2 host: one method per imported function.");
        self.line("///");
        self.line(&format!(
            "/// Every method defaults to failing with `{err}::ENOSYS`. Functions without a wasm result"
        ));
        self.line("/// (`@noreturn`: `exit`, `exit_group`, `rt_sigreturn`) instead return a");
        self.line(
            "/// [`wasmer::RuntimeError`], which traps; by default an \"unimplemented\" one.",
        );
        self.line("pub trait Syscalls: Sized + Send + 'static {");
        for h in funcs {
            self.method(h);
        }
        self.line("}");
    }

    fn method(&mut self, h: &HostFn) {
        let f = h.f;
        self.line("");
        self.docs(&f.docs);
        if !f.docs.is_empty() {
            self.line("///");
        }
        let nr = f.nr.map_or(String::new(), |n| format!(" (riscv32 nr {n})"));
        let _ = writeln!(
            self.out,
            "/// Wasm import `{}.{}`{nr}.",
            h.module, f.import_name
        );
        if f.unsupported {
            self.line("///");
            self.line("/// Declared for completeness; hosts may leave it unimplemented.");
        }
        self.too_many_args(h);
        let mut sig = format!(
            "fn {}({CTX}: &mut wasmer::FunctionEnvMut<'_, Self>",
            h.method
        );
        for (p, name) in f.params.iter().zip(&h.params) {
            let _ = write!(sig, ", {name}: {}", ty(&p.ty));
        }
        let _ = writeln!(self.out, "{sig}) -> {} {{", self.method_result(f));
        let mut used = vec![CTX.to_string()];
        used.extend(h.params.iter().cloned());
        if used.len() == 1 {
            let _ = writeln!(self.out, "let _ = {CTX};");
        } else {
            let _ = writeln!(self.out, "let _ = ({});", used.join(", "));
        }
        if f.result.is_some() {
            let _ = writeln!(self.out, "Err({}::ENOSYS)", self.err);
        } else {
            let _ = writeln!(
                self.out,
                "Err(wasmer::RuntimeError::new(\"wasix2: `{}` is not implemented\"))",
                f.import_name
            );
        }
        self.line("}");
    }

    fn host_fns(&mut self, funcs: &[HostFn]) {
        self.line("");
        self.line("/// Wasmer host functions: decode raw wasm params, call [`Syscalls`], encode the result.");
        self.line("pub mod host {");
        self.line("use super::*;");
        for h in funcs {
            self.host_fn(h);
        }
        self.line("}");
    }

    fn host_fn(&mut self, h: &HostFn) {
        let f = h.f;
        self.line("");
        let _ = writeln!(
            self.out,
            "/// Host function for `{}.{}`; see [`Syscalls::{}`].",
            h.module, f.import_name, h.method
        );
        self.too_many_args(h);
        let mut sig = format!(
            "pub fn {}<E: Syscalls>(mut {CTX}: wasmer::FunctionEnvMut<'_, E>",
            h.method
        );
        let mut args = vec![format!("&mut {CTX}")];
        for (p, name) in f.params.iter().zip(&h.params) {
            let raw = raw_prim(self.valtype(&p.ty));
            let _ = write!(sig, ", {name}: {}", prim(raw));
            args.push(self.decode(name, &p.ty, raw));
        }
        let call = format!("E::{}({})", h.method, args.join(", "));
        match &f.result {
            None => {
                let _ = writeln!(self.out, "{sig}) -> Result<(), wasmer::RuntimeError> {{");
                self.line(&call);
            }
            Some(r) => {
                let raw = raw_prim(self.valtype(r));
                let _ = writeln!(self.out, "{sig}) -> {} {{", prim(raw));
                let ok = self.encode("v", r, raw);
                let err = self.encode_err(raw);
                let _ = writeln!(
                    self.out,
                    "match {call} {{ Ok(v) => {ok}, Err(e) => {err}, }}"
                );
            }
        }
        self.line("}");
    }

    /// Raw wasm value `name` (of type `raw`) -> the typed param.
    fn decode(&self, name: &str, t: &TyRef, raw: Prim) -> String {
        let addr = Prim::U32.with_size(self.model.target.pointer_width);
        match repr(self.model, t) {
            Repr::Int(p) => cast(name, raw, p),
            Repr::Ptr => format!("{}::new({})", ty(t), cast(name, raw, addr)),
            Repr::Const(n, p) => format!("{n}({})", cast(name, raw, p)),
        }
    }

    /// Typed value `v` -> the raw wasm value of type `raw`.
    fn encode(&self, v: &str, t: &TyRef, raw: Prim) -> String {
        let addr = Prim::U32.with_size(self.model.target.pointer_width);
        match repr(self.model, t) {
            Repr::Int(p) => cast(v, p, raw),
            Repr::Ptr => cast(&format!("{v}.addr()"), addr, raw),
            Repr::Const(_, p) => cast(&format!("{v}.0"), p, raw),
        }
    }

    /// `e.to_ret()` returns the default result type.
    fn encode_err(&self, raw: Prim) -> String {
        let ret = self
            .default_result
            .and_then(|r| self.model.scalar_prim(r))
            .unwrap_or(raw);
        cast("e.to_ret()", ret, raw)
    }

    fn imports(&mut self, funcs: &[HostFn]) {
        self.line("");
        self.line("/// Builds the imports of every wasix2 function, backed by `env`.");
        self.line("pub fn imports<E: Syscalls>(store: &mut impl wasmer::AsStoreMut, env: &wasmer::FunctionEnv<E>) -> wasmer::Imports {");
        self.line("let mut imports = wasmer::Imports::new();");
        for h in funcs {
            let _ = writeln!(
                self.out,
                "imports.define(\"{}\", \"{}\", wasmer::Function::new_typed_with_env(store, env, host::{}::<E>));",
                h.module, h.f.import_name, h.method
            );
        }
        self.line("imports");
        self.line("}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Target;
    use crate::parse::parse_str;

    const TYPES: &str = "package t:t@0.1.0;\ninterface types {\n/// @pointer-sized\ntype slong = s32;\n/// @repr(s32)\n/// @abi-error\nenum errno {\n/// @value(38)\nenosys }\n}\n";

    fn gen_src(funcs: &str) -> Result<String> {
        let src = format!("{TYPES}{funcs}");
        let model = parse_str("t.wit", &src, Target::WASM32).unwrap();
        generate(&model, "t.wit")
    }

    #[test]
    fn non_linux_interfaces_are_prefixed() {
        let out = gen_src(
            "/// @module(a)\ninterface a {\nuse types.{slong};\n/// @nr(1)\nx: func() -> slong;\n}\n\
             /// @module(b)\ninterface b {\nuse types.{slong};\nx: func() -> slong;\n}\n",
        )
        .unwrap();
        assert!(out.contains("fn x(ctx"), "{out}");
        assert!(out.contains("fn b_x(ctx"), "{out}");
        assert!(out.contains("imports.define(\"b\", \"x\""), "{out}");
    }

    #[test]
    fn prefixed_collisions_are_rejected() {
        let e = gen_src(
            "/// @module(a)\ninterface a {\nuse types.{slong};\n/// @nr(1)\nb-x: func() -> slong;\n}\n\
             /// @module(b)\ninterface b {\nuse types.{slong};\nx: func() -> slong;\n}\n",
        )
        .unwrap_err();
        assert!(
            format!("{e:#}").contains("both map to Rust method `b_x`"),
            "{e:#}"
        );
    }

    #[test]
    fn ctx_param_is_renamed() {
        let out = gen_src(
            "/// @module(a)\ninterface a {\nuse types.{slong};\nx: func(ctx: u64) -> slong;\n}\n",
        )
        .unwrap();
        assert!(out.contains("fn a_x(ctx"), "{out}");
        assert!(out.contains("ctx_: u64"), "{out}");
        assert!(out.contains("ctx_: i64"), "{out}");
        assert!(out.contains("E::a_x(&mut ctx, ctx_ as u64)"), "{out}");
    }
}
