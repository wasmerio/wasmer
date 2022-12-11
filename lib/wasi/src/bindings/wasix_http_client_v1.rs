#[derive(Clone)]
pub enum Method<'a> {
    Get,
    Head,
    Post,
    Put,
    Delete,
    Connect,
    Options,
    Trace,
    Patch,
    Other(&'a str),
}
impl<'a> core::fmt::Debug for Method<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Method::Get => f.debug_tuple("Method::Get").finish(),
            Method::Head => f.debug_tuple("Method::Head").finish(),
            Method::Post => f.debug_tuple("Method::Post").finish(),
            Method::Put => f.debug_tuple("Method::Put").finish(),
            Method::Delete => f.debug_tuple("Method::Delete").finish(),
            Method::Connect => f.debug_tuple("Method::Connect").finish(),
            Method::Options => f.debug_tuple("Method::Options").finish(),
            Method::Trace => f.debug_tuple("Method::Trace").finish(),
            Method::Patch => f.debug_tuple("Method::Patch").finish(),
            Method::Other(e) => f.debug_tuple("Method::Other").field(e).finish(),
        }
    }
}
#[derive(Clone)]
pub struct HeaderParam<'a> {
    pub key: &'a str,
    pub value: &'a [u8],
}
impl<'a> core::fmt::Debug for HeaderParam<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeaderParam")
            .field("key", &self.key)
            .field("value", &self.value)
            .finish()
    }
}
#[derive(Clone)]
pub struct HeaderResult {
    pub key: String,
    pub value: Vec<u8>,
}
impl core::fmt::Debug for HeaderResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeaderResult")
            .field("key", &self.key)
            .field("value", &self.value)
            .finish()
    }
}
pub type HeaderListParam<'a> = Vec<HeaderParam<'a>>;
pub type HeaderListResult = Vec<HeaderResult>;
pub type Fd = u32;
pub type TimeoutMs = u32;
#[repr(C)]
#[derive(Copy, Clone)]
pub struct RedirectFollow {
    pub max: u32,
}
impl core::fmt::Debug for RedirectFollow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RedirectFollow")
            .field("max", &self.max)
            .finish()
    }
}
impl wai_bindgen_wasmer::Endian for RedirectFollow {
    fn into_le(self) -> Self {
        Self {
            max: self.max.into_le(),
        }
    }
    fn from_le(self) -> Self {
        Self {
            max: self.max.from_le(),
        }
    }
}
unsafe impl wai_bindgen_wasmer::AllBytesValid for RedirectFollow {}
#[derive(Clone, Copy)]
pub enum RedirectPolicy {
    NoFollow,
    Follow(RedirectFollow),
}
impl core::fmt::Debug for RedirectPolicy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            RedirectPolicy::NoFollow => f.debug_tuple("RedirectPolicy::NoFollow").finish(),
            RedirectPolicy::Follow(e) => f.debug_tuple("RedirectPolicy::Follow").field(e).finish(),
        }
    }
}
#[derive(Clone)]
pub enum BodyParam<'a> {
    Data(&'a [u8]),
    Fd(Fd),
}
impl<'a> core::fmt::Debug for BodyParam<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BodyParam::Data(e) => f.debug_tuple("BodyParam::Data").field(e).finish(),
            BodyParam::Fd(e) => f.debug_tuple("BodyParam::Fd").field(e).finish(),
        }
    }
}
#[derive(Clone)]
pub enum BodyResult {
    Data(Vec<u8>),
    Fd(Fd),
}
impl core::fmt::Debug for BodyResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BodyResult::Data(e) => f.debug_tuple("BodyResult::Data").field(e).finish(),
            BodyResult::Fd(e) => f.debug_tuple("BodyResult::Fd").field(e).finish(),
        }
    }
}
#[derive(Clone)]
pub struct Request<'a> {
    pub url: &'a str,
    pub method: Method<'a>,
    pub headers: HeaderListParam<'a>,
    pub body: Option<BodyParam<'a>>,
    pub timeout: Option<TimeoutMs>,
    pub redirect_policy: Option<RedirectPolicy>,
}
impl<'a> core::fmt::Debug for Request<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Request")
            .field("url", &self.url)
            .field("method", &self.method)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("timeout", &self.timeout)
            .field("redirect-policy", &self.redirect_policy)
            .finish()
    }
}
#[derive(Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HeaderListResult,
    pub body: BodyResult,
    pub redirect_urls: Option<Vec<String>>,
}
impl core::fmt::Debug for Response {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("redirect-urls", &self.redirect_urls)
            .finish()
    }
}
pub trait WasixHttpClientV1: Sized + Send + Sync + 'static {
    type Client: std::fmt::Debug;
    fn client_new(&mut self) -> Result<Self::Client, String>;
    fn client_send(
        &mut self,
        self_: &Self::Client,
        request: Request<'_>,
    ) -> Result<Response, String>;
    fn drop_client(&mut self, state: Self::Client) {
        drop(state);
    }
}
pub struct WasixHttpClientV1Tables<T: WasixHttpClientV1> {
    pub(crate) client_table: wai_bindgen_wasmer::Table<T::Client>,
}
impl<T: WasixHttpClientV1> Default for WasixHttpClientV1Tables<T> {
    fn default() -> Self {
        Self {
            client_table: Default::default(),
        }
    }
}
impl<T: WasixHttpClientV1> Clone for WasixHttpClientV1Tables<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}
pub struct LazyInitialized {
    memory: wasmer::Memory,
    func_canonical_abi_realloc: wasmer::TypedFunction<(i32, i32, i32, i32), i32>,
}
#[must_use = "The returned initializer function must be called
      with the instance and the store before starting the runtime"]
pub fn add_to_imports<T>(
    store: &mut impl wasmer::AsStoreMut,
    imports: &mut wasmer::Imports,
    data: T,
) -> Box<dyn FnOnce(&wasmer::Instance, &dyn wasmer::AsStoreRef) -> Result<(), anyhow::Error>>
where
    T: WasixHttpClientV1,
{
    #[derive(Clone)]
    struct EnvWrapper<T: WasixHttpClientV1> {
        data: T,
        tables: std::rc::Rc<core::cell::RefCell<WasixHttpClientV1Tables<T>>>,
        lazy: std::rc::Rc<OnceCell<LazyInitialized>>,
    }
    unsafe impl<T: WasixHttpClientV1> Send for EnvWrapper<T> {}
    unsafe impl<T: WasixHttpClientV1> Sync for EnvWrapper<T> {}
    let lazy = std::rc::Rc::new(OnceCell::new());
    let env = EnvWrapper {
        data,
        tables: std::rc::Rc::default(),
        lazy: std::rc::Rc::clone(&lazy),
    };
    let env = wasmer::FunctionEnv::new(&mut *store, env);
    let mut exports = wasmer::Exports::new();
    let mut store = store.as_store_mut();
    exports.insert(
        "client::new",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_http_client_v1",
                    function = "client::new",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let host = &mut data_mut.data;
                let result = host.client_new();
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg0 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        caller_memory.store(
                            arg0 + 4,
                            wai_bindgen_wasmer::rt::as_i32({
                                let data_mut = store.data_mut();
                                let mut tables = data_mut.tables.borrow_mut();
                                tables.client_table.insert(e) as i32
                            }),
                        )?;
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg0 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        let vec0 = e;
                        let ptr0 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            vec0.len() as i32,
                        )?;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store_many(ptr0, vec0.as_bytes())?;
                        caller_memory
                            .store(arg0 + 8, wai_bindgen_wasmer::rt::as_i32(vec0.len() as i32))?;
                        caller_memory.store(arg0 + 4, wai_bindgen_wasmer::rt::as_i32(ptr0))?;
                    }
                };
                Ok(())
            },
        ),
    );
    exports.insert(
        "client::send",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  arg0: i32,
                  arg1: i32|
                  -> Result<(), wasmer::RuntimeError> {
                let span = wai_bindgen_wasmer::tracing::span!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    "wai-bindgen abi",
                    module = "wasix_http_client_v1",
                    function = "client::send",
                );
                let _enter = span.enter();
                let func_canonical_abi_realloc = store
                    .data()
                    .lazy
                    .get()
                    .unwrap()
                    .func_canonical_abi_realloc
                    .clone();
                let _memory: wasmer::Memory = store.data().lazy.get().unwrap().memory.clone();
                let _memory_view = _memory.view(&store);
                let mut _bc = wai_bindgen_wasmer::BorrowChecker::new(unsafe {
                    _memory_view.data_unchecked_mut()
                });
                let data_mut = store.data_mut();
                let tables = data_mut.tables.borrow_mut();
                let load0 = _bc.load::<i32>(arg0 + 0)?;
                let load1 = _bc.load::<i32>(arg0 + 4)?;
                let load2 = _bc.load::<i32>(arg0 + 8)?;
                let ptr3 = load1;
                let len3 = load2;
                let load4 = _bc.load::<u8>(arg0 + 12)?;
                let load8 = _bc.load::<i32>(arg0 + 24)?;
                let load9 = _bc.load::<i32>(arg0 + 28)?;
                let len16 = load9;
                let base16 = load8;
                let mut result16 = Vec::with_capacity(len16 as usize);
                for i in 0..len16 {
                    let base = base16 + i * 16;
                    result16.push({
                        let load10 = _bc.load::<i32>(base + 0)?;
                        let load11 = _bc.load::<i32>(base + 4)?;
                        let ptr12 = load10;
                        let len12 = load11;
                        let load13 = _bc.load::<i32>(base + 8)?;
                        let load14 = _bc.load::<i32>(base + 12)?;
                        let ptr15 = load13;
                        let len15 = load14;
                        HeaderParam {
                            key: _bc.slice_str(ptr12, len12)?,
                            value: _bc.slice(ptr15, len15)?,
                        }
                    });
                }
                let load17 = _bc.load::<u8>(arg0 + 32)?;
                let load23 = _bc.load::<u8>(arg0 + 48)?;
                let load25 = _bc.load::<u8>(arg0 + 56)?;
                let param0 = tables
                    .client_table
                    .get((load0) as u32)
                    .ok_or_else(|| wasmer::RuntimeError::new("invalid handle index"))?;
                let param1 = Request {
                    url: _bc.slice_str(ptr3, len3)?,
                    method: match i32::from(load4) {
                        0 => Method::Get,
                        1 => Method::Head,
                        2 => Method::Post,
                        3 => Method::Put,
                        4 => Method::Delete,
                        5 => Method::Connect,
                        6 => Method::Options,
                        7 => Method::Trace,
                        8 => Method::Patch,
                        9 => Method::Other({
                            let load5 = _bc.load::<i32>(arg0 + 16)?;
                            let load6 = _bc.load::<i32>(arg0 + 20)?;
                            let ptr7 = load5;
                            let len7 = load6;
                            _bc.slice_str(ptr7, len7)?
                        }),
                        _ => return Err(invalid_variant("Method")),
                    },
                    headers: result16,
                    body: match i32::from(load17) {
                        0 => None,
                        1 => Some({
                            let load18 = _bc.load::<u8>(arg0 + 36)?;
                            match i32::from(load18) {
                                0 => BodyParam::Data({
                                    let load19 = _bc.load::<i32>(arg0 + 40)?;
                                    let load20 = _bc.load::<i32>(arg0 + 44)?;
                                    let ptr21 = load19;
                                    let len21 = load20;
                                    _bc.slice(ptr21, len21)?
                                }),
                                1 => BodyParam::Fd({
                                    let load22 = _bc.load::<i32>(arg0 + 40)?;
                                    load22 as u32
                                }),
                                _ => return Err(invalid_variant("BodyParam")),
                            }
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                    timeout: match i32::from(load23) {
                        0 => None,
                        1 => Some({
                            let load24 = _bc.load::<i32>(arg0 + 52)?;
                            load24 as u32
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                    redirect_policy: match i32::from(load25) {
                        0 => None,
                        1 => Some({
                            let load26 = _bc.load::<u8>(arg0 + 60)?;
                            match i32::from(load26) {
                                0 => RedirectPolicy::NoFollow,
                                1 => RedirectPolicy::Follow({
                                    let load27 = _bc.load::<i32>(arg0 + 64)?;
                                    RedirectFollow { max: load27 as u32 }
                                }),
                                _ => return Err(invalid_variant("RedirectPolicy")),
                            }
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                };
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    self_ = wai_bindgen_wasmer::tracing::field::debug(&param0),
                    request = wai_bindgen_wasmer::tracing::field::debug(&param1),
                );
                let host = &mut data_mut.data;
                let result = host.client_send(param0, param1);
                drop(tables);
                wai_bindgen_wasmer::tracing::event!(
                    wai_bindgen_wasmer::tracing::Level::TRACE,
                    result = wai_bindgen_wasmer::tracing::field::debug(&result),
                );
                match result {
                    Ok(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                        let Response {
                            status: status28,
                            headers: headers28,
                            body: body28,
                            redirect_urls: redirect_urls28,
                        } = e;
                        caller_memory.store(
                            arg1 + 4,
                            wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(status28))
                                as u16,
                        )?;
                        let vec32 = headers28;
                        let len32 = vec32.len() as i32;
                        let result32 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            4,
                            len32 * 16,
                        )?;
                        for (i, e) in vec32.into_iter().enumerate() {
                            let base = result32 + (i as i32) * 16;
                            {
                                let HeaderResult {
                                    key: key29,
                                    value: value29,
                                } = e;
                                let vec30 = key29;
                                let ptr30 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    vec30.len() as i32,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr30, vec30.as_bytes())?;
                                caller_memory.store(
                                    base + 4,
                                    wai_bindgen_wasmer::rt::as_i32(vec30.len() as i32),
                                )?;
                                caller_memory
                                    .store(base + 0, wai_bindgen_wasmer::rt::as_i32(ptr30))?;
                                let vec31 = value29;
                                let ptr31 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    (vec31.len() as i32) * 1,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr31, &vec31)?;
                                caller_memory.store(
                                    base + 12,
                                    wai_bindgen_wasmer::rt::as_i32(vec31.len() as i32),
                                )?;
                                caller_memory
                                    .store(base + 8, wai_bindgen_wasmer::rt::as_i32(ptr31))?;
                            }
                        }
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store(arg1 + 12, wai_bindgen_wasmer::rt::as_i32(len32))?;
                        caller_memory.store(arg1 + 8, wai_bindgen_wasmer::rt::as_i32(result32))?;
                        match body28 {
                            BodyResult::Data(e) => {
                                caller_memory
                                    .store(arg1 + 16, wai_bindgen_wasmer::rt::as_i32(0i32) as u8)?;
                                let vec33 = e;
                                let ptr33 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    1,
                                    (vec33.len() as i32) * 1,
                                )?;
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory.store_many(ptr33, &vec33)?;
                                caller_memory.store(
                                    arg1 + 24,
                                    wai_bindgen_wasmer::rt::as_i32(vec33.len() as i32),
                                )?;
                                caller_memory
                                    .store(arg1 + 20, wai_bindgen_wasmer::rt::as_i32(ptr33))?;
                            }
                            BodyResult::Fd(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg1 + 16, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                                caller_memory.store(
                                    arg1 + 20,
                                    wai_bindgen_wasmer::rt::as_i32(wai_bindgen_wasmer::rt::as_i32(
                                        e,
                                    )),
                                )?;
                            }
                        };
                        match redirect_urls28 {
                            Some(e) => {
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg1 + 28, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                                let vec35 = e;
                                let len35 = vec35.len() as i32;
                                let result35 = func_canonical_abi_realloc.call(
                                    &mut store.as_store_mut(),
                                    0,
                                    0,
                                    4,
                                    len35 * 8,
                                )?;
                                for (i, e) in vec35.into_iter().enumerate() {
                                    let base = result35 + (i as i32) * 8;
                                    {
                                        let vec34 = e;
                                        let ptr34 = func_canonical_abi_realloc.call(
                                            &mut store.as_store_mut(),
                                            0,
                                            0,
                                            1,
                                            vec34.len() as i32,
                                        )?;
                                        let _memory_view = _memory.view(&store);
                                        let caller_memory =
                                            unsafe { _memory_view.data_unchecked_mut() };
                                        caller_memory.store_many(ptr34, vec34.as_bytes())?;
                                        caller_memory.store(
                                            base + 4,
                                            wai_bindgen_wasmer::rt::as_i32(vec34.len() as i32),
                                        )?;
                                        caller_memory.store(
                                            base + 0,
                                            wai_bindgen_wasmer::rt::as_i32(ptr34),
                                        )?;
                                    }
                                }
                                let _memory_view = _memory.view(&store);
                                let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                                caller_memory
                                    .store(arg1 + 36, wai_bindgen_wasmer::rt::as_i32(len35))?;
                                caller_memory
                                    .store(arg1 + 32, wai_bindgen_wasmer::rt::as_i32(result35))?;
                            }
                            None => {
                                let e = ();
                                {
                                    let _memory_view = _memory.view(&store);
                                    let caller_memory =
                                        unsafe { _memory_view.data_unchecked_mut() };
                                    caller_memory.store(
                                        arg1 + 28,
                                        wai_bindgen_wasmer::rt::as_i32(0i32) as u8,
                                    )?;
                                    let () = e;
                                }
                            }
                        };
                    }
                    Err(e) => {
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory
                            .store(arg1 + 0, wai_bindgen_wasmer::rt::as_i32(1i32) as u8)?;
                        let vec36 = e;
                        let ptr36 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            vec36.len() as i32,
                        )?;
                        let _memory_view = _memory.view(&store);
                        let caller_memory = unsafe { _memory_view.data_unchecked_mut() };
                        caller_memory.store_many(ptr36, vec36.as_bytes())?;
                        caller_memory
                            .store(arg1 + 8, wai_bindgen_wasmer::rt::as_i32(vec36.len() as i32))?;
                        caller_memory.store(arg1 + 4, wai_bindgen_wasmer::rt::as_i32(ptr36))?;
                    }
                };
                Ok(())
            },
        ),
    );
    imports.register_namespace("wasix_http_client_v1", exports);
    let mut canonical_abi = imports
        .get_namespace_exports("canonical_abi")
        .unwrap_or_else(wasmer::Exports::new);
    canonical_abi.insert(
        "resource_drop_client",
        wasmer::Function::new_typed_with_env(
            &mut store,
            &env,
            move |mut store: wasmer::FunctionEnvMut<EnvWrapper<T>>,
                  handle: u32|
                  -> Result<(), wasmer::RuntimeError> {
                let data_mut = store.data_mut();
                let mut tables = data_mut.tables.borrow_mut();
                let handle = tables.client_table.remove(handle).map_err(|e| {
                    wasmer::RuntimeError::new(format!("failed to remove handle: {}", e))
                })?;
                let host = &mut data_mut.data;
                host.drop_client(handle);
                Ok(())
            },
        ),
    );
    imports.register_namespace("canonical_abi", canonical_abi);
    let f = move |_instance: &wasmer::Instance, _store: &dyn wasmer::AsStoreRef| {
        let memory = _instance.exports.get_memory("memory")?.clone();
        let func_canonical_abi_realloc = _instance
            .exports
            .get_typed_function(&_store.as_store_ref(), "canonical_abi_realloc")
            .unwrap()
            .clone();
        lazy.set(LazyInitialized {
            memory,
            func_canonical_abi_realloc,
        })
        .map_err(|_e| anyhow::anyhow!("Couldn't set lazy initialized data"))?;
        Ok(())
    };
    Box::new(f)
}
use wai_bindgen_wasmer::once_cell::unsync::OnceCell;
use wai_bindgen_wasmer::rt::invalid_variant;
use wai_bindgen_wasmer::rt::RawMem;
#[allow(unused_imports)]
use wasmer::AsStoreMut as _;
#[allow(unused_imports)]
use wasmer::AsStoreRef as _;
