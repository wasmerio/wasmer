//! Memory management for tables.
//!
//! `Table` is to WebAssembly tables what `LinearMemory` is to WebAssembly linear memories.

use crate::trap::{Trap, TrapCode};
use crate::vmcontext::{VMCallerCheckedAnyfunc, VMTableDefinition};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::convert::{TryFrom, TryInto};
use wasm_common::{FunctionIndex, GlobalIndex, TableIndex, TableType, Type};

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

/// A table instance.
#[derive(Debug)]
pub struct Table {
    vec: RefCell<Vec<VMCallerCheckedAnyfunc>>,
    maximum: Option<u32>,
    plan: TablePlan,
}

impl Table {
    /// Create a new table instance with specified minimum and maximum number of elements.
    pub fn new(plan: &TablePlan) -> Result<Self, String> {
        match plan.table.ty {
            Type::FuncRef => (),
            ty => return Err(format!("tables of types other than anyfunc ({})", ty)),
        };
        if let Some(max) = plan.table.maximum {
            if max < plan.table.minimum {
                return Err(format!(
                    "Table minimum ({}) is larger than maximum ({})!",
                    plan.table.minimum, max
                ));
            }
        }
        match plan.style {
            TableStyle::CallerChecksSignature => Ok(Self {
                vec: RefCell::new(vec![
                    VMCallerCheckedAnyfunc::default();
                    usize::try_from(plan.table.minimum).map_err(|_| {
                        "Table minimum is bigger than usize".to_string()
                    })?
                ]),
                maximum: plan.table.maximum,
                plan: plan.clone(),
            }),
        }
    }

    /// Returns the table plan for this Table.
    pub fn plan(&self) -> &TablePlan {
        &self.plan
    }

    /// Returns the number of allocated elements.
    pub fn size(&self) -> u32 {
        self.vec.borrow().len().try_into().unwrap()
    }

    /// Grow table by the specified amount of elements.
    ///
    /// Returns `None` if table can't be grown by the specified amount
    /// of elements, otherwise returns the previous size of the table.
    pub fn grow(&self, delta: u32) -> Option<u32> {
        let size = self.size();
        let new_len = size.checked_add(delta)?;
        if self.maximum.map_or(false, |max| new_len > max) {
            return None;
        }
        self.vec.borrow_mut().resize(
            usize::try_from(new_len).unwrap(),
            VMCallerCheckedAnyfunc::default(),
        );
        Some(size)
    }

    /// Get reference to the specified element.
    ///
    /// Returns `None` if the index is out of bounds.
    pub fn get(&self, index: u32) -> Option<VMCallerCheckedAnyfunc> {
        self.vec.borrow().get(index as usize).cloned()
    }

    /// Set reference to the specified element.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds.
    pub fn set(&self, index: u32, func: VMCallerCheckedAnyfunc) -> Result<(), Trap> {
        match self.vec.borrow_mut().get_mut(index as usize) {
            Some(slot) => {
                *slot = func;
                Ok(())
            }
            None => Err(Trap::wasm(TrapCode::TableAccessOutOfBounds)),
        }
    }

    /// Copy `len` elements from `src_table[src_index..]` into `dst_table[dst_index..]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    pub fn copy(
        dst_table: &Self,
        src_table: &Self,
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

        if dst_index
            .checked_add(len)
            .map_or(true, |m| m > dst_table.size())
        {
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
                dst_table.set(d, src_table.get(s).unwrap())?;
            }
        } else {
            for (s, d) in srcs.rev().zip(dsts.rev()) {
                dst_table.set(d, src_table.get(s).unwrap())?;
            }
        }

        Ok(())
    }

    /// Return a `VMTableDefinition` for exposing the table to compiled wasm code.
    pub fn vmtable(&self) -> VMTableDefinition {
        let mut vec = self.vec.borrow_mut();
        VMTableDefinition {
            base: vec.as_mut_ptr() as *mut u8,
            current_elements: vec.len().try_into().unwrap(),
        }
    }

    /// Get the table host as mutable pointer
    ///
    /// This function is used in the `wasmer_runtime::Instance` to retrieve
    /// the host table pointer and interact with the host table directly.
    pub fn as_mut_ptr(&self) -> *mut Self {
        self as *const Self as *mut Self
    }
}
