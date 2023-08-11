import init, { start } from '../pkg/index.js';
import 'regenerator-runtime/runtime.js'
import './workers-polyfill.js'

if (location.protocol !== 'https:') {
  if (location.hostname !== 'localhost') {
    location.replace(`https:${location.href.substring(location.protocol.length)}`);
  }
}

async function init_encoded() {
  let init_cfg = await (await fetch("/init.json")).json();
  return 'init=' + encodeURIComponent(init_cfg.init) + '&' +
    'uses=' + encodeURIComponent(init_cfg.uses) + '&' +
    'prompt=' + encodeURIComponent(init_cfg.prompt) + '&' +
    'no_welcome=' + encodeURIComponent(init_cfg.no_welcome);
}

async function run() {
  Error.stackTraceLimit = 20;
  await init();
  await start(await init_encoded());
}

run();
