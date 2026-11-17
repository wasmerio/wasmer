use super::{
    IDLE_WORKER, STACK_POOL, StackExt, VMConfig, YIELDER, drain_stack_pool, on_host_stack,
    on_wasm_stack, tests::GLOBAL_STATE,
};
use crate::{
    Trap, VMContext, VMFunctionBody, VMFunctionContext, raise_user_trap, wasmer_call_trampoline,
};
use std::{cell::Cell, io};
use wasmer_types::RawValue;

thread_local! {
    static WORKERS_CREATED: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn worker_created() {
    WORKERS_CREATED.set(WORKERS_CREATED.get() + 1);
}

type Target = fn(i32, i32) -> i32;
const STACK_SIZE: usize = 1024 * 1024;

unsafe extern "C" fn trampoline(
    ctx: *mut VMContext,
    target: *const VMFunctionBody,
    slot: *mut RawValue,
) {
    // `call` supplies an i32 host environment, a Target, and one initialized i32 slot.
    unsafe {
        let target: Target = std::mem::transmute(target);
        *slot = target(*ctx.cast::<i32>(), (*slot).i32).into();
    }
}

fn call(stack_size: usize, target: Target, env: &i32, slot: &mut RawValue) -> Result<(), Trap> {
    let config = VMConfig {
        wasm_stack_size: Some(stack_size),
    };
    let ctx = VMFunctionContext {
        host_env: std::ptr::from_ref(env).cast_mut().cast(),
    };
    unsafe {
        wasmer_call_trampoline(
            None,
            &config,
            ctx,
            trampoline,
            target as *const VMFunctionBody,
            std::ptr::from_mut(slot).cast(),
        )
    }
}

fn has_idle_worker() -> bool {
    IDLE_WORKER.with(|cache| {
        let worker = cache.0.take();
        let present = worker.is_some();
        cache.0.set(worker);
        present
    })
}

#[test]
fn completes_distinct_calls_then_retires_a_trapped_worker() {
    let _lock = GLOBAL_STATE.lock().unwrap();
    drain_stack_pool();
    let before = WORKERS_CREATED.get();
    let mut first = 3.into();
    let mut second = 5.into();
    call(STACK_SIZE, i32::wrapping_add, &7, &mut first).unwrap();
    call(STACK_SIZE, i32::wrapping_mul, &11, &mut second).unwrap();
    assert_eq!(first, 10);
    assert_eq!(second, 55);
    assert_eq!(WORKERS_CREATED.get(), before + 1);

    let trap = call(
        STACK_SIZE,
        |_, _| unsafe { raise_user_trap(Box::new(io::Error::other("trapped worker"))) },
        &0,
        &mut first,
    )
    .unwrap_err();
    assert!(matches!(trap, Trap::User(error) if error.to_string() == "trapped worker"));
    assert!(!has_idle_worker());

    call(STACK_SIZE, i32::wrapping_sub, &60, &mut second).unwrap();
    assert_eq!(second, 5);
    assert_eq!(WORKERS_CREATED.get(), before + 2);
    drain_stack_pool();
}

#[test]
fn host_reentry_reuses_only_the_outer_worker() {
    fn reenter(depth: i32, value: i32) -> i32 {
        if depth == 0 {
            return value + 1;
        }
        on_host_stack(|| {
            let mut slot = value.into();
            call(STACK_SIZE, reenter, &(depth - 1), &mut slot).unwrap();
            unsafe { slot.i32 }
        })
    }

    let _lock = GLOBAL_STATE.lock().unwrap();
    drain_stack_pool();
    let before = WORKERS_CREATED.get();
    let mut slot = 0.into();
    for depth in [0, 2, 1, 2] {
        call(STACK_SIZE, reenter, &depth, &mut slot).unwrap();
    }
    assert_eq!(slot, 4);
    assert_eq!(WORKERS_CREATED.get(), before + 1);
    assert!(has_idle_worker());
    drain_stack_pool();
}

#[test]
fn nested_trampoline_restores_its_guest_stack_caller() {
    let _lock = GLOBAL_STATE.lock().unwrap();
    drain_stack_pool();
    let before = WORKERS_CREATED.get();
    let restored = on_wasm_stack(STACK_SIZE, None, || {
        let outer = YIELDER.with(Cell::get);
        let mut slot = 1.into();
        call(STACK_SIZE, i32::wrapping_add, &2, &mut slot).unwrap();
        slot == 3 && outer.is_some() && YIELDER.with(Cell::get) == outer
    });
    assert!(matches!(restored, Ok(true)));
    assert_eq!(WORKERS_CREATED.get(), before);
    drain_stack_pool();
}

#[test]
fn larger_stack_request_replaces_worker_and_drain_releases_it() {
    let _lock = GLOBAL_STATE.lock().unwrap();
    drain_stack_pool();
    let before = WORKERS_CREATED.get();
    let mut slot = 0.into();
    call(STACK_SIZE, i32::wrapping_add, &1, &mut slot).unwrap();
    call(4 * STACK_SIZE, i32::wrapping_add, &1, &mut slot).unwrap();
    assert_eq!(slot, 2);
    assert_eq!(WORKERS_CREATED.get(), before + 2);
    assert!(has_idle_worker());
    drain_stack_pool();
    assert!(!has_idle_worker());
}

#[test]
fn thread_exit_returns_idle_worker_stack_to_pool() {
    let _lock = GLOBAL_STATE.lock().unwrap();
    drain_stack_pool();
    std::thread::spawn(|| {
        let mut slot = 1.into();
        call(STACK_SIZE, i32::wrapping_add, &2, &mut slot).unwrap();
        assert_eq!(slot, 3);
        assert!(has_idle_worker());
    })
    .join()
    .unwrap();
    let returned = STACK_POOL
        .pop()
        .expect("worker stack returned at thread exit");
    assert!(returned.size() >= STACK_SIZE);
}
