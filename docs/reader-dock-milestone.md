# Reader dock milestone

2026-09-12. Implementation of the first slice of reader-product-design-plan.md.

## Changes

- Replaced the 350px scrolling control form with an 88px dock. Primary listening
  action, chapter controls, current voice, bookmark and options stay near the page.
- Voice selection and the passage casting form use a contextual right sidebar.
  Reader entry hides the inspector; closing Reader restores its prior visibility.
- Technical diagnostics and parser choice live under Playback details. Display
  states are typed and translated into preparation, ready, paused and error copy.
- Listen loads the saved document and starts playback. Save & listen uses the
  existing kernel save path. Retiring a prior controller runs on the background
  executor; cancellation prevents a pending replacement from starting.
- Opening the casting form pauses audio and captures a fixed passage and excerpt.
  Voice assignments continue to use existing revision-bound cast/cache contracts.
- Embedded vector icons avoid network or external asset dependencies. Editor assets
  continue to delegate to Velotype's asset source.
- Unchanged status snapshots no longer trigger repeated shell render notifications.
- Reader hides caret-only formatting controls; selected text retains its toolbar.
- Narrator cards show name, description, Designed/Reference type, an explicit Use
  as narrator action and a checked Selected narrator state. Current passage voice
  is separate from the narrator selection. Both existing library profiles are
  designed voices; no reference enrollment is implied.

## Verification

- 50 contract tests, formatting and Clippy passed. Receipt:
  D:/phoenix-target-reader-contract/reader-contract-proof.json.
- Two standalone presentation-state tests passed from the C: junction after D:
  compilation: preparation cancellation vs playback pause, edit/error recovery.
- First live dock candidate (2DDE989A...) verified 88px layout, one-action loading,
  retry, playback, pause, source highlighting and sidebar disclosure at1280x830.
  Found and fixed cancellation being presented as failure, undersized form inputs,
  and a stale inspector footer label.
- Final explicit-voice build succeeded without reported warnings:
  D:/phoenix-tts/reader-dock-explicit-voices-build.log.
- Final live executable SHA256:
  6588004D9958B36F9CFA876A297279DE63D91D74128F6F00E356D3144D6E5534.
  Launch receipt: D:/phoenix-tts/reader-voices-ui-20260906/explicit-20260912-235539.json.
  PID32904 at launch; reverify before process operations.
- On the final executable at1280x830: header Listen starts preparation, caret-only
  toolbar disappears in Reader, cancelling returns to Listen without failure,
  narrator cards show description/type/selected state, Use as narrator updates the
  card and dock, voice form inputs occupy the panel width. Restored Storyteller QA
  after testing Calm narrator; app remains open with narrator cards visible.
- Full DPI matrix, keyboard/screen-reader acceptance and performance benchmarks
  remain open. Final full playback after these presentation refinements was not
  repeated; playback/pause/highlighting were observed on the first dock candidate.

## Scope limits

Automatic following, typography preferences, arbitrary selection casting, reference
enrollment, seek/time scrubber, speed control and full accessibility qualification
are later slices. No dummy controls represent those capabilities. Graph backend
qualification and GLiNER2.5 promotion remain separate release requirements.
