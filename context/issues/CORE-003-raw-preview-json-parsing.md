# CORE-003: Raw JSON previews build and discard the formatted tree

Status: fixed; independent re-review clean.

`inspection::model` parses every complete JSON preview into an owned tree even
when raw mode is requested. Raw mode never uses that tree: its model comes from
the retained cell text and exposes the same actions regardless of parse success.
Broad JSON objects therefore allocate thousands of unused keys and nodes before
displaying their original text on the application loop.

Skip tree parsing in raw mode. An allocation-counting regression checks a broad
JSON fixture while asserting the returned raw rows preserve the captured text.

The regression failed before the correction at **18,378 allocations** and now
passes at **355 allocations** for 2,000 small JSON objects. All four formatting
integration tests and all six inspection unit tests passed on Linux x86-64.
