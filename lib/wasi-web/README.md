# https://wasmer.sh WebSite

Contains all the files needed to serve the wasmer.sh website natively from WebAssmebly - this includes a HTTP server

# deploy

cargo install wasmer-cli --git https://github.com/wasmerio/wasmer --branch upgrade-edge-cli
wasmer app create static-site
or wasmer app create
now produce an app.yaml
wasmer deploy deploys it