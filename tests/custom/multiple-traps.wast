;; We assert that we can call a function that traps repeatedly

(module
  (func (export "throw_trap")
    unreachable
  ))

(assert_trap (invoke "as-call_indirect-last") "unreachable")
(assert_trap (invoke "as-call_indirect-last") "unreachable")
(assert_trap (invoke "as-call_indirect-last") "unreachable")
