//! [`Model`] -> Rust source for `wasixv2-exec`: the import module name, `imports()` and one async
//! import shim per function. A shim decodes the wasm32 arguments, calls the `Syscalls` method,
//! then takes the return-to-guest path.

use std::fmt::Write as _;

use anyhow::{Result, bail};

use crate::ir::Model;
use crate::rust::{header, prim};
use crate::shape::{Method, Param, decode, decode_types, encode, methods, slice_type, uses_slice};

/// Items the generated module names, which ABI types must not shadow.
const LOCAL_ITEMS: &[&str] = &[
    "Arc",
    "AsyncFunctionEnvMut",
    "Caller",
    "Control",
    "ExecEnv",
    "Function",
    "FunctionEnv",
    "GuestMemory",
    "Imports",
    "RuntimeError",
    "Store",
    "Syscalls",
    "TaskId",
    "UserSlice",
    "UserSliceMut",
];

/// Generates the (unformatted) module for `model`.
pub fn generate(model: &Model, source: &str) -> Result<String> {
    let methods = methods(model)?;
    let types = decode_types(model, &methods);
    if let Some(t) = types.iter().find(|t| LOCAL_ITEMS.contains(&t.as_str())) {
        bail!("ABI type `{t}` collides with an item of wasixv2-exec");
    }
    let mut g = Gen {
        model,
        out: String::new(),
    };
    g.out.push_str(&header(source));
    g.prelude(&methods, &types);
    g.imports(&methods);
    g.out.push_str(CALLER_PARTS);
    for m in &methods {
        g.shim(m);
    }
    Ok(g.out)
}

struct Gen<'a> {
    model: &'a Model,
    out: String,
}

impl Gen<'_> {
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn prelude(&mut self, methods: &[Method], types: &std::collections::BTreeSet<String>) {
        self.line("\nuse std::sync::Arc;\n");
        if !types.is_empty() {
            let types: Vec<&str> = types.iter().map(String::as_str).collect();
            let _ = writeln!(self.out, "use wasixv2_abi::{{{}}};", types.join(", "));
        }
        let mut items = vec!["Caller", "Control", "Syscalls", "TaskId"];
        items.extend(
            [false, true]
                .into_iter()
                .filter(|m| uses_slice(methods, *m))
                .map(slice_type),
        );
        let _ = writeln!(self.out, "use wasixv2_syscalls::{{{}}};", items.join(", "));
        self.line(
            "use wasmer::{AsyncFunctionEnvMut, Function, FunctionEnv, Imports, RuntimeError, Store};",
        );
        self.line("");
        self.line("use crate::{ExecEnv, memory::GuestMemory, return_to_guest};");
        let module = methods.first().map_or("", |m| m.module);
        self.line("");
        self.line("/// Name of the import module that holds every syscall and extension.");
        let _ = writeln!(self.out, "pub const IMPORT_MODULE: &str = \"{module}\";");
    }

    fn imports(&mut self, methods: &[Method]) {
        self.line("");
        self.line(
            "pub(crate) fn imports<S>(store: &mut Store, env: &FunctionEnv<ExecEnv<S>>) -> Imports",
        );
        self.line("where S: Syscalls + Control + Send + Sync + 'static,");
        self.line("{");
        self.line("let mut imports = Imports::new();");
        for m in methods {
            let _ = writeln!(
                self.out,
                "imports.define(IMPORT_MODULE, \"{}\", Function::new_typed_with_env_async(store, env, {}::<S>));",
                m.f.import_name, m.name
            );
        }
        self.line("imports");
        self.line("}");
    }

    fn shim(&mut self, m: &Method) {
        let mut params = vec!["env: AsyncFunctionEnvMut<ExecEnv<S>>".to_string()];
        params.extend(
            m.raw
                .iter()
                .map(|r| format!("{}: {}", r.name, prim(r.prim))),
        );
        let result = if m.ret.is_some() {
            prim(m.raw_ret)
        } else {
            "()"
        };
        self.line("");
        if params.len() > 7 {
            self.line("#[allow(clippy::too_many_arguments)]");
        }
        let _ = writeln!(
            self.out,
            "async fn {}<S>({}) -> Result<{result}, RuntimeError>",
            m.name,
            params.join(", ")
        );
        self.line("where S: Syscalls + Control + 'static,");
        self.line("{");
        self.line("let (syscalls, task) = caller_parts(&env).await;");
        self.line("let memory = GuestMemory { env: &env };");
        self.line("let caller = Caller::new(task, &memory);");
        // Qualified, since method syntax would find `Arc::clone` before `Syscalls::clone`.
        let mut args = vec!["&*syscalls".to_string(), "&caller".to_string()];
        args.extend(m.params.iter().map(|p| self.arg(m, p)));
        let _ = writeln!(
            self.out,
            "let ret = Syscalls::{}({}).await;",
            m.name,
            args.join(", ")
        );
        match &m.ret {
            Some(t) => {
                let ret = match encode(self.model, t, "v", m.raw_ret) {
                    Some(expr) => format!("ret.map(|v| {expr})"),
                    None => "ret".to_string(),
                };
                let _ = writeln!(self.out, "return_to_guest(&*syscalls, task, {ret})");
            }
            None => {
                // TODO: a vfork child's `exit`, `exit_group` and successful `execve` resume the
                // parent, so the call returns to the guest; the return path needs a
                // `GuestAction` for that instead of this trap.
                self.line("return_to_guest(&*syscalls, task, ret.map(|()| 0))?;");
                let _ = writeln!(
                    self.out,
                    "Err(RuntimeError::new(\"`{}` returned to the guest\"))",
                    m.f.import_name
                );
            }
        }
        self.line("}");
    }

    fn arg(&self, m: &Method, p: &Param) -> String {
        match p {
            Param::Value { raw, .. } => decode(self.model, &m.raw[*raw]),
            Param::Slice {
                mutable, ptr, len, ..
            } => format!(
                "{}::new({} as u32, {} as u32)",
                slice_type(*mutable),
                m.raw[*ptr].name,
                m.raw[*len].name
            ),
        }
    }
}

const CALLER_PARTS: &str = r#"
async fn caller_parts<S: 'static>(env: &AsyncFunctionEnvMut<ExecEnv<S>>) -> (Arc<S>, TaskId) {
    let env = env.read().await;
    (env.data().syscalls.clone(), env.data().task)
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscalls::tests::model;

    #[test]
    fn shims_decode_and_encode() {
        let out = generate(
            &model(
                "/// @slice(buf, count)\n/// @returns(u32)\nread: func(fd: fd, buf: buf, count: usize) -> slong;\n\
                 /// @returns(fd)\nopenat: func(dfd: fd, path: cstr, mode: u32, pos: s64) -> slong;\n\
                 /// @noreturn\nexit-group: func(error-code: s32);\ngetpid: func() -> slong;\n\
                 lseek: func(fd: fd, offset: s64, whence: u32) -> s64;",
            ),
            "t.wit",
        )
        .unwrap();
        for want in [
            "pub const IMPORT_MODULE: &str = \"m\";",
            "imports.define(IMPORT_MODULE, \"exit_group\", Function::new_typed_with_env_async(store, env, exit_group::<S>));",
            "async fn read<S>(env: AsyncFunctionEnvMut<ExecEnv<S>>, fd: i32, buf: i32, count: i32) -> Result<i32, RuntimeError>",
            "let ret = Syscalls::read(&*syscalls, &caller, Fd(fd), UserSliceMut::new(buf as u32, count as u32)).await;",
            "return_to_guest(&*syscalls, task, ret.map(|v| v as i32))",
            "let ret = Syscalls::openat(&*syscalls, &caller, Fd(dfd), Cstr::new(path as u32), mode as u32, pos).await;",
            "return_to_guest(&*syscalls, task, ret.map(|v| v.0))",
            "async fn exit_group<S>(env: AsyncFunctionEnvMut<ExecEnv<S>>, error_code: i32) -> Result<(), RuntimeError>",
            "Err(RuntimeError::new(\"`exit_group` returned to the guest\"))",
            "return_to_guest(&*syscalls, task, ret)\n",
            "async fn lseek<S>(env: AsyncFunctionEnvMut<ExecEnv<S>>, fd: i32, offset: i64, whence: i32) -> Result<i64, RuntimeError>",
            "use wasixv2_abi::{Cstr, Fd};",
            "use wasixv2_syscalls::{Caller, Control, Syscalls, TaskId, UserSliceMut};",
        ] {
            assert!(out.contains(want), "missing `{want}` in:\n{out}");
        }
    }
}
