#!/usr/bin/env bash
curl -O https://download.libsodium.org/libsodium/releases/libsodium-1.0.17.tar.gz
tar xf libsodium-1.0.17.tar.gz
cd libsodium-1.0.17/
./configure
make && make check
sudo make install
