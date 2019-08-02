(module
  (func $inc (import "env" "inc"))
  (func $mul (import "env" "mul"))
  (func $get (import "env" "get") (result i32))

  (func (export "inc_and_get") (result i32)
      call $inc
      call $get)

  (func (export "mul_and_get") (result i32)
      call $mul
      call $get))
