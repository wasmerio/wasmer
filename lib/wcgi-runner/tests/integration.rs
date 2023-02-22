use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use bytes::Bytes;
use http::{Request, StatusCode};
use hyper::{body::HttpBody, Body};
use wcgi_host::CgiDialect;
use wcgi_runner::Builder;

const FERRIS_SAYS: &str = "https://registry-cdn.wapm.dev/packages/wasmer-examples/ferris-says/ferris-says-0.2.0-2f5dfb76-77a9-11ed-a646-d2c429a5b858.webc";

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
    let static_server = build_wasi("staticserver").join("serve.wasm");
    let wasm = std::fs::read(&static_server).unwrap();

    let runner = Builder::default()
        .program(static_server.display().to_string())
        .map_dir(
            "example",
            project_root()
                .join("examples")
                .join("staticserver")
                .join("example"),
        )
        .build_wasm(wasm)
        .unwrap();
    let req = Request::builder()
        .uri("/example/index.html")
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
    assert!(body.contains("<h1>Welcome to WebC</h1>"));
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

/// Compile a package in this workspace to `wasm32-wasi` and get the directory
/// the final binary was saved to.
fn build_wasi(name: &str) -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let mut cmd = Command::new(cargo);
    cmd.arg("build")
        .arg("--target=wasm32-wasi")
        .args(["--package", name])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Note: We were seeing build failures in CI because "cargo llvm-cov"
    // automatically sets $RUSTFLAGS to include "-Zinstrument-coverage" and
    // "profielr_builtins" isn't available for WebAssembly.
    // See https://github.com/taiki-e/cargo-llvm-cov/issues/221
    cmd.env("RUSTFLAGS", "");

    let Output {
        status,
        stdout,
        stderr,
    } = cmd.output().expect("Unable to invoke cargo");

    if !status.success() {
        if !stdout.is_empty() {
            eprintln!("---- STDOUT ----");
            eprintln!("{}", String::from_utf8_lossy(&stdout));
        }
        if !stderr.is_empty() {
            eprintln!("---- STDERR ----");
            eprintln!("{}", String::from_utf8_lossy(&stderr));
        }
        panic!("{cmd:?} failed with {status}");
    }

    let target_dir = match std::env::var("CARGO_TARGET_DIR") {
        Ok(s) => PathBuf::from(s),
        Err(_) => project_root().join("target"),
    };
    target_dir.join("wasm32-wasi").join("debug")
}

fn project_root() -> &'static Path {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap();

    assert!(path.join(".git").exists());

    path
}
