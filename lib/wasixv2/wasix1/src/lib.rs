//! The WASIX v1 guest ABI (`wasi_snapshot_preview1`, `wasi_unstable`, `wasix_32v1`,
//! nominal `wasix_64v1` and `wasi`) over memory32, served by the WASIX v2 kernel.
//!
//! [`Wasix1`] is a second [`Abi`] of the execution layer. Its import shims are clients of the
//! [`Syscalls`] trait, like the generated `wasixv2` ones, so the kernel never learns about v1 and
//! the trace, record and policy layers see v1 programs as the v2 calls they make. Everything v1
//! needs that Linux does not have (preopens, rights, v1 layouts and errnos, the startup block as
//! WASI's `args_get`) lives here.
//!
//! # Layout of the crate
//!
//! - [`abi`]: v1 constants and struct layouts ([`abi::GuestLayout`] impls with explicit
//!   offsets); `tests/layout.rs` checks them against `wasmer-wasix-types`.
//! - [`imports`]: the table of every v1 import with its signature and stub.
//! - [`shim`]: [`shim::Frame`] and [`shim::call`], the pattern every import follows.
//! - [`memory`]: [`memory::CompatMemory`], the guest memory plus a scratch window for layout
//!   translation.
//! - [`process`]: per-process compat state ([`process::ProcState`]).
//! - `calls/`: the shims, one file per syscall family.
//!
//! # Conventions for shims and tests
//!
//! - A shim is `async fn name<S: Syscalls + Control + 'static>(env: Env<S>, args...) ->
//!   Result<i32, RuntimeError>` whose body is one [`shim::errno`] (or [`shim::void`]) call; see
//!   [`shim`]. Arguments arrive as the wasm types in [`imports::TABLE`].
//! - Map v1 semantics onto Linux ones in the shim; never add v1 behaviour to the kernel.
//! - Unit tests for translation tables sit next to the code (`#[cfg(test)] mod tests`). Guest
//!   tests are WAT modules in `tests/<family>.rs`, run through `wasixv2::Machine` as
//!   `tests/guest.rs` does; they import the v1 modules and report through their exit status
//!   and stdout. No prebuilt wasm is checked in.

pub mod abi;
mod binding;
mod calls;
pub mod errno;
pub mod http;
pub mod imports;
mod instance;
pub mod memory;
pub mod process;
mod rights;
pub mod shim;
mod vectors;

use std::sync::{Arc, RwLock};

pub use instance::{SIGNAL_CALLBACK, THREAD_ENTRY};
pub use process::{CommandDefaults, Commands, FileId};
use wasixv2_exec::{Abi, AbiInstance, ExecEnv, ExecError, Role};
use wasixv2_syscalls::{Control, Syscalls, TaskId};
use wasmer::{FunctionEnv, Imports, Instance, Module, Store};

use crate::{imports::Module as V1Module, instance::Wasix1Instance, process::Procs};

/// Immutable v1 allocation/request ceilings, independent of kernel resource limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Original subscriptions in one poll call, before deduplication; None is unlimited.
    pub max_poll_subscriptions: Option<u32>,
    /// Sum of requested receive iovec bytes; None leaves the kernel transfer bounds.
    pub max_sock_recv_size: Option<u64>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_poll_subscriptions: Some(1024),
            max_sock_recv_size: Some(16 * 1024 * 1024),
        }
    }
}

/// The WASIX v1 ABI. Register it with [`wasixv2_exec::Abis::push`]; one value serves every
/// process of one kernel.
#[derive(Default, Clone)]
pub struct Wasix1 {
    procs: Arc<Procs>,
    commands: Arc<RwLock<Commands>>,
    http: Option<Arc<dyn http::HttpProvider>>,
    limits: Limits,
}

impl Wasix1 {
    pub fn new() -> Self {
        Self::default()
    }

    /// The ABI for a machine with package commands: a v1 program that starts one of the files
    /// in `commands` passes it the command's [`CommandDefaults`], as v1 did. The embedder may
    /// change `commands` at any time.
    pub fn with_commands(commands: Arc<RwLock<Commands>>) -> Self {
        Self {
            procs: Arc::default(),
            commands,
            http: None,
            limits: Limits::default(),
        }
    }

    /// Sets immutable import request ceilings, retained by every image and thread binding.
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Enables the historical v1 HTTP extension with an explicit embedder capability.
    /// Browser providers must complete their I/O on an independent event-loop worker.
    pub fn with_http_provider(mut self, provider: Arc<dyn http::HttpProvider>) -> Self {
        self.http = Some(provider);
        self
    }
}

impl<S> Abi<S> for Wasix1
where
    S: Syscalls + Control + Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        "wasix_32v1"
    }

    fn matches(&self, module: &Module) -> bool {
        matches!(
            module_flavor(module),
            Some(V1Flavor::Wasm32 | V1Flavor::Wasm64 | V1Flavor::HttpClient)
        )
    }

    fn matches_image(&self, image: &[u8]) -> bool {
        matches!(
            is_wasix1_module(image),
            Some(V1Flavor::Wasm32 | V1Flavor::Wasm64 | V1Flavor::HttpClient)
        )
    }

    fn for_image(&self, module: &Module) -> Option<Arc<dyn Abi<S>>> {
        <Self as Abi<S>>::matches(self, module).then_some(())?;
        wasixv2_dylink::compose(
            module,
            Arc::new(self.clone()),
            Arc::new(calls::dynamic::Policy),
        )
    }

    fn imports(
        &self,
        store: &mut Store,
        env: &FunctionEnv<ExecEnv<S>>,
        module: &Module,
    ) -> Imports {
        imports::imports(store, env, module)
    }

    fn bind(
        &self,
        instance: &Instance,
        _store: &mut Store,
        task: TaskId,
        role: Role,
    ) -> Result<Arc<dyn AbiInstance<S>>, ExecError> {
        let (procs, commands) = (self.procs.clone(), self.commands.clone());
        let bound = Wasix1Instance::bind(
            procs,
            commands,
            self.http.clone(),
            self.limits,
            instance,
            _store,
            task,
            role,
        )?;
        Ok(Arc::new(bound))
    }
}

/// Which kind of WASIX v1 module a module is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V1Flavor {
    /// wasm32, served by [`Wasix1`].
    Wasm32,
    /// `wasix_64v1` arguments and layouts over memory32.
    Wasm64,
    /// An actual Memory64 linear memory, unsupported by this execution profile.
    Memory64,
    /// Imports the historical `wasix_http_client_v1` extension, served by an embedder provider.
    HttpClient,
}

impl V1Flavor {
    /// Why the flavour cannot run, or `None` for supported memory32 profiles.
    pub fn unsupported_reason(self) -> Option<&'static str> {
        match self {
            Self::Wasm32 | Self::Wasm64 | Self::HttpClient => None,
            Self::Memory64 => Some("Memory64 linear memories are not supported"),
        }
    }
}

/// Whether `wasm` is a WASIX v1 (or plain WASI) module, and which kind. Its import
/// and memory headers are parsed before code; malformed headers, a `wasixv2` module or
/// one without v1 imports produce `None`.
pub fn is_wasix1_module(wasm: &[u8]) -> Option<V1Flavor> {
    use wasmparser::{Parser, Payload, TypeRef};

    let mut namespaces = None;
    let mut memory64 = false;
    for payload in Parser::new(0).parse_all(wasm) {
        match payload {
            Ok(Payload::ImportSection(imports)) => {
                let mut modules = Vec::new();
                for import in imports.into_imports() {
                    let import = import.ok()?;
                    if let TypeRef::Memory(memory) = import.ty {
                        memory64 |= memory.memory64;
                    }
                    modules.push(import.module.to_owned());
                }
                namespaces = flavor(modules.into_iter());
            }
            Ok(Payload::MemorySection(memories)) => {
                for memory in memories {
                    memory64 |= memory.ok()?.memory64;
                }
            }
            Ok(Payload::CodeSectionStart { .. }) => break,
            Err(_) => return None,
            Ok(_) => {}
        }
    }
    namespaces.map(|flavor| if memory64 { V1Flavor::Memory64 } else { flavor })
}

/// [`is_wasix1_module`] for a compiled module, e.g. one deserialized from an artifact.
pub fn module_flavor(module: &Module) -> Option<V1Flavor> {
    flavor(module.imports().map(|import| import.module().to_owned()))
}

/// The flavour of a module importing from `modules`.
fn flavor(modules: impl Iterator<Item = String>) -> Option<V1Flavor> {
    let mut v1 = false;
    let mut flavor = V1Flavor::Wasm32;
    for module in modules {
        match module.as_str() {
            wasixv2_exec::IMPORT_MODULE => return None,
            "wasix_64v1" => flavor = V1Flavor::Wasm64,
            "wasix_http_client_v1" if flavor == V1Flavor::Wasm32 => {
                flavor = V1Flavor::HttpClient;
            }
            name if V1Module::from_name(name)
                .is_some_and(|module| module != V1Module::Canonical) =>
            {
                v1 = true
            }
            _ => {}
        }
    }
    (v1 || flavor != V1Flavor::Wasm32).then_some(flavor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modules(names: &[&str]) -> impl Iterator<Item = String> {
        names
            .iter()
            .map(|name| name.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn flavors() {
        assert_eq!(
            flavor(modules(&["wasi_snapshot_preview1", "env"])),
            Some(V1Flavor::Wasm32)
        );
        assert_eq!(
            flavor(modules(&["wasix_32v1", "wasi"])),
            Some(V1Flavor::Wasm32)
        );
        assert_eq!(flavor(modules(&["wasix_64v1"])), Some(V1Flavor::Wasm64));
        assert_eq!(
            flavor(modules(&["wasi_snapshot_preview1", "wasix_http_client_v1"])),
            Some(V1Flavor::HttpClient)
        );
        assert_eq!(
            flavor(modules(&["wasixv2", "wasi_snapshot_preview1"])),
            None
        );
        assert_eq!(flavor(modules(&["env"])), None);
        assert_eq!(flavor(modules(&["canonical_abi"])), None);
    }

    #[test]
    fn nominal64_namespaces_do_not_imply_memory64() {
        for (memory, expected) in [
            ("(memory (export \"memory\") 1)", V1Flavor::Wasm64),
            ("(memory (export \"memory\") i64 1)", V1Flavor::Memory64),
            (
                "(import \"env\" \"memory\" (memory i64 1))",
                V1Flavor::Memory64,
            ),
        ] {
            let bytes = wat::parse_str(format!(
                "(module (import \"wasix_64v1\" \"proc_exit\" (func (param i32))) {memory})"
            ))
            .unwrap();
            assert_eq!(is_wasix1_module(&bytes), Some(expected));
        }
    }

    #[test]
    fn detects_from_bytes() {
        let wasm =
            wat::parse_str(r#"(module (import "wasi_unstable" "proc_exit" (func (param i32))))"#)
                .unwrap();
        assert_eq!(is_wasix1_module(&wasm), Some(V1Flavor::Wasm32));
    }
}
