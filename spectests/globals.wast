
;; Test globals

(module
  (global $a i32 (i32.const -2))
  (global (;1;) f32 (f32.const -3))
  (global (;2;) f64 (f64.const -4))
  (global $b i64 (i64.const -5))

  (global $x (mut i32) (i32.const -12))
  (global (;5;) (mut f32) (f32.const -13))
  (global (;6;) (mut f64) (f64.const -14))
  (global $y (mut i64) (i64.const -15))

  (func (export "get-a") (result i32) (get_global $a))
  (func (export "get-b") (result i64) (get_global $b))
  (func (export "get-x") (result i32) (get_global $x))
  (func (export "get-y") (result i64) (get_global $y))
  (func (export "set-x") (param i32) (set_global $x (get_local 0)))
  (func (export "set-y") (param i64) (set_global $y (get_local 0)))

  (func (export "get-1") (result f32) (get_global 1))
  (func (export "get-2") (result f64) (get_global 2))
  (func (export "get-5") (result f32) (get_global 5))
  (func (export "get-6") (result f64) (get_global 6))
  (func (export "set-5") (param f32) (set_global 5 (get_local 0)))
  (func (export "set-6") (param f64) (set_global 6 (get_local 0)))

  ;; As the argument of control constructs and instructions

  (memory 1)

  (func $dummy)

  (func (export "as-select-first") (result i32)
    (select (get_global $x) (i32.const 2) (i32.const 3))
  )
  (func (export "as-select-mid") (result i32)
    (select (i32.const 2) (get_global $x) (i32.const 3))
  )
  (func (export "as-select-last") (result i32)
    (select (i32.const 2) (i32.const 3) (get_global $x))
  )

  (func (export "as-loop-first") (result i32)
    (loop (result i32)
      (get_global $x) (call $dummy) (call $dummy)
    )
  )
  (func (export "as-loop-mid") (result i32)
    (loop (result i32)
      (call $dummy) (get_global $x) (call $dummy)
    )
  )
  (func (export "as-loop-last") (result i32)
    (loop (result i32)
      (call $dummy) (call $dummy) (get_global $x)
    )
  )

  (func (export "as-if-condition") (result i32)
    (if (result i32) (get_global $x)
      (then (call $dummy) (i32.const 2))
      (else (call $dummy) (i32.const 3))
    )
  )
  (func (export "as-if-then") (result i32)
    (if (result i32) (i32.const 1)
      (then (get_global $x)) (else (i32.const 2))
    )
  )
  (func (export "as-if-else") (result i32)
    (if (result i32) (i32.const 0)
      (then (i32.const 2)) (else (get_global $x))
    )
  )

  (func (export "as-br_if-first") (result i32)
    (block (result i32)
      (br_if 0 (get_global $x) (i32.const 2))
      (return (i32.const 3))
    )
  )
  (func (export "as-br_if-last") (result i32)
    (block (result i32)
      (br_if 0 (i32.const 2) (get_global $x))
      (return (i32.const 3))
    )
  )

  (func (export "as-br_table-first") (result i32)
    (block (result i32)
      (get_global $x) (i32.const 2) (br_table 0 0)
    )
  )
  (func (export "as-br_table-last") (result i32)
    (block (result i32)
      (i32.const 2) (get_global $x) (br_table 0 0)
    )
  )

  (func $func (param i32 i32) (result i32) (get_local 0))
  (type $check (func (param i32 i32) (result i32)))
  (table anyfunc (elem $func))
  (func (export "as-call_indirect-first") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (get_global $x) (i32.const 2) (i32.const 0)
      )
    )
  )
  (func (export "as-call_indirect-mid") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2) (get_global $x) (i32.const 0)
      )
    )
  )
 (func (export "as-call_indirect-last") (result i32)
    (block (result i32)
      (call_indirect (type $check)
        (i32.const 2) (i32.const 0) (get_global $x)
      )
    )
  )

  (func (export "as-store-first")
    (get_global $x) (i32.const 1) (i32.store)
  )
  (func (export "as-store-last")
    (i32.const 0) (get_global $x) (i32.store)
  )
  (func (export "as-load-operand") (result i32)
    (i32.load (get_global $x))
  )
  (func (export "as-memory.grow-value") (result i32)
    (memory.grow (get_global $x))
  )

  (func $f (param i32) (result i32) (get_local 0))
  (func (export "as-call-value") (result i32)
    (call $f (get_global $x))
  )

  (func (export "as-return-value") (result i32)
    (get_global $x) (return)
  )
  (func (export "as-drop-operand")
    (drop (get_global $x))
  )
  (func (export "as-br-value") (result i32)
    (block (result i32) (br 0 (get_global $x)))
  )

  (func (export "as-set_local-value") (param i32) (result i32)
    (set_local 0 (get_global $x))
    (get_local 0)
  )
  (func (export "as-tee_local-value") (param i32) (result i32)
    (tee_local 0 (get_global $x))
  )
  (func (export "as-set_global-value") (result i32)
    (set_global $x (get_global $x))
    (get_global $x)
  )

  (func (export "as-unary-operand") (result i32)
    (i32.eqz (get_global $x))
  )
  (func (export "as-binary-operand") (result i32)
    (i32.mul
      (get_global $x) (get_global $x)
    )
  )
  (func (export "as-compare-operand") (result i32)
    (i32.gt_u
      (get_global 0) (i32.const 1)
    )
  )
)

(assert_return (invoke "get-a") (i32.const -2))
(assert_return (invoke "get-b") (i64.const -5))
(assert_return (invoke "get-x") (i32.const -12))
(assert_return (invoke "get-y") (i64.const -15))

(assert_return (invoke "get-1") (f32.const -3))
(assert_return (invoke "get-2") (f64.const -4))
(assert_return (invoke "get-5") (f32.const -13))
(assert_return (invoke "get-6") (f64.const -14))

(assert_return (invoke "set-x" (i32.const 6)))
(assert_return (invoke "set-y" (i64.const 7)))
(assert_return (invoke "set-5" (f32.const 8)))
(assert_return (invoke "set-6" (f64.const 9)))

(assert_return (invoke "get-x") (i32.const 6))
(assert_return (invoke "get-y") (i64.const 7))
(assert_return (invoke "get-5") (f32.const 8))
(assert_return (invoke "get-6") (f64.const 9))

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

(assert_return (invoke "as-set_local-value" (i32.const 1)) (i32.const 6))
(assert_return (invoke "as-tee_local-value" (i32.const 1)) (i32.const 6))
(assert_return (invoke "as-set_global-value") (i32.const 6))

(assert_return (invoke "as-unary-operand") (i32.const 0))
(assert_return (invoke "as-binary-operand") (i32.const 36))
(assert_return (invoke "as-compare-operand") (i32.const 1))

(assert_invalid
  (module (global f32 (f32.const 0)) (func (set_global 0 (i32.const 1))))
  "global is immutable"
)

;; mutable globals can be exported
;; SKIP_MUTABLE_GLOBALS
;; (module (global (mut f32) (f32.const 0)) (export "a" (global 0)))

;; SKIP_MUTABLE_GLOBALS
;; (module (global (export "a") (mut f32) (f32.const 0)))

(assert_invalid
  (module (global f32 (f32.neg (f32.const 0))))
  "constant expression required"
)

(assert_invalid
  (module (global f32 (get_local 0)))
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
  (module (global i32 (i32.const 0)) (global i64 (get_global 1)))
  "unknown global"
)


(assert_invalid
  (module (global i32 (;empty instruction sequence;)))
  "type mismatch"
)

(assert_invalid
  (module (global i32 (get_global 0)))
  "unknown global"
)

(assert_invalid
  (module (global i32 (get_global 1)) (global i32 (i32.const 0)))
  "unknown global"
)

(module
  (import "spectest" "global_i32" (global i32))
  (global i32 (get_global 0))
  (func (export "get-0") (result i32) (get_global 0))
  (func (export "get-0-ref") (result i32) (get_global 1))
)

(assert_return (invoke "get-0") (i32.const 666))
(assert_return (invoke "get-0-ref") (i32.const 666))

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\94\80\80\80\00"             ;; import section
      "\01"                          ;; length 1
      "\08\73\70\65\63\74\65\73\74"  ;; "spectest"
      "\0a\67\6c\6f\62\61\6c\5f\69\33\32" ;; "global_i32"
      "\03"                          ;; GlobalImport
      "\7f"                          ;; i32
      "\02"                          ;; invalid mutability
  )
  "invalid mutability"
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\94\80\80\80\00"             ;; import section
      "\01"                          ;; length 1
      "\08\73\70\65\63\74\65\73\74"  ;; "spectest"
      "\0a\67\6c\6f\62\61\6c\5f\69\33\32" ;; "global_i32"
      "\03"                          ;; GlobalImport
      "\7f"                          ;; i32
      "\ff"                          ;; invalid mutability
  )
  "invalid mutability"
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
      "\02"               ;; invalid mutability
      "\41\00"            ;; i32.const 0
      "\0b"               ;; end
  )
  "invalid mutability"
)
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\06\86\80\80\80\00"  ;; global section
      "\01"               ;; length 1
      "\7f"               ;; i32
      "\ff"               ;; invalid mutability
      "\41\00"            ;; i32.const 0
      "\0b"               ;; end
  )
  "invalid mutability"
)
