import init, { start } from '../pkg/index.js';
import { init_cfg } from '../js/init.js';
import 'regenerator-runtime/runtime.js'
import './workers-polyfill.js'

if (location.protocol !== 'https:') {
  if (location.hostname !== 'localhost') {
    location.replace(`https:${location.href.substring(location.protocol.length)}`);
  }
}

export function init_encoded() {
  return 'init=' + encodeURIComponent(init_cfg.init) + '&' +
    'uses=' + encodeURIComponent(init_cfg.uses) + '&' +
    'prompt=' + encodeURIComponent(init_cfg.prompt) + '&' +
    'no_welcome=' + encodeURIComponent(init_cfg.no_welcome);
}

async function run() {
  Error.stackTraceLimit = 20;
  await init();
  await start(init_encoded());
}

run();
