;; Memories

(module (memory 0) (export "a" (memory 0)))
(module (memory 0) (export "a" (memory 0)) (export "b" (memory 0)))
(module (memory 0) (memory 0) (export "a" (memory 0)) (export "b" (memory 1)))
(module
  (memory $mem0 0)
  (memory $mem1 0)
  (memory $mem2 0)
  (memory $mem3 0)
  (memory $mem4 0)
  (memory $mem5 0)
  (memory $mem6 0)
  
  (export "a" (memory $mem0))
  (export "b" (memory $mem1))
  (export "ac" (memory $mem2))
  (export "bc" (memory $mem3))
  (export "ad" (memory $mem4))
  (export "bd" (memory $mem5))
  (export "be" (memory $mem6))
  
  (export "za" (memory $mem0))
  (export "zb" (memory $mem1))
  (export "zac" (memory $mem2))
  (export "zbc" (memory $mem3))
  (export "zad" (memory $mem4))
  (export "zbd" (memory $mem5))
  (export "zbe" (memory $mem6))
)

(module
  (export "a" (memory 0))
  (memory 6)

  (export "b" (memory 1))
  (memory 3)
)

(module
  (export "a" (memory 0))
  (memory 0 1)
  (memory 0 1)
  (memory 0 1)
  (memory 0 1)

  (export "b" (memory 3))
)
(module (export "a" (memory $a)) (memory $a 0))
(module (export "a" (memory $a)) (memory $a 0 1))

