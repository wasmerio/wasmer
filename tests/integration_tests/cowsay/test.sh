#! /bin/bash

nohup ./target/release/wasmer run examples/cowsay.wasm --disable-cache -- "hello integration"

if grep "hello integration" ./nohup.out
then
    echo "cowsay wasi integration test succeeded"
    rm ./nohup.out
    exit 0
else
    echo "cowsay wasi integration test failed"
    rm ./nohup.out
    exit -1
fi
