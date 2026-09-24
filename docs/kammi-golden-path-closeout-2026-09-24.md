# KAMMI golden-path closeout — 2026-09-24

## Scope

This product-only change set brings KAMMI's local `llama.cpp` path through an
observable note workflow while keeping editor authority and revision checks in
Phoenix. It was developed on `codex/phoenix-kammi-golden-path-20260924` in the
isolated checkout `D:\phoenix-product-snapshot-20260923`. The shared dirty
`clean-rust` checkout, QPS/science work, and other agents' branches were left
untouched.

## Implemented

- Added **Bonsai 2 · 27B** as a featured OpenRouter model preset with its exact
  provider/model ID and a one-click selection action. The local GGUF workflow
  stays provider-neutral; the tested local model below is MiniCPM5, not Bonsai.
- Replaced the reasoning-effort button grid with a labeled discrete slider for
  Auto, Off, Min, Low, Medium, High, Max, and XHigh. The selected value remains
  provider-specific and persists with KAMMI settings.
- Shows only the configuration panel for the selected provider. Local
  `llama.cpp` UI clarifies executable, GGUF, endpoint, context, GPU layers,
  thread count, performance profile, validation, and the fork requirement for
  the Bonsai GGUF.
- Added a persisted, opt-in active-note context control. When enabled, KAMMI
  receives a revision-tagged, read-only Markdown snapshot, capped at 8 KiB for
  local inference or 64 KiB for OpenRouter, with a visible truncation marker.
  Graph and memory are not silently added to this context.
- Tied every completed response to the editor revision, block UUID, byte
  offset, and target preview captured when the request started. Stale targets,
  incomplete replies, missing leases, and empty content fail closed.
- Added explicit **ADD AFTER** and **REPLACE BLOCK** actions. Both use the
  native editor operation; replacement is atomic and undoable. Saving goes
  through the document lease and rolls the editor back if the kernel save
  fails.
- Added `phx block replace` with required note identity, block UUID, expected
  document revision, and idempotency key. It uses a canonical payload digest,
  persists the edit with undo/rollback, and returns a revision receipt.

## Evidence and gates

| Gate | Result | Evidence |
|---|---|---|
| Product isolation | PASS | Dated branch in an isolated product checkout; only the ten scoped files plus this closeout note are included. |
| Bonsai 2 discoverability | PASS in source/build | Featured OpenRouter preset uses `prism-ml/ternary-bonsai-2-27b`. |
| Reasoning control | PASS in source/tests | Slider mappings cover all eight provider values and clamp out-of-range input. |
| Context safety | PASS in tests | Local 8 KiB and remote 64 KiB caps, UTF-8-safe truncation, revision label, and visible truncation tests. |
| Local inference | PASS on candidate | Phoenix launched the process-isolated `llama-server`; local MiniCPM5-2B-Q8_0 answered from the active note's first 8 KiB. No OpenRouter call was used in this test. |
| Add and durable readback | PASS on candidate | A completed local response was added after its captured heading. `phoenixctl phx note cat note://3` read the saved content at document revision 3. The source note already contains unrelated fenced-code/blank-block material nearby; comparison with the untouched source copy confirmed the insertion itself remained a paragraph. |
| Replace and durable readback | PASS on isolated QA workspace | Replaced the added paragraph at expected revision 3; receipt advanced document revision to 4. A subsequent read returned `KAMMI replacement gate passed.` |
| Stale-write protection | PASS on isolated QA workspace | Repeating a replacement with expected revision 3 after revision 4 returned `conflict: stale document revision`; no mutation occurred. |
| Editor transaction behavior | PASS | Five `velotype::editor::agent_commands` tests pass, including insertion serialization, atomic revision-bound replacement, undo/redo, and stale/empty rejection. |
| Control parser | PASS | Five `phoenix-agent-control::parser` tests pass, including revision-checked insert and replace forms. |
| KAMMI suite | PASS | `cargo test -p phoenix-shell kammi:: -- --nocapture`: 28 passed, 0 failed. |
| Live replacement-button click-through | NOT VERIFIED | The current computer-use bridge exposes browser surfaces only and returned no native Phoenix app controls in this run. The open candidate remained running; replacement was verified through editor tests and `phoenixctl`, not by clicking the UI control. |

The candidate executable is `C:\phoenix-bin\phoenix-kammi-golden-path-20260924\phoenix-shell.exe`.
At the last hash check it was SHA-256
`4CF167595F05914427AF1E74877C6AA3658588EF8E6483A34CF65C6DE27C0C0C` and was
running as PID 11228 against the copied QA workspace. Its graph scene had 86
entities loaded from the QA copy. The frozen Reader executable remained open
as PID 2412. The local `llama-server` candidate was PID 16176.

## Boundaries

- Active-note context is opt-in and bounded; a long note is truncated. The
  current local test exercised an excerpt from the beginning, not full-note
  retrieval.
- KAMMI does not yet have an autonomous model-to-tool loop. The model receives
  a read-only note snapshot; a human explicitly approves adding or replacing
  the response through the UI. The CLI replacement command is separately
  available and revision-checked.
- This closes the local note read/add/replace slice and configuration surface.
  It does not qualify graph queries as model context, persistent-memory
  context, automatic note creation, or end-to-end session recovery.
- The QA copy was deliberately used for note mutations. Keep it separate from
  the frozen Reader workspace.
- The open candidate's persistent QA note was advanced to revision 4 by the
  external CLI replacement test after the app's live add test. Its in-memory
  editor may still show revision 3; relaunch the candidate against the QA copy
  before continuing to edit that note. The Reader app remains separate.
