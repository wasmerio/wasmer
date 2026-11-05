//! Source-only PIC guests exercise unchanged v1 loader imports through Machine.
use std::path::PathBuf;
use wasixv2::{Machine, ProcessConfig};

fn leb(mut value: u32, out: &mut Vec<u8>) {
    loop {
        let next = (value & 127) as u8;
        value >>= 7;
        out.push(next | if value == 0 { 0 } else { 128 });
        if value == 0 {
            return;
        }
    }
}

// Same dylink.0 source fixture structure as dylink::tests::bytes and api::retirement.
fn pic(wat: &str, needed: &[&str]) -> Vec<u8> {
    pic_with_table(wat, needed, 0)
}

fn pic_with_table(wat: &str, needed: &[&str], table_entries: u32) -> Vec<u8> {
    let mut image = wat::parse_str(wat).unwrap();
    let mut section = vec![8];
    section.extend_from_slice(b"dylink.0");
    let mut info = Vec::new();
    for value in [512, 0, table_entries, 0] {
        leb(value, &mut info);
    }
    section.push(1);
    leb(info.len() as u32, &mut section);
    section.extend(info);
    if !needed.is_empty() {
        let mut names = Vec::new();
        leb(needed.len() as u32, &mut names);
        for name in needed {
            leb(name.len() as u32, &mut names);
            names.extend_from_slice(name.as_bytes());
        }
        section.push(2);
        leb(names.len() as u32, &mut section);
        section.extend(names);
    }
    image.push(0);
    leb(section.len() as u32, &mut image);
    image.extend(section);
    image
}

struct Programs(PathBuf);
impl Programs {
    fn new(name: &str, images: &[(&str, Vec<u8>)]) -> Self {
        let path =
            std::env::temp_dir().join(format!("wasix1-dynamic-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        for (name, bytes) in images {
            std::fs::write(path.join(name), bytes).unwrap();
        }
        Self(path)
    }
}
impl Drop for Programs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main_image(body: &str, mark: &str, needed: &[&str]) -> Vec<u8> {
    main_image_with(
        body,
        mark,
        needed,
        0,
        "",
        "",
        "(drop (call $load (i32.const 14)))",
    )
}

fn main_image_with(
    body: &str,
    mark: &str,
    needed: &[&str],
    table_entries: u32,
    extra_imports: &str,
    extra_code: &str,
    thread_body: &str,
) -> Vec<u8> {
    let table_size = 3 + table_entries;
    pic_with_table(
        &format!(
            r#"(module
      (type $result (func (result i32)))
      (import "env" "memory" (memory 1 65536 shared))
      (import "env" "__indirect_function_table" (table {table_size} funcref))
      (import "env" "__stack_pointer" (global $stack (mut i32)))
      (import "env" "__memory_base" (global $base i32))
      (import "env" "__table_base" (global $table_base i32))
      (import "wasix_32v1" "dlopen" (func $open (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "dlsym" (func $sym (param i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "dl_invalid_handle" (func $valid (param i32) (result i32)))
      (import "wasix_32v1" "call_dynamic" (func $invoke (param i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "reflect_signature" (func $reflect (param i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasi_snapshot_preview1" "fd_write" (func $write (param i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (import "wasix_32v1" "thread_spawn_v2" (func $spawn (param i32 i32) (result i32)))
      (import "wasix_32v1" "thread_join" (func $join (param i32) (result i32)))
      (import "wasi_snapshot_preview1" "path_unlink_file" (func $unlink (param i32 i32 i32) (result i32)))
      {extra_imports}
      (export "__indirect_function_table" (table 0))
      (data (global.get $base) "/prog/side.so\00/prog/nest.so\00value\00main_value\00missing\00/prog\00")
      (func $at (param i32) (result i32) (i32.add (global.get $base) (local.get 0)))
      (func $check (param i32 i32 i32)
        (if (i32.ne (local.get 0) (local.get 1)) (then (call $exit (local.get 2)))))
      (func $load (param $path i32) (result i32)
        (call $check (call $open (call $at (local.get $path)) (i32.const 13) (i32.const -1)
          (call $at (i32.const 256)) (i32.const 64) (call $at (i32.const 53)) (i32.const 5)
          (call $at (i32.const 192))) (i32.const 0) (i32.const 10))
        (i32.load (call $at (i32.const 192))))
      (func $lookup (param $handle i32) (param $name i32) (param $len i32) (result i32)
        (call $check (call $sym (local.get $handle) (call $at (local.get $name)) (local.get $len)
          (call $at (i32.const 256)) (i32.const 64) (call $at (i32.const 196))) (i32.const 0) (i32.const 11))
        (i32.load (call $at (i32.const 196))))
      {extra_code}
      (func (export "mark") (param $n i32)
        (i32.store (call $at (i32.const 200))
          (i32.add (i32.mul (i32.load (call $at (i32.const 200))) (i32.const 10)) (local.get $n)))
        {mark})
      (func (export "wasi_thread_start") (param i32 i32)
        {thread_body})
      (func (export "main_value") (result i32) (i32.const 7))
      (func (export "_start") (local $handle i32) (local $grown i32) {body}))"#
        ),
        needed,
        table_entries,
    )
}

fn closure_main(body: &str, extra_code: &str, thread_body: &str) -> Vec<u8> {
    main_image_with(
        body,
        "",
        &[],
        1,
        r#"
      (import "wasix_32v1" "closure_allocate" (func $allocate (param i32) (result i32)))
      (import "wasix_32v1" "closure_prepare" (func $prepare (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "closure_free" (func $free (param i32) (result i32)))
      "#,
        extra_code,
        thread_body,
    )
}

fn side(mark: i32, needed: &[&str]) -> Vec<u8> {
    pic(
        &format!(
            r#"(module
      (import "env" "memory" (memory 1 65536 shared))
      (import "env" "__indirect_function_table" (table 0 funcref))
      (import "env" "__memory_base" (global i32))
      (import "env" "__table_base" (global i32))
      (import "env" "mark" (func $mark (param i32)))
      (func (export "__wasm_call_ctors") (call $mark (i32.const {mark})))
      (func (export "value") (result i32) (i32.const 42)))"#
        ),
        needed,
    )
}

const CLOSURE_BACKING: &str = r#"
      (type $i32 (func (param i32) (result i32)))
      (type $i64 (func (param i64) (result i64)))
      (type $multi (func (param i32 i64 f32 f64 v128) (result i32 i64 f32 f64 v128)))
      (func $backing (param $values i32) (param $results i32) (param $env i32) (local $value i32)
        (if (i32.eq (local.get $env) (i32.const 1)) (then
          (i32.store (local.get $results)
            (i32.add (i32.load (local.get $values)) (i32.const 1)))))
        (if (i32.eq (local.get $env) (i32.const 2)) (then
          (i64.store (local.get $results)
            (i64.add (i64.load (local.get $values)) (i64.const 7)))))
        (if (i32.eq (local.get $env) (i32.const 3)) (then
          (i32.store (local.get $results)
            (i32.add (i32.load (local.get $values)) (i32.const 1)))
          (i64.store (i32.add (local.get $results) (i32.const 4))
            (i64.add (i64.load (i32.add (local.get $values) (i32.const 4))) (i64.const 2)))
          (f32.store (i32.add (local.get $results) (i32.const 12))
            (f32.mul (f32.load (i32.add (local.get $values) (i32.const 12))) (f32.const 2)))
          (f64.store (i32.add (local.get $results) (i32.const 16))
            (f64.add (f64.load (i32.add (local.get $values) (i32.const 16))) (f64.const 0.75)))
          (v128.store (i32.add (local.get $results) (i32.const 24))
            (i32x4.add (v128.load (i32.add (local.get $values) (i32.const 24)))
              (v128.const i32x4 1 1 1 1)))))
        (if (i32.eq (local.get $env) (i32.const 4)) (then
          (local.set $value (i32.add (i32.load (local.get $values)) (i32.const 40)))
          (call $check
            (call $prepare (global.get $table_base) (i32.load (call $at (i32.const 224)))
              (call $at (i32.const 128)) (i32.const 1)
              (call $at (i32.const 136)) (i32.const 1) (i32.const 1))
            (i32.const 0) (i32.const 71))
          (i32.store (local.get $results) (local.get $value))))
        (if (i32.eq (local.get $env) (i32.const 5)) (then
          (local.set $value (i32.add (i32.load (local.get $values)) (i32.const 50)))
          (call $check (call $free (i32.load (call $at (i32.const 224))))
            (i32.const 0) (i32.const 81))
          (i32.store (local.get $results) (local.get $value)))))
      (func $put_multi_types
        (i32.store8 (call $at (i32.const 128)) (i32.const 0))
        (i32.store8 (call $at (i32.const 129)) (i32.const 1))
        (i32.store8 (call $at (i32.const 130)) (i32.const 2))
        (i32.store8 (call $at (i32.const 131)) (i32.const 3))
        (i32.store8 (call $at (i32.const 132)) (i32.const 4))
        (i32.store8 (call $at (i32.const 136)) (i32.const 0))
        (i32.store8 (call $at (i32.const 137)) (i32.const 1))
        (i32.store8 (call $at (i32.const 138)) (i32.const 2))
        (i32.store8 (call $at (i32.const 139)) (i32.const 3))
        (i32.store8 (call $at (i32.const 140)) (i32.const 4)))
      (elem (global.get $table_base) $backing)
"#;

fn run(name: &str, main: &[u8], images: &[(&str, Vec<u8>)]) -> i32 {
    let programs = Programs::new(name, images);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, async {
        let machine = Machine::new();
        machine
            .mount_host_dir("/prog", &programs.0, false)
            .await
            .unwrap();
        machine
            .run_process(
                main,
                ProcessConfig {
                    env: vec!["LD_LIBRARY_PATH=/prog".into()],
                    args: vec!["dynamic".into()],
                    stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
                },
            )
            .await
            .unwrap()
    })
}

#[test]
fn an_http_only_pic_main_uses_the_existing_v1_preparation_path() {
    let image = pic(
        r#"(module
      (import "env" "memory" (memory 1 65536 shared))
      (import "env" "__indirect_function_table" (table 3 funcref))
      (import "env" "__stack_pointer" (global (mut i32)))
      (import "env" "__memory_base" (global i32))
      (import "env" "__table_base" (global i32))
      (import "wasix_http_client_v1" "client::new" (func (param i32)))
      (export "memory" (memory 0))
      (func (export "canonical_abi_realloc") (param i32 i32 i32 i32) (result i32) i32.const 0)
      (func (export "_start")))"#,
        &[],
    );
    assert_eq!(run("http-only", &image, &[]), 0);
}

#[test]
fn old_handles_flags_scopes_and_error_buffers_survive_unchanged_imports() {
    let main = main_image(
        r#"
      (call $check (call $open (i32.const 0) (i32.const -1) (i32.const -1)
        (call $at (i32.const 256)) (i32.const 64) (i32.const -1) (i32.const -1)
        (call $at (i32.const 192))) (i32.const 0) (i32.const 13))
      (call $check (i32.load (call $at (i32.const 192))) (i32.const 1) (i32.const 14))
      (call $check (call $open (call $at (i32.const 0)) (i32.const 0) (i32.const -1)
        (call $at (i32.const 256)) (i32.const 1) (i32.const 0) (i32.const 0)
        (call $at (i32.const 192))) (i32.const 79) (i32.const 15))
      (call $check (i32.load8_u (call $at (i32.const 256))) (i32.const 0) (i32.const 16))
      (local.set $handle (call $load (i32.const 0)))
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 1) (i32.const 1))
      (call $check (call $valid (local.get $handle)) (i32.const 0) (i32.const 2))
      (call $check (call $valid (local.get $handle)) (i32.const 0) (i32.const 3))
      (call $check (call $valid (i32.const 0)) (i32.const 45) (i32.const 4))
      (call $check (call $valid (i32.const 1)) (i32.const 45) (i32.const 5))
      (call $check (call_indirect (type $result) (call $lookup (i32.const 0) (i32.const 28) (i32.const 5))) (i32.const 42) (i32.const 6))
      (call $check (call_indirect (type $result) (call $lookup (i32.const 1) (i32.const 34) (i32.const 10))) (i32.const 7) (i32.const 7))
      (call $check (call $sym (i32.const 1) (call $at (i32.const 28)) (i32.const 5)
        (call $at (i32.const 256)) (i32.const 1) (call $at (i32.const 196))) (i32.const 79) (i32.const 8))
      (call $check (i32.load8_u (call $at (i32.const 256))) (i32.const 0) (i32.const 9))
      (drop (call $load (i32.const 0)))
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 1) (i32.const 12))
    "#,
        "",
        &[],
    );
    assert_eq!(run("imports", &main, &[("side.so", side(1, &[]))]), 0);
}

#[test]
fn dynamic_function_capabilities_use_the_composed_instances_actual_table() {
    let main = main_image(
        r#"
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $invoke (call $lookup (local.get $handle) (i32.const 28) (i32.const 5))
        (i32.const -1) (i32.const 0) (call $at (i32.const 224)) (i32.const 4) (i32.const 1)) (i32.const 0) (i32.const 1))
      (call $check (i32.load (call $at (i32.const 224))) (i32.const 42) (i32.const 2))
      (i32.store8 (call $at (i32.const 232)) (i32.const 255))
      (call $check (call $reflect (call $lookup (local.get $handle) (i32.const 28) (i32.const 5))
        (i32.const -1) (i32.const 0) (call $at (i32.const 232)) (i32.const 1) (call $at (i32.const 240))) (i32.const 0) (i32.const 3))
      (call $check (i32.load8_u (call $at (i32.const 232))) (i32.const 0) (i32.const 4))
      (call $check (i32.load16_u (call $at (i32.const 242))) (i32.const 0) (i32.const 5))
      (call $check (i32.load16_u (call $at (i32.const 244))) (i32.const 1) (i32.const 6))
    "#,
        "",
        &[],
    );
    assert_eq!(run("function-caps", &main, &[("side.so", side(1, &[]))]), 0);
}

#[test]
fn startup_and_recursive_side_constructor_batches_run_once_in_dependency_order() {
    let main = main_image(
        r#"
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 231) (i32.const 1))
      (call $check (call $open (call $at (i32.const 6)) (i32.const 7) (i32.const -1)
        (call $at (i32.const 256)) (i32.const 64) (call $at (i32.const 53)) (i32.const 5)
        (call $at (i32.const 192))) (i32.const 0) (i32.const 10))
      (call $check (i32.load (call $at (i32.const 192))) (i32.const 3) (i32.const 3))
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 231) (i32.const 2))
    "#,
        r#"(if (i32.eq (local.get $n) (i32.const 2)) (then (drop (call $load (i32.const 14)))))"#,
        &["side.so"],
    );
    assert_eq!(
        run(
            "recursive",
            &main,
            &[
                ("side.so", side(1, &["dep.so"])),
                ("dep.so", side(2, &[])),
                ("nest.so", side(3, &[]))
            ]
        ),
        0
    );
}

#[test]
fn a_constructor_can_join_a_peer_which_loads_an_unrelated_library() {
    let main = main_image(
        r#"
      (drop (call $load (i32.const 0)))
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 13) (i32.const 1))
    "#,
        r#"(if (i32.eq (local.get $n) (i32.const 1)) (then
      (call $check (call $spawn (call $at (i32.const 320)) (call $at (i32.const 324))) (i32.const 0) (i32.const 2))
      (call $check (call $join (i32.load (call $at (i32.const 324)))) (i32.const 0) (i32.const 3))))"#,
        &[],
    );
    assert_eq!(
        run(
            "peer-join",
            &main,
            &[("side.so", side(1, &[])), ("nest.so", side(3, &[]))]
        ),
        0
    );
}

#[test]
fn resident_reopen_uses_the_old_name_without_touching_the_unlinked_file() {
    let main = main_image(
        r#"
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $unlink (i32.const 3) (call $at (i32.const 0)) (i32.const 13)) (i32.const 0) (i32.const 1))
      (call $check (call $load (i32.const 0)) (local.get $handle) (i32.const 2))
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 1) (i32.const 3))
    "#,
        "",
        &[],
    );
    assert_eq!(run("unlink", &main, &[("side.so", side(1, &[]))]), 0);
}

#[test]
fn a_late_constructor_trap_keeps_publication_and_never_replays_the_claimed_batch() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use wasixv2::{DirectoryEntry, Host, TokioHost, populate_directory};
    use wasixv2_exec::{Abis, run_with_abis};
    use wasixv2_kernel::{Dispatch, HostClock, Kernel, SyscallEntry};
    use wasixv2_syscalls::{
        Caller, Layer, Layered, LoaderControl, MaybeFuture, SysErr, SysResult, Syscall, SyscallRet,
        Syscalls,
    };
    use wasixv2_vfs_mem::MemFs;
    use wasmer::Store;

    struct Observe(Arc<AtomicBool>);
    impl Layer<Kernel> for Observe {
        fn call<'a>(
            &'a self,
            caller: &'a Caller<'_>,
            call: Syscall,
            next: &'a Kernel,
        ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
            MaybeFuture::Future(Box::pin(async move {
                let ret = Dispatch.call(caller, call, next).await;
                if matches!(ret, Err(SysErr::GuestUnwind)) {
                    let gate = next.loader_gate(caller.task()).unwrap();
                    drop(
                        gate.admit()
                            .expect("a guest constructor trap must not fault the loader image"),
                    );
                    if let Syscall::DlOpen {
                        file,
                        flags,
                        search_path,
                        caller_addr,
                        handle,
                        err,
                    } = call
                    {
                        assert_eq!(
                            next.dl_open(
                                caller,
                                file,
                                flags,
                                search_path,
                                caller_addr,
                                handle,
                                err
                            )
                            .await,
                            Ok(0),
                            "resident reopen must not replay the abandoned constructor batch"
                        );
                        self.0.store(true, Ordering::Release);
                    }
                }
                ret
            }))
        }
    }
    let main = main_image("(drop (call $load (i32.const 0)))", "", &[]);
    // The already published image must remain resident after its ctor traps.
    let trapping = pic(
        r#"(module
      (import "env" "memory" (memory 1 65536 shared))
      (import "env" "__indirect_function_table" (table 0 funcref))
      (import "env" "__memory_base" (global i32))
      (import "env" "__table_base" (global i32))
      (func (export "__wasm_call_ctors") unreachable))"#,
        &[],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, async move {
        let host: Arc<dyn Host> = Arc::new(TokioHost::default());
        let root = Arc::new(MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1));
        populate_directory(
            root.clone(),
            &[
                DirectoryEntry::Directory {
                    path: "".into(),
                    mode: 0o755,
                },
                DirectoryEntry::Directory {
                    path: "prog".into(),
                    mode: 0o755,
                },
                DirectoryEntry::File {
                    path: "prog/side.so".into(),
                    mode: 0o755,
                    data: trapping,
                },
            ],
            1,
            1,
        )
        .await
        .unwrap();
        let kernel = Kernel::new(host.clone(), root);
        let task = kernel
            .spawn(ProcessConfig {
                args: vec!["dynamic".into()],
                env: vec![],
                stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
            })
            .unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let stack = Arc::new(Layered::new(
            SyscallEntry(Observe(observed.clone())),
            kernel,
        ));
        let mut abis = Abis::new();
        abis.push(Arc::new(wasixv2_wasix1::Wasix1::new()));
        let store = Store::default();
        let module = wasixv2_dylink::load(store.engine(), &main).unwrap();
        assert_eq!(
            run_with_abis(store, &module, stack, task, host, Arc::new(abis))
                .await
                .unwrap(),
            132
        );
        assert!(observed.load(Ordering::Acquire));
    });
}

#[test]
fn a_linear_heap_stays_readable_and_disjoint_across_late_loader_growth() {
    let main = main_image(
        r#"
      (local.set $handle (call $load (i32.const 0)))
      (local.set $grown (i32.shl (memory.grow (i32.const 1)) (i32.const 16)))
      (i32.store (local.get $grown) (i32.const 1885431144))
      (i32.store (call $at (i32.const 208)) (local.get $grown))
      (i32.store (call $at (i32.const 212)) (i32.const 4))
      (call $check (call $write (i32.const 1) (call $at (i32.const 208)) (i32.const 1) (call $at (i32.const 216))) (i32.const 0) (i32.const 1))
      (drop (call $load (i32.const 14)))
      (call $check (i32.load (local.get $grown)) (i32.const 1885431144) (i32.const 2))
      (call $check (call_indirect (type $result) (call $lookup (local.get $handle) (i32.const 28) (i32.const 5))) (i32.const 42) (i32.const 3))
      (call $check (i32.load (call $at (i32.const 200))) (i32.const 13) (i32.const 4))
    "#,
        "",
        &[],
    );
    assert_eq!(
        run(
            "linear-growth",
            &main,
            &[("side.so", side(1, &[])), ("nest.so", side(3, &[]))]
        ),
        0
    );
}

#[test]
fn missing_loader_uses_old_unknown_error_before_reading_open_arguments() {
    let image = wat::parse_str(r#"(module
      (import "wasix_32v1" "dlopen" (func $open (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "dlsym" (func $sym (param i32 i32 i32 i32 i32 i32) (result i32)))
      (import "wasix_32v1" "dl_invalid_handle" (func $valid (param i32) (result i32)))
      (import "wasix_32v1" "proc_exit2" (func $exit (param i32)))
      (memory (export "memory") 1)
      (data (i32.const 600) "value")
      (func (export "_start")
        (if (i32.ne (call $open (i32.const -1) (i32.const 4) (i32.const -1)
          (i32.const 256) (i32.const 1) (i32.const -1) (i32.const 4) (i32.const 512)) (i32.const 79)) (then (call $exit (i32.const 1))))
        (if (i32.load8_u (i32.const 256)) (then (call $exit (i32.const 2))))
        (if (i32.ne (call $sym (i32.const 1) (i32.const 600) (i32.const 5)
          (i32.const 256) (i32.const 1) (i32.const 512)) (i32.const 79)) (then (call $exit (i32.const 3))))
        (if (i32.ne (call $valid (i32.const 1)) (i32.const 45)) (then (call $exit (i32.const 4))))))"#).unwrap();
    assert_eq!(run("missing-loader", &image, &[]), 0);
}

#[test]
fn prepared_closure_crosses_the_wasm_boundary_with_scalar_simd_values_and_restores_sp() {
    let body = r#"
      (local $closure i32) (local $sp_before i32)
      (local $i32_result i32) (local $i64_result i64)
      (local $f32_result f32) (local $f64_result f64) (local $v128_result v128)
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $allocate (call $at (i32.const 224))) (i32.const 0) (i32.const 20))
      (local.set $closure (i32.load (call $at (i32.const 224))))
      (call $put_multi_types)
      (call $check
        (call $prepare (global.get $table_base) (local.get $closure)
          (call $at (i32.const 128)) (i32.const 5)
          (call $at (i32.const 136)) (i32.const 5) (i32.const 3))
        (i32.const 0) (i32.const 21))
      (local.set $sp_before (global.get $stack))
      (call_indirect (type $multi)
        (i32.const 17)
        (i64.const 81985529216486895)
        (f32.const 2.5)
        (f64.const -3.25)
        (v128.const i32x4 10 20 30 40)
        (local.get $closure))
      (local.set $v128_result)
      (local.set $f64_result)
      (local.set $f32_result)
      (local.set $i64_result)
      (local.set $i32_result)
      (call $check (local.get $i32_result) (i32.const 18) (i32.const 30))
      (if (i64.ne (local.get $i64_result) (i64.const 81985529216486897))
        (then (call $exit (i32.const 31))))
      (if (f32.ne (local.get $f32_result) (f32.const 5)) (then (call $exit (i32.const 32))))
      (if (f64.ne (local.get $f64_result) (f64.const -2.5)) (then (call $exit (i32.const 33))))
      (call $check (i32x4.extract_lane 0 (local.get $v128_result)) (i32.const 11) (i32.const 34))
      (call $check (i32x4.extract_lane 1 (local.get $v128_result)) (i32.const 21) (i32.const 35))
      (call $check (i32x4.extract_lane 2 (local.get $v128_result)) (i32.const 31) (i32.const 36))
      (call $check (i32x4.extract_lane 3 (local.get $v128_result)) (i32.const 41) (i32.const 37))
      (call $check (global.get $stack) (local.get $sp_before) (i32.const 38))
      (call $exit (i32.const 0))
    "#;
    let main = closure_main(body, CLOSURE_BACKING, "(drop (call $load (i32.const 14)))");
    assert_eq!(run("closure-simd", &main, &[("side.so", side(1, &[]))]), 0);
}

#[test]
fn closures_reprepare_reuse_the_freed_slot_and_trap_when_called_after_free() {
    let body = r#"
      (local $closure i32) (local $reused i32)
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $allocate (call $at (i32.const 224))) (i32.const 0) (i32.const 20))
      (local.set $closure (i32.load (call $at (i32.const 224))))
      (i32.store8 (call $at (i32.const 128)) (i32.const 0))
      (i32.store8 (call $at (i32.const 136)) (i32.const 0))
      (call $check
        (call $prepare (global.get $table_base) (local.get $closure)
          (call $at (i32.const 128)) (i32.const 1)
          (call $at (i32.const 136)) (i32.const 1) (i32.const 1))
        (i32.const 0) (i32.const 21))
      (call $check (call_indirect (type $i32) (i32.const 10) (local.get $closure))
        (i32.const 11) (i32.const 22))
      (call $check (call $free (local.get $closure)) (i32.const 0) (i32.const 23))
      (if (i32.eqz (ref.is_null (table.get (local.get $closure))))
        (then (call $exit (i32.const 24))))
      ;; V1 free is idempotent, and allocation reuses the same logical slot.
      (call $check (call $free (local.get $closure)) (i32.const 0) (i32.const 25))
      (call $check (call $allocate (call $at (i32.const 228))) (i32.const 0) (i32.const 26))
      (local.set $reused (i32.load (call $at (i32.const 228))))
      (call $check (local.get $reused) (local.get $closure) (i32.const 27))
      (call $check
        (call $prepare (global.get $table_base) (local.get $reused)
          (call $at (i32.const 128)) (i32.const 1)
          (call $at (i32.const 136)) (i32.const 1) (i32.const 1))
        (i32.const 0) (i32.const 28))
      (call $check (call_indirect (type $i32) (i32.const 20) (local.get $reused))
        (i32.const 21) (i32.const 29))
      (call $check (call $free (local.get $reused)) (i32.const 0) (i32.const 30))
      ;; A null closure slot follows the engine's normal indirect-call trap path.
      (drop (call_indirect (type $i32) (i32.const 1) (local.get $reused)))
      (call $exit (i32.const 31))
    "#;
    let main = closure_main(body, CLOSURE_BACKING, "(drop (call $load (i32.const 14)))");
    assert_eq!(
        run("closure-reuse", &main, &[("side.so", side(1, &[]))]),
        139 // Calls through null pointers follow the existing SIGSEGV path.
    );
}

#[test]
fn closure_callback_can_reprepare_its_own_slot_without_losing_the_captured_binding() {
    let body = r#"
      (local $closure i32)
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $allocate (call $at (i32.const 224))) (i32.const 0) (i32.const 20))
      (local.set $closure (i32.load (call $at (i32.const 224))))
      (i32.store8 (call $at (i32.const 128)) (i32.const 0))
      (i32.store8 (call $at (i32.const 136)) (i32.const 0))
      (call $check
        (call $prepare (global.get $table_base) (local.get $closure)
          (call $at (i32.const 128)) (i32.const 1)
          (call $at (i32.const 136)) (i32.const 1) (i32.const 4))
        (i32.const 0) (i32.const 21))
      (call $check (call_indirect (type $i32) (i32.const 5) (local.get $closure))
        (i32.const 45) (i32.const 22))
      ;; The callback changed its descriptor to env=1; the next call uses that binding.
      (call $check (call_indirect (type $i32) (i32.const 5) (local.get $closure))
        (i32.const 6) (i32.const 23))
      ;; Free from inside the callback must leave its in-flight call intact.
      (call $check
        (call $prepare (global.get $table_base) (local.get $closure)
          (call $at (i32.const 128)) (i32.const 1)
          (call $at (i32.const 136)) (i32.const 1) (i32.const 5))
        (i32.const 0) (i32.const 24))
      (call $check (call_indirect (type $i32) (i32.const 5) (local.get $closure))
        (i32.const 55) (i32.const 25))
      (if (i32.eqz (ref.is_null (table.get (local.get $closure))))
        (then (call $exit (i32.const 26))))
      (call $exit (i32.const 0))
    "#;
    let main = closure_main(body, CLOSURE_BACKING, "(drop (call $load (i32.const 14)))");
    assert_eq!(
        run("closure-reentry", &main, &[("side.so", side(1, &[]))]),
        0
    );
}

#[test]
fn closure_rethrows_the_same_wasm_exception_after_restoring_shadow_sp() {
    let extra_code = r#"
      (tag $closure_error (param i32))
      (type $i32 (func (param i32) (result i32)))
      (func $backing (param i32 i32 i32)
        (throw $closure_error (i32.const 44)))
      (elem (global.get $table_base) $backing)
    "#;
    let body = r#"
      (local $closure i32) (local $sp_before i32) (local $caught i32)
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $allocate (call $at (i32.const 224))) (i32.const 0) (i32.const 20))
      (local.set $closure (i32.load (call $at (i32.const 224))))
      (i32.store8 (call $at (i32.const 128)) (i32.const 0))
      (i32.store8 (call $at (i32.const 136)) (i32.const 0))
      (call $check
        (call $prepare (global.get $table_base) (local.get $closure)
          (call $at (i32.const 128)) (i32.const 1)
          (call $at (i32.const 136)) (i32.const 1) (i32.const 0))
        (i32.const 0) (i32.const 21))
      (local.set $sp_before (global.get $stack))
      (local.set $caught
        (block $caught (result i32)
          (try_table (catch $closure_error $caught)
            (drop (call_indirect (type $i32) (i32.const 7) (local.get $closure))))
          (i32.const -1)))
      (call $check (local.get $caught) (i32.const 44) (i32.const 22))
      (call $check (global.get $stack) (local.get $sp_before) (i32.const 23))
      (call $exit (i32.const 0))
    "#;
    let main = closure_main(body, extra_code, "(drop (call $load (i32.const 14)))");
    assert_eq!(run("closure-eh", &main, &[("side.so", side(1, &[]))]), 0);
}

#[test]
fn a_thread_created_after_prepare_gets_the_closure_in_its_store() {
    let body = r#"
      (local $closure i32)
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $allocate (call $at (i32.const 224))) (i32.const 0) (i32.const 20))
      (local.set $closure (i32.load (call $at (i32.const 224))))
      (i32.store8 (call $at (i32.const 128)) (i32.const 0))
      (i32.store8 (call $at (i32.const 136)) (i32.const 0))
      (call $check
        (call $prepare (global.get $table_base) (local.get $closure)
          (call $at (i32.const 128)) (i32.const 1)
          (call $at (i32.const 136)) (i32.const 1) (i32.const 1))
        (i32.const 0) (i32.const 21))
      (call $check (call_indirect (type $i32) (i32.const 10) (local.get $closure))
        (i32.const 11) (i32.const 22))
      (i32.store (call $at (i32.const 240)) (local.get $closure))
      (i32.store (call $at (i32.const 248)) (i32.const 0))
      (call $check (call $spawn (call $at (i32.const 240)) (call $at (i32.const 244)))
        (i32.const 0) (i32.const 23))
      (call $check (call $join (i32.load (call $at (i32.const 244)))) (i32.const 0) (i32.const 24))
      (call $check (i32.load (call $at (i32.const 248))) (i32.const 1) (i32.const 25))
      (call $exit (i32.const 0))
    "#;
    let thread_body = r#"
      (call $check
        (call_indirect (type $i32) (i32.const 41) (i32.load (local.get 1)))
        (i32.const 42) (i32.const 80))
      (i32.store (call $at (i32.const 248)) (i32.const 1))
    "#;
    let main = closure_main(body, CLOSURE_BACKING, thread_body);
    assert_eq!(
        run("closure-thread", &main, &[("side.so", side(1, &[]))]),
        0
    );
}

#[test]
fn a_layer_denied_closure_free_einval_is_not_normalized_as_an_unknown_id() {
    use std::sync::Arc;
    use wasixv2::{DirectoryEntry, Host, TokioHost, populate_directory};
    use wasixv2_exec::{Abis, run_with_abis};
    use wasixv2_kernel::{Dispatch, HostClock, Kernel, SyscallEntry};
    use wasixv2_syscalls::{
        Caller, Errno, Layer, Layered, MaybeFuture, SysErr, SysResult, Syscall, SyscallRet,
    };
    use wasixv2_vfs_mem::MemFs;
    use wasmer::Store;

    struct DenyFree;
    impl Layer<Kernel> for DenyFree {
        fn call<'a>(
            &'a self,
            caller: &'a Caller<'_>,
            call: Syscall,
            next: &'a Kernel,
        ) -> MaybeFuture<'a, SysResult<SyscallRet>> {
            MaybeFuture::Future(Box::pin(async move {
                match call {
                    Syscall::ClosureFree { .. } => Err(SysErr::Errno(Errno::EINVAL)),
                    call => Dispatch.call(caller, call, next).await,
                }
            }))
        }
    }

    let body = r#"
      (local.set $handle (call $load (i32.const 0)))
      (call $check (call $free (i32.const 99)) (i32.const 28) (i32.const 90))
      (call $exit (i32.const 0))
    "#;
    let main = closure_main(body, "", "(drop (call $load (i32.const 14)))");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, async {
        let host: Arc<dyn Host> = Arc::new(TokioHost::default());
        let root = Arc::new(MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1));
        populate_directory(
            root.clone(),
            &[
                DirectoryEntry::Directory {
                    path: "".into(),
                    mode: 0o755,
                },
                DirectoryEntry::Directory {
                    path: "prog".into(),
                    mode: 0o755,
                },
                DirectoryEntry::File {
                    path: "prog/side.so".into(),
                    mode: 0o755,
                    data: side(1, &[]),
                },
            ],
            1,
            1,
        )
        .await
        .unwrap();
        let kernel = Kernel::new(host.clone(), root);
        let task = kernel
            .spawn(ProcessConfig {
                args: vec!["dynamic".into()],
                env: vec!["LD_LIBRARY_PATH=/prog".into()],
                stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
            })
            .unwrap();
        let stack = Arc::new(Layered::new(SyscallEntry(DenyFree), kernel));
        let mut abis = Abis::new();
        abis.push(Arc::new(wasixv2_wasix1::Wasix1::new()));
        let store = Store::default();
        let module = wasixv2_dylink::load(store.engine(), &main).unwrap();
        assert_eq!(
            run_with_abis(store, &module, stack, task, host, Arc::new(abis))
                .await
                .unwrap(),
            0
        );
    });
}
