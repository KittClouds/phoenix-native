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
| Narrator continuity | Unassigned prose keeps the selected speaker across independently generated passages; intentional character casting remains separate. | The user reported audible speaker changes during the long run. Live inspection showed Calm narrator, a description-only Breeze design, selected for the review book. The app sent the same profile and seed for each passage; a design has no fixed reference recording. At the user's request, the book was switched to Woods, a saved Breeze reference voice. The dock showed Woods from passage 1 through passage 35. The candidate voice panel copy distinguishes designs and reference voices; that copy is not in the running binary yet. | Operational selection passed; long-form acoustic consistency pending user listening |
| Freeze listening | Long-form Breeze and Supertonic listening, cold and cached starts, repeated pause/resume, voice continuity, and the seven-day listening gate. | No new long-form or seven-day run in this cut. | Pending |

The Reader is not frozen. The final build passed the scoped interaction checks
and a short uninterrupted 1x playback run. The longer Breeze and Supertonic
listening pass, sustained faster-speed playback, and the seven-day listening gate remain
open. The reported design-voice drift remains an acoustic failure for that design;
the Woods switch needs a long-form listening verdict. Woods reached passage 9 with
zero rebufferings and device gaps. By passage 35 the panel showed zero
rebufferings and one device gap. A release build overlapped this run and was
stopped to protect playback; the cause of the gap is not established. A clean build and a short live pass do not establish acoustic quality or
long-form continuity.

Final review executable: `C:\phoenix-bin\phoenix-reader-closeout-v10-20260923\release\phoenix-shell.exe`.
External SHA-256: `959344207A5B38A07A19FCE71E29ED8CCD43B6AAF85841A2CF867B7B1488AA6A`.
