TEMP_DIR1=$(mktemp -d)
TEMP_DIR2=$(mktemp -d)

$WASMER -q run main.wasm --mapdir /temp1:$TEMP_DIR1 --mapdir /temp2:$TEMP_DIR2 > output

printf "0" | diff -u output - 1>/dev/null
