(module
  (type $run_t (func (param i32 i32) (result i32)))
  (type $early_exit_t (func (param) (result)))
  (import "env" "early_exit" (func $early_exit (type $early_exit_t)))
  (func $run (type $run_t) (param $x i32) (param $y i32) (result i32)
    (call $early_exit)
    (i32.add
        (local.get $x)
        (local.get $y)))
  (export "run" (func $run)))

