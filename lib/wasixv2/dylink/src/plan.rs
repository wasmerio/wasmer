use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    ops::Range,
};

use wasmparser::{SymbolFlags, ValType};

use crate::{Export, ExternType, Import, Module};

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error(transparent)]
    Wasm(#[from] wasmparser::BinaryReaderError),
    #[error("invalid dylink metadata: {0}")]
    Metadata(&'static str),
    #[error("unsupported module: {0}")]
    Unsupported(&'static str),
    #[error("dependency graph: {0}")]
    Graph(&'static str),
    #[error("relative allocation exceeds the wasm32 address space")]
    LayoutOverflow,
    #[error("module {module}: missing symbol {name}")]
    MissingSymbol { module: usize, name: String },
    #[error("module {module}: incompatible symbol {name}")]
    TypeMismatch { module: usize, name: String },
}

/// Only the planning policy: Lazy records missing references; it creates no runtime stubs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingMode {
    Now,
    Lazy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub module: usize,
    pub memory: Range<u64>,
    pub table: Range<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixupKind {
    DirectFunction,
    GotFunction,
    GotMemory,
    Host,
}

/// All indices/slots are plan-relative. Even known offsets need the eventual image's base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingTarget {
    Function {
        module: usize,
        index: u32,
        slot: Option<u32>,
    },
    Data {
        module: usize,
        index: u32,
        offset: Option<u32>,
        tls: bool,
        absolute: bool,
    },
    /// Supplied by exec/ABI glue, whose signature must be checked by the eventual binder.
    Host,
    /// A weak missing reference; zero is not a callable function stub.
    WeakZero,
    /// A missing Lazy reference. Runtime binding and failure policy are not implemented.
    Deferred,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixup {
    pub module: usize,
    /// Ordinal in the whole import section, not the per-kind wasm index space.
    pub import: usize,
    pub kind: FixupKind,
    pub target: BindingTarget,
}

/// This core deliberately does not evaluate expressions, instantiate imports or reconcile
/// exported/re-exported functions with element-segment slots. No proposed slot is publishable
/// until its function identity is reconciled with those segments and imported aliases.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeferredCheck {
    HostImport {
        module: usize,
        import: usize,
    },
    GlobalValue {
        module: usize,
        index: u32,
    },
    FunctionIdentity {
        module: usize,
        index: u32,
        proposed_slot: u32,
    },
}

/// A deterministic plan for a caller-supplied, already-opened module graph.
///
/// No filesystem policy, runtime handle, lifetime or synchronization protocol is chosen here.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Breadth-first local lookup/load order, followed by supplied global scope dependencies.
    pub load_order: Vec<usize>,
    /// Dependency-first DFS postorder; cycle back-edges are ignored. This is not a guarantee
    /// that constructors completed (the libc constructor protocol remains a separate stage).
    pub initialization_order: Vec<usize>,
    pub layouts: Vec<Layout>,
    pub fixups: Vec<Fixup>,
    /// Checks the live binder must complete before publishing any result.
    pub deferred_checks: Vec<DeferredCheck>,
    pub memory_size: u64,
    pub table_size: u64,
    /// The whole reservation must have at least these alignments, not just its subranges.
    pub memory_alignment: u32,
    pub table_alignment: u32,
}

impl Plan {
    /// `dependencies[i][j]` identifies the module opened for `modules[i].needed()[j]`.
    /// The caller deduplicates opened-file identities; filenames are not identities.
    /// `global_scope` is the caller's existing global symbol order. All referenced modules
    /// receive relative layouts; adding these to live-image addresses is not implemented.
    pub fn new(
        modules: &[Module],
        dependencies: &[Vec<usize>],
        root: usize,
        global_scope: &[usize],
        mode: BindingMode,
    ) -> Result<Self, PlanError> {
        if root >= modules.len() || dependencies.len() != modules.len() {
            return Err(PlanError::Graph("missing root or dependency list"));
        }
        for (module, deps) in modules.iter().zip(dependencies) {
            if deps.len() != module.needed.len() || deps.iter().any(|i| *i >= modules.len()) {
                return Err(PlanError::Graph(
                    "needed list does not match supplied dependency edges",
                ));
            }
        }
        if global_scope.iter().any(|i| *i >= modules.len()) {
            return Err(PlanError::Graph("global scope index out of range"));
        }
        let local_scope = breadth_first(std::iter::once(root), dependencies);
        let load_order = breadth_first(
            local_scope
                .iter()
                .copied()
                .chain(global_scope.iter().copied()),
            dependencies,
        );
        let initialization_order = postorder(&load_order, dependencies);
        let mut memory_size = 0;
        let mut table_size = 3; // 0=null, 1=SIG_IGN, 2=SIG_HOLD (existing wasixv2 signal ABI).
        let mut memory_alignment = 1;
        let mut table_alignment = 1;
        let mut layouts = Vec::new();
        for &module in &load_order {
            let info = modules[module].memory;
            memory_alignment = memory_alignment.max(1u32 << info.memory_alignment);
            table_alignment = table_alignment.max(1u32 << info.table_alignment);
            layouts.push(Layout {
                module,
                memory: allocate(&mut memory_size, info.memory_size, info.memory_alignment)?,
                table: allocate(&mut table_size, info.table_size, info.table_alignment)?,
            });
        }
        let mut fixups = Vec::new();
        let mut slots = BTreeMap::new();
        let mut deferred_checks = Vec::new();
        for &module in &load_order {
            let scope = breadth_first(std::iter::once(module), dependencies);
            // musl ldso/dynlink.c:find_sym2 takes the first visible definition, including
            // weak definitions. A later strong definition does not displace an earlier weak.
            let lookup = global_scope
                .iter()
                .chain(local_scope.iter())
                .chain(scope.iter())
                .copied()
                .collect::<Vec<_>>();
            for (index, import) in modules[module].imports.iter().enumerate() {
                let kind = classify(import)?;
                let target = if kind == FixupKind::Host {
                    BindingTarget::Host
                } else if let Some((provider, export)) = find(modules, module, import, &lookup) {
                    bind(
                        module,
                        import,
                        kind,
                        provider,
                        export,
                        &mut slots,
                        &mut table_size,
                    )?
                } else if import.flags.contains(SymbolFlags::BINDING_WEAK) {
                    BindingTarget::WeakZero
                } else if mode == BindingMode::Lazy {
                    BindingTarget::Deferred
                } else {
                    return Err(PlanError::MissingSymbol {
                        module,
                        name: import.name.clone(),
                    });
                };
                match &target {
                    BindingTarget::Host => deferred_checks.push(DeferredCheck::HostImport {
                        module,
                        import: index,
                    }),
                    BindingTarget::Function {
                        module,
                        index,
                        slot: Some(slot),
                    } => {
                        let check = DeferredCheck::FunctionIdentity {
                            module: *module,
                            index: *index,
                            proposed_slot: *slot,
                        };
                        if !deferred_checks.contains(&check) {
                            deferred_checks.push(check);
                        }
                    }
                    BindingTarget::Data {
                        module,
                        index,
                        offset: None,
                        ..
                    } => deferred_checks.push(DeferredCheck::GlobalValue {
                        module: *module,
                        index: *index,
                    }),
                    _ => {}
                }
                fixups.push(Fixup {
                    module,
                    import: index,
                    kind,
                    target,
                });
            }
        }
        Ok(Self {
            load_order,
            initialization_order,
            layouts,
            fixups,
            deferred_checks,
            memory_size,
            table_size,
            memory_alignment,
            table_alignment,
        })
    }
}

fn allocate(cursor: &mut u64, size: u32, exponent: u32) -> Result<Range<u64>, PlanError> {
    let align = 1u64
        .checked_shl(exponent)
        .ok_or(PlanError::LayoutOverflow)?;
    let start = cursor
        .checked_add(align - 1)
        .ok_or(PlanError::LayoutOverflow)?
        & !(align - 1);
    let end = start
        .checked_add(u64::from(size))
        .filter(|end| *end <= 1u64 << 32)
        .ok_or(PlanError::LayoutOverflow)?;
    *cursor = end;
    Ok(start..end)
}

fn breadth_first(
    roots: impl IntoIterator<Item = usize>,
    dependencies: &[Vec<usize>],
) -> Vec<usize> {
    let mut queue = VecDeque::from_iter(roots);
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    while let Some(module) = queue.pop_front() {
        if seen.insert(module) {
            order.push(module);
            queue.extend(dependencies[module].iter().copied());
        }
    }
    order
}

fn postorder(roots: &[usize], dependencies: &[Vec<usize>]) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut order = Vec::new();
    for &root in roots {
        let mut stack = vec![(root, false)];
        while let Some((module, done)) = stack.pop() {
            if done {
                order.push(module);
            } else if seen.insert(module) {
                stack.push((module, true));
                stack.extend(dependencies[module].iter().rev().map(|&dep| (dep, false)));
            }
        }
    }
    order
}

fn classify(import: &Import) -> Result<FixupKind, PlanError> {
    match import.module.as_str() {
        "GOT.func" | "GOT.mem" => {
            if !matches!(import.ty, ExternType::Global(ty) if ty.content_type == ValType::I32 && ty.mutable && !ty.shared)
            {
                return Err(PlanError::Metadata(
                    "GOT import must be a mutable nonshared i32 global",
                ));
            }
            Ok(if import.module == "GOT.func" {
                FixupKind::GotFunction
            } else {
                FixupKind::GotMemory
            })
        }
        "env"
            if matches!(
                import.name.as_str(),
                "memory"
                    | "__indirect_function_table"
                    | "__stack_pointer"
                    | "__memory_base"
                    | "__table_base"
                    | "__tls_base"
                    | "__cpp_exception"
                    | "__c_longjmp"
            ) =>
        {
            Ok(FixupKind::Host)
        }
        "env" if matches!(import.ty, ExternType::Function(_)) => Ok(FixupKind::DirectFunction),
        "env" => Err(PlanError::Unsupported("non-function env symbol")),
        _ => Ok(FixupKind::Host),
    }
}

fn find<'a>(
    modules: &'a [Module],
    caller: usize,
    import: &Import,
    scope: &[usize],
) -> Option<(usize, &'a Export)> {
    let private = SymbolFlags::VISIBILITY_HIDDEN | SymbolFlags::BINDING_LOCAL;
    if let Some(export) = modules[caller].exports.get(&import.name)
        && export.flags.intersects(private)
    {
        return Some((caller, export));
    }
    // Hidden/local imports bind to their own module, not a preempting global definition.
    if import.flags.intersects(private) {
        return modules[caller]
            .exports
            .get(&import.name)
            .map(|export| (caller, export));
    }
    scope.iter().find_map(|&module| {
        let export = modules[module].exports.get(&import.name)?;
        (!export.flags.intersects(private)).then_some((module, export))
    })
}

fn bind(
    caller: usize,
    import: &Import,
    kind: FixupKind,
    module: usize,
    export: &Export,
    slots: &mut BTreeMap<(usize, u32), u32>,
    table_size: &mut u64,
) -> Result<BindingTarget, PlanError> {
    let mismatch = || PlanError::TypeMismatch {
        module: caller,
        name: import.name.clone(),
    };
    match (&export.ty, kind) {
        (ExternType::Function(ty), FixupKind::DirectFunction | FixupKind::GotFunction) => {
            if kind == FixupKind::DirectFunction && import.ty != ExternType::Function(ty.clone()) {
                return Err(mismatch());
            }
            let slot = if kind == FixupKind::GotFunction {
                let key = (module, export.index);
                Some(match slots.get(&key) {
                    Some(&slot) => slot,
                    None => {
                        let slot =
                            u32::try_from(*table_size).map_err(|_| PlanError::LayoutOverflow)?;
                        *table_size += 1;
                        slots.insert(key, slot);
                        slot
                    }
                })
            } else {
                None
            };
            Ok(BindingTarget::Function {
                module,
                index: export.index,
                slot,
            })
        }
        (ExternType::Global(ty), FixupKind::GotMemory)
            if ty.content_type == ValType::I32 && !ty.mutable && !ty.shared =>
        {
            Ok(BindingTarget::Data {
                module,
                index: export.index,
                offset: export.offset,
                tls: export.flags.contains(SymbolFlags::TLS),
                absolute: export.flags.contains(SymbolFlags::ABSOLUTE),
            })
        }
        _ => Err(mismatch()),
    }
}
