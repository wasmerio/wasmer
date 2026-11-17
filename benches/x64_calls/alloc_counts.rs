//! Counts the calling thread's Rust global allocator operations in a separate executable.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    hint::black_box,
};

#[derive(Clone, Copy, Default)]
struct Counts {
    alloc: u64,
    zeroed: u64,
    realloc: u64,
    free: u64,
    bytes_requested: u64,
}

thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}

fn record(f: impl FnOnce(&mut Counts)) {
    let _ = COUNTS.try_with(|state| {
        if let Some(mut counts) = state.get() {
            f(&mut counts);
            state.set(Some(counts));
        }
    });
}

struct CountingSystem;

#[global_allocator]
static ALLOCATOR: CountingSystem = CountingSystem;

unsafe impl GlobalAlloc for CountingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(|c| {
            c.alloc += 1;
            c.bytes_requested += layout.size() as u64;
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(|c| {
            c.zeroed += 1;
            c.bytes_requested += layout.size() as u64;
        });
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record(|c| c.free += 1);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record(|c| {
            c.realloc += 1;
            c.bytes_requested += new_size as u64;
        });
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

pub struct Criterion(pub usize);

pub struct Bencher<'a> {
    name: &'a str,
    count: usize,
}

impl Criterion {
    pub fn bench_function(&mut self, name: &str, f: impl FnOnce(&mut Bencher<'_>)) {
        f(&mut Bencher {
            name,
            count: self.0,
        });
    }
}

impl Bencher<'_> {
    pub fn iter<T>(&mut self, mut f: impl FnMut() -> T) {
        for _ in 0..4 {
            black_box(f());
        }
        for sample in 1..=3 {
            COUNTS.set(Some(Counts::default()));
            for _ in 0..self.count {
                black_box(f());
            }
            let counts = COUNTS.take().expect("missing allocation measurement");
            println!(
                "{},{},{},{},{},{},{},{}",
                self.name,
                sample,
                self.count,
                counts.alloc,
                counts.zeroed,
                counts.realloc,
                counts.free,
                counts.bytes_requested
            );
        }
    }
}
