# TEST-003: Native help acceptance assumes an adjacent heading stays visible

Status: fixed; independent re-review clean.

`test_space_question_explains_the_current_page` searches for the full Page size
action description and then waits for the adjacent `Columns and paging` group.
On current Runyte, selecting that long search match horizontally scrolls the
help document. The heading exists but its first words are outside the viewport,
so the test times out. The baseline screenshot shows the clipped heading as
`paging` and the matched action as `— Set browse page size`.

Search for the heading explicitly after verifying the action description. This
checks both registered strings independently and permits ordinary horizontal
scrolling behavior. No application or host behavior needs changing.

The focused case passes in 4.524 seconds on Linux x86-64 against the same local
Runyte 0.3.5 (`ee679f45`) host with all five current-feature expectation flags
enabled. Other platforms and historical-host execution are not claimed here.
