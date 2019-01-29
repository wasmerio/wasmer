use crate::{
    export::Export,
    import::IsExport,
    types::{ElementType, TableDesc},
    vm,
};
use std::{cell::RefCell, fmt, ptr, rc::Rc};

mod anyfunc;

pub use self::anyfunc::Anyfunc;
use self::anyfunc::AnyfuncTable;

pub enum Element<'a> {
    Anyfunc(Anyfunc<'a>),
}

// #[derive(Debug)]
pub enum TableStorage {
    /// This is intended to be a caller-checked Anyfunc.
    Anyfunc(Box<AnyfuncTable>),
}

pub struct Table {
    desc: TableDesc,
    storage: Rc<RefCell<(TableStorage, vm::LocalTable)>>,
}

impl Table {
    pub fn new(desc: TableDesc) -> Result<Self, ()> {
        let mut local = vm::LocalTable {
            base: ptr::null_mut(),
            count: 0,
            table: ptr::null_mut(),
        };

        let storage = match desc.ty {
            ElementType::Anyfunc => TableStorage::Anyfunc(AnyfuncTable::new(desc, &mut local)?),
        };

        Ok(Self {
            desc,
            storage: Rc::new(RefCell::new((storage, local))),
        })
    }

    pub fn description(&self) -> TableDesc {
        self.desc
    }

    pub fn set(&self, index: u32, element: Element) -> Result<(), ()> {
        match &mut *self.storage.borrow_mut() {
            (TableStorage::Anyfunc(ref mut anyfunc_table), _) => {
                match element {
                    Element::Anyfunc(anyfunc) => anyfunc_table.set(index, anyfunc),
                    // _ => panic!("wrong element type for anyfunc table"),
                }
            }
        }
    }

    pub(crate) fn anyfunc_direct_access_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut [vm::Anyfunc]) -> R,
    {
        match &mut *self.storage.borrow_mut() {
            (TableStorage::Anyfunc(ref mut anyfunc_table), _) => f(anyfunc_table.internal_buffer()),
        }
    }

    pub fn current_size(&self) -> u32 {
        match &*self.storage.borrow() {
            (TableStorage::Anyfunc(ref anyfunc_table), _) => anyfunc_table.current_size(),
        }
    }

    pub fn grow(&self, delta: u32) -> Option<u32> {
        if delta == 0 {
            return Some(self.current_size());
        }

        match &mut *self.storage.borrow_mut() {
            (TableStorage::Anyfunc(ref mut anyfunc_table), ref mut local) => {
                anyfunc_table.grow(delta, local)
            }
        }
    }

    pub(crate) fn vm_local_table(&mut self) -> *mut vm::LocalTable {
        &mut self.storage.borrow_mut().1
    }
}

impl IsExport for Table {
    fn to_export(&mut self) -> Export {
        Export::Table(self.clone())
    }
}

impl Clone for Table {
    fn clone(&self) -> Self {
        Self {
            desc: self.desc,
            storage: Rc::clone(&self.storage),
        }
    }
}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Table").field("desc", &self.desc).finish()
    }
}
