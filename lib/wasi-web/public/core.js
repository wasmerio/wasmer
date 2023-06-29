import { schedule_wasm_task, register_web_worker, return_web_worker, get_web_worker, claim_web_worker } from '../../..';

// WebWorkers are returned to a pool so that they can be reused, this
// is important as the cost of starting another web worker is a very
// costly process as it needs to do a fetch back to the main servers
// to load the main WASM module

// The purpose of this file is two-fold:
// First: Expose a function to start a web worker. This function must
// not be inlined into the Rust lib, as otherwise bundlers could not
// bundle it -- huh.
export function startWorker(module, memory, state, opts) {
    try {
        const worker = new Worker(new URL('./worker.js',
            import.meta.url), opts);

        // When the worker wants to schedule some work it will
        // post a message back to the main thread, otherwise it
        // might actually post itself back to the main thread
        // when its finished
        worker.onmessage = async ev => {
            // Web worker has scheduled some work which the main
            // thread must orchestrate
            let [task, module, memory] = ev.data;
            await schedule_wasm_task(task, module, memory);
        };
        worker.postMessage([module, memory, state]);
    } catch (err) {
        return new Promise((res, rej) => {
            rej(err);
        });
    }
    return new Promise((res, rej) => {
        res();
    });
}
export function startWasm(module, memory, ctx, opts, wasm_module, wasm_memory, wasm_cache) {
    return new Promise(async (res, rej) => {
        try {
            // Attempt to get a worker from the pool before we
            // create a new web worker as its quite expensive
            var worker = null;
            var worker_id = claim_web_worker();

            // If the worker is not available then create a new one
            if (worker_id == null) {
                worker = new Worker(new URL('./worker.js',
                    import.meta.url), opts);

                // Register the web worker in the thread pool
                worker_id = register_web_worker(worker);
            } else {
                // If we have claimed a web worker then we need to
                // get a reference to it
                worker = get_web_worker(worker_id);
            }

            // When the worker wants to schedule some work it will
            // post a message back to the main thread, otherwise it
            // might actually post itself back to the main thread
            // when its finished
            worker.onmessage = async ev => {
                if (ev.data.length == 3) {
                    // Web worker has scheduled some work which the main
                    // thread must orchestrate
                    let [task, module, memory] = ev.data;
                    await schedule_wasm_task(task, module, memory);
                } else {
                    // Web worker has finished and is now returned to the
                    // main pool so it can be reused
                    let [id] = ev.data;
                    return_web_worker(id);
                }
            };
            worker.postMessage([worker_id, module, memory, ctx, wasm_module, wasm_memory, wasm_cache]);
        } catch (err) {
            return new Promise((res, rej) => {
                rej(err);
            });
        }
        res();
    });
}
