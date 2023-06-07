use wasix_http_client::{Body, HttpClient, RequestBuilder};

fn main() {
    let c = HttpClient::new().unwrap();
    let r = RequestBuilder::new()
        .uri("http://ferris2.christoph.app.wapm.dev/http-client-test")
        .body(Body::empty())
        .unwrap();
    eprintln!("fetching: {r:?}");

    let res = c.send(r).unwrap();
    dbg!(&res);
    assert!(res.status().is_success());

    let body = res.into_body().read_all().unwrap();
    let s = String::from_utf8(body).unwrap();
    eprintln!("Response body: {s}");

    assert!(s.contains("http-client-test"));
}
