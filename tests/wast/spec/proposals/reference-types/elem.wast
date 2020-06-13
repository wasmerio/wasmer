;; Test the element section

;; Syntax
(module
  (table $t 10 funcref)
  (func $f)
  (func $g)

  ;; Passive
  (elem funcref)
  (elem funcref (ref.func $f) (item ref.func $f) (item (ref.null)) (ref.func $g))
  (elem func)
  (elem func $f $f $g $g)

  (elem $p1 funcref)
  (elem $p2 funcref (ref.func $f) (ref.func $f) (ref.null) (ref.func $g))
  (elem $p3 func)
  (elem $p4 func $f $f $g $g)

  ;; Active
  (elem (table $t) (i32.const 0) funcref)
  (elem (table $t) (i32.const 0) funcref (ref.func $f) (ref.null))
  (elem (table $t) (i32.const 0) func)
  (elem (table $t) (i32.const 0) func $f $g)
  (elem (table $t) (offset (i32.const 0)) funcref)
  (elem (table $t) (offset (i32.const 0)) func $f $g)
  (elem (table 0) (i32.const 0) func)
  (elem (table 0x0) (i32.const 0) func $f $f)
  (elem (table 0x000) (offset (i32.const 0)) func)
  (elem (table 0) (offset (i32.const 0)) func $f $f)
  (elem (table $t) (i32.const 0) func)
  (elem (table $t) (i32.const 0) func $f $f)
  (elem (table $t) (offset (i32.const 0)) func)
  (elem (table $t) (offset (i32.const 0)) func $f $f)
  (elem (offset (i32.const 0)))
  (elem (offset (i32.const 0)) funcref (ref.func $f) (ref.null))
  (elem (offset (i32.const 0)) func $f $f)
  (elem (offset (i32.const 0)) $f $f)
  (elem (i32.const 0))
  (elem (i32.const 0) funcref (ref.func $f) (ref.null))
  (elem (i32.const 0) func $f $f)
  (elem (i32.const 0) $f $f)

  (elem $a1 (table $t) (i32.const 0) funcref)
  (elem $a2 (table $t) (i32.const 0) funcref (ref.func $f) (ref.null))
  (elem $a3 (table $t) (i32.const 0) func)
  (elem $a4 (table $t) (i32.const 0) func $f $g)
  (elem $a9 (table $t) (offset (i32.const 0)) funcref)
  (elem $a10 (table $t) (offset (i32.const 0)) func $f $g)
  (elem $a11 (table 0) (i32.const 0) func)
  (elem $a12 (table 0x0) (i32.const 0) func $f $f)
  (elem $a13 (table 0x000) (offset (i32.const 0)) func)
  (elem $a14 (table 0) (offset (i32.const 0)) func $f $f)
  (elem $a15 (table $t) (i32.const 0) func)
  (elem $a16 (table $t) (i32.const 0) func $f $f)
  (elem $a17 (table $t) (offset (i32.const 0)) func)
  (elem $a18 (table $t) (offset (i32.const 0)) func $f $f)
  (elem $a19 (offset (i32.const 0)))
  (elem $a20 (offset (i32.const 0)) funcref (ref.func $f) (ref.null))
  (elem $a21 (offset (i32.const 0)) func $f $f)
  (elem $a22 (offset (i32.const 0)) $f $f)
  (elem $a23 (i32.const 0))
  (elem $a24 (i32.const 0) funcref (ref.func $f) (ref.null))
  (elem $a25 (i32.const 0) func $f $f)
  (elem $a26 (i32.const 0) $f $f)

  ;; Declarative
  (elem declare funcref)
  (elem declare funcref (ref.func $f) (ref.func $f) (ref.null) (ref.func $g))
  (elem declare func)
  (elem declare func $f $f $g $g)

  (elem $d1 declare funcref)
  (elem $d2 declare funcref (ref.func $f) (ref.func $f) (ref.null) (ref.func $g))
  (elem $d3 declare func)
  (elem $d4 declare func $f $f $g $g)
)

(module
  (func $f)
  (func $g)

  (table $t funcref (elem (ref.func $f) (ref.null) (ref.func $g)))
)


;; Basic use

(module
  (table 10 funcref)
  (func $f)
  (elem (i32.const 0) $f)
)
(module
  (import "spectest" "table" (table 10 funcref))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (table 10 funcref)
  (func $f)
  (elem (i32.const 0) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 7) $f)
  (elem (i32.const 5) $f)
  (elem (i32.const 3) $f)
)
(module
  (import "spectest" "table" (table 10 funcref))
  (func $f)
  (elem (i32.const 9) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 7) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 5) $f)
)

(module
  (global (import "spectest" "global_i32") i32)
  (table 1000 funcref)
  (func $f)
  (elem (global.get 0) $f)
)

(module
  (global $g (import "spectest" "global_i32") i32)
  (table 1000 funcref)
  (func $f)
  (elem (global.get $g) $f)
)

(module
  (type $out-i32 (func (result i32)))
  (table 10 funcref)
  (elem (i32.const 7) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-7") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 7))
  )
  (func (export "call-9") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-7") (i32.const 65))
(assert_return (invoke "call-9") (i32.const 66))

;; Corner cases

(module
  (table 10 funcref)
  (func $f)
  (elem (i32.const 9) $f)
)
(module
  (import "spectest" "table" (table 10 funcref))
  (func $f)
  (elem (i32.const 9) $f)
)

(module
  (table 0 funcref)
  (elem (i32.const 0))
)
(module
  (import "spectest" "table" (table 0 funcref))
  (elem (i32.const 0))
)

(module
  (table 0 0 funcref)
  (elem (i32.const 0))
)

(module
  (table 20 funcref)
  (elem (i32.const 20))
)

(module
  (import "spectest" "table" (table 0 funcref))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (import "spectest" "table" (table 0 100 funcref))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (import "spectest" "table" (table 0 funcref))
  (func $f)
  (elem (i32.const 1) $f)
)

(module
  (import "spectest" "table" (table 0 30 funcref))
  (func $f)
  (elem (i32.const 1) $f)
)

;; Invalid bounds for elements

(assert_trap
  (module
    (table 0 funcref)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "out of bounds"
)

(assert_trap
  (module
    (table 0 0 funcref)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "out of bounds"
)

(assert_trap
  (module
    (table 0 1 funcref)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "out of bounds"
)

(assert_trap
  (module
    (table 0 funcref)
    (elem (i32.const 1))
  )
  "out of bounds"
)
(assert_trap
  (module
    (table 10 funcref)
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds"
)

(assert_trap
  (module
    (table 10 20 funcref)
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const 10) $f)
  )
  "out of bounds"
)

(assert_trap
  (module
    (table 10 funcref)
    (func $f)
    (elem (i32.const -1) $f)
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const -1) $f)
  )
  "out of bounds"
)

(assert_trap
  (module
    (table 10 funcref)
    (func $f)
    (elem (i32.const -10) $f)
  )
  "out of bounds"
)
(assert_trap
  (module
    (import "spectest" "table" (table 10 funcref))
    (func $f)
    (elem (i32.const -10) $f)
  )
  "out of bounds"
)

;; Implicitly dropped elements

(module
  (table 10 funcref)
  (elem $e (i32.const 0) func $f)
  (func $f)
  (func (export "init")
    (table.init $e (i32.const 0) (i32.const 0) (i32.const 1))
  )
)
(assert_trap (invoke "init") "out of bounds")

(module
  (table 10 funcref)
  (elem $e declare func $f)
  (func $f)
  (func (export "init")
    (table.init $e (i32.const 0) (i32.const 0) (i32.const 1))
  )
)
(assert_trap (invoke "init") "out of bounds")

;; Element without table

(assert_invalid
  (module
    (func $f)
    (elem (i32.const 0) $f)
  )
  "unknown table"
)

;; Invalid offsets

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i64.const 0))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (i32.ctz (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (nop))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (offset (nop) (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 funcref)
    (elem (offset (i32.const 0) (nop)))
  )
  "constant expression required"
)

;; Use of internal globals in constant expressions is not allowed in MVP.
;; (assert_invalid
;;   (module (memory 1) (data (global.get $g)) (global $g (mut i32) (i32.const 0)))
;;   "constant expression required"
;; )

;; Two elements target the same slot

(module
  (type $out-i32 (func (result i32)))
  (table 10 funcref)
  (elem (i32.const 9) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-overwritten") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-overwritten") (i32.const 66))

(module
  (type $out-i32 (func (result i32)))
  (import "spectest" "table" (table 10 funcref))
  (elem (i32.const 9) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-overwritten-element") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)
(assert_return (invoke "call-overwritten-element") (i32.const 66))

;; Element sections across multiple modules change the same table

(module $module1
  (type $out-i32 (func (result i32)))
  (table (export "shared-table") 10 funcref)
  (elem (i32.const 8) $const-i32-a)
  (elem (i32.const 9) $const-i32-b)
  (func $const-i32-a (type $out-i32) (i32.const 65))
  (func $const-i32-b (type $out-i32) (i32.const 66))
  (func (export "call-7") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 7))
  )
  (func (export "call-8") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 8))
  )
  (func (export "call-9") (type $out-i32)
    (call_indirect (type $out-i32) (i32.const 9))
  )
)

(register "module1" $module1)

(assert_trap (invoke $module1 "call-7") "uninitialized element")
(assert_return (invoke $module1 "call-8") (i32.const 65))
(assert_return (invoke $module1 "call-9") (i32.const 66))

(module $module2
  (type $out-i32 (func (result i32)))
  (import "module1" "shared-table" (table 10 funcref))
  (elem (i32.const 7) $const-i32-c)
  (elem (i32.const 8) $const-i32-d)
  (func $const-i32-c (type $out-i32) (i32.const 67))
  (func $const-i32-d (type $out-i32) (i32.const 68))
)

(assert_return (invoke $module1 "call-7") (i32.const 67))
(assert_return (invoke $module1 "call-8") (i32.const 68))
(assert_return (invoke $module1 "call-9") (i32.const 66))

(module $module3
  (type $out-i32 (func (result i32)))
  (import "module1" "shared-table" (table 10 funcref))
  (elem (i32.const 8) $const-i32-e)
  (elem (i32.const 9) $const-i32-f)
  (func $const-i32-e (type $out-i32) (i32.const 69))
  (func $const-i32-f (type $out-i32) (i32.const 70))
)

(assert_return (invoke $module1 "call-7") (i32.const 67))
(assert_return (invoke $module1 "call-8") (i32.const 69))
(assert_return (invoke $module1 "call-9") (i32.const 70))
