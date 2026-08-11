# QPS V3 promotion loop

QPS V3 is learned final ordering over the frozen V2 retrieval substrate:

```text
V2 query planning
  -> exact/fuzzy expansion groups
  -> posting traversal
  -> bounded candidate pool (maximum 160)
  -> primitive RankEvidenceV3
  -> constitutional RelevanceTier
  -> immutable linear V3 ranker
  -> stable top-k
```

V2's final score is retained in diagnostics and as the explicit rollback engine. It is not a V3 model feature and cannot affect V3 order.

## Current verified boundary (2026-08-08)

| Phase | State | Evidence |
|---|---|---|
| 1 | verified | frozen mixed and LongMemEval baseline receipt |
| 2 | verified | 30 bounded primitive features; no `baseline_score` or `candidate_strength` |
| 3 | verified | hard constitutional tiers precede learned score |
| 4 | verified | keyed, append-only ledger contract and frozen-fixture receipt |
| 5 | shadow corpus only | independent mined candidates exist, but structural-only agent curation was revoked and cannot satisfy authoritative-review gates |
| 6 | implemented, promotion gated | deterministic grouped split with leakage and per-class checks |
| 7 | implemented, promotion gated | deterministic monotonic linear training remains available for a qualified split |
| 8 | not verified | best non-revoked diagnostic remains below MRR, graded NDCG, and worst-shape targets; V2 remains active |
| 9 | implementation verified | exact model-bound kernel and E2E performance passed for a diagnostic model, not a promotable one |
| 10 | not verified | fallback is verified; revoked review provenance and quality failure block reconciliation and atomic promotion |
| 11 | verified decision, tree deferred | eligibility policy is verified; tree is disabled because linear promotion and 10,000-pair prerequisites are unmet |

The authoritative locks are `phoenix-native/memory-lock/qps-v3-phase*-2026-08-04.json`. A readiness lock is not execution evidence for a later phase.

### Revoked review provenance

`qps-v3-agent-curation-revocation-2026-08-08.json` revokes every artifact whose
authority depends on the former `curate-benchmark` method. That command checked
only packet structure and feature/identity alignment, then emitted
`positive_preferred` for every mined pair. It did not compare document meaning
and therefore could not truthfully upgrade mined negatives to
`CuratedRegressionCase`.

The command has been removed. Benchmark generation now stops at a review packet.
Only explicit decisions supplied to `apply-reviews` may create authoritative
review lineage. Dataset qrels establish that a positive is relevant; an
unjudged same-tier hard negative remains non-authoritative until its relative
preference is actually reviewed.

### Semantic review surface

`prepare-review-batch` now emits
`phoenix.qps.semantic-review-batch/v2`. It deterministically balances the 11
technical failure classes and carries the evidence a reviewer actually needs:
query, candidate text, V2 positions, LoCoMo reference answer, official session
time label, and any image caption/query. Reference answers, timestamps, and
multimodal captions are reviewer-only context; none are added to
`RankEvidenceV3` or the serving model.

The completed authorized review cut inspected 220 pairs, accepted 170, reversed
14 mislabeled preferences, and abstained on 50 ambiguous or inconsistent pairs.
Phase 4 passes for this append-only lineage with zero duplicates,
contradictions, or release leakage. Phase 5 remains false at 170 active training
judgments, 78 unique queries, and 113 independent sources, as required;
automatically mined rows do not count toward readiness.

### Expanded human review

The next queue contains 1,100 new pairs and excludes all 220 identities from the
completed cut:

```text
C:\benchmarks\phoenix-qps-v3-20260804\semantic-review-batch-v2-expanded-new-1100.json
```

Open `docs/qps-v3-semantic-review.html` in a browser and load that JSON file.
The offline page stores progress locally, supports queue/class filters and
keyboard decisions, exports resumable work backups, and emits timestamped,
cumulative checkpoints using the exact
`phoenix.qps.relevance-review-decisions/v1` envelope accepted by
`apply-reviews`. Private notes remain only in work backups and never enter the
training ledger. Work backups are bound to the exact source and excluded-batch
hashes, so they cannot be imported into a different queue accidentally.

Apply any exported checkpoint with the runner below. It resolves the latest
qualified ledger from its atomic pointer, skips unchanged cumulative decisions,
appends genuine revisions with lineage, reruns Phase 4 and Phase 5, and writes a
compact readiness receipt. When Phase 5 passes it automatically continues
through split, linear training, and Phase 8 quality qualification:

```powershell
& 'C:\code land\clean-rust\phoenix-native\scripts\Invoke-QpsV3ReviewCheckpoint.ps1' `
  -Decisions C:\path\to\qps-v3-review-checkpoint-50-2026-08-08.json
```

The current queue audit is
`semantic-review-batch-v2-expanded-new-1100-audit.json`: all 11 classes occur
exactly five times in the first 55 items, no adjacent items share a class, and
985 of 1,100 pairs are V2 disagreements. Rank displacement is reported only as
an impact proxy; the audit makes no unsupported semantic-difficulty claim.
Future batches interleave high-impact and close-rank cases within the existing
class round-robin. The active 1,100-item batch remains byte-identical so current
browser progress is preserved.

The grouped follow-on cut is recorded in
`QPS_V3_GROUPED_CURATION_CUT_2026-08-08.md`. It groups three to five existing
pair identities by query for efficient agent-authorized semantic review while
retaining independent per-challenger decisions and fail-closed authority.

## Evidence intake

The live capture/review system must serialize a canonical `phoenix.qps.relevance-ledger/v3` (`RelevanceLedgerV3`). Every row contains complete candidate-pool positions, primitive feature vectors, constitutional tiers, source grouping, model identities, generation, source/reason/confidence/weight, and contradiction or supersession lineage.

Private query and document-version identities must already be derived with the workspace key. The raw 32-byte key must never appear in the ledger. The qualifier rejects LongMemEval release-cohort intersections and requires any constitutional-suite row to remain a frozen holdout.

```powershell
$bin = 'C:\phoenix-bin\qps-v3-phase1-20260804\phoenix-memory-lock.exe'
$manifest = 'C:\code land\clean-rust\phoenix-native\memory-lock\longmemeval-cleaned-v1.json'
$phase3 = 'C:\benchmarks\phoenix-qps-v3-20260804\phase3-constitutional-tiers-e5a659d.json'
$key = 'C:\benchmarks\phoenix-qps-v3-20260804\workspace-identity-v3.key'

& $bin qps-v3-qualify-ledger --manifest $manifest `
  --ledger C:\path\to\relevance-ledger-v3.json `
  --phase-3 $phase3 --workspace-key $key `
  --output C:\path\to\phase4-qualified-ledger.json

& $bin qps-v3-audit-corpus --manifest $manifest `
  --phase-4 C:\path\to\phase4-qualified-ledger.json `
  --phase-3 $phase3 --workspace-key $key `
  --output C:\path\to\phase5-corpus.json
```

Phase 5 remains false until both shadow and promotion volume targets, failure-class review coverage, provenance completeness, duplicate/contradiction gates, and hard-negative grouping gates pass.

`real_user_correction` is operational judgment provenance, not a retrieval failure class. The Phase 5 v2 bootstrap contract therefore requires 100 authoritative reviews for each of the 11 technical failure classes and reports explicit user corrections separately. Initial bootstrap may train from independently sourced curator-confirmed evidence with the real-user count truthfully at zero; post-bootstrap operational retraining requires at least 100 explicit user corrections. Agent-curated evidence must retain its user-authorization attestation and must never be relabeled as an explicit user correction.

## Split and train

```powershell
& $bin qps-v3-split-ledger --manifest $manifest `
  --phase-5 C:\path\to\phase5-corpus.json `
  --phase-4 C:\path\to\phase4-qualified-ledger.json `
  --phase-3 $phase3 --output C:\path\to\phase6-split.json

& $bin qps-v3-train-linear --manifest $manifest `
  --phase-6 C:\path\to\phase6-split.json `
  --phase-4 C:\path\to\phase4-qualified-ledger.json `
  --phase-3 $phase3 --output C:\path\to\qps-v3-linear-model.json
```

Identical inputs produce byte-identical model artifacts. Runtime training is forbidden.

## Quality input

The independent graded suite uses this envelope and must not contain training or release-cohort leakage:

```json
{
  "contract": "phoenix.qps.graded-evaluation-suite/v3",
  "schema_version": 3,
  "queries": [
    {
      "query_identity": "keyed-or-public-suite-identity",
      "candidates": [
        {
          "document_identity": "stable-document-version",
          "v2_order": 0,
          "rank_evidence_v3": {},
          "relevance_tier": "CompleteExactGroups",
          "grade": 4
        }
      ]
    }
  ]
}
```

`rank_evidence_v3` is the complete serialized fixed-width `RankEvidenceV3`; grades are integers from 0 through 4.

```powershell
& $bin qps-v3-qualify-quality --manifest $manifest `
  --model C:\path\to\qps-v3-linear-model.json `
  --phase-6 C:\path\to\phase6-split.json `
  --phase-4 C:\path\to\phase4-qualified-ledger.json `
  --phase-3 $phase3 --graded-suite C:\path\to\graded-v3.json `
  --output C:\path\to\phase8-quality.json
```

## Model-bound performance

The benchmark commands accept `--model`. Omitting it is allowed only for readiness diagnostics and can never produce Phase 9 qualification.

```powershell
& $bin qps-v3-benchmark-kernel --manifest $manifest --phase-3 $phase3 `
  --model C:\path\to\qps-v3-linear-model.json --repetitions 50000 `
  --output C:\path\to\phase9-kernel.json

& $bin qps-v3-benchmark-e2e --manifest $manifest `
  --workload C:\benchmarks\phoenix-qps-v201-release-lock-20260730\longmemeval-small.plmw `
  --model C:\path\to\qps-v3-linear-model.json --repetitions 16 `
  --output C:\path\to\phase9-e2e.json

& $bin qps-v3-qualify-performance --manifest $manifest `
  --model C:\path\to\qps-v3-linear-model.json `
  --kernel C:\path\to\phase9-kernel.json --e2e C:\path\to\phase9-e2e.json `
  --output C:\path\to\phase9-performance.json
```

The combined receipt cryptographically binds the exact model file and model identity to both benchmark arms.

## Shadow, reconciliation, and promotion

Shadow evaluation compares V2 and V3 over the same frozen candidate pools and records stable, impact-sorted disagreement identities while V2 remains the returned engine.

```powershell
& $bin qps-v3-shadow --manifest $manifest `
  --model C:\path\to\qps-v3-linear-model.json --phase-3 $phase3 `
  --output C:\path\to\phase10-shadow.json
```

Review produces `phoenix.qps.v3-shadow-reconciliation/v1`:

```json
{
  "contract": "phoenix.qps.v3-shadow-reconciliation/v1",
  "shadow_receipt_sha256": "...",
  "model_identity": [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
  "reviews": [
    {
      "disagreement_identity": "...",
      "disposition": "v2_better",
      "reviewer_identity": "keyed-reviewer",
      "ledger_judgment_identities": ["permanent-ledger-judgment-id"]
    }
  ]
}
```

Allowed dispositions are `v3_better`, `v2_better`, `equivalent`, and `invalid_comparison`. Every disagreement requires exactly one explicit review. A `v2_better` decision is a verified V3 failure and must reference at least one permanent ledger judgment. After those judgments are appended, repeat corpus qualification, split, training, quality, performance, and shadow for the new model.

Only a fully reconciled same-model shadow plus verified same-model Phase 8 and 9 receipts can publish V3:

```powershell
& $bin qps-v3-promote --manifest $manifest `
  --model C:\path\to\qps-v3-linear-model.json `
  --phase-8 C:\path\to\phase8-quality.json `
  --phase-9 C:\path\to\phase9-performance.json `
  --shadow C:\path\to\phase10-shadow.json `
  --reconciliation C:\path\to\shadow-reconciliation.json `
  --active-pointer C:\path\to\qps-v3-active-model.json `
  --output C:\path\to\phase10-promotion.json
```

The active pointer is replaced atomically with write-through semantics and retains both the V2 rollback identity and the prior V3 model identity. Missing, corrupt, incompatible, or unqualified artifacts report `V2 active`; silent V3 fallback is forbidden.

## Tree eligibility decision

Phase 11 verifies a policy decision, not the existence or activation of a tree model. A verified deferred result has `phase_11_verified: true`, `tree_eligible: false`, and `tree_model_enabled: false`. This does not assert Phase 8 or Phase 10 promotion.

```powershell
& $bin qps-v3-audit-tree-eligibility --manifest $manifest `
  --phase-5 C:\path\to\phase5-corpus.json `
  --phase-8 C:\path\to\phase8-quality.json `
  --output C:\path\to\phase11-tree-decision.json
```

The audit rejects corrupt, incompatible, or unverified Phase 5 receipts. The Phase 8 receipt must be structurally valid, but it may truthfully report failure; that produces `deferred_linear_unqualified` while V2 remains active.

A future tree challenger supplies `--tree-evidence` using the contract in `qps-v3-tree-challenger-evidence.schema.json`. Eligibility requires all of the following:

- Phase 8 linear promotion gates pass.
- Phase 5 records at least 10,000 reconciled pairs and matches the evidence count.
- At least one repeatable interaction is reproduced across at least two independent holdout slices.
- Blind tree NDCG@10 and MRR both exceed the linear model.
- Rank-160 p99 is strictly below 15 microseconds with a non-empty sample.
- The evidence binds the exact Phase 5 receipt, Phase 8 receipt, linear model identity, and tree artifact hash.
- The hash-bound tree artifact is non-empty and below 256 KiB.
- Monotonic constraints hold with zero constitutional or deterministic-ranking failures.

Passing these gates yields `eligible_for_promotion_review`; it still does not enable the tree. Tree activation requires a separate explicit, atomic promotion implementation.
