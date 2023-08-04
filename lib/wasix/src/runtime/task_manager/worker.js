console.log("Entered worker");
Error.stackTraceLimit = 50;

console.log(globalThis);

globalThis.onerror = console.error;

globalThis.onmessage = async ev => {
    console.log("Message", ev.data);

    if (ev.data.length == 3) {
        let [module, memory, state] = ev.data;
        const { default: init, worker_entry_point } = await import("$IMPORT_META_URL");
        await init(module, memory);
        worker_entry_point(state);
    } else {
        var is_returned = false;
        try {
            globalThis.onmessage = ev => { console.error("wasm threads can only run a single process then exit", ev) }
            let [id, module, memory, ctx, wasm_module, wasm_memory, wasm_cache] = ev.data;
            const { default: init, wasm_entry_point } = await import("$IMPORT_META_URL");
            await init(module, memory);
            wasm_entry_point(ctx, wasm_module, wasm_memory, wasm_cache);

            // Return the web worker to the thread pool
            postMessage([id]);
            is_returned = true;
        } finally {
            //Terminate the worker
            if (is_returned == false) {
                close();
            }
        }
    }
};
