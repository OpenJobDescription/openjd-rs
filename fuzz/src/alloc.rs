// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Allocation tracking for budget oracles.
//!
//! The expression evaluator promises memory-bounded evaluation: it must
//! check its budget *before* allocating, not after (`zfill('5', -1)`
//! aborting the process, and `center`/`ljust`/`replace`/`join` building the
//! result before the budget check, were both bugs of this shape). A
//! post-hoc check on the result value cannot see a transient allocation
//! bomb, so this global allocator records the peak live bytes allocated on
//! the current thread inside a [`measure`] scope.
//!
//! Accounting is per thread (const-initialized thread-locals, which do not
//! allocate), so parallel `cargo test` threads do not pollute each other.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct Tracking;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
}

fn add(delta: isize) {
    let _ = LIVE.try_with(|live| {
        let now = live.get() + delta;
        live.set(now);
        let _ = PEAK.try_with(|peak| {
            if now > peak.get() {
                peak.set(now);
            }
        });
    });
}

unsafe impl GlobalAlloc for Tracking {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            add(layout.size() as isize);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            add(layout.size() as isize);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        add(-(layout.size() as isize));
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            add(new_size as isize - layout.size() as isize);
        }
        p
    }
}

#[global_allocator]
static GLOBAL: Tracking = Tracking;

/// Run `f` and return its result with the peak number of bytes it had live
/// at once on this thread, above what was live when it started.
pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(base));
    let r = f();
    let peak = PEAK.with(Cell::get);
    (r, (peak - base).max(0) as usize)
}
