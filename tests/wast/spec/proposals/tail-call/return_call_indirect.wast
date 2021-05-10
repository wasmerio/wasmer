;; Test `return_call_indirect` operator

(module
  ;; Auxiliary definitions
  (type $proc (func))
  (type $out-i32 (func (result i32)))
  (type $out-i64 (func (result i64)))
  (type $out-f32 (func (result f32)))
  (type $out-f64 (func (result f64)))
  (type $over-i32 (func (param i32) (result i32)))
  (type $over-i64 (func (param i64) (result i64)))
  (type $over-f32 (func (param f32) (result f32)))
  (type $over-f64 (func (param f64) (result f64)))
  (type $f32-i32 (func (param f32 i32) (result i32)))
  (type $i32-i64 (func (param i32 i64) (result i64)))
  (type $f64-f32 (func (param f64 f32) (result f32)))
  (type $i64-f64 (func (param i64 f64) (result f64)))
  (type $over-i32-duplicate (func (param i32) (result i32)))
  (type $over-i64-duplicate (func (param i64) (result i64)))
  (type $over-f32-duplicate (func (param f32) (result f32)))
  (type $over-f64-duplicate (func (param f64) (result f64)))

  (func $const-i32 (type $out-i32) (i32.const 0x132))
  (func $const-i64 (type $out-i64) (i64.const 0x164))
  (func $const-f32 (type $out-f32) (f32.const 0xf32))
  (func $const-f64 (type $out-f64) (f64.const 0xf64))

  (func $id-i32 (type $over-i32) (local.get 0))
  (func $id-i64 (type $over-i64) (local.get 0))
  (func $id-f32 (type $over-f32) (local.get 0))
  (func $id-f64 (type $over-f64) (local.get 0))

  (func $i32-i64 (type $i32-i64) (local.get 1))
  (func $i64-f64 (type $i64-f64) (local.get 1))
  (func $f32-i32 (type $f32-i32) (local.get 1))
  (func $f64-f32 (type $f64-f32) (local.get 1))

  (func $over-i32-duplicate (type $over-i32-duplicate) (local.get 0))
  (func $over-i64-duplicate (type $over-i64-duplicate) (local.get 0))
  (func $over-f32-duplicate (type $over-f32-duplicate) (local.get 0))
  (func $over-f64-duplicate (type $over-f64-duplicate) (local.get 0))

  (table funcref
    (elem
      $const-i32 $const-i64 $const-f32 $const-f64
      $id-i32 $id-i64 $id-f32 $id-f64
      $f32-i32 $i32-i64 $f64-f32 $i64-f64
      $fac $fac-acc $even $odd
      $over-i32-duplicate $over-i64-duplicate
      $over-f32-duplicate $over-f64-duplicate
    )
  )

  ;; Syntax

  (func
    (return_call_indirect (i32.const 0))
    (return_call_indirect (param i64) (i64.const 0) (i32.const 0))
    (return_call_indirect (param i64) (param) (param f64 i32 i64)
      (i64.const 0) (f64.const 0) (i32.const 0) (i64.const 0) (i32.const 0)
    )
    (return_call_indirect (result) (i32.const 0))
  )

  (func (result i32)
    (return_call_indirect (result i32) (i32.const 0))
    (return_call_indirect (result i32) (result) (i32.const 0))
    (return_call_indirect (param i64) (result i32) (i64.const 0) (i32.const 0))
    (return_call_indirect
      (param) (param i64) (param) (param f64 i32 i64) (param) (param)
      (result) (result i32) (result) (result)
      (i64.const 0) (f64.const 0) (i32.const 0) (i64.const 0) (i32.const 0)
    )
  )

  (func (result i64)
    (return_call_indirect (type $over-i64) (param i64) (result i64)
      (i64.const 0) (i32.const 0)
    )
  )

  ;; Typing

  (func (export "type-i32") (result i32)
    (return_call_indirect (type $out-i32) (i32.const 0))
  )
  (func (export "type-i64") (result i64)
    (return_call_indirect (type $out-i64) (i32.const 1))
  )
  (func (export "type-f32") (result f32)
    (return_call_indirect (type $out-f32) (i32.const 2))
  )
  (func (export "type-f64") (result f64)
    (return_call_indirect (type $out-f64) (i32.const 3))
  )

  (func (export "type-index") (result i64)
    (return_call_indirect (type $over-i64) (i64.const 100) (i32.const 5))
  )

  (func (export "type-first-i32") (result i32)
    (return_call_indirect (type $over-i32) (i32.const 32) (i32.const 4))
  )
  (func (export "type-first-i64") (result i64)
    (return_call_indirect (type $over-i64) (i64.const 64) (i32.const 5))
  )
  (func (export "type-first-f32") (result f32)
    (return_call_indirect (type $over-f32) (f32.const 1.32) (i32.const 6))
  )
  (func (export "type-first-f64") (result f64)
    (return_call_indirect (type $over-f64) (f64.const 1.64) (i32.const 7))
  )

  (func (export "type-second-i32") (result i32)
    (return_call_indirect (type $f32-i32)
      (f32.const 32.1) (i32.const 32) (i32.const 8)
    )
  )
  (func (export "type-second-i64") (result i64)
    (return_call_indirect (type $i32-i64)
      (i32.const 32) (i64.const 64) (i32.const 9)
    )
  )
  (func (export "type-second-f32") (result f32)
    (return_call_indirect (type $f64-f32)
      (f64.const 64) (f32.const 32) (i32.const 10)
    )
  )
  (func (export "type-second-f64") (result f64)
    (return_call_indirect (type $i64-f64)
      (i64.const 64) (f64.const 64.1) (i32.const 11)
    )
  )

  ;; Dispatch

  (func (export "dispatch") (param i32 i64) (result i64)
    (return_call_indirect (type $over-i64) (local.get 1) (local.get 0))
  )

  (func (export "dispatch-structural") (param i32) (result i64)
    (return_call_indirect (type $over-i64-duplicate)
      (i64.const 9) (local.get 0)
    )
  )

  ;; Multiple tables

  (table $tab2 funcref (elem $tab-f1))
  (table $tab3 funcref (elem $tab-f2))

  (func $tab-f1 (result i32) (i32.const 0x133))
  (func $tab-f2 (result i32) (i32.const 0x134))

  (func (export "call-tab") (param $i i32) (result i32)
    (if (i32.eq (local.get $i) (i32.const 0))
      (then (return_call_indirect (type $out-i32) (i32.const 0)))
    )
    (if (i32.eq (local.get $i) (i32.const 1))
      (then (return_call_indirect 1 (type $out-i32) (i32.const 0)))
    )
    (if (i32.eq (local.get $i) (i32.const 2))
      (then (return_call_indirect $tab3 (type $out-i32) (i32.const 0)))
    )
    (i32.const 0)
  )

  ;; Recursion

  (func $fac (export "fac") (type $over-i64)
    (return_call_indirect (param i64 i64) (result i64)
      (local.get 0) (i64.const 1) (i32.const 13)
    )
  )

  (func $fac-acc (param i64 i64) (result i64)
    (if (result i64) (i64.eqz (local.get 0))
      (then (local.get 1))
      (else
        (return_call_indirect (param i64 i64) (result i64)
          (i64.sub (local.get 0) (i64.const 1))
          (i64.mul (local.get 0) (local.get 1))
          (i32.const 13)
        )
      )
    )
  )

  (func $even (export "even") (param i32) (result i32)
    (if (result i32) (i32.eqz (local.get 0))
      (then (i32.const 44))
      (else
        (return_call_indirect (type $over-i32)
          (i32.sub (local.get 0) (i32.const 1))
          (i32.const 15)
        )
      )
    )
  )
  (func $odd (export "odd") (param i32) (result i32)
    (if (result i32) (i32.eqz (local.get 0))
      (then (i32.const 99))
      (else
        (return_call_indirect (type $over-i32)
          (i32.sub (local.get 0) (i32.const 1))
          (i32.const 14)
        )
      )
    )
  )
)

(assert_return (invoke "type-i32") (i32.const 0x132))
(assert_return (invoke "type-i64") (i64.const 0x164))
(assert_return (invoke "type-f32") (f32.const 0xf32))
(assert_return (invoke "type-f64") (f64.const 0xf64))

(assert_return (invoke "type-index") (i64.const 100))

(assert_return (invoke "type-first-i32") (i32.const 32))
(assert_return (invoke "type-first-i64") (i64.const 64))
(assert_return (invoke "type-first-f32") (f32.const 1.32))
(assert_return (invoke "type-first-f64") (f64.const 1.64))

(assert_return (invoke "type-second-i32") (i32.const 32))
(assert_return (invoke "type-second-i64") (i64.const 64))
(assert_return (invoke "type-second-f32") (f32.const 32))
(assert_return (invoke "type-second-f64") (f64.const 64.1))

(assert_return (invoke "dispatch" (i32.const 5) (i64.const 2)) (i64.const 2))
(assert_return (invoke "dispatch" (i32.const 5) (i64.const 5)) (i64.const 5))
(assert_return (invoke "dispatch" (i32.const 12) (i64.const 5)) (i64.const 120))
(assert_return (invoke "dispatch" (i32.const 17) (i64.const 2)) (i64.const 2))
(assert_trap (invoke "dispatch" (i32.const 0) (i64.const 2)) "indirect call type mismatch")
(assert_trap (invoke "dispatch" (i32.const 15) (i64.const 2)) "indirect call type mismatch")
(assert_trap (invoke "dispatch" (i32.const 20) (i64.const 2)) "undefined element")
(assert_trap (invoke "dispatch" (i32.const -1) (i64.const 2)) "undefined element")
(assert_trap (invoke "dispatch" (i32.const 1213432423) (i64.const 2)) "undefined element")

(assert_return (invoke "dispatch-structural" (i32.const 5)) (i64.const 9))
(assert_return (invoke "dispatch-structural" (i32.const 5)) (i64.const 9))
(assert_return (invoke "dispatch-structural" (i32.const 12)) (i64.const 362880))
(assert_return (invoke "dispatch-structural" (i32.const 17)) (i64.const 9))
(assert_trap (invoke "dispatch-structural" (i32.const 11)) "indirect call type mismatch")
(assert_trap (invoke "dispatch-structural" (i32.const 16)) "indirect call type mismatch")

(assert_return (invoke "call-tab" (i32.const 0)) (i32.const 0x132))
(assert_return (invoke "call-tab" (i32.const 1)) (i32.const 0x133))
(assert_return (invoke "call-tab" (i32.const 2)) (i32.const 0x134))

(assert_return (invoke "fac" (i64.const 0)) (i64.const 1))
(assert_return (invoke "fac" (i64.const 1)) (i64.const 1))
(assert_return (invoke "fac" (i64.const 5)) (i64.const 120))
(assert_return (invoke "fac" (i64.const 25)) (i64.const 7034535277573963776))

(assert_return (invoke "even" (i32.const 0)) (i32.const 44))
(assert_return (invoke "even" (i32.const 1)) (i32.const 99))
(assert_return (invoke "even" (i32.const 100)) (i32.const 44))
(assert_return (invoke "even" (i32.const 77)) (i32.const 99))
(assert_return (invoke "even" (i32.const 100_000)) (i32.const 44))
(assert_return (invoke "even" (i32.const 111_111)) (i32.const 99))
(assert_return (invoke "odd" (i32.const 0)) (i32.const 99))
(assert_return (invoke "odd" (i32.const 1)) (i32.const 44))
(assert_return (invoke "odd" (i32.const 200)) (i32.const 99))
(assert_return (invoke "odd" (i32.const 77)) (i32.const 44))
(assert_return (invoke "odd" (i32.const 200_002)) (i32.const 99))
(assert_return (invoke "odd" (i32.const 300_003)) (i32.const 44))


;; Invalid syntax

(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (type $sig) (result i32) (param i32)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (param i32) (type $sig) (result i32)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (param i32) (result i32) (type $sig)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (result i32) (type $sig) (param i32)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (result i32) (param i32) (type $sig)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (result i32) (param i32)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "unexpected token"
)

(assert_malformed
  (module quote
    "(table 0 funcref)"
    "(func (return_call_indirect (param $x i32) (i32.const 0) (i32.const 0)))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (type $sig) (result i32) (i32.const 0))"
    ")"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (type $sig) (result i32) (i32.const 0))"
    ")"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(table 0 funcref)"
    "(func"
    "  (return_call_indirect (type $sig) (param i32)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32 i32) (result i32)))"
    "(table 0 funcref)"
    "(func (result i32)"
    "  (return_call_indirect (type $sig) (param i32) (result i32)"
    "    (i32.const 0) (i32.const 0)"
    "  )"
    ")"
  )
  "inline function type"
)

;; Invalid typing

(assert_invalid
  (module
    (type (func))
    (func $no-table (return_call_indirect (type 0) (i32.const 0)))
  )
  "unknown table"
)

(assert_invalid
  (module
    (type (func))
    (table 0 funcref)
    (func $type-void-vs-num (i32.eqz (return_call_indirect (type 0) (i32.const 0))))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type (func (result i64)))
    (table 0 funcref)
    (func $type-num-vs-num (i32.eqz (return_call_indirect (type 0) (i32.const 0))))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type (func (param i32)))
    (table 0 funcref)
    (func $arity-0-vs-1 (return_call_indirect (type 0) (i32.const 0)))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type (func (param f64 i32)))
    (table 0 funcref)
    (func $arity-0-vs-2 (return_call_indirect (type 0) (i32.const 0)))
  )
  "type mismatch"
)

(module
  (type (func))
  (table 0 funcref)
  (func $arity-1-vs-0 (return_call_indirect (type 0) (i32.const 1) (i32.const 0)))
)

(module
  (type (func))
  (table 0 funcref)
  (func $arity-2-vs-0
    (return_call_indirect (type 0) (f64.const 2) (i32.const 1) (i32.const 0))
  )
)

(assert_invalid
  (module
    (type (func (param i32)))
    (table 0 funcref)
    (func $type-func-void-vs-i32 (return_call_indirect (type 0) (i32.const 1) (nop)))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type (func (param i32)))
    (table 0 funcref)
    (func $type-func-num-vs-i32 (return_call_indirect (type 0) (i32.const 0) (i64.const 1)))
  )
  "type mismatch"
)

(assert_invalid
  (module
    (type (func (param i32 i32)))
    (table 0 funcref)
    (func $type-first-void-vs-num
      (return_call_indirect (type 0) (nop) (i32.const 1) (i32.const 0))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type (func (param i32 i32)))
    (table 0 funcref)
    (func $type-second-void-vs-num
      (return_call_indirect (type 0) (i32.const 1) (nop) (i32.const 0))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type (func (param i32 f64)))
    (table 0 funcref)
    (func $type-first-num-vs-num
      (return_call_indirect (type 0) (f64.const 1) (i32.const 1) (i32.const 0))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (type (func (param f64 i32)))
    (table 0 funcref)
    (func $type-second-num-vs-num
      (return_call_indirect (type 0) (i32.const 1) (f64.const 1) (i32.const 0))
    )
  )
  "type mismatch"
)


;; Unbound type

(assert_invalid
  (module
    (table 0 funcref)
    (func $unbound-type (return_call_indirect (type 1) (i32.const 0)))
  )
  "unknown type"
)
(assert_invalid
  (module
    (table 0 funcref)
    (func $large-type (return_call_indirect (type 1012321300) (i32.const 0)))
  )
  "unknown type"
)


;; Unbound function in table

(assert_invalid
  (module (table funcref (elem 0 0)))
  "unknown function 0"
)
