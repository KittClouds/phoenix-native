# Short-dialogue whitespace mitigation

The original Chapter 1 paragraph 77 failure was reproduced twice with separate
caches. Both fresh PCM hashes exactly match the user-confirmed bad output from
the original chapter. This establishes repeatability on the pinned native worker,
model, seed 42 and request, rather than attributing the error to cache corruption.

The controlled matrix changes one input factor at a time where possible:
trailing whitespace, quotes, seed, or contiguous neighboring source context.
Evidence: `D:\phoenix-tts\breeze-native-20260906\dialogue-controlled-02`.
All twelve cases ended normally. Mechanical EOS is not semantic fidelity.

| Case | Duration | Diagnostic recognition |
| --- | --- | --- |
| Exact original, repeated twice | 3.84 s each | Same extra words as original |
| Trim only paragraph-edge whitespace | 1.36 s | Only source words |
| ASCII quotes, no trailing whitespace | 2.32 s | Only source words |
| No quotes, no trailing whitespace | 1.36 s | Only source words |
| Adjacent source context | 16.32–47.20 s | Target phrase without the original extra speech |
| Original text, seeds 43/44 | 1.60/1.44 s | Only source words |

These are controlled observations, not a proof of a universal model fix. ASR is
diagnostic; the trimmed audio is provided for listening. Do not select a new seed
per failing input, cut generated audio, or silently strip punctuation.

## Implemented candidate

`phoenix_reader_session::plan_plain_chapter` creates a revision-bound single
plain-text chapter plan. It copies each nonempty line's text unchanged and records
leading/trailing Unicode whitespace and blank lines as explicit Omit mapping
runs under `unicode-edge-whitespace-omit/v1`. Internal spaces and quotes remain
unchanged. All source bytes are accounted for, and spoken text stays contiguous
across the segment table. Each segment is still requested independently.

This function does not discover chapter headings, handle general Markdown,
produce word/sentence alignment, normalize loudness, or activate shell playback.
Paragraph ordinal is used as the sentence field for these paragraph-only plans;
it is not an alignment claim. The existing Markdown planner is unchanged.

Tests cover indented prose, CRLF, blank lines, Unicode whitespace, curly quotes,
accented text, internal spaces, mapping validation, and content-sensitive plan IDs.
The provider remains source-exact for the *planned spoken text*. Cache identity
already includes that text, and the plan ID includes the versioned omission rule.

## Requalification

The chapter harness retains its original behavior by default. The explicit
`PHOENIX_QUALIFY_TRIM_EDGES=1` qualification mode selects the new plan, emits its
complete spec to `plan.json`, and synthesizes `spec.spoken` ranges. Runtime/model
and generation settings remain unchanged. Fresh result root:
`D:\phoenix-tts\breeze-native-20260906\shortrun-chapter1-trim-02`.

No original evidence is repaired or overwritten. This mitigation stays candidate
until the new chapter's transcript and listening gates are adjudicated. Normal
cache completion still does not imply that the model's content is qualified.

## Rerun result

All 102 paragraphs rendered, producing 1018.88 seconds (16:58.88) of audio.
The full source-map audit accounts for 16247 bytes: 15822 copied and 425 omitted
whitespace bytes. No words or punctuation were omitted. There were no silent
segments or clipped samples.

Paragraph 77 PCM exactly matches the isolated trimmed clip that the user
confirmed clean. Thus this targeted regression is corrected in the chapter
context. A suspected added word at paragraph 7 was reviewed by the user, who
reported that it ended perfectly; record that as an ASR false positive.

Warm first PCM p50/p95: 297/331 ms. Warm p95 RTF: 0.8262, failing the 0.8 gate.
The diagnostic recognizer found 68 edits / 2744 normalized tokens (2.48%), versus
56 / 2744 (2.04%) in the original run. This does not establish a true TTS WER
regression: spelling/number differences and recognition errors remain mixed
with possible synthesis errors. Other substitutions and pronunciation cases
remain open for listening adjudication. Do not promote this as a chapter-wide
quality pass or universal hallucination fix.

Next work is targeted review of the remaining substantive word disagreements
and measured throughput improvement, retaining both full chapter runs. The
original rejected chapter and regression evidence remain unchanged.
