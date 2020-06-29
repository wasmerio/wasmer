use crate::ValType;
use std::borrow::{Borrow, BorrowMut};
use std::cell::UnsafeCell;
use std::convert::TryFrom;
use std::ptr::NonNull;
use std::sync::Mutex;
use wasmer_runtime::{
    Table, TablePlan, TableStyle, Trap, TrapCode, VMCallerCheckedAnyfunc, VMTableDefinition,
};

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
    pub fn new(plan: &TablePlan) -> Result<Self, String> {
        match plan.table.ty {
            ValType::FuncRef => (),
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
        let table_minimum = usize::try_from(plan.table.minimum)
            .map_err(|_| "Table minimum is bigger than usize".to_string())?;
        let mut vec = vec![VMCallerCheckedAnyfunc::default(); table_minimum];
        let base = vec.as_mut_ptr();
        match plan.style {
            TableStyle::CallerChecksSignature => Ok(Self {
                vec: Mutex::new(vec),
                maximum: plan.table.maximum,
                plan: plan.clone(),
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
