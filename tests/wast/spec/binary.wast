(module binary "\00asm\01\00\00\00")
(module binary "\00asm" "\01\00\00\00")
(module $M1 binary "\00asm\01\00\00\00")
(module $M2 binary "\00asm" "\01\00\00\00")

(assert_malformed (module binary "") "unexpected end")
(assert_malformed (module binary "\01") "unexpected end")
(assert_malformed (module binary "\00as") "unexpected end")
(assert_malformed (module binary "asm\00") "magic header not detected")
(assert_malformed (module binary "msa\00") "magic header not detected")
(assert_malformed (module binary "msa\00\01\00\00\00") "magic header not detected")
(assert_malformed (module binary "msa\00\00\00\00\01") "magic header not detected")
(assert_malformed (module binary "asm\01\00\00\00\00") "magic header not detected")
(assert_malformed (module binary "wasm\01\00\00\00") "magic header not detected")
(assert_malformed (module binary "\7fasm\01\00\00\00") "magic header not detected")
(assert_malformed (module binary "\80asm\01\00\00\00") "magic header not detected")
(assert_malformed (module binary "\82asm\01\00\00\00") "magic header not detected")
(assert_malformed (module binary "\ffasm\01\00\00\00") "magic header not detected")

;; 8-byte endian-reversed.
(assert_malformed (module binary "\00\00\00\01msa\00") "magic header not detected")

;; Middle-endian byte orderings.
(assert_malformed (module binary "a\00ms\00\01\00\00") "magic header not detected")
(assert_malformed (module binary "sm\00a\00\00\01\00") "magic header not detected")

;; Upper-cased.
(assert_malformed (module binary "\00ASM\01\00\00\00") "magic header not detected")

;; EBCDIC-encoded magic.
(assert_malformed (module binary "\00\81\a2\94\01\00\00\00") "magic header not detected")

;; Leading UTF-8 BOM.
(assert_malformed (module binary "\ef\bb\bf\00asm\01\00\00\00") "magic header not detected")

;; Malformed binary version.
(assert_malformed (module binary "\00asm") "unexpected end")
(assert_malformed (module binary "\00asm\01") "unexpected end")
(assert_malformed (module binary "\00asm\01\00\00") "unexpected end")
(assert_malformed (module binary "\00asm\00\00\00\00") "unknown binary version")
(assert_malformed (module binary "\00asm\0d\00\00\00") "unknown binary version")
(assert_malformed (module binary "\00asm\0e\00\00\00") "unknown binary version")
(assert_malformed (module binary "\00asm\00\01\00\00") "unknown binary version")
(assert_malformed (module binary "\00asm\00\00\01\00") "unknown binary version")
(assert_malformed (module binary "\00asm\00\00\00\01") "unknown binary version")

;; Invalid section id.
(assert_malformed (module binary "\00asm" "\01\00\00\00" "\0c\00") "malformed section id")
(assert_malformed (module binary "\00asm" "\01\00\00\00" "\7f\00") "malformed section id")
(assert_malformed (module binary "\00asm" "\01\00\00\00" "\80\00\01\00") "malformed section id")
(assert_malformed (module binary "\00asm" "\01\00\00\00" "\81\00\01\00") "malformed section id")
(assert_malformed (module binary "\00asm" "\01\00\00\00" "\ff\00\01\00") "malformed section id")


;; Type section with signed LEB128 encoded type
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01"                     ;; Type section id
    "\05"                     ;; Type section length
    "\01"                     ;; Types vector length
    "\e0\7f"                  ;; Malformed functype, -0x20 in signed LEB128 encoding
    "\00\00"
  )
  "integer representation too long"
)


;; call_indirect reserved byte equal to zero.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"      ;; Type section
    "\03\02\01\00"            ;; Function section
    "\04\04\01\70\00\00"      ;; Table section
    "\0a\09\01"               ;; Code section

    ;; function 0
    "\07\00"
    "\41\00"                   ;; i32.const 0
    "\11\00"                   ;; call_indirect (type 0)
    "\01"                      ;; call_indirect reserved byte is not equal to zero!
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; call_indirect reserved byte should not be a "long" LEB128 zero.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"      ;; Type section
    "\03\02\01\00"            ;; Function section
    "\04\04\01\70\00\00"      ;; Table section
    "\0a\0a\01"               ;; Code section

    ;; function 0
    "\07\00"
    "\41\00"                   ;; i32.const 0
    "\11\00"                   ;; call_indirect (type 0)
    "\80\00"                   ;; call_indirect reserved byte
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; Same as above for 3, 4, and 5-byte zero encodings.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"      ;; Type section
    "\03\02\01\00"            ;; Function section
    "\04\04\01\70\00\00"      ;; Table section
    "\0a\0b\01"               ;; Code section

    ;; function 0
    "\08\00"
    "\41\00"                   ;; i32.const 0
    "\11\00"                   ;; call_indirect (type 0)
    "\80\80\00"                ;; call_indirect reserved byte
    "\0b"                      ;; end
  )
  "zero flag expected"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"      ;; Type section
    "\03\02\01\00"            ;; Function section
    "\04\04\01\70\00\00"      ;; Table section
    "\0a\0c\01"               ;; Code section

    ;; function 0
    "\09\00"
    "\41\00"                   ;; i32.const 0
    "\11\00"                   ;; call_indirect (type 0)
    "\80\80\80\00"             ;; call_indirect reserved byte
    "\0b"                      ;; end
  )
  "zero flag expected"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"      ;; Type section
    "\03\02\01\00"            ;; Function section
    "\04\04\01\70\00\00"      ;; Table section
    "\0a\0d\01"               ;; Code section

    ;; function 0
    "\0a\00"
    "\41\00"                   ;; i32.const 0
    "\11\00"                   ;; call_indirect (type 0)
    "\80\80\80\80\00"          ;; call_indirect reserved byte
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; memory.grow reserved byte equal to zero.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\09\01"                ;; Code section

    ;; function 0
    "\07\00"
    "\41\00"                   ;; i32.const 0
    "\40"                      ;; memory.grow
    "\01"                      ;; memory.grow reserved byte is not equal to zero!
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; memory.grow reserved byte should not be a "long" LEB128 zero.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\0a\01"                ;; Code section

    ;; function 0
    "\08\00"
    "\41\00"                   ;; i32.const 0
    "\40"                      ;; memory.grow
    "\80\00"                   ;; memory.grow reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; Same as above for 3, 4, and 5-byte zero encodings.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\0b\01"                ;; Code section

    ;; function 0
    "\09\00"
    "\41\00"                   ;; i32.const 0
    "\40"                      ;; memory.grow
    "\80\80\00"                ;; memory.grow reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\0c\01"                ;; Code section

    ;; function 0
    "\0a\00"
    "\41\00"                   ;; i32.const 0
    "\40"                      ;; memory.grow
    "\80\80\80\00"             ;; memory.grow reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\0d\01"                ;; Code section

    ;; function 0
    "\0b\00"
    "\41\00"                   ;; i32.const 0
    "\40"                      ;; memory.grow
    "\80\80\80\80\00"          ;; memory.grow reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; memory.size reserved byte equal to zero.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\07\01"                ;; Code section

    ;; function 0
    "\05\00"
    "\3f"                      ;; memory.size
    "\01"                      ;; memory.size reserved byte is not equal to zero!
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; memory.size reserved byte should not be a "long" LEB128 zero.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\08\01"                ;; Code section

    ;; function 0
    "\06\00"
    "\3f"                      ;; memory.size
    "\80\00"                   ;; memory.size reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; Same as above for 3, 4, and 5-byte zero encodings.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\09\01"                ;; Code section

    ;; function 0
    "\07\00"
    "\3f"                      ;; memory.size
    "\80\80\00"                ;; memory.size reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\0a\01"                ;; Code section

    ;; function 0
    "\08\00"
    "\3f"                      ;; memory.size
    "\80\80\80\00"             ;; memory.size reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\05\03\01\00\00"          ;; Memory section
    "\0a\0b\01"                ;; Code section

    ;; function 0
    "\09\00"
    "\3f"                      ;; memory.size
    "\80\80\80\80\00"          ;; memory.size reserved byte
    "\1a"                      ;; drop
    "\0b"                      ;; end
  )
  "zero flag expected"
)

;; Local number is unsigned 32 bit
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\0a\0c\01"                ;; Code section

    ;; function 0
    "\0a\02"
    "\80\80\80\80\10\7f"       ;; 0x100000000 i32
    "\02\7e"                   ;; 0x00000002 i64
    "\0b"                      ;; end
  )
  "integer too large"
)

;; No more than 2^32-1 locals.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\0a\0c\01"                ;; Code section

    ;; function 0
    "\0a\02"
    "\ff\ff\ff\ff\0f\7f"       ;; 0xFFFFFFFF i32
    "\02\7e"                   ;; 0x00000002 i64
    "\0b"                      ;; end
  )
  "too many locals"
)

(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\06\01\60\02\7f\7f\00" ;; Type section: (param i32 i32)
    "\03\02\01\00"             ;; Function section
    "\0a\1c\01"                ;; Code section

    ;; function 0
    "\1a\04"
    "\80\80\80\80\04\7f"       ;; 0x40000000 i32
    "\80\80\80\80\04\7e"       ;; 0x40000000 i64
    "\80\80\80\80\04\7d"       ;; 0x40000000 f32
    "\80\80\80\80\04\7c"       ;; 0x40000000 f64
    "\0b"                      ;; end
  )
  "too many locals"
)

;; Local count can be 0.
(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01\60\00\00"     ;; Type section
  "\03\02\01\00"           ;; Function section
  "\0a\0a\01"              ;; Code section

  ;; function 0
  "\08\03"
  "\00\7f"                 ;; 0 i32
  "\00\7e"                 ;; 0 i64
  "\02\7d"                 ;; 2 f32
  "\0b"                    ;; end
)

;; Function section has non-zero count, but code section is absent.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"  ;; Type section
    "\03\03\02\00\00"     ;; Function section with 2 functions
  )
  "function and code section have inconsistent lengths"
)

;; Code section has non-zero count, but function section is absent.
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\0a\04\01\02\00\0b"  ;; Code section with 1 empty function
  )
  "function and code section have inconsistent lengths"
)

;; Function section count > code section count
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"  ;; Type section
    "\03\03\02\00\00"     ;; Function section with 2 functions
    "\0a\04\01\02\00\0b"  ;; Code section with 1 empty function
  )
  "function and code section have inconsistent lengths"
)

;; Function section count < code section count
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"           ;; Type section
    "\03\02\01\00"                 ;; Function section with 1 function
    "\0a\07\02\02\00\0b\02\00\0b"  ;; Code section with 2 empty functions
  )
  "function and code section have inconsistent lengths"
)

;; Function section has zero count, and code section is absent.
(module binary
  "\00asm" "\01\00\00\00"
  "\03\01\00"  ;; Function section with 0 functions
)

;; Code section has zero count, and function section is absent.
(module binary
  "\00asm" "\01\00\00\00"
  "\0a\01\00"  ;; Code section with 0 functions
)

;; Type count can be zero
(module binary
  "\00asm" "\01\00\00\00"
  "\01\01\00"                               ;; type count can be zero
)

;; 2 type declared, 1 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\07\02"                             ;; type section with inconsistent count (2 declared, 1 given)
    "\60\00\00"                             ;; 1st type
    ;; "\60\00\00"                          ;; 2nd type (missed)
  )
  "unexpected end of section or function"
)

;; 1 type declared, 2 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\07\01"                             ;; type section with inconsistent count (1 declared, 2 given)
    "\60\00\00"                             ;; 1st type
    "\60\00\00"                             ;; 2nd type (redundant)
  )
  "section size mismatch"
)

;; Import count can be zero
(module binary
    "\00asm" "\01\00\00\00"
    "\01\05\01"                             ;; type section
    "\60\01\7f\00"                          ;; type 0
    "\02\01\00"                             ;; import count can be zero
)

;; Malformed import kind
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\02\04\01"                           ;; import section with single entry
      "\00"                                 ;; string length 0
      "\00"                                 ;; string length 0
      "\04"                                 ;; malformed import kind
  )
  "malformed import kind"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\02\05\01"                           ;; import section with single entry
      "\00"                                 ;; string length 0
      "\00"                                 ;; string length 0
      "\04"                                 ;; malformed import kind
      "\00"                                 ;; dummy byte
  )
  "malformed import kind"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\02\04\01"                           ;; import section with single entry
      "\00"                                 ;; string length 0
      "\00"                                 ;; string length 0
      "\05"                                 ;; malformed import kind
  )
  "malformed import kind"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\02\05\01"                           ;; import section with single entry
      "\00"                                 ;; string length 0
      "\00"                                 ;; string length 0
      "\05"                                 ;; malformed import kind
      "\00"                                 ;; dummy byte
  )
  "malformed import kind"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\02\04\01"                           ;; import section with single entry
      "\00"                                 ;; string length 0
      "\00"                                 ;; string length 0
      "\80"                                 ;; malformed import kind
  )
  "malformed import kind"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\02\05\01"                           ;; import section with single entry
      "\00"                                 ;; string length 0
      "\00"                                 ;; string length 0
      "\80"                                 ;; malformed import kind
      "\00"                                 ;; dummy byte
  )
  "malformed import kind"
)

;; 2 import declared, 1 given
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\01\05\01"                           ;; type section
      "\60\01\7f\00"                        ;; type 0
      "\02\16\02"                           ;; import section with inconsistent count (2 declared, 1 given)
      ;; 1st import
      "\08"                                 ;; string length
      "\73\70\65\63\74\65\73\74"            ;; spectest
      "\09"                                 ;; string length
      "\70\72\69\6e\74\5f\69\33\32"         ;; print_i32
      "\00\00"                              ;; import kind, import signature index
      ;; 2nd import
      ;; (missed)
  )
  "unexpected end of section or function"
)

;; 1 import declared, 2 given
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\01\09\02"                           ;; type section
      "\60\01\7f\00"                        ;; type 0
      "\60\01\7d\00"                        ;; type 1
      "\02\2b\01"                           ;; import section with inconsistent count (1 declared, 2 given)
      ;; 1st import
      "\08"                                 ;; string length
      "\73\70\65\63\74\65\73\74"            ;; spectest
      "\09"                                 ;; string length
      "\70\72\69\6e\74\5f\69\33\32"         ;; print_i32
      "\00\00"                              ;; import kind, import signature index
      ;; 2nd import
      ;; (redundant)
      "\08"                                 ;; string length
      "\73\70\65\63\74\65\73\74"            ;; spectest
      "\09"                                 ;; string length
      "\70\72\69\6e\74\5f\66\33\32"         ;; print_f32
      "\00\01"                              ;; import kind, import signature index
  )
  "section size mismatch"
)

;; Table count can be zero
(module binary
    "\00asm" "\01\00\00\00"
    "\04\01\00"                             ;; table count can be zero
)

;; 1 table declared, 0 given
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\04\01\01"                           ;; table section with inconsistent count (1 declared, 0 given)
      ;; "\70\01\00\00"                     ;; table entity
  )
  "unexpected end of section or function"
)

;; Malformed table limits flag
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\03\01"                           ;; table section with one entry
      "\70"                                 ;; anyfunc
      "\02"                                 ;; malformed table limits flag
  )
  "integer too large"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\04\01"                           ;; table section with one entry
      "\70"                                 ;; anyfunc
      "\02"                                 ;; malformed table limits flag
      "\00"                                 ;; dummy byte
  )
  "integer too large"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\06\01"                           ;; table section with one entry
      "\70"                                 ;; anyfunc
      "\81\00"                              ;; malformed table limits flag as LEB128
      "\00\00"                              ;; dummy bytes
  )
  "integer too large"
)

;; Memory count can be zero
(module binary
    "\00asm" "\01\00\00\00"
    "\05\01\00"                             ;; memory count can be zero
)

;; 1 memory declared, 0 given
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\01\01"                           ;; memory section with inconsistent count (1 declared, 0 given)
      ;; "\00\00"                           ;; memory 0 (missed)
  )
  "unexpected end of section or function"
)

;; Malformed memory limits flag
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\02\01"                           ;; memory section with one entry
      "\02"                                 ;; malformed memory limits flag
  )
  "integer too large"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\03\01"                           ;; memory section with one entry
      "\02"                                 ;; malformed memory limits flag
      "\00"                                 ;; dummy byte
  )
  "integer too large"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\05\01"                           ;; memory section with one entry
      "\81\00"                              ;; malformed memory limits flag as LEB128
      "\00\00"                              ;; dummy bytes
  )
  "integer representation too long"
)
(assert_malformed
  (module binary
      "\00asm" "\01\00\00\00"
      "\05\05\01"                           ;; memory section with one entry
      "\81\01"                              ;; malformed memory limits flag as LEB128
      "\00\00"                              ;; dummy bytes
  )
  "integer representation too long"
)

;; Global count can be zero
(module binary
  "\00asm" "\01\00\00\00"
  "\06\01\00"                               ;; global count can be zero
)

;; 2 global declared, 1 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\06\06\02"                             ;; global section with inconsistent count (2 declared, 1 given)
    "\7f\00\41\00\0b"                       ;; global 0
    ;; "\7f\00\41\00\0b"                    ;; global 1 (missed)
  )
  "unexpected end of section or function"
)

;; 1 global declared, 2 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\06\0b\01"                             ;; global section with inconsistent count (1 declared, 2 given)
    "\7f\00\41\00\0b"                       ;; global 0
    "\7f\00\41\00\0b"                       ;; global 1 (redundant)
  )
  "section size mismatch"
)

;; Export count can be 0
(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01"                               ;; type section
  "\60\00\00"                               ;; type 0
  "\03\03\02\00\00"                         ;; func section
  "\07\01\00"                               ;; export count can be zero
  "\0a\07\02"                               ;; code section
  "\02\00\0b"                               ;; function body 0
  "\02\00\0b"                               ;; function body 1
)

;; 2 export declared, 1 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                             ;; type section
    "\60\00\00"                             ;; type 0
    "\03\03\02\00\00"                       ;; func section
    "\07\06\02"                             ;; export section with inconsistent count (2 declared, 1 given)
    "\02"                                   ;; export 0
    "\66\31"                                ;; export name
    "\00\00"                                ;; export kind, export func index
    ;; "\02"                                ;; export 1 (missed)
    ;; "\66\32"                             ;; export name
    ;; "\00\01"                             ;; export kind, export func index
    "\0a\07\02"                             ;; code section
    "\02\00\0b"                             ;; function body 0
    "\02\00\0b"                             ;; function body 1
  )
  "unexpected end of section or function"
)

;; 1 export declared, 2 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                             ;; type section
    "\60\00\00"                             ;; type 0
    "\03\03\02\00\00"                       ;; func section
    "\07\0b\01"                             ;; export section with inconsistent count (1 declared, 2 given)
    "\02"                                   ;; export 0
    "\66\31"                                ;; export name
    "\00\00"                                ;; export kind, export func index
    "\02"                                   ;; export 1 (redundant)
    "\66\32"                                ;; export name
    "\00\01"                                ;; export kind, export func index
    "\0a\07\02"                             ;; code section
    "\02\00\0b"                             ;; function body 0
    "\02\00\0b"                             ;; function body 1
  )
  "section size mismatch"
)

;; elem segment count can be zero
(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01"                               ;; type section
  "\60\00\00"                               ;; type 0
  "\03\02\01\00"                            ;; func section
  "\04\04\01"                               ;; table section
  "\70\00\01"                               ;; table 0
  "\09\01\00"                               ;; elem segment count can be zero
  "\0a\04\01"                               ;; code section
  "\02\00\0b"                               ;; function body
)

;; 2 elem segment declared, 1 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                             ;; type section
    "\60\00\00"                             ;; type 0
    "\03\02\01\00"                          ;; func section
    "\04\04\01"                             ;; table section
    "\70\00\01"                             ;; table 0
    "\09\07\02"                             ;; elem with inconsistent segment count (2 declared, 1 given)
    "\00\41\00\0b\01\00"                    ;; elem 0
    ;; "\00\41\00\0b\01\00"                 ;; elem 1 (missed)
  )
  "unexpected end"
)

;; 2 elem segment declared, 1.5 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                             ;; type section
    "\60\00\00"                             ;; type 0
    "\03\02\01\00"                          ;; func section
    "\04\04\01"                             ;; table section
    "\70\00\01"                             ;; table 0
    "\09\07\02"                             ;; elem with inconsistent segment count (2 declared, 1 given)
    "\00\41\00\0b\01\00"                    ;; elem 0
    "\00\41\00"                             ;; elem 1 (partial)
    ;; "\0b\01\00"                          ;; elem 1 (missing part)
  )
  "unexpected end"
)

;; 1 elem segment declared, 2 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                             ;; type section
    "\60\00\00"                             ;; type 0
    "\03\02\01\00"                          ;; func section
    "\04\04\01"                             ;; table section
    "\70\00\01"                             ;; table 0
    "\09\0d\01"                             ;; elem with inconsistent segment count (1 declared, 2 given)
    "\00\41\00\0b\01\00"                    ;; elem 0
    "\00\41\00\0b\01\00"                    ;; elem 1 (redundant)
    "\0a\04\01"                             ;; code section
    "\02\00\0b"                             ;; function body
  )
  "section size mismatch"
)

;; data segment count can be zero
(module binary
  "\00asm" "\01\00\00\00"
  "\05\03\01"                               ;; memory section
  "\00\01"                                  ;; memory 0
  "\0b\01\00"                               ;; data segment count can be zero
)

;; 2 data segment declared, 1 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\03\01"                             ;; memory section
    "\00\01"                                ;; memory 0
    "\0b\07\02"                             ;; data with inconsistent segment count (2 declared, 1 given)
    "\00\41\00\0b\01\61"                    ;; data 0
    ;; "\00\41\01\0b\01\62"                 ;; data 1 (missed)
  )
  "unexpected end of section or function"
)

;; 1 data segment declared, 2 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\03\01"                             ;; memory section
    "\00\01"                                ;; memory 0
    "\0b\0d\01"                             ;; data with inconsistent segment count (1 declared, 2 given)
    "\00\41\00\0b\01\61"                    ;; data 0
    "\00\41\01\0b\01\62"                    ;; data 1 (redundant)
  )
  "section size mismatch"
)

;; data segment has 7 bytes declared, but 6 bytes given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\03\01"                             ;; memory section
    "\00\01"                                ;; memory 0
    "\0b\0c\01"                             ;; data section
    "\00\41\03\0b"                          ;; data segment 0
    "\07"                                   ;; data segment size with inconsistent lengths (7 declared, 6 given)
    "\61\62\63\64\65\66"                    ;; 6 bytes given
  )
  "unexpected end of section or function"
)

;; data segment has 5 bytes declared, but 6 bytes given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\05\03\01"                             ;; memory section
    "\00\01"                                ;; memory 0
    "\0b\0c\01"                             ;; data section
    "\00\41\00\0b"                          ;; data segment 0
    "\05"                                   ;; data segment size with inconsistent lengths (5 declared, 6 given)
    "\61\62\63\64\65\66"                    ;; 6 bytes given
  )
  "section size mismatch"
)

;; br_table target count can be zero
(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01"                               ;; type section
  "\60\00\00"                               ;; type 0
  "\03\02\01\00"                            ;; func section
  "\0a\11\01"                               ;; code section
  "\0f\00"                                  ;; func 0
  "\02\40"                                  ;; block 0
  "\41\01"                                  ;; condition of if 0
  "\04\40"                                  ;; if 0
  "\41\01"                                  ;; index of br_table element
  "\0e\00"                                  ;; br_table target count can be zero
  "\02"                                     ;; break depth for default
  "\0b\0b\0b"                               ;; end
)

;; 1 br_table target declared, 2 given
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01"                             ;; type section
    "\60\00\00"                             ;; type 0
    "\03\02\01\00"                          ;; func section
    "\0a\12\01"                             ;; code section
    "\11\00"                                ;; func 0
    "\02\40"                                ;; block 0
    "\41\01"                                ;; condition of if 0
    "\04\40"                                ;; if 0
    "\41\01"                                ;; index of br_table element
    "\0e\01"                                ;; br_table with inconsistent target count (1 declared, 2 given)
    "\00"                                   ;; break depth 0
    "\01"                                   ;; break depth 1
    "\02"                                   ;; break depth for default
    "\0b\0b\0b"                             ;; end
  )
  "unexpected end"
)

;; Start section
(module binary
  "\00asm" "\01\00\00\00"
  "\01\04\01\60\00\00"       ;; Type section
  "\03\02\01\00"             ;; Function section
  "\08\01\00"                ;; Start section: function 0

  "\0a\04\01"                ;; Code section
  ;; function 0
  "\02\00"
  "\0b"                      ;; end
)

;; Multiple start sections
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\01\04\01\60\00\00"       ;; Type section
    "\03\02\01\00"             ;; Function section
    "\08\01\00"                ;; Start section: function 0
    "\08\01\00"                ;; Start section: function 0

    "\0a\04\01"                ;; Code section
    ;; function 0
    "\02\00"
    "\0b"                      ;; end
  )
  "junk after last section"
)
