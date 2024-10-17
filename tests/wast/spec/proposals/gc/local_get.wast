;; Test `local.get` operator

(module
  ;; Typing

  (func (export "type-local-i32") (result i32) (local i32) (local.get 0))
  (func (export "type-local-i64") (result i64) (local i64) (local.get 0))
  (func (export "type-local-f32") (result f32) (local f32) (local.get 0))
  (func (export "type-local-f64") (result f64) (local f64) (local.get 0))

  (func (export "type-param-i32") (param i32) (result i32) (local.get 0))
  (func (export "type-param-i64") (param i64) (result i64) (local.get 0))
  (func (export "type-param-f32") (param f32) (result f32) (local.get 0))
  (func (export "type-param-f64") (param f64) (result f64) (local.get 0))

  (func (export "type-mixed") (param i64 f32 f64 i32 i32)
    (local f32 i64 i64 f64)
    (drop (i64.eqz (local.get 0)))
    (drop (f32.neg (local.get 1)))
    (drop (f64.neg (local.get 2)))
    (drop (i32.eqz (local.get 3)))
    (drop (i32.eqz (local.get 4)))
    (drop (f32.neg (local.get 5)))
    (drop (i64.eqz (local.get 6)))
    (drop (i64.eqz (local.get 7)))
    (drop (f64.neg (local.get 8)))
  )

  ;; Reading

  (func (export "read") (param i64 f32 f64 i32 i32) (result f64)
    (local f32 i64 i64 f64)
    (local.set 5 (f32.const 5.5))
    (local.set 6 (i64.const 6))
    (local.set 8 (f64.const 8))
    (f64.add
      (f64.convert_i64_u (local.get 0))
      (f64.add
        (f64.promote_f32 (local.get 1))
        (f64.add
          (local.get 2)
          (f64.add
            (f64.convert_i32_u (local.get 3))
            (f64.add
              (f64.convert_i32_s (local.get 4))
              (f64.add
                (f64.promote_f32 (local.get 5))
                (f64.add
                  (f64.convert_i64_u (local.get 6))
                  (f64.add
                    (f64.convert_i64_u (local.get 7))
                    (local.get 8)
                  )
                )
              )
            )
          )
        )
      )
    )
  )

  ;; As parameter of control constructs and instructions

  (func (export "as-block-value") (param i32) (result i32)
    (block (result i32) (local.get 0))
  )
  (func (export "as-loop-value") (param i32) (result i32)
    (loop (result i32) (local.get 0))
  )
  (func (export "as-br-value") (param i32) (result i32)
    (block (result i32) (br 0 (local.get 0)))
  )
  (func (export "as-br_if-value") (param i32) (result i32)
    (block $l0 (result i32) (br_if $l0 (local.get 0) (i32.const 1)))
  )

  (func (export "as-br_if-value-cond") (param i32) (result i32)
    (block (result i32)
      (br_if 0 (local.get 0) (local.get 0))
    )
  )
  (func (export "as-br_table-value") (param i32) (result i32)
    (block
      (block
        (block
          (br_table 0 1 2 (local.get 0))
          (return (i32.const 0))
        )
        (return (i32.const 1))
      )
      (return (i32.const 2))
    )
    (i32.const 3)
  )

  (func (export "as-return-value") (param i32) (result i32)
    (return (local.get 0))
  )

  (func (export "as-if-then") (param i32) (result i32)
    (if (result i32) (local.get 0) (then (local.get 0)) (else (i32.const 0)))
  )
  (func (export "as-if-else") (param i32) (result i32)
    (if (result i32) (local.get 0) (then (i32.const 1)) (else (local.get 0)))
  )
)

(assert_return (invoke "type-local-i32") (i32.const 0))
(assert_return (invoke "type-local-i64") (i64.const 0))
(assert_return (invoke "type-local-f32") (f32.const 0))
(assert_return (invoke "type-local-f64") (f64.const 0))

(assert_return (invoke "type-param-i32" (i32.const 2)) (i32.const 2))
(assert_return (invoke "type-param-i64" (i64.const 3)) (i64.const 3))
(assert_return (invoke "type-param-f32" (f32.const 4.4)) (f32.const 4.4))
(assert_return (invoke "type-param-f64" (f64.const 5.5)) (f64.const 5.5))

(assert_return (invoke "as-block-value" (i32.const 6)) (i32.const 6))
(assert_return (invoke "as-loop-value" (i32.const 7)) (i32.const 7))

(assert_return (invoke "as-br-value" (i32.const 8)) (i32.const 8))
(assert_return (invoke "as-br_if-value" (i32.const 9)) (i32.const 9))
(assert_return (invoke "as-br_if-value-cond" (i32.const 10)) (i32.const 10))
(assert_return (invoke "as-br_table-value" (i32.const 1)) (i32.const 2))

(assert_return (invoke "as-return-value" (i32.const 0)) (i32.const 0))

(assert_return (invoke "as-if-then" (i32.const 1)) (i32.const 1))
(assert_return (invoke "as-if-else" (i32.const 0)) (i32.const 0))

(assert_return
  (invoke "type-mixed"
    (i64.const 1) (f32.const 2.2) (f64.const 3.3) (i32.const 4) (i32.const 5)
  )
)

(assert_return
  (invoke "read"
    (i64.const 1) (f32.const 2) (f64.const 3.3) (i32.const 4) (i32.const 5)
  )
  (f64.const 34.8)
)


;; Invalid typing of access to locals

(assert_invalid
  (module (func $type-local-num-vs-num (result i64) (local i32) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-local-num-vs-num (result i32) (local f32) (i32.eqz (local.get 0))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-local-num-vs-num (result f64) (local f64 i64) (f64.neg (local.get 1))))
  "type mismatch"
)


;; Invalid typing of access to parameters

(assert_invalid
  (module (func $type-param-num-vs-num (param i32) (result i64) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-num (param f32) (result i32) (i32.eqz (local.get 0))))
  "type mismatch"
)
(assert_invalid
  (module (func $type-param-num-vs-num (param f64 i64) (result f64) (f64.neg (local.get 1))))
  "type mismatch"
)


;; Invalid result type

(assert_invalid
  (module (func $type-empty-vs-i32 (local i32) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-vs-i64 (local i64) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-vs-f32 (local f32) (local.get 0)))
  "type mismatch"
)
(assert_invalid
  (module (func $type-empty-vs-f64 (local f64) (local.get 0)))
  "type mismatch"
)


;; Invalid local index

(assert_invalid
  (module (func $unbound-local (local i32 i64) (local.get 3) drop))
  "unknown local"
)
(assert_invalid
  (module (func $large-local (local i32 i64) (local.get 14324343) drop))
  "unknown local"
)

(assert_invalid
  (module (func $unbound-param (param i32 i64) (local.get 2) drop))
  "unknown local"
)
(assert_invalid
  (module (func $large-param (param i32 i64) (local.get 714324343) drop))
  "unknown local"
)

(assert_invalid
  (module (func $unbound-mixed (param i32) (local i32 i64) (local.get 3) drop))
  "unknown local"
)
(assert_invalid
  (module (func $large-mixed (param i64) (local i32 i64) (local.get 214324343) drop))
  "unknown local"
)
