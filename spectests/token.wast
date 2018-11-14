;; Test tokenization

(assert_malformed
  (module quote "(func (drop (i32.const0)))")
  "unknown operator"
)
(assert_malformed
  (module quote "(func br 0drop)")
  "unknown operator"
)
