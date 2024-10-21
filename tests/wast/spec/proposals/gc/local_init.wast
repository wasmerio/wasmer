;; Uninitialized undefaulted locals

(module
  (func (export "get-after-set") (param $p (ref extern)) (result (ref extern))
    (local $x (ref extern))
    (local.set $x (local.get $p))
    (local.get $x)
  )
  (func (export "get-after-tee") (param $p (ref extern)) (result (ref extern))
    (local $x (ref extern))
    (drop (local.tee $x (local.get $p)))
    (local.get $x)
  )
  (func (export "get-in-block-after-set") (param $p (ref extern)) (result (ref extern))
    (local $x (ref extern))
    (local.set $x (local.get $p))
    (block (result (ref extern)) (local.get $x))
  )
)

(assert_return (invoke "get-after-set" (ref.extern 1)) (ref.extern 1))
(assert_return (invoke "get-after-tee" (ref.extern 2)) (ref.extern 2))
(assert_return (invoke "get-in-block-after-set" (ref.extern 3)) (ref.extern 3))

(assert_invalid
  (module (func $uninit (local $x (ref extern)) (drop (local.get $x))))
  "uninitialized local"
)
(assert_invalid
  (module
    (func $uninit-after-end (param $p (ref extern))
      (local $x (ref extern))
      (block (local.set $x (local.get $p)) (drop (local.tee $x (local.get $p))))
      (drop (local.get $x))
    )
  )
  "uninitialized local"
)
(assert_invalid
  (module
    (func $uninit-in-else (param $p (ref extern))
      (local $x (ref extern))
      (if (i32.const 0)
        (then (local.set $x (local.get $p)))
	(else (local.get $x))
      )
    )
  )
  "uninitialized local"
)

(assert_invalid
  (module
    (func $uninit-from-if (param $p (ref extern))
      (local $x (ref extern))
      (if (i32.const 0)
        (then (local.set $x (local.get $p)))
	(else (local.set $x (local.get $p)))
      )
      (drop (local.get $x))
    )
  )
  "uninitialized local"
)

(module
  (func (export "tee-init") (param $p (ref extern)) (result (ref extern))
    (local $x (ref extern))
    (drop (local.tee $x (local.get $p)))
    (local.get $x)
  )
)

(assert_return (invoke "tee-init" (ref.extern 1)) (ref.extern 1))
