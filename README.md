<div align="center">
  <a href="https://wasmer.io">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="./assets/logo-white.svg">
      <img width="300" src="./assets/logo.svg" alt="Wasmer">
    </picture>
  </a>

  <h3>Lightweight sandboxes for your apps and AI agents</h3>

  <p>
    <a href="#run-your-first-sandbox">Run with the CLI</a> ·
    <a href="#wasmer-sdk">Embed with the SDK</a> ·
    <a href="https://wasmer.sh">Try in your browser</a>
  </p>

  <p>
    <a href="https://github.com/wasmerio/wasmer/releases">
      <img src="https://img.shields.io/github/v/release/wasmerio/wasmer" alt="CLI release">
    </a>
    <a href="./LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="MIT license">
    </a>
    <a href="https://github.com/wasmerio/wasmer-sdk">
      <img src="https://img.shields.io/badge/SDK-sandboxes-6f42c1" alt="Wasmer Sandbox SDK">
    </a>
    <a href="https://discord.gg/rWkMNStrEW">
      <img src="https://img.shields.io/discord/1110300506942881873?label=Discord&logo=discord&logoColor=white" alt="Wasmer on Discord">
    </a>
  </p>
</div>

Wasmer runs your apps in *fast*, *secure*, and *lightweight sandboxes*: locally,
in the cloud, or in your browser.

- **Secure** by default. You control file, network, and environment access.
- **Lightweight**. Fast startup with a small memory footprint.
- **Ready to run**. Python, JavaScript, Bash, and more from the [registry](https://wasmer.io/explore).
- **Embeddable**. Add sandboxes to your app with the [Wasmer SDK](#wasmer-sdk).

## Run your first sandbox

Install the CLI on macOS or Linux:

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
<summary>Windows and other installation options</summary>

**Windows (PowerShell)**

```powershell
iwr https://win.wasmer.io -useb | iex
```

**Homebrew (macOS / Linux)**

```sh
brew install wasmer
```

See the [installation guide](https://docs.wasmer.io/install/) for more options.

</details>

Run Python in a sandbox:

```sh
wasmer run python/python -- -c "print('Hello from Wasmer')"
```

```text
Hello from Wasmer
```

Or try a shell command and Cowsay:

```sh
wasmer run wasmer/bash -- -c "echo 'Hello from Wasmer'"
wasmer run syrusakbary/cowsay -- "Hello from Wasmer"
```

Wasmer downloads the package and caches it for future runs. Use `--` to separate
Wasmer options from the arguments passed to your program. To work with local
files, grant access to a directory with `--volume`; enable host networking with
`--net` when your program needs it. See the
[CLI guide](https://docs.wasmer.io/runtime/cli/) for details.

**No installation needed:** open [wasmer.sh](https://wasmer.sh) to try a sandbox
in your browser.

## Wasmer SDK

Create a sandbox, choose your tools, and run code directly in your app with the
[Wasmer SDK](https://github.com/wasmerio/wasmer-sdk).

### Install the SDK

**JavaScript / TypeScript** (Node.js 20 or newer):

```sh
npm install @wasmer/sdk
```

<details>
<summary>Rust</summary>

The Rust SDK currently uses a source dependency. Clone
[`wasmerio/wasmer-sdk`](https://github.com/wasmerio/wasmer-sdk), then point your
application at its `rust` directory:

```toml
[dependencies]
wasmer-sdk = { path = "../wasmer-sdk/rust" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Follow the [Rust setup guide](https://github.com/wasmerio/wasmer-sdk/tree/main/rust)
for the required toolchain and workspace patches. Crates.io publishing is
currently disabled.

</details>

<details>
<summary>Python</summary>

```sh
python -m pip install wasmer-sdk
```

The SDK's Python wheels support macOS and Linux on Intel and ARM64.
See the [Python guide](https://github.com/wasmerio/wasmer-sdk/tree/main/python).

</details>

<details>
<summary>Swift (iOS and macOS)</summary>

Use Swift 6 or newer. The SDK supports macOS 12+ and iOS 27+; iOS apps require
Xcode 27 or newer.

Add the package to your `Package.swift` dependencies:

```swift
.package(
    url: "https://github.com/wasmerio/wasmer-sdk.git",
    revision: "wasmer-sdk-swift-v0.4.0"
)
```

Then add the product to your target's dependencies:

```swift
.product(name: "WasmerSDK", package: "wasmer-sdk")
```

In Xcode, you can instead add the repository URL, select
`wasmer-sdk-swift-v0.4.0` as a revision, and choose the **WasmerSDK** product.
SwiftPM downloads the prebuilt macOS binaries and iOS runtime resources.

For iOS, set the deployment target to **iOS 27.0** and allow local networking
in your app's `Info.plist`:

```xml
<key>NSAppTransportSecurity</key>
<dict>
    <key>NSAllowsLocalNetworking</key>
    <true/>
</dict>
```

Both platforms use `import WasmerSDK` and the same API. On iOS, the SDK manages
a hidden `WKWebView`; your app does not need to display one. See the
[Swift guide](https://github.com/wasmerio/wasmer-sdk/tree/main/swift) and
[iOS setup guide](https://github.com/wasmerio/wasmer-sdk/tree/main/swift/WasmerWKSDK#add-to-an-app)
for more details.

</details>

### Run code in a sandbox

**JavaScript / TypeScript**

Save this as `sandbox.mjs`:

```javascript
import { Wasmer } from "@wasmer/sdk/node";

const wasmer = new Wasmer();
const sandbox = await wasmer.sandboxes.create({
  packages: ["python/python@=3.13.20"],
});

const output = await sandbox
  .command("python", ["-c", "print('Hello from Wasmer')"])
  .run();

console.log(output.text());
```

Run it with `node sandbox.mjs`. Python executes inside the sandbox; you don't
need Python installed on the host. See the
[JavaScript guide](https://github.com/wasmerio/wasmer-sdk/tree/main/js).

<details>
<summary>Python example</summary>

Save this as `sandbox.py`:

```python
import asyncio

from wasmer_sdk import Wasmer


async def main():
    wasmer = Wasmer()
    sandbox = await wasmer.sandboxes.create(
        packages=["python/python@=3.13.20"],
    )
    output = await sandbox.command(
        "python", ["-c", "print('Hello from Wasmer')"]
    ).run()
    print(output.text())


asyncio.run(main())
```

Run it with `python sandbox.py`. See the
[Python guide](https://github.com/wasmerio/wasmer-sdk/tree/main/python).

</details>

<details>
<summary>Swift example (iOS and macOS)</summary>

Use this code in an `async throws` context:

```swift
import WasmerSDK

let wasmer = try Wasmer()
let sandbox = try await wasmer.sandboxes.create(
    packages: ["python/python@=3.13.20"]
)
let output = try await sandbox.command(
    "python", ["-c", "print('Hello from Swift!')"]
).run(timeout: 30)
print(try output.text())
```

See the [Swift guide](https://github.com/wasmerio/wasmer-sdk/tree/main/swift)
for embedding in an iOS or macOS app and more examples.

</details>

<details>
<summary>JavaScript in the browser</summary>

Use the same JavaScript sandbox API with the `@wasmer/sdk/browser` import.
Execution happens in the browser. Your page needs cross-origin isolation
(`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`) for worker-backed execution.
Follow the [browser setup guide](https://github.com/wasmerio/wasmer-sdk/tree/main/js#browser)
or explore [wasmer.sh](https://wasmer.sh).

</details>

<details>
<summary>Rust example</summary>

```rust
use wasmer_sdk::{Result, Wasmer};

#[tokio::main]
async fn main() -> Result<()> {
    let wasmer = Wasmer::new()?;
    let sandbox = wasmer
        .sandboxes()
        .create()
        .package("python/python@=3.13.20")
        .await?;

    let output = sandbox
        .command("python")
        .args(["-c", "print('Hello from Wasmer')"])
        .run()
        .await?;

    println!("{}", output.text()?);
    Ok(())
}
```

See the [Rust guide](https://github.com/wasmerio/wasmer-sdk/tree/main/rust)
for workspace setup and more examples.

</details>

### Go further

Build more with the sandbox API:

- **Commands**. Run code and capture output.
- **Processes**. Stream output and manage long-running tasks.
- **Files**. Read and write the sandbox workspace.
- **Tools**. Combine languages and packages in one sandbox.
- **Services**. Run servers with explicit network access.

The SDK is currently **alpha**. See the language guides for platform support and
capabilities, and the [SDK examples](https://github.com/wasmerio/wasmer-sdk#what-can-you-run)
for Python scripts, an Edge.js HTTP server, PostgreSQL, and multiple tools sharing
one sandbox.

## Software you can run

Start with packages from the registry:

| Software | Package |
| --- | --- |
| Python | [`python/python`](https://wasmer.io/python/python) |
| JavaScript / Node.js-compatible apps | [`wasmer/edgejs`](https://wasmer.io/wasmer/edgejs) |
| Bash | [`wasmer/bash`](https://wasmer.io/wasmer/bash) |
| PHP | [`php/php`](https://wasmer.io/php/php) |
| PostgreSQL | [`wasmer/pglite`](https://wasmer.io/wasmer/pglite) |
| SQLite | [`sqlite/sqlite`](https://wasmer.io/sqlite/sqlite) |
| FFmpeg | [`wasmer/ffmpeg`](https://wasmer.io/wasmer/ffmpeg) |

[Explore more packages](https://wasmer.io/explore),
[package your own software](https://docs.wasmer.io/registry/get-started/), or
[deploy an application to Wasmer Edge](https://docs.wasmer.io/edge/get-started/).

## Develop and contribute

This repository contains the Wasmer runtime and CLI that power the sandbox
experience. The sandbox SDKs live in
[`wasmerio/wasmer-sdk`](https://github.com/wasmerio/wasmer-sdk).
For lower-level WebAssembly embedding, see the [Rust API](https://docs.rs/wasmer/)
and [C API](./lib/c-api).

- [Build Wasmer from source](./docs/BUILD.md)
- [Test your changes](./docs/TEST.md)
- [Report a security issue](./docs/SECURITY.md)
- [Open an issue](https://github.com/wasmerio/wasmer/issues)

Contributions are welcome. For guidance on getting started, read
[Contributing to Complex Projects](https://mitchellh.com/writing/contributing-to-complex-projects).
Wasmer is [MIT licensed](./LICENSE).

## Community

Get help, share what you're building, and meet other Wasmer users:

[Discord](https://discord.gg/rWkMNStrEW) ·
[X](https://x.com/wasmerio) ·
[LinkedIn](https://www.linkedin.com/company/wasmerio) ·
[Blog](https://wasmer.io/posts)

<details>
<summary>Community translations (may describe an earlier version)</summary>

[中文](./docs/cn/README.md) ·
[Deutsch](./docs/de/README.md) ·
[Español](./docs/es/README.md) ·
[Français](./docs/fr/README.md) ·
[日本語](./docs/ja/README.md) ·
[한국어](./docs/ko/README.md)

</details>
