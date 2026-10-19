//! Memory-leak proof for the paired `c_parser_parse` / `c_parser_free_string`
//! calls.
//!
//! This test lives in its own binary (separate from the other FFI tests) so a
//! process-wide counting allocator can be installed and observed without
//! interference from concurrently running tests.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use c_parser_ffi::{c_parser_free_string, c_parser_parse};

// ---------------------------------------------------------------------------
// Counting global allocator: tracks the net number of live bytes.
// ---------------------------------------------------------------------------

struct CountingAllocator;

/// Net live bytes = sum(allocated) - sum(deallocated).
static NET_ALLOCATED: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            NET_ALLOCATED.fetch_add(layout.size() as isize, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            NET_ALLOCATED.fetch_add(layout.size() as isize, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        NET_ALLOCATED.fetch_sub(layout.size() as isize, Ordering::SeqCst);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            // Live bytes change by (new_size - old_size), regardless of
            // whether the block was moved in place or copied.
            NET_ALLOCATED.fetch_add(new_size as isize - layout.size() as isize, Ordering::SeqCst);
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// A slightly richer source so each cycle allocates a non-trivial AST.
const SOURCE: &[u8] = b"\
int add(int a, int b) { return a + b; }\n\
int main(void) {\n\
    int x = 1;\n\
    int y = add(x, 2);\n\
    return y;\n\
}\n";

const CONFIG: &[u8] = b"{}";

#[test]
fn paired_parse_free_does_not_leak() {
    // Warm up so one-time lazily-initialized allocations settle before we
    // take the baseline.
    for _ in 0..3 {
        // SAFETY: `SOURCE`/`CONFIG` are valid, live slices.
        let p =
            unsafe { c_parser_parse(SOURCE.as_ptr(), SOURCE.len(), CONFIG.as_ptr(), CONFIG.len()) };
        assert!(!p.is_null(), "c_parser_parse returned NULL during warm-up");
        // SAFETY: `p` came from `c_parser_parse` and is freed exactly once.
        unsafe { c_parser_free_string(p) };
    }

    let baseline = NET_ALLOCATED.load(Ordering::SeqCst);

    const ITERATIONS: usize = 2_000;
    for _ in 0..ITERATIONS {
        // SAFETY: `SOURCE`/`CONFIG` are valid, live slices.
        let p =
            unsafe { c_parser_parse(SOURCE.as_ptr(), SOURCE.len(), CONFIG.as_ptr(), CONFIG.len()) };
        assert!(!p.is_null(), "c_parser_parse returned NULL");
        // SAFETY: `p` came from `c_parser_parse` and is freed exactly once.
        unsafe { c_parser_free_string(p) };
    }

    let after = NET_ALLOCATED.load(Ordering::SeqCst);
    assert_eq!(
        after,
        baseline,
        "leak detected: {} bytes retained after {} parse/free cycles",
        after - baseline,
        ITERATIONS
    );
}
