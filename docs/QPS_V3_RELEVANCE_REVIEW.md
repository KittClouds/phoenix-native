# QPS V3 offline relevance review

The independent-data generator emits an immutable review packet without making
its mined negatives eligible for training. The packet is bound to the keyed
judgment identities in the candidate ledger.

## Artifacts

- Candidate ledger: `phoenix.qps.relevance-ledger/v3`
- Review packet: `phoenix.qps.relevance-review-packet/v2`
- Semantic review batch: `phoenix.qps.semantic-review-batch/v2`
- Decisions: `phoenix.qps.relevance-review-decisions/v1`
- Application receipt: `phoenix.qps.relevance-review-application/v1`
- Checkpoint progress: `phoenix.qps.review-checkpoint-progress/v1`

The review packet contains public benchmark query and document text. The
candidate ledger contains keyed identities and primitive `RankEvidenceV3`; it
does not contain the workspace key.

## Decision contract

Create a JSON document shaped like this:

```json
{
  "contract": "phoenix.qps.relevance-review-decisions/v1",
  "schema_version": 1,
  "reviewer_identity": "human-curator-identity",
  "reviewed_at_unix_seconds": 1786161600,
  "attestation": "human_reviewed",
  "authorization_context": null,
  "decisions": [
    {
      "judgment_identity": "64-lowercase-hex-characters",
      "verdict": "positive_preferred",
      "reason": "phrase_order_failure",
      "source": "curated_regression_case",
      "confidence": 1.0
    }
  ]
}
```

`verdict` is `positive_preferred` or `negative_preferred`. A reversed decision
swaps the documents, evidence, positions, and directional split provenance.

`attestation` is `human_reviewed` or
`agent_curated_with_user_authorization`. Agent curation requires a non-empty
`authorization_context`, and every resulting source must be
`curated_regression_case`.

`source` must be `curated_regression_case` or `explicit_user_correction`.
Use `explicit_user_correction` only for a real product user correction. A model,
benchmark label, click, or inferred preference must not attest that source.

Attestation records provenance rather than semantic capability. A model review
may be a valid semantic comparison and uses
`agent_curated_with_user_authorization`; it must retain its model and
authorization context rather than being relabeled `human_reviewed`. Governance
may qualify either provenance for a given training cut. Keep unreviewed items
absent from the decisions file, and never infer `explicit_user_correction` from
benchmark or model evidence.

## Apply reviewed decisions

```powershell
& 'D:\phoenix-target-qps-v3\release\phoenix-v3-independent-data.exe' `
  apply-reviews `
  --ledger 'C:\path\candidate-ledger.json' `
  --decisions 'C:\path\review-decisions.json' `
  --ledger-output 'C:\path\reviewed-ledger.json' `
  --receipt-output 'C:\path\review-application-receipt.json'
```

Application outputs are create-only. Decision files may be cumulative:
unchanged decisions already present in the source ledger are skipped
idempotently. A changed decision appends a new owner which supersedes the prior
review; a direction change also records contradiction lineage. No row is
deleted or edited. Split construction, corpus accounting, training, and blind
evaluation consume only active judgments.

The normal checkpoint command applies the cumulative export, reruns Phase 4 and
Phase 5, writes per-class remaining counts, and automatically continues through
Phase 6, Phase 7, and Phase 8 only after Phase 5 verifies:

```powershell
& 'C:\code land\clean-rust\phoenix-native\scripts\Invoke-QpsV3ReviewCheckpoint.ps1' `
  -Decisions C:\path\to\qps-v3-review-checkpoint-50-2026-08-08.json
```

Immutable checkpoint directories live under
`C:\benchmarks\phoenix-qps-v3-20260804\review-checkpoints`. The replaceable
`current.json` file is only an atomic pointer to the latest fully qualified
checkpoint; it is not evidence itself. Each directory also contains a readable
`review-progress.md` projection beside its authoritative JSON receipts.

## Fail-closed behavior

- `ordinary_click` and `automatically_mined_negative` are training-ineligible.
- Missing, duplicate, unknown, or unattested decisions fail.
- Cumulative repeats must match their active review owner; changed decisions
  append explicit revision lineage.
- Review sources other than the two authoritative sources fail.
- Confidence must be finite and between `0.5` and `1.0`.
- Output artifacts are never overwritten.
- Phase 4 still checks frozen-release evidence fingerprints after review.

Phase 5 remains unverified until the original corpus thresholds are satisfied,
including 500 authoritative promotion pairs, 100 reviewed pairs per major
failure class, and real user corrections that are not inferred from benchmarks.
