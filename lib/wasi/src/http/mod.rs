#[derive(Debug, Default)]
pub struct HttpRequestOptions {
    pub gzip: bool,
    pub cors_proxy: Option<String>,
}

pub struct HttpResponse {
    pub pos: usize,
    pub data: Option<Vec<u8>>,
    pub ok: bool,
    pub redirected: bool,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
}
