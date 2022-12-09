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
    pub values: &'a [&'a [u8]],
}
impl<'a> core::fmt::Debug for HeaderParam<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeaderParam")
            .field("key", &self.key)
            .field("values", &self.values)
            .finish()
    }
}
#[derive(Clone)]
pub struct HeaderResult {
    pub key: String,
    pub values: Vec<Vec<u8>>,
}
impl core::fmt::Debug for HeaderResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeaderResult")
            .field("key", &self.key)
            .field("values", &self.values)
            .finish()
    }
}
pub type HeaderListParam<'a> = &'a [HeaderParam<'a>];
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
pub struct Request<'a> {
    pub url: &'a str,
    pub method: Method<'a>,
    pub headers: HeaderListParam<'a>,
    pub body: Option<Fd>,
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
    pub body: Fd,
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
#[derive(Debug)]
pub struct Client(wai_bindgen_wasmer::rt::ResourceIndex);
#[doc = " Auxiliary data associated with the wasm exports."]
#[derive(Default)]
pub struct WasixHttpClientV1Data {
    index_slab0: wai_bindgen_wasmer::rt::IndexSlab,
    resource_slab0: wai_bindgen_wasmer::rt::ResourceSlab,
    dtor0: OnceCell<wasmer::TypedFunction<i32, ()>>,
}
pub struct WasixHttpClientV1 {
    #[allow(dead_code)]
    env: wasmer::FunctionEnv<WasixHttpClientV1Data>,
    func_canonical_abi_free: wasmer::TypedFunction<(i32, i32, i32), ()>,
    func_canonical_abi_realloc: wasmer::TypedFunction<(i32, i32, i32, i32), i32>,
    func_client_new: wasmer::TypedFunction<(), i32>,
    func_client_perform: wasmer::TypedFunction<
        (
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
            i32,
        ),
        i32,
    >,
    memory: wasmer::Memory,
}
impl WasixHttpClientV1 {
    #[doc = " Adds any intrinsics, if necessary for this exported wasm"]
    #[doc = " functionality to the `ImportObject` provided."]
    #[doc = ""]
    #[doc = " This function returns the `WasixHttpClientV1Data` which needs to be"]
    #[doc = " passed through to `WasixHttpClientV1::new`."]
    fn add_to_imports(
        mut store: impl wasmer::AsStoreMut,
        imports: &mut wasmer::Imports,
    ) -> wasmer::FunctionEnv<WasixHttpClientV1Data> {
        let env = wasmer::FunctionEnv::new(&mut store, WasixHttpClientV1Data::default());
        let mut canonical_abi = imports
            .get_namespace_exports("canonical_abi")
            .unwrap_or_else(wasmer::Exports::new);
        canonical_abi.insert(
            "resource_drop_client",
            wasmer::Function::new_typed_with_env(
                &mut store,
                &env,
                move |mut store: wasmer::FunctionEnvMut<WasixHttpClientV1Data>,
                      idx: u32|
                      -> Result<(), wasmer::RuntimeError> {
                    let resource_idx = store.data_mut().index_slab0.remove(idx)?;
                    let wasm = match store.data_mut().resource_slab0.drop(resource_idx) {
                        Some(wasm) => wasm,
                        None => return Ok(()),
                    };
                    let dtor = store.data_mut().dtor0.get().unwrap().clone();
                    dtor.call(&mut store, wasm)?;
                    Ok(())
                },
            ),
        );
        canonical_abi.insert(
            "resource_clone_client",
            wasmer::Function::new_typed_with_env(
                &mut store,
                &env,
                move |mut store: wasmer::FunctionEnvMut<WasixHttpClientV1Data>,
                      idx: u32|
                      -> Result<u32, wasmer::RuntimeError> {
                    let state = &mut *store.data_mut();
                    let resource_idx = state.index_slab0.get(idx)?;
                    state.resource_slab0.clone(resource_idx)?;
                    Ok(state.index_slab0.insert(resource_idx))
                },
            ),
        );
        canonical_abi.insert(
            "resource_get_client",
            wasmer::Function::new_typed_with_env(
                &mut store,
                &env,
                move |mut store: wasmer::FunctionEnvMut<WasixHttpClientV1Data>,
                      idx: u32|
                      -> Result<i32, wasmer::RuntimeError> {
                    let state = &mut *store.data_mut();
                    let resource_idx = state.index_slab0.get(idx)?;
                    Ok(state.resource_slab0.get(resource_idx))
                },
            ),
        );
        canonical_abi.insert(
            "resource_new_client",
            wasmer::Function::new_typed_with_env(
                &mut store,
                &env,
                move |mut store: wasmer::FunctionEnvMut<WasixHttpClientV1Data>,
                      val: i32|
                      -> Result<u32, wasmer::RuntimeError> {
                    let state = &mut *store.data_mut();
                    let resource_idx = state.resource_slab0.insert(val);
                    Ok(state.index_slab0.insert(resource_idx))
                },
            ),
        );
        imports.register_namespace("canonical_abi", canonical_abi);
        env
    }
    #[doc = " Instantiates the provided `module` using the specified"]
    #[doc = " parameters, wrapping up the result in a structure that"]
    #[doc = " translates between wasm and the host."]
    #[doc = ""]
    #[doc = " The `imports` provided will have intrinsics added to it"]
    #[doc = " automatically, so it's not necessary to call"]
    #[doc = " `add_to_imports` beforehand. This function will"]
    #[doc = " instantiate the `module` otherwise using `imports`, and"]
    #[doc = " both an instance of this structure and the underlying"]
    #[doc = " `wasmer::Instance` will be returned."]
    pub fn instantiate(
        mut store: impl wasmer::AsStoreMut,
        module: &wasmer::Module,
        imports: &mut wasmer::Imports,
    ) -> anyhow::Result<(Self, wasmer::Instance)> {
        let env = Self::add_to_imports(&mut store, imports);
        let instance = wasmer::Instance::new(&mut store, module, &*imports)?;
        {
            let dtor0 = instance
                .exports
                .get_typed_function(&store, "canonical_abi_drop_client")?
                .clone();
            env.as_mut(&mut store)
                .dtor0
                .set(dtor0)
                .map_err(|_e| anyhow::anyhow!("Couldn't set canonical_abi_drop_client"))?;
        }
        Ok((Self::new(store, &instance, env)?, instance))
    }
    #[doc = " Low-level creation wrapper for wrapping up the exports"]
    #[doc = " of the `instance` provided in this structure of wasm"]
    #[doc = " exports."]
    #[doc = ""]
    #[doc = " This function will extract exports from the `instance`"]
    #[doc = " and wrap them all up in the returned structure which can"]
    #[doc = " be used to interact with the wasm module."]
    pub fn new(
        store: impl wasmer::AsStoreMut,
        _instance: &wasmer::Instance,
        env: wasmer::FunctionEnv<WasixHttpClientV1Data>,
    ) -> Result<Self, wasmer::ExportError> {
        let func_canonical_abi_free = _instance
            .exports
            .get_typed_function(&store, "canonical_abi_free")?;
        let func_canonical_abi_realloc = _instance
            .exports
            .get_typed_function(&store, "canonical_abi_realloc")?;
        let func_client_new = _instance
            .exports
            .get_typed_function(&store, "client::new")?;
        let func_client_perform = _instance
            .exports
            .get_typed_function(&store, "client::perform")?;
        let memory = _instance.exports.get_memory("memory")?.clone();
        Ok(WasixHttpClientV1 {
            func_canonical_abi_free,
            func_canonical_abi_realloc,
            func_client_new,
            func_client_perform,
            memory,
            env,
        })
    }
    pub fn client_new(
        &self,
        store: &mut wasmer::Store,
    ) -> Result<Result<Client, String>, wasmer::RuntimeError> {
        let func_canonical_abi_free = &self.func_canonical_abi_free;
        let _memory = &self.memory;
        let result0 = self.func_client_new.call(store)?;
        let _memory_view = _memory.view(&store);
        let load1 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result0 + 0)?;
        Ok(match i32::from(load1) {
            0 => Ok({
                let _memory_view = _memory.view(&store);
                let load2 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result0 + 4)?;
                let state = self.env.as_mut(store);
                let handle3 = state.index_slab0.remove(load2 as u32)?;
                Client(handle3)
            }),
            1 => Err({
                let _memory_view = _memory.view(&store);
                let load4 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result0 + 4)?;
                let _memory_view = _memory.view(&store);
                let load5 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result0 + 8)?;
                let ptr6 = load4;
                let len6 = load5;
                let data6 = copy_slice(store, _memory, func_canonical_abi_free, ptr6, len6, 1)?;
                String::from_utf8(data6).map_err(|_| wasmer::RuntimeError::new("invalid utf-8"))?
            }),
            _ => return Err(invalid_variant("expected")),
        })
    }
    pub fn client_perform(
        &self,
        store: &mut wasmer::Store,
        self_: &Client,
        request: Request<'_>,
    ) -> Result<Result<Response, String>, wasmer::RuntimeError> {
        let func_canonical_abi_realloc = &self.func_canonical_abi_realloc;
        let func_canonical_abi_free = &self.func_canonical_abi_free;
        let _memory = &self.memory;
        let obj0 = self_;
        let handle0 = {
            let state = self.env.as_mut(store);
            state.resource_slab0.clone(obj0.0)?;
            state.index_slab0.insert(obj0.0)
        };
        let Request {
            url: url1,
            method: method1,
            headers: headers1,
            body: body1,
            timeout: timeout1,
            redirect_policy: redirect_policy1,
        } = request;
        let vec2 = url1;
        let ptr2 = func_canonical_abi_realloc.call(
            &mut store.as_store_mut(),
            0,
            0,
            1,
            vec2.len() as i32,
        )?;
        let _memory_view = _memory.view(&store);
        unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr2, vec2.as_bytes())?;
        let (result4_0, result4_1, result4_2) = match method1 {
            Method::Get => {
                let e = ();
                {
                    let () = e;
                    (0i32, 0i32, 0i32)
                }
            }
            Method::Head => {
                let e = ();
                {
                    let () = e;
                    (1i32, 0i32, 0i32)
                }
            }
            Method::Post => {
                let e = ();
                {
                    let () = e;
                    (2i32, 0i32, 0i32)
                }
            }
            Method::Put => {
                let e = ();
                {
                    let () = e;
                    (3i32, 0i32, 0i32)
                }
            }
            Method::Delete => {
                let e = ();
                {
                    let () = e;
                    (4i32, 0i32, 0i32)
                }
            }
            Method::Connect => {
                let e = ();
                {
                    let () = e;
                    (5i32, 0i32, 0i32)
                }
            }
            Method::Options => {
                let e = ();
                {
                    let () = e;
                    (6i32, 0i32, 0i32)
                }
            }
            Method::Trace => {
                let e = ();
                {
                    let () = e;
                    (7i32, 0i32, 0i32)
                }
            }
            Method::Patch => {
                let e = ();
                {
                    let () = e;
                    (8i32, 0i32, 0i32)
                }
            }
            Method::Other(e) => {
                let vec3 = e;
                let ptr3 = func_canonical_abi_realloc.call(
                    &mut store.as_store_mut(),
                    0,
                    0,
                    1,
                    vec3.len() as i32,
                )?;
                let _memory_view = _memory.view(&store);
                unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr3, vec3.as_bytes())?;
                (9i32, ptr3, vec3.len() as i32)
            }
        };
        let vec9 = headers1;
        let len9 = vec9.len() as i32;
        let result9 =
            func_canonical_abi_realloc.call(&mut store.as_store_mut(), 0, 0, 4, len9 * 16)?;
        for (i, e) in vec9.into_iter().enumerate() {
            let base = result9 + (i as i32) * 16;
            {
                let HeaderParam {
                    key: key5,
                    values: values5,
                } = e;
                let vec6 = key5;
                let ptr6 = func_canonical_abi_realloc.call(
                    &mut store.as_store_mut(),
                    0,
                    0,
                    1,
                    vec6.len() as i32,
                )?;
                let _memory_view = _memory.view(&store);
                unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr6, vec6.as_bytes())?;
                let _memory_view = _memory.view(&store);
                unsafe { _memory_view.data_unchecked_mut() }
                    .store(base + 4, wai_bindgen_wasmer::rt::as_i32(vec6.len() as i32))?;
                let _memory_view = _memory.view(&store);
                unsafe { _memory_view.data_unchecked_mut() }
                    .store(base + 0, wai_bindgen_wasmer::rt::as_i32(ptr6))?;
                let vec8 = values5;
                let len8 = vec8.len() as i32;
                let result8 = func_canonical_abi_realloc.call(
                    &mut store.as_store_mut(),
                    0,
                    0,
                    4,
                    len8 * 8,
                )?;
                for (i, e) in vec8.into_iter().enumerate() {
                    let base = result8 + (i as i32) * 8;
                    {
                        let vec7 = e;
                        let ptr7 = func_canonical_abi_realloc.call(
                            &mut store.as_store_mut(),
                            0,
                            0,
                            1,
                            (vec7.len() as i32) * 1,
                        )?;
                        let _memory_view = _memory.view(&store);
                        unsafe { _memory_view.data_unchecked_mut() }.store_many(ptr7, &vec7)?;
                        let _memory_view = _memory.view(&store);
                        unsafe { _memory_view.data_unchecked_mut() }
                            .store(base + 4, wai_bindgen_wasmer::rt::as_i32(vec7.len() as i32))?;
                        let _memory_view = _memory.view(&store);
                        unsafe { _memory_view.data_unchecked_mut() }
                            .store(base + 0, wai_bindgen_wasmer::rt::as_i32(ptr7))?;
                    }
                }
                let _memory_view = _memory.view(&store);
                unsafe { _memory_view.data_unchecked_mut() }
                    .store(base + 12, wai_bindgen_wasmer::rt::as_i32(len8))?;
                let _memory_view = _memory.view(&store);
                unsafe { _memory_view.data_unchecked_mut() }
                    .store(base + 8, wai_bindgen_wasmer::rt::as_i32(result8))?;
            }
        }
        let (result10_0, result10_1) = match body1 {
            Some(e) => (1i32, wai_bindgen_wasmer::rt::as_i32(e)),
            None => {
                let e = ();
                {
                    let () = e;
                    (0i32, 0i32)
                }
            }
        };
        let (result11_0, result11_1) = match timeout1 {
            Some(e) => (1i32, wai_bindgen_wasmer::rt::as_i32(e)),
            None => {
                let e = ();
                {
                    let () = e;
                    (0i32, 0i32)
                }
            }
        };
        let (result14_0, result14_1, result14_2) = match redirect_policy1 {
            Some(e) => {
                let (result13_0, result13_1) = match e {
                    RedirectPolicy::NoFollow => {
                        let e = ();
                        {
                            let () = e;
                            (0i32, 0i32)
                        }
                    }
                    RedirectPolicy::Follow(e) => {
                        let RedirectFollow { max: max12 } = e;
                        (1i32, wai_bindgen_wasmer::rt::as_i32(max12))
                    }
                };
                (1i32, result13_0, result13_1)
            }
            None => {
                let e = ();
                {
                    let () = e;
                    (0i32, 0i32, 0i32)
                }
            }
        };
        let result15 = self.func_client_perform.call(
            store,
            handle0 as i32,
            ptr2,
            vec2.len() as i32,
            result4_0,
            result4_1,
            result4_2,
            result9,
            len9,
            result10_0,
            result10_1,
            result11_0,
            result11_1,
            result14_0,
            result14_1,
            result14_2,
        )?;
        let _memory_view = _memory.view(&store);
        let load16 = unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result15 + 0)?;
        Ok(match i32::from(load16) {
            0 => Ok({
                let _memory_view = _memory.view(&store);
                let load17 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<u16>(result15 + 4)?;
                let _memory_view = _memory.view(&store);
                let load18 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result15 + 8)?;
                let _memory_view = _memory.view(&store);
                let load19 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result15 + 12)?;
                let len29 = load19;
                let base29 = load18;
                let mut result29 = Vec::with_capacity(len29 as usize);
                for i in 0..len29 {
                    let base = base29 + i * 16;
                    result29.push({
                        let _memory_view = _memory.view(&store);
                        let load20 =
                            unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(base + 0)?;
                        let _memory_view = _memory.view(&store);
                        let load21 =
                            unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(base + 4)?;
                        let ptr22 = load20;
                        let len22 = load21;
                        let data22 =
                            copy_slice(store, _memory, func_canonical_abi_free, ptr22, len22, 1)?;
                        let _memory_view = _memory.view(&store);
                        let load23 =
                            unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(base + 8)?;
                        let _memory_view = _memory.view(&store);
                        let load24 =
                            unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(base + 12)?;
                        let len28 = load24;
                        let base28 = load23;
                        let mut result28 = Vec::with_capacity(len28 as usize);
                        for i in 0..len28 {
                            let base = base28 + i * 8;
                            result28.push({
                                let _memory_view = _memory.view(&store);
                                let load25 = unsafe { _memory_view.data_unchecked_mut() }
                                    .load::<i32>(base + 0)?;
                                let _memory_view = _memory.view(&store);
                                let load26 = unsafe { _memory_view.data_unchecked_mut() }
                                    .load::<i32>(base + 4)?;
                                let ptr27 = load25;
                                let len27 = load26;
                                copy_slice(
                                    store,
                                    _memory,
                                    func_canonical_abi_free,
                                    ptr27,
                                    len27,
                                    1,
                                )?
                            });
                        }
                        func_canonical_abi_free.call(
                            &mut store.as_store_mut(),
                            base28,
                            len28 * 8,
                            4,
                        )?;
                        HeaderResult {
                            key: String::from_utf8(data22)
                                .map_err(|_| wasmer::RuntimeError::new("invalid utf-8"))?,
                            values: result28,
                        }
                    });
                }
                func_canonical_abi_free.call(&mut store.as_store_mut(), base29, len29 * 16, 4)?;
                let _memory_view = _memory.view(&store);
                let load30 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result15 + 16)?;
                let _memory_view = _memory.view(&store);
                let load31 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<u8>(result15 + 20)?;
                Response {
                    status: u16::try_from(i32::from(load17)).map_err(bad_int)?,
                    headers: result29,
                    body: load30 as u32,
                    redirect_urls: match i32::from(load31) {
                        0 => None,
                        1 => Some({
                            let _memory_view = _memory.view(&store);
                            let load32 = unsafe { _memory_view.data_unchecked_mut() }
                                .load::<i32>(result15 + 24)?;
                            let _memory_view = _memory.view(&store);
                            let load33 = unsafe { _memory_view.data_unchecked_mut() }
                                .load::<i32>(result15 + 28)?;
                            let len37 = load33;
                            let base37 = load32;
                            let mut result37 = Vec::with_capacity(len37 as usize);
                            for i in 0..len37 {
                                let base = base37 + i * 8;
                                result37.push({
                                    let _memory_view = _memory.view(&store);
                                    let load34 = unsafe { _memory_view.data_unchecked_mut() }
                                        .load::<i32>(base + 0)?;
                                    let _memory_view = _memory.view(&store);
                                    let load35 = unsafe { _memory_view.data_unchecked_mut() }
                                        .load::<i32>(base + 4)?;
                                    let ptr36 = load34;
                                    let len36 = load35;
                                    let data36 = copy_slice(
                                        store,
                                        _memory,
                                        func_canonical_abi_free,
                                        ptr36,
                                        len36,
                                        1,
                                    )?;
                                    String::from_utf8(data36)
                                        .map_err(|_| wasmer::RuntimeError::new("invalid utf-8"))?
                                });
                            }
                            func_canonical_abi_free.call(
                                &mut store.as_store_mut(),
                                base37,
                                len37 * 8,
                                4,
                            )?;
                            result37
                        }),
                        _ => return Err(invalid_variant("option")),
                    },
                }
            }),
            1 => Err({
                let _memory_view = _memory.view(&store);
                let load38 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result15 + 4)?;
                let _memory_view = _memory.view(&store);
                let load39 =
                    unsafe { _memory_view.data_unchecked_mut() }.load::<i32>(result15 + 8)?;
                let ptr40 = load38;
                let len40 = load39;
                let data40 = copy_slice(store, _memory, func_canonical_abi_free, ptr40, len40, 1)?;
                String::from_utf8(data40).map_err(|_| wasmer::RuntimeError::new("invalid utf-8"))?
            }),
            _ => return Err(invalid_variant("expected")),
        })
    }
    #[doc = " Drops the host-owned handle to the resource"]
    #[doc = " specified."]
    #[doc = ""]
    #[doc = " Note that this may execute the WebAssembly-defined"]
    #[doc = " destructor for this type. This also may not run"]
    #[doc = " the destructor if there are still other references"]
    #[doc = " to this type."]
    pub fn drop_client(
        &self,
        store: &mut wasmer::Store,
        val: Client,
    ) -> Result<(), wasmer::RuntimeError> {
        let state = self.env.as_mut(store);
        let wasm = match state.resource_slab0.drop(val.0) {
            Some(val) => val,
            None => return Ok(()),
        };
        let dtor0 = state.dtor0.get().unwrap().clone();
        dtor0.call(store, wasm)?;
        Ok(())
    }
}
use core::convert::TryFrom;
use wai_bindgen_wasmer::once_cell::unsync::OnceCell;
use wai_bindgen_wasmer::rt::bad_int;
use wai_bindgen_wasmer::rt::copy_slice;
use wai_bindgen_wasmer::rt::invalid_variant;
use wai_bindgen_wasmer::rt::RawMem;
#[allow(unused_imports)]
use wasmer::AsStoreMut as _;
#[allow(unused_imports)]
use wasmer::AsStoreRef as _;
