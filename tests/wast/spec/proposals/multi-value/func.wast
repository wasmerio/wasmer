;; Test `func` declarations, i.e. functions

(module
  ;; Auxiliary definition
  (type $sig (func))
  (func $dummy)

  ;; Syntax

  (func)
  (func (export "f"))
  (func $f)
  (func $h (export "g"))

  (func (local))
  (func (local) (local))
  (func (local i32))
  (func (local $x i32))
  (func (local i32 f64 i64))
  (func (local i32) (local f64))
  (func (local i32 f32) (local $x i64) (local) (local i32 f64))

  (func (param))
  (func (param) (param))
  (func (param i32))
  (func (param $x i32))
  (func (param i32 f64 i64))
  (func (param i32) (param f64))
  (func (param i32 f32) (param $x i64) (param) (param i32 f64))

  (func (result))
  (func (result) (result))
  (func (result i32) (unreachable))
  (func (result i32 f64 f32) (unreachable))
  (func (result i32) (result f64) (unreachable))
  (func (result i32 f32) (result i64) (result) (result i32 f64) (unreachable))

  (type $sig-1 (func))
  (type $sig-2 (func (result i32)))
  (type $sig-3 (func (param $x i32)))
  (type $sig-4 (func (param i32 f64 i32) (result i32)))

  (func (export "type-use-1") (type $sig-1))
  (func (export "type-use-2") (type $sig-2) (i32.const 0))
  (func (export "type-use-3") (type $sig-3))
  (func (export "type-use-4") (type $sig-4) (i32.const 0))
  (func (export "type-use-5") (type $sig-2) (result i32) (i32.const 0))
  (func (export "type-use-6") (type $sig-3) (param i32))
  (func (export "type-use-7")
    (type $sig-4) (param i32) (param f64 i32) (result i32) (i32.const 0)
  )

  (func (type $sig))
  (func (type $forward))  ;; forward reference

  (func $complex
    (param i32 f32) (param $x i64) (param) (param i32)
    (result) (result i32) (result) (result i64 i32)
    (local f32) (local $y i32) (local i64 i32) (local) (local f64 i32)
    (unreachable) (unreachable)
  )
  (func $complex-sig
    (type $sig)
    (local f32) (local $y i32) (local i64 i32) (local) (local f64 i32)
    (unreachable) (unreachable)
  )

  (type $forward (func))

  ;; Typing of locals

  (func (export "local-first-i32") (result i32) (local i32 i32) (local.get 0))
  (func (export "local-first-i64") (result i64) (local i64 i64) (local.get 0))
  (func (export "local-first-f32") (result f32) (local f32 f32) (local.get 0))
  (func (export "local-first-f64") (result f64) (local f64 f64) (local.get 0))
  (func (export "local-second-i32") (result i32) (local i32 i32) (local.get 1))
  (func (export "local-second-i64") (result i64) (local i64 i64) (local.get 1))
  (func (export "local-second-f32") (result f32) (local f32 f32) (local.get 1))
  (func (export "local-second-f64") (result f64) (local f64 f64) (local.get 1))
  (func (export "local-mixed") (result f64)
    (local f32) (local $x i32) (local i64 i32) (local) (local f64 i32)
    (drop (f32.neg (local.get 0)))
    (drop (i32.eqz (local.get 1)))
    (drop (i64.eqz (local.get 2)))
    (drop (i32.eqz (local.get 3)))
    (drop (f64.neg (local.get 4)))
    (drop (i32.eqz (local.get 5)))
    (local.get 4)
  )

  ;; Typing of parameters

  (func (export "param-first-i32") (param i32 i32) (result i32) (local.get 0))
  (func (export "param-first-i64") (param i64 i64) (result i64) (local.get 0))
  (func (export "param-first-f32") (param f32 f32) (result f32) (local.get 0))
  (func (export "param-first-f64") (param f64 f64) (result f64) (local.get 0))
  (func (export "param-second-i32") (param i32 i32) (result i32) (local.get 1))
  (func (export "param-second-i64") (param i64 i64) (result i64) (local.get 1))
  (func (export "param-second-f32") (param f32 f32) (result f32) (local.get 1))
  (func (export "param-second-f64") (param f64 f64) (result f64) (local.get 1))
  (func (export "param-mixed") (param f32 i32) (param) (param $x i64) (param i32 f64 i32)
    (result f64)
    (drop (f32.neg (local.get 0)))
    (drop (i32.eqz (local.get 1)))
    (drop (i64.eqz (local.get 2)))
    (drop (i32.eqz (local.get 3)))
    (drop (f64.neg (local.get 4)))
    (drop (i32.eqz (local.get 5)))
    (local.get 4)
  )

  ;; Typing of results

  (func (export "empty"))
  (func (export "value-void") (call $dummy))
  (func (export "value-i32") (result i32) (i32.const 77))
  (func (export "value-i64") (result i64) (i64.const 7777))
  (func (export "value-f32") (result f32) (f32.const 77.7))
  (func (export "value-f64") (result f64) (f64.const 77.77))
  (func (export "value-i32-f64") (result i32 f64) (i32.const 77) (f64.const 7))
  (func (export "value-i32-i32-i32") (result i32 i32 i32)
    (i32.const 1) (i32.const 2) (i32.const 3)
  )
  (func (export "value-block-void") (block (call $dummy) (call $dummy)))
  (func (export "value-block-i32") (result i32)
    (block (result i32) (call $dummy) (i32.const 77))
  )
  (func (export "value-block-i32-i64") (result i32 i64)
    (block (result i32 i64) (call $dummy) (i32.const 1) (i64.const 2))
  )

  (func (export "return-empty") (return))
  (func (export "return-i32") (result i32) (return (i32.const 78)))
  (func (export "return-i64") (result i64) (return (i64.const 7878)))
  (func (export "return-f32") (result f32) (return (f32.const 78.7)))
  (func (export "return-f64") (result f64) (return (f64.const 78.78)))
  (func (export "return-i32-f64") (result i32 f64)
    (return (i32.const 78) (f64.const 78.78))
  )
  (func (export "return-i32-i32-i32") (result i32 i32 i32)
    (return (i32.const 1) (i32.const 2) (i32.const 3))
  )
  (func (export "return-block-i32") (result i32)
    (return (block (result i32) (call $dummy) (i32.const 77)))
  )
  (func (export "return-block-i32-i64") (result i32 i64)
    (return (block (result i32 i64) (call $dummy) (i32.const 1) (i64.const 2)))
  )

  (func (export "break-empty") (br 0))
  (func (export "break-i32") (result i32) (br 0 (i32.const 79)))
  (func (export "break-i64") (result i64) (br 0 (i64.const 7979)))
  (func (export "break-f32") (result f32) (br 0 (f32.const 79.9)))
  (func (export "break-f64") (result f64) (br 0 (f64.const 79.79)))
  (func (export "break-i32-f64") (result i32 f64)
    (br 0 (i32.const 79) (f64.const 79.79))
  )
  (func (export "break-i32-i32-i32") (result i32 i32 i32)
    (br 0 (i32.const 1) (i32.const 2) (i32.const 3))
  )
  (func (export "break-block-i32") (result i32)
    (br 0 (block (result i32) (call $dummy) (i32.const 77)))
  )
  (func (export "break-block-i32-i64") (result i32 i64)
    (br 0 (block (result i32 i64) (call $dummy) (i32.const 1) (i64.const 2)))
  )

  (func (export "break-br_if-empty") (param i32)
    (br_if 0 (local.get 0))
  )
  (func (export "break-br_if-num") (param i32) (result i32)
    (drop (br_if 0 (i32.const 50) (local.get 0))) (i32.const 51)
  )
  (func (export "break-br_if-num-num") (param i32) (result i32 i64)
    (drop (drop (br_if 0 (i32.const 50) (i64.const 51) (local.get 0))))
    (i32.const 51) (i64.const 52)
  )

  (func (export "break-br_table-empty") (param i32)
    (br_table 0 0 0 (local.get 0))
  )
  (func (export "break-br_table-num") (param i32) (result i32)
    (br_table 0 0 (i32.const 50) (local.get 0)) (i32.const 51)
  )
  (func (export "break-br_table-num-num") (param i32) (result i32 i64)
    (br_table 0 0 (i32.const 50) (i64.const 51) (local.get 0))
    (i32.const 51) (i64.const 52)
  )
  (func (export "break-br_table-nested-empty") (param i32)
    (block (br_table 0 1 0 (local.get 0)))
  )
  (func (export "break-br_table-nested-num") (param i32) (result i32)
    (i32.add
      (block (result i32)
        (br_table 0 1 0 (i32.const 50) (local.get 0)) (i32.const 51)
      )
      (i32.const 2)
    )
  )
  (func (export "break-br_table-nested-num-num") (param i32) (result i32 i32)
    (i32.add
      (block (result i32 i32)
        (br_table 0 1 0 (i32.const 50) (i32.const 51) (local.get 0))
        (i32.const 51) (i32.const -3)
      )
    )
    (i32.const 52)
  )

  ;; Large signatures

  (func (export "large-sig")
    (param i32 i64 f32 f32 i32 f64 f32 i32 i32 i32 f32 f64 f64 f64 i32 i32 f32)
    (result f64 f32 i32 i32 i32 i64 f32 i32 i32 f32 f64 f64 i32 f32 i32 f64)
    (local.get 5)
    (local.get 2)
    (local.get 0)
    (local.get 8)
    (local.get 7)
    (local.get 1)
    (local.get 3)
    (local.get 9)
    (local.get 4)
    (local.get 6)
    (local.get 13)
    (local.get 11)
    (local.get 15)
    (local.get 16)
    (local.get 14)
    (local.get 12)
  )

  ;; Default initialization of locals

  (func (export "init-local-i32") (result i32) (local i32) (local.get 0))
  (func (export "init-local-i64") (result i64) (local i64) (local.get 0))
  (func (export "init-local-f32") (result f32) (local f32) (local.get 0))
  (func (export "init-local-f64") (result f64) (local f64) (local.get 0))
)

(assert_return (invoke "type-use-1"))
(assert_return (invoke "type-use-2") (i32.const 0))
(assert_return (invoke "type-use-3" (i32.const 1)))
(assert_return
  (invoke "type-use-4" (i32.const 1) (f64.const 1) (i32.const 1))
  (i32.const 0)
)
(assert_return (invoke "type-use-5") (i32.const 0))
(assert_return (invoke "type-use-6" (i32.const 1)))
(assert_return
  (invoke "type-use-7" (i32.const 1) (f64.const 1) (i32.const 1))
  (i32.const 0)
)

(assert_return (invoke "local-first-i32") (i32.const 0))
(assert_return (invoke "local-first-i64") (i64.const 0))
(assert_return (invoke "local-first-f32") (f32.const 0))
(assert_return (invoke "local-first-f64") (f64.const 0))
(assert_return (invoke "local-second-i32") (i32.const 0))
(assert_return (invoke "local-second-i64") (i64.const 0))
(assert_return (invoke "local-second-f32") (f32.const 0))
(assert_return (invoke "local-second-f64") (f64.const 0))
(assert_return (invoke "local-mixed") (f64.const 0))

(assert_return
  (invoke "param-first-i32" (i32.const 2) (i32.const 3)) (i32.const 2)
)
(assert_return
  (invoke "param-first-i64" (i64.const 2) (i64.const 3)) (i64.const 2)
)
(assert_return
  (invoke "param-first-f32" (f32.const 2) (f32.const 3)) (f32.const 2)
)
(assert_return
  (invoke "param-first-f64" (f64.const 2) (f64.const 3)) (f64.const 2)
)
(assert_return
  (invoke "param-second-i32" (i32.const 2) (i32.const 3)) (i32.const 3)
)
(assert_return
  (invoke "param-second-i64" (i64.const 2) (i64.const 3)) (i64.const 3)
)
(assert_return
  (invoke "param-second-f32" (f32.const 2) (f32.const 3)) (f32.const 3)
)
(assert_return
  (invoke "param-second-f64" (f64.const 2) (f64.const 3)) (f64.const 3)
)

(assert_return
  (invoke "param-mixed"
    (f32.const 1) (i32.const 2) (i64.const 3)
    (i32.const 4) (f64.const 5.5) (i32.const 6)
  )
  (f64.const 5.5)
)

(assert_return (invoke "empty"))
(assert_return (invoke "value-void"))
(assert_return (invoke "value-i32") (i32.const 77))
(assert_return (invoke "value-i64") (i64.const 7777))
(assert_return (invoke "value-f32") (f32.const 77.7))
(assert_return (invoke "value-f64") (f64.const 77.77))
(assert_return (invoke "value-i32-f64") (i32.const 77) (f64.const 7))
(assert_return (invoke "value-i32-i32-i32")
  (i32.const 1) (i32.const 2) (i32.const 3)
)
(assert_return (invoke "value-block-void"))
(assert_return (invoke "value-block-i32") (i32.const 77))
(assert_return (invoke "value-block-i32-i64") (i32.const 1) (i64.const 2))

(assert_return (invoke "return-empty"))
(assert_return (invoke "return-i32") (i32.const 78))
(assert_return (invoke "return-i64") (i64.const 7878))
(assert_return (invoke "return-f32") (f32.const 78.7))
(assert_return (invoke "return-f64") (f64.const 78.78))
(assert_return (invoke "return-i32-f64") (i32.const 78) (f64.const 78.78))
(assert_return (invoke "return-i32-i32-i32")
  (i32.const 1) (i32.const 2) (i32.const 3)
)
(assert_return (invoke "return-block-i32") (i32.const 77))
(assert_return (invoke "return-block-i32-i64") (i32.const 1) (i64.const 2))

(assert_return (invoke "break-empty"))
(assert_return (invoke "break-i32") (i32.const 79))
(assert_return (invoke "break-i64") (i64.const 7979))
(assert_return (invoke "break-f32") (f32.const 79.9))
(assert_return (invoke "break-f64") (f64.const 79.79))
(assert_return (invoke "break-i32-f64") (i32.const 79) (f64.const 79.79))
(assert_return (invoke "break-i32-i32-i32")
  (i32.const 1) (i32.const 2) (i32.const 3)
)
(assert_return (invoke "break-block-i32") (i32.const 77))
(assert_return (invoke "break-block-i32-i64") (i32.const 1) (i64.const 2))

(assert_return (invoke "break-br_if-empty" (i32.const 0)))
(assert_return (invoke "break-br_if-empty" (i32.const 2)))
(assert_return (invoke "break-br_if-num" (i32.const 0)) (i32.const 51))
(assert_return (invoke "break-br_if-num" (i32.const 1)) (i32.const 50))
(assert_return (invoke "break-br_if-num-num" (i32.const 0))
  (i32.const 51) (i64.const 52)
)
(assert_return (invoke "break-br_if-num-num" (i32.const 1))
  (i32.const 50) (i64.const 51)
)

(assert_return (invoke "break-br_table-empty" (i32.const 0)))
(assert_return (invoke "break-br_table-empty" (i32.const 1)))
(assert_return (invoke "break-br_table-empty" (i32.const 5)))
(assert_return (invoke "break-br_table-empty" (i32.const -1)))
(assert_return (invoke "break-br_table-num" (i32.const 0)) (i32.const 50))
(assert_return (invoke "break-br_table-num" (i32.const 1)) (i32.const 50))
(assert_return (invoke "break-br_table-num" (i32.const 10)) (i32.const 50))
(assert_return (invoke "break-br_table-num" (i32.const -100)) (i32.const 50))
(assert_return (invoke "break-br_table-num-num" (i32.const 0))
  (i32.const 50) (i64.const 51)
)
(assert_return (invoke "break-br_table-num-num" (i32.const 1))
  (i32.const 50) (i64.const 51)
)
(assert_return (invoke "break-br_table-num-num" (i32.const 10))
  (i32.const 50) (i64.const 51)
)
(assert_return (invoke "break-br_table-num-num" (i32.const -100))
  (i32.const 50) (i64.const 51)
)
(assert_return (invoke "break-br_table-nested-empty" (i32.const 0)))
(assert_return (invoke "break-br_table-nested-empty" (i32.const 1)))
(assert_return (invoke "break-br_table-nested-empty" (i32.const 3)))
(assert_return (invoke "break-br_table-nested-empty" (i32.const -2)))
(assert_return
  (invoke "break-br_table-nested-num" (i32.const 0)) (i32.const 52)
)
(assert_return
  (invoke "break-br_table-nested-num" (i32.const 1)) (i32.const 50)
)
(assert_return
  (invoke "break-br_table-nested-num" (i32.const 2)) (i32.const 52)
)
(assert_return
  (invoke "break-br_table-nested-num" (i32.const -3)) (i32.const 52)
)
(assert_return
  (invoke "break-br_table-nested-num-num" (i32.const 0))
  (i32.const 101) (i32.const 52)
)
(assert_return
  (invoke "break-br_table-nested-num-num" (i32.const 1))
  (i32.const 50) (i32.const 51)
)
(assert_return
  (invoke "break-br_table-nested-num-num" (i32.const 2))
  (i32.const 101) (i32.const 52)
)
(assert_return
  (invoke "break-br_table-nested-num-num" (i32.const -3))
  (i32.const 101) (i32.const 52)
)

(assert_return
  (invoke "large-sig"
    (i32.const 0) (i64.const 1) (f32.const 2) (f32.const 3)
    (i32.const 4) (f64.const 5) (f32.const 6) (i32.const 7)
    (i32.const 8) (i32.const 9) (f32.const 10) (f64.const 11)
    (f64.const 12) (f64.const 13) (i32.const 14) (i32.const 15)
    (f32.const 16)
  )
  (f64.const 5) (f32.const 2) (i32.const 0) (i32.const 8)
  (i32.const 7) (i64.const 1) (f32.const 3) (i32.const 9)
  (i32.const 4) (f32.const 6) (f64.const 13) (f64.const 11)
  (i32.const 15) (f32.const 16) (i32.const 14) (f64.const 12)
)

(assert_return (invoke "init-local-i32") (i32.const 0))
(assert_return (invoke "init-local-i64") (i64.const 0))
(assert_return (invoke "init-local-f32") (f32.const 0))
(assert_return (invoke "init-local-f64") (f64.const 0))


;; Expansion of inline function types

(module
  (func $f (result f64) (f64.const 0))  ;; adds implicit type definition
  (func $g (param i32))                 ;; reuses explicit type definition
  (type $t (func (param i32)))

  (func $i32->void (type 0))                ;; (param i32)
  (func $void->f64 (type 1) (f64.const 0))  ;; (result f64)
  (func $check
    (call $i32->void (i32.const 0))
    (drop (call $void->f64))
  )
)

(assert_invalid
  (module
    (func $f (result f64) (f64.const 0))  ;; adds implicit type definition
    (func $g (param i32))                 ;; reuses explicit type definition
    (func $h (result f64) (f64.const 1))  ;; reuses implicit type definition
    (type $t (func (param i32)))

    (func (type 2))  ;; does not exist
  )
  "unknown type"
)


(module
  (type $sig (func))

  (func $empty-sig-1)  ;; should be assigned type $sig
  (func $complex-sig-1 (param f64 i64 f64 i64 f64 i64 f32 i32))
  (func $empty-sig-2)  ;; should be assigned type $sig
  (func $complex-sig-2 (param f64 i64 f64 i64 f64 i64 f32 i32))
  (func $complex-sig-3 (param f64 i64 f64 i64 f64 i64 f32 i32))
  (func $complex-sig-4 (param i64 i64 f64 i64 f64 i64 f32 i32))
  (func $complex-sig-5 (param i64 i64 f64 i64 f64 i64 f32 i32))

  (type $empty-sig-duplicate (func))
  (type $complex-sig-duplicate (func (param i64 i64 f64 i64 f64 i64 f32 i32)))
  (table funcref
    (elem
      $complex-sig-3 $empty-sig-2 $complex-sig-1 $complex-sig-3 $empty-sig-1
      $complex-sig-4 $complex-sig-5
    )
  )

  (func (export "signature-explicit-reused")
    (call_indirect (type $sig) (i32.const 1))
    (call_indirect (type $sig) (i32.const 4))
  )

  (func (export "signature-implicit-reused")
    ;; The implicit index 3 in this test depends on the function and
    ;; type definitions, and may need adapting if they change.
    (call_indirect (type 3)
      (f64.const 0) (i64.const 0) (f64.const 0) (i64.const 0)
      (f64.const 0) (i64.const 0) (f32.const 0) (i32.const 0)
      (i32.const 0)
    )
    (call_indirect (type 3)
      (f64.const 0) (i64.const 0) (f64.const 0) (i64.const 0)
      (f64.const 0) (i64.const 0) (f32.const 0) (i32.const 0)
      (i32.const 2)
    )
    (call_indirect (type 3)
      (f64.const 0) (i64.const 0) (f64.const 0) (i64.const 0)
      (f64.const 0) (i64.const 0) (f32.const 0) (i32.const 0)
      (i32.const 3)
    )
  )

  (func (export "signature-explicit-duplicate")
    (call_indirect (type $empty-sig-duplicate) (i32.const 1))
  )

  (func (export "signature-implicit-duplicate")
    (call_indirect (type $complex-sig-duplicate)
      (i64.const 0) (i64.const 0) (f64.const 0) (i64.const 0)
      (f64.const 0) (i64.const 0) (f32.const 0) (i32.const 0)
      (i32.const 5)
    )
    (call_indirect (type $complex-sig-duplicate)
      (i64.const 0) (i64.const 0) (f64.const 0) (i64.const 0)
      (f64.const 0) (i64.const 0) (f32.const 0) (i32.const 0)
      (i32.const 6)
    )
  )
)

(assert_return (invoke "signature-explicit-reused"))
(assert_return (invoke "signature-implicit-reused"))
(assert_return (invoke "signature-explicit-duplicate"))
(assert_return (invoke "signature-implicit-duplicate"))


;; Malformed type use

(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (type $sig) (result i32) (param i32) (i32.const 0))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (param i32) (type $sig) (result i32) (i32.const 0))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (param i32) (result i32) (type $sig) (i32.const 0))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (result i32) (type $sig) (param i32) (i32.const 0))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (result i32) (param i32) (type $sig) (i32.const 0))"
  )
  "unexpected token"
)
(assert_malformed
  (module quote
    "(func (result i32) (param i32) (i32.const 0))"
  )
  "unexpected token"
)

(assert_malformed
  (module quote
    "(type $sig (func))"
    "(func (type $sig) (result i32) (i32.const 0))"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (type $sig) (result i32) (i32.const 0))"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32) (result i32)))"
    "(func (type $sig) (param i32) (i32.const 0))"
  )
  "inline function type"
)
(assert_malformed
  (module quote
    "(type $sig (func (param i32 i32) (result i32)))"
    "(func (type $sig) (param i32) (result i32) (unreachable))"
  )
  "inline function type"
)


;; Invalid typing of locals

(assert_invalid
  (module (func $type-local-num-vs-num (result i64) (local i32) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-local-num-vs-num (local f32) (i32.eqz (local.get 0))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-local-num-vs-num (local f64 i64) (f64.neg (local.get 1))))
  "type mismatch"
)


;; Invalid typing of parameters

(assert_invalid
  (module (func $type-param-num-vs-num (param i32) (result i64) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-num (param f32) (i32.eqz (local.get 0))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-num (param f64 i64) (f64.neg (local.get 1))))
  "type mismatch"
)


;; Invalid typing of result

(assert_invalid
  (module (func $type-empty-i32 (result i32)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-i64 (result i64)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f32 (result f32)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f64 (result f64)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-f64-i32 (result f64 i32)))
  "type mismatch"
)

(assert_invalid
  (module (func $type-value-void-vs-num (result i32)
    (nop)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-void-vs-nums (result i32 i32)
    (nop)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-num-vs-void
    (i32.const 0)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-nums-vs-void
    (i32.const 0) (i64.const 0)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-num-vs-num (result i32)
    (f32.const 0)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-num-vs-nums (result f32 f32)
    (f32.const 0)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-value-nums-vs-num (result f32)
    (f32.const 0) (f32.const 0)
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-return-last-empty-vs-num (result i32)
    (return)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-last-empty-vs-nums (result i32 i32)
    (return)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-last-void-vs-num (result i32)
    (return (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-last-void-vs-nums (result i32 i64)
    (return (nop))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-last-num-vs-num (result i32)
    (return (i64.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-last-num-vs-nums (result i64 i64)
    (return (i64.const 0))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-return-empty-vs-num (result i32)
    (return) (i32.const 1)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-empty-vs-nums (result i32 i32)
    (return) (i32.const 1) (i32.const 2)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-partial-vs-nums (result i32 i32)
    (i32.const 1) (return) (i32.const 2)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-void-vs-num (result i32)
    (return (nop)) (i32.const 1)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-void-vs-nums (result i32 i32)
    (return (nop)) (i32.const 1)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-num-vs-num (result i32)
    (return (i64.const 1)) (i32.const 1)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-num-vs-nums (result i32 i32)
    (return (i64.const 1)) (i32.const 1) (i32.const 2)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-first-num-vs-num (result i32)
    (return (i64.const 1)) (return (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-return-first-num-vs-nums (result i32 i32)
    (return (i32.const 1)) (return (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-last-void-vs-num (result i32)
    (br 0)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-void-vs-nums (result i32 i32)
    (br 0)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-num-vs-num (result i32)
    (br 0 (f32.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-last-num-vs-nums (result i32 i32)
    (br 0 (i32.const 0))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-void-vs-num (result i32)
    (br 0) (i32.const 1)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-void-vs-nums (result i32 i32)
    (br 0) (i32.const 1) (i32.const 2)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-num-vs-num (result i32)
    (br 0 (i64.const 1)) (i32.const 1)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-num-vs-nums (result i32 i32)
    (br 0 (i32.const 1)) (i32.const 1) (i32.const 2)
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-first-num-vs-num (result i32)
    (br 0 (i64.const 1)) (br 0 (i32.const 1))
  ))
  "type mismatch"
)

(assert_invalid
  (module (func $type-break-nested-empty-vs-num (result i32)
    (block (br 1)) (br 0 (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-empty-vs-nums (result i32 i32)
    (block (br 1)) (br 0 (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-void-vs-num (result i32)
    (block (br 1 (nop))) (br 0 (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-void-vs-nums (result i32 i32)
    (block (br 1 (nop))) (br 0 (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-num-vs-num (result i32)
    (block (br 1 (i64.const 1))) (br 0 (i32.const 1))
  ))
  "type mismatch"
)
(assert_invalid
  (module (func $type-break-nested-num-vs-nums (result i32 i32)
    (block (result i32) (br 1 (i32.const 1))) (br 0 (i32.const 1) (i32.const 2))
  ))
  "type mismatch"
)


;; Syntax errors

(assert_malformed
  (module quote "(func (nop) (local i32))")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (nop) (param i32))")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (nop) (result i32))")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (local i32) (param i32))")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (local i32) (result i32) (local.get 0))")
  "unexpected token"
)
(assert_malformed
  (module quote "(func (result i32) (param i32) (local.get 0))")
  "unexpected token"
)
