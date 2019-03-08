#! /bin/bash

nohup ./target/release/wasmer run examples/lua.wasm --disable-cache -- -v

if grep "Lua 5.4.0  Copyright (C) 1994-2018 Lua.org, PUC-Rio" ./nohup.out
then
    echo "lua integration test succeeded"
    rm ./nohup.out
    exit 0
else
    echo "lua integration test failed"
    rm ./nohup.out
    exit -1
fi
