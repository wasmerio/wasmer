#!/usr/bin/python3

"""
Testing script for running all tests in a unified way
(without using make)

Available configurations:

|------------|-----------|----------|--------------|-------|
| Compiler   ⨯ Engine    ⨯ Platform ⨯ Architecture ⨯ libc  |
|------------|-----------|----------|--------------|-------|
| Cranelift  | Universal | Linux    | amd64        | glibc |
| LLVM       |           | Darwin   | aarch64      | musl  |
| Singlepass |           | Windows  |              |       |
|------------|-----------|----------|--------------|-------|
"""

from typing import List, Final, Optional

class Compiler:
    def __init__(self, _type: str) -> None:
        self._type = _type

class Engine:
    def __init__(self, _type: str) -> None:
        self._type = _type

class Platform:
    def __init__(self, _type: str) -> None:
        self._type = _type

class Architecture:
    def __init__(self, _type: str) -> None:
        self._type = _type

class Libc:
    def __init__(self, _type: str) -> None:
        self._type = _type

COMPILER_CRANELIFT: Final = Compiler("cranelift")
COMPILER_LLVM: Final = Compiler("llvm")
COMPILER_SINGLEPASS: Final = Compiler("singlepass")
ALL_COMPILERS = [COMPILER_CRANELIFT, COMPILER_LLVM, COMPILER_SINGLEPASS]

ENGINE_UNIVERSAL: Final = Engine("universal")
ALL_ENGINES = [ENGINE_UNIVERSAL]

PLATFORM_WINDOWS: Final = Platform("windows")
PLATFORM_LINUX: Final = Platform("linux")
PLATFORM_DARWIN: Final = Platform("darwin")
ALL_PLATFORMS = [PLATFORM_WINDOWS, PLATFORM_LINUX, PLATFORM_DARWIN]

ARCH_AMD64: Final = Architecture("amd64")
ARCH_AARCH64: Final = Architecture("aarch64")
ALL_ARCHITECTURES = [ARCH_AMD64, ARCH_AARCH64]

LIBC_GILIBC: Final = Libc("glibc")
LIBC_MUSL: Final = Libc("musl")
ALL_LIBCS = [LIBC_GILIBC, LIBC_MUSL]

class Configuration:

    compiler: Compiler
    engine: Engine
    platform: Platform
    architecture: Architecture
    libc: Libc

    def __init__(self, compiler: Compiler, engine: Engine, platform: Platform, architecture: Architecture, libc: Libc) -> None:
        self.compiler = compiler
        self.engine = engine
        self.platform = platform
        self.architecture = architecture
        self.libc = libc

def generate_configuration_matrix() -> List[Configuration]:
    work = []
    for c in ALL_COMPILERS:
        for e in ALL_ENGINES:
            for p in ALL_PLATFORMS:
                for a in ALL_ARCHITECTURES:
                    for l in ALL_LIBCS:
                        work.append(Configuration(c, e, p, a, l))
    return work

def get_current_architecture() -> Optional[Architecture]:
    pass

def get_current_platform() -> Optional[Platform]:
    pass

def main():
    for m in generate_configuration_matrix():
        print(m.compiler._type)

if __name__ == "__main__":
    main()