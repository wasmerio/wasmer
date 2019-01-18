#! /bin/bash

nohup ./target/release/wasmer run examples/nginx/nginx.wasm -- -p integration_tests/nginx/ -c nginx.conf &
sleep 3s

curl localhost:8080 > ./nginx.out


if grep "wasmer" ./nginx.out
then
    echo "nginx integration test succeeded"
    rm ./nohup.out
    rm ./nginx.out
    rm -rf ./integration_tests/nginx/*_temp
    exit 0
else
    echo "nginx integration test failed"
    rm ./nohup.out
    rm ./nginx.out
    rm -rf ./integration_tests/nginx/*_temp
    exit -1
fi
