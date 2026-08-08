# QPS V3 offline relevance review

The independent-data generator emits an immutable review packet without making
its mined negatives eligible for training. The packet is bound to the keyed
judgment identities in the candidate ledger.

## Artifacts

- Candidate ledger: `phoenix.qps.relevance-ledger/v3`
- Review packet: `phoenix.qps.relevance-review-packet/v1`
- Decisions: `phoenix.qps.relevance-review-decisions/v1`
- Application receipt: `phoenix.qps.relevance-review-application/v1`

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

The `human_reviewed` attestation must not be emitted for generated or model-only
decisions. The verifier binds and validates the attestation but cannot prove the
reviewer's humanity; that remains an external provenance obligation. Keep
unreviewed items absent from the decisions file. Agent-curated decisions record
the user authorization and never claim real-user-correction provenance.

## Apply reviewed decisions

```powershell
& 'D:\phoenix-target-qps-v3\release\phoenix-v3-independent-data.exe' `
  apply-reviews `
  --ledger 'C:\path\candidate-ledger.json' `
  --decisions 'C:\path\review-decisions.json' `
  --ledger-output 'C:\path\reviewed-ledger.json' `
  --receipt-output 'C:\path\review-application-receipt.json'
```

Application is create-only. Every accepted decision appends a judgment with a
`supersedes` edge to the mined candidate. The mined row remains in the ledger
for provenance but is inactive. Split construction, corpus accounting,
training, and blind evaluation consume only active judgments.

## Fail-closed behavior

- `ordinary_click` and `automatically_mined_negative` are training-ineligible.
- Missing, duplicate, unknown, already-superseded, or unattested decisions fail.
- Review sources other than the two authoritative sources fail.
- Confidence must be finite and between `0.5` and `1.0`.
- Output artifacts are never overwritten.
- Phase 4 still checks frozen-release evidence fingerprints after review.

Phase 5 remains unverified until the original corpus thresholds are satisfied,
including 500 authoritative promotion pairs, 100 reviewed pairs per major
failure class, and real user corrections that are not inferred from benchmarks.
