(module
  (type $t (func))
  (func (export "externref") (result externref) (ref.null extern))
  (func (export "funcref") (result funcref) (ref.null func))
  (func (export "ref") (result (ref null $t)) (ref.null $t))

  (global externref (ref.null extern))
  (global funcref (ref.null func))
  (global (ref null $t) (ref.null $t))
)

(assert_return (invoke "externref") (ref.null extern))
(assert_return (invoke "funcref") (ref.null func))
(assert_return (invoke "ref") (ref.null))
