/// Reproducer for https://github.com/wasmerio/wasmer/issues/6793 and
/// https://github.com/wasmerio/wasmer/issues/6795
///
/// Before the fix: the native stack overflows inside wasmer_vm_throw /
///   eh::gcc::throw, causing a stack-buffer-overflow (ASan) or a kernel
///   panic / system crash (bare metal, no ASan).
/// After the fix: the runtime raises a clean StackOverflow trap instead.
///
/// Run with:
///   cargo run --example repro-6795 --features cranelift --release
///
/// ## Stack size
///
/// This reproducer uses a **256 KiB** coroutine stack.  The original issue
/// used the default 1 MiB stack; the bug is identical on both sizes.  We use
/// 256 KiB because DWARF-based stack unwinding is O(n) per throw in the
/// number of live frames, making deep recursion on a 1 MiB stack very slow
/// (~30 min to overflow).  On 256 KiB the headroom check fires in ~7 seconds.
///
/// ## Why bounded recursion (not infinite)
///
/// The original issue WAT used `(func $main ... call $main)` — infinite.
/// Cranelift in release mode detects that a function with no base case never
/// returns and optimises the tail call into a flat native loop (constant
/// stack depth).  This means the headroom check never fires for infinite WAT,
/// not because the fix is wrong, but because there are no accumulating frames.
///
/// With a base case (`$remaining == 0 → return`), Cranelift must preserve
/// each caller's frame so it can resume after the recursive call.  Frames
/// accumulate on the Wasm coroutine stack until the headroom check in
/// `eh::gcc::throw` fires and raises a clean StackOverflow trap.
///
/// ## What the fix does
///
/// Before entering the native unwinder (`_Unwind_RaiseException`), the
/// `eh::gcc::throw` function reads the current stack pointer and the
/// coroutine stack limit (both available in TLS).  If fewer than
/// `UNWIND_STACK_HEADROOM` (64 KiB) bytes remain, it raises a clean
/// StackOverflow trap instead of letting the unwinder corrupt memory.

use wasmer::{Instance, Module, Store, Value, imports, wat2wasm};

/// Upper bound on recursion depth.  The Wasm coroutine stack is 256 KiB and
/// each frame is well above 1 byte, so exhaustion happens far below this.
/// 10_000 is chosen to be clearly above the realistic frame budget while
/// still being finite so the process cannot loop forever.
const MAX_DEPTH: i32 = 10_000;

/// Coroutine stack size in bytes.  256 KiB is small enough to overflow in
/// ~7 seconds but large enough for the compiler + EH runtime frames.
const STACK_SIZE: usize = 256 * 1024;

/// The WAT module — bounded recursive countdown with EH throw/catch.
///
/// $recurse(n): builds a chain of n catch_ref / throw_ref frames.
///   when n == 0 -> throws $e-depth
///   otherwise   -> catch_ref the inner call, then throw_ref it
///
/// $wrap(n):  top-level catch_all_ref around $recurse(n); returns 0.
///
/// $main(remaining): bounded recursive countdown.
///   Calls $wrap(3) at each level, then recurses with (remaining - 1).
///   Frames accumulate on the coroutine stack until it overflows.
const WAT: &str = r#"
(module
  (type $i32_exnref (func (result i32 exnref)))
  (tag $e-depth (param i32))
  (export "main" (func $main))

  (func $recurse (param $n i32)
    local.get $n
    i32.eqz
    if
      i32.const 0
      throw $e-depth
    end
    block $h (type $i32_exnref) (result i32 exnref)
      try_table (type $i32_exnref) (result i32 exnref) (catch_ref $e-depth $h)
        local.get $n
        i32.const 1
        i32.sub
        call $recurse
        unreachable
      end
    end
    throw_ref
  )

  (func $wrap (param i32) (result i32)
    block $top (result exnref)
      try_table (result exnref) (catch_all_ref $top)
        local.get 0
        call $recurse
        unreachable
      end
    end
    drop
    i32.const 0
  )

  ;; Bounded recursive countdown.
  ;; Each level calls $wrap(3) (exercises the full EH chain) then recurses.
  ;; Frames accumulate on the Wasm coroutine stack until the headroom check
  ;; fires and raises a clean StackOverflow trap.
  (func $main (param $remaining i32)
    local.get $remaining
    i32.eqz
    if return end
    i32.const 3
    call $wrap
    drop
    local.get $remaining
    i32.const 1
    i32.sub
    call $main
  )
)
"#;

fn main() {
    println!("=== repro_6795: EH deep-recursion stack-overflow test ===\n");
    println!("Reproducer for issues #6793 / #6795.");
    println!("Stack size = {STACK_SIZE} bytes ({} KiB)", STACK_SIZE / 1024);
    println!("Calling $main({MAX_DEPTH}) -- trap fires long before counter reaches 0.\n");

    // Use a 256 KiB stack for fast execution.
    wasmer_vm::set_stack_size(STACK_SIZE);

    let mut store = Store::default();

    let wasm = match wat2wasm(WAT.as_bytes()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("ERROR: wat2wasm failed: {e}");
            std::process::exit(1);
        }
    };

    let module = match Module::new(&store, wasm) {
        Ok(m) => m,
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("exceptions proposal not enabled") || msg.contains("not supported") {
                println!("SKIP: exceptions proposal not enabled in this build.");
                return;
            }
            eprintln!("ERROR: compile failed: {msg}");
            std::process::exit(1);
        }
    };

    let instance = Instance::new(&mut store, &module, &imports! {})
        .expect("instantiate failed");
    let main_fn = instance.exports.get_function("main")
        .expect("export 'main' not found");

    match main_fn.call(&mut store, &[Value::I32(MAX_DEPTH)]) {
        Err(e) => {
            let msg = e.message();
            println!("TRAP: {msg}");
            if msg.contains("call stack exhausted")
                || msg.contains("out of stack space")
                || msg.contains("stack overflow")
            {
                println!("\nFix is working -- deep EH recursion traps cleanly with StackOverflow.");
                println!("Before the fix this would crash/freeze the entire system.");
            } else {
                eprintln!("\nUnexpected trap: {msg}");
                std::process::exit(1);
            }
        }
        Ok(results) if results.is_empty() => {
            eprintln!("ERROR: $main returned normally after {MAX_DEPTH} levels.");
            eprintln!("No trap fired -- increase MAX_DEPTH or check that the fix is applied.");
            std::process::exit(1);
        }
        Ok(v) => {
            eprintln!("ERROR: expected trap, got: {v:?}");
            std::process::exit(1);
        }
    }
}
