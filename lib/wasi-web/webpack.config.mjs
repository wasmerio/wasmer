import { dirname, resolve } from 'path';
import { fileURLToPath } from 'url';
import WasmPackPlugin from '@wasm-tool/wasm-pack-plugin';
import CopyPlugin from 'copy-webpack-plugin';

const __dirname = dirname(fileURLToPath(import.meta.url));

export default {
  mode: 'production',
  entry: resolve(__dirname, './js/index.js'),
  plugins: [
    new CopyPlugin({
      patterns: [
        { from: resolve(__dirname, "node_modules/xterm/css/xterm.css") },
        { from: resolve(__dirname, "node_modules/xterm/lib/xterm.js.map") },
        { from: resolve(__dirname, "public/index.html") },
        { from: resolve(__dirname, "public/wasmer.css") },
        { from: resolve(__dirname, "public/worker.js") },
      ],
    }),
    new WasmPackPlugin({
        crateDirectory: resolve(__dirname, './'),

        // Check https://rustwasm.github.io/wasm-pack/book/commands/build.html for
        // the available set of arguments.
        //
        // Optional space delimited arguments to appear before the wasm-pack
        // command. Default arguments are `--verbose`.
        args: '--log-level warn',
        // Default arguments are `--typescript --target browser --mode normal`.
        extraArgs: '--target web',

        // Optional array of absolute paths to directories, changes to which
        // will trigger the build.
        // watchDirectories: [
        //   path.resolve(__dirname, "another-crate/src")
        // ],

        // The same as the `--out-dir` option for `wasm-pack`
        // outDir: "pkg",

        // The same as the `--out-name` option for `wasm-pack`
        // outName: "index",

        // If defined, `forceWatch` will force activate/deactivate watch mode for
        // `.rs` files.
        //
        // The default (not set) aligns watch mode for `.rs` files to Webpack's
        // watch mode.
        // forceWatch: true,

        // If defined, `forceMode` will force the compilation mode for `wasm-pack`
        //
        // Possible values are `development` and `production`.
        //
        // the mode `development` makes `wasm-pack` build in `debug` mode.
        // the mode `production` makes `wasm-pack` build in `release` mode.
        // forceMode: "development",

        // Controls plugin output verbosity, either 'info' or 'error'.
        // Defaults to 'info'.
        // pluginLogLevel: 'info'
    }),
  ],
  devServer: {
    compress: true,
    client: {
      overlay: {
        errors: true,
        warnings: false,
      },
    },
    headers: {
      // This headers are needed so the SharedArrayBuffer can be properly
      // inited in the browsers
      'cross-origin-embedder-policy': 'require-corp',
      'cross-origin-opener-policy': 'same-origin'
    },
    // hot: false,
    port: 9000,
  },
  output: {
    filename: 'main.js',
    path: resolve(__dirname, 'dist'),
  },
  optimization: {
    minimize: false
  },
};
