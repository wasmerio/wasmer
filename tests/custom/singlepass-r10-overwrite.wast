;; A bug was introduced in the commit below, where the `R10` register is incorrectly overwritten.
;; This test case covers this specific case.
;;
;; https://github.com/wasmerio/wasmer/commit/ed826cb389b17273002e729160bf076213b7e2f2#diff-8c30560d501545a19acafa7ebb21ebfeR1784
;;

(module
  (func $call_target (param i64) (param i64) (param i64) (param i64) (param i64) (param i64) (result i64)
    (local.get 0)
  )

  (func (export "test") (result i64)
    ;; Use `i64.add`s to actually push values onto the runtime stack.

    ;; rsi
    (i64.const 1)
    (i64.const 1)
    (i64.add)

    ;; rdi
    (i64.const 1)
    (i64.const 1)
    (i64.add)

    ;; r8
    (i64.const 1)
    (i64.const 1)
    (i64.add)

    ;; r9
    (i64.const 1)
    (i64.const 1)
    (i64.add)

    ;; r10 (!)
    (i64.const 1)
    (i64.const 1)
    (i64.add)

    ;; Imm64's as arguments
    (i64.const 1)
    (i64.const 1)
    (i64.const 1)
    (i64.const 1)
    (i64.const 1) ;; allocated to the first memory slot

    ;; call
    (call $call_target)
    (return)
  )
)

(assert_return (invoke "test") (i64.const 2))
