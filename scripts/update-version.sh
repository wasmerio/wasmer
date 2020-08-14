# A script to update the version of all the crates at the same time
PREVIOUS_VERSION='1.0.0-alpha.1'
NEXT_VERSION='1.0.0-alpha.2'

# quick hack
fd Cargo.toml --exec sed -i '' "s/version = \"$PREVIOUS_VERSION\"/version = \"$NEXT_VERSION\"/"
echo "manually check changes to Cargo.toml"

fd wasmer.iss --exec sed -i '' "s/AppVersion=$PREVIOUS_VERSION/AppVersion=$NEXT_VERSION/"
echo "manually check changes to wasmer.iss"

# Order to upload packages in
## wasmer-types
## runtime-core
## win-exception-handler
## compiler
## compiler-cranelift
## compiler-llvm
## compiler-singlepass
## emscripten
## wasi
## wasmer (api)
