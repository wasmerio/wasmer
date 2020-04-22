tests-spec-update-testsuite:
    git subtree pull --prefix tests/wast/spec https://github.com/WebAssembly/testsuite.git master --squash

test:
    cargo test --release
