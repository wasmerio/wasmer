# wasmer-argus

Automatically test packages from the registry. 

## Building
Simply build with `cargo build --package wasmer-argus`. The `wasmer-argus`
binary will be in the `target/debug` directory.

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



