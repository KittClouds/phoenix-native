# Reader voices and editor integration: active build checkpoint

User authorized implementation after planning on 2026-09-06. Preserve unrelated
dirty Kammi, graph, experiments and existing running Phoenix processes. Build on
D:, test through C:/phoenix-bin junctions. Do not commit or clean the checkout.

## Accepted scope

1. Stable named narrator profiles, audition/reference persistence, supervised
   native Breeze saved-voice support. Distinguish voice identity from delivery.
2. Reader controls in editor, independent narration highlight, exact source
   projection, playback-clock timing, pause/clear on edits and revision changes.
3. Manual per-book character cast and utterance assignments; narrator fallback
   for unassigned text, no silent fallback for missing assigned voices.
4. Per-utterance synthesis/cache identity, bounded prefetch and cancellation,
   restart persistence; meaningful unit/controller/live UI qualification.

## Baseline evidence

Reader prefetch: 39 tests passed. Real device smoke
`D:/phoenix-tts/reader-prefetch-smoke-03/passed.json`: zero rebufferings/device
starvations, five segments generated during playback. Live UI acceptance recorded
in `reader-prefetch-acceptance.md`.
Current model runtime: D:/phoenix-tts/breeze-native-20260906; supervised worker
source is crates/phoenix-tts-native/native/worker.cpp (PBN1). Source-only current
voice instruction is fixed; reference controls are NOT implemented at baseline.
Current shell build: D:/phoenix-target-shell-publish (C:/phoenix-bin/reader-shell-preview-20260906).
Tests: scripts/verify-reader-contracts.ps1, target D:/phoenix-target-reader-contract.

## Release blocker retained

Reader-only launch omitted graph producer/model arguments. Existing qualified
producer C:/phoenix-bin/atlas-runtime-qualified-cbf349be-20260813/phoenix-analysis-bridge.exe,
NER D:/hf-models/gliner-bi-base-v2.0-onnx, NLI D:/phoenix-models/modernbert-base-nli-onnx.
Path configuration enabled Warm Models; graph generation with new shell is still
unqualified, as is GLiNER 2.5 promotion. Do not call this a frozen release.

## Current build progress

Resumed after account switch. Implemented native BRZV saved-voice protocol kind 12,
immutable mmap voice validation, voice/cast profiles, dense utterance voice slots,
and provider fault tests. Separate candidate worker compiled successfully at
D:/phoenix-tts/breeze-native-20260906/build/phoenix-breeze-voices-worker.exe;
original worker retained. Real saved-reference inference remains unqualified.

45 focused contract tests, formatting and Clippy passed on resumed run:
D:/phoenix-tts/voices-contracts-resumed.log. Expanded persistence tests passed in
D:/phoenix-tts/voices-contracts-integrated.log (check script exit/receipt before claiming final).

Editor narration paint layer is independent from semantic/entity highlights.
Reader now renders editor above controls, publishes projected source ranges from
playback segment, clears/stops on edit or changed document lease, rejects dirty
start. Baseline shell check passed; latest named voice/casting UI still building
in D:/phoenix-tts/voices-editor-build.log (session 55671).

Named designed narrator creation and per-book selection persist under configured
storage/voices. Manual current-passage casting stores revision-bound assignments
and requires reload to apply. Configured saved reference voices use hash-pinned
assets; reference enrollment UI and precise sub-passage casting not implemented.
Per-utterance prepared voice travels with generation job; cache key and runtime
voice table match. Backend cast selection loads from voice library or explicit config.

Owned disposable preview PID 23548 was closed via Computer Use Alt+F4 for rebuild.
Main Phoenix PID 26104 untouched. Do not claim live UI acceptance of new changes yet.
Next: finish build; run focused tests again only after new fixes, qualify GPUI
narration test and current controller smoke, launch preview with graph producer
arguments preserved, exercise narrator creation/selection, passage casting,
highlight/pause/edit invalidation, restart persistence. Keep checkpoint updated.


Additional resumed work: immutable voice catalog listing, per-book selected profile,
revision-bound cast persistence, named designed narrator creation UI, current-passage
casting UI. Character voice assignments survive narrator overrides (regression added).
Latest contract run: D:/phoenix-tts/voices-contracts-final.log (use process/log truth).
Latest build may need an incremental rerun because source was improved while the
first shell compile was still running. Always run build again after it finishes
before launch. Current GPU readiness: RTX3080 12288MiB total / 9400MiB free.
No actual saved-reference inference yet; no .breeze files existed at resume.


Live acceptance in new disposable workspace D:/phoenix-tts/reader-voices-ui-20260906:
first final build SHA256 3649BC52F20980C9014A7987EF9FB427B792C12F94FF5DB3C355323993870017,
PID34660 (now gracefully closed for cancellation fix). Actual editor shown above
Reader controls. Dirty start rejected. Focused Ctrl+S committed revision1. Created
Storyteller QA via UI, selected it, verified 2 immutable profiles and selected-book
file. Model hashing made startup sluggish; cancellation now checks each 1MiB mmap
chunk (hash identity unchanged), test coverage added. Latest replacement build
D:/phoenix-tts/voices-editor-cancel-build.log/session28450. Need launch after success.
GPUI projection suite: all6 passed at D:/phoenix-tts/voices-editor-tests.log.
Contract idle run: cancellation250ms passed; new cast runtime test fixture quota
was too small, corrected to4MiB. Final all-contract rerun currently
D:/phoenix-tts/voices-contracts-verified.log/session91558.

Real BRZV voice encoded successfully from existing generated control-context.wav
and its exact cases.json transcript. No new third-party voice source.
D:/phoenix-tts/reference-voice-qualification-01/narrator.breeze:210frames16.80s.
Real supervised reference smoke passed: smoke/passed.json plus passage-0.wav and
passage-1.wav. Warm firstPCM406ms;7084ms generation for5.44s audio (~1.30RTF) under
concurrent compilation; do NOT qualify gap-free speed or speaker consistency.
Reference smoke source crates/phoenix-tts-native/examples/voice_reference_smoke.rs.
Reference import/enrollment UI, automatic following and finer-grained casting still
remain. Current implemented casting assigns whole planned passage, not selection.


Final revised contract suite passed (48 tests including new runtime binding and
2 cancellable-hash tests); receipt D:/phoenix-target-reader-contract/reader-contract-proof.json
updated2026-09-06 23:11:37. Separate shell voices::prepare/assign_passage integration
regression now compiling in session83152. Shell cancellation build still active
session28450 at23:12; no replacement launch yet. Main user app untouched.


Live replacement DC13F48546DDF1B756D6B15F74CB1B3BAEE34F1C150AFA78EC2225CA793229FE
PID46768 restored note revision1 and StorytellerQA selection successfully. Playback
then exposed session::set_position/resume_position comparing per-audio voice with
voice-table fingerprint. Safe rejection, no audio playback. Fixed through private
set_position_with_voices/resume_with_voices validation (full identity plus table
fingerprint+plan match); legacy public single-voice APIs retain their checks.
Runtime install/observe/transition/resume all pass bound table. New successful
cast-session play/checkpoint/resume test added. Current tests session61691,
D:/phoenix-tts/voices-session-binding-tests.log; build session93228,
D:/phoenix-tts/voices-session-binding-build.log. PID46768 gracefully closed viaUI.
Do not claim playback acceptance until this corrected build is live-tested.
User listened to both reference clips and said 'same' (same narrator).

## Recovery after PC update - 2026-09-12

The binding fix build completed and the final contract suite passed: 50 focused
tests, formatting and Clippy (voices-session-binding-tests.log). The executable
at C:/phoenix-bin/reader-shell-preview-20260906/debug/phoenix-shell.exe has SHA256
E42F9C90CC1B4DE476DB72B80DEA3DFA98030E2BB543EB300F1518B60354BF33, matching the saved
binding-build.json receipt. No compiler work was needed for this restart.

Restarted with workspace D:/phoenix-tts/reader-voices-ui-20260906/workspace.json,
the atlas-runtime-qualified-cbf349be-20260813 analysis bridge, GLiNER v2.0 root,
and ModernBERT NLI root. Graph configuration is present; graph production and
GLiNER2.5 remain unqualified. Initial restart PID5240 was gracefully closed after
the acceptance edit was undone. Latest preview PID8796 (verify before reuse),
restart receipt restart-20260912-230626.json in the same test artifact root.

Live evidence on this hash: restored revision1 / Storyteller QA, chapter2,
segment7/8 at1s. Saved Traveler QA character casting to Calm narrator through the
UI, then reloaded. Observed Storyteller QA segment6 -> Calm narrator segment7 ->
Storyteller QA segment8 with source highlights and zero reported rebufferings or
device gaps. Editing the disposable heading during segment8 stopped narration
and cleared highlighting; undo restored394chars, then closed without saving.
Cold model verification remains slow and is not latency-qualified.

Earlier idle controller smoke passed at
D:/phoenix-tts/reader-voices-controller-idle-02/passed.json (pause, bookmark,
chapters, restart/resume, shutdown; zero rebufferings/device starvations).
This does not erase the earlier loaded-machine run with3device starvations.

Remaining: reference enrollment UI, automatic editor following, finer-grained
casting, long-form uninterrupted performance, graph live qualification and final
packaged release qualification. See reader-voices-editor-acceptance.md.

Final visible state after the second restart: Reader open, saved revision1 /
394chars, Storyteller QA, chapter2, segment8/8 at4s, ready to resume. This restores
the revised cast session after process exit; no automatic playback was started.

## Reader product UI milestone - late 2026-09-12

Implemented reader-product-design-plan.md first slice: compact88px dock, typed
status copy, one-action Listen/Save & listen, background controller retirement,
contextual voice sidebar and hidden diagnostics. User requested more explicit voice
selection: cards now show description, Designed/Reference type, Use as narrator and
checked Selected narrator. Casting captures a fixed passage when the form opens.
Caret-only editor toolbar hidden during Reader; selected-text formatting preserved.

Latest built exe at the same C: junction has hash
6588004D9958B36F9CFA876A297279DE63D91D74128F6F00E356D3144D6E5534.
PID32904 launched with original disposable workspace and graph backend arguments.
See reader-dock-milestone.md for verified behavior and outstanding qualification.
50 contracts +2 presentation tests passed; final live narrator switching, cancellation,
layout and input sizing checked. No sealed release or graph promotion claimed.
