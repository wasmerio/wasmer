This is a simple [Javascript Service Worker](https://python.org/) server template running with [WinterJS](https://github.com/wasmerio/winterjs).

> This starter's full tutorial is available [here](https://docs.wasmer.io/edge/quickstart/js-wintercg).

## Usage

Modify the logic of your the Javascript worker in the `src/index.js` file.

You can run the JS Service Worker locally with (check out the [Wasmer install guide](https://docs.wasmer.io/install)):

```bash
wasmer run . --net
```

Open [http://localhost:8080](http://localhost:8080) with your browser to see the worker working!


## Deploy on Wasmer Edge

The easiest way to deploy your Javascript Worker is to use the [Wasmer Edge](https://wasmer.io/products/edge).

Live example: https://wasmer-js-worker-starter.wasmer.app/

```bash
wasmer deploy
```
