use std::path::Path;

use bytes::Bytes;
use http::{Request, StatusCode};
use hyper::{body::HttpBody, Body};
use tempfile::TempDir;
use wcgi_host::CgiDialect;
use wcgi_runner::Builder;

const FERRIS_SAYS: &str = "https://registry-cdn.wapm.dev/packages/wasmer-examples/ferris-says/ferris-says-0.2.0-2f5dfb76-77a9-11ed-a646-d2c429a5b858.webc";
const STATICSERVER: &str =
    "https://registry-cdn.wapm.dev/contents/syrusakbary/staticserver/1.0.2/serve.wasm";

#[tokio::test]
async fn execute_a_webc_server() {
    let ferris_says = cached(FERRIS_SAYS);

    let runner = Builder::default()
        .cgi_dialect(CgiDialect::Wcgi)
        .build_webc(ferris_says)
        .unwrap();
    let req = Request::new(Body::default());
    let response = runner.handle(req).await.unwrap();

    let (parts, mut body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    let mut buffer = Vec::new();
    while let Some(result) = body.data().await {
        let chunk = result.unwrap();
        buffer.extend(chunk);
    }
    let body = String::from_utf8(buffer).unwrap();
    assert!(body.contains("Wasmer Deploy <3 Rustaceans!"));
}

#[tokio::test]
async fn execute_a_webassembly_server_with_mounted_directories() {
    let wasm = cached(STATICSERVER);
    let temp = TempDir::new().unwrap();
    let example = temp.path().join("example");
    std::fs::create_dir_all(&example).unwrap();
    std::fs::write(example.join("file.txt"), b"Hello, World!").unwrap();

    let runner = Builder::default()
        .program("staticserver")
        .map_dir("example", example)
        .build_wasm(wasm)
        .unwrap();
    let req = Request::builder()
        .uri("/example/file.txt")
        .body(Body::default())
        .unwrap();
    let response = runner.handle(req).await.unwrap();

    let (parts, mut body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
    let mut buffer = Vec::new();
    while let Some(result) = body.data().await {
        let chunk = result.unwrap();
        buffer.extend(chunk);
    }
    let body = String::from_utf8(buffer).unwrap();
    assert!(body.contains("Hello, World!"));
}

/// Download a file, caching it in $CARGO_TARGET_TMPDIR to avoid unnecessary
/// downloads.
fn cached(url: &str) -> Bytes {
    let uri: http::Uri = url.parse().unwrap();

    let file_name = Path::new(uri.path()).file_name().unwrap();
    let cache_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(env!("CARGO_PKG_NAME"));
    let cached_path = cache_dir.join(file_name);

    if cached_path.exists() {
        return std::fs::read(&cached_path).unwrap().into();
    }

    let response = ureq::get(url).call().unwrap();
    assert_eq!(
        response.status(),
        200,
        "Unable to get \"{url}\": {} {}",
        response.status(),
        response.status_text()
    );

    let mut body = Vec::new();
    response.into_reader().read_to_end(&mut body).unwrap();

    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(&cached_path, &body).unwrap();

    body.into()
}
