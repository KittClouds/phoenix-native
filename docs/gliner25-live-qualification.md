# GLiNER2.5 live pipeline qualification

## Scope and frozen comparison

Promote the qualified FP32 entity decoder into the existing Dynamic NER model
slot, preserve the structural/NLI/graph contracts, and run `docs/shortrun.md`
through the native application's Run Pipeline action. Preserve the preceding
qualified producer for rollback. No gold-set accuracy or final release claim is
implied by reference parity.

The first comparison preserves the existing Dynamic NER router's maximum of 20
selected model windows. It is not exhaustive neural extraction of the novel.
GLiNER2.5 further divides a selected long window into chapter-bounded 384-word
windows with 64-word overlap. Threshold is 0.5, flat overlap, FP32 CPU. The
Ryzen 7 5800X3D qualification selected four intra-op threads after an eight-thread
baseline. Larger windows and INT8 previously changed outputs and remain
excluded. This cut exposes entity extraction, not the experimental attribute,
classification, joint relation or record surfaces.

## Runtime boundary

- `rust-native/phoenix-gliner25`: production copy of the entity/long-context
  decoder from `experiments/phoenix-gliner25-eval`, pinned upstream revision
  `a639bad1ee744a7884deea2bd01512fddd886b8d`; separate ORT rc.13 process.
- `rust-native/phoenix-gliner25-contract`: bounded length-prefixed requests and
  replies, sequential request IDs, exact source hashes, strict UTF-8 ranges and
  finite confidences. Sealed model/worker/runtime hashes enter model identity.
- Existing analysis bridge retains ORT rc.9 for ModernBERT and the v2.0 rollback
  route. It supervises the new process, validates its identity handshake and
  replies, and uses a Windows kill-on-close job for parent cancellation/crash.
- Any model failure aborts analysis before publication. A failed worker is not
  silently replaced by a deterministic or old-model result.

## Evidence collected

- Real worker: all 12 frozen Python reference cases have identical entity
  text/labels/byte ranges. All source slices validate. Repeated sequence is
  rejected. Receipt: `D:/phoenix-gliner25-live-20260914/worker-parity.json`.
- Six decoder/window unit tests passed via the C: test junction.
- Two IPC/range contract tests passed. All 56 app-core tests passed, including
  the regression preventing reused analysis from replaying original timings.
- Shortrun fixture is seeded with the production workspace/document codec:
  161,490 bytes, document revision 1, BLAKE3
  `0da62ebf93c97df40f71fba1c37dd1788e8cf73ad5a5e688e57f1ab3a54e6c3a`.
  Workspace: `D:/phoenix-gliner25-live-20260914/shortrun/workspace.json`.
- Baseline bundle: `D:/phoenix-gliner25-live-20260914/bundle/gliner25.json`.
- Selected bundle: `D:/phoenix-gliner25-live-20260914/bundle-threads4/gliner25.json`.
- Existing Reader workspace and its saved voices remain separate.

## Timing semantics

`PHOENIX_PIPELINE_TIMING` uses one process-monotonic clock for UI request,
publication delivery, renderer projection and first presentation submission.
Projection duration includes installing the new scene and product index.
The frame counter reports CPU encode/submit duration. Presentation submission
is not physical display latency or a GPU timestamp.

Existing Atlas receipts retain extraction, chunker, NLI, compiler and publisher
timings, counts, model identity and reuse disposition. `PHOENIX_NER_STAGES`
records coarse internal routing/graph/scoring stages. Worker-window receipts
record inference duration and returned span counts. Nested timings must not be
summed as independent costs. Reused runs must not be called fresh inference.

## Live application results, September 14–15

The release shell opened the seeded Shortrun workspace, warmed the supervised
producer, and generated the graph using the actual UI. Generation 5 was started
from the graph toolbar with the viewport already visible.

| Measurement | Eight-thread baseline G3 | Four-thread G5 |
| --- | ---: | ---: |
| Kernel pipeline | 6,116.178 ms | 4,769.810 ms |
| Analysis, including boundary overhead | 5,955.241 ms | 4,572.324 ms |
| Structural chunker, nested in analysis | 87.026 ms | 85.176 ms |
| Dynamic NER, nested in analysis | 4,040.603 ms | 2,739.081 ms |
| NLI adjudication, nested in analysis | 1,717.412 ms | 1,686.859 ms |
| Scene compiler | 9.539 ms | 10.789 ms |
| Publication | 90.459 ms | 121.678 ms |

These are individual in-app runs, not a statistical end-to-end benchmark.
Model warmup is excluded. G5 UI request to publication delivery was 4,776.341 ms;
request to first graph presentation submission was 5,142.012 ms. Renderer
projection took 11.397 ms; first-frame CPU encode/submit took 0.547 ms. Baseline
G3 first presentation followed manual tab navigation, so it is not a comparable
render-latency baseline. Substage totals are nested, not additive.

The same graph census appeared in both app runs: 6,941 nodes, 9,736 edges,
86 entities, 1,115 mentions, 398 chunks, 2,361 sentences, and 59 NLI candidate
adjudications. Review counts were 6,013 accepted and 3,723 proposed edges.
Native semantic candidate counts: temporal 76, causal 7, memory state 41,
events 232, contextual cooccurrences 1,036. These are graph/native producer
outputs, not claims that GLiNER2.5 supplied every relation family.
Editor projection reported 1,113 applied highlights and zero unmapped ranges.

### Controlled thread sweep

`thread-sweep/results.json` records six process cohorts in order 8, 4, 2, 1,
4, 8, with three complete routed Shortrun runs each. Each cohort's first run is
excluded from its warm median. Cohort warm medians in milliseconds:
3,527.946; 2,267.328; 3,168.038; 5,529.268; 2,234.906; 3,644.447.
Four threads improved the average cohort median by approximately 37% against
eight. All 18 runs had the same BLAKE3 over serialized entities, mentions,
candidates and candidate evidence:
`10b39e81c6029ef2893bbaabeb3ba4d01be7a32436d52efe07e3178ca5e9d59d`.
This setting is qualified on this CPU/workload, not every supported computer.

### Reuse, failure and restoration

- G6 in-app rerun: pipeline 192.079 ms, reuse lookup 2.100 ms, compiler 9.904 ms,
  publication 162.417 ms. No new worker inference occurred. Receipt identifies
  `ReusedResident`; model timings are `NotRun`. Earlier G4 incorrectly labeled
  reuse as `Computed` and retained original model times; that receipt remains
  historical evidence of the bug, not fresh inference performance.
- The corrected code carries reuse disposition directly from the reuse branch
  into the durable receipt. Original resource counts remain available without
  claiming their model stages ran again.
- `failure-20260915/receipt.json`: terminated the isolated worker during actual
  Shortrun inference. Bridge exited 1, created no analysis/structural/coordinator
  outputs, and left the live `current.pspm` SHA-256 unchanged. Error explicitly
  refused partial publication. The live app's worker was not terminated.
- The saved G4 graph was restored in the release app with matching counts.
- `rollback-smoke-final.json`: preserved v2.0 producer warmed its existing roots
  and answered READY/BYE successfully. This is a startup smoke, not a second
  full graph qualification. The first harness receipt misparsed the existing
  `PHOENIX_CONTROL` prefix; the corrected harness passed.

Raw validated Atlas receipts and text inspections are under
`D:/phoenix-gliner25-live-20260914/shortrun/atlas-run-authority-v1`.
`inspect_atlas_receipt` uses the same bounded, hash-checked mmap reader as the
app and performs no workspace writes.

## Follow-on presentation work

The measurement revealed a roughly 350 ms gap between publication delivery and
renderer projection. Editor highlighting preceded the renderer wake. The next
shell sends the graph synchronization request first and measures editor
projection separately; this must be verified in the rebuilt app before claiming
a latency improvement.

UI fixes include a correctly formatted producer PID, visible model name,
wrapping pipeline controls, explicit fresh/reused status, and a graph/sidebar
polish pass that preserves the geometry, color authority and virtualized rows.

### Polished-shell verification

Release build passed. Running executable SHA-256:
`039208CB9F491E73F9762E65A34130E9BA4779B6EBC993ECA080561CE0EE63B6`.
Launch receipt: `shortrun/polished-launch.json`; stage log:
`shortrun/polished-stderr.log`. The app restored G6 with matching counts.
Live UI verified graph controls with the inspector both open and closed; at
narrow width the action group wraps and Build graph remains reachable.

G7 reused run: kernel pipeline 115.262 ms, compiler 8.941 ms, publisher
87.152 ms, reuse lookup 1.959 ms. UI request to first presentation submission
was 140.955 ms. Publication delivery to first submission was 19.508 ms,
compared with 397.976 ms in G6 before moving the renderer wake. Editor
highlight projection took 386.075 ms and completed after the first frame;
1,113 highlights applied with zero unmapped ranges. This is one observed
comparison, not a percentile guarantee. Counts remained 6,941 nodes and 9,736
edges. The UI thread still spends that time on highlighting; moving the graph
wake removes the presentation dependency rather than optimizing highlighting.

## Remaining qualification limits

No novel gold-set precision/recall claim, exhaustive GLiNER neural coverage,
all-feature GLiNER promotion, GPU inference qualification, or freeze/release
certification is implied. Next performance candidates are the remaining neural
inference and publication/editor costs; reduce them only while preserving
source ranges, output/coverage contracts and real-app responsiveness.
