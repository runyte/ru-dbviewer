# Upstream provenance

The runtime is an independent Rust implementation of Runyte's public protocol;
it neither vendors a runtime SDK nor links private Runyte APIs.

`tests/fixtures/runyte-1.schema.json` and `tests/fixtures/stable-fixtures.json`
are copied unchanged from `docs/plugins/` in
https://github.com/runyte/runyte at
`cd711f294716a52a800d701016374026036c9b71` (MPL-2.0).
This revision is also the native host pinned in CI; its package reports 0.3.0.
The source pin is provenance, not a claim about a published release tag.

The `Screen` and `NativeEditor` PTY harness in `tests/native.py` is adapted from
`tests/test_native.py` in https://github.com/runyte/ru-time at
`4e586d9316a9a3d675818e0cc75ede0c19001e0d` (MPL-2.0). The database-specific tests
and configuration are maintained here. No sibling checkout is needed at runtime.

`LICENSE` is the MPL-2.0 license text from Runyte at the revision above.

`third_party/objc2-LICENSE.md` is the upstream licensing notice at
https://github.com/madsmtm/objc2/blob/7b1abfd750a2cacaea71d6a56ecfb83cb7de560b/LICENSE.md .
The `objc2-core-foundation` and `objc2-system-configuration` 0.3.2 registry
packages omit their root notice; their Cargo VCS metadata identifies that revision.
The release collector supplies this notice and the Apache-2.0 option's full text
from `third_party/Apache-2.0.txt` (standard text copied from serde 1.0.229).

`tests/fixtures/view-row-actions.json` is the additive `row.actions` property
from the coordinated `view-row-actions` host change (MPL-2.0). Feature-enabled
wire tests layer it onto the retained base schema; fallback tests continue to
validate against that unchanged base schema. It does not replace or claim a
new upstream revision for the frozen fixtures above.

`tests/fixtures/input-path-completion.json` is the additive field definition from
the coordinated `input-path-completion` host extension (MPL-2.0). Feature-aware
tests layer it onto the unchanged frozen schema; legacy tests omit it.
