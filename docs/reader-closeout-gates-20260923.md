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
| Freeze listening | Long-form Breeze and Supertonic listening, cold and cached starts, repeated pause/resume, voice continuity, and the seven-day listening gate. | No new long-form or seven-day run in this cut. | Pending |

The Reader is not frozen. The final build passed the scoped interaction checks
and a short uninterrupted 1x playback run. The longer Breeze and Supertonic
listening pass, faster playback speeds, and the seven-day listening gate remain
open. A clean build and a short live pass do not establish acoustic quality or
long-form continuity.
