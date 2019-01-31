(module
  (func (export "i32.test") (result i32) (return (i32.const 0x0bAdD00D)))
  (func (export "i32.umax") (result i32) (return (i32.const 0xffffffff)))
  (func (export "i32.smax") (result i32) (return (i32.const 0x7fffffff)))
  (func (export "i32.neg_smax") (result i32) (return (i32.const -0x7fffffff)))
  (func (export "i32.smin") (result i32) (return (i32.const -0x80000000)))
  (func (export "i32.alt_smin") (result i32) (return (i32.const 0x80000000)))
  (func (export "i32.inc_smin") (result i32) (return (i32.add (i32.const -0x80000000) (i32.const 1))))
  (func (export "i32.neg_zero") (result i32) (return (i32.const -0x0)))
  (func (export "i32.not_octal") (result i32) (return (i32.const 010)))
  (func (export "i32.unsigned_decimal") (result i32) (return (i32.const 4294967295)))
  (func (export "i32.plus_sign") (result i32) (return (i32.const +42)))

  (func (export "i64.test") (result i64) (return (i64.const 0x0CABBA6E0ba66a6e)))
  (func (export "i64.umax") (result i64) (return (i64.const 0xffffffffffffffff)))
  (func (export "i64.smax") (result i64) (return (i64.const 0x7fffffffffffffff)))
  (func (export "i64.neg_smax") (result i64) (return (i64.const -0x7fffffffffffffff)))
  (func (export "i64.smin") (result i64) (return (i64.const -0x8000000000000000)))
  (func (export "i64.alt_smin") (result i64) (return (i64.const 0x8000000000000000)))
  (func (export "i64.inc_smin") (result i64) (return (i64.add (i64.const -0x8000000000000000) (i64.const 1))))
  (func (export "i64.neg_zero") (result i64) (return (i64.const -0x0)))
  (func (export "i64.not_octal") (result i64) (return (i64.const 010)))
  (func (export "i64.unsigned_decimal") (result i64) (return (i64.const 18446744073709551615)))
  (func (export "i64.plus_sign") (result i64) (return (i64.const +42)))

  (func (export "i32-dec-sep1") (result i32) (i32.const 1_000_000))
  (func (export "i32-dec-sep2") (result i32) (i32.const 1_0_0_0))
  (func (export "i32-hex-sep1") (result i32) (i32.const 0xa_0f_00_99))
  (func (export "i32-hex-sep2") (result i32) (i32.const 0x1_a_A_0_f))

  (func (export "i64-dec-sep1") (result i64) (i64.const 1_000_000))
  (func (export "i64-dec-sep2") (result i64) (i64.const 1_0_0_0))
  (func (export "i64-hex-sep1") (result i64) (i64.const 0xa_f00f_0000_9999))
  (func (export "i64-hex-sep2") (result i64) (i64.const 0x1_a_A_0_f))
)

(assert_return (invoke "i32.test") (i32.const 195940365))
(assert_return (invoke "i32.umax") (i32.const -1))
(assert_return (invoke "i32.smax") (i32.const 2147483647))
(assert_return (invoke "i32.neg_smax") (i32.const -2147483647))
(assert_return (invoke "i32.smin") (i32.const -2147483648))
(assert_return (invoke "i32.alt_smin") (i32.const -2147483648))
(assert_return (invoke "i32.inc_smin") (i32.const -2147483647))
(assert_return (invoke "i32.neg_zero") (i32.const 0))
(assert_return (invoke "i32.not_octal") (i32.const 10))
(assert_return (invoke "i32.unsigned_decimal") (i32.const -1))
(assert_return (invoke "i32.plus_sign") (i32.const 42))

(assert_return (invoke "i64.test") (i64.const 913028331277281902))
(assert_return (invoke "i64.umax") (i64.const -1))
(assert_return (invoke "i64.smax") (i64.const 9223372036854775807))
(assert_return (invoke "i64.neg_smax") (i64.const -9223372036854775807))
(assert_return (invoke "i64.smin") (i64.const -9223372036854775808))
(assert_return (invoke "i64.alt_smin") (i64.const -9223372036854775808))
(assert_return (invoke "i64.inc_smin") (i64.const -9223372036854775807))
(assert_return (invoke "i64.neg_zero") (i64.const 0))
(assert_return (invoke "i64.not_octal") (i64.const 10))
(assert_return (invoke "i64.unsigned_decimal") (i64.const -1))
(assert_return (invoke "i64.plus_sign") (i64.const 42))

(assert_return (invoke "i32-dec-sep1") (i32.const 1000000))
(assert_return (invoke "i32-dec-sep2") (i32.const 1000))
(assert_return (invoke "i32-hex-sep1") (i32.const 0xa0f0099))
(assert_return (invoke "i32-hex-sep2") (i32.const 0x1aa0f))

(assert_return (invoke "i64-dec-sep1") (i64.const 1000000))
(assert_return (invoke "i64-dec-sep2") (i64.const 1000))
(assert_return (invoke "i64-hex-sep1") (i64.const 0xaf00f00009999))
(assert_return (invoke "i64-hex-sep2") (i64.const 0x1aa0f))

(assert_malformed
  (module quote "(global i32 (i32.const _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i32 (i32.const 0xff__ffff))")
  "unknown operator"
)

(assert_malformed
  (module quote "(global i64 (i64.const _100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const +_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const -_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const 99_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const 1__000))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const _0x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const 0_x100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const 0x_100))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const 0x00_))")
  "unknown operator"
)
(assert_malformed
  (module quote "(global i64 (i64.const 0xff__ffff))")
  "unknown operator"
)
