#!/usr/bin/env python
import os
import subprocess
import sys

def print_usage():
    print('USAGE:')
    print('wdt ccr <file>       Compiles <file> with EMCC and runs with cargo run')

def ccr(args):
    if len(args) == 0:
        print('file argument required')
    else:
        file = args[0]
        file_base_name = os.path.splitext(os.path.basename(file))[0]
        realpath = os.path.realpath(file)
        dir_name = os.path.dirname(realpath)
        outfile_js = os.path.join(dir_name, file_base_name + ".js")
        call_external_command(['emcc', realpath,'-s','WASM=1','-o', outfile_js])
        outfile_wasm = os.path.join(dir_name, file_base_name + ".wasm")
        call_external_command(['cargo', 'run', 'run', outfile_wasm])

def call_external_command(command):
    print 'RUNNING COMMAND: ' + (' '.join(command))
    subprocess.call(command)

def run_command(args):
    command, rest = args[0], args[1:]
    if command == 'ccr':
        ccr(rest)
    elif command == 'help':
        print_usage()
    else:
        print('Unknown command: ' + command + ' See `wdt help` for usage')

if __name__ == '__main__':
    if len(sys.argv) == 1:
        print_usage()
    else:
        run_command(sys.argv[1:])

