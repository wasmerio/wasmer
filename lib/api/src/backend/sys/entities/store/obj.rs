use wasmer_vm::StoreId;

use crate::AsStoreMut;

impl crate::StoreObjects {
    /// Consume store objects into [`crate::backend::sys::store::StoreObjects`].
    pub fn into_sys(self) -> crate::backend::sys::store::StoreObjects {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }

    /// Convert a reference to store objects into a reference [`crate::backend::sys::store::StoreObjects`].
    pub fn as_sys(&self) -> &crate::backend::sys::store::StoreObjects {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }

    /// Convert a mutable reference to store objects into a mutable reference [`crate::backend::sys::store::StoreObjects`].
    pub fn as_sys_mut(&mut self) -> &mut crate::backend::sys::store::StoreObjects {
        match self {
            Self::Sys(s) => s,
            _ => panic!("Not a `sys` store!"),
        }
    }
}

//pub trait GetStoreObjects {
//    /// Return a mutable reference to [`wasmer_vm::StoreObjects`] and a reference to the current
//    /// engine.
//    fn engine_and_objects_mut(
//        &mut self,
//    ) -> (&crate::Engine, &mut crate::backend::sys::store::StoreObjects);
//
//    /// Return a mutable reference to [`wasmer_vm::StoreObjects`].
//    fn objects_mut(&mut self) -> &mut crate::backend::sys::store::StoreObjects;
//}
//
//impl GetStoreObjects for crate::StoreInner {
//    fn objects_mut(&mut self) -> &mut crate::backend::sys::store::StoreObjects {
//        self.objects.as_sys_mut()
//    }
//
//    fn engine_and_objects_mut(
//        &mut self,
//    ) -> (&crate::Engine, &mut crate::backend::sys::store::StoreObjects) {
//        match (&mut self.objects, &self.store) {
//            (crate::StoreObjects::Sys(o), crate::BackendStore::Sys(s)) => (&s.engine, o),
//            _ => panic!("Not a `sys` store!"),
//        }
//    }
//}
//
//impl<T: AsStoreMut> GetStoreObjects for T {
//    fn objects_mut<'a>(&'a mut self) -> &'a mut crate::backend::sys::store::StoreObjects {
//        match self.as_store_mut().inner.objects {
//            crate::StoreObjects::Sys(ref mut s) => s,
//            _ => panic!("Not a `sys` store!"),
//        }
//    }
//
//    fn engine_and_objects_mut(
//        &mut self,
//    ) -> (&crate::Engine, &mut crate::backend::sys::store::StoreObjects) {
//        let mut store = self.as_store_mut();
//        match (&mut store.inner.objects, &store.inner.store) {
//            (crate::StoreObjects::Sys(ref mut o), crate::BackendStore::Sys(ref s)) => {
//                (&s.engine, o)
//            }
//            _ => panic!("Not a `sys` store!"),
//        }
//    }
//}
