//! Bounded initialization extracted from lld output. None of these functions calls wasm.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use wasmparser::{DataKind, FunctionBody, Operator, Parser, Payload, TypeRef, ValType};

use crate::{Module, PlanError};

#[derive(Clone, Debug)]
enum Op {
    Const(u64),
    Get(u32),
    Set(u32),
    LocalGet(u32),
    LocalSet(u32),
    LocalTee(u32),
    Add32,
    Add64,
    Store(u32, u64),
    Copy(u32),
    Fill,
    DropData(u32),
}

#[derive(Clone, Debug)]
struct Program {
    params: usize,
    locals: usize,
    ops: Vec<Op>,
}

/// Validated initializers and original data images. The plans are private to prevent callers
/// from constructing unchecked instructions and treating them as authenticated helper code.
#[derive(Clone, Debug)]
pub struct Initialization {
    first: Vec<Program>,
    peer: Vec<Program>,
    tls: Option<Program>,
    tls_reloc: Option<Program>,
    segments: Vec<Vec<u8>>,
    /// Data segments dropped before user entry, in both initial and subsequent instances.
    dropped: BTreeSet<u32>,
    original: Vec<u8>,
    functions: u32,
    globals: u32,
    memories: u32,
    tables: u32,
    aliases: Vec<(String, wasmparser::ExternalKind, u32)>,
    tags: u32,
    active_data: Vec<(Address, u32)>,
    active_elements: Vec<(Address, Vec<Option<u32>>)>,
}

/// A checked write into a newly reserved module/TLS region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitWrite {
    pub address: u32,
    pub bytes: Vec<u8>,
}

/// One active element placement, using module-local function indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitTable {
    pub index: u32,
    pub functions: Vec<Option<u32>>,
}

/// Fully checked effects; computing them does not modify memory or engine globals.
#[derive(Clone, Debug)]
pub struct InitEffects {
    pub writes: Vec<InitWrite>,
    pub globals: BTreeMap<u32, u64>,
}

impl Initialization {
    /// Extracts start, data relocations and optional TLS initialization. Only a finite
    /// straight-line helper graph and the exact lld shared-PIC once template are accepted.
    /// Helpers may not call imports or constructors, branch/loop, read memory, grow tables
    /// or memory, or modify imported globals. Symbol names select roots, never confer trust.
    pub fn parse(bytes: &[u8]) -> Result<Self, PlanError> {
        let module = Module::parse(bytes)?;
        let mut bodies = Vec::new();
        let mut imported_functions = 0;
        let mut imported_globals = 0;
        let mut mutable_globals = BTreeSet::new();
        let mut segments = Vec::new();
        let mut active_data = Vec::new();
        let mut active_elements = Vec::new();
        let mut globals_count = 0;
        let mut memories = 0;
        let mut tables = 0;
        let mut aliases = Vec::new();
        let mut tags = 0;
        for payload in Parser::new(0).parse_all(bytes) {
            match payload? {
                Payload::ImportSection(section) => {
                    for import in section.into_imports() {
                        let import = import?;
                        if matches!(import.ty, TypeRef::Global(_))
                            && matches!(import.name, "__stack_pointer" | "__tls_base")
                        {
                            aliases.push((
                                import.name.to_string(),
                                wasmparser::ExternalKind::Global,
                                imported_globals,
                            ));
                        }
                        match import.ty {
                            TypeRef::Func(_) => imported_functions += 1,
                            TypeRef::Global(_) => imported_globals += 1,
                            TypeRef::Memory(_) => memories += 1,
                            TypeRef::Table(_) => tables += 1,
                            TypeRef::Tag(_) => tags += 1,
                            _ => {}
                        }
                    }
                }
                Payload::GlobalSection(section) => {
                    globals_count = section.count();
                    for (i, global) in section.into_iter().enumerate() {
                        if global?.ty.mutable {
                            mutable_globals.insert(imported_globals + i as u32);
                        }
                    }
                }
                Payload::DataSection(section) => {
                    for data in section {
                        let data = data?;
                        if let DataKind::Active {
                            memory_index,
                            offset_expr,
                        } = data.kind
                        {
                            if memory_index != 0 {
                                return Err(invalid());
                            }
                            active_data.push((Address::parse(offset_expr)?, segments.len() as u32));
                        }
                        segments.push(data.data.to_vec());
                    }
                }
                Payload::ElementSection(section) => {
                    for element in section {
                        let element = element?;
                        if let wasmparser::ElementKind::Active {
                            table_index,
                            offset_expr,
                        } = element.kind
                        {
                            if table_index.unwrap_or(0) != 0 {
                                return Err(invalid());
                            }
                            let functions = match element.items {
                                wasmparser::ElementItems::Functions(items) => items
                                    .into_iter()
                                    .map(|i| i.map(Some))
                                    .collect::<Result<Vec<_>, _>>()?,
                                wasmparser::ElementItems::Expressions(ty, items)
                                    if ty == wasmparser::RefType::FUNCREF =>
                                {
                                    let mut functions = Vec::new();
                                    for expr in items {
                                        let expr = expr?;
                                        let mut ops = expr.get_operators_reader();
                                        let function = match ops.read()? {
                                            Operator::RefFunc { function_index } => {
                                                Some(function_index)
                                            }
                                            Operator::RefNull {
                                                hty:
                                                    wasmparser::HeapType::Abstract {
                                                        ty: wasmparser::AbstractHeapType::Func,
                                                        ..
                                                    },
                                            } => None,
                                            _ => return Err(invalid()),
                                        };
                                        if !matches!(ops.read()?, Operator::End) || !ops.eof() {
                                            return Err(invalid());
                                        }
                                        functions.push(function);
                                    }
                                    functions
                                }
                                _ => return Err(invalid()),
                            };
                            active_elements.push((Address::parse(offset_expr)?, functions));
                        }
                    }
                }
                Payload::MemorySection(section) => memories += section.count(),
                Payload::TableSection(section) => tables += section.count(),
                Payload::TagSection(section) => tags += section.count(),
                Payload::CodeSectionEntry(body) => bodies.push(body),
                _ => {}
            }
        }
        let exports = module.exports();
        if exports
            .keys()
            .any(|name| name.starts_with("__wasix_dylink_"))
        {
            return Err(invalid());
        }
        if tables > 0 {
            aliases.push((
                "__indirect_function_table".to_string(),
                wasmparser::ExternalKind::Table,
                0,
            ));
        }
        for (name, kind, index) in &aliases {
            if let Some(export) = exports.get(name) {
                let correct = match kind {
                    wasmparser::ExternalKind::Global => {
                        matches!(export.ty, crate::ExternType::Global(_))
                    }
                    _ => matches!(export.ty, crate::ExternType::Table(_)),
                };
                if !correct || export.index != *index {
                    return Err(invalid());
                }
            }
        }
        aliases.retain(|(name, _, _)| !exports.contains_key(name));
        for name in [
            "__wasm_init_tls",
            "__wasm_apply_tls_relocs",
            "__wasm_apply_data_relocs",
            "__wasm_call_ctors",
        ] {
            if let Some(export) = exports.get(name) {
                let crate::ExternType::Function(ty) = &export.ty else {
                    return Err(invalid());
                };
                let expected: &[ValType] = if name == "__wasm_init_tls" {
                    &[ValType::I32]
                } else {
                    &[]
                };
                if ty.params() != expected || !ty.results().is_empty() {
                    return Err(invalid());
                }
            }
        }
        let constructors = exports.get("__wasm_call_ctors").map(|e| e.index);
        let mut roots = module.start().into_iter().collect::<Vec<_>>();
        if let Some(export) = exports.get("__wasm_apply_data_relocs") {
            roots.push(export.index);
        }
        // Writer.cpp's shared-memory start initializes the static TLS image and global
        // TLS relocations, but does not apply relocations inside that image itself.
        if let Some(export) = exports.get("__wasm_apply_tls_relocs") {
            roots.push(export.index);
        }

        let mut first = Vec::new();
        let mut peer = Vec::new();
        let mut dropped = BTreeSet::new();
        let context = Compile {
            bodies: &bodies,
            imported_functions,
            mutable_globals: &mutable_globals,
            constructors,
            functions: &module.functions,
        };
        for root in roots {
            let a = context.program(root, true, 0)?;
            let b = context.program(root, false, 0)?;
            for op in &a.ops {
                if let Op::DropData(i) = op {
                    dropped.insert(*i);
                }
            }
            first.push(a);
            if Some(root) == module.start() {
                peer.push(b);
            }
        }
        let tls = exports
            .get("__wasm_init_tls")
            .map(|e| context.program(e.index, false, 1))
            .transpose()?;
        let tls_reloc = exports
            .get("__wasm_apply_tls_relocs")
            .map(|e| context.program(e.index, false, 0))
            .transpose()?;
        Ok(Self {
            first,
            peer,
            tls,
            tls_reloc,
            segments,
            dropped,
            original: bytes.to_vec(),
            functions: module.functions.len() as u32,
            globals: imported_globals + globals_count,
            memories,
            tables,
            aliases,
            tags,
            active_data,
            active_elements,
        })
    }

    /// Computes writes/globals for the first instance (`tls = None`) or a peer's separate
    /// TLS block. The caller supplies scalar global inputs after symbol/base resolution.
    /// Every memory effect, including the once flag, must fall inside `fresh` reservations.
    pub fn effects(
        &self,
        mut globals: BTreeMap<u32, u64>,
        fresh: &[Range<u64>],
        tls: Option<u32>,
    ) -> Result<InitEffects, PlanError> {
        let before = globals.clone();
        let mut writes = Vec::new();
        let mut dropped = BTreeSet::new();
        if tls.is_none() {
            for (address, segment) in &self.active_data {
                checked_write(
                    u64::from(address.value(&globals)?),
                    &self.segments[*segment as usize],
                    fresh,
                    &mut writes,
                )?;
            }
        }
        let programs = if tls.is_some() {
            &self.peer
        } else {
            &self.first
        };
        for program in programs {
            self.evaluate(program, &[], &mut globals, fresh, &mut writes, &mut dropped)?;
        }
        if let Some(block) = tls
            && let Some(program) = &self.tls
        {
            self.evaluate(
                program,
                &[u64::from(block)],
                &mut globals,
                fresh,
                &mut writes,
                &mut dropped,
            )?;
        }
        // WASIX lld's main __wasm_init_tls applies pointer relocations before updating
        // the per-store TLS GOT globals. Reapply the validated relocations after that
        // helper's complete effect plan, so pointers reference this thread's final GOT.
        if tls.is_some()
            && let Some(program) = &self.tls_reloc
        {
            self.evaluate(program, &[], &mut globals, fresh, &mut writes, &mut dropped)?;
        }
        globals.retain(|index, value| before.get(index) != Some(value));
        Ok(InitEffects { writes, globals })
    }

    /// The main image's per-store relocations, with main TLS left to pthread_create as in
    /// DESIGN.md. No memory write is permitted here; shared main data already exists.
    pub fn peer_globals(&self, mut globals: BTreeMap<u32, u64>) -> Result<InitEffects, PlanError> {
        let before = globals.clone();
        let mut writes = Vec::new();
        let mut dropped = BTreeSet::new();
        for program in &self.peer {
            self.evaluate(program, &[], &mut globals, &[], &mut writes, &mut dropped)?;
        }
        globals.retain(|index, value| before.get(index) != Some(value));
        Ok(InitEffects { writes, globals })
    }

    /// Returns a no-auto-start form. Active data become empty passive segments, active
    /// elements become declared (already dropped); original passive segment ordinals and
    /// contents remain intact unless the initialization plan drops them before user entry.
    pub fn instantiate_bytes(&self) -> Result<Vec<u8>, PlanError> {
        use wasm_encoder::reencode::Reencode;
        let mut stripped = wasm_encoder::Module::new();
        let mut has_exports = false;
        for payload in Parser::new(0).parse_all(&self.original) {
            let payload = payload?;
            if let Some((id, range)) = payload.as_section() {
                if !has_exports && matches!(id, 8..=12) {
                    stripped.section(&wasm_encoder::ExportSection::new());
                    has_exports = true;
                }
                if id == 7 {
                    has_exports = true;
                }
                if id != 8 {
                    stripped.section(&wasm_encoder::RawSection {
                        id,
                        data: &self.original[range.start as usize..range.end as usize],
                    });
                }
            }
        }
        if !has_exports {
            stripped.section(&wasm_encoder::ExportSection::new());
        }
        let mut output = wasm_encoder::Module::new();
        let mut clean = Sanitize {
            dropped: &self.dropped,
            data: 0,
            functions: self.functions,
            globals: self.globals,
            memories: self.memories,
            tables: self.tables,
            aliases: &self.aliases,
            tags: self.tags,
        };
        clean
            .parse_core_module(&mut output, Parser::new(0), &stripped.finish())
            .map_err(|_| invalid())?;
        let bytes = output.finish();
        wasmparser::Validator::new().validate_all(&bytes)?;
        Ok(bytes)
    }

    /// Evaluates active element placements against the new module's table reservation.
    /// Returned values are wasm function indices, not function pointers or live functions.
    pub fn table_effects(
        &self,
        globals: &BTreeMap<u32, u64>,
        fresh: &[Range<u64>],
    ) -> Result<Vec<InitTable>, PlanError> {
        let mut result = Vec::new();
        for (address, functions) in &self.active_elements {
            let address = address.value(globals)?;
            check_range(u64::from(address), functions.len() as u64, fresh)?;
            result.push(InitTable {
                index: address,
                functions: functions.clone(),
            });
        }
        Ok(result)
    }

    /// Segment ordinals which must be empty/dropped in the no-auto-start instantiation form.
    pub fn dropped_segments(&self) -> &BTreeSet<u32> {
        &self.dropped
    }

    fn evaluate(
        &self,
        program: &Program,
        args: &[u64],
        globals: &mut BTreeMap<u32, u64>,
        fresh: &[Range<u64>],
        writes: &mut Vec<InitWrite>,
        dropped: &mut BTreeSet<u32>,
    ) -> Result<(), PlanError> {
        if args.len() != program.params {
            return Err(invalid());
        }
        let mut locals = args.to_vec();
        locals.resize(program.locals + args.len(), 0);
        let mut stack = Vec::<u64>::new();
        for op in &program.ops {
            match *op {
                Op::Const(value) => stack.push(value),
                Op::Get(index) => stack.push(*globals.get(&index).ok_or_else(invalid)?),
                Op::Set(index) => {
                    globals.insert(index, pop(&mut stack)?);
                }
                Op::LocalGet(index) => stack.push(*locals.get(index as usize).ok_or_else(invalid)?),
                Op::LocalSet(index) => {
                    *locals.get_mut(index as usize).ok_or_else(invalid)? = pop(&mut stack)?
                }
                Op::LocalTee(index) => {
                    *locals.get_mut(index as usize).ok_or_else(invalid)? =
                        *stack.last().ok_or_else(invalid)?
                }
                Op::Add32 | Op::Add64 => {
                    let b = pop(&mut stack)?;
                    let a = pop(&mut stack)?;
                    stack.push(if matches!(op, Op::Add32) {
                        u64::from((a as u32).wrapping_add(b as u32))
                    } else {
                        a.wrapping_add(b)
                    });
                }
                Op::Store(width, offset) => {
                    let value = pop(&mut stack)?;
                    let address = pop(&mut stack)?.checked_add(offset).ok_or_else(invalid)?;
                    checked_write(
                        address,
                        &value.to_le_bytes()[..width as usize],
                        fresh,
                        writes,
                    )?;
                }
                Op::Copy(segment) => {
                    let len = pop(&mut stack)?;
                    let source = pop(&mut stack)?;
                    let address = pop(&mut stack)?;
                    let original = self.segments.get(segment as usize).ok_or_else(invalid)?;
                    let data = if dropped.contains(&segment) {
                        &[][..]
                    } else {
                        original.as_slice()
                    };
                    let end = source.checked_add(len).ok_or_else(invalid)?;
                    let bytes = data
                        .get(
                            usize::try_from(source).map_err(|_| invalid())?
                                ..usize::try_from(end).map_err(|_| invalid())?,
                        )
                        .ok_or_else(invalid)?;
                    checked_write(address, bytes, fresh, writes)?;
                }
                Op::Fill => {
                    let len = pop(&mut stack)?;
                    let value = pop(&mut stack)?;
                    let address = pop(&mut stack)?;
                    check_range(address, len, fresh)?;
                    // Allocation is fallible even for a metadata-valid enormous fill.
                    let len = usize::try_from(len).map_err(|_| invalid())?;
                    let mut bytes = Vec::new();
                    bytes
                        .try_reserve_exact(len)
                        .map_err(|_| PlanError::Unsupported("initialization allocation failed"))?;
                    bytes.resize(len, value as u8);
                    checked_write(address, &bytes, fresh, writes)?;
                }
                Op::DropData(index) => {
                    if index as usize >= self.segments.len() {
                        return Err(invalid());
                    }
                    dropped.insert(index);
                }
            }
        }
        if !stack.is_empty() {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> PlanError {
    PlanError::Unsupported("initializer outside bounded lld contract")
}
fn pop(stack: &mut Vec<u64>) -> Result<u64, PlanError> {
    stack.pop().ok_or_else(invalid)
}
fn check_range(address: u64, len: u64, fresh: &[Range<u64>]) -> Result<(), PlanError> {
    let end = address
        .checked_add(len)
        .filter(|e| *e <= 1u64 << 32)
        .ok_or_else(invalid)?;
    if !fresh
        .iter()
        .any(|range| address >= range.start && end <= range.end)
    {
        return Err(invalid());
    }
    Ok(())
}
fn checked_write(
    address: u64,
    bytes: &[u8],
    fresh: &[Range<u64>],
    writes: &mut Vec<InitWrite>,
) -> Result<(), PlanError> {
    check_range(address, bytes.len() as u64, fresh)?;
    writes.push(InitWrite {
        address: u32::try_from(address).map_err(|_| invalid())?,
        bytes: bytes.to_vec(),
    });
    Ok(())
}

struct Compile<'a> {
    bodies: &'a [FunctionBody<'a>],
    imported_functions: u32,
    mutable_globals: &'a BTreeSet<u32>,
    constructors: Option<u32>,
    functions: &'a [wasmparser::FuncType],
}

impl Compile<'_> {
    fn program(&self, root: u32, first: bool, params: usize) -> Result<Program, PlanError> {
        let mut program = Program {
            params,
            locals: 0,
            ops: Vec::new(),
        };
        self.append(root, first, params, &mut BTreeSet::new(), &mut program)?;
        Ok(program)
    }

    fn append(
        &self,
        root: u32,
        first: bool,
        params: usize,
        visiting: &mut BTreeSet<u32>,
        out: &mut Program,
    ) -> Result<(), PlanError> {
        if visiting.len() >= 64 || Some(root) == self.constructors || !visiting.insert(root) {
            return Err(invalid());
        }
        let ty = self.functions.get(root as usize).ok_or_else(invalid)?;
        if ty.params() != vec![ValType::I32; params] || !ty.results().is_empty() {
            return Err(invalid());
        }
        let index = root
            .checked_sub(self.imported_functions)
            .ok_or_else(invalid)? as usize;
        let body = self.bodies.get(index).ok_or_else(invalid)?;
        let locals = body
            .get_locals_reader()?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        if locals.iter().any(|(_, ty)| *ty != ValType::I32) {
            return Err(invalid());
        }
        let count = locals
            .iter()
            .try_fold(0usize, |n, (count, _)| n.checked_add(*count as usize))
            .ok_or_else(invalid)?;
        if count > 2 {
            return Err(invalid());
        }
        let local_base = out.locals;
        out.locals = out
            .locals
            .checked_add(count)
            .filter(|count| *count <= 1024)
            .ok_or_else(invalid)?;
        let mut reader = body.get_operators_reader()?;
        let mut ops = Vec::new();
        while !reader.eof() {
            ops.push(reader.read()?);
        }
        let ops = once_body(&ops, first)?;
        let root_params = out.params;
        let remap_local = |index: u32| -> u32 {
            if (index as usize) < params {
                index
            } else {
                (root_params + local_base + index as usize - params) as u32
            }
        };
        for op in ops {
            let op = match op {
                Operator::I32Const { value } => Op::Const(u64::from(value as u32)),
                Operator::I64Const { value } => Op::Const(value as u64),
                Operator::GlobalGet { global_index } => Op::Get(global_index),
                Operator::GlobalSet { global_index }
                    if self.mutable_globals.contains(&global_index) =>
                {
                    Op::Set(global_index)
                }
                Operator::LocalGet { local_index } if local_index < (count + params) as u32 => {
                    Op::LocalGet(remap_local(local_index))
                }
                Operator::LocalSet { local_index } if local_index < (count + params) as u32 => {
                    Op::LocalSet(remap_local(local_index))
                }
                Operator::LocalTee { local_index } if local_index < (count + params) as u32 => {
                    Op::LocalTee(remap_local(local_index))
                }
                Operator::I32Add => Op::Add32,
                Operator::I64Add => Op::Add64,
                Operator::I32Store { memarg } if memarg.memory == 0 => Op::Store(4, memarg.offset),
                Operator::I64Store { memarg } if memarg.memory == 0 => Op::Store(8, memarg.offset),
                Operator::MemoryInit { data_index, mem: 0 } => Op::Copy(data_index),
                Operator::MemoryFill { mem: 0 } => Op::Fill,
                Operator::DataDrop { data_index } => Op::DropData(data_index),
                Operator::Call { function_index } => {
                    // lld synthetic callees take no parameters/results. TLS root alone takes
                    // its new block pointer, and ordinary user functions are never roots.
                    self.append(function_index, first, 0, visiting, out)?;
                    continue;
                }
                Operator::End | Operator::Nop => continue,
                _ => return Err(invalid()),
            };
            if out.ops.len() >= 1_048_576 {
                return Err(invalid());
            }
            out.ops.push(op);
        }
        visiting.remove(&root);
        Ok(())
    }
}

// Recognize the exact wasm32 shared-PIC once prologue/epilogue generated by
// lld/wasm/Writer.cpp:createInitMemoryFunction. Branches/atomics are NOT interpreted.
// The host owns initialization order: first applies the finite body and marks flag=2;
// peers only perform the data.drop semantics. All addresses are still checked at evaluation.
fn once_body<'a>(ops: &[Operator<'a>], first: bool) -> Result<Vec<Operator<'a>>, PlanError> {
    let Some(cmp) = ops
        .iter()
        .position(|op| matches!(op, Operator::I32AtomicRmwCmpxchg { .. }))
    else {
        return Ok(ops.to_vec());
    };
    if cmp != 10
        || !matches!(ops[0], Operator::GlobalGet { .. })
        || !matches!(ops[1], Operator::I32Const { .. })
        || !matches!(ops[2], Operator::I32Add)
        || !matches!(ops[3], Operator::LocalSet { local_index: 0 })
        || !(4..7).all(|i| {
            matches!(
                ops[i],
                Operator::Block {
                    blockty: wasmparser::BlockType::Empty
                }
            )
        })
        || !matches!(ops[7], Operator::LocalGet { local_index: 0 })
        || !matches!(ops[8], Operator::I32Const { value: 0 })
        || !matches!(ops[9], Operator::I32Const { value: 1 })
        || !matches!(ops[10], Operator::I32AtomicRmwCmpxchg { memarg } if memarg.memory == 0 && memarg.offset == 0 && memarg.align == 2)
        || !matches!(ops.get(12), Some(Operator::End))
    {
        return Err(invalid());
    }
    let Operator::BrTable { ref targets } = ops[11] else {
        return Err(invalid());
    };
    if targets.default() != 2 || targets.targets().collect::<Result<Vec<_>, _>>()? != [0, 1] {
        return Err(invalid());
    }
    let store = ops
        .iter()
        .position(|op| matches!(op, Operator::I32AtomicStore { .. }))
        .ok_or_else(invalid)?;
    if store < 15 {
        return Err(invalid());
    }
    let tail = &ops[store - 2..];
    if tail.len() < 16 {
        return Err(invalid());
    }
    let pattern = matches!(tail[0], Operator::LocalGet { local_index: 0 })
        && matches!(tail[1], Operator::I32Const { value: 2 })
        && matches!(tail[2], Operator::I32AtomicStore { memarg } if memarg.memory == 0 && memarg.offset == 0 && memarg.align == 2)
        && matches!(tail[3], Operator::LocalGet { local_index: 0 })
        && matches!(tail[4], Operator::I32Const { value: -1 })
        && matches!(tail[5], Operator::MemoryAtomicNotify { memarg } if memarg.memory == 0 && memarg.offset == 0 && memarg.align == 2)
        && matches!(tail[6], Operator::Drop)
        && matches!(tail[7], Operator::Br { relative_depth: 1 })
        && matches!(tail[8], Operator::End)
        && matches!(tail[9], Operator::LocalGet { local_index: 0 })
        && matches!(tail[10], Operator::I32Const { value: 1 })
        && matches!(tail[11], Operator::I64Const { value: -1 })
        && matches!(tail[12], Operator::MemoryAtomicWait32 { memarg } if memarg.memory == 0 && memarg.offset == 0 && memarg.align == 2)
        && matches!(tail[13], Operator::Drop)
        && matches!(tail[14], Operator::End);
    if !pattern
        || !matches!(tail.last(), Some(Operator::End))
        || !tail[15..tail.len() - 1]
            .iter()
            .all(|op| matches!(op, Operator::DataDrop { .. }))
    {
        return Err(invalid());
    }
    let mut result = if first {
        ops[..4]
            .iter()
            .chain(&ops[13..store - 2])
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if first {
        result.extend([
            Operator::LocalGet { local_index: 0 },
            Operator::I32Const { value: 2 },
            Operator::I32Store {
                memarg: wasmparser::MemArg {
                    align: 2,
                    max_align: 2,
                    offset: 0,
                    memory: 0,
                },
            },
        ]);
    }
    result.extend_from_slice(&tail[15..]);
    Ok(result)
}

#[derive(Clone, Debug)]
enum Address {
    Constant(u32),
    Global(u32),
    Add(Box<Address>, Box<Address>),
}

impl Address {
    fn parse(expr: wasmparser::ConstExpr<'_>) -> Result<Self, PlanError> {
        let mut ops = expr.get_operators_reader();
        let mut stack = Vec::new();
        let mut count = 0;
        while !ops.eof() {
            count += 1;
            if count > 64 {
                return Err(invalid());
            }
            match ops.read()? {
                Operator::I32Const { value } => stack.push(Self::Constant(value as u32)),
                Operator::GlobalGet { global_index } => stack.push(Self::Global(global_index)),
                Operator::I32Add => {
                    let b = stack.pop().ok_or_else(invalid)?;
                    let a = stack.pop().ok_or_else(invalid)?;
                    stack.push(Self::Add(Box::new(a), Box::new(b)));
                }
                Operator::End if ops.eof() => {}
                _ => return Err(invalid()),
            }
            if stack.len() > 16 {
                return Err(invalid());
            }
        }
        if stack.len() != 1 {
            return Err(invalid());
        }
        Ok(stack.pop().unwrap())
    }

    fn value(&self, globals: &BTreeMap<u32, u64>) -> Result<u32, PlanError> {
        Ok(match self {
            Self::Constant(value) => *value,
            Self::Global(index) => {
                u32::try_from(*globals.get(index).ok_or_else(invalid)?).map_err(|_| invalid())?
            }
            Self::Add(a, b) => a.value(globals)?.wrapping_add(b.value(globals)?),
        })
    }
}

struct Sanitize<'a> {
    dropped: &'a BTreeSet<u32>,
    data: u32,
    functions: u32,
    globals: u32,
    memories: u32,
    tables: u32,
    aliases: &'a [(String, wasmparser::ExternalKind, u32)],
    tags: u32,
}

impl wasm_encoder::reencode::Reencode for Sanitize<'_> {
    type Error = std::convert::Infallible;

    fn parse_export_section(
        &mut self,
        exports: &mut wasm_encoder::ExportSection,
        section: wasmparser::ExportSectionReader<'_>,
    ) -> Result<(), wasm_encoder::reencode::Error<Self::Error>> {
        wasm_encoder::reencode::utils::parse_export_section(self, exports, section)?;
        for i in 0..self.functions {
            exports.export(
                &format!("__wasix_dylink_fn_{i}"),
                wasm_encoder::ExportKind::Func,
                i,
            );
        }
        for i in 0..self.memories {
            exports.export(
                &format!("__wasix_dylink_memory_{i}"),
                wasm_encoder::ExportKind::Memory,
                i,
            );
        }
        for i in 0..self.tables {
            exports.export(
                &format!("__wasix_dylink_table_{i}"),
                wasm_encoder::ExportKind::Table,
                i,
            );
        }
        for (name, kind, index) in self.aliases {
            exports.export(
                name,
                match kind {
                    wasmparser::ExternalKind::Global => wasm_encoder::ExportKind::Global,
                    _ => wasm_encoder::ExportKind::Table,
                },
                *index,
            );
        }
        for i in 0..self.tags {
            exports.export(
                &format!("__wasix_dylink_tag_{i}"),
                wasm_encoder::ExportKind::Tag,
                i,
            );
        }
        for i in 0..self.globals {
            exports.export(
                &format!("__wasix_dylink_global_{i}"),
                wasm_encoder::ExportKind::Global,
                i,
            );
        }
        Ok(())
    }

    fn parse_data(
        &mut self,
        data: &mut wasm_encoder::DataSection,
        datum: wasmparser::Data<'_>,
    ) -> Result<(), wasm_encoder::reencode::Error<Self::Error>> {
        let empty =
            matches!(datum.kind, DataKind::Active { .. }) || self.dropped.contains(&self.data);
        self.data += 1;
        if empty {
            data.passive([]);
        } else {
            wasm_encoder::reencode::utils::parse_data(self, data, datum)?;
        }
        Ok(())
    }

    fn parse_element(
        &mut self,
        elements: &mut wasm_encoder::ElementSection,
        element: wasmparser::Element<'_>,
    ) -> Result<(), wasm_encoder::reencode::Error<Self::Error>> {
        if matches!(element.kind, wasmparser::ElementKind::Active { .. }) {
            elements.declared(self.element_items(element.items)?);
        } else {
            wasm_encoder::reencode::utils::parse_element(self, elements, element)?;
        }
        Ok(())
    }
}
