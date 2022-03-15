#! /usr/bin/env python3

# This is a script for publishing the wasmer crates to crates.io.
# It should be run in the root of wasmer like `python3 scripts/publish.py --no-dry-run`.
# By default the script executes a test run and does not publish the crates to crates.io.

# install dependencies:
# pip3 install toposort

import argparse
import os
import re
import subprocess
import time

from typing import Optional
try:
    from toposort import toposort_flatten
except ImportError:
    print("Please install toposort, `pip3 install toposort`")


# TODO: find this automatically
target_version = "2.2.1"

# TODO: generate this by parsing toml files
dep_graph = {
    "wasmer-types": set([]),
    "wasmer-derive": set([]),
    "wasmer-vm": set(["wasmer-types"]),
    "wasmer-compiler": set(["wasmer-vm", "wasmer-types"]),
    "wasmer-object": set(["wasmer-types", "wasmer-compiler"]),
    "wasmer-engine": set(["wasmer-types", "wasmer-vm", "wasmer-compiler"]),
    "wasmer-compiler-singlepass": set(["wasmer-types", "wasmer-vm", "wasmer-compiler"]),
    "wasmer-compiler-cranelift": set(["wasmer-types", "wasmer-vm", "wasmer-compiler"]),
    "wasmer-compiler-llvm": set(["wasmer-types", "wasmer-vm", "wasmer-compiler"]),
    "wasmer-engine-universal": set(["wasmer-types", "wasmer-vm", "wasmer-compiler", "wasmer-engine"]),
    "wasmer-engine-dylib": set(["wasmer-types", "wasmer-vm", "wasmer-compiler", "wasmer-engine",
                                 "wasmer-object"]),
    "wasmer-engine-staticlib": set(["wasmer-types", "wasmer-vm", "wasmer-compiler", "wasmer-engine",
                                      "wasmer-object"]),
    "wasmer": set(["wasmer-vm", "wasmer-compiler-singlepass", "wasmer-compiler-cranelift",
                   "wasmer-compiler-llvm", "wasmer-compiler", "wasmer-engine", "wasmer-engine-universal",
                   "wasmer-engine-dylib", "wasmer-engine-staticlib", "wasmer-types", "wasmer-derive"]),
    "wasmer-vfs": set([]),
    "wasmer-cache": set(["wasmer"]),
    "wasmer-wasi": set(["wasmer", "wasmer-wasi-types", "wasmer-vfs"]),
    "wasmer-wasi-types": set(["wasmer-types"]),
    "wasmer-wasi-experimental-io-devices": set(["wasmer-wasi"]),
    "wasmer-emscripten": set(["wasmer"]),
    "wasmer-c-api": set(["wasmer", "wasmer-compiler", "wasmer-compiler-cranelift", "wasmer-compiler-singlepass",
                         "wasmer-compiler-llvm", "wasmer-emscripten", "wasmer-engine", "wasmer-engine-universal",
                         "wasmer-engine-dylib", "wasmer-engine-staticlib", "wasmer-wasi", "wasmer-types"]),
    "wasmer-middlewares": set(["wasmer", "wasmer-types", "wasmer-vm"]),
    "wasmer-wast": set(["wasmer", "wasmer-wasi", "wasmer-vfs"]),
    "wasmer-cli": set(["wasmer", "wasmer-compiler", "wasmer-compiler-cranelift", "wasmer-compiler-singlepass",
                       "wasmer-compiler-llvm", "wasmer-emscripten", "wasmer-engine", "wasmer-engine-universal",
                       "wasmer-engine-dylib", "wasmer-engine-staticlib", "wasmer-vm", "wasmer-wasi",
                       "wasmer-wasi-experimental-io-devices", "wasmer-wast", "wasmer-cache", "wasmer-types",
                       "wasmer-vfs"]),
}

# where each crate is located in the `lib` directory
# TODO: this could also be generated from the toml files
location = {
    "wasmer-types": "types",
    "wasmer-derive": "derive",
    "wasmer-vm": "vm",
    "wasmer-compiler": "compiler",
    "wasmer-object": "object",
    "wasmer-engine": "engine",
    "wasmer-compiler-singlepass": "compiler-singlepass",
    "wasmer-compiler-cranelift": "compiler-cranelift",
    "wasmer-compiler-llvm": "compiler-llvm",
    "wasmer-engine": "engine",
    "wasmer-engine-universal": "engine-universal",
    "wasmer-engine-dylib": "engine-dylib",
    "wasmer-engine-staticlib": "engine-staticlib",
    "wasmer-cache": "cache",
    "wasmer": "api",
    "wasmer-wasi": "wasi",
    "wasmer-wasi-types": "wasi-types",
    "wasmer-emscripten": "emscripten",
    "wasmer-wasi-experimental-io-devices": "wasi-experimental-io-devices",
    "wasmer-c-api": "c-api",
    "wasmer-middlewares": "middlewares",
    "wasmer-vfs": "vfs",
    "wasmer-cli": "cli",
    "wasmer-wast": "../tests/lib/wast",
}

no_dry_run = False

def get_latest_version_for_crate(crate_name: str) -> Optional[str]:
    output = subprocess.run(["cargo", "search", crate_name], capture_output=True)
    rexp_src = '^{} = "([^"]+)"'.format(crate_name)
    prog = re.compile(rexp_src)
    haystack = output.stdout.decode("utf-8")
    for line in haystack.splitlines():
        result = prog.match(line)
        if result:
            return result.group(1)

def is_crate_already_published(crate_name: str) -> bool:
    found_string = get_latest_version_for_crate(crate_name)
    if found_string is None:
        return False

    return target_version == found_string

def publish_crate(crate: str):
    starting_dir = os.getcwd()
    os.chdir("lib/{}".format(location[crate]))

    global no_dry_run
    if no_dry_run:
        output = subprocess.run(["cargo", "publish"])
    else:
        print("In dry-run: not publishing crate `{}`".format(crate))

    os.chdir(starting_dir)

def main():
    os.environ['WASMER_PUBLISH_SCRIPT_IS_RUNNING'] = '1'
    parser = argparse.ArgumentParser(description='Publish the Wasmer crates to crates.io')
    parser.add_argument('--no-dry-run', default=False, action='store_true',
                        help='Run the script without actually publishing anything to crates.io')
    args = vars(parser.parse_args())

    global no_dry_run
    no_dry_run = args['no_dry_run']

    # get the order to publish the crates in
    order = list(toposort_flatten(dep_graph, sort=True))

    for crate in order:
        print("Publishing `{}`...".format(crate))
        if not is_crate_already_published(crate):
            publish_crate(crate)
        else:
            print("`{}` was already published!".format(crate))
            continue
        # sleep for 16 seconds between crates to ensure the crates.io index has time to update
        # this can be optimized with knowledge of our dep graph via toposort; we can even publish
        # crates in parallel; however this is out of scope for the first version of this script
        if no_dry_run:
            print("Sleeping for 16 seconds to allow the `crates.io` index to update...")
            time.sleep(16)
        else:
            print("In dry-run: not sleeping for crates.io to update.")


if __name__ == "__main__":
    main()
