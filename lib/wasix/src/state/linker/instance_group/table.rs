use tracing::trace;
use wasmer::{AsStoreMut, Function, RuntimeError, Value};

use super::{InstanceGroupState, LinkError, LinkerState, ModuleHandle};

impl InstanceGroupState {
    /// Allocate space on the indirect function table for the given number of functions.
    ///
    /// table_alignment is the alignment of the table as a power of two.
    pub(in crate::state::linker) fn allocate_function_table(
        &mut self,
        store: &mut impl AsStoreMut,
        table_size: u32,
        table_alignment: u32,
    ) -> Result<u64, RuntimeError> {
        trace!(table_size, "Allocating table indices");

        let base_index = if table_size == 0 {
            0
        } else {
            let current_size = self.indirect_function_table.size(store);
            let alignment = 2_u32.pow(table_alignment);

            let offset = if !current_size.is_multiple_of(alignment) {
                alignment - (current_size % alignment)
            } else {
                0
            };

            let delta = table_size + offset;
            trace!(?current_size, ?delta, "Growing indirect function table");
            let start = self
                .indirect_function_table
                .grow(store, delta, Value::FuncRef(None))?;

            (start + offset) as u64
        };

        trace!(
            base_index,
            new_table_size = ?self.indirect_function_table.size(store),
            "Allocated table indices"
        );

        Ok(base_index)
    }

    pub(in crate::state::linker) fn append_to_function_table(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
    ) -> Result<u32, RuntimeError> {
        let table = &self.indirect_function_table;

        let ty = func.ty(store).to_string();
        let index: u32 = table.size(store);
        trace!(?index, ?ty, "Appending function in table");

        table.grow(store, 1, func.into())
    }

    pub(super) fn place_in_function_table_at(
        &self,
        store: &mut impl AsStoreMut,
        func: Function,
        index: u32,
    ) -> Result<(), RuntimeError> {
        trace!(
            ?index,
            ?func,
            "Placing function in table at pre-defined index"
        );

        let table = &self.indirect_function_table;
        let size = table.size(store);

        if size <= index {
            let delta = index - size + 1;
            trace!(
                current_size = ?size,
                ?delta,
                "Growing indirect function table"
            );
            table.grow(store, delta, Value::FuncRef(None))?;
        } else if !table.is_v8() {
            // TODO: With V8 we might hit a table entry that is an internal placeholder
            // used for lazy initialization, investigate more.
            let existing = table.get(store, index).unwrap();
            if let Value::FuncRef(Some(_)) = existing {
                panic!("Internal error: function table index {index} already occupied");
            }
        }

        let ty = func.ty(store).to_string();
        trace!(?index, ?ty, "Placing function in table at index");
        table.set(store, index, Value::FuncRef(Some(func)))
    }

    pub(super) fn allocate_function_table_for_existing_module(
        &mut self,
        linker_state: &LinkerState,
        store: &mut impl AsStoreMut,
        module_handle: ModuleHandle,
    ) -> Result<(), LinkError> {
        if self.side_instances.contains_key(&module_handle) {
            panic!(
                "Internal error: Module with handle {module_handle:?} \
                was already instantiated in this group"
            )
        };

        let dl_module = linker_state
            .side_modules
            .get(&module_handle)
            .expect("Internal error: module not loaded into linker");

        let table_base = self
            .allocate_function_table(
                store,
                dl_module.dylink_info.mem_info.table_size,
                dl_module.dylink_info.mem_info.table_alignment,
            )
            .map_err(LinkError::TableAllocationError)?;

        if table_base != dl_module.table_base {
            panic!("Internal error: table base out of sync with linker state");
        }

        trace!(table_base, "Allocated table indices for existing module");

        Ok(())
    }

    pub(super) fn apply_resolved_function(
        &self,
        store: &mut impl AsStoreMut,
        name: &str,
        resolved_from: ModuleHandle,
        function_table_index: u32,
    ) -> Result<(), LinkError> {
        trace!(
            ?name,
            ?resolved_from,
            function_table_index,
            "Applying resolved function"
        );

        let instance = &self.try_instance(resolved_from).unwrap_or_else(|| {
            panic!("Internal error: module {resolved_from:?} not loaded by this group")
        });

        let func = instance.exports.get_function(name).unwrap_or_else(|e| {
            panic!("Internal error: failed to resolve exported function {name}: {e:?}")
        });

        self.place_in_function_table_at(store, func.clone(), function_table_index)
            .map_err(LinkError::TableAllocationError)?;

        Ok(())
    }

    pub(super) fn apply_function_table_allocation(
        &mut self,
        store: &mut impl AsStoreMut,
        index: u32,
        size: u32,
    ) -> Result<(), LinkError> {
        trace!(index, "Applying function table allocation");
        let allocated_index = self
            .allocate_function_table(store, size, 0)
            .map_err(LinkError::TableAllocationError)? as u32;
        if allocated_index != index {
            panic!(
                "Internal error: allocated index {allocated_index} does not match expected index {index}"
            );
        }
        Ok(())
    }
}
