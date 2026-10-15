//! Golden/up-to-date checks for generated Rust and C, plus a compile-and-run check of the fixture
//! output.
//!
//! Set `WASIX_CODEGEN_BLESS=1` to rewrite the fixture golden files.

use std::path::{Path, PathBuf};

use wasix_codegen::{
    Target, generate_c, generate_rust, generate_rust_host, rust::source_from_header,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

const FIXTURE: &str = "lib/wasix2/wasix-codegen/tests/fixtures/sample.wit";
const GOLDEN: &str = "lib/wasix2/wasix-codegen/tests/golden/sample.rs";
/// Mirrors the libc2 tree layout.
const GOLDEN_C: &str = "lib/wasix2/wasix-codegen/tests/golden/sample_c";
const WIT: &str = "lib/wasix2/wit";
/// Mirrors the libc2 tree layout.
const GOLDEN_WASIX2_C: &str = "lib/wasix2/wasix-codegen/tests/golden/wasix2_c";
const TYPES2: &str = "lib/wasix2/wasix-types2/src/generated.rs";
const SYSCALLS2: &str = "lib/wasix2/wasix-syscalls2/src/generated.rs";

#[test]
fn fixture_golden_is_up_to_date() {
    let root = repo_root();
    let fresh = generate_rust(&root.join(FIXTURE), FIXTURE, Target::WASM32).unwrap();
    let golden_path = root.join(GOLDEN);
    if std::env::var_os("WASIX_CODEGEN_BLESS").is_some() {
        std::fs::write(&golden_path, &fresh).unwrap();
    }
    let golden = std::fs::read_to_string(&golden_path).unwrap();
    assert!(
        fresh == golden,
        "{GOLDEN} is stale; rerun with WASIX_CODEGEN_BLESS=1"
    );
}

#[test]
fn fixture_c_golden_is_up_to_date() {
    check_c_golden(FIXTURE, GOLDEN_C);
}

/// Snapshot of the C glue for the real definitions, so a WIT change shows up in review as a libc2
/// diff even without a libc2 checkout.
#[test]
fn wasix2_c_golden_is_up_to_date() {
    check_c_golden(WIT, GOLDEN_WASIX2_C);
}

/// Compares (or with `WASIX_CODEGEN_BLESS`, rewrites) every generated file under `golden_dir`,
/// and rejects files there that the generator no longer produces.
fn check_c_golden(wit: &str, golden_dir: &str) {
    let root = repo_root();
    let dir = root.join(golden_dir);
    let bless = std::env::var_os("WASIX_CODEGEN_BLESS").is_some();
    let files = generate_c(&root.join(wit), Target::WASM32).unwrap();
    for file in &files {
        let path = dir.join(file.path);
        if bless {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &file.contents).unwrap();
        }
        let golden = std::fs::read_to_string(&path).unwrap();
        assert!(
            file.contents == golden,
            "{golden_dir}/{} is stale; rerun with WASIX_CODEGEN_BLESS=1",
            file.path
        );
    }
    for path in files_under(&dir) {
        let rel = path.strip_prefix(&dir).unwrap();
        if rel == Path::new(".clang-format") || files.iter().any(|f| Path::new(f.path) == rel) {
            continue;
        }
        if bless {
            std::fs::remove_file(&path).unwrap();
        } else {
            panic!(
                "{golden_dir}/{} is no longer generated; rerun with WASIX_CODEGEN_BLESS=1",
                rel.display()
            );
        }
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(files_under(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// Regenerates wasix-types2 from the WIT source recorded in its header.
#[test]
fn wasix_types2_is_up_to_date() {
    let root = repo_root();
    let current = std::fs::read_to_string(root.join(TYPES2)).unwrap();
    let source = source_from_header(&current).expect("generated header names its source");
    let fresh = generate_rust(&root.join(source), source, Target::WASM32).unwrap();
    assert!(
        fresh == current,
        "{TYPES2} is stale; from the repo root run:\n  \
         cargo run -p wasix-codegen -- rust --wit {source} --out {TYPES2}"
    );
}

/// Regenerates wasix-syscalls2 from the WIT source recorded in its header.
#[test]
fn wasix_syscalls2_is_up_to_date() {
    let root = repo_root();
    let current = std::fs::read_to_string(root.join(SYSCALLS2)).unwrap();
    let source = source_from_header(&current).expect("generated header names its source");
    let fresh = generate_rust_host(&root.join(source), source, Target::WASM32).unwrap();
    assert!(
        fresh == current,
        "{SYSCALLS2} is stale; from the repo root run:\n  \
         cargo run -p wasix-codegen -- rust-host --wit {source} --out {SYSCALLS2}"
    );
}

#[allow(dead_code)]
mod sample {
    include!("golden/sample.rs");
}

use sample::*;

#[test]
fn record_roundtrip_is_little_endian() {
    let ts = Timespec {
        tv_sec: 0x0102_0304_0506_0708,
        tv_nsec: -2,
    };
    let mut buf = [0xffu8; 16];
    ts.write_to(&mut buf);
    assert_eq!(
        buf,
        [
            8, 7, 6, 5, 4, 3, 2, 1, 0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff
        ]
    );
    assert_eq!(Timespec::read_from(&buf), Some(ts));
    assert_eq!(Timespec::read_from(&buf[..15]), None);
}

#[test]
fn nested_record_with_union_and_implicit_padding() {
    let ev = EpollEvent {
        events: EpollEvents::EPOLLIN | EpollEvents::EPOLLET,
        data: EpollData::from_fd(7),
    };
    let mut buf = [0xaau8; 16];
    ev.write_to(&mut buf);
    // events at 0, padding zeroed at 4..8, data at 8.
    assert_eq!(buf[..8], [1, 0, 0, 0x80, 0, 0, 0, 0]);
    assert_eq!(buf[8..12], [7, 0, 0, 0]);
    let back = EpollEvent::read_from(&buf).unwrap();
    assert_eq!(back.data.fd(), 7);
    assert_eq!(back.data.u64(), 7);
    assert_eq!(<Statx as GuestLayout>::SIZE, 256);
    assert_eq!(Statx::default().stx_atime, StatxTimestamp::default());
}

#[test]
fn constants_and_flags() {
    assert_eq!(Errno::ENOENT.0, 2);
    assert_eq!(Errno::EWOULDBLOCK, Errno::EAGAIN);
    assert_eq!(Errno::EAGAIN.name(), Some("EAGAIN"));
    assert_eq!(format!("{:?}", Errno(9999)), "Errno(9999)");
    assert_eq!(AtFd::AT_FDCWD.0, -100);
    assert_eq!(IoctlRequest::FIONREAD.0, 0x541b);
    let f = OpenFlags::O_CREAT | OpenFlags::O_CLOEXEC;
    assert!(f.contains(OpenFlags::O_CREAT));
    assert!(!f.contains(OpenFlags::O_EXCL));
    assert_eq!(format!("{f:?}"), "OpenFlags(O_CREAT | O_CLOEXEC)");
    assert_eq!(
        format!("{:?}", OpenFlags(0x4000_0040)),
        "OpenFlags(O_CREAT | 0x40000000)"
    );
    assert_eq!(format!("{:?}", OpenFlags::O_RDONLY), "OpenFlags(0x0)");
    assert_eq!(WaitOptions::__WALL.0, 0x4000_0000);
    assert_eq!(
        format!("{:?}", WaitOptions::WNOHANG | WaitOptions::__WCLONE),
        "WaitOptions(WNOHANG | __WCLONE)"
    );
    assert_eq!(SignalLimits::_NSIG.name(), Some("_NSIG"));
    assert_eq!(SignalLimits::SIGRTmax.0, 64);
}

#[test]
fn syscall_metadata() {
    let llseek = SYSCALLS.iter().find(|s| s.name == "llseek").unwrap();
    assert_eq!(llseek.nr, Some(62));
    assert_eq!(llseek.module, "wasix2");
    assert_eq!(llseek.merged_i64, &["offset"]);
    let fadvise = SYSCALLS.iter().find(|s| s.name == "fadvise64_64").unwrap();
    assert_eq!(fadvise.merged_i64, &["offset", "len"]);
    let tys: Vec<_> = llseek.params.iter().map(|p| p.ty).collect();
    assert_eq!(
        tys,
        [ValType::I32, ValType::I64, ValType::I32, ValType::I32]
    );
    assert_eq!(llseek.result, Some(ValType::I32));

    let fcntl = SYSCALLS.iter().find(|s| s.name == "fcntl").unwrap();
    assert_eq!(fcntl.import_name, "fcntl64");
    let exit = SYSCALLS.iter().find(|s| s.name == "exit").unwrap();
    assert!(exit.noreturn);
    assert_eq!(exit.result, None);
    let spawn = SYSCALLS.iter().find(|s| s.name == "thread_spawn").unwrap();
    assert_eq!((spawn.module, spawn.nr), ("wasix_ext", None));
    assert!(
        SYSCALLS
            .iter()
            .any(|s| s.name == "io_setup" && s.unsupported)
    );
}
