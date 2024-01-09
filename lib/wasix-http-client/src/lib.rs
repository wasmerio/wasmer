// Auto-generated sys bindings.
#[allow(dead_code, clippy::all)]
mod wasix_http_client_v1;

use anyhow::bail;

use crate::wasix_http_client_v1 as sys;

pub use http::{header, Method, StatusCode};

#[derive(Clone)]
pub struct HttpClient {
    client: sys::Client,
}

// TODO: use proper custom error type
impl HttpClient {
    pub fn new() -> Result<Self, anyhow::Error> {
        let client = sys::Client::new().map_err(anyhow::Error::msg)?;
        Ok(Self { client })
    }

    pub fn send(&self, request: Request) -> Result<Response, anyhow::Error> {
        let (parts, body) = request.into_parts();

        let uri = parts.uri.to_string();
        let method = match &parts.method {
            m if m == Method::GET => sys::Method::Get,
            m if m == Method::HEAD => sys::Method::Head,
            m if m == Method::POST => sys::Method::Post,
            m if m == Method::PUT => sys::Method::Put,
            m if m == Method::DELETE => sys::Method::Delete,
            m if m == Method::CONNECT => sys::Method::Connect,
            m if m == Method::OPTIONS => sys::Method::Options,
            m if m == Method::TRACE => sys::Method::Trace,
            m if m == Method::PATCH => sys::Method::Patch,
            m => sys::Method::Other(m.as_str()),
        };

        let headers: Vec<_> = parts
            .headers
            .iter()
            .map(|(key, val)| sys::HeaderParam {
                key: key.as_str(),
                value: val.as_bytes(),
            })
            .collect();

        let body = match &body.0 {
            BodyInner::Data(d) => Some(sys::BodyParam::Data(d)),
        };

        let req = sys::Request {
            url: &uri,
            method,
            headers: &headers,
            body,
            timeout: None,
            redirect_policy: None,
        };

        let res = self.client.send(req).map_err(anyhow::Error::msg)?;

        let body = match res.body {
            sys::BodyResult::Data(d) => Body::new_data(d),
            sys::BodyResult::Fd(_) => {
                bail!("Received file descriptor response body, which is not suppported yet");
            }
        };

        let mut builder = http::Response::builder().status(res.status);
        for param in res.headers {
            builder = builder.header(param.key, param.value);
        }
        builder.body(body).map_err(anyhow::Error::from)
    }
}

// TODO: use custom request which can also hold timeout config, etc.
// #[derive(Clone, Debug)]
// pub struct Request {
//     pub method: Method,
//     pub headers: http::HeaderMap,
// }

pub type Request = http::Request<Body>;
pub type Response = http::Response<Body>;

#[derive(Debug)]
pub struct Body(BodyInner);

impl Body {
    pub fn empty() -> Self {
        Self::new_data(Vec::<u8>::new())
    }

    pub fn new_data<I>(data: I) -> Self
    where
        I: Into<Vec<u8>>,
    {
        Self(BodyInner::Data(data.into()))
    }

    pub fn read_all(self) -> Result<Vec<u8>, anyhow::Error> {
        match self.0 {
            BodyInner::Data(d) => Ok(d),
        }
    }
}

#[derive(Debug)]
pub enum BodyInner {
    Data(Vec<u8>),
}

impl From<Vec<u8>> for Body {
    fn from(value: Vec<u8>) -> Self {
        Self(BodyInner::Data(value))
    }
}

impl<'a> From<&'a [u8]> for Body {
    fn from(value: &'a [u8]) -> Self {
        Self(BodyInner::Data(value.to_vec()))
    }
}

impl From<String> for Body {
    fn from(value: String) -> Self {
        Self(BodyInner::Data(value.into_bytes()))
    }
}

impl<'a> From<&'a str> for Body {
    fn from(value: &'a str) -> Self {
        Self(BodyInner::Data(value.as_bytes().to_vec()))
    }
}

pub type RequestBuilder = http::request::Builder;
