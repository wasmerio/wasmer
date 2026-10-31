//! One store's staged side module. No guest initializer is executed.
use crate::{InitEffects, Initialization};
use std::{collections::BTreeMap, ops::Range};
use wasmer::{
    AsStoreMut, AsStoreRef, Function, Global, Imports, Instance, Memory, Module, RuntimeError,
    Table, Value,
};

fn error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(error.to_string())
}

/// Effects remain private until every store has prepared. The coordinator must explicitly
/// rollback an applied staging object on its owning store or consume it at publication.
/// Wasm capacity growth during prepare cannot be undone, but exposes no module or handle.
pub struct StagedModule {
    instance: Instance,
    memory: Memory,
    table: Table,
    effects: InitEffects,
    globals: Vec<(Global, Value)>,
    slots: BTreeMap<u32, Function>,
    applied_slots: Vec<u32>,
    applied_writes: usize,
    applied: bool,
    complete: bool,
}
impl StagedModule {
    /// Compile and instantiate the validated no-auto-start form. The resolved imports must
    /// refer to this store's memory/table/globals/functions; the engine checks their types.
    /// `tls` selects a separate peer TLS block. `None` initializes shared data exactly once.
    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        store: &mut impl AsStoreMut,
        source: &[u8],
        imports: &Imports,
        memory: Memory,
        table: Table,
        fresh_memory: &[Range<u64>],
        fresh_table: &[Range<u64>],
        tls: Option<u32>,
    ) -> Result<Self, RuntimeError> {
        let initialization = Initialization::parse(source).map_err(error)?;
        for payload in wasmparser::Parser::new(0).parse_all(source) {
            if let wasmparser::Payload::TableSection(section) = payload.map_err(error)?
                && section.count() > 0
            {
                return Err(error("side module defines a private function table"));
            }
        }
        let bytes = initialization.instantiate_bytes().map_err(error)?;
        // Side-module loops must reach the same return-to-guest boundary as the main.
        // Never inherit load()'s unpreemptible fallback for a live all-store transaction.
        let bytes = wasixv2_exec::instrument(&bytes).map_err(error)?;
        let module = Module::new(&store.as_store_ref(), bytes).map_err(error)?;
        let instance = Instance::new(store, &module, imports).map_err(error)?;
        if instance
            .exports
            .get_memory("__wasix_dylink_memory_0")
            .map_err(error)?
            != &memory
        {
            return Err(error(
                "side module memory differs from address-space memory",
            ));
        }
        let mut tables = 0;
        for import in module.imports() {
            if matches!(import.ty(), wasmer::ExternType::Table(_)) {
                tables += 1;
                if imports.get_export(import.module(), import.name())
                    != Some(wasmer::Extern::Table(table.clone()))
                {
                    return Err(error("side module imports a different function table"));
                }
            }
        }
        if tables > 1 {
            return Err(error("side module imports multiple function tables"));
        }
        let mut globals = BTreeMap::new();
        for (name, export) in instance.exports.iter() {
            if let Some(index) = name.strip_prefix("__wasix_dylink_global_") {
                let index = index.parse::<u32>().map_err(error)?;
                let wasmer::Extern::Global(global) = export else {
                    return Err(error("invalid private global export"));
                };
                let value = global.get(&mut *store);
                let scalar = match value {
                    Value::I32(value) => u64::from(value as u32),
                    Value::I64(value) => value as u64,
                    _ => return Err(error("non-scalar dynamic initialization global")),
                };
                globals.insert(index, scalar);
            }
        }
        let effects = initialization
            .effects(globals.clone(), fresh_memory, tls)
            .map_err(error)?;
        for write in &effects.writes {
            let end = u64::from(write.address)
                .checked_add(write.bytes.len() as u64)
                .ok_or_else(|| error("initialization address overflow"))?;
            if end > memory.view(&*store).data_size() {
                return Err(error("initialization exceeds memory capacity"));
            }
        }
        let end = fresh_table.iter().map(|range| range.end).max().unwrap_or(0);
        let end = u32::try_from(end).map_err(error)?;
        let size = table.size(&*store);
        if end > size {
            table.grow(store, end - size, Value::FuncRef(None))?;
        }
        let mut changes = Vec::new();
        for (index, value) in &effects.globals {
            let global = instance
                .exports
                .get_global(&format!("__wasix_dylink_global_{index}"))
                .map_err(error)?
                .clone();
            let value = match global.get(&mut *store) {
                Value::I32(_) => Value::I32(*value as i32),
                Value::I64(_) => Value::I64(*value as i64),
                _ => return Err(error("invalid initialization global type")),
            };
            if global.ty(&*store).mutability != wasmer::Mutability::Var {
                return Err(error("initialization changes an immutable global"));
            }
            changes.push((global, value));
        }
        let mut staged = Self {
            instance,
            memory,
            table,
            effects,
            globals: changes,
            slots: BTreeMap::new(),
            applied_slots: Vec::new(),
            applied_writes: 0,
            applied: false,
            complete: false,
        };
        for placement in initialization
            .table_effects(&globals, fresh_table)
            .map_err(error)?
        {
            for (offset, function) in placement.functions.iter().enumerate() {
                if let Some(function) = function {
                    let slot = placement
                        .index
                        .checked_add(offset as u32)
                        .ok_or_else(|| error("element slot overflow"))?;
                    staged.bind_slot(store, slot, *function, fresh_table)?;
                }
            }
        }
        Ok(staged)
    }
    pub fn instance(&self) -> &Instance {
        &self.instance
    }
    pub fn function(&self, index: u32) -> Result<Function, RuntimeError> {
        self.instance
            .exports
            .get_function(&format!("__wasix_dylink_fn_{index}"))
            .cloned()
            .map_err(error)
    }
    /// Reconcile a planner-proposed GOT/function slot with actual engine function identity,
    /// including imported/re-exported aliases and active element placement. Existing image
    /// entries cannot be replaced, even when the requested function happens to be equal.
    pub fn bind_slot(
        &mut self,
        store: &mut impl AsStoreMut,
        slot: u32,
        function: u32,
        fresh: &[Range<u64>],
    ) -> Result<(), RuntimeError> {
        if self.applied {
            return Err(error("cannot change an applied slot plan"));
        }
        if slot < 3 || !fresh.iter().any(|range| range.contains(&u64::from(slot))) {
            return Err(error("function slot is outside fresh reservation"));
        }
        let function = self.function(function)?;
        if let Some(existing) = self.slots.get(&slot) {
            if existing != &function {
                return Err(error("conflicting element/function alias"));
            }
            return Ok(());
        }
        if !matches!(self.table.get(store, slot), Some(Value::FuncRef(None))) {
            return Err(error("function slot is occupied or out of bounds"));
        }
        self.applied_slots
            .try_reserve(self.slots.len() + 1)
            .map_err(error)?;
        self.slots.insert(slot, function);
        Ok(())
    }
    /// Host writes only, with no awaits/imports/guest calls. All effects were range-checked
    /// and all table capacity/types resolved during prepare while peers were parked.
    pub fn apply(&mut self, store: &mut impl AsStoreMut) -> Result<(), RuntimeError> {
        if self.applied {
            return Err(error("staged module already applied"));
        }
        self.applied = true;
        for write in &self.effects.writes {
            self.memory
                .view(&*store)
                .write(u64::from(write.address), &write.bytes)
                .map_err(error)?;
            self.applied_writes += 1;
        }
        for (global, value) in &self.globals {
            global.set(store, value.clone())?;
        }
        for (index, function) in &self.slots {
            self.table
                .set(store, *index, Value::FuncRef(Some(function.clone())))?;
            self.applied_slots.push(*index);
        }
        self.complete = true;
        Ok(())
    }
    /// Undo only this staging object's new slots/new bytes. Old image instances and GOTs
    /// were never modified. The mm reservation owner subsequently releases these ranges.
    pub fn rollback(&mut self, store: &mut impl AsStoreMut) -> Result<(), RuntimeError> {
        while let Some(&index) = self.applied_slots.last() {
            self.table.set(store, index, Value::FuncRef(None))?;
            self.applied_slots.pop();
        }
        for write in &self.effects.writes[..self.applied_writes] {
            let zero = [0; 4096];
            let mut offset = 0;
            while offset < write.bytes.len() {
                let count = (write.bytes.len() - offset).min(zero.len());
                self.memory
                    .view(&*store)
                    .write(u64::from(write.address) + offset as u64, &zero[..count])
                    .map_err(error)?;
                offset += count;
            }
        }
        self.applied_writes = 0;
        self.applied = false;
        self.complete = false;
        Ok(())
    }
    /// Called only after the mm-wide coordinator has completed publication in every store.
    pub fn publish(self) -> Result<Instance, RuntimeError> {
        if !self.complete {
            return Err(error("staged module was not applied"));
        }
        Ok(self.instance)
    }
}
