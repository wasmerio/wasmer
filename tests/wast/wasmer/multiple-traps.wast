;; We assert that we can call a function that traps repeatedly

(module
  (func (export "throw_trap")
    unreachable
  ))

(assert_trap (invoke "throw_trap") "unreachable")
(assert_trap (invoke "throw_trap") "unreachable")
(assert_trap (invoke "throw_trap") "unreachable")
