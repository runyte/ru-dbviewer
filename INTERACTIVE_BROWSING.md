# Interactive browsing implementation

## Public API audit

The integration boundary remains runyte-1. The optional `view-row-actions`
feature adds selection-dependent actions on supporting hosts. Native forms support required fields
and revision-checked asynchronous validation. They do not expose path completion
or initial values. Pickers support search but only one choice. Native view rows
have stable IDs; base-profile actions are model-wide. Commands have
a single context, so global db-* aliases need separate registrations from the
short contextual action names. pane.show can return to an explicit retained
parent without rerunning SQL. Ordinary SQL buffers must use a command to return.

## Implementation

The plugin uses existing public APIs: revision-checked validation for connection
forms, explicit submitted path completion, searchable paged choices for columns,
and retained view identities for parent navigation. The generated configuration
adds the scoped `back: "-"` binding; no global SQL editing key is taken over.
`db-return` and `db-transactions` are the new documented global aliases.

Browse requests bind values in both adapters. Generated SQL uses separately tested
literal rendering. View generations are retained through inspection descendants.
Query guidance and filenames are presentation; associations remain captured state.
JSON trees preserve original number spelling and duplicate object keys.

On hosts without `view-row-actions`, Databases exposes a
`profile-actions` picker for the captured selected profile and generation.
Disconnected profiles offer connect; live profiles offer query, mode, disconnect
and transactions; pending transactions also offer commit/rollback. Only unresolved
outcomes offer acknowledgement. Connection-specific views omit unavailable
settlement, disconnect and acknowledgement actions. This explicit profile menu
preserves compatibility with the original stable host. When the optional
feature is negotiated, Databases instead publishes per-row actions: connected
profiles expose query/mode/disconnect directly in Tab, disconnected profiles
expose connect, and pending/uncertain states expose only their applicable
settlement/recovery actions. Headers expose connect-new and transactions.
The plugin omits the new row fields entirely when the feature is unavailable.
Completion similarly uses explicit submission, rather than silently extending
public input fields or private DTOs.

## Acceptance

See VALIDATION.md for actual checks and remaining platform gates. No publication,
release, or completion of the coordinating Runyte plan is implied by this record.
