//! [`Model`] -> Rust source for `wasixv2-syscalls`: the `Syscalls` trait with one async method
//! per import, the `Syscall` request enum, `SyscallRet`, `dispatch`, the `Layer` trait and the
//! forwarding `Layered` adapter.

use std::fmt::Write as _;

use anyhow::{Result, bail};

use crate::ir::Model;
use crate::rust::{header, push_docs};
use crate::shape::{Method, methods, signature_types, slice_type, uses_slice};

/// Items of `wasixv2-syscalls` the generated module names, which ABI types must not shadow.
const LOCAL_ITEMS: &[&str] = &[
    "Caller",
    "Control",
    "Layer",
    "Layered",
    "SysResult",
    "Syscall",
    "SyscallRet",
    "Syscalls",
    "TaskId",
    "UserSlice",
    "UserSliceMut",
];

/// Generates the (unformatted) module for `model`.
pub fn generate(model: &Model, source: &str) -> Result<String> {
    let methods = methods(model)?;
    let mut types = signature_types(&methods);
    if let Some(t) = types.iter().find(|t| LOCAL_ITEMS.contains(&t.as_str())) {
        bail!("ABI type `{t}` collides with an item of wasixv2-syscalls");
    }
    types.insert("Errno".to_string());
    let mut g = Gen { out: String::new() };
    g.out.push_str(&header(source));
    g.imports(&methods, &types);
    g.trait_def(&methods);
    g.enums(&methods);
    g.dispatch(&methods);
    g.out.push_str(LAYER);
    g.layered(&methods);
    Ok(g.out)
}

struct Gen {
    out: String,
}

impl Gen {
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn imports(&mut self, methods: &[Method], types: &std::collections::BTreeSet<String>) {
        let types: Vec<&str> = types.iter().map(String::as_str).collect();
        let _ = writeln!(self.out, "\nuse wasixv2_abi::{{{}}};\n", types.join(", "));
        let slices: Vec<&str> = [false, true]
            .into_iter()
            .filter(|m| uses_slice(methods, *m))
            .map(slice_type)
            .collect();
        let memory = match slices.as_slice() {
            [] => String::new(),
            [one] => format!("memory::{one}, "),
            many => format!("memory::{{{}}}, ", many.join(", ")),
        };
        let _ = writeln!(
            self.out,
            "use crate::{{caller::Caller, {memory}result::SysResult}};"
        );
    }

    fn trait_def(&mut self, methods: &[Method]) {
        self.out.push_str(TRAIT_DOCS);
        self.line("pub trait Syscalls {");
        for m in methods {
            self.line("");
            push_docs(&mut self.out, &m.f.docs);
            if !m.f.docs.is_empty() {
                self.line("///");
            }
            self.line(&m.import_doc());
            if m.f.unsupported {
                self.line("///");
                self.line(
                    "/// Declared for completeness; implementations may leave it unimplemented.",
                );
            }
            if m.too_many_args() {
                self.line("#[allow(clippy::too_many_arguments)]");
            }
            let _ = writeln!(self.out, "{} {{ Err(Errno::ENOSYS.into()) }}", signature(m));
        }
        self.line("}");
    }

    fn enums(&mut self, methods: &[Method]) {
        self.line("");
        self.line("/// One syscall with its arguments, for layers that treat every call alike.");
        self.line("#[derive(Clone, Copy, Debug, PartialEq, Eq)]");
        self.line("pub enum Syscall {");
        for m in methods {
            let params = m.typed_params();
            if params.is_empty() {
                let _ = writeln!(self.out, "{},", m.variant);
            } else {
                let _ = writeln!(self.out, "{} {{ {} }},", m.variant, params.join(", "));
            }
        }
        self.line("}");
        self.line("");
        self.line("/// The value a [`Syscall`] produced.");
        self.line("#[derive(Clone, Copy, Debug, PartialEq, Eq)]");
        self.line("pub enum SyscallRet {");
        for m in methods {
            match m.ret_type() {
                Some(t) => {
                    let _ = writeln!(self.out, "{}({t}),", m.variant);
                }
                None => {
                    let _ = writeln!(self.out, "{},", m.variant);
                }
            }
        }
        self.line("}");
    }

    fn dispatch(&mut self, methods: &[Method]) {
        self.line("");
        self.line("pub async fn dispatch<S: Syscalls + ?Sized>(");
        self.line("syscalls: &S, caller: &Caller<'_>, call: Syscall,");
        self.line(") -> SysResult<SyscallRet> {");
        self.line("Ok(match call {");
        for m in methods {
            let call = format!("syscalls.{}({}).await?", m.name, call_args(m).join(", "));
            let pattern = pattern(m);
            match m.ret {
                Some(_) => {
                    let _ = writeln!(self.out, "{pattern} => SyscallRet::{}({call}),", m.variant);
                }
                None => {
                    let _ = writeln!(
                        self.out,
                        "{pattern} => {{ {call}; SyscallRet::{} }}",
                        m.variant
                    );
                }
            }
        }
        self.line("})");
        self.line("}");
    }

    fn layered(&mut self, methods: &[Method]) {
        self.line("");
        self.line("impl<L: Layer, S: Syscalls> Syscalls for Layered<L, S> {");
        for m in methods {
            self.line("");
            if m.too_many_args() {
                self.line("#[allow(clippy::too_many_arguments)]");
            }
            let _ = writeln!(self.out, "{} {{", signature(m));
            let _ = writeln!(self.out, "let call = {};", pattern(m));
            self.line("match self.layer.call(caller, call, &self.inner).await? {");
            match m.ret {
                Some(_) => {
                    let _ = writeln!(self.out, "SyscallRet::{}(ret) => Ok(ret),", m.variant);
                }
                None => {
                    let _ = writeln!(self.out, "SyscallRet::{} => Ok(()),", m.variant);
                }
            }
            let _ = writeln!(self.out, "_ => mismatch(\"{}\"),", m.f.import_name);
            self.line("}");
            self.line("}");
        }
        self.line("}");
        self.out.push_str(CONTROL);
    }
}

/// `async fn name(&self, caller: &Caller<'_>, ...) -> SysResult<T>`.
fn signature(m: &Method) -> String {
    let mut params = vec!["&self".to_string(), "caller: &Caller<'_>".to_string()];
    params.extend(m.typed_params());
    format!(
        "async fn {}({}) -> {}",
        m.name,
        params.join(", "),
        m.result_type()
    )
}

fn call_args(m: &Method) -> Vec<String> {
    let mut args = vec!["caller".to_string()];
    args.extend(m.param_names().into_iter().map(str::to_string));
    args
}

/// `Syscall::Name { a, b }`, also usable as an expression.
fn pattern(m: &Method) -> String {
    let names = m.param_names();
    if names.is_empty() {
        format!("Syscall::{}", m.variant)
    } else {
        format!("Syscall::{} {{ {} }}", m.variant, names.join(", "))
    }
}

const TRAIT_DOCS: &str = r#"
/// One async method per import of the guest ABI, with typed arguments.
///
/// Every method fails with `ENOSYS` unless overridden, so an implementation (the kernel, a test
/// double) provides only the syscalls it supports.
// The futures are deliberately not `Send`: wasmer's async host calls are not.
#[allow(async_fn_in_trait, unused_variables)]
"#;

const LAYER: &str = r#"
/// A layer that sees every syscall as a [`Syscall`] value.
#[allow(async_fn_in_trait)]
pub trait Layer {
    async fn call<S: Syscalls>(
        &self,
        caller: &Caller<'_>,
        call: Syscall,
        next: &S,
    ) -> SysResult<SyscallRet>;
}

/// Runs `layer` in front of `inner`.
pub struct Layered<L, S> {
    pub layer: L,
    pub inner: S,
}

impl<L, S> Layered<L, S> {
    pub fn new(layer: L, inner: S) -> Self {
        Self { layer, inner }
    }
}

fn mismatch(call: &str) -> ! {
    unreachable!("a layer answered `{call}` with another syscall's result")
}
"#;

const CONTROL: &str = r#"
impl<L, S: crate::Control> crate::Control for Layered<L, S> {
    fn take_pending(&self, task: crate::TaskId) -> Option<crate::GuestAction> {
        self.inner.take_pending(task)
    }
}
"#;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ir::Target;
    use crate::parse::parse_str;

    pub(crate) const TYPES: &str = "package t:t@0.1.0;\ninterface types {\n/// @pointer-sized\ntype uptr = u32;\n/// @pointer-sized\ntype slong = s32;\n/// @pointer-sized\ntype usize = u32;\n/// @newtype\ntype fd = s32;\n/// @ptr(mut u8[])\ntype buf = uptr;\n/// @ptr(const u8[])\ntype cstr = uptr;\n/// @repr(s32)\n/// @abi-error\nenum errno {\n/// @value(38)\nenosys }\n}\n";

    pub(crate) fn model(funcs: &str) -> Model {
        let src = format!(
            "{TYPES}/// @module(m)\ninterface s {{\nuse types.{{uptr, slong, usize, fd, buf, cstr}};\n{funcs}\n}}\n"
        );
        parse_str("t.wit", &src, Target::WASM32).unwrap()
    }

    #[test]
    fn slices_and_returns_shape_the_signature() {
        let out = generate(
            &model(
                "/// @nr(63)\n/// @slice(buf, count)\n/// @returns(u32)\nread: func(fd: fd, buf: buf, count: usize) -> slong;\n\
                 /// @returns(fd)\nopenat: func(dfd: fd, path: cstr) -> slong;\n\
                 /// @noreturn\nexit-group: func(error-code: s32);\ngetpid: func() -> slong;",
            ),
            "t.wit",
        )
        .unwrap();
        for want in [
            "async fn read(&self, caller: &Caller<'_>, fd: Fd, buf: UserSliceMut) -> SysResult<u32> { Err(Errno::ENOSYS.into()) }",
            "async fn openat(&self, caller: &Caller<'_>, dfd: Fd, path: Cstr) -> SysResult<Fd>",
            "async fn exit_group(&self, caller: &Caller<'_>, error_code: i32) -> SysResult<()>",
            "async fn getpid(&self, caller: &Caller<'_>) -> SysResult<i32>",
            "Read { fd: Fd, buf: UserSliceMut },",
            "Getpid,\n",
            "ExitGroup,\n",
            "Read(u32),",
            "Syscall::ExitGroup { error_code } => { syscalls.exit_group(caller, error_code).await?; SyscallRet::ExitGroup }",
            "Syscall::Getpid => SyscallRet::Getpid(syscalls.getpid(caller).await?),",
            "SyscallRet::Openat(ret) => Ok(ret),",
            "_ => mismatch(\"exit_group\"),",
            "/// Wasm import `m.read` (riscv32 nr 63).",
            "use wasixv2_abi::{Cstr, Errno, Fd};",
            "use crate::{caller::Caller, memory::UserSliceMut, result::SysResult};",
        ] {
            assert!(out.contains(want), "missing `{want}` in:\n{out}");
        }
    }

    #[test]
    fn rejects_unusable_shapes() {
        let e = |funcs: &str| format!("{:#}", generate(&model(funcs), "t.wit").unwrap_err());
        assert!(e("x: func(a-b: s32, A-B: s32) -> slong;").contains("duplicate `a_b`"));
        assert!(e("x: func();").contains("is not `@noreturn`"));
        assert!(e("imports: func() -> slong;").contains("reserved by generated code"));
        let e = e("x-y: func() -> slong;\n/// @import(z)\nX-Y: func() -> slong;");
        assert!(e.contains("both map to Rust method `x_y`"), "{e}");
    }

    #[test]
    fn one_import_module() {
        let src = format!(
            "{TYPES}/// @module(a)\ninterface a {{\nx: func() -> s32;\n}}\n/// @module(b)\ninterface b {{\ny: func() -> s32;\n}}\n"
        );
        let m = parse_str("t.wit", &src, Target::WASM32).unwrap();
        let e = format!("{:#}", generate(&m, "t.wit").unwrap_err());
        assert!(e.contains("imported from both `a` and `b`"), "{e}");
    }
}
