// Command doubles for cold producer tests: no downloads or compiler builds.
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export function mockProducer(directory) {
    const bin = join(directory, "commands");
    mkdirSync(bin);
    const command = join(bin, "command");
    writeFileSync(command, `#!/usr/bin/env python3
import json, os, pathlib, sys
name, *args = sys.argv[1:]
root = pathlib.Path(os.environ['WASIXV2_BROWSER_TOOLS'])
with open(os.environ['PRODUCER_COMMANDS'], 'a') as log:
    log.write(json.dumps([name, *args]) + '\\n')
def file(path, data='', executable=False):
    path = pathlib.Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data)
    if executable:
        path.chmod(0o755)
if name == 'git':
    while args and args[0] == '-c':
        args = args[2:]
    if args[0] == 'ls-remote':
        if os.environ.get('FAIL_ACCESS'):
            sys.exit(128)
    elif args[0] == 'init':
        (pathlib.Path(args[1])/'.git').mkdir(parents=True)
    else:
        assert args[0] == '-C', args
        directory = pathlib.Path(args[1])
        args = args[2:]
        if directory.name == 'codegen':
            print(os.environ['MOCK_ABI'])
        elif args[0] == 'fetch':
            if os.environ.get('FAIL_FETCH') == directory.name:
                sys.exit(128)
            file(directory/'.git/revision', args[-1])
        elif args[0] == 'rev-parse':
            revision = directory/'.git/revision'
            if not revision.exists():
                sys.exit(128)
            print(revision.read_text())
        elif args[0] == 'submodule':
            file(directory/'wasixv2/abi/.git/revision', os.environ['MOCK_ABI'])
            file(directory/'wasixv2/build-sysroot.sh', '''#!/bin/sh
set -eu
out= config=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) out=$2; shift 2 ;;
        --config) config=$2; shift 2 ;;
        *) shift ;;
    esac
done
[ "$config" = eh ] && suffix= || suffix=-asyncify
lib="$out/sysroot-wasixv2$suffix/lib/wasm32-wasixv2"
mkdir -p "$lib" "$out/sysroot-wasixv2$suffix/include"
for f in libc.a crt1.o libclang_rt.builtins.a; do touch "$lib/$f"; done
touch "$out/sysroot-wasixv2$suffix/include/stdio.h"
''')
        elif args[0] == 'ls-remote' and os.environ.get('FAIL_ACCESS'):
            sys.exit(128)
elif name == 'cmake':
    if '--build' in args:
        build = pathlib.Path(args[args.index('--build')+1])
        if build.name == 'llvm':
            for tool in ['clang', 'lld', 'llvm-ar', 'llvm-ranlib', 'llvm-nm']:
                file(build/'bin'/tool, '#!/bin/sh\\nexit 0\\n', True)
            file(build/'lib/clang/22/include/stddef.h')
        else:
            file(build/'bin/wasm-opt', '#!/bin/sh\\nexit 0\\n', True)
elif name == 'cargo':
    file(pathlib.Path(os.environ['CARGO_TARGET_DIR'])/'debug/wasixccenv', '#!/bin/sh\\nexit 0\\n', True)
`);
    for (const name of ["git", "cmake", "cargo", "rustup"]) {
        const path = join(bin, name);
        writeFileSync(path, `#!/bin/sh\nexec python3 '${command}' ${name} "$@"\n`);
        chmodSync(path, 0o755);
    }
    for (const [name, count] of [["nice", 2], ["taskset", 2]]) {
        const path = join(bin, name);
        writeFileSync(path, `#!/bin/sh\nshift ${count}\nexec "$@"\n`);
        chmodSync(path, 0o755);
    }
    return { ...process.env, PATH: `${bin}:${process.env.PATH}`, PRODUCER_COMMANDS: join(directory, "commands.jsonl") };
}
