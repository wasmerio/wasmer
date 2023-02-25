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
#[derive(Debug)]
#[repr(transparent)]
pub struct Client(i32);
impl Client {
    pub unsafe fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    pub fn into_raw(self) -> i32 {
        let ret = self.0;
        core::mem::forget(self);
        return ret;
    }

    pub fn as_raw(&self) -> i32 {
        self.0
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_drop_client"]
            fn close(fd: i32);
        }
        unsafe {
            close(self.0);
        }
    }
}
impl Clone for Client {
    fn clone(&self) -> Self {
        #[link(wasm_import_module = "canonical_abi")]
        extern "C" {
            #[link_name = "resource_clone_client"]
            fn clone(val: i32) -> i32;
        }
        unsafe { Self(clone(self.0)) }
    }
}
impl Client {
    pub fn new() -> Result<Client, String> {
        unsafe {
            let ptr0 = WASIX_HTTP_CLIENT_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_http_client_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "client::new")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_http_client_v1_client::new"
                )]
                fn wai_import(_: i32);
            }
            wai_import(ptr0);
            match i32::from(*((ptr0 + 0) as *const u8)) {
                0 => Ok(Client(*((ptr0 + 4) as *const i32))),
                1 => Err({
                    let len1 = *((ptr0 + 8) as *const i32) as usize;

                    String::from_utf8(Vec::from_raw_parts(
                        *((ptr0 + 4) as *const i32) as *mut _,
                        len1,
                        len1,
                    ))
                    .unwrap()
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}
impl Client {
    pub fn send(&self, request: Request<'_>) -> Result<Response, String> {
        unsafe {
            let ptr0 = WASIX_HTTP_CLIENT_V1_RET_AREA.0.as_mut_ptr() as i32;
            *((ptr0 + 0) as *mut i32) = self.0;
            let Request {
                url: url1,
                method: method1,
                headers: headers1,
                body: body1,
                timeout: timeout1,
                redirect_policy: redirect_policy1,
            } = request;
            let vec2 = url1;
            let ptr2 = vec2.as_ptr() as i32;
            let len2 = vec2.len() as i32;
            *((ptr0 + 8) as *mut i32) = len2;
            *((ptr0 + 4) as *mut i32) = ptr2;
            match method1 {
                Method::Get => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
                Method::Head => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (1i32) as u8;
                        let () = e;
                    }
                }
                Method::Post => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (2i32) as u8;
                        let () = e;
                    }
                }
                Method::Put => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (3i32) as u8;
                        let () = e;
                    }
                }
                Method::Delete => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (4i32) as u8;
                        let () = e;
                    }
                }
                Method::Connect => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (5i32) as u8;
                        let () = e;
                    }
                }
                Method::Options => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (6i32) as u8;
                        let () = e;
                    }
                }
                Method::Trace => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (7i32) as u8;
                        let () = e;
                    }
                }
                Method::Patch => {
                    let e = ();
                    {
                        *((ptr0 + 12) as *mut u8) = (8i32) as u8;
                        let () = e;
                    }
                }
                Method::Other(e) => {
                    *((ptr0 + 12) as *mut u8) = (9i32) as u8;
                    let vec3 = e;
                    let ptr3 = vec3.as_ptr() as i32;
                    let len3 = vec3.len() as i32;
                    *((ptr0 + 20) as *mut i32) = len3;
                    *((ptr0 + 16) as *mut i32) = ptr3;
                }
            };
            let vec7 = headers1;
            let len7 = vec7.len() as i32;
            let layout7 = core::alloc::Layout::from_size_align_unchecked(vec7.len() * 16, 4);
            let result7 = std::alloc::alloc(layout7);
            if result7.is_null() {
                std::alloc::handle_alloc_error(layout7);
            }
            for (i, e) in vec7.into_iter().enumerate() {
                let base = result7 as i32 + (i as i32) * 16;
                {
                    let HeaderParam {
                        key: key4,
                        value: value4,
                    } = e;
                    let vec5 = key4;
                    let ptr5 = vec5.as_ptr() as i32;
                    let len5 = vec5.len() as i32;
                    *((base + 4) as *mut i32) = len5;
                    *((base + 0) as *mut i32) = ptr5;
                    let vec6 = value4;
                    let ptr6 = vec6.as_ptr() as i32;
                    let len6 = vec6.len() as i32;
                    *((base + 12) as *mut i32) = len6;
                    *((base + 8) as *mut i32) = ptr6;
                }
            }
            *((ptr0 + 28) as *mut i32) = len7;
            *((ptr0 + 24) as *mut i32) = result7 as i32;
            match body1 {
                Some(e) => {
                    *((ptr0 + 32) as *mut u8) = (1i32) as u8;
                    match e {
                        BodyParam::Data(e) => {
                            *((ptr0 + 36) as *mut u8) = (0i32) as u8;
                            let vec8 = e;
                            let ptr8 = vec8.as_ptr() as i32;
                            let len8 = vec8.len() as i32;
                            *((ptr0 + 44) as *mut i32) = len8;
                            *((ptr0 + 40) as *mut i32) = ptr8;
                        }
                        BodyParam::Fd(e) => {
                            *((ptr0 + 36) as *mut u8) = (1i32) as u8;
                            *((ptr0 + 40) as *mut i32) = wai_bindgen_rust::rt::as_i32(e);
                        }
                    };
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 32) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            match timeout1 {
                Some(e) => {
                    *((ptr0 + 48) as *mut u8) = (1i32) as u8;
                    *((ptr0 + 52) as *mut i32) = wai_bindgen_rust::rt::as_i32(e);
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 48) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            match redirect_policy1 {
                Some(e) => {
                    *((ptr0 + 56) as *mut u8) = (1i32) as u8;
                    match e {
                        RedirectPolicy::NoFollow => {
                            let e = ();
                            {
                                *((ptr0 + 60) as *mut u8) = (0i32) as u8;
                                let () = e;
                            }
                        }
                        RedirectPolicy::Follow(e) => {
                            *((ptr0 + 60) as *mut u8) = (1i32) as u8;
                            let RedirectFollow { max: max9 } = e;
                            *((ptr0 + 64) as *mut i32) = wai_bindgen_rust::rt::as_i32(max9);
                        }
                    };
                }
                None => {
                    let e = ();
                    {
                        *((ptr0 + 56) as *mut u8) = (0i32) as u8;
                        let () = e;
                    }
                }
            };
            let ptr10 = WASIX_HTTP_CLIENT_V1_RET_AREA.0.as_mut_ptr() as i32;
            #[link(wasm_import_module = "wasix_http_client_v1")]
            extern "C" {
                #[cfg_attr(target_arch = "wasm32", link_name = "client::send")]
                #[cfg_attr(
                    not(target_arch = "wasm32"),
                    link_name = "wasix_http_client_v1_client::send"
                )]
                fn wai_import(_: i32, _: i32);
            }
            wai_import(ptr0, ptr10);
            std::alloc::dealloc(result7, layout7);
            match i32::from(*((ptr10 + 0) as *const u8)) {
                0 => Ok({
                    let base13 = *((ptr10 + 8) as *const i32);
                    let len13 = *((ptr10 + 12) as *const i32);
                    let mut result13 = Vec::with_capacity(len13 as usize);
                    for i in 0..len13 {
                        let base = base13 + i * 16;
                        result13.push({
                            let len11 = *((base + 4) as *const i32) as usize;
                            let len12 = *((base + 12) as *const i32) as usize;

                            HeaderResult {
                                key: String::from_utf8(Vec::from_raw_parts(
                                    *((base + 0) as *const i32) as *mut _,
                                    len11,
                                    len11,
                                ))
                                .unwrap(),
                                value: Vec::from_raw_parts(
                                    *((base + 8) as *const i32) as *mut _,
                                    len12,
                                    len12,
                                ),
                            }
                        });
                    }
                    std::alloc::dealloc(
                        base13 as *mut _,
                        std::alloc::Layout::from_size_align_unchecked((len13 as usize) * 16, 4),
                    );

                    Response {
                        status: i32::from(*((ptr10 + 4) as *const u16)) as u16,
                        headers: result13,
                        body: match i32::from(*((ptr10 + 16) as *const u8)) {
                            0 => BodyResult::Data({
                                let len14 = *((ptr10 + 24) as *const i32) as usize;

                                Vec::from_raw_parts(
                                    *((ptr10 + 20) as *const i32) as *mut _,
                                    len14,
                                    len14,
                                )
                            }),
                            1 => BodyResult::Fd(*((ptr10 + 20) as *const i32) as u32),
                            _ => panic!("invalid enum discriminant"),
                        },
                        redirect_urls: match i32::from(*((ptr10 + 28) as *const u8)) {
                            0 => None,
                            1 => Some({
                                let base16 = *((ptr10 + 32) as *const i32);
                                let len16 = *((ptr10 + 36) as *const i32);
                                let mut result16 = Vec::with_capacity(len16 as usize);
                                for i in 0..len16 {
                                    let base = base16 + i * 8;
                                    result16.push({
                                        let len15 = *((base + 4) as *const i32) as usize;

                                        String::from_utf8(Vec::from_raw_parts(
                                            *((base + 0) as *const i32) as *mut _,
                                            len15,
                                            len15,
                                        ))
                                        .unwrap()
                                    });
                                }
                                std::alloc::dealloc(
                                    base16 as *mut _,
                                    std::alloc::Layout::from_size_align_unchecked(
                                        (len16 as usize) * 8,
                                        4,
                                    ),
                                );

                                result16
                            }),
                            _ => panic!("invalid enum discriminant"),
                        },
                    }
                }),
                1 => Err({
                    let len17 = *((ptr10 + 8) as *const i32) as usize;

                    String::from_utf8(Vec::from_raw_parts(
                        *((ptr10 + 4) as *const i32) as *mut _,
                        len17,
                        len17,
                    ))
                    .unwrap()
                }),
                _ => panic!("invalid enum discriminant"),
            }
        }
    }
}

#[repr(align(4))]
struct RetArea([u8; 68]);
static mut WASIX_HTTP_CLIENT_V1_RET_AREA: RetArea = RetArea([0; 68]);
