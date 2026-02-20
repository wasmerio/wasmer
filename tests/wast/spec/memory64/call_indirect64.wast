;; Test `call_indirect` operator

(module
  ;; Auxiliary definitions
  (type $out-i32 (func (result i32)))

  (func $const-i32 (type $out-i32) (i32.const 0x132))

  (table $t64 i64 funcref
    (elem $const-i32)
  )

  ;; Syntax

  (func
    (call_indirect $t64 (i64.const 0))
  )

  ;; Typing

  (func (export "type-i32-t64") (result i32)
    (call_indirect $t64 (type $out-i32) (i64.const 0))
  )
)

(assert_return (invoke "type-i32-t64") (i32.const 0x132))
