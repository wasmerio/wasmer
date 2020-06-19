(wasi_test "readlink.wasm"
  (map_dirs ".:test_fs/hamlet")
  (assert_return (i64.const 0))
  (assert_stdout "../act1/scene2.txt\nSCENE II. A room of state in the castle.\n\n    Enter KING CLAUDIUS, QUEEN GERTRUDE, HAMLET, POLONIUS, LAERTES, VOLTIMAND, CORNELI\n")
)