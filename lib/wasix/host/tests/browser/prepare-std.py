#!/usr/bin/env python3
"""Expose Rust's existing TLS cleanup in an isolated, pinned build-std source tree.

Rust's wasm guard::enable has no OS thread-exit hook. wasm-bindgen frees TLS storage
but cannot run std's private destructor list. Keep this patch local to the artifact;
never change rustup's source installation or its default toolchain.
"""
import pathlib
import shutil
import shlex
import subprocess
import sys

root = pathlib.Path(sys.argv[1]).resolve() / "browser-sysroot"
pinned = pathlib.Path(subprocess.check_output(
    ["rustc", "+" + sys.argv[2], "--print", "sysroot"], text=True).strip())
root.mkdir(parents=True, exist_ok=True)
(root / "lib" / "rustlib").mkdir(parents=True, exist_ok=True)
for entry in (pinned / "lib").iterdir():
    if entry.name != "rustlib" and not (root / "lib" / entry.name).exists():
        (root / "lib" / entry.name).symlink_to(entry)
for entry in (pinned / "lib" / "rustlib").iterdir():
    if entry.name != "src" and not (root / "lib" / "rustlib" / entry.name).exists():
        (root / "lib" / "rustlib" / entry.name).symlink_to(entry)
source = root / "lib/rustlib/src/rust/library"
if not source.exists():
    shutil.copytree(pinned / "lib/rustlib/src/rust/library", source, symlinks=True)
file = source / "std/src/sys/thread_local/mod.rs"
patch = '''
// WASIX browser artifact: the host calls this once, after all Rust work on a
// worker has ended. Linux kernel/exit.c:do_exit owns kernel task teardown; TLS
// destruction is the userspace runtime's responsibility before host-thread exit.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics", target_os = "unknown"))]
#[unstable(feature = "wasixv2_browser_thread_cleanup", issue = "none")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __wasixv2_thread_cleanup() {
    // SAFETY: the browser executor calls this only on final worker retirement,
    // with no live references into this worker's TLS and no subsequent Rust work.
    unsafe { destructors::run() };
    crate::rt::thread_cleanup();
}
'''
text = file.read_text()
if "fn __wasixv2_thread_cleanup" not in text:
    file.write_text(text + patch)
# Cargo build-std locates rust-src through RUSTC --print sysroot. Native libraries
# remain symlinks to the pinned installation; only its source tree is copied.
wrapper = root / "rustc"
wrapper.write_text("#!/bin/sh\nexec " + shlex.quote(str(pinned / "bin/rustc")) + " --sysroot " + shlex.quote(str(root)) + " \"$@\"\n")
wrapper.chmod(0o755)
print(wrapper)
