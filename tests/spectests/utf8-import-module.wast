;;;;;; Invalid UTF-8 import module names

;;;; Continuation bytes not preceded by prefixes

;; encoding starts with (first) continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\80"                       ;; "\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; encoding starts with (0x8f) continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\8f"                       ;; "\8f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; encoding starts with (0x90) continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\90"                       ;; "\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; encoding starts with (0x9f) continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\9f"                       ;; "\9f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; encoding starts with (0xa0) continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\a0"                       ;; "\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; encoding starts with (last) continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\bf"                       ;; "\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 2-byte sequences

;; 2-byte sequence contains 3 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\c2\80\80"                 ;; "\c2\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 2-byte sequence contains 1 byte at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\c2"                       ;; "\c2"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 2-byte sequence contains 1 byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c2\2e"                    ;; "\c2."
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 2-byte sequence contents

;; overlong encoding after 0xc0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c0\80"                    ;; "\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xc0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c0\bf"                    ;; "\c0\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xc1 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c1\80"                    ;; "\c1\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xc1 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c1\bf"                    ;; "\c1\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (first) 2-byte prefix not a contination byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c2\00"                    ;; "\c2\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (first) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c2\7f"                    ;; "\c2\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (first) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c2\c0"                    ;; "\c2\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (first) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\c2\fd"                    ;; "\c2\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (last) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\df\00"                    ;; "\df\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (last) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\df\7f"                    ;; "\df\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (last) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\df\c0"                    ;; "\df\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte after (last) 2-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\df\fd"                    ;; "\df\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 3-byte sequences

;; 3-byte sequence contains 4 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\e1\80\80\80"              ;; "\e1\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 3-byte sequence contains 2 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\e1\80"                    ;; "\e1\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 3-byte sequence contains 2 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\80\2e"                 ;; "\e1\80."
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 3-byte sequence contains 1 byte at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\e1"                       ;; "\e1"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 3-byte sequence contains 1 byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\e1\2e"                    ;; "\e1."
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 3-byte sequence contents

;; first byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\00\a0"                 ;; "\e0\00\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\7f\a0"                 ;; "\e0\7f\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xe0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\80\80"                 ;; "\e0\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xe0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\80\a0"                 ;; "\e0\80\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xe0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\9f\a0"                 ;; "\e0\9f\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xe0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\9f\bf"                 ;; "\e0\9f\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\c0\a0"                 ;; "\e0\c0\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\fd\a0"                 ;; "\e0\fd\a0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\00\80"                 ;; "\e1\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\7f\80"                 ;; "\e1\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\c0\80"                 ;; "\e1\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\fd\80"                 ;; "\e1\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\00\80"                 ;; "\ec\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\7f\80"                 ;; "\ec\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\c0\80"                 ;; "\ec\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\fd\80"                 ;; "\ec\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\00\80"                 ;; "\ed\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\7f\80"                 ;; "\ed\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte sequence reserved for UTF-16 surrogate half
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\a0\80"                 ;; "\ed\a0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte sequence reserved for UTF-16 surrogate half
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\a0\bf"                 ;; "\ed\a0\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte sequence reserved for UTF-16 surrogate half
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\bf\80"                 ;; "\ed\bf\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; byte sequence reserved for UTF-16 surrogate half
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\bf\bf"                 ;; "\ed\bf\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\c0\80"                 ;; "\ed\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\fd\80"                 ;; "\ed\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\00\80"                 ;; "\ee\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\7f\80"                 ;; "\ee\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\c0\80"                 ;; "\ee\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\fd\80"                 ;; "\ee\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\00\80"                 ;; "\ef\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\7f\80"                 ;; "\ef\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\c0\80"                 ;; "\ef\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\fd\80"                 ;; "\ef\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 3-byte sequence contents (third byte)

;; second byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\a0\00"                 ;; "\e0\a0\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\a0\7f"                 ;; "\e0\a0\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\a0\c0"                 ;; "\e0\a0\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xe0) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e0\a0\fd"                 ;; "\e0\a0\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\80\00"                 ;; "\e1\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\80\7f"                 ;; "\e1\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\80\c0"                 ;; "\e1\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\e1\80\fd"                 ;; "\e1\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\80\00"                 ;; "\ec\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\80\7f"                 ;; "\ec\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\80\c0"                 ;; "\ec\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ec\80\fd"                 ;; "\ec\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\80\00"                 ;; "\ed\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\80\7f"                 ;; "\ed\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\80\c0"                 ;; "\ed\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xed) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ed\80\fd"                 ;; "\ed\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\80\00"                 ;; "\ee\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\80\7f"                 ;; "\ee\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\80\c0"                 ;; "\ee\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ee\80\fd"                 ;; "\ee\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\80\00"                 ;; "\ef\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\80\7f"                 ;; "\ef\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\80\c0"                 ;; "\ef\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 3-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\ef\80\fd"                 ;; "\ef\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 4-byte sequences

;; 4-byte sequence contains 5 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0f"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\05\f1\80\80\80\80"           ;; "\f1\80\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 4-byte sequence contains 3 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\f1\80\80"                 ;; "\f1\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 4-byte sequence contains 3 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\80\23"              ;; "\f1\80\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 4-byte sequence contains 2 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\f1\80"                    ;; "\f1\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 4-byte sequence contains 2 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\f1\80\23"                 ;; "\f1\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 4-byte sequence contains 1 byte at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\f1"                       ;; "\f1"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 4-byte sequence contains 1 byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\f1\23"                    ;; "\f1#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 4-byte sequence contents

;; first byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\00\90\90"              ;; "\f0\00\90\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\7f\90\90"              ;; "\f0\7f\90\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xf0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\80\80\80"              ;; "\f0\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xf0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\80\90\90"              ;; "\f0\80\90\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xf0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\8f\90\90"              ;; "\f0\8f\90\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; overlong encoding after 0xf0 prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\8f\bf\bf"              ;; "\f0\8f\bf\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\c0\90\90"              ;; "\f0\c0\90\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\fd\90\90"              ;; "\f0\fd\90\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\00\80\80"              ;; "\f1\00\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\7f\80\80"              ;; "\f1\7f\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\c0\80\80"              ;; "\f1\c0\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\fd\80\80"              ;; "\f1\fd\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\00\80\80"              ;; "\f3\00\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\7f\80\80"              ;; "\f3\7f\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\c0\80\80"              ;; "\f3\c0\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\fd\80\80"              ;; "\f3\fd\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\00\80\80"              ;; "\f4\00\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\7f\80\80"              ;; "\f4\7f\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; (first) invalid code point
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\90\80\80"              ;; "\f4\90\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; invalid code point
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\bf\80\80"              ;; "\f4\bf\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\c0\80\80"              ;; "\f4\c0\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; first byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\fd\80\80"              ;; "\f4\fd\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; (first) invalid 4-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f5\80\80\80"              ;; "\f5\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; (last) invalid 4-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f7\80\80\80"              ;; "\f7\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; (last) invalid 4-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f7\bf\bf\bf"              ;; "\f7\bf\bf\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 4-byte sequence contents (third byte)

;; second byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\00\90"              ;; "\f0\90\00\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\7f\90"              ;; "\f0\90\7f\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\c0\90"              ;; "\f0\90\c0\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\fd\90"              ;; "\f0\90\fd\90"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\00\80"              ;; "\f1\80\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\7f\80"              ;; "\f1\80\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\c0\80"              ;; "\f1\80\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\fd\80"              ;; "\f1\80\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\00\80"              ;; "\f3\80\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\7f\80"              ;; "\f3\80\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\c0\80"              ;; "\f3\80\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\fd\80"              ;; "\f3\80\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\00\80"              ;; "\f4\80\00\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\7f\80"              ;; "\f4\80\7f\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\c0\80"              ;; "\f4\80\c0\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; second byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\fd\80"              ;; "\f4\80\fd\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 4-byte sequence contents (fourth byte)

;; third byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\90\00"              ;; "\f0\90\90\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\90\7f"              ;; "\f0\90\90\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\90\c0"              ;; "\f0\90\90\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf0) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f0\90\90\fd"              ;; "\f0\90\90\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\80\00"              ;; "\f1\80\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\80\7f"              ;; "\f1\80\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\80\c0"              ;; "\f1\80\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (first normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f1\80\80\fd"              ;; "\f1\80\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\80\00"              ;; "\f3\80\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\80\7f"              ;; "\f3\80\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\80\c0"              ;; "\f3\80\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (last normal) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f3\80\80\fd"              ;; "\f3\80\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\80\00"              ;; "\f4\80\80\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\80\7f"              ;; "\f4\80\80\7f"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\80\c0"              ;; "\f4\80\80\c0"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; third byte after (0xf4) 4-byte prefix not a continuation byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f4\80\80\fd"              ;; "\f4\80\80\fd"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 5-byte sequences

;; 5-byte sequence contains 6 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\10"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\06\f8\80\80\80\80\80"        ;; "\f8\80\80\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 4 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f8\80\80\80"              ;; "\f8\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 4 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0f"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\05\f8\80\80\80\23"           ;; "\f8\80\80\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 3 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\f8\80\80"                 ;; "\f8\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 3 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\f8\80\80\23"              ;; "\f8\80\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 2 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\f8\80"                    ;; "\f8\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 2 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\f8\80\23"                 ;; "\f8\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 1 byte at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\f8"                       ;; "\f8"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 5-byte sequence contains 1 byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\f8\23"                    ;; "\f8#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 5-byte sequence contents

;; (first) invalid 5-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0f"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\05\f8\80\80\80\80"           ;; "\f8\80\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; (last) invalid 5-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0f"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\05\fb\bf\bf\bf\bf"           ;; "\fb\bf\bf\bf\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 6-byte sequences

;; 6-byte sequence contains 7 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\11"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\07\fc\80\80\80\80\80\80"     ;; "\fc\80\80\80\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 5 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0f"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\05\fc\80\80\80\80"           ;; "\fc\80\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 5 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\10"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\06\fc\80\80\80\80\23"        ;; "\fc\80\80\80\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 4 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\fc\80\80\80"              ;; "\fc\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 4 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0f"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\05\fc\80\80\80\23"           ;; "\fc\80\80\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 3 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\fc\80\80"                 ;; "\fc\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 3 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\fc\80\80\23"              ;; "\fc\80\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 2 bytes at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\fc\80"                    ;; "\fc\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 2 bytes
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0d"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\03\fc\80\23"                 ;; "\fc\80#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 1 byte at end of string
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\fc"                       ;; "\fc"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; 6-byte sequence contains 1 byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\fc\23"                    ;; "\fc#"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; 6-byte sequence contents

;; (first) invalid 6-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\10"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\06\fc\80\80\80\80\80"        ;; "\fc\80\80\80\80\80"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; (last) invalid 6-byte prefix
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\10"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\06\fd\bf\bf\bf\bf\bf"        ;; "\fd\bf\bf\bf\bf\bf"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;;;; Miscellaneous invalid bytes

;; invalid byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\fe"                       ;; "\fe"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; invalid byte
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0b"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\01\ff"                       ;; "\ff"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; UTF-16BE BOM
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\fe\ff"                    ;; "\fe\ff"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; UTF-32BE BOM
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\00\00\fe\ff"              ;; "\00\00\fe\ff"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; UTF-16LE BOM
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0c"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\02\ff\fe"                    ;; "\ff\fe"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

;; UTF-32LE BOM
(assert_malformed
  (module binary
    "\00asm" "\01\00\00\00"
    "\02\0e"                       ;; import section
    "\01"                          ;; length 1
    "\04\74\65\73\74"              ;; "test"
    "\04\ff\fe\00\00"              ;; "\ff\fe\00\00"
    "\03"                          ;; GlobalImport
    "\7f"                          ;; i32
    "\00"                          ;; immutable
  )
  "invalid UTF-8 encoding"
)

