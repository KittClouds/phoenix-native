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

## Current verified boundary (2026-08-04)

| Phase | State | Evidence |
|---|---|---|
| 1 | verified | frozen mixed and LongMemEval baseline receipt |
| 2 | verified | 30 bounded primitive features; no `baseline_score` or `candidate_strength` |
| 3 | verified | hard constitutional tiers precede learned score |
| 4 | verified | keyed, append-only ledger contract and frozen-fixture receipt |
| 5 | not verified | zero training-eligible pairs; all nine current judgments are constitutional holdouts |
| 6 | implemented, gated | grouped deterministic 60/20/20 split refuses unverified Phase 5 |
| 7 | implemented, gated | deterministic monotonic pairwise logistic trainer refuses unverified Phase 6 |
| 8 | implemented, gated | complete quality qualification requires a trained model and independent graded suite |
| 9 | readiness verified, promotion gated | synthetic compute-equivalent kernel/E2E proof passes; exact trained-model binding is required for promotion |
| 10 | implemented, gated | shadow, review reconciliation, atomic promotion, explicit V2 fallback; no trained model exists |
| 11 | policy verified, ineligible | tree model remains disabled until every eligibility gate passes |

The authoritative locks are `phoenix-native/memory-lock/qps-v3-phase*-2026-08-04.json`. A readiness lock is not execution evidence for a later phase.

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
