use super::{Result, failure};
use crate::{BindingMode, BindingTarget, ExternType, Fixup, Initialization, Module, Plan};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use wasmparser::{Operator, Parser, Payload, SymbolFlags, TypeRef};

#[derive(Clone)]
pub(super) struct Library {
    pub source: Arc<Vec<u8>>,
    pub metadata: Module,
    pub identity: Option<(u64, u64)>,
    pub backing: Option<wasixv2_syscalls::Image>,
    pub dependencies: Vec<usize>,
    pub fixups: Vec<Fixup>,
    pub base: u32,
    pub table: u32,
    pub slots: BTreeMap<u32, u32>,
    pub tls_size: u32,
    pub tls_align: u32,
}
impl Library {
    pub fn new(source: Vec<u8>, identity: Option<(u64, u64)>) -> Result<Self> {
        let metadata = Module::parse(&source).map_err(failure)?;
        Initialization::parse(&source).map_err(failure)?;
        if metadata
            .exports()
            .keys()
            .any(|name| name.starts_with("asyncify_"))
        {
            return Err(failure("asyncify dynamic image"));
        }
        let scalar = |name: &str, default| -> Result<u32> {
            metadata.exports().get(name).map_or(Ok(default), |symbol| {
                symbol
                    .offset
                    .ok_or_else(|| failure("nonliteral TLS layout"))
            })
        };
        let tls_size = scalar("__tls_size", 0)?;
        // lld exports alignment zero when no TLS block exists. No allocation or
        // alignment is needed for that shape; normalize the unused internal value.
        let tls_align = if tls_size == 0 {
            1
        } else {
            scalar("__tls_align", 1)?
        };
        if !tls_align.is_power_of_two() {
            return Err(failure("invalid TLS alignment"));
        }
        Ok(Self {
            source: Arc::new(source),
            metadata,
            identity,
            backing: None,
            dependencies: Vec::new(),
            fixups: Vec::new(),
            base: 0,
            table: 3,
            slots: BTreeMap::new(),
            tls_size,
            tls_align,
        })
    }
}
#[derive(Clone)]
pub(super) struct Graph {
    pub libraries: Vec<Library>,
    pub global: Vec<usize>,
    pub cursor: u32,
    pub initialization: Vec<usize>,
}
impl Graph {
    pub fn scope(&self, root: usize) -> Vec<usize> {
        let mut result = Vec::new();
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([root]);
        while let Some(index) = queue.pop_front() {
            if seen.insert(index) {
                result.push(index);
                queue.extend(self.libraries[index].dependencies.iter().copied());
            }
        }
        result
    }
    pub fn plan(&self, root: usize, previously_bound: usize) -> Result<Plan> {
        Plan::for_load(
            &self
                .libraries
                .iter()
                .map(|lib| lib.metadata.clone())
                .collect::<Vec<_>>(),
            &self
                .libraries
                .iter()
                .map(|lib| lib.dependencies.clone())
                .collect::<Vec<_>>(),
            root,
            &self.global,
            BindingMode::Now,
            previously_bound,
        )
        .map_err(failure)
    }
    /// Freeze NOW bindings and dependency order when each load group joins the image.
    /// Future stores replay every group, including earlier RTLD_LOCAL groups, without
    /// rebinding old imports against a later RTLD_GLOBAL scope (musl do_dlsym/reloc_all).
    pub fn seal(&mut self, root: usize, previous: usize) -> Result<()> {
        let plan = self.plan(root, previous)?;
        for index in previous..self.libraries.len() {
            self.libraries[index].fixups = plan
                .fixups
                .iter()
                .filter(|fixup| fixup.module == index)
                .cloned()
                .collect();
        }
        self.initialization.extend(
            plan.initialization_order
                .into_iter()
                .filter(|index| *index >= previous),
        );
        if (previous..self.libraries.len()).any(|index| !self.initialization.contains(&index)) {
            return Err(failure("new library missing from publication order"));
        }
        Ok(())
    }
    pub fn assign_slots(&mut self, index: usize) -> Result<()> {
        let lib = &self.libraries[index];
        let globals = initial_globals(&lib.source, infrastructure(lib, 0, 0))?;
        let end = u64::from(lib.table) + u64::from(lib.metadata.memory_info().table_size);
        let placements = Initialization::parse(&lib.source)
            .map_err(failure)?
            .table_effects(&globals, std::slice::from_ref(&(u64::from(lib.table)..end)))
            .map_err(failure)?;
        let mut slots = BTreeMap::new();
        let mut occupied = BTreeMap::new();
        for placement in placements {
            for (offset, function) in placement.functions.iter().enumerate() {
                if let Some(function) = function {
                    let slot = placement
                        .index
                        .checked_add(offset as u32)
                        .ok_or_else(|| failure("table overflow"))?;
                    if slot < 3 {
                        return Err(failure("signal sentinel element"));
                    }
                    if occupied
                        .insert(slot, *function)
                        .is_some_and(|old| old != *function)
                    {
                        return Err(failure("overlapping distinct element functions"));
                    }
                    slots.entry(*function).or_insert(slot);
                }
            }
        }
        for export in lib.metadata.exports().values() {
            if matches!(export.ty, ExternType::Function(_)) && !slots.contains_key(&export.index) {
                let slot = self.cursor;
                self.cursor = self
                    .cursor
                    .checked_add(1)
                    .ok_or_else(|| failure("table overflow"))?;
                slots.insert(export.index, slot);
            }
        }
        self.libraries[index].slots = slots;
        Ok(())
    }
    pub fn address(
        &self,
        module: usize,
        index: u32,
        offset: Option<u32>,
        tls: bool,
        absolute: bool,
        tls_bases: &[u32],
    ) -> Result<u32> {
        let offset = offset.ok_or_else(|| failure(format!("deferred data global {index}")))?;
        let base = if absolute {
            0
        } else if tls {
            if module == 0 {
                return Err(failure(
                    "side import of main TLS requires post-libc binding",
                ));
            }
            *tls_bases
                .get(module)
                .ok_or_else(|| failure("missing store TLS"))?
        } else {
            self.libraries[module].base
        };
        base.checked_add(offset)
            .ok_or_else(|| failure("symbol address overflow"))
    }
    pub fn target_address(&self, target: &BindingTarget, tls: &[u32]) -> Result<u32> {
        match target {
            BindingTarget::Function { module, index, .. } => self.libraries[*module]
                .slots
                .get(index)
                .copied()
                .ok_or_else(|| failure("unassigned function slot")),
            BindingTarget::Data {
                module,
                index,
                offset,
                tls: is_tls,
                absolute,
            } => self.address(*module, *index, *offset, *is_tls, *absolute, tls),
            BindingTarget::WeakZero => Ok(0),
            _ => Err(failure("unresolved import")),
        }
    }
    pub fn symbol(&self, handle: usize, name: &str, tls: &[u32]) -> Result<u32> {
        for index in self.scope(handle) {
            if let Some(export) = self.libraries[index].metadata.exports().get(name) {
                if export
                    .flags
                    .intersects(SymbolFlags::VISIBILITY_HIDDEN | SymbolFlags::BINDING_LOCAL)
                {
                    continue;
                }
                return match export.ty {
                    ExternType::Function(_) => self.libraries[index]
                        .slots
                        .get(&export.index)
                        .copied()
                        .ok_or_else(|| failure("unassigned export slot")),
                    ExternType::Global(_) => self.address(
                        index,
                        export.index,
                        export.offset,
                        export.flags.contains(SymbolFlags::TLS),
                        export.flags.contains(SymbolFlags::ABSOLUTE),
                        tls,
                    ),
                    _ => Err(failure("symbol is not a function or data")),
                };
            }
        }
        Err(failure(format!("missing symbol {name}")))
    }
}
/// Literal base inputs before engine instantiation; the fully resolved GOT values replace
/// the zero placeholders before any initialization plan is applied.
pub(super) fn infrastructure(lib: &Library, stack: u32, tls: u32) -> BTreeMap<u32, u64> {
    let mut index = 0;
    let mut globals = BTreeMap::new();
    for import in lib.metadata.imports() {
        if matches!(import.ty, ExternType::Global(_)) {
            globals.insert(
                index,
                u64::from(match import.name.as_str() {
                    "__memory_base" => lib.base,
                    "__table_base" => lib.table,
                    "__stack_pointer" => stack,
                    "__tls_base" => tls,
                    _ => 0,
                }),
            );
            index += 1;
        }
    }
    globals
}
pub(super) fn initial_globals(
    source: &[u8],
    mut globals: BTreeMap<u32, u64>,
) -> Result<BTreeMap<u32, u64>> {
    let mut index = 0;
    for payload in Parser::new(0).parse_all(source) {
        match payload.map_err(failure)? {
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    if matches!(import.map_err(failure)?.ty, TypeRef::Global(_)) {
                        index += 1;
                    }
                }
            }
            Payload::GlobalSection(section) => {
                for global in section {
                    let global = global.map_err(failure)?;
                    let value = match global
                        .init_expr
                        .get_operators_reader()
                        .read()
                        .map_err(failure)?
                    {
                        Operator::I32Const { value } => u64::from(value as u32),
                        Operator::I64Const { value } => value as u64,
                        Operator::GlobalGet { global_index } => *globals
                            .get(&global_index)
                            .ok_or_else(|| failure("missing initializer global"))?,
                        _ => return Err(failure("unsupported global initializer")),
                    };
                    globals.insert(index, value);
                    index += 1;
                }
            }
            _ => {}
        }
    }
    Ok(globals)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tls_needs_no_alignment_but_nonempty_tls_requires_a_power_of_two() {
        let library = |size, align| {
            Library::new(
                crate::tests::bytes(
                    &format!(
                        "(module (global (export \"__tls_size\") i32 (i32.const {size})) (global (export \"__tls_align\") i32 (i32.const {align})))"
                    ),
                    [0; 4],
                    &[],
                    &[],
                    &[],
                ),
                None,
            )
        };
        for align in [0, 3] {
            let empty = library(0, align).unwrap();
            assert_eq!((empty.tls_size, empty.tls_align), (0, 1));
            assert!(library(4, align).is_err());
        }
        let nonempty = library(4, 4).unwrap();
        assert_eq!((nonempty.tls_size, nonempty.tls_align), (4, 4));
    }

    #[test]
    fn later_global_definitions_do_not_revalidate_previously_bound_weak_imports() {
        let library = |wat: &str, imports: &[(&str, &str, SymbolFlags)]| {
            Library::new(crate::tests::bytes(wat, [0; 4], &[], &[], imports), None).unwrap()
        };
        let mut graph = Graph {
            libraries: vec![
                library("(module)", &[]),
                library(
                    "(module (import \"GOT.mem\" \"late\" (global (mut i32))))",
                    &[(
                        "env",
                        "late",
                        SymbolFlags::UNDEFINED | SymbolFlags::BINDING_WEAK,
                    )],
                ),
            ],
            global: vec![0, 1],
            cursor: 3,
            initialization: vec![0],
        };
        graph.seal(1, 1).unwrap();
        assert_eq!(graph.libraries[1].fixups[0].target, BindingTarget::WeakZero);
        graph
            .libraries
            .push(library("(module (func (export \"late\")))", &[]));
        graph.global.push(2);
        assert!(graph.plan(2, 0).is_err());
        graph.seal(2, 2).unwrap();
        assert_eq!(graph.libraries[1].fixups[0].target, BindingTarget::WeakZero);
    }

    #[test]
    fn future_stores_replay_all_local_groups_and_preserve_original_now_bindings() {
        let library = |wat: &str| {
            Library::new(crate::tests::bytes(wat, [0; 4], &[], &[], &[]), None).unwrap()
        };
        let mut graph = Graph {
            libraries: vec![
                library("(module)"),
                library("(module (import \"env\" \"choice\" (func)) (func (export \"choice\")))"),
            ],
            global: vec![0],
            cursor: 3,
            initialization: vec![0],
        };
        graph.seal(1, 1).unwrap();
        assert!(matches!(
            graph.libraries[1].fixups[0].target,
            BindingTarget::Function { module: 1, .. }
        ));
        graph
            .libraries
            .push(library("(module (func (export \"choice\")))"));
        graph.global.push(2);
        graph.seal(2, 2).unwrap();
        // Replanning the latest root alone omits the earlier local group entirely.
        assert!(!graph.plan(2, 0).unwrap().initialization_order.contains(&1));
        assert_eq!(graph.initialization, [0, 1, 2]);
        // A later global definition must not change a prior NOW import on a new peer.
        assert!(matches!(
            graph.libraries[1].fixups[0].target,
            BindingTarget::Function { module: 1, .. }
        ));
    }
}
