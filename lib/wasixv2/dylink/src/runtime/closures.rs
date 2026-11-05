//! Typed closure pools participate in the ordinary dynamic-image transaction.
use super::*;
use wasm_encoder::{
    BlockType, Catch, CodeSection, EntityType, ExportKind, ExportSection, FunctionSection,
    GlobalType, ImportSection, Instruction, MemArg, MemoryType, RefType, TableType, TypeSection,
    ValType,
};

const BATCH: u32 = 100;
const DESCRIPTOR: u32 = 8;

#[derive(Clone, Debug, Default)]
pub(super) struct Ledger {
    pub slots: BTreeMap<u32, Slot>,
    pub pools: Vec<Pool>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Slot {
    pub allocated: bool,
    pub binding: Option<Binding>,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Binding {
    pool: usize,
    stub: u32,
    backing: u32,
    environment: u32,
}
#[derive(Clone, Debug)]
pub(super) struct Pool {
    signature: Signature,
    descriptor: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Signature {
    arguments: Vec<u8>,
    results: Vec<u8>,
}
impl Ledger {
    fn allocate(&mut self, cursor: &mut u32) -> SysResult<u32> {
        let available = self
            .slots
            .iter()
            .find_map(|(id, slot)| (!slot.allocated).then_some(*id));
        let id = match available {
            Some(id) => id,
            None => {
                let first = *cursor;
                *cursor = first.checked_add(BATCH).ok_or(Errno::EOVERFLOW)?;
                for id in first..*cursor {
                    self.slots.insert(
                        id,
                        Slot {
                            allocated: false,
                            binding: None,
                            revision: 0,
                        },
                    );
                }
                first
            }
        };
        let slot = self.slots.get_mut(&id).unwrap();
        slot.revision = slot.revision.checked_add(1).ok_or(Errno::EOVERFLOW)?;
        slot.allocated = true;
        Ok(id)
    }

    fn free(&mut self, id: u32) -> SysResult<()> {
        let slot = self
            .slots
            .get_mut(&id)
            .filter(|slot| slot.allocated)
            .ok_or(Errno::EINVAL)?;
        slot.revision = slot.revision.checked_add(1).ok_or(Errno::EOVERFLOW)?;
        slot.allocated = false;
        slot.binding = None;
        Ok(())
    }

    fn available(&self, signature: &Signature, except: u32) -> Option<(usize, u32)> {
        self.pools
            .iter()
            .enumerate()
            .filter(|(_, pool)| &pool.signature == signature)
            .find_map(|(pool, _)| {
                (0..BATCH).find_map(|stub| {
                    (!self.slots.iter().any(|(id, slot)| {
                        *id != except
                            && slot
                                .binding
                                .as_ref()
                                .is_some_and(|binding| binding.pool == pool && binding.stub == stub)
                    }))
                    .then_some((pool, stub))
                })
            })
    }
}

fn value_type(ty: u8) -> SysResult<(ValType, u32)> {
    Ok(match ty {
        0 => (ValType::I32, 4),
        1 => (ValType::I64, 8),
        2 => (ValType::F32, 4),
        3 => (ValType::F64, 8),
        4 => (ValType::V128, 16),
        _ => return Err(Errno::EINVAL.into()),
    })
}
fn packet_size(types: &[u8]) -> SysResult<u32> {
    let size = types.iter().try_fold(0u32, |sum, ty| -> SysResult<u32> {
        sum.checked_add(value_type(*ty)?.1)
            .ok_or(Errno::EOVERFLOW.into())
    })?;
    size.checked_add(15)
        .map(|size| size & !15)
        .ok_or(Errno::EOVERFLOW.into())
}
fn memarg(offset: u64) -> MemArg {
    MemArg {
        offset,
        align: 0,
        memory_index: 0,
    }
}
fn store_value(ty: u8, offset: u64) -> Instruction<'static> {
    let mem = memarg(offset);
    match ty {
        0 => Instruction::I32Store(mem),
        1 => Instruction::I64Store(mem),
        2 => Instruction::F32Store(mem),
        3 => Instruction::F64Store(mem),
        4 => Instruction::V128Store(mem),
        _ => unreachable!(),
    }
}
fn load(ty: u8, offset: u64) -> Instruction<'static> {
    let mem = memarg(offset);
    match ty {
        0 => Instruction::I32Load(mem),
        1 => Instruction::I64Load(mem),
        2 => Instruction::F32Load(mem),
        3 => Instruction::F64Load(mem),
        4 => Instruction::V128Load(mem),
        _ => unreachable!(),
    }
}
fn name(stub: u32) -> String {
    format!("__wasix_closure_{stub}")
}

fn instantiate(
    cache: &forward::Cache,
    store: &mut StoreMut<'_>,
    memory: &Memory,
    table: &Table,
    stack: &Global,
    imports: &Imports,
    pool: &Pool,
) -> Result<BoundModule> {
    let signature = &pool.signature;
    // Managed dynamic images use exnref EH; graph admission rejects asyncify.
    let arguments = packet_size(&signature.arguments).map_err(failure)?;
    let results = packet_size(&signature.results).map_err(failure)?;
    let mut module = wasm_encoder::Module::new();
    let mut types = TypeSection::new();
    types.ty().function(
        signature
            .arguments
            .iter()
            .map(|ty| value_type(*ty).unwrap().0),
        signature
            .results
            .iter()
            .map(|ty| value_type(*ty).unwrap().0),
    );
    types.ty().function([ValType::I32; 3], []);
    module.section(&types);
    let mut imported = ImportSection::new();
    let memory_type = memory.ty(&*store);
    imported.import(
        "env",
        "memory",
        EntityType::Memory(MemoryType {
            minimum: u64::from(memory_type.minimum.0),
            maximum: memory_type.maximum.map(|pages| u64::from(pages.0)),
            memory64: false,
            shared: memory_type.shared,
            page_size_log2: None,
        }),
    );
    let table_type = table.ty(&*store);
    imported.import(
        "env",
        "table",
        EntityType::Table(TableType {
            element_type: RefType::FUNCREF,
            minimum: u64::from(table_type.minimum),
            maximum: table_type.maximum.map(u64::from),
            table64: false,
            shared: false,
        }),
    );
    imported.import(
        "env",
        "stack",
        EntityType::Global(GlobalType {
            val_type: ValType::I32,
            mutable: true,
            shared: false,
        }),
    );
    imported.import(
        "env",
        "descriptor",
        EntityType::Global(GlobalType {
            val_type: ValType::I32,
            mutable: false,
            shared: false,
        }),
    );
    module.section(&imported);
    let mut functions = FunctionSection::new();
    let mut exports = ExportSection::new();
    let mut code = CodeSection::new();
    for stub in 0..BATCH {
        functions.function(0);
        exports.export(&name(stub), ExportKind::Func, stub);
        let first = u32::try_from(signature.arguments.len()).map_err(failure)?;
        let (original, values, returned, backing, environment, exception) =
            (first, first + 1, first + 2, first + 3, first + 4, first + 5);
        let locals = [(5, ValType::I32), (1, ValType::Ref(RefType::EXNREF))];
        let mut body = wasm_encoder::Function::new(locals);
        for (offset, local) in [(0, backing), (4, environment)] {
            body.instruction(&Instruction::GlobalGet(1));
            body.instruction(&Instruction::I32Load(memarg(u64::from(
                stub * DESCRIPTOR + offset,
            ))));
            body.instruction(&Instruction::LocalSet(local));
        }
        for instruction in [
            Instruction::GlobalGet(0),
            Instruction::LocalTee(original),
            Instruction::I32Const(results as i32),
            Instruction::I32Sub,
            Instruction::LocalTee(returned),
            Instruction::I32Const(arguments as i32),
            Instruction::I32Sub,
            Instruction::LocalTee(values),
            Instruction::GlobalSet(0),
        ] {
            body.instruction(&instruction);
        }
        let mut offset = 0;
        for (argument, ty) in signature.arguments.iter().enumerate() {
            body.instruction(&Instruction::LocalGet(values));
            body.instruction(&Instruction::LocalGet(argument as u32));
            body.instruction(&store_value(*ty, offset));
            offset += u64::from(value_type(*ty).unwrap().1);
        }
        // A guest exception restores this frame's shadow SP and rethrows the same
        // exception reference. Host-abort/trap teardown follows the engine path.
        body.instruction(&Instruction::Block(BlockType::Empty));
        body.instruction(&Instruction::Block(BlockType::Result(ValType::Ref(
            RefType::EXNREF,
        ))));
        body.instruction(&Instruction::TryTable(
            BlockType::Empty,
            vec![Catch::AllRef { label: 0 }].into(),
        ));
        for local in [values, returned, environment, backing] {
            body.instruction(&Instruction::LocalGet(local));
        }
        body.instruction(&Instruction::CallIndirect {
            type_index: 1,
            table_index: 0,
        });
        body.instruction(&Instruction::Br(2));
        body.instruction(&Instruction::End);
        body.instruction(&Instruction::Unreachable);
        body.instruction(&Instruction::End);
        body.instruction(&Instruction::LocalSet(exception));
        body.instruction(&Instruction::LocalGet(original));
        body.instruction(&Instruction::GlobalSet(0));
        body.instruction(&Instruction::LocalGet(exception));
        body.instruction(&Instruction::ThrowRef);
        body.instruction(&Instruction::End);
        let mut offset = 0;
        for ty in &signature.results {
            body.instruction(&Instruction::LocalGet(returned));
            body.instruction(&load(*ty, offset));
            offset += u64::from(value_type(*ty).unwrap().1);
        }
        body.instruction(&Instruction::LocalGet(original));
        body.instruction(&Instruction::GlobalSet(0));
        body.instruction(&Instruction::End);
        code.function(&body);
    }
    module.section(&functions);
    module.section(&exports);
    module.section(&code);
    let bytes = wasixv2_exec::instrument(&module.finish()).map_err(failure)?;
    let module = cache.module(&*store, &bytes)?;
    let mut imports = imports.clone();
    imports.define("env", "memory", memory.clone());
    imports.define("env", "table", table.clone());
    imports.define("env", "stack", stack.clone());
    imports.define(
        "env",
        "descriptor",
        Global::new(&mut *store, Value::I32(pool.descriptor as i32)),
    );
    let instance = Instance::new(store, &module, &imports).map_err(failure)?;
    Ok(BoundModule::new(&instance))
}

pub(super) struct DescriptorWrite {
    memory: Memory,
    address: u32,
    before: [u8; DESCRIPTOR as usize],
    after: [u8; DESCRIPTOR as usize],
    applied: bool,
}
impl DescriptorWrite {
    pub(super) fn apply(&mut self, store: &mut impl AsStoreMut) -> Result<()> {
        self.memory
            .view(&*store)
            .write(u64::from(self.address), &self.after)
            .map_err(failure)?;
        self.applied = true;
        Ok(())
    }
    pub(super) fn rollback(&mut self, store: &mut impl AsStoreMut) -> Result<()> {
        if self.applied {
            self.memory
                .view(&*store)
                .write(u64::from(self.address), &self.before)
                .map_err(failure)?;
            self.applied = false;
        }
        Ok(())
    }
}

impl Local {
    pub(super) fn stage_closures(
        &mut self,
        store: &mut StoreMut<'_>,
        ledger: &Ledger,
        imports: &Imports,
    ) -> Result<()> {
        if ledger.slots.is_empty() {
            return Ok(());
        }
        let stack = self.instances[0]
            .exports
            .get_global("__stack_pointer")
            .map_err(failure)?
            .clone();
        let stage = self
            .stage
            .as_mut()
            .ok_or_else(|| failure("closure stage is missing"))?;
        stage
            .closure_pools
            .try_reserve(ledger.pools.len().saturating_sub(self.closure_pools.len()))
            .map_err(failure)?;
        self.closure_pools
            .try_reserve(ledger.pools.len().saturating_sub(self.closure_pools.len()))
            .map_err(failure)?;
        for pool in ledger
            .pools
            .iter()
            .skip(self.closure_pools.len() + stage.closure_pools.len())
        {
            stage.closure_pools.push(instantiate(
                &self.forward_code,
                store,
                &self.memory,
                &self.table,
                &stack,
                imports,
                pool,
            )?);
        }
        for (id, slot) in &ledger.slots {
            if self.closure_slots.get(id) == Some(slot) {
                continue;
            }
            let function = match &slot.binding {
                Some(binding) => {
                    let pool = ledger
                        .pools
                        .get(binding.pool)
                        .ok_or_else(|| failure("closure pool is missing"))?;
                    let instance = self
                        .closure_pools
                        .get(binding.pool)
                        .or_else(|| {
                            stage
                                .closure_pools
                                .get(binding.pool - self.closure_pools.len())
                        })
                        .ok_or_else(|| failure("closure pool is not bound"))?;
                    let address = pool
                        .descriptor
                        .checked_add(
                            binding
                                .stub
                                .checked_mul(DESCRIPTOR)
                                .ok_or_else(|| failure("closure descriptor overflow"))?,
                        )
                        .ok_or_else(|| failure("closure descriptor overflow"))?;
                    let mut before = [0; DESCRIPTOR as usize];
                    self.memory
                        .view(&*store)
                        .read(u64::from(address), &mut before)
                        .map_err(failure)?;
                    let mut after = [0; DESCRIPTOR as usize];
                    after[..4].copy_from_slice(&binding.backing.to_le_bytes());
                    after[4..].copy_from_slice(&binding.environment.to_le_bytes());
                    stage.descriptors.push(DescriptorWrite {
                        memory: self.memory.clone(),
                        address,
                        before,
                        after,
                        applied: false,
                    });
                    Value::FuncRef(Some(
                        instance
                            .exports
                            .get_function(&name(binding.stub))
                            .map_err(failure)?
                            .clone(),
                    ))
                }
                None => Value::FuncRef(None),
            };
            let before = self
                .table
                .get(&mut *store, *id)
                .ok_or_else(|| failure("closure slot is missing"))?;
            stage.lazy_slots.push(LazySlot {
                slot: *id,
                before,
                function,
                applied: false,
            });
        }
        stage.closure_slots = Some(ledger.slots.clone());
        Ok(())
    }
}

pub(super) enum Operation {
    Allocate(wasixv2_abi::U32Ptr),
    Prepare {
        backing: u32,
        closure: u32,
        signature: Signature,
        environment: u32,
    },
    Free(u32),
}

pub(super) async fn signature(
    caller: &Caller<'_>,
    arguments: wasixv2_syscalls::UserSlice,
    results: wasixv2_syscalls::UserSlice,
) -> SysResult<Signature> {
    async fn read(caller: &Caller<'_>, types: wasixv2_syscalls::UserSlice) -> SysResult<Vec<u8>> {
        let mut values = Vec::new();
        let mut offset = 0;
        while offset < types.len {
            let count = (types.len - offset).min(4096) as usize;
            let mut bytes = [0; 4096];
            let address = types.addr.checked_add(offset).ok_or(Errno::EFAULT)?;
            caller.memory().read(address, &mut bytes[..count]).await?;
            for ty in &bytes[..count] {
                value_type(*ty)?;
            }
            values.try_reserve(count).map_err(|_| Errno::ENOMEM)?;
            values.extend_from_slice(&bytes[..count]);
            offset += count as u32;
        }
        packet_size(&values)?;
        Ok(values)
    }
    Ok(Signature {
        arguments: read(caller, arguments).await?,
        results: read(caller, results).await?,
    })
}

impl<S: Syscalls + Control + Send + Sync + 'static> StoreImage<S> {
    pub(super) async fn mutate_closure(
        &self,
        env: &AsyncFunctionEnvMut<ExecEnv<S>>,
        caller: &Caller<'_>,
        operation: Operation,
    ) -> SysResult<i32> {
        self.ensure_live().map_err(|_| Errno::ESTALE)?;
        let gate = self.root.gate().map_err(|_| Errno::ENOSYS)?.clone();
        let syscalls = env.read().await.data().syscalls().clone();
        if !syscalls
            .loader_gate(caller.task())
            .is_some_and(|current| Arc::ptr_eq(&current, &gate))
        {
            return Err(Errno::ESTALE.into());
        }
        if gate.busy() {
            self.service(&Context::Import(env))
                .await
                .map_err(|_| Errno::EINTR)?;
        }
        let old = self.root.graph();
        let mut graph = (*old).clone();
        let mut output = None;
        match &operation {
            Operation::Allocate(pointer) => {
                let mut previous = [0; 4];
                caller.memory().read(pointer.addr(), &mut previous).await?;
                output = Some((*pointer, graph.closures.allocate(&mut graph.cursor)?));
            }
            Operation::Free(id) => graph.closures.free(*id)?,
            Operation::Prepare {
                backing, closure, ..
            } => {
                if !graph
                    .closures
                    .slots
                    .get(closure)
                    .is_some_and(|slot| slot.allocated)
                {
                    return Err(Errno::EINVAL.into());
                }
                let valid = Context::Import(env)
                    .with(|_, store| {
                        let local = lock(&self.local);
                        local
                            .table
                            .get_typed_function(store, *backing)
                            .is_some_and(|function| {
                                let ty = function.ty(store);
                                ty.params() == [Type::I32, Type::I32, Type::I32]
                                    && ty.results().is_empty()
                            })
                    })
                    .await;
                if !valid {
                    return Err(Errno::EINVAL.into());
                }
            }
        }
        let closing = match gate.close(caller.task()) {
            Ok(closing) => closing,
            Err(Errno::EAGAIN) => return Err(wasixv2_syscalls::SysErr::RestartNoIntr),
            Err(error) => return Err(error.into()),
        };
        syscalls
            .loader_sleep(caller.task(), &gate, LoaderWait::Drained)
            .await?;
        if !Arc::ptr_eq(&old, &self.root.graph()) {
            return Err(wasixv2_syscalls::SysErr::RestartNoIntr);
        }
        let ((cleanup_scope, store), transaction) = load::activate_prepared(
            async {
                let data = env.read().await;
                Ok((
                    data.data().loader_cleanup().cloned().ok_or(Errno::ENOSYS)?,
                    env.as_store_async().store(),
                ))
            },
            || syscalls.loader_activate(caller.task(), closing),
        )
        .await?;
        let mut coordinator = load::Coordinator {
            transaction: Some(transaction),
            cleanup: None,
            root: self.root.clone(),
            local: self.local.clone(),
            env: env.as_ref(),
            store,
            syscalls: syscalls.clone(),
            cleanup_scope,
            task: caller.task(),
        };
        let work = Arc::new(Work {
            publication: Mutex::new(None),
            previous: old.libraries.len(),
            first: caller.task(),
            leases: Mutex::new(Vec::new()),
            committed: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        lock(&self.root.state).work = Some(work.clone());
        let failure_error = std::cell::Cell::new(wasixv2_syscalls::SysErr::Errno(Errno::EIO));
        let result = async {
            self.finish_phase(&Context::Import(env), &coordinator)
                .await?;
            if let Operation::Prepare {
                backing,
                closure,
                signature,
                environment,
            } = operation
            {
                let (pool, stub) = match graph.closures.available(&signature, closure) {
                    Some(available) => available,
                    None => {
                        let lease = syscalls
                            .loader_reserve(caller, BATCH * DESCRIPTOR, 16)
                            .await
                            .map_err(|errno| {
                                failure_error.set(errno);
                                failure(errno)
                            })?;
                        let descriptor = lease.address();
                        lock(&work.leases).push(lease);
                        let pool = graph.closures.pools.len();
                        graph.closures.pools.push(Pool {
                            signature,
                            descriptor,
                        });
                        (pool, 0)
                    }
                };
                let slot = graph
                    .closures
                    .slots
                    .get_mut(&closure)
                    .ok_or_else(|| failure("closure ownership changed"))?;
                slot.revision = slot
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| failure("closure revision exhausted"))?;
                slot.binding = Some(Binding {
                    pool,
                    stub,
                    backing,
                    environment,
                });
            }
            let state = lock(&self.root.state);
            let publication = Publication {
                graph: Arc::new(graph),
                references: state.references.clone(),
                ctors: state.ctors.clone(),
            };
            drop(state);
            *lock(&work.publication) = Some(publication);
            for phase in [LoaderPhase::Prepare, LoaderPhase::Apply] {
                coordinator
                    .transaction
                    .as_ref()
                    .unwrap()
                    .advance(phase)
                    .map_err(failure)?;
                self.finish_phase(&Context::Import(env), &coordinator)
                    .await?;
                if let Some(error) = lock(&work.error).clone() {
                    return Err(failure(error));
                }
            }
            lock(&self.root.state)
                .leases
                .try_reserve(lock(&work.leases).len())
                .map_err(failure)?;
            if let Some((pointer, id)) = output {
                write_value(caller.memory(), pointer, &id)
                    .await
                    .map_err(|error| {
                        failure_error.set(Errno::EFAULT.into());
                        failure(error)
                    })?;
            }
            let publication = lock(&work.publication)
                .take()
                .ok_or_else(|| failure("closure publication is missing"))?;
            {
                let mut state = lock(&self.root.state);
                state.graph = publication.graph;
                work.committed.store(true, Ordering::Release);
            }
            if let Err(error) = coordinator
                .transaction
                .as_ref()
                .unwrap()
                .advance(LoaderPhase::Publish)
            {
                let _ = syscalls.loader_fatal(&gate);
                return Err(failure(error));
            }
            self.finish_phase(&Context::Import(env), &coordinator)
                .await?;
            lock(&self.root.state)
                .leases
                .extend(std::mem::take(&mut *lock(&work.leases)));
            lock(&self.root.state).work = None;
            coordinator
                .transaction
                .take()
                .unwrap()
                .finish()
                .map_err(failure)?;
            Ok(())
        }
        .await;
        match result {
            Ok(()) => Ok(0),
            Err(error) => {
                tracing::debug!(%error, "closure publication failed");
                if work.committed.load(Ordering::Acquire) {
                    let _ = syscalls.loader_fatal(&gate);
                    return Err(Errno::EIO.into());
                }
                coordinator.abort().await.map_err(|_| Errno::EIO)?;
                Err(failure_error.get())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer::{MemoryType as RuntimeMemoryType, Store};

    #[test]
    fn ledger_reuses_freed_slots_and_advances_their_revision() {
        let mut ledger = Ledger::default();
        let mut cursor = 3;
        let first = ledger.allocate(&mut cursor).unwrap();
        let next = ledger.allocate(&mut cursor).unwrap();
        assert_eq!((first, next, cursor), (3, 4, 103));
        assert_eq!(ledger.slots[&first].revision, 1);

        ledger.free(first).unwrap();
        assert_eq!(ledger.slots[&first].revision, 2);
        assert_eq!(ledger.allocate(&mut cursor).unwrap(), first);
        assert_eq!(ledger.slots[&first].revision, 3);
        assert!(ledger.slots[&first].allocated);
    }

    #[test]
    fn descriptor_rollback_restores_the_exact_pretransaction_bytes() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, RuntimeMemoryType::new(1, Some(1), false)).unwrap();
        let before = [0x17, 0x23, 0x35, 0x41, 0x52, 0x63, 0x74, 0x85];
        let after = [0x91, 0x82, 0x73, 0x64, 0x55, 0x46, 0x37, 0x28];
        memory.view(&store).write(64, &before).unwrap();
        let mut descriptor = DescriptorWrite {
            memory: memory.clone(),
            address: 64,
            before,
            after,
            applied: false,
        };

        descriptor.apply(&mut store).unwrap();
        let mut observed = [0; DESCRIPTOR as usize];
        memory.view(&store).read(64, &mut observed).unwrap();
        assert_eq!(observed, after);

        descriptor.rollback(&mut store).unwrap();
        memory.view(&store).read(64, &mut observed).unwrap();
        assert_eq!(observed, before);
        descriptor.rollback(&mut store).unwrap();
        memory.view(&store).read(64, &mut observed).unwrap();
        assert_eq!(observed, before);
    }
}

#[cfg(all(test, feature = "engine-tests"))]
mod stage_tests {
    use super::*;
    use wasmer::{AsStoreMut, Function, Value};

    #[test]
    fn failed_closure_stage_restores_the_exact_table_function_and_descriptor() {
        let (mut graph, mut local, mut store, imports) = super::super::cyclic_tests::fixture();
        let closure = 3;
        let backing = Function::new_typed(&mut store, |_: i32, _: i32, _: i32| {});
        let previous = Function::new_typed(&mut store, |value: i32| -> i32 { value + 99 });
        local
            .table
            .grow(&mut store, 1, Value::FuncRef(None))
            .unwrap();
        local
            .table
            .set(&mut store, 1, Value::FuncRef(Some(backing)))
            .unwrap();
        local
            .table
            .set(&mut store, closure, Value::FuncRef(Some(previous.clone())))
            .unwrap();

        let descriptor = 512;
        let original_descriptor = [0x17, 0x23, 0x35, 0x41, 0x52, 0x63, 0x74, 0x85];
        local
            .memory
            .view(&store)
            .write(u64::from(descriptor), &original_descriptor)
            .unwrap();
        graph.closures.pools.push(Pool {
            signature: Signature {
                arguments: vec![0],
                results: vec![0],
            },
            descriptor,
        });
        graph.closures.slots.insert(
            closure,
            Slot {
                allocated: true,
                binding: Some(Binding {
                    pool: 0,
                    stub: 0,
                    backing: 1,
                    environment: 7,
                }),
                revision: 1,
            },
        );
        LocalStage::resume(&mut local.stage, vec![0], Vec::new());
        local
            .stage_closures(&mut store.as_store_mut(), &graph.closures, &imports)
            .unwrap();

        // Fail after the closure's descriptor and function slot have been applied.
        local.stage.as_mut().unwrap().lazy_slots.push(LazySlot {
            slot: u32::MAX,
            before: Value::FuncRef(None),
            function: Value::FuncRef(None),
            applied: false,
        });
        assert!(
            local
                .stage
                .as_mut()
                .unwrap()
                .apply(&mut store, &local.table)
                .is_err()
        );
        local
            .stage
            .as_mut()
            .unwrap()
            .rollback(&mut store, &local.table)
            .unwrap();

        let Some(Value::FuncRef(Some(restored))) = local.table.get(&mut store, closure) else {
            panic!("rollback must restore the old non-null table entry")
        };
        assert_eq!(
            restored
                .typed::<i32, i32>(&store)
                .unwrap()
                .call(&mut store, 1)
                .unwrap(),
            100
        );
        let mut observed_descriptor = [0; DESCRIPTOR as usize];
        local
            .memory
            .view(&store)
            .read(u64::from(descriptor), &mut observed_descriptor)
            .unwrap();
        assert_eq!(observed_descriptor, original_descriptor);
    }
}
