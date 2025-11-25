$WASMER -q run main.wasm > output

# Check that the process exited with code 0 (alarm was triggered)
if [ $? -eq 0 ]; then
    exit 0
else
    exit 1
fi
