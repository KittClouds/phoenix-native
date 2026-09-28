# P1P3E2-C Amendment 01: LFM Zero-Overlap Diagnostic

**Date:** 2026-09-28
**Status:** frozen before extension acquisition
**Parent protocol:** P1P3E2-C v1, commit `1d016b972c5e32ce1621511c657e572406eb482f`
**Purpose:** preserve the conservative transport policy while measuring whether a frozen language representation can recover compatible cases with no exact lexical anchor.

## Scope

This amendment is additive and diagnostic-only. The P1P3E2-C primary lane, its universal exact-content-overlap ALLOW guard, model-selection rule, thresholds, packet identities, and retrieval restrictions remain unchanged. No candidate selection, review, feature construction, or fitting may use this diagnostic.

The primary question remains the production-shaped comparison: every candidate model must pass the exact shared non-stopword content token or content n-gram guard before it can ALLOW. The diagnostic asks a separate question: whether the already-frozen `LFM2.5-230M-Base` or `LFM2.5-1.2B-Base` representation/readout can identify safe transport opportunities when that exact lexical anchor is absent.

## Diagnostic arm

For each available frozen LFM arm, reuse without change:

- the exact backbone revision, tokenizer, prompt serialization, final-layer representation, and readout already frozen in P1P3E2-C;
- the same TRAIN/DEV-NEW/DEV-EXT/TEST-EXT identities, labels, Gate 0 support lookup, Gate 1 sufficiency readout, Gate 2 compatibility readout, and that LFM arm's already-frozen shared `T_allow`;
- the existing function-word/evidence-sufficiency safeguards.

Change only one decision guard for this diagnostic: bypass the requirement for an exact shared non-stopword content token or content n-gram. A row remains ineligible for diagnostic ALLOW if Gate 0 says the relation is unsupported, Gate 1 says evidence is insufficient, or either side lacks a visible non-stopword content token. No empty-context, marker-only, or function-word-only input may ALLOW. The LFM receives only the already-frozen relation-conditioned text prompt; no structural-only feature vector is added.

If an LFM arm has no feasible primary DEV operating point and therefore no frozen `T_allow`, report that arm's diagnostic as `NO_FROZEN_THRESHOLD`; do not choose a new threshold. The diagnostic cannot nominate or replace the primary selected model.

## Reporting and firewall

Run the diagnostic on existing eligible rows only. Do not fit another readout, alter a threshold, add data, tune features, or inspect retrieval outcomes. Seal primary and diagnostic predictions before joining TEST-EXT labels. Report separately for each available LFM arm and partition:

- zero-exact-overlap eligible base groups by natural SAME, DIFFERENT, and UNKNOWN;
- diagnostic ALLOW / REFUSE / ABSTAIN counts;
- false ALLOW on natural DIFFERENT and UNKNOWN;
- SAME ALLOW coverage;
- the corresponding primary guarded decisions on the same rows.

This diagnostic is not eligible for model selection, relation support, acquisition changes, a promotion claim, retrieval evaluation, authority updates, lexical promotion, or serving. Any future deployable exception to the lexical-anchor guard needs a separately frozen experiment.

## Single-review wording clarification

P1P3E2-C allows one designated review pass. Therefore “reviewer discordance” is not a possible extension label and must not be used as a reason for underpowering. For each fixed partition/relation quota, an `UNKNOWN`, missing judgment, or invalid judgment consumes its packet slot and may cause the SAME/DIFFERENT class floor to fail. Do not replace, backfill, or relabel such a packet. Apply the existing DEV stop and TEST underpowered rules unchanged.

## Explicitly unchanged

The 108 packet identities, lane quotas, source snapshots, identity exclusions, class floors, reviewer rubric, primary ALLOW guard, frozen model roster, model parameters, selection procedure, and all no-retrieval/no-authority/no-serving prohibitions in P1P3E2-C v1 remain unchanged. This amendment does not authorize acquisition until its lock is committed alongside the parent protocol.
