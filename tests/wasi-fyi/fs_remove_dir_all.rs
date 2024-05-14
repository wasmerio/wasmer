use std::fs;

fn main() {
    assert!(fs::create_dir_all("/fyi/foo/bar").is_ok());
    assert!(fs::create_dir_all("/fyi/foo/baz").is_ok());
    assert_eq!(
        fs::read_dir("/fyi/foo")
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        vec!["bar", "baz"]
    );
    assert!(fs::remove_dir_all("/fyi/foo").is_ok());
    assert!(fs::read_dir("/fyi/foo").is_err());
}
