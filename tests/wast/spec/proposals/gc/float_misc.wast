;; Platforms intended to run WebAssembly must support IEEE 754 arithmetic.
;; This testsuite is not currently sufficient for full IEEE 754 conformance
;; testing; platforms are currently expected to meet these requirements in
;; their own way (widely-used hardware platforms already do this).
;;
;; What this testsuite does test is that (a) the platform is basically IEEE 754
;; rather than something else entirely, (b) it's configured correctly for
;; WebAssembly (rounding direction, exception masks, precision level, subnormal
;; mode, etc.), (c) the WebAssembly implementation doesn't perform any common
;; value-changing optimizations, and (d) that the WebAssembly implementation
;; doesn't exhibit any known implementation bugs.
;;
;; This file supplements f32.wast, f64.wast, f32_bitwise.wast, f64_bitwise.wast,
;; f32_cmp.wast, and f64_cmp.wast with additional single-instruction tests
;; covering additional miscellaneous interesting cases.

(module
  (func (export "f32.add") (param $x f32) (param $y f32) (result f32) (f32.add (local.get $x) (local.get $y)))
  (func (export "f32.sub") (param $x f32) (param $y f32) (result f32) (f32.sub (local.get $x) (local.get $y)))
  (func (export "f32.mul") (param $x f32) (param $y f32) (result f32) (f32.mul (local.get $x) (local.get $y)))
  (func (export "f32.div") (param $x f32) (param $y f32) (result f32) (f32.div (local.get $x) (local.get $y)))
  (func (export "f32.sqrt") (param $x f32) (result f32) (f32.sqrt (local.get $x)))
  (func (export "f32.abs") (param $x f32) (result f32) (f32.abs (local.get $x)))
  (func (export "f32.neg") (param $x f32) (result f32) (f32.neg (local.get $x)))
  (func (export "f32.copysign") (param $x f32) (param $y f32) (result f32) (f32.copysign (local.get $x) (local.get $y)))
  (func (export "f32.ceil") (param $x f32) (result f32) (f32.ceil (local.get $x)))
  (func (export "f32.floor") (param $x f32) (result f32) (f32.floor (local.get $x)))
  (func (export "f32.trunc") (param $x f32) (result f32) (f32.trunc (local.get $x)))
  (func (export "f32.nearest") (param $x f32) (result f32) (f32.nearest (local.get $x)))
  (func (export "f32.min") (param $x f32) (param $y f32) (result f32) (f32.min (local.get $x) (local.get $y)))
  (func (export "f32.max") (param $x f32) (param $y f32) (result f32) (f32.max (local.get $x) (local.get $y)))

  (func (export "f64.add") (param $x f64) (param $y f64) (result f64) (f64.add (local.get $x) (local.get $y)))
  (func (export "f64.sub") (param $x f64) (param $y f64) (result f64) (f64.sub (local.get $x) (local.get $y)))
  (func (export "f64.mul") (param $x f64) (param $y f64) (result f64) (f64.mul (local.get $x) (local.get $y)))
  (func (export "f64.div") (param $x f64) (param $y f64) (result f64) (f64.div (local.get $x) (local.get $y)))
  (func (export "f64.sqrt") (param $x f64) (result f64) (f64.sqrt (local.get $x)))
  (func (export "f64.abs") (param $x f64) (result f64) (f64.abs (local.get $x)))
  (func (export "f64.neg") (param $x f64) (result f64) (f64.neg (local.get $x)))
  (func (export "f64.copysign") (param $x f64) (param $y f64) (result f64) (f64.copysign (local.get $x) (local.get $y)))
  (func (export "f64.ceil") (param $x f64) (result f64) (f64.ceil (local.get $x)))
  (func (export "f64.floor") (param $x f64) (result f64) (f64.floor (local.get $x)))
  (func (export "f64.trunc") (param $x f64) (result f64) (f64.trunc (local.get $x)))
  (func (export "f64.nearest") (param $x f64) (result f64) (f64.nearest (local.get $x)))
  (func (export "f64.min") (param $x f64) (param $y f64) (result f64) (f64.min (local.get $x) (local.get $y)))
  (func (export "f64.max") (param $x f64) (param $y f64) (result f64) (f64.max (local.get $x) (local.get $y)))
)

;; Miscellaneous values.
(assert_return (invoke "f32.add" (f32.const 1.1234567890) (f32.const 1.2345e-10)) (f32.const 1.123456789))
(assert_return (invoke "f64.add" (f64.const 1.1234567890) (f64.const 1.2345e-10)) (f64.const 0x1.1f9add37c11f7p+0))

;; Test adding the greatest value to 1.0 that rounds back to 1.0, and the
;; least that rounds to something greater.
(assert_return (invoke "f32.add" (f32.const 1.0) (f32.const 0x1p-24)) (f32.const 0x1.0p+0))
(assert_return (invoke "f32.add" (f32.const 1.0) (f32.const 0x1.000002p-24)) (f32.const 0x1.000002p+0))
(assert_return (invoke "f64.add" (f64.const 1.0) (f64.const 0x1p-53)) (f64.const 0x1.0p+0))
(assert_return (invoke "f64.add" (f64.const 1.0) (f64.const 0x1.0000000000001p-53)) (f64.const 0x1.0000000000001p+0))

;; Max subnormal + min subnormal = min normal.
(assert_return (invoke "f32.add" (f32.const 0x1p-149) (f32.const 0x1.fffffcp-127)) (f32.const 0x1p-126))
(assert_return (invoke "f64.add" (f64.const 0x0.0000000000001p-1022) (f64.const 0x0.fffffffffffffp-1022)) (f64.const 0x1p-1022))

;; Test for a case of double rounding, example from:
;; http://perso.ens-lyon.fr/jean-michel.muller/Handbook.html
;; section 3.3.1: A typical problem: "double rounding"
(assert_return (invoke "f32.add" (f32.const 0x1p+31) (f32.const 1024.25)) (f32.const 0x1.000008p+31))
(assert_return (invoke "f64.add" (f64.const 0x1p+63) (f64.const 1024.25)) (f64.const 0x1.0000000000001p+63))

;; Test a case that was "tricky" on MMIX.
;; http://mmix.cs.hm.edu/bugs/bug_rounding.html
(assert_return (invoke "f64.add" (f64.const -0x1p-1008) (f64.const 0x0.0000000001716p-1022)) (f64.const -0x1.fffffffffffffp-1009))

;; http://www.vinc17.org/software/tst-ieee754.xsl
(assert_return (invoke "f64.add" (f64.const 9007199254740992) (f64.const 1.00001)) (f64.const 9007199254740994))

;; http://www.vinc17.org/software/test.java
(assert_return (invoke "f64.add" (f64.const 9007199254740994) (f64.const 0x1.fffep-1)) (f64.const 9007199254740994))

;; Computations that round differently in ties-to-odd mode.
(assert_return (invoke "f32.add" (f32.const 0x1p23) (f32.const 0x1p-1)) (f32.const 0x1p23))
(assert_return (invoke "f32.add" (f32.const 0x1.000002p+23) (f32.const 0x1p-1)) (f32.const 0x1.000004p+23))
(assert_return (invoke "f64.add" (f64.const 0x1p52) (f64.const 0x1p-1)) (f64.const 0x1p52))
(assert_return (invoke "f64.add" (f64.const 0x1.0000000000001p+52) (f64.const 0x1p-1)) (f64.const 0x1.0000000000002p+52))

;; Computations that round differently in round-upward mode.
(assert_return (invoke "f32.add" (f32.const -0x1.39675ap+102) (f32.const 0x1.76c94cp-99)) (f32.const -0x1.39675ap+102))
(assert_return (invoke "f32.add" (f32.const 0x1.6c0f24p+67) (f32.const -0x1.2b92dp+52)) (f32.const 0x1.6c0cccp+67))
(assert_return (invoke "f32.add" (f32.const 0x1.e62318p-83) (f32.const 0x1.f74abep-125)) (f32.const 0x1.e62318p-83))
(assert_return (invoke "f32.add" (f32.const 0x1.2a71d4p+39) (f32.const -0x1.c9f10cp+55)) (f32.const -0x1.c9efe2p+55))
(assert_return (invoke "f32.add" (f32.const 0x1.f8f736p-15) (f32.const 0x1.7bd45ep+106)) (f32.const 0x1.7bd45ep+106))
(assert_return (invoke "f64.add" (f64.const 0x1.f33e1fbca27aap-413) (f64.const -0x1.6b192891ed61p+249)) (f64.const -0x1.6b192891ed61p+249))
(assert_return (invoke "f64.add" (f64.const -0x1.46f75d130eeb1p+76) (f64.const 0x1.25275d6f7a4acp-184)) (f64.const -0x1.46f75d130eeb1p+76))
(assert_return (invoke "f64.add" (f64.const 0x1.04dec9265a731p-148) (f64.const -0x1.11eed4e8c127cp-12)) (f64.const -0x1.11eed4e8c127cp-12))
(assert_return (invoke "f64.add" (f64.const 0x1.05773b7166b0ap+497) (f64.const 0x1.134022f2da37bp+66)) (f64.const 0x1.05773b7166b0ap+497))
(assert_return (invoke "f64.add" (f64.const 0x1.ef4f794282a82p+321) (f64.const 0x1.14a82266badep+394)) (f64.const 0x1.14a82266badep+394))

;; Computations that round differently in round-downward mode.
(assert_return (invoke "f32.add" (f32.const 0x1.1bf976p+72) (f32.const -0x1.7f5868p+20)) (f32.const 0x1.1bf976p+72))
(assert_return (invoke "f32.add" (f32.const 0x1.7f9c6cp-45) (f32.const -0x1.b9bb0ep-78)) (f32.const 0x1.7f9c6cp-45))
(assert_return (invoke "f32.add" (f32.const -0x1.32d1bcp-42) (f32.const 0x1.f7d214p+125)) (f32.const 0x1.f7d214p+125))
(assert_return (invoke "f32.add" (f32.const -0x1.8e5c0ep-44) (f32.const -0x1.3afa4cp-106)) (f32.const -0x1.8e5c0ep-44))
(assert_return (invoke "f32.add" (f32.const 0x1.13cd78p-10) (f32.const -0x1.3af316p-107)) (f32.const 0x1.13cd78p-10))
(assert_return (invoke "f64.add" (f64.const 0x1.f8dd15ca97d4ap+179) (f64.const -0x1.367317d1fe8bfp-527)) (f64.const 0x1.f8dd15ca97d4ap+179))
(assert_return (invoke "f64.add" (f64.const 0x1.5db08d739228cp+155) (f64.const -0x1.fb316fa147dcbp-61)) (f64.const 0x1.5db08d739228cp+155))
(assert_return (invoke "f64.add" (f64.const 0x1.bbb403cb85c07p-404) (f64.const -0x1.7e44046b8bbf3p-979)) (f64.const 0x1.bbb403cb85c07p-404))
(assert_return (invoke "f64.add" (f64.const -0x1.34d38af291831p+147) (f64.const -0x1.9890b47439953p+139)) (f64.const -0x1.366c1ba705bcap+147))
(assert_return (invoke "f64.add" (f64.const -0x1.b61dedf4e0306p+3) (f64.const 0x1.09e2f31773c4ap+290)) (f64.const 0x1.09e2f31773c4ap+290))

;; Computations that round differently in round-toward-zero mode.
(assert_return (invoke "f32.add" (f32.const -0x1.129bd8p-117) (f32.const 0x1.c75012p-43)) (f32.const 0x1.c75012p-43))
(assert_return (invoke "f32.add" (f32.const -0x1.c204a2p-16) (f32.const 0x1.80b132p-27)) (f32.const -0x1.c1d48cp-16))
(assert_return (invoke "f32.add" (f32.const -0x1.decc1cp+36) (f32.const 0x1.c688dap-109)) (f32.const -0x1.decc1cp+36))
(assert_return (invoke "f32.add" (f32.const 0x1.61ce6ap-118) (f32.const -0x1.772892p+30)) (f32.const -0x1.772892p+30))
(assert_return (invoke "f32.add" (f32.const -0x1.3dc826p-120) (f32.const 0x1.fc3f66p+95)) (f32.const 0x1.fc3f66p+95))
(assert_return (invoke "f64.add" (f64.const 0x1.bf68acc263a0fp-777) (f64.const -0x1.5f9352965e5a6p+1004)) (f64.const -0x1.5f9352965e5a6p+1004))
(assert_return (invoke "f64.add" (f64.const -0x1.76eaa70911f51p+516) (f64.const -0x1.2d746324ce47ap+493)) (f64.const -0x1.76eaa963fabb6p+516))
(assert_return (invoke "f64.add" (f64.const -0x1.b637d82c15a7ap-967) (f64.const 0x1.cc654ccab4152p-283)) (f64.const 0x1.cc654ccab4152p-283))
(assert_return (invoke "f64.add" (f64.const -0x1.a5b1fb66e846ep-509) (f64.const 0x1.4bdd36f0bb5ccp-860)) (f64.const -0x1.a5b1fb66e846ep-509))
(assert_return (invoke "f64.add" (f64.const -0x1.14108da880f9ep+966) (f64.const 0x1.417f35701e89fp+800)) (f64.const -0x1.14108da880f9ep+966))

;; Computations that round differently on x87.
(assert_return (invoke "f64.add" (f64.const -0x1.fa0caf21ffebcp+804) (f64.const 0x1.4ca8fdcff89f9p+826)) (f64.const 0x1.4ca8f5e7c5e31p+826))
(assert_return (invoke "f64.add" (f64.const 0x1.016f1fcbdfd38p+784) (f64.const 0x1.375dffcbc9a2cp+746)) (f64.const 0x1.016f1fcbe4b0fp+784))
(assert_return (invoke "f64.add" (f64.const -0x1.dffda6d5bff3ap+624) (f64.const 0x1.f9e8cc2dff782p+674)) (f64.const 0x1.f9e8cc2dff77bp+674))
(assert_return (invoke "f64.add" (f64.const 0x1.fff4b43687dfbp+463) (f64.const 0x1.0fd5617c4a809p+517)) (f64.const 0x1.0fd5617c4a809p+517))
(assert_return (invoke "f64.add" (f64.const 0x1.535d380035da2p-995) (f64.const 0x1.cce37dddbb73bp-963)) (f64.const 0x1.cce37ddf0ed0fp-963))

;; Computations that round differently when computed via f32.
(assert_return (invoke "f64.add" (f64.const -0x1.d91cd3fc0c66fp+752) (f64.const -0x1.4e18c80229734p+952)) (f64.const -0x1.4e18c80229734p+952))
(assert_return (invoke "f64.add" (f64.const 0x1.afc70fd36e372p+193) (f64.const -0x1.bd10a9b377b46p+273)) (f64.const -0x1.bd10a9b377b46p+273))
(assert_return (invoke "f64.add" (f64.const -0x1.2abd570b078b2p+302) (f64.const 0x1.b3c1ad759cb5bp-423)) (f64.const -0x1.2abd570b078b2p+302))
(assert_return (invoke "f64.add" (f64.const -0x1.5b2ae84c0686cp-317) (f64.const -0x1.dba7a1c022823p+466)) (f64.const -0x1.dba7a1c022823p+466))
(assert_return (invoke "f64.add" (f64.const -0x1.ac627bd7cbf38p-198) (f64.const 0x1.2312e265b8d59p-990)) (f64.const -0x1.ac627bd7cbf38p-198))

;; Computations that utilize the maximum exponent value to avoid overflow.
(assert_return (invoke "f32.add" (f32.const 0x1.2b91ap+116) (f32.const 0x1.cbcd52p+127)) (f32.const 0x1.cbf2c4p+127))
(assert_return (invoke "f32.add" (f32.const 0x1.96f392p+127) (f32.const -0x1.6b3fecp+107)) (f32.const 0x1.96f37cp+127))
(assert_return (invoke "f32.add" (f32.const 0x1.132f1cp+118) (f32.const -0x1.63d632p+127)) (f32.const -0x1.634c9ap+127))
(assert_return (invoke "f32.add" (f32.const -0x1.1dda64p+120) (f32.const -0x1.ef02ep+127)) (f32.const -0x1.f13e94p+127))
(assert_return (invoke "f32.add" (f32.const -0x1.4ad8dap+127) (f32.const -0x1.eae082p+125)) (f32.const -0x1.c590fap+127))
(assert_return (invoke "f64.add" (f64.const 0x1.017099f2a4b8bp+1023) (f64.const 0x1.1f63b28f05454p+981)) (f64.const 0x1.017099f2a5009p+1023))
(assert_return (invoke "f64.add" (f64.const 0x1.d88b6c74984efp+1023) (f64.const 0x1.33b444775eabcp+990)) (f64.const 0x1.d88b6c7532291p+1023))
(assert_return (invoke "f64.add" (f64.const -0x1.84576422fdf5p+1023) (f64.const 0x1.60ee6aa12fb9cp+1012)) (f64.const -0x1.842b4655a9cf1p+1023))
(assert_return (invoke "f64.add" (f64.const -0x1.9aaace3e79f7dp+1001) (f64.const 0x1.e4068af295cb6p+1023)) (f64.const 0x1.e4068487ea926p+1023))
(assert_return (invoke "f64.add" (f64.const 0x1.06cdae79f27b9p+1023) (f64.const -0x1.e05cb0c96f975p+991)) (f64.const 0x1.06cdae78121eep+1023))

;; Computations that utilize the minimum exponent value.
(assert_return (invoke "f32.add" (f32.const 0x1.6a1a2p-127) (f32.const 0x1.378p-140)) (f32.const 0x1.6a23dcp-127))
(assert_return (invoke "f32.add" (f32.const 0x1.28p-144) (f32.const -0x1p-148)) (f32.const 0x1.18p-144))
(assert_return (invoke "f32.add" (f32.const -0x1p-146) (f32.const 0x1.c3cap-128)) (f32.const 0x1.c3c9cp-128))
(assert_return (invoke "f32.add" (f32.const -0x1.4p-145) (f32.const 0x1.424052p-122)) (f32.const 0x1.42405p-122))
(assert_return (invoke "f32.add" (f32.const 0x1.c5p-141) (f32.const -0x1.72f8p-135)) (f32.const -0x1.6be4p-135))
(assert_return (invoke "f64.add" (f64.const 0x1.4774c681d1e21p-1022) (f64.const -0x1.271e58e9f58cap-1021)) (f64.const -0x1.06c7eb5219373p-1022))
(assert_return (invoke "f64.add" (f64.const 0x1.10b3a75e31916p-1021) (f64.const -0x1.ffb82b0e868a7p-1021)) (f64.const -0x1.de090760a9f22p-1022))
(assert_return (invoke "f64.add" (f64.const -0x0.6b58448b8098ap-1022) (f64.const -0x1.579796ed04cbep-1022)) (f64.const -0x1.c2efdb7885648p-1022))
(assert_return (invoke "f64.add" (f64.const 0x1.9eb9e7baae8d1p-1020) (f64.const -0x1.d58e136f8c6eep-1020)) (f64.const -0x0.db50aed377874p-1022))
(assert_return (invoke "f64.add" (f64.const -0x1.f1115deeafa0bp-1022) (f64.const 0x1.221b1c87dca29p-1022)) (f64.const -0x0.cef64166d2fe2p-1022))

;; Test an add of the second-greatest finite value with the distance to greatest
;; finite value.
(assert_return (invoke "f32.add" (f32.const 0x1.fffffcp+127) (f32.const 0x1p+104)) (f32.const 0x1.fffffep+127))
(assert_return (invoke "f64.add" (f64.const 0x1.ffffffffffffep+1023) (f64.const 0x1p+971)) (f64.const 0x1.fffffffffffffp+1023))

;; http://news.harvard.edu/gazette/story/2013/09/dawn-of-a-revolution/
(assert_return (invoke "f32.add" (f32.const 2.0) (f32.const 2.0)) (f32.const 4.0))
(assert_return (invoke "f64.add" (f64.const 2.0) (f64.const 2.0)) (f64.const 4.0))

;; Test rounding above the greatest finite value.
(assert_return (invoke "f32.add" (f32.const 0x1.fffffep+127) (f32.const 0x1.fffffep+102)) (f32.const 0x1.fffffep+127))
(assert_return (invoke "f32.add" (f32.const 0x1.fffffep+127) (f32.const 0x1p+103)) (f32.const inf))
(assert_return (invoke "f64.add" (f64.const 0x1.fffffffffffffp+1023) (f64.const 0x1.fffffffffffffp+969)) (f64.const 0x1.fffffffffffffp+1023))
(assert_return (invoke "f64.add" (f64.const 0x1.fffffffffffffp+1023) (f64.const 0x1p+970)) (f64.const inf))

;; Test for a historic spreadsheet bug.
;; https://blogs.office.com/2007/09/25/calculation-issue-update/
(assert_return (invoke "f32.sub" (f32.const 65536.0) (f32.const 0x1p-37)) (f32.const 65536.0))
(assert_return (invoke "f64.sub" (f64.const 65536.0) (f64.const 0x1p-37)) (f64.const 0x1.fffffffffffffp+15))

;; Test subtracting the greatest value from 1.0 that rounds back to 1.0, and the
;; least that rounds to something less.
(assert_return (invoke "f32.sub" (f32.const 1.0) (f32.const 0x1p-25)) (f32.const 0x1.0p+0))
(assert_return (invoke "f32.sub" (f32.const 1.0) (f32.const 0x1.000002p-25)) (f32.const 0x1.fffffep-1))
(assert_return (invoke "f64.sub" (f64.const 1.0) (f64.const 0x1p-54)) (f64.const 0x1.0p+0))
(assert_return (invoke "f64.sub" (f64.const 1.0) (f64.const 0x1.0000000000001p-54)) (f64.const 0x1.fffffffffffffp-1))

;; Computations that round differently in round-upward mode.
(assert_return (invoke "f32.sub" (f32.const 0x1.ee2466p-106) (f32.const -0x1.16277ep+119)) (f32.const 0x1.16277ep+119))
(assert_return (invoke "f32.sub" (f32.const -0x1.446f9ep+119) (f32.const -0x1.4396a4p+43)) (f32.const -0x1.446f9ep+119))
(assert_return (invoke "f32.sub" (f32.const 0x1.74773cp+0) (f32.const -0x1.a25512p-82)) (f32.const 0x1.74773cp+0))
(assert_return (invoke "f32.sub" (f32.const 0x1.9345c4p-117) (f32.const 0x1.6792c2p-76)) (f32.const -0x1.6792c2p-76))
(assert_return (invoke "f32.sub" (f32.const 0x1.9ecfa4p-18) (f32.const -0x1.864b44p-107)) (f32.const 0x1.9ecfa4p-18))
(assert_return (invoke "f64.sub" (f64.const -0x1.5b798875e7845p-333) (f64.const -0x1.b5147117452fep-903)) (f64.const -0x1.5b798875e7845p-333))
(assert_return (invoke "f64.sub" (f64.const -0x1.6c87baeb6d72dp+552) (f64.const -0x1.64fb35d4b5571p-158)) (f64.const -0x1.6c87baeb6d72dp+552))
(assert_return (invoke "f64.sub" (f64.const 0x1.b3d369fcf74bp-461) (f64.const -0x1.ea1668c0dec93p-837)) (f64.const 0x1.b3d369fcf74bp-461))
(assert_return (invoke "f64.sub" (f64.const 0x1.0abd449353eadp-1005) (f64.const -0x1.0422ea3e82ee9p+154)) (f64.const 0x1.0422ea3e82ee9p+154))
(assert_return (invoke "f64.sub" (f64.const -0x1.aadbc6b43cc3dp-143) (f64.const -0x1.e7f922ef1ee58p-539)) (f64.const -0x1.aadbc6b43cc3dp-143))

;; Computations that round differently in round-downward mode.
(assert_return (invoke "f32.sub" (f32.const -0x1.61e262p+108) (f32.const -0x1.baf3e4p+112)) (f32.const 0x1.a4d5bep+112))
(assert_return (invoke "f32.sub" (f32.const -0x1.62c2f6p+109) (f32.const 0x1.6e514ap+6)) (f32.const -0x1.62c2f6p+109))
(assert_return (invoke "f32.sub" (f32.const -0x1.287c94p-83) (f32.const 0x1.0f2f9cp-24)) (f32.const -0x1.0f2f9cp-24))
(assert_return (invoke "f32.sub" (f32.const -0x1.c8825cp-77) (f32.const -0x1.4aead6p-12)) (f32.const 0x1.4aead6p-12))
(assert_return (invoke "f32.sub" (f32.const -0x1.2976a4p+99) (f32.const 0x1.c6e3b8p-59)) (f32.const -0x1.2976a4p+99))
(assert_return (invoke "f64.sub" (f64.const -0x1.76cb28ae6c045p+202) (f64.const -0x1.0611f2af4e9b9p+901)) (f64.const 0x1.0611f2af4e9b9p+901))
(assert_return (invoke "f64.sub" (f64.const 0x1.baf35eff22e9ep-368) (f64.const 0x1.5c3e08ecf73ecp-451)) (f64.const 0x1.baf35eff22e9ep-368))
(assert_return (invoke "f64.sub" (f64.const -0x1.8fd354b376f1fp-200) (f64.const 0x1.513c860f386ffp-508)) (f64.const -0x1.8fd354b376f1fp-200))
(assert_return (invoke "f64.sub" (f64.const -0x1.760d447230ae6p-992) (f64.const -0x1.16f788438ae3ep-328)) (f64.const 0x1.16f788438ae3ep-328))
(assert_return (invoke "f64.sub" (f64.const -0x1.73aab4fcfc7ap+112) (f64.const 0x1.7c589f990b884p+171)) (f64.const -0x1.7c589f990b884p+171))

;; Computations that round differently in round-toward-zero mode.
(assert_return (invoke "f32.sub" (f32.const 0x1.ea264cp+95) (f32.const 0x1.852988p-15)) (f32.const 0x1.ea264cp+95))
(assert_return (invoke "f32.sub" (f32.const -0x1.14ec7cp+19) (f32.const -0x1.0ad3fep-35)) (f32.const -0x1.14ec7cp+19))
(assert_return (invoke "f32.sub" (f32.const -0x1.3251dap-36) (f32.const -0x1.49c97ep-56)) (f32.const -0x1.3251c6p-36))
(assert_return (invoke "f32.sub" (f32.const -0x1.13565ep-14) (f32.const 0x1.2f89a8p-13)) (f32.const -0x1.b934d8p-13))
(assert_return (invoke "f32.sub" (f32.const -0x1.6032b6p-33) (f32.const -0x1.bb5196p-104)) (f32.const -0x1.6032b6p-33))
(assert_return (invoke "f64.sub" (f64.const -0x1.b5b0797af491p-157) (f64.const -0x1.694b8348189e8p+722)) (f64.const 0x1.694b8348189e8p+722))
(assert_return (invoke "f64.sub" (f64.const -0x1.72b142826ed73p+759) (f64.const -0x1.010477bc9afbdp+903)) (f64.const 0x1.010477bc9afbdp+903))
(assert_return (invoke "f64.sub" (f64.const 0x1.83273b6bb94cfp-796) (f64.const 0x1.1a93f948a2abbp+181)) (f64.const -0x1.1a93f948a2abbp+181))
(assert_return (invoke "f64.sub" (f64.const -0x1.207e7156cbf2p-573) (f64.const 0x1.cf3f12fd3814dp-544)) (f64.const -0x1.cf3f13063c086p-544))
(assert_return (invoke "f64.sub" (f64.const -0x1.837e6844f1718p-559) (f64.const -0x1.1c29b757f98abp-14)) (f64.const 0x1.1c29b757f98abp-14))

;; Computations that round differently on x87.
(assert_return (invoke "f64.sub" (f64.const 0x1.c21151a709b6cp-78) (f64.const 0x1.0a12fff8910f6p-115)) (f64.const 0x1.c21151a701663p-78))
(assert_return (invoke "f64.sub" (f64.const 0x1.c57912aae2f64p-982) (f64.const 0x1.dbfbd4800b7cfp-1010)) (f64.const 0x1.c579128d2338fp-982))
(assert_return (invoke "f64.sub" (f64.const 0x1.ffef4399af9c6p-254) (f64.const 0x1.edb96dfaea8b1p-200)) (f64.const -0x1.edb96dfaea8b1p-200))
(assert_return (invoke "f64.sub" (f64.const -0x1.363eee391cde2p-39) (f64.const -0x1.a65462000265fp-69)) (f64.const -0x1.363eee32838c9p-39))
(assert_return (invoke "f64.sub" (f64.const 0x1.59016dba002a1p-25) (f64.const 0x1.5d4374f124cccp-3)) (f64.const -0x1.5d436f8d1f15dp-3))

;; Computations that round differently when computed via f32.
(assert_return (invoke "f64.sub" (f64.const -0x1.18196bca005cfp-814) (f64.const -0x1.db7b01ce3f52fp-766)) (f64.const 0x1.db7b01ce3f51dp-766))
(assert_return (invoke "f64.sub" (f64.const -0x1.d17b3528d219p+33) (f64.const 0x1.fd739d4ea220ap+367)) (f64.const -0x1.fd739d4ea220ap+367))
(assert_return (invoke "f64.sub" (f64.const 0x1.dea46994de319p+114) (f64.const 0x1.b5b19cd55c7d3p-590)) (f64.const 0x1.dea46994de319p+114))
(assert_return (invoke "f64.sub" (f64.const 0x1.b60f9b2fbd9ecp-489) (f64.const -0x1.6f81c59ec5b8ep-694)) (f64.const 0x1.b60f9b2fbd9ecp-489))
(assert_return (invoke "f64.sub" (f64.const 0x1.5e423fe8571f4p-57) (f64.const 0x1.9624ed7c162dfp-618)) (f64.const 0x1.5e423fe8571f4p-57))

;; pow(e, π) - π
;; https://xkcd.com/217/
(assert_return (invoke "f32.sub" (f32.const 0x1.724046p+4) (f32.const 0x1.921fb6p+1)) (f32.const 0x1.3ffc5p+4))
(assert_return (invoke "f64.sub" (f64.const 0x1.724046eb0933ap+4) (f64.const 0x1.921fb54442d18p+1)) (f64.const 0x1.3ffc504280d97p+4))

;; https://www.cnet.com/news/googles-calculator-muffs-some-math-problems/
(assert_return (invoke "f32.sub" (f32.const 2999999) (f32.const 2999998)) (f32.const 1.0))
(assert_return (invoke "f32.sub" (f32.const 1999999) (f32.const 1999995)) (f32.const 4.0))
(assert_return (invoke "f32.sub" (f32.const 1999999) (f32.const 1999993)) (f32.const 6.0))
(assert_return (invoke "f32.sub" (f32.const 400002) (f32.const 400001)) (f32.const 1.0))
(assert_return (invoke "f32.sub" (f32.const 400002) (f32.const 400000)) (f32.const 2.0))
(assert_return (invoke "f64.sub" (f64.const 2999999999999999) (f64.const 2999999999999998)) (f64.const 1.0))
(assert_return (invoke "f64.sub" (f64.const 1999999999999999) (f64.const 1999999999999995)) (f64.const 4.0))
(assert_return (invoke "f64.sub" (f64.const 1999999999999999) (f64.const 1999999999999993)) (f64.const 6.0))
(assert_return (invoke "f64.sub" (f64.const 400000000000002) (f64.const 400000000000001)) (f64.const 1.0))
(assert_return (invoke "f64.sub" (f64.const 400000000000002) (f64.const 400000000000000)) (f64.const 2.0))

;; Min normal - max subnormal = min subnormal.
(assert_return (invoke "f32.sub" (f32.const 0x1p-126) (f32.const 0x1.fffffcp-127)) (f32.const 0x1p-149))
(assert_return (invoke "f64.sub" (f64.const 0x1p-1022) (f64.const 0x0.fffffffffffffp-1022)) (f64.const 0x0.0000000000001p-1022))

;; Test subtraction of numbers very close to 1.
(assert_return (invoke "f32.sub" (f32.const 0x1.000002p+0) (f32.const 0x1.fffffep-1)) (f32.const 0x1.8p-23))
(assert_return (invoke "f32.sub" (f32.const 0x1.000002p+0) (f32.const 0x1.0p+0)) (f32.const 0x1p-23))
(assert_return (invoke "f32.sub" (f32.const 0x1p+0) (f32.const 0x1.fffffep-1)) (f32.const 0x1p-24))
(assert_return (invoke "f64.sub" (f64.const 0x1.0000000000001p+0) (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1.8p-52))
(assert_return (invoke "f64.sub" (f64.const 0x1.0000000000001p+0) (f64.const 0x1.0p+0)) (f64.const 0x1p-52))
(assert_return (invoke "f64.sub" (f64.const 0x1p+0) (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1p-53))

;; Test the least value that can be subtracted from the max value to produce a
;; different value.
(assert_return (invoke "f32.sub" (f32.const 0x1.fffffep+127) (f32.const 0x1.fffffep+102)) (f32.const 0x1.fffffep+127))
(assert_return (invoke "f32.sub" (f32.const 0x1.fffffep+127) (f32.const 0x1p+103)) (f32.const 0x1.fffffcp+127))
(assert_return (invoke "f64.sub" (f64.const 0x1.fffffffffffffp+1023) (f64.const 0x1.fffffffffffffp+969)) (f64.const 0x1.fffffffffffffp+1023))
(assert_return (invoke "f64.sub" (f64.const 0x1.fffffffffffffp+1023) (f64.const 0x1p+970)) (f64.const 0x1.ffffffffffffep+1023))

;; Miscellaneous values.
(assert_return (invoke "f32.mul" (f32.const 1e15) (f32.const 1e15)) (f32.const 0x1.93e592p+99))
(assert_return (invoke "f32.mul" (f32.const 1e20) (f32.const 1e20)) (f32.const inf))
(assert_return (invoke "f32.mul" (f32.const 1e25) (f32.const 1e25)) (f32.const inf))
(assert_return (invoke "f64.mul" (f64.const 1e15) (f64.const 1e15)) (f64.const 0x1.93e5939a08ceap+99))
(assert_return (invoke "f64.mul" (f64.const 1e20) (f64.const 1e20)) (f64.const 0x1.d6329f1c35ca5p+132))
(assert_return (invoke "f64.mul" (f64.const 1e25) (f64.const 1e25)) (f64.const 0x1.11b0ec57e649bp+166))

;; Test for a case of double rounding, example from:
;; http://perso.ens-lyon.fr/jean-michel.muller/Handbook.html
;; section 3.3.1: A typical problem: "double rounding"
(assert_return (invoke "f32.mul" (f32.const 1848874880.0) (f32.const 19954563072.0)) (f32.const 0x1.000002p+65))
(assert_return (invoke "f64.mul" (f64.const 1848874847.0) (f64.const 19954562207.0)) (f64.const 3.6893488147419111424e+19))

;; Test for a historic spreadsheet bug.
;; http://www.joelonsoftware.com/items/2007/09/26b.html
(assert_return (invoke "f32.mul" (f32.const 77.1) (f32.const 850)) (f32.const 65535))
(assert_return (invoke "f64.mul" (f64.const 77.1) (f64.const 850)) (f64.const 65534.99999999999272404))

;; Computations that round differently in round-upward mode.
(assert_return (invoke "f32.mul" (f32.const -0x1.14df2ep+61) (f32.const 0x1.748878p-36)) (f32.const -0x1.92e7e8p+25))
(assert_return (invoke "f32.mul" (f32.const -0x1.5629e2p+102) (f32.const -0x1.c33012p-102)) (f32.const 0x1.2d8604p+1))
(assert_return (invoke "f32.mul" (f32.const -0x1.b17694p+92) (f32.const -0x1.e4b56ap-97)) (f32.const 0x1.9a5baep-4))
(assert_return (invoke "f32.mul" (f32.const -0x1.1626a6p+79) (f32.const -0x1.c57d7p-75)) (f32.const 0x1.ecbaaep+4))
(assert_return (invoke "f32.mul" (f32.const 0x1.7acf72p+53) (f32.const 0x1.6c89acp+5)) (f32.const 0x1.0db556p+59))
(assert_return (invoke "f64.mul" (f64.const -0x1.25c293f6f37e4p+425) (f64.const 0x1.f5fd4fa41c6d8p+945)) (f64.const -inf))
(assert_return (invoke "f64.mul" (f64.const -0x1.cc1ae79fffc5bp-986) (f64.const -0x1.c36ccc2861ca6p-219)) (f64.const 0x0p+0))
(assert_return (invoke "f64.mul" (f64.const 0x1.c0232b3e64b56p+606) (f64.const -0x1.f6939cf3affaap+106)) (f64.const -0x1.b7e3aedf190d3p+713))
(assert_return (invoke "f64.mul" (f64.const -0x1.60f289966b271p-313) (f64.const 0x1.28a5497f0c259p+583)) (f64.const -0x1.98fc50bcec259p+270))
(assert_return (invoke "f64.mul" (f64.const 0x1.37dab12d3afa2p+795) (f64.const 0x1.81e156bd393f1p-858)) (f64.const 0x1.d6126554b8298p-63))

;; Computations that round differently in round-downward mode.
(assert_return (invoke "f32.mul" (f32.const -0x1.3f57a2p-89) (f32.const -0x1.041d68p+92)) (f32.const 0x1.4479bp+3))
(assert_return (invoke "f32.mul" (f32.const 0x1.4d0582p+73) (f32.const 0x1.6e043ap+19)) (f32.const 0x1.dc236p+92))
(assert_return (invoke "f32.mul" (f32.const -0x1.2fdap-32) (f32.const -0x1.e1731cp+74)) (f32.const 0x1.1db89ep+43))
(assert_return (invoke "f32.mul" (f32.const 0x1.7bc8fep+67) (f32.const -0x1.3ad592p+15)) (f32.const -0x1.d3115ep+82))
(assert_return (invoke "f32.mul" (f32.const 0x1.936742p+30) (f32.const -0x1.a7a19p+66)) (f32.const -0x1.4dc71ap+97))
(assert_return (invoke "f64.mul" (f64.const -0x1.ba737b4ca3b13p-639) (f64.const 0x1.8923309857438p-314)) (f64.const -0x1.53bc0d07baa37p-952))
(assert_return (invoke "f64.mul" (f64.const 0x1.7c1932e610219p-276) (f64.const -0x1.2605db646489fp-635)) (f64.const -0x1.b48da2b0d2ae3p-911))
(assert_return (invoke "f64.mul" (f64.const -0x1.e43cdf3b2108p+329) (f64.const -0x1.99d96abbd61d1p+835)) (f64.const inf))
(assert_return (invoke "f64.mul" (f64.const 0x1.4c19466551da3p+947) (f64.const 0x1.0bdcd6c7646e9p-439)) (f64.const 0x1.5b7cd8c3f638ap+508))
(assert_return (invoke "f64.mul" (f64.const 0x1.ff1da1726e3dfp+339) (f64.const -0x1.043c44f52b158p+169)) (f64.const -0x1.03c9364bb585cp+509))

;; Computations that round differently in round-toward-zero mode.
(assert_return (invoke "f32.mul" (f32.const -0x1.907e8ap+46) (f32.const -0x1.5d3668p+95)) (f32.const inf))
(assert_return (invoke "f32.mul" (f32.const -0x1.8c9f74p-3) (f32.const 0x1.e2b452p-99)) (f32.const -0x1.75edccp-101))
(assert_return (invoke "f32.mul" (f32.const -0x1.cc605ap-19) (f32.const 0x1.ec321ap+105)) (f32.const -0x1.ba91a4p+87))
(assert_return (invoke "f32.mul" (f32.const -0x1.5fbb7ap+56) (f32.const 0x1.a8965ep-96)) (f32.const -0x1.23ae8ep-39))
(assert_return (invoke "f32.mul" (f32.const -0x1.fb7f12p+16) (f32.const 0x1.3a701ap-119)) (f32.const -0x1.37ac0cp-102))
(assert_return (invoke "f64.mul" (f64.const -0x1.5b0266454c26bp-496) (f64.const -0x1.af5787e3e0399p+433)) (f64.const 0x1.2457d81949e0bp-62))
(assert_return (invoke "f64.mul" (f64.const 0x1.0d54a82393d45p+478) (f64.const -0x1.425760807ceaep-764)) (f64.const -0x1.532068c8d0d5dp-286))
(assert_return (invoke "f64.mul" (f64.const -0x1.b532af981786p+172) (f64.const 0x1.ada95085ba36fp+359)) (f64.const -0x1.6ee38c1e01864p+532))
(assert_return (invoke "f64.mul" (f64.const 0x1.e132f4d49d1cep+768) (f64.const -0x1.a75afe9a7d864p+374)) (f64.const -inf))
(assert_return (invoke "f64.mul" (f64.const 0x1.68bbf1cfff90ap+81) (f64.const 0x1.09cd17d652c5p+70)) (f64.const 0x1.768b8d67d794p+151))

;; Computations that round differently on x87.
(assert_return (invoke "f64.mul" (f64.const 0x1.f99fb602c89b7p-341) (f64.const 0x1.6caab46a31a2ep-575)) (f64.const 0x1.68201f986e9d7p-915))
(assert_return (invoke "f64.mul" (f64.const -0x1.86999c5eee379p-9) (f64.const 0x1.6e3b9e0d53e0dp+723)) (f64.const -0x1.17654a0ef35f5p+715))
(assert_return (invoke "f64.mul" (f64.const -0x1.069571b176f9p+367) (f64.const -0x1.e248b6ab0a0e3p-652)) (f64.const 0x1.eeaff575cae1dp-285))
(assert_return (invoke "f64.mul" (f64.const 0x1.c217645777dd2p+775) (f64.const 0x1.d93f5715dd646p+60)) (f64.const 0x1.a0064aa1d920dp+836))
(assert_return (invoke "f64.mul" (f64.const -0x1.848981b6e694ap-276) (f64.const 0x1.f5aacb64a0d19p+896)) (f64.const -0x1.7cb2296e6c2e5p+621))

;; Computations that round differently on x87 in double-precision mode.
(assert_return (invoke "f64.mul" (f64.const 0x1.db3bd2a286944p-599) (f64.const 0x1.ce910af1d55cap-425)) (f64.const 0x0.d6accdd538a39p-1022))
(assert_return (invoke "f64.mul" (f64.const -0x1.aca223916012p-57) (f64.const -0x1.2b2b4958dd228p-966)) (f64.const 0x0.fa74eccae5615p-1022))
(assert_return (invoke "f64.mul" (f64.const -0x1.bd062def16cffp-488) (f64.const -0x1.7ddd91a0c4c0ep-536)) (f64.const 0x0.a5f4d7769d90dp-1022))
(assert_return (invoke "f64.mul" (f64.const -0x1.c6a56169e9cep-772) (f64.const 0x1.517d55a474122p-255)) (f64.const -0x0.12baf260afb77p-1022))
(assert_return (invoke "f64.mul" (f64.const -0x1.08951b0b41705p-516) (f64.const -0x1.102dc27168d09p-507)) (f64.const 0x0.8ca6dbf3f592bp-1022))

;; Computations that round differently when computed via f32.
(assert_return (invoke "f64.mul" (f64.const 0x1.8d0dea50c8c9bp+852) (f64.const 0x1.21cac31d87a24p-881)) (f64.const 0x1.c177311f7cd73p-29))
(assert_return (invoke "f64.mul" (f64.const 0x1.98049118e3063p-7) (f64.const 0x1.6362525151b58p-149)) (f64.const 0x1.1b358514103f9p-155))
(assert_return (invoke "f64.mul" (f64.const -0x1.ea65cb0631323p+1) (f64.const 0x1.fce683201a19bp-41)) (f64.const -0x1.e76dc8c223667p-39))
(assert_return (invoke "f64.mul" (f64.const 0x1.e4d235961d543p-373) (f64.const 0x1.bc56f20ef9a48p-205)) (f64.const 0x1.a4c09efcb71d6p-577))
(assert_return (invoke "f64.mul" (f64.const -0x1.b9612e66faba8p+77) (f64.const 0x1.e2bc6aa782273p-348)) (f64.const -0x1.a026ea4f81db1p-270))

;; Test the least positive value with a positive square.
(assert_return (invoke "f32.mul" (f32.const 0x1p-75) (f32.const 0x1p-75)) (f32.const 0x0p+0))
(assert_return (invoke "f32.mul" (f32.const 0x1.000002p-75) (f32.const 0x1.000002p-75)) (f32.const 0x1p-149))
(assert_return (invoke "f64.mul" (f64.const 0x1.6a09e667f3bccp-538) (f64.const 0x1.6a09e667f3bccp-538)) (f64.const 0x0p+0))
(assert_return (invoke "f64.mul" (f64.const 0x1.6a09e667f3bcdp-538) (f64.const 0x1.6a09e667f3bcdp-538)) (f64.const 0x0.0000000000001p-1022))

;; Test the greatest positive value with a finite square.
(assert_return (invoke "f32.mul" (f32.const 0x1.fffffep+63) (f32.const 0x1.fffffep+63)) (f32.const 0x1.fffffcp+127))
(assert_return (invoke "f32.mul" (f32.const 0x1p+64) (f32.const 0x1p+64)) (f32.const inf))
(assert_return (invoke "f64.mul" (f64.const 0x1.fffffffffffffp+511) (f64.const 0x1.fffffffffffffp+511)) (f64.const 0x1.ffffffffffffep+1023))
(assert_return (invoke "f64.mul" (f64.const 0x1p+512) (f64.const 0x1p+512)) (f64.const inf))

;; Test the squares of values very close to 1.
(assert_return (invoke "f32.mul" (f32.const 0x1.000002p+0) (f32.const 0x1.000002p+0)) (f32.const 0x1.000004p+0))
(assert_return (invoke "f32.mul" (f32.const 0x1.fffffep-1) (f32.const 0x1.fffffep-1)) (f32.const 0x1.fffffcp-1))
(assert_return (invoke "f64.mul" (f64.const 0x1.0000000000001p+0) (f64.const 0x1.0000000000001p+0)) (f64.const 0x1.0000000000002p+0))
(assert_return (invoke "f64.mul" (f64.const 0x1.fffffffffffffp-1) (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1.ffffffffffffep-1))

;; Test multiplication of numbers very close to 1.
(assert_return (invoke "f32.mul" (f32.const 0x1.000002p+0) (f32.const 0x1.fffffep-1)) (f32.const 0x1p+0))
(assert_return (invoke "f32.mul" (f32.const 0x1.000004p+0) (f32.const 0x1.fffffcp-1)) (f32.const 0x1.000002p+0))
(assert_return (invoke "f64.mul" (f64.const 0x1.0000000000001p+0) (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1p+0))
(assert_return (invoke "f64.mul" (f64.const 0x1.0000000000002p+0) (f64.const 0x1.ffffffffffffep-1)) (f64.const 0x1.0000000000001p+0))

;; Test MIN * EPSILON.
;; http://www.mpfr.org/mpfr-2.0.1/patch2
(assert_return (invoke "f32.mul" (f32.const 0x1p-126) (f32.const 0x1p-23)) (f32.const 0x1p-149))
(assert_return (invoke "f64.mul" (f64.const 0x1p-1022) (f64.const 0x1p-52)) (f64.const 0x0.0000000000001p-1022))

;; http://opencores.org/bug,view,2454
(assert_return (invoke "f32.mul" (f32.const -0x1.0006p+4) (f32.const 0x1.ap-132)) (f32.const -0x1.a009cp-128))

;; Miscellaneous values.
(assert_return (invoke "f32.div" (f32.const 1.123456789) (f32.const 100)) (f32.const 0x1.702264p-7))
(assert_return (invoke "f32.div" (f32.const 8391667.0) (f32.const 12582905.0)) (f32.const 0x1.55754p-1))
(assert_return (invoke "f32.div" (f32.const 65536.0) (f32.const 0x1p-37)) (f32.const 0x1p+53))
(assert_return (invoke "f32.div" (f32.const 0x1.dcbf6ap+0) (f32.const 0x1.fffffep+127)) (f32.const 0x1.dcbf68p-128))
(assert_return (invoke "f32.div" (f32.const 4) (f32.const 3)) (f32.const 0x1.555556p+0))
(assert_return (invoke "f64.div" (f64.const 1.123456789) (f64.const 100)) (f64.const 0.01123456789))
(assert_return (invoke "f64.div" (f64.const 8391667.0) (f64.const 12582905.0)) (f64.const 0x1.55753f1d9ba27p-1))
(assert_return (invoke "f64.div" (f64.const 65536.0) (f64.const 0x1p-37)) (f64.const 0x1p+53))
(assert_return (invoke "f64.div" (f64.const 0x1.dcbf6ap+0) (f64.const 0x1.fffffffffffffp+1023)) (f64.const 0x0.772fda8p-1022))
(assert_return (invoke "f64.div" (f64.const 4) (f64.const 3)) (f64.const 0x1.5555555555555p+0))

;; Test for a historic hardware bug.
;; https://en.wikipedia.org/wiki/Pentium_FDIV_bug
(assert_return (invoke "f32.div" (f32.const 4195835) (f32.const 3145727)) (f32.const 0x1.557542p+0))
(assert_return (invoke "f64.div" (f64.const 4195835) (f64.const 3145727)) (f64.const 0x1.557541c7c6b43p+0))

;; Computations that round differently in round-upward mode.
(assert_return (invoke "f32.div" (f32.const 0x1.6a6c5ap-48) (f32.const 0x1.fa0b7p+127)) (f32.const 0x0p+0))
(assert_return (invoke "f32.div" (f32.const 0x1.616fb2p-87) (f32.const 0x1.332172p+68)) (f32.const 0x0p+0))
(assert_return (invoke "f32.div" (f32.const -0x1.96e778p+16) (f32.const 0x1.eb0c56p-80)) (f32.const -0x1.a8440ap+95))
(assert_return (invoke "f32.div" (f32.const -0x1.e2624p-76) (f32.const -0x1.ed236ep-122)) (f32.const 0x1.f4d584p+45))
(assert_return (invoke "f32.div" (f32.const -0x1.e2374ep+41) (f32.const 0x1.71fcdcp-80)) (f32.const -0x1.4da706p+121))
(assert_return (invoke "f64.div" (f64.const 0x1.163c09d0c38c1p+147) (f64.const 0x1.e04cc737348e6p+223)) (f64.const 0x1.289921caeed23p-77))
(assert_return (invoke "f64.div" (f64.const 0x1.d6867e741e0a9p-626) (f64.const 0x1.335eb19a9aae4p-972)) (f64.const 0x1.87e342d11f519p+346))
(assert_return (invoke "f64.div" (f64.const -0x1.d5edf648aeb98p+298) (f64.const 0x1.0dda15b079355p+640)) (f64.const -0x1.bdceaf9734b5cp-342))
(assert_return (invoke "f64.div" (f64.const -0x1.b683e3934aedap+691) (f64.const 0x1.c364e1df00dffp+246)) (f64.const -0x1.f16456e7afe3bp+444))
(assert_return (invoke "f64.div" (f64.const -0x1.44ca7539cc851p+540) (f64.const 0x1.58501bccc58fep+453)) (f64.const -0x1.e2f8657e0924ep+86))

;; Computations that round differently in round-downward mode.
(assert_return (invoke "f32.div" (f32.const -0x1.c2c54ap+69) (f32.const -0x1.00d142p-86)) (f32.const inf))
(assert_return (invoke "f32.div" (f32.const 0x1.e35abep-46) (f32.const 0x1.c69dfp+44)) (f32.const 0x1.102eb4p-90))
(assert_return (invoke "f32.div" (f32.const 0x1.45ff2ap+0) (f32.const -0x1.1e8754p+89)) (f32.const -0x1.23434ep-89))
(assert_return (invoke "f32.div" (f32.const 0x1.8db18ap-51) (f32.const 0x1.47c678p-128)) (f32.const 0x1.369b96p+77))
(assert_return (invoke "f32.div" (f32.const 0x1.78599p+90) (f32.const 0x1.534144p+87)) (f32.const 0x1.1bfddcp+3))
(assert_return (invoke "f64.div" (f64.const 0x0.f331c4f47eb51p-1022) (f64.const -0x1.c7ff45bf6f03ap+362)) (f64.const -0x0p+0))
(assert_return (invoke "f64.div" (f64.const -0x1.0fc8707b9d19cp-987) (f64.const 0x1.77524d5f4a563p-536)) (f64.const -0x1.72c1a937d231p-452))
(assert_return (invoke "f64.div" (f64.const -0x1.edb3aa64bb338p-403) (f64.const -0x1.1c7c164320e4p+45)) (f64.const 0x1.bc44cc1c5ae63p-448))
(assert_return (invoke "f64.div" (f64.const -0x1.6534b34e8686bp+80) (f64.const 0x1.c34a7fc59e3c3p-791)) (f64.const -0x1.95421bf291b66p+870))
(assert_return (invoke "f64.div" (f64.const -0x1.91f58d7ed1237p+236) (f64.const -0x1.f190d808383c8p+55)) (f64.const 0x1.9d9eb0836f906p+180))

;; Computations that round differently in round-toward-zero mode.
(assert_return (invoke "f32.div" (f32.const 0x1.64b2a4p+26) (f32.const 0x1.e95752p-119)) (f32.const inf))
(assert_return (invoke "f32.div" (f32.const -0x1.53c9b6p+77) (f32.const 0x1.d689ap+27)) (f32.const -0x1.71baa4p+49))
(assert_return (invoke "f32.div" (f32.const 0x1.664a8ap+38) (f32.const -0x1.59dba2p+96)) (f32.const -0x1.0933f4p-58))
(assert_return (invoke "f32.div" (f32.const -0x1.99e0fap+111) (f32.const -0x1.c2b5a8p+9)) (f32.const 0x1.d19de6p+101))
(assert_return (invoke "f32.div" (f32.const -0x1.5a815ap+92) (f32.const -0x1.b5820ap+13)) (f32.const 0x1.9580b8p+78))
(assert_return (invoke "f64.div" (f64.const -0x1.81fd1e2af7bebp-655) (f64.const 0x1.edefc4eae536cp-691)) (f64.const -0x1.901abdd91b661p+35))
(assert_return (invoke "f64.div" (f64.const -0x1.47cf932953c43p+782) (f64.const -0x1.bc40496b1f2a1p-553)) (f64.const inf))
(assert_return (invoke "f64.div" (f64.const -0x1.2bd2e8fbdcad7p-746) (f64.const 0x1.b115674cc476ep-65)) (f64.const -0x1.62752bf19fa81p-682))
(assert_return (invoke "f64.div" (f64.const -0x1.f923e3fea9efep+317) (f64.const -0x1.8044c74d27a39p-588)) (f64.const 0x1.5086518cc7186p+905))
(assert_return (invoke "f64.div" (f64.const 0x1.516ed2051d6bbp+181) (f64.const -0x1.c9f455eb9c2eep+214)) (f64.const -0x1.79414d67f2889p-34))

;; Computations that round differently on x87.
(assert_return (invoke "f64.div" (f64.const -0x1.9c52726aed366p+585) (f64.const -0x1.7d0568c75660fp+195)) (f64.const 0x1.1507ca2a65f23p+390))
(assert_return (invoke "f64.div" (f64.const -0x1.522672f461667p+546) (f64.const -0x1.36d36572c9f71p+330)) (f64.const 0x1.1681369370619p+216))
(assert_return (invoke "f64.div" (f64.const 0x1.01051b4e8cd61p+185) (f64.const -0x1.2cbb5ca3d33ebp+965)) (f64.const -0x1.b59471598a2f3p-781))
(assert_return (invoke "f64.div" (f64.const 0x1.5f93bb80fc2cbp+217) (f64.const 0x1.7e051aae9f0edp+427)) (f64.const 0x1.d732fa926ba4fp-211))
(assert_return (invoke "f64.div" (f64.const -0x1.e251d762163ccp+825) (f64.const 0x1.3ee63581e1796p+349)) (f64.const -0x1.8330077d90a07p+476))

;; Computations that round differently on x87 in double-precision mode.
(assert_return (invoke "f64.div" (f64.const 0x1.dcbf69f10006dp+0) (f64.const 0x1.fffffffffffffp+1023)) (f64.const 0x0.772fda7c4001bp-1022))
(assert_return (invoke "f64.div" (f64.const 0x1.e14169442fbcap-1011) (f64.const 0x1.505451d62ff7dp+12)) (f64.const 0x0.b727e85f38b39p-1022))
(assert_return (invoke "f64.div" (f64.const -0x1.d3ebe726ec964p-144) (f64.const -0x1.4a7bfc0b83608p+880)) (f64.const 0x0.5a9d8c50cbf87p-1022))
(assert_return (invoke "f64.div" (f64.const -0x1.6c3def770aee1p-393) (f64.const -0x1.8b84724347598p+631)) (f64.const 0x0.3af0707fcd0c7p-1022))
(assert_return (invoke "f64.div" (f64.const 0x1.16abda1bb3cb3p-856) (f64.const 0x1.6c9c7198eb1e6p+166)) (f64.const 0x0.c3a8fd6741649p-1022))
(assert_return (invoke "f64.div" (f64.const 0x1.7057d6ab553cap-1005) (f64.const -0x1.2abf1e98660ebp+23)) (f64.const -0x0.04ee8d8ec01cdp-1022))

;; Computations that round differently when div is mul by reciprocal.
(assert_return (invoke "f32.div" (f32.const 0x1.ada9aap+89) (f32.const 0x1.69884cp+42)) (f32.const 0x1.303e2ep+47))
(assert_return (invoke "f32.div" (f32.const 0x1.8281c8p+90) (f32.const -0x1.62883cp+106)) (f32.const -0x1.17169cp-16))
(assert_return (invoke "f32.div" (f32.const 0x1.5c6be2p+81) (f32.const 0x1.d01dfep-1)) (f32.const 0x1.805e32p+81))
(assert_return (invoke "f32.div" (f32.const -0x1.bbd252p+19) (f32.const -0x1.fba95p+33)) (f32.const 0x1.bf9d56p-15))
(assert_return (invoke "f32.div" (f32.const -0x1.0f41d6p-42) (f32.const -0x1.3f2dbep+56)) (f32.const 0x1.b320d8p-99))
(assert_return (invoke "f64.div" (f64.const 0x1.b2348a1c81899p+61) (f64.const -0x1.4a58aad903dd3p-861)) (f64.const -0x1.507c1e2a41b35p+922))
(assert_return (invoke "f64.div" (f64.const 0x1.23fa5137a918ap-130) (f64.const -0x1.7268db1951263p-521)) (f64.const -0x1.93965e0d896bep+390))
(assert_return (invoke "f64.div" (f64.const 0x1.dcb3915d82deep+669) (f64.const 0x1.50caaa1dc6b19p+638)) (f64.const 0x1.6a58ec814b09dp+31))
(assert_return (invoke "f64.div" (f64.const -0x1.046e378c0cc46p+182) (f64.const 0x1.ac925009a922bp+773)) (f64.const -0x1.3720aa94dab18p-592))
(assert_return (invoke "f64.div" (f64.const -0x1.8945fd69d8e11p-871) (f64.const -0x1.0a37870af809ap-646)) (f64.const 0x1.7a2e286c62382p-225))

;; Computations that round differently when computed via f32.
(assert_return (invoke "f64.div" (f64.const 0x1.82002af0ea1f3p-57) (f64.const 0x1.d0a9b0c2fa339p+0)) (f64.const 0x1.a952fbd1fc17cp-58))
(assert_return (invoke "f64.div" (f64.const 0x1.1e12b515db471p-102) (f64.const -0x1.41fc3c94fba5p-42)) (f64.const -0x1.c6e50cccb7cb6p-61))
(assert_return (invoke "f64.div" (f64.const 0x1.aba5adcd6f583p-41) (f64.const 0x1.17dfac639ce0fp-112)) (f64.const 0x1.872b0a008c326p+71))
(assert_return (invoke "f64.div" (f64.const 0x1.cf82510d0ae6bp+89) (f64.const 0x1.0207d86498053p+97)) (f64.const 0x1.cbdc804e2cf14p-8))
(assert_return (invoke "f64.div" (f64.const 0x1.4c82cbb508e21p-11) (f64.const -0x1.6b57208c2d5d5p+52)) (f64.const -0x1.d48e8b369129ap-64))

;; Division involving the maximum subnormal value and the minimum normal value.
(assert_return (invoke "f32.div" (f32.const 0x1p-126) (f32.const 0x1.fffffcp-127)) (f32.const 0x1.000002p+0))
(assert_return (invoke "f32.div" (f32.const 0x1.fffffcp-127) (f32.const 0x1p-126)) (f32.const 0x1.fffffcp-1))
(assert_return (invoke "f64.div" (f64.const 0x1p-1022) (f64.const 0x0.fffffffffffffp-1022)) (f64.const 0x1.0000000000001p+0))
(assert_return (invoke "f64.div" (f64.const 0x0.fffffffffffffp-1022) (f64.const 0x1p-1022)) (f64.const 0x1.ffffffffffffep-1))

;; Test the least positive value with a positive quotient with the maximum value.
(assert_return (invoke "f32.div" (f32.const 0x1.fffffep-23) (f32.const 0x1.fffffep+127)) (f32.const 0x0p+0))
(assert_return (invoke "f32.div" (f32.const 0x1p-22) (f32.const 0x1.fffffep+127)) (f32.const 0x1p-149))
(assert_return (invoke "f64.div" (f64.const 0x1.fffffffffffffp-52) (f64.const 0x1.fffffffffffffp+1023)) (f64.const 0x0p+0))
(assert_return (invoke "f64.div" (f64.const 0x1p-51) (f64.const 0x1.fffffffffffffp+1023)) (f64.const 0x0.0000000000001p-1022))

;; Test the least positive value with a finite reciprocal.
(assert_return (invoke "f32.div" (f32.const 1.0) (f32.const 0x1p-128)) (f32.const inf))
(assert_return (invoke "f32.div" (f32.const 1.0) (f32.const 0x1.000008p-128)) (f32.const 0x1.fffffp+127))
(assert_return (invoke "f64.div" (f64.const 1.0) (f64.const 0x0.4p-1022)) (f64.const inf))
(assert_return (invoke "f64.div" (f64.const 1.0) (f64.const 0x0.4000000000001p-1022)) (f64.const 0x1.ffffffffffff8p+1023))

;; Test the least positive value that has a subnormal reciprocal.
(assert_return (invoke "f32.div" (f32.const 1.0) (f32.const 0x1.000002p+126)) (f32.const 0x1.fffffcp-127))
(assert_return (invoke "f32.div" (f32.const 1.0) (f32.const 0x1p+126)) (f32.const 0x1p-126))
(assert_return (invoke "f64.div" (f64.const 1.0) (f64.const 0x1.0000000000001p+1022)) (f64.const 0x0.fffffffffffffp-1022))
(assert_return (invoke "f64.div" (f64.const 1.0) (f64.const 0x1p+1022)) (f64.const 0x1p-1022))

;; Test that the last binary digit of 1.0/3.0 is even in f32,
;; https://en.wikipedia.org/wiki/Single-precision_floating-point_format#Single-precision_examples
;;
;; and odd in f64,
;; https://en.wikipedia.org/wiki/Double-precision_floating-point_format#Double-precision_examples
;;
;; and that 1.0/3.0, 3.0/9.0, and 9.0/27.0 all agree.
;; http://www.netlib.org/paranoia
(assert_return (invoke "f32.div" (f32.const 0x1p+0) (f32.const 0x1.8p+1)) (f32.const 0x1.555556p-2))
(assert_return (invoke "f32.div" (f32.const 0x3p+0) (f32.const 0x1.2p+3)) (f32.const 0x1.555556p-2))
(assert_return (invoke "f32.div" (f32.const 0x1.2p+3) (f32.const 0x1.bp+4)) (f32.const 0x1.555556p-2))
(assert_return (invoke "f64.div" (f64.const 0x1p+0) (f64.const 0x1.8p+1)) (f64.const 0x1.5555555555555p-2))
(assert_return (invoke "f64.div" (f64.const 0x3p+0) (f64.const 0x1.2p+3)) (f64.const 0x1.5555555555555p-2))
(assert_return (invoke "f64.div" (f64.const 0x1.2p+3) (f64.const 0x1.bp+4)) (f64.const 0x1.5555555555555p-2))

;; Test division of numbers very close to 1.
(assert_return (invoke "f32.div" (f32.const 0x1.000002p+0) (f32.const 0x1.fffffep-1)) (f32.const 0x1.000004p+0))
(assert_return (invoke "f32.div" (f32.const 0x1.fffffep-1) (f32.const 0x1.000002p+0)) (f32.const 0x1.fffffap-1))
(assert_return (invoke "f32.div" (f32.const 0x1.0p+0) (f32.const 0x1.fffffep-1)) (f32.const 0x1.000002p+0))
(assert_return (invoke "f32.div" (f32.const 0x1.0p+0) (f32.const 0x1.000002p+0)) (f32.const 0x1.fffffcp-1))
(assert_return (invoke "f64.div" (f64.const 0x1.0000000000001p+0) (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1.0000000000002p+0))
(assert_return (invoke "f64.div" (f64.const 0x1.fffffffffffffp-1) (f64.const 0x1.0000000000001p+0)) (f64.const 0x1.ffffffffffffdp-1))
(assert_return (invoke "f64.div" (f64.const 0x1.0p+0) (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1.0000000000001p+0))
(assert_return (invoke "f64.div" (f64.const 0x1.0p+0) (f64.const 0x1.0000000000001p+0)) (f64.const 0x1.ffffffffffffep-1))

;; Test for bugs found in an early RISC-V implementation.
;; https://github.com/riscv/riscv-tests/pull/8
(assert_return (invoke "f32.sqrt" (f32.const 0x1.56p+7)) (f32.const 0x1.a2744cp+3))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.594dfcp-23)) (f32.const 0x1.a4789cp-12))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.56p+7)) (f64.const 0x1.a2744ce9674f5p+3))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.594dfc70aa105p-23)) (f64.const 0x1.a4789c0e37f99p-12))

;; Computations that round differently on x87.
(assert_return (invoke "f64.sqrt" (f64.const 0x1.0263fcc94f259p-164)) (f64.const 0x1.0131485de579fp-82))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.352dfa278c43dp+338)) (f64.const 0x1.195607dac5417p+169))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.b15daa23924fap+402)) (f64.const 0x1.4d143db561493p+201))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.518c8e68cb753p-37)) (f64.const 0x1.9fb8ef1ad5bfdp-19))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.86d8b6518078ep-370)) (f64.const 0x1.3c5142a48fcadp-185))

;; Test another sqrt case on x87.
;; https://gcc.gnu.org/bugzilla/show_bug.cgi?id=52593
(assert_return (invoke "f64.sqrt" (f64.const 0x1.fffffffffffffp-1)) (f64.const 0x1.fffffffffffffp-1))

;; Computations that round differently in round-upward mode.
(assert_return (invoke "f32.sqrt" (f32.const 0x1.098064p-3)) (f32.const 0x1.70b23p-2))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.d9befp+100)) (f32.const 0x1.5c4052p+50))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.42b5b6p-4)) (f32.const 0x1.1f6d0ep-2))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.3684dp-71)) (f32.const 0x1.8ebae2p-36))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.d8bc4ep-11)) (f32.const 0x1.ebf9eap-6))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.5c39f220d5704p-924)) (f64.const 0x1.2a92bc24ceae9p-462))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.53521a635745cp+727)) (f64.const 0x1.a0cfdc4ef8ff1p+363))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.dfd5bbc9f4678p+385)) (f64.const 0x1.efa817117c94cp+192))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.33f9640811cd4p+105)) (f64.const 0x1.8d17c9243baa3p+52))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.6c0ef0267ff45p+999)) (f64.const 0x1.afbcfae3f2b4p+499))

;; Computations that round differently in round-downward mode.
(assert_return (invoke "f32.sqrt" (f32.const 0x1.26a62ep+27)) (f32.const 0x1.84685p+13))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.166002p-113)) (f32.const 0x1.798762p-57))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.3dfb5p-15)) (f32.const 0x1.937e38p-8))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.30eb2cp-120)) (f32.const 0x1.176406p-60))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.cb705cp-123)) (f32.const 0x1.e5020ap-62))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.edae8aea0543p+695)) (f64.const 0x1.f6c1ea4fc8dd2p+347))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.f7ee4bda5c9c3p-763)) (f64.const 0x1.fbf30bdaf11c5p-382))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.a48f348266ad1p-30)) (f64.const 0x1.481ee7540baf7p-15))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.feb5a1ce3ed9cp-242)) (f64.const 0x1.6995060c20d46p-121))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.957d9796e3834p+930)) (f64.const 0x1.42305213157bap+465))

;; Computations that round differently in round-toward-zero mode.
(assert_return (invoke "f32.sqrt" (f32.const 0x1.65787cp+118)) (f32.const 0x1.2e82a4p+59))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.736044p+15)) (f32.const 0x1.b40e4p+7))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.a00edp-1)) (f32.const 0x1.cd8aecp-1))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.7a4c8p-87)) (f32.const 0x1.b819e4p-44))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.5d24d4p-94)) (f32.const 0x1.2af75ep-47))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.a008948ead274p+738)) (f64.const 0x1.4659b37c39b19p+369))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.70f6199ed21f5p-381)) (f64.const 0x1.b2a2bddf3300dp-191))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.35c1d49f2a352p+965)) (f64.const 0x1.8e3d9f01a9716p+482))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.3fbdcfb2b2a15p-45)) (f64.const 0x1.949ba4feca42ap-23))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.c201b94757145p-492)) (f64.const 0x1.5369ee6bf2967p-246))

;; Computations that round differently when computed via f32.
(assert_return (invoke "f64.sqrt" (f64.const -0x1.360e8d0032adp-963)) (f64.const nan:canonical))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.d9a6f5eef0503p+103)) (f64.const 0x1.ec73f56c166f6p+51))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.aa051a5c4ec27p-760)) (f64.const 0x1.4a3e771ff5149p-380))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.e5522a741babep-276)) (f64.const 0x1.607ae2b6feb7dp-138))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.4832badc0c061p+567)) (f64.const 0x1.99ec7934139b2p+283))

;; Test the least value with a sqrt that rounds to one.
(assert_return (invoke "f32.sqrt" (f32.const 0x1.000002p+0)) (f32.const 0x1p+0))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.000004p+0)) (f32.const 0x1.000002p+0))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.0000000000001p+0)) (f64.const 0x1p+0))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.0000000000002p+0)) (f64.const 0x1.0000000000001p+0))

;; Test the greatest value less than one for which sqrt is not an identity.
(assert_return (invoke "f32.sqrt" (f32.const 0x1.fffffcp-1)) (f32.const 0x1.fffffep-1))
(assert_return (invoke "f32.sqrt" (f32.const 0x1.fffffap-1)) (f32.const 0x1.fffffcp-1))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.ffffffffffffep-1)) (f64.const 0x1.fffffffffffffp-1))
(assert_return (invoke "f64.sqrt" (f64.const 0x1.ffffffffffffdp-1)) (f64.const 0x1.ffffffffffffep-1))

;; Test that the bitwise floating point operators are bitwise on NaN.

(assert_return (invoke "f32.abs" (f32.const nan:0x0f1e2)) (f32.const nan:0x0f1e2))
(assert_return (invoke "f32.abs" (f32.const -nan:0x0f1e2)) (f32.const nan:0x0f1e2))
(assert_return (invoke "f64.abs" (f64.const nan:0x0f1e27a6b)) (f64.const nan:0x0f1e27a6b))
(assert_return (invoke "f64.abs" (f64.const -nan:0x0f1e27a6b)) (f64.const nan:0x0f1e27a6b))

(assert_return (invoke "f32.neg" (f32.const nan:0x0f1e2)) (f32.const -nan:0x0f1e2))
(assert_return (invoke "f32.neg" (f32.const -nan:0x0f1e2)) (f32.const nan:0x0f1e2))
(assert_return (invoke "f64.neg" (f64.const nan:0x0f1e27a6b)) (f64.const -nan:0x0f1e27a6b))
(assert_return (invoke "f64.neg" (f64.const -nan:0x0f1e27a6b)) (f64.const nan:0x0f1e27a6b))

(assert_return (invoke "f32.copysign" (f32.const nan:0x0f1e2) (f32.const nan)) (f32.const nan:0x0f1e2))
(assert_return (invoke "f32.copysign" (f32.const nan:0x0f1e2) (f32.const -nan)) (f32.const -nan:0x0f1e2))
(assert_return (invoke "f32.copysign" (f32.const -nan:0x0f1e2) (f32.const nan)) (f32.const nan:0x0f1e2))
(assert_return (invoke "f32.copysign" (f32.const -nan:0x0f1e2) (f32.const -nan)) (f32.const -nan:0x0f1e2))
(assert_return (invoke "f64.copysign" (f64.const nan:0x0f1e27a6b) (f64.const nan)) (f64.const nan:0x0f1e27a6b))
(assert_return (invoke "f64.copysign" (f64.const nan:0x0f1e27a6b) (f64.const -nan)) (f64.const -nan:0x0f1e27a6b))
(assert_return (invoke "f64.copysign" (f64.const -nan:0x0f1e27a6b) (f64.const nan)) (f64.const nan:0x0f1e27a6b))
(assert_return (invoke "f64.copysign" (f64.const -nan:0x0f1e27a6b) (f64.const -nan)) (f64.const -nan:0x0f1e27a6b))

;; Test values close to 1.0.
(assert_return (invoke "f32.ceil" (f32.const 0x1.fffffep-1)) (f32.const 1.0))
(assert_return (invoke "f32.ceil" (f32.const 0x1.000002p+0)) (f32.const 2.0))
(assert_return (invoke "f64.ceil" (f64.const 0x1.fffffffffffffp-1)) (f64.const 1.0))
(assert_return (invoke "f64.ceil" (f64.const 0x1.0000000000001p+0)) (f64.const 2.0))

;; Test the maximum and minimum value for which ceil is not an identity operator.
(assert_return (invoke "f32.ceil" (f32.const 0x1.fffffep+22)) (f32.const 0x1p+23))
(assert_return (invoke "f32.ceil" (f32.const -0x1.fffffep+22)) (f32.const -0x1.fffffcp+22))
(assert_return (invoke "f64.ceil" (f64.const 0x1.fffffffffffffp+51)) (f64.const 0x1p+52))
(assert_return (invoke "f64.ceil" (f64.const -0x1.fffffffffffffp+51)) (f64.const -0x1.ffffffffffffep+51))

;; Test that implementations don't do the x+0x1p52-0x1p52 trick outside the
;; range where it's safe.
(assert_return (invoke "f32.ceil" (f32.const 0x1.fffffep+23)) (f32.const 0x1.fffffep+23))
(assert_return (invoke "f32.ceil" (f32.const -0x1.fffffep+23)) (f32.const -0x1.fffffep+23))
(assert_return (invoke "f64.ceil" (f64.const 0x1.fffffffffffffp+52)) (f64.const 0x1.fffffffffffffp+52))
(assert_return (invoke "f64.ceil" (f64.const -0x1.fffffffffffffp+52)) (f64.const -0x1.fffffffffffffp+52))

;; Test values close to -1.0.
(assert_return (invoke "f32.floor" (f32.const -0x1.fffffep-1)) (f32.const -1.0))
(assert_return (invoke "f32.floor" (f32.const -0x1.000002p+0)) (f32.const -2.0))
(assert_return (invoke "f64.floor" (f64.const -0x1.fffffffffffffp-1)) (f64.const -1.0))
(assert_return (invoke "f64.floor" (f64.const -0x1.0000000000001p+0)) (f64.const -2.0))

;; Test the maximum and minimum value for which floor is not an identity operator.
(assert_return (invoke "f32.floor" (f32.const -0x1.fffffep+22)) (f32.const -0x1p+23))
(assert_return (invoke "f32.floor" (f32.const 0x1.fffffep+22)) (f32.const 0x1.fffffcp+22))
(assert_return (invoke "f64.floor" (f64.const -0x1.fffffffffffffp+51)) (f64.const -0x1p+52))
(assert_return (invoke "f64.floor" (f64.const 0x1.fffffffffffffp+51)) (f64.const 0x1.ffffffffffffep+51))

;; Test that floor isn't implemented as XMVectorFloor.
;; http://dss.stephanierct.com/DevBlog/?p=8#comment-4
(assert_return (invoke "f32.floor" (f32.const 88607.0)) (f32.const 88607.0))
(assert_return (invoke "f64.floor" (f64.const 88607.0)) (f64.const 88607.0))

;; Test the maximum and minimum value for which trunc is not an identity operator.
(assert_return (invoke "f32.trunc" (f32.const -0x1.fffffep+22)) (f32.const -0x1.fffffcp+22))
(assert_return (invoke "f32.trunc" (f32.const 0x1.fffffep+22)) (f32.const 0x1.fffffcp+22))
(assert_return (invoke "f64.trunc" (f64.const -0x1.fffffffffffffp+51)) (f64.const -0x1.ffffffffffffep+51))
(assert_return (invoke "f64.trunc" (f64.const 0x1.fffffffffffffp+51)) (f64.const 0x1.ffffffffffffep+51))

;; Test that nearest isn't implemented naively.
;; http://blog.frama-c.com/index.php?post/2013/05/02/nearbyintf1
;; http://blog.frama-c.com/index.php?post/2013/05/04/nearbyintf3
(assert_return (invoke "f32.nearest" (f32.const 0x1.000002p+23)) (f32.const 0x1.000002p+23))
(assert_return (invoke "f32.nearest" (f32.const 0x1.000004p+23)) (f32.const 0x1.000004p+23))
(assert_return (invoke "f32.nearest" (f32.const 0x1.fffffep-2)) (f32.const 0.0))
(assert_return (invoke "f32.nearest" (f32.const 0x1.fffffep+47)) (f32.const 0x1.fffffep+47))
(assert_return (invoke "f64.nearest" (f64.const 0x1.0000000000001p+52)) (f64.const 0x1.0000000000001p+52))
(assert_return (invoke "f64.nearest" (f64.const 0x1.0000000000002p+52)) (f64.const 0x1.0000000000002p+52))
(assert_return (invoke "f64.nearest" (f64.const 0x1.fffffffffffffp-2)) (f64.const 0.0))
(assert_return (invoke "f64.nearest" (f64.const 0x1.fffffffffffffp+105)) (f64.const 0x1.fffffffffffffp+105))

;; Nearest should not round halfway cases away from zero (as C's round(3) does)
;; or up (as JS's Math.round does).
(assert_return (invoke "f32.nearest" (f32.const 4.5)) (f32.const 4.0))
(assert_return (invoke "f32.nearest" (f32.const -4.5)) (f32.const -4.0))
(assert_return (invoke "f32.nearest" (f32.const -3.5)) (f32.const -4.0))
(assert_return (invoke "f64.nearest" (f64.const 4.5)) (f64.const 4.0))
(assert_return (invoke "f64.nearest" (f64.const -4.5)) (f64.const -4.0))
(assert_return (invoke "f64.nearest" (f64.const -3.5)) (f64.const -4.0))

;; Test the maximum and minimum value for which nearest is not an identity operator.
(assert_return (invoke "f32.nearest" (f32.const -0x1.fffffep+22)) (f32.const -0x1p+23))
(assert_return (invoke "f32.nearest" (f32.const 0x1.fffffep+22)) (f32.const 0x1p+23))
(assert_return (invoke "f64.nearest" (f64.const -0x1.fffffffffffffp+51)) (f64.const -0x1p+52))
(assert_return (invoke "f64.nearest" (f64.const 0x1.fffffffffffffp+51)) (f64.const 0x1p+52))
