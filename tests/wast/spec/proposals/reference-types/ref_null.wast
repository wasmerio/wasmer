(module
  (func (export "externref") (result externref) (ref.null extern))
  (func (export "funcref") (result funcref) (ref.null func))

  (global externref (ref.null extern))
  (global funcref (ref.null func))
)

(assert_return (invoke "externref") (ref.null extern))
(assert_return (invoke "funcref") (ref.null func))
