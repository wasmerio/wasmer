pub mod wasix_http_client_v1;

impl std::fmt::Display for wasix_http_client_v1::Method<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = match self {
            wasix_http_client_v1::Method::Get => "GET",
            wasix_http_client_v1::Method::Head => "HEAD",
            wasix_http_client_v1::Method::Post => "POST",
            wasix_http_client_v1::Method::Put => "PUT",
            wasix_http_client_v1::Method::Delete => "DELETE",
            wasix_http_client_v1::Method::Connect => "CONNECT",
            wasix_http_client_v1::Method::Options => "OPTIONS",
            wasix_http_client_v1::Method::Trace => "TRACE",
            wasix_http_client_v1::Method::Patch => "PATCH",
            wasix_http_client_v1::Method::Other(other) => *other,
        };
        write!(f, "{v}")
    }
}
