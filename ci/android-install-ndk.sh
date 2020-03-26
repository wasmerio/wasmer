#!/usr/bin/env sh
# Copyright 2016 The Rust Project Developers. See the COPYRIGHT
# file at the top-level directory of this distribution and at
# http://rust-lang.org/COPYRIGHT.
#
# Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
# http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
# <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
# option. This file may not be copied, modified, or distributed
# except according to those terms.

set -ex

NDK=android-ndk-r19c
curl --retry 20 -O https://dl.google.com/android/repository/${NDK}-linux-x86_64.zip
unzip -q -d ndk ${NDK}-linux-x86_64.zip
mv ./ndk/"$NDK"/* ./ndk/

rm -rf ./${NDK}-linux-x86_64.zip
