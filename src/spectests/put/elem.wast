;; Test the element section

;; Syntax
(module
  (table $t 10 anyfunc)
  (func $f)
  (elem (i32.const 0))
  (elem (i32.const 0) $f $f)
  (elem (offset (i32.const 0)))
  (elem (offset (i32.const 0)) $f $f)
  (elem 0 (i32.const 0))
  (elem 0x0 (i32.const 0) $f $f)
  (elem 0x000 (offset (i32.const 0)))
  (elem 0 (offset (i32.const 0)) $f $f)
  (elem $t (i32.const 0))
  (elem $t (i32.const 0) $f $f)
  (elem $t (offset (i32.const 0)))
  (elem $t (offset (i32.const 0)) $f $f)
)

;; Basic use

(module
  (table 10 anyfunc)
  (func $f)
  (elem (i32.const 0) $f)
)
(module
  (import "spectest" "table" (table 10 anyfunc))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (table 10 anyfunc)
  (func $f)
  (elem (i32.const 0) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 7) $f)
  (elem (i32.const 5) $f)
  (elem (i32.const 3) $f)
)
(module
  (import "spectest" "table" (table 10 anyfunc))
  (func $f)
  (elem (i32.const 9) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 7) $f)
  (elem (i32.const 3) $f)
  (elem (i32.const 5) $f)
)

(module
  (global (import "spectest" "global_i32") i32)
  (table 1000 anyfunc)
  (func $f)
  (elem (get_global 0) $f)
)

(module
  (global $g (import "spectest" "global_i32") i32)
  (table 1000 anyfunc)
  (func $f)
  (elem (get_global $g) $f)
)

(module
  (type $out-i32 (func (result i32)))
  (table 10 anyfunc)
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
  (table 10 anyfunc)
  (func $f)
  (elem (i32.const 9) $f)
)
(module
  (import "spectest" "table" (table 10 anyfunc))
  (func $f)
  (elem (i32.const 9) $f)
)

(module
  (table 0 anyfunc)
  (elem (i32.const 0))
)
(module
  (import "spectest" "table" (table 0 anyfunc))
  (elem (i32.const 0))
)

(module
  (table 0 0 anyfunc)
  (elem (i32.const 0))
)

(module
  (table 20 anyfunc)
  (elem (i32.const 20))
)

(module
  (import "spectest" "table" (table 0 anyfunc))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (import "spectest" "table" (table 0 100 anyfunc))
  (func $f)
  (elem (i32.const 0) $f)
)

(module
  (import "spectest" "table" (table 0 anyfunc))
  (func $f)
  (elem (i32.const 1) $f)
)

(module
  (import "spectest" "table" (table 0 30 anyfunc))
  (func $f)
  (elem (i32.const 1) $f)
)

;; Invalid bounds for elements

(assert_unlinkable
  (module
    (table 0 anyfunc)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 0 0 anyfunc)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 0 1 anyfunc)
    (func $f)
    (elem (i32.const 0) $f)
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 0 anyfunc)
    (elem (i32.const 1))
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 10 anyfunc)
    (func $f)
    (elem (i32.const 10) $f)
  )
  "elements segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "table" (table 10 anyfunc))
    (func $f)
    (elem (i32.const 10) $f)
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 10 20 anyfunc)
    (func $f)
    (elem (i32.const 10) $f)
  )
  "elements segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "table" (table 10 anyfunc))
    (func $f)
    (elem (i32.const 10) $f)
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 10 anyfunc)
    (func $f)
    (elem (i32.const -1) $f)
  )
  "elements segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "table" (table 10 anyfunc))
    (func $f)
    (elem (i32.const -1) $f)
  )
  "elements segment does not fit"
)

(assert_unlinkable
  (module
    (table 10 anyfunc)
    (func $f)
    (elem (i32.const -10) $f)
  )
  "elements segment does not fit"
)
(assert_unlinkable
  (module
    (import "spectest" "table" (table 10 anyfunc))
    (func $f)
    (elem (i32.const -10) $f)
  )
  "elements segment does not fit"
)

;; Element without table

(assert_invalid
  (module
    (func $f)
    (elem (i32.const 0) $f)
  )
  "unknown table 0"
)

;; Invalid offsets

(assert_invalid
  (module
    (table 1 anyfunc)
    (elem (i64.const 0))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (table 1 anyfunc)
    (elem (i32.ctz (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 anyfunc)
    (elem (nop))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 anyfunc)
    (elem (offset (nop) (i32.const 0)))
  )
  "constant expression required"
)

(assert_invalid
  (module
    (table 1 anyfunc)
    (elem (offset (i32.const 0) (nop)))
  )
  "constant expression required"
)

;; Use of internal globals in constant expressions is not allowed in MVP.
;; (assert_invalid
;;   (module (memory 1) (data (get_global $g)) (global $g (mut i32) (i32.const 0)))
;;   "constant expression required"
;; )

;; Two elements target the same slot

(module
  (type $out-i32 (func (result i32)))
  (table 10 anyfunc)
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
  (import "spectest" "table" (table 10 anyfunc))
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
  (table (export "shared-table") 10 anyfunc)
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

(assert_trap (invoke $module1 "call-7") "uninitialized element 7")
(assert_return (invoke $module1 "call-8") (i32.const 65))
(assert_return (invoke $module1 "call-9") (i32.const 66))

(module $module2
  (type $out-i32 (func (result i32)))
  (import "module1" "shared-table" (table 10 anyfunc))
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
  (import "module1" "shared-table" (table 10 anyfunc))
  (elem (i32.const 8) $const-i32-e)
  (elem (i32.const 9) $const-i32-f)
  (func $const-i32-e (type $out-i32) (i32.const 69))
  (func $const-i32-f (type $out-i32) (i32.const 70))
)

(assert_return (invoke $module1 "call-7") (i32.const 67))
(assert_return (invoke $module1 "call-8") (i32.const 69))
(assert_return (invoke $module1 "call-9") (i32.const 70))
