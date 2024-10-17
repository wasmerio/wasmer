;; Test globals

(module
  (global (import "spectest" "global_i32") i32)
  (global (import "spectest" "global_i64") i64)

  (global $a i32 (i32.const -2))
  (global (;3;) f32 (f32.const -3))
  (global (;4;) f64 (f64.const -4))
  (global $b i64 (i64.const -5))

  (global $x (mut i32) (i32.const -12))
  (global (;7;) (mut f32) (f32.const -13))
  (global (;8;) (mut f64) (f64.const -14))
  (global $y (mut i64) (i64.const -15))

  (global $z1 i32 (global.get 0))
  (global $z2 i64 (global.get 1))

  (global $r externref (ref.null extern))
  (global $mr (mut externref) (ref.null extern))
  (global funcref (ref.null func))

  (func (export "get-a") (result i32) (global.get $a))
  (func (export "get-b") (result i64) (global.get $b))
  (func (export "get-r") (result externref) (global.get $r))
  (func (export "get-mr") (result externref) (global.get $mr))
  (func (export "get-x") (result i32) (global.get $x))
  (func (export "get-y") (result i64) (global.get $y))
  (func (export "get-z1") (result i32) (global.get $z1))
  (func (export "get-z2") (result i64) (global.get $z2))
  (func (export "set-x") (param i32) (global.set $x (local.get 0)))
  (func (export "set-y") (param i64) (global.set $y (local.get 0)))
  (func (export "set-mr") (param externref) (global.set $mr (local.get 0)))

  (func (export "get-3") (result f32) (global.get 3))
  (func (export "get-4") (result f64) (global.get 4))
  (func (export "get-7") (result f32) (global.get 7))
  (func (export "get-8") (result f64) (global.get 8))
  (func (export "set-7") (param f32) (global.set 7 (local.get 0)))
  (func (export "set-8") (param f64) (global.set 8 (local.get 0)))

  ;; As the argument of control constructs and instructions

  (memory 1)

  (func $dummy)

  (func (export "as-select-first") (result i32)
    (select (global.get $x) (i32.const 2) (i32.const 3))
  )
  (func (export "as-select-mid") (result i32)
    (select (i32.const 2) (global.get $x) (i32.const 3))
  )
  (func (export "as-select-last") (result i32)
    (select (i32.const 2) (i32.const 3) (global.get $x))
  )

  (func (export "as-loop-first") (result i32)
    (loop (result i32)
      (global.get $x) (call $dummy) (call $dummy)
    )
  )
  (func (export "as-loop-mid") (result i32)
    (loop (result i32)
      (call $dummy) (global.get $x) (call $dummy)
    )
  )
  (func (export "as-loop-last") (result i32)
    (loop (result i32)
      (call $dummy) (call $dummy) (global.get $x)
    )
  )

  (func (export "as-if-condition") (result i32)
    (if (result i32) (global.get $x)
      (then (call $dummy) (i32.const 2))
      (else (call $dummy) (i32.const 3))
    )
  )
  (func (export "as-if-then") (result i32)
    (if (result i32) (i32.const 1)
      (then (global.get $x)) (else (i32.const 2))
    )
  )
  (func (export "as-if-else") (result i32)
    (if (result i32) (i32.const 0)
      (then (i32.const 2)) (else (global.get $x))
    )
  )

  (func (export "as-br_if-first") (result i32)
    (block (result i32)
      (br_if 0 (global.get $x) (i32.const 2))
      (return (i32.const 3))
    )
  )
  (func (export "as-br_if-last") (result i32)
    (block (result i32)
      (br_if 0 (i32.const 2) (global.get $x))
      (return (i32.const 3))
    )
  )

  (func (export "as-br_table-first") (result i32)
    (block (result i32)
      (global.get $x) (i32.const 2) (br_table 0 0)
    )
  )
  (func (export "as-br_table-last") (result i32)
    (block (result i32)
      (i32.const 2) (global.get $x) (br_table 0 0)
    )
  )

  (func $func (param i32 i32) (result i32) (local.get 0))
  (type $check (func (param i32 i32) (result i32)))
  (table funcref (elem $func))
  (func (export "as-call_indirect-first") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (global.get $x) (i32.const 2) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-mid") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2) (global.get $x) (i32.const 0)
      )
    )
  )
 (func (export "as-call_indirect-last") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2) (i32.const 0) (global.get $x)
      )
    )
  )

  (func (export "as-store-first")
    (global.get $x) (i32.const 1) (i32.store)
  )
  (func (export "as-store-last")
    (i32.const 0) (global.get $x) (i32.store)
  )
  (func (export "as-load-operand") (result i32)
    (i32.load (global.get $x))
  )
  (func (export "as-memory.grow-value") (result i32)
    (memory.grow (global.get $x))
  )

  (func $f (param i32) (result i32) (local.get 0))
  (func (export "as-call-value") (result i32)
    (call $f (global.get $x))
  )

  (func (export "as-return-value") (result i32)
    (global.get $x) (return)
  )
  (func (export "as-drop-operand")
    (drop (global.get $x))
  )
  (func (export "as-br-value") (result i32)
    (block (result i32) (br 0 (global.get $x)))
  )

  (func (export "as-local.set-value") (param i32) (result i32)
    (local.set 0 (global.get $x))
    (local.get 0)
  )
  (func (export "as-local.tee-value") (param i32) (result i32)
    (local.tee 0 (global.get $x))
  )
  (func (export "as-global.set-value") (result i32)
    (global.set $x (global.get $x))
    (global.get $x)
  )

  (func (export "as-unary-operand") (result i32)
    (i32.eqz (global.get $x))
  )
  (func (export "as-binary-operand") (result i32)
    (i32.mul
      (global.get $x) (global.get $x)
    )
  )
  (func (export "as-compare-operand") (result i32)
    (i32.gt_u
      (global.get 0) (i32.const 1)
    )
  )
)

(assert_return (invoke "get-a") (i32.const -2))
(assert_return (invoke "get-b") (i64.const -5))
(assert_return (invoke "get-r") (ref.null extern))
(assert_return (invoke "get-mr") (ref.null extern))
(assert_return (invoke "get-x") (i32.const -12))
(assert_return (invoke "get-y") (i64.const -15))
(assert_return (invoke "get-z1") (i32.const 666))
(assert_return (invoke "get-z2") (i64.const 666))

(assert_return (invoke "get-3") (f32.const -3))
(assert_return (invoke "get-4") (f64.const -4))
(assert_return (invoke "get-7") (f32.const -13))
(assert_return (invoke "get-8") (f64.const -14))

(assert_return (invoke "set-x" (i32.const 6)))
(assert_return (invoke "set-y" (i64.const 7)))

(assert_return (invoke "set-7" (f32.const 8)))
(assert_return (invoke "set-8" (f64.const 9)))

(assert_return (invoke "get-x") (i32.const 6))
(assert_return (invoke "get-y") (i64.const 7))
(assert_return (invoke "get-7") (f32.const 8))
(assert_return (invoke "get-8") (f64.const 9))

(assert_return (invoke "set-7" (f32.const 8)))
(assert_return (invoke "set-8" (f64.const 9)))
(assert_return (invoke "set-mr" (ref.extern 10)))

(assert_return (invoke "get-x") (i32.const 6))
(assert_return (invoke "get-y") (i64.const 7))
(assert_return (invoke "get-7") (f32.const 8))
(assert_return (invoke "get-8") (f64.const 9))
(assert_return (invoke "get-mr") (ref.extern 10))

(assert_return (invoke "as-select-first") (i32.const 6))
(assert_return (invoke "as-select-mid") (i32.const 2))
(assert_return (invoke "as-select-last") (i32.const 2))

(assert_return (invoke "as-loop-first") (i32.const 6))
(assert_return (invoke "as-loop-mid") (i32.const 6))
(assert_return (invoke "as-loop-last") (i32.const 6))

(assert_return (invoke "as-if-condition") (i32.const 2))
(assert_return (invoke "as-if-then") (i32.const 6))
(assert_return (invoke "as-if-else") (i32.const 6))

(assert_return (invoke "as-br_if-first") (i32.const 6))
(assert_return (invoke "as-br_if-last") (i32.const 2))

(assert_return (invoke "as-br_table-first") (i32.const 6))
(assert_return (invoke "as-br_table-last") (i32.const 2))

(assert_return (invoke "as-call_indirect-first") (i32.const 6))
(assert_return (invoke "as-call_indirect-mid") (i32.const 2))
(assert_trap (invoke "as-call_indirect-last") "undefined element")

(assert_return (invoke "as-store-first"))
(assert_return (invoke "as-store-last"))
(assert_return (invoke "as-load-operand") (i32.const 1))
(assert_return (invoke "as-memory.grow-value") (i32.const 1))

(assert_return (invoke "as-call-value") (i32.const 6))

(assert_return (invoke "as-return-value") (i32.const 6))
(assert_return (invoke "as-drop-operand"))
(assert_return (invoke "as-br-value") (i32.const 6))

(assert_return (invoke "as-local.set-value" (i32.const 1)) (i32.const 6))
(assert_return (invoke "as-local.tee-value" (i32.const 1)) (i32.const 6))
(assert_return (invoke "as-global.set-value") (i32.const 6))

(assert_return (invoke "as-unary-operand") (i32.const 0))
(assert_return (invoke "as-binary-operand") (i32.const 36))
(assert_return (invoke "as-compare-operand") (i32.const 1))

(assert_invalid
  (module (global f32 (f32.const 0)) (func (global.set 0 (f32.const 1))))
  "global is immutable"
)

(assert_invalid
  (module (import "spectest" "global_i32" (global i32)) (func (global.set 0 (i32.const 1))))
  "global is immutable"
)

;; mutable globals can be exported
(module (global (mut f32) (f32.const 0)) (export "a" (global 0)))
(module (global (export "a") (mut f32) (f32.const 0)))

(assert_invalid
  (module (global f32 (f32.neg (f32.const 0))))
  "constant expression required"
)

(assert_invalid
  (module (global f32 (local.get 0)))
  "constant expression required"
)

(assert_invalid
  (module (global f32 (f32.neg (f32.const 1))))
  "constant expression required"
)

(assert_invalid
  (module (global i32 (i32.const 0) (nop)))
  "constant expression required"
)

(assert_invalid
  (module (global i32 (i32.ctz (i32.const 0))))
  "constant expression required"
)

(assert_invalid
  (module (global i32 (nop)))
  "constant expression required"
)

(assert_invalid
  (module (global i32 (f32.const 0)))
  "type mismatch"
)

(assert_invalid
  (module (global i32 (i32.const 0) (i32.const 0)))
  "type mismatch"
)

(assert_invalid
  (module (global i32 (;empty instruction sequence;)))
  "type mismatch"
)

(assert_invalid
  (module (global (import "" "") externref) (global funcref (global.get 0)))
  "type mismatch"
)

(assert_invalid
  (module (global (import "test" "global-i32") i32) (global i32 (global.get 0) (global.get 0)))
  "type mismatch"
)

(assert_invalid
  (module (global (import "test" "global-i32") i32) (global i32 (i32.const 0) (global.get 0)))
  "type mismatch"
)

(assert_invalid
  (module (global i32 (global.get 0)))
  "unknown global"
)

(assert_invalid
  (module (global i32 (i32.const 0)) (global i32 (global.get 0)))
  "unknown global"
)
(assert_invalid
  (module (global $g i32 (i32.const 0)) (global i32 (global.get $g)))
  "unknown global"
)

(assert_invalid
  (module (global i32 (global.get 1)) (global i32 (i32.const 0)))
  "unknown global"
)

(assert_invalid
  (module (global (import "test" "global-i32") i32) (global i32 (global.get 2)))
  "unknown global"
)

(assert_invalid
  (module (global (import "test" "global-mut-i32") (mut i32)) (global i32 (global.get 0)))
  "constant expression required"
)

(module
  (import "spectest" "global_i32" (global i32))
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\98\80\80\80\00"             ;; import section
      "\01"                          ;; length 1
      "\08\73\70\65\63\74\65\73\74"  ;; "spectest"
      "\0a\67\6c\6f\62\61\6c\5f\69\33\32" ;; "global_i32"
      "\03"                          ;; GlobalImport
      "\7f"                          ;; i32
      "\02"                          ;; malformed mutability
  )
  "malformed mutability"
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\98\80\80\80\00"             ;; import section
      "\01"                          ;; length 1
      "\08\73\70\65\63\74\65\73\74"  ;; "spectest"
      "\0a\67\6c\6f\62\61\6c\5f\69\33\32" ;; "global_i32"
      "\03"                          ;; GlobalImport
      "\7f"                          ;; i32
      "\ff"                          ;; malformed mutability
  )
  "malformed mutability"
)

(module
  (global i32 (i32.const 0))
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\06\86\80\80\80\00"  ;; global section
      "\01"               ;; length 1
      "\7f"               ;; i32
      "\02"               ;; malformed mutability
      "\41\00"            ;; i32.const 0
      "\0b"               ;; end
  )
  "malformed mutability"
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\06\86\80\80\80\00"  ;; global section
      "\01"               ;; length 1
      "\7f"               ;; i32
      "\ff"               ;; malformed mutability
      "\41\00"            ;; i32.const 0
      "\0b"               ;; end
  )
  "malformed mutability"
)

;; global.get with invalid index
(assert_invalid
  (module (func (result i32) (global.get 0)))
  "unknown global"
)

(assert_invalid
  (module
    (global i32 (i32.const 0))
    (func (result i32) (global.get 1))
  )
  "unknown global"
)

(assert_invalid
  (module
    (import "spectest" "global_i32" (global i32))
    (func (result i32) (global.get 1))
  )
  "unknown global"
)

(assert_invalid
  (module
    (import "spectest" "global_i32" (global i32))
    (global i32 (i32.const 0))
    (func (result i32) (global.get 2))
  )
  "unknown global"
)

;; global.set with invalid index
(assert_invalid
  (module (func (i32.const 0) (global.set 0)))
  "unknown global"
)

(assert_invalid
  (module
    (global i32 (i32.const 0))
    (func (i32.const 0) (global.set 1))
  )
  "unknown global"
)

(assert_invalid
  (module
    (import "spectest" "global_i32" (global i32))
    (func (i32.const 0) (global.set 1))
  )
  "unknown global"
)

(assert_invalid
  (module
    (import "spectest" "global_i32" (global i32))
    (global i32 (i32.const 0))
    (func (i32.const 0) (global.set 2))
  )
  "unknown global"
)


(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty
      (global.set $x)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-block
      (i32.const 0)
      (block (global.set $x))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-loop
      (i32.const 0)
      (loop (global.set $x))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-then
      (i32.const 0) (i32.const 0)
      (if (then (global.set $x)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-else
      (i32.const 0) (i32.const 0)
      (if (result i32) (then (i32.const 0)) (else (global.set $x)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-br
      (i32.const 0)
      (block (br 0 (global.set $x)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-br_if
      (i32.const 0)
      (block (br_if 0 (global.set $x)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-br_table
      (i32.const 0)
      (block (br_table 0 (global.set $x)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-return
      (return (global.set $x))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-select
      (select (global.set $x) (i32.const 1) (i32.const 2))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-global.set-value-empty-in-call
      (call 1 (global.set $x))
    )
    (func (param i32) (result i32) (local.get 0))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $f (param i32) (result i32) (local.get 0))
    (type $sig (func (param i32) (result i32)))
    (table funcref (elem $f))
    (func $type-global.set-value-empty-in-call_indirect
      (block (result i32)
        (call_indirect (type $sig)
          (global.set $x) (i32.const 0)
        )
      )
    )
  )
  "type mismatch"
)

;; Duplicate identifier errors

(assert_malformed (module quote
  "(global $foo i32 (i32.const 0))"
  "(global $foo i32 (i32.const 0))")
  "duplicate global")
(assert_malformed (module quote
  "(import \"\" \"\" (global $foo i32))"
  "(global $foo i32 (i32.const 0))")
  "duplicate global")
(assert_malformed (module quote
  "(import \"\" \"\" (global $foo i32))"
  "(import \"\" \"\" (global $foo i32))")
  "duplicate global")
