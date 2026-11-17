//! Host functions live in a store's objects, which safe code can move to
//! another store (`AsStoreMut::objects_mut` hands them out, and a user's
//! `AsStoreMut` decides which objects a function is made in). A host function
//! must then find the store its objects are in, and never the store it was made
//! in, which may be gone.
#![cfg(feature = "sys")]

use std::cell::RefCell;

use wasmer::*;

const WAT: &str = r#"(module
    (func $host (import "host" "run") (result i32))
    (func (export "run") (result i32) (call $host)))"#;

/// A store whose host function counts its calls in its environment and
/// reports the id of the objects its store holds.
fn counting_instance(store: &mut Store, module: &Module) -> TypedFunction<(), i32> {
    let env = FunctionEnv::new(store, 0i32);
    let host = Function::new_typed_with_env(store, &env, |mut env: FunctionEnvMut<i32>| {
        let id = env.as_store_mut().objects_mut().id();
        SEEN.with(|seen| seen.borrow_mut().push(format!("{id:?}")));
        *env.data_mut() += 1;
        *env.data()
    });
    let instance = Instance::new(store, module, &imports! { "host" => { "run" => host } }).unwrap();
    instance
        .exports
        .get_typed_function::<(), i32>(&*store, "run")
        .unwrap()
}

thread_local! {
    static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static OTHER: RefCell<Option<(Store, TypedFunction<(), i32>)>> = const { RefCell::new(None) };
}

fn take_seen() -> Vec<String> {
    SEEN.with(|seen| std::mem::take(&mut *seen.borrow_mut()))
}

/// Takes the 8-byte allocations a freed store left behind, with a value that
/// is not null, so that a host function reading one would not find it empty.
#[allow(clippy::vec_box)] // the boxes are the point
fn fill_freed_memory() -> Vec<Box<usize>> {
    (0..10_000).map(|_| Box::new(0x4141_4141)).collect()
}

#[test]
fn host_functions_follow_their_objects_to_another_store() {
    let engine = Engine::default();
    let module = Module::new(&engine, WAT).unwrap();
    let mut a = Store::new(engine.clone());
    let mut b = Store::new(engine.clone());
    let run = counting_instance(&mut a, &module);
    let objects = format!("{:?}", a.id());
    std::mem::swap(a.objects_mut(), b.objects_mut());
    assert_eq!(format!("{:?}", b.id()), objects);

    assert_eq!(run.call(&mut b).unwrap(), 1);
    assert_eq!(run.call(&mut b).unwrap(), 2);
    assert_eq!(take_seen(), vec![objects.clone(), objects]);
}

#[test]
fn host_functions_outlive_the_store_they_were_made_in() {
    let engine = Engine::default();
    let module = Module::new(&engine, WAT).unwrap();
    let mut a = Store::new(engine.clone());
    let mut b = Store::new(engine.clone());
    let run = counting_instance(&mut a, &module);
    let objects = format!("{:?}", a.id());
    std::mem::swap(a.objects_mut(), b.objects_mut());
    drop(a);
    let filler = fill_freed_memory();

    assert_eq!(run.call(&mut b).unwrap(), 1);
    assert_eq!(take_seen(), vec![objects]);
    drop(filler);
}

/// A host function of objects that moved to `q` is called through `q` from a
/// host function of `p`, which still holds `p`: the inner one must get `q`,
/// not the store it was made in.
#[test]
fn a_host_function_called_through_its_new_store_gets_that_store() {
    let engine = Engine::default();
    let module = Module::new(&engine, WAT).unwrap();
    let mut p = Store::new(engine.clone());
    let mut q = Store::new(engine.clone());
    let run_inner = counting_instance(&mut p, &module);
    let inner_objects = format!("{:?}", p.id());
    std::mem::swap(p.objects_mut(), q.objects_mut());

    // Made in `p` after the swap, so among the objects `p` holds now.
    let env = FunctionEnv::new(&mut p, ());
    let outer = Function::new_typed_with_env(&mut p, &env, |mut env: FunctionEnvMut<()>| {
        let before = env.as_store_mut().objects_mut().id();
        let inner = OTHER.with(|other| {
            let mut other = other.borrow_mut();
            let (store, run) = other.as_mut().unwrap();
            run.call(store).unwrap()
        });
        // Its own store is still usable, and still the same.
        assert_eq!(env.as_store_mut().objects_mut().id(), before);
        inner + 10
    });
    let instance =
        Instance::new(&mut p, &module, &imports! { "host" => { "run" => outer } }).unwrap();
    let run_outer = instance
        .exports
        .get_typed_function::<(), i32>(&p, "run")
        .unwrap();
    OTHER.with(|other| *other.borrow_mut() = Some((q, run_inner)));

    assert_eq!(run_outer.call(&mut p).unwrap(), 11);
    assert_eq!(take_seen(), vec![inner_objects]);
    OTHER.with(|other| other.borrow_mut().take());
}

/// An `AsStoreMut` whose store and objects are those of two different stores.
struct Split<'a> {
    store: &'a mut Store,
    objects: &'a mut Store,
}

impl AsStoreRef for Split<'_> {
    fn as_store_ref(&self) -> StoreRef<'_> {
        self.objects.as_store_ref()
    }
}

impl AsStoreMut for Split<'_> {
    fn as_store_mut(&mut self) -> StoreMut<'_> {
        self.store.as_store_mut()
    }

    fn objects_mut(&mut self) -> &mut StoreObjects {
        self.objects.objects_mut()
    }
}

/// A host function belongs to the objects it is made in, whichever store the
/// `AsStoreMut` it was made with also names.
#[test]
fn a_host_function_belongs_to_the_objects_it_was_made_in() {
    let engine = Engine::default();
    let module = Module::new(&engine, WAT).unwrap();
    let mut a = Store::new(engine.clone());
    let mut b = Store::new(engine.clone());
    let env = FunctionEnv::new(&mut b, 0i32);
    let host = {
        let mut split = Split {
            store: &mut a,
            objects: &mut b,
        };
        Function::new_typed_with_env(&mut split, &env, |mut env: FunctionEnvMut<i32>| {
            *env.data_mut() += 1;
            *env.data()
        })
    };
    let instance =
        Instance::new(&mut b, &module, &imports! { "host" => { "run" => host } }).unwrap();
    let run = instance
        .exports
        .get_typed_function::<(), i32>(&b, "run")
        .unwrap();
    drop(a);
    let filler = fill_freed_memory();

    assert_eq!(run.call(&mut b).unwrap(), 1);
    assert_eq!(run.call(&mut b).unwrap(), 2);
    drop(filler);
}
