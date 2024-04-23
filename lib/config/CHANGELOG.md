# Changelog

## [0.10.0](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.9.3...wasmer-toml-v0.10.0) (2024-04-09)


### Features

* Add support for unnamed packages ([#40](https://github.com/wasmerio/wasmer-toml/issues/40)) ([#41](https://github.com/wasmerio/wasmer-toml/issues/41)) ([7d1fd97](https://github.com/wasmerio/wasmer-toml/commit/7d1fd978852736afab5de36ff9b3066d7a2a6108))

## [0.9.3](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.9.2...wasmer-toml-v0.9.3) (2024-04-09)


### Miscellaneous Chores

* release 0.9.3 ([#36](https://github.com/wasmerio/wasmer-toml/issues/36)) ([d2e0003](https://github.com/wasmerio/wasmer-toml/commit/d2e0003a7b014ac01e4db94d66b757c6e3a5b409))

## [0.9.2](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.9.1...wasmer-toml-v0.9.2) (2023-10-10)


### Features

* Deprecated CommandV1 and several unused Package fields ([331831e](https://github.com/wasmerio/wasmer-toml/commit/331831e1064f5f49d3fc134ba76297cb777fcdcb))


### Bug Fixes

* Serializing a `wasmer_toml::Package` won't include the `private` flag unless it is `true` ([1791623](https://github.com/wasmerio/wasmer-toml/commit/1791623d0c8ff4d03429b78053d93561ff62da70))

## [0.9.1](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.9.0...wasmer-toml-v0.9.1) (2023-10-09)


### Features

* Packages can be marked as private by setting `private = true` under `[package]` ([6eb00dc](https://github.com/wasmerio/wasmer-toml/commit/6eb00dc55d72ec04ab04dda96d169a01cf56bef0))

## [0.9.0](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.8.1...wasmer-toml-v0.9.0) (2023-09-29)


### ⚠ BREAKING CHANGES

* Upgraded public dependencies

### Miscellaneous Chores

* Upgraded public dependencies ([2749624](https://github.com/wasmerio/wasmer-toml/commit/2749624bb63bb8fe614eb26d0d871828cce49b14))

## [0.8.1](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.8.0...wasmer-toml-v0.8.1) (2023-09-29)


### Bug Fixes

* Public dependencies that aren't 1.0 yet are now re-exported using `pub extern crate` ([f320204](https://github.com/wasmerio/wasmer-toml/commit/f320204adc8cff1fa635b59e651adcdffff11702))

## [0.8.0](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.7.0...wasmer-toml-v0.8.0) (2023-09-19)


### ⚠ BREAKING CHANGES

* Removed some unnecessary command getters and switched others from returning owned copies to returning references

### Features

* Commands can now use modules from other dependencies with the `module = "my/dependency:module"` syntax ([88b784d](https://github.com/wasmerio/wasmer-toml/commit/88b784dc6ed5ddae6c2edc69c82c416be62cef35))
* Removed some unnecessary command getters and switched others from returning owned copies to returning references ([88b784d](https://github.com/wasmerio/wasmer-toml/commit/88b784dc6ed5ddae6c2edc69c82c416be62cef35))


### Miscellaneous Chores

* Release 0.8.0 ([c885839](https://github.com/wasmerio/wasmer-toml/commit/c8858399767cec116f8560a5e913bdfdf3e00771))

## [0.7.0](https://github.com/wasmerio/wasmer-toml/compare/wasmer-toml-v0.6.0...wasmer-toml-v0.7.0) (2023-07-20)


### ⚠ BREAKING CHANGES

* Manifest and Package are now #[non_exhaustive] and configurable via a builder API
* made ManifestError and ValidationError more strongly typed and descriptive
* Removed unnecessary Option wrappers from the Manifest type

### Features

* Added an "entrypoint" field to the "[package]" table (fixes [#15](https://github.com/wasmerio/wasmer-toml/issues/15)) ([d6bce6b](https://github.com/wasmerio/wasmer-toml/commit/d6bce6b620000dd156e3cc5a6aefa9c316c7c8ac))
* Added validation for duplicate commands and modules ([26f8f84](https://github.com/wasmerio/wasmer-toml/commit/26f8f84e168c01e30d5838b10b2eea10b457f57c))
* Added validation to check that the entrypoint is valid ([b9b677c](https://github.com/wasmerio/wasmer-toml/commit/b9b677cc461896cdc26246d32add2043b26ffd1e))
* made ManifestError and ValidationError more strongly typed and descriptive ([75040b8](https://github.com/wasmerio/wasmer-toml/commit/75040b8bb73a267024ae2f11aeda88387a56795e))
* Manifest and Package are now #[non_exhaustive] and configurable via a builder API ([2b99e5c](https://github.com/wasmerio/wasmer-toml/commit/2b99e5cc8a1f9c1e6aa1a9e6d9da05ca6a5cd998))
* Removed unnecessary Option wrappers from the Manifest type ([5307784](https://github.com/wasmerio/wasmer-toml/commit/53077842114d39b0d1ce8277c4158f669e641545))


### Miscellaneous Chores

* release 0.7.0 ([e855934](https://github.com/wasmerio/wasmer-toml/commit/e85593437f3d862b06659b105528199fbfcb1cbf))
