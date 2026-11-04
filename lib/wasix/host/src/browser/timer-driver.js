// Lives on a dedicated event-loop worker. Rust owns the hard-ordered deadline queue.
// A generation also rejects a stale callback already queued when clearTimeout ran.
export function timerDriver(bindings, schedule = setTimeout, cancel = clearTimeout) {
    if (typeof bindings.browser_timers_tick !== "function" || typeof bindings.browser_timers_pending !== "function") {
        throw new Error("artifact lacks browser timer exports");
    }
    let alarm;
    let generation = 0;
    function clear() {
        ++generation;
        if (alarm !== undefined) cancel(alarm);
        alarm = undefined;
    }
    function rearm() {
        clear();
        const delay = bindings.browser_timers_tick();
        if (delay < 0) return;
        const current = generation;
        alarm = schedule(() => {
            if (current !== generation) return;
            alarm = undefined;
            try { rearm(); }
            catch (error) { postMessage({ type: "fatal", error: error.stack ?? String(error) }); }
        }, delay);
    }
    return { rearm, close() {
        clear();
        if (bindings.browser_timers_pending() !== 0) throw new Error("browser timer shutdown with live waits");
    } };
}
