import init, { start } from '../pkg/index.js';
import 'regenerator-runtime/runtime.js'
import './workers-polyfill.js'

async function run() {
  Error.stackTraceLimit = 20;
  await init();
  await start();
}

run();
