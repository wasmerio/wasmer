;; Renamed in https://github.com/WebAssembly/spec/pull/720
(assert_malformed
  (module quote
    "(memory 1)"
    "(func (drop (current_memory)))"
  )
  "unknown operator current_memory"
)

(assert_malformed
  (module quote
    "(memory 1)"
    "(func (drop (grow_memory (i32.const 0))))"
  )
  "unknown operator grow_memory"
)

;; Renamed in https://github.com/WebAssembly/spec/pull/926
(assert_malformed
  (module quote
    "(func (local $i i32) (drop (get_local $i)))"
  )
  "unknown operator get_local"
)

(assert_malformed
  (module quote
    "(func (local $i i32) (set_local $i (i32.const 0)))"
  )
  "unknown operator set_local"
)

(assert_malformed
  (module quote
    "(func (local $i i32) (drop (tee_local $i (i32.const 0))))"
  )
  "unknown operator tee_local"
)

(assert_malformed
  (module quote
    "(global $g anyfunc (ref.null func))"
  )
  "unknown operator anyfunc"
)

(assert_malformed
  (module quote
    "(global $g i32 (i32.const 0))"
    "(func (drop (get_global $g)))"
  )
  "unknown operator get_global"
)

(assert_malformed
  (module quote
    "(global $g (mut i32) (i32.const 0))"
    "(func (set_global $g (i32.const 0)))"
  )
  "unknown operator set_global"
)

(assert_malformed
  (module quote
    "(func (drop (i32.wrap/i64 (i64.const 0))))"
  )
  "unknown operator i32.wrap/i64"
)

(assert_malformed
  (module quote
    "(func (drop (i32.trunc_s:sat/f32 (f32.const 0))))"
  )
  "unknown operator i32.trunc_s:sat/f32"
)

(assert_malformed
  (module quote
    "(func (drop (f32x4.convert_s/i32x4 (v128.const i64x2 0 0))))"
  )
  "unknown operator f32x4.convert_s/i32x4"
)
