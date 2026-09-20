# Reader voices and editor acceptance - 2026-09-06

## Implemented

- Independent editor narration paint using exact source projection, guarded by
  saved lease and editor revision. Editing or changing documents stops the old
  session and clears narration. Dirty documents must be saved before starting.
- Editor stays visible above Reader controls. Controls scroll within a bounded
  area. Highlight is the current synthesized passage, not invented word timing.
- Named designed profiles, immutable saved revisions and per-note narrator
  selection. Missing assigned revisions fail explicitly.
- Manual current-passage character assignment, stored against the document
  revision. Narrator changes do not override explicitly assigned characters.
  Reload applies a new cast/session and discards previous pending work.
- Dense per-utterance voice identities flow through the existing bounded
  prefetch pipeline. Only affected synthesis identities invalidate audio.
- Saved BRZV reference support in a separate native worker candidate. Files are
  validated and mmap-pinned. EOS plus the quiet barrier still gates publication.
- Bundle hashing checks cancellation between 1MiB chunks without changing hashes.

## Verified

- Focused contract suite:50 tests, formatting and Clippy passed, including
  interrupted-generation nonpublication and wrong voice audio rejection.
- GPUI projection suite:6tests passed through C: junction after D: compilation.
- Live first preview: editor layout, dirty-start rejection, saved revision1,
  named profile creation and persisted selection.
- Real saved-reference worker smoke:2passages, verified completion and cache.
  Warm first PCM406ms;7.084s total for5.44s audio while build work was running.
  This is not a sustained-speed or speaker-consistency qualification.

## Remaining qualification

Reference enrollment UI, automatic following, finer-grained utterance assignment,
and a representative uninterrupted performance run. Word alignment is not claimed.
Graph backend live qualification and GLiNER2.5 promotion remain release blockers.

## Receipts

- D:/phoenix-target-reader-contract/reader-contract-proof.json
- D:/phoenix-tts/voices-session-binding-tests.log
- D:/phoenix-tts/voices-editor-tests.log
- D:/phoenix-tts/reader-voices-ui-20260906/build.json
- D:/phoenix-tts/reference-voice-qualification-01/smoke/passed.json

This is a development preview, not a sealed or frozen release.

User listening receipt: reported 'same' for the two saved-reference clips. This supports perceived speaker consistency for these two passages; no broader quality rating inferred.

## Live restart and casting acceptance - 2026-09-12

Restarted the existing preview after the PC update, with executable SHA-256
`E42F9C90CC1B4DE476DB72B80DEA3DFA98030E2BB543EB300F1518B60354BF33`.
The executable matched binding-build.json; no rebuild was needed. Launch retained
the analysis bridge, GLiNER v2.0 model root and ModernBERT model root arguments.
This verifies launch configuration, not graph production or GLiNER2.5 promotion.

- Saved revision 1 and Storyteller QA selection survived the computer restart.
- Loading Markdown restored chapter 2, segment 7/8, one second into the segment.
- In the actual UI, assigned Traveler QA / Calm narrator to the current passage;
  the UI confirmed the cast was saved. Reload created the revised voice session.
- Playback visibly used Storyteller QA on segment 6, Calm narrator on segment 7,
  and Storyteller QA again on segment 8. The editor highlighted each active source
  passage. At segment 8 the counters showed zero rebufferings and device gaps.
- Typing one character in the disposable document during segment 8 stopped the
  session, cleared narration paint, and displayed the document-changed message.
  The test edit was undone; the preview was closed without saving a new revision.
- Cold model verification took noticeably longer after the PC restart; this pass
  does not qualify startup latency or uninterrupted long-form throughput.

Launch receipt: D:/phoenix-tts/reader-voices-ui-20260906/restart-20260912-230127.json.
The test workspace remains separate from the user's primary workspace.
