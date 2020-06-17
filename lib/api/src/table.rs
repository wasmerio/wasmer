use crate::ValType;
use std::cell::RefCell;
use std::convert::{TryFrom, TryInto};
use wasmer_runtime::{
    Table, TablePlan, TableStyle, Trap, TrapCode, VMCallerCheckedAnyfunc, VMTableDefinition,
};

/// A table instance.
#[derive(Debug)]
pub struct LinearTable {
    vec: RefCell<Vec<VMCallerCheckedAnyfunc>>,
    maximum: Option<u32>,
    plan: TablePlan,
}

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
}

impl Table for LinearTable {
    /// Returns the table plan for this Table.
    fn plan(&self) -> &TablePlan {
        &self.plan
    }

    /// Returns the number of allocated elements.
    fn size(&self) -> u32 {
        self.vec.borrow().len().try_into().unwrap()
    }

    /// Grow table by the specified amount of elements.
    ///
    /// Returns `None` if table can't be grown by the specified amount
    /// of elements, otherwise returns the previous size of the table.
    fn grow(&self, delta: u32) -> Option<u32> {
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
    fn get(&self, index: u32) -> Option<VMCallerCheckedAnyfunc> {
        self.vec.borrow().get(index as usize).cloned()
    }

    /// Set reference to the specified element.
    ///
    /// # Errors
    ///
    /// Returns an error if the index is out of bounds.
    fn set(&self, index: u32, func: VMCallerCheckedAnyfunc) -> Result<(), Trap> {
        match self.vec.borrow_mut().get_mut(index as usize) {
            Some(slot) => {
                *slot = func;
                Ok(())
            }
            None => Err(Trap::wasm(TrapCode::TableAccessOutOfBounds)),
        }
    }

    /// Return a `VMTableDefinition` for exposing the table to compiled wasm code.
    fn vmtable(&self) -> VMTableDefinition {
        let mut vec = self.vec.borrow_mut();
        VMTableDefinition {
            base: vec.as_mut_ptr() as *mut u8,
            current_elements: vec.len().try_into().unwrap(),
        }
    }
}
