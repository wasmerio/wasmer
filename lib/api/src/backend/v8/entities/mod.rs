pub mod engine;
pub(crate) mod exception;
pub(crate) mod external;
pub(crate) mod function;
pub(crate) mod global;
pub(crate) mod instance;
pub(crate) mod memory;
pub(crate) mod module;
pub(crate) mod store;
pub(crate) mod table;
pub(crate) mod tag;

pub(self) fn check_isolate(store: &impl crate::AsStoreRef) {
    let store = store.as_store_ref();
    let v8_store = store.inner.store.as_v8();

    if v8_store.thread_id != std::thread::current().id() {
        panic!("Fatal error (v8): current thread is different from the thread the store was created in!");
    }
}
