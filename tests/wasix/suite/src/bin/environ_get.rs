wasix_conformance_suite_shared::declare!(|suite| {
    suite
        .register("read an environment variable")
        .env("VALUE", "42");
});

fn main() {
    let value = std::env::var("VALUE").unwrap();
    assert_eq!(value, "42");
}
