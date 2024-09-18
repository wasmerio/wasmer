;; Test tokenization

(assert_malformed
  (module quote "(func (drop (i32.const0)))")
  "unknown operator"
)
(assert_malformed
  (module quote "(func br 0drop)")
  "unknown operator"
)


;; Tokens can be delimited by parentheses

(module
  (func(nop))
)
(module
  (func (nop)nop)
)
(module
  (func nop(nop))
)
(module
  (func(nop)(nop))
)
(module
  (func $f(nop))
)
(module
  (func br 0(nop))
)
(module
  (table 1 funcref)
  (func)
  (elem (i32.const 0)0)
)
(module
  (table 1 funcref)
  (func $f)
  (elem (i32.const 0)$f)
)
(module
  (memory 1)
  (data (i32.const 0)"a")
)
(module
  (import "spectest" "print"(func))
)


;; Tokens can be delimited by comments

(module
  (func;;bla
  )
)
(module
  (func (nop);;bla
  )
)
(module
  (func nop;;bla
  )
)
(module
  (func $f;;bla
  )
)
(module
  (func br 0;;bla
  )
)
(module
  (data "a";;bla
  )
)


;; Space required between symbols and non-parenthesis tokens

(module
  (func (block $l (i32.const 0) (br_table 0 $l)))
)
(assert_malformed
  (module quote
    "(func (block $l (i32.const 0) (br_table 0$l)))"
  )
  "unknown operator"
)

(module
  (func (block $l (i32.const 0) (br_table $l 0)))
)
(assert_malformed
  (module quote
    "(func (block $l (i32.const 0) (br_table $l0)))"
  )
  "unknown label"
)

(module
  (func (block $l (i32.const 0) (br_table $l $l)))
)
(assert_malformed
  (module quote
    "(func (block $l (i32.const 0) (br_table $l$l)))"
  )
  "unknown label"
)

(module
  (func (block $l0 (i32.const 0) (br_table $l0)))
)
(module
  (func (block $l$l (i32.const 0) (br_table $l$l)))
)


;; Space required between strings and non-parenthesis tokens

(module
  (data "a")
)
(assert_malformed
  (module quote
    "(data\"a\")"
  )
  "unknown operator"
)

(module
  (data $l "a")
)
(assert_malformed
  (module quote
    "(data $l\"a\")"
  )
  "unknown operator"
)

(module
  (data $l " a")
)
(assert_malformed
  (module quote
    "(data $l\" a\")"
  )
  "unknown operator"
)

(module
  (data $l "a ")
)
(assert_malformed
  (module quote
    "(data $l\"a \")"
  )
  "unknown operator"
)

(module
  (data $l "a " "b")
)
(assert_malformed
  (module quote
    "(data $l\"a \"\"b\")"
  )
  "unknown operator"
)

(module
  (data $l "")
)
(assert_malformed
  (module quote
    "(data $l\"\")"
  )
  "unknown operator"
)

(module
  (data $l " ")
)
(assert_malformed
  (module quote
    "(data $l\" \")"
  )
  "unknown operator"
)

(module
  (data $l " ")
)
(assert_malformed
  (module quote
    "(data $l\" \")"
  )
  "unknown operator"
)

(module
  (data "a" "b")
)
(assert_malformed
  (module quote
    "(data \"a\"\"b\")"
  )
  "unknown operator"
)

(module
  (data "a" " b")
)
(assert_malformed
  (module quote
    "(data \"a\"\" b\")"
  )
  "unknown operator"
)

(module
  (data "a " "b")
)
(assert_malformed
  (module quote
    "(data \"a \"\"b\")"
  )
  "unknown operator"
)

(module
  (data "" "")
)
(assert_malformed
  (module quote
    "(data \"\"\"\")"
  )
  "unknown operator"
)

(module
  (data "" " ")
)
(assert_malformed
  (module quote
    "(data \"\"\" \")"
  )
  "unknown operator"
)

(module
  (data " " "")
)
(assert_malformed
  (module quote
    "(data \" \"\"\")"
  )
  "unknown operator"
)


(assert_malformed
  (module quote
    "(func \"a\"x)"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(func \"a\"0)"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(func 0\"a\")"
  )
  "unknown operator"
)
(assert_malformed
  (module quote
    "(func \"a\"$x)"
  )
  "unknown operator"
)
