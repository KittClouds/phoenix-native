# Phoenix Reader: product design plan

Date: 2026-09-12. Status: proposed design; no application changes in this pass.

## Product direction

Make Phoenix feel like a beautifully typeset writing desk with an audiobook player
built into it. The document is the central object. Listening follows that document;
the author can shape its performance through a discreet casting workspace.

The quality bar is a coherent, distinctive, dependable product. An award is an
aspiration, not an acceptance criterion. We can evaluate hierarchy, typography,
interaction, accessibility, responsiveness, and whether people enjoy a long session.

The signature interaction: select a passage in the editor, assign a character,
audition their voice, and hear that performance in context while the page follows.

## What the current screenshot tells us

1. The Reader panel consumes a large part of the writing surface. Its height grows
   with setup controls, even though those controls are rarely needed during listening.
2. Nearly every action has equal visual weight. Play competes with format loading,
   narrator choices, chapter navigation, bookmarks, and Voice Studio.
3. Internal states such as `NeedsAudio(7)` and buffer counters require interpretation.
   The interface exposes the implementation before explaining what the reader can do.
4. The right inspector occupies valuable space with IDs, authority revisions and a
   prominent Delete button during a listening task.
5. The formatting toolbar sits across the prose. Permanent heading rules and heavy
   chrome compete with the content. Mint accents appear too broadly to guide attention.

The existing editor projection, saved sessions, voice identities and cache boundaries
are useful foundations. The redesign should preserve them while replacing the
presentation and adding the specific missing interactions listed below.

## One document, three arrangements

Use the existing editor and document model throughout. Do not create a second text
renderer whose wrapping, selections or source ranges can diverge.

| Arrangement | Main purpose | Visible furniture |
|---|---|---|
| Write | Edit and listen while working | Workspace, editor, compact player; optional inspector |
| Read | Long listening sessions | Centered page, chapter outline on demand, compact player |
| Cast | Direct a performance | Editor with passage assignments, voice/cast sidebar, player |

Enter listening through a clear **Listen** action in the document header. Once a
session exists, the dock persists across these arrangements. A **Focus reading**
action collapses sidebars and suppresses editing affordances; **Return to editing**
restores their previous sizes and focus. Switching arrangements never restarts audio.

In the first release, changing documents pauses the old session, consistent with
current revision safety. Background listening across documents is a separate feature.

### Intended desktop layout

```text
┌──────────────────────────────────────────────────────────────────────┐
│ Phoenix    ShortRun / Chapter 5          Write  Read    Cast     ··· │
├────────────┬─────────────────────────────────────────┬───────────────┤
│ Chapters   │                                         │ Cast          │
│            │            Chapter Five                 │               │
│ 1 Arrival  │                                         │ Narrator      │
│ 2 Morning  │     Generous margins. Comfortable        │ Storyteller   │
│ 3 Journey  │     lines. The current passage has       │ Preview Change│
│            │     a restrained highlight.             │               │
│ Bookmarks  │                                         │ Characters    │
│            │     Passage assignments appear in       │ Kai    Rowan  │
│            │     the margin when casting is open.    │ Echo   Mira   │
│            │                                         │ + Add         │
├────────────┴─────────────────────────────────────────┴───────────────┤
│ Chapter 5   elapsed ━━━━━━━━━━━━━━━ available audio ━━━━━━━          │
│ Storyteller     −15        [ Play / Pause ]       +15    1.0×  ☆ ··· │
└──────────────────────────────────────────────────────────────────────┘
```

Names in this wireframe are illustrative. Cast and chapters are optional panels;
the default listening arrangement leaves both closed when space is constrained.
The elapsed/seek surface and skip/speed controls require the capability work below.

## The playback dock

Target 76–88 logical pixels tall on a desktop, with a 104-pixel compact variant.
Anchor it beneath the document area; do not place it inside the document scroll.
The first visual pass can use the smaller feature set that already works.

- Left: document/chapter title and current speaker. The speaker name opens Cast.
- Center: one dominant 44-pixel Play/Pause control, eventually flanked by ±15 seconds.
- Right: bookmark, playback speed when supported, and a labeled overflow menu.
- Upper edge: playback progress only where the duration model supports it honestly.
- Stop/end session, audio storage, runtime details and diagnostic counters go in
  secondary surfaces. Pause remains immediately accessible.

**Listen** should load the saved document and start playback in one action. The
document type chooses the planner; Markdown/plain-text parsing belongs in import
or advanced document settings, not two permanent playback buttons.

For unsaved edits, show **Save and listen** and retain the document save semantics.
For an existing session, show **Resume**. A failed preparation exposes **Retry** and
**Details**. During preparation, the main action becomes **Cancel preparation**;
during an interruption in active playback, it remains **Pause**.

### Progress must be truthful

Unrendered chapters have unknown audio duration. Show chapter/passages progress
with a text-position navigation surface until timed audio is available. Do not
draw a time scrubber from character counts or label a guessed duration as exact.

When timing is known, distinguish played audio, available audio and ungenerated
text. A seek into uncached text identifies the target passage, prepares its audio,
and reports that work. Arbitrary seek and ±15 seconds need explicit controller
commands and cross-segment tests; a clickable track alone does not implement them.

## The page is the experience

- Center the prose within a readable measure, initially 62–72 characters per line.
  Let users choose text size, line spacing and font family through a small display menu.
- Start Read mode at approximately 20px text and 1.6 line height. Preserve an
  efficient writing size in Write mode. Font candidates must be compared in the
  actual native renderer with punctuation, italics, dialogue and long chapters.
- Use a warm reading face with a restrained UI sans. Prototype with installed
  fonts; bundle a licensed, tested font only after selection. Avoid runtime downloads.
- Reduce the heading rules to cases where the document actually contains a rule.
  Formatting controls appear on intentional text selection or a header action.
- Paint the current synthesized passage with a soft mint background and an optional
  small margin marker. The highlight should survive line wraps without moving text.
- Retain passage-level highlighting until genuine alignment supports finer timing.
  Never simulate a word sweep by distributing audio time across characters.

**Follow reading** keeps the active passage in the middle portion of the viewport.
Scroll only when it leaves a safe region. Manual scrolling suspends following and
reveals **Return to reading**. Pause clears active narration paint as it does today;
a quieter location marker can retain orientation. Selection and caret remain separate.

Editing pauses and invalidates the old revision before new highlighting can appear.
Show **Text changed — save to continue**, with an explicit save-and-restart action.
Preserve position only through a validated revision mapping; otherwise show the
chosen restart location. Do not silently guess a matching sentence.

## Voice and character design

Replace the right inspector with a 320–360px Cast sidebar when requested. Only one
right sidebar is open at a time. Returning to Inspect restores its previous state.

### Narrator

Show a compact profile row with a stable monogram, name, short delivery description,
and **Preview / Change** actions. A small type label distinguishes **Designed** from
**Reference** voices. A saved prompt describes a performance; it is not a guarantee
of the stable identity that a qualified reference may provide.

The voice picker offers searchable rows with the same audition passage. Auditions
must have bounded duration, clear cancellation, and a defined relationship with
current playback. With the current single-concurrency provider, pause listening
explicitly for an audition and require Resume afterward. Do not hide GPU contention.

### Cast

Each character row shows name, assigned voice, and assignment count. Expanded rows
offer Preview, Change voice and Find passages. Existing entity IDs can be linked
later; manually creating a character must work without the graph backend.

Select text -> **Assign speaker** -> choose/create character -> choose voice ->
preview the exact affected passage -> **Apply**. Mark assigned passages in the
margin only while casting is visible. Keep source text unchanged.

Current implementation assigns the whole planned passage. The first redesigned
version must label that scope and reveal its exact source range before Apply.
Arbitrary selection requires the planner to split on validated source boundaries,
preserve source-to-spoken mappings, and reject unsupported partial mappings.
Cross-chapter and overlapping assignments need explicit validation.

Applying a cast change cancels old pending work, creates a revised voice session,
and invalidates only affected synthesis identities. The UI handles the existing
save/reload sequence. Resume at the affected passage after validation; indicate
any position change. Narrator replacement preserves explicit character assignments.

### Create a voice

Use a focused dialog with separate Designed and Reference paths. Designed voice:
name, description, audition, save. Reference voice: local recording, editable exact
transcript, ownership/consent note, validation, audition, save. Preserve immutable
voice revisions and local reference storage. Direction controls expose only provider
capabilities we have verified; do not invent pitch/emotion sliders with no reliable
mapping to the runtime.

Automatic speaker attribution remains a later, reviewable suggestion feature.
Quotation marks alone must never silently determine a character or change narration.

## Visual language

Working direction: **ink, paper, mint**. Phoenix's graph can retain its expressive
colors; reading chrome uses color sparingly so the current action is clear.

| Token | Initial design target |
|---|---|
| Canvas | Warm near-black `#151817` |
| Page | `#1B1E1C`, separated mainly by spacing |
| Raised panel | `#222824` |
| Primary text | Warm white `#E8E6DF` |
| Secondary text | `#A9B3AD` |
| Accent | Mint `#72D6B3` for selected action/focus |
| Highlight | Low-opacity mint fill; final opacity tested with all text states |
| Spacing | 4, 8, 12, 16, 24, 32, 48px |
| Corners | 6px inputs, 10px panels, circular primary transport |
| Icons | One consistent native/vector set, 18–20px, accessible labels |
| Motion | 120–180ms hover/panel transitions; reduced-motion alternative |

These are starting values, not verified contrast claims. Measure the rendered
combinations, including disabled, selected and highlight states. Avoid neon fills,
constant glow, decorative waveforms, and persistent motion while reading.

Regular controls should have at least 36px hit regions; transport controls 44px.
Use explicit keyboard focus and meaningful accessible names. Space controls audio
only outside text inputs/editor composition; use a dedicated, conflict-checked
playback shortcut while editing. Escape closes a popover and restores prior focus.
The native preview's sparse accessibility exposure needs an audit and implementation
work; keyboard operation alone does not establish screen-reader accessibility.

## Translate system state into useful language

| Runtime situation | User-facing presentation |
|---|---|
| No session | Listen to this document |
| Verifying model bundle | Preparing local voice… / Cancel |
| Waiting for requested audio | Preparing this passage… |
| Playback interrupted by generation | Preparing the next passage… / Pause |
| Paused | Paused; retain chapter and location |
| Completed | Chapter/book complete; replay or next available chapter |
| Edited document | Text changed — save to continue |
| Missing voice asset | This voice is unavailable / Choose voice / Details |
| Generation failure | Could not prepare this passage / Retry / Details |

Keep a small **Preview** label while Breeze qualification is incomplete. Put model
hashes, checkpoint revisions, buffering statistics and raw errors in **Playback
details**, with an explicit copy action. Do not imply a missing backend is healthy.
Removing the large debug panel must not erase actionable failure information.

## Responsive and efficient native implementation

Budget by available logical width, including sidebars and display scaling. Starting
breakpoints: above 1440px permit both sidebars; 1100–1439px allow one; below1100px
use overlay panels and a compact dock. Validate 960x640 through4K at100/150/200%.
Collapse labels into named icon controls before allowing transport rows to wrap
unpredictably. Persist sidebar sizes and reading preferences independently.

Keep controller ownership and bounded provider/cache queues. Introduce a typed
Reader view state; do not derive behavior by parsing status strings. Update only
changed presentation fields. Existing100ms status polling can be coalesced into
small view updates; it should not invalidate the whole editor on every tick.

Cache formatted labels and chapter summaries. Reuse compact source-range buffers;
project highlights on passage changes. Locate a passage's layout bounds for follow
scrolling without rescanning the chapter. Virtualize large chapter/cast lists and
avoid filesystem access, model hashing or audio copies in render callbacks.

Proposed modules: `reader/transport.rs`, `reader/presentation.rs`,
`reader/outline.rs`, `reader/cast_panel.rs`, `reader/voice_picker.rs`,
`reader/diagnostics.rs`. Retain `worker.rs` as orchestration and the existing native
provider/session/cache boundaries. Extend `highlight.rs` and the editor viewport
API deliberately. Split by responsibility, aiming below800lines per file.

## Delivery sequence and gates

1. **Approve the visual direction.** Produce reviewable native-layout mockups for
   desktop Read, compact Read, Cast, preparation, and failure states. Compare at
   real display scale. Resolve typography and dock density before implementation.
2. **Replace the debug panel.** Ship compact transport, contextual sidebar, native
   icons, semantic status copy, diagnostics disclosure and a single Listen flow.
   Use existing playback/chapter/bookmark capabilities. Do not show dummy controls.
3. **Make reading comfortable.** Add Read arrangement, display settings, follow
   scrolling and Return to reading. Verify long-document layout and edit invalidation.
4. **Make casting direct.** Redesign profile selection and passage assignment first;
   then add audition orchestration, reference enrollment and validated selection
   splitting. Qualify interruption and position behavior for each operation.
5. **Finish transport capability.** Add exact seek, ±15 seconds, multiple bookmarks
   and pitch-preserving speed with explicit contracts and meaningful tests.
6. **Polish and qualify.** Accessibility, DPI, empty/error states, keyboard navigation,
   motion, interaction performance, persistence and a full chapter through the exact
   packaged candidate. Graph/AI acceptance and release freeze remain separate gates.

The next implementation milestone should be step2: an excellent default listening
screen with the capabilities already proven. That gives a large visual improvement
without waiting for every new Reader feature.

## Acceptance criteria

- Ordinary listening shows no protocol enum names, raw hashes, repeated format-load
  buttons or permanent setup form. Playback remains understandable during failures.
- At1280x800 with sidebar closed, the reader gets at least70% of the usable vertical
  area for the document; the default dock stays at or below88logical pixels.
- Play/pause is immediately discoverable. Resume requires one action. Voice picker
  is one action from the dock; assigning a prepared character is at most three
  actions after selecting a supported passage.
- User can read for30minutes without toolbar occlusion, forced scroll fighting,
  changing text layout when highlighting, or losing their selected text/caret.
- Screen-reader labels, focus order, keyboard commands and measured contrast pass
  an explicit accessibility review. No color-only speaker or state identification.
- On the same machine and fixture, capture baseline and candidate frame times,
  input latency, memory and audio counters. Target p95 visible updates within16.7ms
  on60Hz and acknowledge input within100ms; investigate any new underruns or memory
  growth. These are proposed targets, not measurements already achieved.
- Restart restores document, voice/cast selection, listening location and reading
  preferences. Partial, cancelled or failed generation never publishes completed
  cache entries. Stale revisions never paint or play as the active document.
- The user completes Listen, assign a character, pause, bookmark and resume after
  restart without being coached through runtime terminology.

Evidence used: user screenshot; current shell `reader/mod.rs`, `studio.rs`,
`highlight.rs`, `view.rs`; reader-voices-editor-acceptance.md. All behavior beyond
the verified implementation described there is proposed work.
