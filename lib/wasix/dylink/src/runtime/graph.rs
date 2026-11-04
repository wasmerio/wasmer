use super::{Result, failure};
use crate::{BindingMode, BindingTarget, ExternType, Fixup, Initialization, Module, Plan};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};
use wasmparser::{Operator, Parser, Payload, SymbolFlags, TypeRef};

/// A loader-owned direct-call slot, retained after its first concrete binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LazyFunction {
    pub slot: u32,
    pub root: usize,
    pub target: Option<(usize, u32)>,
}

#[derive(Clone)]
pub(super) struct Library {
    pub source: Arc<Vec<u8>>,
    pub metadata: Module,
    pub initialization: Arc<Initialization>,
    pub code_hash: wasmer_types::ModuleHash,
    pub prepared: Option<Arc<crate::artifact::PreparedArtifact>>,
    pub identity: Option<(u64, u64)>,
    pub path: Vec<u8>,
    pub request: Vec<u8>,
    pub backing: Option<wasixv2_syscalls::Image>,
    pub dependencies: Vec<usize>,
    pub fixups: Vec<Fixup>,
    pub lazy: BTreeMap<usize, LazyFunction>,
    pub base: u32,
    pub table: u32,
    pub slots: BTreeMap<u32, u32>,
    pub tls_size: u32,
    pub tls_align: u32,
}
impl Library {
    pub fn new(source: Vec<u8>, identity: Option<(u64, u64)>) -> Result<Self> {
        let metadata = Module::parse(&source).map_err(failure)?;
        let initialization = Arc::new(Initialization::parse(&source).map_err(failure)?);
        let code_hash = wasmer_types::ModuleHash::new(&source);
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
            initialization,
            code_hash,
            prepared: None,
            identity,
            backing: None,
            path: b"./main.wasm".to_vec(),
            request: Vec::new(),
            dependencies: Vec::new(),
            fixups: Vec::new(),
            lazy: BTreeMap::new(),
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
    pub host_imports: Option<wasixv2_exec::ImportDeclarations>,
    pub libraries: Vec<Library>,
    pub global: Vec<usize>,
    pub cursor: u32,
    pub initialization: Vec<usize>,
}
impl Graph {
    /// Reorders only unpublished nodes. Existing handles/slots never move.
    /// Iterative DFS marks on discovery, so a back edge cannot recurse forever.
    pub fn dependencies_first(&mut self, previous: usize) -> Vec<usize> {
        let mut seen = (0..previous).collect::<BTreeSet<_>>();
        let mut order = (0..previous).collect::<Vec<_>>();
        for root in previous..self.libraries.len() {
            let mut stack = vec![(root, false)];
            while let Some((index, done)) = stack.pop() {
                if done {
                    order.push(index);
                } else if seen.insert(index) {
                    stack.push((index, true));
                    stack.extend(
                        self.libraries[index]
                            .dependencies
                            .iter()
                            .rev()
                            .map(|index| (*index, false)),
                    );
                }
            }
        }
        let mut indices = vec![0; order.len()];
        for (new, old) in order.iter().copied().enumerate() {
            indices[old] = new;
        }
        self.libraries = order
            .into_iter()
            .map(|index| self.libraries[index].clone())
            .collect();
        for lib in &mut self.libraries {
            for index in &mut lib.dependencies {
                *index = indices[*index];
            }
        }
        for index in self.global.iter_mut().chain(&mut self.initialization) {
            *index = indices[*index];
        }
        indices
    }

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
    #[cfg(test)]
    pub fn plan(&self, root: usize, previously_bound: usize) -> Result<Plan> {
        self.plan_mode(root, previously_bound, BindingMode::Now)
    }
    fn plan_mode(&self, root: usize, previously_bound: usize, mode: BindingMode) -> Result<Plan> {
        Plan::for_load_with_host_imports(
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
            mode,
            previously_bound,
            &|namespace, name| {
                self.host_imports
                    .as_ref()
                    .is_some_and(|hosts| hosts.provides_import(namespace, name))
            },
        )
        .map_err(failure)
    }
    /// Freeze NOW bindings and dependency order when each load group joins the image.
    /// Future stores replay every group, including earlier RTLD_LOCAL groups, without
    /// rebinding old imports against a later RTLD_GLOBAL scope (musl do_dlsym/reloc_all).
    pub fn seal(&mut self, root: usize, previous: usize) -> Result<()> {
        self.seal_mode(root, previous, BindingMode::Now)
    }
    /// Only new strong direct-function misses defer. Startup and NOW remain eager.
    pub fn seal_lazy(&mut self, root: usize, previous: usize) -> Result<()> {
        self.seal_mode(root, previous, BindingMode::Lazy)
    }
    fn seal_mode(&mut self, root: usize, previous: usize, mode: BindingMode) -> Result<()> {
        if previous > self.libraries.len() {
            return Err(failure("previous graph length exceeds current graph"));
        }
        let plan = self.plan_mode(root, previous, mode)?;
        for index in previous..self.libraries.len() {
            self.libraries[index].fixups = plan
                .fixups
                .iter()
                .filter(|fixup| fixup.module == index)
                .cloned()
                .collect();
        }
        for module in previous..self.libraries.len() {
            let deferred = self.libraries[module]
                .fixups
                .iter()
                .filter(|fixup| fixup.target == BindingTarget::Deferred)
                .map(|fixup| fixup.import)
                .collect::<Vec<_>>();
            for ordinal in deferred {
                if deferred_identity(&self.libraries[module], ordinal)? {
                    return Err(failure(
                        "deferred function requires a concrete exported or referenced identity",
                    ));
                }
                let slot = self.cursor;
                if slot < 3 {
                    return Err(failure("signal sentinel lazy slot"));
                }
                self.cursor = slot
                    .checked_add(1)
                    .ok_or_else(|| failure("table overflow"))?;
                self.libraries[module].lazy.insert(
                    ordinal,
                    LazyFunction {
                        slot,
                        root,
                        target: None,
                    },
                );
            }
        }
        // Re-exports name module-local imported functions. Resolve them before
        // instantiation so a cycle never forwards to its own imported alias.
        for module in previous..self.libraries.len() {
            let mut fixups = self.libraries[module].fixups.clone();
            for fixup in &mut fixups {
                if let BindingTarget::Function { module, index, .. } = &mut fixup.target {
                    (*module, *index) = self.function_definition(*module, *index)?;
                }
            }
            self.libraries[module].fixups = fixups;
        }
        // musl ldso/dynlink.c:redo_lazy_relocs retries unresolved references after
        // scope extension. Preserve original groups and never revisit frozen NOW or
        // previously resolved lazy targets. Metadata-only promotion has no new imports.
        let modules = self
            .libraries
            .iter()
            .map(|lib| lib.metadata.clone())
            .collect::<Vec<_>>();
        let dependencies = self
            .libraries
            .iter()
            .map(|lib| lib.dependencies.clone())
            .collect::<Vec<_>>();
        let unresolved = self
            .libraries
            .iter()
            .take(previous)
            .enumerate()
            .flat_map(|(module, lib)| {
                lib.lazy
                    .iter()
                    .filter(|(_, lazy)| lazy.target.is_none())
                    .map(move |(ordinal, lazy)| (module, *ordinal, lazy.root))
            })
            .collect::<Vec<_>>();
        for (module, ordinal, group) in unresolved {
            let Some(mut target) = Plan::retry_direct(
                &modules,
                &dependencies,
                group,
                &self.global,
                module,
                ordinal,
            )
            .map_err(failure)?
            else {
                continue;
            };
            let BindingTarget::Function {
                module: provider,
                index,
                ..
            } = &mut target
            else {
                return Err(failure("deferred function resolved to a non-function"));
            };
            (*provider, *index) = self.function_definition(*provider, *index)?;
            let concrete = (*provider, *index);
            let lib = &mut self.libraries[module];
            let fixup = lib
                .fixups
                .iter_mut()
                .find(|fixup| fixup.import == ordinal)
                .ok_or_else(|| failure("missing deferred fixup"))?;
            if fixup.target != BindingTarget::Deferred {
                return Err(failure("unresolved lazy edge lost its deferred fixup"));
            }
            fixup.target = target;
            lib.lazy.get_mut(&ordinal).unwrap().target = Some(concrete);
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
    pub fn function_definition(&self, mut module: usize, mut index: u32) -> Result<(usize, u32)> {
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert((module, index)) {
                return Err(failure("function alias cycle has no definition"));
            }
            let lib = &self.libraries[module];
            let signature = lib
                .metadata
                .functions
                .get(index as usize)
                .ok_or_else(|| failure("function index out of bounds"))?;
            let imported = lib
                .metadata
                .imports()
                .iter()
                .enumerate()
                .filter(|(_, import)| matches!(import.ty, ExternType::Function(_)))
                .nth(index as usize);
            let Some((ordinal, _)) = imported else {
                return Ok((module, index));
            };
            let fixup = lib.fixups.iter().find(|fixup| fixup.import == ordinal);
            match fixup.map(|fixup| &fixup.target) {
                None if module == 0 => return Ok((module, index)),
                // Main host imports already have concrete engine functions.
                Some(BindingTarget::Host) => return Ok((module, index)),
                Some(BindingTarget::Function {
                    module: next,
                    index: function,
                    ..
                }) => {
                    if self.libraries[*next]
                        .metadata
                        .functions
                        .get(*function as usize)
                        != Some(signature)
                    {
                        return Err(failure("function alias signature mismatch"));
                    }
                    (module, index) = (*next, *function);
                }
                _ => return Err(failure("function alias has no resolved definition")),
            }
        }
    }
    pub fn assign_slots(&mut self, index: usize) -> Result<()> {
        let lib = &self.libraries[index];
        let globals = initial_globals(&lib.source, infrastructure(lib, 0, 0))?;
        let end = u64::from(lib.table) + u64::from(lib.metadata.memory_info().table_size);
        let placements = lib
            .initialization
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
            let base = *tls_bases
                .get(module)
                .ok_or_else(|| failure("missing store TLS"))?;
            if module == 0 && base == 0 {
                return Err(failure("main TLS is not initialized"));
            }
            base
        } else {
            self.libraries[module].base
        };
        base.checked_add(offset)
            .ok_or_else(|| failure("symbol address overflow"))
    }
    /// Original global indices whose code reads follow the current store's main TLS.
    /// Keep scalar address resolution fail-closed: initializer snapshots cannot use this.
    pub fn main_tls_imports(&self, module: usize) -> Result<BTreeMap<u32, u32>> {
        let library = &self.libraries[module];
        let main = &self.libraries[0];
        let mut result = BTreeMap::new();
        let mut global = 0u32;
        for (ordinal, import) in library.metadata.imports().iter().enumerate() {
            let ExternType::Global(ty) = &import.ty else {
                continue;
            };
            let index = global;
            global = global
                .checked_add(1)
                .ok_or_else(|| failure("global overflow"))?;
            let Some(fixup) = library.fixups.iter().find(|fixup| fixup.import == ordinal) else {
                continue;
            };
            let BindingTarget::Data {
                module: 0,
                tls: true,
                absolute: false,
                offset,
                ..
            } = &fixup.target
            else {
                continue;
            };
            let offset = offset.ok_or_else(|| failure("nonliteral main TLS offset"))?;
            if import.module != "GOT.mem"
                || fixup.kind != crate::FixupKind::GotMemory
                || ty.content_type != wasmparser::ValType::I32
                || !ty.mutable
                || ty.shared
                || offset >= main.tls_size
                || !matches!(main.metadata.exports().get("__tls_base").map(|e| &e.ty),
                    Some(ExternType::Global(base)) if base.content_type == wasmparser::ValType::I32
                        && base.mutable && !base.shared)
            {
                return Err(failure("invalid main TLS code binding"));
            }
            result.insert(index, offset);
        }
        Ok(result)
    }

    pub fn target_address(&self, target: &BindingTarget, tls: &[u32]) -> Result<u32> {
        match target {
            BindingTarget::Function { module, index, .. } => {
                let (module, index) = self.function_definition(*module, *index)?;
                self.libraries[module]
                    .slots
                    .get(&index)
                    .copied()
                    .ok_or_else(|| failure("unassigned function slot"))
            }
            BindingTarget::Data {
                module: 0,
                tls: true,
                absolute: false,
                ..
            } => Err(failure(
                "side import of main TLS requires post-libc binding",
            )),
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
    #[cfg(test)]
    pub fn symbol(&self, handle: usize, name: &str, tls: &[u32]) -> Result<u32> {
        // musl do_dlsym(head): dlopen(NULL)'s explicit main handle searches
        // global scope, including preloads, without enabling DEFAULT/NEXT.
        let mut scope = if handle == 0 {
            self.global.clone()
        } else {
            Vec::new()
        };
        for index in self.scope(handle) {
            if !scope.contains(&index) {
                scope.push(index);
            }
        }
        self.symbol_in_scope(&scope, name, tls)
    }
    pub fn symbol_in_scope(&self, scope: &[usize], name: &str, tls: &[u32]) -> Result<u32> {
        for &index in scope {
            if let Some(export) = self.libraries[index].metadata.exports().get(name) {
                if export
                    .flags
                    .intersects(SymbolFlags::VISIBILITY_HIDDEN | SymbolFlags::BINDING_LOCAL)
                {
                    continue;
                }
                return match export.ty {
                    ExternType::Function(_) => self.target_address(
                        &BindingTarget::Function {
                            module: index,
                            index: export.index,
                            slot: None,
                        },
                        tls,
                    ),
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
/// Deferred calls are not function identities: reject address-taking and every
/// element form, including inactive segments omitted by the initialization effects.
fn deferred_identity(lib: &Library, ordinal: usize) -> Result<bool> {
    let function = lib
        .metadata
        .imports()
        .iter()
        .take(ordinal)
        .filter(|import| matches!(import.ty, ExternType::Function(_)))
        .count() as u32;
    if lib.slots.contains_key(&function)
        || lib
            .metadata
            .exports()
            .values()
            .any(|export| export.index == function && matches!(export.ty, ExternType::Function(_)))
    {
        return Ok(true);
    }
    let references = |reader: wasmparser::OperatorsReader<'_>| -> Result<bool> {
        for op in reader {
            if matches!(op.map_err(failure)?, Operator::RefFunc { function_index } if function_index == function)
            {
                return Ok(true);
            }
        }
        Ok(false)
    };
    for payload in Parser::new(0).parse_all(&lib.source) {
        match payload.map_err(failure)? {
            Payload::ElementSection(section) => {
                for element in section {
                    match element.map_err(failure)?.items {
                        wasmparser::ElementItems::Functions(items) => {
                            for index in items {
                                if index.map_err(failure)? == function {
                                    return Ok(true);
                                }
                            }
                        }
                        wasmparser::ElementItems::Expressions(_, items) => {
                            for expr in items {
                                if references(expr.map_err(failure)?.get_operators_reader())? {
                                    return Ok(true);
                                }
                            }
                        }
                    }
                }
            }
            Payload::GlobalSection(section) => {
                for global in section {
                    if references(global.map_err(failure)?.init_expr.get_operators_reader())? {
                        return Ok(true);
                    }
                }
            }
            Payload::TableSection(section) => {
                for table in section {
                    if let wasmparser::TableInit::Expr(expr) = table.map_err(failure)?.init
                        && references(expr.get_operators_reader())?
                    {
                        return Ok(true);
                    }
                }
            }
            Payload::CodeSectionEntry(body)
                if references(body.get_operators_reader().map_err(failure)?)? =>
            {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
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

    fn lazy_library(wat: &str) -> Library {
        Library::new(crate::tests::bytes(wat, [0; 4], &[], &[], &[]), None).unwrap()
    }
    fn missing_direct() -> Graph {
        Graph {
            host_imports: None,
            libraries: vec![
                lazy_library("(module)"),
                lazy_library(r#"(module (import "env" "f" (func (param i32) (result i32))))"#),
            ],
            global: vec![0],
            cursor: 3,
            initialization: vec![0],
        }
    }
    fn append_library(graph: &mut Graph, wat: &str) -> usize {
        let index = graph.libraries.len();
        graph.libraries.push(lazy_library(wat));
        graph.assign_slots(index).unwrap();
        index
    }
    #[test]
    fn unpublished_dependency_order_preserves_old_handles_and_terminates_cycles() {
        let mut graph = missing_direct();
        graph.libraries.push(lazy_library("(module)"));
        graph.libraries.push(lazy_library("(module)"));
        graph.libraries[0].table = 99;
        graph.libraries[1].request = b"root".to_vec();
        graph.libraries[2].request = b"dependency".to_vec();
        graph.libraries[3].request = b"leaf".to_vec();
        graph.libraries[1].dependencies = vec![2, 3];
        graph.libraries[2].dependencies = vec![1];
        graph.global = vec![0, 1];
        graph.initialization = vec![0];
        assert_eq!(graph.dependencies_first(1), vec![0, 3, 1, 2]);
        assert_eq!(graph.libraries[0].table, 99);
        assert_eq!(graph.libraries[1].request, b"dependency");
        assert_eq!(graph.libraries[2].request, b"leaf");
        assert_eq!(graph.libraries[3].request, b"root");
        assert_eq!(graph.libraries[3].dependencies, vec![1, 2]);
        assert_eq!(graph.libraries[1].dependencies, vec![3]);
        assert_eq!(graph.global, vec![0, 3]);
        assert_eq!(graph.initialization, vec![0]);
    }

    #[test]
    fn lazy_defers_only_missing_strong_direct_edges_with_stable_import_ordinal_slots() {
        let mut graph = missing_direct();
        graph.libraries[1] = lazy_library(
            r#"(module
            (import "env" "__memory_base" (global i32))
            (import "wasixv2" "host" (func))
            (import "env" "f" (func (param i32) (result i32))))"#,
        );
        assert!(
            graph
                .clone()
                .seal(1, 1)
                .unwrap_err()
                .to_string()
                .contains("MissingSymbol")
        );
        graph.seal_lazy(1, 1).unwrap();
        let edge = graph.libraries[1].lazy[&2].clone();
        assert_eq!(
            edge,
            LazyFunction {
                slot: 3,
                root: 1,
                target: None
            }
        );
        assert_eq!(graph.libraries[1].fixups[2].target, BindingTarget::Deferred);
        assert_eq!(graph.cursor, 4);
        assert!(graph.function_definition(1, 1).is_err());
        graph.seal(1, 2).unwrap();
        assert_eq!(graph.libraries[1].lazy[&2], edge);
        assert_eq!(graph.cursor, 4, "retry must not allocate another slot");
        for cursor in [0, u32::MAX] {
            let mut invalid = missing_direct();
            invalid.cursor = cursor;
            assert!(invalid.seal_lazy(1, 1).is_err());
        }
    }
    #[test]
    fn deferred_dependency_keeps_the_requested_lookup_group() {
        let root = Library::new(
            crate::tests::bytes("(module)", [0; 4], &["dep"], &[], &[]),
            None,
        )
        .unwrap();
        let mut graph = missing_direct();
        graph.libraries.push(graph.libraries[1].clone());
        graph.libraries[1] = root;
        graph.libraries[1].dependencies = vec![2];
        graph.seal_lazy(1, 1).unwrap();
        assert_eq!(graph.libraries[2].lazy[&0].root, 1);
        assert_eq!(graph.libraries[2].lazy[&0].target, None);
        let unrelated = append_library(
            &mut graph,
            r#"(module (func (export "f") (param i32) (result i32) local.get 0))"#,
        );
        graph.seal(unrelated, unrelated).unwrap();
        assert_eq!(graph.libraries[2].lazy[&0].target, None);
    }

    #[test]
    fn unrelated_local_definition_does_not_resolve_until_global_promotion_then_freezes() {
        let mut graph = missing_direct();
        graph.seal_lazy(1, 1).unwrap();
        let slot = graph.libraries[1].lazy[&0].slot;
        let provider = append_library(
            &mut graph,
            r#"(module (func (export "f") (param i32) (result i32) local.get 0))"#,
        );
        graph.seal(provider, provider).unwrap();
        assert_eq!(graph.libraries[1].lazy[&0].target, None);
        assert_eq!(graph.libraries[1].fixups[0].target, BindingTarget::Deferred);
        graph.global.push(provider);
        graph.seal(provider, graph.libraries.len()).unwrap();
        assert_eq!(
            graph.libraries[1].lazy[&0],
            LazyFunction {
                slot,
                root: 1,
                target: Some((provider, 0))
            }
        );
        assert!(matches!(graph.libraries[1].fixups[0].target,
            BindingTarget::Function { module, index: 0, slot: None } if module == provider));
        let frozen = graph.libraries[1].fixups.clone();
        let incompatible = append_library(&mut graph, "(module (func (export \"f\")))");
        // Deliberately change current lookup priority: even a now-incompatible first
        // definition must not cause a resolved edge to be looked up or typechecked again.
        graph.global.insert(1, incompatible);
        graph.seal_lazy(incompatible, incompatible).unwrap();
        assert_eq!(graph.libraries[1].fixups, frozen);
        assert_eq!(graph.libraries[1].lazy[&0].slot, slot);
        assert_eq!(graph.libraries[1].lazy[&0].target, Some((provider, 0)));
    }
    #[test]
    fn lazy_retry_typechecks_first_visible_definition_without_mutating_old_binding() {
        let mut graph = missing_direct();
        graph.seal_lazy(1, 1).unwrap();
        let original = graph.libraries[1].lazy.clone();
        let wrong = append_library(&mut graph, "(module (func (export \"f\")))");
        graph.global.push(wrong);
        assert!(
            graph
                .seal(wrong, wrong)
                .unwrap_err()
                .to_string()
                .contains("TypeMismatch")
        );
        assert_eq!(graph.libraries[1].lazy, original);
        assert_eq!(graph.libraries[1].fixups[0].target, BindingTarget::Deferred);
    }
    #[test]
    fn old_now_imports_are_not_revalidated_when_lazy_edges_are_retried() {
        let mut graph = missing_direct();
        graph.libraries[1] = lazy_library(
            r#"(module
            (import "env" "f" (func (param i32) (result i32)))
            (func (export "f") (param i32) (result i32) local.get 0))"#,
        );
        graph.assign_slots(1).unwrap();
        graph.seal(1, 1).unwrap();
        let frozen = graph.libraries[1].fixups.clone();
        let wrong = append_library(&mut graph, "(module (func (export \"f\")))");
        graph.seal(wrong, wrong).unwrap();
        graph.global.push(wrong);
        assert!(
            graph.plan(1, 0).is_err(),
            "fresh planning would see an incompatible global"
        );
        graph.seal_lazy(1, graph.libraries.len()).unwrap();
        assert_eq!(graph.libraries[1].fixups, frozen);
        assert!(graph.libraries[1].lazy.is_empty());
    }
    #[test]
    fn lazy_rejects_missing_got_but_keeps_existing_weak_zero_policy() {
        for namespace in ["GOT.func", "GOT.mem"] {
            let mut graph = missing_direct();
            graph.libraries[1] = lazy_library(&format!(
                "(module (import \"{namespace}\" \"missing\" (global (mut i32))))"
            ));
            assert!(
                graph
                    .seal_lazy(1, 1)
                    .unwrap_err()
                    .to_string()
                    .contains("MissingSymbol")
            );
        }
        let mut graph = missing_direct();
        graph.libraries[1] = Library::new(
            crate::tests::bytes(
                r#"(module (import "env" "f" (func (param i32) (result i32))))"#,
                [0; 4],
                &[],
                &[],
                &[(
                    "env",
                    "f",
                    SymbolFlags::BINDING_WEAK | SymbolFlags::UNDEFINED,
                )],
            ),
            None,
        )
        .unwrap();
        graph.seal_lazy(1, 1).unwrap();
        assert_eq!(graph.libraries[1].fixups[0].target, BindingTarget::WeakZero);
        assert!(graph.libraries[1].lazy.is_empty());
    }
    #[test]
    fn deferred_imports_cannot_supply_export_element_or_ref_func_identities() {
        for use_ in [
            r#"(export "alias" (func $f))"#,
            r#"(table 4 funcref) (elem (i32.const 3) $f)"#,
            r#"(elem func $f)"#,
            r#"(elem declare func $f)"#,
            r#"(elem funcref (ref.func $f))"#,
            r#"(elem declare funcref (ref.func $f))"#,
            r#"(global funcref (ref.func $f)) (func ref.func $f drop)"#,
        ] {
            let mut graph = missing_direct();
            graph.libraries[1] = lazy_library(&format!(
                r#"(module (import "env" "missing" (func $f)) {use_})"#
            ));
            assert!(
                graph
                    .seal_lazy(1, 1)
                    .unwrap_err()
                    .to_string()
                    .contains("concrete exported or referenced identity"),
                "{use_}"
            );
        }
        let mut alias = missing_direct();
        alias.libraries[1] =
            lazy_library(r#"(module (import "env" "f" (func $f)) (export "f" (func $f)))"#);
        assert!(
            alias
                .seal_lazy(1, 1)
                .unwrap_err()
                .to_string()
                .contains("alias cycle")
        );
    }

    #[test]
    fn main_tls_lookup_uses_store_base_but_early_relocations_stay_rejected() {
        let main = Library::new(
            crate::tests::bytes(
                r#"(module (global (export "value") i32 (i32.const 8)))"#,
                [0; 4],
                &[],
                &[("value", SymbolFlags::TLS)],
                &[],
            ),
            None,
        )
        .unwrap();
        let graph = Graph {
            host_imports: None,
            libraries: vec![main],
            global: vec![0],
            cursor: 3,
            initialization: vec![0],
        };
        assert_eq!(graph.symbol(0, "value", &[2048]).unwrap(), 2056);
        assert_eq!(graph.symbol(0, "value", &[4096]).unwrap(), 4104);
        for bases in [&[][..], &[0][..], &[u32::MAX - 7][..]] {
            assert!(graph.symbol(0, "value", bases).is_err());
        }
        let target = BindingTarget::Data {
            module: 0,
            index: 0,
            offset: Some(8),
            tls: true,
            absolute: false,
        };
        assert!(
            graph
                .target_address(&target, &[2048])
                .unwrap_err()
                .to_string()
                .contains("post-libc binding")
        );
    }

    #[test]
    fn function_aliases_require_a_concrete_typed_definition_and_canonical_slot() {
        let library = |wat: &str| {
            Library::new(crate::tests::bytes(wat, [0; 4], &[], &[], &[]), None).unwrap()
        };
        let mut graph = Graph {
            host_imports: None,
            libraries: vec![
                library("(module)"),
                library(
                    r#"(module (import "env" "f" (func $f (param i32) (result i32))) (export "f" (func $f)))"#,
                ),
            ],
            global: vec![0],
            cursor: 3,
            initialization: vec![0],
        };
        graph.libraries[1].dependencies = vec![];
        // A self-re-export is not an actual NOW definition.
        assert!(
            graph
                .seal(1, 1)
                .unwrap_err()
                .to_string()
                .contains("alias cycle")
        );
        graph.libraries.push(library(
            "(module (func (export \"f\") (param i32) (result i32) local.get 0))",
        ));
        graph.libraries[1].fixups[0].target = BindingTarget::Function {
            module: 2,
            index: 0,
            slot: None,
        };
        graph.assign_slots(2).unwrap();
        let slot = graph.libraries[2].slots[&0];
        assert_eq!(graph.function_definition(1, 0).unwrap(), (2, 0));
        assert_eq!(graph.symbol(1, "f", &[0; 3]).unwrap(), slot);
        graph.libraries[2].metadata.functions[0] = wasmparser::FuncType::new([], []);
        assert!(
            graph
                .function_definition(1, 0)
                .unwrap_err()
                .to_string()
                .contains("signature mismatch")
        );
    }

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
            host_imports: None,
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
            host_imports: None,
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
