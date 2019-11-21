#!/bin/sh

initArch() {
    ARCH=$(uname -m)
    if [ -n "$WASMER_ARCH" ]; then
        ARCH="$WASMER_ARCH"
    fi
    # If you modify this list, please also modify install.sh
    case $ARCH in
        amd64) ARCH="amd64";;
        x86_64) ARCH="amd64";;
        aarch64) ARCH="arm64";;
        i386) ARCH="386";;
        *) echo "Architecture ${ARCH} is not supported by this installation script"; exit 1;;
    esac
}

initOS() {
    OS=$(uname | tr '[:upper:]' '[:lower:]')
    if [ -n "$WASMER_OS" ]; then
        echo "Using WASMER_OS"
        OS="$WASMER_OS"
    fi
    case "$OS" in
        darwin) OS='darwin';;
        linux) OS='linux';;
        freebsd) OS='freebsd';;
        # mingw*) OS='windows';;
        # msys*) OS='windows';;
        *) echo "OS ${OS} is not supported by this installation script"; exit 1;;
    esac
}

# identify platform based on uname output
initArch
initOS

# determine install directory if required
BINARY="wasmer-${OS}-${ARCH}.tar.gz"

# add .exe if on windows
# if [ "$OS" = "windows" ]; then
#     BINARY="$BINARY.exe"
# fi

echo "${BINARY}"
