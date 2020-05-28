use crate::exports::{ExportError, Exportable};
use crate::externals::Extern;
use crate::store::Store;
use crate::types::{Val, ValAnyFunc};
use crate::RuntimeError;
use crate::TableType;
use wasmer_runtime::{Export, ExportTable, Table as RuntimeTable, VMCallerCheckedAnyfunc};

#[derive(Clone)]
pub struct Table {
    store: Store,
    // If the Table is owned by the Store, not the instance
    owned_by_store: bool,
    exported: ExportTable,
}

fn set_table_item(
    table: &RuntimeTable,
    item_index: u32,
    item: VMCallerCheckedAnyfunc,
) -> Result<(), RuntimeError> {
    table.set(item_index, item).map_err(|e| e.into())
}

impl Table {
    pub fn new(store: &Store, ty: TableType, init: Val) -> Result<Table, RuntimeError> {
        let item = init.into_checked_anyfunc(store)?;
        let tunables = store.engine().tunables();
        let table_plan = tunables.table_plan(ty);
        let table = tunables
            .create_table(table_plan)
            .map_err(RuntimeError::new)?;

        let definition = table.vmtable();
        for i in 0..definition.current_elements {
            set_table_item(&table, i, item.clone())?;
        }

        Ok(Table {
            store: store.clone(),
            owned_by_store: true,
            exported: ExportTable {
                from: Box::leak(Box::new(table)),
                definition: Box::leak(Box::new(definition)),
            },
        })
    }

    fn table(&self) -> &RuntimeTable {
        unsafe { &*self.exported.from }
    }

    pub fn ty(&self) -> &TableType {
        &self.exported.plan().table
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn get(&self, index: u32) -> Option<Val> {
        let item = self.table().get(index)?;
        Some(ValAnyFunc::from_checked_anyfunc(item, &self.store))
    }

    pub fn set(&self, index: u32, val: Val) -> Result<(), RuntimeError> {
        let item = val.into_checked_anyfunc(&self.store)?;
        set_table_item(self.table(), index, item)
    }

    pub fn size(&self) -> u32 {
        self.table().size()
    }

    pub fn grow(&self, delta: u32, init: Val) -> Result<u32, RuntimeError> {
        let item = init.into_checked_anyfunc(&self.store)?;
        let table = self.table();
        match table.grow(delta) {
            Some(len) => {
                for i in 0..delta {
                    let i = len - (delta - i);
                    set_table_item(table, i, item.clone())?;
                }
                Ok(len)
            }
            None => Err(RuntimeError::new(format!(
                "failed to grow table by `{}`",
                delta
            ))),
        }
    }

    pub fn copy(
        dst_table: &Table,
        dst_index: u32,
        src_table: &Table,
        src_index: u32,
        len: u32,
    ) -> Result<(), RuntimeError> {
        if !Store::same(&dst_table.store, &src_table.store) {
            return Err(RuntimeError::new(
                "cross-`Store` table copies are not supported",
            ));
        }
        RuntimeTable::copy(
            dst_table.table(),
            src_table.table(),
            dst_index,
            src_index,
            len,
        )
        .map_err(RuntimeError::from_trap)?;
        Ok(())
    }

    pub(crate) fn from_export(store: &Store, wasmer_export: ExportTable) -> Table {
        Table {
            store: store.clone(),
            owned_by_store: false,
            exported: wasmer_export,
        }
    }
}

impl<'a> Exportable<'a> for Table {
    fn to_export(&self) -> Export {
        self.exported.clone().into()
    }
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Table(table) => Ok(table),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
