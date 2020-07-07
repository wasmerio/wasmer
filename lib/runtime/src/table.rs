// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer-reborn/blob/master/ATTRIBUTIONS.md

//! Memory management for tables.
//!
//! `Table` is to WebAssembly tables what `LinearMemory` is to WebAssembly linear memories.

use crate::trap::{Trap, TrapCode};
use crate::vmcontext::{VMCallerCheckedAnyfunc, VMTableDefinition};
use serde::{Deserialize, Serialize};
use std::borrow::{Borrow, BorrowMut};
use std::cell::UnsafeCell;
use std::convert::TryFrom;
use std::fmt;
use std::ptr::NonNull;
use std::sync::Mutex;
use wasm_common::{FunctionIndex, GlobalIndex, TableIndex, TableType, Type as ValType};

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
pub trait Table: fmt::Debug + Send + Sync {
    /// Returns the table plan for this Table.
    fn plan(&self) -> &TablePlan;

    /// Returns the style for this Table.
    fn style(&self) -> &TableStyle;

    /// Returns the type for this Table.
    fn ty(&self) -> &TableType;

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
    fn vmtable(&self) -> NonNull<VMTableDefinition>;

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

/// A table instance.
#[derive(Debug)]
pub struct LinearTable {
    // TODO: we can remove the mutex by using atomic swaps and preallocating the max table size
    vec: Mutex<Vec<VMCallerCheckedAnyfunc>>,
    maximum: Option<u32>,
    plan: TablePlan,
    vm_table_definition: Box<UnsafeCell<VMTableDefinition>>,
}

/// This is correct because there is no thread-specific data tied to this type.
unsafe impl Send for LinearTable {}
/// This is correct because all internal mutability is protected by a mutex.
unsafe impl Sync for LinearTable {}

impl LinearTable {
    /// Create a new table instance with specified minimum and maximum number of elements.
    pub fn new(table: &TableType, style: &TableStyle) -> Result<Self, String> {
        match table.ty {
            ValType::FuncRef => (),
            ty => return Err(format!("tables of types other than anyfunc ({})", ty)),
        };
        if let Some(max) = table.maximum {
            if max < table.minimum {
                return Err(format!(
                    "Table minimum ({}) is larger than maximum ({})!",
                    table.minimum, max
                ));
            }
        }
        let table_minimum = usize::try_from(table.minimum)
            .map_err(|_| "Table minimum is bigger than usize".to_string())?;
        let mut vec = vec![VMCallerCheckedAnyfunc::default(); table_minimum];
        let base = vec.as_mut_ptr();
        match style {
            TableStyle::CallerChecksSignature => Ok(Self {
                vec: Mutex::new(vec),
                maximum: table.maximum,
                plan: TablePlan {
                    table: table.clone(),
                    style: style.clone(),
                },
                vm_table_definition: Box::new(UnsafeCell::new(VMTableDefinition {
                    base: base as _,
                    current_elements: table_minimum as _,
                })),
            }),
        }
    }
}

impl Table for LinearTable {
    /// Returns the table plan for this Table.
    fn plan(&self) -> &TablePlan {
        &self.plan
    }

    /// Returns the style for this Table.
    fn style(&self) -> &TableStyle {
        &self.plan.style
    }

    /// Returns the style for this Table.
    fn ty(&self) -> &TableType {
        &self.plan.table
    }

    /// Returns the number of allocated elements.
    fn size(&self) -> u32 {
        unsafe {
            let ptr = self.vm_table_definition.get();
            (*ptr).current_elements
        }
    }

    /// Grow table by the specified amount of elements.
    ///
    /// Returns `None` if table can't be grown by the specified amount
    /// of elements, otherwise returns the previous size of the table.
    fn grow(&self, delta: u32) -> Option<u32> {
        let mut vec_guard = self.vec.lock().unwrap();
        let vec = vec_guard.borrow_mut();
        let size = self.size();
        let new_len = size.checked_add(delta)?;
        if self.maximum.map_or(false, |max| new_len > max) {
            return None;
        }
        vec.resize(
            usize::try_from(new_len).unwrap(),
            VMCallerCheckedAnyfunc::default(),
        );
        // update table definition
        unsafe {
            let td = &mut *self.vm_table_definition.get();
            td.current_elements = new_len;
            td.base = vec.as_mut_ptr() as _;
        }
        Some(size)
    }

    /// Get reference to the specified element.
    ///
    /// Returns `None` if the index is out of bounds.
    fn get(&self, index: u32) -> Option<VMCallerCheckedAnyfunc> {
        let vec_guard = self.vec.lock().unwrap();
        vec_guard.borrow().get(index as usize).cloned()
    }

    /// Set reference to the specified element.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds.
    fn set(&self, index: u32, func: VMCallerCheckedAnyfunc) -> Result<(), Trap> {
        let mut vec_guard = self.vec.lock().unwrap();
        let vec = vec_guard.borrow_mut();
        match vec.get_mut(index as usize) {
            Some(slot) => {
                *slot = func;
                Ok(())
            }
            None => Err(Trap::wasm(TrapCode::TableAccessOutOfBounds)),
        }
    }

    /// Return a `VMTableDefinition` for exposing the table to compiled wasm code.
    fn vmtable(&self) -> NonNull<VMTableDefinition> {
        let _vec_guard = self.vec.lock().unwrap();
        let ptr = self.vm_table_definition.as_ref() as *const UnsafeCell<VMTableDefinition>
            as *const VMTableDefinition as *mut VMTableDefinition;
        unsafe { NonNull::new_unchecked(ptr) }
    }
}
