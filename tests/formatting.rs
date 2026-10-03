// SPDX-License-Identifier: MPL-2.0
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct CountingAllocator;
thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation() {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
}

// Forward the unchanged allocation contract to the process allocator. Counting
// is thread-local and enabled only around the formatting operation under test.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn escaping_retained_values_does_not_allocate_per_scalar() {
    for unit in ["a", "é🦀", "\n\0\t\u{2028}\u{2029}"] {
        let input = unit.repeat(ru_dbviewer::results::MAX_VALUE / unit.len());
        ALLOCATIONS.with(|count| count.set(Some(0)));
        let output = ru_dbviewer::results::escape(&input);
        let allocations = ALLOCATIONS.with(|count| count.replace(None).unwrap());
        eprintln!("{} input bytes: {allocations} allocations", input.len());
        assert!(allocations <= 16, "escaping made {allocations} allocations");
        assert!(!output.chars().any(char::is_control));
        assert_eq!(
            output,
            ru_dbviewer::results::escape(unit).repeat(input.len() / unit.len())
        );
    }
    assert_eq!(
        ru_dbviewer::results::escape("é🦀\n\0\t\u{2028}\u{2029}"),
        "é🦀\\n\\u{0}\\t\\u{2028}\\u{2029}"
    );
}
