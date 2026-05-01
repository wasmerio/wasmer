(module
  (func (export "i64.add128") (param i64 i64 i64 i64) (result i64 i64)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    i64.add128)
  (func (export "i64.sub128") (param i64 i64 i64 i64) (result i64 i64)
    local.get 0
    local.get 1
    local.get 2
    local.get 3
    i64.sub128)
  (func (export "i64.mul_wide_s") (param i64 i64) (result i64 i64)
    local.get 0
    local.get 1
    i64.mul_wide_s)
  (func (export "i64.mul_wide_u") (param i64 i64) (result i64 i64)
    local.get 0
    local.get 1
    i64.mul_wide_u)
)

;; simple addition
(assert_return (invoke "i64.add128"
                  (i64.const 0) (i64.const 0)
                  (i64.const 0) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.add128"
                  (i64.const 0) (i64.const 1)
                  (i64.const 1) (i64.const 0))
               (i64.const 1) (i64.const 1))
(assert_return (invoke "i64.add128"
                  (i64.const 1) (i64.const 0)
                  (i64.const -1) (i64.const 0))
               (i64.const 0) (i64.const 1))
(assert_return (invoke "i64.add128"
                  (i64.const 1) (i64.const 1)
                  (i64.const -1) (i64.const -1))
               (i64.const 0) (i64.const 1))

;; simple subtraction
(assert_return (invoke "i64.sub128"
                  (i64.const 0) (i64.const 0)
                  (i64.const 0) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.sub128"
                  (i64.const 0) (i64.const 0)
                  (i64.const 1) (i64.const 0))
               (i64.const -1) (i64.const -1))
(assert_return (invoke "i64.sub128"
                  (i64.const 0) (i64.const 1)
                  (i64.const 1) (i64.const 1))
               (i64.const -1) (i64.const -1))
(assert_return (invoke "i64.sub128"
                  (i64.const 0) (i64.const 0)
                  (i64.const 1) (i64.const 1))
               (i64.const -1) (i64.const -2))

;; simple mul_wide
(assert_return (invoke "i64.mul_wide_s" (i64.const 0) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 0) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const 1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const 1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -1) (i64.const -1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -1) (i64.const 1))
               (i64.const -1) (i64.const -1))
(assert_return (invoke "i64.mul_wide_u" (i64.const -1) (i64.const 1))
               (i64.const -1) (i64.const 0))

;; 20 randomly generated test cases for i64.add128
(assert_return (invoke "i64.add128"
                   (i64.const -2418420703207364752) (i64.const -1)
                   (i64.const -1) (i64.const -1))
               (i64.const -2418420703207364753) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 0) (i64.const 0)
                   (i64.const -4579433644172935106) (i64.const -1))
               (i64.const -4579433644172935106) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 0) (i64.const 0)
                   (i64.const 1) (i64.const -1))
               (i64.const 1) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const 0)
                   (i64.const 1) (i64.const 0))
               (i64.const 2) (i64.const 0))
(assert_return (invoke "i64.add128"
                   (i64.const -1) (i64.const -1)
                   (i64.const -1) (i64.const -1))
               (i64.const -2) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 0) (i64.const -1)
                   (i64.const 1) (i64.const 0))
               (i64.const 1) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 0) (i64.const 0)
                   (i64.const 0) (i64.const -1))
               (i64.const 0) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const 0)
                   (i64.const -1) (i64.const -1))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.add128"
                   (i64.const 0) (i64.const 6184727276166606191)
                   (i64.const 0) (i64.const 1))
               (i64.const 0) (i64.const 6184727276166606192))
(assert_return (invoke "i64.add128"
                   (i64.const -8434911321912688222) (i64.const -1)
                   (i64.const 1) (i64.const -1))
               (i64.const -8434911321912688221) (i64.const -2))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const -1)
                   (i64.const 0) (i64.const -1))
               (i64.const 1) (i64.const -2))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const -5148941131328838092)
                   (i64.const 0) (i64.const 0))
               (i64.const 1) (i64.const -5148941131328838092))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const 1)
                   (i64.const 1) (i64.const 0))
               (i64.const 2) (i64.const 1))
(assert_return (invoke "i64.add128"
                   (i64.const -1) (i64.const -1)
                   (i64.const -3636740005180858631) (i64.const -1))
               (i64.const -3636740005180858632) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const -5529682780229988275) (i64.const -1)
                   (i64.const 0) (i64.const 0))
               (i64.const -5529682780229988275) (i64.const -1))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const -5381447440966559717)
                   (i64.const 1020031372481336745) (i64.const 1))
               (i64.const 1020031372481336746) (i64.const -5381447440966559716))
(assert_return (invoke "i64.add128"
                   (i64.const 1) (i64.const 1)
                   (i64.const 0) (i64.const 0))
               (i64.const 1) (i64.const 1))
(assert_return (invoke "i64.add128"
                   (i64.const -9133888546939907356) (i64.const -1)
                   (i64.const 1) (i64.const 1))
               (i64.const -9133888546939907355) (i64.const 0))
(assert_return (invoke "i64.add128"
                   (i64.const -4612047512704241719) (i64.const -1)
                   (i64.const 0) (i64.const -1))
               (i64.const -4612047512704241719) (i64.const -2))
(assert_return (invoke "i64.add128"
                   (i64.const 414720966820876428) (i64.const -1)
                   (i64.const 1) (i64.const 0))
               (i64.const 414720966820876429) (i64.const -1))


;; 20 randomly generated test cases for i64.sub128
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const -2459085471354756766)
                   (i64.const -9151153060221070927) (i64.const -1))
               (i64.const 9151153060221070927) (i64.const -2459085471354756766))
(assert_return (invoke "i64.sub128"
                   (i64.const 4566502638724063423) (i64.const -4282658540409485563)
                   (i64.const -6884077310018979971) (i64.const -1))
               (i64.const -6996164124966508222) (i64.const -4282658540409485563))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const 3118380319444903041)
                   (i64.const 0) (i64.const 3283115686417695443))
               (i64.const 1) (i64.const -164735366972792402))
(assert_return (invoke "i64.sub128"
                   (i64.const -7208415241680161810) (i64.const -1)
                   (i64.const 1) (i64.const 0))
               (i64.const -7208415241680161811) (i64.const -1))
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const 3944850126731328706)
                   (i64.const 1) (i64.const 1))
               (i64.const -1) (i64.const 3944850126731328704))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const -1)
                   (i64.const -1) (i64.const -1))
               (i64.const 2) (i64.const -1))
(assert_return (invoke "i64.sub128"
                   (i64.const -1) (i64.const -1)
                   (i64.const 4855833073346115923) (i64.const -6826437637438999645))
               (i64.const -4855833073346115924) (i64.const 6826437637438999644))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const 0)
                   (i64.const -1) (i64.const -1))
               (i64.const 2) (i64.const 0))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const 0)
                   (i64.const 1) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.sub128"
                   (i64.const -1) (i64.const -1)
                   (i64.const 0) (i64.const 0))
               (i64.const -1) (i64.const -1))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const -1)
                   (i64.const -6365475388498096428) (i64.const -1))
               (i64.const 6365475388498096429) (i64.const -1))
(assert_return (invoke "i64.sub128"
                   (i64.const 6804238617560992346) (i64.const -1)
                   (i64.const 0) (i64.const -1))
               (i64.const 6804238617560992346) (i64.const 0))
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const 1)
                   (i64.const 1) (i64.const -7756145513466453619))
               (i64.const -1) (i64.const 7756145513466453619))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const -1)
                   (i64.const 1) (i64.const 1))
               (i64.const 0) (i64.const -2))
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const 1)
                   (i64.const 1) (i64.const 0))
               (i64.const -1) (i64.const 0))
(assert_return (invoke "i64.sub128"
                   (i64.const 1) (i64.const 5602881641763648953)
                   (i64.const -2110589244314239080) (i64.const -1))
               (i64.const 2110589244314239081) (i64.const 5602881641763648953))
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const 1)
                   (i64.const -1) (i64.const -1))
               (i64.const 1) (i64.const 1))
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const -1)
                   (i64.const 3553816990259121806) (i64.const -2105235417856431622))
               (i64.const -3553816990259121806) (i64.const 2105235417856431620))
(assert_return (invoke "i64.sub128"
                   (i64.const 1861102705894987245) (i64.const 1)
                   (i64.const 3713781778534059871) (i64.const 1))
               (i64.const -1852679072639072626) (i64.const -1))
(assert_return (invoke "i64.sub128"
                   (i64.const 0) (i64.const -1)
                   (i64.const 1) (i64.const 1832524486821761762))
               (i64.const -1) (i64.const -1832524486821761764))

;; 20 randomly generated test cases for i64.mul_wide_s
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const 1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 0) (i64.const 6287758211025156705))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -6643537319803451357) (i64.const 1))
               (i64.const -6643537319803451357) (i64.const -1))
(assert_return (invoke "i64.mul_wide_s" (i64.const -2483565146858803428) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const 1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -3838951433439430085) (i64.const 3471602925362676030))
               (i64.const 5186941893001237834) (i64.const -722475195264825124))
(assert_return (invoke "i64.mul_wide_s" (i64.const -8262495286814853129) (i64.const 7883241869666573970))
               (i64.const -8557189786755031842) (i64.const -3530988912334554469))
(assert_return (invoke "i64.mul_wide_s" (i64.const 4278371902407959701) (i64.const 1))
               (i64.const 4278371902407959701) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -8852706149487089182) (i64.const -1))
               (i64.const 8852706149487089182) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const -1))
               (i64.const -1) (i64.const -1))
(assert_return (invoke "i64.mul_wide_s" (i64.const -1) (i64.const -4329244561838653387))
               (i64.const 4329244561838653387) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -1) (i64.const -1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 697896157315764057) (i64.const 1))
               (i64.const 697896157315764057) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const 1))
               (i64.const 1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const -1) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 0) (i64.const -3769664482072947073))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const 8414291037346403854))
               (i64.const 8414291037346403854) (i64.const 0))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const -1))
               (i64.const -1) (i64.const -1))
(assert_return (invoke "i64.mul_wide_s" (i64.const 5014655679779318485) (i64.const -5080037812563681985))
               (i64.const 2842857627777395563) (i64.const -1380983027057486843))
(assert_return (invoke "i64.mul_wide_s" (i64.const 0) (i64.const 1))
               (i64.const 0) (i64.const 0))

;; 20 randomly generated test cases for i64.mul_wide_u
(assert_return (invoke "i64.mul_wide_u" (i64.const -4734436040338162711) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 3270597527173764279) (i64.const 6636648075495406358))
               (i64.const -5430303818902260550) (i64.const 1176674035141685826))
(assert_return (invoke "i64.mul_wide_u" (i64.const -7771814344630108151) (i64.const 1))
               (i64.const -7771814344630108151) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const -7864138787704962081))
               (i64.const -7864138787704962081) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const 518555141550256010))
               (i64.const 518555141550256010) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const -1))
               (i64.const -1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1118900477321231571) (i64.const -1))
               (i64.const -1118900477321231571) (i64.const 1118900477321231570))
(assert_return (invoke "i64.mul_wide_u" (i64.const -1) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const -5586890671027490027) (i64.const 1))
               (i64.const -5586890671027490027) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 0) (i64.const 3603850799751152505))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const -1) (i64.const -1))
               (i64.const 1) (i64.const 18446744073709551614))
(assert_return (invoke "i64.mul_wide_u" (i64.const 0) (i64.const 1))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const -7344082851774441644) (i64.const 3896439839137544024))
               (i64.const 5738542512914895072) (i64.const 2345175459296971666))
(assert_return (invoke "i64.mul_wide_u" (i64.const 0) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 616395976148874061) (i64.const 0))
               (i64.const 0) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 2810729703362889816) (i64.const -1))
               (i64.const -2810729703362889816) (i64.const 2810729703362889815))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const -1))
               (i64.const -1) (i64.const 0))
(assert_return (invoke "i64.mul_wide_u" (i64.const 1) (i64.const 0))
               (i64.const 0) (i64.const 0))

;; assert overlong encodings for each instruction's binary encoding are accepted
(module binary
  "\00asm" "\01\00\00\00"

  "\01\11"                  ;; type section, 17 bytes
  "\02"                     ;; 2 count
  "\60"                     ;; type0 = function
  "\04\7e\7e\7e\7e"         ;;  4 params - all i64
  "\02\7e\7e"               ;;  2 results - both i64
  "\60"                     ;; type1 = function
  "\02\7e\7e"               ;;  2 params - both i64
  "\02\7e\7e"               ;;  2 results - both i64

  "\03\05"                  ;; function section, 5 byte
  "\04"                     ;; 4 count
  "\00\00\01\01"            ;; types of each function (0, 0, 1, 1)

  "\07\3d"                  ;; export section 0x3d bytes
  "\04"                     ;; 4 count
  "\0ai64.add128\00\00"     ;; "i64.add128" which is function 0
  "\0ai64.sub128\00\01"     ;; "i64.add128" which is function 1
  "\0ei64.mul_wide_s\00\02" ;; "i64.mul_wide_s" which is function 2
  "\0ei64.mul_wide_u\00\03" ;; "i64.mul_wide_s" which is function 3

  "\0a\37"                  ;; code section + byte length
  "\04"                     ;; 4 count

  "\0e"                     ;; byte length
  "\00"                     ;; no locals
  "\20\00"                  ;; local.get 0
  "\20\01"                  ;; local.get 1
  "\20\02"                  ;; local.get 2
  "\20\03"                  ;; local.get 3
  "\fc\93\80\00"            ;; i64.add128 (overlong)
  "\0b"                     ;; end

  "\0d"                     ;; byte length
  "\00"                     ;; no locals
  "\20\00"                  ;; local.get 0
  "\20\01"                  ;; local.get 1
  "\20\02"                  ;; local.get 2
  "\20\03"                  ;; local.get 3
  "\fc\94\00"               ;; i64.sub128 (overlong)
  "\0b"                     ;; end

  "\0c"                     ;; byte length
  "\00"                     ;; no locals
  "\20\00"                  ;; local.get 0
  "\20\01"                  ;; local.get 1
  "\fc\95\80\80\80\00"      ;; i64.mul_wide_s (overlong)
  "\0b"                     ;; end

  "\0b"                     ;; byte length
  "\00"                     ;; no locals
  "\20\00"                  ;; local.get 0
  "\20\01"                  ;; local.get 1
  "\fc\96\80\80\00"         ;; i64.mul_wide_u (overlong)
  "\0b"                     ;; end
)

(assert_return (invoke "i64.add128"
                  (i64.const 1) (i64.const 2)
                  (i64.const 3) (i64.const 4))
               (i64.const 4) (i64.const 6))
(assert_return (invoke "i64.sub128"
                  (i64.const 2) (i64.const 5)
                  (i64.const 1) (i64.const 2))
               (i64.const 1) (i64.const 3))
(assert_return (invoke "i64.mul_wide_s" (i64.const 1) (i64.const -2))
               (i64.const -2) (i64.const -1))
(assert_return (invoke "i64.mul_wide_u" (i64.const 3) (i64.const 2))
               (i64.const 6) (i64.const 0))

;; some invalid types for these instructions

(assert_invalid
  (module
    (func (param i64 i64 i64 i64) (result i64)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      i64.add128)
  )
  "type mismatch")
(assert_invalid
  (module
    (func (param i64 i64 i64) (result i64 i64)
      local.get 0
      local.get 1
      local.get 2
      i64.add128)
  )
  "type mismatch")

(assert_invalid
  (module
    (func (param i64 i64 i64 i64) (result i64)
      local.get 0
      local.get 1
      local.get 2
      local.get 3
      i64.sub128)
  )
  "type mismatch")
(assert_invalid
  (module
    (func (param i64 i64 i64) (result i64 i64)
      local.get 0
      local.get 1
      local.get 2
      i64.sub128)
  )
  "type mismatch")

(assert_invalid
  (module
    (func (param i64 i64) (result i64)
      local.get 0
      local.get 1
      i64.mul_wide_s)
  )
  "type mismatch")
(assert_invalid
  (module
    (func (param i64) (result i64 i64)
      local.get 0
      i64.mul_wide_s)
  )
  "type mismatch")

(assert_invalid
  (module
    (func (param i64 i64) (result i64)
      local.get 0
      local.get 1
      i64.mul_wide_u)
  )
  "type mismatch")
(assert_invalid
  (module
    (func (param i64) (result i64 i64)
      local.get 0
      i64.mul_wide_u)
  )
  "type mismatch")
