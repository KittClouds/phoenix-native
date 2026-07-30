# Clean-room Cut 0 receipt

Date: 2026-07-29

Verdict: pass

Amendment, 2026-07-29: before the first V2 writer was frozen in Cut 1, the
publication receipt's self-referential `generation_hash` field was corrected
to the non-circular `authority_hash`. The schema-directory hash is now
`3d0486e6e33145fcebbca97186edd2c55d6d5ae76f0e2213106933414e54a66a`.
No V1 bytes or tags changed.

Cut 2 amendment, 2026-07-29: before any V2 kernel or application
registration, candidate evidence ranges were moved from the general evidence
page to a dedicated packed `CandidateEvidenceBindings` page. This prevents
overlapping candidates from accidentally claiming unrelated adjacent evidence.
A second authoritative page preserves source-to-canonical identity bindings
and coordinator decision IDs. The directory now contains 28 required pages.
The amendment changes no V1 bytes, live authority, workspace data, or
application registration.

## Scope

Cut 0 changed only:

- workspace membership and lockfile registration
- an unused `phoenix-graph-generation-v2` contract crate
- clean-room contract, inventory, disposition, and receipt documents

No application, kernel, producer, compiler, renderer, V1 source, workspace
data, scene archive, database, or model artifact was changed.

The Phoenix application was not launched.

## Static oracle

The expected legacy topology families are transcribed into
`tests/angular_expected_topology_oracle.rs`. It is test-only:

- no Angular source is compiled
- no Angular or TypeScript type is imported
- no Angular runtime or persisted store is opened
- no JSON fixture is loaded
- no production dependency can consult the oracle

## V1 preservation

| File | SHA-256 before and after |
|---|---|
| `src/build.rs` | `7DC4360BE5716AA54989734C4B2A998AA4E22F57DA7016AD7C1986D581DFEAE3` |
| `src/error.rs` | `D0203BF9B4FFF6A64F17B6227883C9BC9D05B66D2AF0C4A193C2BA4B542D8BC7` |
| `src/format.rs` | `B0BA05876599FDF2D53353471D50DEA6495BAE861747372A697F4C8F1982DD7F` |
| `src/ids.rs` | `73E0E62406F9B533B34503FC8C7FCB70D49C0B4C6DC31671DAE7F0767129E6B2` |
| `src/lib.rs` | `E9557356FEB66D288A6FDACFA299FA035C4CF11B4EB163D24F261018DAF101E7` |
| `src/open.rs` | `F66F32911CACF07ECF28FC29954D33CFB20C8CA7853D0551CA6DF4A6E052F625` |
| `src/tests.rs` | `ADC309CFC3A2E0A6BCC192C229B579A8EFC8E2A7B116BCA67D83A77E2957DDFF` |

## Verification

Target:

```text
D:\phoenix-target-cleanroom-contract-v2
```

Commands:

```text
cargo fmt --all -- --check
cargo test -p phoenix-graph-generation-v2 -p phoenix-graph-generation -p phoenix-scene-compiler
cargo clippy -p phoenix-graph-generation-v2 --all-targets -- -D warnings
```

Focused test result:

- V1 graph generation: 8
- V2 unit tests: 9
- V2 oracle tests: 2
- scene compiler: 13
- total: 32 passed, 0 failed
- V2 Clippy: passed with warnings denied

All three commands passed on the scoped target.

## Stop/go

| Gate | Result |
|---|---|
| No source/semantic/projection ambiguity | Pass: every page has one frozen authority; projections have no V2 page |
| V1 readable and unchanged | Pass: source hashes preserved and V1 tests included |
| V2 represents every expected topology family | Pass: static oracle covers twelve clean-room families |
| No implementation imports an Angular type | Pass: V2 has no Angular dependency or type |
| Rollback remains additive | Pass: remove V2 membership, crate, lock entry, and documents |

## Deliberately not claimed

- No producer emits V2 yet.
- No production consumer reads V2 yet.
- No V2-to-scene compilation exists.
- No semantic parity, live application, or performance claim is made.
