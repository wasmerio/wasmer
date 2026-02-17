(module

  ;; Check that both sides of the select are evaluated
  (func (export "select-trap-left") (param $cond i32) (result i32)
    (select (unreachable) (i32.const 0) (local.get $cond))
  )
  (func (export "select-trap-right") (param $cond i32) (result i32)
    (select (i32.const 0) (unreachable) (local.get $cond))
  )

  (func (export "select-unreached")
    (unreachable) (select)
    (unreachable) (i32.const 0) (select)
    (unreachable) (i32.const 0) (i32.const 0) (select)
    (unreachable) (i32.const 0) (i32.const 0) (i32.const 0) (select)
    (unreachable) (f32.const 0) (i32.const 0) (select)
    (unreachable)
  )

  (func (export "select-unreached-result1") (result i32)
    (unreachable) (i32.add (select))
  )

  (func (export "select-unreached-result2") (result i64)
    (unreachable) (i64.add (select (i64.const 0) (i32.const 0)))
  )

  (func (export "select-unreached-num")
    (unreachable)
    (select)
    (i32.eqz)
    (drop)
  )
  (func (export "select-unreached-ref")
    (unreachable)
    (select)
    (ref.is_null)
    (drop)
  )

  (type $t (func (param i32) (result i32)))
  (func (export "call_ref-unreached") (result i32)
    (unreachable)
    (call_ref $t)
  )
)

(assert_trap (invoke "select-trap-left" (i32.const 1)) "unreachable")
(assert_trap (invoke "select-trap-left" (i32.const 0)) "unreachable")
(assert_trap (invoke "select-trap-right" (i32.const 1)) "unreachable")
(assert_trap (invoke "select-trap-right" (i32.const 0)) "unreachable")

(assert_trap (invoke "select-unreached-result1") "unreachable")
(assert_trap (invoke "select-unreached-result2") "unreachable")
(assert_trap (invoke "select-unreached-num") "unreachable")
(assert_trap (invoke "select-unreached-ref") "unreachable")

(assert_trap (invoke "call_ref-unreached") "unreachable")


;; Validation after unreachable

(module
  (func (export "meet-bottom")
    (block (result f64)
      (block (result f32)
        (unreachable)
        (br_table 0 1 1 (i32.const 1))
      )
      (drop)
      (f64.const 0)
    )
    (drop)
  )
)

(assert_trap (invoke "meet-bottom") "unreachable")


;; Bottom heap type

(module
  (func (result (ref func))
    (unreachable)
    (ref.as_non_null)
  )
  (func (result (ref extern))
    (unreachable)
    (ref.as_non_null)
  )

  (func (result (ref func))
    (block (result funcref)
      (unreachable)
      (br_on_null 0)
      (return)
    )
    (unreachable)
  )
  (func (result (ref extern))
    (block (result externref)
      (unreachable)
      (br_on_null 0)
      (return)
    )
    (unreachable)
  )
)
