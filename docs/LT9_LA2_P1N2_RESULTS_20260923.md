# LT9-LA2 P1N2: candidate-conditioned local observability results

Date: 2026-09-23
Status: **controlled diagnostic complete; no natural router qualified**

## Result

P1N2 finds a sharp split in this controlled assay. The named-family classifier is weak and often degenerate, while pairwise context compatibility is perfectly separable on the held-out constructed variants. The latter is evidence that “same context or different context?” is easier than assigning a named family **in this stimulus construction**. It is not evidence that natural text has solved contextual lexical ownership.

The safe interpretation is:

> Exact noncandidate local context features can encode the constructed frame distinctions and unknown cases. This assay does not show that they identify naturally occurring lexical senses.

## Frozen execution

The run used 3 candidate pairs, 12 base groups per pair, and 8 controlled variants per group: **288 stimuli / 36 base groups**. Groups 0–7 fit each depth-3 tree; groups 8–11 were held out as indivisible groups. There were 32 held-out stimuli and 112 held-out within-group context pairs per candidate. The unknown conditions were cue removal and conflicting local cues.

The local feature firewall recorded **zero candidate-token feature keys** and **zero candidate-ID feature keys**. Candidate identity selected a separate model only. No corpus text, natural labels, qrels, outcome receipts, or Webis-Touche2020 were read. No authority was updated and no retrieval or ranking ran.

The release executable was copied from the D: target directory to the C: test location and run twice. A final implementation audit found that one local n-gram could cross a title/body boundary despite the frozen within-field contract. No fitted tree selected n-gram features, but the extractor was corrected and a regression test was added before sealing. The protocol, stimuli, labels, split, model, and metrics did not change. Two final executions of the corrected binary were byte-identical: 8,991 bytes, SHA-256 `fc3533a22514ce02bbc75c12b38490edec125ea190f017df17239459e96a4f9b`; all 9 unit tests passed. The earlier intermediate outputs are superseded and are not the committed result.

## Held-out results

| Candidate pair | Current family accuracy | Context-only family accuracy | Local frame accuracy | Local topic invariance | Correct A/B sense transition | Unknown calibration | Pair compatibility accuracy |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `bank_to_water` | 0.281 | 0.219 | 0.625 | 1.000 | 0/4 groups | 1.000 | 1.000 |
| `car_to_vehicle` | 0.219 | 0.188 | 0.813 | 0.625 | 0/4 groups | 1.000 | 1.000 |
| `insurance_to_coverage` | 0.156 | 0.125 | 0.719 | 1.000 | 1/4 groups | 1.000 | 1.000 |

Topic invariance compares each base prediction directly with both the topic-swap and topic-amplification prediction, for both frames: 16 comparisons per candidate. The high local invariance is not enough by itself: bank and insurance predictions were largely constant on one frame, so the correct A/B transition was missed in most or all groups. The explicit named-frame probe therefore did **not** demonstrate reliable sense classification.

The pairwise compatibility tree reached 112/112 correct pairs per candidate, including all three target classes (SAME, DIFFERENT, UNKNOWN) in every candidate. Its selected splits were local-context overlap and role-count features. This is a strong result for the assay’s controlled construction, where same-frame variants deliberately retain overlapping cue identities and different-frame variants use separate cue inventories. That construction makes the signal observable by design, so perfect performance must not be generalized to natural contexts.

The family-route controls were both inaccurate and sensitive to topic perturbations. Current-route topic stability was 0.000, 0.313, and 0.250 for the three candidates; context-only stability was 0.000, 0.063, and 0.000. Removing candidate self-votes did not repair the family classifier.

## Interpretation and boundary

P1N2 supports a **diagnostic preference** for pairwise context compatibility over named global-family classification. It also shows that the local feature extractor can preserve the distinction between topic perturbation, local cue change, and explicit unknown conditions when the controlled cue patterns are available.

It does not qualify a natural sense observer or compatibility gate. In particular, the pairwise result depends on constructed cue overlap, and the named-frame probe’s high invariance is partly degenerate. This result does not unblock LA2-B, alter the phenotype router, or authorize lexical transport. Natural, independently judged contexts are still needed before claiming relation-sense observability.

## Artifacts

- Protocol: `docs/LT9_LA2_P1N2_LOCAL_SENSE_OBSERVABILITY_20260923.md`
- Pre-run manifest: `experiments/lt9-la2-p1n2/pre-run-manifest-20260923.json`
- Source: `apps/phoenix-memory-lock/src/bin/lt9_la2p1n2.rs`, `lt9_la2p1n2_data.rs`, and `lt9_la2p1n2_tree.rs`
- Result: `experiments/lt9-la2-p1n2/p1n2-result-20260923.json`
- Seal: `experiments/lt9-la2-p1n2/artifact-seal-20260923.json`
