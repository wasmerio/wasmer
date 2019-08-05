use wasmer_runtime_core::vm::Ctx;

pub fn _pthread_attr_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_attr_destroy");
    0
}

pub fn _pthread_attr_getstack(
    _ctx: &mut Ctx,
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

pub fn _pthread_attr_init(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_attr_init({})", _a);
    0
}

pub fn _pthread_attr_setstacksize(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_attr_setstacksize");
    0
}

pub fn _pthread_cleanup_pop(_ctx: &mut Ctx, _a: i32) -> () {
    trace!("emscripten::_pthread_cleanup_pop");
}

pub fn _pthread_cleanup_push(_ctx: &mut Ctx, _a: i32, _b: i32) -> () {
    trace!("emscripten::_pthread_cleanup_push");
}

pub fn _pthread_cond_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_cond_destroy");
    0
}

pub fn _pthread_cond_init(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_cond_init");
    0
}

pub fn _pthread_cond_signal(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_cond_signal");
    0
}

pub fn _pthread_cond_timedwait(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) -> i32 {
    trace!("emscripten::_pthread_cond_timedwait");
    0
}

pub fn _pthread_cond_wait(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_cond_wait");
    0
}

pub fn _pthread_condattr_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_condattr_destroy");
    0
}

pub fn _pthread_condattr_init(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_condattr_init");
    0
}

pub fn _pthread_condattr_setclock(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_condattr_setclock");
    0
}

pub fn _pthread_create(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32, _d: i32) -> i32 {
    trace!("emscripten::_pthread_create");
    // 11 seems to mean "no"
    11
}

pub fn _pthread_detach(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_detach");
    0
}

pub fn _pthread_equal(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_equal");
    0
}

pub fn _pthread_exit(_ctx: &mut Ctx, _a: i32) -> () {
    trace!("emscripten::_pthread_exit");
}

pub fn _pthread_getattr_np(_ctx: &mut Ctx, _thread: i32, _attr: i32) -> i32 {
    trace!("emscripten::_pthread_getattr_np({}, {})", _thread, _attr);
    0
}

pub fn _pthread_getspecific(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_getspecific");
    0
}

pub fn _pthread_join(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_join");
    0
}

pub fn _pthread_self(_ctx: &mut Ctx) -> i32 {
    trace!("emscripten::_pthread_self");
    0
}

pub fn _pthread_key_create(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_key_create");
    0
}

pub fn _pthread_mutex_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_mutex_destroy");
    0
}

pub fn _pthread_mutex_init(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_mutex_init");
    0
}

pub fn _pthread_mutexattr_destroy(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_mutexattr_destroy");
    0
}

pub fn _pthread_mutexattr_init(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_mutexattr_init");
    0
}

pub fn _pthread_mutexattr_settype(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_mutexattr_settype");
    0
}

pub fn _pthread_once(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_once");
    0
}

pub fn _pthread_rwlock_destroy(_ctx: &mut Ctx, _rwlock: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_destroy({})", _rwlock);
    0
}

pub fn _pthread_rwlock_init(_ctx: &mut Ctx, _rwlock: i32, _attr: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_init({}, {})", _rwlock, _attr);
    0
}

pub fn _pthread_rwlock_rdlock(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_rdlock");
    0
}

pub fn _pthread_rwlock_unlock(_ctx: &mut Ctx, _a: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_unlock");
    0
}

pub fn _pthread_rwlock_wrlock(_ctx: &mut Ctx, _rwlock: i32) -> i32 {
    trace!("emscripten::_pthread_rwlock_wrlock({})", _rwlock);
    0
}

pub fn _pthread_setcancelstate(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_setcancelstate");
    0
}

pub fn _pthread_setspecific(_ctx: &mut Ctx, _a: i32, _b: i32) -> i32 {
    trace!("emscripten::_pthread_setspecific");
    0
}

pub fn _pthread_sigmask(_ctx: &mut Ctx, _a: i32, _b: i32, _c: i32) -> i32 {
    trace!("emscripten::_pthread_sigmask");
    0
}
