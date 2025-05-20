;; Test try-catch blocks.

(module
  (tag $e0 (export "e0"))
  (func (export "throw") (throw $e0))
)

(register "test")

(module
  (tag $imported-e0 (import "test" "e0"))
  (tag $imported-e0-alias (import "test" "e0"))
  (func $imported-throw (import "test" "throw"))
  (tag $e0)
  (tag $e1)
  (tag $e2)
  (tag $e-i32 (param i32))
  (tag $e-f32 (param f32))
  (tag $e-i64 (param i64))
  (tag $e-f64 (param f64))

  (func $throw-if (param i32) (result i32)
    (local.get 0)
    (i32.const 0) (if (i32.ne) (then (throw $e0)))
    (i32.const 0)
  )

  (func (export "simple-throw-catch") (param i32) (result i32)
    (block $h
      (try_table (result i32) (catch $e0 $h)
        (if (i32.eqz (local.get 0)) (then (throw $e0)) (else))
        (i32.const 42)
      )
      (return)
    )
    (i32.const 23)
  )

  (func (export "unreachable-not-caught")
    (block $h
      (try_table (catch_all $h) (unreachable))
      (return)
    )
  )

  (func $div (param i32 i32) (result i32)
    (local.get 0) (local.get 1) (i32.div_u)
  )
  (func (export "trap-in-callee") (param i32 i32) (result i32)
    (block $h
      (try_table (result i32) (catch_all $h)
        (call $div (local.get 0) (local.get 1))
      )
      (return)
    )
    (i32.const 11)
  )

  (func (export "catch-complex-1") (param i32) (result i32)
    (block $h1
      (try_table (result i32) (catch $e1 $h1)
        (block $h0
          (try_table (result i32) (catch $e0 $h0)
            (if (i32.eqz (local.get 0))
              (then (throw $e0))
              (else
                (if (i32.eq (local.get 0) (i32.const 1))
                  (then (throw $e1))
                  (else (throw $e2))
                )
              )
            )
            (i32.const 2)
          )
          (br 1)
        )
        (i32.const 3)
      )
      (return)
    )
    (i32.const 4)
  )

  (func (export "catch-complex-2") (param i32) (result i32)
    (block $h0
      (block $h1
        (try_table (result i32) (catch $e0 $h0) (catch $e1 $h1)
          (if (i32.eqz (local.get 0))
            (then (throw $e0))
            (else
              (if (i32.eq (local.get 0) (i32.const 1))
                (then (throw $e1))
                (else (throw $e2))
              )
            )
           )
          (i32.const 2)
        )
        (return)
      )
      (return (i32.const 4))
    )
    (i32.const 3)
  )

  (func (export "throw-catch-param-i32") (param i32) (result i32)
    (block $h (result i32)
      (try_table (result i32) (catch $e-i32 $h)
        (throw $e-i32 (local.get 0))
        (i32.const 2)
      )
      (return)
    )
    (return)
  )

  (func (export "throw-catch-param-f32") (param f32) (result f32)
    (block $h (result f32)
      (try_table (result f32) (catch $e-f32 $h)
        (throw $e-f32 (local.get 0))
        (f32.const 0)
      )
      (return)
    )
    (return)
  )

  (func (export "throw-catch-param-i64") (param i64) (result i64)
    (block $h (result i64)
      (try_table (result i64) (catch $e-i64 $h)
        (throw $e-i64 (local.get 0))
        (i64.const 2)
      )
      (return)
    )
    (return)
  )

  (func (export "throw-catch-param-f64") (param f64) (result f64)
    (block $h (result f64)
      (try_table (result f64) (catch $e-f64 $h)
        (throw $e-f64 (local.get 0))
        (f64.const 0)
      )
      (return)
    )
    (return)
  )

  (func (export "throw-catch_ref-param-i32") (param i32) (result i32)
    (block $h (result i32 exnref)
      (try_table (result i32) (catch_ref $e-i32 $h)
        (throw $e-i32 (local.get 0))
        (i32.const 2)
      )
      (return)
    )
    (drop) (return)
  )

  (func (export "throw-catch_ref-param-f32") (param f32) (result f32)
    (block $h (result f32 exnref)
      (try_table (result f32) (catch_ref $e-f32 $h)
        (throw $e-f32 (local.get 0))
        (f32.const 0)
      )
      (return)
    )
    (drop) (return)
  )

  (func (export "throw-catch_ref-param-i64") (param i64) (result i64)
    (block $h (result i64 exnref)
      (try_table (result i64) (catch_ref $e-i64 $h)
        (throw $e-i64 (local.get 0))
        (i64.const 2)
      )
      (return)
    )
    (drop) (return)
  )

  (func (export "throw-catch_ref-param-f64") (param f64) (result f64)
    (block $h (result f64 exnref)
      (try_table (result f64) (catch_ref $e-f64 $h)
        (throw $e-f64 (local.get 0))
        (f64.const 0)
      )
      (return)
    )
    (drop) (return)
  )

  (func $throw-param-i32 (param i32) (throw $e-i32 (local.get 0)))
  (func (export "catch-param-i32") (param i32) (result i32)
    (block $h (result i32)
      (try_table (result i32) (catch $e-i32 $h)
        (i32.const 0)
        (call $throw-param-i32 (local.get 0))
      )
      (return)
    )
  )

  (func (export "catch-imported") (result i32)
    (block $h
      (try_table (result i32) (catch $imported-e0 $h)
        (call $imported-throw (i32.const 1))
      )
      (return)
    )
    (i32.const 2)
  )

  (func (export "catch-imported-alias") (result i32)
    (block $h
      (try_table (result i32) (catch $imported-e0 $h)
        (throw $imported-e0-alias (i32.const 1))
      )
      (return)
    )
    (i32.const 2)
  )

  ;; (xdoardo) Disabled for now (requires rethrow)
  ;; (func (export "catchless-try") (param i32) (result i32)
  ;;   (block $h
  ;;     (try_table (result i32) (catch $e0 $h)
  ;;       (try_table (result i32) (call $throw-if (local.get 0)))
  ;;     )
  ;;     (return)
  ;;   )
  ;;   (i32.const 1)
  ;; )

  (func $throw-void (throw $e0))
  ;; (xdoardo): Disabled for now (requires tail-call proposal implementation)
  ;; (func (export "return-call-in-try-catch")
  ;;   (block $h
  ;;     (try_table (catch $e0 $h)
  ;;       (return_call $throw-void)
  ;;     )
  ;;   )
  ;; )

  (table funcref (elem $throw-void))
  ;; (xdoardo): Disabled for now (requires tail-call proposal implementation)
  ;; (func (export "return-call-indirect-in-try-catch")
  ;;   (block $h
  ;;     (try_table (catch $e0 $h)
  ;;       (return_call_indirect (i32.const 0))
  ;;     )
  ;;   )
  ;; )

  ;; (xdoardo) Disabled for now (requires rethrow)
  ;; (func (export "try-with-param")
  ;;   (i32.const 0) (try_table (param i32) (drop))
  ;; )
)

(assert_return (invoke "simple-throw-catch" (i32.const 0)) (i32.const 23))
(assert_return (invoke "simple-throw-catch" (i32.const 1)) (i32.const 42))

(assert_trap (invoke "unreachable-not-caught") "unreachable")

(assert_return (invoke "trap-in-callee" (i32.const 7) (i32.const 2)) (i32.const 3))
(assert_trap (invoke "trap-in-callee" (i32.const 1) (i32.const 0)) "integer divide by zero")

(assert_return (invoke "catch-complex-1" (i32.const 0)) (i32.const 3))
(assert_return (invoke "catch-complex-1" (i32.const 1)) (i32.const 4))
(assert_exception (invoke "catch-complex-1" (i32.const 2)))

(assert_return (invoke "catch-complex-2" (i32.const 0)) (i32.const 3))
(assert_return (invoke "catch-complex-2" (i32.const 1)) (i32.const 4))
(assert_exception (invoke "catch-complex-2" (i32.const 2)))

(assert_return (invoke "throw-catch-param-i32" (i32.const 0)) (i32.const 0))
(assert_return (invoke "throw-catch-param-i32" (i32.const 1)) (i32.const 1))
(assert_return (invoke "throw-catch-param-i32" (i32.const 10)) (i32.const 10))

(assert_return (invoke "throw-catch-param-f32" (f32.const 5.0)) (f32.const 5.0))
(assert_return (invoke "throw-catch-param-f32" (f32.const 10.5)) (f32.const 10.5))

(assert_return (invoke "throw-catch-param-i64" (i64.const 5)) (i64.const 5))
(assert_return (invoke "throw-catch-param-i64" (i64.const 0)) (i64.const 0))
(assert_return (invoke "throw-catch-param-i64" (i64.const -1)) (i64.const -1))

(assert_return (invoke "throw-catch-param-f64" (f64.const 5.0)) (f64.const 5.0))
(assert_return (invoke "throw-catch-param-f64" (f64.const 10.5)) (f64.const 10.5))

(assert_return (invoke "throw-catch_ref-param-i32" (i32.const 0)) (i32.const 0))
(assert_return (invoke "throw-catch_ref-param-i32" (i32.const 1)) (i32.const 1))
(assert_return (invoke "throw-catch_ref-param-i32" (i32.const 10)) (i32.const 10))

(assert_return (invoke "throw-catch_ref-param-f32" (f32.const 5.0)) (f32.const 5.0))
(assert_return (invoke "throw-catch_ref-param-f32" (f32.const 10.5)) (f32.const 10.5))

(assert_return (invoke "throw-catch_ref-param-i64" (i64.const 5)) (i64.const 5))
(assert_return (invoke "throw-catch_ref-param-i64" (i64.const 0)) (i64.const 0))
(assert_return (invoke "throw-catch_ref-param-i64" (i64.const -1)) (i64.const -1))

(assert_return (invoke "throw-catch_ref-param-f64" (f64.const 5.0)) (f64.const 5.0))
(assert_return (invoke "throw-catch_ref-param-f64" (f64.const 10.5)) (f64.const 10.5))

(assert_return (invoke "catch-param-i32" (i32.const 5)) (i32.const 5))

(assert_return (invoke "catch-imported") (i32.const 2))
;; (assert_return (invoke "catch-imported-alias") (i32.const 2))

;;(assert_return (invoke "catchless-try" (i32.const 0)) (i32.const 0))
;;(assert_return (invoke "catchless-try" (i32.const 1)) (i32.const 1))

;; (assert_exception (invoke "return-call-in-try-catch"))
;; (assert_exception (invoke "return-call-indirect-in-try-catch"))

;; (assert_return (invoke "try-with-param"))

 (module
   (func $imported-throw (import "test" "throw"))
   (tag $e0)
 
   (func (export "imported-mismatch") (result i32)
     (block $h
       (try_table (result i32) (catch_all $h)
         (block $h0
           (try_table (result i32) (catch $e0 $h0)
             (i32.const 1)
             (call $imported-throw)
           )
           (return)
         )
         (i32.const 2)
       )
       (return)
     )
     (i32.const 3)
   )
 )
 
;; (assert_return (invoke "imported-mismatch") (i32.const 3))
 
 (assert_malformed
   (module quote "(module (func (catch_all)))")
   "unexpected token"
 )
 
 (assert_malformed
   (module quote "(module (tag $e) (func (catch $e)))")
   "unexpected token"
 )
 
 (module
   (tag $e)
   (func (try_table (catch $e 0) (catch $e 0)))
   (func (try_table (catch_all 0) (catch $e 0)))
   (func (try_table (catch_all 0) (catch_all 0)))
   (func (result exnref) (try_table (catch_ref $e 0) (catch_ref $e 0)) (unreachable))
   (func (result exnref) (try_table (catch_all_ref 0) (catch_ref $e 0)) (unreachable))
   (func (result exnref) (try_table (catch_all_ref 0) (catch_all_ref 0)) (unreachable))
 )
 
 (assert_invalid
   (module (func (result i32) (try_table (result i32))))
   "type mismatch"
 )
 (assert_invalid
   (module (func (result i32) (try_table (result i32) (i64.const 42))))
   "type mismatch"
 )
 
 (assert_invalid
   (module (tag) (func (try_table (catch_ref 0 0))))
   "type mismatch"
 )
 (assert_invalid
   (module (tag) (func (result exnref) (try_table (catch 0 0)) (unreachable)))
   "type mismatch"
 )
 (assert_invalid
   (module (func (try_table (catch_all_ref 0))))
   "type mismatch"
 )
 (assert_invalid
   (module (func (result exnref) (try_table (catch_all 0)) (unreachable)))
   "type mismatch"
 )
 (assert_invalid
   (module
     (tag (param i64))
     (func (result i32 exnref) (try_table (result i32) (catch_ref 0 0) (i32.const 42)))
   )
   "type mismatch"
 )
