TEMP_DIR1=$(mktemp -d)
TEMP_DIR2=$(mktemp -d)

$WASMER_RUN main.wasm --volume $TEMP_DIR1:/temp1 --volume $TEMP_DIR2:/temp2 > output

printf "0" | diff -u output - 1>/dev/null
