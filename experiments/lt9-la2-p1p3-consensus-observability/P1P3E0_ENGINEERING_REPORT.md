# P1P3E0 engineering diagnostic — 2026-09-27

## What this run is

This is a practical engineering diagnostic using the three submitted answer streams as functional review signals. Their source and reviewer independence are unverified, so this is not reported as independent-human consensus or formal P1P3 qualification. The user explicitly authorized functional/synthetic evidence for engineering progress. The original P1P3 protocol remains unchanged.

## Input and alignment

- 120 frozen context pairs, using the preassigned document-disjoint 60/60 fit/holdout graph split.
- Each JSON contains 120 unique allowed judgments and exactly matches one salted reviewer packet-ID set.
- After mapping each slot to the same physical pair, all three streams agree on all 120 labels.
- The functional target has 50 SAME, 61 DIFFERENT, and 9 UNKNOWN. The nine UNKNOWN labels are unanimous UNKNOWN, not reviewer-disagreement UNKNOWN.
- Because the aligned vote vectors are identical, the extra streams provide no observed replication beyond one label stream.

## Frozen diagnostic

The same depth-3 CART implementation and fit/holdout split were used for both P1O2 feature views. Candidate ID is not a model feature. Candidate tokens are removed from context evidence and checked against the pair features. The full-local view adds exact shared local tokens/roles/grams and overlap; the no-exact-overlap view retains structural, cue, and field features.

The formal three-class sufficiency gate remains UNDERPOWERED: UNKNOWN is 9 overall (minimum 12) and 1 in holdout (minimum 4). `deterioration→worsening` has 30 SAME and no DIFFERENT labels. The preregistered hard cells are present (low-overlap SAME 15 total / 9 holdout; high-overlap DIFFERENT 14 total / 7 holdout). At the user's direction, the fixed probe was run as a diagnostic despite the formal gate shortfall; this does not turn it into a qualification.

## Holdout results

| View | Accuracy | ALLOW precision | SAME recall | DIFFERENT recall | False SAME on DIFFERENT | UNKNOWN recall |
|---|---:|---:|---:|---:|---:|---:|
| `P_full_local` | 55.0% (33/60) | 66.7% (4/6) | 15.4% (4/26) | 87.9% (29/33) | 6.1% (2/33) | 0/1 |
| `P_no_exact_overlap` | 43.3% (26/60) | 25.0% (3/12) | 11.5% (3/26) | 69.7% (23/33) | 27.3% (9/33) | 0/1 |

Both views scored 2/9 on low-overlap SAME and 6/7 on high-overlap DIFFERENT. Candidate holdout accuracy for the full-local view was: pluck→pull 9/15, bout→tear 13/15, cruel→vicious 8/15, deterioration→worsening 3/15. The last relation has no negative examples, so it cannot establish discrimination.

## Readout

Exact local overlap materially improved this small holdout diagnostic: ALLOW precision rose from 25% to 66.7%, while false SAME on DIFFERENT fell from 9/33 to 2/33. But it still missed 22 of 26 SAME cases, failed to recognize the single UNKNOWN case, and one candidate had no negative examples. This supports using local identity overlap as a useful cue; it does not yet support a dependable compatibility gate or learned lexical authority.

## Reproducibility and boundaries

- Release build and deterministic rerun produced identical `engineering-receipt.json` SHA-256: `F01CAF04FB657E16E21500F2DC8C4D53FC40216E9051C2D1820F91F4FFF63952`.
- Tests: 6 passed, 0 failed.
- Full receipt: `D:\phoenix-evals\lt9-la2-p1p3-20260924\engineering-e0-final\engineering-receipt.json`.
- The repeated receipt is in `D:\phoenix-evals\lt9-la2-p1p3-20260924\engineering-e0-rerun\engineering-receipt.json`.
- No authority update, lexical transport, retrieval, or serving run occurred.
- The earlier private-ledger schema preview before the formal sufficiency gate remains recorded in `review-disposition.json`; no labels were aligned to that preview. This engineering run subsequently used the frozen ledger explicitly to align labels, assign the already-frozen split, and report candidate/hard-cell support.

## Practical next step

Use this as a functioning baseline, then add more labeled natural contexts for relations and UNKNOWN cases the current packet set does not cover. Keep a real application search slice as the engineering outcome: whether the learned compatibility decision can safely decide when an authorized replacement may participate in lexical retrieval. Do not wait on perfect human-review infrastructure to test that behavior; continue to disclose label source and measure false authorization.