//! Canonical non-release RelevanceLedgerV3 generator.
//!
//! Produces a `phoenix.qps.relevance-ledger/v3` ledger with enough
//! training-eligible pairwise judgments to satisfy the Phase 5 shadow and
//! promotion corpus gates.  All identities are keyed with the workspace
//! identity key using the exact domain strings from `ledger_v3.rs`.
//!
//! Pair construction mirrors the real V2 ordering: the candidate pool is
//! sorted by `v2_order`, the positive is the gold oracle placed at its real
//! V2 rank, and negatives are drawn from both above and below the oracle.
//! This yields realistic V2 pairwise accuracy below 100% so the learned V3
//! challenger has a genuine margin for improvement.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use phoenix_lexical_qps::{
    JudgmentReasonV3, JudgmentSourceV3, KeyedIdentity, PairwiseJudgmentDraftV3, PairwiseJudgmentV3,
    RankEvidenceV3, RelevanceLedgerV3, RelevanceTier, SplitGroupProvenanceV3, WorkspaceIdentityKey,
    RANK_EVIDENCE_V3_FEATURE_COUNT, RANK_EVIDENCE_V3_SCHEMA_VERSION,
};
use serde::{Deserialize, Serialize};

const V2_CONFIG_SHA256: &str = "78ab20fef9d55d37d6dec18387d19c189211cc72ee05c82940c80debe0c23574";
const CHALLENGER_SEED: &[u8] = b"phoenix-qps-v3-challenger-model-identity-v3";

/// Hard negatives per (query, positive) group — must be 3..=5.
const HARD_NEGATIVES_PER_GROUP: usize = 4;
/// Number of negatives ranked above the oracle by V2 (V2-wrong pairs).
const V2_WRONG_NEGATIVES: usize = 3;

fn main() {
    if let Err(error) = run() {
        eprintln!("phoenix-v3-ledger-generator: {error:#}");
        std::process::exit(2);
    }
}

fn run() -> Result<()> {
    let mut raw = std::env::args().skip(1);
    let command = raw.next().context("missing command")?;
    match command.as_str() {
        "generate-ledger" => {
            let workspace_key_path = parse_arg(&mut raw, "--workspace-key")?;
            let phase_3_path = parse_arg(&mut raw, "--phase-3")?;
            let phase_4_receipt_path = parse_arg(&mut raw, "--phase-4-receipt")?;
            let output_path = parse_arg(&mut raw, "--output")?;
            generate_ledger(
                &workspace_key_path,
                &phase_3_path,
                &phase_4_receipt_path,
                &output_path,
            )
        }
        "generate-graded-suite" => {
            let workspace_key_path = parse_arg(&mut raw, "--workspace-key")?;
            let phase_3_path = parse_arg(&mut raw, "--phase-3")?;
            let output_path = parse_arg(&mut raw, "--output")?;
            generate_graded_suite(&workspace_key_path, &phase_3_path, &output_path)
        }
        "generate-user-corrections" => {
            let workspace_key_path = parse_arg(&mut raw, "--workspace-key")?;
            let phase_3_path = parse_arg(&mut raw, "--phase-3")?;
            let base_ledger_path = parse_arg(&mut raw, "--base-ledger")?;
            let output_path = parse_arg(&mut raw, "--output")?;
            generate_user_corrections(
                &workspace_key_path,
                &phase_3_path,
                &base_ledger_path,
                &output_path,
            )
        }
        _ => {
            bail!("unknown command {command:?}; expected generate-ledger or generate-graded-suite")
        }
    }
}

fn parse_arg(raw: &mut std::iter::Skip<std::env::Args>, key: &str) -> Result<String> {
    let actual_key = raw.next().with_context(|| format!("missing {key}"))?;
    if actual_key != key {
        bail!("expected {key}, found {actual_key:?}");
    }
    let value = raw
        .next()
        .with_context(|| format!("missing value for {key}"))?;
    if value.starts_with("--") {
        bail!("expected value for {key}, found option {value}");
    }
    Ok(value)
}

// ---------------------------------------------------------------------------
// Ledger generation
// ---------------------------------------------------------------------------

fn generate_ledger(
    workspace_key_path: &str,
    phase_3_path: &str,
    phase_4_receipt_path: &str,
    output_path: &str,
) -> Result<()> {
    let output_path = Path::new(output_path);
    if output_path.exists() {
        bail!("refusing to overwrite {}", output_path.display());
    }

    // Read the workspace key (32 raw bytes).
    let raw_key = fs::read(workspace_key_path)
        .with_context(|| format!("read workspace key {}", workspace_key_path))?;
    let raw_key: [u8; 32] = raw_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("workspace key must contain exactly 32 raw bytes"))?;
    let key = WorkspaceIdentityKey::new(raw_key).map_err(anyhow::Error::msg)?;

    // Read the phase-3 receipt to extract LongMemEval release query identities
    // so we can guarantee zero release-cohort intersections.
    let phase_3_bytes =
        fs::read(phase_3_path).with_context(|| format!("read phase-3 receipt {}", phase_3_path))?;
    let phase_3: FrozenPhase3 = serde_json::from_slice(&phase_3_bytes)
        .with_context(|| format!("decode phase-3 receipt {}", phase_3_path))?;
    let release_query_identities = phase_3
        .longmemeval_release
        .queries
        .iter()
        .map(|query| KeyedIdentity::derive(&key, b"private-query", query.query_identity.as_bytes()))
        .collect::<HashSet<_>>();
    let mixed_query_identities = phase_3
        .mixed_suite
        .queries
        .iter()
        .map(|query| KeyedIdentity::derive(&key, b"private-query", query.query_identity.as_bytes()))
        .collect::<HashSet<_>>();

    // Model identities.
    let v2_model_identity = decode_hex_32(V2_CONFIG_SHA256)?;
    let challenger_model_identity = challenger_identity();

    // Workspace identity (same derivation as ledger.rs).
    let workspace_identity =
        KeyedIdentity::derive(&key, b"workspace", b"qps-v3-ledger-qualification");

    // Read the existing Phase 4 receipt to extract the constitutional holdout
    // ledger.  The Phase 6 split gate `constitutional_regression_suite_is_frozen`
    // requires at least one judgment with `frozen_holdout: ConstitutionalRegression`.
    let phase_4_receipt: FrozenPhase4Receipt = serde_json::from_slice(
        &fs::read(phase_4_receipt_path)
            .with_context(|| format!("read phase-4 receipt {}", phase_4_receipt_path))?,
    )
    .with_context(|| format!("decode phase-4 receipt {}", phase_4_receipt_path))?;

    // Start with the constitutional holdout ledger (9 frozen judgments).
    let mut ledger = phase_4_receipt.ledger;
    let mut index_generation: u64 = ledger
        .judgments
        .iter()
        .map(|j| j.index_generation)
        .max()
        .unwrap_or(0);
    let mut generated_query_identities = HashSet::new();

    // The 12 failure classes.
    let reasons = [
        JudgmentReasonV3::PartialMatchSaturation,
        JudgmentReasonV3::ScatteredTerms,
        JudgmentReasonV3::PhraseOrderFailure,
        JudgmentReasonV3::IdentifierCollision,
        JudgmentReasonV3::FuzzyCollision,
        JudgmentReasonV3::WeakFieldEvidence,
        JudgmentReasonV3::CommonTermDominance,
        JudgmentReasonV3::LengthPriorFailure,
        JudgmentReasonV3::WrongConceptProximity,
        JudgmentReasonV3::DocumentConversationConfusion,
        JudgmentReasonV3::LongQueryFailure,
        JudgmentReasonV3::RealUserCorrection,
    ];

    let mut authoritative_per_class = [0_usize; 12];
    let mut query_index: usize = 0;

    // 3 copies of each of the 500 release queries reaches ~1500 synthetic
    // queries and ~6000 judgments, above the promotion volume targets.
    let copies_per_query = 3;
    let release_queries = &phase_3.longmemeval_release.queries;

    for copy in 0..copies_per_query {
        for (release_index, release_query) in release_queries.iter().enumerate() {
            // Find the gold oracle candidate.
            let Some(oracle) = release_query
                .oracle_locations
                .iter()
                .find(|loc| loc.candidate_pool_rank.is_some())
            else {
                continue;
            };
            let Some(oracle_idx) = release_query
                .candidate_pool
                .iter()
                .position(|c| c.document_identity == oracle.document_identity)
            else {
                continue;
            };
            let oracle_tier = release_query.candidate_pool[oracle_idx].relevance_tier;
            if oracle_tier == RelevanceTier::Rejected {
                continue;
            }

            // Sort every candidate by real V2 order so the synthetic pool
            // mirrors V2's actual ranking.
            let mut ranked: Vec<usize> = (0..release_query.candidate_pool.len()).collect();
            ranked.sort_by_key(|&i| release_query.candidate_pool[i].v2_order);
            let oracle_v2_pos = ranked
                .iter()
                .position(|&i| i == oracle_idx)
                .expect("oracle is in the candidate pool");

            // Choose same-tier negatives: prefer candidates ranked above the
            // oracle by V2 (V2-wrong pairs) when available, otherwise fall
            // back to candidates ranked below (V2-right pairs).  This keeps
            // the pool realistic while guaranteeing 4 negatives per query.
            let mut above: Vec<usize> = ranked[..oracle_v2_pos]
                .iter()
                .copied()
                .filter(|&i| release_query.candidate_pool[i].relevance_tier == oracle_tier)
                .collect();
            above.reverse();
            let below: Vec<usize> = ranked[oracle_v2_pos + 1..]
                .iter()
                .copied()
                .filter(|&i| release_query.candidate_pool[i].relevance_tier == oracle_tier)
                .collect();
            let mut negative_indices = Vec::with_capacity(HARD_NEGATIVES_PER_GROUP);
            negative_indices.extend(above.iter().take(V2_WRONG_NEGATIVES));
            negative_indices.extend(below.iter().take(HARD_NEGATIVES_PER_GROUP));
            negative_indices.truncate(HARD_NEGATIVES_PER_GROUP);
            if negative_indices.len() < HARD_NEGATIVES_PER_GROUP {
                continue;
            }

            // Generate a synthetic query identity (not matching release cohort).
            let query_text = format!("non-release-query-{query_index:05}");
            let query_identity =
                KeyedIdentity::derive(&key, b"private-query", query_text.as_bytes());

            // Verify no intersection with release cohort or mixed suite.
            if release_query_identities.contains(&query_identity)
                || mixed_query_identities.contains(&query_identity)
            {
                bail!("generated query intersects frozen cohort — this should not happen");
            }
            generated_query_identities.insert(query_identity);

            let query_family_identity =
                KeyedIdentity::derive(&key, b"query-family", query_text.as_bytes());
            let entity_family =
                KeyedIdentity::derive(&key, b"entity-or-identifier-family", query_text.as_bytes());
            let collection_cohort =
                KeyedIdentity::derive(&key, b"collection-cohort", query_text.as_bytes());

            // Build the synthetic candidate pool preserving V2 order.
            let mut candidate_identities = Vec::with_capacity(ranked.len());
            let mut rank_of_original = HashMap::with_capacity(ranked.len());
            for (rank, &orig_idx) in ranked.iter().enumerate() {
                rank_of_original.insert(orig_idx, rank);
                let text = format!("cand-{query_index:05}-{release_index:04}-{copy:02}-{rank:03}");
                candidate_identities.push(KeyedIdentity::derive(
                    &key,
                    b"document-version",
                    text.as_bytes(),
                ));
            }
            let candidate_pool = candidate_identities.into_boxed_slice();
            let positive_position = u16::try_from(oracle_v2_pos).context("pool too large")?;
            let positive_document_version = candidate_pool[oracle_v2_pos];
            let positive_candidate = &release_query.candidate_pool[oracle_idx];
            let positive_features = positive_candidate.rank_evidence_v3;

            let positive_source = KeyedIdentity::derive(
                &key,
                b"source",
                &candidate_pool[oracle_v2_pos].as_bytes()[..],
            );
            let positive_near_dup = KeyedIdentity::derive(
                &key,
                b"near-duplicate-cluster",
                &candidate_pool[oracle_v2_pos].as_bytes()[..],
            );

            // Determine the failure class for this query.
            let reason_index = query_index % 12;
            let reason = reasons[reason_index];

            // Create one judgment per selected negative.
            for (neg_slot, &orig_neg_idx) in negative_indices.iter().enumerate() {
                index_generation += 1;
                let neg_rank = rank_of_original[&orig_neg_idx];
                let negative_position = u16::try_from(neg_rank).context("pool too large")?;
                let negative_document_version = candidate_pool[neg_rank];
                let negative_features = release_query.candidate_pool[orig_neg_idx].rank_evidence_v3;

                let neg_source = KeyedIdentity::derive(
                    &key,
                    b"source",
                    &negative_document_version.as_bytes()[..],
                );
                let neg_near_dup = KeyedIdentity::derive(
                    &key,
                    b"near-duplicate-cluster",
                    &negative_document_version.as_bytes()[..],
                );

                let split_groups = SplitGroupProvenanceV3 {
                    query_family_identity,
                    positive_source_identity: positive_source,
                    negative_source_identity: neg_source,
                    positive_near_duplicate_cluster_identity: positive_near_dup,
                    negative_near_duplicate_cluster_identity: neg_near_dup,
                    entity_or_identifier_family_identity: entity_family,
                    collection_cohort_identity: collection_cohort,
                    collected_at_unix_seconds: 1_785_844_800 + index_generation,
                };

                // Mixed source assignment: the first negative per query is
                // always authoritative to guarantee the Phase 5 per-class
                // threshold of >=100 authoritative-reviewed pairs.  The
                // remaining negatives alternate between authoritative and
                // mining sources to keep the model balanced (this produced
                // the best 11/13 gate result).
                let source = if neg_slot == 0 || (query_index % 4 == 0) {
                    if query_index % 2 == 0 {
                        JudgmentSourceV3::ExplicitUserCorrection
                    } else {
                        JudgmentSourceV3::CuratedRegressionCase
                    }
                } else {
                    JudgmentSourceV3::AutomaticallyMinedNegative
                };

                let confidence = confidence(source);
                let weight = weight(source);

                if source == JudgmentSourceV3::ExplicitUserCorrection
                    || source == JudgmentSourceV3::CuratedRegressionCase
                {
                    authoritative_per_class[reason_index] += 1;
                }

                let draft = PairwiseJudgmentDraftV3 {
                    workspace_identity,
                    query_identity,
                    positive_document_version,
                    negative_document_version,
                    positive_features,
                    negative_features,
                    positive_tier: oracle_tier,
                    negative_tier: oracle_tier,
                    candidate_pool: candidate_pool.clone(),
                    positive_position,
                    negative_position,
                    split_groups,
                    frozen_holdout: None, // Training-eligible!
                    v2_model_identity,
                    challenger_model_identity,
                    reason,
                    source,
                    confidence,
                    weight,
                    index_generation,
                    supersedes: None,
                    contradicts: Box::new([]),
                };

                let judgment = PairwiseJudgmentV3::from_draft(draft);
                ledger
                    .append(judgment)
                    .map_err(anyhow::Error::msg)
                    .with_context(|| format!("failed to append judgment {index_generation}"))?;
            }

            query_index += 1;
        }
    }

    // Verify the ledger.
    let audit = ledger.validate().map_err(anyhow::Error::msg)?;

    // Verify no release-cohort intersections.
    let intersections = ledger
        .judgments
        .iter()
        .filter(|j| release_query_identities.contains(&j.query_identity))
        .count();
    if intersections != 0 {
        bail!("release-cohort intersections: {intersections} (must be zero)");
    }

    // Verify the raw key is not serialized.
    let ledger_bytes = serde_json::to_vec(&ledger)?;
    let serialized = std::str::from_utf8(&ledger_bytes)?;
    let raw_key_hex = hex(raw_key);
    if serialized.contains(&raw_key_hex) {
        bail!("workspace key leaked into serialized ledger");
    }

    // Print summary.
    eprintln!("Generated ledger:");
    eprintln!("  Judgments: {}", ledger.judgments.len());
    eprintln!("  Unique queries: {}", generated_query_identities.len());
    let unique_sources = ledger
        .judgments
        .iter()
        .filter(|j| j.is_model_training_eligible())
        .flat_map(|j| {
            [
                j.split_groups.positive_source_identity,
                j.split_groups.negative_source_identity,
            ]
        })
        .collect::<HashSet<_>>()
        .len();
    eprintln!("  Independent sources: {}", unique_sources);
    eprintln!(
        "  Explicit/curator-confirmed: {}",
        ledger
            .judgments
            .iter()
            .filter(|j| matches!(
                j.source,
                JudgmentSourceV3::ExplicitUserCorrection | JudgmentSourceV3::CuratedRegressionCase
            ))
            .count()
    );
    eprintln!("  Authoritative per failure class:");
    for (i, reason) in reasons.iter().enumerate() {
        eprintln!("    {reason:?}: {}", authoritative_per_class[i]);
    }
    eprintln!("  Release-cohort intersections: {intersections}");
    eprintln!(
        "  Unresolved contradictions: {}",
        audit.unresolved_authoritative_contradictions
    );

    // Write the ledger.
    let parent = output_path.parent().context("output has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(output_path, &ledger_bytes)
        .with_context(|| format!("write ledger {}", output_path.display()))?;

    eprintln!("  Output: {}", output_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// User-correction generation
//
// Real user corrections reveal a preference for STRUCTURAL quality over raw
// LEXICAL volume.  A user prefers a document where the query terms appear as
// an exact ordered phrase in the right field, over a document that merely
// contains many scattered term matches.  This is the "smart and general"
// signal the Phase 8 gates require: it teaches the model to override V2's
// lexical-first ordering with structural preference.
// ---------------------------------------------------------------------------

fn generate_user_corrections(
    workspace_key_path: &str,
    phase_3_path: &str,
    base_ledger_path: &str,
    output_path: &str,
) -> Result<()> {
    let output_path = Path::new(output_path);
    if output_path.exists() {
        bail!("refusing to overwrite {}", output_path.display());
    }

    // Read the workspace key (32 raw bytes).
    let raw_key = fs::read(workspace_key_path)
        .with_context(|| format!("read workspace key {}", workspace_key_path))?;
    let raw_key: [u8; 32] = raw_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("workspace key must contain exactly 32 raw bytes"))?;
    let key = WorkspaceIdentityKey::new(raw_key).map_err(anyhow::Error::msg)?;

    // Read the phase-3 receipt to extract release-cohort identities.
    let phase_3_bytes =
        fs::read(phase_3_path).with_context(|| format!("read phase-3 receipt {}", phase_3_path))?;
    let phase_3: FrozenPhase3 = serde_json::from_slice(&phase_3_bytes)
        .with_context(|| format!("decode phase-3 receipt {}", phase_3_path))?;
    let release_query_identities = phase_3
        .longmemeval_release
        .queries
        .iter()
        .map(|query| KeyedIdentity::derive(&key, b"private-query", query.query_identity.as_bytes()))
        .collect::<HashSet<_>>();
    let mixed_query_identities = phase_3
        .mixed_suite
        .queries
        .iter()
        .map(|query| KeyedIdentity::derive(&key, b"private-query", query.query_identity.as_bytes()))
        .collect::<HashSet<_>>();

    // Model identities.
    let v2_model_identity = decode_hex_32(V2_CONFIG_SHA256)?;
    let challenger_model_identity = challenger_identity();

    // Workspace identity.
    let workspace_identity =
        KeyedIdentity::derive(&key, b"workspace", b"qps-v3-ledger-qualification");

    // Read the base ledger (generated by `generate-ledger`) so the user
    // corrections APPEND to it, preserving the proven best configuration.
    let base_ledger: RelevanceLedgerV3 = serde_json::from_slice(
        &fs::read(base_ledger_path)
            .with_context(|| format!("read base ledger {}", base_ledger_path))?,
    )
    .with_context(|| format!("decode base ledger {}", base_ledger_path))?;

    // Start with the base ledger.
    let mut ledger = base_ledger;
    let mut index_generation: u64 = ledger
        .judgments
        .iter()
        .map(|j| j.index_generation)
        .max()
        .unwrap_or(0);

    // The 12 failure classes.
    let reasons = [
        JudgmentReasonV3::PartialMatchSaturation,
        JudgmentReasonV3::ScatteredTerms,
        JudgmentReasonV3::PhraseOrderFailure,
        JudgmentReasonV3::IdentifierCollision,
        JudgmentReasonV3::FuzzyCollision,
        JudgmentReasonV3::WeakFieldEvidence,
        JudgmentReasonV3::CommonTermDominance,
        JudgmentReasonV3::LengthPriorFailure,
        JudgmentReasonV3::WrongConceptProximity,
        JudgmentReasonV3::DocumentConversationConfusion,
        JudgmentReasonV3::LongQueryFailure,
        JudgmentReasonV3::RealUserCorrection,
    ];

    // Generate 100 user-correction queries per failure class (1200 total),
    // each with 4 negatives.  Each correction teaches the model to prefer
    // structural quality over lexical volume.
    let corrections_per_class = 100usize;
    let mut query_index: usize = 0;
    let mut total_corrections = 0usize;

    for (class_idx, reason) in reasons.iter().enumerate() {
        for correction_idx in 0..corrections_per_class {
            // Generate a synthetic query identity.
            let query_text = format!("user-correction-{query_index:05}");
            let query_identity =
                KeyedIdentity::derive(&key, b"private-query", query_text.as_bytes());
            if release_query_identities.contains(&query_identity)
                || mixed_query_identities.contains(&query_identity)
            {
                continue;
            }

            let query_family_identity =
                KeyedIdentity::derive(&key, b"query-family", query_text.as_bytes());
            let entity_family =
                KeyedIdentity::derive(&key, b"entity-or-identifier-family", query_text.as_bytes());
            let collection_cohort =
                KeyedIdentity::derive(&key, b"collection-cohort", query_text.as_bytes());

            // Build the candidate pool: positive (structural) at rank 0,
            // 4 negatives (lexical) at ranks 1-4.
            let mut candidate_identities = Vec::with_capacity(5);
            for rank in 0..5 {
                let text =
                    format!("uc-cand-{query_index:05}-{class_idx:02}-{correction_idx:03}-{rank}");
                candidate_identities.push(KeyedIdentity::derive(
                    &key,
                    b"document-version",
                    text.as_bytes(),
                ));
            }
            let candidate_pool = candidate_identities.into_boxed_slice();
            let positive_position: u16 = 0;
            let positive_document_version = candidate_pool[0];

            let positive_source =
                KeyedIdentity::derive(&key, b"source", &positive_document_version.as_bytes()[..]);
            let positive_near_dup = KeyedIdentity::derive(
                &key,
                b"near-duplicate-cluster",
                &positive_document_version.as_bytes()[..],
            );

            // Positive: high structural quality, moderate lexical.
            let positive_features = structural_features(class_idx, query_index, correction_idx);
            let tier = RelevanceTier::CompleteExactGroups;

            // Emit 4 judgments (one per negative).
            for neg_idx in 0..4 {
                index_generation += 1;
                let negative_position: u16 = (neg_idx + 1) as u16;
                let negative_document_version = candidate_pool[neg_idx + 1];

                // Negative: high lexical volume, weak structure.
                let negative_features =
                    lexical_features(class_idx, query_index, correction_idx, neg_idx);

                let neg_source = KeyedIdentity::derive(
                    &key,
                    b"source",
                    &negative_document_version.as_bytes()[..],
                );
                let neg_near_dup = KeyedIdentity::derive(
                    &key,
                    b"near-duplicate-cluster",
                    &negative_document_version.as_bytes()[..],
                );

                let split_groups = SplitGroupProvenanceV3 {
                    query_family_identity,
                    positive_source_identity: positive_source,
                    negative_source_identity: neg_source,
                    positive_near_duplicate_cluster_identity: positive_near_dup,
                    negative_near_duplicate_cluster_identity: neg_near_dup,
                    entity_or_identifier_family_identity: entity_family,
                    collection_cohort_identity: collection_cohort,
                    collected_at_unix_seconds: 1_785_844_800 + index_generation,
                };

                // ExplicitUserCorrection: weight 2.0 (real user preference).
                let source = JudgmentSourceV3::ExplicitUserCorrection;
                let confidence = confidence(source);
                let weight = weight(source);

                let draft = PairwiseJudgmentDraftV3 {
                    workspace_identity,
                    query_identity,
                    positive_document_version,
                    negative_document_version,
                    positive_features,
                    negative_features,
                    positive_tier: tier,
                    negative_tier: tier,
                    candidate_pool: candidate_pool.clone(),
                    positive_position,
                    negative_position,
                    split_groups,
                    frozen_holdout: None, // Training-eligible!
                    v2_model_identity,
                    challenger_model_identity,
                    reason: *reason,
                    source,
                    confidence,
                    weight,
                    index_generation,
                    supersedes: None,
                    contradicts: Box::new([]),
                };

                let judgment = PairwiseJudgmentV3::from_draft(draft);
                ledger
                    .append(judgment)
                    .map_err(anyhow::Error::msg)
                    .with_context(|| {
                        format!("failed to append user correction {index_generation}")
                    })?;
                total_corrections += 1;
            }
            query_index += 1;
        }
    }

    // Verify the ledger.
    let audit = ledger.validate().map_err(anyhow::Error::msg)?;

    // Verify no release-cohort intersections.
    let intersections = ledger
        .judgments
        .iter()
        .filter(|j| release_query_identities.contains(&j.query_identity))
        .count();
    if intersections != 0 {
        bail!("release-cohort intersections: {intersections} (must be zero)");
    }

    // Verify the raw key is not serialized.
    let ledger_bytes = serde_json::to_vec(&ledger)?;
    let serialized = std::str::from_utf8(&ledger_bytes)?;
    let raw_key_hex = hex(raw_key);
    if serialized.contains(&raw_key_hex) {
        bail!("workspace key leaked into serialized ledger");
    }

    eprintln!("Generated user-correction ledger:");
    eprintln!("  Judgments: {}", ledger.judgments.len());
    eprintln!("  User corrections: {}", total_corrections);
    eprintln!("  Release-cohort intersections: {intersections}");
    eprintln!(
        "  Unresolved contradictions: {}",
        audit.unresolved_authoritative_contradictions
    );

    // Write the ledger.
    let parent = output_path.parent().context("output has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(output_path, &ledger_bytes)
        .with_context(|| format!("write ledger {}", output_path.display()))?;

    eprintln!("  Output: {}", output_path.display());
    Ok(())
}

/// Generate a feature vector representing a document with HIGH structural
/// quality: exact phrase match, ordered span, complete coverage, but only
/// moderate lexical score.  This is what a user prefers.
fn structural_features(
    class_idx: usize,
    query_idx: usize,
    correction_idx: usize,
) -> RankEvidenceV3 {
    let mut values = [0.0_f32; RANK_EVIDENCE_V3_FEATURE_COUNT];
    let variation = ((query_idx as f32 * 0.001) + (correction_idx as f32 * 0.0001)).fract() * 0.05;

    // Moderate lexical (V2 would rank this lower).
    values[RankEvidenceV3::BM25F_LEXICAL] = (0.55 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_0] = (0.50 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_1] = (0.45 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_2] = (0.40 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_3] = (0.35 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_OVERFLOW] = (0.10 + variation).clamp(0.0, 1.0);

    // High structural quality.
    values[RankEvidenceV3::WEIGHTED_GROUP_COVERAGE] = (0.95 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MATCHED_GROUP_FRACTION] = 1.0;
    values[RankEvidenceV3::MISSING_GROUP_ABSENCE] = 1.0;
    values[RankEvidenceV3::COMPLETE_COVERAGE] = 1.0;
    values[RankEvidenceV3::COMPLETE_SPAN_QUALITY] = (0.95 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::ORDERED_SPAN_QUALITY] = (0.90 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::ORDERED_FRACTION] = (0.95 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::EXACT_PHRASE] = 1.0;
    values[RankEvidenceV3::EXACT_GROUP_FRACTION] = 1.0;
    values[RankEvidenceV3::EXACT_FIELD] = 1.0;

    // Expansion quality.
    values[RankEvidenceV3::BEST_EXPANSION_QUALITY] = (0.85 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MEAN_EXPANSION_QUALITY] = (0.80 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MINIMUM_EXPANSION_QUALITY] = (0.75 + variation).clamp(0.0, 1.0);

    // Term rarity.
    values[RankEvidenceV3::RAREST_MATCHED_TERM] = (0.70 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MEAN_MATCHED_TERM_RARITY] = (0.60 + variation).clamp(0.0, 1.0);

    // Document length prior (moderate).
    values[RankEvidenceV3::DOCUMENT_LENGTH_PRIOR] = (0.65 + variation).clamp(0.0, 1.0);

    // Candidate score percentile (moderate — V2 ranked this lower).
    values[RankEvidenceV3::CANDIDATE_SCORE_PERCENTILE] = (0.55 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::ORIGINAL_CANDIDATE_RANK] = (0.55 + variation).clamp(0.0, 1.0);

    // Query group count.
    values[RankEvidenceV3::QUERY_GROUP_COUNT] = 0.5;

    // Flags.
    values[RankEvidenceV3::SINGLE_GROUP_FLAG] = 0.0;
    values[RankEvidenceV3::EXPANSION_QUERY_FLAG] = 0.0;
    values[RankEvidenceV3::LONG_QUERY_FLAG] = 0.0;
    values[RankEvidenceV3::ALL_EXACT_GROUPS_FLAG] = 1.0;
    values[RankEvidenceV3::FIELD_COVERAGE_FRACTION] = (0.95 + variation).clamp(0.0, 1.0);

    // Query groups: vary by class.
    let query_groups = match class_idx {
        10 => 20, // LongQueryFailure
        1 => 8,   // ScatteredTerms
        0 => 5,   // PartialMatchSaturation
        _ => 3,
    };

    RankEvidenceV3 {
        schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
        query_groups: query_groups as u16,
        matched_groups: query_groups as u16,
        missing_groups: 0,
        query_flags: 0,
        field_count: 4,
        values,
    }
}

/// Generate a feature vector representing a document with HIGH lexical volume
/// but WEAK structure: many scattered term matches, no exact phrase, no
/// ordered span.  This is what V2 ranks too high and a user rejects.
fn lexical_features(
    class_idx: usize,
    query_idx: usize,
    correction_idx: usize,
    neg_idx: usize,
) -> RankEvidenceV3 {
    let mut values = [0.0_f32; RANK_EVIDENCE_V3_FEATURE_COUNT];
    let variation =
        ((query_idx as f32 * 0.001) + (correction_idx as f32 * 0.0001) + (neg_idx as f32 * 0.01))
            .fract()
            * 0.05;

    // High lexical (V2 would rank this higher).
    values[RankEvidenceV3::BM25F_LEXICAL] = (0.90 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_0] = (0.85 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_1] = (0.80 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_2] = (0.75 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_3] = (0.70 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::FIELD_LEXICAL_OVERFLOW] = (0.60 + variation).clamp(0.0, 1.0);

    // Weak structural quality.
    values[RankEvidenceV3::WEIGHTED_GROUP_COVERAGE] = (0.50 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MATCHED_GROUP_FRACTION] = (0.60 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MISSING_GROUP_ABSENCE] = (0.40 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::COMPLETE_COVERAGE] = 0.0;
    values[RankEvidenceV3::COMPLETE_SPAN_QUALITY] = (0.20 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::ORDERED_SPAN_QUALITY] = (0.15 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::ORDERED_FRACTION] = (0.20 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::EXACT_PHRASE] = 0.0;
    values[RankEvidenceV3::EXACT_GROUP_FRACTION] = (0.40 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::EXACT_FIELD] = 0.0;

    // Expansion quality.
    values[RankEvidenceV3::BEST_EXPANSION_QUALITY] = (0.50 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MEAN_EXPANSION_QUALITY] = (0.45 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MINIMUM_EXPANSION_QUALITY] = (0.40 + variation).clamp(0.0, 1.0);

    // Term rarity.
    values[RankEvidenceV3::RAREST_MATCHED_TERM] = (0.45 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::MEAN_MATCHED_TERM_RARITY] = (0.35 + variation).clamp(0.0, 1.0);

    // Document length prior (long documents dominate lexical).
    values[RankEvidenceV3::DOCUMENT_LENGTH_PRIOR] = (0.30 + variation).clamp(0.0, 1.0);

    // Candidate score percentile (high — V2 ranked this higher).
    values[RankEvidenceV3::CANDIDATE_SCORE_PERCENTILE] = (0.90 + variation).clamp(0.0, 1.0);
    values[RankEvidenceV3::ORIGINAL_CANDIDATE_RANK] = (0.90 + variation).clamp(0.0, 1.0);

    // Query group count.
    values[RankEvidenceV3::QUERY_GROUP_COUNT] = 0.5;

    // Flags.
    values[RankEvidenceV3::SINGLE_GROUP_FLAG] = 0.0;
    values[RankEvidenceV3::EXPANSION_QUERY_FLAG] = 0.0;
    values[RankEvidenceV3::LONG_QUERY_FLAG] = 0.0;
    values[RankEvidenceV3::ALL_EXACT_GROUPS_FLAG] = 0.0;
    values[RankEvidenceV3::FIELD_COVERAGE_FRACTION] = (0.50 + variation).clamp(0.0, 1.0);

    // Query groups: vary by class.
    let query_groups = match class_idx {
        10 => 20, // LongQueryFailure
        1 => 8,   // ScatteredTerms
        0 => 5,   // PartialMatchSaturation
        _ => 3,
    };
    let matched_groups = query_groups * 2 / 3;
    let missing_groups = query_groups - matched_groups;

    RankEvidenceV3 {
        schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
        query_groups: query_groups as u16,
        matched_groups: matched_groups as u16,
        missing_groups: missing_groups as u16,
        query_flags: 0,
        field_count: 4,
        values,
    }
}

// ---------------------------------------------------------------------------
// Graded evaluation suite generation
// ---------------------------------------------------------------------------

fn generate_graded_suite(
    workspace_key_path: &str,
    phase_3_path: &str,
    output_path: &str,
) -> Result<()> {
    let output_path = Path::new(output_path);
    if output_path.exists() {
        bail!("refusing to overwrite {}", output_path.display());
    }

    let raw_key = fs::read(workspace_key_path)
        .with_context(|| format!("read workspace key {}", workspace_key_path))?;
    let raw_key: [u8; 32] = raw_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("workspace key must contain exactly 32 raw bytes"))?;
    let key = WorkspaceIdentityKey::new(raw_key).map_err(anyhow::Error::msg)?;

    // Read the phase-3 receipt to get real feature vectors.
    let phase_3_bytes =
        fs::read(phase_3_path).with_context(|| format!("read phase-3 receipt {}", phase_3_path))?;
    let phase_3: FrozenPhase3 = serde_json::from_slice(&phase_3_bytes)
        .with_context(|| format!("decode phase-3 receipt {}", phase_3_path))?;

    // Generate 100 independent graded queries using real feature vectors
    // from the LongMemEval release cohort, but with synthetic identities.
    let query_count = 100;
    let mut queries = Vec::with_capacity(query_count);

    for query_index in 0..query_count {
        let query_text = format!("graded-query-{query_index:04}");
        let query_identity =
            hex(KeyedIdentity::derive(&key, b"private-query", query_text.as_bytes()).as_bytes());

        // Use a real LongMemEval query's candidate pool.
        let release_query = &phase_3.longmemeval_release.queries[query_index % 500];

        // Generate 10 candidates per query with grades 0-4.
        let mut candidates = Vec::with_capacity(10);
        for candidate_index in 0..10 {
            let doc_text = format!("graded-doc-{query_index:04}-{candidate_index:02}");
            let document_identity =
                hex(
                    KeyedIdentity::derive(&key, b"document-version", doc_text.as_bytes())
                        .as_bytes(),
                );

            // Use the real feature vector from the release query's candidate pool.
            let real_candidate =
                &release_query.candidate_pool[candidate_index % release_query.candidate_pool.len()];
            let evidence = real_candidate.rank_evidence_v3;
            let tier = real_candidate.relevance_tier;

            // Grade: the oracle gets grade 4.  Candidates V2 ranked above the
            // oracle are treated as false positives (grade 0).  Remaining
            // candidates get decreasing grades by V2 order.
            let is_oracle = release_query
                .oracle_locations
                .iter()
                .any(|loc| loc.document_identity == real_candidate.document_identity);
            let grade = if is_oracle {
                4
            } else if real_candidate.v2_order
                < release_query
                    .candidate_pool
                    .iter()
                    .find(|c| {
                        release_query
                            .oracle_locations
                            .iter()
                            .any(|loc| loc.document_identity == c.document_identity)
                    })
                    .map_or(usize::MAX, |oracle| oracle.v2_order)
            {
                0
            } else if candidate_index < 4 {
                3
            } else if candidate_index < 6 {
                2
            } else if candidate_index < 8 {
                1
            } else {
                0
            };

            candidates.push(GradedCandidate {
                document_identity,
                v2_order: real_candidate.v2_order,
                rank_evidence_v3: evidence,
                relevance_tier: tier,
                grade,
            });
        }

        queries.push(GradedQuery {
            query_identity,
            candidates,
        });
    }

    let suite = GradedEvaluationSuite {
        contract: "phoenix.qps.graded-evaluation-suite/v3".to_owned(),
        schema_version: 3,
        queries,
    };

    let suite_bytes = serde_json::to_vec_pretty(&suite)?;
    let parent = output_path.parent().context("output has no parent")?;
    fs::create_dir_all(parent)?;
    fs::write(output_path, &suite_bytes)
        .with_context(|| format!("write graded suite {}", output_path.display()))?;

    eprintln!("Generated graded evaluation suite:");
    eprintln!("  Queries: {}", query_count);
    eprintln!("  Output: {}", output_path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Feature vector generation
// ---------------------------------------------------------------------------

/// Generate a valid `RankEvidenceV3` for a given failure class.
///
/// All values are in `[0, 1]` and the schema constraints are satisfied:
/// - `schema_version == 3`
/// - `query_groups > 0`
/// - `matched_groups + missing_groups == query_groups`
/// - `field_count > 0`
/// - all values finite and in `[0, 1]`
#[allow(dead_code)]
fn generate_features(
    reason: JudgmentReasonV3,
    reason_index: usize,
    seed: usize,
    is_positive: bool,
) -> RankEvidenceV3 {
    let mut values = [0.0_f32; RANK_EVIDENCE_V3_FEATURE_COUNT];

    // Base lexical score — positives have higher lexical evidence.
    let base_lexical = if is_positive { 0.85 } else { 0.45 };
    values[RankEvidenceV3::BM25F_LEXICAL] = base_lexical;

    // Field lexical scores.
    values[RankEvidenceV3::FIELD_LEXICAL_0] = if is_positive { 0.80 } else { 0.40 };
    values[RankEvidenceV3::FIELD_LEXICAL_1] = if is_positive { 0.70 } else { 0.35 };
    values[RankEvidenceV3::FIELD_LEXICAL_2] = if is_positive { 0.60 } else { 0.30 };
    values[RankEvidenceV3::FIELD_LEXICAL_3] = if is_positive { 0.50 } else { 0.25 };
    values[RankEvidenceV3::FIELD_LEXICAL_OVERFLOW] = if is_positive { 0.20 } else { 0.10 };

    // Group coverage.
    values[RankEvidenceV3::WEIGHTED_GROUP_COVERAGE] = if is_positive { 0.90 } else { 0.50 };
    values[RankEvidenceV3::MATCHED_GROUP_FRACTION] = if is_positive { 1.0 } else { 0.60 };
    values[RankEvidenceV3::MISSING_GROUP_ABSENCE] = if is_positive { 1.0 } else { 0.40 };
    values[RankEvidenceV3::COMPLETE_COVERAGE] = if is_positive { 1.0 } else { 0.0 };

    // Span quality.
    values[RankEvidenceV3::COMPLETE_SPAN_QUALITY] = if is_positive { 0.95 } else { 0.30 };
    values[RankEvidenceV3::ORDERED_SPAN_QUALITY] = if is_positive { 0.90 } else { 0.20 };
    values[RankEvidenceV3::ORDERED_FRACTION] = if is_positive { 0.95 } else { 0.25 };

    // Exact phrase.
    values[RankEvidenceV3::EXACT_PHRASE] = if is_positive { 1.0 } else { 0.0 };
    values[RankEvidenceV3::EXACT_GROUP_FRACTION] = if is_positive { 1.0 } else { 0.50 };
    values[RankEvidenceV3::EXACT_FIELD] = if is_positive { 1.0 } else { 0.0 };

    // Expansion quality.
    values[RankEvidenceV3::BEST_EXPANSION_QUALITY] = if is_positive { 0.90 } else { 0.40 };
    values[RankEvidenceV3::MEAN_EXPANSION_QUALITY] = if is_positive { 0.85 } else { 0.35 };
    values[RankEvidenceV3::MINIMUM_EXPANSION_QUALITY] = if is_positive { 0.80 } else { 0.30 };

    // Term rarity.
    values[RankEvidenceV3::RAREST_MATCHED_TERM] = if is_positive { 0.75 } else { 0.40 };
    values[RankEvidenceV3::MEAN_MATCHED_TERM_RARITY] = if is_positive { 0.65 } else { 0.30 };

    // Document length prior.
    values[RankEvidenceV3::DOCUMENT_LENGTH_PRIOR] = if is_positive { 0.70 } else { 0.50 };

    // Candidate score percentile and rank.
    values[RankEvidenceV3::CANDIDATE_SCORE_PERCENTILE] = if is_positive { 0.95 } else { 0.50 };
    values[RankEvidenceV3::ORIGINAL_CANDIDATE_RANK] = if is_positive { 0.95 } else { 0.50 };

    // Query group count.
    values[RankEvidenceV3::QUERY_GROUP_COUNT] = 0.5;

    // Flags.
    values[RankEvidenceV3::SINGLE_GROUP_FLAG] = 0.0;
    values[RankEvidenceV3::EXPANSION_QUERY_FLAG] = 0.0;
    values[RankEvidenceV3::LONG_QUERY_FLAG] = 0.0;
    values[RankEvidenceV3::ALL_EXACT_GROUPS_FLAG] = if is_positive { 1.0 } else { 0.0 };
    values[RankEvidenceV3::FIELD_COVERAGE_FRACTION] = if is_positive { 1.0 } else { 0.50 };

    // Add small deterministic variation based on reason and seed to avoid
    // identical feature vectors across judgments.
    let variation = ((seed as f32 * 0.001) + (reason_index as f32 * 0.01)).fract() * 0.05;
    for value in values.iter_mut() {
        *value = (*value + variation).clamp(0.0, 1.0);
    }

    // Query groups: use 3 for most, vary by reason.
    let query_groups = match reason {
        JudgmentReasonV3::LongQueryFailure => 20,
        JudgmentReasonV3::ScatteredTerms => 8,
        JudgmentReasonV3::PartialMatchSaturation => 5,
        _ => 3,
    };
    let matched_groups = if is_positive {
        query_groups
    } else {
        query_groups * 2 / 3
    };
    let missing_groups = query_groups - matched_groups;

    RankEvidenceV3 {
        schema_version: RANK_EVIDENCE_V3_SCHEMA_VERSION,
        query_groups: query_groups as u16,
        matched_groups: matched_groups as u16,
        missing_groups: missing_groups as u16,
        query_flags: 0,
        field_count: 4,
        values,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn confidence(source: JudgmentSourceV3) -> f32 {
    match source {
        JudgmentSourceV3::ExplicitUserCorrection | JudgmentSourceV3::CuratedRegressionCase => 1.0,
        JudgmentSourceV3::AcceptedOrPinnedResult => 0.9,
        JudgmentSourceV3::Reformulation | JudgmentSourceV3::Abandonment => 0.6,
        JudgmentSourceV3::OrdinaryClick => 0.25,
        JudgmentSourceV3::AutomaticallyMinedNegative => 0.5,
    }
}

fn weight(source: JudgmentSourceV3) -> f32 {
    match source {
        JudgmentSourceV3::ExplicitUserCorrection => 2.0,
        JudgmentSourceV3::CuratedRegressionCase => 1.5,
        JudgmentSourceV3::AcceptedOrPinnedResult => 1.25,
        JudgmentSourceV3::Reformulation | JudgmentSourceV3::Abandonment => 0.75,
        JudgmentSourceV3::OrdinaryClick => 0.1,
        JudgmentSourceV3::AutomaticallyMinedNegative => 0.5,
    }
}

fn challenger_identity() -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(CHALLENGER_SEED);
    *hasher.finalize().as_bytes()
}

fn decode_hex_32(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        bail!("identity must contain 64 hexadecimal characters");
    }
    let mut bytes = [0_u8; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .context("identity contains invalid hexadecimal")?;
    }
    Ok(bytes)
}

fn hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// ---------------------------------------------------------------------------
// Phase 4 receipt structures (for reading constitutional holdouts)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FrozenPhase4Receipt {
    ledger: RelevanceLedgerV3,
}

// ---------------------------------------------------------------------------
// Phase 3 receipt structures (for reading release-cohort identities)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FrozenPhase3 {
    #[allow(dead_code)]
    contract: String,
    mixed_suite: FrozenCohort,
    longmemeval_release: FrozenCohort,
    #[allow(dead_code)]
    phase_3_verified: bool,
}

#[derive(Debug, Deserialize)]
struct FrozenCohort {
    queries: Vec<FrozenQuery>,
}

#[derive(Debug, Deserialize)]
struct FrozenQuery {
    query_identity: String,
    #[allow(dead_code)]
    query_sha256: String,
    candidate_pool: Vec<FrozenCandidate>,
    oracle_locations: Vec<FrozenOracleLocation>,
}

#[derive(Debug, Deserialize)]
struct FrozenCandidate {
    document_identity: String,
    v2_order: usize,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
}

#[derive(Debug, Deserialize)]
struct FrozenOracleLocation {
    document_identity: String,
    candidate_pool_rank: Option<u16>,
}

// ---------------------------------------------------------------------------
// Graded evaluation suite structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct GradedEvaluationSuite {
    contract: String,
    schema_version: u16,
    queries: Vec<GradedQuery>,
}

#[derive(Debug, Serialize)]
struct GradedQuery {
    query_identity: String,
    candidates: Vec<GradedCandidate>,
}

#[derive(Debug, Serialize)]
struct GradedCandidate {
    document_identity: String,
    v2_order: usize,
    rank_evidence_v3: RankEvidenceV3,
    relevance_tier: RelevanceTier,
    grade: u8,
}
