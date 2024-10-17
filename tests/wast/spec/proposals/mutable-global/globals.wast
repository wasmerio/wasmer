;; Test globals
;; xdoardo (2024/09/06): These tests are not in the reference test suite anymore. We keep them for now, but adapt them to "new" keywords (e.g. local.get -> local.get)

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
  (func (export "set-x") (param i32) (set_global $x (local.get 0)))
  (func (export "set-y") (param i64) (set_global $y (local.get 0)))

  (func (export "get-1") (result f32) (get_global 1))
  (func (export "get-2") (result f64) (get_global 2))
  (func (export "get-5") (result f32) (get_global 5))
  (func (export "get-6") (result f64) (get_global 6))
  (func (export "set-5") (param f32) (set_global 5 (local.get 0)))
  (func (export "set-6") (param f64) (set_global 6 (local.get 0)))
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

(assert_invalid
  (module (global f32 (f32.const 0)) (func (set_global 0 (i32.const 1))))
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
  (module (global i32 (get_global 0)))
  "unknown global"
)

(assert_invalid
  (module (global i32 (get_global 1)) (global i32 (i32.const 0)))
  "unknown global"
)

(module
  (import "spectest" "global_i32" (global i32))
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
