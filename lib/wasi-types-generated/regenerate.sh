#!/bin/bash

wit-bindgen wasmer --import wit/wasi-snapshot0.wit wit/wasi-filesystem.wit --out-dir src/
