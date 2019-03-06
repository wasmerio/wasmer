#! /bin/bash

nohup ./target/release/wasmer run examples/nginx/nginx.wasm --disable-cache -- -v

if grep "nginx version: nginx/1.15.3" ./nohup.out
then
    echo "nginx integration test succeeded"
    rm ./nohup.out
    exit 0
else
    echo "nginx integration test failed"
    rm ./nohup.out
    exit -1
fi
