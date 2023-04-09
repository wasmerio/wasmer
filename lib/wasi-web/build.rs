extern crate build_deps;

fn main() {
    build_deps::rerun_if_changed_paths("public/bin/*").unwrap();
    build_deps::rerun_if_changed_paths("public/*").unwrap();
    build_deps::rerun_if_changed_paths("public/bin").unwrap();
    build_deps::rerun_if_changed_paths("public").unwrap();
}
