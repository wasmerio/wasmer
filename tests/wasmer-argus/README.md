# wasmer-argus

Automatically test packages from the registry. 

## Building
If you want to use the local `wasmer` crate, you shall 
build the project with `cargo build --package wasmer-argus --features wasmer_lib`.

On macOS, you may encounter an error where the linker does not find `zstd`: a possible 
solution to this problem is to install `zstd` using `brew` (`brew install zstd`) and 
using the following command: 

`RUSTFLAGS="-L$(brew --prefix)/lib" cargo build --package wasmer-argus --features wasmer_lib`

Another possiblity is to add the your brew prefix with `/lib` (probably = `/opt/homebrew/lib/`) 
to the global Cargo config something like:
```
[target.aarch64-apple-darwin]
rustflags = ["-L/opt/homebrew/lib"]
```

## Usage
This binary fetches packages from the graphql endpoint  of a registry. By
default, it uses `http://registry.wasmer.io/graphql`; and the needed
authorization token is retrieved from the environment using the `WASMER_TOKEN`. 
Users can specify the token via CLI with the appropriate flag.

This testsuite is parallelised, and the degree of parallelism available can be
specified both by CLI flag or automatically using
`std::thread::available_parallelism`.

```
Fetch and test packages from a WebContainer registry

Usage: wasmer-argus [OPTIONS]

Options:
  -r, --registry-url <REGISTRY_URL>  
        The GraphQL endpoint of the registry to test [default: http://registry.wasmer.io/graphql]
  -b, --backend <COMPILER_BACKEND>   
        The backend to test the compilation against [default: singlepass] [possible values: llvm, singlepass, cranelift]
      --run                          
        Whether or not to run packages during tests
  -o, --outdir <OUTDIR>              
        The output directory [default: /home/ecmm/sw/wasmer/wasmer/target/debug/out]
      --auth-token <AUTH_TOKEN>      
        The authorization token needed to see packages [default: <env::WASMER_TOKEN>]
      --jobs <JOBS>                  
        The number of concurrent tests (jobs) to perform [default: 12]
  -h, --help                         Print help
  -V, --version                      Print version
```



