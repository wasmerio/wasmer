(module
  (func (export "anyref") (result anyref) (ref.null))
  (func (export "funcref") (result funcref) (ref.null))
  (func (export "nullref") (result nullref) (ref.null))

  (global anyref (ref.null))
  (global funcref (ref.null))
  (global nullref (ref.null))
)

(assert_return (invoke "anyref") (ref.null))
(assert_return (invoke "funcref") (ref.null))
(assert_return (invoke "nullref") (ref.null))
