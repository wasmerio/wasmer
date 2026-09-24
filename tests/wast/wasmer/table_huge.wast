(module
  (table 1 10000000 funcref)
)

(assert_unlinkable
  (module
    (table 10000000 funcref)
  )
  "Insufficient resources: Total fixed table size (10000000) is larger than maximum allowed size (1000000)!"
)

(assert_unlinkable
  (module
    (table 10000000 10000000 funcref)
  )
  "Insufficient resources: Total fixed table size (10000000) is larger than maximum allowed size (1000000)!"
)
