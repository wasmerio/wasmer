(module
  (rec
    (type $t1 (func (param i32 (ref $t3))))
    (type $t2 (func (param i32 (ref $t1))))
    (type $t3 (func (param i32 (ref $t2))))
  )
)

(module
  (rec
    (type $t0 (func (param i32 (ref $t2) (ref $t3))))
    (type $t1 (func (param i32 (ref $t0) i32 (ref $t4))))
    (type $t2 (func (param i32 (ref $t2) (ref $t1))))
    (type $t3 (func (param i32 (ref $t2) i32 (ref $t4))))
    (type $t4 (func (param (ref $t0) (ref $t2))))
  )
)
