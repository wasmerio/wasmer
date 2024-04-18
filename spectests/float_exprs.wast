;; Test interesting floating-point "expressions". These tests contain code
;; patterns which tempt common value-changing optimizations.

;; Test that x*y+z is not done with x87-style intermediate precision.

(module
  (func (export "f64.no_contraction") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.add (f64.mul (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f64.no_contraction" (f64.const -0x1.9e87ce14273afp-103) (f64.const 0x1.2515ad31db63ep+664) (f64.const 0x1.868c6685e6185p+533)) (f64.const -0x1.da94885b11493p+561))
(assert_return (invoke "f64.no_contraction" (f64.const 0x1.da21c460a6f44p+52) (f64.const 0x1.60859d2e7714ap-321) (f64.const 0x1.e63f1b7b660e1p-302)) (f64.const 0x1.4672f256d1794p-268))
(assert_return (invoke "f64.no_contraction" (f64.const -0x1.f3eaf43f327cp-594) (f64.const 0x1.dfcc009906b57p+533) (f64.const 0x1.5984e03c520a1p-104)) (f64.const -0x1.d4797fb3db166p-60))
(assert_return (invoke "f64.no_contraction" (f64.const 0x1.dab6c772cb2e2p-69) (f64.const -0x1.d761663679a84p-101) (f64.const 0x1.f22f92c843226p-218)) (f64.const -0x1.b50d72dfcef68p-169))
(assert_return (invoke "f64.no_contraction" (f64.const -0x1.87c5def1e4d3dp-950) (f64.const -0x1.50cd5dab2207fp+935) (f64.const 0x1.e629bd0da8c5dp-54)) (f64.const 0x1.01b6feb4e78a7p-14))

;; Test that x*y+z is not folded to fma.

(module
  (func (export "f32.no_fma") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.add (f32.mul (get_local $x) (get_local $y)) (get_local $z)))
  (func (export "f64.no_fma") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.add (f64.mul (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f32.no_fma" (f32.const 0x1.a78402p+124) (f32.const 0x1.cf8548p-23) (f32.const 0x1.992adap+107)) (f32.const 0x1.a5262cp+107))
(assert_return (invoke "f32.no_fma" (f32.const 0x1.ed15a4p-28) (f32.const -0x1.613c72p-50) (f32.const 0x1.4757bp-88)) (f32.const -0x1.5406b8p-77))
(assert_return (invoke "f32.no_fma" (f32.const 0x1.ae63a2p+37) (f32.const 0x1.b3a59ap-13) (f32.const 0x1.c16918p+10)) (f32.const 0x1.6e385cp+25))
(assert_return (invoke "f32.no_fma" (f32.const 0x1.2a77fap-8) (f32.const -0x1.bb7356p+22) (f32.const -0x1.32be2ap+1)) (f32.const -0x1.0286d4p+15))
(assert_return (invoke "f32.no_fma" (f32.const 0x1.298fb6p+126) (f32.const -0x1.03080cp-70) (f32.const -0x1.418de6p+34)) (f32.const -0x1.2d15c6p+56))
(assert_return (invoke "f64.no_fma" (f64.const 0x1.ac357ff46eed4p+557) (f64.const 0x1.852c01a5e7297p+430) (f64.const -0x1.05995704eda8ap+987)) (f64.const 0x1.855d905d338ep+987))
(assert_return (invoke "f64.no_fma" (f64.const 0x1.e2fd6bf32010cp+749) (f64.const 0x1.01c2238d405e4p-130) (f64.const 0x1.2ecc0db4b9f94p+573)) (f64.const 0x1.e64eb07e063bcp+619))
(assert_return (invoke "f64.no_fma" (f64.const 0x1.92b7c7439ede3p-721) (f64.const -0x1.6aa97586d3de6p+1011) (f64.const 0x1.8de4823f6358ap+237)) (f64.const -0x1.1d4139fd20ecdp+291))
(assert_return (invoke "f64.no_fma" (f64.const -0x1.466d30bddb453p-386) (f64.const -0x1.185a4d739c7aap+443) (f64.const 0x1.5f9c436fbfc7bp+55)) (f64.const 0x1.bd61a350fcc1ap+57))
(assert_return (invoke "f64.no_fma" (f64.const 0x1.7e2c44058a799p+52) (f64.const 0x1.c73b71765b8b2p+685) (f64.const -0x1.16c641df0b108p+690)) (f64.const 0x1.53ccb53de0bd1p+738))

;; Test that x+0.0 is not folded to x.
;; See IEEE 754-2008 10.4 "Literal meaning and value-changing optimizations".

(module
  (func (export "f32.no_fold_add_zero") (param $x f32) (result f32)
    (f32.add (get_local $x) (f32.const 0.0)))
  (func (export "f64.no_fold_add_zero") (param $x f64) (result f64)
    (f64.add (get_local $x) (f64.const 0.0)))
)

(assert_return (invoke "f32.no_fold_add_zero" (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f64.no_fold_add_zero" (f64.const -0.0)) (f64.const 0.0))
(assert_return_arithmetic_nan (invoke "f32.no_fold_add_zero" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_add_zero" (f64.const nan:0x4000000000000)))

;; Test that 0.0 - x is not folded to -x.

(module
  (func (export "f32.no_fold_zero_sub") (param $x f32) (result f32)
    (f32.sub (f32.const 0.0) (get_local $x)))
  (func (export "f64.no_fold_zero_sub") (param $x f64) (result f64)
    (f64.sub (f64.const 0.0) (get_local $x)))
)

(assert_return (invoke "f32.no_fold_zero_sub" (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f64.no_fold_zero_sub" (f64.const 0.0)) (f64.const 0.0))
(assert_return_arithmetic_nan (invoke "f32.no_fold_zero_sub" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_zero_sub" (f64.const nan:0x4000000000000)))

;; Test that x - 0.0 is not folded to x.

(module
  (func (export "f32.no_fold_sub_zero") (param $x f32) (result f32)
    (f32.sub (get_local $x) (f32.const 0.0)))
  (func (export "f64.no_fold_sub_zero") (param $x f64) (result f64)
    (f64.sub (get_local $x) (f64.const 0.0)))
)

(assert_return_arithmetic_nan (invoke "f32.no_fold_sub_zero" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_sub_zero" (f64.const nan:0x4000000000000)))

;; Test that x*0.0 is not folded to 0.0.

(module
  (func (export "f32.no_fold_mul_zero") (param $x f32) (result f32)
    (f32.mul (get_local $x) (f32.const 0.0)))
  (func (export "f64.no_fold_mul_zero") (param $x f64) (result f64)
    (f64.mul (get_local $x) (f64.const 0.0)))
)

(assert_return (invoke "f32.no_fold_mul_zero" (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_mul_zero" (f32.const -1.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_mul_zero" (f32.const -2.0)) (f32.const -0.0))
(assert_return_arithmetic_nan (invoke "f32.no_fold_mul_zero" (f32.const nan:0x200000)))
(assert_return (invoke "f64.no_fold_mul_zero" (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_mul_zero" (f64.const -1.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_mul_zero" (f64.const -2.0)) (f64.const -0.0))
(assert_return_arithmetic_nan (invoke "f64.no_fold_mul_zero" (f64.const nan:0x4000000000000)))

;; Test that x*1.0 is not folded to x.
;; See IEEE 754-2008 10.4 "Literal meaning and value-changing optimizations".

(module
  (func (export "f32.no_fold_mul_one") (param $x f32) (result f32)
    (f32.mul (get_local $x) (f32.const 1.0)))
  (func (export "f64.no_fold_mul_one") (param $x f64) (result f64)
    (f64.mul (get_local $x) (f64.const 1.0)))
)

(assert_return_arithmetic_nan (invoke "f32.no_fold_mul_one" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_mul_one" (f64.const nan:0x4000000000000)))

;; Test that 0.0/x is not folded to 0.0.

(module
  (func (export "f32.no_fold_zero_div") (param $x f32) (result f32)
    (f32.div (f32.const 0.0) (get_local $x)))
  (func (export "f64.no_fold_zero_div") (param $x f64) (result f64)
    (f64.div (f64.const 0.0) (get_local $x)))
)

(assert_return_canonical_nan (invoke "f32.no_fold_zero_div" (f32.const 0.0)))
(assert_return_canonical_nan (invoke "f32.no_fold_zero_div" (f32.const -0.0)))
(assert_return_canonical_nan (invoke "f32.no_fold_zero_div" (f32.const nan)))
(assert_return_arithmetic_nan (invoke "f32.no_fold_zero_div" (f32.const nan:0x200000)))
(assert_return_canonical_nan (invoke "f64.no_fold_zero_div" (f64.const 0.0)))
(assert_return_canonical_nan (invoke "f64.no_fold_zero_div" (f64.const -0.0)))
(assert_return_canonical_nan (invoke "f64.no_fold_zero_div" (f64.const nan)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_zero_div" (f64.const nan:0x4000000000000)))

;; Test that x/1.0 is not folded to x.

(module
  (func (export "f32.no_fold_div_one") (param $x f32) (result f32)
    (f32.div (get_local $x) (f32.const 1.0)))
  (func (export "f64.no_fold_div_one") (param $x f64) (result f64)
    (f64.div (get_local $x) (f64.const 1.0)))
)

(assert_return_arithmetic_nan (invoke "f32.no_fold_div_one" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_div_one" (f64.const nan:0x4000000000000)))

;; Test that x/-1.0 is not folded to -x.

(module
  (func (export "f32.no_fold_div_neg1") (param $x f32) (result f32)
    (f32.div (get_local $x) (f32.const -1.0)))
  (func (export "f64.no_fold_div_neg1") (param $x f64) (result f64)
    (f64.div (get_local $x) (f64.const -1.0)))
)

(assert_return_arithmetic_nan (invoke "f32.no_fold_div_neg1" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_div_neg1" (f64.const nan:0x4000000000000)))

;; Test that -0.0 - x is not folded to -x.

(module
  (func (export "f32.no_fold_neg0_sub") (param $x f32) (result f32)
    (f32.sub (f32.const -0.0) (get_local $x)))
  (func (export "f64.no_fold_neg0_sub") (param $x f64) (result f64)
    (f64.sub (f64.const -0.0) (get_local $x)))
)

(assert_return_arithmetic_nan (invoke "f32.no_fold_neg0_sub" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_neg0_sub" (f64.const nan:0x4000000000000)))

;; Test that -1.0 * x is not folded to -x.

(module
  (func (export "f32.no_fold_neg1_mul") (param $x f32) (result f32)
    (f32.mul (f32.const -1.0) (get_local $x)))
  (func (export "f64.no_fold_neg1_mul") (param $x f64) (result f64)
    (f64.mul (f64.const -1.0) (get_local $x)))
)

(assert_return_arithmetic_nan (invoke "f32.no_fold_neg1_mul" (f32.const nan:0x200000)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_neg1_mul" (f64.const nan:0x4000000000000)))

;; Test that x == x is not folded to true.

(module
  (func (export "f32.no_fold_eq_self") (param $x f32) (result i32)
    (f32.eq (get_local $x) (get_local $x)))
  (func (export "f64.no_fold_eq_self") (param $x f64) (result i32)
    (f64.eq (get_local $x) (get_local $x)))
)

(assert_return (invoke "f32.no_fold_eq_self" (f32.const nan)) (i32.const 0))
(assert_return (invoke "f64.no_fold_eq_self" (f64.const nan)) (i32.const 0))

;; Test that x != x is not folded to false.

(module
  (func (export "f32.no_fold_ne_self") (param $x f32) (result i32)
    (f32.ne (get_local $x) (get_local $x)))
  (func (export "f64.no_fold_ne_self") (param $x f64) (result i32)
    (f64.ne (get_local $x) (get_local $x)))
)

(assert_return (invoke "f32.no_fold_ne_self" (f32.const nan)) (i32.const 1))
(assert_return (invoke "f64.no_fold_ne_self" (f64.const nan)) (i32.const 1))

;; Test that x - x is not folded to 0.0.

(module
  (func (export "f32.no_fold_sub_self") (param $x f32) (result f32)
    (f32.sub (get_local $x) (get_local $x)))
  (func (export "f64.no_fold_sub_self") (param $x f64) (result f64)
    (f64.sub (get_local $x) (get_local $x)))
)

(assert_return_canonical_nan (invoke "f32.no_fold_sub_self" (f32.const inf)))
(assert_return_canonical_nan (invoke "f32.no_fold_sub_self" (f32.const nan)))
(assert_return_canonical_nan (invoke "f64.no_fold_sub_self" (f64.const inf)))
(assert_return_canonical_nan (invoke "f64.no_fold_sub_self" (f64.const nan)))

;; Test that x / x is not folded to 1.0.

(module
  (func (export "f32.no_fold_div_self") (param $x f32) (result f32)
    (f32.div (get_local $x) (get_local $x)))
  (func (export "f64.no_fold_div_self") (param $x f64) (result f64)
    (f64.div (get_local $x) (get_local $x)))
)

(assert_return_canonical_nan (invoke "f32.no_fold_div_self" (f32.const inf)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_self" (f32.const nan)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_self" (f32.const 0.0)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_self" (f32.const -0.0)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_self" (f64.const inf)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_self" (f64.const nan)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_self" (f64.const 0.0)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_self" (f64.const -0.0)))

;; Test that x/3 is not folded to x*(1/3).

(module
  (func (export "f32.no_fold_div_3") (param $x f32) (result f32)
    (f32.div (get_local $x) (f32.const 3.0)))
  (func (export "f64.no_fold_div_3") (param $x f64) (result f64)
    (f64.div (get_local $x) (f64.const 3.0)))
)

(assert_return (invoke "f32.no_fold_div_3" (f32.const -0x1.359c26p+50)) (f32.const -0x1.9cd032p+48))
(assert_return (invoke "f32.no_fold_div_3" (f32.const -0x1.e45646p+93)) (f32.const -0x1.42e42ep+92))
(assert_return (invoke "f32.no_fold_div_3" (f32.const -0x1.2a3916p-83)) (f32.const -0x1.8da172p-85))
(assert_return (invoke "f32.no_fold_div_3" (f32.const -0x1.1f8b38p-124)) (f32.const -0x1.7f644ap-126))
(assert_return (invoke "f32.no_fold_div_3" (f32.const -0x1.d64f64p-56)) (f32.const -0x1.398a42p-57))
(assert_return (invoke "f64.no_fold_div_3" (f64.const -0x1.a8a88d29e2cc3p+632)) (f64.const -0x1.1b1b08c69732dp+631))
(assert_return (invoke "f64.no_fold_div_3" (f64.const -0x1.bcf52dc950972p-167)) (f64.const -0x1.28a373db8b0f7p-168))
(assert_return (invoke "f64.no_fold_div_3" (f64.const 0x1.bd3c0d989f7a4p-874)) (f64.const 0x1.28d2b3bb14fc3p-875))
(assert_return (invoke "f64.no_fold_div_3" (f64.const -0x1.0138bf530a53cp+1007)) (f64.const -0x1.56f6546eb86fbp+1005))
(assert_return (invoke "f64.no_fold_div_3" (f64.const 0x1.052b87f9d794dp+415)) (f64.const 0x1.5c3a0aa274c67p+413))

;; Test that (x*z)+(y*z) is not folded to (x+y)*z.

(module
  (func (export "f32.no_factor") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.add (f32.mul (get_local $x) (get_local $z)) (f32.mul (get_local $y) (get_local $z))))
  (func (export "f64.no_factor") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.add (f64.mul (get_local $x) (get_local $z)) (f64.mul (get_local $y) (get_local $z))))
)

(assert_return (invoke "f32.no_factor" (f32.const -0x1.4e2352p+40) (f32.const -0x1.842e2cp+49) (f32.const 0x1.eea602p+59)) (f32.const -0x1.77a7dp+109))
(assert_return (invoke "f32.no_factor" (f32.const -0x1.b4e7f6p-6) (f32.const 0x1.8c990cp-5) (f32.const -0x1.70cc02p-9)) (f32.const -0x1.00a342p-14))
(assert_return (invoke "f32.no_factor" (f32.const -0x1.06722ep-41) (f32.const 0x1.eed3cep-64) (f32.const 0x1.5c5558p+123)) (f32.const -0x1.651aaep+82))
(assert_return (invoke "f32.no_factor" (f32.const -0x1.f8c6a4p-64) (f32.const 0x1.08c806p-83) (f32.const 0x1.b5ceccp+118)) (f32.const -0x1.afa15p+55))
(assert_return (invoke "f32.no_factor" (f32.const -0x1.3aaa1ep-84) (f32.const 0x1.c6d5eep-71) (f32.const 0x1.8d2924p+20)) (f32.const 0x1.60c9cep-50))
(assert_return (invoke "f64.no_factor" (f64.const 0x1.3adeda9144977p-424) (f64.const 0x1.c15af887049e1p-462) (f64.const -0x1.905179c4c4778p-225)) (f64.const -0x1.ec606bcb87b1ap-649))
(assert_return (invoke "f64.no_factor" (f64.const 0x1.3c84821c1d348p-662) (f64.const -0x1.4ffd4c77ad037p-1009) (f64.const -0x1.dd275335c6f4p-957)) (f64.const 0x0p+0))
(assert_return (invoke "f64.no_factor" (f64.const -0x1.074f372347051p-334) (f64.const -0x1.aaeef661f4c96p-282) (f64.const -0x1.9bd34abe8696dp+479)) (f64.const 0x1.5767029593e2p+198))
(assert_return (invoke "f64.no_factor" (f64.const -0x1.c4ded58a6f389p-289) (f64.const 0x1.ba6fdef5d59c9p-260) (f64.const -0x1.c1201c0470205p-253)) (f64.const -0x1.841ada2e0f184p-512))
(assert_return (invoke "f64.no_factor" (f64.const 0x1.9d3688f8e375ap-608) (f64.const 0x1.bf91311588256p-579) (f64.const -0x1.1605a6b5d5ff8p+489)) (f64.const -0x1.e6118ca76af53p-90))

;; Test that (x+y)*z is not folded to (x*z)+(y*z).

(module
  (func (export "f32.no_distribute") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.mul (f32.add (get_local $x) (get_local $y)) (get_local $z)))
  (func (export "f64.no_distribute") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.mul (f64.add (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f32.no_distribute" (f32.const -0x1.4e2352p+40) (f32.const -0x1.842e2cp+49) (f32.const 0x1.eea602p+59)) (f32.const -0x1.77a7d2p+109))
(assert_return (invoke "f32.no_distribute" (f32.const -0x1.b4e7f6p-6) (f32.const 0x1.8c990cp-5) (f32.const -0x1.70cc02p-9)) (f32.const -0x1.00a34p-14))
(assert_return (invoke "f32.no_distribute" (f32.const -0x1.06722ep-41) (f32.const 0x1.eed3cep-64) (f32.const 0x1.5c5558p+123)) (f32.const -0x1.651abp+82))
(assert_return (invoke "f32.no_distribute" (f32.const -0x1.f8c6a4p-64) (f32.const 0x1.08c806p-83) (f32.const 0x1.b5ceccp+118)) (f32.const -0x1.afa14ep+55))
(assert_return (invoke "f32.no_distribute" (f32.const -0x1.3aaa1ep-84) (f32.const 0x1.c6d5eep-71) (f32.const 0x1.8d2924p+20)) (f32.const 0x1.60c9ccp-50))
(assert_return (invoke "f64.no_distribute" (f64.const 0x1.3adeda9144977p-424) (f64.const 0x1.c15af887049e1p-462) (f64.const -0x1.905179c4c4778p-225)) (f64.const -0x1.ec606bcb87b1bp-649))
(assert_return (invoke "f64.no_distribute" (f64.const 0x1.3c84821c1d348p-662) (f64.const -0x1.4ffd4c77ad037p-1009) (f64.const -0x1.dd275335c6f4p-957)) (f64.const -0x0p+0))
(assert_return (invoke "f64.no_distribute" (f64.const -0x1.074f372347051p-334) (f64.const -0x1.aaeef661f4c96p-282) (f64.const -0x1.9bd34abe8696dp+479)) (f64.const 0x1.5767029593e1fp+198))
(assert_return (invoke "f64.no_distribute" (f64.const -0x1.c4ded58a6f389p-289) (f64.const 0x1.ba6fdef5d59c9p-260) (f64.const -0x1.c1201c0470205p-253)) (f64.const -0x1.841ada2e0f183p-512))
(assert_return (invoke "f64.no_distribute" (f64.const 0x1.9d3688f8e375ap-608) (f64.const 0x1.bf91311588256p-579) (f64.const -0x1.1605a6b5d5ff8p+489)) (f64.const -0x1.e6118ca76af52p-90))

;; Test that x*(y/z) is not folded to (x*y)/z.

(module
  (func (export "f32.no_regroup_div_mul") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.mul (get_local $x) (f32.div (get_local $y) (get_local $z))))
  (func (export "f64.no_regroup_div_mul") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.mul (get_local $x) (f64.div (get_local $y) (get_local $z))))
)

(assert_return (invoke "f32.no_regroup_div_mul" (f32.const -0x1.2d14a6p-115) (f32.const -0x1.575a6cp-64) (f32.const 0x1.5cee0ep-116)) (f32.const 0x1.2844cap-63))
(assert_return (invoke "f32.no_regroup_div_mul" (f32.const -0x1.454738p+91) (f32.const -0x1.b28a66p-115) (f32.const -0x1.f53908p+72)) (f32.const -0x0p+0))
(assert_return (invoke "f32.no_regroup_div_mul" (f32.const -0x1.6be56ep+16) (f32.const -0x1.b46fc6p-21) (f32.const -0x1.a51df6p-123)) (f32.const -0x1.792258p+118))
(assert_return (invoke "f32.no_regroup_div_mul" (f32.const -0x1.c343f8p-94) (f32.const 0x1.e4d906p+73) (f32.const 0x1.be69f8p+68)) (f32.const -0x1.ea1df2p-89))
(assert_return (invoke "f32.no_regroup_div_mul" (f32.const 0x1.c6ae76p+112) (f32.const 0x1.fc953cp+24) (f32.const -0x1.60b3e8p+71)) (f32.const -0x1.47d0eap+66))
(assert_return (invoke "f64.no_regroup_div_mul" (f64.const 0x1.3c04b815e30bp-423) (f64.const -0x1.379646fd98127p-119) (f64.const 0x1.bddb158506031p-642)) (f64.const -0x1.b9b3301f2dd2dp+99))
(assert_return (invoke "f64.no_regroup_div_mul" (f64.const 0x1.46b3a402f86d5p+337) (f64.const 0x1.6fbf1b9e1798dp-447) (f64.const -0x1.bd9704a5a6a06p+797)) (f64.const -0x0p+0))
(assert_return (invoke "f64.no_regroup_div_mul" (f64.const 0x1.6c9765bb4347fp-479) (f64.const 0x1.a4af42e34a141p+902) (f64.const 0x1.d2dde70eb68f9p-448)) (f64.const inf))
(assert_return (invoke "f64.no_regroup_div_mul" (f64.const -0x1.706023645be72p+480) (f64.const -0x1.6c229f7d9101dp+611) (f64.const -0x1.4d50fa68d3d9ep+836)) (f64.const -0x1.926fa3cacc651p+255))
(assert_return (invoke "f64.no_regroup_div_mul" (f64.const 0x1.8cc63d8caf4c7p-599) (f64.const 0x1.8671ac4c35753p-878) (f64.const -0x1.ef35b1695e659p-838)) (f64.const -0x1.38d55f56406dp-639))

;; Test that (x*y)/z is not folded to x*(y/z).

(module
  (func (export "f32.no_regroup_mul_div") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.div (f32.mul (get_local $x) (get_local $y)) (get_local $z)))
  (func (export "f64.no_regroup_mul_div") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.div (f64.mul (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f32.no_regroup_mul_div" (f32.const -0x1.2d14a6p-115) (f32.const -0x1.575a6cp-64) (f32.const 0x1.5cee0ep-116)) (f32.const 0x0p+0))
(assert_return (invoke "f32.no_regroup_mul_div" (f32.const -0x1.454738p+91) (f32.const -0x1.b28a66p-115) (f32.const -0x1.f53908p+72)) (f32.const -0x1.1a00e8p-96))
(assert_return (invoke "f32.no_regroup_mul_div" (f32.const -0x1.6be56ep+16) (f32.const -0x1.b46fc6p-21) (f32.const -0x1.a51df6p-123)) (f32.const -0x1.79225ap+118))
(assert_return (invoke "f32.no_regroup_mul_div" (f32.const -0x1.c343f8p-94) (f32.const 0x1.e4d906p+73) (f32.const 0x1.be69f8p+68)) (f32.const -0x1.ea1df4p-89))
(assert_return (invoke "f32.no_regroup_mul_div" (f32.const 0x1.c6ae76p+112) (f32.const 0x1.fc953cp+24) (f32.const -0x1.60b3e8p+71)) (f32.const -inf))
(assert_return (invoke "f64.no_regroup_mul_div" (f64.const 0x1.3c04b815e30bp-423) (f64.const -0x1.379646fd98127p-119) (f64.const 0x1.bddb158506031p-642)) (f64.const -0x1.b9b3301f2dd2ep+99))
(assert_return (invoke "f64.no_regroup_mul_div" (f64.const 0x1.46b3a402f86d5p+337) (f64.const 0x1.6fbf1b9e1798dp-447) (f64.const -0x1.bd9704a5a6a06p+797)) (f64.const -0x1.0da0b6328e09p-907))
(assert_return (invoke "f64.no_regroup_mul_div" (f64.const 0x1.6c9765bb4347fp-479) (f64.const 0x1.a4af42e34a141p+902) (f64.const 0x1.d2dde70eb68f9p-448)) (f64.const 0x1.4886b6d9a9a79p+871))
(assert_return (invoke "f64.no_regroup_mul_div" (f64.const -0x1.706023645be72p+480) (f64.const -0x1.6c229f7d9101dp+611) (f64.const -0x1.4d50fa68d3d9ep+836)) (f64.const -inf))
(assert_return (invoke "f64.no_regroup_mul_div" (f64.const 0x1.8cc63d8caf4c7p-599) (f64.const 0x1.8671ac4c35753p-878) (f64.const -0x1.ef35b1695e659p-838)) (f64.const -0x0p+0))

;; Test that x+y+z+w is not reassociated.

(module
  (func (export "f32.no_reassociate_add") (param $x f32) (param $y f32) (param $z f32) (param $w f32) (result f32)
    (f32.add (f32.add (f32.add (get_local $x) (get_local $y)) (get_local $z)) (get_local $w)))
  (func (export "f64.no_reassociate_add") (param $x f64) (param $y f64) (param $z f64) (param $w f64) (result f64)
    (f64.add (f64.add (f64.add (get_local $x) (get_local $y)) (get_local $z)) (get_local $w)))
)

(assert_return (invoke "f32.no_reassociate_add" (f32.const -0x1.5f7ddcp+44) (f32.const 0x1.854e1p+34) (f32.const -0x1.b2068cp+47) (f32.const -0x1.209692p+41)) (f32.const -0x1.e26c76p+47))
(assert_return (invoke "f32.no_reassociate_add" (f32.const 0x1.da3b78p-9) (f32.const -0x1.4312fap-7) (f32.const 0x1.0395e6p-4) (f32.const -0x1.6d5ea6p-7)) (f32.const 0x1.78b31ap-5))
(assert_return (invoke "f32.no_reassociate_add" (f32.const -0x1.fdb93ap+34) (f32.const -0x1.b6fce6p+41) (f32.const 0x1.c131d8p+44) (f32.const 0x1.8835b6p+38)) (f32.const 0x1.8ff3a2p+44))
(assert_return (invoke "f32.no_reassociate_add" (f32.const 0x1.1739fcp+47) (f32.const 0x1.a4b186p+49) (f32.const -0x1.0c623cp+35) (f32.const 0x1.16a102p+51)) (f32.const 0x1.913ff6p+51))
(assert_return (invoke "f32.no_reassociate_add" (f32.const 0x1.733cfap+108) (f32.const -0x1.38d30cp+108) (f32.const 0x1.2f5854p+105) (f32.const -0x1.ccb058p+94)) (f32.const 0x1.813716p+106))
(assert_return (invoke "f64.no_reassociate_add" (f64.const -0x1.697a4d9ff19a6p+841) (f64.const 0x1.b305466238397p+847) (f64.const 0x1.e0b2d9bfb4e72p+855) (f64.const -0x1.6e1f3ae2b06bbp+857)) (f64.const -0x1.eb0e5936f087ap+856))
(assert_return (invoke "f64.no_reassociate_add" (f64.const 0x1.00ef6746b30e1p-543) (f64.const 0x1.cc1cfafdf3fe1p-544) (f64.const -0x1.f7726df3ecba6p-543) (f64.const -0x1.b26695f99d307p-594)) (f64.const -0x1.074892e3fad76p-547))
(assert_return (invoke "f64.no_reassociate_add" (f64.const -0x1.e807b3bd6d854p+440) (f64.const 0x1.cedae26c2c5fp+407) (f64.const -0x1.00ab6e1442541p+437) (f64.const 0x1.28538a55997bdp+397)) (f64.const -0x1.040e90bf871ebp+441))
(assert_return (invoke "f64.no_reassociate_add" (f64.const -0x1.ba2b6f35a2402p-317) (f64.const 0x1.ad1c3fea7cd9ep-307) (f64.const -0x1.93aace2bf1261p-262) (f64.const 0x1.9fddbe472847ep-260)) (f64.const 0x1.3af30abc2c01bp-260))
(assert_return (invoke "f64.no_reassociate_add" (f64.const -0x1.ccb9c6092fb1dp+641) (f64.const -0x1.4b7c28c108244p+614) (f64.const 0x1.8a7cefef4bde1p+646) (f64.const -0x1.901b28b08b482p+644)) (f64.const 0x1.1810579194126p+646))

;; Test that x*y*z*w is not reassociated.

(module
  (func (export "f32.no_reassociate_mul") (param $x f32) (param $y f32) (param $z f32) (param $w f32) (result f32)
    (f32.mul (f32.mul (f32.mul (get_local $x) (get_local $y)) (get_local $z)) (get_local $w)))
  (func (export "f64.no_reassociate_mul") (param $x f64) (param $y f64) (param $z f64) (param $w f64) (result f64)
    (f64.mul (f64.mul (f64.mul (get_local $x) (get_local $y)) (get_local $z)) (get_local $w)))
)

(assert_return (invoke "f32.no_reassociate_mul" (f32.const 0x1.950ba8p-116) (f32.const 0x1.efdacep-33) (f32.const -0x1.5f9bcp+102) (f32.const 0x1.f04508p-56)) (f32.const -0x1.ff356ep-101))
(assert_return (invoke "f32.no_reassociate_mul" (f32.const 0x1.5990aep-56) (f32.const -0x1.7dfb04p+102) (f32.const -0x1.4f774ap-125) (f32.const -0x1.595fe6p+70)) (f32.const -0x1.c7c8fcp-8))
(assert_return (invoke "f32.no_reassociate_mul" (f32.const 0x1.6ad9a4p-48) (f32.const -0x1.9138aap+55) (f32.const -0x1.4a774ep-40) (f32.const 0x1.1ff08p+76)) (f32.const 0x1.9cd8ecp+44))
(assert_return (invoke "f32.no_reassociate_mul" (f32.const 0x1.e1caecp-105) (f32.const 0x1.af0dd2p+77) (f32.const -0x1.016eep+56) (f32.const -0x1.ab70d6p+59)) (f32.const 0x1.54870ep+89))
(assert_return (invoke "f32.no_reassociate_mul" (f32.const -0x1.3b1dcp-99) (f32.const 0x1.4e5a34p-49) (f32.const -0x1.38ba5ap+3) (f32.const 0x1.7fb8eep+59)) (f32.const 0x1.5bbf98p-85))
(assert_return (invoke "f64.no_reassociate_mul" (f64.const -0x1.e7842ab7181p-667) (f64.const -0x1.fabf40ceeceafp+990) (f64.const -0x1.1a38a825ab01ap-376) (f64.const -0x1.27e8ea469b14fp+664)) (f64.const 0x1.336eb428af4f3p+613))
(assert_return (invoke "f64.no_reassociate_mul" (f64.const 0x1.4ca2292a6acbcp+454) (f64.const 0x1.6ffbab850089ap-516) (f64.const -0x1.547c32e1f5b93p-899) (f64.const -0x1.c7571d9388375p+540)) (f64.const 0x1.1ac796954fc1p-419))
(assert_return (invoke "f64.no_reassociate_mul" (f64.const 0x1.73881a52e0401p-501) (f64.const -0x1.1b68dd9efb1a7p+788) (f64.const 0x1.d1c5e6a3eb27cp-762) (f64.const -0x1.56cb2fcc7546fp+88)) (f64.const 0x1.f508db92c34efp-386))
(assert_return (invoke "f64.no_reassociate_mul" (f64.const 0x1.2efa87859987cp+692) (f64.const 0x1.68e4373e241p-423) (f64.const 0x1.4e2d0fb383a57p+223) (f64.const -0x1.301d3265c737bp-23)) (f64.const -0x1.4b2b6c393f30cp+470))
(assert_return (invoke "f64.no_reassociate_mul" (f64.const 0x1.1013f7498b95fp-234) (f64.const 0x1.d2d1c36fff138p-792) (f64.const -0x1.cbf1824ea7bfdp+728) (f64.const -0x1.440da9c8b836dp-599)) (f64.const 0x1.1a16512881c91p-895))

;; Test that x/0 is not folded away.

(module
  (func (export "f32.no_fold_div_0") (param $x f32) (result f32)
    (f32.div (get_local $x) (f32.const 0.0)))
  (func (export "f64.no_fold_div_0") (param $x f64) (result f64)
    (f64.div (get_local $x) (f64.const 0.0)))
)

(assert_return (invoke "f32.no_fold_div_0" (f32.const 1.0)) (f32.const inf))
(assert_return (invoke "f32.no_fold_div_0" (f32.const -1.0)) (f32.const -inf))
(assert_return (invoke "f32.no_fold_div_0" (f32.const inf)) (f32.const inf))
(assert_return (invoke "f32.no_fold_div_0" (f32.const -inf)) (f32.const -inf))
(assert_return_canonical_nan (invoke "f32.no_fold_div_0" (f32.const 0)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_0" (f32.const -0)))
(assert_return_arithmetic_nan (invoke "f32.no_fold_div_0" (f32.const nan:0x200000)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_0" (f32.const nan)))
(assert_return (invoke "f64.no_fold_div_0" (f64.const 1.0)) (f64.const inf))
(assert_return (invoke "f64.no_fold_div_0" (f64.const -1.0)) (f64.const -inf))
(assert_return (invoke "f64.no_fold_div_0" (f64.const inf)) (f64.const inf))
(assert_return (invoke "f64.no_fold_div_0" (f64.const -inf)) (f64.const -inf))
(assert_return_canonical_nan (invoke "f64.no_fold_div_0" (f64.const 0)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_0" (f64.const -0)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_0" (f64.const nan)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_div_0" (f64.const nan:0x4000000000000)))

;; Test that x/-0 is not folded away.

(module
  (func (export "f32.no_fold_div_neg0") (param $x f32) (result f32)
    (f32.div (get_local $x) (f32.const -0.0)))
  (func (export "f64.no_fold_div_neg0") (param $x f64) (result f64)
    (f64.div (get_local $x) (f64.const -0.0)))
)

(assert_return (invoke "f32.no_fold_div_neg0" (f32.const 1.0)) (f32.const -inf))
(assert_return (invoke "f32.no_fold_div_neg0" (f32.const -1.0)) (f32.const inf))
(assert_return (invoke "f32.no_fold_div_neg0" (f32.const inf)) (f32.const -inf))
(assert_return (invoke "f32.no_fold_div_neg0" (f32.const -inf)) (f32.const inf))
(assert_return_canonical_nan (invoke "f32.no_fold_div_neg0" (f32.const 0)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_neg0" (f32.const -0)))
(assert_return_arithmetic_nan (invoke "f32.no_fold_div_neg0" (f32.const nan:0x200000)))
(assert_return_canonical_nan (invoke "f32.no_fold_div_neg0" (f32.const nan)))
(assert_return (invoke "f64.no_fold_div_neg0" (f64.const 1.0)) (f64.const -inf))
(assert_return (invoke "f64.no_fold_div_neg0" (f64.const -1.0)) (f64.const inf))
(assert_return (invoke "f64.no_fold_div_neg0" (f64.const inf)) (f64.const -inf))
(assert_return (invoke "f64.no_fold_div_neg0" (f64.const -inf)) (f64.const inf))
(assert_return_canonical_nan (invoke "f64.no_fold_div_neg0" (f64.const 0)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_neg0" (f64.const -0)))
(assert_return_canonical_nan (invoke "f64.no_fold_div_neg0" (f64.const nan)))
(assert_return_arithmetic_nan (invoke "f64.no_fold_div_neg0" (f64.const nan:0x4000000000000)))

;; Test that sqrt(x*x+y*y) is not folded to hypot.

(module
  (func (export "f32.no_fold_to_hypot") (param $x f32) (param $y f32) (result f32)
    (f32.sqrt (f32.add (f32.mul (get_local $x) (get_local $x))
                       (f32.mul (get_local $y) (get_local $y)))))
  (func (export "f64.no_fold_to_hypot") (param $x f64) (param $y f64) (result f64)
    (f64.sqrt (f64.add (f64.mul (get_local $x) (get_local $x))
                       (f64.mul (get_local $y) (get_local $y)))))
)

(assert_return (invoke "f32.no_fold_to_hypot" (f32.const 0x1.c2f338p-81) (f32.const 0x1.401b5ep-68)) (f32.const 0x1.401cccp-68))
(assert_return (invoke "f32.no_fold_to_hypot" (f32.const -0x1.c38d1p-71) (f32.const -0x1.359ddp-107)) (f32.const 0x1.c36a62p-71))
(assert_return (invoke "f32.no_fold_to_hypot" (f32.const -0x1.99e0cap-114) (f32.const -0x1.ed0c6cp-69)) (f32.const 0x1.ed0e48p-69))
(assert_return (invoke "f32.no_fold_to_hypot" (f32.const -0x1.1b6ceap+5) (f32.const 0x1.5440bep+17)) (f32.const 0x1.5440cp+17))
(assert_return (invoke "f32.no_fold_to_hypot" (f32.const 0x1.8f019ep-76) (f32.const -0x1.182308p-71)) (f32.const 0x1.17e2bcp-71))
(assert_return (invoke "f64.no_fold_to_hypot" (f64.const 0x1.1a0ac4f7c8711p-636) (f64.const 0x1.1372ebafff551p-534)) (f64.const 0x1.13463fa37014ep-534))
(assert_return (invoke "f64.no_fold_to_hypot" (f64.const 0x1.b793512167499p+395) (f64.const -0x1.11cbc52af4c36p+410)) (f64.const 0x1.11cbc530783a2p+410))
(assert_return (invoke "f64.no_fold_to_hypot" (f64.const 0x1.76777f44ff40bp-536) (f64.const -0x1.c3896e4dc1fbp-766)) (f64.const 0x1.8p-536))
(assert_return (invoke "f64.no_fold_to_hypot" (f64.const -0x1.889ac72cc6b5dp-521) (f64.const 0x1.8d7084e659f3bp-733)) (f64.const 0x1.889ac72ca843ap-521))
(assert_return (invoke "f64.no_fold_to_hypot" (f64.const 0x1.5ee588c02cb08p-670) (f64.const -0x1.05ce25788d9ecp-514)) (f64.const 0x1.05ce25788d9dfp-514))

;; Test that 1.0/x isn't approximated.

(module
  (func (export "f32.no_approximate_reciprocal") (param $x f32) (result f32)
    (f32.div (f32.const 1.0) (get_local $x)))
)

(assert_return (invoke "f32.no_approximate_reciprocal" (f32.const -0x1.2900b6p-10)) (f32.const -0x1.b950d4p+9))
(assert_return (invoke "f32.no_approximate_reciprocal" (f32.const 0x1.e7212p+127)) (f32.const 0x1.0d11f8p-128))
(assert_return (invoke "f32.no_approximate_reciprocal" (f32.const -0x1.42a466p-93)) (f32.const -0x1.963ee6p+92))
(assert_return (invoke "f32.no_approximate_reciprocal" (f32.const 0x1.5d0c32p+76)) (f32.const 0x1.778362p-77))
(assert_return (invoke "f32.no_approximate_reciprocal" (f32.const -0x1.601de2p-82)) (f32.const -0x1.743d7ep+81))

;; Test that 1.0/sqrt(x) isn't approximated or fused.

(module
  (func (export "f32.no_approximate_reciprocal_sqrt") (param $x f32) (result f32)
    (f32.div (f32.const 1.0) (f32.sqrt (get_local $x))))
  (func (export "f64.no_fuse_reciprocal_sqrt") (param $x f64) (result f64)
    (f64.div (f64.const 1.0) (f64.sqrt (get_local $x))))
)

(assert_return (invoke "f32.no_approximate_reciprocal_sqrt" (f32.const 0x1.6af12ap-43)) (f32.const 0x1.300ed4p+21))
(assert_return (invoke "f32.no_approximate_reciprocal_sqrt" (f32.const 0x1.e82fc6p-8)) (f32.const 0x1.72c376p+3))
(assert_return (invoke "f32.no_approximate_reciprocal_sqrt" (f32.const 0x1.b9fa9cp-66)) (f32.const 0x1.85a9bap+32))
(assert_return (invoke "f32.no_approximate_reciprocal_sqrt" (f32.const 0x1.f4f546p-44)) (f32.const 0x1.6e01c2p+21))
(assert_return (invoke "f32.no_approximate_reciprocal_sqrt" (f32.const 0x1.5da7aap-86)) (f32.const 0x1.b618cap+42))

(assert_return (invoke "f64.no_fuse_reciprocal_sqrt" (f64.const 0x1.1568a63b55fa3p+889)) (f64.const 0x1.5bc9c74c9952p-445))
(assert_return (invoke "f64.no_fuse_reciprocal_sqrt" (f64.const 0x1.239fcd0939cafp+311)) (f64.const 0x1.5334a922b4818p-156))
(assert_return (invoke "f64.no_fuse_reciprocal_sqrt" (f64.const 0x1.6e36a24e11054p+104)) (f64.const 0x1.ac13f20977f29p-53))
(assert_return (invoke "f64.no_fuse_reciprocal_sqrt" (f64.const 0x1.23ee173219f83p+668)) (f64.const 0x1.df753e055862dp-335))
(assert_return (invoke "f64.no_fuse_reciprocal_sqrt" (f64.const 0x1.b30f74caf9babp+146)) (f64.const 0x1.88bfc3d1764a9p-74))

;; Test that sqrt(1.0/x) isn't approximated.

(module
  (func (export "f32.no_approximate_sqrt_reciprocal") (param $x f32) (result f32)
    (f32.sqrt (f32.div (f32.const 1.0) (get_local $x))))
)

(assert_return (invoke "f32.no_approximate_sqrt_reciprocal" (f32.const 0x1.a4c986p+60)) (f32.const 0x1.8f5ac6p-31))
(assert_return (invoke "f32.no_approximate_sqrt_reciprocal" (f32.const 0x1.50511ep-9)) (f32.const 0x1.3bdd46p+4))
(assert_return (invoke "f32.no_approximate_sqrt_reciprocal" (f32.const 0x1.125ec2p+69)) (f32.const 0x1.5db572p-35))
(assert_return (invoke "f32.no_approximate_sqrt_reciprocal" (f32.const 0x1.ba4c5p+13)) (f32.const 0x1.136f16p-7))
(assert_return (invoke "f32.no_approximate_sqrt_reciprocal" (f32.const 0x1.4a5be2p+104)) (f32.const 0x1.c2b5bp-53))

;; Test that converting i32/i64 to f32/f64 and back isn't folded away.

(module
  (func (export "i32.no_fold_f32_s") (param i32) (result i32)
    (i32.trunc_s/f32 (f32.convert_s/i32 (get_local 0))))
  (func (export "i32.no_fold_f32_u") (param i32) (result i32)
    (i32.trunc_u/f32 (f32.convert_u/i32 (get_local 0))))
  (func (export "i64.no_fold_f64_s") (param i64) (result i64)
    (i64.trunc_s/f64 (f64.convert_s/i64 (get_local 0))))
  (func (export "i64.no_fold_f64_u") (param i64) (result i64)
    (i64.trunc_u/f64 (f64.convert_u/i64 (get_local 0))))
)

(assert_return (invoke "i32.no_fold_f32_s" (i32.const 0x1000000)) (i32.const 0x1000000))
(assert_return (invoke "i32.no_fold_f32_s" (i32.const 0x1000001)) (i32.const 0x1000000))
(assert_return (invoke "i32.no_fold_f32_s" (i32.const 0xf0000010)) (i32.const 0xf0000010))

(assert_return (invoke "i32.no_fold_f32_u" (i32.const 0x1000000)) (i32.const 0x1000000))
(assert_return (invoke "i32.no_fold_f32_u" (i32.const 0x1000001)) (i32.const 0x1000000))
(assert_return (invoke "i32.no_fold_f32_u" (i32.const 0xf0000010)) (i32.const 0xf0000000))

(assert_return (invoke "i64.no_fold_f64_s" (i64.const 0x20000000000000)) (i64.const 0x20000000000000))
(assert_return (invoke "i64.no_fold_f64_s" (i64.const 0x20000000000001)) (i64.const 0x20000000000000))
(assert_return (invoke "i64.no_fold_f64_s" (i64.const 0xf000000000000400)) (i64.const 0xf000000000000400))

(assert_return (invoke "i64.no_fold_f64_u" (i64.const 0x20000000000000)) (i64.const 0x20000000000000))
(assert_return (invoke "i64.no_fold_f64_u" (i64.const 0x20000000000001)) (i64.const 0x20000000000000))
(assert_return (invoke "i64.no_fold_f64_u" (i64.const 0xf000000000000400)) (i64.const 0xf000000000000000))

;; Test that x+y-y is not folded to x.

(module
  (func (export "f32.no_fold_add_sub") (param $x f32) (param $y f32) (result f32)
    (f32.sub (f32.add (get_local $x) (get_local $y)) (get_local $y)))
  (func (export "f64.no_fold_add_sub") (param $x f64) (param $y f64) (result f64)
    (f64.sub (f64.add (get_local $x) (get_local $y)) (get_local $y)))
)

(assert_return (invoke "f32.no_fold_add_sub" (f32.const 0x1.b553e4p-47) (f32.const -0x1.67db2cp-26)) (f32.const 0x1.cp-47))
(assert_return (invoke "f32.no_fold_add_sub" (f32.const -0x1.a884dp-23) (f32.const 0x1.f2ae1ep-19)) (f32.const -0x1.a884ep-23))
(assert_return (invoke "f32.no_fold_add_sub" (f32.const -0x1.fc04fp+82) (f32.const -0x1.65403ap+101)) (f32.const -0x1p+83))
(assert_return (invoke "f32.no_fold_add_sub" (f32.const 0x1.870fa2p-78) (f32.const 0x1.c54916p-56)) (f32.const 0x1.8p-78))
(assert_return (invoke "f32.no_fold_add_sub" (f32.const -0x1.17e966p-108) (f32.const -0x1.5fa61ap-84)) (f32.const -0x1p-107))

(assert_return (invoke "f64.no_fold_add_sub" (f64.const -0x1.1053ea172dba8p-874) (f64.const 0x1.113c413408ac8p-857)) (f64.const -0x1.1053ea172p-874))
(assert_return (invoke "f64.no_fold_add_sub" (f64.const 0x1.e377d54807972p-546) (f64.const 0x1.040a0a4d1ff7p-526)) (f64.const 0x1.e377d548p-546))
(assert_return (invoke "f64.no_fold_add_sub" (f64.const -0x1.75f53cd926b62p-30) (f64.const -0x1.66b176e602bb5p-3)) (f64.const -0x1.75f53dp-30))
(assert_return (invoke "f64.no_fold_add_sub" (f64.const -0x1.c450ff28332ap-341) (f64.const 0x1.15a5855023baep-305)) (f64.const -0x1.c451p-341))
(assert_return (invoke "f64.no_fold_add_sub" (f64.const -0x1.1ad4a596d3ea8p-619) (f64.const -0x1.17d81a41c0ea8p-588)) (f64.const -0x1.1ad4a8p-619))

;; Test that x-y+y is not folded to x.

(module
  (func (export "f32.no_fold_sub_add") (param $x f32) (param $y f32) (result f32)
    (f32.add (f32.sub (get_local $x) (get_local $y)) (get_local $y)))
  (func (export "f64.no_fold_sub_add") (param $x f64) (param $y f64) (result f64)
    (f64.add (f64.sub (get_local $x) (get_local $y)) (get_local $y)))
)

(assert_return (invoke "f32.no_fold_sub_add" (f32.const -0x1.523cb8p+9) (f32.const 0x1.93096cp+8)) (f32.const -0x1.523cbap+9))
(assert_return (invoke "f32.no_fold_sub_add" (f32.const -0x1.a31a1p-111) (f32.const 0x1.745efp-95)) (f32.const -0x1.a4p-111))
(assert_return (invoke "f32.no_fold_sub_add" (f32.const 0x1.3d5328p+26) (f32.const 0x1.58567p+35)) (f32.const 0x1.3d54p+26))
(assert_return (invoke "f32.no_fold_sub_add" (f32.const 0x1.374e26p-39) (f32.const -0x1.66a5p-27)) (f32.const 0x1.374p-39))
(assert_return (invoke "f32.no_fold_sub_add" (f32.const 0x1.320facp-3) (f32.const -0x1.ac069ap+14)) (f32.const 0x1.34p-3))

(assert_return (invoke "f64.no_fold_sub_add" (f64.const 0x1.8f92aad2c9b8dp+255) (f64.const -0x1.08cd4992266cbp+259)) (f64.const 0x1.8f92aad2c9b9p+255))
(assert_return (invoke "f64.no_fold_sub_add" (f64.const 0x1.5aaff55742c8bp-666) (f64.const 0x1.8f5f47181f46dp-647)) (f64.const 0x1.5aaff5578p-666))
(assert_return (invoke "f64.no_fold_sub_add" (f64.const 0x1.21bc52967a98dp+251) (f64.const -0x1.fcffaa32d0884p+300)) (f64.const 0x1.2p+251))
(assert_return (invoke "f64.no_fold_sub_add" (f64.const 0x1.9c78361f47374p-26) (f64.const -0x1.69d69f4edc61cp-13)) (f64.const 0x1.9c78361f48p-26))
(assert_return (invoke "f64.no_fold_sub_add" (f64.const 0x1.4dbe68e4afab2p-367) (f64.const -0x1.dc24e5b39cd02p-361)) (f64.const 0x1.4dbe68e4afacp-367))

;; Test that x*y/y is not folded to x.

(module
  (func (export "f32.no_fold_mul_div") (param $x f32) (param $y f32) (result f32)
    (f32.div (f32.mul (get_local $x) (get_local $y)) (get_local $y)))
  (func (export "f64.no_fold_mul_div") (param $x f64) (param $y f64) (result f64)
    (f64.div (f64.mul (get_local $x) (get_local $y)) (get_local $y)))
)

(assert_return (invoke "f32.no_fold_mul_div" (f32.const -0x1.cd859ap+54) (f32.const 0x1.6ca936p-47)) (f32.const -0x1.cd8598p+54))
(assert_return (invoke "f32.no_fold_mul_div" (f32.const -0x1.0b56b8p-26) (f32.const 0x1.48264cp-106)) (f32.const -0x1.0b56a4p-26))
(assert_return (invoke "f32.no_fold_mul_div" (f32.const -0x1.e7555cp-48) (f32.const -0x1.9161cp+48)) (f32.const -0x1.e7555ap-48))
(assert_return (invoke "f32.no_fold_mul_div" (f32.const 0x1.aaa50ep+52) (f32.const -0x1.dfb39ep+60)) (f32.const 0x1.aaa50cp+52))
(assert_return (invoke "f32.no_fold_mul_div" (f32.const -0x1.2b7dfap-92) (f32.const -0x1.7c4ca6p-37)) (f32.const -0x1.2b7dfep-92))

(assert_return (invoke "f64.no_fold_mul_div" (f64.const -0x1.3d79ff4118a1ap-837) (f64.const -0x1.b8b5dda31808cp-205)) (f64.const -0x1.3d79ff412263ep-837))
(assert_return (invoke "f64.no_fold_mul_div" (f64.const 0x1.f894d1ee6b3a4p+384) (f64.const 0x1.8c2606d03d58ap+585)) (f64.const 0x1.f894d1ee6b3a5p+384))
(assert_return (invoke "f64.no_fold_mul_div" (f64.const -0x1.a022260acc993p+238) (f64.const -0x1.5fbc128fc8e3cp-552)) (f64.const -0x1.a022260acc992p+238))
(assert_return (invoke "f64.no_fold_mul_div" (f64.const 0x1.9d4b8ed174f54p-166) (f64.const 0x1.ee3d467aeeac6p-906)) (f64.const 0x1.8dcc95a053b2bp-166))
(assert_return (invoke "f64.no_fold_mul_div" (f64.const -0x1.e95ea897cdcd4p+660) (f64.const -0x1.854d5df085f2ep-327)) (f64.const -0x1.e95ea897cdcd5p+660))

;; Test that x/y*y is not folded to x.

(module
  (func (export "f32.no_fold_div_mul") (param $x f32) (param $y f32) (result f32)
    (f32.mul (f32.div (get_local $x) (get_local $y)) (get_local $y)))
  (func (export "f64.no_fold_div_mul") (param $x f64) (param $y f64) (result f64)
    (f64.mul (f64.div (get_local $x) (get_local $y)) (get_local $y)))
)

(assert_return (invoke "f32.no_fold_div_mul" (f32.const -0x1.dc6364p+38) (f32.const 0x1.d630ecp+29)) (f32.const -0x1.dc6362p+38))
(assert_return (invoke "f32.no_fold_div_mul" (f32.const -0x1.1f9836p-52) (f32.const -0x1.16c4e4p-18)) (f32.const -0x1.1f9838p-52))
(assert_return (invoke "f32.no_fold_div_mul" (f32.const 0x1.c5972cp-126) (f32.const -0x1.d6659ep+7)) (f32.const 0x1.c5980ep-126))
(assert_return (invoke "f32.no_fold_div_mul" (f32.const -0x1.2e3a9ep-74) (f32.const -0x1.353994p+59)) (f32.const -0x1.2e3a4p-74))
(assert_return (invoke "f32.no_fold_div_mul" (f32.const 0x1.d96b82p-98) (f32.const 0x1.95d908p+27)) (f32.const 0x1.d96b84p-98))

(assert_return (invoke "f64.no_fold_div_mul" (f64.const 0x1.d01f913a52481p-876) (f64.const -0x1.2cd0668b28344p+184)) (f64.const 0x1.d020daf71cdcp-876))
(assert_return (invoke "f64.no_fold_div_mul" (f64.const -0x1.81cb7d400918dp-714) (f64.const 0x1.7caa643586d6ep-53)) (f64.const -0x1.81cb7d400918ep-714))
(assert_return (invoke "f64.no_fold_div_mul" (f64.const -0x1.66904c97b5c8ep-145) (f64.const 0x1.5c3481592ad4cp+428)) (f64.const -0x1.66904c97b5c8dp-145))
(assert_return (invoke "f64.no_fold_div_mul" (f64.const -0x1.e75859d2f0765p-278) (f64.const -0x1.5f19b6ab497f9p+283)) (f64.const -0x1.e75859d2f0764p-278))
(assert_return (invoke "f64.no_fold_div_mul" (f64.const -0x1.515fe9c3b5f5p+620) (f64.const 0x1.36be869c99f7ap+989)) (f64.const -0x1.515fe9c3b5f4fp+620))

;; Test that x/2*2 is not folded to x.

(module
  (func (export "f32.no_fold_div2_mul2") (param $x f32) (result f32)
    (f32.mul (f32.div (get_local $x) (f32.const 2.0)) (f32.const 2.0)))
  (func (export "f64.no_fold_div2_mul2") (param $x f64) (result f64)
    (f64.mul (f64.div (get_local $x) (f64.const 2.0)) (f64.const 2.0)))
)

(assert_return (invoke "f32.no_fold_div2_mul2" (f32.const 0x1.fffffep-126)) (f32.const 0x1p-125))
(assert_return (invoke "f64.no_fold_div2_mul2" (f64.const 0x1.fffffffffffffp-1022)) (f64.const 0x1p-1021))

;; Test that promote(demote(x)) is not folded to x.

(module
  (func (export "no_fold_demote_promote") (param $x f64) (result f64)
    (f64.promote/f32 (f32.demote/f64 (get_local $x))))
)

(assert_return (invoke "no_fold_demote_promote" (f64.const -0x1.dece272390f5dp-133)) (f64.const -0x1.decep-133))
(assert_return (invoke "no_fold_demote_promote" (f64.const -0x1.19e6c79938a6fp-85)) (f64.const -0x1.19e6c8p-85))
(assert_return (invoke "no_fold_demote_promote" (f64.const 0x1.49b297ec44dc1p+107)) (f64.const 0x1.49b298p+107))
(assert_return (invoke "no_fold_demote_promote" (f64.const -0x1.74f5bd865163p-88)) (f64.const -0x1.74f5bep-88))
(assert_return (invoke "no_fold_demote_promote" (f64.const 0x1.26d675662367ep+104)) (f64.const 0x1.26d676p+104))

;; Test that demote(promote(x)) is not folded to x, and aside from NaN is
;; bit-preserving.

(module
  (func (export "no_fold_promote_demote") (param $x f32) (result f32)
    (f32.demote/f64 (f64.promote/f32 (get_local $x))))
)

(assert_return_arithmetic_nan (invoke "no_fold_promote_demote" (f32.const nan:0x200000)))
(assert_return (invoke "no_fold_promote_demote" (f32.const 0x0p+0)) (f32.const 0x0p+0))
(assert_return (invoke "no_fold_promote_demote" (f32.const -0x0p+0)) (f32.const -0x0p+0))
(assert_return (invoke "no_fold_promote_demote" (f32.const 0x1p-149)) (f32.const 0x1p-149))
(assert_return (invoke "no_fold_promote_demote" (f32.const -0x1p-149)) (f32.const -0x1p-149))
(assert_return (invoke "no_fold_promote_demote" (f32.const 0x1.fffffcp-127)) (f32.const 0x1.fffffcp-127))
(assert_return (invoke "no_fold_promote_demote" (f32.const -0x1.fffffcp-127)) (f32.const -0x1.fffffcp-127))
(assert_return (invoke "no_fold_promote_demote" (f32.const 0x1p-126)) (f32.const 0x1p-126))
(assert_return (invoke "no_fold_promote_demote" (f32.const -0x1p-126)) (f32.const -0x1p-126))
(assert_return (invoke "no_fold_promote_demote" (f32.const 0x1.fffffep+127)) (f32.const 0x1.fffffep+127))
(assert_return (invoke "no_fold_promote_demote" (f32.const -0x1.fffffep+127)) (f32.const -0x1.fffffep+127))
(assert_return (invoke "no_fold_promote_demote" (f32.const inf)) (f32.const inf))
(assert_return (invoke "no_fold_promote_demote" (f32.const -inf)) (f32.const -inf))

;; Test that demote(x+promote(y)) is not folded to demote(x)+y.

(module
  (func (export "no_demote_mixed_add") (param $x f64) (param $y f32) (result f32)
    (f32.demote/f64 (f64.add (get_local $x) (f64.promote/f32 (get_local $y)))))
  (func (export "no_demote_mixed_add_commuted") (param $y f32) (param $x f64) (result f32)
    (f32.demote/f64 (f64.add (f64.promote/f32 (get_local $y)) (get_local $x))))
)

(assert_return (invoke "no_demote_mixed_add" (f64.const 0x1.f51a9d04854f9p-95) (f32.const 0x1.3f4e9cp-119)) (f32.const 0x1.f51a9ep-95))
(assert_return (invoke "no_demote_mixed_add" (f64.const 0x1.065b3d81ad8dp+37) (f32.const 0x1.758cd8p+38)) (f32.const 0x1.f8ba76p+38))
(assert_return (invoke "no_demote_mixed_add" (f64.const 0x1.626c80963bd17p-119) (f32.const -0x1.9bbf86p-121)) (f32.const 0x1.f6f93ep-120))
(assert_return (invoke "no_demote_mixed_add" (f64.const -0x1.0d5110e3385bbp-20) (f32.const 0x1.096f4ap-29)) (f32.const -0x1.0ccc5ap-20))
(assert_return (invoke "no_demote_mixed_add" (f64.const -0x1.73852db4e5075p-20) (f32.const -0x1.24e474p-41)) (f32.const -0x1.738536p-20))

(assert_return (invoke "no_demote_mixed_add_commuted" (f32.const 0x1.3f4e9cp-119) (f64.const 0x1.f51a9d04854f9p-95)) (f32.const 0x1.f51a9ep-95))
(assert_return (invoke "no_demote_mixed_add_commuted" (f32.const 0x1.758cd8p+38) (f64.const 0x1.065b3d81ad8dp+37)) (f32.const 0x1.f8ba76p+38))
(assert_return (invoke "no_demote_mixed_add_commuted" (f32.const -0x1.9bbf86p-121) (f64.const 0x1.626c80963bd17p-119)) (f32.const 0x1.f6f93ep-120))
(assert_return (invoke "no_demote_mixed_add_commuted" (f32.const 0x1.096f4ap-29) (f64.const -0x1.0d5110e3385bbp-20)) (f32.const -0x1.0ccc5ap-20))
(assert_return (invoke "no_demote_mixed_add_commuted" (f32.const -0x1.24e474p-41) (f64.const -0x1.73852db4e5075p-20)) (f32.const -0x1.738536p-20))

;; Test that demote(x-promote(y)) is not folded to demote(x)-y.

(module
  (func (export "no_demote_mixed_sub") (param $x f64) (param $y f32) (result f32)
    (f32.demote/f64 (f64.sub (get_local $x) (f64.promote/f32 (get_local $y)))))
)

(assert_return (invoke "no_demote_mixed_sub" (f64.const 0x1.a0a183220e9b1p+82) (f32.const 0x1.c5acf8p+61)) (f32.const 0x1.a0a174p+82))
(assert_return (invoke "no_demote_mixed_sub" (f64.const -0x1.6e2c5ac39f63ep+30) (f32.const 0x1.d48ca4p+17)) (f32.const -0x1.6e3bp+30))
(assert_return (invoke "no_demote_mixed_sub" (f64.const -0x1.98c74350dde6ap+6) (f32.const 0x1.9d69bcp-12)) (f32.const -0x1.98c7aap+6))
(assert_return (invoke "no_demote_mixed_sub" (f64.const 0x1.0459f34091dbfp-54) (f32.const 0x1.61ad08p-71)) (f32.const 0x1.045942p-54))
(assert_return (invoke "no_demote_mixed_sub" (f64.const 0x1.a7498dca3fdb7p+14) (f32.const 0x1.ed21c8p+15)) (f32.const -0x1.197d02p+15))

;; Test that converting between integer and float and back isn't folded away.

(module
  (func (export "f32.i32.no_fold_trunc_s_convert_s") (param $x f32) (result f32)
    (f32.convert_s/i32 (i32.trunc_s/f32 (get_local $x))))
  (func (export "f32.i32.no_fold_trunc_u_convert_s") (param $x f32) (result f32)
    (f32.convert_s/i32 (i32.trunc_u/f32 (get_local $x))))
  (func (export "f32.i32.no_fold_trunc_s_convert_u") (param $x f32) (result f32)
    (f32.convert_u/i32 (i32.trunc_s/f32 (get_local $x))))
  (func (export "f32.i32.no_fold_trunc_u_convert_u") (param $x f32) (result f32)
    (f32.convert_u/i32 (i32.trunc_u/f32 (get_local $x))))
  (func (export "f64.i32.no_fold_trunc_s_convert_s") (param $x f64) (result f64)
    (f64.convert_s/i32 (i32.trunc_s/f64 (get_local $x))))
  (func (export "f64.i32.no_fold_trunc_u_convert_s") (param $x f64) (result f64)
    (f64.convert_s/i32 (i32.trunc_u/f64 (get_local $x))))
  (func (export "f64.i32.no_fold_trunc_s_convert_u") (param $x f64) (result f64)
    (f64.convert_u/i32 (i32.trunc_s/f64 (get_local $x))))
  (func (export "f64.i32.no_fold_trunc_u_convert_u") (param $x f64) (result f64)
    (f64.convert_u/i32 (i32.trunc_u/f64 (get_local $x))))
  (func (export "f32.i64.no_fold_trunc_s_convert_s") (param $x f32) (result f32)
    (f32.convert_s/i64 (i64.trunc_s/f32 (get_local $x))))
  (func (export "f32.i64.no_fold_trunc_u_convert_s") (param $x f32) (result f32)
    (f32.convert_s/i64 (i64.trunc_u/f32 (get_local $x))))
  (func (export "f32.i64.no_fold_trunc_s_convert_u") (param $x f32) (result f32)
    (f32.convert_u/i64 (i64.trunc_s/f32 (get_local $x))))
  (func (export "f32.i64.no_fold_trunc_u_convert_u") (param $x f32) (result f32)
    (f32.convert_u/i64 (i64.trunc_u/f32 (get_local $x))))
  (func (export "f64.i64.no_fold_trunc_s_convert_s") (param $x f64) (result f64)
    (f64.convert_s/i64 (i64.trunc_s/f64 (get_local $x))))
  (func (export "f64.i64.no_fold_trunc_u_convert_s") (param $x f64) (result f64)
    (f64.convert_s/i64 (i64.trunc_u/f64 (get_local $x))))
  (func (export "f64.i64.no_fold_trunc_s_convert_u") (param $x f64) (result f64)
    (f64.convert_u/i64 (i64.trunc_s/f64 (get_local $x))))
  (func (export "f64.i64.no_fold_trunc_u_convert_u") (param $x f64) (result f64)
    (f64.convert_u/i64 (i64.trunc_u/f64 (get_local $x))))
)

(assert_return (invoke "f32.i32.no_fold_trunc_s_convert_s" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i32.no_fold_trunc_s_convert_s" (f32.const -1.5)) (f32.const -1.0))
(assert_return (invoke "f32.i32.no_fold_trunc_u_convert_s" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i32.no_fold_trunc_u_convert_s" (f32.const -0.5)) (f32.const 0.0))
(assert_return (invoke "f32.i32.no_fold_trunc_s_convert_u" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i32.no_fold_trunc_s_convert_u" (f32.const -1.5)) (f32.const 0x1p+32))
(assert_return (invoke "f32.i32.no_fold_trunc_u_convert_u" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i32.no_fold_trunc_u_convert_u" (f32.const -0.5)) (f32.const 0.0))

(assert_return (invoke "f64.i32.no_fold_trunc_s_convert_s" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i32.no_fold_trunc_s_convert_s" (f64.const -1.5)) (f64.const -1.0))
(assert_return (invoke "f64.i32.no_fold_trunc_u_convert_s" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i32.no_fold_trunc_u_convert_s" (f64.const -0.5)) (f64.const 0.0))
(assert_return (invoke "f64.i32.no_fold_trunc_s_convert_u" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i32.no_fold_trunc_s_convert_u" (f64.const -1.5)) (f64.const 0x1.fffffffep+31))
(assert_return (invoke "f64.i32.no_fold_trunc_u_convert_u" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i32.no_fold_trunc_u_convert_u" (f64.const -0.5)) (f64.const 0.0))

(assert_return (invoke "f32.i64.no_fold_trunc_s_convert_s" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i64.no_fold_trunc_s_convert_s" (f32.const -1.5)) (f32.const -1.0))
(assert_return (invoke "f32.i64.no_fold_trunc_u_convert_s" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i64.no_fold_trunc_u_convert_s" (f32.const -0.5)) (f32.const 0.0))
(assert_return (invoke "f32.i64.no_fold_trunc_s_convert_u" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i64.no_fold_trunc_s_convert_u" (f32.const -1.5)) (f32.const 0x1p+64))
(assert_return (invoke "f32.i64.no_fold_trunc_u_convert_u" (f32.const 1.5)) (f32.const 1.0))
(assert_return (invoke "f32.i64.no_fold_trunc_u_convert_u" (f32.const -0.5)) (f32.const 0.0))

(assert_return (invoke "f64.i64.no_fold_trunc_s_convert_s" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i64.no_fold_trunc_s_convert_s" (f64.const -1.5)) (f64.const -1.0))
(assert_return (invoke "f64.i64.no_fold_trunc_u_convert_s" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i64.no_fold_trunc_u_convert_s" (f64.const -0.5)) (f64.const 0.0))
(assert_return (invoke "f64.i64.no_fold_trunc_s_convert_u" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i64.no_fold_trunc_s_convert_u" (f64.const -1.5)) (f64.const 0x1p+64))
(assert_return (invoke "f64.i64.no_fold_trunc_u_convert_u" (f64.const 1.5)) (f64.const 1.0))
(assert_return (invoke "f64.i64.no_fold_trunc_u_convert_u" (f64.const -0.5)) (f64.const 0.0))

;; Test that dividing by a loop-invariant constant isn't optimized to be a
;; multiplication by a reciprocal, which would be particularly tempting since
;; the reciprocal computation could be hoisted.

(module
  (memory 1 1)
  (func (export "init") (param $i i32) (param $x f32) (f32.store (get_local $i) (get_local $x)))

  (func (export "run") (param $n i32) (param $z f32)
    (local $i i32)
    (block $exit
      (loop $cont
        (f32.store
          (get_local $i)
          (f32.div (f32.load (get_local $i)) (get_local $z))
        )
        (set_local $i (i32.add (get_local $i) (i32.const 4)))
        (br_if $cont (i32.lt_u (get_local $i) (get_local $n)))
      )
    )
  )

  (func (export "check") (param $i i32) (result f32) (f32.load (get_local $i)))
)

(invoke "init" (i32.const  0) (f32.const 15.1))
(invoke "init" (i32.const  4) (f32.const 15.2))
(invoke "init" (i32.const  8) (f32.const 15.3))
(invoke "init" (i32.const 12) (f32.const 15.4))
(assert_return (invoke "check" (i32.const  0)) (f32.const 15.1))
(assert_return (invoke "check" (i32.const  4)) (f32.const 15.2))
(assert_return (invoke "check" (i32.const  8)) (f32.const 15.3))
(assert_return (invoke "check" (i32.const 12)) (f32.const 15.4))
(invoke "run" (i32.const 16) (f32.const 3.0))
(assert_return (invoke "check" (i32.const  0)) (f32.const 0x1.422222p+2))
(assert_return (invoke "check" (i32.const  4)) (f32.const 0x1.444444p+2))
(assert_return (invoke "check" (i32.const  8)) (f32.const 0x1.466666p+2))
(assert_return (invoke "check" (i32.const 12)) (f32.const 0x1.488888p+2))

(module
  (memory 1 1)
  (func (export "init") (param $i i32) (param $x f64) (f64.store (get_local $i) (get_local $x)))

  (func (export "run") (param $n i32) (param $z f64)
    (local $i i32)
    (block $exit
      (loop $cont
        (f64.store
          (get_local $i)
          (f64.div (f64.load (get_local $i)) (get_local $z))
        )
        (set_local $i (i32.add (get_local $i) (i32.const 8)))
        (br_if $cont (i32.lt_u (get_local $i) (get_local $n)))
      )
    )
  )

  (func (export "check") (param $i i32) (result f64) (f64.load (get_local $i)))
)

(invoke "init" (i32.const  0) (f64.const 15.1))
(invoke "init" (i32.const  8) (f64.const 15.2))
(invoke "init" (i32.const 16) (f64.const 15.3))
(invoke "init" (i32.const 24) (f64.const 15.4))
(assert_return (invoke "check" (i32.const  0)) (f64.const 15.1))
(assert_return (invoke "check" (i32.const  8)) (f64.const 15.2))
(assert_return (invoke "check" (i32.const 16)) (f64.const 15.3))
(assert_return (invoke "check" (i32.const 24)) (f64.const 15.4))
(invoke "run" (i32.const 32) (f64.const 3.0))
(assert_return (invoke "check" (i32.const 0)) (f64.const 0x1.4222222222222p+2))
(assert_return (invoke "check" (i32.const 8)) (f64.const 0x1.4444444444444p+2))
(assert_return (invoke "check" (i32.const 16)) (f64.const 0x1.4666666666667p+2))
(assert_return (invoke "check" (i32.const 24)) (f64.const 0x1.4888888888889p+2))

;; Test that ult/ugt/etc. aren't folded to olt/ogt/etc.

(module
  (func (export "f32.ult") (param $x f32) (param $y f32) (result i32) (i32.eqz (f32.ge (get_local $x) (get_local $y))))
  (func (export "f32.ule") (param $x f32) (param $y f32) (result i32) (i32.eqz (f32.gt (get_local $x) (get_local $y))))
  (func (export "f32.ugt") (param $x f32) (param $y f32) (result i32) (i32.eqz (f32.le (get_local $x) (get_local $y))))
  (func (export "f32.uge") (param $x f32) (param $y f32) (result i32) (i32.eqz (f32.lt (get_local $x) (get_local $y))))

  (func (export "f64.ult") (param $x f64) (param $y f64) (result i32) (i32.eqz (f64.ge (get_local $x) (get_local $y))))
  (func (export "f64.ule") (param $x f64) (param $y f64) (result i32) (i32.eqz (f64.gt (get_local $x) (get_local $y))))
  (func (export "f64.ugt") (param $x f64) (param $y f64) (result i32) (i32.eqz (f64.le (get_local $x) (get_local $y))))
  (func (export "f64.uge") (param $x f64) (param $y f64) (result i32) (i32.eqz (f64.lt (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.ult" (f32.const 3.0) (f32.const 2.0)) (i32.const 0))
(assert_return (invoke "f32.ult" (f32.const 2.0) (f32.const 2.0)) (i32.const 0))
(assert_return (invoke "f32.ult" (f32.const 2.0) (f32.const 3.0)) (i32.const 1))
(assert_return (invoke "f32.ult" (f32.const 2.0) (f32.const nan)) (i32.const 1))
(assert_return (invoke "f32.ule" (f32.const 3.0) (f32.const 2.0)) (i32.const 0))
(assert_return (invoke "f32.ule" (f32.const 2.0) (f32.const 2.0)) (i32.const 1))
(assert_return (invoke "f32.ule" (f32.const 2.0) (f32.const 3.0)) (i32.const 1))
(assert_return (invoke "f32.ule" (f32.const 2.0) (f32.const nan)) (i32.const 1))
(assert_return (invoke "f32.ugt" (f32.const 3.0) (f32.const 2.0)) (i32.const 1))
(assert_return (invoke "f32.ugt" (f32.const 2.0) (f32.const 2.0)) (i32.const 0))
(assert_return (invoke "f32.ugt" (f32.const 2.0) (f32.const 3.0)) (i32.const 0))
(assert_return (invoke "f32.ugt" (f32.const 2.0) (f32.const nan)) (i32.const 1))
(assert_return (invoke "f32.uge" (f32.const 3.0) (f32.const 2.0)) (i32.const 1))
(assert_return (invoke "f32.uge" (f32.const 2.0) (f32.const 2.0)) (i32.const 1))
(assert_return (invoke "f32.uge" (f32.const 2.0) (f32.const 3.0)) (i32.const 0))
(assert_return (invoke "f32.uge" (f32.const 2.0) (f32.const nan)) (i32.const 1))
(assert_return (invoke "f64.ult" (f64.const 3.0) (f64.const 2.0)) (i32.const 0))
(assert_return (invoke "f64.ult" (f64.const 2.0) (f64.const 2.0)) (i32.const 0))
(assert_return (invoke "f64.ult" (f64.const 2.0) (f64.const 3.0)) (i32.const 1))
(assert_return (invoke "f64.ult" (f64.const 2.0) (f64.const nan)) (i32.const 1))
(assert_return (invoke "f64.ule" (f64.const 3.0) (f64.const 2.0)) (i32.const 0))
(assert_return (invoke "f64.ule" (f64.const 2.0) (f64.const 2.0)) (i32.const 1))
(assert_return (invoke "f64.ule" (f64.const 2.0) (f64.const 3.0)) (i32.const 1))
(assert_return (invoke "f64.ule" (f64.const 2.0) (f64.const nan)) (i32.const 1))
(assert_return (invoke "f64.ugt" (f64.const 3.0) (f64.const 2.0)) (i32.const 1))
(assert_return (invoke "f64.ugt" (f64.const 2.0) (f64.const 2.0)) (i32.const 0))
(assert_return (invoke "f64.ugt" (f64.const 2.0) (f64.const 3.0)) (i32.const 0))
(assert_return (invoke "f64.ugt" (f64.const 2.0) (f64.const nan)) (i32.const 1))
(assert_return (invoke "f64.uge" (f64.const 3.0) (f64.const 2.0)) (i32.const 1))
(assert_return (invoke "f64.uge" (f64.const 2.0) (f64.const 2.0)) (i32.const 1))
(assert_return (invoke "f64.uge" (f64.const 2.0) (f64.const 3.0)) (i32.const 0))
(assert_return (invoke "f64.uge" (f64.const 2.0) (f64.const nan)) (i32.const 1))

;; Test that x<y?x:y, etc. using select aren't folded to min, etc.

(module
  (func (export "f32.no_fold_lt_select") (param $x f32) (param $y f32) (result f32) (select (get_local $x) (get_local $y) (f32.lt (get_local $x) (get_local $y))))
  (func (export "f32.no_fold_le_select") (param $x f32) (param $y f32) (result f32) (select (get_local $x) (get_local $y) (f32.le (get_local $x) (get_local $y))))
  (func (export "f32.no_fold_gt_select") (param $x f32) (param $y f32) (result f32) (select (get_local $x) (get_local $y) (f32.gt (get_local $x) (get_local $y))))
  (func (export "f32.no_fold_ge_select") (param $x f32) (param $y f32) (result f32) (select (get_local $x) (get_local $y) (f32.ge (get_local $x) (get_local $y))))

  (func (export "f64.no_fold_lt_select") (param $x f64) (param $y f64) (result f64) (select (get_local $x) (get_local $y) (f64.lt (get_local $x) (get_local $y))))
  (func (export "f64.no_fold_le_select") (param $x f64) (param $y f64) (result f64) (select (get_local $x) (get_local $y) (f64.le (get_local $x) (get_local $y))))
  (func (export "f64.no_fold_gt_select") (param $x f64) (param $y f64) (result f64) (select (get_local $x) (get_local $y) (f64.gt (get_local $x) (get_local $y))))
  (func (export "f64.no_fold_ge_select") (param $x f64) (param $y f64) (result f64) (select (get_local $x) (get_local $y) (f64.ge (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.no_fold_lt_select" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_lt_select" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_lt_select" (f32.const 0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_lt_select" (f32.const -0.0) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_le_select" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_le_select" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_le_select" (f32.const 0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_le_select" (f32.const -0.0) (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_gt_select" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_gt_select" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_gt_select" (f32.const 0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_gt_select" (f32.const -0.0) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_select" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_ge_select" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_select" (f32.const 0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_select" (f32.const -0.0) (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f64.no_fold_lt_select" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_lt_select" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_lt_select" (f64.const 0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_lt_select" (f64.const -0.0) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_le_select" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_le_select" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_le_select" (f64.const 0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_le_select" (f64.const -0.0) (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_gt_select" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_gt_select" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_gt_select" (f64.const 0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_gt_select" (f64.const -0.0) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_select" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_ge_select" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_select" (f64.const 0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_select" (f64.const -0.0) (f64.const 0.0)) (f64.const -0.0))

;; Test that x<y?x:y, etc. using if and else aren't folded to min, etc.

(module
  (func (export "f32.no_fold_lt_if") (param $x f32) (param $y f32) (result f32)
    (if (result f32) (f32.lt (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
  (func (export "f32.no_fold_le_if") (param $x f32) (param $y f32) (result f32)
    (if (result f32) (f32.le (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
  (func (export "f32.no_fold_gt_if") (param $x f32) (param $y f32) (result f32)
    (if (result f32) (f32.gt (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
  (func (export "f32.no_fold_ge_if") (param $x f32) (param $y f32) (result f32)
    (if (result f32) (f32.ge (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )

  (func (export "f64.no_fold_lt_if") (param $x f64) (param $y f64) (result f64)
    (if (result f64) (f64.lt (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
  (func (export "f64.no_fold_le_if") (param $x f64) (param $y f64) (result f64)
    (if (result f64) (f64.le (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
  (func (export "f64.no_fold_gt_if") (param $x f64) (param $y f64) (result f64)
    (if (result f64) (f64.gt (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
  (func (export "f64.no_fold_ge_if") (param $x f64) (param $y f64) (result f64)
    (if (result f64) (f64.ge (get_local $x) (get_local $y))
      (then (get_local $x)) (else (get_local $y))
    )
  )
)

(assert_return (invoke "f32.no_fold_lt_if" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_lt_if" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_lt_if" (f32.const 0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_lt_if" (f32.const -0.0) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_le_if" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_le_if" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_le_if" (f32.const 0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_le_if" (f32.const -0.0) (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_gt_if" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_gt_if" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_gt_if" (f32.const 0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_gt_if" (f32.const -0.0) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_if" (f32.const 0.0) (f32.const nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_ge_if" (f32.const nan) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_if" (f32.const 0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_if" (f32.const -0.0) (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f64.no_fold_lt_if" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_lt_if" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_lt_if" (f64.const 0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_lt_if" (f64.const -0.0) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_le_if" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_le_if" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_le_if" (f64.const 0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_le_if" (f64.const -0.0) (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_gt_if" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_gt_if" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_gt_if" (f64.const 0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_gt_if" (f64.const -0.0) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_if" (f64.const 0.0) (f64.const nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_ge_if" (f64.const nan) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_if" (f64.const 0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_if" (f64.const -0.0) (f64.const 0.0)) (f64.const -0.0))

;; Test that x<0?-x:x, etc. using select aren't folded to abs.

(module
  (func (export "f32.no_fold_lt_select_to_abs") (param $x f32) (result f32) (select (f32.neg (get_local $x)) (get_local $x) (f32.lt (get_local $x) (f32.const 0.0))))
  (func (export "f32.no_fold_le_select_to_abs") (param $x f32) (result f32) (select (f32.neg (get_local $x)) (get_local $x) (f32.le (get_local $x) (f32.const -0.0))))
  (func (export "f32.no_fold_gt_select_to_abs") (param $x f32) (result f32) (select (get_local $x) (f32.neg (get_local $x)) (f32.gt (get_local $x) (f32.const -0.0))))
  (func (export "f32.no_fold_ge_select_to_abs") (param $x f32) (result f32) (select (get_local $x) (f32.neg (get_local $x)) (f32.ge (get_local $x) (f32.const 0.0))))

  (func (export "f64.no_fold_lt_select_to_abs") (param $x f64) (result f64) (select (f64.neg (get_local $x)) (get_local $x) (f64.lt (get_local $x) (f64.const 0.0))))
  (func (export "f64.no_fold_le_select_to_abs") (param $x f64) (result f64) (select (f64.neg (get_local $x)) (get_local $x) (f64.le (get_local $x) (f64.const -0.0))))
  (func (export "f64.no_fold_gt_select_to_abs") (param $x f64) (result f64) (select (get_local $x) (f64.neg (get_local $x)) (f64.gt (get_local $x) (f64.const -0.0))))
  (func (export "f64.no_fold_ge_select_to_abs") (param $x f64) (result f64) (select (get_local $x) (f64.neg (get_local $x)) (f64.ge (get_local $x) (f64.const 0.0))))
)

(assert_return (invoke "f32.no_fold_lt_select_to_abs" (f32.const nan:0x200000)) (f32.const nan:0x200000))
(assert_return (invoke "f32.no_fold_lt_select_to_abs" (f32.const -nan)) (f32.const -nan))
(assert_return (invoke "f32.no_fold_lt_select_to_abs" (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_lt_select_to_abs" (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_le_select_to_abs" (f32.const nan:0x200000)) (f32.const nan:0x200000))
(assert_return (invoke "f32.no_fold_le_select_to_abs" (f32.const -nan)) (f32.const -nan))
(assert_return (invoke "f32.no_fold_le_select_to_abs" (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_le_select_to_abs" (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_gt_select_to_abs" (f32.const nan:0x200000)) (f32.const -nan:0x200000))
(assert_return (invoke "f32.no_fold_gt_select_to_abs" (f32.const -nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_gt_select_to_abs" (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_gt_select_to_abs" (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_select_to_abs" (f32.const nan:0x200000)) (f32.const -nan:0x200000))
(assert_return (invoke "f32.no_fold_ge_select_to_abs" (f32.const -nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_ge_select_to_abs" (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_select_to_abs" (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f64.no_fold_lt_select_to_abs" (f64.const nan:0x4000000000000)) (f64.const nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_lt_select_to_abs" (f64.const -nan)) (f64.const -nan))
(assert_return (invoke "f64.no_fold_lt_select_to_abs" (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_lt_select_to_abs" (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_le_select_to_abs" (f64.const nan:0x4000000000000)) (f64.const nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_le_select_to_abs" (f64.const -nan)) (f64.const -nan))
(assert_return (invoke "f64.no_fold_le_select_to_abs" (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_le_select_to_abs" (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_gt_select_to_abs" (f64.const nan:0x4000000000000)) (f64.const -nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_gt_select_to_abs" (f64.const -nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_gt_select_to_abs" (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_gt_select_to_abs" (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_select_to_abs" (f64.const nan:0x4000000000000)) (f64.const -nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_ge_select_to_abs" (f64.const -nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_ge_select_to_abs" (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_select_to_abs" (f64.const -0.0)) (f64.const -0.0))

;; Test that x<0?-x:x, etc. using if aren't folded to abs.

(module
  (func (export "f32.no_fold_lt_if_to_abs") (param $x f32) (result f32)
    (if (result f32) (f32.lt (get_local $x) (f32.const 0.0))
      (then (f32.neg (get_local $x))) (else (get_local $x))
    )
  )
  (func (export "f32.no_fold_le_if_to_abs") (param $x f32) (result f32)
    (if (result f32) (f32.le (get_local $x) (f32.const -0.0))
      (then (f32.neg (get_local $x))) (else (get_local $x))
    )
  )
  (func (export "f32.no_fold_gt_if_to_abs") (param $x f32) (result f32)
    (if (result f32) (f32.gt (get_local $x) (f32.const -0.0))
      (then (get_local $x)) (else (f32.neg (get_local $x)))
    )
  )
  (func (export "f32.no_fold_ge_if_to_abs") (param $x f32) (result f32)
    (if (result f32) (f32.ge (get_local $x) (f32.const 0.0))
      (then (get_local $x)) (else (f32.neg (get_local $x)))
    )
  )

  (func (export "f64.no_fold_lt_if_to_abs") (param $x f64) (result f64)
    (if (result f64) (f64.lt (get_local $x) (f64.const 0.0))
      (then (f64.neg (get_local $x))) (else (get_local $x))
    )
  )
  (func (export "f64.no_fold_le_if_to_abs") (param $x f64) (result f64)
    (if (result f64) (f64.le (get_local $x) (f64.const -0.0))
      (then (f64.neg (get_local $x))) (else (get_local $x))
    )
  )
  (func (export "f64.no_fold_gt_if_to_abs") (param $x f64) (result f64)
    (if (result f64) (f64.gt (get_local $x) (f64.const -0.0))
      (then (get_local $x)) (else (f64.neg (get_local $x)))
    )
  )
  (func (export "f64.no_fold_ge_if_to_abs") (param $x f64) (result f64)
    (if (result f64) (f64.ge (get_local $x) (f64.const 0.0))
      (then (get_local $x)) (else (f64.neg (get_local $x)))
    )
  )
)

(assert_return (invoke "f32.no_fold_lt_if_to_abs" (f32.const nan:0x200000)) (f32.const nan:0x200000))
(assert_return (invoke "f32.no_fold_lt_if_to_abs" (f32.const -nan)) (f32.const -nan))
(assert_return (invoke "f32.no_fold_lt_if_to_abs" (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_lt_if_to_abs" (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_le_if_to_abs" (f32.const nan:0x200000)) (f32.const nan:0x200000))
(assert_return (invoke "f32.no_fold_le_if_to_abs" (f32.const -nan)) (f32.const -nan))
(assert_return (invoke "f32.no_fold_le_if_to_abs" (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_le_if_to_abs" (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_gt_if_to_abs" (f32.const nan:0x200000)) (f32.const -nan:0x200000))
(assert_return (invoke "f32.no_fold_gt_if_to_abs" (f32.const -nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_gt_if_to_abs" (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_gt_if_to_abs" (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_if_to_abs" (f32.const nan:0x200000)) (f32.const -nan:0x200000))
(assert_return (invoke "f32.no_fold_ge_if_to_abs" (f32.const -nan)) (f32.const nan))
(assert_return (invoke "f32.no_fold_ge_if_to_abs" (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_ge_if_to_abs" (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f64.no_fold_lt_if_to_abs" (f64.const nan:0x4000000000000)) (f64.const nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_lt_if_to_abs" (f64.const -nan)) (f64.const -nan))
(assert_return (invoke "f64.no_fold_lt_if_to_abs" (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_lt_if_to_abs" (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_le_if_to_abs" (f64.const nan:0x4000000000000)) (f64.const nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_le_if_to_abs" (f64.const -nan)) (f64.const -nan))
(assert_return (invoke "f64.no_fold_le_if_to_abs" (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_le_if_to_abs" (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_gt_if_to_abs" (f64.const nan:0x4000000000000)) (f64.const -nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_gt_if_to_abs" (f64.const -nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_gt_if_to_abs" (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_gt_if_to_abs" (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_if_to_abs" (f64.const nan:0x4000000000000)) (f64.const -nan:0x4000000000000))
(assert_return (invoke "f64.no_fold_ge_if_to_abs" (f64.const -nan)) (f64.const nan))
(assert_return (invoke "f64.no_fold_ge_if_to_abs" (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_ge_if_to_abs" (f64.const -0.0)) (f64.const -0.0))

;; Test for a historic spreadsheet bug.
;; https://support.microsoft.com/en-us/kb/78113

(module
  (func (export "f32.incorrect_correction") (result f32)
    (f32.sub (f32.sub (f32.add (f32.const 1.333) (f32.const 1.225)) (f32.const 1.333)) (f32.const 1.225))
  )
  (func (export "f64.incorrect_correction") (result f64)
    (f64.sub (f64.sub (f64.add (f64.const 1.333) (f64.const 1.225)) (f64.const 1.333)) (f64.const 1.225))
  )
)

(assert_return (invoke "f32.incorrect_correction") (f32.const 0x1p-23))
(assert_return (invoke "f64.incorrect_correction") (f64.const -0x1p-52))

;; Test for a historical calculator bug.
;; http://www.hpmuseum.org/cgi-sys/cgiwrap/hpmuseum/articles.cgi?read=735

(module
  (func (export "calculate") (result f32)
    (local $x f32)
    (local $r f32)
    (local $q f32)
    (local $z0 f32)
    (local $z1 f32)
    (set_local $x (f32.const 156.25))
    (set_local $r (f32.const 208.333333334))
    (set_local $q (f32.const 1.77951304201))
    (set_local $z0 (f32.div (f32.mul (f32.neg (get_local $r)) (get_local $x)) (f32.sub (f32.mul (get_local $x) (get_local $q)) (get_local $r))))
    (set_local $z1 (f32.div (f32.mul (f32.neg (get_local $r)) (get_local $x)) (f32.sub (f32.mul (get_local $x) (get_local $q)) (get_local $r))))
    (block (br_if 0 (f32.eq (get_local $z0) (get_local $z1))) (unreachable))
    (get_local $z1)
  )
)

(assert_return (invoke "calculate") (f32.const -0x1.d2ed46p+8))

(module
  (func (export "calculate") (result f64)
    (local $x f64)
    (local $r f64)
    (local $q f64)
    (local $z0 f64)
    (local $z1 f64)
    (set_local $x (f64.const 156.25))
    (set_local $r (f64.const 208.333333334))
    (set_local $q (f64.const 1.77951304201))
    (set_local $z0 (f64.div (f64.mul (f64.neg (get_local $r)) (get_local $x)) (f64.sub (f64.mul (get_local $x) (get_local $q)) (get_local $r))))
    (set_local $z1 (f64.div (f64.mul (f64.neg (get_local $r)) (get_local $x)) (f64.sub (f64.mul (get_local $x) (get_local $q)) (get_local $r))))
    (block (br_if 0 (f64.eq (get_local $z0) (get_local $z1))) (unreachable))
    (get_local $z1)
  )
)

(assert_return (invoke "calculate") (f64.const -0x1.d2ed4d0218c93p+8))

;; Test that 0 - (-0 - x) is not optimized to x.
;; https://llvm.org/bugs/show_bug.cgi?id=26746

(module
  (func (export "llvm_pr26746") (param $x f32) (result f32)
    (f32.sub (f32.const 0.0) (f32.sub (f32.const -0.0) (get_local $x)))
  )
)

(assert_return (invoke "llvm_pr26746" (f32.const -0.0)) (f32.const 0.0))

;; Test for improperly reassociating an addition and a conversion.
;; https://llvm.org/bugs/show_bug.cgi?id=27153

(module
  (func (export "llvm_pr27153") (param $x i32) (result f32)
    (f32.add (f32.convert_s/i32 (i32.and (get_local $x) (i32.const 268435455))) (f32.const -8388608.0))
  )
)

(assert_return (invoke "llvm_pr27153" (i32.const 33554434)) (f32.const 25165824.000000))

;; Test that (float)x + (float)y is not optimized to (float)(x + y) when unsafe.
;; https://llvm.org/bugs/show_bug.cgi?id=27036

(module
  (func (export "llvm_pr27036") (param $x i32) (param $y i32) (result f32)
    (f32.add (f32.convert_s/i32 (i32.or (get_local $x) (i32.const -25034805)))
             (f32.convert_s/i32 (i32.and (get_local $y) (i32.const 14942208))))
  )
)

(assert_return (invoke "llvm_pr27036" (i32.const -25034805) (i32.const 14942208)) (f32.const -0x1.340068p+23))

;; Test for bugs in old versions of historic IEEE 754 platforms as reported in:
;;
;; N. L. Schryer. 1981. A Test of a Computer's Floating-Point Arithmetic Unit.
;; Tech. Rep. Computer Science Technical Report 89, AT&T Bell Laboratories, Feb.
;;
;; specifically, the appendices describing IEEE systems with "The Past" sections
;; describing specific bugs. The 0 < 0 bug is omitted here due to being already
;; covered elsewhere.
(module
  (func (export "thepast0") (param $a f64) (param $b f64) (param $c f64) (param $d f64) (result f64)
    (f64.div (f64.mul (get_local $a) (get_local $b)) (f64.mul (get_local $c) (get_local $d)))
  )

  (func (export "thepast1") (param $a f64) (param $b f64) (param $c f64) (result f64)
    (f64.sub (f64.mul (get_local $a) (get_local $b)) (get_local $c))
  )

  (func (export "thepast2") (param $a f32) (param $b f32) (param $c f32) (result f32)
    (f32.mul (f32.mul (get_local $a) (get_local $b)) (get_local $c))
  )
)

(assert_return (invoke "thepast0" (f64.const 0x1p-1021) (f64.const 0x1.fffffffffffffp-1) (f64.const 0x1p1) (f64.const 0x1p-1)) (f64.const 0x1.fffffffffffffp-1022))
(assert_return (invoke "thepast1" (f64.const 0x1p-54) (f64.const 0x1.fffffffffffffp-1) (f64.const 0x1p-54)) (f64.const -0x1p-107))
(assert_return (invoke "thepast2" (f32.const 0x1p-125) (f32.const 0x1p-1) (f32.const 0x1p0)) (f32.const 0x1p-126))

;; Test for floating point tolerances observed in some GPUs.
;; https://community.amd.com/thread/145582

(module
  (func (export "inverse") (param $x f32) (result f32)
    (f32.div (f32.const 1.0) (get_local $x))
  )
)

(assert_return (invoke "inverse" (f32.const 96.0)) (f32.const 0x1.555556p-7))

;; Test for incorrect rounding on sqrt(4.0).
;; http://www.askvg.com/microsoft-windows-calculator-bug/

(module
  (func (export "f32_sqrt_minus_2") (param $x f32) (result f32)
    (f32.sub (f32.sqrt (get_local $x)) (f32.const 2.0))
  )

  (func (export "f64_sqrt_minus_2") (param $x f64) (result f64)
    (f64.sub (f64.sqrt (get_local $x)) (f64.const 2.0))
  )
)

(assert_return (invoke "f32_sqrt_minus_2" (f32.const 4.0)) (f32.const 0.0))
(assert_return (invoke "f64_sqrt_minus_2" (f64.const 4.0)) (f64.const 0.0))

;; Test that 1.0 / (1.0 / x) is not optimized to x.

(module
  (func (export "f32.no_fold_recip_recip") (param $x f32) (result f32)
    (f32.div (f32.const 1.0) (f32.div (f32.const 1.0) (get_local $x))))

  (func (export "f64.no_fold_recip_recip") (param $x f64) (result f64)
    (f64.div (f64.const 1.0) (f64.div (f64.const 1.0) (get_local $x))))
)

(assert_return (invoke "f32.no_fold_recip_recip" (f32.const -0x1.e8bf18p+65)) (f32.const -0x1.e8bf16p+65))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const 0x1.e24248p-77)) (f32.const 0x1.e24246p-77))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const 0x1.caf0e8p-64)) (f32.const 0x1.caf0eap-64))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const -0x1.e66982p+4)) (f32.const -0x1.e66984p+4))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const 0x1.f99916p+70)) (f32.const 0x1.f99914p+70))

(assert_return (invoke "f32.no_fold_recip_recip" (f32.const -0x0p+0)) (f32.const -0x0p+0))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const 0x0p+0)) (f32.const 0x0p+0))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const -inf)) (f32.const -inf))
(assert_return (invoke "f32.no_fold_recip_recip" (f32.const inf)) (f32.const inf))

(assert_return (invoke "f64.no_fold_recip_recip" (f64.const -0x1.d81248dda63dp+148)) (f64.const -0x1.d81248dda63d1p+148))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const -0x1.f4750312039e3p+66)) (f64.const -0x1.f4750312039e2p+66))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const 0x1.fa50630eec7f6p+166)) (f64.const 0x1.fa50630eec7f5p+166))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const 0x1.db0598617ba92p-686)) (f64.const 0x1.db0598617ba91p-686))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const 0x1.85f1638a0c82bp+902)) (f64.const 0x1.85f1638a0c82ap+902))

(assert_return (invoke "f64.no_fold_recip_recip" (f64.const -0x0p+0)) (f64.const -0x0p+0))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const 0x0p+0)) (f64.const 0x0p+0))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const -inf)) (f64.const -inf))
(assert_return (invoke "f64.no_fold_recip_recip" (f64.const inf)) (f64.const inf))

;; Test that (x+y) * (x-y) is not optimized to x*x - y*y.

(module
  (func (export "f32.no_algebraic_factoring") (param $x f32) (param $y f32) (result f32)
    (f32.mul (f32.add (get_local $x) (get_local $y))
             (f32.sub (get_local $x) (get_local $y))))

  (func (export "f64.no_algebraic_factoring") (param $x f64) (param $y f64) (result f64)
    (f64.mul (f64.add (get_local $x) (get_local $y))
             (f64.sub (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.no_algebraic_factoring" (f32.const -0x1.ef678ep-55) (f32.const 0x1.c160b8p-54)) (f32.const -0x1.129402p-107))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const -0x1.2d76bcp+24) (f32.const 0x1.f4089cp+24)) (f32.const -0x1.36d89ap+49))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const 0x1.7ca2b2p+45) (f32.const -0x1.08513cp+47)) (f32.const -0x1.db10dep+93))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const 0x1.7d5e3p+17) (f32.const -0x1.c783b4p+7)) (f32.const 0x1.1c10a6p+35))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const -0x1.daf96p+7) (f32.const -0x1.dac6bp+19)) (f32.const -0x1.b8422ep+39))

(assert_return (invoke "f64.no_algebraic_factoring" (f64.const 0x1.e17c0a02ac6b5p-476) (f64.const 0x1.e8f13f1fcdc14p-463)) (f64.const -0x1.d2ec518f62863p-925))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const 0x1.971b55a57e3a3p-377) (f64.const 0x1.edeb4233c1b27p-399)) (f64.const 0x1.43b3f69fb258bp-753))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const -0x1.c3b9dc02472fap-378) (f64.const -0x1.74e9faebaff14p-369)) (f64.const -0x1.0f9c07e8caa25p-737))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const -0x1.afaf4688ed019p+179) (f64.const 0x1.b07171cb49e94p+188)) (f64.const -0x1.6d3f2e2bebcf7p+377))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const 0x1.4377a98948f12p+114) (f64.const -0x1.500c05bd24c97p+90)) (f64.const 0x1.98b72dbf7bf72p+228))

;; Test that x*x - y*y is not optimized to (x+y) * (x-y).

(module
  (func (export "f32.no_algebraic_factoring") (param $x f32) (param $y f32) (result f32)
    (f32.sub (f32.mul (get_local $x) (get_local $x))
             (f32.mul (get_local $y) (get_local $y))))

  (func (export "f64.no_algebraic_factoring") (param $x f64) (param $y f64) (result f64)
    (f64.sub (f64.mul (get_local $x) (get_local $x))
             (f64.mul (get_local $y) (get_local $y))))
)

(assert_return (invoke "f32.no_algebraic_factoring" (f32.const 0x1.8e2c14p-46) (f32.const 0x1.bad59ap-39)) (f32.const -0x1.7efe5p-77))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const -0x1.7ef192p+41) (f32.const -0x1.db184ap+33)) (f32.const 0x1.1e6932p+83))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const 0x1.7eb458p-12) (f32.const -0x1.52c498p-13)) (f32.const 0x1.cc0bc6p-24))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const 0x1.2675c6p-44) (f32.const -0x1.edd31ap-46)) (f32.const 0x1.17294cp-88))
(assert_return (invoke "f32.no_algebraic_factoring" (f32.const 0x1.9a5f92p+51) (f32.const -0x1.2b0098p+52)) (f32.const -0x1.7189a6p+103))

(assert_return (invoke "f64.no_algebraic_factoring" (f64.const 0x1.749a128f18f69p+356) (f64.const -0x1.0bc97ee1354e1p+337)) (f64.const 0x1.0f28115518d74p+713))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const -0x1.2dab01b2215eap+309) (f64.const -0x1.e12b288bff2bdp+331)) (f64.const -0x1.c4319ad25d201p+663))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const 0x1.3ed898431e102p+42) (f64.const -0x1.c409183fa92e6p+39)) (f64.const 0x1.80a611103c71dp+84))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const -0x1.be663e4c0e4b2p+182) (f64.const -0x1.da85703760d25p+166)) (f64.const 0x1.853434f1a2ffep+365))
(assert_return (invoke "f64.no_algebraic_factoring" (f64.const -0x1.230e09952df1cp-236) (f64.const -0x1.fa2752adfadc9p-237)) (f64.const 0x1.42e43156bd1b8p-474))

;; Test that platforms where SIMD instructions flush subnormals don't implicitly
;; optimize using SIMD instructions.

(module
  (memory (data
    "\01\00\00\00\01\00\00\80\01\00\00\00\01\00\00\80"
    "\01\00\00\00\01\00\00\00\00\00\00\00\00\00\00\00"
    "\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00"
  ))

  (func (export "f32.simple_x4_sum")
    (param $i i32)
    (param $j i32)
    (param $k i32)
    (local $x0 f32) (local $x1 f32) (local $x2 f32) (local $x3 f32)
    (local $y0 f32) (local $y1 f32) (local $y2 f32) (local $y3 f32)
    (set_local $x0 (f32.load offset=0 (get_local $i)))
    (set_local $x1 (f32.load offset=4 (get_local $i)))
    (set_local $x2 (f32.load offset=8 (get_local $i)))
    (set_local $x3 (f32.load offset=12 (get_local $i)))
    (set_local $y0 (f32.load offset=0 (get_local $j)))
    (set_local $y1 (f32.load offset=4 (get_local $j)))
    (set_local $y2 (f32.load offset=8 (get_local $j)))
    (set_local $y3 (f32.load offset=12 (get_local $j)))
    (f32.store offset=0 (get_local $k) (f32.add (get_local $x0) (get_local $y0)))
    (f32.store offset=4 (get_local $k) (f32.add (get_local $x1) (get_local $y1)))
    (f32.store offset=8 (get_local $k) (f32.add (get_local $x2) (get_local $y2)))
    (f32.store offset=12 (get_local $k) (f32.add (get_local $x3) (get_local $y3)))
  )

  (func (export "f32.load")
    (param $k i32) (result f32)
    (f32.load (get_local $k))
  )
)

(assert_return (invoke "f32.simple_x4_sum" (i32.const 0) (i32.const 16) (i32.const 32)))
(assert_return (invoke "f32.load" (i32.const 32)) (f32.const 0x1p-148))
(assert_return (invoke "f32.load" (i32.const 36)) (f32.const 0x0p+0))
(assert_return (invoke "f32.load" (i32.const 40)) (f32.const 0x1p-149))
(assert_return (invoke "f32.load" (i32.const 44)) (f32.const -0x1p-149))

(module
  (memory (data
    "\01\00\00\00\00\00\00\00\01\00\00\00\00\00\00\80\01\00\00\00\00\00\00\00\01\00\00\00\00\00\00\80"
    "\01\00\00\00\00\00\00\00\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00"
    "\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00"
  ))

  (func (export "f64.simple_x4_sum")
    (param $i i32)
    (param $j i32)
    (param $k i32)
    (local $x0 f64) (local $x1 f64) (local $x2 f64) (local $x3 f64)
    (local $y0 f64) (local $y1 f64) (local $y2 f64) (local $y3 f64)
    (set_local $x0 (f64.load offset=0 (get_local $i)))
    (set_local $x1 (f64.load offset=8 (get_local $i)))
    (set_local $x2 (f64.load offset=16 (get_local $i)))
    (set_local $x3 (f64.load offset=24 (get_local $i)))
    (set_local $y0 (f64.load offset=0 (get_local $j)))
    (set_local $y1 (f64.load offset=8 (get_local $j)))
    (set_local $y2 (f64.load offset=16 (get_local $j)))
    (set_local $y3 (f64.load offset=24 (get_local $j)))
    (f64.store offset=0 (get_local $k) (f64.add (get_local $x0) (get_local $y0)))
    (f64.store offset=8 (get_local $k) (f64.add (get_local $x1) (get_local $y1)))
    (f64.store offset=16 (get_local $k) (f64.add (get_local $x2) (get_local $y2)))
    (f64.store offset=24 (get_local $k) (f64.add (get_local $x3) (get_local $y3)))
  )

  (func (export "f64.load")
    (param $k i32) (result f64)
    (f64.load (get_local $k))
  )
)

(assert_return (invoke "f64.simple_x4_sum" (i32.const 0) (i32.const 32) (i32.const 64)))
(assert_return (invoke "f64.load" (i32.const 64)) (f64.const 0x0.0000000000001p-1021))
(assert_return (invoke "f64.load" (i32.const 72)) (f64.const 0x0p+0))
(assert_return (invoke "f64.load" (i32.const 80)) (f64.const 0x0.0000000000001p-1022))
(assert_return (invoke "f64.load" (i32.const 88)) (f64.const -0x0.0000000000001p-1022))

;; Test that plain summation is not reassociated, and that Kahan summation
;; isn't optimized into plain summation.

(module
  (memory (data
    "\c4\c5\57\24\a5\84\c8\0b\6d\b8\4b\2e\f2\76\17\1c\ca\4a\56\1e\1b\6e\71\22"
    "\5d\17\1e\6e\bf\cd\14\5c\c7\21\55\51\39\9c\1f\b2\51\f0\a3\93\d7\c1\2c\ae"
    "\7e\a8\28\3a\01\21\f4\0a\58\93\f8\42\77\9f\83\39\6a\5f\ba\f7\0a\d8\51\6a"
    "\34\ca\ad\c6\34\0e\d8\26\dc\4c\33\1c\ed\29\90\a8\78\0f\d1\ce\76\31\23\83"
    "\b8\35\e8\f2\44\b0\d3\a1\fc\bb\32\e1\b0\ba\69\44\09\d6\d9\7d\ff\2e\c0\5a"
    "\36\14\33\14\3e\a9\fa\87\6d\8b\bc\ce\9d\a7\fd\c4\e9\85\3f\dd\d7\e1\18\a6"
    "\50\26\72\6e\3f\73\0f\f8\12\93\23\34\61\76\12\48\c0\9b\05\93\eb\ac\86\de"
    "\94\3e\55\e8\8c\e8\dd\e4\fc\95\47\be\56\03\21\20\4c\e6\bf\7b\f6\7f\d5\ba"
    "\73\1c\c1\14\8f\c4\27\96\b3\bd\33\ff\78\41\5f\c0\5a\ce\f6\67\6e\73\9a\17"
    "\66\70\03\f8\ce\27\a3\52\b2\9f\3b\bf\fb\ae\ed\d3\5a\f8\37\57\f0\f5\6e\ef"
    "\b1\4d\70\3d\54\a7\01\9a\85\08\48\91\f5\9d\0c\60\87\5b\d9\54\1e\51\6d\88"
    "\8e\08\8c\a5\71\3a\56\08\67\46\8f\8f\13\2a\2c\ec\2c\1f\b4\62\2b\6f\41\0a"
    "\c4\65\42\a2\31\6b\2c\7d\3e\bb\75\ac\86\97\30\d9\48\cd\9a\1f\56\c4\c6\e4"
    "\12\c0\9d\fb\ee\02\8c\ce\1c\f2\1e\a1\78\23\db\c4\1e\49\03\d3\71\cc\08\50"
    "\c5\d8\5c\ed\d5\b5\65\ac\b5\c9\21\d2\c9\29\76\de\f0\30\1a\5b\3c\f2\3b\db"
    "\3a\39\82\3a\16\08\6f\a8\f1\be\69\69\99\71\a6\05\d3\14\93\2a\16\f2\2f\11"
    "\c7\7e\20\bb\91\44\ee\f8\e4\01\53\c0\b9\7f\f0\bf\f0\03\9c\6d\b1\df\a2\44"
    "\01\6d\6b\71\2b\5c\b3\21\19\46\5e\8f\db\91\d3\7c\78\6b\b7\12\00\8f\eb\bd"
    "\8a\f5\d4\2e\c4\c1\1e\df\73\63\59\47\49\03\0a\b7\cf\24\cf\9c\0e\44\7a\9e"
    "\14\fb\42\bf\9d\39\30\9e\a0\ab\2f\d1\ae\9e\6a\83\43\e3\55\7d\85\bf\63\8a"
    "\f8\96\10\1f\fe\6d\e7\22\1b\e1\69\46\8a\44\c8\c8\f9\0c\2b\19\07\a5\02\3e"
    "\f2\30\10\9a\85\8a\5f\ef\81\45\a0\77\b1\03\10\73\4b\ae\98\9d\47\bf\9a\2d"
    "\3a\d5\0f\03\66\e3\3d\53\d9\40\ce\1f\6f\32\2f\21\2b\23\21\6c\62\d4\a7\3e"
    "\a8\ce\28\31\2d\00\3d\67\5e\af\a0\cf\2e\d2\b9\6b\84\eb\69\08\3c\62\36\be"
    "\12\fd\36\7f\88\3e\ad\bc\0b\c0\41\c4\50\b6\e3\50\31\e8\ce\e2\96\65\55\9c"
    "\16\46\e6\b0\2d\3a\e8\81\05\b0\bf\34\f7\bc\10\1c\fb\cc\3c\f1\85\97\42\9f"
    "\eb\14\8d\3c\bf\d7\17\88\49\9d\8b\2b\b2\3a\83\d1\4f\04\9e\a1\0f\ad\08\9d"
    "\54\af\d1\82\c3\ec\32\2f\02\8f\05\21\2d\a2\b7\e4\f4\6f\2e\81\2b\0b\9c\fc"
    "\cb\fe\74\02\f9\db\f4\f3\ea\00\a8\ec\d1\99\74\26\dd\d6\34\d5\25\b1\46\dd"
    "\9c\aa\71\f5\60\b0\88\c8\e0\0b\59\5a\25\4f\29\66\f9\e3\2e\fe\e9\da\e5\18"
    "\4f\27\62\f4\ce\a4\21\95\74\c7\57\64\27\9a\4c\fd\54\7d\61\ce\c3\ac\87\46"
    "\9c\fa\ff\09\ca\79\97\67\24\74\ca\d4\21\83\26\25\19\12\37\64\19\e5\65\e0"
    "\74\75\8e\dd\c8\ef\74\c7\d8\21\2b\79\04\51\46\65\60\03\5d\fa\d8\f4\65\a4"
    "\9e\5d\23\da\d7\8a\92\80\a4\de\78\3c\f1\57\42\6d\cd\c9\2f\d5\a4\9e\ab\40"
    "\f4\cb\1b\d7\a3\ca\fc\eb\a7\01\b2\9a\69\4e\46\9b\18\4e\dd\79\a7\aa\a6\52"
    "\39\1e\ef\30\cc\9b\bd\5b\ee\4c\21\6d\30\00\72\b0\46\5f\08\cf\c5\b9\e0\3e"
    "\c2\b3\0c\dc\8e\64\de\19\42\79\cf\43\ea\43\5d\8e\88\f7\ab\15\dc\3f\c8\67"
    "\20\db\b8\64\b1\47\1f\de\f2\cb\3f\59\9f\d8\46\90\dc\ae\2f\22\f9\e2\31\89"
    "\d9\9c\1c\4c\d3\a9\4a\57\84\9c\9f\ea\2c\3c\ae\3c\c3\1e\8b\e5\4e\17\01\25"
    "\db\34\46\5f\15\ea\05\0c\7c\d9\45\8c\19\d0\73\8a\96\16\dd\44\f9\05\b7\5b"
    "\71\b0\e6\21\36\5f\75\89\91\73\75\ab\7d\ae\d3\73\ec\37\c6\ea\55\75\ef\ea"
    "\ab\8b\7b\11\dc\6d\1a\b2\6a\c4\25\cf\aa\e3\9f\49\49\89\cb\37\9b\0a\a7\01"
    "\60\70\dc\b7\c8\83\e1\42\f5\be\ad\62\94\ad\8d\a1"
  ))

  (func (export "f32.kahan_sum") (param $p i32) (param $n i32) (result f32)
    (local $sum f32)
    (local $c f32)
    (local $t f32)
    (block $exit
      (loop $top
        (set_local $t
          (f32.sub
            (f32.sub
              (tee_local $sum
                (f32.add
                  (get_local $c)
                  (tee_local $t
                    (f32.sub (f32.load (get_local $p)) (get_local $t))
                  )
                )
              )
              (get_local $c)
            )
            (get_local $t)
          )
        )
        (set_local $p (i32.add (get_local $p) (i32.const 4)))
        (set_local $c (get_local $sum))
        (br_if $top (tee_local $n (i32.add (get_local $n) (i32.const -1))))
      )
    )
    (get_local $sum)
  )

  (func (export "f32.plain_sum") (param $p i32) (param $n i32) (result f32)
    (local $sum f32)
    (block $exit
      (loop $top
        (set_local $sum (f32.add (get_local $sum) (f32.load (get_local $p))))
        (set_local $p (i32.add (get_local $p) (i32.const 4)))
        (set_local $n (i32.add (get_local $n) (i32.const -1)))
        (br_if $top (get_local $n))
      )
    )
    (get_local $sum)
  )
)

(assert_return (invoke "f32.kahan_sum" (i32.const 0) (i32.const 256)) (f32.const -0x1.101a1ap+104))
(assert_return (invoke "f32.plain_sum" (i32.const 0) (i32.const 256)) (f32.const -0x1.a0343ap+103))

(module
  (memory (data "\13\05\84\42\5d\a2\2c\c6\43\db\55\a9\cd\da\55\e3\73\fc\58\d6\ba\d5\00\fd\83\35\42\88\8b\13\5d\38\4a\47\0d\72\73\a1\1a\ef\c4\45\17\57\d8\c9\46\e0\8d\6c\e1\37\70\c8\83\5b\55\5e\5a\2d\73\1e\56\c8\e1\6d\69\14\78\0a\8a\5a\64\3a\09\c7\a8\87\c5\f0\d3\5d\e6\03\fc\93\be\26\ca\d6\a9\91\60\bd\b0\ed\ae\f7\30\7e\92\3a\6f\a7\59\8e\aa\7d\bf\67\58\2a\54\f8\4e\fe\ed\35\58\a6\51\bf\42\e5\4b\66\27\24\6d\7f\42\2d\28\92\18\ec\08\ae\e7\55\da\b1\a6\65\a5\72\50\47\1b\b8\a9\54\d7\a6\06\5b\0f\42\58\83\8a\17\82\c6\10\43\a0\c0\2e\6d\bc\5a\85\53\72\7f\ad\44\bc\30\3c\55\b2\24\9a\74\3a\9e\e1\d8\0f\70\fc\a9\3a\cd\93\4b\ec\e3\7e\dd\5d\27\cd\f8\a0\9d\1c\11\c0\57\2e\fd\c8\13\32\cc\3a\1a\7d\a3\41\55\ed\c3\82\49\2a\04\1e\ef\73\b9\2e\2e\e3\5f\f4\df\e6\b2\33\0c\39\3f\6f\44\6a\03\c1\42\b9\fa\b1\c8\ed\a5\58\99\7f\ed\b4\72\9e\79\eb\fb\43\82\45\aa\bb\95\d2\ff\28\9e\f6\a1\ad\95\d6\55\95\0d\6f\60\11\c7\78\3e\49\f2\7e\48\f4\a2\71\d0\13\8e\b3\de\99\52\e3\45\74\ea\76\0e\1b\2a\c8\ee\14\01\c4\50\5b\36\3c\ef\ba\72\a2\a6\08\f8\7b\36\9d\f9\ef\0b\c7\56\2d\5c\f0\9d\5d\de\fc\b8\ad\0f\64\0e\97\15\32\26\c2\31\e6\05\1e\ef\cb\17\1b\6d\15\0b\74\5d\d3\2e\f8\6b\86\b4\ba\73\52\53\99\a9\76\20\45\c9\40\80\6b\14\ed\a1\fa\80\46\e6\26\d2\e6\98\c4\57\bf\c4\1c\a4\90\7a\36\94\14\ba\15\89\6e\e6\9c\37\8c\f4\de\12\22\5d\a1\79\50\67\0d\3d\7a\e9\d4\aa\2e\7f\2a\7a\30\3d\ea\5d\12\48\fe\e1\18\cd\a4\57\a2\87\3e\b6\9a\8b\db\da\9d\78\9c\cf\8d\b1\4f\90\b4\34\e0\9d\f6\ca\fe\4c\3b\78\6d\0a\5c\18\9f\61\b9\dd\b4\e0\0f\76\e0\1b\69\0d\5e\58\73\70\5e\0e\2d\a1\7d\ff\20\eb\91\34\92\ac\38\72\2a\1f\8e\71\2e\6a\f1\af\c7\27\70\d9\c4\57\f7\d2\3c\1d\b8\f0\f0\64\cf\dc\ae\be\a3\cc\3e\22\7d\4e\69\21\63\17\ed\03\02\54\9a\0f\50\4e\13\5a\35\a1\22\a4\df\86\c2\74\79\16\b8\69\69\a0\52\5d\11\64\bd\5b\93\fc\69\a0\f4\13\d0\81\51\dd\fa\0c\15\c3\7a\c9\62\7a\a9\1d\c9\e6\5a\b3\5b\97\02\3c\64\22\12\3c\22\90\64\2d\30\54\4c\b4\a1\22\09\57\22\5e\8e\38\2b\02\a8\ae\f6\be\0d\2b\f2\03\ad\fa\10\01\71\77\2a\30\02\95\f6\00\3e\d0\c4\8d\34\19\50\21\0a\bc\50\da\3c\30\d6\3a\31\94\8d\3a\fe\ef\14\57\9d\4b\93\00\96\24\0c\6f\fd\bc\23\76\02\6c\eb\52\72\80\11\7e\80\3a\13\12\38\1d\38\49\95\40\27\8a\44\7b\e8\dc\6d\8c\8c\8e\3c\b5\b3\18\0e\f6\08\1a\84\41\35\ff\8b\b8\93\40\ea\e1\51\1d\89\a5\8d\42\68\29\ea\2f\c1\7a\52\eb\90\5d\4d\d6\80\e3\d7\75\48\ce\ed\d3\01\1c\8d\5b\a5\94\0d\78\cf\f1\06\13\2f\98\02\a4\6d\2e\6c\f2\d5\74\29\89\4c\f9\03\f5\c7\18\ad\7a\f0\68\f8\5c\d6\59\87\6e\d6\3f\06\be\86\20\e3\41\91\22\f3\6e\8b\f0\68\1c\57\a7\fc\b0\7c\9e\99\0b\96\1a\89\5f\e6\0d\7c\08\51\a0\a2\67\9a\47\00\93\6b\f9\28\f0\68\db\62\f1\e0\65\2c\53\33\e0\a7\ca\11\42\30\f6\af\01\c1\65\3d\32\01\6f\ab\2e\be\d3\8b\be\14\c3\ff\ec\fb\f0\f9\c5\0c\05\6f\01\09\6b\e3\34\31\0c\1f\66\a6\42\bc\1a\87\49\16\16\8c\b0\90\0d\34\8c\0a\e1\09\5e\10\a4\6b\56\cc\f0\c9\bb\dc\b8\5c\ce\f6\cc\8d\75\7e\b3\07\88\04\2f\b4\5e\c9\e3\4a\23\73\19\62\6c\9a\03\76\44\86\9c\60\fc\db\72\8f\27\a0\dd\b3\c5\da\ff\f9\ec\6a\b1\7b\d3\cf\50\37\c9\7a\78\0c\e4\3a\b6\f5\e6\f4\98\6e\42\7d\35\73\8b\45\c0\56\97\cd\6d\ce\cf\ad\31\b3\c3\54\fa\ef\d5\c0\f4\6a\5f\54\e7\49\3e\33\0a\30\38\fd\d9\05\ff\a5\3f\57\46\14\b5\91\17\ca\6b\98\23\7a\65\b3\6c\02\b4\cc\79\5d\58\d8\b3\d5\94\ae\f4\6d\75\65\f7\92\bf\7e\47\4c\3c\ee\db\ac\f1\32\5d\fb\6f\41\1c\34\c8\83\4f\c2\58\01\be\05\3e\66\16\a6\04\6d\5d\4f\86\09\27\82\25\12\cd\3a\cd\ce\6b\bc\ca\ac\28\9b\ee\6a\25\86\9e\45\70\c6\d2\bd\3b\7d\42\e5\27\af\c7\1d\f4\81\c8\b3\76\8a\a8\36\a3\ae\2a\e6\18\e1\36\22\ad\f6\25\72\b0\39\8b\01\9a\22\7b\84\c3\2d\5f\72\a4\98\ac\15\70\e7\d4\18\e2\7d\d2\30\7c\33\08\cd\ca\c4\22\85\88\75\81\c6\4a\74\58\8d\e0\e8\ac\c5\ab\75\5a\f4\28\12\f0\18\45\52\f2\97\b2\93\41\6f\8d\7f\db\70\fb\a3\5d\1f\a7\8d\98\20\2b\22\9f\3a\01\b5\8b\1b\d2\cb\14\03\0e\14\14\d2\19\5a\1f\ce\5e\cd\81\79\15\01\ca\de\73\74\8c\56\20\9f\77\2d\25\16\f6\61\51\1d\a4\8e\9b\98\a5\c6\ec\a8\45\57\82\59\78\0d\90\b4\df\51\b0\c3\82\94\cc\b3\53\09\15\6d\96\6c\3a\40\47\b7\4a\7a\05\2f\a1\1e\8c\9d\a0\20\88\fb\52\b7\9f\f3\f3\bb\5f\e7\8a\61\a7\21\b1\ac\fa\09\aa\a4\6c\bc\24\80\ba\2a\e9\65\ff\70\ff\cc\fa\65\87\76\f3\c5\15\ce\cb\e8\42\31\00\0c\91\57\d9\e0\9d\35\54\24\ad\a4\d8\f9\08\67\63\c8\cf\81\dd\90\a2\d7\c4\07\4a\e6\10\6f\67\e7\27\d4\23\59\18\f2\a8\9d\5f\d8\94\30\aa\54\86\4f\87\9d\82\b5\26\ca\a6\96\bf\cf\55\f9\9d\37\01\19\48\43\c5\94\6c\f3\74\97\58\4c\3c\9d\08\e8\04\c2\58\30\76\e1\a0\f8\ea\e9\c5\ae\cf\78\9e\a9\0c\ac\b3\44\42\e0\bc\5d\1b\9c\49\58\4a\1c\19\49\c1\3a\ea\f5\eb\3b\81\a9\4b\70\0c\cc\9e\1a\d3\2f\b7\52\2f\20\3b\eb\64\51\1d\a0\2d\b2\3e\be\13\85\48\92\32\2e\db\5c\a1\e7\8c\45\91\35\01\0a\93\c2\eb\09\ce\f3\d2\22\24\d0\8c\cc\1d\9d\38\c8\4d\e3\82\cc\64\15\06\2d\e7\01\2f\ab\bb\b5\04\4c\92\1c\7a\d6\3f\e8\5f\31\15\0c\dc\e4\31\b4\c4\25\3e\2a\aa\00\9e\c8\e5\21\7a\7f\29\f1\c0\af\1d\5e\e8\63\39\ad\f8\7e\6c\c8\c5\7f\c2\a8\97\27\0a\d9\f4\21\6a\ea\03\09\fb\f7\96\3b\83\79\5f\7c\4b\30\9f\56\35\de\b4\73\d4\95\f0\14\c3\74\2f\0d\a3\1d\4e\8d\31\24\b3\1a\84\85\62\5a\7b\3c\14\39\17\e6\6d\eb\37\c2\00\58\5b\0b\e3\3c\8a\62\e1\f8\35\4b\56\e2\87\60\8b\be\a7\38\91\77\54\a9\5a\24\25\90\9f\a5\42\77\f3\5c\39\df\ff\74\07\76\a1\cd\1f\62\0b\81\81\68\af\05\c1\c0\7f\26\ee\c0\91\a3\6a\7d\29\61\45\27\e5\57\88\dc\0d\97\04\1a\33\a9\44\8a\da\02\10\45\3f\8e\55\a6\76\8c\4d\e3\f1\89\83\c8\d0\f8\9b\50\77\9f\47\df\4c\9c\66\0d\aa\18\b8\5f\4f\c4\01\ce\dc\84\ac\46\9e\69\e1\76\45\6b\61\89\e4\5d\94\bb\11\83\9f\78\d8\0a\d2\f5\7e\5d\43\ea\bc\10\f1\3a\c9\e2\64\fb\53\65\d0\c7\b4\a7\fb\d4\05\53\25\d0\cd\29\88\00\56\25\24\7d\5d\b4\f3\41\9f\e9\b5\f7\ae\64\2c\e3\c9\6d\d5\84\3a\72\12\b8\7a\d9\1b\09\e8\38\da\26\4f\04\ce\03\71\6e\8a\44\7b\5c\81\59\9c\d2\e4\c3\ba\59\a6\e5\28\a7\8f\9a\e4\d5\4e\b9\ca\7f\cb\75\b8\2b\43\3e\b3\15\46\b1\a5\bc\9d\9e\38\15\f1\bd\1b\21\aa\f1\82\00\95\fc\a7\77\47\39\a7\33\43\92\d7\52\40\4b\06\81\8a\a0\bd\f1\6b\99\84\42\5b\e2\3b\c5\5e\12\5c\28\4d\b6\0e\4e\c8\5c\e8\01\8a\c5\e7\e4\9d\42\ee\5d\9c\c4\eb\eb\68\09\27\92\95\9a\11\54\73\c4\12\80\fb\7d\fe\c5\08\60\7f\36\41\e0\10\ba\d6\2b\6c\f1\b4\17\fe\26\34\e3\4b\f8\a8\e3\91\be\4f\2a\fc\da\81\b8\e7\fe\d5\26\50\47\f3\1a\65\32\81\e0\05\b8\4f\32\31\26\00\4a\53\97\c2\c3\0e\2e\a1\26\54\ab\05\8e\56\2f\7d\af\22\84\68\a5\8b\97\f6\a4\fd\a8\cc\75\41\96\86\fd\27\3d\29\86\8d\7f\4c\d4\8e\73\41\f4\1e\e2\dd\58\27\97\ce\9c\94\cf\7a\04\2f\dc\ed"
  ))

  (func (export "f64.kahan_sum") (param $p i32) (param $n i32) (result f64)
    (local $sum f64)
    (local $c f64)
    (local $t f64)
    (block $exit
      (loop $top
        (set_local $t
          (f64.sub
            (f64.sub
              (tee_local $sum
                (f64.add
                  (get_local $c)
                  (tee_local $t
                    (f64.sub (f64.load (get_local $p)) (get_local $t))
                  )
                )
              )
              (get_local $c)
            )
            (get_local $t)
          )
        )
        (set_local $p (i32.add (get_local $p) (i32.const 8)))
        (set_local $c (get_local $sum))
        (br_if $top (tee_local $n (i32.add (get_local $n) (i32.const -1))))
      )
    )
    (get_local $sum)
  )

  (func (export "f64.plain_sum") (param $p i32) (param $n i32) (result f64)
    (local $sum f64)
    (block $exit
      (loop $top
        (set_local $sum (f64.add (get_local $sum) (f64.load (get_local $p))))
        (set_local $p (i32.add (get_local $p) (i32.const 8)))
        (set_local $n (i32.add (get_local $n) (i32.const -1)))
        (br_if $top (get_local $n))
      )
    )
    (get_local $sum)
  )
)

(assert_return (invoke "f64.kahan_sum" (i32.const 0) (i32.const 256)) (f64.const 0x1.dd7cb2a5ffc88p+998))
(assert_return (invoke "f64.plain_sum" (i32.const 0) (i32.const 256)) (f64.const 0x1.dd7cb2a63fc87p+998))

;; Test that -(x - y) is not folded to y - x.

(module
  (func (export "f32.no_fold_neg_sub") (param $x f32) (param $y f32) (result f32)
    (f32.neg (f32.sub (get_local $x) (get_local $y))))

  (func (export "f64.no_fold_neg_sub") (param $x f64) (param $y f64) (result f64)
    (f64.neg (f64.sub (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.no_fold_neg_sub" (f32.const -0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_neg_sub" (f32.const 0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_neg_sub" (f32.const -0.0) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_neg_sub" (f32.const 0.0) (f32.const 0.0)) (f32.const -0.0))

(assert_return (invoke "f64.no_fold_neg_sub" (f64.const -0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_neg_sub" (f64.const 0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_neg_sub" (f64.const -0.0) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_neg_sub" (f64.const 0.0) (f64.const 0.0)) (f64.const -0.0))

;; Test that -(x + y) is not folded to (-x + -y).

(module
  (func (export "f32.no_fold_neg_add") (param $x f32) (param $y f32) (result f32)
    (f32.neg (f32.add (get_local $x) (get_local $y))))

  (func (export "f64.no_fold_neg_add") (param $x f64) (param $y f64) (result f64)
    (f64.neg (f64.add (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.no_fold_neg_add" (f32.const -0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_neg_add" (f32.const 0.0) (f32.const -0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_neg_add" (f32.const -0.0) (f32.const 0.0)) (f32.const -0.0))
(assert_return (invoke "f32.no_fold_neg_add" (f32.const 0.0) (f32.const 0.0)) (f32.const -0.0))

(assert_return (invoke "f64.no_fold_neg_add" (f64.const -0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_neg_add" (f64.const 0.0) (f64.const -0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_neg_add" (f64.const -0.0) (f64.const 0.0)) (f64.const -0.0))
(assert_return (invoke "f64.no_fold_neg_add" (f64.const 0.0) (f64.const 0.0)) (f64.const -0.0))

;; Test that (-x + -y) is not folded to -(x + y).

(module
  (func (export "f32.no_fold_add_neg_neg") (param $x f32) (param $y f32) (result f32)
    (f32.add (f32.neg (get_local $x)) (f32.neg (get_local $y))))

  (func (export "f64.no_fold_add_neg_neg") (param $x f64) (param $y f64) (result f64)
    (f64.add (f64.neg (get_local $x)) (f64.neg (get_local $y))))
)

(assert_return (invoke "f32.no_fold_add_neg_neg" (f32.const -0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_add_neg_neg" (f32.const 0.0) (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_add_neg_neg" (f32.const -0.0) (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_add_neg_neg" (f32.const 0.0) (f32.const 0.0)) (f32.const -0.0))

(assert_return (invoke "f64.no_fold_add_neg_neg" (f64.const -0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_add_neg_neg" (f64.const 0.0) (f64.const -0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_add_neg_neg" (f64.const -0.0) (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_add_neg_neg" (f64.const 0.0) (f64.const 0.0)) (f64.const -0.0))

;; Test that -x + x is not folded to 0.0.

(module
  (func (export "f32.no_fold_add_neg") (param $x f32) (result f32)
    (f32.add (f32.neg (get_local $x)) (get_local $x)))

  (func (export "f64.no_fold_add_neg") (param $x f64) (result f64)
    (f64.add (f64.neg (get_local $x)) (get_local $x)))
)

(assert_return (invoke "f32.no_fold_add_neg" (f32.const 0.0)) (f32.const 0.0))
(assert_return (invoke "f32.no_fold_add_neg" (f32.const -0.0)) (f32.const 0.0))
(assert_return_canonical_nan (invoke "f32.no_fold_add_neg" (f32.const inf)))
(assert_return_canonical_nan (invoke "f32.no_fold_add_neg" (f32.const -inf)))

(assert_return (invoke "f64.no_fold_add_neg" (f64.const 0.0)) (f64.const 0.0))
(assert_return (invoke "f64.no_fold_add_neg" (f64.const -0.0)) (f64.const 0.0))
(assert_return_canonical_nan (invoke "f64.no_fold_add_neg" (f64.const inf)))
(assert_return_canonical_nan (invoke "f64.no_fold_add_neg" (f64.const -inf)))

;; Test that x+x+x+x+x+x is not folded to x * 6.

(module
  (func (export "f32.no_fold_6x_via_add") (param $x f32) (result f32)
    (f32.add (f32.add (f32.add (f32.add (f32.add
    (get_local $x)
    (get_local $x)) (get_local $x)) (get_local $x))
    (get_local $x)) (get_local $x)))

  (func (export "f64.no_fold_6x_via_add") (param $x f64) (result f64)
    (f64.add (f64.add (f64.add (f64.add (f64.add
    (get_local $x)
    (get_local $x)) (get_local $x)) (get_local $x))
    (get_local $x)) (get_local $x)))
)

(assert_return (invoke "f32.no_fold_6x_via_add" (f32.const -0x1.598a0cp+99)) (f32.const -0x1.03278ap+102))
(assert_return (invoke "f32.no_fold_6x_via_add" (f32.const -0x1.d3e7acp-77)) (f32.const -0x1.5eedc2p-74))
(assert_return (invoke "f32.no_fold_6x_via_add" (f32.const 0x1.00fa02p-77)) (f32.const 0x1.817702p-75))
(assert_return (invoke "f32.no_fold_6x_via_add" (f32.const -0x1.51f434p-31)) (f32.const -0x1.faee4cp-29))
(assert_return (invoke "f32.no_fold_6x_via_add" (f32.const -0x1.00328ap+80)) (f32.const -0x1.804bcep+82))

(assert_return (invoke "f64.no_fold_6x_via_add" (f64.const -0x1.310e15acaffe6p+68)) (f64.const -0x1.c995208307fdap+70))
(assert_return (invoke "f64.no_fold_6x_via_add" (f64.const -0x1.aad62c78fa9b4p-535)) (f64.const -0x1.4020a15abbf46p-532))
(assert_return (invoke "f64.no_fold_6x_via_add" (f64.const -0x1.f8fbfa94f6ab2p+271)) (f64.const -0x1.7abcfbefb9005p+274))
(assert_return (invoke "f64.no_fold_6x_via_add" (f64.const 0x1.756ccc2830a8ep+751)) (f64.const 0x1.1811991e247ebp+754))
(assert_return (invoke "f64.no_fold_6x_via_add" (f64.const -0x1.8fd1ab1d2402ap+234)) (f64.const -0x1.2bdd4055db01fp+237))

;; Test that (x/y)/z is not optimized to x/(y*z),
;; which is an "allowable alternative Form" in Fortran.

(module
  (func (export "f32.no_fold_div_div") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.div (f32.div (get_local $x) (get_local $y)) (get_local $z)))

  (func (export "f64.no_fold_div_div") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.div (f64.div (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f32.no_fold_div_div" (f32.const -0x1.f70228p+78) (f32.const -0x1.fbc612p-16) (f32.const -0x1.8c379p+10)) (f32.const -0x1.47b43cp+83))
(assert_return (invoke "f32.no_fold_div_div" (f32.const 0x1.d29d2ep-70) (f32.const 0x1.f3a17ep+110) (f32.const -0x1.64d41p-112)) (f32.const -0x0p+0))
(assert_return (invoke "f32.no_fold_div_div" (f32.const 0x1.867f98p+43) (f32.const 0x1.30acfcp-105) (f32.const 0x1.e210d8p+105)) (f32.const inf))
(assert_return (invoke "f32.no_fold_div_div" (f32.const -0x1.c4001ap-14) (f32.const -0x1.9beb6cp+124) (f32.const -0x1.74f34cp-43)) (f32.const -0x1.819874p-96))
(assert_return (invoke "f32.no_fold_div_div" (f32.const 0x1.db0e6ep+46) (f32.const 0x1.55eea2p+56) (f32.const -0x1.f3134p+124)) (f32.const -0x1.6cep-135))

(assert_return (invoke "f64.no_fold_div_div" (f64.const 0x1.b4dc8ec3c7777p+337) (f64.const 0x1.9f95ac2d1863p+584) (f64.const -0x1.d4318abba341ep-782)) (f64.const -0x1.2649159d87e02p+534))
(assert_return (invoke "f64.no_fold_div_div" (f64.const -0x1.ac53af5eb445fp+791) (f64.const 0x1.8549c0a4ceb13p-29) (f64.const 0x1.64e384003c801p+316)) (f64.const -0x1.9417cdccbae91p+503))
(assert_return (invoke "f64.no_fold_div_div" (f64.const -0x1.d2685afb27327p+2) (f64.const -0x1.abb1eeed3dbebp+880) (f64.const 0x1.a543e2e6968a3p+170)) (f64.const 0x0.0000002a69a5fp-1022))
(assert_return (invoke "f64.no_fold_div_div" (f64.const -0x1.47ddede78ad1cp+825) (f64.const 0x1.6d932d070a367p-821) (f64.const 0x1.79cf18cc64fp+961)) (f64.const -inf))
(assert_return (invoke "f64.no_fold_div_div" (f64.const -0x1.f73d4979a9379p-888) (f64.const 0x1.4d83b53e97788p-596) (f64.const -0x1.f8f86c9603b5bp-139)) (f64.const 0x1.87a7bd89c586cp-154))

;; Test that (x/y)*(z/w) is not optimized to (x*z)/(y*w), example from
;; http://perso.ens-lyon.fr/jean-michel.muller/Handbook.html
;; section 7.4.1: FORTRAN Floating Point in a Nutshell: Philosophy

(module
  (func (export "f32.no_fold_mul_divs") (param $x f32) (param $y f32) (param $z f32) (param $w f32) (result f32)
    (f32.mul (f32.div (get_local $x) (get_local $y)) (f32.div (get_local $z) (get_local $w))))

  (func (export "f64.no_fold_mul_divs") (param $x f64) (param $y f64) (param $z f64) (param $w f64) (result f64)
    (f64.mul (f64.div (get_local $x) (get_local $y)) (f64.div (get_local $z) (get_local $w))))
)

(assert_return (invoke "f32.no_fold_mul_divs" (f32.const -0x1.c483bep-109) (f32.const 0x1.ee1c3cp-92) (f32.const 0x1.800756p-88) (f32.const -0x1.95b972p+4)) (f32.const 0x1.bbd30cp-110))
(assert_return (invoke "f32.no_fold_mul_divs" (f32.const -0x1.0f4262p+102) (f32.const 0x1.248498p+25) (f32.const 0x1.f66a7cp-17) (f32.const 0x1.897fc8p-3)) (f32.const -0x1.2f1aa4p+63))
(assert_return (invoke "f32.no_fold_mul_divs" (f32.const -0x1.df5f22p+33) (f32.const -0x1.fcee3ep+39) (f32.const -0x1.9ea914p+29) (f32.const -0x1.2c4d3p+10)) (f32.const 0x1.4cf51cp+13))
(assert_return (invoke "f32.no_fold_mul_divs" (f32.const -0x1.f568bcp+109) (f32.const 0x1.d9963p-34) (f32.const 0x1.37a87ap-16) (f32.const 0x1.a1524ap+78)) (f32.const -inf))
(assert_return (invoke "f32.no_fold_mul_divs" (f32.const 0x1.3dd592p-53) (f32.const -0x1.332c22p-64) (f32.const 0x1.b01064p-91) (f32.const 0x1.92bb3ap-36)) (f32.const -0x1.1c2dbp-44))

(assert_return (invoke "f64.no_fold_mul_divs" (f64.const -0x1.363d6764f7b12p-819) (f64.const -0x1.ed5471f660b5fp-464) (f64.const -0x1.671b0a7f3a42p+547) (f64.const 0x1.0633be34ba1f2p+186)) (f64.const -0x1.b8fa2b76baeebp+5))
(assert_return (invoke "f64.no_fold_mul_divs" (f64.const -0x1.37880182e0fa8p+115) (f64.const 0x1.f842631576147p-920) (f64.const -0x1.999372231d156p+362) (f64.const -0x1.d5db481ab9554p+467)) (f64.const -inf))
(assert_return (invoke "f64.no_fold_mul_divs" (f64.const -0x1.9a747c8d4b541p+308) (f64.const -0x1.99092ad6bbdc8p+192) (f64.const -0x1.cb23755c20101p-140) (f64.const -0x1.de8716f6b0b6ap+732)) (f64.const 0x1.ecf584c8466a5p-757))
(assert_return (invoke "f64.no_fold_mul_divs" (f64.const -0x1.c424b2ece903dp+129) (f64.const -0x1.568ce281db37fp-347) (f64.const 0x1.53900b99fd3dp-957) (f64.const 0x1.5c33952254dadp+223)) (f64.const 0x0p+0))
(assert_return (invoke "f64.no_fold_mul_divs" (f64.const 0x1.a8ec2cecb32a9p-18) (f64.const 0x1.58acab0051851p-277) (f64.const 0x1.35e87c9077f7fp-620) (f64.const -0x1.925ee37ffb386p+352)) (f64.const -0x1.e6286970b31bfp-714))

;; Test that (x/z)+(y/z) is not optimized to (x+y)/z.

(module
  (func (export "f32.no_fold_add_divs") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.add (f32.div (get_local $x) (get_local $z)) (f32.div (get_local $y) (get_local $z))))

  (func (export "f64.no_fold_add_divs") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.add (f64.div (get_local $x) (get_local $z)) (f64.div (get_local $y) (get_local $z))))
)

(assert_return (invoke "f32.no_fold_add_divs" (f32.const 0x1.795e7p+8) (f32.const -0x1.48a5eep-5) (f32.const -0x1.9a244cp+126)) (f32.const -0x1.d709b6p-119))
(assert_return (invoke "f32.no_fold_add_divs" (f32.const -0x1.ae89e8p-63) (f32.const -0x1.e9903ep-49) (f32.const -0x1.370a8cp+47)) (f32.const 0x1.92f3f6p-96))
(assert_return (invoke "f32.no_fold_add_divs" (f32.const -0x1.626408p-46) (f32.const 0x1.2ee5b2p-64) (f32.const -0x1.ecefaap+48)) (f32.const 0x1.701864p-95))
(assert_return (invoke "f32.no_fold_add_divs" (f32.const -0x1.061d3p-101) (f32.const 0x1.383492p-98) (f32.const -0x1.1d92d2p+88)) (f32.const 0x0p+0))
(assert_return (invoke "f32.no_fold_add_divs" (f32.const 0x1.1ea39ep-10) (f32.const 0x1.a7fffep-3) (f32.const 0x1.6fc574p-123)) (f32.const 0x1.28b2dep+120))

(assert_return (invoke "f64.no_fold_add_divs" (f64.const -0x1.c5fcc3273b136p+430) (f64.const 0x1.892a09eed8f6fp+434) (f64.const 0x1.8258b71e64397p+911)) (f64.const 0x1.e36eb9706ad82p-478))
(assert_return (invoke "f64.no_fold_add_divs" (f64.const -0x1.2215d4061b5b3p+53) (f64.const 0x1.fb6184d97f27cp+5) (f64.const -0x1.f3bb59dacc0ebp-957)) (f64.const 0x1.2934eb0118be3p+1009))
(assert_return (invoke "f64.no_fold_add_divs" (f64.const -0x1.e7a4533741d8ep-967) (f64.const 0x1.a519bb7feb802p-976) (f64.const 0x1.1f8a43454e51ap+504)) (f64.const 0x0p+0))
(assert_return (invoke "f64.no_fold_add_divs" (f64.const 0x1.991c6cf93e2b4p+313) (f64.const -0x1.f2f7432698d11p+329) (f64.const 0x1.0d8c1b2453617p-126)) (f64.const -0x1.d9e1d84ddd1d4p+455))
(assert_return (invoke "f64.no_fold_add_divs" (f64.const -0x1.d436849dc1271p-728) (f64.const 0x1.19d1c1450e52dp-755) (f64.const 0x1.fa1be69ea06fep-70)) (f64.const -0x1.d9a9b1c2f5623p-659))

;; Test that sqrt(x*x) is not optimized to abs(x).

(module
  (func (export "f32.no_fold_sqrt_square") (param $x f32) (result f32)
    (f32.sqrt (f32.mul (get_local $x) (get_local $x))))

  (func (export "f64.no_fold_sqrt_square") (param $x f64) (result f64)
    (f64.sqrt (f64.mul (get_local $x) (get_local $x))))
)

(assert_return (invoke "f32.no_fold_sqrt_square" (f32.const -0x1.5cb316p-66)) (f32.const 0x1.5cb322p-66))
(assert_return (invoke "f32.no_fold_sqrt_square" (f32.const -0x1.b0f9e4p-73)) (f32.const 0x1.b211b2p-73))
(assert_return (invoke "f32.no_fold_sqrt_square" (f32.const -0x1.de417cp-71)) (f32.const 0x1.de65b8p-71))
(assert_return (invoke "f32.no_fold_sqrt_square" (f32.const 0x1.64c872p-86)) (f32.const 0x0p+0))
(assert_return (invoke "f32.no_fold_sqrt_square" (f32.const 0x1.e199e4p+108)) (f32.const inf))

(assert_return (invoke "f64.no_fold_sqrt_square" (f64.const 0x1.1759d657203fdp-529)) (f64.const 0x1.1759dd57545f3p-529))
(assert_return (invoke "f64.no_fold_sqrt_square" (f64.const -0x1.4c68de1c78d83p-514)) (f64.const 0x1.4c68de1c78d81p-514))
(assert_return (invoke "f64.no_fold_sqrt_square" (f64.const -0x1.214736edb6e1ep-521)) (f64.const 0x1.214736ed9cf8dp-521))
(assert_return (invoke "f64.no_fold_sqrt_square" (f64.const -0x1.0864b9f68457p-616)) (f64.const 0x0p+0))
(assert_return (invoke "f64.no_fold_sqrt_square" (f64.const 0x1.b2a9855995abap+856)) (f64.const inf))

;; Test that sqrt(x)*sqrt(y) is not optimized to sqrt(x*y).

(module
  (func (export "f32.no_fold_mul_sqrts") (param $x f32) (param $y f32) (result f32)
    (f32.mul (f32.sqrt (get_local $x)) (f32.sqrt (get_local $y))))

  (func (export "f64.no_fold_mul_sqrts") (param $x f64) (param $y f64) (result f64)
    (f64.mul (f64.sqrt (get_local $x)) (f64.sqrt (get_local $y))))
)

(assert_return_canonical_nan (invoke "f32.no_fold_mul_sqrts" (f32.const 0x1.dddda8p-125) (f32.const -0x1.25d22ap-83)))
(assert_return (invoke "f32.no_fold_mul_sqrts" (f32.const 0x1.418d14p-92) (f32.const 0x1.c6535cp-32)) (f32.const 0x1.7e373ap-62))
(assert_return (invoke "f32.no_fold_mul_sqrts" (f32.const 0x1.4de7ep-88) (f32.const 0x1.84ff18p+6)) (f32.const 0x1.686668p-41))
(assert_return (invoke "f32.no_fold_mul_sqrts" (f32.const 0x1.78091ep+101) (f32.const 0x1.81feb8p-9)) (f32.const 0x1.7cfb98p+46))
(assert_return (invoke "f32.no_fold_mul_sqrts" (f32.const 0x1.583ap-56) (f32.const 0x1.14ba2ap-9)) (f32.const 0x1.b47a8ep-33))

(assert_return_canonical_nan (invoke "f64.no_fold_mul_sqrts" (f64.const -0x1.d1144cc28cdbep-635) (f64.const -0x1.bf9bc373d3b6ap-8)))
(assert_return (invoke "f64.no_fold_mul_sqrts" (f64.const 0x1.5a7eb976bebc9p-643) (f64.const 0x1.f30cb8865a4cap-404)) (f64.const 0x1.260a1032d6e76p-523))
(assert_return (invoke "f64.no_fold_mul_sqrts" (f64.const 0x1.711a0c1707935p-89) (f64.const 0x1.6fb5de51a20d3p-913)) (f64.const 0x1.7067ca28e31ecp-501))
(assert_return (invoke "f64.no_fold_mul_sqrts" (f64.const 0x1.fb0bbea33b076p-363) (f64.const 0x1.d963b34894158p-573)) (f64.const 0x1.e9edc1fa624afp-468))
(assert_return (invoke "f64.no_fold_mul_sqrts" (f64.const 0x1.8676eab7a4d0dp+24) (f64.const 0x1.75a58231ba7a5p+513)) (f64.const 0x1.0e16aebe203b3p+269))

;; Test that sqrt(x)/sqrt(y) is not optimized to sqrt(x/y).

(module
  (func (export "f32.no_fold_div_sqrts") (param $x f32) (param $y f32) (result f32)
    (f32.div (f32.sqrt (get_local $x)) (f32.sqrt (get_local $y))))

  (func (export "f64.no_fold_div_sqrts") (param $x f64) (param $y f64) (result f64)
    (f64.div (f64.sqrt (get_local $x)) (f64.sqrt (get_local $y))))
)

(assert_return_canonical_nan (invoke "f32.no_fold_div_sqrts" (f32.const -0x1.bea9bap+25) (f32.const -0x1.db776ep-58)))
(assert_return (invoke "f32.no_fold_div_sqrts" (f32.const 0x1.b983b6p+32) (f32.const 0x1.901f1ep+27)) (f32.const 0x1.7c4df6p+2))
(assert_return (invoke "f32.no_fold_div_sqrts" (f32.const 0x1.d45e72p-120) (f32.const 0x1.ab49ccp+15)) (f32.const 0x1.7b0b04p-68))
(assert_return (invoke "f32.no_fold_div_sqrts" (f32.const 0x1.b2e444p+59) (f32.const 0x1.5b8b16p-30)) (f32.const 0x1.94fca8p+44))
(assert_return (invoke "f32.no_fold_div_sqrts" (f32.const 0x1.835aa6p-112) (f32.const 0x1.d17128p-103)) (f32.const 0x1.4a468p-5))

(assert_return_canonical_nan (invoke "f64.no_fold_div_sqrts" (f64.const -0x1.509fc16411167p-711) (f64.const -0x1.9c4255f5d6517p-187)))
(assert_return (invoke "f64.no_fold_div_sqrts" (f64.const 0x1.b6897bddac76p-587) (f64.const 0x1.104578b4c91f3p+541)) (f64.const 0x1.44e4f21f26cc9p-564))
(assert_return (invoke "f64.no_fold_div_sqrts" (f64.const 0x1.ac83451b08989p+523) (f64.const 0x1.8da575c6d12b8p-109)) (f64.const 0x1.09c003991ce17p+316))
(assert_return (invoke "f64.no_fold_div_sqrts" (f64.const 0x1.bab7836456417p-810) (f64.const 0x1.1ff60d03ba607p+291)) (f64.const 0x1.c0e6c833bf657p-551))
(assert_return (invoke "f64.no_fold_div_sqrts" (f64.const 0x1.a957816ad9515p-789) (f64.const 0x1.8c18a3a222ab1p+945)) (f64.const 0x1.0948539781e92p-867))

;; Test that (x*sqrt(y))/y is not optimized to x/sqrt(y).

(module
  (func (export "f32.no_fold_mul_sqrt_div") (param $x f32) (param $y f32) (result f32)
    (f32.div (f32.mul (get_local $x) (f32.sqrt (get_local $y))) (get_local $y)))

  (func (export "f64.no_fold_mul_sqrt_div") (param $x f64) (param $y f64) (result f64)
    (f64.div (f64.mul (get_local $x) (f64.sqrt (get_local $y))) (get_local $y)))
)

(assert_return (invoke "f32.no_fold_mul_sqrt_div" (f32.const -0x1.f4a7cap+81) (f32.const 0x1.c09adep+92)) (f32.const -inf))
(assert_return (invoke "f32.no_fold_mul_sqrt_div" (f32.const -0x1.90bf1cp-120) (f32.const 0x1.8dbe88p-97)) (f32.const -0x0p+0))
(assert_return (invoke "f32.no_fold_mul_sqrt_div" (f32.const 0x1.8570e8p+29) (f32.const 0x1.217d3p-128)) (f32.const 0x1.6e391ap+93))
(assert_return (invoke "f32.no_fold_mul_sqrt_div" (f32.const -0x1.5b4652p+43) (f32.const 0x1.a9d71cp+112)) (f32.const -0x1.0d423ap-13))
(assert_return (invoke "f32.no_fold_mul_sqrt_div" (f32.const -0x1.910604p+8) (f32.const 0x1.0ca912p+7)) (f32.const -0x1.14cdecp+5))

(assert_return (invoke "f64.no_fold_mul_sqrt_div" (f64.const 0x1.1dcdeb857305fp+698) (f64.const 0x1.a066171c40eb9p+758)) (f64.const inf))
(assert_return (invoke "f64.no_fold_mul_sqrt_div" (f64.const -0x1.8b4f1c218e2abp-827) (f64.const 0x1.5e1ee65953b0bp-669)) (f64.const -0x0p+0))
(assert_return (invoke "f64.no_fold_mul_sqrt_div" (f64.const 0x1.74ee531ddba38p-425) (f64.const 0x1.f370f758857f3p+560)) (f64.const 0x1.0aff34269583ep-705))
(assert_return (invoke "f64.no_fold_mul_sqrt_div" (f64.const -0x1.27f216b0da6c5p+352) (f64.const 0x1.8e0b4e0b9fd7ep-483)) (f64.const -0x1.4fa558aad514ep+593))
(assert_return (invoke "f64.no_fold_mul_sqrt_div" (f64.const 0x1.4c6955df9912bp+104) (f64.const 0x1.0cca42c9d371ep+842)) (f64.const 0x1.4468072f54294p-317))

;; Test that subnormals are not flushed even in an intermediate value in an
;; expression with a normal result.

(module
  (func (export "f32.no_flush_intermediate_subnormal") (param $x f32) (param $y f32) (param $z f32) (result f32)
    (f32.mul (f32.mul (get_local $x) (get_local $y)) (get_local $z)))

  (func (export "f64.no_flush_intermediate_subnormal") (param $x f64) (param $y f64) (param $z f64) (result f64)
    (f64.mul (f64.mul (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f32.no_flush_intermediate_subnormal" (f32.const 0x1p-126) (f32.const 0x1p-23) (f32.const 0x1p23)) (f32.const 0x1p-126))
(assert_return (invoke "f64.no_flush_intermediate_subnormal" (f64.const 0x1p-1022) (f64.const 0x1p-52) (f64.const 0x1p52)) (f64.const 0x1p-1022))

;; Test corner cases of John Hauser's microarchitectural recoding scheme.
;; https://github.com/riscv/riscv-tests/blob/695b86a6fcbe06ffbed8891af7e6fe7bf2062543/isa/rv64uf/recoding.S

(module
  (func (export "f32.recoding_eq") (param $x f32) (param $y f32) (result i32)
    (f32.eq (f32.mul (get_local $x) (get_local $y)) (get_local $x)))

  (func (export "f32.recoding_le") (param $x f32) (param $y f32) (result i32)
    (f32.le (f32.mul (get_local $x) (get_local $y)) (get_local $x)))

  (func (export "f32.recoding_lt") (param $x f32) (param $y f32) (result i32)
    (f32.lt (f32.mul (get_local $x) (get_local $y)) (get_local $x)))

  (func (export "f64.recoding_eq") (param $x f64) (param $y f64) (result i32)
    (f64.eq (f64.mul (get_local $x) (get_local $y)) (get_local $x)))

  (func (export "f64.recoding_le") (param $x f64) (param $y f64) (result i32)
    (f64.le (f64.mul (get_local $x) (get_local $y)) (get_local $x)))

  (func (export "f64.recoding_lt") (param $x f64) (param $y f64) (result i32)
    (f64.lt (f64.mul (get_local $x) (get_local $y)) (get_local $x)))

  (func (export "recoding_demote") (param $x f64) (param $y f32) (result f32)
    (f32.mul (f32.demote/f64 (get_local $x)) (get_local $y)))
)

(assert_return (invoke "f32.recoding_eq" (f32.const -inf) (f32.const 3.0)) (i32.const 1))
(assert_return (invoke "f32.recoding_le" (f32.const -inf) (f32.const 3.0)) (i32.const 1))
(assert_return (invoke "f32.recoding_lt" (f32.const -inf) (f32.const 3.0)) (i32.const 0))

(assert_return (invoke "f32.recoding_eq" (f32.const 0x0p+0) (f32.const 0x1p+0)) (i32.const 1))
(assert_return (invoke "f32.recoding_le" (f32.const 0x0p+0) (f32.const 0x1p+0)) (i32.const 1))
(assert_return (invoke "f32.recoding_lt" (f32.const 0x0p+0) (f32.const 0x1p+0)) (i32.const 0))

(assert_return (invoke "f64.recoding_eq" (f64.const -inf) (f64.const 3.0)) (i32.const 1))
(assert_return (invoke "f64.recoding_le" (f64.const -inf) (f64.const 3.0)) (i32.const 1))
(assert_return (invoke "f64.recoding_lt" (f64.const -inf) (f64.const 3.0)) (i32.const 0))

(assert_return (invoke "f64.recoding_eq" (f64.const 0x0p+0) (f64.const 0x1p+0)) (i32.const 1))
(assert_return (invoke "f64.recoding_le" (f64.const 0x0p+0) (f64.const 0x1p+0)) (i32.const 1))
(assert_return (invoke "f64.recoding_lt" (f64.const 0x0p+0) (f64.const 0x1p+0)) (i32.const 0))

(assert_return (invoke "recoding_demote" (f64.const 0x1.4c8f8p-132) (f32.const 1221)) (f32.const 0x1.8c8a1cp-122))

;; Test that division is not done as on an extended-base system.
;; http://www.ucbtest.org/goldberg/addendum.html

(module
  (func (export "f32.no_extended_precision_div") (param $x f32) (param $y f32) (param $z f32) (result i32)
    (f32.eq (f32.div (get_local $x) (get_local $y)) (get_local $z)))

  (func (export "f64.no_extended_precision_div") (param $x f64) (param $y f64) (param $z f64) (result i32)
    (f64.eq (f64.div (get_local $x) (get_local $y)) (get_local $z)))
)

(assert_return (invoke "f32.no_extended_precision_div" (f32.const 3.0) (f32.const 7.0) (f32.const 0x1.b6db6ep-2)) (i32.const 1))
(assert_return (invoke "f64.no_extended_precision_div" (f64.const 3.0) (f64.const 7.0) (f64.const 0x1.b6db6db6db6dbp-2)) (i32.const 1))

;; a*x + b*x == (a+b)*x for all x only if the operations a*x, b*x, and (a+b)
;; are all exact operations, which is true only if a and b are exact powers of
;; 2. Even then, if a==-b and x==-0, then a*x+b*x==0.0, (a+b)*x==-0.0.
;; https://dlang.org/d-floating-point.html

(module
  (func (export "f32.no_distribute_exact") (param $x f32) (result f32)
    (f32.add (f32.mul (f32.const -8.0) (get_local $x)) (f32.mul (f32.const 8.0) (get_local $x))))

  (func (export "f64.no_distribute_exact") (param $x f64) (result f64)
    (f64.add (f64.mul (f64.const -8.0) (get_local $x)) (f64.mul (f64.const 8.0) (get_local $x))))
)

(assert_return (invoke "f32.no_distribute_exact" (f32.const -0.0)) (f32.const 0.0))
(assert_return (invoke "f64.no_distribute_exact" (f64.const -0.0)) (f64.const 0.0))

;; Test that various approximations of sqrt(2), sqrt(3), and sqrt(5) compute the
;; expected approximation.
;; https://xkcd.com/1047/
(module
  (func (export "f32.sqrt") (param f32) (result f32)
    (f32.sqrt (get_local 0)))

  (func (export "f32.xkcd_sqrt_2") (param f32) (param f32) (param f32) (param f32) (result f32)
    (f32.add (f32.div (get_local 0) (get_local 1)) (f32.div (get_local 2) (f32.sub (get_local 3) (get_local 2)))))

  (func (export "f32.xkcd_sqrt_3") (param f32) (param f32) (param f32) (result f32)
    (f32.div (f32.mul (get_local 0) (get_local 1)) (get_local 2)))

  (func (export "f32.xkcd_sqrt_5") (param f32) (param f32) (param f32) (result f32)
    (f32.add (f32.div (get_local 0) (get_local 1)) (f32.div (get_local 2) (get_local 0))))

  (func (export "f32.xkcd_better_sqrt_5") (param f32) (param f32) (param f32) (param f32) (result f32)
    (f32.div (f32.add (get_local 0) (f32.mul (get_local 1) (get_local 2))) (f32.sub (get_local 3) (f32.mul (get_local 1) (get_local 2)))))

  (func (export "f64.sqrt") (param f64) (result f64)
    (f64.sqrt (get_local 0)))

  (func (export "f64.xkcd_sqrt_2") (param f64) (param f64) (param f64) (param f64) (result f64)
    (f64.add (f64.div (get_local 0) (get_local 1)) (f64.div (get_local 2) (f64.sub (get_local 3) (get_local 2)))))

  (func (export "f64.xkcd_sqrt_3") (param f64) (param f64) (param f64) (result f64)
    (f64.div (f64.mul (get_local 0) (get_local 1)) (get_local 2)))

  (func (export "f64.xkcd_sqrt_5") (param f64) (param f64) (param f64) (result f64)
    (f64.add (f64.div (get_local 0) (get_local 1)) (f64.div (get_local 2) (get_local 0))))

  (func (export "f64.xkcd_better_sqrt_5") (param f64) (param f64) (param f64) (param f64) (result f64)
    (f64.div (f64.add (get_local 0) (f64.mul (get_local 1) (get_local 2))) (f64.sub (get_local 3) (f64.mul (get_local 1) (get_local 2)))))
)

(assert_return (invoke "f32.sqrt" (f32.const 2.0)) (f32.const 0x1.6a09e6p+0))
(assert_return (invoke "f32.xkcd_sqrt_2" (f32.const 3.0) (f32.const 5.0) (f32.const 0x1.921fb6p+1) (f32.const 7.0)) (f32.const 0x1.6a0a54p+0))
(assert_return (invoke "f32.sqrt" (f32.const 3.0)) (f32.const 0x1.bb67aep+0))
(assert_return (invoke "f32.xkcd_sqrt_3" (f32.const 2.0) (f32.const 0x1.5bf0a8p+1) (f32.const 0x1.921fb6p+1)) (f32.const 0x1.bb02d4p+0))
(assert_return (invoke "f32.sqrt" (f32.const 5.0)) (f32.const 0x1.1e377ap+1))
(assert_return (invoke "f32.xkcd_sqrt_5" (f32.const 2.0) (f32.const 0x1.5bf0a8p+1) (f32.const 3.0)) (f32.const 0x1.1e2d58p+1))
(assert_return (invoke "f32.xkcd_better_sqrt_5" (f32.const 13.0) (f32.const 4.0) (f32.const 0x1.921fb6p+1) (f32.const 24.0)) (f32.const 0x1.1e377ap+1))

(assert_return (invoke "f64.sqrt" (f64.const 2.0)) (f64.const 0x1.6a09e667f3bcdp+0))
(assert_return (invoke "f64.xkcd_sqrt_2" (f64.const 3.0) (f64.const 5.0) (f64.const 0x1.921fb54442d18p+1) (f64.const 7.0)) (f64.const 0x1.6a0a5362b055fp+0))
(assert_return (invoke "f64.sqrt" (f64.const 3.0)) (f64.const 0x1.bb67ae8584caap+0))
(assert_return (invoke "f64.xkcd_sqrt_3" (f64.const 2.0) (f64.const 0x1.5bf0a8b145769p+1) (f64.const 0x1.921fb54442d18p+1)) (f64.const 0x1.bb02d4eca8f95p+0))
(assert_return (invoke "f64.sqrt" (f64.const 5.0)) (f64.const 0x1.1e3779b97f4a8p+1))
(assert_return (invoke "f64.xkcd_sqrt_5" (f64.const 2.0) (f64.const 0x1.5bf0a8b145769p+1) (f64.const 3.0)) (f64.const 0x1.1e2d58d8b3bcep+1))
(assert_return (invoke "f64.xkcd_better_sqrt_5" (f64.const 13.0) (f64.const 4.0) (f64.const 0x1.921fb54442d18p+1) (f64.const 24.0)) (f64.const 0x1.1e3778509a5a3p+1))

;; Compute the floating-point radix.
;; M. A. Malcom. Algorithms to reveal properties of floating-point arithmetic.
;; Communications of the ACM, 15(11):949-951, November 1972.
(module
  (func (export "f32.compute_radix") (param $0 f32) (param $1 f32) (result f32)
    (loop $label$0
      (br_if $label$0
        (f32.eq
          (f32.add
            (f32.sub
              (f32.add
                (tee_local $0 (f32.add (get_local $0) (get_local $0)))
                (f32.const 1)
              )
              (get_local $0)
            )
            (f32.const -1)
          )
          (f32.const 0)
        )
      )
    )
    (loop $label$2
      (br_if $label$2
        (f32.ne
          (f32.sub
            (f32.sub
              (f32.add
                (get_local $0)
                (tee_local $1 (f32.add (get_local $1) (f32.const 1)))
              )
              (get_local $0)
            )
            (get_local $1)
          )
          (f32.const 0)
        )
      )
    )
    (get_local $1)
  )

  (func (export "f64.compute_radix") (param $0 f64) (param $1 f64) (result f64)
    (loop $label$0
      (br_if $label$0
        (f64.eq
          (f64.add
            (f64.sub
              (f64.add
                (tee_local $0 (f64.add (get_local $0) (get_local $0)))
                (f64.const 1)
              )
              (get_local $0)
            )
            (f64.const -1)
          )
          (f64.const 0)
        )
      )
    )
    (loop $label$2
      (br_if $label$2
        (f64.ne
          (f64.sub
            (f64.sub
              (f64.add
                (get_local $0)
                (tee_local $1 (f64.add (get_local $1) (f64.const 1)))
              )
              (get_local $0)
            )
            (get_local $1)
          )
          (f64.const 0)
        )
      )
    )
    (get_local $1)
  )
)

(assert_return (invoke "f32.compute_radix" (f32.const 1.0) (f32.const 1.0)) (f32.const 2.0))
(assert_return (invoke "f64.compute_radix" (f64.const 1.0) (f64.const 1.0)) (f64.const 2.0))

;; Test that (x - 1) * y + y is not optimized to x * y.
;; http://blog.frama-c.com/index.php?post/2013/05/14/Contrarianism

(module
  (func (export "f32.no_fold_sub1_mul_add") (param $x f32) (param $y f32) (result f32)
    (f32.add (f32.mul (f32.sub (get_local $x) (f32.const 1.0)) (get_local $y)) (get_local $y)))

  (func (export "f64.no_fold_sub1_mul_add") (param $x f64) (param $y f64) (result f64)
    (f64.add (f64.mul (f64.sub (get_local $x) (f64.const 1.0)) (get_local $y)) (get_local $y)))
)

(assert_return (invoke "f32.no_fold_sub1_mul_add" (f32.const 0x1p-32) (f32.const 1.0)) (f32.const 0x0p+0))
(assert_return (invoke "f64.no_fold_sub1_mul_add" (f64.const 0x1p-64) (f64.const 1.0)) (f64.const 0x0p+0))

;; Test that x+z >= y+z is not optimized to x >= y (monotonicity).
;; http://cs.nyu.edu/courses/spring13/CSCI-UA.0201-003/lecture6.pdf

(module
  (func (export "f32.no_fold_add_le_monotonicity") (param $x f32) (param $y f32) (param $z f32) (result i32)
    (f32.le (f32.add (get_local $x) (get_local $z)) (f32.add (get_local $y) (get_local $z))))

  (func (export "f32.no_fold_add_ge_monotonicity") (param $x f32) (param $y f32) (param $z f32) (result i32)
    (f32.ge (f32.add (get_local $x) (get_local $z)) (f32.add (get_local $y) (get_local $z))))

  (func (export "f64.no_fold_add_le_monotonicity") (param $x f64) (param $y f64) (param $z f64) (result i32)
    (f64.le (f64.add (get_local $x) (get_local $z)) (f64.add (get_local $y) (get_local $z))))

  (func (export "f64.no_fold_add_ge_monotonicity") (param $x f64) (param $y f64) (param $z f64) (result i32)
    (f64.ge (f64.add (get_local $x) (get_local $z)) (f64.add (get_local $y) (get_local $z))))
)

(assert_return (invoke "f32.no_fold_add_le_monotonicity" (f32.const 0.0) (f32.const 0.0) (f32.const nan)) (i32.const 0))
(assert_return (invoke "f32.no_fold_add_le_monotonicity" (f32.const inf) (f32.const -inf) (f32.const inf)) (i32.const 0))
(assert_return (invoke "f64.no_fold_add_le_monotonicity" (f64.const 0.0) (f64.const 0.0) (f64.const nan)) (i32.const 0))
(assert_return (invoke "f64.no_fold_add_le_monotonicity" (f64.const inf) (f64.const -inf) (f64.const inf)) (i32.const 0))

;; Test that !(x < y) and friends are not optimized to x >= y and friends.

(module
  (func (export "f32.not_lt") (param $x f32) (param $y f32) (result i32)
    (i32.eqz (f32.lt (get_local $x) (get_local $y))))

  (func (export "f32.not_le") (param $x f32) (param $y f32) (result i32)
    (i32.eqz (f32.le (get_local $x) (get_local $y))))

  (func (export "f32.not_gt") (param $x f32) (param $y f32) (result i32)
    (i32.eqz (f32.gt (get_local $x) (get_local $y))))

  (func (export "f32.not_ge") (param $x f32) (param $y f32) (result i32)
    (i32.eqz (f32.ge (get_local $x) (get_local $y))))

  (func (export "f64.not_lt") (param $x f64) (param $y f64) (result i32)
    (i32.eqz (f64.lt (get_local $x) (get_local $y))))

  (func (export "f64.not_le") (param $x f64) (param $y f64) (result i32)
    (i32.eqz (f64.le (get_local $x) (get_local $y))))

  (func (export "f64.not_gt") (param $x f64) (param $y f64) (result i32)
    (i32.eqz (f64.gt (get_local $x) (get_local $y))))

  (func (export "f64.not_ge") (param $x f64) (param $y f64) (result i32)
    (i32.eqz (f64.ge (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.not_lt" (f32.const nan) (f32.const 0.0)) (i32.const 1))
(assert_return (invoke "f32.not_le" (f32.const nan) (f32.const 0.0)) (i32.const 1))
(assert_return (invoke "f32.not_gt" (f32.const nan) (f32.const 0.0)) (i32.const 1))
(assert_return (invoke "f32.not_ge" (f32.const nan) (f32.const 0.0)) (i32.const 1))
(assert_return (invoke "f64.not_lt" (f64.const nan) (f64.const 0.0)) (i32.const 1))
(assert_return (invoke "f64.not_le" (f64.const nan) (f64.const 0.0)) (i32.const 1))
(assert_return (invoke "f64.not_gt" (f64.const nan) (f64.const 0.0)) (i32.const 1))
(assert_return (invoke "f64.not_ge" (f64.const nan) (f64.const 0.0)) (i32.const 1))

;; Test that a method for approximating a "machine epsilon" produces the expected
;; approximation.
;; http://blogs.mathworks.com/cleve/2014/07/07/floating-point-numbers/#24cb4f4d-b8a9-4c19-b22b-9d2a9f7f3812

(module
  (func (export "f32.epsilon") (result f32)
    (f32.sub (f32.const 1.0) (f32.mul (f32.const 3.0) (f32.sub (f32.div (f32.const 4.0) (f32.const 3.0)) (f32.const 1.0)))))

  (func (export "f64.epsilon") (result f64)
    (f64.sub (f64.const 1.0) (f64.mul (f64.const 3.0) (f64.sub (f64.div (f64.const 4.0) (f64.const 3.0)) (f64.const 1.0)))))
)

(assert_return (invoke "f32.epsilon") (f32.const -0x1p-23))
(assert_return (invoke "f64.epsilon") (f64.const 0x1p-52))

;; Test that a method for computing a "machine epsilon" produces the expected
;; result.
;; https://www.math.utah.edu/~beebe/software/ieee/

(module
  (func (export "f32.epsilon") (result f32)
    (local $x f32)
    (local $result f32)
    (set_local $x (f32.const 1))
    (loop $loop
      (br_if $loop
        (f32.gt
          (f32.add
            (tee_local $x
              (f32.mul
                (tee_local $result (get_local $x))
                (f32.const 0.5)
              )
            )
            (f32.const 1)
          )
          (f32.const 1)
        )
      )
    )
    (get_local $result)
  )

  (func (export "f64.epsilon") (result f64)
    (local $x f64)
    (local $result f64)
    (set_local $x (f64.const 1))
    (loop $loop
      (br_if $loop
        (f64.gt
          (f64.add
            (tee_local $x
              (f64.mul
                (tee_local $result (get_local $x))
                (f64.const 0.5)
              )
            )
            (f64.const 1)
          )
          (f64.const 1)
        )
      )
    )
    (get_local $result)
  )
)

(assert_return (invoke "f32.epsilon") (f32.const 0x1p-23))
(assert_return (invoke "f64.epsilon") (f64.const 0x1p-52))

;; Test that floating-point numbers are not optimized as if they form a
;; trichotomy.

(module
  (func (export "f32.no_trichotomy_lt") (param $x f32) (param $y f32) (result i32)
    (i32.or (f32.lt (get_local $x) (get_local $y)) (f32.ge (get_local $x) (get_local $y))))
  (func (export "f32.no_trichotomy_le") (param $x f32) (param $y f32) (result i32)
    (i32.or (f32.le (get_local $x) (get_local $y)) (f32.gt (get_local $x) (get_local $y))))
  (func (export "f32.no_trichotomy_gt") (param $x f32) (param $y f32) (result i32)
    (i32.or (f32.gt (get_local $x) (get_local $y)) (f32.le (get_local $x) (get_local $y))))
  (func (export "f32.no_trichotomy_ge") (param $x f32) (param $y f32) (result i32)
    (i32.or (f32.ge (get_local $x) (get_local $y)) (f32.lt (get_local $x) (get_local $y))))

  (func (export "f64.no_trichotomy_lt") (param $x f64) (param $y f64) (result i32)
    (i32.or (f64.lt (get_local $x) (get_local $y)) (f64.ge (get_local $x) (get_local $y))))
  (func (export "f64.no_trichotomy_le") (param $x f64) (param $y f64) (result i32)
    (i32.or (f64.le (get_local $x) (get_local $y)) (f64.gt (get_local $x) (get_local $y))))
  (func (export "f64.no_trichotomy_gt") (param $x f64) (param $y f64) (result i32)
    (i32.or (f64.gt (get_local $x) (get_local $y)) (f64.le (get_local $x) (get_local $y))))
  (func (export "f64.no_trichotomy_ge") (param $x f64) (param $y f64) (result i32)
    (i32.or (f64.ge (get_local $x) (get_local $y)) (f64.lt (get_local $x) (get_local $y))))
)

(assert_return (invoke "f32.no_trichotomy_lt" (f32.const 0.0) (f32.const nan)) (i32.const 0))
(assert_return (invoke "f32.no_trichotomy_le" (f32.const 0.0) (f32.const nan)) (i32.const 0))
(assert_return (invoke "f32.no_trichotomy_gt" (f32.const 0.0) (f32.const nan)) (i32.const 0))
(assert_return (invoke "f32.no_trichotomy_ge" (f32.const 0.0) (f32.const nan)) (i32.const 0))
(assert_return (invoke "f64.no_trichotomy_lt" (f64.const 0.0) (f64.const nan)) (i32.const 0))
(assert_return (invoke "f64.no_trichotomy_le" (f64.const 0.0) (f64.const nan)) (i32.const 0))
(assert_return (invoke "f64.no_trichotomy_gt" (f64.const 0.0) (f64.const nan)) (i32.const 0))
(assert_return (invoke "f64.no_trichotomy_ge" (f64.const 0.0) (f64.const nan)) (i32.const 0))

;; Some test harnesses which can run this testsuite are unable to perform tests
;; of NaN bitpatterns. The following tests whether the underlying platform is
;; generally producing the kinds of NaNs expected.
(module
  (func (export "f32.arithmetic_nan_bitpattern")
        (param $x i32) (param $y i32) (result i32)
    (i32.and (i32.reinterpret/f32
               (f32.div
                 (f32.reinterpret/i32 (get_local $x))
                 (f32.reinterpret/i32 (get_local $y))))
             (i32.const 0x7fc00000)))
  (func (export "f32.canonical_nan_bitpattern")
        (param $x i32) (param $y i32) (result i32)
    (i32.and (i32.reinterpret/f32
               (f32.div
                 (f32.reinterpret/i32 (get_local $x))
                 (f32.reinterpret/i32 (get_local $y))))
             (i32.const 0x7fffffff)))
  (func (export "f32.nonarithmetic_nan_bitpattern")
        (param $x i32) (result i32)
    (i32.reinterpret/f32 (f32.neg (f32.reinterpret/i32 (get_local $x)))))

  (func (export "f64.arithmetic_nan_bitpattern")
        (param $x i64) (param $y i64) (result i64)
    (i64.and (i64.reinterpret/f64
               (f64.div
                 (f64.reinterpret/i64 (get_local $x))
                 (f64.reinterpret/i64 (get_local $y))))
             (i64.const 0x7ff8000000000000)))
  (func (export "f64.canonical_nan_bitpattern")
        (param $x i64) (param $y i64) (result i64)
    (i64.and (i64.reinterpret/f64
               (f64.div
                 (f64.reinterpret/i64 (get_local $x))
                 (f64.reinterpret/i64 (get_local $y))))
             (i64.const 0x7fffffffffffffff)))
  (func (export "f64.nonarithmetic_nan_bitpattern")
        (param $x i64) (result i64)
    (i64.reinterpret/f64 (f64.neg (f64.reinterpret/i64 (get_local $x)))))

  ;; Versions of no_fold testcases that only care about NaN bitpatterns.
  (func (export "f32.no_fold_sub_zero") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.sub (f32.reinterpret/i32 (get_local $x)) (f32.const 0.0)))
             (i32.const 0x7fc00000)))
  (func (export "f32.no_fold_neg0_sub") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.sub (f32.const -0.0) (f32.reinterpret/i32 (get_local $x))))
             (i32.const 0x7fc00000)))
  (func (export "f32.no_fold_mul_one") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.mul (f32.reinterpret/i32 (get_local $x)) (f32.const 1.0)))
             (i32.const 0x7fc00000)))
  (func (export "f32.no_fold_neg1_mul") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.mul (f32.const -1.0) (f32.reinterpret/i32 (get_local $x))))
             (i32.const 0x7fc00000)))
  (func (export "f32.no_fold_div_one") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.div (f32.reinterpret/i32 (get_local $x)) (f32.const 1.0)))
             (i32.const 0x7fc00000)))
  (func (export "f32.no_fold_div_neg1") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.div (f32.reinterpret/i32 (get_local $x)) (f32.const -1.0)))
             (i32.const 0x7fc00000)))
  (func (export "f64.no_fold_sub_zero") (param $x i64) (result i64)
    (i64.and (i64.reinterpret/f64 (f64.sub (f64.reinterpret/i64 (get_local $x)) (f64.const 0.0)))
             (i64.const 0x7ff8000000000000)))
  (func (export "f64.no_fold_neg0_sub") (param $x i64) (result i64)
    (i64.and (i64.reinterpret/f64 (f64.sub (f64.const -0.0) (f64.reinterpret/i64 (get_local $x))))
             (i64.const 0x7ff8000000000000)))
  (func (export "f64.no_fold_mul_one") (param $x i64) (result i64)
    (i64.and (i64.reinterpret/f64 (f64.mul (f64.reinterpret/i64 (get_local $x)) (f64.const 1.0)))
             (i64.const 0x7ff8000000000000)))
  (func (export "f64.no_fold_neg1_mul") (param $x i64) (result i64)
    (i64.and (i64.reinterpret/f64 (f64.mul (f64.const -1.0) (f64.reinterpret/i64 (get_local $x))))
             (i64.const 0x7ff8000000000000)))
  (func (export "f64.no_fold_div_one") (param $x i64) (result i64)
    (i64.and (i64.reinterpret/f64 (f64.div (f64.reinterpret/i64 (get_local $x)) (f64.const 1.0)))
             (i64.const 0x7ff8000000000000)))
  (func (export "f64.no_fold_div_neg1") (param $x i64) (result i64)
    (i64.and (i64.reinterpret/f64 (f64.div (f64.reinterpret/i64 (get_local $x)) (f64.const -1.0)))
             (i64.const 0x7ff8000000000000)))
  (func (export "no_fold_promote_demote") (param $x i32) (result i32)
    (i32.and (i32.reinterpret/f32 (f32.demote/f64 (f64.promote/f32 (f32.reinterpret/i32 (get_local $x)))))
             (i32.const 0x7fc00000)))
)

(assert_return (invoke "f32.arithmetic_nan_bitpattern" (i32.const 0x7f803210) (i32.const 0x7f803210)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.canonical_nan_bitpattern" (i32.const 0) (i32.const 0)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.canonical_nan_bitpattern" (i32.const 0x7fc00000) (i32.const 0x7fc00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.canonical_nan_bitpattern" (i32.const 0xffc00000) (i32.const 0x7fc00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.canonical_nan_bitpattern" (i32.const 0x7fc00000) (i32.const 0xffc00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.canonical_nan_bitpattern" (i32.const 0xffc00000) (i32.const 0xffc00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.nonarithmetic_nan_bitpattern" (i32.const 0x7fc03210)) (i32.const 0xffc03210))
(assert_return (invoke "f32.nonarithmetic_nan_bitpattern" (i32.const 0xffc03210)) (i32.const 0x7fc03210))
(assert_return (invoke "f32.nonarithmetic_nan_bitpattern" (i32.const 0x7f803210)) (i32.const 0xff803210))
(assert_return (invoke "f32.nonarithmetic_nan_bitpattern" (i32.const 0xff803210)) (i32.const 0x7f803210))
(assert_return (invoke "f64.arithmetic_nan_bitpattern" (i64.const 0x7ff0000000003210) (i64.const 0x7ff0000000003210)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.canonical_nan_bitpattern" (i64.const 0) (i64.const 0)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.canonical_nan_bitpattern" (i64.const 0x7ff8000000000000) (i64.const 0x7ff8000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.canonical_nan_bitpattern" (i64.const 0xfff8000000000000) (i64.const 0x7ff8000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.canonical_nan_bitpattern" (i64.const 0x7ff8000000000000) (i64.const 0xfff8000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.canonical_nan_bitpattern" (i64.const 0xfff8000000000000) (i64.const 0xfff8000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.nonarithmetic_nan_bitpattern" (i64.const 0x7ff8000000003210)) (i64.const 0xfff8000000003210))
(assert_return (invoke "f64.nonarithmetic_nan_bitpattern" (i64.const 0xfff8000000003210)) (i64.const 0x7ff8000000003210))
(assert_return (invoke "f64.nonarithmetic_nan_bitpattern" (i64.const 0x7ff0000000003210)) (i64.const 0xfff0000000003210))
(assert_return (invoke "f64.nonarithmetic_nan_bitpattern" (i64.const 0xfff0000000003210)) (i64.const 0x7ff0000000003210))
(assert_return (invoke "f32.no_fold_sub_zero" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.no_fold_neg0_sub" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.no_fold_mul_one" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.no_fold_neg1_mul" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.no_fold_div_one" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f32.no_fold_div_neg1" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))
(assert_return (invoke "f64.no_fold_sub_zero" (i64.const 0x7ff4000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.no_fold_neg0_sub" (i64.const 0x7ff4000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.no_fold_mul_one" (i64.const 0x7ff4000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.no_fold_neg1_mul" (i64.const 0x7ff4000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.no_fold_div_one" (i64.const 0x7ff4000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "f64.no_fold_div_neg1" (i64.const 0x7ff4000000000000)) (i64.const 0x7ff8000000000000))
(assert_return (invoke "no_fold_promote_demote" (i32.const 0x7fa00000)) (i32.const 0x7fc00000))

;; Test that IEEE 754 double precision does, in fact, compute a certain dot
;; product correctly.

(module
  (func (export "dot_product_example")
        (param $x0 f64) (param $x1 f64) (param $x2 f64) (param $x3 f64)
        (param $y0 f64) (param $y1 f64) (param $y2 f64) (param $y3 f64)
        (result f64)
    (f64.add (f64.add (f64.add
      (f64.mul (get_local $x0) (get_local $y0))
      (f64.mul (get_local $x1) (get_local $y1)))
      (f64.mul (get_local $x2) (get_local $y2)))
      (f64.mul (get_local $x3) (get_local $y3)))
  )

  (func (export "with_binary_sum_collapse")
        (param $x0 f64) (param $x1 f64) (param $x2 f64) (param $x3 f64)
        (param $y0 f64) (param $y1 f64) (param $y2 f64) (param $y3 f64)
        (result f64)
      (f64.add (f64.add (f64.mul (get_local $x0) (get_local $y0))
                        (f64.mul (get_local $x1) (get_local $y1)))
               (f64.add (f64.mul (get_local $x2) (get_local $y2))
                        (f64.mul (get_local $x3) (get_local $y3))))
  )
)

(assert_return (invoke "dot_product_example"
    (f64.const 3.2e7) (f64.const 1.0) (f64.const -1.0) (f64.const 8.0e7)
    (f64.const 4.0e7) (f64.const 1.0) (f64.const -1.0) (f64.const -1.6e7))
  (f64.const 2.0))
(assert_return (invoke "with_binary_sum_collapse"
    (f64.const 3.2e7) (f64.const 1.0) (f64.const -1.0) (f64.const 8.0e7)
    (f64.const 4.0e7) (f64.const 1.0) (f64.const -1.0) (f64.const -1.6e7))
  (f64.const 2.0))

;; http://www.vinc17.org/research/fptest.en.html#contract2fma

(module
  (func (export "f32.contract2fma")
        (param $x f32) (param $y f32) (result f32)
    (f32.sqrt (f32.sub (f32.mul (get_local $x) (get_local $x))
                       (f32.mul (get_local $y) (get_local $y)))))
  (func (export "f64.contract2fma")
        (param $x f64) (param $y f64) (result f64)
    (f64.sqrt (f64.sub (f64.mul (get_local $x) (get_local $x))
                       (f64.mul (get_local $y) (get_local $y)))))
)

(assert_return (invoke "f32.contract2fma" (f32.const 1.0) (f32.const 1.0)) (f32.const 0.0))
(assert_return (invoke "f32.contract2fma" (f32.const 0x1.19999ap+0) (f32.const 0x1.19999ap+0)) (f32.const 0.0))
(assert_return (invoke "f32.contract2fma" (f32.const 0x1.333332p+0) (f32.const 0x1.333332p+0)) (f32.const 0.0))
(assert_return (invoke "f64.contract2fma" (f64.const 1.0) (f64.const 1.0)) (f64.const 0.0))
(assert_return (invoke "f64.contract2fma" (f64.const 0x1.199999999999ap+0) (f64.const 0x1.199999999999ap+0)) (f64.const 0.0))
(assert_return (invoke "f64.contract2fma" (f64.const 0x1.3333333333333p+0) (f64.const 0x1.3333333333333p+0)) (f64.const 0.0))

;; Test that floating-point isn't implemented with QuickBasic for MS-DOS.
;; https://support.microsoft.com/en-us/help/42980/-complete-tutorial-to-understand-ieee-floating-point-errors

(module
  (func (export "f32.division_by_small_number")
        (param $a f32) (param $b f32) (param $c f32) (result f32)
    (f32.sub (get_local $a) (f32.div (get_local $b) (get_local $c))))
  (func (export "f64.division_by_small_number")
        (param $a f64) (param $b f64) (param $c f64) (result f64)
    (f64.sub (get_local $a) (f64.div (get_local $b) (get_local $c))))
)

(assert_return (invoke "f32.division_by_small_number" (f32.const 112000000) (f32.const 100000) (f32.const 0.0009)) (f32.const 888888))
(assert_return (invoke "f64.division_by_small_number" (f64.const 112000000) (f64.const 100000) (f64.const 0.0009)) (f64.const 888888.8888888806))

;; Test a simple golden ratio computation.
;; http://mathworld.wolfram.com/GoldenRatio.html

(module
  (func (export "f32.golden_ratio") (param $a f32) (param $b f32) (param $c f32) (result f32)
    (f32.mul (get_local 0) (f32.add (get_local 1) (f32.sqrt (get_local 2)))))
  (func (export "f64.golden_ratio") (param $a f64) (param $b f64) (param $c f64) (result f64)
    (f64.mul (get_local 0) (f64.add (get_local 1) (f64.sqrt (get_local 2)))))
)

(assert_return (invoke "f32.golden_ratio" (f32.const 0.5) (f32.const 1.0) (f32.const 5.0)) (f32.const 1.618034))
(assert_return (invoke "f64.golden_ratio" (f64.const 0.5) (f64.const 1.0) (f64.const 5.0)) (f64.const 1.618033988749895))

;; Test some silver means computations.
;; http://mathworld.wolfram.com/SilverRatio.html

(module
  (func (export "f32.silver_means") (param $n f32) (result f32)
    (f32.mul (f32.const 0.5)
             (f32.add (get_local $n)
                      (f32.sqrt (f32.add (f32.mul (get_local $n) (get_local $n))
                                         (f32.const 4.0))))))
  (func (export "f64.silver_means") (param $n f64) (result f64)
    (f64.mul (f64.const 0.5)
             (f64.add (get_local $n)
                      (f64.sqrt (f64.add (f64.mul (get_local $n) (get_local $n))
                                         (f64.const 4.0))))))
)

(assert_return (invoke "f32.silver_means" (f32.const 0.0)) (f32.const 1.0))
(assert_return (invoke "f32.silver_means" (f32.const 1.0)) (f32.const 1.6180340))
(assert_return (invoke "f32.silver_means" (f32.const 2.0)) (f32.const 2.4142136))
(assert_return (invoke "f32.silver_means" (f32.const 3.0)) (f32.const 3.3027756))
(assert_return (invoke "f32.silver_means" (f32.const 4.0)) (f32.const 4.2360680))
(assert_return (invoke "f32.silver_means" (f32.const 5.0)) (f32.const 5.1925821))
(assert_return (invoke "f64.silver_means" (f64.const 0.0)) (f64.const 1.0))
(assert_return (invoke "f64.silver_means" (f64.const 1.0)) (f64.const 1.618033988749895))
(assert_return (invoke "f64.silver_means" (f64.const 2.0)) (f64.const 2.414213562373095))
(assert_return (invoke "f64.silver_means" (f64.const 3.0)) (f64.const 3.302775637731995))
(assert_return (invoke "f64.silver_means" (f64.const 4.0)) (f64.const 4.236067977499790))
(assert_return (invoke "f64.silver_means" (f64.const 5.0)) (f64.const 5.192582403567252))

;; Test that an f64 0.4 isn't double-rounded as via extended precision.
;; https://bugs.llvm.org/show_bug.cgi?id=11200

(module
  (func (export "point_four") (param $four f64) (param $ten f64) (result i32)
    (f64.lt (f64.div (get_local $four) (get_local $ten)) (f64.const 0.4)))
)

(assert_return (invoke "point_four" (f64.const 4.0) (f64.const 10.0)) (i32.const 0))

;; Test an approximation function for tau; it should produces the correctly
;; rounded result after (and only after) the expected number of iterations.

(module
  (func (export "tau") (param i32) (result f64)
    (local f64 f64 f64 f64)
    f64.const 0x0p+0
    set_local 1
    block
      get_local 0
      i32.const 1
      i32.lt_s
      br_if 0
      f64.const 0x1p+0
      set_local 2
      f64.const 0x0p+0
      set_local 3
      loop
        get_local 1
        get_local 2
        f64.const 0x1p+3
        get_local 3
        f64.const 0x1p+3
        f64.mul
        tee_local 4
        f64.const 0x1p+0
        f64.add
        f64.div
        f64.const 0x1p+2
        get_local 4
        f64.const 0x1p+2
        f64.add
        f64.div
        f64.sub
        f64.const 0x1p+1
        get_local 4
        f64.const 0x1.4p+2
        f64.add
        f64.div
        f64.sub
        f64.const 0x1p+1
        get_local 4
        f64.const 0x1.8p+2
        f64.add
        f64.div
        f64.sub
        f64.mul
        f64.add
        set_local 1
        get_local 3
        f64.const 0x1p+0
        f64.add
        set_local 3
        get_local 2
        f64.const 0x1p-4
        f64.mul
        set_local 2
        get_local 0
        i32.const -1
        i32.add
        tee_local 0
        br_if 0
      end
    end
    get_local 1
  )
)

(assert_return (invoke "tau" (i32.const 10)) (f64.const 0x1.921fb54442d14p+2))
(assert_return (invoke "tau" (i32.const 11)) (f64.const 0x1.921fb54442d18p+2))

;; Test that y < 0 ? x : (x + 1) is not folded to x + (y < 0).

(module
  (func (export "f32.no_fold_conditional_inc") (param $x f32) (param $y f32) (result f32)
    (select (get_local $x)
            (f32.add (get_local $x) (f32.const 1.0))
            (f32.lt (get_local $y) (f32.const 0.0))))
  (func (export "f64.no_fold_conditional_inc") (param $x f64) (param $y f64) (result f64)
    (select (get_local $x)
            (f64.add (get_local $x) (f64.const 1.0))
            (f64.lt (get_local $y) (f64.const 0.0))))
)

(assert_return (invoke "f32.no_fold_conditional_inc" (f32.const -0.0) (f32.const -1.0)) (f32.const -0.0))
(assert_return (invoke "f64.no_fold_conditional_inc" (f64.const -0.0) (f64.const -1.0)) (f64.const -0.0))
