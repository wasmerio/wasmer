fn main() {
    std::fs::create_dir("/a").unwrap();
    assert!(std::fs::metadata("/a").unwrap().is_dir());

    std::fs::rename("/a", "/b").unwrap();
    assert!(matches!(std::fs::metadata("/a"), Err(e) if e.kind() == std::io::ErrorKind::NotFound));
    assert!(std::fs::metadata("/b").unwrap().is_dir());

    std::fs::create_dir("/a").unwrap();
    assert!(std::fs::metadata("/a").unwrap().is_dir());
    assert!(std::fs::metadata("/b").unwrap().is_dir());
}
