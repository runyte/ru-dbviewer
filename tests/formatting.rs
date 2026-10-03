// SPDX-License-Identifier: MPL-2.0
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct CountingAllocator;
thread_local! {
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
    static ALLOCATED_BYTES: Cell<Option<usize>> = const { Cell::new(None) };
}

fn record_allocation(size: usize) {
    let _ = ALLOCATIONS.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + 1));
        }
    });
    let _ = ALLOCATED_BYTES.try_with(|count| {
        if let Some(value) = count.get() {
            count.set(Some(value + size));
        }
    });
}

// Forward the unchanged allocation contract to the process allocator. Counting
// is thread-local and enabled only around the formatting operation under test.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation(size);
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[test]
fn table_preview_allocations_follow_display_size_not_retained_value_size() {
    use ru_dbviewer::{
        results::{Cell as ValueCell, Column, Data, MAX_VALUE},
        views::Content,
    };
    let mut data = Data {
        columns: (0..8)
            .map(|i| Column {
                name: format!("c{i}"),
                kind: "text".into(),
            })
            .collect(),
        ..Data::default()
    };
    for _ in 0..7 {
        assert!(data.push(vec![ValueCell::new(Some("x".repeat(MAX_VALUE))); 8]));
    }
    let content = Content::Result {
        name: "fixture".into(),
        data: std::sync::Arc::new(data),
        table: None,
        page: 0,
        offset: 0,
        columns: Vec::new(),
        record: None,
        source: String::new(),
    };
    ALLOCATED_BYTES.with(|count| count.set(Some(0)));
    let model = content.model("ready", 100);
    let allocated = ALLOCATED_BYTES.with(|count| count.replace(None).unwrap());
    eprintln!("seven rows of eight 64 KiB values: {allocated} allocated bytes");
    assert!(
        allocated < 512 * 1024,
        "preview allocated {allocated} bytes"
    );
    let rows = model["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 7);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["id"], index.to_string());
        for cell in row["cells"].as_array().unwrap() {
            assert_eq!(cell["text"], "x".repeat(508) + " …");
        }
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

#[test]
fn bounded_previews_preserve_unicode_controls_null_empty_and_truncation() {
    use ru_dbviewer::results::{Cell as ValueCell, MAX_VALUE};
    let cells = [
        ValueCell::new(None),
        ValueCell::new(Some(String::new())),
        ValueCell::new(Some("é🦀\n\0\t\u{2028}\u{2029}".repeat(100))),
        ValueCell::new(Some("é🦀".repeat(MAX_VALUE))),
        ValueCell {
            text: Some("é🦀".into()),
            truncated: true,
            ..ValueCell::default()
        },
    ];
    for cell in cells {
        let full = cell.display();
        for max in (0..100).chain([512, 2000, MAX_VALUE * 8]) {
            assert_eq!(
                cell.display_short(max),
                ru_dbviewer::views::short(&full, max),
                "preview limit {max}"
            );
        }
    }
}
