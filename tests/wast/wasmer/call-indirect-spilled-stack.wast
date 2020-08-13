;; Spilled stack tests
;; https://github.com/wasmerio/wasmer/pull/1191

(module
  ;; Auxiliary definitions
  (type $out-i32 (func (result i32)))

  (func $const-i32 (type $out-i32) (i32.const 0x132))

  (table funcref
    (elem
      $const-i32
    )
  )

  (memory 1)

  (func (export "call-indirect-from-spilled-stack") (result i32)
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0) (i64.const 0))
    (i64.add (i64.const 0x100000000) (i64.const 0))
    (i32.wrap_i64)
    (call_indirect (type $out-i32))
    (return)
  )
)

(assert_return (invoke "call-indirect-from-spilled-stack") (i32.const 0x132))
