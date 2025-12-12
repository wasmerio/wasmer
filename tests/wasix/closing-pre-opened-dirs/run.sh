rm output 2>/dev/null

$WASMER_RUN main.wasm --dir . > output

printf "0" | diff -u output - 1>/dev/null
