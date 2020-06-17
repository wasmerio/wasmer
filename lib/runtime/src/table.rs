// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer-reborn/blob/master/ATTRIBUTIONS.md

//! Memory management for tables.
//!
//! `Table` is to WebAssembly tables what `LinearMemory` is to WebAssembly linear memories.

use crate::trap::{Trap, TrapCode};
use crate::vmcontext::{VMCallerCheckedAnyfunc, VMTableDefinition};
use serde::{Deserialize, Serialize};
use std::fmt;
use wasm_common::{FunctionIndex, GlobalIndex, TableIndex, TableType};

/// A WebAssembly table initializer.
#[derive(Clone, Debug, Hash, Serialize, Deserialize)]
pub struct TableElements {
    /// The index of a table to initialize.
    pub table_index: TableIndex,
    /// Optionally, a global variable giving a base index.
    pub base: Option<GlobalIndex>,
    /// The offset to add to the base.
    pub offset: usize,
    /// The values to write into the table elements.
    pub elements: Box<[FunctionIndex]>,
}

/// Implementation styles for WebAssembly tables.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub enum TableStyle {
    /// Signatures are stored in the table and checked in the caller.
    CallerChecksSignature,
}

/// A WebAssembly table description along with our chosen style for
/// implementing it.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct TablePlan {
    /// The WebAssembly table description.
    pub table: TableType,
    /// Our chosen implementation style.
    pub style: TableStyle,
}

/// Trait for implementing the interface of a Wasm table.
pub trait Table: fmt::Debug {
    /// Returns the table plan for this Table.
    fn plan(&self) -> &TablePlan;

    /// Returns the number of allocated elements.
    fn size(&self) -> u32;

    /// Grow table by the specified amount of elements.
    ///
    /// Returns `None` if table can't be grown by the specified amount
    /// of elements, otherwise returns the previous size of the table.
    fn grow(&self, delta: u32) -> Option<u32>;

    /// Get reference to the specified element.
    ///
    /// Returns `None` if the index is out of bounds.
    fn get(&self, index: u32) -> Option<VMCallerCheckedAnyfunc>;

    /// Set reference to the specified element.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds.
    fn set(&self, index: u32, func: VMCallerCheckedAnyfunc) -> Result<(), Trap>;

    /// Return a `VMTableDefinition` for exposing the table to compiled wasm code.
    fn vmtable(&self) -> VMTableDefinition;

    /// Copy `len` elements from `src_table[src_index..]` into `dst_table[dst_index..]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    fn copy(
        &self,
        src_table: &dyn Table,
        dst_index: u32,
        src_index: u32,
        len: u32,
    ) -> Result<(), Trap> {
        // https://webassembly.github.io/bulk-memory-operations/core/exec/instructions.html#exec-table-copy

        if src_index
            .checked_add(len)
            .map_or(true, |n| n > src_table.size())
        {
            return Err(Trap::wasm(TrapCode::TableAccessOutOfBounds));
        }

        if dst_index.checked_add(len).map_or(true, |m| m > self.size()) {
            return Err(Trap::wasm(TrapCode::TableSetterOutOfBounds));
        }

        let srcs = src_index..src_index + len;
        let dsts = dst_index..dst_index + len;

        // Note on the unwraps: the bounds check above means that these will
        // never panic.
        //
        // TODO: investigate replacing this get/set loop with a `memcpy`.
        if dst_index <= src_index {
            for (s, d) in (srcs).zip(dsts) {
                self.set(d, src_table.get(s).unwrap())?;
            }
        } else {
            for (s, d) in srcs.rev().zip(dsts.rev()) {
                self.set(d, src_table.get(s).unwrap())?;
            }
        }

        Ok(())
    }
}
