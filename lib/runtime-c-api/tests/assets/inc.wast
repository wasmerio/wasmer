(module
  (func $inc (import "env" "inc"))
  (func $get (import "env" "get") (result i32))

  (func (export "inc_and_get") (result i32)
      call $inc
      call $get))
