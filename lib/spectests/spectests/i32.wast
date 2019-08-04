;; i32 operations

(module
  (func (export "add") (param $x i32) (param $y i32) (result i32) (i32.add (local.get $x) (local.get $y)))
  (func (export "sub") (param $x i32) (param $y i32) (result i32) (i32.sub (local.get $x) (local.get $y)))
  (func (export "mul") (param $x i32) (param $y i32) (result i32) (i32.mul (local.get $x) (local.get $y)))
  (func (export "div_s") (param $x i32) (param $y i32) (result i32) (i32.div_s (local.get $x) (local.get $y)))
  (func (export "div_u") (param $x i32) (param $y i32) (result i32) (i32.div_u (local.get $x) (local.get $y)))
  (func (export "rem_s") (param $x i32) (param $y i32) (result i32) (i32.rem_s (local.get $x) (local.get $y)))
  (func (export "rem_u") (param $x i32) (param $y i32) (result i32) (i32.rem_u (local.get $x) (local.get $y)))
  (func (export "and") (param $x i32) (param $y i32) (result i32) (i32.and (local.get $x) (local.get $y)))
  (func (export "or") (param $x i32) (param $y i32) (result i32) (i32.or (local.get $x) (local.get $y)))
  (func (export "xor") (param $x i32) (param $y i32) (result i32) (i32.xor (local.get $x) (local.get $y)))
  (func (export "shl") (param $x i32) (param $y i32) (result i32) (i32.shl (local.get $x) (local.get $y)))
  (func (export "shr_s") (param $x i32) (param $y i32) (result i32) (i32.shr_s (local.get $x) (local.get $y)))
  (func (export "shr_u") (param $x i32) (param $y i32) (result i32) (i32.shr_u (local.get $x) (local.get $y)))
  (func (export "rotl") (param $x i32) (param $y i32) (result i32) (i32.rotl (local.get $x) (local.get $y)))
  (func (export "rotr") (param $x i32) (param $y i32) (result i32) (i32.rotr (local.get $x) (local.get $y)))
  (func (export "clz") (param $x i32) (result i32) (i32.clz (local.get $x)))
  (func (export "ctz") (param $x i32) (result i32) (i32.ctz (local.get $x)))
  (func (export "popcnt") (param $x i32) (result i32) (i32.popcnt (local.get $x)))
  (func (export "eqz") (param $x i32) (result i32) (i32.eqz (local.get $x)))
  (func (export "eq") (param $x i32) (param $y i32) (result i32) (i32.eq (local.get $x) (local.get $y)))
  (func (export "ne") (param $x i32) (param $y i32) (result i32) (i32.ne (local.get $x) (local.get $y)))
  (func (export "lt_s") (param $x i32) (param $y i32) (result i32) (i32.lt_s (local.get $x) (local.get $y)))
  (func (export "lt_u") (param $x i32) (param $y i32) (result i32) (i32.lt_u (local.get $x) (local.get $y)))
  (func (export "le_s") (param $x i32) (param $y i32) (result i32) (i32.le_s (local.get $x) (local.get $y)))
  (func (export "le_u") (param $x i32) (param $y i32) (result i32) (i32.le_u (local.get $x) (local.get $y)))
  (func (export "gt_s") (param $x i32) (param $y i32) (result i32) (i32.gt_s (local.get $x) (local.get $y)))
  (func (export "gt_u") (param $x i32) (param $y i32) (result i32) (i32.gt_u (local.get $x) (local.get $y)))
  (func (export "ge_s") (param $x i32) (param $y i32) (result i32) (i32.ge_s (local.get $x) (local.get $y)))
  (func (export "ge_u") (param $x i32) (param $y i32) (result i32) (i32.ge_u (local.get $x) (local.get $y)))
)

(assert_return (invoke "add" (i32.const 1) (i32.const 1)) (i32.const 2))
(assert_return (invoke "add" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "add" (i32.const -1) (i32.const -1)) (i32.const -2))
(assert_return (invoke "add" (i32.const -1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "add" (i32.const 0x7fffffff) (i32.const 1)) (i32.const 0x80000000))
(assert_return (invoke "add" (i32.const 0x80000000) (i32.const -1)) (i32.const 0x7fffffff))
(assert_return (invoke "add" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "add" (i32.const 0x3fffffff) (i32.const 1)) (i32.const 0x40000000))

(assert_return (invoke "sub" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "sub" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "sub" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "sub" (i32.const 0x7fffffff) (i32.const -1)) (i32.const 0x80000000))
(assert_return (invoke "sub" (i32.const 0x80000000) (i32.const 1)) (i32.const 0x7fffffff))
(assert_return (invoke "sub" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "sub" (i32.const 0x3fffffff) (i32.const -1)) (i32.const 0x40000000))

(assert_return (invoke "mul" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "mul" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "mul" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "mul" (i32.const 0x10000000) (i32.const 4096)) (i32.const 0))
(assert_return (invoke "mul" (i32.const 0x80000000) (i32.const 0)) (i32.const 0))
(assert_return (invoke "mul" (i32.const 0x80000000) (i32.const -1)) (i32.const 0x80000000))
(assert_return (invoke "mul" (i32.const 0x7fffffff) (i32.const -1)) (i32.const 0x80000001))
(assert_return (invoke "mul" (i32.const 0x01234567) (i32.const 0x76543210)) (i32.const 0x358e7470))
(assert_return (invoke "mul" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 1))

(assert_trap (invoke "div_s" (i32.const 1) (i32.const 0)) "integer divide by zero")
(assert_trap (invoke "div_s" (i32.const 0) (i32.const 0)) "integer divide by zero")
(assert_trap (invoke "div_s" (i32.const 0x80000000) (i32.const -1)) "integer overflow")
(assert_return (invoke "div_s" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "div_s" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "div_s" (i32.const 0) (i32.const -1)) (i32.const 0))
(assert_return (invoke "div_s" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "div_s" (i32.const 0x80000000) (i32.const 2)) (i32.const 0xc0000000))
(assert_return (invoke "div_s" (i32.const 0x80000001) (i32.const 1000)) (i32.const 0xffdf3b65))
(assert_return (invoke "div_s" (i32.const 5) (i32.const 2)) (i32.const 2))
(assert_return (invoke "div_s" (i32.const -5) (i32.const 2)) (i32.const -2))
(assert_return (invoke "div_s" (i32.const 5) (i32.const -2)) (i32.const -2))
(assert_return (invoke "div_s" (i32.const -5) (i32.const -2)) (i32.const 2))
(assert_return (invoke "div_s" (i32.const 7) (i32.const 3)) (i32.const 2))
(assert_return (invoke "div_s" (i32.const -7) (i32.const 3)) (i32.const -2))
(assert_return (invoke "div_s" (i32.const 7) (i32.const -3)) (i32.const -2))
(assert_return (invoke "div_s" (i32.const -7) (i32.const -3)) (i32.const 2))
(assert_return (invoke "div_s" (i32.const 11) (i32.const 5)) (i32.const 2))
(assert_return (invoke "div_s" (i32.const 17) (i32.const 7)) (i32.const 2))

(assert_trap (invoke "div_u" (i32.const 1) (i32.const 0)) "integer divide by zero")
(assert_trap (invoke "div_u" (i32.const 0) (i32.const 0)) "integer divide by zero")
(assert_return (invoke "div_u" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "div_u" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "div_u" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "div_u" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "div_u" (i32.const 0x80000000) (i32.const 2)) (i32.const 0x40000000))
(assert_return (invoke "div_u" (i32.const 0x8ff00ff0) (i32.const 0x10001)) (i32.const 0x8fef))
(assert_return (invoke "div_u" (i32.const 0x80000001) (i32.const 1000)) (i32.const 0x20c49b))
(assert_return (invoke "div_u" (i32.const 5) (i32.const 2)) (i32.const 2))
(assert_return (invoke "div_u" (i32.const -5) (i32.const 2)) (i32.const 0x7ffffffd))
(assert_return (invoke "div_u" (i32.const 5) (i32.const -2)) (i32.const 0))
(assert_return (invoke "div_u" (i32.const -5) (i32.const -2)) (i32.const 0))
(assert_return (invoke "div_u" (i32.const 7) (i32.const 3)) (i32.const 2))
(assert_return (invoke "div_u" (i32.const 11) (i32.const 5)) (i32.const 2))
(assert_return (invoke "div_u" (i32.const 17) (i32.const 7)) (i32.const 2))

(assert_trap (invoke "rem_s" (i32.const 1) (i32.const 0)) "integer divide by zero")
(assert_trap (invoke "rem_s" (i32.const 0) (i32.const 0)) "integer divide by zero")
(assert_return (invoke "rem_s" (i32.const 0x7fffffff) (i32.const -1)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const 0) (i32.const -1)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const 0x80000000) (i32.const 2)) (i32.const 0))
(assert_return (invoke "rem_s" (i32.const 0x80000001) (i32.const 1000)) (i32.const -647))
(assert_return (invoke "rem_s" (i32.const 5) (i32.const 2)) (i32.const 1))
(assert_return (invoke "rem_s" (i32.const -5) (i32.const 2)) (i32.const -1))
(assert_return (invoke "rem_s" (i32.const 5) (i32.const -2)) (i32.const 1))
(assert_return (invoke "rem_s" (i32.const -5) (i32.const -2)) (i32.const -1))
(assert_return (invoke "rem_s" (i32.const 7) (i32.const 3)) (i32.const 1))
(assert_return (invoke "rem_s" (i32.const -7) (i32.const 3)) (i32.const -1))
(assert_return (invoke "rem_s" (i32.const 7) (i32.const -3)) (i32.const 1))
(assert_return (invoke "rem_s" (i32.const -7) (i32.const -3)) (i32.const -1))
(assert_return (invoke "rem_s" (i32.const 11) (i32.const 5)) (i32.const 1))
(assert_return (invoke "rem_s" (i32.const 17) (i32.const 7)) (i32.const 3))

(assert_trap (invoke "rem_u" (i32.const 1) (i32.const 0)) "integer divide by zero")
(assert_trap (invoke "rem_u" (i32.const 0) (i32.const 0)) "integer divide by zero")
(assert_return (invoke "rem_u" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "rem_u" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "rem_u" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "rem_u" (i32.const 0x80000000) (i32.const -1)) (i32.const 0x80000000))
(assert_return (invoke "rem_u" (i32.const 0x80000000) (i32.const 2)) (i32.const 0))
(assert_return (invoke "rem_u" (i32.const 0x8ff00ff0) (i32.const 0x10001)) (i32.const 0x8001))
(assert_return (invoke "rem_u" (i32.const 0x80000001) (i32.const 1000)) (i32.const 649))
(assert_return (invoke "rem_u" (i32.const 5) (i32.const 2)) (i32.const 1))
(assert_return (invoke "rem_u" (i32.const -5) (i32.const 2)) (i32.const 1))
(assert_return (invoke "rem_u" (i32.const 5) (i32.const -2)) (i32.const 5))
(assert_return (invoke "rem_u" (i32.const -5) (i32.const -2)) (i32.const -5))
(assert_return (invoke "rem_u" (i32.const 7) (i32.const 3)) (i32.const 1))
(assert_return (invoke "rem_u" (i32.const 11) (i32.const 5)) (i32.const 1))
(assert_return (invoke "rem_u" (i32.const 17) (i32.const 7)) (i32.const 3))

(assert_return (invoke "and" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "and" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "and" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "and" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "and" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "and" (i32.const 0x7fffffff) (i32.const -1)) (i32.const 0x7fffffff))
(assert_return (invoke "and" (i32.const 0xf0f0ffff) (i32.const 0xfffff0f0)) (i32.const 0xf0f0f0f0))
(assert_return (invoke "and" (i32.const 0xffffffff) (i32.const 0xffffffff)) (i32.const 0xffffffff))

(assert_return (invoke "or" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "or" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "or" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "or" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "or" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const -1))
(assert_return (invoke "or" (i32.const 0x80000000) (i32.const 0)) (i32.const 0x80000000))
(assert_return (invoke "or" (i32.const 0xf0f0ffff) (i32.const 0xfffff0f0)) (i32.const 0xffffffff))
(assert_return (invoke "or" (i32.const 0xffffffff) (i32.const 0xffffffff)) (i32.const 0xffffffff))

(assert_return (invoke "xor" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "xor" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "xor" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "xor" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "xor" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const -1))
(assert_return (invoke "xor" (i32.const 0x80000000) (i32.const 0)) (i32.const 0x80000000))
(assert_return (invoke "xor" (i32.const -1) (i32.const 0x80000000)) (i32.const 0x7fffffff))
(assert_return (invoke "xor" (i32.const -1) (i32.const 0x7fffffff)) (i32.const 0x80000000))
(assert_return (invoke "xor" (i32.const 0xf0f0ffff) (i32.const 0xfffff0f0)) (i32.const 0x0f0f0f0f))
(assert_return (invoke "xor" (i32.const 0xffffffff) (i32.const 0xffffffff)) (i32.const 0))

(assert_return (invoke "shl" (i32.const 1) (i32.const 1)) (i32.const 2))
(assert_return (invoke "shl" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "shl" (i32.const 0x7fffffff) (i32.const 1)) (i32.const 0xfffffffe))
(assert_return (invoke "shl" (i32.const 0xffffffff) (i32.const 1)) (i32.const 0xfffffffe))
(assert_return (invoke "shl" (i32.const 0x80000000) (i32.const 1)) (i32.const 0))
(assert_return (invoke "shl" (i32.const 0x40000000) (i32.const 1)) (i32.const 0x80000000))
(assert_return (invoke "shl" (i32.const 1) (i32.const 31)) (i32.const 0x80000000))
(assert_return (invoke "shl" (i32.const 1) (i32.const 32)) (i32.const 1))
(assert_return (invoke "shl" (i32.const 1) (i32.const 33)) (i32.const 2))
(assert_return (invoke "shl" (i32.const 1) (i32.const -1)) (i32.const 0x80000000))
(assert_return (invoke "shl" (i32.const 1) (i32.const 0x7fffffff)) (i32.const 0x80000000))

(assert_return (invoke "shr_s" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "shr_s" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "shr_s" (i32.const -1) (i32.const 1)) (i32.const -1))
(assert_return (invoke "shr_s" (i32.const 0x7fffffff) (i32.const 1)) (i32.const 0x3fffffff))
(assert_return (invoke "shr_s" (i32.const 0x80000000) (i32.const 1)) (i32.const 0xc0000000))
(assert_return (invoke "shr_s" (i32.const 0x40000000) (i32.const 1)) (i32.const 0x20000000))
(assert_return (invoke "shr_s" (i32.const 1) (i32.const 32)) (i32.const 1))
(assert_return (invoke "shr_s" (i32.const 1) (i32.const 33)) (i32.const 0))
(assert_return (invoke "shr_s" (i32.const 1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "shr_s" (i32.const 1) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "shr_s" (i32.const 1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "shr_s" (i32.const 0x80000000) (i32.const 31)) (i32.const -1))
(assert_return (invoke "shr_s" (i32.const -1) (i32.const 32)) (i32.const -1))
(assert_return (invoke "shr_s" (i32.const -1) (i32.const 33)) (i32.const -1))
(assert_return (invoke "shr_s" (i32.const -1) (i32.const -1)) (i32.const -1))
(assert_return (invoke "shr_s" (i32.const -1) (i32.const 0x7fffffff)) (i32.const -1))
(assert_return (invoke "shr_s" (i32.const -1) (i32.const 0x80000000)) (i32.const -1))

(assert_return (invoke "shr_u" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "shr_u" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "shr_u" (i32.const -1) (i32.const 1)) (i32.const 0x7fffffff))
(assert_return (invoke "shr_u" (i32.const 0x7fffffff) (i32.const 1)) (i32.const 0x3fffffff))
(assert_return (invoke "shr_u" (i32.const 0x80000000) (i32.const 1)) (i32.const 0x40000000))
(assert_return (invoke "shr_u" (i32.const 0x40000000) (i32.const 1)) (i32.const 0x20000000))
(assert_return (invoke "shr_u" (i32.const 1) (i32.const 32)) (i32.const 1))
(assert_return (invoke "shr_u" (i32.const 1) (i32.const 33)) (i32.const 0))
(assert_return (invoke "shr_u" (i32.const 1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "shr_u" (i32.const 1) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "shr_u" (i32.const 1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "shr_u" (i32.const 0x80000000) (i32.const 31)) (i32.const 1))
(assert_return (invoke "shr_u" (i32.const -1) (i32.const 32)) (i32.const -1))
(assert_return (invoke "shr_u" (i32.const -1) (i32.const 33)) (i32.const 0x7fffffff))
(assert_return (invoke "shr_u" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "shr_u" (i32.const -1) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "shr_u" (i32.const -1) (i32.const 0x80000000)) (i32.const -1))

(assert_return (invoke "rotl" (i32.const 1) (i32.const 1)) (i32.const 2))
(assert_return (invoke "rotl" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "rotl" (i32.const -1) (i32.const 1)) (i32.const -1))
(assert_return (invoke "rotl" (i32.const 1) (i32.const 32)) (i32.const 1))
(assert_return (invoke "rotl" (i32.const 0xabcd9876) (i32.const 1)) (i32.const 0x579b30ed))
(assert_return (invoke "rotl" (i32.const 0xfe00dc00) (i32.const 4)) (i32.const 0xe00dc00f))
(assert_return (invoke "rotl" (i32.const 0xb0c1d2e3) (i32.const 5)) (i32.const 0x183a5c76))
(assert_return (invoke "rotl" (i32.const 0x00008000) (i32.const 37)) (i32.const 0x00100000))
(assert_return (invoke "rotl" (i32.const 0xb0c1d2e3) (i32.const 0xff05)) (i32.const 0x183a5c76))
(assert_return (invoke "rotl" (i32.const 0x769abcdf) (i32.const 0xffffffed)) (i32.const 0x579beed3))
(assert_return (invoke "rotl" (i32.const 0x769abcdf) (i32.const 0x8000000d)) (i32.const 0x579beed3))
(assert_return (invoke "rotl" (i32.const 1) (i32.const 31)) (i32.const 0x80000000))
(assert_return (invoke "rotl" (i32.const 0x80000000) (i32.const 1)) (i32.const 1))

(assert_return (invoke "rotr" (i32.const 1) (i32.const 1)) (i32.const 0x80000000))
(assert_return (invoke "rotr" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "rotr" (i32.const -1) (i32.const 1)) (i32.const -1))
(assert_return (invoke "rotr" (i32.const 1) (i32.const 32)) (i32.const 1))
(assert_return (invoke "rotr" (i32.const 0xff00cc00) (i32.const 1)) (i32.const 0x7f806600))
(assert_return (invoke "rotr" (i32.const 0x00080000) (i32.const 4)) (i32.const 0x00008000))
(assert_return (invoke "rotr" (i32.const 0xb0c1d2e3) (i32.const 5)) (i32.const 0x1d860e97))
(assert_return (invoke "rotr" (i32.const 0x00008000) (i32.const 37)) (i32.const 0x00000400))
(assert_return (invoke "rotr" (i32.const 0xb0c1d2e3) (i32.const 0xff05)) (i32.const 0x1d860e97))
(assert_return (invoke "rotr" (i32.const 0x769abcdf) (i32.const 0xffffffed)) (i32.const 0xe6fbb4d5))
(assert_return (invoke "rotr" (i32.const 0x769abcdf) (i32.const 0x8000000d)) (i32.const 0xe6fbb4d5))
(assert_return (invoke "rotr" (i32.const 1) (i32.const 31)) (i32.const 2))
(assert_return (invoke "rotr" (i32.const 0x80000000) (i32.const 31)) (i32.const 1))

(assert_return (invoke "clz" (i32.const 0xffffffff)) (i32.const 0))
(assert_return (invoke "clz" (i32.const 0)) (i32.const 32))
(assert_return (invoke "clz" (i32.const 0x00008000)) (i32.const 16))
(assert_return (invoke "clz" (i32.const 0xff)) (i32.const 24))
(assert_return (invoke "clz" (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "clz" (i32.const 1)) (i32.const 31))
(assert_return (invoke "clz" (i32.const 2)) (i32.const 30))
(assert_return (invoke "clz" (i32.const 0x7fffffff)) (i32.const 1))

(assert_return (invoke "ctz" (i32.const -1)) (i32.const 0))
(assert_return (invoke "ctz" (i32.const 0)) (i32.const 32))
(assert_return (invoke "ctz" (i32.const 0x00008000)) (i32.const 15))
(assert_return (invoke "ctz" (i32.const 0x00010000)) (i32.const 16))
(assert_return (invoke "ctz" (i32.const 0x80000000)) (i32.const 31))
(assert_return (invoke "ctz" (i32.const 0x7fffffff)) (i32.const 0))

(assert_return (invoke "popcnt" (i32.const -1)) (i32.const 32))
(assert_return (invoke "popcnt" (i32.const 0)) (i32.const 0))
(assert_return (invoke "popcnt" (i32.const 0x00008000)) (i32.const 1))
(assert_return (invoke "popcnt" (i32.const 0x80008000)) (i32.const 2))
(assert_return (invoke "popcnt" (i32.const 0x7fffffff)) (i32.const 31))
(assert_return (invoke "popcnt" (i32.const 0xAAAAAAAA)) (i32.const 16))
(assert_return (invoke "popcnt" (i32.const 0x55555555)) (i32.const 16))
(assert_return (invoke "popcnt" (i32.const 0xDEADBEEF)) (i32.const 24))

(assert_return (invoke "eqz" (i32.const 0)) (i32.const 1))
(assert_return (invoke "eqz" (i32.const 1)) (i32.const 0))
(assert_return (invoke "eqz" (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "eqz" (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "eqz" (i32.const 0xffffffff)) (i32.const 0))

(assert_return (invoke "eq" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "eq" (i32.const -1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "eq" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "eq" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0x80000000) (i32.const 0)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "eq" (i32.const -1) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "eq" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 0))

(assert_return (invoke "ne" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "ne" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "ne" (i32.const -1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "ne" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "ne" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "ne" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0x80000000) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0x80000000) (i32.const -1)) (i32.const 1))
(assert_return (invoke "ne" (i32.const -1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "ne" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 1))

(assert_return (invoke "lt_s" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const -1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "lt_s" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "lt_s" (i32.const 0x80000000) (i32.const 0)) (i32.const 1))
(assert_return (invoke "lt_s" (i32.const 0) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const 0x80000000) (i32.const -1)) (i32.const 1))
(assert_return (invoke "lt_s" (i32.const -1) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "lt_s" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "lt_s" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 0))

(assert_return (invoke "lt_u" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const -1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "lt_u" (i32.const 0x80000000) (i32.const 0)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 0) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "lt_u" (i32.const 0x80000000) (i32.const -1)) (i32.const 1))
(assert_return (invoke "lt_u" (i32.const -1) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "lt_u" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 1))

(assert_return (invoke "le_s" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const -1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "le_s" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 0x80000000) (i32.const 0)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 0) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "le_s" (i32.const 0x80000000) (i32.const -1)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const -1) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "le_s" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "le_s" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 0))

(assert_return (invoke "le_u" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const -1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "le_u" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const 1) (i32.const 0)) (i32.const 0))
(assert_return (invoke "le_u" (i32.const 0) (i32.const 1)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const 0x80000000) (i32.const 0)) (i32.const 0))
(assert_return (invoke "le_u" (i32.const 0) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const 0x80000000) (i32.const -1)) (i32.const 1))
(assert_return (invoke "le_u" (i32.const -1) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "le_u" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "le_u" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 1))

(assert_return (invoke "gt_s" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const -1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "gt_s" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 0x80000000) (i32.const 0)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 0) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "gt_s" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const -1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "gt_s" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "gt_s" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 1))

(assert_return (invoke "gt_u" (i32.const 0) (i32.const 0)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const 1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const -1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "gt_u" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const -1) (i32.const -1)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "gt_u" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const 0x80000000) (i32.const 0)) (i32.const 1))
(assert_return (invoke "gt_u" (i32.const 0) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "gt_u" (i32.const -1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "gt_u" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "gt_u" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 0))

(assert_return (invoke "ge_s" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const -1) (i32.const 1)) (i32.const 0))
(assert_return (invoke "ge_s" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "ge_s" (i32.const 0x80000000) (i32.const 0)) (i32.const 0))
(assert_return (invoke "ge_s" (i32.const 0) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "ge_s" (i32.const -1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ge_s" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 0))
(assert_return (invoke "ge_s" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 1))

(assert_return (invoke "ge_u" (i32.const 0) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const -1) (i32.const 1)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 0x80000000) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 0x7fffffff) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const -1) (i32.const -1)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 1) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 0) (i32.const 1)) (i32.const 0))
(assert_return (invoke "ge_u" (i32.const 0x80000000) (i32.const 0)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 0) (i32.const 0x80000000)) (i32.const 0))
(assert_return (invoke "ge_u" (i32.const 0x80000000) (i32.const -1)) (i32.const 0))
(assert_return (invoke "ge_u" (i32.const -1) (i32.const 0x80000000)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 0x80000000) (i32.const 0x7fffffff)) (i32.const 1))
(assert_return (invoke "ge_u" (i32.const 0x7fffffff) (i32.const 0x80000000)) (i32.const 0))


(assert_invalid
  (module
    (func $type-unary-operand-empty
      (i32.eqz) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-block
      (i32.const 0)
      (block (i32.eqz) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-loop
      (i32.const 0)
      (loop (i32.eqz) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-if
      (i32.const 0) (i32.const 0)
      (if (then (i32.eqz) (drop)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-else
      (i32.const 0) (i32.const 0)
      (if (result i32) (then (i32.const 0)) (else (i32.eqz))) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-br
      (i32.const 0)
      (block (br 0 (i32.eqz)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-br_if
      (i32.const 0)
      (block (br_if 0 (i32.eqz) (i32.const 1)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-br_table
      (i32.const 0)
      (block (br_table 0 (i32.eqz)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-return
      (return (i32.eqz)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-select
      (select (i32.eqz) (i32.const 1) (i32.const 2)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-call
      (call 1 (i32.eqz)) (drop)
    )
    (func (param i32) (result i32) (local.get 0))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $f (param i32) (result i32) (local.get 0))
    (type $sig (func (param i32) (result i32)))
    (table funcref (elem $f))
    (func $type-unary-operand-empty-in-call_indirect
      (block (result i32)
        (call_indirect (type $sig)
          (i32.eqz) (i32.const 0)
        )
        (drop)
      )
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-local.set
      (local i32)
      (local.set 0 (i32.eqz)) (local.get 0) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-unary-operand-empty-in-local.tee
      (local i32)
      (local.tee 0 (i32.eqz)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-unary-operand-empty-in-global.set
      (global.set $x (i32.eqz)) (global.get $x) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-unary-operand-empty-in-memory.grow
      (memory.grow (i32.eqz)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-unary-operand-empty-in-load
      (i32.load (i32.eqz)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 1)
    (func $type-unary-operand-empty-in-store
      (i32.store (i32.eqz) (i32.const 1))
    )
  )
  "type mismatch"
)

(assert_invalid
  (module
    (func $type-binary-1st-operand-empty
      (i32.add) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty
      (i32.const 0) (i32.add) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-block
      (i32.const 0) (i32.const 0)
      (block (i32.add) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-block
      (i32.const 0)
      (block (i32.const 0) (i32.add) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-loop
      (i32.const 0) (i32.const 0)
      (loop (i32.add) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-loop
      (i32.const 0)
      (loop (i32.const 0) (i32.add) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-if
      (i32.const 0) (i32.const 0) (i32.const 0)
      (if (i32.add) (then (drop)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-if
      (i32.const 0) (i32.const 0)
      (if (i32.const 0) (then (i32.add)) (else (drop)))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-else
      (i32.const 0) (i32.const 0) (i32.const 0)
      (if (result i32) (then (i32.const 0)) (else (i32.add) (i32.const 0)))
      (drop) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-else
      (i32.const 0) (i32.const 0)
      (if (result i32) (then (i32.const 0)) (else (i32.add)))
      (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-br
      (i32.const 0) (i32.const 0)
      (block (br 0 (i32.add)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-br
      (i32.const 0)
      (block (br 0 (i32.const 0) (i32.add)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-br_if
      (i32.const 0) (i32.const 0)
      (block (br_if 0 (i32.add) (i32.const 1)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-br_if
      (i32.const 0)
      (block (br_if 0 (i32.const 0) (i32.add) (i32.const 1)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-br_table
      (i32.const 0) (i32.const 0)
      (block (br_table 0 (i32.add)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-br_table
      (i32.const 0)
      (block (br_table 0 (i32.const 0) (i32.add)) (drop))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-return
      (return (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-return
      (return (i32.const 0) (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-select
      (select (i32.add) (i32.const 1) (i32.const 2)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-select
      (select (i32.const 0) (i32.add) (i32.const 1) (i32.const 2)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-call
      (call 1 (i32.add)) (drop)
    )
    (func (param i32 i32) (result i32) (local.get 0))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-call
      (call 1 (i32.const 0) (i32.add)) (drop)
    )
    (func (param i32 i32) (result i32) (local.get 0))
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $f (param i32) (result i32) (local.get 0))
    (type $sig (func (param i32) (result i32)))
    (table funcref (elem $f))
    (func $type-binary-1st-operand-empty-in-call_indirect
      (block (result i32)
        (call_indirect (type $sig)
          (i32.add) (i32.const 0)
        )
        (drop)
      )
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $f (param i32) (result i32) (local.get 0))
    (type $sig (func (param i32) (result i32)))
    (table funcref (elem $f))
    (func $type-binary-2nd-operand-empty-in-call_indirect
      (block (result i32)
        (call_indirect (type $sig)
          (i32.const 0) (i32.add) (i32.const 0)
        )
        (drop)
      )
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-local.set
      (local i32)
      (local.set 0 (i32.add)) (local.get 0) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-local.set
      (local i32)
      (local.set 0 (i32.const 0) (i32.add)) (local.get 0) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-1st-operand-empty-in-local.tee
      (local i32)
      (local.tee 0 (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (func $type-binary-2nd-operand-empty-in-local.tee
      (local i32)
      (local.tee 0 (i32.const 0) (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-binary-1st-operand-empty-in-global.set
      (global.set $x (i32.add)) (global.get $x) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (global $x (mut i32) (i32.const 0))
    (func $type-binary-2nd-operand-empty-in-global.set
      (global.set $x (i32.const 0) (i32.add)) (global.get $x) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-binary-1st-operand-empty-in-memory.grow
      (memory.grow (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-binary-2nd-operand-empty-in-memory.grow
      (memory.grow (i32.const 0) (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-binary-1st-operand-empty-in-load
      (i32.load (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 0)
    (func $type-binary-2nd-operand-empty-in-load
      (i32.load (i32.const 0) (i32.add)) (drop)
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 1)
    (func $type-binary-1st-operand-empty-in-store
      (i32.store (i32.add) (i32.const 1))
    )
  )
  "type mismatch"
)
(assert_invalid
  (module
    (memory 1)
    (func $type-binary-2nd-operand-empty-in-store
      (i32.store (i32.const 1) (i32.add) (i32.const 0))
    )
  )
  "type mismatch"
)


;; Type check

(assert_invalid (module (func (result i32) (i32.add (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.and (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.div_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.div_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.mul (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.or (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.rem_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.rem_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.rotl (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.rotr (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.shl (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.shr_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.shr_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.sub (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.xor (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.eqz (i64.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.clz (i64.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.ctz (i64.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.popcnt (i64.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.eq (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.ge_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.ge_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.gt_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.gt_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.le_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.le_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.lt_s (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.lt_u (i64.const 0) (f32.const 0)))) "type mismatch")
(assert_invalid (module (func (result i32) (i32.ne (i64.const 0) (f32.const 0)))) "type mismatch")
