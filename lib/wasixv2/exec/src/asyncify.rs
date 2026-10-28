//! The asyncify build configuration: binaryen's `--asyncify` lets the host unwind the guest's
//! wasm call stack into linear memory and rewind it from there. A wasm call stack cannot be
//! copied or returned to otherwise, and fork and `setjmp` without wasm exceptions need both.
//!
//! This module is the ABI-neutral machinery; the imports that use it live with their ABI
//! (wasixv2's in `fork.rs`, other ABIs' through [`import`]).
//!
//! - **Levels.** Every call the host makes into an asyncify instance goes through [`call`] and
//!   is one level: the entry point, and the signal handlers and vfork returns the host runs from
//!   inside imports on top of it. An unwind only reaches the innermost level's call, since host
//!   frames sit between levels.
//! - **Unwind and rewind.** An import asks for the stack with [`unwind`] and returns; the guest
//!   unwinds to the level's call, which does the work the [`Request`] names and rewinds by
//!   calling the same function again. The rewinding guest calls the import again, which takes
//!   the [`Resume`] with [`resumed`] and returns what it says.
//! - **Saved stacks.** [`Request::Save`] records the level's stack, and [`restore`] gives the
//!   error that discards the current stack up to that level, which then rewinds the saved one.
//!   That is `longjmp`: memory stays as it is, and the frames from the level's entry to the
//!   save point come back with their wasm locals as they were saved.
//! - **Lifetime of a saved stack.** A save is named by a [`StackId`] with a generation unique
//!   to the instance, which the import hands to the guest (inside its `jmp_buf`), so a copy of
//!   those bytes restores the same save wherever the guest keeps it. A save stays restorable
//!   until its frame returned, which the host sees in three ways: its level returned; a later
//!   save of the level was made with the shadow stack pointer above it, so a frame that held
//!   the save point was popped; or a later save to the same slot came from a stack of the same
//!   shape (stack pointer and unwound size), which is the same `setjmp` run again or a sibling
//!   call, as a nested frame always adds to the unwound stack. See [`Saved::dead_after`].
//! - **Fork.** [`Request::Fork`] copies the instance (memory, exported mutable globals and the
//!   unwound stack) into a [`ForkImage`] that a new instance rewinds as the child. Only the
//!   outermost level can fork: the levels below would be host frames the copy cannot hold.
//!   Ranges of memory that show a shared object are not copied: the child's memory shows the
//!   same object there. Ranges `MADV_WIPEONFORK` marked are not copied either: they read zero
//!   in the child.

use std::{any::Any, collections::HashMap, future::Future, ops::Range, sync::Arc};

use wasixv2_syscalls::{SharedMapping, TaskId};
use wasmer::{
    AsStoreAsync, AsStoreMut, AsyncFunctionEnvMut, Function, FunctionEnv, Global, Instance, Module,
    Mutability, RuntimeError, StoreAsync, StoreMut, TypedFunction, Value,
};

use crate::{ExecEnv, memory_image::MemoryImage};

/// Pages of linear memory an instance takes, on its first unwind, for its unwound stacks. An
/// unwind that needs more traps.
const BUFFER_PAGES: u32 = 16;
const BUFFER_SIZE: u32 = BUFFER_PAGES * 64 * 1024;
/// asyncify's data header: the position, where an unwind pushes and a rewind pops, and the end.
const HEADER: u32 = 8;

const START_UNWIND: &str = "asyncify_start_unwind";
const STOP_UNWIND: &str = "asyncify_stop_unwind";
const START_REWIND: &str = "asyncify_start_rewind";
const STOP_REWIND: &str = "asyncify_stop_rewind";

/// The shadow stack pointer, which a saved stack restores and a fork child takes over.
const STACK_POINTER: &str = "__stack_pointer";

/// Whether `module` was instrumented with binaryen's asyncify in a way this machinery can use:
/// it exports asyncify's functions and its shadow stack pointer, as libc-built modules do. A
/// module that keeps the stack pointer to itself runs as an uninstrumented one, since neither
/// a saved stack nor a fork child could get its stack pointer back.
pub(crate) fn instrumented(module: &Module) -> bool {
    let has = |name: &str| module.exports().any(|export| export.name() == name);
    [
        START_UNWIND,
        STOP_UNWIND,
        START_REWIND,
        STOP_REWIND,
        STACK_POINTER,
    ]
    .into_iter()
    .all(has)
}

/// What an unwind is for, as the import that started it asked.
pub(crate) enum Request {
    /// Record the stack, saved from `slot`, a guest address the import picks (such as its
    /// `jmp_buf`).
    Save { slot: u32 },
    /// Copy the instance for the child task `child`, whose memory shows the objects of `shared`
    /// where the parent's does and reads zero in `wiped`. `tag` comes back to the import on both
    /// sides of the fork.
    Fork {
        child: TaskId,
        tag: Tag,
        shared: Vec<SharedMapping>,
        wiped: Vec<Range<u64>>,
    },
}

/// Data an import passes through a fork to its resumption on both sides.
pub type Tag = Arc<dyn Any + Send + Sync>;

/// What a rewound import returns, after the work its [`Request`] asked for.
pub(crate) enum Resume {
    /// The stack was saved under this id.
    Saved(StackId),
    /// [`restore`] rewound the stack saved here, with this value.
    Restored(i32),
    /// The instance was copied: `child` is the child task in the parent, and `None` in the
    /// child.
    Forked { child: Option<TaskId>, tag: Tag },
}

/// Names one saved stack: the slot it was saved from, its generation, unique in the instance,
/// and its level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackId {
    pub slot: u32,
    pub generation: u64,
    pub level: u32,
}

/// One saved stack: the unwound data and the shadow stack pointer of the save point.
#[derive(Debug)]
pub(crate) struct Saved {
    id: StackId,
    data: Vec<u8>,
    stack_pointer: Value,
}

impl Saved {
    /// Whether a later save of the same level, from `slot` with the shadow stack at
    /// `stack_pointer` and `len` bytes unwound, shows that this save's frame returned.
    ///
    /// Both stack pointers are taken in the saving import, under the same guest function
    /// (libc's `setjmp`), so they compare as the frames that called it. The shadow stack grows
    /// down: a save above this one was made after the frame that held this save point was
    /// popped. At the same stack pointer and slot, a nested frame would have added to the
    /// unwound stack, so a stack of the same size is the same `setjmp` run again or a sibling
    /// call of it.
    fn dead_after(&self, slot: u32, stack_pointer: u64, len: usize) -> bool {
        let own = address(&self.stack_pointer);
        own < stack_pointer
            || (own == stack_pointer && self.id.slot == slot && self.data.len() == len)
    }
}

/// The address a stack pointer global holds.
fn address(value: &Value) -> u64 {
    match *value {
        Value::I32(addr) => u64::from(addr as u32),
        Value::I64(addr) => addr as u64,
        _ => 0,
    }
}

/// Raised to discard the stack up to the level that saved `saved`, which rewinds it.
#[derive(Debug, thiserror::Error)]
#[error("restoring a saved guest stack")]
struct Restore {
    saved: Arc<Saved>,
    val: i32,
}

/// One asyncify instance's exports and unwind state.
pub(crate) struct Asyncify {
    start_unwind: TypedFunction<i32, ()>,
    stop_unwind: TypedFunction<(), ()>,
    start_rewind: TypedFunction<i32, ()>,
    stop_rewind: TypedFunction<(), ()>,
    stack_pointer: Global,
    /// Exported mutable globals of a numeric type, which a fork copies.
    globals: Vec<(String, Global)>,
    /// The guest address of the unwind buffer (header and data), once taken.
    buffer: Option<u32>,
    /// How many levels are running.
    depth: u32,
    unwinding: Option<Request>,
    resume: Option<Resume>,
    /// Saved stacks by generation; see [`Saved::dead_after`] for when they go.
    saved: HashMap<u64, Arc<Saved>>,
    generation: u64,
}

impl Asyncify {
    /// The asyncify state of `instance`, or `None` if its module is not [`instrumented`].
    pub(crate) fn new(
        instance: &Instance,
        store: &mut impl AsStoreMut,
    ) -> Result<Option<Self>, wasmer::ExportError> {
        if !instrumented(instance.module()) {
            return Ok(None);
        }
        let exports = &instance.exports;
        let globals = exports
            .iter()
            .filter_map(|(name, export)| match export {
                wasmer::Extern::Global(global) => Some((name.clone(), global.clone())),
                _ => None,
            })
            .filter(|(_, global)| {
                let ty = global.ty(store);
                ty.mutability == Mutability::Var && ty.ty.is_num()
            })
            .collect();
        Ok(Some(Self {
            start_unwind: exports.get_typed_function(store, START_UNWIND)?,
            stop_unwind: exports.get_typed_function(store, STOP_UNWIND)?,
            start_rewind: exports.get_typed_function(store, START_REWIND)?,
            stop_rewind: exports.get_typed_function(store, STOP_REWIND)?,
            stack_pointer: exports.get_global(STACK_POINTER)?.clone(),
            globals,
            buffer: None,
            depth: 0,
            unwinding: None,
            resume: None,
            saved: HashMap::new(),
            generation: 0,
        }))
    }

    /// Whether the guest's stack is being unwound or rewound right now.
    pub(crate) fn moving(&self) -> bool {
        self.unwinding.is_some() || self.resume.is_some()
    }

    /// Takes over a parent's state in a forked child, which rewinds as `resume` says.
    fn inherit(&mut self, from: &ForkState) {
        self.buffer = Some(from.buffer);
        self.saved = from.saved.clone();
        self.generation = from.generation;
        self.resume = Some(Resume::Forked {
            child: None,
            tag: from.tag.clone(),
        });
    }
}

/// How the execution layer reaches an instance: from outside any guest call (its function
/// environment and store), or from an import (its async environment).
pub(crate) trait Access<S: 'static> {
    fn store(&self) -> StoreAsync;
    fn with<R>(
        &self,
        f: impl FnOnce(&mut ExecEnv<S>, &mut StoreMut<'_>) -> R,
    ) -> impl Future<Output = R>;
}

/// An instance seen from outside any guest call.
pub(crate) struct Outside<'a, S: 'static> {
    pub(crate) env: &'a FunctionEnv<ExecEnv<S>>,
    pub(crate) store: &'a StoreAsync,
}

impl<S: Send + Sync + 'static> Access<S> for Outside<'_, S> {
    fn store(&self) -> StoreAsync {
        self.store.store()
    }

    async fn with<R>(&self, f: impl FnOnce(&mut ExecEnv<S>, &mut StoreMut<'_>) -> R) -> R {
        let mut lock = self.store.write_lock().await;
        let mut env = self.env.clone().into_mut(&mut lock);
        let (data, mut store) = env.data_and_store_mut();
        f(data, &mut store)
    }
}

impl<S: 'static> Access<S> for AsyncFunctionEnvMut<ExecEnv<S>> {
    fn store(&self) -> StoreAsync {
        self.as_store_async().store()
    }

    async fn with<R>(&self, f: impl FnOnce(&mut ExecEnv<S>, &mut StoreMut<'_>) -> R) -> R {
        let mut guard = self.write().await;
        let (data, store) = guard.data_and_store_mut();
        f(data, &mut store.as_store_mut())
    }
}

/// Whether an import running now could have the whole stack unwound: the instance is
/// instrumented and the import runs in the outermost level.
pub(crate) async fn whole_stack<S: 'static>(cx: &impl Access<S>) -> bool {
    cx.with(|env, _| env.asyncify.as_ref().is_some_and(|a| a.depth == 1))
        .await
}

/// Calls `function` as one level of guest calls on the instance behind `cx` and carries out
/// the unwinds and rewinds that its imports ask for, until it returns or fails for good.
/// `entry` names `function` as an export, which a fork child calls to rewind; only an entry
/// point has one.
pub(crate) async fn call<S: 'static>(
    cx: &impl Access<S>,
    entry: Option<&'static str>,
    function: &Function,
    args: &[Value],
) -> Result<Box<[Value]>, RuntimeError> {
    let level = cx
        .with(|env, _| {
            env.asyncify.as_mut().map(|a| {
                a.depth += 1;
                a.depth
            })
        })
        .await;
    let Some(level) = level else {
        return function.call_async(&cx.store(), args.to_vec()).await;
    };
    let result = loop {
        let result = function.call_async(&cx.store(), args.to_vec()).await;
        match settle(cx, level, entry, args, result).await {
            Ok(None) => continue,
            Ok(Some(values)) => break Ok(values),
            Err(err) => break Err(err),
        }
    };
    cx.with(|env, _| {
        let a = env.asyncify.as_mut().expect("entered above");
        a.depth -= 1;
        if result.is_err() {
            // An unwind or rewind the error cut short (a trap on the way, an unwound stack
            // that did not fit) never finishes; left set, it would keep the preemption points
            // from delivering for the rest of the instance's life.
            a.unwinding = None;
            a.resume = None;
        }
        // Stacks saved in this level cannot be restored once it returned.
        a.saved.retain(|_, saved| saved.id.level < level);
    })
    .await;
    result
}

/// After one call of a level's function: its values, `None` to call it again because a rewind
/// started, or the error that ends the level.
async fn settle<S: 'static>(
    cx: &impl Access<S>,
    level: u32,
    entry: Option<&'static str>,
    args: &[Value],
    result: Result<Box<[Value]>, RuntimeError>,
) -> Result<Option<Box<[Value]>>, RuntimeError> {
    let values = match result {
        Ok(values) => values,
        Err(err) => match err.downcast::<Restore>() {
            Ok(restore) if restore.saved.id.level == level => {
                rewind_saved(cx, &restore.saved, restore.val).await?;
                return Ok(None);
            }
            Ok(restore) => return Err(RuntimeError::user(Box::new(restore))),
            Err(err) => return Err(err),
        },
    };
    let Some((request, functions)) = cx
        .with(|env, _| {
            let a = env.asyncify.as_mut()?;
            Some((
                a.unwinding.take()?,
                (a.stop_unwind.clone(), a.start_rewind.clone()),
            ))
        })
        .await
    else {
        return Ok(Some(values));
    };
    let (stop_unwind, start_rewind) = functions;
    stop_unwind.call_async(&cx.store()).await?;
    let resume = match request {
        Request::Save { slot } => Resume::Saved(save(cx, level, slot).await?),
        Request::Fork {
            child,
            tag,
            shared,
            wiped,
        } => {
            let entry = entry.filter(|_| level == 1).ok_or_else(|| {
                RuntimeError::new("fork from a guest call that is not the entry point")
            })?;
            let image = snapshot(cx, entry, args, tag.clone(), shared, &wiped).await?;
            let spawner = cx.with(|env, _| env.proc.spawner.clone()).await;
            spawner.fork(child, image);
            Resume::Forked {
                child: Some(child),
                tag,
            }
        }
    };
    let buffer = cx
        .with(|env, _| {
            let a = env.asyncify.as_mut().expect("unwound above");
            a.resume = Some(resume);
            a.buffer.expect("an unwind has a buffer")
        })
        .await;
    start_rewind.call_async(&cx.store(), buffer as i32).await?;
    Ok(None)
}

/// Records the just unwound stack of `level`, saved from `slot`, and drops the saves of the
/// level whose frames this shows returned.
async fn save<S: 'static>(
    cx: &impl Access<S>,
    level: u32,
    slot: u32,
) -> Result<StackId, RuntimeError> {
    cx.with(|env, store| {
        let memory = env.memory.clone().expect("an instance has memory");
        let a = env.asyncify.as_mut().expect("unwound above");
        let buffer = a.buffer.expect("an unwind has a buffer");
        let read = |addr: u32, buf: &mut [u8]| {
            crate::memory::read(&memory, &*store, addr, buf).map_err(fault)
        };
        let mut header = [0u8; 4];
        read(buffer, &mut header)?;
        let end = u32::from_le_bytes(header);
        let mut data = vec![0; end.saturating_sub(buffer + HEADER) as usize];
        read(buffer + HEADER, &mut data)?;
        a.generation += 1;
        let id = StackId {
            slot,
            generation: a.generation,
            level,
        };
        let stack_pointer = a.stack_pointer.get(store);
        let sp = address(&stack_pointer);
        a.saved
            .retain(|_, old| old.id.level != level || !old.dead_after(slot, sp, data.len()));
        a.saved.insert(
            id.generation,
            Arc::new(Saved {
                id,
                data,
                stack_pointer,
            }),
        );
        Ok(id)
    })
    .await
}

/// Puts `saved` back into the unwind buffer and starts rewinding it; the import that saved it
/// resumes with `val`.
async fn rewind_saved<S: 'static>(
    cx: &impl Access<S>,
    saved: &Saved,
    val: i32,
) -> Result<(), RuntimeError> {
    let (start_rewind, buffer) = cx
        .with(|env, store| {
            let memory = env.memory.clone().expect("an instance has memory");
            let (low_guard, read_only) = (env.low_guard, env.read_only.clone());
            let a = env.asyncify.as_mut().expect("saved stacks are asyncify's");
            let buffer = a.buffer.expect("a saved stack went through the buffer");
            let end = buffer + HEADER + saved.data.len() as u32;
            let write = |addr: u32, bytes: &[u8]| {
                crate::memory::write(&memory, low_guard, &read_only, &*store, addr, bytes)
                    .map_err(fault)
            };
            let mut header = [0u8; HEADER as usize];
            header[..4].copy_from_slice(&end.to_le_bytes());
            header[4..].copy_from_slice(&(buffer + BUFFER_SIZE).to_le_bytes());
            write(buffer, &header)?;
            write(buffer + HEADER, &saved.data)?;
            a.stack_pointer.set(store, saved.stack_pointer.clone())?;
            a.resume = Some(Resume::Restored(val));
            Ok::<_, RuntimeError>((a.start_rewind.clone(), buffer))
        })
        .await?;
    start_rewind.call_async(&cx.store(), buffer as i32).await
}

/// Starts unwinding the calling level's stack for `request`. The import returns afterwards
/// (any value), and is called again once the work is done.
pub(crate) async fn unwind<S: 'static>(
    cx: &impl Access<S>,
    request: Request,
) -> Result<(), RuntimeError> {
    let buffer = buffer(cx).await?;
    let start_unwind = cx
        .with(|env, store| {
            let mut header = [0u8; HEADER as usize];
            header[..4].copy_from_slice(&(buffer + HEADER).to_le_bytes());
            header[4..].copy_from_slice(&(buffer + BUFFER_SIZE).to_le_bytes());
            env.write_memory(&*store, buffer, &header).map_err(fault)?;
            let a = env.asyncify.as_mut().ok_or_else(not_instrumented)?;
            a.unwinding = Some(request);
            Ok::<_, RuntimeError>(a.start_unwind.clone())
        })
        .await?;
    start_unwind.call_async(&cx.store(), buffer as i32).await
}

/// The unwind buffer, taken from the top of memory the first time. Pages grown here count as
/// mapped for the kernel, as pages the guest grows itself do.
async fn buffer<S: 'static>(cx: &impl Access<S>) -> Result<u32, RuntimeError> {
    cx.with(|env, store| {
        let memory = env.memory.clone().expect("an instance has memory");
        let a = env.asyncify.as_mut().ok_or_else(not_instrumented)?;
        if let Some(buffer) = a.buffer {
            return Ok(buffer);
        }
        let previous = memory
            .grow(store, BUFFER_PAGES)
            .map_err(|err| RuntimeError::new(format!("no memory for unwinding: {err}")))?;
        let buffer = u32::try_from(previous.bytes().0)
            .map_err(|_| RuntimeError::new("no memory for unwinding"))?;
        a.buffer = Some(buffer);
        Ok(buffer)
    })
    .await
}

/// The [`Resume`] of the import the guest is rewinding into, which ends the rewind; `None` when
/// the import is called normally.
pub(crate) async fn resumed<S: 'static>(
    cx: &impl Access<S>,
) -> Result<Option<Resume>, RuntimeError> {
    let resumed = cx
        .with(|env, _| {
            let a = env.asyncify.as_mut()?;
            Some((a.resume.take()?, a.stop_rewind.clone()))
        })
        .await;
    let Some((resume, stop_rewind)) = resumed else {
        return Ok(None);
    };
    stop_rewind.call_async(&cx.store()).await?;
    Ok(Some(resume))
}

/// The error that discards the guest's stack up to the level that saved `id`, which then
/// rewinds that stack, so that its import resumes with [`Resume::Restored`]`(val)`. The import
/// returns it. Fails if `id` is not a saved stack of a level still running.
pub(crate) async fn restore<S: 'static>(
    cx: &impl Access<S>,
    id: StackId,
    val: i32,
) -> RuntimeError {
    let saved = cx
        .with(|env, _| {
            let a = env.asyncify.as_ref()?;
            let saved = a.saved.get(&id.generation)?;
            (saved.id == id && id.level <= a.depth).then(|| saved.clone())
        })
        .await;
    match saved {
        Some(saved) => RuntimeError::user(Box::new(Restore { saved, val })),
        None => RuntimeError::new("restoring a stack that was not saved, or whose call returned"),
    }
}

/// A copy of an instance for a forked child.
pub(crate) struct ForkImage {
    pub(crate) module: Module,
    /// Linear memory, unwound stack included, but for `shared`.
    pub(crate) memory: MemoryImage,
    /// The ranges of memory that show shared objects, which the child's memory shows too.
    pub(crate) shared: Vec<SharedMapping>,
    /// Exported mutable globals and their values.
    pub(crate) globals: Vec<(String, Value)>,
    /// The entry point the child calls to rewind, and its arguments.
    pub(crate) entry: &'static str,
    pub(crate) args: Vec<Value>,
    state: ForkState,
}

/// The asyncify state a child takes over.
struct ForkState {
    buffer: u32,
    saved: HashMap<u64, Arc<Saved>>,
    generation: u64,
    tag: Tag,
}

impl ForkImage {
    /// Makes `asyncify`, a fresh instance of the image with this memory and these globals,
    /// rewind as the child: it calls `entry` through [`call`], after [`start_rewind`].
    pub(crate) fn adopt(&self, asyncify: &mut Asyncify) {
        asyncify.inherit(&self.state);
    }

    pub(crate) fn buffer(&self) -> u32 {
        self.state.buffer
    }
}

/// Copies the instance behind `cx`, whose outermost level just unwound, but for the ranges of
/// memory that show the objects of `shared`, and with `wiped` reading zero.
async fn snapshot<S: 'static>(
    cx: &impl Access<S>,
    entry: &'static str,
    args: &[Value],
    tag: Tag,
    shared: Vec<SharedMapping>,
    wiped: &[Range<u64>],
) -> Result<ForkImage, RuntimeError> {
    cx.with(|env, store| {
        let module = env
            .proc
            .module
            .clone()
            .ok_or_else(|| RuntimeError::new("fork needs the instance's module"))?;
        let memory = env.memory.clone().expect("an instance has memory");
        let a = env.asyncify.as_ref().expect("unwound above");
        let ranges: Vec<_> = shared
            .iter()
            .map(|mapping| {
                let start = u64::from(mapping.addr);
                start..start + mapping.len
            })
            .collect();
        let image = MemoryImage::capture(&memory, &*store, &ranges, wiped)
            .map_err(|_| RuntimeError::new("cannot copy guest memory"))?;
        let globals = a
            .globals
            .iter()
            .map(|(name, global)| (name.clone(), global.get(store)))
            .collect();
        // Stacks saved in the levels above cannot be restored in the child.
        let saved = a
            .saved
            .iter()
            .filter(|(_, saved)| saved.id.level == 1)
            .map(|(&generation, saved)| (generation, saved.clone()))
            .collect();
        Ok(ForkImage {
            module,
            memory: image,
            shared,
            globals,
            entry,
            args: args.to_vec(),
            state: ForkState {
                buffer: a.buffer.expect("an unwind has a buffer"),
                saved,
                generation: a.generation,
                tag,
            },
        })
    })
    .await
}

/// Starts rewinding a fork child's stack; the child then calls its entry point through
/// [`call`].
pub(crate) async fn start_rewind<S: 'static>(
    cx: &impl Access<S>,
    buffer: u32,
) -> Result<(), RuntimeError> {
    let start_rewind = cx
        .with(|env, _| env.asyncify.as_ref().map(|a| a.start_rewind.clone()))
        .await
        .ok_or_else(not_instrumented)?;
    start_rewind.call_async(&cx.store(), buffer as i32).await
}

fn fault(_: wasixv2_syscalls::Fault) -> RuntimeError {
    RuntimeError::new("unwind buffer outside guest memory")
}

fn not_instrumented() -> RuntimeError {
    RuntimeError::new("the module is not instrumented with asyncify")
}

/// The asyncify machinery for the imports of ABIs other than wasixv2, which reach their
/// instance through its async function environment.
pub mod import {
    use wasixv2_syscalls::TaskId;
    use wasmer::{AsyncFunctionEnvMut, RuntimeError};

    use super::{Request, Resume, StackId, Tag};
    use crate::ExecEnv;

    /// How an import the guest rewinds into ended.
    pub enum Resumed {
        /// [`save`] saved the stack under this id.
        Saved(StackId),
        /// [`restore`] rewound a saved stack with this value.
        Restored(i32),
        /// A fork copied the instance: `child` is the child in the parent and `None` in the
        /// child; `payload` is what the import gave [`set_fork_payload`] before it forked.
        Forked {
            child: Option<TaskId>,
            payload: Option<Tag>,
        },
    }

    /// Whether the instance is instrumented with asyncify.
    pub async fn instrumented<S: 'static>(env: &AsyncFunctionEnvMut<ExecEnv<S>>) -> bool {
        env.read().await.data().asyncify.is_some()
    }

    /// The outcome of the import the guest is rewinding into, which ends the rewind; `None`
    /// when the import is called normally. Every import that unwinds calls this first.
    pub async fn resumed<S: 'static>(
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
    ) -> Result<Option<Resumed>, RuntimeError> {
        Ok(super::resumed(env).await?.map(|resume| match resume {
            Resume::Saved(id) => Resumed::Saved(id),
            Resume::Restored(val) => Resumed::Restored(val),
            Resume::Forked { child, tag } => Resumed::Forked {
                child,
                payload: crate::fork::payload(&tag),
            },
        }))
    }

    /// Starts unwinding the stack to save it from `slot`; the import returns afterwards and
    /// resumes with [`Resumed::Saved`].
    pub async fn save<S: 'static>(
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        slot: u32,
    ) -> Result<(), RuntimeError> {
        super::unwind(env, Request::Save { slot }).await
    }

    /// The error that rewinds the stack saved as `id`, whose import resumes with
    /// [`Resumed::Restored`]`(val)`; the import returns it.
    pub async fn restore<S: 'static>(
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        id: StackId,
        val: i32,
    ) -> RuntimeError {
        super::restore(env, id, val).await
    }

    /// Hands `payload` to both sides of the fork the import is about to start (with `clone`),
    /// in [`Resumed::Forked`].
    pub async fn set_fork_payload<S: 'static>(env: &AsyncFunctionEnvMut<ExecEnv<S>>, payload: Tag) {
        env.write().await.data_mut().fork_payload = Some(payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(slot: u32, stack_pointer: i32, len: usize) -> Saved {
        Saved {
            id: StackId {
                slot,
                generation: 1,
                level: 1,
            },
            data: vec![0; len],
            stack_pointer: Value::I32(stack_pointer),
        }
    }

    #[test]
    fn a_save_from_an_outer_frame_outlives_nested_saves_of_its_slot() {
        // bash's return_catch: the jmp_buf is copied aside and set again in a nested frame,
        // deeper on the shadow stack or, without a frame of its own there, with more unwound.
        let outer = saved(0x100, 0x8000, 64);
        assert!(!outer.dead_after(0x100, 0x7f00, 64));
        assert!(!outer.dead_after(0x100, 0x8000, 96));
    }

    #[test]
    fn a_save_dies_once_its_frame_was_popped() {
        let inner = saved(0x100, 0x7f00, 96);
        assert!(inner.dead_after(0x200, 0x8000, 64));
    }

    #[test]
    fn running_setjmp_again_replaces_the_save() {
        let first = saved(0x100, 0x8000, 64);
        assert!(first.dead_after(0x100, 0x8000, 64));
        // Another jmp_buf set from the same frame does not.
        assert!(!first.dead_after(0x200, 0x8000, 64));
    }

    /// asyncify's exports, with `run` calling an import that asks to save the stack and then
    /// trapping where an instrumented guest would unwind, as one whose unwound stack does not
    /// fit the buffer does.
    const TRAP_WHILE_UNWINDING: &str = r#"
      (module
        (import "test" "save" (func $save))
        (memory (export "memory") 1)
        (global $state (mut i32) (i32.const 0))
        (global (export "__stack_pointer") (mut i32) (i32.const 1024))
        (func (export "asyncify_start_unwind") (param i32) (global.set $state (i32.const 1)))
        (func (export "asyncify_stop_unwind") (global.set $state (i32.const 0)))
        (func (export "asyncify_start_rewind") (param i32) (global.set $state (i32.const 2)))
        (func (export "asyncify_stop_rewind") (global.set $state (i32.const 0)))
        (func (export "run")
          (call $save)
          (if (i32.eq (global.get $state) (i32.const 1)) (then unreachable))))
    "#;

    struct NoSpawn;

    impl crate::process::Spawn for NoSpawn {
        fn spawn(&self, _: TaskId, _: crate::Image) {
            unreachable!("the test spawns nothing")
        }
        fn fork(&self, _: TaskId, _: ForkImage) {
            unreachable!("the test forks nothing")
        }
        fn take_over(&self, _: TaskId, _: crate::Image, _: Arc<crate::process::Handoff>) {
            unreachable!("the test starts no image")
        }
    }

    async fn save(env: AsyncFunctionEnvMut<ExecEnv<()>>) -> Result<(), RuntimeError> {
        unwind(&env, Request::Save { slot: 0 }).await
    }

    #[test]
    fn a_level_that_fails_while_unwinding_leaves_the_instance_still() {
        let mut store = wasmer::Store::default();
        let module = Module::new(&store, wat::parse_str(TRAP_WHILE_UNWINDING).unwrap()).unwrap();
        let env = FunctionEnv::new(
            &mut store,
            ExecEnv {
                syscalls: Arc::new(()),
                task: TaskId(1),
                work: Arc::new(wasixv2_syscalls::WorkFlag::always()),
                memory: None,
                low_guard: 0,
                read_only: Arc::default(),
                stack_pointer: None,
                abi: None,
                threads: None,
                proc: crate::process::ProcExports::new(Arc::new(NoSpawn), Arc::default()),
                asyncify: None,
                fork_payload: None,
            },
        );
        let imports = wasmer::imports! {
            "test" => { "save" => Function::new_typed_with_env_async(&mut store, &env, save) },
        };
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let asyncify = Asyncify::new(&instance, &mut store).unwrap();
        assert!(asyncify.is_some(), "the module looks instrumented");
        let data = env.as_mut(&mut store);
        data.memory = Some(instance.exports.get_memory("memory").unwrap().clone());
        data.asyncify = asyncify;
        let run = instance.exports.get_function("run").unwrap().clone();

        let store = store.into_async();
        let cx = Outside {
            env: &env,
            store: &store,
        };
        let result = futures::executor::block_on(call(&cx, None, &run, &[]));
        assert!(result.is_err(), "the guest trapped");
        let moving = futures::executor::block_on(
            cx.with(|env, _| env.asyncify.as_ref().map(Asyncify::moving)),
        );
        assert_eq!(moving, Some(false));
    }
}
