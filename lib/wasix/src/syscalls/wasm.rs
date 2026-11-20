use chrono::Local;
use wasm_bindgen::prelude::wasm_bindgen;
use wasmer::WasmRef;

use crate::syscalls::types::wasi::{Errno, Snapshot0Clockid, Timestamp};

pub fn platform_clock_res_get(
    clock_id: Snapshot0Clockid,
    _resolution: WasmRef<Timestamp>,
) -> Result<i64, Errno> {
    let t_out = match clock_id {
        Snapshot0Clockid::Monotonic => 10_000_000,
        Snapshot0Clockid::Realtime => 1,
        Snapshot0Clockid::ProcessCputimeId | Snapshot0Clockid::ThreadCputimeId => {
            return Err(Errno::Notsup);
        }
        _ => return Err(Errno::Inval),
    };
    Ok(t_out)
}

// Capture the clock when the JS module loads so later changes to guest
// globals cannot redirect it. Adding timeOrigin puts workers in the same
// estimated monotonic time domain even though their now() origins differ.
#[wasm_bindgen(inline_js = r#"
const wasmerClockPerformance = globalThis.performance;
const wasmerClockTimeOrigin = wasmerClockPerformance.timeOrigin;
const wasmerClockNow = wasmerClockPerformance.now.bind(wasmerClockPerformance);
export function wasmer_browser_monotonic_milliseconds() {
    return wasmerClockTimeOrigin + wasmerClockNow();
}
"#)]
extern "C" {
    fn wasmer_browser_monotonic_milliseconds() -> f64;
}

fn monotonic_milliseconds_to_nanoseconds(milliseconds: f64) -> Result<i64, Errno> {
    let nanoseconds = milliseconds * 1_000_000.0;
    // i64::MAX rounds up to 2^63 as f64. The exclusive bound also rejects
    // nonfinite values before the cast can saturate.
    if !(0.0..i64::MAX as f64).contains(&nanoseconds) {
        return Err(Errno::Overflow);
    }
    Ok(nanoseconds as i64)
}

pub fn platform_clock_time_get(
    clock_id: Snapshot0Clockid,
    _precision: Timestamp,
) -> Result<i64, Errno> {
    match clock_id {
        Snapshot0Clockid::Monotonic => {
            monotonic_milliseconds_to_nanoseconds(wasmer_browser_monotonic_milliseconds())
        }
        Snapshot0Clockid::Realtime => Local::now().timestamp_nanos_opt().ok_or(Errno::Overflow),
        Snapshot0Clockid::ProcessCputimeId | Snapshot0Clockid::ThreadCputimeId => {
            Err(Errno::Notsup)
        }
        _ => Err(Errno::Inval),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen::{JsCast, closure::Closure};
    use wasm_bindgen_test::wasm_bindgen_test;
    use wasmer::{Memory, MemoryType, Pages, Store};

    #[wasm_bindgen(inline_js = r#"
export function with_replaced_clocks(run) {
    const original = globalThis.performance;
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, "performance");
    const nowDescriptor = Object.getOwnPropertyDescriptor(original, "now");
    const OriginalDate = globalThis.Date;
    try {
        Object.defineProperty(original, "now", { configurable: true, value: () => NaN });
        Object.defineProperty(globalThis, "performance", {
            configurable: true,
            value: { timeOrigin: 0, now: () => NaN },
        });
        globalThis.Date = class extends OriginalDate {
            constructor(...args) {
                super(...(args.length ? args : [1000]));
            }
            static now() {
                return 1000;
            }
        };
        run();
    } finally {
        globalThis.Date = OriginalDate;
        if (descriptor) Object.defineProperty(globalThis, "performance", descriptor);
        else delete globalThis.performance;
        if (nowDescriptor) Object.defineProperty(original, "now", nowDescriptor);
        else delete original.now;
    }
}
"#)]
    extern "C" {
        fn with_replaced_clocks(run: &js_sys::Function);
    }

    #[wasm_bindgen_test]
    fn monotonic_and_realtime_use_separate_clocks() {
        let run = Closure::<dyn Fn()>::new(|| {
            let realtime = platform_clock_time_get(Snapshot0Clockid::Realtime, 0).unwrap();
            assert_eq!(realtime, 1_000_000_000);
            let first = platform_clock_time_get(Snapshot0Clockid::Monotonic, 0).unwrap();
            assert!(first > realtime);
            assert!(platform_clock_time_get(Snapshot0Clockid::Monotonic, 0).unwrap() >= first);
        });
        with_replaced_clocks(run.as_ref().unchecked_ref());
    }

    #[wasm_bindgen_test]
    fn clock_conversion_rejects_invalid_and_out_of_range_values() {
        assert_eq!(monotonic_milliseconds_to_nanoseconds(0.0), Ok(0));
        assert_eq!(monotonic_milliseconds_to_nanoseconds(1.25), Ok(1_250_000));
        for value in [f64::NAN, f64::INFINITY, -1.0, i64::MAX as f64 / 1_000_000.0] {
            assert_eq!(
                monotonic_milliseconds_to_nanoseconds(value),
                Err(Errno::Overflow)
            );
        }
    }

    #[wasm_bindgen_test]
    fn clock_resolution_and_errors_are_reported() {
        let mut store = Store::default();
        let memory = Memory::new(&mut store, MemoryType::new(Pages(1), None, false)).unwrap();
        let view = memory.view(&store);
        assert_eq!(
            platform_clock_res_get(Snapshot0Clockid::Monotonic, WasmRef::new(&view, 0)),
            Ok(10_000_000)
        );
        assert_eq!(
            platform_clock_res_get(Snapshot0Clockid::Realtime, WasmRef::new(&view, 0)),
            Ok(1)
        );
        for clock in [
            Snapshot0Clockid::ProcessCputimeId,
            Snapshot0Clockid::ThreadCputimeId,
        ] {
            assert_eq!(platform_clock_time_get(clock, 0), Err(Errno::Notsup));
            assert_eq!(
                platform_clock_res_get(clock, WasmRef::new(&view, 0)),
                Err(Errno::Notsup)
            );
        }
        assert_eq!(
            platform_clock_time_get(Snapshot0Clockid::Unknown, 0),
            Err(Errno::Inval)
        );
        assert_eq!(
            platform_clock_res_get(Snapshot0Clockid::Unknown, WasmRef::new(&view, 0)),
            Err(Errno::Inval)
        );
    }
}
