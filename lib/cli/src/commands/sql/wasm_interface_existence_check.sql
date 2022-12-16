SELECT 1
FROM wasm_interfaces
WHERE interface_name = (?1) 
   AND version = (?2)
LIMIT 1
