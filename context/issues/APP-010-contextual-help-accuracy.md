<!-- SPDX-License-Identifier: MPL-2.0 -->
# APP-010: Contextual help overstates confirmations and navigation behavior

Status: fixed; independent re-review clean.

The Transactions topic and README promise a confirmation before Commit/Roll
back, although both existing commands deliberately settle the selected pending
transaction directly. The Schema topic always promises Back to Tables, even
when schema inspection was opened from Rows. The Value topic describes every
JSON display as an expandable outline, although complete documents and bounded
preview fallbacks use readable text.

Resolution: correct the prose to match existing behavior, including target
connection checks, captured parent navigation, and the distinction between
preview outlines and complete-value documents. No extra prompts or UX changes.

Validation: the independent help reviewer verified all three discrepancies and
confirmed topic/schema bounds. The focused Rust topic-bounds test passed after
the changes, and the same independent reviewer confirmed that all corrected
prose matches the existing implementation. No runtime behavior changed.
