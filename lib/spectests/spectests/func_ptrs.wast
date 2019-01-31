(module
  (type    (func))                           ;; 0: void -> void
  (type $S (func))                           ;; 1: void -> void
  (type    (func (param)))                   ;; 2: void -> void
  (type    (func (result i32)))              ;; 3: void -> i32
  (type    (func (param) (result i32)))      ;; 4: void -> i32
  (type $T (func (param i32) (result i32)))  ;; 5: i32 -> i32
  (type $U (func (param i32)))               ;; 6: i32 -> void

  (func $print (import "spectest" "print_i32") (type 6))

  (func (type 0))
  (func (type $S))

  (func (export "one") (type 4) (i32.const 13))
  (func (export "two") (type $T) (i32.add (get_local 0) (i32.const 1)))

  ;; Both signature and parameters are allowed (and required to match)
  ;; since this allows the naming of parameters.
  (func (export "three") (type $T) (param $a i32) (result i32)
    (i32.sub (get_local 0) (i32.const 2))
  )

  (func (export "four") (type $U) (call $print (get_local 0)))
)

(assert_return (invoke "one") (i32.const 13))
(assert_return (invoke "two" (i32.const 13)) (i32.const 14))
(assert_return (invoke "three" (i32.const 13)) (i32.const 11))
(invoke "four" (i32.const 83))

(assert_invalid (module (elem (i32.const 0))) "unknown table")
(assert_invalid (module (elem (i32.const 0) 0) (func)) "unknown table")

(assert_invalid
  (module (table 1 anyfunc) (elem (i64.const 0)))
  "type mismatch"
)
(assert_invalid
  (module (table 1 anyfunc) (elem (i32.ctz (i32.const 0))))
  "constant expression required"
)
(assert_invalid
  (module (table 1 anyfunc) (elem (nop)))
  "constant expression required"
)

(assert_invalid (module (func (type 42))) "unknown type")
(assert_invalid (module (import "spectest" "print_i32" (func (type 43)))) "unknown type")

(module
  (type $T (func (param) (result i32)))
  (type $U (func (param) (result i32)))
  (table anyfunc (elem $t1 $t2 $t3 $u1 $u2 $t1 $t3))

  (func $t1 (type $T) (i32.const 1))
  (func $t2 (type $T) (i32.const 2))
  (func $t3 (type $T) (i32.const 3))
  (func $u1 (type $U) (i32.const 4))
  (func $u2 (type $U) (i32.const 5))

  (func (export "callt") (param $i i32) (result i32)
    (call_indirect (type $T) (get_local $i))
  )

  (func (export "callu") (param $i i32) (result i32)
    (call_indirect (type $U) (get_local $i))
  )
)

(assert_return (invoke "callt" (i32.const 0)) (i32.const 1))
(assert_return (invoke "callt" (i32.const 1)) (i32.const 2))
(assert_return (invoke "callt" (i32.const 2)) (i32.const 3))
(assert_return (invoke "callt" (i32.const 3)) (i32.const 4))
(assert_return (invoke "callt" (i32.const 4)) (i32.const 5))
(assert_return (invoke "callt" (i32.const 5)) (i32.const 1))
(assert_return (invoke "callt" (i32.const 6)) (i32.const 3))
(assert_trap (invoke "callt" (i32.const 7)) "undefined element")
(assert_trap (invoke "callt" (i32.const 100)) "undefined element")
(assert_trap (invoke "callt" (i32.const -1)) "undefined element")

(assert_return (invoke "callu" (i32.const 0)) (i32.const 1))
(assert_return (invoke "callu" (i32.const 1)) (i32.const 2))
(assert_return (invoke "callu" (i32.const 2)) (i32.const 3))
(assert_return (invoke "callu" (i32.const 3)) (i32.const 4))
(assert_return (invoke "callu" (i32.const 4)) (i32.const 5))
(assert_return (invoke "callu" (i32.const 5)) (i32.const 1))
(assert_return (invoke "callu" (i32.const 6)) (i32.const 3))
(assert_trap (invoke "callu" (i32.const 7)) "undefined element")
(assert_trap (invoke "callu" (i32.const 100)) "undefined element")
(assert_trap (invoke "callu" (i32.const -1)) "undefined element")

(module
  (type $T (func (result i32)))
  (table anyfunc (elem 0 1))

  (func $t1 (type $T) (i32.const 1))
  (func $t2 (type $T) (i32.const 2))

  (func (export "callt") (param $i i32) (result i32)
    (call_indirect (type $T) (get_local $i))
  )
)

(assert_return (invoke "callt" (i32.const 0)) (i32.const 1))
(assert_return (invoke "callt" (i32.const 1)) (i32.const 2))
