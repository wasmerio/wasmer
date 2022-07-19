use crate::EmEnv;
use wasmer::FunctionEnvMut;

pub fn _pthread_attr_destroy(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_attr_destroy");
    0
}

pub fn _pthread_attr_getstack(
    mut _ctx: FunctionEnvMut<EmEnv>,
    _stackaddr: i32,
    _stacksize: i32,
    _other: i32,
) -> i32 {
    trace!(
        "emscripten::_pthread_attr_getstack({}, {}, {})",
        _stackaddr,
        _stacksize,
        _other
    );
    // TODO: Translate from Emscripten
    // HEAP32[stackaddr >> 2] = STACK_BASE;
    // HEAP32[stacksize >> 2] = TOTAL_STACK;
    0
}

pub fn _pthread_attr_init(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_attr_init({})", _a);
    0
}

pub fn _pthread_attr_setstacksize(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_attr_setstacksize");
    0
}

pub fn _pthread_cleanup_pop(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) {
    trace!("emscripten::_pthread_cleanup_pop");
}

pub fn _pthread_cleanup_push(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) {
    trace!("emscripten::_pthread_cleanup_push");
}

pub fn _pthread_cond_destroy(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_cond_destroy");
    0
}

pub fn _pthread_cond_init(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_cond_init");
    0
}

pub fn _pthread_cond_signal(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_cond_signal");
    0
}

pub fn _pthread_cond_timedwait(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32, _c: i32) -> i32 {
    trace!("emscripten::_pthread_cond_timedwait");
    0
}

pub fn _pthread_cond_wait(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_cond_wait");
    0
}

pub fn _pthread_condattr_destroy(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_condattr_destroy");
    0
}

pub fn _pthread_condattr_init(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_condattr_init");
    0
}

pub fn _pthread_condattr_setclock(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_condattr_setclock");
    0
}

pub fn _pthread_create(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    trace!("emscripten::_pthread_create");
    // 11 seems to mean "no"
    11
}

pub fn _pthread_detach(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_detach");
    0
}

pub fn _pthread_equal(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_equal");
    0
}

pub fn _pthread_exit(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) {
    trace!("emscripten::_pthread_exit");
}

pub fn _pthread_getattr_np(mut _ctx: FunctionEnvMut<EmEnv>, _thread: i32, _attr: i32) -> i32 {
    trace!("emscripten::_pthread_getattr_np({}, {})", _thread, _attr);
    0
}

pub fn _pthread_getspecific(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_getspecific");
    0
}

pub fn _pthread_join(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_join");
    0
}

pub fn _pthread_self(mut _ctx: FunctionEnvMut<EmEnv>) -> i32 {
    trace!("emscripten::_pthread_self");
    0
}

pub fn _pthread_key_create(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_key_create");
    0
}

pub fn _pthread_mutex_destroy(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_mutex_destroy");
    0
}

pub fn _pthread_mutex_init(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_mutex_init");
    0
}

pub fn _pthread_mutexattr_destroy(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_mutexattr_destroy");
    0
}

pub fn _pthread_mutexattr_init(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_mutexattr_init");
    0
}

pub fn _pthread_mutexattr_settype(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_mutexattr_settype");
    0
}

pub fn _pthread_once(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_once");
    0
}

pub fn _pthread_rwlock_destroy(mut _ctx: FunctionEnvMut<EmEnv>, _rwlock: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_destroy({})", _rwlock);
    0
}

pub fn _pthread_rwlock_init(mut _ctx: FunctionEnvMut<EmEnv>, _rwlock: i32, _attr: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_init({}, {})", _rwlock, _attr);
    0
}

pub fn _pthread_rwlock_rdlock(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_rdlock");
    0
}

pub fn _pthread_rwlock_unlock(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_unlock");
    0
}

pub fn _pthread_rwlock_wrlock(mut _ctx: FunctionEnvMut<EmEnv>, _rwlock: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_wrlock({})", _rwlock);
    0
}

pub fn _pthread_setcancelstate(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_setcancelstate");
    0
}

pub fn _pthread_setspecific(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_setspecific");
    0
}

pub fn _pthread_sigmask(mut _ctx: FunctionEnvMut<EmEnv>, _a: i32, _b: i32, _c: i32) -> i32 {
    trace!("emscripten::_pthread_sigmask");
    0
}
