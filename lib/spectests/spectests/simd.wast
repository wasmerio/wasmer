(module (func (export "v128.bitselect") (param $a v128) (param $b v128) (param $c v128) (result v128) (v128.bitselect (local.get $a) (local.get $b) (local.get $c))))
