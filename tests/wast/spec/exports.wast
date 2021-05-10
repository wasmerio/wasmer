;; Functions

(module (func) (export "a" (func 0)))
(module (func) (export "a" (func 0)) (export "b" (func 0)))
(module (func) (func) (export "a" (func 0)) (export "b" (func 1)))

(module (func (export "a")))
(module (func (export "a") (export "b") (export "c")))
(module (func (export "a") (export "b") (param i32)))
(module (func) (export "a" (func 0)))
(module (func $a (export "a")))
(module (func $a) (export "a" (func $a)))
(module (export "a" (func 0)) (func))
(module (export "a" (func $a)) (func $a))

(module $Func
  (export "e" (func $f))
  (func $f (param $n i32) (result i32)
    (return (i32.add (local.get $n) (i32.const 1)))
  )
)
(assert_return (invoke "e" (i32.const 42)) (i32.const 43))
(assert_return (invoke $Func "e" (i32.const 42)) (i32.const 43))
(module)
(module $Other1)
(assert_return (invoke $Func "e" (i32.const 42)) (i32.const 43))

(module
  (type (;0;) (func (result i32)))
  (func (;0;) (type 0) (result i32) i32.const 42)
  (export "a" (func 0))
  (export "b" (func 0))
  (export "c" (func 0)))
(assert_return (invoke "a") (i32.const 42))
(assert_return (invoke "b") (i32.const 42))
(assert_return (invoke "c") (i32.const 42))

(assert_invalid
  (module (export "a" (func 0)))
  "unknown function"
)
(assert_invalid
  (module (func) (export "a" (func 1)))
  "unknown function"
)
(assert_invalid
  (module (import "spectest" "print_i32" (func (param i32))) (export "a" (func 1)))
  "unknown function"
)
(assert_invalid
  (module (func) (export "a" (func 0)) (export "a" (func 0)))
  "duplicate export name"
)
(assert_invalid
  (module (func) (func) (export "a" (func 0)) (export "a" (func 1)))
  "duplicate export name"
)
(assert_invalid
  (module (func) (global i32 (i32.const 0)) (export "a" (func 0)) (export "a" (global 0)))
  "duplicate export name"
)
(assert_invalid
  (module (func) (table 0 funcref) (export "a" (func 0)) (export "a" (table 0)))
  "duplicate export name"
)
(assert_invalid
  (module (func) (memory 0) (export "a" (func 0)) (export "a" (memory 0)))
  "duplicate export name"
)


;; Globals

(module (global i32 (i32.const 0)) (export "a" (global 0)))
(module (global i32 (i32.const 0)) (export "a" (global 0)) (export "b" (global 0)))
(module (global i32 (i32.const 0)) (global i32 (i32.const 0)) (export "a" (global 0)) (export "b" (global 1)))

(module (global (export "a") i32 (i32.const 0)))
(module (global i32 (i32.const 0)) (export "a" (global 0)))
(module (global $a (export "a") i32 (i32.const 0)))
(module (global $a i32 (i32.const 0)) (export "a" (global $a)))
(module (export "a" (global 0)) (global i32 (i32.const 0)))
(module (export "a" (global $a)) (global $a i32 (i32.const 0)))

(module $Global
  (export "e" (global $g))
  (global $g i32 (i32.const 42))
)
(assert_return (get "e") (i32.const 42))
(assert_return (get $Global "e") (i32.const 42))
(module)
(module $Other2)
(assert_return (get $Global "e") (i32.const 42))

(assert_invalid
  (module (export "a" (global 0)))
  "unknown global"
)
(assert_invalid
  (module (global i32 (i32.const 0)) (export "a" (global 1)))
  "unknown global"
)
(assert_invalid
  (module (import "spectest" "global_i32" (global i32)) (export "a" (global 1)))
  "unknown global"
)
(assert_invalid
  (module (global i32 (i32.const 0)) (export "a" (global 0)) (export "a" (global 0)))
  "duplicate export name"
)
(assert_invalid
  (module (global i32 (i32.const 0)) (global i32 (i32.const 0)) (export "a" (global 0)) (export "a" (global 1)))
  "duplicate export name"
)
(assert_invalid
  (module (global i32 (i32.const 0)) (func) (export "a" (global 0)) (export "a" (func 0)))
  "duplicate export name"
)
(assert_invalid
  (module (global i32 (i32.const 0)) (table 0 funcref) (export "a" (global 0)) (export "a" (table 0)))
  "duplicate export name"
)
(assert_invalid
  (module (global i32 (i32.const 0)) (memory 0) (export "a" (global 0)) (export "a" (memory 0)))
  "duplicate export name"
)


;; Tables

(module (table 0 funcref) (export "a" (table 0)))
(module (table 0 funcref) (export "a" (table 0)) (export "b" (table 0)))
(module (table 0 funcref) (table 0 funcref) (export "a" (table 0)) (export "b" (table 1)))

(module (table (export "a") 0 funcref))
(module (table (export "a") 0 1 funcref))
(module (table 0 funcref) (export "a" (table 0)))
(module (table 0 1 funcref) (export "a" (table 0)))
(module (table $a (export "a") 0 funcref))
(module (table $a (export "a") 0 1 funcref))
(module (table $a 0 funcref) (export "a" (table $a)))
(module (table $a 0 1 funcref) (export "a" (table $a)))
(module (export "a" (table 0)) (table 0 funcref))
(module (export "a" (table 0)) (table 0 1 funcref))
(module (export "a" (table $a)) (table $a 0 funcref))
(module (export "a" (table $a)) (table $a 0 1 funcref))

(; TODO: access table ;)

(assert_invalid
  (module (export "a" (table 0)))
  "unknown table"
)
(assert_invalid
  (module (table 0 funcref) (export "a" (table 1)))
  "unknown table"
)
(assert_invalid
  (module  (import "spectest" "table" (table 10 20 funcref)) (export "a" (table 1)))
  "unknown table"
)
(assert_invalid
  (module (table 0 funcref) (export "a" (table 0)) (export "a" (table 0)))
  "duplicate export name"
)
(assert_invalid
  (module (table 0 funcref) (table 0 funcref) (export "a" (table 0)) (export "a" (table 1)))
  "duplicate export name"
)
(assert_invalid
  (module (table 0 funcref) (func) (export "a" (table 0)) (export "a" (func 0)))
  "duplicate export name"
)
(assert_invalid
  (module (table 0 funcref) (global i32 (i32.const 0)) (export "a" (table 0)) (export "a" (global 0)))
  "duplicate export name"
)
(assert_invalid
  (module (table 0 funcref) (memory 0) (export "a" (table 0)) (export "a" (memory 0)))
  "duplicate export name"
)


;; Memories

(module (memory 0) (export "a" (memory 0)))
(module (memory 0) (export "a" (memory 0)) (export "b" (memory 0)))
;; No multiple memories yet.
;; (module (memory 0) (memory 0) (export "a" (memory 0)) (export "b" (memory 1)))

(module (memory (export "a") 0))
(module (memory (export "a") 0 1))
(module (memory 0) (export "a" (memory 0)))
(module (memory 0 1) (export "a" (memory 0)))
(module (memory $a (export "a") 0))
(module (memory $a (export "a") 0 1))
(module (memory $a 0) (export "a" (memory $a)))
(module (memory $a 0 1) (export "a" (memory $a)))
(module (export "a" (memory 0)) (memory 0))
(module (export "a" (memory 0)) (memory 0 1))
(module (export "a" (memory $a)) (memory $a 0))
(module (export "a" (memory $a)) (memory $a 0 1))

(; TODO: access memory ;)

(assert_invalid
  (module (export "a" (memory 0)))
  "unknown memory"
)
(assert_invalid
  (module (memory 0) (export "a" (memory 1)))
  "unknown memory"
)
(assert_invalid
  (module  (import "spectest" "memory" (memory 1 2)) (export "a" (memory 1)))
  "unknown memory"
)
(assert_invalid
  (module (memory 0) (export "a" (memory 0)) (export "a" (memory 0)))
  "duplicate export name"
)
;; No multiple memories yet.
;; (assert_invalid
;;   (module (memory 0) (memory 0) (export "a" (memory 0)) (export "a" (memory 1)))
;;   "duplicate export name"
;; )
(assert_invalid
  (module (memory 0) (func) (export "a" (memory 0)) (export "a" (func 0)))
  "duplicate export name"
)
(assert_invalid
  (module (memory 0) (global i32 (i32.const 0)) (export "a" (memory 0)) (export "a" (global 0)))
  "duplicate export name"
)
(assert_invalid
  (module (memory 0) (table 0 funcref) (export "a" (memory 0)) (export "a" (table 0)))
  "duplicate export name"
)
