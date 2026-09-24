# Reader closeout gates, 2026-09-23

Scope: the Phoenix Native product branch. The review uses a copied workspace,
voice library, graph publication, and model roots. QPS and other science branches
are outside this cut.

| Gate | Acceptance | Evidence | State |
|---|---|---|---|
| Transport geometry | The primary play button is centered within the Reader pane at normal and maximized window sizes. | Visually checked in the isolated app at 1280 px and 2194 px window widths. Equal-width side regions keep the control group centered. | Passed |
| Selected speech isolation | The selection toolbar exposes a speaker action only for a valid rendered selection. It speaks the selected visible words, never neighboring text or Markdown marks. | Focused editor test passes. Live selection produced a 19-byte excerpt snapshot containing only the highlighted words. On the final build the same selection completed twice; the second run did not reuse a stale checkpoint. | Passed for the tested prose selection and repeat |
| Active note authority | Full Reader playback uses the open saved note's document lease. Edits invalidate playback; a dirty note is saved before narration. | Live switch from excerpt to the open `Welcome` note reached saved revision 1 and advanced to chapter 1, passage 7. | Passed in the review workspace |
| Long excerpt bounds | A selection longer than one TTS request is split at sentence or word boundaries without dropping source text or breaking UTF-8. | `long_selected_prose_is_bounded_without_omitting_words` passes. | Passed |
| Playback continuity | Audio quality settings are unchanged. During live playback, rebufferings and device gaps remain zero; generation proceeds while audio plays. | Selected excerpt: 0 rebufferings, 0 device gaps. Earlier full-note policies rebuffered twice by passage 20 and once by passage 36. The final bounded reservoir resumed the saved note at passage 45 and reached passage 99 in roughly four minutes of observation, with 0 rebufferings, 0 device gaps, 44 seconds buffered, synthesis RTF about 0.7, and generation during playback. | Passed for this short 1x run; extended and faster-speed runs pending |
| Playback menu and speed | The dock kebab opens a distinct settings panel. Speeds 0.85×, 1×, 1.15×, and 1.3× change tempo without changing the cached voice PCM, pitch, or saved source-frame position. | Runtime test verifies queue flush and source-frame position across a live speed change; a sine test verifies shorter duration with stable pitch. In the review app, the menu opened, 1.3× resumed from passage 99 and reached 112 with 0 rebufferings and 0 device gaps. An in-flight switch to 0.85× retained those counters. After a restart, the saved note resumed at passage 120 with 0.85× restored, then reached passage 157 with 0 rebufferings and 0 device gaps. The final executable restored 0.85× and reached passage 159, again with zero counters. Acoustic listening quality remains for the user to judge. | Function passed; acoustic qualification pending |
| Narrator continuity | Unassigned prose keeps the selected speaker across independently generated passages; intentional character casting remains separate. | Earlier listening found design-voice drift. The user reports the current Alyssa expressive run has now passed passage 100 and remains expressive and, most importantly, consistent. Woods was also previously reported consistent over several minutes beyond passage 85. The voice panel distinguishes designs, references, and directed references. | Passed for the user's current long-form listening observation; seven-day gate remains pending |
| GPU model lifecycle | Pausing retains position, releases an idle Breeze worker, and restarts it on demand; previews do not hold GPU memory during playback. | In the 2026-09-24 build, Woods paused at passage 67 and its worker exited after the 20-second idle window. GPU use fell from 8,292 to 4,357 MiB. Resume advanced to passage 72 with a new worker; directed Alyssa preview played with no worker resident. See `reader-voices-and-model-lifecycle-20260924.md`. | Passed for the live pause/resume and preview check |
| Alyssa voice direction | A delivery instruction adds enthusiastic energy while preserving the recognizable reference voice. | In the same-reference A/B short sample, the user heard the same voice with more underlying energy and judged the result good. The user now reports Alyssa expressive stayed expressive and consistent beyond passage 100. | Passed by user report for short sample and current long-form run |
| In-app voice enrollment | Clone a short WAV with an exact transcript, preview it, and make it available without restarting Reader. Retire the book worker before opening the model and release that model before spawning the clone encoder. | In the isolated QA workspace, `Gilly UI QA` was cloned from a local WAV, previewed, published, and appeared in the live list as voice 2. The resulting directed reference played through passage 5 with zero rebufferings and device gaps. | Passed in isolated QA; broader file/error cases pending |
| Voice switch position | Switching the book narrator restarts at the current sentence with a fresh voice identity, preserving the configured speed. | `voice_handoff_restarts_only_current_sentence_without_old_audio_identity` passes in release mode. Exact audio-frame transfer between different voices is intentionally not claimed. | Contract passed; live switch check pending on v13 |
| Cast edit position | Saving a passage cast and pressing Listen continues at the cast passage rather than returning to passage 1. | QA on v12 exposed a restart at passage 1 after casting passage 5. On v13 in the full workspace, I assigned Woods to temporary character `Temporary Reader QA` at passage 142. After save, Listen displayed `Preparing this passage…`, chapter 1, passage 142, and Woods in the transport. It advanced to passage 143, which returned to Alyssa expressive as expected. I restored the pretest cast archive byte for byte (SHA-256 `9F3978E1311B3A055D20FC1CA83B57F16F2B073385AD20632FE80C0799EC9B3A`) and relaunched the app. | Passed live; test assignment removed |
| Editor shortcut and cast discovery | `Ctrl+Shift+Space` speaks the visible selection or toggles Reader; the transport CAST chip opens the voice panel. | The selected-visible-text editor unit test passes and the CAST chip opened the full catalog in v13. The shortcut has not yet had a live keyboard pass. | Partial; live shortcut check pending |
| One full product instance | The screenshot app has the published graph, full voice library, and backend arguments in a single process. | V13 replaced the prior full-workspace process and the isolated QA process. The live Reader showed 13 voices with Alyssa expressive selected; the same window showed Atlas 86 entities and rendered the graph. Process inspection found one `phoenix-shell.exe`, launched with the full workspace, scene publication root, producer, NER root, and NLI root. | Passed for visible catalog and graph; fresh extraction not rerun |
| Freeze listening | Long-form Breeze and Supertonic listening, cold and cached starts, repeated pause/resume, voice continuity, and the seven-day listening gate. | The user reports Alyssa expressive remained expressive and consistent beyond passage 100 in the current session. No new cold/cached comparison or seven-day run in this cut. | Long-form Alyssa listening passed by user report; remaining freeze checks pending |

The Reader is not frozen. The final build passed the scoped interaction checks
and a short uninterrupted 1x playback run. The longer Breeze and Supertonic
listening pass, sustained faster-speed playback, and the seven-day listening gate remain
open. The reported design-voice drift remains an acoustic failure for that design;
the Woods switch needs a long-form listening verdict. Woods reached passage 9 with
zero rebufferings and device gaps. By passage 35 the panel showed zero
rebufferings and one device gap. A release build overlapped this run and was
stopped to protect playback; the cause of the gap is not established. A clean build and a short live pass do not establish acoustic quality or
long-form continuity.

Later user listening observation: Woods retained its voice over several minutes
past passage 85; Alyssa expressive also held beyond passage 80. After Reader
closed, the Breeze worker disappeared from Task Manager and GPU fan noise wound
down. These observations strengthen the continuity and lifecycle gates, while
the scheduled seven-day listening gate remains open.

Latest user listening observation: Alyssa expressive continued beyond passage
100 and was described as expressive and consistent. This is user listening
evidence; the seven-day gate remains open.

Prior review executable: `C:\phoenix-bin\phoenix-reader-voices-lifecycle-v11-20260924\release\phoenix-shell.exe`.
External SHA-256: `E93C2A302D3F9DE96025B9A0F64014C88EDE283E8954AA75AF2572FD37EF42E0`.

2026-09-24 replacement app: `C:\phoenix-bin\phoenix-reader-voice-studio-v13-20260924\release\phoenix-shell.exe`.
External SHA-256: `A2A56C60CD717864B3261A6F00003E07956D1BCCEB6F503C93DC0B1E77D6DB9B`.
The v11 identity above is retained as the prior review baseline; v13 is the running product instance.
