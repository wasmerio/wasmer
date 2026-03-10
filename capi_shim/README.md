# Shim for wasmer 1 (legacy)

## Build

First, go to the `capi_shim` directory (this directory):

```sh
cd ./capi_shim
```

On **Linux AMD64**: not applicable (not needed).

On **MacOS AMD64**: not applicable (not needed).

On **MacOS ARM64**:

```
go build -buildmode=c-shared -ldflags="-w" -o libwasmer_darwin_arm64_shim.dylib .

install_name_tool -id @rpath/libwasmer_darwin_arm64_shim.dylib libwasmer_darwin_arm64_shim.dylib
```

On **Linux ARM64**:

```
go build -buildmode=c-shared -o libwasmer_linux_arm64_shim.so .

patchelf --set-soname libwasmer_linux_arm64_shim.so libwasmer_linux_arm64_shim.so
```
