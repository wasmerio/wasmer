;; Test t.const instructions

;; Syntax error

(module (func (i32.const 0xffffffff) drop))
(module (func (i32.const -0x80000000) drop))
(assert_malformed
  (module quote "(func (i32.const 0x100000000) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (i32.const -0x80000001) drop)")
  "constant out of range"
)

(module (func (i32.const 4294967295) drop))
(module (func (i32.const -2147483648) drop))
(assert_malformed
  (module quote "(func (i32.const 4294967296) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (i32.const -2147483649) drop)")
  "constant out of range"
)

(module (func (i64.const 0xffffffffffffffff) drop))
(module (func (i64.const -0x8000000000000000) drop))
(assert_malformed
  (module quote "(func (i64.const 0x10000000000000000) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (i64.const -0x8000000000000001) drop)")
  "constant out of range"
)

(module (func (i64.const 18446744073709551615) drop))
(module (func (i64.const -9223372036854775808) drop))
(assert_malformed
  (module quote "(func (i64.const 18446744073709551616) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (i64.const -9223372036854775809) drop)")
  "constant out of range"
)

(module (func (f32.const 0x1p127) drop))
(module (func (f32.const -0x1p127) drop))
(module (func (f32.const 0x1.fffffep127) drop))
(module (func (f32.const -0x1.fffffep127) drop))
(module (func (f32.const 0x1.fffffe7p127) drop))
(module (func (f32.const -0x1.fffffe7p127) drop))
(assert_malformed
  (module quote "(func (f32.const 0x1p128) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f32.const -0x1p128) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f32.const 0x1.ffffffp127) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f32.const -0x1.ffffffp127) drop)")
  "constant out of range"
)

(module (func (f32.const 1e38) drop))
(module (func (f32.const -1e38) drop))
(assert_malformed
  (module quote "(func (f32.const 1e39) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f32.const -1e39) drop)")
  "constant out of range"
)

(module (func (f32.const 340282356779733623858607532500980858880) drop))
(module (func (f32.const -340282356779733623858607532500980858880) drop))
(assert_malformed
  (module quote "(func (f32.const 340282356779733661637539395458142568448) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f32.const -340282356779733661637539395458142568448) drop)")
  "constant out of range"
)

(module (func (f64.const 0x1p1023) drop))
(module (func (f64.const -0x1p1023) drop))
(module (func (f64.const 0x1.fffffffffffffp1023) drop))
(module (func (f64.const -0x1.fffffffffffffp1023) drop))
(module (func (f64.const 0x1.fffffffffffff7p1023) drop))
(module (func (f64.const -0x1.fffffffffffff7p1023) drop))
(assert_malformed
  (module quote "(func (f64.const 0x1p1024) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f64.const -0x1p1024) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f64.const 0x1.fffffffffffff8p1023) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f64.const -0x1.fffffffffffff8p1023) drop)")
  "constant out of range"
)

(module (func (f64.const 1e308) drop))
(module (func (f64.const -1e308) drop))
(assert_malformed
  (module quote "(func (f64.const 1e309) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f64.const -1e309) drop)")
  "constant out of range"
)

(module (func (f64.const 179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368) drop))
(module (func (f64.const -179769313486231570814527423731704356798070567525844996598917476803157260780028538760589558632766878171540458953514382464234321326889464182768467546703537516986049910576551282076245490090389328944075868508455133942304583236903222948165808559332123348274797826204144723168738177180919299881250404026184124858368) drop))
(assert_malformed
  (module quote "(func (f64.const 269653970229347356221791135597556535197105851288767494898376215204735891170042808140884337949150317257310688430271573696351481990334196274152701320055306275479074865864826923114368235135583993416113802762682700913456874855354834422248712838998185022412196739306217084753107265771378949821875606039276187287552) drop)")
  "constant out of range"
)
(assert_malformed
  (module quote "(func (f64.const -269653970229347356221791135597556535197105851288767494898376215204735891170042808140884337949150317257310688430271573696351481990334196274152701320055306275479074865864826923114368235135583993416113802762682700913456874855354834422248712838998185022412196739306217084753107265771378949821875606039276187287552) drop)")
  "constant out of range"
)
